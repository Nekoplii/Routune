use crate::graph::AudioNode;

/// Real-time noise gate node for cutting background floor noise below a threshold.
pub struct NoiseGateNode {
    id: String,
    name: String,
    enabled: bool,
    threshold_db: f32,
    attack_ms: f32,
    release_ms: f32,
    sample_rate: u32,
    gain: f32,
}

impl NoiseGateNode {
    pub fn new(id: impl Into<String>, name: impl Into<String>, sample_rate: u32) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            enabled: false,
            threshold_db: -40.0,
            attack_ms: 5.0,
            release_ms: 100.0,
            sample_rate: sample_rate.max(1),
            gain: 1.0,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.gain = 1.0;
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_threshold(&mut self, threshold_db: f32) {
        self.threshold_db = threshold_db.clamp(-80.0, 0.0);
    }

    pub fn threshold_db(&self) -> f32 {
        self.threshold_db
    }

    pub fn set_attack_ms(&mut self, attack_ms: f32) {
        self.attack_ms = attack_ms.clamp(0.1, 200.0);
    }

    pub fn attack_ms(&self) -> f32 {
        self.attack_ms
    }

    pub fn set_release_ms(&mut self, release_ms: f32) {
        self.release_ms = release_ms.clamp(1.0, 2000.0);
    }

    pub fn release_ms(&self) -> f32 {
        self.release_ms
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate.max(1);
    }
}

impl AudioNode for NoiseGateNode {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn node_type(&self) -> &str {
        "NoiseGateNode"
    }

    fn set_noise_gate(&mut self, enabled: bool, threshold_db: f32, attack_ms: f32, release_ms: f32) {
        self.set_enabled(enabled);
        self.set_threshold(threshold_db);
        self.set_attack_ms(attack_ms);
        self.set_release_ms(release_ms);
    }

    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        let len = input.len().min(output.len());
        if len == 0 {
            return;
        }

        if !self.enabled {
            output[..len].copy_from_slice(&input[..len]);
            return;
        }

        let thresh_linear = 10.0f32.powf(self.threshold_db / 20.0);
        let alpha_attack = (-1.0 / ((self.attack_ms * 0.001) * self.sample_rate as f32)).exp();
        let alpha_release = (-1.0 / ((self.release_ms * 0.001) * self.sample_rate as f32)).exp();

        let frames = len / 2;
        for i in 0..frames {
            let left = input[i * 2];
            let right = input[i * 2 + 1];
            let abs_max = left.abs().max(right.abs());

            let target_gain = if abs_max >= thresh_linear {
                1.0f32
            } else {
                0.0f32
            };

            if target_gain > self.gain {
                self.gain = alpha_attack * self.gain + (1.0 - alpha_attack) * target_gain;
            } else {
                self.gain = alpha_release * self.gain + (1.0 - alpha_release) * target_gain;
            }

            output[i * 2] = (left * self.gain).clamp(-1.0, 1.0);
            output[i * 2 + 1] = (right * self.gain).clamp(-1.0, 1.0);
        }

        if len % 2 == 1 {
            let sample = input[len - 1];
            let target_gain = if sample.abs() >= thresh_linear { 1.0 } else { 0.0 };
            if target_gain > self.gain {
                self.gain = alpha_attack * self.gain + (1.0 - alpha_attack) * target_gain;
            } else {
                self.gain = alpha_release * self.gain + (1.0 - alpha_release) * target_gain;
            }
            output[len - 1] = (sample * self.gain).clamp(-1.0, 1.0);
        }
    }
}

/// Dynamic range compressor for voice leveling and peak control.
pub struct CompressorNode {
    id: String,
    name: String,
    enabled: bool,
    threshold_db: f32,
    ratio: f32,
    attack_ms: f32,
    release_ms: f32,
    makeup_gain_db: f32,
    sample_rate: u32,
    current_gr_db: f32,
}

impl CompressorNode {
    pub fn new(id: impl Into<String>, name: impl Into<String>, sample_rate: u32) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            enabled: false,
            threshold_db: -18.0,
            ratio: 4.0,
            attack_ms: 10.0,
            release_ms: 100.0,
            makeup_gain_db: 0.0,
            sample_rate: sample_rate.max(1),
            current_gr_db: 0.0,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.current_gr_db = 0.0;
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_threshold(&mut self, threshold_db: f32) {
        self.threshold_db = threshold_db.clamp(-60.0, 0.0);
    }

    pub fn threshold_db(&self) -> f32 {
        self.threshold_db
    }

    pub fn set_ratio(&mut self, ratio: f32) {
        self.ratio = ratio.clamp(1.0, 20.0);
    }

    pub fn ratio(&self) -> f32 {
        self.ratio
    }

    pub fn set_attack_ms(&mut self, attack_ms: f32) {
        self.attack_ms = attack_ms.clamp(0.1, 500.0);
    }

    pub fn attack_ms(&self) -> f32 {
        self.attack_ms
    }

    pub fn set_release_ms(&mut self, release_ms: f32) {
        self.release_ms = release_ms.clamp(1.0, 3000.0);
    }

    pub fn release_ms(&self) -> f32 {
        self.release_ms
    }

    pub fn set_makeup_gain_db(&mut self, makeup_gain_db: f32) {
        self.makeup_gain_db = makeup_gain_db.clamp(0.0, 24.0);
    }

    pub fn makeup_gain_db(&self) -> f32 {
        self.makeup_gain_db
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate.max(1);
    }
}

impl AudioNode for CompressorNode {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn node_type(&self) -> &str {
        "CompressorNode"
    }

    fn set_compressor(
        &mut self,
        enabled: bool,
        threshold_db: f32,
        ratio: f32,
        attack_ms: f32,
        release_ms: f32,
        makeup_gain_db: f32,
    ) {
        self.set_enabled(enabled);
        self.set_threshold(threshold_db);
        self.set_ratio(ratio);
        self.set_attack_ms(attack_ms);
        self.set_release_ms(release_ms);
        self.set_makeup_gain_db(makeup_gain_db);
    }

    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        let len = input.len().min(output.len());
        if len == 0 {
            return;
        }

        if !self.enabled {
            output[..len].copy_from_slice(&input[..len]);
            return;
        }

        let alpha_attack = (-1.0 / ((self.attack_ms * 0.001) * self.sample_rate as f32)).exp();
        let alpha_release = (-1.0 / ((self.release_ms * 0.001) * self.sample_rate as f32)).exp();
        let makeup_linear = 10.0f32.powf(self.makeup_gain_db / 20.0);

        let frames = len / 2;
        for i in 0..frames {
            let left = input[i * 2];
            let right = input[i * 2 + 1];
            let abs_max = left.abs().max(right.abs()).max(1e-6);
            let input_db = 20.0 * abs_max.log10();

            let target_gr_db = if input_db > self.threshold_db {
                -(input_db - self.threshold_db) * (1.0 - 1.0 / self.ratio)
            } else {
                0.0
            };

            if target_gr_db < self.current_gr_db {
                self.current_gr_db = alpha_attack * self.current_gr_db + (1.0 - alpha_attack) * target_gr_db;
            } else {
                self.current_gr_db = alpha_release * self.current_gr_db + (1.0 - alpha_release) * target_gr_db;
            }

            let gr_linear = 10.0f32.powf(self.current_gr_db / 20.0);
            let total_gain = gr_linear * makeup_linear;

            output[i * 2] = (left * total_gain).clamp(-1.0, 1.0);
            output[i * 2 + 1] = (right * total_gain).clamp(-1.0, 1.0);
        }

        if len % 2 == 1 {
            let sample = input[len - 1];
            let abs_val = sample.abs().max(1e-6);
            let input_db = 20.0 * abs_val.log10();
            let target_gr_db = if input_db > self.threshold_db {
                -(input_db - self.threshold_db) * (1.0 - 1.0 / self.ratio)
            } else {
                0.0
            };
            if target_gr_db < self.current_gr_db {
                self.current_gr_db = alpha_attack * self.current_gr_db + (1.0 - alpha_attack) * target_gr_db;
            } else {
                self.current_gr_db = alpha_release * self.current_gr_db + (1.0 - alpha_release) * target_gr_db;
            }
            let gr_linear = 10.0f32.powf(self.current_gr_db / 20.0);
            output[len - 1] = (sample * gr_linear * makeup_linear).clamp(-1.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noise_gate_passthrough_when_disabled() {
        let mut gate = NoiseGateNode::new("test-gate", "Test Gate", 48000);
        let input = [0.01f32, -0.01, 0.05, -0.05];
        let mut output = [0.0f32; 4];
        gate.process(&input, &mut output);
        assert_eq!(input, output);
    }

    #[test]
    fn test_noise_gate_attenuates_below_threshold() {
        let mut gate = NoiseGateNode::new("test-gate", "Test Gate", 48000);
        gate.set_enabled(true);
        gate.set_threshold(-20.0); // ~0.1 linear
        gate.set_release_ms(1.0); // fast release for test
        gate.gain = 0.0; // simulate closed gate

        // Input signals well below -20 dB (e.g. 0.001 ~ -60 dB)
        let input = [0.001f32; 256];
        let mut output = [0.0f32; 256];
        gate.process(&input, &mut output);

        // All output samples must remain heavily attenuated near zero
        for &s in &output[200..] {
            assert!(s.abs() < 0.0001);
        }
    }

    #[test]
    fn test_noise_gate_opens_above_threshold() {
        let mut gate = NoiseGateNode::new("test-gate", "Test Gate", 48000);
        gate.set_enabled(true);
        gate.set_threshold(-20.0);
        gate.set_attack_ms(0.5);

        // Loud input well above threshold (0.8 linear ~ -1.9 dB)
        let input = [0.8f32; 512];
        let mut output = [0.0f32; 512];
        gate.process(&input, &mut output);

        // Gate should fully open towards 0.8
        let last = output[511];
        assert!((last - 0.8).abs() < 0.05);
    }

    #[test]
    fn test_compressor_attenuates_above_threshold() {
        let mut comp = CompressorNode::new("test-comp", "Test Comp", 48000);
        comp.set_enabled(true);
        comp.set_threshold(-12.0); // ~0.25 linear
        comp.set_ratio(4.0);
        comp.set_attack_ms(0.5);

        // Loud signal at 1.0 (0 dB) -> 12 dB above threshold.
        // With 4:1 ratio, gain reduction is 12 * (1 - 1/4) = 9 dB reduction (~0.355 total amplitude).
        let input = [1.0f32; 1024];
        let mut output = [0.0f32; 1024];
        comp.process(&input, &mut output);

        let final_val = output[1023];
        assert!(final_val < 0.5 && final_val > 0.3);
    }

    #[test]
    fn test_compressor_makeup_gain() {
        let mut comp = CompressorNode::new("test-comp", "Test Comp", 48000);
        comp.set_enabled(true);
        comp.set_threshold(0.0); // no compression on quiet signals
        comp.set_makeup_gain_db(6.0); // ~2x linear boost

        let input = [0.1f32; 100];
        let mut output = [0.0f32; 100];
        comp.process(&input, &mut output);

        let boosted = output[99];
        assert!((boosted - 0.2).abs() < 0.02);
    }
}
