use anyhow::Result;
use pipewire as pw;
use pw::spa;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use tracing::info;

use routune_core::ai_noise::AiNoiseNode;
use routune_core::config::RoutuneConfig;
use routune_core::graph::{
    calculate_metrics, AudioEngineGraph, AudioNode, DspBypassNode, EqNode, GraphCommand,
    GraphController, PhysicalOutputNode, ResamplerNode, VirtualChannel, VirtualInputNode,
};
use routune_core::spatial::{SpatialAudioMode, SpatialAudioNode, VolumeBoostNode};

use crate::audio::{AudioRingBuffer, ChannelPusher};
use crate::server::proto::{
    AudioChannel, ChannelVolumeMetric, GraphEdgeInfo, GraphNodeInfo, GraphTopology, SinkMetric,
};

pub const SCRATCH_BUFFER_SAMPLES: usize = 4096;

struct BridgeShutdown;

#[derive(Debug, Clone)]
pub struct AppRouteCommand {
    pub app_id: u32,
    pub target_channel: AudioChannel,
}

#[derive(Debug, Clone)]
pub struct DiscoveredApp {
    pub id: u32,
    pub node_ids: Vec<u32>,
    #[allow(dead_code)]
    pub app_key: String,
    pub name: String,
    pub icon_name: String,
    pub target_node: Option<u32>,
    pub target_object: Option<String>,
}

impl DiscoveredApp {
    pub fn to_proto(&self, virtual_sinks: &std::collections::HashMap<String, u32>) -> crate::server::proto::AppStream {
        let (current_sink, current_sink_name) = self.resolve_channel(virtual_sinks);
        crate::server::proto::AppStream {
            app_id: self.id,
            name: self.name.clone(),
            icon_name: self.icon_name.clone(),
            current_sink: current_sink as i32,
            current_sink_name,
        }
    }

    pub fn resolve_channel(&self, virtual_sinks: &std::collections::HashMap<String, u32>) -> (AudioChannel, String) {
        if let Some(ref obj) = self.target_object {
            match obj.as_str() {
                "routune-game" => return (AudioChannel::Game, "Routune Game".to_string()),
                "routune-chat" => return (AudioChannel::Chat, "Routune Chat".to_string()),
                "routune-media" => return (AudioChannel::Media, "Routune Media".to_string()),
                "routune-aux" => return (AudioChannel::Aux, "Routune Aux".to_string()),
                "routune-mic" | "routune-mic-source" | "routune-virtual-mic" => return (AudioChannel::Mic, "Routune Mic".to_string()),
                _ => {}
            }
        }
        if let Some(target_id) = self.target_node {
            for (name, id) in virtual_sinks {
                if *id == target_id {
                    match name.as_str() {
                        "routune-game" => return (AudioChannel::Game, "Routune Game".to_string()),
                        "routune-chat" => return (AudioChannel::Chat, "Routune Chat".to_string()),
                        "routune-media" => return (AudioChannel::Media, "Routune Media".to_string()),
                        "routune-aux" => return (AudioChannel::Aux, "Routune Aux".to_string()),
                        "routune-mic" | "routune-mic-source" | "routune-virtual-mic" => return (AudioChannel::Mic, "Routune Mic".to_string()),
                        _ => {}
                    }
                }
            }
        }
        (AudioChannel::Master, "Default Output".to_string())
    }
}

struct VirtualSinkUserData {
    ring_buf: Arc<AudioRingBuffer>,
    peak_db: Arc<AtomicU32>,
    rms_db: Arc<AtomicU32>,
    is_active: Arc<AtomicBool>,
    noise_enabled: Option<Arc<AtomicBool>>,
    ai_noise_node: Option<AiNoiseNode>,
    spatial_node: Option<SpatialAudioNode>,
    volume_boost_node: Option<VolumeBoostNode>,
    eq_node: EqNode,
    cmd_rx: crossbeam_channel::Receiver<GraphCommand>,
    scratch_buf: Box<[f32; SCRATCH_BUFFER_SAMPLES]>,
    scratch_buf_2: Box<[f32; SCRATCH_BUFFER_SAMPLES]>,
    gain: f32,
    muted: bool,
    master_gain: f32,
    master_muted: bool,
}

struct DesktopMonitorUserData {
    desktop_peak: Arc<AtomicU32>,
    desktop_rms: Arc<AtomicU32>,
    is_active: Arc<AtomicBool>,
}

pub struct PipeWireBridgeService {
    thread_handle: Option<JoinHandle<()>>,
    shutdown_sender: pw::channel::Sender<BridgeShutdown>,
    route_app_tx: pw::channel::Sender<AppRouteCommand>,
    persistent_app_routes: Arc<std::sync::Mutex<std::collections::HashMap<String, AudioChannel>>>,
    discovered_apps: Arc<std::sync::Mutex<std::collections::HashMap<String, DiscoveredApp>>>,
    virtual_sinks: Arc<std::sync::Mutex<std::collections::HashMap<String, u32>>>,
    graph_controller: Arc<std::sync::Mutex<GraphController>>,
    game_eq_tx: crossbeam_channel::Sender<GraphCommand>,
    chat_eq_tx: crossbeam_channel::Sender<GraphCommand>,
    media_eq_tx: crossbeam_channel::Sender<GraphCommand>,
    aux_eq_tx: crossbeam_channel::Sender<GraphCommand>,
    mic_cmd_tx: crossbeam_channel::Sender<GraphCommand>,
    pub is_active: Arc<AtomicBool>,
    pub game_peak: Arc<AtomicU32>,
    pub game_rms: Arc<AtomicU32>,
    pub chat_peak: Arc<AtomicU32>,
    pub chat_rms: Arc<AtomicU32>,
    pub media_peak: Arc<AtomicU32>,
    pub media_rms: Arc<AtomicU32>,
    pub aux_peak: Arc<AtomicU32>,
    pub aux_rms: Arc<AtomicU32>,
    pub mic_peak: Arc<AtomicU32>,
    pub mic_rms: Arc<AtomicU32>,
    pub desktop_peak: Arc<AtomicU32>,
    pub desktop_rms: Arc<AtomicU32>,
    pub chat_noise_enabled: Arc<AtomicBool>,
    pub mic_noise_enabled: Arc<AtomicBool>,
}

impl PipeWireBridgeService {
    #[allow(dead_code)]
    pub fn start(pusher: ChannelPusher) -> anyhow::Result<Self> {
        Self::start_with_config(pusher, None)
    }

    pub fn start_with_config(
        pusher: ChannelPusher,
        config: Option<&RoutuneConfig>,
    ) -> anyhow::Result<Self> {
        pw::init();

        let (cmd_tx, cmd_rx) = crossbeam_channel::bounded(128);
        let (recycle_tx, recycle_rx) = crossbeam_channel::bounded(128);

        let _graph = AudioEngineGraph::new_with_recycler(cmd_rx, Some(recycle_tx), SCRATCH_BUFFER_SAMPLES);
        let mut controller = GraphController::new_with_recycler(cmd_tx, Some(recycle_rx));

        let is_active = Arc::new(AtomicBool::new(false));
        let game_peak = Arc::new(AtomicU32::new((-100.0f32).to_bits()));
        let game_rms = Arc::new(AtomicU32::new((-100.0f32).to_bits()));
        let chat_peak = Arc::new(AtomicU32::new((-100.0f32).to_bits()));
        let chat_rms = Arc::new(AtomicU32::new((-100.0f32).to_bits()));
        let media_peak = Arc::new(AtomicU32::new((-100.0f32).to_bits()));
        let media_rms = Arc::new(AtomicU32::new((-100.0f32).to_bits()));
        let aux_peak = Arc::new(AtomicU32::new((-100.0f32).to_bits()));
        let aux_rms = Arc::new(AtomicU32::new((-100.0f32).to_bits()));
        let desktop_peak = Arc::new(AtomicU32::new((-100.0f32).to_bits()));
        let desktop_rms = Arc::new(AtomicU32::new((-100.0f32).to_bits()));

        let chat_noise_enabled = Arc::new(AtomicBool::new(config.map(|c| c.get_noise_cancellation(VirtualChannel::Chat)).unwrap_or(false)));
        let mic_noise_enabled = pusher.mic_noise_enabled.clone();
        if let Some(cfg) = config {
            mic_noise_enabled.store(cfg.get_noise_cancellation(VirtualChannel::Mic), Ordering::Relaxed);
        }

        let mic_cmd_tx = pusher.mic_cmd_tx.clone();
        let mic_peak = pusher.mic_peak.clone();
        let mic_rms = pusher.mic_rms.clone();

        let (game_eq_tx, game_eq_rx) = crossbeam_channel::bounded(64);
        let (chat_eq_tx, chat_eq_rx) = crossbeam_channel::bounded(64);
        let (media_eq_tx, media_eq_rx) = crossbeam_channel::bounded(64);
        let (aux_eq_tx, aux_eq_rx) = crossbeam_channel::bounded(64);

        // Setup default graph: independent channel chains -> channel-specific PhysicalOutputNode
        let channels: [(&str, &str, Option<(&str, &str)>, &str, &str, &str, &str, &str, &str, &str, &str, VirtualChannel); 5] = [
            ("routune-game", "Routune Game", None, "game-eq", "Game Equalizer", "game-resampler", "Game Resampler", "game-bypass", "Game DSP Bypass", "game-output", "Game Output", VirtualChannel::Game),
            ("routune-chat", "Routune Chat", Some(("chat-noise", "Chat AI Noise")), "chat-eq", "Chat Equalizer", "chat-resampler", "Chat Resampler", "chat-bypass", "Chat DSP Bypass", "chat-output", "Chat Output", VirtualChannel::Chat),
            ("routune-media", "Routune Media", None, "media-eq", "Media Equalizer", "media-resampler", "Media Resampler", "media-bypass", "Media DSP Bypass", "media-output", "Media Output", VirtualChannel::Media),
            ("routune-aux", "Routune Aux", None, "aux-eq", "Aux Equalizer", "aux-resampler", "Aux Resampler", "aux-bypass", "Aux DSP Bypass", "aux-output", "Aux Output", VirtualChannel::Aux),
            ("routune-mic", "Routune Mic", Some(("mic-noise", "Mic AI Noise")), "mic-eq", "Mic Equalizer", "mic-resampler", "Mic Resampler", "mic-bypass", "Mic DSP Bypass", "mic-output", "Mic Output", VirtualChannel::Mic),
        ];

        for (in_id, in_name, noise_info, eq_id, eq_name, resamp_id, resamp_name, bypass_id, bypass_name, out_id, out_name, v_ch) in channels {
            let in_node = VirtualInputNode::new(in_id, in_name, in_id);
            controller.add_node(Box::new(in_node)).map_err(|e| anyhow::anyhow!(e))?;

            let prev_id = if let Some((noise_id, noise_name)) = noise_info {
                let mut noise_node = AiNoiseNode::new(noise_id, noise_name);
                if let Some(cfg) = config {
                    noise_node.set_enabled(cfg.get_noise_cancellation(v_ch));
                    noise_node.set_level(cfg.get_noise_level());
                }
                controller.add_node(Box::new(noise_node)).map_err(|e| anyhow::anyhow!(e))?;
                controller.connect(in_id, noise_id).map_err(|e| anyhow::anyhow!(e))?;
                noise_id
            } else {
                in_id
            };

            let mut eq_node = EqNode::new(eq_id, eq_name, 48000);
            if let Some(cfg) = config {
                let gains = cfg.get_eq(v_ch);
                for (b, &g) in gains.iter().enumerate() {
                    eq_node.set_band_gain(b, g);
                }
            }
            controller.add_node(Box::new(eq_node)).map_err(|e| anyhow::anyhow!(e))?;
            controller.connect(prev_id, eq_id).map_err(|e| anyhow::anyhow!(e))?;

            let mut last_id = eq_id;

            if matches!(v_ch, VirtualChannel::Game | VirtualChannel::Media | VirtualChannel::Aux) {
                let spatial_id = match v_ch {
                    VirtualChannel::Game => "game-spatial",
                    VirtualChannel::Media => "media-spatial",
                    VirtualChannel::Aux => "aux-spatial",
                    _ => unreachable!(),
                };
                let boost_id = match v_ch {
                    VirtualChannel::Game => "game-boost",
                    VirtualChannel::Media => "media-boost",
                    VirtualChannel::Aux => "aux-boost",
                    _ => unreachable!(),
                };
                let mut spatial_node = SpatialAudioNode::new(spatial_id, format!("{v_ch:?} Spatial Audio"));
                let mut boost_node = VolumeBoostNode::new(boost_id, format!("{v_ch:?} Volume Boost"));
                if let Some(cfg) = config {
                    if let Some(s) = cfg.get_spatial_audio(v_ch) {
                        spatial_node.set_enabled(s.enabled);
                        spatial_node.set_mode(s.mode);
                        spatial_node.set_strength(s.strength);
                    }
                    if let Some(b) = cfg.get_volume_boost(v_ch) {
                        boost_node.set_enabled(b.enabled);
                        boost_node.set_boost_db(b.boost_db);
                    }
                }
                controller.add_node(Box::new(spatial_node)).map_err(|e| anyhow::anyhow!(e))?;
                controller.add_node(Box::new(boost_node)).map_err(|e| anyhow::anyhow!(e))?;

                controller.connect(last_id, spatial_id).map_err(|e| anyhow::anyhow!(e))?;
                controller.connect(spatial_id, boost_id).map_err(|e| anyhow::anyhow!(e))?;
                last_id = boost_id;
            }

            let resamp_node = ResamplerNode::new(resamp_id, resamp_name, 48000, 48000, 2, 256)
                .map_err(|e| anyhow::anyhow!("Failed to create {resamp_name}: {:?}", e))?;
            let bypass_node = DspBypassNode::new(bypass_id, bypass_name);
            let target_device = config.map(|c| c.get_route(v_ch)).unwrap_or("unassigned");
            let out_node = PhysicalOutputNode::new(out_id, out_name, target_device);

            controller.add_node(Box::new(resamp_node)).map_err(|e| anyhow::anyhow!(e))?;
            controller.add_node(Box::new(bypass_node)).map_err(|e| anyhow::anyhow!(e))?;
            controller.add_node(Box::new(out_node)).map_err(|e| anyhow::anyhow!(e))?;

            controller.connect(last_id, resamp_id).map_err(|e| anyhow::anyhow!(e))?;
            controller.connect(resamp_id, bypass_id).map_err(|e| anyhow::anyhow!(e))?;
            controller.connect(bypass_id, out_id).map_err(|e| anyhow::anyhow!(e))?;
        }

        // 2. Setup PipeWire channel for shutdown and application routing
        let (shutdown_sender, shutdown_receiver) = pw::channel::channel::<BridgeShutdown>();
        let (route_app_tx, route_app_rx) = pw::channel::channel::<AppRouteCommand>();

        let discovered_apps: Arc<std::sync::Mutex<std::collections::HashMap<String, DiscoveredApp>>> =
            Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let virtual_sinks: Arc<std::sync::Mutex<std::collections::HashMap<String, u32>>> =
            Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));

        let mut initial_app_routes = std::collections::HashMap::new();
        if let Some(cfg) = config {
            for (app, ch_str) in &cfg.app_routes {
                let ch = match ch_str.to_lowercase().as_str() {
                    "game" => AudioChannel::Game,
                    "chat" => AudioChannel::Chat,
                    "media" => AudioChannel::Media,
                    "aux" => AudioChannel::Aux,
                    "mic" => AudioChannel::Mic,
                    _ => AudioChannel::Master,
                };
                initial_app_routes.insert(app.to_lowercase(), ch);
            }
        }
        let persistent_app_routes = Arc::new(std::sync::Mutex::new(initial_app_routes));

        // Rendezvous channel to wait for thread startup outcome
        let (init_tx, init_rx) = std::sync::mpsc::sync_channel::<anyhow::Result<()>>(0);

        let t_game_ring = pusher.game.clone();
        let t_chat_ring = pusher.chat.clone();
        let t_media_ring = pusher.media.clone();
        let t_aux_ring = pusher.aux.clone();

        let t_game_peak = game_peak.clone();
        let t_game_rms = game_rms.clone();
        let t_chat_peak = chat_peak.clone();
        let t_chat_rms = chat_rms.clone();
        let t_media_peak = media_peak.clone();
        let t_media_rms = media_rms.clone();
        let t_aux_peak = aux_peak.clone();
        let t_aux_rms = aux_rms.clone();
        let t_desktop_peak = desktop_peak.clone();
        let t_desktop_rms = desktop_rms.clone();
        let t_is_active = is_active.clone();

        let t_chat_noise_enabled = chat_noise_enabled.clone();

        let t_game_eq_rx = game_eq_rx;
        let t_chat_eq_rx = chat_eq_rx;
        let t_media_eq_rx = media_eq_rx;
        let t_aux_eq_rx = aux_eq_rx;

        let thread_config = config.cloned();

        let t_discovered_apps = discovered_apps.clone();
        let t_virtual_sinks = virtual_sinks.clone();
        let t_persistent_routes = persistent_app_routes.clone();
        let t_route_app_tx_worker = route_app_tx.clone();

        let thread_handle = std::thread::Builder::new()
            .name("pipewire-bridge".into())
            .spawn(move || {
                let mainloop = match pw::main_loop::MainLoopRc::new(None) {
                    Ok(m) => m,
                    Err(e) => {
                        let _ = init_tx.send(Err(anyhow::anyhow!("Failed to create MainLoop: {:?}", e)));
                        return;
                    }
                };

                let context = match pw::context::ContextRc::new(&mainloop, None) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = init_tx.send(Err(anyhow::anyhow!("Failed to create Context: {:?}", e)));
                        return;
                    }
                };

                let core = match context.connect_rc(None) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = init_tx.send(Err(anyhow::anyhow!("Failed to connect to PipeWire core: {:?}", e)));
                        return;
                    }
                };

                let registry = match core.get_registry_rc() {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = init_tx.send(Err(anyhow::anyhow!("Failed to get PipeWire registry: {:?}", e)));
                        return;
                    }
                };

                let metadata_cell: std::rc::Rc<std::cell::RefCell<Option<pw::metadata::Metadata>>> =
                    std::rc::Rc::new(std::cell::RefCell::new(None));
                let metadata_listener_cell: std::rc::Rc<std::cell::RefCell<Option<pw::metadata::MetadataListener>>> =
                    std::rc::Rc::new(std::cell::RefCell::new(None));

                let t_apps_reg = t_discovered_apps.clone();
                let t_sinks_reg = t_virtual_sinks.clone();
                let t_meta_cell = metadata_cell.clone();
                let t_meta_listener_cell = metadata_listener_cell.clone();
                let t_registry = registry.clone();

                let _registry_listener = registry
                    .add_listener_local()
                    .global(move |global| {
                        if global.type_ == pw::types::ObjectType::Metadata {
                            if let Some(props) = &global.props {
                                let dict = props.as_ref();
                                if dict.get("metadata.name") == Some("default") {
                                    match t_registry.bind::<pw::metadata::Metadata, _>(global) {
                                        Ok(meta) => {
                                            info!("Successfully bound to PipeWire 'default' metadata global id {}", global.id);
                                            let prop_apps = t_apps_reg.clone();
                                            let listener = meta
                                                .add_listener_local()
                                                .property(move |subject, key, _type, value| {
                                                    if let Some(key) = key {
                                                        if key == "target.node" || key == "target.object" {
                                                            if let Ok(mut apps) = prop_apps.lock() {
                                                                for app in apps.values_mut() {
                                                                    if app.id == subject || app.node_ids.contains(&subject) {
                                                                        if key == "target.node" {
                                                                            app.target_node = value.and_then(|v| v.parse::<u32>().ok());
                                                                        } else if key == "target.object" {
                                                                            app.target_object = value.map(|v| v.to_string());
                                                                        }
                                                                        break;
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                    0
                                                })
                                                .register();
                                            *t_meta_listener_cell.borrow_mut() = Some(listener);
                                            *t_meta_cell.borrow_mut() = Some(meta);
                                        }
                                        Err(e) => {
                                            tracing::warn!("Failed to bind to metadata global: {:?}", e);
                                        }
                                    }
                                }
                            }
                        } else if global.type_ == pw::types::ObjectType::Device {
                            if let Some(props) = &global.props {
                                let dict = props.as_ref();
                                let desc = dict.get("device.description")
                                    .or_else(|| dict.get("device.nick"))
                                    .unwrap_or("");
                                if !desc.is_empty() {
                                    if let Some(path) = dict.get("object.path") {
                                        let card_id = path.strip_prefix("alsa:acp:").unwrap_or(path);
                                        crate::audio::register_pipewire_device_description(card_id, desc);
                                    }
                                    if let Some(name) = dict.get("device.name") {
                                        crate::audio::register_pipewire_device_description(name, desc);
                                    }
                                }
                            }
                        } else if global.type_ == pw::types::ObjectType::Node {
                            if let Some(props) = &global.props {
                                let dict = props.as_ref();
                                let node_name = dict.get("node.name").unwrap_or("");
                                if node_name == "routune-game" || node_name == "routune-chat" || node_name == "routune-media" || node_name == "routune-aux" || node_name == "routune-mic" || node_name == "routune-mic-source" || node_name == "routune-virtual-mic" {
                                    if let Ok(mut sinks) = t_sinks_reg.lock() {
                                        sinks.insert(node_name.to_string(), global.id);
                                    }
                                }

                                let media_class = dict.get("media.class").unwrap_or("");
                                if media_class == "Stream/Output/Audio" {
                                    let app_name = dict.get("application.name")
                                        .or_else(|| dict.get("window.title"))
                                        .or_else(|| dict.get("media.name"))
                                        .or_else(|| dict.get("node.name"))
                                        .unwrap_or("Unknown App");

                                    let binary = dict.get("application.process.binary");
                                    let pid = dict.get("application.process.id");
                                    let app_key = if let (Some(b), Some(p)) = (binary, pid) {
                                        format!("{b}:{p}")
                                    } else if let Some(b) = binary {
                                        b.to_string()
                                    } else if let Some(p) = pid {
                                        format!("{app_name}:{p}")
                                    } else {
                                        app_name.to_string()
                                    };

                                    if !node_name.starts_with("routune-") && !app_name.starts_with("routune-") {
                                        let icon_name = dict.get("application.icon-name")
                                            .or_else(|| binary)
                                            .unwrap_or("")
                                            .to_string();

                                        let mut target_obj = dict.get("target.object").map(|s| s.to_string());
                                        let mut target_node = dict.get("target.node").and_then(|s| s.parse::<u32>().ok());

                                        // Check if application has a persisted route configuration
                                        let matched_channel = {
                                            if let Ok(routes) = t_persistent_routes.lock() {
                                                let app_name_low = app_name.to_lowercase();
                                                let binary_low = binary.map(|b| b.to_lowercase());
                                                let app_key_low = app_key.to_lowercase();

                                                routes.get(&app_name_low)
                                                    .or_else(|| binary_low.as_ref().and_then(|b| routes.get(b)))
                                                    .or_else(|| routes.get(&app_key_low))
                                                    .copied()
                                            } else {
                                                None
                                            }
                                        };

                                        if let Some(target_ch) = matched_channel {
                                            let sink_name = match target_ch {
                                                AudioChannel::Game => "routune-game",
                                                AudioChannel::Chat => "routune-chat",
                                                AudioChannel::Media => "routune-media",
                                                AudioChannel::Aux => "routune-aux",
                                                AudioChannel::Mic => "routune-virtual-mic",
                                                AudioChannel::Master => "routune-game",
                                            };
                                            let t_id = t_sinks_reg.lock().ok().and_then(|s| s.get(sink_name).copied());
                                            target_obj = Some(sink_name.to_string());
                                            target_node = t_id;

                                            // Auto-route discovered stream to persisted channel
                                            let _ = t_route_app_tx_worker.send(AppRouteCommand {
                                                app_id: global.id,
                                                target_channel: target_ch,
                                            });

                                            let nid = global.id;
                                            let s_name = sink_name.to_string();
                                            let t_id_str = t_id.map(|i| i.to_string());
                                            std::thread::spawn(move || {
                                                if let Some(ref tn) = t_id_str {
                                                    let _ = std::process::Command::new("pw-metadata")
                                                        .args([&nid.to_string(), "target.node", tn, "Spa:Id"])
                                                        .output();
                                                }
                                                let _ = std::process::Command::new("pw-metadata")
                                                    .args([&nid.to_string(), "target.object", &s_name])
                                                    .output();
                                            });
                                        }

                                        if let Ok(mut apps) = t_apps_reg.lock() {
                                            if let Some(existing) = apps.get_mut(&app_key) {
                                                if !existing.node_ids.contains(&global.id) {
                                                    existing.node_ids.push(global.id);
                                                }
                                                if matched_channel.is_some() {
                                                    existing.target_object = target_obj.clone();
                                                    existing.target_node = target_node;
                                                }
                                                // Sync new stream node to existing routing if already routed
                                                if let Some(ref obj) = existing.target_object {
                                                    let obj_clone = obj.clone();
                                                    let t_node = existing.target_node;
                                                    let nid = global.id;
                                                    std::thread::spawn(move || {
                                                        if let Some(tn) = t_node {
                                                            let _ = std::process::Command::new("pw-metadata")
                                                                .args([&nid.to_string(), "target.node", &tn.to_string(), "Spa:Id"])
                                                                .output();
                                                        }
                                                        let _ = std::process::Command::new("pw-metadata")
                                                            .args([&nid.to_string(), "target.object", &obj_clone])
                                                            .output();
                                                    });
                                                }
                                            } else {
                                                apps.insert(app_key.clone(), DiscoveredApp {
                                                    id: global.id,
                                                    node_ids: vec![global.id],
                                                    app_key,
                                                    name: app_name.to_string(),
                                                    icon_name,
                                                    target_node,
                                                    target_object: target_obj,
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    })
                    .global_remove({
                        let t_apps_rem = t_discovered_apps.clone();
                        let t_sinks_rem = t_virtual_sinks.clone();
                        move |id| {
                            if let Ok(mut apps) = t_apps_rem.lock() {
                                apps.retain(|_, app| {
                                    app.node_ids.retain(|&nid| nid != id);
                                    if app.id == id {
                                        if let Some(&first) = app.node_ids.first() {
                                            app.id = first;
                                        }
                                    }
                                    !app.node_ids.is_empty()
                                });
                            }
                            if let Ok(mut sinks) = t_sinks_rem.lock() {
                                sinks.retain(|_, v| *v != id);
                            }
                        }
                    })
                    .register();

                let t_sinks_cmd = t_virtual_sinks.clone();
                let cmd_meta_cell = metadata_cell.clone();
                let _route_app_sub = route_app_rx.attach(mainloop.loop_(), move |cmd: AppRouteCommand| {
                    let target_sink_name = match cmd.target_channel {
                        AudioChannel::Game => "routune-game",
                        AudioChannel::Chat => "routune-chat",
                        AudioChannel::Media => "routune-media",
                        AudioChannel::Aux => "routune-aux",
                        AudioChannel::Mic => "routune-virtual-mic",
                        AudioChannel::Master => "routune-game",
                    };
                    let target_id = t_sinks_cmd.lock().ok().and_then(|s| s.get(target_sink_name).copied());

                    if let Some(ref meta) = *cmd_meta_cell.borrow() {
                        meta.set_property(cmd.app_id, "target.object", None, Some(target_sink_name));
                        if let Some(tid) = target_id {
                            meta.set_property(cmd.app_id, "target.node", Some("Spa:Id"), Some(&tid.to_string()));
                        }
                        info!("PipeWire mainloop set metadata property for app {} -> {} (target node {:?})", cmd.app_id, target_sink_name, target_id);
                    }
                });

                let _shutdown_sub = shutdown_receiver.attach(mainloop.loop_(), {
                    let mainloop = mainloop.clone();
                    move |_| {
                        info!("PipeWire bridge received terminate signal, quitting main loop");
                        mainloop.quit();
                    }
                });

                // Virtual Sink 1: Game
                let game_props = pw::properties::properties! {
                    *pw::keys::MEDIA_TYPE => "Audio",
                    *pw::keys::MEDIA_CATEGORY => "Playback",
                    *pw::keys::MEDIA_CLASS => "Audio/Sink",
                    *pw::keys::NODE_NAME => "routune-game",
                    *pw::keys::NODE_DESCRIPTION => "Routune Game",
                    *pw::keys::AUDIO_CHANNELS => "2",
                    "audio.position" => "[ FL FR ]",
                };
                let game_stream = match pw::stream::StreamBox::new(&core, "routune-game", game_props) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = init_tx.send(Err(anyhow::anyhow!("Failed to create game StreamBox: {:?}", e)));
                        return;
                    }
                };

                // Virtual Sink 2: Chat
                let chat_props = pw::properties::properties! {
                    *pw::keys::MEDIA_TYPE => "Audio",
                    *pw::keys::MEDIA_CATEGORY => "Playback",
                    *pw::keys::MEDIA_CLASS => "Audio/Sink",
                    *pw::keys::NODE_NAME => "routune-chat",
                    *pw::keys::NODE_DESCRIPTION => "Routune Chat",
                    *pw::keys::AUDIO_CHANNELS => "2",
                    "audio.position" => "[ FL FR ]",
                };
                let chat_stream = match pw::stream::StreamBox::new(&core, "routune-chat", chat_props) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = init_tx.send(Err(anyhow::anyhow!("Failed to create chat StreamBox: {:?}", e)));
                        return;
                    }
                };

                // Virtual Sink 3: Media
                let media_props = pw::properties::properties! {
                    *pw::keys::MEDIA_TYPE => "Audio",
                    *pw::keys::MEDIA_CATEGORY => "Playback",
                    *pw::keys::MEDIA_CLASS => "Audio/Sink",
                    *pw::keys::NODE_NAME => "routune-media",
                    *pw::keys::NODE_DESCRIPTION => "Routune Media",
                    *pw::keys::AUDIO_CHANNELS => "2",
                    "audio.position" => "[ FL FR ]",
                };
                let media_stream = match pw::stream::StreamBox::new(&core, "routune-media", media_props) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = init_tx.send(Err(anyhow::anyhow!("Failed to create media StreamBox: {:?}", e)));
                        return;
                    }
                };

                // Virtual Sink 4: Aux
                let aux_props = pw::properties::properties! {
                    *pw::keys::MEDIA_TYPE => "Audio",
                    *pw::keys::MEDIA_CATEGORY => "Playback",
                    *pw::keys::MEDIA_CLASS => "Audio/Sink",
                    *pw::keys::NODE_NAME => "routune-aux",
                    *pw::keys::NODE_DESCRIPTION => "Routune Aux",
                    *pw::keys::AUDIO_CHANNELS => "2",
                    "audio.position" => "[ FL FR ]",
                };
                let aux_stream = match pw::stream::StreamBox::new(&core, "routune-aux", aux_props) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = init_tx.send(Err(anyhow::anyhow!("Failed to create aux StreamBox: {:?}", e)));
                        return;
                    }
                };

                // Desktop Audio Monitor (capturing default physical sink monitor)
                let monitor_props = pw::properties::properties! {
                    *pw::keys::MEDIA_TYPE => "Audio",
                    *pw::keys::MEDIA_CATEGORY => "Monitor",
                    *pw::keys::STREAM_CAPTURE_SINK => "true",
                    *pw::keys::STREAM_MONITOR => "true",
                    *pw::keys::NODE_PASSIVE => "true",
                    *pw::keys::NODE_NAME => "routune-desktop-monitor",
                    *pw::keys::NODE_DESCRIPTION => "Routune Desktop Audio Monitor",
                    *pw::keys::AUDIO_CHANNELS => "2",
                    "audio.position" => "[ FL FR ]",
                };
                let desktop_monitor_stream = match pw::stream::StreamBox::new(&core, "routune-desktop-monitor", monitor_props) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = init_tx.send(Err(anyhow::anyhow!("Failed to create desktop monitor StreamBox: {:?}", e)));
                        return;
                    }
                };

                // Stream audio params: F32LE, 48000 Hz, 2 channels
                let format_bytes = match create_audio_format_pod(48000, 2) {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = init_tx.send(Err(anyhow::anyhow!("Failed to create audio format POD: {:?}", e)));
                        return;
                    }
                };

                // Helper to attach sink listener
                macro_rules! register_sink_listener {
                    ($stream:expr, $ring_buf:expr, $peak:expr, $rms:expr, $active:expr, $noise_enabled:expr, $eq_rx:expr, $noise_info:expr, $eq_id:expr, $eq_name:expr, $v_ch:expr, $name:expr) => {{
                        let (ai_noise_node, noise_enabled_opt) = if let (Some((noise_id, noise_name)), Some(atomic)) = ($noise_info, $noise_enabled) {
                            let mut node = AiNoiseNode::new(noise_id, noise_name);
                            let init_noise = thread_config.as_ref().map(|c| c.get_noise_cancellation($v_ch)).unwrap_or(false);
                            let init_level = thread_config.as_ref().map(|c| c.get_noise_level()).unwrap_or(1.0);
                            node.set_enabled(init_noise);
                            node.set_level(init_level);
                            atomic.store(init_noise, Ordering::Relaxed);
                            (Some(node), Some(atomic))
                        } else {
                            (None, None)
                        };
                        let mut eq_node = EqNode::new($eq_id, $eq_name, 48000);
                        if let Some(ref cfg) = thread_config {
                            let gains = cfg.get_eq($v_ch);
                            for (b, &g) in gains.iter().enumerate() {
                                eq_node.set_band_gain(b, g);
                            }
                        }
                        let init_vol = thread_config.as_ref().map(|c| c.get_volume($v_ch)).unwrap_or(0.0);
                        let init_gain = if init_vol <= -48.0 { 0.0f32 } else { 10.0f32.powf(init_vol / 20.0) };
                        let init_muted = thread_config.as_ref().map(|c| c.get_mute($v_ch)).unwrap_or(false);

                        let init_m_vol = thread_config.as_ref().map(|c| c.get_volume(VirtualChannel::Master)).unwrap_or(0.0);
                        let init_m_gain = if init_m_vol <= -48.0 { 0.0f32 } else { 10.0f32.powf(init_m_vol / 20.0) };
                        let init_m_muted = thread_config.as_ref().map(|c| c.get_mute(VirtualChannel::Master)).unwrap_or(false);

                        let (spatial_node, volume_boost_node) = match $v_ch {
                            VirtualChannel::Game | VirtualChannel::Media | VirtualChannel::Aux => {
                                let mut s_node = SpatialAudioNode::new(format!("{}-spatial", $name), format!("{} Spatial Audio", $eq_name));
                                let mut b_node = VolumeBoostNode::new(format!("{}-boost", $name), format!("{} Volume Boost", $eq_name));
                                if let Some(ref cfg) = thread_config {
                                    if let Some(s) = cfg.get_spatial_audio($v_ch) {
                                        s_node.set_enabled(s.enabled);
                                        s_node.set_mode(s.mode);
                                        s_node.set_strength(s.strength);
                                    }
                                    if let Some(b) = cfg.get_volume_boost($v_ch) {
                                        b_node.set_enabled(b.enabled);
                                        b_node.set_boost_db(b.boost_db);
                                    }
                                }
                                (Some(s_node), Some(b_node))
                            }
                            _ => (None, None),
                        };

                        let user_data = VirtualSinkUserData {
                            ring_buf: $ring_buf,
                            peak_db: $peak,
                            rms_db: $rms,
                            is_active: $active,
                            noise_enabled: noise_enabled_opt,
                            ai_noise_node,
                            spatial_node,
                            volume_boost_node,
                            eq_node,
                            cmd_rx: $eq_rx,
                            scratch_buf: Box::new([0.0f32; SCRATCH_BUFFER_SAMPLES]),
                            scratch_buf_2: Box::new([0.0f32; SCRATCH_BUFFER_SAMPLES]),
                            gain: init_gain,
                            muted: init_muted,
                            master_gain: init_m_gain,
                            master_muted: init_m_muted,
                        };
                        match $stream
                            .add_local_listener_with_user_data(user_data)
                            .process(|stream, data| {
                                while let Ok(cmd) = data.cmd_rx.try_recv() {
                                    match cmd {
                                        GraphCommand::UpdateEq(_channel, band, gain) => {
                                            data.eq_node.set_band_gain(band, gain);
                                        }
                                        GraphCommand::ToggleAiNoise(_channel, enabled) => {
                                            if let Some(ref mut node) = data.ai_noise_node {
                                                node.set_enabled(enabled);
                                            }
                                            if let Some(ref noise_enabled) = data.noise_enabled {
                                                noise_enabled.store(enabled, Ordering::Relaxed);
                                            }
                                        }
                                        GraphCommand::SetAiNoiseLevel(_channel, level) => {
                                            if let Some(ref mut node) = data.ai_noise_node {
                                                node.set_level(level);
                                            }
                                        }
                                        GraphCommand::UpdateSpatialAudio { channel, enabled, mode, strength } => {
                                            if channel == $v_ch {
                                                if let Some(ref mut node) = data.spatial_node {
                                                    node.set_enabled(enabled);
                                                    node.set_mode(mode);
                                                    node.set_strength(strength);
                                                }
                                            }
                                        }
                                        GraphCommand::UpdateVolumeBoost { channel, enabled, boost_db } => {
                                            if channel == $v_ch {
                                                if let Some(ref mut node) = data.volume_boost_node {
                                                    node.set_enabled(enabled);
                                                    node.set_boost_db(boost_db);
                                                }
                                            }
                                        }
                                        GraphCommand::UpdateVolume(channel, volume_db) => {
                                            let linear = if volume_db <= -48.0 {
                                                0.0f32
                                            } else {
                                                10.0f32.powf(volume_db / 20.0)
                                            };
                                            if channel == VirtualChannel::Master {
                                                data.master_gain = linear;
                                            } else {
                                                data.gain = linear;
                                            }
                                        }
                                        GraphCommand::UpdateMute(channel, muted) => {
                                            if channel == VirtualChannel::Master {
                                                data.master_muted = muted;
                                            } else {
                                                data.muted = muted;
                                            }
                                        }
                                        _ => {}
                                    }
                                }

                                if let Some(mut buf) = stream.dequeue_buffer() {
                                    let datas = buf.datas_mut();
                                    if datas.len() >= 2 {
                                        // Planar audio: datas[0] = Left, datas[1] = Right
                                        let (datas_left, datas_right) = datas.split_at_mut(1);
                                        let chunk_l = datas_left[0].chunk();
                                        let off_l = chunk_l.offset() as usize;
                                        let size_l = chunk_l.size() as usize;

                                        let chunk_r = datas_right[0].chunk();
                                        let off_r = chunk_r.offset() as usize;
                                        let size_r = chunk_r.size() as usize;

                                        let slice_l = datas_left[0].data();
                                        let slice_r = datas_right[0].data();

                                        if let (Some(sl_l), Some(sl_r)) = (slice_l, slice_r) {
                                            if off_l + size_l <= sl_l.len() && off_r + size_r <= sl_r.len() && size_l >= 4 && size_r >= 4 {
                                                let (prefix_l, f_l, _) = unsafe { sl_l[off_l..off_l + size_l].align_to::<f32>() };
                                                let (prefix_r, f_r, _) = unsafe { sl_r[off_r..off_r + size_r].align_to::<f32>() };

                                                if prefix_l.is_empty() && prefix_r.is_empty() && !f_l.is_empty() && !f_r.is_empty() {
                                                    let frames = f_l.len().min(f_r.len()).min(SCRATCH_BUFFER_SAMPLES / 2);
                                                    for i in 0..frames {
                                                        data.scratch_buf[i * 2] = f_l[i];
                                                        data.scratch_buf[i * 2 + 1] = f_r[i];
                                                    }
                                                    let len = frames * 2;
                                                    if len >= 2 {
                                                        if let Some(ref mut noise_node) = data.ai_noise_node {
                                                            noise_node.process(&data.scratch_buf[..len], &mut data.scratch_buf_2[..len]);
                                                            data.eq_node.process(&data.scratch_buf_2[..len], &mut data.scratch_buf[..len]);
                                                        } else {
                                                            data.scratch_buf_2[..len].copy_from_slice(&data.scratch_buf[..len]);
                                                            data.eq_node.process(&data.scratch_buf_2[..len], &mut data.scratch_buf[..len]);
                                                        }
                                                        if let Some(ref mut spatial) = data.spatial_node {
                                                            spatial.process(&data.scratch_buf[..len], &mut data.scratch_buf_2[..len]);
                                                            data.scratch_buf[..len].copy_from_slice(&data.scratch_buf_2[..len]);
                                                        }
                                                        if let Some(ref mut boost) = data.volume_boost_node {
                                                            boost.process(&data.scratch_buf[..len], &mut data.scratch_buf_2[..len]);
                                                            data.scratch_buf[..len].copy_from_slice(&data.scratch_buf_2[..len]);
                                                        }

                                                        let effective_gain = if data.muted || data.master_muted {
                                                            0.0f32
                                                        } else {
                                                            data.gain * data.master_gain
                                                        };

                                                        if effective_gain == 0.0 {
                                                            data.scratch_buf[..len].fill(0.0);
                                                        } else if (effective_gain - 1.0).abs() > 0.0001 {
                                                            for s in &mut data.scratch_buf[..len] {
                                                                *s = (*s * effective_gain).clamp(-1.0, 1.0);
                                                            }
                                                        }

                                                        let processed = &data.scratch_buf[..len];

                                                        let (peak, rms) = calculate_metrics(processed);
                                                        data.peak_db.store(peak.to_bits(), Ordering::Relaxed);
                                                        data.rms_db.store(rms.to_bits(), Ordering::Relaxed);
                                                        if peak > -80.0 {
                                                            data.is_active.store(true, Ordering::Relaxed);
                                                        }

                                                        data.ring_buf.push_slice(processed);
                                                    }
                                                }
                                            }
                                        }
                                    } else if !datas.is_empty() {
                                        // Interleaved audio: datas[0] = [L0, R0, L1, R1, ...]
                                        let chunk = datas[0].chunk();
                                        let offset = chunk.offset() as usize;
                                        let size = chunk.size() as usize;
                                        if let Some(slice) = datas[0].data() {
                                            if offset + size <= slice.len() && size >= 4 {
                                                let raw_bytes = &slice[offset..offset + size];
                                                let (prefix, floats, _) = unsafe { raw_bytes.align_to::<f32>() };
                                                if prefix.is_empty() && !floats.is_empty() {
                                                    let len = (floats.len().min(SCRATCH_BUFFER_SAMPLES)) & !1;
                                                    if len >= 2 {
                                                        if let Some(ref mut noise_node) = data.ai_noise_node {
                                                            noise_node.process(&floats[..len], &mut data.scratch_buf_2[..len]);
                                                            data.eq_node.process(&data.scratch_buf_2[..len], &mut data.scratch_buf[..len]);
                                                        } else {
                                                            data.eq_node.process(&floats[..len], &mut data.scratch_buf[..len]);
                                                        }
                                                        if let Some(ref mut spatial) = data.spatial_node {
                                                            spatial.process(&data.scratch_buf[..len], &mut data.scratch_buf_2[..len]);
                                                            data.scratch_buf[..len].copy_from_slice(&data.scratch_buf_2[..len]);
                                                        }
                                                        if let Some(ref mut boost) = data.volume_boost_node {
                                                            boost.process(&data.scratch_buf[..len], &mut data.scratch_buf_2[..len]);
                                                            data.scratch_buf[..len].copy_from_slice(&data.scratch_buf_2[..len]);
                                                        }

                                                        let effective_gain = if data.muted || data.master_muted {
                                                            0.0f32
                                                        } else {
                                                            data.gain * data.master_gain
                                                        };

                                                        if effective_gain == 0.0 {
                                                            data.scratch_buf[..len].fill(0.0);
                                                        } else if (effective_gain - 1.0).abs() > 0.0001 {
                                                            for s in &mut data.scratch_buf[..len] {
                                                                *s = (*s * effective_gain).clamp(-1.0, 1.0);
                                                            }
                                                        }

                                                        let processed = &data.scratch_buf[..len];

                                                        let (peak, rms) = calculate_metrics(processed);
                                                        data.peak_db.store(peak.to_bits(), Ordering::Relaxed);
                                                        data.rms_db.store(rms.to_bits(), Ordering::Relaxed);
                                                        if peak > -80.0 {
                                                            data.is_active.store(true, Ordering::Relaxed);
                                                        }

                                                        data.ring_buf.push_slice(processed);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            })
                            .register()
                        {
                            Ok(l) => l,
                            Err(e) => {
                                let _ = init_tx.send(Err(anyhow::anyhow!("Failed to register {} listener: {:?}", $name, e)));
                                return;
                            }
                        }
                    }};
                }

                let _game_listener = register_sink_listener!(game_stream, t_game_ring, t_game_peak, t_game_rms, t_is_active.clone(), None::<Arc<AtomicBool>>, t_game_eq_rx, None::<(&str, &str)>, "game-eq", "Game Equalizer", VirtualChannel::Game, "game");
                let _chat_listener = register_sink_listener!(chat_stream, t_chat_ring, t_chat_peak, t_chat_rms, t_is_active.clone(), Some(t_chat_noise_enabled), t_chat_eq_rx, Some(("chat-noise", "Chat AI Noise")), "chat-eq", "Chat Equalizer", VirtualChannel::Chat, "chat");
                let _media_listener = register_sink_listener!(media_stream, t_media_ring, t_media_peak, t_media_rms, t_is_active.clone(), None::<Arc<AtomicBool>>, t_media_eq_rx, None::<(&str, &str)>, "media-eq", "Media Equalizer", VirtualChannel::Media, "media");
                let _aux_listener = register_sink_listener!(aux_stream, t_aux_ring, t_aux_peak, t_aux_rms, t_is_active.clone(), None::<Arc<AtomicBool>>, t_aux_eq_rx, None::<(&str, &str)>, "aux-eq", "Aux Equalizer", VirtualChannel::Aux, "aux");

                let monitor_user_data = DesktopMonitorUserData {
                    desktop_peak: t_desktop_peak,
                    desktop_rms: t_desktop_rms,
                    is_active: t_is_active.clone(),
                };

                let _desktop_monitor_listener = match desktop_monitor_stream
                    .add_local_listener_with_user_data(monitor_user_data)
                    .process(|stream, data| {
                        if let Some(mut buf) = stream.dequeue_buffer() {
                            let datas = buf.datas_mut();
                            if !datas.is_empty() {
                                let chunk = datas[0].chunk();
                                let offset = chunk.offset() as usize;
                                let size = chunk.size() as usize;
                                if let Some(slice) = datas[0].data() {
                                    if offset + size <= slice.len() && size >= 4 {
                                        let raw_bytes = &slice[offset..offset + size];
                                        let (prefix, floats, _) = unsafe { raw_bytes.align_to::<f32>() };
                                        if prefix.is_empty() && !floats.is_empty() {
                                            let (peak, rms) = calculate_metrics(floats);
                                            data.desktop_peak.store(peak.to_bits(), Ordering::Relaxed);
                                            data.desktop_rms.store(rms.to_bits(), Ordering::Relaxed);
                                            if peak > -80.0 {
                                                data.is_active.store(true, Ordering::Relaxed);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    })
                    .register()
                {
                    Ok(l) => l,
                    Err(e) => {
                        let _ = init_tx.send(Err(anyhow::anyhow!("Failed to register desktop monitor listener: {:?}", e)));
                        return;
                    }
                };

                // Connect streams
                macro_rules! connect_sink_stream {
                    ($stream:expr, $name:expr) => {{
                        let mut param = match spa::pod::Pod::from_bytes(&format_bytes) {
                            Some(p) => [p],
                            None => {
                                let _ = init_tx.send(Err(anyhow::anyhow!("Failed to construct Pod from format_bytes")));
                                return;
                            }
                        };
                        if let Err(e) = $stream.connect(
                            spa::utils::Direction::Input,
                            None,
                            pw::stream::StreamFlags::MAP_BUFFERS | pw::stream::StreamFlags::RT_PROCESS,
                            &mut param,
                        ) {
                            let _ = init_tx.send(Err(anyhow::anyhow!("Failed to connect {} stream: {:?}", $name, e)));
                            return;
                        }
                    }};
                }

                connect_sink_stream!(game_stream, "game");
                connect_sink_stream!(chat_stream, "chat");
                connect_sink_stream!(media_stream, "media");
                connect_sink_stream!(aux_stream, "aux");

                if let Ok(mut sinks) = t_virtual_sinks.lock() {
                    let g = game_stream.node_id();
                    if g > 0 { sinks.insert("routune-game".to_string(), g); }
                    let c = chat_stream.node_id();
                    if c > 0 { sinks.insert("routune-chat".to_string(), c); }
                    let m = media_stream.node_id();
                    if m > 0 { sinks.insert("routune-media".to_string(), m); }
                    let a = aux_stream.node_id();
                    if a > 0 { sinks.insert("routune-aux".to_string(), a); }
                }

                let mut monitor_param = match spa::pod::Pod::from_bytes(&format_bytes) {
                    Some(p) => [p],
                    None => {
                        let _ = init_tx.send(Err(anyhow::anyhow!("Failed to construct Pod from format_bytes")));
                        return;
                    }
                };
                if let Err(e) = desktop_monitor_stream.connect(
                    spa::utils::Direction::Input,
                    None,
                    pw::stream::StreamFlags::AUTOCONNECT
                        | pw::stream::StreamFlags::MAP_BUFFERS
                        | pw::stream::StreamFlags::RT_PROCESS,
                    &mut monitor_param,
                ) {
                    let _ = init_tx.send(Err(anyhow::anyhow!("Failed to connect desktop monitor stream: {:?}", e)));
                    return;
                }

                info!("PipeWire 4-sink virtual audio bridge successfully initialized and connected");
                let _ = init_tx.send(Ok(()));

                // Run PipeWire loop until shutdown signal
                mainloop.run();
                info!("PipeWire mainloop exited");
            })
            .map_err(|e| anyhow::anyhow!("Failed to spawn PipeWire bridge thread: {:?}", e))?;

        // Wait for thread initialization
        init_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("PipeWire initialization thread hung or died"))??;

        Ok(Self {
            thread_handle: Some(thread_handle),
            shutdown_sender,
            route_app_tx,
            persistent_app_routes,
            discovered_apps,
            virtual_sinks,
            graph_controller: Arc::new(std::sync::Mutex::new(controller)),
            game_eq_tx,
            chat_eq_tx,
            media_eq_tx,
            aux_eq_tx,
            mic_cmd_tx,
            is_active,
            game_peak,
            game_rms,
            chat_peak,
            chat_rms,
            media_peak,
            media_rms,
            aux_peak,
            aux_rms,
            mic_peak,
            mic_rms,
            desktop_peak,
            desktop_rms,
            chat_noise_enabled,
            mic_noise_enabled,
        })
    }

    pub fn get_active_applications(&self) -> Vec<crate::server::proto::AppStream> {
        let sinks = self.virtual_sinks.lock().map(|s| s.clone()).unwrap_or_default();
        if let Ok(apps) = self.discovered_apps.lock() {
            apps.values().map(|app| app.to_proto(&sinks)).collect()
        } else {
            vec![]
        }
    }

    pub fn route_application(
        &self,
        app_id: u32,
        target_channel: AudioChannel,
    ) -> Result<Option<String>, &'static str> {
        let (target_sink_name, friendly_name) = match target_channel {
            AudioChannel::Game => ("routune-game", "Routune Game"),
            AudioChannel::Chat => ("routune-chat", "Routune Chat"),
            AudioChannel::Media => ("routune-media", "Routune Media"),
            AudioChannel::Aux => ("routune-aux", "Routune Aux"),
            AudioChannel::Mic => ("routune-mic", "Routune Mic"),
            AudioChannel::Master => ("routune-game", "Routune Game"),
        };

        let target_id = self.virtual_sinks.lock().ok().and_then(|s| s.get(target_sink_name).copied());

        // 1. Immediately update in-memory cache for all associated streams
        let mut target_node_ids = Vec::new();
        let mut app_identifier = None;
        if let Ok(mut apps) = self.discovered_apps.lock() {
            for app in apps.values_mut() {
                if app.id == app_id || app.node_ids.contains(&app_id) {
                    app.target_object = Some(target_sink_name.to_string());
                    app.target_node = target_id;
                    target_node_ids = app.node_ids.clone();
                    let ident = if !app.name.is_empty() {
                        app.name.clone()
                    } else {
                        app.app_key.split(':').next().unwrap_or(&app.app_key).to_string()
                    };
                    app_identifier = Some(ident);
                    break;
                }
            }
        }
        if target_node_ids.is_empty() {
            target_node_ids.push(app_id);
        }

        if let Some(ref ident) = app_identifier {
            if let Ok(mut persistent) = self.persistent_app_routes.lock() {
                persistent.insert(ident.to_lowercase(), target_channel);
            }
        }

        // 2. Dispatch to PipeWire loop for each stream node
        for &nid in &target_node_ids {
            let cmd = AppRouteCommand {
                app_id: nid,
                target_channel,
            };
            let _ = self.route_app_tx.send(cmd);
        }

        // 3. Fallback CLI command execution to guarantee WirePlumber relinking
        let target_id_str = target_id.map(|i| i.to_string());
        let target_sink_str = target_sink_name.to_string();
        std::thread::spawn(move || {
            for nid in target_node_ids {
                if let Some(ref tid) = target_id_str {
                    let _ = std::process::Command::new("pw-metadata")
                        .args([&nid.to_string(), "target.node", tid, "Spa:Id"])
                        .output();
                }
                let _ = std::process::Command::new("pw-metadata")
                    .args([&nid.to_string(), "target.object", &target_sink_str])
                    .output();
            }
        });

        info!("Routed application {app_id} to {friendly_name} ({target_sink_name}, target node id {target_id:?})");
        Ok(app_identifier)
    }

    pub fn update_eq(
        &self,
        channel: VirtualChannel,
        band_index: usize,
        gain_db: f32,
    ) -> Result<(), &'static str> {
        let cmd = GraphCommand::UpdateEq(channel, band_index, gain_db);
        match channel {
            VirtualChannel::Game => {
                let _ = self.game_eq_tx.try_send(cmd);
            }
            VirtualChannel::Chat => {
                let _ = self.chat_eq_tx.try_send(cmd);
            }
            VirtualChannel::Media => {
                let _ = self.media_eq_tx.try_send(cmd);
            }
            VirtualChannel::Aux => {
                let _ = self.aux_eq_tx.try_send(cmd);
            }
            VirtualChannel::Mic => {
                let _ = self.mic_cmd_tx.try_send(cmd);
            }
            VirtualChannel::Master => {
                let _ = self.game_eq_tx.try_send(GraphCommand::UpdateEq(VirtualChannel::Game, band_index, gain_db));
                let _ = self.chat_eq_tx.try_send(GraphCommand::UpdateEq(VirtualChannel::Chat, band_index, gain_db));
                let _ = self.media_eq_tx.try_send(GraphCommand::UpdateEq(VirtualChannel::Media, band_index, gain_db));
                let _ = self.aux_eq_tx.try_send(GraphCommand::UpdateEq(VirtualChannel::Aux, band_index, gain_db));
            }
        }
        let _ = self.graph_controller.lock().unwrap().update_eq(channel, band_index, gain_db);
        Ok(())
    }

    pub fn toggle_ai_noise(
        &self,
        channel: VirtualChannel,
        enabled: bool,
    ) -> Result<(), &'static str> {
        let cmd = GraphCommand::ToggleAiNoise(channel, enabled);
        match channel {
            VirtualChannel::Chat => {
                self.chat_noise_enabled.store(enabled, Ordering::Relaxed);
                let _ = self.chat_eq_tx.try_send(cmd);
            }
            VirtualChannel::Mic => {
                self.mic_noise_enabled.store(enabled, Ordering::Relaxed);
                let _ = self.mic_cmd_tx.try_send(cmd);
            }
            _ => return Ok(()),
        }
        if let Ok(mut controller) = self.graph_controller.lock() {
            let _ = controller.toggle_ai_noise(channel, enabled);
        }
        Ok(())
    }

    pub fn set_ai_noise_level(
        &self,
        channel: VirtualChannel,
        level: f32,
    ) -> Result<(), &'static str> {
        let cmd = GraphCommand::SetAiNoiseLevel(channel, level);
        match channel {
            VirtualChannel::Chat => {
                let _ = self.chat_eq_tx.try_send(cmd);
            }
            VirtualChannel::Mic => {
                let _ = self.mic_cmd_tx.try_send(cmd);
            }
            _ => return Ok(()),
        }
        if let Ok(mut controller) = self.graph_controller.lock() {
            let _ = controller.set_ai_noise_level(channel, level);
        }
        Ok(())
    }

    pub fn update_volume(&self, channel: VirtualChannel, volume_db: f32) -> Result<(), &'static str> {
        let cmd = GraphCommand::UpdateVolume(channel, volume_db);
        match channel {
            VirtualChannel::Game => {
                let _ = self.game_eq_tx.try_send(cmd);
            }
            VirtualChannel::Chat => {
                let _ = self.chat_eq_tx.try_send(cmd);
            }
            VirtualChannel::Media => {
                let _ = self.media_eq_tx.try_send(cmd);
            }
            VirtualChannel::Aux => {
                let _ = self.aux_eq_tx.try_send(cmd);
            }
            VirtualChannel::Mic => {
                let _ = self.mic_cmd_tx.try_send(cmd);
            }
            VirtualChannel::Master => {
                let _ = self.game_eq_tx.try_send(GraphCommand::UpdateVolume(VirtualChannel::Master, volume_db));
                let _ = self.chat_eq_tx.try_send(GraphCommand::UpdateVolume(VirtualChannel::Master, volume_db));
                let _ = self.media_eq_tx.try_send(GraphCommand::UpdateVolume(VirtualChannel::Master, volume_db));
                let _ = self.aux_eq_tx.try_send(GraphCommand::UpdateVolume(VirtualChannel::Master, volume_db));
            }
        }
        if let Ok(mut controller) = self.graph_controller.lock() {
            let _ = controller.update_volume(channel, volume_db);
        }
        Ok(())
    }

    pub fn update_mute(&self, channel: VirtualChannel, muted: bool) -> Result<(), &'static str> {
        let cmd = GraphCommand::UpdateMute(channel, muted);
        match channel {
            VirtualChannel::Game => {
                let _ = self.game_eq_tx.try_send(cmd);
            }
            VirtualChannel::Chat => {
                let _ = self.chat_eq_tx.try_send(cmd);
            }
            VirtualChannel::Media => {
                let _ = self.media_eq_tx.try_send(cmd);
            }
            VirtualChannel::Aux => {
                let _ = self.aux_eq_tx.try_send(cmd);
            }
            VirtualChannel::Mic => {
                let _ = self.mic_cmd_tx.try_send(cmd);
            }
            VirtualChannel::Master => {
                let _ = self.game_eq_tx.try_send(GraphCommand::UpdateMute(VirtualChannel::Master, muted));
                let _ = self.chat_eq_tx.try_send(GraphCommand::UpdateMute(VirtualChannel::Master, muted));
                let _ = self.media_eq_tx.try_send(GraphCommand::UpdateMute(VirtualChannel::Master, muted));
                let _ = self.aux_eq_tx.try_send(GraphCommand::UpdateMute(VirtualChannel::Master, muted));
            }
        }
        if let Ok(mut controller) = self.graph_controller.lock() {
            let _ = controller.update_mute(channel, muted);
        }
        Ok(())
    }

    pub fn set_noise_gate(
        &self,
        channel: VirtualChannel,
        enabled: bool,
        threshold_db: f32,
        attack_ms: f32,
        release_ms: f32,
    ) -> Result<(), &'static str> {
        let cmd = GraphCommand::UpdateNoiseGate {
            channel,
            enabled,
            threshold_db,
            attack_ms,
            release_ms,
        };
        if channel == VirtualChannel::Mic {
            let _ = self.mic_cmd_tx.try_send(cmd);
        }
        if let Ok(mut controller) = self.graph_controller.lock() {
            let _ = controller.set_noise_gate(channel, enabled, threshold_db, attack_ms, release_ms);
        }
        Ok(())
    }

    pub fn set_compressor(
        &self,
        channel: VirtualChannel,
        enabled: bool,
        threshold_db: f32,
        ratio: f32,
        attack_ms: f32,
        release_ms: f32,
        makeup_gain_db: f32,
    ) -> Result<(), &'static str> {
        let cmd = GraphCommand::UpdateCompressor {
            channel,
            enabled,
            threshold_db,
            ratio,
            attack_ms,
            release_ms,
            makeup_gain_db,
        };
        if channel == VirtualChannel::Mic {
            let _ = self.mic_cmd_tx.try_send(cmd);
        }
        if let Ok(mut controller) = self.graph_controller.lock() {
            let _ = controller.set_compressor(
                channel,
                enabled,
                threshold_db,
                ratio,
                attack_ms,
                release_ms,
                makeup_gain_db,
            );
        }
        Ok(())
    }

    pub fn set_spatial_audio(
        &self,
        channel: VirtualChannel,
        enabled: bool,
        mode: SpatialAudioMode,
        strength: f32,
    ) -> Result<(), &'static str> {
        let cmd = GraphCommand::UpdateSpatialAudio {
            channel,
            enabled,
            mode,
            strength,
        };
        match channel {
            VirtualChannel::Game => {
                let _ = self.game_eq_tx.try_send(cmd);
            }
            VirtualChannel::Media => {
                let _ = self.media_eq_tx.try_send(cmd);
            }
            VirtualChannel::Aux => {
                let _ = self.aux_eq_tx.try_send(cmd);
            }
            _ => return Ok(()),
        }
        if let Ok(mut controller) = self.graph_controller.lock() {
            let _ = controller.set_spatial_audio(channel, enabled, mode, strength);
        }
        Ok(())
    }

    pub fn set_volume_boost(
        &self,
        channel: VirtualChannel,
        enabled: bool,
        boost_db: f32,
    ) -> Result<(), &'static str> {
        let cmd = GraphCommand::UpdateVolumeBoost {
            channel,
            enabled,
            boost_db,
        };
        match channel {
            VirtualChannel::Game => {
                let _ = self.game_eq_tx.try_send(cmd);
            }
            VirtualChannel::Media => {
                let _ = self.media_eq_tx.try_send(cmd);
            }
            VirtualChannel::Aux => {
                let _ = self.aux_eq_tx.try_send(cmd);
            }
            _ => return Ok(()),
        }
        if let Ok(mut controller) = self.graph_controller.lock() {
            let _ = controller.set_volume_boost(channel, enabled, boost_db);
        }
        Ok(())
    }

    pub fn get_sink_metrics(&self) -> Vec<SinkMetric> {
        vec![
            SinkMetric {
                sink_id: "routune-game".to_string(),
                sink_name: "Routune Game".to_string(),
                peak_volume_db: f32::from_bits(self.game_peak.load(Ordering::Relaxed)),
                rms_volume_db: f32::from_bits(self.game_rms.load(Ordering::Relaxed)),
            },
            SinkMetric {
                sink_id: "routune-chat".to_string(),
                sink_name: "Routune Chat".to_string(),
                peak_volume_db: f32::from_bits(self.chat_peak.load(Ordering::Relaxed)),
                rms_volume_db: f32::from_bits(self.chat_rms.load(Ordering::Relaxed)),
            },
            SinkMetric {
                sink_id: "routune-media".to_string(),
                sink_name: "Routune Media".to_string(),
                peak_volume_db: f32::from_bits(self.media_peak.load(Ordering::Relaxed)),
                rms_volume_db: f32::from_bits(self.media_rms.load(Ordering::Relaxed)),
            },
            SinkMetric {
                sink_id: "routune-aux".to_string(),
                sink_name: "Routune Aux".to_string(),
                peak_volume_db: f32::from_bits(self.aux_peak.load(Ordering::Relaxed)),
                rms_volume_db: f32::from_bits(self.aux_rms.load(Ordering::Relaxed)),
            },
            SinkMetric {
                sink_id: "routune-mic".to_string(),
                sink_name: "Routune Mic".to_string(),
                peak_volume_db: f32::from_bits(self.mic_peak.load(Ordering::Relaxed)),
                rms_volume_db: f32::from_bits(self.mic_rms.load(Ordering::Relaxed)),
            },
        ]
    }

    pub fn get_channel_metrics(&self) -> Vec<ChannelVolumeMetric> {
        let (m_peak, m_rms) = self.get_master_metrics();
        vec![
            ChannelVolumeMetric {
                channel: AudioChannel::Master as i32,
                channel_name: "Master".to_string(),
                peak_volume_db: m_peak,
                rms_volume_db: m_rms,
                ai_noise_enabled: false,
            },
            ChannelVolumeMetric {
                channel: AudioChannel::Game as i32,
                channel_name: "Game".to_string(),
                peak_volume_db: Self::read_decay_metric(&self.game_peak),
                rms_volume_db: f32::from_bits(self.game_rms.load(Ordering::Relaxed)),
                ai_noise_enabled: false,
            },
            ChannelVolumeMetric {
                channel: AudioChannel::Chat as i32,
                channel_name: "Chat".to_string(),
                peak_volume_db: Self::read_decay_metric(&self.chat_peak),
                rms_volume_db: f32::from_bits(self.chat_rms.load(Ordering::Relaxed)),
                ai_noise_enabled: self.chat_noise_enabled.load(Ordering::Relaxed),
            },
            ChannelVolumeMetric {
                channel: AudioChannel::Media as i32,
                channel_name: "Media".to_string(),
                peak_volume_db: Self::read_decay_metric(&self.media_peak),
                rms_volume_db: f32::from_bits(self.media_rms.load(Ordering::Relaxed)),
                ai_noise_enabled: false,
            },
            ChannelVolumeMetric {
                channel: AudioChannel::Aux as i32,
                channel_name: "Aux".to_string(),
                peak_volume_db: Self::read_decay_metric(&self.aux_peak),
                rms_volume_db: f32::from_bits(self.aux_rms.load(Ordering::Relaxed)),
                ai_noise_enabled: false,
            },
            ChannelVolumeMetric {
                channel: AudioChannel::Mic as i32,
                channel_name: "Mic".to_string(),
                peak_volume_db: Self::read_decay_metric(&self.mic_peak),
                rms_volume_db: f32::from_bits(self.mic_rms.load(Ordering::Relaxed)),
                ai_noise_enabled: self.mic_noise_enabled.load(Ordering::Relaxed),
            },
        ]
    }

    fn read_decay_metric(atomic: &AtomicU32) -> f32 {
        let val = f32::from_bits(atomic.load(Ordering::Relaxed));
        // Decay peak slightly each tick toward silence to prevent infinite freeze when stream stops
        let decayed = if val > -95.0 { (val - 2.0).max(-100.0) } else { -100.0 };
        atomic.store(decayed.to_bits(), Ordering::Relaxed);
        val
    }

    pub fn get_master_metrics(&self) -> (f32, f32) {
        let mut p = -100.0f32;
        let mut r = -100.0f32;

        for peak_atom in [&self.game_peak, &self.chat_peak, &self.media_peak, &self.aux_peak, &self.desktop_peak] {
            let ch_p = Self::read_decay_metric(peak_atom);
            if ch_p > p {
                p = ch_p;
            }
        }
        for rms_atom in [&self.game_rms, &self.chat_rms, &self.media_rms, &self.aux_rms, &self.desktop_rms] {
            let ch_r = f32::from_bits(rms_atom.load(Ordering::Relaxed));
            if ch_r > r {
                r = ch_r;
            }
        }

        (p, r)
    }

    pub fn get_topology(&self) -> GraphTopology {
        let controller = self.graph_controller.lock().unwrap();
        let nodes = controller
            .node_infos()
            .iter()
            .map(|info| {
                let (peak, rms) = match info.id.as_str() {
                    "routune-game" | "game-noise" | "game-eq" | "game-spatial" | "game-boost" | "game-output" => (
                        f32::from_bits(self.game_peak.load(Ordering::Relaxed)),
                        f32::from_bits(self.game_rms.load(Ordering::Relaxed)),
                    ),
                    "routune-chat" | "chat-noise" | "chat-eq" | "chat-output" => (
                        f32::from_bits(self.chat_peak.load(Ordering::Relaxed)),
                        f32::from_bits(self.chat_rms.load(Ordering::Relaxed)),
                    ),
                    "routune-media" | "media-noise" | "media-eq" | "media-spatial" | "media-boost" | "media-output" => (
                        f32::from_bits(self.media_peak.load(Ordering::Relaxed)),
                        f32::from_bits(self.media_rms.load(Ordering::Relaxed)),
                    ),
                    "routune-aux" | "aux-noise" | "aux-eq" | "aux-spatial" | "aux-boost" | "aux-output" => (
                        f32::from_bits(self.aux_peak.load(Ordering::Relaxed)),
                        f32::from_bits(self.aux_rms.load(Ordering::Relaxed)),
                    ),
                    _ => (-100.0, -100.0),
                };

                GraphNodeInfo {
                    id: info.id.clone(),
                    name: info.name.clone(),
                    node_type: info.node_type.clone(),
                    bypassed: info.bypassed,
                    peak_volume_db: peak,
                    rms_volume_db: rms,
                }
            })
            .collect();

        let edges = controller
            .active_edges()
            .iter()
            .map(|(from, to)| GraphEdgeInfo {
                from_node: from.clone(),
                to_node: to.clone(),
            })
            .collect();

        GraphTopology { nodes, edges }
    }

    pub fn shutdown(mut self) {
        if self.thread_handle.is_some() {
            let _ = self.shutdown_sender.send(BridgeShutdown);
            if let Some(handle) = self.thread_handle.take() {
                let _ = handle.join();
            }
        }
    }
}

impl Drop for PipeWireBridgeService {
    fn drop(&mut self) {
        if self.thread_handle.is_some() {
            let _ = self.shutdown_sender.send(BridgeShutdown);
            if let Some(handle) = self.thread_handle.take() {
                let _ = handle.join();
            }
        }
    }
}

fn create_audio_format_pod(rate: u32, channels: u32) -> Result<Vec<u8>> {
    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(spa::param::audio::AudioFormat::F32LE);
    audio_info.set_rate(rate);
    audio_info.set_channels(channels);

    let mut pos = [spa::sys::SPA_AUDIO_CHANNEL_UNKNOWN; spa::sys::SPA_AUDIO_MAX_CHANNELS as usize];
    if channels == 1 {
        pos[0] = spa::sys::SPA_AUDIO_CHANNEL_MONO;
    } else if channels >= 2 {
        pos[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
        pos[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;
    }
    audio_info.set_position(pos);

    let obj = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };

    let serialized = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    )
    .map_err(|e| anyhow::anyhow!("Failed to serialize format POD: {:?}", e))?
    .0
    .into_inner();

    Ok(serialized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use spa::pod::Pod;

    #[test]
    fn test_desktop_monitor_stream_builder() {
        if std::env::var("CI").is_ok() {
            return;
        }

        pw::init();
        let mainloop = match pw::main_loop::MainLoopRc::new(None) {
            Ok(m) => m,
            Err(_) => return,
        };
        let context = match pw::context::ContextRc::new(&mainloop, None) {
            Ok(c) => c,
            Err(_) => return,
        };
        let core = match context.connect_rc(None) {
            Ok(c) => c,
            Err(_) => return,
        };

        let monitor_props = pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Monitor",
            *pw::keys::STREAM_CAPTURE_SINK => "true",
            *pw::keys::STREAM_MONITOR => "true",
            *pw::keys::NODE_NAME => "routune-test-monitor",
            *pw::keys::NODE_DESCRIPTION => "Routune Test Desktop Monitor",
            *pw::keys::AUDIO_CHANNELS => "2",
        };

        let stream = pw::stream::StreamBox::new(&core, "routune-test-monitor", monitor_props)
            .expect("StreamBox failed");

        let format_bytes = create_audio_format_pod(48000, 2).expect("format pod failed");
        let mut param = [Pod::from_bytes(&format_bytes).expect("pod failed")];

        let res = stream.connect(
            spa::utils::Direction::Input,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut param,
        );

        assert!(res.is_ok(), "Connecting desktop monitor stream failed: {:?}", res.err());
    }

    #[test]
    fn test_create_audio_format_pod() {
        let pod_bytes = create_audio_format_pod(48000, 2).expect("Must serialize format pod");
        assert!(!pod_bytes.is_empty());
        let pod = Pod::from_bytes(&pod_bytes);
        assert!(pod.is_some());
    }

    #[test]
    fn test_pipewire_bridge_service_lifecycle() {
        if std::env::var("CI").is_ok() {
            return;
        }

        let pusher = ChannelPusher::default();
        match PipeWireBridgeService::start(pusher) {
            Ok(service) => {
                let metrics = service.get_sink_metrics();
                assert_eq!(metrics.len(), 5);
                assert_eq!(metrics[0].sink_id, "routune-game");
                assert_eq!(metrics[1].sink_id, "routune-chat");
                assert_eq!(metrics[2].sink_id, "routune-media");
                assert_eq!(metrics[3].sink_id, "routune-aux");
                assert_eq!(metrics[4].sink_id, "routune-mic");

                let channel_metrics = service.get_channel_metrics();
                assert_eq!(channel_metrics.len(), 6);
                assert_eq!(channel_metrics[0].channel_name, "Master");
                assert_eq!(channel_metrics[1].channel_name, "Game");
                assert_eq!(channel_metrics[2].channel_name, "Chat");
                assert_eq!(channel_metrics[3].channel_name, "Media");
                assert_eq!(channel_metrics[4].channel_name, "Aux");
                assert_eq!(channel_metrics[5].channel_name, "Mic");

                let topology = service.get_topology();
                assert_eq!(topology.nodes.len(), 33);
                assert_eq!(topology.edges.len(), 28);

                let eq_res = service.update_eq(VirtualChannel::Game, 3, 5.0);
                assert!(eq_res.is_ok());

                let noise_res = service.toggle_ai_noise(VirtualChannel::Game, true);
                assert!(noise_res.is_ok());

                service.shutdown();
            }
            Err(err) => {
                // If PipeWire daemon is not running in test runner, skip
                eprintln!("PipeWire daemon not available in test environment: {err}");
            }
        }
    }

    #[test]
    fn test_discovered_app_resolution() {
        let mut sinks = std::collections::HashMap::new();
        sinks.insert("routune-game".to_string(), 82);
        sinks.insert("routune-chat".to_string(), 84);
        sinks.insert("routune-media".to_string(), 89);
        sinks.insert("routune-aux".to_string(), 106);

        // 1. Resolve via target_object
        let app1 = DiscoveredApp {
            id: 99,
            node_ids: vec![99],
            app_key: "spotify:1234".to_string(),
            name: "Spotify".to_string(),
            icon_name: "spotify-client".to_string(),
            target_node: None,
            target_object: Some("routune-media".to_string()),
        };
        let proto1 = app1.to_proto(&sinks);
        assert_eq!(proto1.app_id, 99);
        assert_eq!(proto1.name, "Spotify");
        assert_eq!(proto1.icon_name, "spotify-client");
        assert_eq!(proto1.current_sink, AudioChannel::Media as i32);
        assert_eq!(proto1.current_sink_name, "Routune Media");

        // 2. Resolve via target_node
        let app2 = DiscoveredApp {
            id: 101,
            node_ids: vec![101, 105],
            app_key: "vesktop:9058".to_string(),
            name: "Discord".to_string(),
            icon_name: "discord".to_string(),
            target_node: Some(84),
            target_object: None,
        };
        let proto2 = app2.to_proto(&sinks);
        assert_eq!(proto2.current_sink, AudioChannel::Chat as i32);
        assert_eq!(proto2.current_sink_name, "Routune Chat");

        // 3. Fallback default
        let app3 = DiscoveredApp {
            id: 102,
            node_ids: vec![102],
            app_key: "game:4444".to_string(),
            name: "GameApp".to_string(),
            icon_name: "".to_string(),
            target_node: None,
            target_object: None,
        };
        let proto3 = app3.to_proto(&sinks);
        assert_eq!(proto3.current_sink, AudioChannel::Master as i32);
        assert_eq!(proto3.current_sink_name, "Default Output");
    }
}
