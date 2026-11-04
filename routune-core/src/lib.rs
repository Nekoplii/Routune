pub mod ai_noise;
pub mod config;
pub mod dynamics;
pub mod graph;
pub mod spatial;

pub use ai_noise::AiNoiseNode;
pub use dynamics::{CompressorNode, NoiseGateNode};
pub use spatial::{soft_limit, SpatialAudioMode, SpatialAudioNode, VolumeBoostNode};
pub use config::{ChannelEqConfig, ChannelNoiseConfig, ChannelRoutingConfig, RoutuneConfig};
pub use graph::{
    AudioEngineGraph, AudioNode, BypassNode, Channel, DspBypassNode, EqNode, Graph, GraphCommand,
    GraphController, NodeInfo, PhysicalOutputNode, PipeWireInputNode, ProcessContext,
    VirtualChannel, VirtualInputNode, DEFAULT_EQ_Q, EQ_BANDS_COUNT, EQ_FREQUENCIES_HZ,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ChannelLayout {
    Mono,
    Stereo,
    Surround51,
    Surround71,
    Custom(usize),
}

impl ChannelLayout {
    pub fn channels(&self) -> usize {
        match self {
            Self::Mono => 1,
            Self::Stereo => 2,
            Self::Surround51 => 6,
            Self::Surround71 => 8,
            Self::Custom(c) => *c,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub is_input: bool,
    pub sample_rate: u32,
    pub channels: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterType {
    LowPass,
    HighPass,
    BandPass,
    Peaking,
    Notch,
    LowShelf,
    HighShelf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EqBandConfig {
    pub enabled: bool,
    pub filter_type: FilterType,
    pub frequency_hz: f32,
    pub q_factor: f32,
    pub gain_db: f32,
}

impl Default for EqBandConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            filter_type: FilterType::Peaking,
            frequency_hz: 1000.0,
            q_factor: 0.707,
            gain_db: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GraphNodeType {
    SourceDevice { device_id: String },
    SinkDevice { device_id: String },
    VirtualSink { name: String },
    Equalizer { bands: Vec<EqBandConfig> },
    VolumeGain { gain_linear: f32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioGraphNode {
    pub id: String,
    pub name: String,
    pub node_type: GraphNodeType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioGraphEdge {
    pub from_node: String,
    pub from_channel: usize,
    pub to_node: String,
    pub to_channel: usize,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RoutingGraph {
    pub nodes: Vec<AudioGraphNode>,
    pub edges: Vec<AudioGraphEdge>,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
pub struct AudioStreamMetrics {
    pub peak_volume_db: f32,
    pub rms_volume_db: f32,
    pub frames_processed: u64,
    pub buffer_underruns: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_layout_counts() {
        assert_eq!(ChannelLayout::Mono.channels(), 1);
        assert_eq!(ChannelLayout::Stereo.channels(), 2);
        assert_eq!(ChannelLayout::Surround51.channels(), 6);
        assert_eq!(ChannelLayout::Custom(4).channels(), 4);
    }

    #[test]
    fn test_default_eq_band() {
        let band = EqBandConfig::default();
        assert!(band.enabled);
        assert_eq!(band.filter_type, FilterType::Peaking);
        assert_eq!(band.frequency_hz, 1000.0);
    }
}
