use anyhow::Result;
use pipewire as pw;
use pw::spa;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use tracing::info;

use routune_core::graph::{
    calculate_metrics, AudioEngineGraph, DspBypassNode, GraphController,
    PhysicalOutputNode, ResamplerNode, VirtualInputNode,
};

use crate::audio::ChannelPusher;
use crate::server::proto::{
    AudioChannel, ChannelVolumeMetric, GraphEdgeInfo, GraphNodeInfo, GraphTopology, SinkMetric,
};

pub const SCRATCH_BUFFER_SAMPLES: usize = 4096;

struct BridgeShutdown;

struct VirtualSinkUserData {
    pusher: Arc<std::sync::Mutex<Option<rtrb::Producer<f32>>>>,
    peak_db: Arc<AtomicU32>,
    rms_db: Arc<AtomicU32>,
    is_active: Arc<AtomicBool>,
}

struct DesktopMonitorUserData {
    desktop_peak: Arc<AtomicU32>,
    desktop_rms: Arc<AtomicU32>,
    is_active: Arc<AtomicBool>,
}

pub struct PipeWireBridgeService {
    thread_handle: Option<JoinHandle<()>>,
    shutdown_sender: pw::channel::Sender<BridgeShutdown>,
    graph_controller: Arc<std::sync::Mutex<GraphController>>,
    pub is_active: Arc<AtomicBool>,
    pub game_peak: Arc<AtomicU32>,
    pub game_rms: Arc<AtomicU32>,
    pub chat_peak: Arc<AtomicU32>,
    pub chat_rms: Arc<AtomicU32>,
    pub media_peak: Arc<AtomicU32>,
    pub media_rms: Arc<AtomicU32>,
    pub aux_peak: Arc<AtomicU32>,
    pub aux_rms: Arc<AtomicU32>,
    pub desktop_peak: Arc<AtomicU32>,
    pub desktop_rms: Arc<AtomicU32>,
}

impl PipeWireBridgeService {
    pub fn start(pusher: ChannelPusher) -> anyhow::Result<Self> {
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

        // Setup default graph: 4 independent channel chains -> channel-specific PhysicalOutputNode
        let channels = [
            ("routune-game", "Routune Game", "game-resampler", "Game Resampler", "game-bypass", "Game DSP Bypass", "game-output", "Game Output"),
            ("routune-chat", "Routune Chat", "chat-resampler", "Chat Resampler", "chat-bypass", "Chat DSP Bypass", "chat-output", "Chat Output"),
            ("routune-media", "Routune Media", "media-resampler", "Media Resampler", "media-bypass", "Media DSP Bypass", "media-output", "Media Output"),
            ("routune-aux", "Routune Aux", "aux-resampler", "Aux Resampler", "aux-bypass", "Aux DSP Bypass", "aux-output", "Aux Output"),
        ];

        for (in_id, in_name, resamp_id, resamp_name, bypass_id, bypass_name, out_id, out_name) in channels {
            let in_node = VirtualInputNode::new(in_id, in_name, in_id);
            let resamp_node = ResamplerNode::new(resamp_id, resamp_name, 48000, 48000, 2, 256)
                .map_err(|e| anyhow::anyhow!("Failed to create {resamp_name}: {:?}", e))?;
            let bypass_node = DspBypassNode::new(bypass_id, bypass_name);
            let out_node = PhysicalOutputNode::new(out_id, out_name, "unassigned");

            controller.add_node(Box::new(in_node)).map_err(|e| anyhow::anyhow!(e))?;
            controller.add_node(Box::new(resamp_node)).map_err(|e| anyhow::anyhow!(e))?;
            controller.add_node(Box::new(bypass_node)).map_err(|e| anyhow::anyhow!(e))?;
            controller.add_node(Box::new(out_node)).map_err(|e| anyhow::anyhow!(e))?;

            controller.connect(in_id, resamp_id).map_err(|e| anyhow::anyhow!(e))?;
            controller.connect(resamp_id, bypass_id).map_err(|e| anyhow::anyhow!(e))?;
            controller.connect(bypass_id, out_id).map_err(|e| anyhow::anyhow!(e))?;
        }

        // 2. Setup PipeWire channel for shutdown
        let (shutdown_sender, shutdown_receiver) = pw::channel::channel::<BridgeShutdown>();

        // Rendezvous channel to wait for thread startup outcome
        let (init_tx, init_rx) = std::sync::mpsc::sync_channel::<anyhow::Result<()>>(0);

        let t_game_pusher = pusher.game.clone();
        let t_chat_pusher = pusher.chat.clone();
        let t_media_pusher = pusher.media.clone();
        let t_aux_pusher = pusher.aux.clone();

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
                    *pw::keys::NODE_NAME => "routune-desktop-monitor",
                    *pw::keys::NODE_DESCRIPTION => "Routune Desktop Audio Monitor",
                    *pw::keys::AUDIO_CHANNELS => "2",
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
                    ($stream:expr, $pusher:expr, $peak:expr, $rms:expr, $active:expr, $name:expr) => {{
                        let user_data = VirtualSinkUserData {
                            pusher: $pusher,
                            peak_db: $peak,
                            rms_db: $rms,
                            is_active: $active,
                        };
                        match $stream
                            .add_local_listener_with_user_data(user_data)
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
                                                    data.peak_db.store(peak.to_bits(), Ordering::Relaxed);
                                                    data.rms_db.store(rms.to_bits(), Ordering::Relaxed);
                                                    if peak > -80.0 {
                                                        data.is_active.store(true, Ordering::Relaxed);
                                                    }

                                                    if let Ok(mut guard) = data.pusher.try_lock() {
                                                        if let Some(prod) = guard.as_mut() {
                                                            for &sample in floats {
                                                                let _ = prod.push(sample);
                                                            }
                                                        }
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

                let _game_listener = register_sink_listener!(game_stream, t_game_pusher, t_game_peak, t_game_rms, t_is_active.clone(), "game");
                let _chat_listener = register_sink_listener!(chat_stream, t_chat_pusher, t_chat_peak, t_chat_rms, t_is_active.clone(), "chat");
                let _media_listener = register_sink_listener!(media_stream, t_media_pusher, t_media_peak, t_media_rms, t_is_active.clone(), "media");
                let _aux_listener = register_sink_listener!(aux_stream, t_aux_pusher, t_aux_peak, t_aux_rms, t_is_active.clone(), "aux");

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
            graph_controller: Arc::new(std::sync::Mutex::new(controller)),
            is_active,
            game_peak,
            game_rms,
            chat_peak,
            chat_rms,
            media_peak,
            media_rms,
            aux_peak,
            aux_rms,
            desktop_peak,
            desktop_rms,
        })
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
            },
            ChannelVolumeMetric {
                channel: AudioChannel::Game as i32,
                channel_name: "Game".to_string(),
                peak_volume_db: Self::read_decay_metric(&self.game_peak),
                rms_volume_db: f32::from_bits(self.game_rms.load(Ordering::Relaxed)),
            },
            ChannelVolumeMetric {
                channel: AudioChannel::Chat as i32,
                channel_name: "Chat".to_string(),
                peak_volume_db: Self::read_decay_metric(&self.chat_peak),
                rms_volume_db: f32::from_bits(self.chat_rms.load(Ordering::Relaxed)),
            },
            ChannelVolumeMetric {
                channel: AudioChannel::Media as i32,
                channel_name: "Media".to_string(),
                peak_volume_db: Self::read_decay_metric(&self.media_peak),
                rms_volume_db: f32::from_bits(self.media_rms.load(Ordering::Relaxed)),
            },
            ChannelVolumeMetric {
                channel: AudioChannel::Aux as i32,
                channel_name: "Aux".to_string(),
                peak_volume_db: Self::read_decay_metric(&self.aux_peak),
                rms_volume_db: f32::from_bits(self.aux_rms.load(Ordering::Relaxed)),
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
                    "routune-game" | "game-output" => (
                        f32::from_bits(self.game_peak.load(Ordering::Relaxed)),
                        f32::from_bits(self.game_rms.load(Ordering::Relaxed)),
                    ),
                    "routune-chat" | "chat-output" => (
                        f32::from_bits(self.chat_peak.load(Ordering::Relaxed)),
                        f32::from_bits(self.chat_rms.load(Ordering::Relaxed)),
                    ),
                    "routune-media" | "media-output" => (
                        f32::from_bits(self.media_peak.load(Ordering::Relaxed)),
                        f32::from_bits(self.media_rms.load(Ordering::Relaxed)),
                    ),
                    "routune-aux" | "aux-output" => (
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
                assert_eq!(metrics.len(), 4);
                assert_eq!(metrics[0].sink_id, "routune-game");
                assert_eq!(metrics[1].sink_id, "routune-chat");
                assert_eq!(metrics[2].sink_id, "routune-media");
                assert_eq!(metrics[3].sink_id, "routune-aux");

                let channel_metrics = service.get_channel_metrics();
                assert_eq!(channel_metrics.len(), 5);
                assert_eq!(channel_metrics[0].channel_name, "Master");
                assert_eq!(channel_metrics[1].channel_name, "Game");
                assert_eq!(channel_metrics[2].channel_name, "Chat");
                assert_eq!(channel_metrics[3].channel_name, "Media");
                assert_eq!(channel_metrics[4].channel_name, "Aux");

                let topology = service.get_topology();
                assert_eq!(topology.nodes.len(), 16);
                assert_eq!(topology.edges.len(), 12);

                service.shutdown();
            }
            Err(err) => {
                // If PipeWire daemon is not running in test runner, skip
                eprintln!("PipeWire daemon not available in test environment: {err}");
            }
        }
    }
}
