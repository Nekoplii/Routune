use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use crossbeam_channel::{Receiver, Sender};
use rtrb::{Consumer, Producer, RingBuffer};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use tracing::{info, warn};

use crate::server::proto;

pub const CHANNEL_RING_CAPACITY: usize = 16384;
pub const VIRTUAL_SINK_SAMPLE_RATE: u32 = 48_000;

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
    pub game: Arc<std::sync::Mutex<Option<Producer<f32>>>>,
    pub chat: Arc<std::sync::Mutex<Option<Producer<f32>>>>,
    pub media: Arc<std::sync::Mutex<Option<Producer<f32>>>>,
    pub aux: Arc<std::sync::Mutex<Option<Producer<f32>>>>,
}

impl Default for ChannelPusher {
    fn default() -> Self {
        Self {
            game: Arc::new(std::sync::Mutex::new(None)),
            chat: Arc::new(std::sync::Mutex::new(None)),
            media: Arc::new(std::sync::Mutex::new(None)),
            aux: Arc::new(std::sync::Mutex::new(None)),
        }
    }
}

impl ChannelPusher {
    pub fn get_slot(&self, channel: i32) -> Option<&Arc<std::sync::Mutex<Option<Producer<f32>>>>> {
        match channel {
            1 => Some(&self.game),
            2 => Some(&self.chat),
            3 => Some(&self.media),
            4 => Some(&self.aux),
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
    pub peak_db: Arc<AtomicU32>,
    pub rms_db: Arc<AtomicU32>,
    pub is_active: Arc<AtomicBool>,
}

pub struct ChannelRouter {
    host: cpal::Host,
    pusher: ChannelPusher,
    routes: HashMap<i32, ChannelRouteState>,
    death_tx: Sender<StreamDeathSignal>,
    death_rx: Receiver<StreamDeathSignal>,
}

unsafe impl Send for ChannelRouter {}

pub fn enumerate_host_devices(host: &cpal::Host) -> Vec<cpal::Device> {
    if let Ok(devs) = host.devices() {
        return devs.collect();
    }
    if let Ok(devs) = host.output_devices() {
        return devs.collect();
    }
    Vec::new()
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
        ];

        for (ch, name) in channels {
            routes.insert(
                ch,
                ChannelRouteState {
                    channel: ch,
                    channel_name: name.to_string(),
                    target_device: None,
                    output_stream: None,
                    peak_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
                    rms_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
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
            },
            pusher,
        )
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

    pub fn set_channel_routing(&mut self, channel: i32, target_device_name: &str) -> Result<String> {
        let trimmed = target_device_name.trim();

        // 1. Handle unassigned routing
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") || trimmed.eq_ignore_ascii_case("unassigned") {
            let route = self.routes.get_mut(&channel)
                .ok_or_else(|| anyhow::anyhow!("Invalid audio channel: {channel}"))?;
            drop(route.output_stream.take());
            route.is_active.store(false, Ordering::Relaxed);
            route.target_device = None;

            if let Some(slot) = self.pusher.get_slot(channel) {
                if let Ok(mut guard) = slot.lock() {
                    *guard = None;
                }
            }

            info!("Unassigned output routing for channel {}", route.channel_name);
            return Ok("Unassigned".to_string());
        }

        // 2. Find target physical device before mutable borrow
        let target_device = self.find_device(trimmed)
            .ok_or_else(|| anyhow::anyhow!("Device not found: {trimmed}"))?;
        let actual_name = target_device.name().unwrap_or_else(|_| trimmed.to_string());

        // 3. Negotiate optimal 48kHz stream configuration
        let (config, sample_format) = choose_best_stream_config(&target_device, VIRTUAL_SINK_SAMPLE_RATE)?;

        let route = self.routes.get_mut(&channel)
            .ok_or_else(|| anyhow::anyhow!("Invalid audio channel: {channel}"))?;

        // 4. Drop existing stream to release hardware handle
        drop(route.output_stream.take());
        route.is_active.store(false, Ordering::Relaxed);

        info!(
            "Binding channel {} -> '{}' (format={:?}, rate={}, channels={})",
            route.channel_name, actual_name, sample_format, config.sample_rate.0, config.channels
        );

        // 5. Create fresh dedicated RingBuffer bridge
        let (prod, cons) = RingBuffer::<f32>::new(CHANNEL_RING_CAPACITY);
        if let Some(slot) = self.pusher.get_slot(channel) {
            if let Ok(mut guard) = slot.lock() {
                *guard = Some(prod);
            }
        }

        // 6. Build and launch channel output stream
        let death_tx = self.death_tx.clone();
        let is_active = route.is_active.clone();
        let err_name = actual_name.clone();
        let err_fn = move |err: cpal::StreamError| {
            is_active.store(false, Ordering::Relaxed);
            warn!("Channel stream error ({err_name}): {err:?}");
            let _ = death_tx.try_send(StreamDeathSignal::DeviceNotAvailable);
        };

        let stream = build_channel_output_stream(
            &target_device,
            &config,
            sample_format,
            cons,
            route.peak_db.clone(),
            route.rms_db.clone(),
            route.is_active.clone(),
            err_fn,
        )?;

        stream.play().context("Failed to start channel physical stream")?;
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
            if let Ok(name) = dev.name() {
                if name == q_trimmed || name.to_lowercase() == q_lower {
                    return Some(dev.clone());
                }
            }
        }

        // 2. Fallback: match hardware device by .contains() using clean display_name
        for dev in &devices {
            if let Ok(name) = dev.name() {
                if is_rejected_device_name(&name) {
                    continue;
                }
                let disp = format_device_display_name(&name).to_lowercase();
                if disp.contains(&q_lower) || q_lower.contains(&disp) {
                    return Some(dev.clone());
                }
            }
        }

        // 3. Fallback: match by raw name substring
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

        // 4. Physical device heuristic matching (ryzen, usb, etc.)
        find_physical_output_device(&self.host, query)
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

        for ch in [1, 2, 3, 4] {
            if let Some(r) = self.routes.get(&ch) {
                let p = Self::read_decay_metric(&r.peak_db);
                let r_val = f32::from_bits(r.rms_db.load(Ordering::Relaxed));
                list.push(proto::ChannelVolumeMetric {
                    channel: ch,
                    channel_name: r.channel_name.clone(),
                    peak_volume_db: p,
                    rms_volume_db: r_val,
                    ai_noise_enabled: false,
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

    pub fn try_recv_death_signal(&self) -> Option<StreamDeathSignal> {
        self.death_rx.try_recv().ok()
    }
}

fn build_channel_output_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    mut consumer: Consumer<f32>,
    peak_db: Arc<AtomicU32>,
    rms_db: Arc<AtomicU32>,
    is_active: Arc<AtomicBool>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<Stream> {
    let channels = config.channels as usize;

    match sample_format {
        SampleFormat::F32 => device.build_output_stream(
            config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                is_active.store(true, Ordering::Relaxed);
                data.fill(0.0);
                if channels == 0 { return; }

                for frame in data.chunks_exact_mut(channels) {
                    let mut left = 0.0f32;
                    let mut right = 0.0f32;

                    if let Ok(l) = consumer.pop() {
                        if !l.is_nan() && !l.is_infinite() { left = l; }
                        if let Ok(r) = consumer.pop() {
                            if !r.is_nan() && !r.is_infinite() { right = r; }
                        }
                    }

                    let left_clamped = left.clamp(-1.0, 1.0);
                    let right_clamped = right.clamp(-1.0, 1.0);

                    if channels >= 2 {
                        frame[0] = left_clamped;
                        frame[1] = right_clamped;
                        for c in 2..channels { frame[c] = 0.0; }
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
        ).context("Failed to build F32 channel stream"),

        SampleFormat::I16 => {
            let mut float_buf = Vec::<f32>::with_capacity(2048);
            device.build_output_stream(
                config,
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    is_active.store(true, Ordering::Relaxed);
                    data.fill(0);
                    float_buf.clear();
                    if channels == 0 { return; }

                    for frame in data.chunks_exact_mut(channels) {
                        let mut left = 0.0f32;
                        let mut right = 0.0f32;

                        if let Ok(l) = consumer.pop() {
                            if !l.is_nan() && !l.is_infinite() { left = l; }
                            if let Ok(r) = consumer.pop() {
                                if !r.is_nan() && !r.is_infinite() { right = r; }
                            }
                        }

                        let left_clamped = left.clamp(-1.0, 1.0);
                        let right_clamped = right.clamp(-1.0, 1.0);

                        if channels >= 2 {
                            frame[0] = (left_clamped * 32767.0).clamp(-32768.0, 32767.0) as i16;
                            frame[1] = (right_clamped * 32767.0).clamp(-32768.0, 32767.0) as i16;
                            for c in 2..channels { frame[c] = 0; }
                            float_buf.push(left_clamped);
                            float_buf.push(right_clamped);
                        } else {
                            let mono = (0.5 * (left_clamped + right_clamped)).clamp(-1.0, 1.0);
                            frame[0] = (mono * 32767.0).clamp(-32768.0, 32767.0) as i16;
                            float_buf.push(mono);
                        }
                    }

                    let sample = compute_telemetry_chunk(&float_buf);
                    peak_db.store(sample.peak_volume_db.to_bits(), Ordering::Relaxed);
                    rms_db.store(sample.rms_volume_db.to_bits(), Ordering::Relaxed);
                },
                err_fn,
                None,
            ).context("Failed to build I16 channel stream")
        }

        SampleFormat::I32 => {
            let mut float_buf = Vec::<f32>::with_capacity(2048);
            device.build_output_stream(
                config,
                move |data: &mut [i32], _: &cpal::OutputCallbackInfo| {
                    is_active.store(true, Ordering::Relaxed);
                    data.fill(0);
                    float_buf.clear();
                    if channels == 0 { return; }

                    for frame in data.chunks_exact_mut(channels) {
                        let mut left = 0.0f32;
                        let mut right = 0.0f32;

                        if let Ok(l) = consumer.pop() {
                            if !l.is_nan() && !l.is_infinite() { left = l; }
                            if let Ok(r) = consumer.pop() {
                                if !r.is_nan() && !r.is_infinite() { right = r; }
                            }
                        }

                        let left_clamped = left.clamp(-1.0, 1.0);
                        let right_clamped = right.clamp(-1.0, 1.0);

                        if channels >= 2 {
                            frame[0] = (left_clamped * 2147483647.0).clamp(-2147483648.0, 2147483647.0) as i32;
                            frame[1] = (right_clamped * 2147483647.0).clamp(-2147483648.0, 2147483647.0) as i32;
                            for c in 2..channels { frame[c] = 0; }
                            float_buf.push(left_clamped);
                            float_buf.push(right_clamped);
                        } else {
                            let mono = (0.5 * (left_clamped + right_clamped)).clamp(-1.0, 1.0);
                            frame[0] = (mono * 2147483647.0).clamp(-2147483648.0, 2147483647.0) as i32;
                            float_buf.push(mono);
                        }
                    }

                    let sample = compute_telemetry_chunk(&float_buf);
                    peak_db.store(sample.peak_volume_db.to_bits(), Ordering::Relaxed);
                    rms_db.store(sample.rms_volume_db.to_bits(), Ordering::Relaxed);
                },
                err_fn,
                None,
            ).context("Failed to build I32 channel stream")
        }

        SampleFormat::U16 => {
            let mut float_buf = Vec::<f32>::with_capacity(2048);
            device.build_output_stream(
                config,
                move |data: &mut [u16], _: &cpal::OutputCallbackInfo| {
                    is_active.store(true, Ordering::Relaxed);
                    data.fill(32768);
                    float_buf.clear();
                    if channels == 0 { return; }

                    for frame in data.chunks_exact_mut(channels) {
                        let mut left = 0.0f32;
                        let mut right = 0.0f32;

                        if let Ok(l) = consumer.pop() {
                            if !l.is_nan() && !l.is_infinite() { left = l; }
                            if let Ok(r) = consumer.pop() {
                                if !r.is_nan() && !r.is_infinite() { right = r; }
                            }
                        }

                        let left_clamped = left.clamp(-1.0, 1.0);
                        let right_clamped = right.clamp(-1.0, 1.0);

                        if channels >= 2 {
                            frame[0] = ((left_clamped * 32767.0) + 32768.0).clamp(0.0, 65535.0) as u16;
                            frame[1] = ((right_clamped * 32767.0) + 32768.0).clamp(0.0, 65535.0) as u16;
                            for c in 2..channels { frame[c] = 32768; }
                            float_buf.push(left_clamped);
                            float_buf.push(right_clamped);
                        } else {
                            let mono = (0.5 * (left_clamped + right_clamped)).clamp(-1.0, 1.0);
                            frame[0] = ((mono * 32767.0) + 32768.0).clamp(0.0, 65535.0) as u16;
                            float_buf.push(mono);
                        }
                    }

                    let sample = compute_telemetry_chunk(&float_buf);
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

pub fn format_device_display_name(name: &str) -> String {
    let lower = name.to_lowercase();
    if lower == "pipewire" {
        return "PipeWire Default Output".to_string();
    }
    if lower == "default" {
        return "System Default".to_string();
    }
    if lower == "pulse" {
        return "PulseAudio Server".to_string();
    }

    // ALSA CARD parameter extraction (e.g. "sysdefault:CARD=Device", "front:CARD=Generic_1,DEV=0")
    if let Some(card_idx) = lower.find("card=") {
        let after_card = &name[card_idx + 5..];
        let card_raw = after_card.split(',').next().unwrap_or(after_card).trim();
        let card_lower = card_raw.to_lowercase();

        if card_lower == "device" || card_lower.contains("usb") {
            return "USB Audio Device".to_string();
        }
        if card_lower == "generic_1"
            || card_lower.contains("alc")
            || card_lower.contains("analog")
            || card_lower.contains("ryzen")
        {
            return "Ryzen HD Audio (Headphones)".to_string();
        }
        if card_lower == "generic" {
            return "HD-Audio Generic".to_string();
        }
        if card_lower == "nvidia" {
            return "NVIDIA High Definition Audio".to_string();
        }

        return card_raw.replace('_', " ");
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

    let cleaned_lower = cleaned.to_lowercase();
    if cleaned_lower.contains("usb") || cleaned_lower.contains("device") {
        "USB Audio Device".to_string()
    } else if cleaned_lower.contains("generic_1")
        || cleaned_lower.contains("ryzen")
        || cleaned_lower.contains("analog")
    {
        "Ryzen HD Audio (Headphones)".to_string()
    } else {
        cleaned.replace('_', " ")
    }
}

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

    let mut score = 0;
    if lower.contains("usb") || lower.contains("device") {
        score += 25;
    }
    if lower.contains("card=") {
        score += 15;
    }
    if lower.contains("pci") {
        score += 10;
    }
    if lower.contains("analog") || lower.contains("generic_1") || lower.contains("ryzen") {
        score += 20;
    }
    if lower.contains("speaker") || lower.contains("headphone") || lower.contains("headset") {
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
                || target_kw.contains(&display)
                || (target_kw.contains("ryzen")
                    && (name_lower.contains("generic_1")
                        || name_lower.contains("generic")
                        || name_lower.contains("pci")))
                || (target_kw.contains("usb")
                    && (name_lower.contains("device")
                        || name_lower.contains("usb")));

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

        let format_rank = |fmt: SampleFormat| match fmt {
            SampleFormat::F32 => 0,
            SampleFormat::I16 => 1,
            SampleFormat::I32 => 2,
            SampleFormat::U16 => 3,
            _ => 10,
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

    if let Ok(def) = device.default_output_config() {
        let fmt = def.sample_format();
        let stream_cfg = def.into();
        return Ok((stream_cfg, fmt));
    }

    // Fallback if device configuration query failed (e.g. ALSA slave locked)
    Ok((
        StreamConfig {
            channels: 2,
            sample_rate: cpal::SampleRate(target_rate),
            buffer_size: cpal::BufferSize::Default,
        },
        SampleFormat::F32,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_rejection_virtual_and_routune() {
        assert!(is_rejected_device_name("Routune Game"));
        assert!(is_rejected_device_name("routune-chat"));
        assert!(is_rejected_device_name("Virtual Audio Device"));
        assert!(is_rejected_device_name("virtual-sink"));
        assert!(is_rejected_device_name("Null Output"));
        assert!(is_rejected_device_name("Dummy Output"));
        assert!(is_rejected_device_name("ALSA Loopback Device"));
        assert!(is_rejected_device_name("surround40:CARD=Generic_1,DEV=0"));
        assert!(is_rejected_device_name("surround51:CARD=Device,DEV=0"));
        assert!(is_rejected_device_name("iec958:CARD=Device,DEV=0"));
        assert!(is_rejected_device_name("hdmi:CARD=NVidia,DEV=0"));
        assert!(is_rejected_device_name("hdmi:CARD=Generic,DEV=1"));
        assert!(is_rejected_device_name("spdif:CARD=Generic_1,DEV=0"));

        assert!(!is_rejected_device_name("sysdefault:CARD=Device"));
        assert!(!is_rejected_device_name("front:CARD=Generic_1,DEV=0"));
        assert!(!is_rejected_device_name("USB Audio DAC"));
        assert!(!is_rejected_device_name("Speakers (Realtek Audio)"));
        assert!(!is_rejected_device_name("Ryzen HD Audio"));
    }

    #[test]
    fn test_format_display_name_beautification() {
        assert_eq!(format_device_display_name("sysdefault:CARD=Device"), "USB Audio Device");
        assert_eq!(format_device_display_name("front:CARD=Device,DEV=0"), "USB Audio Device");
        assert_eq!(format_device_display_name("sysdefault:CARD=Generic_1"), "Ryzen HD Audio (Headphones)");
        assert_eq!(format_device_display_name("front:CARD=Generic_1,DEV=0"), "Ryzen HD Audio (Headphones)");
        assert_eq!(format_device_display_name("pipewire"), "PipeWire Default Output");
        assert_eq!(format_device_display_name("default"), "System Default");
    }

    #[test]
    fn test_hardware_outputs_deduplication_and_usb_presence() {
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

        // Verify USB device is captured and sysdefault preferred
        if let Some(usb) = outputs.iter().find(|o| o.display_name == "USB Audio Device") {
            assert_eq!(usb.id, "sysdefault:CARD=Device");
        }
    }

    #[test]
    fn test_device_resolution_and_fallback_matching() {
        let (mut router, pusher) = ChannelRouter::new();

        // 1. Unassign channel
        let unassign_res = router.set_channel_routing(1, "none").unwrap();
        assert_eq!(unassign_res, "Unassigned");
        assert!(pusher.game.lock().unwrap().is_none());

        // 2. Exact match on raw ID
        if let Some(dev) = router.find_device("sysdefault:CARD=Device") {
            assert!(dev.name().unwrap().contains("Device"));
        }

        // 3. Fallback match by clean display name
        if let Some(dev) = router.find_device("USB Audio Device") {
            assert!(dev.name().unwrap().contains("Device"));
        }

        // 4. Fallback match by clean display name substring
        if let Some(dev) = router.find_device("Headphones") {
            let name = dev.name().unwrap();
            assert!(name.contains("Generic_1") || name.contains("Generic") || name.contains("ryzen") || name.contains("analog"));
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
        let usb_score = score_physical_output_device("sysdefault:CARD=Device (USB Audio)");
        let ryzen_score = score_physical_output_device("front:CARD=Generic_1 (Ryzen HD Audio)");

        assert!(usb_score > default_bridge_score);
        assert!(usb_score > pipewire_bridge_score);
        assert!(ryzen_score > default_bridge_score);
        assert!(ryzen_score > pipewire_bridge_score);
    }
}
