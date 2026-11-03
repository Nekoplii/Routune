use std::fs;
use std::io;
use std::path::Path;
use serde::{Deserialize, Serialize};

use crate::graph::{VirtualChannel, EQ_BANDS_COUNT};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelRoutingConfig {
    pub game: String,
    pub chat: String,
    pub media: String,
    pub aux: String,
    #[serde(default = "default_route")]
    pub mic: String,
}

fn default_route() -> String {
    "none".to_string()
}

impl Default for ChannelRoutingConfig {
    fn default() -> Self {
        Self {
            game: "none".to_string(),
            chat: "none".to_string(),
            media: "none".to_string(),
            aux: "none".to_string(),
            mic: "none".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChannelEqConfig {
    pub game: [f32; EQ_BANDS_COUNT],
    pub chat: [f32; EQ_BANDS_COUNT],
    pub media: [f32; EQ_BANDS_COUNT],
    pub aux: [f32; EQ_BANDS_COUNT],
    #[serde(default = "default_bands")]
    pub mic: [f32; EQ_BANDS_COUNT],
}

fn default_bands() -> [f32; EQ_BANDS_COUNT] {
    [0.0f32; EQ_BANDS_COUNT]
}

impl Default for ChannelEqConfig {
    fn default() -> Self {
        Self {
            game: [0.0f32; EQ_BANDS_COUNT],
            chat: [0.0f32; EQ_BANDS_COUNT],
            media: [0.0f32; EQ_BANDS_COUNT],
            aux: [0.0f32; EQ_BANDS_COUNT],
            mic: [0.0f32; EQ_BANDS_COUNT],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ChannelNoiseConfig {
    pub game: bool,
    pub chat: bool,
    pub media: bool,
    pub aux: bool,
    #[serde(default)]
    pub mic: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChannelVolumeConfig {
    pub master: f32,
    pub game: f32,
    pub chat: f32,
    pub media: f32,
    pub aux: f32,
    #[serde(default)]
    pub mic: f32,
}

impl Default for ChannelVolumeConfig {
    fn default() -> Self {
        Self {
            master: 0.0,
            game: 0.0,
            chat: 0.0,
            media: 0.0,
            aux: 0.0,
            mic: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ChannelMuteConfig {
    pub master: bool,
    pub game: bool,
    pub chat: bool,
    pub media: bool,
    pub aux: bool,
    #[serde(default)]
    pub mic: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoiseGateConfig {
    pub enabled: bool,
    pub threshold_db: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
}

impl Default for NoiseGateConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold_db: -40.0,
            attack_ms: 5.0,
            release_ms: 100.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompressorConfig {
    pub enabled: bool,
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub makeup_gain_db: f32,
}

impl Default for CompressorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold_db: -18.0,
            ratio: 4.0,
            attack_ms: 10.0,
            release_ms: 100.0,
            makeup_gain_db: 0.0,
        }
    }
}

/// Persistent configuration for Routune daemon state.
/// Holds hardware device mappings and 10-band EQ settings per channel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RoutuneConfig {
    pub routing: ChannelRoutingConfig,
    pub eq: ChannelEqConfig,
    #[serde(default)]
    pub noise_cancellation: ChannelNoiseConfig,
    #[serde(default)]
    pub volume: ChannelVolumeConfig,
    #[serde(default)]
    pub mute: ChannelMuteConfig,
    #[serde(default)]
    pub noise_gate: NoiseGateConfig,
    #[serde(default)]
    pub compressor: CompressorConfig,
}

impl RoutuneConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_route(&self, channel: VirtualChannel) -> &str {
        match channel {
            VirtualChannel::Game => &self.routing.game,
            VirtualChannel::Chat => &self.routing.chat,
            VirtualChannel::Media => &self.routing.media,
            VirtualChannel::Aux => &self.routing.aux,
            VirtualChannel::Mic => &self.routing.mic,
            VirtualChannel::Master => "none",
        }
    }

    pub fn set_route(&mut self, channel: VirtualChannel, device: impl Into<String>) {
        let val = device.into();
        match channel {
            VirtualChannel::Game => self.routing.game = val,
            VirtualChannel::Chat => self.routing.chat = val,
            VirtualChannel::Media => self.routing.media = val,
            VirtualChannel::Aux => self.routing.aux = val,
            VirtualChannel::Mic => self.routing.mic = val,
            VirtualChannel::Master => {}
        }
    }

    pub fn get_eq(&self, channel: VirtualChannel) -> [f32; EQ_BANDS_COUNT] {
        match channel {
            VirtualChannel::Game => self.eq.game,
            VirtualChannel::Chat => self.eq.chat,
            VirtualChannel::Media => self.eq.media,
            VirtualChannel::Aux => self.eq.aux,
            VirtualChannel::Mic => self.eq.mic,
            VirtualChannel::Master => [0.0f32; EQ_BANDS_COUNT],
        }
    }

    pub fn set_eq_band(&mut self, channel: VirtualChannel, band_index: usize, gain_db: f32) {
        if band_index >= EQ_BANDS_COUNT {
            return;
        }
        let clamped = gain_db.clamp(-24.0, 24.0);
        match channel {
            VirtualChannel::Game => self.eq.game[band_index] = clamped,
            VirtualChannel::Chat => self.eq.chat[band_index] = clamped,
            VirtualChannel::Media => self.eq.media[band_index] = clamped,
            VirtualChannel::Aux => self.eq.aux[band_index] = clamped,
            VirtualChannel::Mic => self.eq.mic[band_index] = clamped,
            VirtualChannel::Master => {
                self.eq.game[band_index] = clamped;
                self.eq.chat[band_index] = clamped;
                self.eq.media[band_index] = clamped;
                self.eq.aux[band_index] = clamped;
                self.eq.mic[band_index] = clamped;
            }
        }
    }

    pub fn set_eq(&mut self, channel: VirtualChannel, gains: [f32; EQ_BANDS_COUNT]) {
        match channel {
            VirtualChannel::Game => self.eq.game = gains,
            VirtualChannel::Chat => self.eq.chat = gains,
            VirtualChannel::Media => self.eq.media = gains,
            VirtualChannel::Aux => self.eq.aux = gains,
            VirtualChannel::Mic => self.eq.mic = gains,
            VirtualChannel::Master => {
                self.eq.game = gains;
                self.eq.chat = gains;
                self.eq.media = gains;
                self.eq.aux = gains;
                self.eq.mic = gains;
            }
        }
    }

    pub fn get_noise_cancellation(&self, channel: VirtualChannel) -> bool {
        match channel {
            VirtualChannel::Game => self.noise_cancellation.game,
            VirtualChannel::Chat => self.noise_cancellation.chat,
            VirtualChannel::Media => self.noise_cancellation.media,
            VirtualChannel::Aux => self.noise_cancellation.aux,
            VirtualChannel::Mic => self.noise_cancellation.mic,
            VirtualChannel::Master => false,
        }
    }

    pub fn set_noise_cancellation(&mut self, channel: VirtualChannel, enabled: bool) {
        match channel {
            VirtualChannel::Game => self.noise_cancellation.game = enabled,
            VirtualChannel::Chat => self.noise_cancellation.chat = enabled,
            VirtualChannel::Media => self.noise_cancellation.media = enabled,
            VirtualChannel::Aux => self.noise_cancellation.aux = enabled,
            VirtualChannel::Mic => self.noise_cancellation.mic = enabled,
            VirtualChannel::Master => {
                self.noise_cancellation.game = enabled;
                self.noise_cancellation.chat = enabled;
                self.noise_cancellation.media = enabled;
                self.noise_cancellation.aux = enabled;
                self.noise_cancellation.mic = enabled;
            }
        }
    }

    pub fn get_volume(&self, channel: VirtualChannel) -> f32 {
        match channel {
            VirtualChannel::Master => self.volume.master,
            VirtualChannel::Game => self.volume.game,
            VirtualChannel::Chat => self.volume.chat,
            VirtualChannel::Media => self.volume.media,
            VirtualChannel::Aux => self.volume.aux,
            VirtualChannel::Mic => self.volume.mic,
        }
    }

    pub fn set_volume(&mut self, channel: VirtualChannel, volume_db: f32) {
        let clamped = volume_db.clamp(-48.0, 12.0);
        match channel {
            VirtualChannel::Master => self.volume.master = clamped,
            VirtualChannel::Game => self.volume.game = clamped,
            VirtualChannel::Chat => self.volume.chat = clamped,
            VirtualChannel::Media => self.volume.media = clamped,
            VirtualChannel::Aux => self.volume.aux = clamped,
            VirtualChannel::Mic => self.volume.mic = clamped,
        }
    }

    pub fn get_mute(&self, channel: VirtualChannel) -> bool {
        match channel {
            VirtualChannel::Master => self.mute.master,
            VirtualChannel::Game => self.mute.game,
            VirtualChannel::Chat => self.mute.chat,
            VirtualChannel::Media => self.mute.media,
            VirtualChannel::Aux => self.mute.aux,
            VirtualChannel::Mic => self.mute.mic,
        }
    }

    pub fn set_mute(&mut self, channel: VirtualChannel, muted: bool) {
        match channel {
            VirtualChannel::Master => self.mute.master = muted,
            VirtualChannel::Game => self.mute.game = muted,
            VirtualChannel::Chat => self.mute.chat = muted,
            VirtualChannel::Media => self.mute.media = muted,
            VirtualChannel::Aux => self.mute.aux = muted,
            VirtualChannel::Mic => self.mute.mic = muted,
        }
    }

    pub fn get_noise_gate(&self) -> &NoiseGateConfig {
        &self.noise_gate
    }

    pub fn set_noise_gate(
        &mut self,
        enabled: bool,
        threshold_db: f32,
        attack_ms: f32,
        release_ms: f32,
    ) {
        self.noise_gate = NoiseGateConfig {
            enabled,
            threshold_db: threshold_db.clamp(-80.0, 0.0),
            attack_ms: attack_ms.clamp(0.1, 200.0),
            release_ms: release_ms.clamp(1.0, 2000.0),
        };
    }

    pub fn get_compressor(&self) -> &CompressorConfig {
        &self.compressor
    }

    pub fn set_compressor(
        &mut self,
        enabled: bool,
        threshold_db: f32,
        ratio: f32,
        attack_ms: f32,
        release_ms: f32,
        makeup_gain_db: f32,
    ) {
        self.compressor = CompressorConfig {
            enabled,
            threshold_db: threshold_db.clamp(-60.0, 0.0),
            ratio: ratio.clamp(1.0, 20.0),
            attack_ms: attack_ms.clamp(0.1, 500.0),
            release_ms: release_ms.clamp(1.0, 3000.0),
            makeup_gain_db: makeup_gain_db.clamp(0.0, 24.0),
        };
    }

    /// Load configuration from a JSON file, or return default template if not found.
    pub fn load_or_default(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        if path.exists() {
            match fs::read_to_string(path) {
                Ok(content) => match serde_json::from_str(&content) {
                    Ok(cfg) => return cfg,
                    Err(e) => {
                        eprintln!("Failed to parse config at {:?}: {}, using defaults", path, e);
                    }
                },
                Err(e) => {
                    eprintln!("Failed to read config at {:?}: {}, using defaults", path, e);
                }
            }
        }
        Self::default()
    }

    /// Save configuration atomically to disk using a temporary file.
    pub fn save_to_file(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let tmp_path = path.with_extension("tmp");
        fs::write(&tmp_path, json)?;
        fs::rename(tmp_path, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_routune_config_defaults() {
        let cfg = RoutuneConfig::default();
        assert_eq!(cfg.get_route(VirtualChannel::Game), "none");
        assert_eq!(cfg.get_route(VirtualChannel::Chat), "none");
        assert_eq!(cfg.get_route(VirtualChannel::Media), "none");
        assert_eq!(cfg.get_route(VirtualChannel::Aux), "none");

        assert_eq!(cfg.get_eq(VirtualChannel::Game), [0.0; EQ_BANDS_COUNT]);
    }

    #[test]
    fn test_routune_config_mutations() {
        let mut cfg = RoutuneConfig::new();
        cfg.set_route(VirtualChannel::Game, "USB Headset");
        cfg.set_eq_band(VirtualChannel::Game, 5, 4.5);

        assert_eq!(cfg.get_route(VirtualChannel::Game), "USB Headset");
        assert_eq!(cfg.get_eq(VirtualChannel::Game)[5], 4.5);

        // Clamp check
        cfg.set_eq_band(VirtualChannel::Chat, 2, 99.0);
        assert_eq!(cfg.get_eq(VirtualChannel::Chat)[2], 24.0);
    }

    #[test]
    fn test_routune_config_save_and_load() {
        let temp_dir = std::env::temp_dir().join("routune_test_config");
        let config_file = temp_dir.join("config.json");

        let mut cfg = RoutuneConfig::new();
        cfg.set_route(VirtualChannel::Media, "Speakers");
        cfg.set_eq_band(VirtualChannel::Media, 0, -3.0);
        cfg.set_eq_band(VirtualChannel::Media, 9, 6.0);

        cfg.save_to_file(&config_file).expect("Failed to save config");

        let loaded = RoutuneConfig::load_or_default(&config_file);
        assert_eq!(loaded.get_route(VirtualChannel::Media), "Speakers");
        assert_eq!(loaded.get_eq(VirtualChannel::Media)[0], -3.0);
        assert_eq!(loaded.get_eq(VirtualChannel::Media)[9], 6.0);

        let _ = fs::remove_file(&config_file);
        let _ = fs::remove_dir(&temp_dir);
    }
}
