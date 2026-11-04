use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::graph::{calculate_metrics, AudioNode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SpatialAudioMode {
    #[default]
    Headphone = 0,
    Speaker = 1,
}

/// Circular delay buffer size in samples.
/// At 48 kHz, 32 samples = ~0.67 ms (well exceeds ITD of 0.28 ms).
const DELAY_BUF_SIZE: usize = 32;

/// Spatial Audio Node for Game, Media, and Aux channels.
/// Provides binaural crossfeed (Headphone mode) or acoustic crosstalk cancellation
/// and stereo widening (Speaker mode). Zero allocation in processing loop.
pub struct SpatialAudioNode {
    id: String,
    name: String,
    enabled: bool,
    mode: SpatialAudioMode,
    strength: f32,
    // Headphone binaural ITD delay lines (circular buffers)
    delay_l: [f32; DELAY_BUF_SIZE],
    delay_r: [f32; DELAY_BUF_SIZE],
    delay_write_pos: usize,
    // Head shadow 1-pole low-pass filter states
    lp_l: f32,
    lp_r: f32,
    // Speaker Haas side-channel delay buffer
    speaker_side_delay: [f32; DELAY_BUF_SIZE],
    speaker_write_pos: usize,
    peak_db: Arc<AtomicU32>,
    rms_db: Arc<AtomicU32>,
}

impl SpatialAudioNode {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            enabled: false,
            mode: SpatialAudioMode::Headphone,
            strength: 1.0,
            delay_l: [0.0; DELAY_BUF_SIZE],
            delay_r: [0.0; DELAY_BUF_SIZE],
            delay_write_pos: 0,
            lp_l: 0.0,
            lp_r: 0.0,
            speaker_side_delay: [0.0; DELAY_BUF_SIZE],
            speaker_write_pos: 0,
            peak_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
            rms_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.reset_internal();
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_mode(&mut self, mode: SpatialAudioMode) {
        if self.mode != mode {
            self.mode = mode;
            self.reset_internal();
        }
    }

    pub fn mode(&self) -> SpatialAudioMode {
        self.mode
    }

    pub fn set_strength(&mut self, strength: f32) {
        self.strength = strength.clamp(0.0, 1.0);
    }

    pub fn strength(&self) -> f32 {
        self.strength
    }

    fn reset_internal(&mut self) {
        self.delay_l.fill(0.0);
        self.delay_r.fill(0.0);
        self.delay_write_pos = 0;
        self.lp_l = 0.0;
        self.lp_r = 0.0;
        self.speaker_side_delay.fill(0.0);
        self.speaker_write_pos = 0;
    }
}

impl AudioNode for SpatialAudioNode {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn node_type(&self) -> &str {
        "SpatialAudioNode"
    }

    fn is_bypassed(&self) -> bool {
        !self.enabled
    }

    fn set_bypassed(&mut self, bypassed: bool) {
        self.set_enabled(!bypassed);
    }

    fn reset(&mut self) {
        self.reset_internal();
    }

    fn set_spatial_audio(&mut self, enabled: bool, mode: SpatialAudioMode, strength: f32) {
        self.set_enabled(enabled);
        self.set_mode(mode);
        self.set_strength(strength);
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

        if !self.enabled || self.strength <= 1e-4 {
            output[..len].copy_from_slice(&input[..len]);
            if len < output.len() {
                output[len..].fill(0.0);
            }
            let (peak, rms) = calculate_metrics(&output[..len]);
            self.peak_db.store(peak.to_bits(), Ordering::Relaxed);
            self.rms_db.store(rms.to_bits(), Ordering::Relaxed);
            return;
        }

        let num_frames = len / 2;
        let strength = self.strength;

        match self.mode {
            SpatialAudioMode::Headphone => {
                // Binaural crossfeed + head shadow simulation
                // ITD: 13 samples delay (~0.27ms at 48kHz)
                const ITD_DELAY_SAMPLES: usize = 13;
                // ILD 1-pole low-pass filter: alpha for ~1.6kHz cutoff at 48kHz
                const LP_ALPHA: f32 = 0.1736;
                let cross_gain = 0.35 * strength;
                let direct_gain = 1.0 - cross_gain * 0.4;
                let side_boost = 1.0 + 0.25 * strength;

                for f in 0..num_frames {
                    let in_l = input[f * 2];
                    let in_r = input[f * 2 + 1];

                    // Store into circular buffer
                    let w_pos = self.delay_write_pos;
                    self.delay_l[w_pos] = in_l;
                    self.delay_r[w_pos] = in_r;

                    // Read delayed samples for contralateral ear
                    let r_pos = (w_pos + DELAY_BUF_SIZE - ITD_DELAY_SAMPLES) % DELAY_BUF_SIZE;
                    let delayed_l = self.delay_l[r_pos];
                    let delayed_r = self.delay_r[r_pos];

                    self.delay_write_pos = (w_pos + 1) % DELAY_BUF_SIZE;

                    // Apply 1-pole head-shadow low-pass filter
                    self.lp_l += LP_ALPHA * (delayed_l - self.lp_l);
                    self.lp_r += LP_ALPHA * (delayed_r - self.lp_r);

                    // Crossfeed: L hears delayed/filtered R, R hears delayed/filtered L
                    let mut out_l = direct_gain * in_l + cross_gain * self.lp_r;
                    let mut out_r = direct_gain * in_r + cross_gain * self.lp_l;

                    // Mid/Side spatial widening
                    let mid = 0.5 * (out_l + out_r);
                    let side = 0.5 * (out_l - out_r) * side_boost;

                    out_l = mid + side;
                    out_r = mid - side;

                    output[f * 2] = out_l.clamp(-1.0, 1.0);
                    output[f * 2 + 1] = out_r.clamp(-1.0, 1.0);
                }
            }
            SpatialAudioMode::Speaker => {
                // Acoustic crosstalk cancellation and stereo soundstage expansion
                // Haas delay for inverted cross-feed: 7 samples (~0.15ms at 48kHz)
                const HAAS_DELAY_SAMPLES: usize = 7;
                let side_scale = 1.0 + 0.6 * strength;
                let cross_cancel_gain = 0.22 * strength;
                let norm = 1.0 / (1.0 + 0.25 * strength).sqrt();

                for f in 0..num_frames {
                    let in_l = input[f * 2];
                    let in_r = input[f * 2 + 1];

                    let mid = 0.5 * (in_l + in_r);
                    let side = 0.5 * (in_l - in_r);

                    // Store side signal into Haas delay buffer
                    let w_pos = self.speaker_write_pos;
                    self.speaker_side_delay[w_pos] = side;

                    let r_pos = (w_pos + DELAY_BUF_SIZE - HAAS_DELAY_SAMPLES) % DELAY_BUF_SIZE;
                    let delayed_side = self.speaker_side_delay[r_pos];
                    self.speaker_write_pos = (w_pos + 1) % DELAY_BUF_SIZE;

                    // Widened stereo with cross-channel decorrelation
                    let expanded_side = side * side_scale;
                    let cancel = cross_cancel_gain * delayed_side;

                    let out_l = (mid + expanded_side - cancel) * norm;
                    let out_r = (mid - expanded_side + cancel) * norm;

                    output[f * 2] = out_l.clamp(-1.0, 1.0);
                    output[f * 2 + 1] = out_r.clamp(-1.0, 1.0);
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
}

/// Smooth rational soft limiter curve.
/// Linear when |x| <= 0.70; smoothly compresses towards +/- 1.0 as |x| increases.
/// Continuous 1st derivative matches 1.0 at x = 0.70.
#[inline]
pub fn soft_limit(x: f32) -> f32 {
    let abs = x.abs();
    const T: f32 = 0.7;
    const M: f32 = 0.3;
    if abs <= T {
        x
    } else {
        let d = abs - T;
        let sat = T + (M * d) / (M + d);
        sat.copysign(x).clamp(-1.0, 1.0)
    }
}

/// Volume Boost Node for Game, Media, and Aux channels.
/// Amplifies quieter details with transparent soft-saturation limiting
/// to prevent digital clipping.
pub struct VolumeBoostNode {
    id: String,
    name: String,
    enabled: bool,
    boost_db: f32,
    gain: f32,
    peak_db: Arc<AtomicU32>,
    rms_db: Arc<AtomicU32>,
}

impl VolumeBoostNode {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            enabled: false,
            boost_db: 6.0,
            gain: 10.0f32.powf(6.0 / 20.0),
            peak_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
            rms_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_boost_db(&mut self, boost_db: f32) {
        let clamped = boost_db.clamp(0.0, 12.0);
        self.boost_db = clamped;
        self.gain = 10.0f32.powf(clamped / 20.0);
    }

    pub fn boost_db(&self) -> f32 {
        self.boost_db
    }
}

impl AudioNode for VolumeBoostNode {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn node_type(&self) -> &str {
        "VolumeBoostNode"
    }

    fn is_bypassed(&self) -> bool {
        !self.enabled
    }

    fn set_bypassed(&mut self, bypassed: bool) {
        self.set_enabled(!bypassed);
    }

    fn set_volume_boost(&mut self, enabled: bool, boost_db: f32) {
        self.set_enabled(enabled);
        self.set_boost_db(boost_db);
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

        if !self.enabled || self.boost_db <= 0.001 {
            output[..len].copy_from_slice(&input[..len]);
        } else {
            let gain = self.gain;
            for i in 0..len {
                output[i] = soft_limit(input[i] * gain);
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
    fn test_spatial_audio_node_bypass_passthrough() {
        let mut node = SpatialAudioNode::new("test-spatial", "Test Spatial");
        let input = [0.1f32, -0.2, 0.3, -0.4];
        let mut output = [0.0f32; 4];

        node.process(&input, &mut output);
        assert_eq!(input, output);
    }

    #[test]
    fn test_spatial_audio_headphone_and_speaker_processing() {
        let mut node = SpatialAudioNode::new("test-spatial", "Test Spatial");
        node.set_enabled(true);
        node.set_strength(1.0);

        // Headphone mode
        node.set_mode(SpatialAudioMode::Headphone);
        let input = [0.5f32, 0.0, 0.5, 0.0, 0.5, 0.0, 0.5, 0.0];
        let mut output_hp = [0.0f32; 8];
        node.process(&input, &mut output_hp);
        // Left should have direct signal, right should remain bounded
        assert!(output_hp[0] > 0.0);
        assert!(output_hp.iter().all(|&s| s.abs() <= 1.0));

        // Speaker mode
        node.set_mode(SpatialAudioMode::Speaker);
        let mut output_sp = [0.0f32; 8];
        node.process(&input, &mut output_sp);
        assert!(output_sp.iter().all(|&s| s.abs() <= 1.0));
    }

    #[test]
    fn test_volume_boost_soft_limit_and_amplification() {
        let mut boost = VolumeBoostNode::new("test-boost", "Test Boost");
        boost.set_enabled(true);
        boost.set_boost_db(6.0); // ~2.0x linear gain

        let quiet_in = [0.1f32, -0.1];
        let mut quiet_out = [0.0f32; 2];
        boost.process(&quiet_in, &mut quiet_out);
        assert!(quiet_out[0] > quiet_in[0]);
        assert!((quiet_out[0] - 0.2).abs() < 0.02);

        // Loud signal should be smoothly saturated below 1.0, not hard-clipped or overflowing
        let loud_in = [0.9f32, -0.9];
        let mut loud_out = [0.0f32; 2];
        boost.process(&loud_in, &mut loud_out);
        assert!(loud_out[0] < 1.0);
        assert!(loud_out[0] > 0.8);
        assert!(loud_out[1] > -1.0);
        assert!(loud_out[1] < -0.8);
    }

    #[test]
    fn test_soft_limit_continuity() {
        assert_eq!(soft_limit(0.5), 0.5);
        assert_eq!(soft_limit(0.7), 0.7);
        let near = soft_limit(0.7001);
        assert!((near - 0.7001).abs() < 1e-4);
        assert!(soft_limit(10.0) < 1.0);
        assert!(soft_limit(-10.0) > -1.0);
    }
}
