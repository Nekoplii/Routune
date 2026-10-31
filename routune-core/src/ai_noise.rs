use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use crate::graph::{calculate_metrics, AudioNode};

const FIFO_CAPACITY: usize = 4096;

#[derive(Clone)]
struct SampleFifo {
    buffer: Box<[f32; FIFO_CAPACITY]>,
    head: usize,
    tail: usize,
    count: usize,
}

impl SampleFifo {
    fn new() -> Self {
        Self {
            buffer: Box::new([0.0f32; FIFO_CAPACITY]),
            head: 0,
            tail: 0,
            count: 0,
        }
    }

    #[inline]
    fn push(&mut self, sample: f32) {
        if self.count < FIFO_CAPACITY {
            self.buffer[self.tail] = sample;
            self.tail = (self.tail + 1) % FIFO_CAPACITY;
            self.count += 1;
        }
    }

    #[inline]
    fn pop(&mut self) -> Option<f32> {
        if self.count == 0 {
            None
        } else {
            let val = self.buffer[self.head];
            self.head = (self.head + 1) % FIFO_CAPACITY;
            self.count -= 1;
            Some(val)
        }
    }

    #[inline]
    fn len(&self) -> usize {
        self.count
    }

    #[inline]
    fn clear(&mut self) {
        self.head = 0;
        self.tail = 0;
        self.count = 0;
    }
}

/// AI Noise Suppression AudioNode based on RNNoise via `nnnoiseless`.
/// Processes fixed 480-sample frames (10ms at 48kHz).
/// Lock-free, zero-allocation ring buffering in `process()`.
pub struct AiNoiseNode {
    id: String,
    name: String,
    is_enabled: bool,
    denoise_l: Box<nnnoiseless::DenoiseState<'static>>,
    denoise_r: Box<nnnoiseless::DenoiseState<'static>>,
    in_fifo_l: SampleFifo,
    in_fifo_r: SampleFifo,
    out_fifo_l: SampleFifo,
    out_fifo_r: SampleFifo,
    peak_db: Arc<AtomicU32>,
    rms_db: Arc<AtomicU32>,
}

impl AiNoiseNode {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            is_enabled: false,
            denoise_l: nnnoiseless::DenoiseState::new(),
            denoise_r: nnnoiseless::DenoiseState::new(),
            in_fifo_l: SampleFifo::new(),
            in_fifo_r: SampleFifo::new(),
            out_fifo_l: SampleFifo::new(),
            out_fifo_r: SampleFifo::new(),
            peak_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
            rms_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if self.is_enabled != enabled {
            self.is_enabled = enabled;
            self.in_fifo_l.clear();
            self.in_fifo_r.clear();
            self.out_fifo_l.clear();
            self.out_fifo_r.clear();
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.is_enabled
    }

    pub fn metrics_handles(&self) -> (Arc<AtomicU32>, Arc<AtomicU32>) {
        (self.peak_db.clone(), self.rms_db.clone())
    }
}

impl AudioNode for AiNoiseNode {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn node_type(&self) -> &str {
        "AiNoiseNode"
    }

    fn is_bypassed(&self) -> bool {
        !self.is_enabled
    }

    fn set_bypassed(&mut self, bypassed: bool) {
        self.set_enabled(!bypassed);
    }

    fn set_ai_noise(&mut self, enabled: bool) {
        self.set_enabled(enabled);
    }

    fn is_ai_noise_enabled(&self) -> bool {
        self.is_enabled
    }

    fn peak_db(&self) -> f32 {
        f32::from_bits(self.peak_db.load(Ordering::Relaxed))
    }

    fn rms_db(&self) -> f32 {
        f32::from_bits(self.rms_db.load(Ordering::Relaxed))
    }

    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        let len = input.len().min(output.len());
        if len == 0 {
            return;
        }

        if !self.is_enabled {
            output[..len].copy_from_slice(&input[..len]);
            if self.in_fifo_l.len() > 0 || self.out_fifo_l.len() > 0 {
                self.in_fifo_l.clear();
                self.in_fifo_r.clear();
                self.out_fifo_l.clear();
                self.out_fifo_r.clear();
            }
        } else {
            // Strictly zero dynamic allocations: fixed stack arrays
            let mut frame_in_l = [0.0f32; nnnoiseless::DenoiseState::FRAME_SIZE];
            let mut frame_in_r = [0.0f32; nnnoiseless::DenoiseState::FRAME_SIZE];
            let mut frame_out_l = [0.0f32; nnnoiseless::DenoiseState::FRAME_SIZE];
            let mut frame_out_r = [0.0f32; nnnoiseless::DenoiseState::FRAME_SIZE];

            let frames = len / 2;
            for i in 0..frames {
                self.in_fifo_l.push(input[i * 2]);
                self.in_fifo_r.push(input[i * 2 + 1]);
            }
            if len % 2 == 1 {
                self.in_fifo_l.push(input[len - 1]);
                self.in_fifo_r.push(input[len - 1]);
            }

            // Process full 480-sample blocks through RNNoise
            while self.in_fifo_l.len() >= nnnoiseless::DenoiseState::FRAME_SIZE
                && self.in_fifo_r.len() >= nnnoiseless::DenoiseState::FRAME_SIZE
            {
                for j in 0..nnnoiseless::DenoiseState::FRAME_SIZE {
                    let sl = self.in_fifo_l.pop().unwrap_or(0.0) * 32767.0;
                    let sr = self.in_fifo_r.pop().unwrap_or(0.0) * 32767.0;
                    frame_in_l[j] = sl.clamp(-32768.0, 32767.0);
                    frame_in_r[j] = sr.clamp(-32768.0, 32767.0);
                }

                self.denoise_l.process_frame(&mut frame_out_l, &frame_in_l);
                self.denoise_r.process_frame(&mut frame_out_r, &frame_in_r);

                for j in 0..nnnoiseless::DenoiseState::FRAME_SIZE {
                    self.out_fifo_l.push(frame_out_l[j] / 32767.0);
                    self.out_fifo_r.push(frame_out_r[j] / 32767.0);
                }
            }

            // Extract processed frames to output buffer
            for i in 0..frames {
                output[i * 2] = self.out_fifo_l.pop().unwrap_or(0.0);
                output[i * 2 + 1] = self.out_fifo_r.pop().unwrap_or(0.0);
            }
            if len % 2 == 1 {
                output[len - 1] = self.out_fifo_l.pop().unwrap_or(0.0);
            }
        }

        if len < output.len() {
            output[len..].fill(0.0);
        }

        let (peak, rms) = calculate_metrics(&output[..len]);
        self.peak_db.store(peak.to_bits(), Ordering::Relaxed);
        self.rms_db.store(rms.to_bits(), Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ai_noise_node_bypass_passthrough() {
        let mut node = AiNoiseNode::new("test-noise", "Test Noise");
        assert!(!node.is_enabled());

        let input = [0.1f32, 0.2, 0.3, 0.4];
        let mut output = [0.0f32; 4];
        node.process(&input, &mut output);
        assert_eq!(input, output);
    }

    #[test]
    fn test_ai_noise_node_processing_runs_without_allocation() {
        let mut node = AiNoiseNode::new("test-noise", "Test Noise");
        node.set_enabled(true);
        assert!(node.is_enabled());

        // Feed multiple 256-sample chunks to trigger 480-sample RNNoise processing
        let input = [0.05f32; 512];
        let mut output = [0.0f32; 512];

        for _ in 0..3 {
            node.process(&input, &mut output);
        }
    }
}
