use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};
use tonic::{Request, Response, Status};
use tracing::debug;

use crate::audio::TelemetrySample;

pub enum TelemetryConsumer {
    RingBuffer(rtrb::Consumer<TelemetrySample>),
    Channel(crossbeam_channel::Receiver<TelemetrySample>),
}

impl From<rtrb::Consumer<TelemetrySample>> for TelemetryConsumer {
    fn from(c: rtrb::Consumer<TelemetrySample>) -> Self {
        TelemetryConsumer::RingBuffer(c)
    }
}

impl From<crossbeam_channel::Receiver<TelemetrySample>> for TelemetryConsumer {
    fn from(r: crossbeam_channel::Receiver<TelemetrySample>) -> Self {
        TelemetryConsumer::Channel(r)
    }
}

pub mod proto {
    tonic::include_proto!("routune");
}

pub trait TelemetryMetricsProvider: Send + Sync {
    fn get_sink_metrics(&self) -> Vec<proto::SinkMetric>;
    fn get_topology(&self) -> Option<proto::GraphTopology>;
    fn get_master_metrics(&self) -> Option<(f32, f32)>;
    fn get_channel_metrics(&self) -> Vec<proto::ChannelVolumeMetric>;
    fn is_active(&self) -> bool;
    fn update_eq(
        &self,
        _channel: routune_core::graph::VirtualChannel,
        _band_index: usize,
        _gain_db: f32,
    ) -> Result<(), &'static str> {
        Ok(())
    }
    fn get_active_applications(&self) -> Vec<proto::AppStream> {
        vec![]
    }
    fn route_application(
        &self,
        _app_id: u32,
        _target_channel: proto::AudioChannel,
    ) -> Result<Option<String>, &'static str> {
        Ok(None)
    }
    fn toggle_ai_noise(
        &self,
        _channel: routune_core::graph::VirtualChannel,
        _enabled: bool,
    ) -> Result<(), &'static str> {
        Ok(())
    }
    fn set_ai_noise_level(
        &self,
        _channel: routune_core::graph::VirtualChannel,
        _level: f32,
    ) -> Result<(), &'static str> {
        Ok(())
    }
    fn set_volume(
        &self,
        _channel: routune_core::graph::VirtualChannel,
        _volume_db: f32,
    ) -> Result<(), &'static str> {
        Ok(())
    }
    fn set_mute(
        &self,
        _channel: routune_core::graph::VirtualChannel,
        _muted: bool,
    ) -> Result<(), &'static str> {
        Ok(())
    }
    fn set_noise_gate(
        &self,
        _channel: routune_core::graph::VirtualChannel,
        _enabled: bool,
        _threshold_db: f32,
        _attack_ms: f32,
        _release_ms: f32,
    ) -> Result<(), &'static str> {
        Ok(())
    }
    fn set_compressor(
        &self,
        _channel: routune_core::graph::VirtualChannel,
        _enabled: bool,
        _threshold_db: f32,
        _ratio: f32,
        _attack_ms: f32,
        _release_ms: f32,
        _makeup_gain_db: f32,
    ) -> Result<(), &'static str> {
        Ok(())
    }
    fn set_spatial_audio(
        &self,
        _channel: routune_core::graph::VirtualChannel,
        _enabled: bool,
        _mode: routune_core::spatial::SpatialAudioMode,
        _strength: f32,
    ) -> Result<(), &'static str> {
        Ok(())
    }
    fn set_volume_boost(
        &self,
        _channel: routune_core::graph::VirtualChannel,
        _enabled: bool,
        _boost_db: f32,
    ) -> Result<(), &'static str> {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl TelemetryMetricsProvider for crate::pipewire_bridge::PipeWireBridgeService {
    fn get_sink_metrics(&self) -> Vec<proto::SinkMetric> {
        self.get_sink_metrics()
    }

    fn get_topology(&self) -> Option<proto::GraphTopology> {
        Some(self.get_topology())
    }

    fn get_master_metrics(&self) -> Option<(f32, f32)> {
        Some(self.get_master_metrics())
    }

    fn get_channel_metrics(&self) -> Vec<proto::ChannelVolumeMetric> {
        self.get_channel_metrics()
    }

    fn is_active(&self) -> bool {
        self.is_active.load(Ordering::Relaxed)
    }

    fn update_eq(
        &self,
        channel: routune_core::graph::VirtualChannel,
        band_index: usize,
        gain_db: f32,
    ) -> Result<(), &'static str> {
        self.update_eq(channel, band_index, gain_db)
    }

    fn get_active_applications(&self) -> Vec<proto::AppStream> {
        self.get_active_applications()
    }

    fn route_application(
        &self,
        app_id: u32,
        target_channel: proto::AudioChannel,
    ) -> Result<Option<String>, &'static str> {
        self.route_application(app_id, target_channel)
    }

    fn toggle_ai_noise(
        &self,
        channel: routune_core::graph::VirtualChannel,
        enabled: bool,
    ) -> Result<(), &'static str> {
        self.toggle_ai_noise(channel, enabled)
    }

    fn set_ai_noise_level(
        &self,
        channel: routune_core::graph::VirtualChannel,
        level: f32,
    ) -> Result<(), &'static str> {
        self.set_ai_noise_level(channel, level)
    }

    fn set_volume(
        &self,
        channel: routune_core::graph::VirtualChannel,
        volume_db: f32,
    ) -> Result<(), &'static str> {
        self.update_volume(channel, volume_db)
    }

    fn set_mute(
        &self,
        channel: routune_core::graph::VirtualChannel,
        muted: bool,
    ) -> Result<(), &'static str> {
        self.update_mute(channel, muted)
    }

    fn set_noise_gate(
        &self,
        channel: routune_core::graph::VirtualChannel,
        enabled: bool,
        threshold_db: f32,
        attack_ms: f32,
        release_ms: f32,
    ) -> Result<(), &'static str> {
        self.set_noise_gate(channel, enabled, threshold_db, attack_ms, release_ms)
    }

    fn set_compressor(
        &self,
        channel: routune_core::graph::VirtualChannel,
        enabled: bool,
        threshold_db: f32,
        ratio: f32,
        attack_ms: f32,
        release_ms: f32,
        makeup_gain_db: f32,
    ) -> Result<(), &'static str> {
        self.set_compressor(channel, enabled, threshold_db, ratio, attack_ms, release_ms, makeup_gain_db)
    }

    fn set_spatial_audio(
        &self,
        channel: routune_core::graph::VirtualChannel,
        enabled: bool,
        mode: routune_core::spatial::SpatialAudioMode,
        strength: f32,
    ) -> Result<(), &'static str> {
        self.set_spatial_audio(channel, enabled, mode, strength)
    }

    fn set_volume_boost(
        &self,
        channel: routune_core::graph::VirtualChannel,
        enabled: bool,
        boost_db: f32,
    ) -> Result<(), &'static str> {
        self.set_volume_boost(channel, enabled, boost_db)
    }
}

pub struct ControlServiceImpl {
    is_audio_active: Arc<AtomicBool>,
    telemetry_tx: broadcast::Sender<proto::AudioTelemetry>,
    provider: Option<Arc<dyn TelemetryMetricsProvider>>,
    router: Arc<tokio::sync::Mutex<crate::audio::ChannelRouter>>,
    config_manager: Option<Arc<crate::config::ConfigManager>>,
}

impl ControlServiceImpl {
    #[allow(dead_code)]
    pub fn new(
        is_audio_active: Arc<AtomicBool>,
        telemetry_tx: broadcast::Sender<proto::AudioTelemetry>,
        provider: Option<Arc<dyn TelemetryMetricsProvider>>,
        router: Arc<tokio::sync::Mutex<crate::audio::ChannelRouter>>,
    ) -> Self {
        Self::new_with_config(is_audio_active, telemetry_tx, provider, router, None)
    }

    pub fn new_with_config(
        is_audio_active: Arc<AtomicBool>,
        telemetry_tx: broadcast::Sender<proto::AudioTelemetry>,
        provider: Option<Arc<dyn TelemetryMetricsProvider>>,
        router: Arc<tokio::sync::Mutex<crate::audio::ChannelRouter>>,
        config_manager: Option<Arc<crate::config::ConfigManager>>,
    ) -> Self {
        Self {
            is_audio_active,
            telemetry_tx,
            provider,
            router,
            config_manager,
        }
    }
}

#[tonic::async_trait]
impl proto::routune_control_server::RoutuneControl for ControlServiceImpl {
    type StreamTelemetryStream =
        Pin<Box<dyn Stream<Item = Result<proto::AudioTelemetry, Status>> + Send + 'static>>;

    async fn ping(
        &self,
        request: Request<proto::PingRequest>,
    ) -> Result<Response<proto::PingResponse>, Status> {
        let req = request.into_inner();
        debug!("Received ping from client: {:?}", req.client_id);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        let audio_active = self.is_audio_active.load(Ordering::Relaxed)
            || self.provider.as_ref().map(|p| p.is_active()).unwrap_or(false)
            || self.router.lock().await.is_any_active();

        let response = proto::PingResponse {
            message: "pong".to_string(),
            timestamp_ms: now,
            audio_active,
        };

        Ok(Response::new(response))
    }

    async fn stream_telemetry(
        &self,
        request: Request<proto::TelemetryRequest>,
    ) -> Result<Response<Self::StreamTelemetryStream>, Status> {
        let req = request.into_inner();
        debug!("Client requested telemetry stream with target_fps={}", req.target_fps);

        let rx = self.telemetry_tx.subscribe();
        let stream = BroadcastStream::new(rx).filter_map(|res| match res {
            Ok(telemetry) => Some(Ok(telemetry)),
            Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(_)) => None,
        });

        Ok(Response::new(Box::pin(stream)))
    }

    async fn get_hardware_outputs(
        &self,
        _request: Request<proto::Empty>,
    ) -> Result<Response<proto::DeviceList>, Status> {
        let router = self.router.lock().await;
        let devices = router.get_hardware_outputs();
        Ok(Response::new(proto::DeviceList { devices }))
    }

    async fn get_hardware_inputs(
        &self,
        _request: Request<proto::Empty>,
    ) -> Result<Response<proto::DeviceList>, Status> {
        let router = self.router.lock().await;
        let devices = router.get_hardware_inputs();
        Ok(Response::new(proto::DeviceList { devices }))
    }

    async fn set_channel_routing(
        &self,
        request: Request<proto::RouteRequest>,
    ) -> Result<Response<proto::RouteResponse>, Status> {
        let req = request.into_inner();
        let mut router = self.router.lock().await;
        match router.set_channel_routing(req.channel, &req.target_hardware_device_name) {
            Ok(active_name) => {
                if let Some(ref config_manager) = self.config_manager {
                    let v_ch = match proto::AudioChannel::try_from(req.channel) {
                        Ok(proto::AudioChannel::Game) => Some(routune_core::graph::VirtualChannel::Game),
                        Ok(proto::AudioChannel::Chat) => Some(routune_core::graph::VirtualChannel::Chat),
                        Ok(proto::AudioChannel::Media) => Some(routune_core::graph::VirtualChannel::Media),
                        Ok(proto::AudioChannel::Aux) => Some(routune_core::graph::VirtualChannel::Aux),
                        Ok(proto::AudioChannel::Mic) => Some(routune_core::graph::VirtualChannel::Mic),
                        _ => None,
                    };
                    if let Some(ch) = v_ch {
                        config_manager.set_route(ch, &req.target_hardware_device_name).await;
                    }
                }
                Ok(Response::new(proto::RouteResponse {
                    success: true,
                    message: format!("Successfully routed channel to {active_name}"),
                    channel: req.channel,
                    active_device_name: active_name,
                }))
            }
            Err(e) => {
                tracing::warn!("Failed to set channel routing: {e:?}");
                Ok(Response::new(proto::RouteResponse {
                    success: false,
                    message: e.to_string(),
                    channel: req.channel,
                    active_device_name: String::new(),
                }))
            }
        }
    }

    async fn set_eq_band(
        &self,
        request: Request<proto::EqRequest>,
    ) -> Result<Response<proto::Empty>, Status> {
        let req = request.into_inner();
        let virtual_channel = match proto::AudioChannel::try_from(req.channel) {
            Ok(proto::AudioChannel::Master) => routune_core::graph::VirtualChannel::Master,
            Ok(proto::AudioChannel::Game) => routune_core::graph::VirtualChannel::Game,
            Ok(proto::AudioChannel::Chat) => routune_core::graph::VirtualChannel::Chat,
            Ok(proto::AudioChannel::Media) => routune_core::graph::VirtualChannel::Media,
            Ok(proto::AudioChannel::Aux) => routune_core::graph::VirtualChannel::Aux,
            Ok(proto::AudioChannel::Mic) => routune_core::graph::VirtualChannel::Mic,
            Err(_) => return Err(Status::invalid_argument("Invalid audio channel")),
        };

        if req.band_index >= 10 {
            return Err(Status::invalid_argument("band_index must be between 0 and 9"));
        }

        let clamped_gain = req.gain_db.clamp(-12.0, 12.0);
        if let Some(ref provider) = self.provider {
            if let Err(err) = provider.update_eq(virtual_channel, req.band_index as usize, clamped_gain) {
                return Err(Status::internal(format!("Failed to update EQ band: {err}")));
            }
        }

        if let Some(ref config_manager) = self.config_manager {
            config_manager.set_eq_band(virtual_channel, req.band_index as usize, clamped_gain).await;
        }

        Ok(Response::new(proto::Empty {}))
    }

    async fn get_active_applications(
        &self,
        _request: Request<proto::Empty>,
    ) -> Result<Response<proto::AppList>, Status> {
        let apps = if let Some(ref provider) = self.provider {
            provider.get_active_applications()
        } else {
            vec![]
        };
        Ok(Response::new(proto::AppList { apps }))
    }

    async fn route_application(
        &self,
        request: Request<proto::AppRouteRequest>,
    ) -> Result<Response<proto::Empty>, Status> {
        let req = request.into_inner();
        let target_channel = match proto::AudioChannel::try_from(req.target_channel) {
            Ok(ch) => ch,
            Err(_) => return Err(Status::invalid_argument("Invalid audio channel")),
        };

        if let Some(ref provider) = self.provider {
            match provider.route_application(req.app_id, target_channel) {
                Ok(Some(app_ident)) => {
                    if let Some(ref config_manager) = self.config_manager {
                        let ch_str = match target_channel {
                            proto::AudioChannel::Game => "game",
                            proto::AudioChannel::Chat => "chat",
                            proto::AudioChannel::Media => "media",
                            proto::AudioChannel::Aux => "aux",
                            proto::AudioChannel::Mic => "mic",
                            proto::AudioChannel::Master => "master",
                        };
                        config_manager.set_app_route(&app_ident, ch_str).await;
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    return Err(Status::internal(format!("Failed to route application: {err}")));
                }
            }
        }
        Ok(Response::new(proto::Empty {}))
    }

    async fn toggle_ai_noise(
        &self,
        request: Request<proto::AiNoiseRequest>,
    ) -> Result<Response<proto::Empty>, Status> {
        let req = request.into_inner();
        let virtual_channel = match proto::AudioChannel::try_from(req.channel) {
            Ok(proto::AudioChannel::Chat) => routune_core::graph::VirtualChannel::Chat,
            Ok(proto::AudioChannel::Mic) => routune_core::graph::VirtualChannel::Mic,
            Ok(_) => return Ok(Response::new(proto::Empty {})),
            Err(_) => return Err(Status::invalid_argument("Invalid channel")),
        };

        if let Some(ref provider) = self.provider {
            if let Err(err) = provider.toggle_ai_noise(virtual_channel, req.enabled) {
                return Err(Status::internal(format!("Failed to toggle AI noise: {err}")));
            }
            if req.level > 0.0 {
                let _ = provider.set_ai_noise_level(virtual_channel, req.level);
            }
        }

        if let Some(ref config_manager) = self.config_manager {
            config_manager.set_noise_cancellation(virtual_channel, req.enabled).await;
            if req.level > 0.0 {
                config_manager.set_noise_level(req.level).await;
            }
        }

        Ok(Response::new(proto::Empty {}))
    }

    async fn set_ai_noise(
        &self,
        request: Request<proto::AiNoiseRequest>,
    ) -> Result<Response<proto::Empty>, Status> {
        self.toggle_ai_noise(request).await
    }

    async fn set_noise_gate(
        &self,
        request: Request<proto::NoiseGateRequest>,
    ) -> Result<Response<proto::Empty>, Status> {
        let req = request.into_inner();
        let virtual_channel = match proto::AudioChannel::try_from(req.channel) {
            Ok(proto::AudioChannel::Mic) => routune_core::graph::VirtualChannel::Mic,
            Ok(_) => routune_core::graph::VirtualChannel::Mic,
            Err(_) => return Err(Status::invalid_argument("Invalid channel")),
        };
        let cfg = req.config.unwrap_or_default();
        if let Some(ref provider) = self.provider {
            if let Err(err) = provider.set_noise_gate(
                virtual_channel,
                cfg.enabled,
                cfg.threshold_db,
                cfg.attack_ms,
                cfg.release_ms,
            ) {
                return Err(Status::internal(format!("Failed to set noise gate: {err}")));
            }
        }
        if let Some(ref config_manager) = self.config_manager {
            config_manager
                .set_noise_gate(cfg.enabled, cfg.threshold_db, cfg.attack_ms, cfg.release_ms)
                .await;
        }
        Ok(Response::new(proto::Empty {}))
    }

    async fn set_compressor(
        &self,
        request: Request<proto::CompressorRequest>,
    ) -> Result<Response<proto::Empty>, Status> {
        let req = request.into_inner();
        let virtual_channel = match proto::AudioChannel::try_from(req.channel) {
            Ok(proto::AudioChannel::Mic) => routune_core::graph::VirtualChannel::Mic,
            Ok(_) => routune_core::graph::VirtualChannel::Mic,
            Err(_) => return Err(Status::invalid_argument("Invalid channel")),
        };
        let cfg = req.config.unwrap_or_default();
        if let Some(ref provider) = self.provider {
            if let Err(err) = provider.set_compressor(
                virtual_channel,
                cfg.enabled,
                cfg.threshold_db,
                cfg.ratio,
                cfg.attack_ms,
                cfg.release_ms,
                cfg.makeup_gain_db,
            ) {
                return Err(Status::internal(format!("Failed to set compressor: {err}")));
            }
        }
        if let Some(ref config_manager) = self.config_manager {
            config_manager
                .set_compressor(
                    cfg.enabled,
                    cfg.threshold_db,
                    cfg.ratio,
                    cfg.attack_ms,
                    cfg.release_ms,
                    cfg.makeup_gain_db,
                )
                .await;
        }
        Ok(Response::new(proto::Empty {}))
    }

    async fn set_channel_volume(
        &self,
        request: Request<proto::ChannelVolumeRequest>,
    ) -> Result<Response<proto::Empty>, Status> {
        let req = request.into_inner();
        let virtual_channel = match proto::AudioChannel::try_from(req.channel) {
            Ok(proto::AudioChannel::Master) => routune_core::graph::VirtualChannel::Master,
            Ok(proto::AudioChannel::Game) => routune_core::graph::VirtualChannel::Game,
            Ok(proto::AudioChannel::Chat) => routune_core::graph::VirtualChannel::Chat,
            Ok(proto::AudioChannel::Media) => routune_core::graph::VirtualChannel::Media,
            Ok(proto::AudioChannel::Aux) => routune_core::graph::VirtualChannel::Aux,
            Ok(proto::AudioChannel::Mic) => routune_core::graph::VirtualChannel::Mic,
            Err(_) => return Err(Status::invalid_argument("Invalid channel")),
        };

        let clamped_volume = req.volume_db.clamp(-48.0, 12.0);

        if let Some(ref provider) = self.provider {
            if let Err(err) = provider.set_volume(virtual_channel, clamped_volume) {
                return Err(Status::internal(format!("Failed to set channel volume: {err}")));
            }
        }

        if let Some(ref config_manager) = self.config_manager {
            config_manager.set_volume(virtual_channel, clamped_volume).await;
        }

        Ok(Response::new(proto::Empty {}))
    }

    async fn set_channel_mute(
        &self,
        request: Request<proto::ChannelMuteRequest>,
    ) -> Result<Response<proto::Empty>, Status> {
        let req = request.into_inner();
        let virtual_channel = match proto::AudioChannel::try_from(req.channel) {
            Ok(proto::AudioChannel::Master) => routune_core::graph::VirtualChannel::Master,
            Ok(proto::AudioChannel::Game) => routune_core::graph::VirtualChannel::Game,
            Ok(proto::AudioChannel::Chat) => routune_core::graph::VirtualChannel::Chat,
            Ok(proto::AudioChannel::Media) => routune_core::graph::VirtualChannel::Media,
            Ok(proto::AudioChannel::Aux) => routune_core::graph::VirtualChannel::Aux,
            Ok(proto::AudioChannel::Mic) => routune_core::graph::VirtualChannel::Mic,
            Err(_) => return Err(Status::invalid_argument("Invalid channel")),
        };

        if let Some(ref provider) = self.provider {
            if let Err(err) = provider.set_mute(virtual_channel, req.muted) {
                return Err(Status::internal(format!("Failed to set channel mute: {err}")));
            }
        }

        if let Some(ref config_manager) = self.config_manager {
            config_manager.set_mute(virtual_channel, req.muted).await;
        }

        Ok(Response::new(proto::Empty {}))
    }

    async fn set_spatial_audio(
        &self,
        request: Request<proto::SpatialAudioRequest>,
    ) -> Result<Response<proto::Empty>, Status> {
        let req = request.into_inner();
        let virtual_channel = match proto::AudioChannel::try_from(req.channel) {
            Ok(proto::AudioChannel::Game) => routune_core::graph::VirtualChannel::Game,
            Ok(proto::AudioChannel::Media) => routune_core::graph::VirtualChannel::Media,
            Ok(proto::AudioChannel::Aux) => routune_core::graph::VirtualChannel::Aux,
            _ => return Ok(Response::new(proto::Empty {})),
        };

        let cfg = req.config.unwrap_or_default();
        let mode = match proto::SpatialAudioMode::try_from(cfg.mode) {
            Ok(proto::SpatialAudioMode::Speaker) => routune_core::spatial::SpatialAudioMode::Speaker,
            _ => routune_core::spatial::SpatialAudioMode::Headphone,
        };
        let strength = cfg.strength.clamp(0.0, 1.0);

        if let Some(ref provider) = self.provider {
            let _ = provider.set_spatial_audio(virtual_channel, cfg.enabled, mode, strength);
        }

        if let Some(ref config_manager) = self.config_manager {
            config_manager
                .set_spatial_audio(
                    virtual_channel,
                    routune_core::config::SpatialAudioConfig {
                        enabled: cfg.enabled,
                        mode,
                        strength,
                    },
                )
                .await;
        }

        Ok(Response::new(proto::Empty {}))
    }

    async fn set_volume_boost(
        &self,
        request: Request<proto::VolumeBoostRequest>,
    ) -> Result<Response<proto::Empty>, Status> {
        let req = request.into_inner();
        let virtual_channel = match proto::AudioChannel::try_from(req.channel) {
            Ok(proto::AudioChannel::Game) => routune_core::graph::VirtualChannel::Game,
            Ok(proto::AudioChannel::Media) => routune_core::graph::VirtualChannel::Media,
            Ok(proto::AudioChannel::Aux) => routune_core::graph::VirtualChannel::Aux,
            _ => return Ok(Response::new(proto::Empty {})),
        };

        let cfg = req.config.unwrap_or_default();
        let boost_db = cfg.boost_db.clamp(0.0, 12.0);

        if let Some(ref provider) = self.provider {
            let _ = provider.set_volume_boost(virtual_channel, cfg.enabled, boost_db);
        }

        if let Some(ref config_manager) = self.config_manager {
            config_manager
                .set_volume_boost(
                    virtual_channel,
                    routune_core::config::VolumeBoostConfig {
                        enabled: cfg.enabled,
                        boost_db,
                    },
                )
                .await;
        }

        Ok(Response::new(proto::Empty {}))
    }

    async fn get_daemon_config(
        &self,
        _request: Request<proto::Empty>,
    ) -> Result<Response<proto::DaemonConfigResponse>, Status> {
        let cfg = if let Some(ref config_manager) = self.config_manager {
            config_manager.get_config().await
        } else {
            routune_core::config::RoutuneConfig::default()
        };

        let channels = [
            (proto::AudioChannel::Master, routune_core::graph::VirtualChannel::Master),
            (proto::AudioChannel::Game, routune_core::graph::VirtualChannel::Game),
            (proto::AudioChannel::Chat, routune_core::graph::VirtualChannel::Chat),
            (proto::AudioChannel::Media, routune_core::graph::VirtualChannel::Media),
            (proto::AudioChannel::Aux, routune_core::graph::VirtualChannel::Aux),
            (proto::AudioChannel::Mic, routune_core::graph::VirtualChannel::Mic),
        ];

        let mut channel_states = Vec::new();
        for (proto_ch, core_ch) in channels {
            let route = cfg.get_route(core_ch).to_string();
            let eq_bands = cfg.get_eq(core_ch).to_vec();
            let noise_cancellation = cfg.get_noise_cancellation(core_ch);
            let volume_db = cfg.get_volume(core_ch);
            let muted = cfg.get_mute(core_ch);

            let (noise_gate, compressor) = if core_ch == routune_core::graph::VirtualChannel::Mic {
                let g = cfg.get_noise_gate();
                let c = cfg.get_compressor();
                (
                    Some(proto::NoiseGateConfig {
                        enabled: g.enabled,
                        threshold_db: g.threshold_db,
                        attack_ms: g.attack_ms,
                        release_ms: g.release_ms,
                    }),
                    Some(proto::CompressorConfig {
                        enabled: c.enabled,
                        threshold_db: c.threshold_db,
                        ratio: c.ratio,
                        attack_ms: c.attack_ms,
                        release_ms: c.release_ms,
                        makeup_gain_db: c.makeup_gain_db,
                    }),
                )
            } else {
                (None, None)
            };

            let (spatial_audio, volume_boost) = match core_ch {
                routune_core::graph::VirtualChannel::Game
                | routune_core::graph::VirtualChannel::Media
                | routune_core::graph::VirtualChannel::Aux => {
                    let s = cfg.get_spatial_audio(core_ch);
                    let v = cfg.get_volume_boost(core_ch);
                    let s_proto = s.map(|cfg| proto::SpatialAudioConfig {
                        enabled: cfg.enabled,
                        mode: match cfg.mode {
                            routune_core::spatial::SpatialAudioMode::Speaker => {
                                proto::SpatialAudioMode::Speaker as i32
                            }
                            routune_core::spatial::SpatialAudioMode::Headphone => {
                                proto::SpatialAudioMode::Headphone as i32
                            }
                        },
                        strength: cfg.strength,
                    });
                    let v_proto = v.map(|cfg| proto::VolumeBoostConfig {
                        enabled: cfg.enabled,
                        boost_db: cfg.boost_db,
                    });
                    (s_proto, v_proto)
                }
                _ => (None, None),
            };

            channel_states.push(proto::ChannelConfigState {
                channel: proto_ch as i32,
                volume_db,
                muted,
                route_device: route,
                eq_bands,
                noise_cancellation,
                noise_gate,
                compressor,
                ai_noise_level: cfg.get_noise_level(),
                spatial_audio,
                volume_boost,
            });
        }

        Ok(Response::new(proto::DaemonConfigResponse {
            channels: channel_states,
        }))
    }

    async fn set_mic_test(
        &self,
        request: Request<proto::MicTestRequest>,
    ) -> Result<Response<proto::MicTestResponse>, Status> {
        let req = request.into_inner();
        let router = self.router.lock().await;
        router.set_mic_test(req.enabled);
        Ok(Response::new(proto::MicTestResponse {
            enabled: req.enabled,
        }))
    }

    async fn get_mic_test(
        &self,
        _request: Request<proto::Empty>,
    ) -> Result<Response<proto::MicTestResponse>, Status> {
        let router = self.router.lock().await;
        let enabled = router.is_mic_test_enabled();
        Ok(Response::new(proto::MicTestResponse {
            enabled,
        }))
    }
}

pub async fn run_telemetry_dispatcher(
    consumer: Option<TelemetryConsumer>,
    telemetry_tx: broadcast::Sender<proto::AudioTelemetry>,
    provider: Option<Arc<dyn TelemetryMetricsProvider>>,
    router: Option<Arc<tokio::sync::Mutex<crate::audio::ChannelRouter>>>,
) {
    let mut consumer = consumer;
    let mut interval = tokio::time::interval(Duration::from_millis(16));
    let mut seq = 0u64;

    loop {
        interval.tick().await;

        let mut latest_peak = -100.0f32;
        let mut sum_rms_db = 0.0f32;
        let mut sample_count = 0usize;

        if let Some(c_enum) = &mut consumer {
            match c_enum {
                TelemetryConsumer::RingBuffer(c) => {
                    while let Ok(sample) = c.pop() {
                        if sample.peak_volume_db > latest_peak {
                            latest_peak = sample.peak_volume_db;
                        }
                        sum_rms_db += sample.rms_volume_db;
                        sample_count += 1;
                    }
                }
                TelemetryConsumer::Channel(rx) => {
                    while let Ok(sample) = rx.try_recv() {
                        if sample.peak_volume_db > latest_peak {
                            latest_peak = sample.peak_volume_db;
                        }
                        sum_rms_db += sample.rms_volume_db;
                        sample_count += 1;
                    }
                }
            }
        }

        let mut rms_db = if sample_count > 0 {
            sum_rms_db / sample_count as f32
        } else {
            -100.0
        };

        let (sink_metrics, topology, mut channel_metrics) = if let Some(p) = &provider {
            if let Some((m_peak, m_rms)) = p.get_master_metrics() {
                if m_peak > latest_peak {
                    latest_peak = m_peak;
                }
                if m_rms > -99.0 {
                    rms_db = if sample_count > 0 {
                        (rms_db + m_rms) * 0.5
                    } else {
                        m_rms
                    };
                }
            }
            (p.get_sink_metrics(), p.get_topology(), p.get_channel_metrics())
        } else if let Some(r_mutex) = &router {
            if let Ok(r) = r_mutex.try_lock() {
                let (m_peak, m_rms) = r.get_master_metrics();
                if m_peak > latest_peak { latest_peak = m_peak; }
                if m_rms > rms_db { rms_db = m_rms; }
                (Vec::new(), None, r.get_channel_metrics())
            } else {
                (Vec::new(), None, Vec::new())
            }
        } else {
            let fallback_channels = vec![
                proto::ChannelVolumeMetric {
                    channel: proto::AudioChannel::Master as i32,
                    channel_name: "Master".to_string(),
                    peak_volume_db: latest_peak,
                    rms_volume_db: rms_db,
                    ai_noise_enabled: false,
                },
                proto::ChannelVolumeMetric {
                    channel: proto::AudioChannel::Game as i32,
                    channel_name: "Game".to_string(),
                    peak_volume_db: -100.0,
                    rms_volume_db: -100.0,
                    ai_noise_enabled: false,
                },
                proto::ChannelVolumeMetric {
                    channel: proto::AudioChannel::Chat as i32,
                    channel_name: "Chat".to_string(),
                    peak_volume_db: -100.0,
                    rms_volume_db: -100.0,
                    ai_noise_enabled: false,
                },
                proto::ChannelVolumeMetric {
                    channel: proto::AudioChannel::Media as i32,
                    channel_name: "Media".to_string(),
                    peak_volume_db: -100.0,
                    rms_volume_db: -100.0,
                    ai_noise_enabled: false,
                },
                proto::ChannelVolumeMetric {
                    channel: proto::AudioChannel::Aux as i32,
                    channel_name: "Aux".to_string(),
                    peak_volume_db: -100.0,
                    rms_volume_db: -100.0,
                    ai_noise_enabled: false,
                },
                proto::ChannelVolumeMetric {
                    channel: proto::AudioChannel::Mic as i32,
                    channel_name: "Mic".to_string(),
                    peak_volume_db: -100.0,
                    rms_volume_db: -100.0,
                    ai_noise_enabled: false,
                },
            ];
            (Vec::new(), None, fallback_channels)
        };

        // Merge router channel metrics if active
        if let Some(r_mutex) = &router {
            if let Ok(r) = r_mutex.try_lock() {
                let (r_peak, r_rms) = r.get_master_metrics();
                if r_peak > latest_peak { latest_peak = r_peak; }
                if r_rms > rms_db { rms_db = r_rms; }

                for r_ch in r.get_channel_metrics() {
                    if let Some(existing) = channel_metrics.iter_mut().find(|m| m.channel == r_ch.channel) {
                        if r_ch.peak_volume_db > existing.peak_volume_db {
                            existing.peak_volume_db = r_ch.peak_volume_db;
                        }
                        if r_ch.rms_volume_db > existing.rms_volume_db {
                            existing.rms_volume_db = r_ch.rms_volume_db;
                        }
                    }
                }
            }
        }

        let telemetry = proto::AudioTelemetry {
            peak_volume_db: latest_peak,
            rms_volume_db: rms_db,
            spectrum: Vec::new(),
            sequence_number: seq,
            sink_metrics,
            topology,
            channel_metrics,
        };
        seq = seq.wrapping_add(1);

        // Discard error if no active subscribers
        let _ = telemetry_tx.send(telemetry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::routune_control_server::RoutuneControl;

    #[tokio::test]
    async fn test_ping_response() {
        let is_active = Arc::new(AtomicBool::new(true));
        let (tx, _) = broadcast::channel(16);
        let (router, _) = crate::audio::ChannelRouter::new();
        let router = Arc::new(tokio::sync::Mutex::new(router));
        let service = ControlServiceImpl::new(is_active, tx, None, router);

        let req = tonic::Request::new(proto::PingRequest {
            client_id: "test-client".to_string(),
        });
        let res = service.ping(req).await.expect("ping failed").into_inner();

        assert_eq!(res.message, "pong");
        assert!(res.audio_active);
        assert!(res.timestamp_ms > 0);
    }

    #[tokio::test]
    async fn test_stream_telemetry_flow() {
        let is_active = Arc::new(AtomicBool::new(true));
        let (tx, _) = broadcast::channel(16);
        let (router, _) = crate::audio::ChannelRouter::new();
        let router = Arc::new(tokio::sync::Mutex::new(router));
        let service = ControlServiceImpl::new(is_active, tx.clone(), None, router);

        let req = tonic::Request::new(proto::TelemetryRequest { target_fps: 60 });
        let res = service
            .stream_telemetry(req)
            .await
            .expect("stream_telemetry failed")
            .into_inner();

        let mut stream = res;

        // Emit sample from background
        let telemetry_frame = proto::AudioTelemetry {
            peak_volume_db: -3.5,
            rms_volume_db: -9.2,
            spectrum: vec![0.1, 0.5],
            sequence_number: 42,
            sink_metrics: vec![proto::SinkMetric {
                sink_id: "sink-1".to_string(),
                sink_name: "Sink 1".to_string(),
                peak_volume_db: -5.0,
                rms_volume_db: -12.0,
            }],
            topology: Some(proto::GraphTopology {
                nodes: vec![proto::GraphNodeInfo {
                    id: "n1".into(),
                    name: "N1".into(),
                    node_type: "TestNode".into(),
                    bypassed: false,
                    peak_volume_db: -5.0,
                    rms_volume_db: -12.0,
                }],
                edges: vec![],
            }),
            channel_metrics: vec![proto::ChannelVolumeMetric {
                channel: proto::AudioChannel::Master as i32,
                channel_name: "Master".to_string(),
                peak_volume_db: -3.5,
                rms_volume_db: -9.2,
                ai_noise_enabled: false,
            }],
        };
        tx.send(telemetry_frame).expect("send broadcast failed");

        let received = stream.next().await.expect("expected stream item").expect("stream item error");
        assert_eq!(received.sequence_number, 42);
        assert!((received.peak_volume_db - (-3.5)).abs() < 0.001);
        assert_eq!(received.sink_metrics.len(), 1);
        assert_eq!(received.sink_metrics[0].sink_id, "sink-1");
        assert!(received.topology.is_some());
        assert_eq!(received.channel_metrics.len(), 1);
        assert_eq!(received.channel_metrics[0].channel_name, "Master");
    }

    #[tokio::test]
    async fn test_hardware_outputs_and_routing_rpc() {
        let is_active = Arc::new(AtomicBool::new(false));
        let (tx, _) = broadcast::channel(16);
        let (router, _) = crate::audio::ChannelRouter::new();
        let router = Arc::new(tokio::sync::Mutex::new(router));
        let service = ControlServiceImpl::new(is_active, tx, None, router);

        let dev_req = tonic::Request::new(proto::Empty {});
        let dev_res = service.get_hardware_outputs(dev_req).await.unwrap().into_inner();
        println!("Found {} hardware outputs:", dev_res.devices.len());
        for d in &dev_res.devices {
            println!("  id: '{}' | display_name: '{}' | channels: {} | rate: {}", d.id, d.display_name, d.channels, d.sample_rate);
        }

        let route_req = tonic::Request::new(proto::RouteRequest {
            channel: proto::AudioChannel::Game as i32,
            target_hardware_device_name: "none".to_string(),
        });
        let route_res = service.set_channel_routing(route_req).await.unwrap().into_inner();
        assert!(route_res.success);
        assert_eq!(route_res.active_device_name, "Unassigned");
    }

    #[tokio::test]
    async fn test_telemetry_dispatcher_raw_telemetry() {
        let (mut producer, consumer) = rtrb::RingBuffer::new(16);
        let (tx, mut rx) = broadcast::channel(16);

        let dispatcher_handle = tokio::spawn(run_telemetry_dispatcher(Some(consumer.into()), tx, None, None));

        // 1. Send active sample
        producer
            .push(TelemetrySample {
                peak_volume_db: -6.0,
                rms_volume_db: -12.0,
            })
            .expect("push failed");

        // Wait for first telemetry frame with active audio: raw instantaneously sent
        let first = rx.recv().await.expect("receive failed");
        assert!((first.peak_volume_db - (-6.0)).abs() < 0.01);
        assert!((first.rms_volume_db - (-12.0)).abs() < 0.01);

        // 2. Next tick with no samples: immediate raw silence (-100.0 dB) without smoothing or decay hold
        let second = rx.recv().await.expect("receive failed");
        assert_eq!(second.peak_volume_db, -100.0);
        assert_eq!(second.rms_volume_db, -100.0);

        dispatcher_handle.abort();
    }

    #[tokio::test]
    async fn test_set_eq_band_rpc() {
        use proto::routune_control_server::RoutuneControl;

        let is_active = Arc::new(AtomicBool::new(false));
        let (tx, _) = broadcast::channel(16);
        let (router, _) = crate::audio::ChannelRouter::new();
        let router = Arc::new(tokio::sync::Mutex::new(router));

        struct MockEqProvider {
            last_update: std::sync::Mutex<Option<(routune_core::graph::VirtualChannel, usize, f32)>>,
        }

        impl TelemetryMetricsProvider for MockEqProvider {
            fn get_sink_metrics(&self) -> Vec<proto::SinkMetric> { vec![] }
            fn get_topology(&self) -> Option<proto::GraphTopology> { None }
            fn get_master_metrics(&self) -> Option<(f32, f32)> { None }
            fn get_channel_metrics(&self) -> Vec<proto::ChannelVolumeMetric> { vec![] }
            fn is_active(&self) -> bool { false }
            fn update_eq(
                &self,
                channel: routune_core::graph::VirtualChannel,
                band_index: usize,
                gain_db: f32,
            ) -> Result<(), &'static str> {
                *self.last_update.lock().unwrap() = Some((channel, band_index, gain_db));
                Ok(())
            }
        }

        let mock_provider = Arc::new(MockEqProvider {
            last_update: std::sync::Mutex::new(None),
        });

        let service = ControlServiceImpl::new(
            is_active,
            tx,
            Some(mock_provider.clone()),
            router,
        );

        let req = tonic::Request::new(proto::EqRequest {
            channel: proto::AudioChannel::Game as i32,
            band_index: 3,
            gain_db: 4.5,
        });

        let res = service.set_eq_band(req).await;
        assert!(res.is_ok());

        let last = mock_provider.last_update.lock().unwrap().take().unwrap();
        assert_eq!(last.0, routune_core::graph::VirtualChannel::Game);
        assert_eq!(last.1, 3);
        assert!((last.2 - 4.5).abs() < 1e-4);

        // Test invalid band index (> 9)
        let invalid_req = tonic::Request::new(proto::EqRequest {
            channel: proto::AudioChannel::Game as i32,
            band_index: 10,
            gain_db: 0.0,
        });
        let invalid_res = service.set_eq_band(invalid_req).await;
        assert!(invalid_res.is_err());
    }

    #[tokio::test]
    async fn test_set_channel_routing_and_eq_persistence() {
        use proto::routune_control_server::RoutuneControl;
        use std::time::Duration;

        let temp_dir = std::env::temp_dir().join("routune_server_persist_test");
        let cfg_path = temp_dir.join("config.json");
        let _ = std::fs::remove_file(&cfg_path);

        let cfg_mgr = Arc::new(crate::config::ConfigManager::with_path(cfg_path.clone()).unwrap());

        let is_active = Arc::new(AtomicBool::new(false));
        let (tx, _) = broadcast::channel(16);
        let (router, _) = crate::audio::ChannelRouter::new();
        let router = Arc::new(tokio::sync::Mutex::new(router));

        let service = ControlServiceImpl::new_with_config(
            is_active,
            tx,
            None,
            router,
            Some(cfg_mgr.clone()),
        );

        // 1. Test routing persistence (unassigning / setting none)
        let route_req = tonic::Request::new(proto::RouteRequest {
            channel: proto::AudioChannel::Chat as i32,
            target_hardware_device_name: "none".to_string(),
        });
        let route_res = service.set_channel_routing(route_req).await;
        assert!(route_res.is_ok());

        // 2. Test EQ persistence
        let eq_req = tonic::Request::new(proto::EqRequest {
            channel: proto::AudioChannel::Chat as i32,
            band_index: 4,
            gain_db: 6.0,
        });
        let eq_res = service.set_eq_band(eq_req).await;
        assert!(eq_res.is_ok());

        // In-memory check
        let in_memory = cfg_mgr.get_config().await;
        assert_eq!(in_memory.get_route(routune_core::graph::VirtualChannel::Chat), "none");
        assert_eq!(in_memory.get_eq(routune_core::graph::VirtualChannel::Chat)[4], 6.0);

        // 3. Debounced disk write check
        tokio::time::sleep(Duration::from_millis(300)).await;
        let loaded = routune_core::config::RoutuneConfig::load_or_default(&cfg_path);
        assert_eq!(loaded.get_route(routune_core::graph::VirtualChannel::Chat), "none");
        assert_eq!(loaded.get_eq(routune_core::graph::VirtualChannel::Chat)[4], 6.0);

        let _ = std::fs::remove_file(&cfg_path);
        let _ = std::fs::remove_dir(&temp_dir);
    }

    #[tokio::test]
    async fn test_app_routing_and_discovery_rpc() {
        use proto::routune_control_server::RoutuneControl;

        let is_active = Arc::new(AtomicBool::new(false));
        let (tx, _) = broadcast::channel(16);
        let (router, _) = crate::audio::ChannelRouter::new();
        let router = Arc::new(tokio::sync::Mutex::new(router));

        struct MockAppProvider {
            routed: std::sync::Mutex<Option<(u32, proto::AudioChannel)>>,
        }

        impl TelemetryMetricsProvider for MockAppProvider {
            fn get_sink_metrics(&self) -> Vec<proto::SinkMetric> { vec![] }
            fn get_topology(&self) -> Option<proto::GraphTopology> { None }
            fn get_master_metrics(&self) -> Option<(f32, f32)> { None }
            fn get_channel_metrics(&self) -> Vec<proto::ChannelVolumeMetric> { vec![] }
            fn is_active(&self) -> bool { true }
            fn get_active_applications(&self) -> Vec<proto::AppStream> {
                vec![
                    proto::AppStream {
                        app_id: 94,
                        name: "Spotify".to_string(),
                        icon_name: "spotify-client".to_string(),
                        current_sink: proto::AudioChannel::Media as i32,
                        current_sink_name: "Routune Media".to_string(),
                    }
                ]
            }
            fn route_application(
                &self,
                app_id: u32,
                target_channel: proto::AudioChannel,
            ) -> Result<Option<String>, &'static str> {
                *self.routed.lock().unwrap() = Some((app_id, target_channel));
                Ok(Some("Spotify".to_string()))
            }
        }

        let mock_provider = Arc::new(MockAppProvider {
            routed: std::sync::Mutex::new(None),
        });

        let service = ControlServiceImpl::new(
            is_active,
            tx,
            Some(mock_provider.clone()),
            router,
        );

        // 1. Test GetActiveApplications
        let list_res = service.get_active_applications(tonic::Request::new(proto::Empty {})).await;
        assert!(list_res.is_ok());
        let apps = list_res.unwrap().into_inner().apps;
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].app_id, 94);
        assert_eq!(apps[0].name, "Spotify");
        assert_eq!(apps[0].current_sink, proto::AudioChannel::Media as i32);

        // 2. Test RouteApplication
        let route_req = tonic::Request::new(proto::AppRouteRequest {
            app_id: 94,
            target_channel: proto::AudioChannel::Game as i32,
        });
        let route_res = service.route_application(route_req).await;
        assert!(route_res.is_ok());

        let last_route = mock_provider.routed.lock().unwrap().take().unwrap();
        assert_eq!(last_route.0, 94);
        assert_eq!(last_route.1, proto::AudioChannel::Game);

        // 3. Test Invalid Channel
        let invalid_req = tonic::Request::new(proto::AppRouteRequest {
            app_id: 94,
            target_channel: 999,
        });
        let invalid_res = service.route_application(invalid_req).await;
        assert!(invalid_res.is_err());
    }

    #[tokio::test]
    async fn test_toggle_ai_noise_rpc_and_persistence() {
        use proto::routune_control_server::RoutuneControl;
        use std::time::Duration;

        let temp_dir = std::env::temp_dir().join("routune_noise_test");
        let cfg_path = temp_dir.join("config.json");
        let _ = std::fs::remove_file(&cfg_path);

        let cfg_mgr = Arc::new(crate::config::ConfigManager::with_path(cfg_path.clone()).unwrap());

        let is_active = Arc::new(AtomicBool::new(false));
        let (tx, _) = broadcast::channel(16);
        let (router, _) = crate::audio::ChannelRouter::new();
        let router = Arc::new(tokio::sync::Mutex::new(router));

        struct MockNoiseProvider {
            last_noise: std::sync::Mutex<Option<(routune_core::graph::VirtualChannel, bool)>>,
        }

        impl TelemetryMetricsProvider for MockNoiseProvider {
            fn get_sink_metrics(&self) -> Vec<proto::SinkMetric> { vec![] }
            fn get_topology(&self) -> Option<proto::GraphTopology> { None }
            fn get_master_metrics(&self) -> Option<(f32, f32)> { None }
            fn get_channel_metrics(&self) -> Vec<proto::ChannelVolumeMetric> { vec![] }
            fn is_active(&self) -> bool { true }
            fn toggle_ai_noise(
                &self,
                channel: routune_core::graph::VirtualChannel,
                enabled: bool,
            ) -> Result<(), &'static str> {
                *self.last_noise.lock().unwrap() = Some((channel, enabled));
                Ok(())
            }
        }

        let mock_provider = Arc::new(MockNoiseProvider {
            last_noise: std::sync::Mutex::new(None),
        });

        let service = ControlServiceImpl::new_with_config(
            is_active,
            tx,
            Some(mock_provider.clone()),
            router,
            Some(cfg_mgr.clone()),
        );

        let req = tonic::Request::new(proto::AiNoiseRequest {
            channel: proto::AudioChannel::Chat as i32,
            enabled: true,
            level: 0.8,
        });

        let res = service.toggle_ai_noise(req).await;
        assert!(res.is_ok());

        let last = mock_provider.last_noise.lock().unwrap().take().unwrap();
        assert_eq!(last.0, routune_core::graph::VirtualChannel::Chat);
        assert!(last.1);

        tokio::time::sleep(Duration::from_millis(250)).await;

        let saved_cfg = routune_core::config::RoutuneConfig::load_or_default(&cfg_path);
        assert!(saved_cfg.get_noise_cancellation(routune_core::graph::VirtualChannel::Chat));
        assert!(!saved_cfg.get_noise_cancellation(routune_core::graph::VirtualChannel::Game));
        assert!((saved_cfg.get_noise_level() - 0.8).abs() < 1e-4);

        let _ = std::fs::remove_file(&cfg_path);
        let _ = std::fs::remove_dir(&temp_dir);
    }

    #[tokio::test]
    async fn test_channel_volume_and_mute_rpc_and_persistence() {
        use proto::routune_control_server::RoutuneControl;
        use std::time::Duration;

        let temp_dir = std::env::temp_dir().join("routune_vol_test");
        let cfg_path = temp_dir.join("config.json");
        let _ = std::fs::remove_file(&cfg_path);

        let cfg_mgr = Arc::new(crate::config::ConfigManager::with_path(cfg_path.clone()).unwrap());

        let is_active = Arc::new(AtomicBool::new(false));
        let (tx, _) = broadcast::channel(16);
        let (router, _) = crate::audio::ChannelRouter::new();
        let router = Arc::new(tokio::sync::Mutex::new(router));

        struct MockVolProvider {
            last_vol: std::sync::Mutex<Option<(routune_core::graph::VirtualChannel, f32)>>,
            last_mute: std::sync::Mutex<Option<(routune_core::graph::VirtualChannel, bool)>>,
        }

        impl TelemetryMetricsProvider for MockVolProvider {
            fn get_sink_metrics(&self) -> Vec<proto::SinkMetric> { vec![] }
            fn get_topology(&self) -> Option<proto::GraphTopology> { None }
            fn get_master_metrics(&self) -> Option<(f32, f32)> { None }
            fn get_channel_metrics(&self) -> Vec<proto::ChannelVolumeMetric> { vec![] }
            fn is_active(&self) -> bool { true }
            fn set_volume(&self, channel: routune_core::graph::VirtualChannel, vol: f32) -> Result<(), &'static str> {
                *self.last_vol.lock().unwrap() = Some((channel, vol));
                Ok(())
            }
            fn set_mute(&self, channel: routune_core::graph::VirtualChannel, muted: bool) -> Result<(), &'static str> {
                *self.last_mute.lock().unwrap() = Some((channel, muted));
                Ok(())
            }
        }

        let mock_provider = Arc::new(MockVolProvider {
            last_vol: std::sync::Mutex::new(None),
            last_mute: std::sync::Mutex::new(None),
        });

        let service = ControlServiceImpl::new_with_config(
            is_active,
            tx,
            Some(mock_provider.clone()),
            router,
            Some(cfg_mgr),
        );

        // 1. Test SetChannelVolume
        let vol_req = tonic::Request::new(proto::ChannelVolumeRequest {
            channel: proto::AudioChannel::Game as i32,
            volume_db: -6.0,
        });
        let res = service.set_channel_volume(vol_req).await;
        assert!(res.is_ok());

        let (ch, v) = mock_provider.last_vol.lock().unwrap().take().unwrap();
        assert_eq!(ch, routune_core::graph::VirtualChannel::Game);
        assert!((v - (-6.0)).abs() < 0.001);

        // 2. Test SetChannelMute
        let mute_req = tonic::Request::new(proto::ChannelMuteRequest {
            channel: proto::AudioChannel::Game as i32,
            muted: true,
        });
        let res = service.set_channel_mute(mute_req).await;
        assert!(res.is_ok());

        let (ch, m) = mock_provider.last_mute.lock().unwrap().take().unwrap();
        assert_eq!(ch, routune_core::graph::VirtualChannel::Game);
        assert!(m);

        // 3. Test persistence
        tokio::time::sleep(Duration::from_millis(250)).await;
        let saved_cfg = routune_core::config::RoutuneConfig::load_or_default(&cfg_path);
        assert!((saved_cfg.get_volume(routune_core::graph::VirtualChannel::Game) - (-6.0)).abs() < 0.001);
        assert!(saved_cfg.get_mute(routune_core::graph::VirtualChannel::Game));

        // 4. Test GetDaemonConfig RPC
        let config_res = service.get_daemon_config(tonic::Request::new(proto::Empty {})).await;
        assert!(config_res.is_ok());
        let channels = config_res.unwrap().into_inner().channels;
        assert_eq!(channels.len(), 6);
        let game_ch = channels.iter().find(|c| c.channel == proto::AudioChannel::Game as i32).unwrap();
        assert!((game_ch.volume_db - (-6.0)).abs() < 0.001);
        assert!(game_ch.muted);

        let _ = std::fs::remove_file(&cfg_path);
        let _ = std::fs::remove_dir(&temp_dir);
    }

    #[tokio::test]
    async fn test_mic_dynamics_rpc_and_persistence() {
        use proto::routune_control_server::RoutuneControl;
        use std::time::Duration;

        let temp_dir = std::env::temp_dir().join("routune_dynamics_persist_test");
        let cfg_path = temp_dir.join("config.json");
        let _ = std::fs::remove_file(&cfg_path);

        let cfg_mgr = Arc::new(crate::config::ConfigManager::with_path(cfg_path.clone()).unwrap());

        let is_active = Arc::new(AtomicBool::new(false));
        let (tx, _) = broadcast::channel(16);
        let (router, _) = crate::audio::ChannelRouter::new();
        let router = Arc::new(tokio::sync::Mutex::new(router));

        struct MockDynamicsProvider {
            last_gate: std::sync::Mutex<Option<(bool, f32, f32, f32)>>,
            last_comp: std::sync::Mutex<Option<(bool, f32, f32, f32, f32, f32)>>,
        }

        impl TelemetryMetricsProvider for MockDynamicsProvider {
            fn get_sink_metrics(&self) -> Vec<proto::SinkMetric> { vec![] }
            fn get_topology(&self) -> Option<proto::GraphTopology> { None }
            fn get_master_metrics(&self) -> Option<(f32, f32)> { None }
            fn get_channel_metrics(&self) -> Vec<proto::ChannelVolumeMetric> { vec![] }
            fn is_active(&self) -> bool { true }
            fn set_noise_gate(
                &self,
                _channel: routune_core::graph::VirtualChannel,
                enabled: bool,
                threshold_db: f32,
                attack_ms: f32,
                release_ms: f32,
            ) -> Result<(), &'static str> {
                *self.last_gate.lock().unwrap() = Some((enabled, threshold_db, attack_ms, release_ms));
                Ok(())
            }
            fn set_compressor(
                &self,
                _channel: routune_core::graph::VirtualChannel,
                enabled: bool,
                threshold_db: f32,
                ratio: f32,
                attack_ms: f32,
                release_ms: f32,
                makeup_gain_db: f32,
            ) -> Result<(), &'static str> {
                *self.last_comp.lock().unwrap() = Some((enabled, threshold_db, ratio, attack_ms, release_ms, makeup_gain_db));
                Ok(())
            }
        }

        let mock_provider = Arc::new(MockDynamicsProvider {
            last_gate: std::sync::Mutex::new(None),
            last_comp: std::sync::Mutex::new(None),
        });

        let service = ControlServiceImpl::new_with_config(
            is_active,
            tx,
            Some(mock_provider.clone()),
            router,
            Some(cfg_mgr.clone()),
        );

        // 1. Test SetNoiseGate
        let gate_req = tonic::Request::new(proto::NoiseGateRequest {
            channel: proto::AudioChannel::Mic as i32,
            config: Some(proto::NoiseGateConfig {
                enabled: true,
                threshold_db: -35.0,
                attack_ms: 8.0,
                release_ms: 120.0,
            }),
        });
        let res = service.set_noise_gate(gate_req).await;
        assert!(res.is_ok());

        let (gate_en, gthresh, gatt, grel) = mock_provider.last_gate.lock().unwrap().take().unwrap();
        assert!(gate_en);
        assert!((gthresh - (-35.0)).abs() < 0.001);
        assert!((gatt - 8.0).abs() < 0.001);
        assert!((grel - 120.0).abs() < 0.001);

        // 2. Test SetCompressor
        let comp_req = tonic::Request::new(proto::CompressorRequest {
            channel: proto::AudioChannel::Mic as i32,
            config: Some(proto::CompressorConfig {
                enabled: true,
                threshold_db: -16.0,
                ratio: 3.5,
                attack_ms: 15.0,
                release_ms: 150.0,
                makeup_gain_db: 3.0,
            }),
        });
        let res = service.set_compressor(comp_req).await;
        assert!(res.is_ok());

        let (cen, cthresh, crat, catt, crel, cmake) = mock_provider.last_comp.lock().unwrap().take().unwrap();
        assert!(cen);
        assert!((cthresh - (-16.0)).abs() < 0.001);
        assert!((crat - 3.5).abs() < 0.001);
        assert!((catt - 15.0).abs() < 0.001);
        assert!((crel - 150.0).abs() < 0.001);
        assert!((cmake - 3.0).abs() < 0.001);

        // 3. Test persistence
        tokio::time::sleep(Duration::from_millis(250)).await;
        let saved_cfg = routune_core::config::RoutuneConfig::load_or_default(&cfg_path);
        let g = saved_cfg.get_noise_gate();
        assert!(g.enabled);
        assert!((g.threshold_db - (-35.0)).abs() < 0.001);
        let c = saved_cfg.get_compressor();
        assert!(c.enabled);
        assert!((c.ratio - 3.5).abs() < 0.001);

        // 4. Test GetDaemonConfig returns dynamics for mic
        let config_res = service.get_daemon_config(tonic::Request::new(proto::Empty {})).await.unwrap().into_inner();
        let mic_ch = config_res.channels.into_iter().find(|c| c.channel == proto::AudioChannel::Mic as i32).unwrap();
        assert!(mic_ch.noise_gate.is_some());
        assert!(mic_ch.noise_gate.unwrap().enabled);
        assert!(mic_ch.compressor.is_some());
        assert!(mic_ch.compressor.unwrap().enabled);

        let _ = std::fs::remove_file(&cfg_path);
        let _ = std::fs::remove_dir(&temp_dir);
    }

    #[tokio::test]
    async fn test_mic_test_rpc() {
        use proto::routune_control_server::RoutuneControl;

        let is_active = Arc::new(AtomicBool::new(false));
        let (tx, _) = broadcast::channel(16);
        let (router, _) = crate::audio::ChannelRouter::new();
        let router = Arc::new(tokio::sync::Mutex::new(router));

        let service = ControlServiceImpl::new(is_active, tx, None, router.clone());

        // Initial state is false
        let get_res = service.get_mic_test(tonic::Request::new(proto::Empty {})).await.unwrap().into_inner();
        assert!(!get_res.enabled);

        // Enable mic test
        let set_res = service
            .set_mic_test(tonic::Request::new(proto::MicTestRequest { enabled: true }))
            .await
            .unwrap()
            .into_inner();
        assert!(set_res.enabled);

        let get_res = service.get_mic_test(tonic::Request::new(proto::Empty {})).await.unwrap().into_inner();
        assert!(get_res.enabled);

        // Disable mic test
        let set_res = service
            .set_mic_test(tonic::Request::new(proto::MicTestRequest { enabled: false }))
            .await
            .unwrap()
            .into_inner();
        assert!(!set_res.enabled);

        let get_res = service.get_mic_test(tonic::Request::new(proto::Empty {})).await.unwrap().into_inner();
        assert!(!get_res.enabled);
    }

    #[tokio::test]
    async fn test_spatial_audio_and_volume_boost_rpc_and_persistence() {
        use proto::routune_control_server::RoutuneControl;
        use std::time::Duration;

        let temp_dir = std::env::temp_dir().join("routune_spatial_test");
        let cfg_path = temp_dir.join("config.json");
        let _ = std::fs::remove_file(&cfg_path);

        let cfg_mgr = Arc::new(crate::config::ConfigManager::with_path(cfg_path.clone()).unwrap());
        let is_active = Arc::new(AtomicBool::new(false));
        let (tx, _) = broadcast::channel(16);
        let (router, _) = crate::audio::ChannelRouter::new();
        let router = Arc::new(tokio::sync::Mutex::new(router));

        struct MockSpatialProvider {
            last_spatial: std::sync::Mutex<Option<(routune_core::graph::VirtualChannel, bool, routune_core::spatial::SpatialAudioMode, f32)>>,
            last_boost: std::sync::Mutex<Option<(routune_core::graph::VirtualChannel, bool, f32)>>,
        }

        impl TelemetryMetricsProvider for MockSpatialProvider {
            fn get_sink_metrics(&self) -> Vec<proto::SinkMetric> { vec![] }
            fn get_topology(&self) -> Option<proto::GraphTopology> { None }
            fn get_master_metrics(&self) -> Option<(f32, f32)> { None }
            fn get_channel_metrics(&self) -> Vec<proto::ChannelVolumeMetric> { vec![] }
            fn is_active(&self) -> bool { true }
            fn set_spatial_audio(
                &self,
                channel: routune_core::graph::VirtualChannel,
                enabled: bool,
                mode: routune_core::spatial::SpatialAudioMode,
                strength: f32,
            ) -> Result<(), &'static str> {
                *self.last_spatial.lock().unwrap() = Some((channel, enabled, mode, strength));
                Ok(())
            }
            fn set_volume_boost(
                &self,
                channel: routune_core::graph::VirtualChannel,
                enabled: bool,
                boost_db: f32,
            ) -> Result<(), &'static str> {
                *self.last_boost.lock().unwrap() = Some((channel, enabled, boost_db));
                Ok(())
            }
        }

        let mock = Arc::new(MockSpatialProvider {
            last_spatial: std::sync::Mutex::new(None),
            last_boost: std::sync::Mutex::new(None),
        });

        let service = ControlServiceImpl::new_with_config(
            is_active,
            tx,
            Some(mock.clone()),
            router,
            Some(cfg_mgr.clone()),
        );

        // 1. SetSpatialAudio for Game (Headphone mode)
        let s_req = tonic::Request::new(proto::SpatialAudioRequest {
            channel: proto::AudioChannel::Game as i32,
            config: Some(proto::SpatialAudioConfig {
                enabled: true,
                mode: proto::SpatialAudioMode::Headphone as i32,
                strength: 0.85,
            }),
        });
        let res = service.set_spatial_audio(s_req).await;
        assert!(res.is_ok());

        let (ch, en, mode, strg) = mock.last_spatial.lock().unwrap().take().unwrap();
        assert_eq!(ch, routune_core::graph::VirtualChannel::Game);
        assert!(en);
        assert_eq!(mode, routune_core::spatial::SpatialAudioMode::Headphone);
        assert!((strg - 0.85).abs() < 0.001);

        // 2. SetVolumeBoost for Game
        let b_req = tonic::Request::new(proto::VolumeBoostRequest {
            channel: proto::AudioChannel::Game as i32,
            config: Some(proto::VolumeBoostConfig {
                enabled: true,
                boost_db: 7.5,
            }),
        });
        let res = service.set_volume_boost(b_req).await;
        assert!(res.is_ok());

        let (ch, en, b_db) = mock.last_boost.lock().unwrap().take().unwrap();
        assert_eq!(ch, routune_core::graph::VirtualChannel::Game);
        assert!(en);
        assert!((b_db - 7.5).abs() < 0.001);

        // 3. Test persistence
        tokio::time::sleep(Duration::from_millis(250)).await;
        let saved = routune_core::config::RoutuneConfig::load_or_default(&cfg_path);
        let s_saved = saved.get_spatial_audio(routune_core::graph::VirtualChannel::Game).unwrap();
        assert!(s_saved.enabled);
        assert_eq!(s_saved.mode, routune_core::spatial::SpatialAudioMode::Headphone);
        assert!((s_saved.strength - 0.85).abs() < 0.001);

        let b_saved = saved.get_volume_boost(routune_core::graph::VirtualChannel::Game).unwrap();
        assert!(b_saved.enabled);
        assert!((b_saved.boost_db - 7.5).abs() < 0.001);

        // 4. Test GetDaemonConfig returns spatial & boost for Game
        let daemon_cfg = service.get_daemon_config(tonic::Request::new(proto::Empty {})).await.unwrap().into_inner();
        let game_ch = daemon_cfg.channels.into_iter().find(|c| c.channel == proto::AudioChannel::Game as i32).unwrap();
        assert!(game_ch.spatial_audio.is_some());
        assert!(game_ch.spatial_audio.unwrap().enabled);
        assert!(game_ch.volume_boost.is_some());
        assert!(game_ch.volume_boost.unwrap().enabled);

        let _ = std::fs::remove_file(&cfg_path);
        let _ = std::fs::remove_dir(&temp_dir);
    }
}