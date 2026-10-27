use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use rtrb::{Producer, RingBuffer};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{error, info, warn};

pub const RING_BUFFER_CAPACITY: usize = 2048;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TelemetrySample {
    pub peak_volume_db: f32,
    pub rms_volume_db: f32,
}

/// Real-time audio DSP callback logic.
///
/// MUST remain lock-free, wait-free, and allocation-free.
#[inline]
pub fn compute_telemetry_chunk(data: &[f32]) -> TelemetrySample {
    if data.is_empty() {
        return TelemetrySample {
            peak_volume_db: -100.0,
            rms_volume_db: -100.0,
        };
    }

    let mut sum_sq = 0.0f32;
    let mut peak = 0.0f32;

    for &sample in data {
        let abs = sample.abs();
        if abs > peak {
            peak = abs;
        }
        sum_sq += sample * sample;
    }

    let rms = (sum_sq / data.len() as f32).sqrt();

    let peak_db = if peak > 1e-5 {
        20.0 * peak.log10()
    } else {
        -100.0
    };

    let rms_db = if rms > 1e-5 {
        20.0 * rms.log10()
    } else {
        -100.0
    };

    TelemetrySample {
        peak_volume_db: peak_db,
        rms_volume_db: rms_db,
    }
}

pub struct AudioCaptureService {
    _stream: Option<Stream>,
    pub is_active: Arc<AtomicBool>,
}

pub fn initialize_audio_capture() -> (AudioCaptureService, rtrb::Consumer<TelemetrySample>) {
    let (producer, consumer) = RingBuffer::<TelemetrySample>::new(RING_BUFFER_CAPACITY);
    let is_active = Arc::new(AtomicBool::new(false));

    match start_cpal_stream(producer, is_active.clone()) {
        Ok(stream) => {
            info!("CPAL audio stream initialized successfully");
            (
                AudioCaptureService {
                    _stream: Some(stream),
                    is_active,
                },
                consumer,
            )
        }
        Err(err) => {
            warn!("Could not start audio stream ({err}). Running in fallback mode.");
            (
                AudioCaptureService {
                    _stream: None,
                    is_active,
                },
                consumer,
            )
        }
    }
}

fn start_cpal_stream(
    mut producer: Producer<TelemetrySample>,
    is_active: Arc<AtomicBool>,
) -> Result<Stream> {
    let host = cpal::default_host();
    info!("CPAL host: {:?}", host.id());

    // Prefer default output device for WASAPI loopback, fallback to default input device
    let (device, is_loopback) = if let Some(out_dev) = host.default_output_device() {
        info!("Selected default output device for loopback");
        (out_dev, true)
    } else if let Some(in_dev) = host.default_input_device() {
        info!("No output device found; using default input device");
        (in_dev, false)
    } else {
        anyhow::bail!("No audio output or input devices available on host");
    };

    let device_name = device.name().unwrap_or_else(|_| "Unknown Device".into());
    info!("Target audio device: {device_name} (loopback: {is_loopback})");

    let supported_config = device
        .default_input_config()
        .or_else(|_| device.default_output_config())
        .context("Failed to query default stream config")?;

    let sample_format = supported_config.sample_format();
    let config: StreamConfig = supported_config.into();

    let err_fn = {
        let is_active = is_active.clone();
        move |err| {
            is_active.store(false, Ordering::Relaxed);
            error!("CPAL audio stream error: {err}");
        }
    };

    let active_flag = is_active.clone();
    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                active_flag.store(true, Ordering::Relaxed);
                let sample = compute_telemetry_chunk(data);
                let _ = producer.push(sample);
            },
            err_fn,
            None,
        )?,
        SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                active_flag.store(true, Ordering::Relaxed);
                if data.is_empty() {
                    return;
                }
                let mut sum_sq = 0.0f32;
                let mut peak = 0.0f32;
                for &s in data {
                    let normalized = s as f32 / 32768.0;
                    let abs = normalized.abs();
                    if abs > peak {
                        peak = abs;
                    }
                    sum_sq += normalized * normalized;
                }
                let rms = (sum_sq / data.len() as f32).sqrt();
                let peak_db = if peak > 1e-5 { 20.0 * peak.log10() } else { -100.0 };
                let rms_db = if rms > 1e-5 { 20.0 * rms.log10() } else { -100.0 };
                let _ = producer.push(TelemetrySample {
                    peak_volume_db: peak_db,
                    rms_volume_db: rms_db,
                });
            },
            err_fn,
            None,
        )?,
        SampleFormat::U16 => device.build_input_stream(
            &config,
            move |data: &[u16], _: &cpal::InputCallbackInfo| {
                active_flag.store(true, Ordering::Relaxed);
                if data.is_empty() {
                    return;
                }
                let mut sum_sq = 0.0f32;
                let mut peak = 0.0f32;
                for &s in data {
                    let normalized = (s as f32 - 32768.0) / 32768.0;
                    let abs = normalized.abs();
                    if abs > peak {
                        peak = abs;
                    }
                    sum_sq += normalized * normalized;
                }
                let rms = (sum_sq / data.len() as f32).sqrt();
                let peak_db = if peak > 1e-5 { 20.0 * peak.log10() } else { -100.0 };
                let rms_db = if rms > 1e-5 { 20.0 * rms.log10() } else { -100.0 };
                let _ = producer.push(TelemetrySample {
                    peak_volume_db: peak_db,
                    rms_volume_db: rms_db,
                });
            },
            err_fn,
            None,
        )?,
        sample_format => anyhow::bail!("Unsupported sample format: {sample_format}"),
    };

    stream.play().context("Failed to start audio stream")?;
    is_active.store(true, Ordering::Relaxed);

    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_telemetry_silence() {
        let samples = [0.0f32; 128];
        let telemetry = compute_telemetry_chunk(&samples);
        assert_eq!(telemetry.peak_volume_db, -100.0);
        assert_eq!(telemetry.rms_volume_db, -100.0);
    }

    #[test]
    fn test_compute_telemetry_full_scale_square() {
        let samples = [1.0f32, -1.0f32, 1.0f32, -1.0f32];
        let telemetry = compute_telemetry_chunk(&samples);
        assert!((telemetry.peak_volume_db - 0.0).abs() < 0.01);
        assert!((telemetry.rms_volume_db - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_ring_buffer_transfer_lock_free() {
        let (mut producer, mut consumer) = RingBuffer::<TelemetrySample>::new(16);
        let sample = TelemetrySample {
            peak_volume_db: -6.0,
            rms_volume_db: -12.0,
        };
        assert!(producer.push(sample).is_ok());
        let popped = consumer.pop().expect("Should pop pushed sample");
        assert_eq!(popped, sample);
    }
}
