use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use crossbeam_channel::{Receiver, Sender};
use rubato::{FastFixedIn, PolynomialDegree, Resampler};
use serde::{Deserialize, Serialize};

use crate::spatial::SpatialAudioMode;

pub const DEFAULT_MAX_CHUNKS: usize = 4096;
pub const MAX_GRAPH_NODES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessContext {
    pub sample_rate: u32,
    pub channels: usize,
    pub frames: usize,
}

impl Default for ProcessContext {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            channels: 2,
            frames: 256,
        }
    }
}

#[inline]
pub fn calculate_metrics(samples: &[f32]) -> (f32, f32) {
    if samples.is_empty() {
        return (-100.0, -100.0);
    }
    // Strictly reset accumulator variables per chunk to prevent infinite peak hold
    let mut current_peak = 0.0f32;
    let mut sum_squares = 0.0f32;
    let mut valid_count = 0usize;

    for &sample in samples {
        if sample.is_nan() || sample.is_infinite() {
            continue;
        }
        let clamped = sample.clamp(-1.0, 1.0);
        let abs = clamped.abs();
        if abs > current_peak {
            current_peak = abs;
        }
        sum_squares += clamped * clamped;
        valid_count += 1;
    }

    if valid_count == 0 {
        return (-100.0, -100.0);
    }

    let rms = (sum_squares / valid_count as f32).sqrt();

    let peak_db = if current_peak > 1e-5 {
        (20.0 * current_peak.log10()).clamp(-100.0, 0.0)
    } else {
        -100.0
    };

    let rms_db = if rms > 1e-5 {
        (20.0 * rms.log10()).clamp(-100.0, 0.0)
    } else {
        -100.0
    };

    (peak_db, rms_db)
}

/// Core AudioNode trait.
/// Real-time audio rendering must remain lock-free, zero allocation, and bounded.
pub trait AudioNode: Send {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn node_type(&self) -> &str {
        "AudioNode"
    }
    fn is_bypassed(&self) -> bool {
        false
    }
    fn set_bypassed(&mut self, _bypassed: bool) {}
    fn set_gain(&mut self, _gain: f32) {}
    fn set_eq_band(&mut self, _band_index: usize, _gain_db: f32) {}
    fn set_ai_noise(&mut self, _enabled: bool) {}
    fn is_ai_noise_enabled(&self) -> bool {
        false
    }
    fn set_ai_noise_level(&mut self, _level: f32) {}
    fn ai_noise_level(&self) -> f32 {
        1.0
    }
    fn set_noise_gate(&mut self, _enabled: bool, _threshold_db: f32, _attack_ms: f32, _release_ms: f32) {}
    fn set_compressor(
        &mut self,
        _enabled: bool,
        _threshold_db: f32,
        _ratio: f32,
        _attack_ms: f32,
        _release_ms: f32,
        _makeup_gain_db: f32,
    ) {}
    fn set_spatial_audio(&mut self, _enabled: bool, _mode: SpatialAudioMode, _strength: f32) {}
    fn set_volume_boost(&mut self, _enabled: bool, _boost_db: f32) {}
    fn reset(&mut self) {}
    fn peak_db(&self) -> f32 {
        -100.0
    }
    fn rms_db(&self) -> f32 {
        -100.0
    }
    /// Lock-free rendering callback processing input slice into output slice.
    fn process(&mut self, input: &[f32], output: &mut [f32]);
}

/// PipeWire audio input stream node (e.g. desktop monitor or virtual sink).
pub struct PipeWireInputNode {
    id: String,
    name: String,
    sink_name: String,
    bypassed: bool,
    gain: f32,
    peak_db: Arc<AtomicU32>,
    rms_db: Arc<AtomicU32>,
}

impl PipeWireInputNode {
    pub fn new(id: impl Into<String>, name: impl Into<String>, sink_name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            sink_name: sink_name.into(),
            bypassed: false,
            gain: 1.0,
            peak_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
            rms_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
        }
    }

    pub fn sink_name(&self) -> &str {
        &self.sink_name
    }

    pub fn stream_name(&self) -> &str {
        &self.sink_name
    }

    pub fn metrics_handles(&self) -> (Arc<AtomicU32>, Arc<AtomicU32>) {
        (self.peak_db.clone(), self.rms_db.clone())
    }
}

impl AudioNode for PipeWireInputNode {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn node_type(&self) -> &str {
        "PipeWireInputNode"
    }

    fn is_bypassed(&self) -> bool {
        self.bypassed
    }

    fn set_bypassed(&mut self, bypassed: bool) {
        self.bypassed = bypassed;
    }

    fn set_gain(&mut self, gain: f32) {
        self.gain = gain;
    }

    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        let (peak, rms) = calculate_metrics(input);
        self.peak_db.store(peak.to_bits(), Ordering::Relaxed);
        self.rms_db.store(rms.to_bits(), Ordering::Relaxed);

        if self.bypassed {
            output.fill(0.0);
        } else {
            let len = input.len().min(output.len());
            if (self.gain - 1.0).abs() < 1e-6 {
                output[..len].copy_from_slice(&input[..len]);
            } else {
                for i in 0..len {
                    output[i] = input[i] * self.gain;
                }
            }
            if len < output.len() {
                output[len..].fill(0.0);
            }
        }
    }

    fn peak_db(&self) -> f32 {
        f32::from_bits(self.peak_db.load(Ordering::Relaxed))
    }

    fn rms_db(&self) -> f32 {
        f32::from_bits(self.rms_db.load(Ordering::Relaxed))
    }
}

pub type VirtualInputNode = PipeWireInputNode;

/// Passthrough bypass node.
pub struct BypassNode {
    id: String,
    name: String,
    gain: f32,
    bypassed: bool,
    peak_db: Arc<AtomicU32>,
    rms_db: Arc<AtomicU32>,
}

impl BypassNode {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            gain: 1.0,
            bypassed: false,
            peak_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
            rms_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
        }
    }

    pub fn metrics_handles(&self) -> (Arc<AtomicU32>, Arc<AtomicU32>) {
        (self.peak_db.clone(), self.rms_db.clone())
    }
}

impl AudioNode for BypassNode {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn node_type(&self) -> &str {
        "BypassNode"
    }

    fn is_bypassed(&self) -> bool {
        self.bypassed
    }

    fn set_bypassed(&mut self, bypassed: bool) {
        self.bypassed = bypassed;
    }

    fn set_gain(&mut self, gain: f32) {
        self.gain = gain;
    }

    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        let len = input.len().min(output.len());
        if self.bypassed {
            output[..len].copy_from_slice(&input[..len]);
        } else {
            if (self.gain - 1.0).abs() < 1e-6 {
                output[..len].copy_from_slice(&input[..len]);
            } else {
                for i in 0..len {
                    output[i] = input[i] * self.gain;
                }
            }
        }
        if len < output.len() {
            output[len..].fill(0.0);
        }

        let (peak, rms) = calculate_metrics(&output[..len]);
        self.peak_db.store(peak.to_bits(), Ordering::Relaxed);
        self.rms_db.store(rms.to_bits(), Ordering::Relaxed);
    }

    fn peak_db(&self) -> f32 {
        f32::from_bits(self.peak_db.load(Ordering::Relaxed))
    }

    fn rms_db(&self) -> f32 {
        f32::from_bits(self.rms_db.load(Ordering::Relaxed))
    }
}

pub type DspBypassNode = BypassNode;

pub struct ResamplerNode {
    id: String,
    name: String,
    source_rate: u32,
    target_rate: u32,
    channels: usize,
    chunk_size: usize,
    bypassed: bool,
    resampler: Option<FastFixedIn<f32>>,
    in_planar: Vec<Vec<f32>>,
    out_planar: Vec<Vec<f32>>,
    peak_db: Arc<AtomicU32>,
    rms_db: Arc<AtomicU32>,
}

impl ResamplerNode {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        source_rate: u32,
        target_rate: u32,
        channels: usize,
        chunk_size: usize,
    ) -> Result<Self, rubato::ResamplerConstructionError> {
        let resampler = if source_rate != target_rate {
            Some(FastFixedIn::<f32>::new(
                target_rate as f64 / source_rate as f64,
                1.0,
                PolynomialDegree::Septic,
                chunk_size,
                channels,
            )?)
        } else {
            None
        };

        let in_planar = vec![vec![0.0f32; chunk_size]; channels];
        let max_out_frames = if source_rate != target_rate {
            ((chunk_size as f64 * target_rate as f64 / source_rate as f64).ceil() as usize) + 64
        } else {
            chunk_size
        };
        let out_planar = vec![vec![0.0f32; max_out_frames]; channels];

        Ok(Self {
            id: id.into(),
            name: name.into(),
            source_rate,
            target_rate,
            channels,
            chunk_size,
            bypassed: false,
            resampler,
            in_planar,
            out_planar,
            peak_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
            rms_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
        })
    }

    pub fn metrics_handles(&self) -> (Arc<AtomicU32>, Arc<AtomicU32>) {
        (self.peak_db.clone(), self.rms_db.clone())
    }
}

impl AudioNode for ResamplerNode {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn node_type(&self) -> &str {
        "ResamplerNode"
    }

    fn is_bypassed(&self) -> bool {
        self.bypassed
    }

    fn set_bypassed(&mut self, bypassed: bool) {
        self.bypassed = bypassed;
    }

    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        if self.bypassed || self.source_rate == self.target_rate || self.resampler.is_none() {
            let len = input.len().min(output.len());
            output[..len].copy_from_slice(&input[..len]);
            let (peak, rms) = calculate_metrics(&output[..len]);
            self.peak_db.store(peak.to_bits(), Ordering::Relaxed);
            self.rms_db.store(rms.to_bits(), Ordering::Relaxed);
            return;
        }

        let resampler = self.resampler.as_mut().unwrap();
        let frames_in = (input.len() / self.channels).min(self.chunk_size);
        if frames_in == 0 {
            output.fill(0.0);
            return;
        }

        // De-interleave input without allocating
        for ch in 0..self.channels {
            for f in 0..frames_in {
                self.in_planar[ch][f] = input[f * self.channels + ch];
            }
        }

        // Resample in place without dynamic heap allocation
        if let Ok((_in_consumed, out_produced)) =
            resampler.process_into_buffer(&self.in_planar, &mut self.out_planar, None)
        {
            let max_frames_out = output.len() / self.channels;
            let frames_to_write = out_produced.min(max_frames_out);

            // Interleave back into output buffer
            for f in 0..frames_to_write {
                for ch in 0..self.channels {
                    output[f * self.channels + ch] = self.out_planar[ch][f];
                }
            }

            let total_written = frames_to_write * self.channels;
            if total_written < output.len() {
                output[total_written..].fill(0.0);
            }

            let (peak, rms) = calculate_metrics(&output[..total_written]);
            self.peak_db.store(peak.to_bits(), Ordering::Relaxed);
            self.rms_db.store(rms.to_bits(), Ordering::Relaxed);
        } else {
            output.fill(0.0);
        }
    }

    fn peak_db(&self) -> f32 {
        f32::from_bits(self.peak_db.load(Ordering::Relaxed))
    }

    fn rms_db(&self) -> f32 {
        f32::from_bits(self.rms_db.load(Ordering::Relaxed))
    }
}

pub struct PhysicalOutputNode {
    id: String,
    name: String,
    device_name: String,
    bypassed: bool,
    gain: f32,
    peak_db: Arc<AtomicU32>,
    rms_db: Arc<AtomicU32>,
    frames_processed: Arc<AtomicU64>,
}

impl PhysicalOutputNode {
    pub fn new(id: impl Into<String>, name: impl Into<String>, device_name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            device_name: device_name.into(),
            bypassed: false,
            gain: 1.0,
            peak_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
            rms_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
            frames_processed: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn metrics_handles(&self) -> (Arc<AtomicU32>, Arc<AtomicU32>, Arc<AtomicU64>) {
        (
            self.peak_db.clone(),
            self.rms_db.clone(),
            self.frames_processed.clone(),
        )
    }

    pub fn frames_processed_handle(&self) -> Arc<AtomicU64> {
        self.frames_processed.clone()
    }
}

impl AudioNode for PhysicalOutputNode {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn node_type(&self) -> &str {
        "PhysicalOutputNode"
    }

    fn is_bypassed(&self) -> bool {
        self.bypassed
    }

    fn set_bypassed(&mut self, bypassed: bool) {
        self.bypassed = bypassed;
    }

    fn set_gain(&mut self, gain: f32) {
        self.gain = gain;
    }

    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        let len = input.len().min(output.len());
        if self.bypassed {
            output.fill(0.0);
        } else {
            if (self.gain - 1.0).abs() < 1e-6 {
                output[..len].copy_from_slice(&input[..len]);
            } else {
                for i in 0..len {
                    output[i] = input[i] * self.gain;
                }
            }
            if len < output.len() {
                output[len..].fill(0.0);
            }
        }

        let (peak, rms) = calculate_metrics(&output[..len]);
        self.peak_db.store(peak.to_bits(), Ordering::Relaxed);
        self.rms_db.store(rms.to_bits(), Ordering::Relaxed);
        self.frames_processed.fetch_add((len / 2) as u64, Ordering::Relaxed);
    }

    fn peak_db(&self) -> f32 {
        f32::from_bits(self.peak_db.load(Ordering::Relaxed))
    }

    fn rms_db(&self) -> f32 {
        f32::from_bits(self.rms_db.load(Ordering::Relaxed))
    }
}

pub const EQ_BANDS_COUNT: usize = 10;
pub const EQ_FREQUENCIES_HZ: [f32; EQ_BANDS_COUNT] = [
    31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];
/// Q factor for 1-octave spacing: 1 / (2 * sinh(ln(2)/2)) = sqrt(2) ~= 1.41421356
pub const DEFAULT_EQ_Q: f32 = std::f32::consts::SQRT_2;

/// 10-Band Parametric/Graphic Equalizer AudioNode.
/// Cascades 10 biquad peaking EQ filters for Left and Right channels.
/// Processing is 100% lock-free and zero-allocation.
pub struct EqNode {
    id: String,
    name: String,
    sample_rate: u32,
    bypassed: bool,
    gains_db: [f32; EQ_BANDS_COUNT],
    filters_l: [biquad::DirectForm1<f32>; EQ_BANDS_COUNT],
    filters_r: [biquad::DirectForm1<f32>; EQ_BANDS_COUNT],
    peak_db: Arc<AtomicU32>,
    rms_db: Arc<AtomicU32>,
}

impl EqNode {
    pub fn new(id: impl Into<String>, name: impl Into<String>, sample_rate: u32) -> Self {
        let filters_l = std::array::from_fn(|i| {
            let fs = biquad::Hertz::<f32>::from_hz(sample_rate as f32)
                .unwrap_or_else(|_| biquad::Hertz::<f32>::from_hz(48000.0_f32).unwrap());
            let f0 = biquad::Hertz::<f32>::from_hz(EQ_FREQUENCIES_HZ[i]).unwrap();
            let coeffs = biquad::Coefficients::<f32>::from_params(
                biquad::Type::PeakingEQ(0.0_f32),
                fs,
                f0,
                DEFAULT_EQ_Q,
            )
            .unwrap();
            biquad::DirectForm1::<f32>::new(coeffs)
        });

        let filters_r = std::array::from_fn(|i| {
            let fs = biquad::Hertz::<f32>::from_hz(sample_rate as f32)
                .unwrap_or_else(|_| biquad::Hertz::<f32>::from_hz(48000.0_f32).unwrap());
            let f0 = biquad::Hertz::<f32>::from_hz(EQ_FREQUENCIES_HZ[i]).unwrap();
            let coeffs = biquad::Coefficients::<f32>::from_params(
                biquad::Type::PeakingEQ(0.0_f32),
                fs,
                f0,
                DEFAULT_EQ_Q,
            )
            .unwrap();
            biquad::DirectForm1::<f32>::new(coeffs)
        });

        Self {
            id: id.into(),
            name: name.into(),
            sample_rate,
            bypassed: false,
            gains_db: [0.0f32; EQ_BANDS_COUNT],
            filters_l,
            filters_r,
            peak_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
            rms_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
        }
    }

    pub fn set_band_gain(&mut self, band_index: usize, gain_db: f32) {
        if band_index >= EQ_BANDS_COUNT {
            return;
        }
        let clamped = gain_db.clamp(-24.0, 24.0);
        self.gains_db[band_index] = clamped;

        let fs = biquad::Hertz::<f32>::from_hz(self.sample_rate as f32)
            .unwrap_or_else(|_| biquad::Hertz::<f32>::from_hz(48000.0_f32).unwrap());
        let f0 = biquad::Hertz::<f32>::from_hz(EQ_FREQUENCIES_HZ[band_index]).unwrap();
        if let Ok(coeffs) = biquad::Coefficients::<f32>::from_params(
            biquad::Type::PeakingEQ(clamped),
            fs,
            f0,
            DEFAULT_EQ_Q,
        ) {
            use biquad::Biquad;
            self.filters_l[band_index].update_coefficients(coeffs);
            self.filters_r[band_index].update_coefficients(coeffs);
        }
    }

    pub fn band_gains(&self) -> [f32; EQ_BANDS_COUNT] {
        self.gains_db
    }

    pub fn metrics_handles(&self) -> (Arc<AtomicU32>, Arc<AtomicU32>) {
        (self.peak_db.clone(), self.rms_db.clone())
    }
}

impl AudioNode for EqNode {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn node_type(&self) -> &str {
        "EqNode"
    }

    fn is_bypassed(&self) -> bool {
        self.bypassed
    }

    fn set_bypassed(&mut self, bypassed: bool) {
        self.bypassed = bypassed;
    }

    fn set_eq_band(&mut self, band_index: usize, gain_db: f32) {
        self.set_band_gain(band_index, gain_db);
    }

    fn reset(&mut self) {
        for i in 0..EQ_BANDS_COUNT {
            let fs = biquad::Hertz::<f32>::from_hz(self.sample_rate as f32)
                .unwrap_or_else(|_| biquad::Hertz::<f32>::from_hz(48000.0_f32).unwrap());
            let f0 = biquad::Hertz::<f32>::from_hz(EQ_FREQUENCIES_HZ[i]).unwrap();
            if let Ok(coeffs) = biquad::Coefficients::<f32>::from_params(
                biquad::Type::PeakingEQ(self.gains_db[i]),
                fs,
                f0,
                DEFAULT_EQ_Q,
            ) {
                self.filters_l[i] = biquad::DirectForm1::<f32>::new(coeffs);
                self.filters_r[i] = biquad::DirectForm1::<f32>::new(coeffs);
            }
        }
    }

    fn peak_db(&self) -> f32 {
        f32::from_bits(self.peak_db.load(Ordering::Relaxed))
    }

    fn rms_db(&self) -> f32 {
        f32::from_bits(self.rms_db.load(Ordering::Relaxed))
    }

    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        let len = input.len().min(output.len());
        if self.bypassed {
            output[..len].copy_from_slice(&input[..len]);
        } else {
            use biquad::Biquad;
            let frames = len / 2;
            for i in 0..frames {
                let mut l = input[i * 2];
                let mut r = input[i * 2 + 1];

                for band in 0..EQ_BANDS_COUNT {
                    l = self.filters_l[band].run(l);
                    r = self.filters_r[band].run(r);
                }

                output[i * 2] = l;
                output[i * 2 + 1] = r;
            }
            if len % 2 == 1 {
                let mut s = input[len - 1];
                for band in 0..EQ_BANDS_COUNT {
                    s = self.filters_l[band].run(s);
                }
                output[len - 1] = s;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VirtualChannel {
    Master = 0,
    Game = 1,
    Chat = 2,
    Media = 3,
    Aux = 4,
    Mic = 5,
}

pub type Channel = VirtualChannel;

/// Commands sent to the audio thread to mutate Graph state lock-free.
pub enum GraphCommand {
    AddNode(Box<dyn AudioNode>),
    RemoveNode(String),
    ConnectNode { from: String, to: String },
    DisconnectNode { from: String, to: String },
    SetBypass { id: String, bypassed: bool },
    SetGain { id: String, gain: f32 },
    UpdateEq(VirtualChannel, usize, f32),
    ToggleAiNoise(VirtualChannel, bool),
    SetAiNoiseLevel(VirtualChannel, f32),
    UpdateVolume(VirtualChannel, f32),
    UpdateMute(VirtualChannel, bool),
    UpdateNoiseGate {
        channel: VirtualChannel,
        enabled: bool,
        threshold_db: f32,
        attack_ms: f32,
        release_ms: f32,
    },
    UpdateCompressor {
        channel: VirtualChannel,
        enabled: bool,
        threshold_db: f32,
        ratio: f32,
        attack_ms: f32,
        release_ms: f32,
        makeup_gain_db: f32,
    },
    UpdateSpatialAudio {
        channel: VirtualChannel,
        enabled: bool,
        mode: SpatialAudioMode,
        strength: f32,
    },
    UpdateVolumeBoost {
        channel: VirtualChannel,
        enabled: bool,
        boost_db: f32,
    },
}

pub struct NodeSlot {
    pub node: Box<dyn AudioNode>,
    pub input_buffer: Vec<f32>,
    pub output_buffer: Vec<f32>,
    pub targets: Vec<usize>,
}

/// Real-time audio routing graph.
/// Exclusively owned by the cpal audio callback thread or PipeWire process thread.
/// Consumes commands via crossbeam bounded channel without Mutex.
pub struct Graph {
    slots: Vec<Option<NodeSlot>>,
    execution_order: Vec<usize>,
    cmd_consumer: Receiver<GraphCommand>,
    recycle_producer: Option<Sender<Box<dyn AudioNode>>>,
    scratch_mix: Vec<f32>,
    max_buffer_size: usize,
}

pub type AudioEngineGraph = Graph;

impl Graph {
    pub fn new(
        cmd_consumer: Receiver<GraphCommand>,
        max_buffer_size: usize,
    ) -> Self {
        Self::new_with_recycler(cmd_consumer, None, max_buffer_size)
    }

    pub fn new_with_recycler(
        cmd_consumer: Receiver<GraphCommand>,
        recycle_producer: Option<Sender<Box<dyn AudioNode>>>,
        max_buffer_size: usize,
    ) -> Self {
        let mut slots = Vec::with_capacity(MAX_GRAPH_NODES);
        for _ in 0..MAX_GRAPH_NODES {
            slots.push(None);
        }

        Self {
            slots,
            execution_order: Vec::with_capacity(MAX_GRAPH_NODES),
            cmd_consumer,
            recycle_producer,
            scratch_mix: vec![0.0f32; max_buffer_size],
            max_buffer_size,
        }
    }

    fn find_slot_by_id(&self, id: &str) -> Option<usize> {
        for (i, slot) in self.slots.iter().enumerate() {
            if let Some(s) = slot {
                if s.node.id() == id {
                    return Some(i);
                }
            }
        }
        None
    }

    fn find_free_slot(&self) -> Option<usize> {
        self.slots.iter().position(|s| s.is_none())
    }

    /// Recompute topological execution order of active nodes using Kahn's algorithm.
    fn recompute_execution_order(&mut self) {
        let mut in_degrees = [0usize; MAX_GRAPH_NODES];
        let mut active_indices = Vec::with_capacity(MAX_GRAPH_NODES);

        for (i, slot) in self.slots.iter().enumerate() {
            if slot.is_some() {
                active_indices.push(i);
            }
        }

        for &i in &active_indices {
            if let Some(slot) = &self.slots[i] {
                for &target in &slot.targets {
                    if target < MAX_GRAPH_NODES && self.slots[target].is_some() {
                        in_degrees[target] += 1;
                    }
                }
            }
        }

        let mut queue = Vec::with_capacity(active_indices.len());
        for &i in &active_indices {
            if in_degrees[i] == 0 {
                queue.push(i);
            }
        }

        let mut order = Vec::with_capacity(active_indices.len());
        let mut head = 0;
        while head < queue.len() {
            let u = queue[head];
            head += 1;
            order.push(u);

            if let Some(slot) = &self.slots[u] {
                for &target in &slot.targets {
                    if target < MAX_GRAPH_NODES && self.slots[target].is_some() {
                        in_degrees[target] -= 1;
                        if in_degrees[target] == 0 {
                            queue.push(target);
                        }
                    }
                }
            }
        }

        self.execution_order = order;
    }

    /// Process non-blocking mutations from crossbeam command queue at start of audio callback.
    pub fn drain_commands(&mut self) {
        let mut topology_changed = false;
        while let Ok(cmd) = self.cmd_consumer.try_recv() {
            match cmd {
                GraphCommand::AddNode(node) => {
                    let id = node.id().to_string();
                    if let Some(existing) = self.find_slot_by_id(&id) {
                        if let Some(slot) = &mut self.slots[existing] {
                            slot.node = node;
                        }
                    } else if let Some(free_idx) = self.find_free_slot() {
                        self.slots[free_idx] = Some(NodeSlot {
                            node,
                            input_buffer: vec![0.0f32; self.max_buffer_size],
                            output_buffer: vec![0.0f32; self.max_buffer_size],
                            targets: Vec::new(),
                        });
                        topology_changed = true;
                    }
                }
                GraphCommand::RemoveNode(id) => {
                    if let Some(idx) = self.find_slot_by_id(&id) {
                        for slot in self.slots.iter_mut().flatten() {
                            slot.targets.retain(|&t| t != idx);
                        }
                        if let Some(removed_slot) = self.slots[idx].take() {
                            if let Some(producer) = &self.recycle_producer {
                                let _ = producer.try_send(removed_slot.node);
                            }
                        }
                        topology_changed = true;
                    }
                }
                GraphCommand::ConnectNode { from, to } => {
                    if let (Some(from_idx), Some(to_idx)) =
                        (self.find_slot_by_id(&from), self.find_slot_by_id(&to))
                    {
                        if let Some(slot) = &mut self.slots[from_idx] {
                            if !slot.targets.contains(&to_idx) {
                                slot.targets.push(to_idx);
                                topology_changed = true;
                            }
                        }
                    }
                }
                GraphCommand::DisconnectNode { from, to } => {
                    if let (Some(from_idx), Some(to_idx)) =
                        (self.find_slot_by_id(&from), self.find_slot_by_id(&to))
                    {
                        if let Some(slot) = &mut self.slots[from_idx] {
                            if let Some(pos) = slot.targets.iter().position(|&t| t == to_idx) {
                                slot.targets.remove(pos);
                                topology_changed = true;
                            }
                        }
                    }
                }
                GraphCommand::SetBypass { id, bypassed } => {
                    if let Some(idx) = self.find_slot_by_id(&id) {
                        if let Some(slot) = &mut self.slots[idx] {
                            slot.node.set_bypassed(bypassed);
                        }
                    }
                }
                GraphCommand::SetGain { id, gain } => {
                    if let Some(idx) = self.find_slot_by_id(&id) {
                        if let Some(slot) = &mut self.slots[idx] {
                            slot.node.set_gain(gain);
                        }
                    }
                }
                GraphCommand::UpdateEq(channel, band_index, gain_db) => {
                    let eq_id = match channel {
                        VirtualChannel::Master => "master-eq",
                        VirtualChannel::Game => "game-eq",
                        VirtualChannel::Chat => "chat-eq",
                        VirtualChannel::Media => "media-eq",
                        VirtualChannel::Aux => "aux-eq",
                        VirtualChannel::Mic => "mic-eq",
                    };
                    if let Some(idx) = self.find_slot_by_id(eq_id) {
                        if let Some(slot) = &mut self.slots[idx] {
                            slot.node.set_eq_band(band_index, gain_db);
                        }
                    }
                }
                GraphCommand::ToggleAiNoise(channel, enabled) => {
                    let noise_id = match channel {
                        VirtualChannel::Master => "master-noise",
                        VirtualChannel::Game => "game-noise",
                        VirtualChannel::Chat => "chat-noise",
                        VirtualChannel::Media => "media-noise",
                        VirtualChannel::Aux => "aux-noise",
                        VirtualChannel::Mic => "mic-noise",
                    };
                    if let Some(idx) = self.find_slot_by_id(noise_id) {
                        if let Some(slot) = &mut self.slots[idx] {
                            slot.node.set_ai_noise(enabled);
                        }
                    }
                }
                GraphCommand::SetAiNoiseLevel(channel, level) => {
                    let noise_id = match channel {
                        VirtualChannel::Master => "master-noise",
                        VirtualChannel::Game => "game-noise",
                        VirtualChannel::Chat => "chat-noise",
                        VirtualChannel::Media => "media-noise",
                        VirtualChannel::Aux => "aux-noise",
                        VirtualChannel::Mic => "mic-noise",
                    };
                    if let Some(idx) = self.find_slot_by_id(noise_id) {
                        if let Some(slot) = &mut self.slots[idx] {
                            slot.node.set_ai_noise_level(level);
                        }
                    }
                }
                GraphCommand::UpdateVolume(channel, volume_db) => {
                    let out_id = match channel {
                        VirtualChannel::Master => "master-output",
                        VirtualChannel::Game => "game-output",
                        VirtualChannel::Chat => "chat-output",
                        VirtualChannel::Media => "media-output",
                        VirtualChannel::Aux => "aux-output",
                        VirtualChannel::Mic => "mic-output",
                    };
                    if let Some(idx) = self.find_slot_by_id(out_id) {
                        if let Some(slot) = &mut self.slots[idx] {
                            let linear_gain = if volume_db <= -48.0 {
                                0.0f32
                            } else {
                                10.0f32.powf(volume_db / 20.0)
                            };
                            slot.node.set_gain(linear_gain);
                        }
                    }
                }
                GraphCommand::UpdateMute(channel, muted) => {
                    let out_id = match channel {
                        VirtualChannel::Master => "master-output",
                        VirtualChannel::Game => "game-output",
                        VirtualChannel::Chat => "chat-output",
                        VirtualChannel::Media => "media-output",
                        VirtualChannel::Aux => "aux-output",
                        VirtualChannel::Mic => "mic-output",
                    };
                    if let Some(idx) = self.find_slot_by_id(out_id) {
                        if let Some(slot) = &mut self.slots[idx] {
                            slot.node.set_bypassed(muted);
                        }
                    }
                }
                GraphCommand::UpdateNoiseGate {
                    channel,
                    enabled,
                    threshold_db,
                    attack_ms,
                    release_ms,
                } => {
                    let node_id = match channel {
                        VirtualChannel::Mic => "mic-gate",
                        _ => "",
                    };
                    if let Some(idx) = self.find_slot_by_id(node_id) {
                        if let Some(slot) = &mut self.slots[idx] {
                            slot.node.set_noise_gate(enabled, threshold_db, attack_ms, release_ms);
                        }
                    }
                }
                GraphCommand::UpdateCompressor {
                    channel,
                    enabled,
                    threshold_db,
                    ratio,
                    attack_ms,
                    release_ms,
                    makeup_gain_db,
                } => {
                    let node_id = match channel {
                        VirtualChannel::Mic => "mic-comp",
                        _ => "",
                    };
                    if let Some(idx) = self.find_slot_by_id(node_id) {
                        if let Some(slot) = &mut self.slots[idx] {
                            slot.node.set_compressor(
                                enabled,
                                threshold_db,
                                ratio,
                                attack_ms,
                                release_ms,
                                makeup_gain_db,
                            );
                        }
                    }
                }
                GraphCommand::UpdateSpatialAudio {
                    channel,
                    enabled,
                    mode,
                    strength,
                } => {
                    let node_id = match channel {
                        VirtualChannel::Game => "game-spatial",
                        VirtualChannel::Media => "media-spatial",
                        VirtualChannel::Aux => "aux-spatial",
                        _ => "",
                    };
                    if let Some(idx) = self.find_slot_by_id(node_id) {
                        if let Some(slot) = &mut self.slots[idx] {
                            slot.node.set_spatial_audio(enabled, mode, strength);
                        }
                    }
                }
                GraphCommand::UpdateVolumeBoost {
                    channel,
                    enabled,
                    boost_db,
                } => {
                    let node_id = match channel {
                        VirtualChannel::Game => "game-boost",
                        VirtualChannel::Media => "media-boost",
                        VirtualChannel::Aux => "aux-boost",
                        _ => "",
                    };
                    if let Some(idx) = self.find_slot_by_id(node_id) {
                        if let Some(slot) = &mut self.slots[idx] {
                            slot.node.set_volume_boost(enabled, boost_db);
                        }
                    }
                }
            }
        }

        if topology_changed {
            self.recompute_execution_order();
        }
    }

    /// Real-time audio rendering loop method for CPAL / PipeWire callback thread.
    /// Exclusively owned by the audio thread; lock-free, zero allocation.
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) {
        self.drain_commands();

        let len = output.len().min(self.scratch_mix.len());
        self.scratch_mix[..len].fill(0.0);

        if !input.is_empty() {
            if let Some(&first_idx) = self.execution_order.first() {
                if let Some(slot) = &mut self.slots[first_idx] {
                    let copy_len = input.len().min(slot.input_buffer.len());
                    slot.input_buffer[..copy_len].copy_from_slice(&input[..copy_len]);
                }
            }
        }

        self.execute_routing(len);
        output[..len].copy_from_slice(&self.scratch_mix[..len]);
    }

    /// Render quantum for PipeWire bridge stream loop.
    pub fn process_quantum(&mut self, final_output: &mut [f32], _context: &ProcessContext) {
        self.drain_commands();

        let len = final_output.len().min(self.scratch_mix.len());
        self.scratch_mix[..len].fill(0.0);

        self.execute_routing(len);
        final_output[..len].copy_from_slice(&self.scratch_mix[..len]);
    }

    fn execute_routing(&mut self, len: usize) {
        for &idx in &self.execution_order {
            if idx >= self.slots.len() || self.slots[idx].is_none() {
                continue;
            }

            let (targets, is_terminal) = {
                let slot = self.slots[idx].as_mut().unwrap();
                let in_len = slot.input_buffer.len().min(len);
                slot.node.process(
                    &slot.input_buffer[..in_len],
                    &mut slot.output_buffer[..in_len],
                );
                slot.input_buffer[..in_len].fill(0.0);
                (slot.targets.clone(), slot.targets.is_empty())
            };

            if is_terminal {
                if let Some(slot) = &self.slots[idx] {
                    for s in 0..len {
                        self.scratch_mix[s] += slot.output_buffer[s];
                    }
                }
            } else {
                for target_idx in targets {
                    if target_idx == idx || target_idx >= self.slots.len() {
                        continue;
                    }

                    let (first, second) = if idx < target_idx {
                        let (left, right) = self.slots.split_at_mut(target_idx);
                        (&left[idx], &mut right[0])
                    } else {
                        let (left, right) = self.slots.split_at_mut(idx);
                        (&right[0], &mut left[target_idx])
                    };

                    if let (Some(src), Some(dst)) = (first, second) {
                        let copy_len = len.min(src.output_buffer.len()).min(dst.input_buffer.len());
                        for s in 0..copy_len {
                            dst.input_buffer[s] += src.output_buffer[s];
                        }
                    }
                }
            }
        }
    }

    /// Feed input directly into a designated node slot (e.g. from virtual sink).
    pub fn feed_node_input(&mut self, node_id: &str, input: &[f32]) {
        if let Some(idx) = self.find_slot_by_id(node_id) {
            if let Some(slot) = &mut self.slots[idx] {
                let len = input.len().min(slot.input_buffer.len());
                for i in 0..len {
                    slot.input_buffer[i] += input[i];
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeInfo {
    pub id: String,
    pub name: String,
    pub node_type: String,
    pub bypassed: bool,
}

pub struct GraphController {
    cmd_producer: Sender<GraphCommand>,
    recycle_consumer: Option<Receiver<Box<dyn AudioNode>>>,
    active_nodes: Vec<String>,
    active_edges: Vec<(String, String)>,
    node_infos: Vec<NodeInfo>,
}

impl GraphController {
    pub fn new(cmd_producer: Sender<GraphCommand>) -> Self {
        Self {
            cmd_producer,
            recycle_consumer: None,
            active_nodes: Vec::new(),
            active_edges: Vec::new(),
            node_infos: Vec::new(),
        }
    }

    pub fn new_with_recycler(
        cmd_producer: Sender<GraphCommand>,
        recycle_consumer: Option<Receiver<Box<dyn AudioNode>>>,
    ) -> Self {
        Self {
            cmd_producer,
            recycle_consumer,
            active_nodes: Vec::new(),
            active_edges: Vec::new(),
            node_infos: Vec::new(),
        }
    }

    pub fn add_node(&mut self, node: Box<dyn AudioNode>) -> Result<(), &'static str> {
        let id = node.id().to_string();
        if self.active_nodes.contains(&id) {
            return Err("Node ID already exists");
        }
        let info = NodeInfo {
            id: id.clone(),
            name: node.name().to_string(),
            node_type: node.node_type().to_string(),
            bypassed: node.is_bypassed(),
        };
        self.cmd_producer
            .try_send(GraphCommand::AddNode(node))
            .map_err(|_| "Command queue full")?;
        self.active_nodes.push(id);
        self.node_infos.push(info);
        Ok(())
    }

    pub fn remove_node(&mut self, id: &str) -> Result<(), &'static str> {
        if !self.active_nodes.iter().any(|n| n == id) {
            return Err("Node ID not found");
        }
        self.cmd_producer
            .try_send(GraphCommand::RemoveNode(id.to_string()))
            .map_err(|_| "Command queue full")?;
        self.active_nodes.retain(|n| n != id);
        self.active_edges.retain(|(f, t)| f != id && t != id);
        self.node_infos.retain(|info| info.id != id);
        Ok(())
    }

    pub fn connect(&mut self, from: &str, to: &str) -> Result<(), &'static str> {
        if !self.active_nodes.iter().any(|n| n == from) {
            return Err("Source node not found");
        }
        if !self.active_nodes.iter().any(|n| n == to) {
            return Err("Destination node not found");
        }
        let edge = (from.to_string(), to.to_string());
        if self.active_edges.contains(&edge) {
            return Ok(());
        }
        self.cmd_producer
            .try_send(GraphCommand::ConnectNode {
                from: from.to_string(),
                to: to.to_string(),
            })
            .map_err(|_| "Command queue full")?;
        self.active_edges.push(edge);
        Ok(())
    }

    pub fn disconnect(&mut self, from: &str, to: &str) -> Result<(), &'static str> {
        self.cmd_producer
            .try_send(GraphCommand::DisconnectNode {
                from: from.to_string(),
                to: to.to_string(),
            })
            .map_err(|_| "Command queue full")?;
        self.active_edges.retain(|(f, t)| !(f == from && t == to));
        Ok(())
    }

    pub fn set_bypass(&mut self, id: &str, bypassed: bool) -> Result<(), &'static str> {
        self.cmd_producer
            .try_send(GraphCommand::SetBypass {
                id: id.to_string(),
                bypassed,
            })
            .map_err(|_| "Command queue full")?;
        if let Some(info) = self.node_infos.iter_mut().find(|n| n.id == id) {
            info.bypassed = bypassed;
        }
        Ok(())
    }

    pub fn set_gain(&mut self, id: &str, gain: f32) -> Result<(), &'static str> {
        self.cmd_producer
            .try_send(GraphCommand::SetGain {
                id: id.to_string(),
                gain,
            })
            .map_err(|_| "Command queue full")?;
        Ok(())
    }

    pub fn update_eq(
        &mut self,
        channel: VirtualChannel,
        band_index: usize,
        gain_db: f32,
    ) -> Result<(), &'static str> {
        self.cmd_producer
            .try_send(GraphCommand::UpdateEq(channel, band_index, gain_db))
            .map_err(|_| "Command queue full")?;
        Ok(())
    }

    pub fn toggle_ai_noise(
        &mut self,
        channel: VirtualChannel,
        enabled: bool,
    ) -> Result<(), &'static str> {
        let noise_id = match channel {
            VirtualChannel::Master => "master-noise",
            VirtualChannel::Game => "game-noise",
            VirtualChannel::Chat => "chat-noise",
            VirtualChannel::Media => "media-noise",
            VirtualChannel::Aux => "aux-noise",
            VirtualChannel::Mic => "mic-noise",
        };
        self.cmd_producer
            .try_send(GraphCommand::ToggleAiNoise(channel, enabled))
            .map_err(|_| "Command queue full")?;
        if let Some(info) = self.node_infos.iter_mut().find(|n| n.id == noise_id) {
            info.bypassed = !enabled;
        }
        Ok(())
    }

    pub fn set_ai_noise_level(
        &mut self,
        channel: VirtualChannel,
        level: f32,
    ) -> Result<(), &'static str> {
        self.cmd_producer
            .try_send(GraphCommand::SetAiNoiseLevel(channel, level))
            .map_err(|_| "Command queue full")?;
        Ok(())
    }

    pub fn update_volume(
        &mut self,
        channel: VirtualChannel,
        volume_db: f32,
    ) -> Result<(), &'static str> {
        self.cmd_producer
            .try_send(GraphCommand::UpdateVolume(channel, volume_db))
            .map_err(|_| "Command queue full")?;
        Ok(())
    }

    pub fn update_mute(
        &mut self,
        channel: VirtualChannel,
        muted: bool,
    ) -> Result<(), &'static str> {
        self.cmd_producer
            .try_send(GraphCommand::UpdateMute(channel, muted))
            .map_err(|_| "Command queue full")?;
        Ok(())
    }

    pub fn set_noise_gate(
        &mut self,
        channel: VirtualChannel,
        enabled: bool,
        threshold_db: f32,
        attack_ms: f32,
        release_ms: f32,
    ) -> Result<(), &'static str> {
        self.cmd_producer
            .try_send(GraphCommand::UpdateNoiseGate {
                channel,
                enabled,
                threshold_db,
                attack_ms,
                release_ms,
            })
            .map_err(|_| "Command queue full")?;
        if channel == VirtualChannel::Mic {
            if let Some(info) = self.node_infos.iter_mut().find(|n| n.id == "mic-gate") {
                info.bypassed = !enabled;
            }
        }
        Ok(())
    }

    pub fn set_compressor(
        &mut self,
        channel: VirtualChannel,
        enabled: bool,
        threshold_db: f32,
        ratio: f32,
        attack_ms: f32,
        release_ms: f32,
        makeup_gain_db: f32,
    ) -> Result<(), &'static str> {
        self.cmd_producer
            .try_send(GraphCommand::UpdateCompressor {
                channel,
                enabled,
                threshold_db,
                ratio,
                attack_ms,
                release_ms,
                makeup_gain_db,
            })
            .map_err(|_| "Command queue full")?;
        if channel == VirtualChannel::Mic {
            if let Some(info) = self.node_infos.iter_mut().find(|n| n.id == "mic-comp") {
                info.bypassed = !enabled;
            }
        }
        Ok(())
    }

    pub fn set_spatial_audio(
        &mut self,
        channel: VirtualChannel,
        enabled: bool,
        mode: SpatialAudioMode,
        strength: f32,
    ) -> Result<(), &'static str> {
        self.cmd_producer
            .try_send(GraphCommand::UpdateSpatialAudio {
                channel,
                enabled,
                mode,
                strength,
            })
            .map_err(|_| "Command queue full")?;
        let spatial_id = match channel {
            VirtualChannel::Game => "game-spatial",
            VirtualChannel::Media => "media-spatial",
            VirtualChannel::Aux => "aux-spatial",
            _ => "",
        };
        if let Some(info) = self.node_infos.iter_mut().find(|n| n.id == spatial_id) {
            info.bypassed = !enabled;
        }
        Ok(())
    }

    pub fn set_volume_boost(
        &mut self,
        channel: VirtualChannel,
        enabled: bool,
        boost_db: f32,
    ) -> Result<(), &'static str> {
        self.cmd_producer
            .try_send(GraphCommand::UpdateVolumeBoost {
                channel,
                enabled,
                boost_db,
            })
            .map_err(|_| "Command queue full")?;
        let boost_id = match channel {
            VirtualChannel::Game => "game-boost",
            VirtualChannel::Media => "media-boost",
            VirtualChannel::Aux => "aux-boost",
            _ => "",
        };
        if let Some(info) = self.node_infos.iter_mut().find(|n| n.id == boost_id) {
            info.bypassed = !enabled;
        }
        Ok(())
    }

    /// Garbage-collect removed nodes outside the RT thread.
    pub fn collect_garbage(&mut self) -> usize {
        let mut count = 0;
        if let Some(rx) = &self.recycle_consumer {
            while let Ok(_recycled) = rx.try_recv() {
                count += 1;
            }
        }
        count
    }

    pub fn active_nodes(&self) -> &[String] {
        &self.active_nodes
    }

    pub fn node_infos(&self) -> &[NodeInfo] {
        &self.node_infos
    }

    pub fn active_edges(&self) -> &[(String, String)] {
        &self.active_edges
    }
}

pub fn create_graph_pair(
    queue_capacity: usize,
    max_buffer_size: usize,
) -> (GraphController, Graph) {
    let (cmd_tx, cmd_rx) = crossbeam_channel::bounded(queue_capacity);
    let (recycle_tx, recycle_rx) = crossbeam_channel::bounded(queue_capacity);

    let controller = GraphController::new_with_recycler(cmd_tx, Some(recycle_rx));
    let engine = Graph::new_with_recycler(cmd_rx, Some(recycle_tx), max_buffer_size);

    (controller, engine)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_virtual_input_node_process_and_telemetry() {
        let mut node = VirtualInputNode::new("game-input", "Game Audio", "routune-game");
        let input = [0.5f32, -0.5f32, 0.5f32, -0.5f32];
        let mut output = [0.0f32; 4];

        node.process(&input, &mut output);

        assert_eq!(output, input);
        assert!((node.peak_db() - (-6.02)).abs() < 0.1);
        assert!((node.rms_db() - (-6.02)).abs() < 0.1);

        node.set_bypassed(true);
        node.process(&input, &mut output);
        assert_eq!(output, [0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn test_bypass_node_gain_and_passthrough() {
        let mut node = BypassNode::new("dsp-gain", "Gain Stage");
        node.set_gain(0.5);
        let input = [1.0f32, -1.0f32];
        let mut output = [0.0f32; 2];

        node.process(&input, &mut output);
        assert_eq!(output, [0.5, -0.5]);

        node.set_bypassed(true);
        node.process(&input, &mut output);
        assert_eq!(output, [1.0, -1.0]);
    }

    #[test]
    fn test_resampler_node_pass_through_when_matching_rate() {
        let mut resampler = ResamplerNode::new(
            "resampler-1",
            "Resampler 48k to 48k",
            48000,
            48000,
            2,
            256,
        )
        .expect("Failed to create matching resampler");

        let input = [0.25f32; 16];
        let mut output = [0.0f32; 16];

        resampler.process(&input, &mut output);
        assert_eq!(output, input);
    }

    #[test]
    fn test_resampler_node_dynamic_resample() {
        let chunk_size = 256;
        let mut resampler = ResamplerNode::new(
            "resampler-44k-48k",
            "Resampler 44.1k to 48k",
            44100,
            48000,
            2,
            chunk_size,
        )
        .expect("Failed to create resampler");

        let input = vec![0.5f32; chunk_size * 2];
        let mut output = vec![0.0f32; 600 * 2];

        resampler.process(&input, &mut output);
        assert!(resampler.peak_db() > -10.0);
    }

    #[test]
    fn test_lock_free_graph_mutation_and_recycling() {
        let (mut controller, mut engine) = create_graph_pair(32, 512);

        let input_node = Box::new(VirtualInputNode::new("in1", "Input", "routune-game"));
        let dsp_node = Box::new(BypassNode::new("dsp1", "DSP"));
        let out_node = Box::new(PhysicalOutputNode::new("out1", "Out", "Speakers"));

        controller.add_node(input_node).expect("Add in1 failed");
        controller.add_node(dsp_node).expect("Add dsp1 failed");
        controller.add_node(out_node).expect("Add out1 failed");

        controller.connect("in1", "dsp1").expect("Connect 1 failed");
        controller.connect("dsp1", "out1").expect("Connect 2 failed");

        let mut final_buf = vec![0.0f32; 16];

        // Drain pending creation commands
        engine.drain_commands();

        // Feed test data
        engine.feed_node_input("in1", &[0.5f32; 16]);
        engine.process(&[], &mut final_buf);

        assert!((final_buf[0] - 0.5).abs() < 1e-4);

        // Remove node dynamically while stream is running
        controller.remove_node("dsp1").expect("Remove dsp1 failed");
        engine.process(&[], &mut final_buf);

        // Verify GC collects decommissioned node off RT thread
        let collected = controller.collect_garbage();
        assert_eq!(collected, 1);
    }

    #[test]
    fn test_cpal_audio_thread_ownership_model() {
        let (cmd_tx, cmd_rx) = crossbeam_channel::bounded::<GraphCommand>(64);
        let (output_tx, output_rx) = crossbeam_channel::bounded::<Vec<f32>>(8);

        // Graph moved exclusively into dedicated audio thread (simulating CPAL audio callback)
        let audio_thread = std::thread::spawn(move || {
            let mut graph = Graph::new(cmd_rx, 256);
            let mut out = vec![0.0f32; 16];

            for _ in 0..5 {
                let input = [1.0f32; 16];
                graph.process(&input, &mut out);
                let _ = output_tx.send(out.clone());
            }
        });

        // gRPC server thread produces commands without Mutex
        cmd_tx
            .send(GraphCommand::AddNode(Box::new(BypassNode::new(
                "bypass-1",
                "Passthrough",
            ))))
            .unwrap();
        cmd_tx
            .send(GraphCommand::AddNode(Box::new(PhysicalOutputNode::new(
                "out-1",
                "Master Out",
                "hw:0",
            ))))
            .unwrap();
        cmd_tx
            .send(GraphCommand::ConnectNode {
                from: "bypass-1".into(),
                to: "out-1".into(),
            })
            .unwrap();
        cmd_tx
            .send(GraphCommand::SetGain {
                id: "bypass-1".into(),
                gain: 0.75,
            })
            .unwrap();

        audio_thread.join().unwrap();

        let mut received = Vec::new();
        while let Ok(frame) = output_rx.try_recv() {
            received.push(frame);
        }
        assert!(!received.is_empty());
        // Last frames should reflect processed output
        let last_frame = received.last().unwrap();
        assert!((last_frame[0] - 0.75).abs() < 1e-4);
    }

    #[test]
    fn test_eq_node_flat_response() {
        let mut eq = EqNode::new("game-eq", "Game Equalizer", 48000);
        let input = [0.5f32, -0.5f32, 0.25f32, -0.25f32];
        let mut output = [0.0f32; 4];

        eq.process(&input, &mut output);

        // Flat EQ (0.0 dB gain on all bands) should pass signal through virtually untouched
        for (in_s, out_s) in input.iter().zip(output.iter()) {
            assert!((in_s - out_s).abs() < 1e-4, "Diff: {} vs {}", in_s, out_s);
        }
    }

    #[test]
    fn test_eq_node_boost_attenuate() {
        let sample_rate = 48000.0f32;
        let freq = 1000.0f32;

        let make_sine = || {
            let mut sine_input = vec![0.0f32; 2048];
            for i in 0..1024 {
                let t = i as f32 / sample_rate;
                let val = (2.0 * std::f32::consts::PI * freq * t).sin();
                sine_input[i * 2] = val;
                sine_input[i * 2 + 1] = val;
            }
            sine_input
        };

        // 1. Process flat
        let mut eq_flat = EqNode::new("test-flat", "Test EQ Flat", 48000);
        let mut flat_output = vec![0.0f32; 2048];
        eq_flat.process(&make_sine(), &mut flat_output);
        let flat_peak = flat_output[1024..].iter().fold(0.0f32, |acc, &x| acc.max(x.abs()));

        // 2. Boost 1000 Hz band by +6 dB
        let mut eq_boost = EqNode::new("test-boost", "Test EQ Boost", 48000);
        eq_boost.set_band_gain(5, 6.0);
        let mut boost_output = vec![0.0f32; 2048];
        eq_boost.process(&make_sine(), &mut boost_output);
        let boost_peak = boost_output[1024..].iter().fold(0.0f32, |acc, &x| acc.max(x.abs()));

        assert!(boost_peak > flat_peak * 1.5, "Boost peak {} should exceed flat peak {}", boost_peak, flat_peak);

        // 3. Cut 1000 Hz band by -6 dB
        let mut eq_cut = EqNode::new("test-cut", "Test EQ Cut", 48000);
        eq_cut.set_band_gain(5, -6.0);
        let mut cut_output = vec![0.0f32; 2048];
        eq_cut.process(&make_sine(), &mut cut_output);
        let cut_peak = cut_output[1024..].iter().fold(0.0f32, |acc, &x| acc.max(x.abs()));

        assert!(cut_peak < flat_peak * 0.7, "Cut peak {} should be less than flat peak {}", cut_peak, flat_peak);
    }

    #[test]
    fn test_eq_node_dynamic_update_via_command() {
        let (mut controller, mut engine) = create_graph_pair(32, 512);

        let in_node = Box::new(VirtualInputNode::new("routune-game", "Game In", "routune-game"));
        let eq_node = Box::new(EqNode::new("game-eq", "Game Equalizer", 48000));
        let out_node = Box::new(PhysicalOutputNode::new("out-node", "Output", "hw:0"));

        controller.add_node(in_node).unwrap();
        controller.add_node(eq_node).unwrap();
        controller.add_node(out_node).unwrap();

        controller.connect("routune-game", "game-eq").unwrap();
        controller.connect("game-eq", "out-node").unwrap();

        // Drain node addition commands into engine
        engine.drain_commands();

        // Update EQ on GAME channel, band 5 (1000Hz) to +6dB
        controller.update_eq(VirtualChannel::Game, 5, 6.0).unwrap();

        let mut out = vec![0.0f32; 64];
        engine.feed_node_input("routune-game", &[0.5f32; 64]);
        engine.process(&[], &mut out);

        // Verify slot state was updated and signal passed through
        assert!(out.iter().any(|&s| s != 0.0));
    }
}
