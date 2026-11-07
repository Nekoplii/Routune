use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use crossbeam_channel::{Receiver, Sender};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use tracing::{info, warn};

use routune_core::ai_noise::AiNoiseNode;
use routune_core::dynamics::{CompressorNode, NoiseGateNode};
use routune_core::graph::{calculate_metrics, AudioNode, EqNode, GraphCommand};
use crate::server::proto;

pub const AUDIO_RING_CAPACITY: usize = 32768; // 16384 stereo frames (~341ms at 48kHz)
pub const AUDIO_RING_MASK: usize = AUDIO_RING_CAPACITY - 1;
#[allow(dead_code)]
pub const MAX_AUDIO_BACKLOG: usize = 4096; // 2048 stereo frames (~42.6ms)
#[allow(dead_code)]
pub const DRAIN_AUDIO_TARGET: usize = 1024; // 512 stereo frames (~10.6ms)
pub const VIRTUAL_SINK_SAMPLE_RATE: u32 = 48_000;

/// Lock-free, wait-free, zero-allocation circular audio ring buffer.
///
/// Designed for real-time audio threads under dynamic quantum shifts and clock drift.
/// - Push: Overruns cleanly drop the oldest frames without desynchronizing stereo channels.
/// - Pop: Underruns cleanly fill missing frames with 0.0 silence without blocking or panicking.
/// - Latency control: Purges stale backlog if latency exceeds MAX_AUDIO_BACKLOG.
/// - Zero Mutex locks across both producer and consumer.
pub struct AudioRingBuffer {
    buffer: Box<[AtomicU32; AUDIO_RING_CAPACITY]>,
    write_pos: AtomicUsize,
    read_pos: AtomicUsize,
}

impl Default for AudioRingBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioRingBuffer {
    pub fn new() -> Self {
        let mut v = Vec::with_capacity(AUDIO_RING_CAPACITY);
        for _ in 0..AUDIO_RING_CAPACITY {
            v.push(AtomicU32::new(0));
        }
        let boxed_slice = v.into_boxed_slice();
        let buffer = match boxed_slice.try_into() {
            Ok(b) => b,
            Err(_) => unreachable!(),
        };
        Self {
            buffer,
            write_pos: AtomicUsize::new(0),
            read_pos: AtomicUsize::new(0),
        }
    }

    #[inline]
    pub fn push_slice(&self, input: &[f32]) {
        let len = input.len() & !1;
        if len == 0 {
            return;
        }
        let to_push = len.min(AUDIO_RING_CAPACITY);
        let start_idx = len - to_push;
        let input = &input[start_idx..start_idx + to_push];

        let w = self.write_pos.load(Ordering::Relaxed);
        for (i, &sample) in input.iter().enumerate() {
            let clamped = sample.clamp(-1.0, 1.0);
            self.buffer[(w.wrapping_add(i)) & AUDIO_RING_MASK].store(clamped.to_bits(), Ordering::Relaxed);
        }
        self.write_pos.store(w.wrapping_add(to_push), Ordering::Release);
    }

    /// Pop a stereo frame [Left, Right].
    ///
    /// Strictly guarantees stereo interlock (frame[0] is Left, frame[1] is Right).
    /// If fewer than 2 samples (1 stereo frame) are available, returns None.
    #[inline]
    pub fn pop(&self) -> Option<[f32; 2]> {
        let w = self.write_pos.load(Ordering::Acquire);
        let mut r = self.read_pos.load(Ordering::Relaxed);
        let mut available = w.wrapping_sub(r);

        // Hardware overrun protection: if producer wrapped past capacity while consumer was paused
        if available > AUDIO_RING_CAPACITY {
            r = (w.wrapping_sub(AUDIO_RING_CAPACITY)) & !1;
            self.read_pos.store(r, Ordering::Relaxed);
            available = w.wrapping_sub(r);
        }

        if available < 2 {
            return None;
        }

        let b0 = self.buffer[r & AUDIO_RING_MASK].load(Ordering::Relaxed);
        let b1 = self.buffer[(r.wrapping_add(1)) & AUDIO_RING_MASK].load(Ordering::Relaxed);
        self.read_pos.store(r.wrapping_add(2), Ordering::Release);

        let l = f32::from_bits(b0);
        let right = f32::from_bits(b1);
        let l_clean = if l.is_nan() || l.is_infinite() { 0.0 } else { l };
        let r_clean = if right.is_nan() || right.is_infinite() { 0.0 } else { right };

        Some([l_clean, r_clean])
    }

    #[allow(dead_code)]
    #[inline]
    pub fn pop_slice(&self, output: &mut [f32]) -> usize {
        let requested = output.len() & !1;
        if requested == 0 {
            output.fill(0.0);
            return 0;
        }

        let w = self.write_pos.load(Ordering::Acquire);
        let mut r = self.read_pos.load(Ordering::Relaxed);

        let mut available = w.wrapping_sub(r);

        // Hardware overrun protection: if producer wrapped around while consumer was paused
        if available > AUDIO_RING_CAPACITY {
            r = (w.wrapping_sub(AUDIO_RING_CAPACITY)) & !1;
            available = w.wrapping_sub(r);
        }

        // Clock drift & latency purge: if producer accumulated excessive backlog (> 42ms),
        // cleanly drop oldest frames down to 10.6ms without desynchronizing stereo channels
        if available > MAX_AUDIO_BACKLOG {
            let drop_samples = (available - DRAIN_AUDIO_TARGET) & !1;
            r = r.wrapping_add(drop_samples);
            available = w.wrapping_sub(r);
        }

        let to_read = (requested.min(available)) & !1;

        for (i, out) in output.iter_mut().enumerate().take(to_read) {
            let bits = self.buffer[(r.wrapping_add(i)) & AUDIO_RING_MASK].load(Ordering::Relaxed);
            let s = f32::from_bits(bits);
            *out = if s.is_nan() || s.is_infinite() { 0.0 } else { s };
        }

        // Buffer starvation / underrun protection: pad remainder with 0.0 silence
        if to_read < output.len() {
            output[to_read..].fill(0.0);
        }

        self.read_pos.store(r.wrapping_add(to_read), Ordering::Release);
        to_read
    }

    #[inline]
    pub fn clear(&self) {
        let w = self.write_pos.load(Ordering::Relaxed);
        self.read_pos.store(w, Ordering::Release);
    }
}

/// Zero-allocation linear resampler with continuous phase accumulation across chunks.
pub struct LinearResampler {
    source_rate: u32,
    target_rate: u32,
    phase: f64,
    last_frame: [f32; 2],
    has_last: bool,
}

impl LinearResampler {
    pub fn new(source_rate: u32, target_rate: u32) -> Self {
        Self {
            source_rate: source_rate.max(1),
            target_rate: target_rate.max(1),
            phase: 0.0,
            last_frame: [0.0, 0.0],
            has_last: false,
        }
    }

    #[inline]
    pub fn resample(&mut self, input: &[f32], output: &mut [f32]) -> usize {
        let in_len = input.len() & !1;
        let out_len = output.len() & !1;
        if in_len == 0 || out_len == 0 {
            output.fill(0.0);
            return 0;
        }

        // Fast path: exact sample rate match, zero interpolation overhead
        if self.source_rate == self.target_rate {
            let to_copy = in_len.min(out_len);
            output[..to_copy].copy_from_slice(&input[..to_copy]);
            if to_copy < output.len() {
                output[to_copy..].fill(0.0);
            }
            return to_copy;
        }

        let in_frames = in_len / 2;
        let out_frames = out_len / 2;
        let ratio = self.source_rate as f64 / self.target_rate as f64;

        let mut out_idx = 0usize;

        // Interpolate across chunk boundary using last frame of previous chunk
        while self.has_last && self.phase < 1.0 && out_idx < out_frames {
            let alpha = self.phase as f32;
            let l = self.last_frame[0] * (1.0 - alpha) + input[0] * alpha;
            let r = self.last_frame[1] * (1.0 - alpha) + input[1] * alpha;
            output[out_idx * 2] = l.clamp(-1.0, 1.0);
            output[out_idx * 2 + 1] = r.clamp(-1.0, 1.0);
            out_idx += 1;
            self.phase += ratio;
        }

        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        // Main interpolation loop over current chunk
        while out_idx < out_frames {
            let in_pos = self.phase;
            let idx = in_pos.floor() as usize;
            if idx + 1 >= in_frames {
                break;
            }
            let alpha = (in_pos - idx as f64) as f32;
            let l0 = input[idx * 2];
            let r0 = input[idx * 2 + 1];
            let l1 = input[(idx + 1) * 2];
            let r1 = input[(idx + 1) * 2 + 1];

            let l = l0 * (1.0 - alpha) + l1 * alpha;
            let r = r0 * (1.0 - alpha) + r1 * alpha;
            output[out_idx * 2] = l.clamp(-1.0, 1.0);
            output[out_idx * 2 + 1] = r.clamp(-1.0, 1.0);
            out_idx += 1;
            self.phase += ratio;
        }

        // Store last frame of current input for boundary continuity into next chunk
        self.last_frame = [input[(in_frames - 1) * 2], input[(in_frames - 1) * 2 + 1]];
        self.has_last = true;
        let last_idx = (in_frames - 1) as f64;
        if self.phase >= last_idx {
            self.phase -= last_idx;
        } else {
            self.phase = 0.0;
        }

        let samples_written = out_idx * 2;
        if samples_written < output.len() {
            output[samples_written..].fill(0.0);
        }
        samples_written
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TelemetrySample {
    pub peak_volume_db: f32,
    pub rms_volume_db: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamDeathSignal {
    DeviceNotAvailable,
    #[allow(dead_code)]
    BackendError(String),
}

/// Calculate proper RMS and Peak for a single PCM chunk.
///
/// MUST remain lock-free, wait-free, and allocation-free.
/// Resets accumulators to 0.0 at the beginning of EVERY chunk to prevent leakage/overflow.
#[inline]
pub fn compute_telemetry_chunk(data: &[f32]) -> TelemetrySample {
    if data.is_empty() {
        return TelemetrySample {
            peak_volume_db: -100.0,
            rms_volume_db: -100.0,
        };
    }

    let mut sum_squares = 0.0f32;
    let mut current_peak = 0.0f32;
    let mut valid_samples = 0usize;

    for &sample in data {
        if sample.is_nan() || sample.is_infinite() {
            continue;
        }
        let clamped = sample.clamp(-1.0, 1.0);
        let abs = clamped.abs();
        if abs > current_peak {
            current_peak = abs;
        }
        sum_squares += clamped * clamped;
        valid_samples += 1;
    }

    if valid_samples == 0 {
        return TelemetrySample {
            peak_volume_db: -100.0,
            rms_volume_db: -100.0,
        };
    }

    let rms = (sum_squares / valid_samples as f32).sqrt();

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

    TelemetrySample {
        peak_volume_db: peak_db,
        rms_volume_db: rms_db,
    }
}

/// Thread-safe pusher map enabling virtual sinks to push frames directly to channel ring buffers.
#[derive(Clone)]
pub struct ChannelPusher {
    pub game: Arc<AudioRingBuffer>,
    pub chat: Arc<AudioRingBuffer>,
    pub media: Arc<AudioRingBuffer>,
    pub aux: Arc<AudioRingBuffer>,
    pub mic: Arc<AudioRingBuffer>,
    pub mic_test: Arc<AudioRingBuffer>,
    pub mic_cmd_tx: Sender<GraphCommand>,
    pub mic_cmd_rx: Receiver<GraphCommand>,
    pub mic_peak: Arc<AtomicU32>,
    pub mic_rms: Arc<AtomicU32>,
    pub mic_noise_enabled: Arc<AtomicBool>,
    pub mic_test_enabled: Arc<AtomicBool>,
    pub mic_muted: Arc<AtomicBool>,
    pub mic_gain: Arc<AtomicU32>,
}

impl Default for ChannelPusher {
    fn default() -> Self {
        let (mic_cmd_tx, mic_cmd_rx) = crossbeam_channel::bounded::<GraphCommand>(128);
        Self {
            game: Arc::new(AudioRingBuffer::new()),
            chat: Arc::new(AudioRingBuffer::new()),
            media: Arc::new(AudioRingBuffer::new()),
            aux: Arc::new(AudioRingBuffer::new()),
            mic: Arc::new(AudioRingBuffer::new()),
            mic_test: Arc::new(AudioRingBuffer::new()),
            mic_cmd_tx,
            mic_cmd_rx,
            mic_peak: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
            mic_rms: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
            mic_noise_enabled: Arc::new(AtomicBool::new(false)),
            mic_test_enabled: Arc::new(AtomicBool::new(false)),
            mic_muted: Arc::new(AtomicBool::new(false)),
            mic_gain: Arc::new(AtomicU32::new(1.0f32.to_bits())),
        }
    }
}

impl ChannelPusher {
    pub fn get_slot(&self, channel: i32) -> Option<&Arc<AudioRingBuffer>> {
        match channel {
            1 => Some(&self.game),
            2 => Some(&self.chat),
            3 => Some(&self.media),
            4 => Some(&self.aux),
            5 => Some(&self.mic),
            _ => None,
        }
    }
}

pub struct ChannelRouteState {
    #[allow(dead_code)]
    pub channel: i32,
    pub channel_name: String,
    pub target_device: Option<String>,
    pub output_stream: Option<Stream>,
    pub ring_buffer: Arc<AudioRingBuffer>,
    pub peak_db: Arc<AtomicU32>,
    pub rms_db: Arc<AtomicU32>,
    pub is_active: Arc<AtomicBool>,
}

pub struct NativeVirtualMic {
    child: Option<std::process::Child>,
    node_id: Option<u32>,
}

impl NativeVirtualMic {
    pub fn spawn() -> Result<Self> {
        Self::cleanup_dangling();

        #[cfg(target_os = "linux")]
        use std::os::unix::process::CommandExt;

        let mut cmd = std::process::Command::new("pw-cli");
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());

        #[cfg(target_os = "linux")]
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }

        let mut child = cmd
            .spawn()
            .context("Failed to spawn pw-cli for native virtual mic")?;

        if let Some(ref mut stdin) = child.stdin {
            use std::io::Write;
            writeln!(
                stdin,
                "create-node adapter {{ factory.name=support.null-audio-sink node.name=\"routune-virtual-mic\" node.description=\"Routune Mic\" media.class=Audio/Source/Virtual audio.position=[FL FR] }}"
            )?;
            stdin.flush()?;
        }

        let mut node_id = None;
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            if let Some(id) = Self::find_node_id("routune-virtual-mic") {
                node_id = Some(id);
                break;
            }
        }

        info!("Native PipeWire virtual mic spawned successfully (node_id: {:?})", node_id);
        if let Some(id) = node_id {
            let _ = std::process::Command::new("wpctl")
                .args(["set-default", &id.to_string()])
                .output();
        }
        let _ = std::process::Command::new("pactl")
            .args(["set-default-source", "routune-virtual-mic"])
            .output();

        Ok(Self {
            child: Some(child),
            node_id,
        })
    }

    pub fn cleanup_dangling() {
        if let Ok(output) = std::process::Command::new("pw-dump").arg("Node").output()
            && output.status.success()
                && let Ok(val) = serde_json::from_slice::<serde_json::Value>(&output.stdout)
                    && let Some(nodes) = val.as_array() {
                        for node in nodes {
                            let name = node["info"]["props"]["node.name"].as_str().unwrap_or("");
                            if name == "routune-virtual-mic"
                                && let Some(id) = node["id"].as_u64() {
                                    info!("Cleaning up dangling native PipeWire virtual mic node ID {id}");
                                    let _ = std::process::Command::new("pw-cli")
                                        .stdin(std::process::Stdio::null())
                                        .args(["destroy", &id.to_string()])
                                        .output();
                                }
                        }
                    }
    }

    pub fn find_node_id(node_name: &str) -> Option<u32> {
        let output = std::process::Command::new("pw-dump").arg("Node").output().ok()?;
        if !output.status.success() {
            return None;
        }
        let val: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
        let nodes = val.as_array()?;
        for node in nodes {
            if node["info"]["props"]["node.name"].as_str() == Some(node_name) {
                return node["id"].as_u64().map(|id| id as u32);
            }
        }
        None
    }
}

impl Drop for NativeVirtualMic {
    fn drop(&mut self) {
        if let Some(id) = self.node_id.take() {
            info!("Destroying native PipeWire virtual mic node ID {id}");
            let _ = std::process::Command::new("pw-cli")
                .stdin(std::process::Stdio::null())
                .args(["destroy", &id.to_string()])
                .output();
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Find PipeWire node id by node.name property from a pw-dump items array.
fn find_node_id_in_dump(items: &[serde_json::Value], node_name: &str) -> Option<u64> {
    for obj in items {
        if obj.get("type").and_then(|t| t.as_str()) == Some("PipeWire:Interface:Node")
            && obj["info"]["props"]["node.name"].as_str() == Some(node_name) {
                return obj["id"].as_u64();
            }
    }
    None
}

fn find_all_dsp_out_ids(items: &[serde_json::Value]) -> Vec<u64> {
    let mut ids = Vec::new();
    for obj in items {
        if obj.get("type").and_then(|t| t.as_str()) == Some("PipeWire:Interface:Node") {
            let props = &obj["info"]["props"];
            let name = props["node.name"].as_str().unwrap_or("");
            let app = props["application.name"].as_str().unwrap_or("");
            let bin = props["application.process.binary"].as_str().unwrap_or("");
            if (name == "routune-mic-dsp-out"
                || (bin == "routune-daemon" && app.contains("ALSA plug-in")))
                && let Some(id) = obj["id"].as_u64() {
                    ids.push(id);
                }
        }
    }
    ids
}

fn find_all_capture_in_ids(items: &[serde_json::Value]) -> Vec<u64> {
    let mut ids = Vec::new();
    for obj in items {
        if obj.get("type").and_then(|t| t.as_str()) == Some("PipeWire:Interface:Node") {
            let props = &obj["info"]["props"];
            let bin = props["application.process.binary"].as_str().unwrap_or("");
            let app = props["application.name"].as_str().unwrap_or("");
            let media_class = props["media.class"].as_str().unwrap_or("");
            let media_name = props["media.name"].as_str().unwrap_or("");
            if (bin == "routune-daemon" || app.contains("routune-daemon"))
                && (media_class == "Stream/Input/Audio" || media_name.contains("Capture"))
                && let Some(id) = obj["id"].as_u64() {
                    ids.push(id);
                }
        }
    }
    ids
}

fn find_physical_source_node(items: &[serde_json::Value]) -> Option<u64> {
    for obj in items {
        if obj.get("type").and_then(|t| t.as_str()) == Some("PipeWire:Interface:Node") {
            let props = &obj["info"]["props"];
            let media_class = props.get("media.class").and_then(|v| v.as_str()).unwrap_or("");
            let node_name = props.get("node.name").and_then(|v| v.as_str()).unwrap_or("");
            if media_class == "Audio/Source" && node_name != "routune-virtual-mic" {
                return obj["id"].as_u64();
            }
        }
    }
    None
}

static IS_UNLINKING: AtomicBool = AtomicBool::new(false);

pub fn unlink_unwanted_mic_dsp_links() {
    if IS_UNLINKING.swap(true, Ordering::SeqCst) {
        return;
    }
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            IS_UNLINKING.store(false, Ordering::SeqCst);
        }
    }
    let _guard = Guard;

    // Fast path: explicitly sever links to routune-game default playback sink
    let _ = std::process::Command::new("pw-link")
        .stdin(std::process::Stdio::null())
        .args(["-d", "routune-mic-dsp-out:output_FL", "routune-game:playback_FL"])
        .output();
    let _ = std::process::Command::new("pw-link")
        .stdin(std::process::Stdio::null())
        .args(["-d", "routune-mic-dsp-out:output_FR", "routune-game:playback_FR"])
        .output();
    let _ = std::process::Command::new("pw-link")
        .stdin(std::process::Stdio::null())
        .args(["-d", "ALSA plug-in [routune-daemon]:output_FL", "routune-game:playback_FL"])
        .output();
    let _ = std::process::Command::new("pw-link")
        .stdin(std::process::Stdio::null())
        .args(["-d", "ALSA plug-in [routune-daemon]:output_FR", "routune-game:playback_FR"])
        .output();

    let output = match std::process::Command::new("pw-dump")
        .stdin(std::process::Stdio::null())
        .output()
    {
        Ok(o) if o.status.success() => o.stdout,
        _ => return,
    };

    let objects: serde_json::Value = match serde_json::from_slice(&output) {
        Ok(v) => v,
        _ => return,
    };

    let items = match objects.as_array() {
        Some(a) => a,
        None => return,
    };

    let dsp_ids = find_all_dsp_out_ids(items);
    let virtual_mic_id = match find_node_id_in_dump(items, "routune-virtual-mic") {
        Some(id) => id,
        None => return,
    };
    let capture_in_ids = find_all_capture_in_ids(items);
    let phys_source_id = find_physical_source_node(items);

    for obj in items {
        if obj.get("type").and_then(|t| t.as_str()) == Some("PipeWire:Interface:Link") {
            let info = &obj["info"];
            let out_node = info.get("output-node-id").and_then(|v| v.as_u64());
            let in_node = info.get("input-node-id").and_then(|v| v.as_u64());

            // 1. Mic DSP output must only route to routune-virtual-mic, unlink anything else
            if let Some(out_id) = out_node
                && dsp_ids.contains(&out_id) && in_node != Some(virtual_mic_id)
                    && let Some(link_id) = obj["id"].as_u64() {
                        info!("Removing unwanted autoconnect link {link_id} from mic DSP output (input node {in_node:?})");
                        let _ = std::process::Command::new("pw-link")
                            .stdin(std::process::Stdio::null())
                            .args(["-d", &link_id.to_string()])
                            .output();
                    }

            // 2. Capture inputs of routune-daemon must NEVER be linked to routune-virtual-mic (feedback loop)
            if out_node == Some(virtual_mic_id)
                && let Some(in_id) = in_node
                    && capture_in_ids.contains(&in_id)
                        && let Some(link_id) = obj["id"].as_u64() {
                            info!("Removing invalid feedback link {link_id} from virtual mic to routune capture node {in_id}");
                            let _ = std::process::Command::new("pw-link")
                                .stdin(std::process::Stdio::null())
                                .args(["-d", &link_id.to_string()])
                                .output();
                        }
        }
    }

    // 3. Ensure routune capture stream is linked to the physical microphone source
    if let Some(phys_id) = phys_source_id
        && !capture_in_ids.is_empty() {
            let mut phys_ports = Vec::new();
            let mut cap_ports = Vec::new();
            for obj in items {
                if obj.get("type").and_then(|t| t.as_str()) == Some("PipeWire:Interface:Port") {
                    let port_info = &obj["info"];
                    let dir = port_info.get("direction").and_then(|v| v.as_str()).unwrap_or("");
                    let node_id = port_info["props"].get("node.id").and_then(|v| v.as_u64());
                    if let Some(pid) = obj["id"].as_u64() {
                        if node_id == Some(phys_id) && dir == "output" {
                            phys_ports.push(pid);
                        } else if node_id.is_some_and(|nid| capture_in_ids.contains(&nid)) && dir == "input" {
                            cap_ports.push(pid);
                        }
                    }
                }
            }

            for p_port in &phys_ports {
                for c_port in &cap_ports {
                    let _ = std::process::Command::new("pw-link")
                        .stdin(std::process::Stdio::null())
                        .args([&p_port.to_string(), &c_port.to_string()])
                        .output();
                }
            }
        }
}

pub fn link_mic_dsp_to_virtual_mic() {
    std::thread::spawn(move || {
        let mut linked = false;
        for attempt in 1..=60 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            unlink_unwanted_mic_dsp_links();

            let res_l = std::process::Command::new("pw-link")
                .stdin(std::process::Stdio::null())
                .args(["routune-mic-dsp-out:output_FL", "routune-virtual-mic:input_FL"])
                .output();
            let res_r = std::process::Command::new("pw-link")
                .stdin(std::process::Stdio::null())
                .args(["routune-mic-dsp-out:output_FR", "routune-virtual-mic:input_FR"])
                .output();

            let l_ok = res_l.as_ref().is_ok_and(|o| {
                o.status.success() || String::from_utf8_lossy(&o.stderr).contains("File exists")
            });
            let r_ok = res_r.as_ref().is_ok_and(|o| {
                o.status.success() || String::from_utf8_lossy(&o.stderr).contains("File exists")
            });

            if l_ok && r_ok {
                info!("Successfully linked routune-mic-dsp-out -> routune-virtual-mic on attempt {attempt}");
                linked = true;
                break;
            } else {
                // Fallback attempt with ALSA plug-in name if pulse node property was delayed
                let fb_l = std::process::Command::new("pw-link")
                    .stdin(std::process::Stdio::null())
                    .args(["ALSA plug-in [routune-daemon]:output_FL", "routune-virtual-mic:input_FL"])
                    .output();
                let fb_r = std::process::Command::new("pw-link")
                    .stdin(std::process::Stdio::null())
                    .args(["ALSA plug-in [routune-daemon]:output_FR", "routune-virtual-mic:input_FR"])
                    .output();
                let fb_l_ok = fb_l.as_ref().is_ok_and(|o| {
                    o.status.success() || String::from_utf8_lossy(&o.stderr).contains("File exists")
                });
                let fb_r_ok = fb_r.as_ref().is_ok_and(|o| {
                    o.status.success() || String::from_utf8_lossy(&o.stderr).contains("File exists")
                });
                if fb_l_ok && fb_r_ok {
                    info!("Successfully linked routune-mic-dsp-out (via ALSA plugin name) -> routune-virtual-mic on attempt {attempt}");
                    linked = true;
                    break;
                }
            }
        }

        if !linked {
            warn!("routune-mic-dsp-out not immediately available; supervisor loop will retry periodically");
        }

        // Supervisor loop: periodically ensure mic DSP is linked and unwanted autoconnects purged
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            unlink_unwanted_mic_dsp_links();
            let _ = std::process::Command::new("pw-link")
                .stdin(std::process::Stdio::null())
                .args(["routune-mic-dsp-out:output_FL", "routune-virtual-mic:input_FL"])
                .output();
            let _ = std::process::Command::new("pw-link")
                .stdin(std::process::Stdio::null())
                .args(["routune-mic-dsp-out:output_FR", "routune-virtual-mic:input_FR"])
                .output();
            let _ = std::process::Command::new("pw-link")
                .stdin(std::process::Stdio::null())
                .args(["ALSA plug-in [routune-daemon]:output_FL", "routune-virtual-mic:input_FL"])
                .output();
            let _ = std::process::Command::new("pw-link")
                .stdin(std::process::Stdio::null())
                .args(["ALSA plug-in [routune-daemon]:output_FR", "routune-virtual-mic:input_FR"])
                .output();
        }
    });
}

pub struct ChannelRouter {
    host: cpal::Host,
    pusher: ChannelPusher,
    routes: HashMap<i32, ChannelRouteState>,
    death_tx: Sender<StreamDeathSignal>,
    death_rx: Receiver<StreamDeathSignal>,
    mic_dsp_output_stream: Option<Stream>,
    native_virtual_mic: Option<NativeVirtualMic>,
}

unsafe impl Send for ChannelRouter {}

pub fn enumerate_host_devices(host: &cpal::Host) -> Vec<cpal::Device> {
    #[cfg(target_os = "linux")]
    let _silence = StderrSilencer::new();

    if let Ok(devs) = host.devices() {
        return devs.collect();
    }
    if let Ok(devs) = host.output_devices() {
        return devs.collect();
    }
    Vec::new()
}

pub fn extract_card_name(name: &str) -> Option<String> {
    let lower = name.to_lowercase();
    if let Some(card_idx) = lower.find("card=") {
        let after_card = &name[card_idx + 5..];
        let card_raw = after_card.split(',').next().unwrap_or(after_card).trim();
        if !card_raw.is_empty() {
            return Some(card_raw.to_string());
        }
    }
    None
}

impl ChannelRouter {
    pub fn new() -> (Self, ChannelPusher) {
        let host = get_audio_host();
        let pusher = ChannelPusher::default();
        let (death_tx, death_rx) = crossbeam_channel::bounded::<StreamDeathSignal>(32);

        let mut routes = HashMap::new();
        let channels = [
            (1, "Game"),
            (2, "Chat"),
            (3, "Media"),
            (4, "Aux"),
            (5, "Mic"),
        ];

        for (ch, name) in channels {
            let peak_db = if ch == 5 {
                pusher.mic_peak.clone()
            } else {
                Arc::new(AtomicU32::new((-100.0f32).to_bits()))
            };
            let rms_db = if ch == 5 {
                pusher.mic_rms.clone()
            } else {
                Arc::new(AtomicU32::new((-100.0f32).to_bits()))
            };
            let ring_buffer = pusher.get_slot(ch).cloned().unwrap_or_else(|| Arc::new(AudioRingBuffer::new()));
            routes.insert(
                ch,
                ChannelRouteState {
                    channel: ch,
                    channel_name: name.to_string(),
                    target_device: None,
                    output_stream: None,
                    ring_buffer,
                    peak_db,
                    rms_db,
                    is_active: Arc::new(AtomicBool::new(false)),
                },
            );
        }

        (
            Self {
                host,
                pusher: pusher.clone(),
                routes,
                death_tx,
                death_rx,
                mic_dsp_output_stream: None,
                native_virtual_mic: None,
            },
            pusher,
        )
    }

    pub fn start_mic_dsp_output_stream(&mut self) -> Result<()> {
        #[cfg(target_os = "linux")]
        let _silence = StderrSilencer::new();

        let device = self.find_device("pulse")
            .or_else(|| self.find_device("default"))
            .or_else(|| self.host.default_output_device())
            .or_else(|| {
                self.host.output_devices().ok()?.find(|d| {
                    d.name().is_ok_and(|n| n == "pulse" || n == "default" || n == "pipewire")
                })
            })
            .context("No suitable output device for mic DSP output stream")?;

        let (config, sample_format) = choose_best_stream_config(&device, VIRTUAL_SINK_SAMPLE_RATE)?;

        // PULSE_PROP is honoured by the PulseAudio ALSA plugin (which PipeWire emulates),
        // correctly setting node.name on the PW node.
        // NOTE: Do not include node.autoconnect=false here as libasound_module_pcm_pulse
        // will timeout waiting for a ready state when autoconnect is disabled.
        // Instead, autoconnected links to playback sinks are unlinked immediately below.
        unsafe {
            std::env::set_var(
                "PULSE_PROP",
                "media.role=dsp node.name=routune-mic-dsp-out",
            );
        }

        let is_active = Arc::new(AtomicBool::new(false));
        let peak_db = Arc::new(AtomicU32::new((-100.0f32).to_bits()));
        let rms_db = Arc::new(AtomicU32::new((-100.0f32).to_bits()));
        let ring_buffer = self.pusher.mic.clone();

        let last_err_ms = Arc::new(AtomicU64::new(0));
        let err_fn = move |err: cpal::StreamError| {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let prev = last_err_ms.load(Ordering::Relaxed);
            if now_ms.saturating_sub(prev) >= 2000
                && last_err_ms.compare_exchange(prev, now_ms, Ordering::SeqCst, Ordering::Relaxed).is_ok() {
                warn!("Mic DSP output stream error: {err:?}");
            }
        };

        let stream_res = build_channel_output_stream(
            &device,
            &config,
            sample_format,
            ring_buffer,
            peak_db,
            rms_db,
            is_active,
            err_fn,
            None,
            None,
        );

        unsafe {
            std::env::remove_var("PULSE_PROP");
            std::env::remove_var("PIPEWIRE_AUTOCONNECT");
            std::env::remove_var("PIPEWIRE_PROPS");
        }

        let stream = stream_res.context("Failed to build mic DSP output stream")?;
        stream.play().context("Failed to start mic DSP output stream")?;
        self.mic_dsp_output_stream = Some(stream);
        info!("Started CPAL mic DSP output stream ('routune-mic-dsp-out')");

        Ok(())
    }

    pub fn init_virtual_mic_pipeline(&mut self) -> Result<()> {
        // Step 1: Native PipeWire Virtual Source Creation
        let native_mic = NativeVirtualMic::spawn()?;
        self.native_virtual_mic = Some(native_mic);

        // Step 2: The CPAL DSP Output Stream
        self.start_mic_dsp_output_stream()?;

        // Step 3: The Auto-Link Bridge (pw-link)
        link_mic_dsp_to_virtual_mic();
        unlink_unwanted_mic_dsp_links();

        Ok(())
    }

    pub fn get_hardware_outputs(&self) -> Vec<proto::OutputDeviceInfo> {
        #[cfg(target_os = "linux")]
        let _silence = StderrSilencer::new();

        use std::collections::HashMap;
        let mut dedup_map: HashMap<String, (proto::OutputDeviceInfo, i32)> = HashMap::new();

        let devices = enumerate_host_devices(&self.host);
        for dev in devices {
            if let Ok(name) = dev.name() {
                if is_rejected_device_name(&name) {
                    continue;
                }

                    let (channels, sample_rate) = if let Ok(cfg) = dev.default_output_config() {
                        (cfg.channels() as u32, cfg.sample_rate().0)
                    } else {
                        (2, 48000)
                    };

                    let display_name = format_device_display_name(&name);
                    let priority = device_id_priority(&name);

                    let info = proto::OutputDeviceInfo {
                        id: name.clone(),
                        name: name.clone(),
                        display_name: display_name.clone(),
                        channels,
                        sample_rate,
                    };

                    match dedup_map.get(&display_name) {
                        Some((_, existing_pri)) if *existing_pri >= priority => {}
                        _ => {
                            dedup_map.insert(display_name, (info, priority));
                        }
                    }
                }
            }

        let mut outputs: Vec<proto::OutputDeviceInfo> = dedup_map.into_values().map(|(info, _)| info).collect();

        // Sort: USB Audio & Analog physical devices first
        outputs.sort_by_key(|o| -score_physical_output_device(&o.name));
        outputs
    }

    pub fn get_hardware_inputs(&self) -> Vec<proto::OutputDeviceInfo> {
        #[cfg(target_os = "linux")]
        let _silence = StderrSilencer::new();

        use std::collections::HashMap;
        let mut dedup_map: HashMap<String, (proto::OutputDeviceInfo, i32)> = HashMap::new();

        let mut devices: Vec<cpal::Device> = self.host.input_devices().map(|d| d.collect()).unwrap_or_default();
        if let Ok(all_devs) = self.host.devices() {
            for dev in all_devs {
                if !devices.iter().any(|existing| existing.name().ok() == dev.name().ok())
                    && (dev.default_input_config().is_ok()
                        || dev.supported_input_configs().map(|mut it| it.next().is_some()).unwrap_or(false))
                    {
                        devices.push(dev);
                    }
            }
        }
        for dev in devices {
            if let Ok(name) = dev.name() {
                if is_rejected_device_name(&name) {
                    continue;
                }

                let (channels, sample_rate) = if let Ok(cfg) = dev.default_input_config() {
                    (cfg.channels() as u32, cfg.sample_rate().0)
                } else {
                    (1, 48000)
                };

                let display_name = format_input_device_display_name(&name);
                let priority = device_id_priority(&name);

                let info = proto::OutputDeviceInfo {
                    id: name.clone(),
                    name: name.clone(),
                    display_name: display_name.clone(),
                    channels,
                    sample_rate,
                };

                match dedup_map.get(&display_name) {
                    Some((_, existing_pri)) if *existing_pri >= priority => {}
                    _ => {
                        dedup_map.insert(display_name, (info, priority));
                    }
                }
            }
        }

        let mut inputs: Vec<proto::OutputDeviceInfo> = dedup_map.into_values().map(|(info, _)| info).collect();
        inputs.sort_by_key(|o| -score_physical_input_device(&o.name));
        inputs
    }

    pub fn set_channel_routing(&mut self, channel: i32, target_device_name: &str) -> Result<String> {
        #[cfg(target_os = "linux")]
        let _silence = StderrSilencer::new();

        let trimmed = target_device_name.trim();

        // Special handling for Mic (Channel 5): physical capture stream
        if channel == 5 {
            if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") || trimmed.eq_ignore_ascii_case("unassigned") {
                let route = self.routes.get_mut(&5)
                    .ok_or_else(|| anyhow::anyhow!("Invalid audio channel: 5"))?;
                drop(route.output_stream.take());
                route.is_active.store(false, Ordering::Relaxed);
                route.target_device = None;
                self.pusher.mic_peak.store((-100.0f32).to_bits(), Ordering::Relaxed);
                self.pusher.mic_rms.store((-100.0f32).to_bits(), Ordering::Relaxed);
                self.pusher.mic.clear();
                info!("Unassigned microphone input routing");
                return Ok("Unassigned".to_string());
            }

            let target_device = self.find_input_device(trimmed)
                .ok_or_else(|| anyhow::anyhow!("Input device not found: {trimmed}"))?;
            let actual_name = target_device.name().unwrap_or_else(|_| trimmed.to_string());

            let (config, sample_format) = choose_best_input_stream_config(&target_device, VIRTUAL_SINK_SAMPLE_RATE)?;
            let route = self.routes.get_mut(&5)
                .ok_or_else(|| anyhow::anyhow!("Invalid audio channel: 5"))?;

            drop(route.output_stream.take());
            route.is_active.store(false, Ordering::Relaxed);

            info!(
                "Binding Mic input -> '{}' (format={:?}, rate={}, channels={})",
                actual_name, sample_format, config.sample_rate.0, config.channels
            );

            self.pusher.mic.clear();

            let death_tx = self.death_tx.clone();
            let is_active = route.is_active.clone();
            let err_name = actual_name.clone();
            let last_err_ms = Arc::new(AtomicU64::new(0));

            let mut formats_to_try = vec![sample_format];
            if !formats_to_try.contains(&SampleFormat::I16) {
                formats_to_try.push(SampleFormat::I16);
            }
            if !formats_to_try.contains(&SampleFormat::F32) {
                formats_to_try.push(SampleFormat::F32);
            }

            let mut last_err = None;
            let mut built_stream = None;

            for fmt in formats_to_try {
                let death_tx_cl = death_tx.clone();
                let is_active_cl = is_active.clone();
                let err_name_cl = err_name.clone();
                let last_err_ms_cl = last_err_ms.clone();
                let err_fn = move |err: cpal::StreamError| {
                    is_active_cl.store(false, Ordering::Relaxed);
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    let prev = last_err_ms_cl.load(Ordering::Relaxed);
                    if now_ms.saturating_sub(prev) >= 2000
                        && last_err_ms_cl.compare_exchange(prev, now_ms, Ordering::SeqCst, Ordering::Relaxed).is_ok() {
                            warn!("Mic stream error ({err_name_cl}): {err:?}");
                            let _ = death_tx_cl.try_send(StreamDeathSignal::DeviceNotAvailable);
                        }
                };

                match build_channel_input_stream(
                    &target_device,
                    &config,
                    fmt,
                    self.pusher.mic.clone(),
                    self.pusher.mic_cmd_rx.clone(),
                    self.pusher.mic_peak.clone(),
                    self.pusher.mic_rms.clone(),
                    route.is_active.clone(),
                    self.pusher.mic_noise_enabled.clone(),
                    self.pusher.mic_muted.clone(),
                    self.pusher.mic_gain.clone(),
                    err_fn,
                    self.pusher.mic_test_enabled.clone(),
                    self.pusher.mic_test.clone(),
                ) {
                    Ok(stream) => match stream.play() {
                        Ok(()) => {
                            if fmt != sample_format {
                                info!(
                                    "Successfully bound Mic input using fallback format {:?} -> '{}'",
                                    fmt, actual_name
                                );
                            }
                            built_stream = Some(stream);
                            break;
                        }
                        Err(play_err) => {
                            warn!("Failed to start mic capture stream ({fmt:?}): {play_err:?}");
                            last_err = Some(anyhow::anyhow!(play_err));
                        }
                    },
                    Err(build_err) => {
                        tracing::debug!(
                            "Mic input stream build failed for format {:?} on '{}': {build_err:?}",
                            fmt, actual_name
                        );
                        last_err = Some(build_err);
                    }
                }
            }

            let stream = built_stream.ok_or_else(|| {
                last_err.unwrap_or_else(|| anyhow::anyhow!("Failed to build mic stream for {actual_name}"))
            })?;

            route.output_stream = Some(stream);
            route.target_device = Some(actual_name.clone());
            route.is_active.store(true, Ordering::Relaxed);

            info!("Successfully routed Mic directly from hardware '{}'", actual_name);
            return Ok(actual_name);
        }

        // 1. Handle unassigned routing
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") || trimmed.eq_ignore_ascii_case("unassigned") {
            let route = self.routes.get_mut(&channel)
                .ok_or_else(|| anyhow::anyhow!("Invalid audio channel: {channel}"))?;
            drop(route.output_stream.take());
            route.is_active.store(false, Ordering::Relaxed);
            route.target_device = None;
            route.ring_buffer.clear();

            info!("Unassigned output routing for channel {}", route.channel_name);
            return Ok("Unassigned".to_string());
        }

        // 2. Find target physical device before mutable borrow
        let target_device = self.find_device(trimmed)
            .ok_or_else(|| anyhow::anyhow!("Device not found: {trimmed}"))?;
        let actual_name = target_device.name().unwrap_or_else(|_| trimmed.to_string());

        // 3. Negotiate optimal 48kHz stream configuration
        let (config, sample_format) = choose_best_stream_config(&target_device, VIRTUAL_SINK_SAMPLE_RATE)?;
        let is_primary_output = channel == 1 || (channel != 5 && self.routes.get(&1).is_none_or(|r| r.target_device.is_none()));

        let route = self.routes.get_mut(&channel)
            .ok_or_else(|| anyhow::anyhow!("Invalid audio channel: {channel}"))?;

        // 4. Drop existing stream to release hardware handle
        drop(route.output_stream.take());
        route.is_active.store(false, Ordering::Relaxed);

        info!(
            "Binding channel {} -> '{}' (format={:?}, rate={}, channels={})",
            route.channel_name, actual_name, sample_format, config.sample_rate.0, config.channels
        );

        // 5. Clear dedicated RingBuffer bridge
        route.ring_buffer.clear();

        // 6. Build and launch channel output stream
        let death_tx = self.death_tx.clone();
        let is_active = route.is_active.clone();
        let err_name = actual_name.clone();

        let mic_test_enabled = if is_primary_output {
            Some(self.pusher.mic_test_enabled.clone())
        } else {
            None
        };
        let mic_test_ring_buf = if is_primary_output {
            Some(self.pusher.mic_test.clone())
        } else {
            None
        };

        let mut formats_to_try = vec![sample_format];
        if !formats_to_try.contains(&SampleFormat::I16) {
            formats_to_try.push(SampleFormat::I16);
        }
        if !formats_to_try.contains(&SampleFormat::F32) {
            formats_to_try.push(SampleFormat::F32);
        }

        let mut last_err = None;
        let mut built_stream = None;
        let last_err_ms = Arc::new(AtomicU64::new(0));

        for fmt in formats_to_try {
            let death_tx_cl = death_tx.clone();
            let is_active_cl = is_active.clone();
            let err_name_cl = err_name.clone();
            let last_err_ms_cl = last_err_ms.clone();
            let err_fn = move |err: cpal::StreamError| {
                is_active_cl.store(false, Ordering::Relaxed);
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                let prev = last_err_ms_cl.load(Ordering::Relaxed);
                if now_ms.saturating_sub(prev) >= 2000
                    && last_err_ms_cl.compare_exchange(prev, now_ms, Ordering::SeqCst, Ordering::Relaxed).is_ok() {
                        warn!("Channel stream error ({err_name_cl}): {err:?}");
                        let _ = death_tx_cl.try_send(StreamDeathSignal::DeviceNotAvailable);
                    }
            };

            match build_channel_output_stream(
                &target_device,
                &config,
                fmt,
                route.ring_buffer.clone(),
                route.peak_db.clone(),
                route.rms_db.clone(),
                route.is_active.clone(),
                err_fn,
                mic_test_enabled.clone(),
                mic_test_ring_buf.clone(),
            ) {
                Ok(stream) => match stream.play() {
                    Ok(()) => {
                        if fmt != sample_format {
                            info!(
                                "Successfully bound channel {} using fallback format {:?} -> '{}'",
                                route.channel_name, fmt, actual_name
                            );
                        }
                        built_stream = Some(stream);
                        break;
                    }
                    Err(play_err) => {
                        warn!("Failed to start channel physical stream ({fmt:?}): {play_err:?}");
                        last_err = Some(anyhow::anyhow!(play_err));
                    }
                },
                Err(build_err) => {
                    tracing::debug!(
                        "Stream build failed for format {:?} on '{}': {build_err:?}",
                        fmt, actual_name
                    );
                    last_err = Some(build_err);
                }
            }
        }

        let stream = built_stream.ok_or_else(|| {
            last_err.unwrap_or_else(|| anyhow::anyhow!("Failed to build channel stream for {actual_name}"))
        })?;

        route.output_stream = Some(stream);
        route.target_device = Some(actual_name.clone());
        route.is_active.store(true, Ordering::Relaxed);

        info!("Successfully routed channel {} directly to physical hardware '{}'", route.channel_name, actual_name);
        Ok(actual_name)
    }

    fn find_device(&self, query: &str) -> Option<cpal::Device> {
        #[cfg(target_os = "linux")]
        let _silence = StderrSilencer::new();

        let q_trimmed = query.trim();
        let q_lower = q_trimmed.to_lowercase();
        let devices = enumerate_host_devices(&self.host);

        // 1. Direct exact match on raw ID / name across host devices
        for dev in &devices {
            if let Ok(name) = dev.name()
                && (name == q_trimmed || name.to_lowercase() == q_lower) {
                    return Some(dev.clone());
                }
        }

        // 2. Exact ALSA CARD parameter match (e.g. sysdefault:CARD=Device vs front:CARD=Device,DEV=0)
        if let Some(q_card) = extract_card_name(q_trimmed) {
            let mut card_matches: Vec<(cpal::Device, i32)> = Vec::new();
            for dev in &devices {
                if let Ok(name) = dev.name() {
                    if is_rejected_device_name(&name) {
                        continue;
                    }
                    if let Some(d_card) = extract_card_name(&name)
                        && d_card.eq_ignore_ascii_case(&q_card) {
                            let priority = device_id_priority(&name);
                            card_matches.push((dev.clone(), priority));
                        }
                }
            }
            if !card_matches.is_empty() {
                card_matches.sort_by_key(|a| std::cmp::Reverse(a.1));
                return Some(card_matches.remove(0).0);
            }
        }

        // 3. Match hardware device by clean display_name comparison
        let q_disp = format_device_display_name(q_trimmed).to_lowercase();
        let mut disp_matches: Vec<(cpal::Device, i32)> = Vec::new();
        for dev in &devices {
            if let Ok(name) = dev.name() {
                if is_rejected_device_name(&name) {
                    continue;
                }
                let disp = format_device_display_name(&name).to_lowercase();
                if disp == q_disp || disp.contains(&q_lower) || q_lower.contains(&disp) || disp.contains(&q_disp) || q_disp.contains(&disp) {
                    let priority = device_id_priority(&name);
                    disp_matches.push((dev.clone(), priority));
                }
            }
        }
        if !disp_matches.is_empty() {
            disp_matches.sort_by_key(|a| std::cmp::Reverse(a.1));
            return Some(disp_matches.remove(0).0);
        }

        // 4. Fallback: match by raw name substring
        for dev in &devices {
            if let Ok(name) = dev.name() {
                if is_rejected_device_name(&name) {
                    continue;
                }
                if name.to_lowercase().contains(&q_lower) {
                    return Some(dev.clone());
                }
            }
        }

        // 5. Physical device heuristic matching (ryzen, usb, etc.)
        find_physical_output_device(&self.host, query)
            .or_else(|| find_physical_output_device(&self.host, &q_disp))
    }

    fn find_input_device(&self, query: &str) -> Option<cpal::Device> {
        #[cfg(target_os = "linux")]
        let _silence = StderrSilencer::new();

        let q_trimmed = query.trim();
        let q_lower = q_trimmed.to_lowercase();
        let mut devices: Vec<cpal::Device> = self.host.input_devices().map(|d| d.collect()).unwrap_or_default();
        if let Ok(all_devs) = self.host.devices() {
            for dev in all_devs {
                if !devices.iter().any(|existing| existing.name().ok() == dev.name().ok()) {
                    devices.push(dev);
                }
            }
        }

        // 1. Direct exact match on raw ID / name across host input devices
        for dev in &devices {
            if let Ok(name) = dev.name()
                && (name == q_trimmed || name.to_lowercase() == q_lower) {
                    return Some(dev.clone());
                }
        }

        // 2. Exact ALSA CARD parameter match for capture devices
        if let Some(q_card) = extract_card_name(q_trimmed) {
            let mut card_matches: Vec<(cpal::Device, i32)> = Vec::new();
            for dev in &devices {
                if let Ok(name) = dev.name() {
                    if is_rejected_device_name(&name) {
                        continue;
                    }
                    if let Some(d_card) = extract_card_name(&name)
                        && d_card.eq_ignore_ascii_case(&q_card) {
                            let priority = device_id_priority(&name);
                            card_matches.push((dev.clone(), priority));
                        }
                }
            }
            if !card_matches.is_empty() {
                card_matches.sort_by_key(|a| std::cmp::Reverse(a.1));
                return Some(card_matches.remove(0).0);
            }
        }

        // 3. Fallback: match hardware device by clean input display_name
        let q_disp = format_input_device_display_name(q_trimmed).to_lowercase();
        let mut disp_matches: Vec<(cpal::Device, i32)> = Vec::new();
        for dev in &devices {
            if let Ok(name) = dev.name() {
                if is_rejected_device_name(&name) {
                    continue;
                }
                let disp = format_input_device_display_name(&name).to_lowercase();
                if disp == q_disp || disp.contains(&q_lower) || q_lower.contains(&disp) || disp.contains(&q_disp) || q_disp.contains(&disp) {
                    let priority = device_id_priority(&name);
                    disp_matches.push((dev.clone(), priority));
                }
            }
        }
        if !disp_matches.is_empty() {
            disp_matches.sort_by_key(|a| std::cmp::Reverse(a.1));
            return Some(disp_matches.remove(0).0);
        }

        // 4. Fallback: match by raw name substring
        for dev in &devices {
            if let Ok(name) = dev.name() {
                if is_rejected_device_name(&name) {
                    continue;
                }
                if name.to_lowercase().contains(&q_lower) {
                    return Some(dev.clone());
                }
            }
        }

        // 5. Fallback for sound server bridges if target is a PipeWire source node or USB device not directly exposed by ALSA
        if q_lower.starts_with("alsa_input.") || q_lower.contains("usb") {
            for bridge_name in &["pulse", "pipewire", "default"] {
                for dev in &devices {
                    if let Ok(name) = dev.name()
                        && name.eq_ignore_ascii_case(bridge_name) {
                            return Some(dev.clone());
                        }
                }
            }
        }

        None
    }

    pub fn get_channel_metrics(&self) -> Vec<proto::ChannelVolumeMetric> {
        let (m_peak, m_rms) = self.get_master_metrics();
        let mut list = vec![proto::ChannelVolumeMetric {
            channel: proto::AudioChannel::Master as i32,
            channel_name: "Master".to_string(),
            peak_volume_db: m_peak,
            rms_volume_db: m_rms,
            ai_noise_enabled: false,
        }];

        for ch in [1, 2, 3, 4, 5] {
            if let Some(r) = self.routes.get(&ch) {
                let p = Self::read_decay_metric(&r.peak_db);
                let r_val = f32::from_bits(r.rms_db.load(Ordering::Relaxed));
                let ai_noise = if ch == 5 {
                    self.pusher.mic_noise_enabled.load(Ordering::Relaxed)
                } else {
                    false
                };
                list.push(proto::ChannelVolumeMetric {
                    channel: ch,
                    channel_name: r.channel_name.clone(),
                    peak_volume_db: p,
                    rms_volume_db: r_val,
                    ai_noise_enabled: ai_noise,
                });
            }
        }

        list
    }

    fn read_decay_metric(atomic: &AtomicU32) -> f32 {
        let val = f32::from_bits(atomic.load(Ordering::Relaxed));
        let decayed = if val > -95.0 { (val - 2.0).max(-100.0) } else { -100.0 };
        atomic.store(decayed.to_bits(), Ordering::Relaxed);
        val
    }

    pub fn get_master_metrics(&self) -> (f32, f32) {
        let mut p = -100.0f32;
        let mut r = -100.0f32;

        for r_state in self.routes.values() {
            if r_state.channel == 5 {
                continue;
            }
            let ch_p = f32::from_bits(r_state.peak_db.load(Ordering::Relaxed));
            let ch_r = f32::from_bits(r_state.rms_db.load(Ordering::Relaxed));
            if ch_p > p { p = ch_p; }
            if ch_r > r { r = ch_r; }
        }

        (p, r)
    }

    pub fn is_any_active(&self) -> bool {
        self.routes.values().any(|r| r.is_active.load(Ordering::Relaxed))
    }

    pub fn set_mic_test(&self, enabled: bool) {
        self.pusher.mic_test.clear();
        self.pusher.mic_test_enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn is_mic_test_enabled(&self) -> bool {
        self.pusher.mic_test_enabled.load(Ordering::Relaxed)
    }

    pub fn is_channel_stream_active(&self, channel: i32) -> bool {
        self.routes
            .get(&channel)
            .map(|r| r.output_stream.is_some() && r.is_active.load(Ordering::Relaxed))
            .unwrap_or(false)
    }

    #[allow(dead_code)]
    pub fn get_channel_target(&self, channel: i32) -> Option<String> {
        self.routes
            .get(&channel)
            .and_then(|r| r.target_device.clone())
    }

    pub fn try_recv_death_signal(&self) -> Option<StreamDeathSignal> {
        self.death_rx.try_recv().ok()
    }

    pub fn cleanup_inactive_streams(&mut self) {
        for route in self.routes.values_mut() {
            if !route.is_active.load(Ordering::Relaxed) && route.output_stream.is_some() {
                drop(route.output_stream.take());
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_channel_output_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    ring_buffer: Arc<AudioRingBuffer>,
    peak_db: Arc<AtomicU32>,
    rms_db: Arc<AtomicU32>,
    is_active: Arc<AtomicBool>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
    mic_test_enabled: Option<Arc<AtomicBool>>,
    mic_test_ring_buf: Option<Arc<AudioRingBuffer>>,
) -> Result<Stream> {
    let channels = config.channels as usize;

    match sample_format {
        SampleFormat::F32 => {
            device.build_output_stream(
                config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    is_active.store(true, Ordering::Relaxed);
                    if channels == 0 || data.is_empty() { return; }

                    let test_mic = mic_test_enabled
                        .as_ref()
                        .map(|e| e.load(Ordering::Relaxed))
                        .unwrap_or(false);

                    for frame in data.chunks_mut(channels) {
                        let (mut left, mut right) = match ring_buffer.pop() {
                            Some([l, r]) => (l, r),
                            None => (0.0, 0.0),
                        };

                        if test_mic
                            && let Some(ref m_buf) = mic_test_ring_buf
                                && let Some([ml, mr]) = m_buf.pop() {
                                    left += ml;
                                    right += mr;
                                }

                        let left_clamped = left.clamp(-1.0, 1.0);
                        let right_clamped = right.clamp(-1.0, 1.0);

                        if channels >= 2 {
                            frame[0] = left_clamped;
                            frame[1] = right_clamped;
                            frame[2..].fill(0.0);
                        } else {
                            frame[0] = (0.5 * (left_clamped + right_clamped)).clamp(-1.0, 1.0);
                        }
                    }

                    let sample = compute_telemetry_chunk(data);
                    peak_db.store(sample.peak_volume_db.to_bits(), Ordering::Relaxed);
                    rms_db.store(sample.rms_volume_db.to_bits(), Ordering::Relaxed);
                },
                err_fn,
                None,
            ).context("Failed to build F32 channel stream")
        }

        SampleFormat::I16 => {
            let mut float_buf = Box::new([0.0f32; 8192]);

            device.build_output_stream(
                config,
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    is_active.store(true, Ordering::Relaxed);
                    if channels == 0 || data.is_empty() { return; }

                    let test_mic = mic_test_enabled
                        .as_ref()
                        .map(|e| e.load(Ordering::Relaxed))
                        .unwrap_or(false);

                    let mut telem_idx = 0usize;

                    for frame in data.chunks_mut(channels) {
                        let (mut left, mut right) = match ring_buffer.pop() {
                            Some([l, r]) => (l, r),
                            None => (0.0, 0.0),
                        };

                        if test_mic
                            && let Some(ref m_buf) = mic_test_ring_buf
                                && let Some([ml, mr]) = m_buf.pop() {
                                    left += ml;
                                    right += mr;
                                }

                        let left_clamped = left.clamp(-1.0, 1.0);
                        let right_clamped = right.clamp(-1.0, 1.0);

                        if channels >= 2 {
                            frame[0] = (left_clamped * 32767.0).clamp(-32768.0, 32767.0) as i16;
                            frame[1] = (right_clamped * 32767.0).clamp(-32768.0, 32767.0) as i16;
                            frame[2..].fill(0);

                            if telem_idx + 1 < 8192 {
                                float_buf[telem_idx] = left_clamped;
                                float_buf[telem_idx + 1] = right_clamped;
                                telem_idx += 2;
                            }
                        } else {
                            let mono = (0.5 * (left_clamped + right_clamped)).clamp(-1.0, 1.0);
                            frame[0] = (mono * 32767.0).clamp(-32768.0, 32767.0) as i16;

                            if telem_idx < 8192 {
                                float_buf[telem_idx] = mono;
                                telem_idx += 1;
                            }
                        }
                    }

                    let sample = compute_telemetry_chunk(&float_buf[..telem_idx]);
                    peak_db.store(sample.peak_volume_db.to_bits(), Ordering::Relaxed);
                    rms_db.store(sample.rms_volume_db.to_bits(), Ordering::Relaxed);
                },
                err_fn,
                None,
            ).context("Failed to build I16 channel stream")
        }

        SampleFormat::I32 => {
            let mut float_buf = Box::new([0.0f32; 8192]);

            device.build_output_stream(
                config,
                move |data: &mut [i32], _: &cpal::OutputCallbackInfo| {
                    is_active.store(true, Ordering::Relaxed);
                    if channels == 0 || data.is_empty() { return; }

                    let test_mic = mic_test_enabled
                        .as_ref()
                        .map(|e| e.load(Ordering::Relaxed))
                        .unwrap_or(false);

                    let mut telem_idx = 0usize;

                    for frame in data.chunks_mut(channels) {
                        let (mut left, mut right) = match ring_buffer.pop() {
                            Some([l, r]) => (l, r),
                            None => (0.0, 0.0),
                        };

                        if test_mic
                            && let Some(ref m_buf) = mic_test_ring_buf
                                && let Some([ml, mr]) = m_buf.pop() {
                                    left += ml;
                                    right += mr;
                                }

                        let left_clamped = left.clamp(-1.0, 1.0);
                        let right_clamped = right.clamp(-1.0, 1.0);

                        if channels >= 2 {
                            frame[0] = (left_clamped * 2147483647.0).clamp(-2147483648.0, 2147483647.0) as i32;
                            frame[1] = (right_clamped * 2147483647.0).clamp(-2147483648.0, 2147483647.0) as i32;
                            frame[2..].fill(0);

                            if telem_idx + 1 < 8192 {
                                float_buf[telem_idx] = left_clamped;
                                float_buf[telem_idx + 1] = right_clamped;
                                telem_idx += 2;
                            }
                        } else {
                            let mono = (0.5 * (left_clamped + right_clamped)).clamp(-1.0, 1.0);
                            frame[0] = (mono * 2147483647.0).clamp(-2147483648.0, 2147483647.0) as i32;

                            if telem_idx < 8192 {
                                float_buf[telem_idx] = mono;
                                telem_idx += 1;
                            }
                        }
                    }

                    let sample = compute_telemetry_chunk(&float_buf[..telem_idx]);
                    peak_db.store(sample.peak_volume_db.to_bits(), Ordering::Relaxed);
                    rms_db.store(sample.rms_volume_db.to_bits(), Ordering::Relaxed);
                },
                err_fn,
                None,
            ).context("Failed to build I32 channel stream")
        }

        SampleFormat::U16 => {
            let mut float_buf = Box::new([0.0f32; 8192]);

            device.build_output_stream(
                config,
                move |data: &mut [u16], _: &cpal::OutputCallbackInfo| {
                    is_active.store(true, Ordering::Relaxed);
                    if channels == 0 || data.is_empty() { return; }

                    let test_mic = mic_test_enabled
                        .as_ref()
                        .map(|e| e.load(Ordering::Relaxed))
                        .unwrap_or(false);

                    let mut telem_idx = 0usize;

                    for frame in data.chunks_mut(channels) {
                        let (mut left, mut right) = match ring_buffer.pop() {
                            Some([l, r]) => (l, r),
                            None => (0.0, 0.0),
                        };

                        if test_mic
                            && let Some(ref m_buf) = mic_test_ring_buf
                                && let Some([ml, mr]) = m_buf.pop() {
                                    left += ml;
                                    right += mr;
                                }

                        let left_clamped = left.clamp(-1.0, 1.0);
                        let right_clamped = right.clamp(-1.0, 1.0);

                        if channels >= 2 {
                            frame[0] = ((left_clamped * 32767.0) + 32768.0).clamp(0.0, 65535.0) as u16;
                            frame[1] = ((right_clamped * 32767.0) + 32768.0).clamp(0.0, 65535.0) as u16;
                            frame[2..].fill(32768);

                            if telem_idx + 1 < 8192 {
                                float_buf[telem_idx] = left_clamped;
                                float_buf[telem_idx + 1] = right_clamped;
                                telem_idx += 2;
                            }
                        } else {
                            let mono = (0.5 * (left_clamped + right_clamped)).clamp(-1.0, 1.0);
                            frame[0] = ((mono * 32767.0) + 32768.0).clamp(0.0, 65535.0) as u16;

                            if telem_idx < 8192 {
                                float_buf[telem_idx] = mono;
                                telem_idx += 1;
                            }
                        }
                    }

                    let sample = compute_telemetry_chunk(&float_buf[..telem_idx]);
                    peak_db.store(sample.peak_volume_db.to_bits(), Ordering::Relaxed);
                    rms_db.store(sample.rms_volume_db.to_bits(), Ordering::Relaxed);
                },
                err_fn,
                None,
            ).context("Failed to build U16 channel stream")
        }

        format => anyhow::bail!("Unsupported output sample format: {format}"),
    }
}

pub fn device_id_priority(id: &str) -> i32 {
    let lower = id.to_lowercase();
    if lower.starts_with("sysdefault:") {
        100
    } else if lower.starts_with("front:") {
        80
    } else if lower == "pipewire" {
        60
    } else if lower == "default" {
        50
    } else if lower.starts_with("hw:") {
        40
    } else {
        10
    }
}

static DEVICE_NAME_CACHE: std::sync::OnceLock<std::sync::RwLock<HashMap<String, String>>> =
    std::sync::OnceLock::new();

fn get_device_name_cache() -> &'static std::sync::RwLock<HashMap<String, String>> {
    DEVICE_NAME_CACHE.get_or_init(|| std::sync::RwLock::new(HashMap::new()))
}

pub fn register_pipewire_device_description(card_id_or_path: &str, description: &str) {
    if card_id_or_path.is_empty() || description.is_empty() {
        return;
    }
    if let Ok(mut cache) = get_device_name_cache().write() {
        cache.insert(card_id_or_path.to_lowercase(), description.to_string());
    }
}

pub fn resolve_hardware_card_name(card_raw: &str, is_input: bool) -> String {
    let raw_trimmed = card_raw.trim();
    let lower = raw_trimmed.to_lowercase();

    // 1. Check in-memory cache populated dynamically from PipeWire device announcements
    if let Ok(cache) = get_device_name_cache().read()
        && let Some(desc) = cache.get(&lower) {
            return desc.clone();
        }

    // 2. Query Linux /sys/class/sound/ and udevadm for dynamic hardware driver and model info
    #[cfg(target_os = "linux")]
    {
        if let Ok(entries) = std::fs::read_dir("/sys/class/sound") {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("card") && name[4..].chars().all(|c| c.is_ascii_digit()) {
                    let card_dir = entry.path();
                    let id_path = card_dir.join("id");
                    if let Ok(cid) = std::fs::read_to_string(&id_path) {
                        let cid_clean = cid.trim();
                        if cid_clean.eq_ignore_ascii_case(&lower) || name[4..].eq_ignore_ascii_case(&lower) {
                            if let Ok(output) = std::process::Command::new("udevadm")
                                .args(["info", "-q", "property", card_dir.to_str().unwrap_or("")])
                                .output()
                                && output.status.success() {
                                    let text = String::from_utf8_lossy(&output.stdout);
                                    let mut model = None;
                                    for line in text.lines() {
                                        if let Some((k, v)) = line.split_once('=') {
                                            match k {
                                                "ID_MODEL_FROM_DATABASE" => model = Some(v.to_string()),
                                                "ID_MODEL" if model.is_none() => model = Some(v.replace('_', " ")),
                                                "SOUND_DESCRIPTION" if model.is_none() => model = Some(v.to_string()),
                                                _ => {}
                                            }
                                        }
                                    }
                                    if let Some(m) = model {
                                        let final_name = m.replace('_', " ");
                                        if let Ok(mut cache) = get_device_name_cache().write() {
                                            cache.insert(lower.clone(), final_name.clone());
                                        }
                                        return final_name;
                                    }
                                }

                            // Fallback: check pcm info from ALSA
                            let pcm_file = if is_input {
                                card_dir.join("pcm0c/info")
                            } else {
                                card_dir.join("pcm0p/info")
                            };
                            if let Ok(info_text) = std::fs::read_to_string(&pcm_file) {
                                for line in info_text.lines() {
                                    if let Some(n) = line.strip_prefix("name:") {
                                        let pcm_name = n.trim().to_string();
                                        if !pcm_name.is_empty() {
                                            if let Ok(mut cache) = get_device_name_cache().write() {
                                                cache.insert(lower.clone(), pcm_name.clone());
                                            }
                                            return pcm_name;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    raw_trimmed.replace('_', " ")
}

pub fn format_device_display_name(name: &str) -> String {
    format_device_display_name_internal(name, false)
}

pub fn format_input_device_display_name(name: &str) -> String {
    format_device_display_name_internal(name, true)
}

fn format_device_display_name_internal(name: &str, is_input: bool) -> String {
    let lower = name.to_lowercase();
    if lower == "pipewire" {
        return if is_input {
            "PipeWire Default Input".to_string()
        } else {
            "PipeWire Default Output".to_string()
        };
    }
    if lower == "default" {
        return "System Default".to_string();
    }
    if lower == "pulse" {
        return "PulseAudio Server".to_string();
    }

    // ALSA CARD parameter extraction (e.g. "sysdefault:CARD=Audio", "front:CARD=DAC,DEV=0")
    if let Some(card_idx) = lower.find("card=") {
        let after_card = &name[card_idx + 5..];
        let card_raw = after_card.split(',').next().unwrap_or(after_card).trim();
        return resolve_hardware_card_name(card_raw, is_input);
    }

    // Strip raw ALSA prefixes if no CARD= parameter
    let cleaned = if let Some(s) = name.strip_prefix("sysdefault:") {
        s
    } else if let Some(s) = name.strip_prefix("front:") {
        s
    } else if let Some(s) = name.strip_prefix("hw:") {
        s
    } else {
        name
    };

    resolve_hardware_card_name(cleaned, is_input)
}

#[cfg(target_os = "linux")]
pub fn silence_alsa_logging() {
    unsafe extern "C" {
        fn snd_lib_error_set_handler(handler: *const ()) -> libc::c_int;
    }
    unsafe {
        snd_lib_error_set_handler(std::ptr::null());
    }
}

#[cfg(not(target_os = "linux"))]
pub fn silence_alsa_logging() {}

#[cfg(target_os = "linux")]
struct StderrSilencer {
    saved_stderr: i32,
    null_fd: i32,
}

#[cfg(target_os = "linux")]
impl StderrSilencer {
    fn new() -> Self {
        unsafe {
            let null_path = b"/dev/null\0";
            let null_fd = libc::open(null_path.as_ptr() as *const libc::c_char, libc::O_WRONLY);
            if null_fd < 0 {
                return Self { saved_stderr: -1, null_fd: -1 };
            }
            let saved_stderr = libc::dup(libc::STDERR_FILENO);
            if saved_stderr >= 0 {
                libc::dup2(null_fd, libc::STDERR_FILENO);
            }
            Self { saved_stderr, null_fd }
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for StderrSilencer {
    fn drop(&mut self) {
        unsafe {
            if self.saved_stderr >= 0 {
                libc::dup2(self.saved_stderr, libc::STDERR_FILENO);
                libc::close(self.saved_stderr);
            }
            if self.null_fd >= 0 {
                libc::close(self.null_fd);
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn get_audio_host() -> cpal::Host {
    let _silence = StderrSilencer::new();
    if let Ok(host) = cpal::host_from_id(cpal::HostId::Alsa) {
        return host;
    }
    cpal::default_host()
}

#[cfg(not(target_os = "linux"))]
fn get_audio_host() -> cpal::Host {
    cpal::default_host()
}

pub fn is_rejected_device_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("routune")
        || lower.contains("virtual")
        || lower.contains("dummy")
        || lower.contains("null")
        || lower.contains("loopback")
        || lower.contains("surround")
        || lower.contains("iec958")
        || lower.contains("spdif")
        || lower.contains("hdmi")
}

pub fn score_physical_output_device(name: &str) -> i32 {
    let lower = name.to_lowercase();
    if is_rejected_device_name(&lower) {
        return -1000;
    }

    let disp = format_device_display_name(name).to_lowercase();

    let mut score = 0;
    if lower.contains("usb") || disp.contains("usb") {
        score += 25;
    }
    if lower.contains("card=") {
        score += 15;
    }
    if lower.contains("pci") || lower.contains("analog") || lower.contains("hda") {
        score += 20;
    }
    if lower.contains("speaker") || lower.contains("headphone") || lower.contains("headset") || disp.contains("headset") || disp.contains("headphone") {
        score += 15;
    }
    if lower.contains("sysdefault:") {
        score += 10;
    }
    if lower.contains("front:") {
        score += 5;
    }

    if lower == "pipewire" || lower == "pulse" {
        score -= 20;
    }
    if lower == "default" {
        score -= 30;
    }

    score
}

pub fn score_physical_input_device(name: &str) -> i32 {
    let lower = name.to_lowercase();
    if is_rejected_device_name(&lower) {
        return -1000;
    }

    let disp = format_input_device_display_name(name).to_lowercase();

    let mut score = 0;
    if lower.contains("usb") || disp.contains("usb") {
        score += 50;
    }
    if lower.contains("mic") || disp.contains("mic") || lower.contains("input") || disp.contains("input") {
        score += 30;
    }
    if lower.contains("headset") || disp.contains("headset") {
        score += 25;
    }

    if lower == "pipewire" {
        score += 25;
    } else if lower == "pulse" {
        score += 24;
    } else if lower == "default" {
        score += 20;
    }

    if lower.contains("card=") {
        if lower.contains("usb") {
            score += 20;
        } else {
            // Motherboard PCI analog jacks often unplugged, rank below sound server bridges
            score -= 10;
        }
    }

    score
}

pub fn find_physical_output_device(host: &cpal::Host, keyword: &str) -> Option<cpal::Device> {
    #[cfg(target_os = "linux")]
    let _silence = StderrSilencer::new();

    let target_kw = keyword.to_lowercase();
    let devices = enumerate_host_devices(host);
    for dev in devices {
        if let Ok(name) = dev.name() {
            if is_rejected_device_name(&name) {
                continue;
            }
            let name_lower = name.to_lowercase();
            let display = format_device_display_name(&name).to_lowercase();
            let matches = name_lower.contains(&target_kw)
                || display.contains(&target_kw)
                || target_kw.contains(&display);

            if matches {
                return Some(dev);
            }
        }
    }
    None
}

pub fn choose_best_stream_config(
    device: &cpal::Device,
    target_rate: u32,
) -> Result<(StreamConfig, SampleFormat)> {
    let supported = match device.supported_output_configs() {
        Ok(iter) => iter.collect::<Vec<_>>(),
        Err(e) => {
            warn!("Failed to query supported output configs for device: {e}");
            Vec::new()
        }
    };

    let dev_name = device.name().unwrap_or_default();
    let is_hw_card = dev_name.contains("CARD=") || dev_name.starts_with("hw:") || dev_name.starts_with("front:");
    let default_cfg = device.default_output_config().ok();
    let default_fmt = default_cfg.as_ref().map(|d| d.sample_format());

    if !supported.is_empty() {
        let rate_matching: Vec<_> = supported
            .iter()
            .filter(|c| c.min_sample_rate().0 <= target_rate && target_rate <= c.max_sample_rate().0)
            .copied()
            .collect();

        let candidates = if !rate_matching.is_empty() {
            rate_matching
        } else {
            supported.clone()
        };

        let format_rank = |fmt: SampleFormat| {
            if Some(fmt) == default_fmt {
                0
            } else if is_hw_card {
                match fmt {
                    SampleFormat::I16 => 1,
                    SampleFormat::F32 => 2,
                    SampleFormat::I32 => 3,
                    SampleFormat::U16 => 4,
                    _ => 10,
                }
            } else {
                match fmt {
                    SampleFormat::F32 => 1,
                    SampleFormat::I16 => 2,
                    SampleFormat::I32 => 3,
                    SampleFormat::U16 => 4,
                    _ => 10,
                }
            }
        };

        let channel_rank = |ch: u16| {
            if ch == 2 {
                0
            } else if ch > 2 {
                1
            } else {
                2
            }
        };

        let mut sorted = candidates;
        sorted.sort_by_key(|c| (channel_rank(c.channels()), format_rank(c.sample_format())));

        if let Some(best) = sorted.first() {
            let sample_rate = if best.min_sample_rate().0 <= target_rate && target_rate <= best.max_sample_rate().0 {
                cpal::SampleRate(target_rate)
            } else if target_rate < best.min_sample_rate().0 {
                best.min_sample_rate()
            } else {
                best.max_sample_rate()
            };

            let stream_cfg = best.with_sample_rate(sample_rate).config();
            return Ok((stream_cfg, best.sample_format()));
        }
    }

    if let Some(def) = default_cfg {
        let fmt = def.sample_format();
        let stream_cfg = def.into();
        return Ok((stream_cfg, fmt));
    }

    let fallback_fmt = if is_hw_card {
        SampleFormat::I16
    } else {
        SampleFormat::F32
    };

    Ok((
        StreamConfig {
            channels: 2,
            sample_rate: cpal::SampleRate(target_rate),
            buffer_size: cpal::BufferSize::Default,
        },
        fallback_fmt,
    ))
}

pub fn choose_best_input_stream_config(
    device: &cpal::Device,
    target_rate: u32,
) -> Result<(StreamConfig, SampleFormat)> {
    let supported = match device.supported_input_configs() {
        Ok(iter) => iter.collect::<Vec<_>>(),
        Err(e) => {
            warn!("Failed to query supported input configs for device: {e}");
            Vec::new()
        }
    };

    let dev_name = device.name().unwrap_or_default();
    let is_hw_card = dev_name.contains("CARD=") || dev_name.starts_with("hw:") || dev_name.starts_with("front:");
    let default_cfg = device.default_input_config().ok();
    let default_fmt = default_cfg.as_ref().map(|d| d.sample_format());

    if !supported.is_empty() {
        let rate_matching: Vec<_> = supported
            .iter()
            .filter(|c| c.min_sample_rate().0 <= target_rate && target_rate <= c.max_sample_rate().0)
            .copied()
            .collect();

        let candidates = if !rate_matching.is_empty() {
            rate_matching
        } else {
            supported.clone()
        };

        let format_rank = |fmt: SampleFormat| {
            if Some(fmt) == default_fmt {
                0
            } else if is_hw_card {
                match fmt {
                    SampleFormat::I16 => 1,
                    SampleFormat::F32 => 2,
                    SampleFormat::I32 => 3,
                    SampleFormat::U16 => 4,
                    _ => 10,
                }
            } else {
                match fmt {
                    SampleFormat::F32 => 1,
                    SampleFormat::I16 => 2,
                    SampleFormat::I32 => 3,
                    SampleFormat::U16 => 4,
                    _ => 10,
                }
            }
        };

        let channel_rank = |ch: u16| {
            if ch == 2 {
                0
            } else if ch == 1 {
                1
            } else {
                2
            }
        };

        let mut sorted = candidates;
        sorted.sort_by_key(|c| (channel_rank(c.channels()), format_rank(c.sample_format())));

        if let Some(best) = sorted.first() {
            let sample_rate = if best.min_sample_rate().0 <= target_rate && target_rate <= best.max_sample_rate().0 {
                cpal::SampleRate(target_rate)
            } else if target_rate < best.min_sample_rate().0 {
                best.min_sample_rate()
            } else {
                best.max_sample_rate()
            };

            let stream_cfg = best.with_sample_rate(sample_rate).config();
            return Ok((stream_cfg, best.sample_format()));
        }
    }

    if let Some(def) = default_cfg {
        let fmt = def.sample_format();
        let stream_cfg = def.into();
        return Ok((stream_cfg, fmt));
    }

    let fallback_fmt = if is_hw_card {
        SampleFormat::I16
    } else {
        SampleFormat::F32
    };

    Ok((
        StreamConfig {
            channels: 1,
            sample_rate: cpal::SampleRate(target_rate),
            buffer_size: cpal::BufferSize::Default,
        },
        fallback_fmt,
    ))
}

#[allow(clippy::too_many_arguments)]
fn build_channel_input_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    ring_buffer: Arc<AudioRingBuffer>,
    cmd_rx: Receiver<GraphCommand>,
    peak_db: Arc<AtomicU32>,
    rms_db: Arc<AtomicU32>,
    is_active: Arc<AtomicBool>,
    noise_enabled: Arc<AtomicBool>,
    mic_muted: Arc<AtomicBool>,
    mic_gain: Arc<AtomicU32>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
    mic_test_enabled: Arc<AtomicBool>,
    mic_test_ring_buf: Arc<AudioRingBuffer>,
) -> Result<Stream> {
    let channels = config.channels as usize;
    let mut ai_noise_node = AiNoiseNode::new("mic-noise", "Mic AI Noise");
    let init_noise = noise_enabled.load(Ordering::Relaxed);
    ai_noise_node.set_enabled(init_noise);
    let mut noise_gate_node = NoiseGateNode::new("mic-gate", "Mic Noise Gate", 48000);
    let eq_node = EqNode::new("mic-eq", "Mic Equalizer", 48000);
    let mut compressor_node = CompressorNode::new("mic-comp", "Mic Compressor", 48000);
    let resampler = LinearResampler::new(config.sample_rate.0, VIRTUAL_SINK_SAMPLE_RATE);

    let in_sample_rate = config.sample_rate.0.max(1) as u64;
    let init_gain = f32::from_bits(mic_gain.load(Ordering::Relaxed));
    let init_muted = mic_muted.load(Ordering::Relaxed);

    match sample_format {
        SampleFormat::F32 => {
            let mut hw_stereo = Box::new([0.0f32; 8192]);
            let mut resampled_stereo = Box::new([0.0f32; 8192]);
            let mut dsp_scratch = Box::new([0.0f32; 8192]);
            let mut eq_node = eq_node;
            let mut resampler = resampler;
            let mut gain = init_gain;
            let mut muted = init_muted;
            let mic_gain_cl = mic_gain.clone();
            let mic_muted_cl = mic_muted.clone();

            device.build_input_stream(
                config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    while let Ok(cmd) = cmd_rx.try_recv() {
                        match cmd {
                            GraphCommand::UpdateEq(_, band, g) => {
                                eq_node.set_band_gain(band, g);
                            }
                            GraphCommand::ToggleAiNoise(_, en) => {
                                ai_noise_node.set_enabled(en);
                                noise_enabled.store(en, Ordering::Relaxed);
                            }
                            GraphCommand::SetAiNoiseLevel(_, lvl) => {
                                ai_noise_node.set_level(lvl);
                            }
                            GraphCommand::UpdateNoiseGate {
                                enabled,
                                threshold_db,
                                attack_ms,
                                release_ms,
                                ..
                            } => {
                                noise_gate_node.set_enabled(enabled);
                                noise_gate_node.set_threshold(threshold_db);
                                noise_gate_node.set_attack_ms(attack_ms);
                                noise_gate_node.set_release_ms(release_ms);
                            }
                            GraphCommand::UpdateCompressor {
                                enabled,
                                threshold_db,
                                ratio,
                                attack_ms,
                                release_ms,
                                makeup_gain_db,
                                ..
                            } => {
                                compressor_node.set_enabled(enabled);
                                compressor_node.set_threshold(threshold_db);
                                compressor_node.set_ratio(ratio);
                                compressor_node.set_attack_ms(attack_ms);
                                compressor_node.set_release_ms(release_ms);
                                compressor_node.set_makeup_gain_db(makeup_gain_db);
                            }
                            GraphCommand::UpdateVolume(_, vol_db) => {
                                gain = if vol_db <= -48.0 {
                                    0.0f32
                                } else {
                                    10.0f32.powf(vol_db / 20.0)
                                };
                                mic_gain_cl.store(gain.to_bits(), Ordering::Relaxed);
                            }
                            GraphCommand::UpdateMute(_, m) => {
                                muted = m;
                                mic_muted_cl.store(m, Ordering::Relaxed);
                            }
                            _ => {}
                        }
                    }

                    if channels == 0 || data.is_empty() {
                        return;
                    }
                    is_active.store(true, Ordering::Relaxed);

                    let num_hw_frames = if channels == 1 {
                        data.len()
                    } else {
                        data.len() / channels
                    };
                    let hw_stereo_len = (num_hw_frames * 2).min(8192) & !1;
                    if hw_stereo_len == 0 {
                        return;
                    }

                    if channels == 1 {
                        for (i, &sample) in data[..hw_stereo_len / 2].iter().enumerate() {
                            let s = sample.clamp(-1.0, 1.0);
                            hw_stereo[i * 2] = s;
                            hw_stereo[i * 2 + 1] = s;
                        }
                    } else {
                        for i in 0..(hw_stereo_len / 2) {
                            let base = i * channels;
                            hw_stereo[i * 2] = data[base].clamp(-1.0, 1.0);
                            hw_stereo[i * 2 + 1] = data[base + 1].clamp(-1.0, 1.0);
                        }
                    }

                    let target_frames = ((num_hw_frames as u64 * VIRTUAL_SINK_SAMPLE_RATE as u64) / in_sample_rate + 4) as usize;
                    let target_samples = (target_frames * 2).min(8192) & !1;
                    let resampled_len = resampler.resample(&hw_stereo[..hw_stereo_len], &mut resampled_stereo[..target_samples]);
                    if resampled_len == 0 {
                        return;
                    }

                    // DSP chain: AI Noise -> Noise Gate -> EQ -> Compressor
                    ai_noise_node.process(&resampled_stereo[..resampled_len], &mut dsp_scratch[..resampled_len]);
                    noise_gate_node.process(&dsp_scratch[..resampled_len], &mut resampled_stereo[..resampled_len]);
                    eq_node.process(&resampled_stereo[..resampled_len], &mut dsp_scratch[..resampled_len]);
                    compressor_node.process(&dsp_scratch[..resampled_len], &mut resampled_stereo[..resampled_len]);

                    let effective_gain = if muted { 0.0f32 } else { gain };
                    if effective_gain == 0.0 {
                        resampled_stereo[..resampled_len].fill(0.0);
                    } else if (effective_gain - 1.0).abs() > 0.0001 {
                        for s in &mut resampled_stereo[..resampled_len] {
                            *s = (*s * effective_gain).clamp(-1.0, 1.0);
                        }
                    }

                    let processed = &resampled_stereo[..resampled_len];
                    let (peak, rms) = calculate_metrics(processed);
                    peak_db.store(peak.to_bits(), Ordering::Relaxed);
                    rms_db.store(rms.to_bits(), Ordering::Relaxed);

                    ring_buffer.push_slice(processed);

                    if mic_test_enabled.load(Ordering::Relaxed) {
                        mic_test_ring_buf.push_slice(processed);
                    }
                },
                err_fn,
                None,
            ).context("Failed to build F32 mic input stream")
        }
        SampleFormat::I16 => {
            let mut hw_stereo = Box::new([0.0f32; 8192]);
            let mut resampled_stereo = Box::new([0.0f32; 8192]);
            let mut dsp_scratch = Box::new([0.0f32; 8192]);
            let mut eq_node = eq_node;
            let mut resampler = resampler;
            let mut gain = init_gain;
            let mut muted = init_muted;
            let mic_gain_cl = mic_gain.clone();
            let mic_muted_cl = mic_muted.clone();

            device.build_input_stream(
                config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    while let Ok(cmd) = cmd_rx.try_recv() {
                        match cmd {
                            GraphCommand::UpdateEq(_, band, g) => {
                                eq_node.set_band_gain(band, g);
                            }
                            GraphCommand::ToggleAiNoise(_, en) => {
                                ai_noise_node.set_enabled(en);
                                noise_enabled.store(en, Ordering::Relaxed);
                            }
                            GraphCommand::SetAiNoiseLevel(_, lvl) => {
                                ai_noise_node.set_level(lvl);
                            }
                            GraphCommand::UpdateNoiseGate {
                                enabled,
                                threshold_db,
                                attack_ms,
                                release_ms,
                                ..
                            } => {
                                noise_gate_node.set_enabled(enabled);
                                noise_gate_node.set_threshold(threshold_db);
                                noise_gate_node.set_attack_ms(attack_ms);
                                noise_gate_node.set_release_ms(release_ms);
                            }
                            GraphCommand::UpdateCompressor {
                                enabled,
                                threshold_db,
                                ratio,
                                attack_ms,
                                release_ms,
                                makeup_gain_db,
                                ..
                            } => {
                                compressor_node.set_enabled(enabled);
                                compressor_node.set_threshold(threshold_db);
                                compressor_node.set_ratio(ratio);
                                compressor_node.set_attack_ms(attack_ms);
                                compressor_node.set_release_ms(release_ms);
                                compressor_node.set_makeup_gain_db(makeup_gain_db);
                            }
                            GraphCommand::UpdateVolume(_, vol_db) => {
                                gain = if vol_db <= -48.0 {
                                    0.0f32
                                } else {
                                    10.0f32.powf(vol_db / 20.0)
                                };
                                mic_gain_cl.store(gain.to_bits(), Ordering::Relaxed);
                            }
                            GraphCommand::UpdateMute(_, m) => {
                                muted = m;
                                mic_muted_cl.store(m, Ordering::Relaxed);
                            }
                            _ => {}
                        }
                    }

                    if channels == 0 || data.is_empty() {
                        return;
                    }
                    is_active.store(true, Ordering::Relaxed);

                    let num_hw_frames = if channels == 1 {
                        data.len()
                    } else {
                        data.len() / channels
                    };
                    let hw_stereo_len = (num_hw_frames * 2).min(8192) & !1;
                    if hw_stereo_len == 0 {
                        return;
                    }

                    if channels == 1 {
                        for (i, &sample) in data[..hw_stereo_len / 2].iter().enumerate() {
                            let s = (sample as f32 / 32768.0).clamp(-1.0, 1.0);
                            hw_stereo[i * 2] = s;
                            hw_stereo[i * 2 + 1] = s;
                        }
                    } else {
                        for i in 0..(hw_stereo_len / 2) {
                            let base = i * channels;
                            hw_stereo[i * 2] = (data[base] as f32 / 32768.0).clamp(-1.0, 1.0);
                            hw_stereo[i * 2 + 1] = (data[base + 1] as f32 / 32768.0).clamp(-1.0, 1.0);
                        }
                    }

                    let target_frames = ((num_hw_frames as u64 * VIRTUAL_SINK_SAMPLE_RATE as u64) / in_sample_rate + 4) as usize;
                    let target_samples = (target_frames * 2).min(8192) & !1;
                    let resampled_len = resampler.resample(&hw_stereo[..hw_stereo_len], &mut resampled_stereo[..target_samples]);
                    if resampled_len == 0 {
                        return;
                    }

                    // DSP chain: AI Noise -> Noise Gate -> EQ -> Compressor
                    ai_noise_node.process(&resampled_stereo[..resampled_len], &mut dsp_scratch[..resampled_len]);
                    noise_gate_node.process(&dsp_scratch[..resampled_len], &mut resampled_stereo[..resampled_len]);
                    eq_node.process(&resampled_stereo[..resampled_len], &mut dsp_scratch[..resampled_len]);
                    compressor_node.process(&dsp_scratch[..resampled_len], &mut resampled_stereo[..resampled_len]);

                    let effective_gain = if muted { 0.0f32 } else { gain };
                    if effective_gain == 0.0 {
                        resampled_stereo[..resampled_len].fill(0.0);
                    } else if (effective_gain - 1.0).abs() > 0.0001 {
                        for s in &mut resampled_stereo[..resampled_len] {
                            *s = (*s * effective_gain).clamp(-1.0, 1.0);
                        }
                    }

                    let processed = &resampled_stereo[..resampled_len];
                    let (peak, rms) = calculate_metrics(processed);
                    peak_db.store(peak.to_bits(), Ordering::Relaxed);
                    rms_db.store(rms.to_bits(), Ordering::Relaxed);

                    ring_buffer.push_slice(processed);

                    if mic_test_enabled.load(Ordering::Relaxed) {
                        mic_test_ring_buf.push_slice(processed);
                    }
                },
                err_fn,
                None,
            ).context("Failed to build I16 mic input stream")
        }
        _ => Err(anyhow::anyhow!("Unsupported sample format for mic input stream: {:?}", sample_format)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_devices_enumeration_and_routing() {
        let (mut router, _) = ChannelRouter::new();
        let inputs = router.get_hardware_inputs();
        if inputs.is_empty() {
            // Headless CI environment with no physical soundcard
            return;
        }

        // Test routing the first detected hardware input
        let first_id = &inputs[0].id;
        let res = router.set_channel_routing(5, first_id);
        assert!(res.is_ok(), "Setting mic routing to first input should succeed: {:?}", res);
    }

    #[test]
    fn test_device_rejection_virtual_and_routune() {
        assert!(is_rejected_device_name("routune-chat"));
        assert!(is_rejected_device_name("Virtual Audio Device"));
        assert!(is_rejected_device_name("virtual-sink"));
        assert!(is_rejected_device_name("Null Output"));
        assert!(is_rejected_device_name("Dummy Output"));
        assert!(is_rejected_device_name("ALSA Loopback Device"));
        assert!(is_rejected_device_name("surround40:CARD=MockCard,DEV=0"));
        assert!(is_rejected_device_name("surround51:CARD=MockCard,DEV=0"));
        assert!(is_rejected_device_name("iec958:CARD=MockCard,DEV=0"));
        assert!(is_rejected_device_name("hdmi:CARD=MockGPU,DEV=0"));
        assert!(is_rejected_device_name("spdif:CARD=MockCard,DEV=0"));

        assert!(!is_rejected_device_name("sysdefault:CARD=MockCard"));
        assert!(!is_rejected_device_name("front:CARD=MockCard,DEV=0"));
        assert!(!is_rejected_device_name("USB Audio DAC"));
        assert!(!is_rejected_device_name("Speakers (High Definition Audio)"));
        assert!(!is_rejected_device_name("Headphones (Analog Output)"));
    }

    #[test]
    fn test_format_display_name_beautification() {
        register_pipewire_device_description("MockUSB", "Studio USB DAC");
        register_pipewire_device_description("MockPCIe", "HD Audio Controller");

        assert_eq!(format_device_display_name("sysdefault:CARD=MockUSB"), "Studio USB DAC");
        assert_eq!(format_device_display_name("front:CARD=MockUSB,DEV=0"), "Studio USB DAC");
        assert_eq!(format_device_display_name("sysdefault:CARD=MockPCIe"), "HD Audio Controller");
        assert_eq!(format_device_display_name("front:CARD=MockPCIe,DEV=0"), "HD Audio Controller");
        assert_eq!(format_device_display_name("pipewire"), "PipeWire Default Output");
        assert_eq!(format_device_display_name("default"), "System Default");

        assert_eq!(format_input_device_display_name("sysdefault:CARD=MockPCIe"), "HD Audio Controller");
        assert_eq!(format_input_device_display_name("front:CARD=MockPCIe,DEV=0"), "HD Audio Controller");
        assert_eq!(format_input_device_display_name("pipewire"), "PipeWire Default Input");

        // Fallback for un-cached raw card name: replaces underscores with spaces
        assert_eq!(format_device_display_name("sysdefault:CARD=Uncached_Card"), "Uncached Card");
    }

    #[test]
    fn test_extract_card_name() {
        assert_eq!(extract_card_name("sysdefault:CARD=MockCard"), Some("MockCard".to_string()));
        assert_eq!(extract_card_name("front:CARD=Other_Card,DEV=0"), Some("Other_Card".to_string()));
        assert_eq!(extract_card_name("hw:CARD=Audio,DEV=1"), Some("Audio".to_string()));
        assert_eq!(extract_card_name("pipewire"), None);
        assert_eq!(extract_card_name("default"), None);
    }

    #[test]
    fn test_hardware_outputs_deduplication_and_filtering() {
        let (router, _) = ChannelRouter::new();
        let outputs = router.get_hardware_outputs();

        // Must not contain any blacklisted devices
        for out in &outputs {
            assert!(!is_rejected_device_name(&out.id));
            assert!(!is_rejected_device_name(&out.name));
        }

        // Verify uniqueness of display_names
        let mut seen = std::collections::HashSet::new();
        for out in &outputs {
            assert!(seen.insert(out.display_name.clone()), "Duplicate display name found: {}", out.display_name);
        }
    }

    #[test]
    fn test_device_resolution_and_fallback_matching() {
        silence_alsa_logging();
        let (mut router, _pusher) = ChannelRouter::new();

        // 1. Unassign channel
        let unassign_res = router.set_channel_routing(1, "none").unwrap();
        assert_eq!(unassign_res, "Unassigned");
        assert!(router.routes.get(&1).unwrap().target_device.is_none());

        // 2. Dynamic matching against whatever hardware exists on host
        let devices = router.get_hardware_outputs();
        if let Some(first) = devices.first() {
            let found = router.find_device(&first.id);
            assert!(found.is_some());
        }
    }

    #[test]
    fn test_physical_device_scoring_prefers_hardware() {
        let routune_score = score_physical_output_device("Routune Game");
        let virtual_score = score_physical_output_device("Virtual Cable");
        assert_eq!(routune_score, -1000);
        assert_eq!(virtual_score, -1000);

        let default_bridge_score = score_physical_output_device("default");
        let pipewire_bridge_score = score_physical_output_device("pipewire");
        let usb_score = score_physical_output_device("sysdefault:CARD=ExternalDAC (USB Audio)");
        let pci_score = score_physical_output_device("front:CARD=BuiltinAudio,DEV=0 (HD Audio)");

        assert!(usb_score > default_bridge_score);
        assert!(usb_score > pipewire_bridge_score);
        assert!(pci_score > default_bridge_score);
        assert!(pci_score > pipewire_bridge_score);
    }

    #[test]
    fn test_input_device_scoring_prefers_usb_and_bridges() {
        let routune_score = score_physical_input_device("Routune Mic");
        assert_eq!(routune_score, -1000);

        let pipewire_score = score_physical_input_device("pipewire");
        let pulse_score = score_physical_input_device("pulse");
        let default_score = score_physical_input_device("default");
        let usb_mic_score = score_physical_input_device("sysdefault:CARD=Device (USB Audio)");
        let onboard_score = score_physical_input_device("sysdefault:CARD=Generic_1");

        assert!(usb_mic_score > pipewire_score);
        assert!(pipewire_score > onboard_score);
        assert!(pulse_score > onboard_score);
        assert!(default_score > onboard_score);
    }

    #[test]
    fn test_audio_ring_buffer_basic_push_pop() {
        let rb = AudioRingBuffer::new();
        let input = [0.1f32, 0.2, 0.3, 0.4];
        rb.push_slice(&input);

        let mut output = [0.0f32; 4];
        let read = rb.pop_slice(&mut output);
        assert_eq!(read, 4);
        assert_eq!(output, input);
    }

    #[test]
    fn test_audio_ring_buffer_pop_stereo_interlock() {
        let rb = AudioRingBuffer::new();
        assert_eq!(rb.pop(), None);

        // Push 2 stereo frames: [L0, R0, L1, R1]
        let frames = [0.1f32, -0.1, 0.2, -0.2];
        rb.push_slice(&frames);

        let f0 = rb.pop().expect("frame 0 must exist");
        assert_eq!(f0, [0.1, -0.1]);

        let f1 = rb.pop().expect("frame 1 must exist");
        assert_eq!(f1, [0.2, -0.2]);

        // Ring buffer exhausted -> None
        assert_eq!(rb.pop(), None);
    }

    #[test]
    fn test_audio_ring_buffer_underrun_zero_padding() {
        let rb = AudioRingBuffer::new();
        let input = [0.5f32, -0.5];
        rb.push_slice(&input);

        let mut output = [1.0f32; 6];
        let read = rb.pop_slice(&mut output);
        assert_eq!(read, 2);
        assert_eq!(output[0], 0.5);
        assert_eq!(output[1], -0.5);
        // Remainder must be padded with 0.0 silence
        assert_eq!(&output[2..], &[0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn test_audio_ring_buffer_stereo_alignment_truncation() {
        let rb = AudioRingBuffer::new();
        // Odd length input (3 samples): must truncate to 2 to preserve stereo alignment
        let odd_input = [0.1f32, 0.2, 0.3];
        rb.push_slice(&odd_input);

        let mut output = [0.0f32; 4];
        let read = rb.pop_slice(&mut output);
        assert_eq!(read, 2);
        assert_eq!(output[0], 0.1);
        assert_eq!(output[1], 0.2);
        assert_eq!(output[2], 0.0);
        assert_eq!(output[3], 0.0);
    }

    #[test]
    fn test_audio_ring_buffer_overrun_and_backlog_purge() {
        let rb = AudioRingBuffer::new();
        // Push more than MAX_AUDIO_BACKLOG (4096)
        let large_input: Vec<f32> = (0..5000).map(|i| i as f32).collect();
        rb.push_slice(&large_input);

        let mut output = vec![0.0f32; 2048];
        let read = rb.pop_slice(&mut output);
        // Because backlog exceeded 4096, pop_slice purges excess down to DRAIN_AUDIO_TARGET (1024)
        assert_eq!(read, 1024);
        // Read length is 1024, remainder of 2048 output is 0.0
        assert_eq!(&output[1024..], &vec![0.0f32; 1024][..]);
    }

    #[test]
    fn test_audio_ring_buffer_extreme_overrun_wrap() {
        let rb = AudioRingBuffer::new();
        // Push 40,000 samples (exceeding AUDIO_RING_CAPACITY of 32,768)
        let chunk: Vec<f32> = (0..1000).map(|i| i as f32 / 1000.0).collect();
        for _ in 0..40 {
            rb.push_slice(&chunk);
        }

        let mut output = vec![0.0f32; 512];
        let read = rb.pop_slice(&mut output);
        assert!(read > 0);
        // Should not panic, should maintain valid audio values
        for &s in &output[..read] {
            assert!(s.is_finite());
            assert!(s >= -1.0 && s <= 1.0);
        }
    }

    #[test]
    fn test_linear_resampler_exact_rate_passthrough() {
        let mut resampler = LinearResampler::new(48000, 48000);
        let input = [0.1f32, 0.2, 0.3, 0.4, 0.5, 0.6];
        let mut output = [0.0f32; 6];
        let written = resampler.resample(&input, &mut output);
        assert_eq!(written, 6);
        assert_eq!(output, input);
    }

    #[test]
    fn test_linear_resampler_upsample_44100_to_48000() {
        let mut resampler = LinearResampler::new(44100, 48000);
        // 441 frames = 882 samples
        let input: Vec<f32> = (0..882).map(|i| (i as f32 * 0.01).sin()).collect();
        let mut output = vec![0.0f32; 1024];
        let written = resampler.resample(&input, &mut output);
        // 441 frames at 44100Hz = 10ms. At 48000Hz, 10ms = 480 frames = 960 samples.
        assert!(written >= 958 && written <= 962, "Written samples: {}", written);
        assert_eq!(written % 2, 0, "Resampled output must remain stereo aligned");
        for &s in &output[..written] {
            assert!(s.is_finite());
            assert!(s >= -1.0 && s <= 1.0);
        }
    }

    #[test]
    fn test_linear_resampler_phase_continuity_across_chunks() {
        let mut resampler = LinearResampler::new(44100, 48000);
        // Constant DC signal of 0.75 across two chunks
        let chunk1 = [0.75f32; 100];
        let chunk2 = [0.75f32; 100];

        let mut out1 = [0.0f32; 128];
        let mut out2 = [0.0f32; 128];

        let written1 = resampler.resample(&chunk1, &mut out1);
        let written2 = resampler.resample(&chunk2, &mut out2);

        assert!(written1 > 0);
        assert!(written2 > 0);

        for &s in &out1[..written1] {
            assert!((s - 0.75).abs() < 1e-4, "Sample was {}", s);
        }
        for &s in &out2[..written2] {
            assert!((s - 0.75).abs() < 1e-4, "Sample was {}", s);
        }
    }

    #[test]
    fn test_channel_pusher_initial_mute_and_gain_state() {
        let pusher = ChannelPusher::default();
        assert!(!pusher.mic_muted.load(Ordering::Relaxed));
        assert!((f32::from_bits(pusher.mic_gain.load(Ordering::Relaxed)) - 1.0f32).abs() < 1e-4);

        pusher.mic_muted.store(true, Ordering::Relaxed);
        pusher.mic_gain.store(0.0f32.to_bits(), Ordering::Relaxed);
        assert!(pusher.mic_muted.load(Ordering::Relaxed));
        assert_eq!(f32::from_bits(pusher.mic_gain.load(Ordering::Relaxed)), 0.0f32);
    }
}
