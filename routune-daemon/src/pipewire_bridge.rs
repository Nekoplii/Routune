#[cfg(target_os = "linux")]
use anyhow::{Context, Result};
use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use pw::spa::pod::Pod;
use rtrb::RingBuffer;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use tracing::info;

use routune_core::graph::{
    calculate_metrics, AudioEngineGraph, DspBypassNode, GraphController,
    PhysicalOutputNode, ProcessContext, ResamplerNode, VirtualInputNode,
};

use crate::server::proto::{GraphEdgeInfo, GraphNodeInfo, GraphTopology, SinkMetric};

pub const RING_BUFFER_SAMPLES: usize = 16384;
pub const SCRATCH_BUFFER_SAMPLES: usize = 4096;

struct BridgeShutdown;

struct GameSinkUserData {
    producer: rtrb::Producer<f32>,
    peak_db: Arc<AtomicU32>,
    rms_db: Arc<AtomicU32>,
}

struct ChatSinkUserData {
    producer: rtrb::Producer<f32>,
    peak_db: Arc<AtomicU32>,
    rms_db: Arc<AtomicU32>,
}

struct PlaybackUserData {
    game_consumer: rtrb::Consumer<f32>,
    chat_consumer: rtrb::Consumer<f32>,
    graph: AudioEngineGraph,
    game_scratch: Vec<f32>,
    chat_scratch: Vec<f32>,
    render_scratch: Vec<f32>,
    master_peak: Arc<AtomicU32>,
    master_rms: Arc<AtomicU32>,
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
    pub master_peak: Arc<AtomicU32>,
    pub master_rms: Arc<AtomicU32>,
    pub desktop_peak: Arc<AtomicU32>,
    pub desktop_rms: Arc<AtomicU32>,
}

impl PipeWireBridgeService {
    pub fn start() -> Result<Self> {
        pw::init();

        let (game_prod, game_cons) = RingBuffer::<f32>::new(RING_BUFFER_SAMPLES);
        let (chat_prod, chat_cons) = RingBuffer::<f32>::new(RING_BUFFER_SAMPLES);

        let (cmd_prod, cmd_cons) = RingBuffer::new(128);
        let (recycle_prod, recycle_cons) = RingBuffer::new(128);

        let graph = AudioEngineGraph::new(cmd_cons, recycle_prod, SCRATCH_BUFFER_SAMPLES);
        let mut controller = GraphController::new(cmd_prod, recycle_cons);

        let is_active = Arc::new(AtomicBool::new(false));
        let game_peak = Arc::new(AtomicU32::new((-100.0f32).to_bits()));
        let game_rms = Arc::new(AtomicU32::new((-100.0f32).to_bits()));
        let chat_peak = Arc::new(AtomicU32::new((-100.0f32).to_bits()));
        let chat_rms = Arc::new(AtomicU32::new((-100.0f32).to_bits()));
        let master_peak = Arc::new(AtomicU32::new((-100.0f32).to_bits()));
        let master_rms = Arc::new(AtomicU32::new((-100.0f32).to_bits()));
        let desktop_peak = Arc::new(AtomicU32::new((-100.0f32).to_bits()));
        let desktop_rms = Arc::new(AtomicU32::new((-100.0f32).to_bits()));

        // 1. Setup default graph nodes
        let game_in = VirtualInputNode::new("routune-game", "Routune Game", "routune-game");
        let chat_in = VirtualInputNode::new("routune-chat", "Routune Chat", "routune-chat");
        let game_dsp = DspBypassNode::new("game-bypass", "Game DSP Bypass");
        let chat_dsp = DspBypassNode::new("chat-bypass", "Chat DSP Bypass");
        let game_resampler =
            ResamplerNode::new("game-resampler", "Game Resampler", 48000, 48000, 2, 256)
                .map_err(|e| anyhow::anyhow!("Failed to create game resampler: {:?}", e))?;
        let chat_resampler =
            ResamplerNode::new("chat-resampler", "Chat Resampler", 48000, 48000, 2, 256)
                .map_err(|e| anyhow::anyhow!("Failed to create chat resampler: {:?}", e))?;
        let master_out = PhysicalOutputNode::new("master-output", "Master Output", "default");

        controller
            .add_node(Box::new(game_in))
            .map_err(|e| anyhow::anyhow!(e))?;
        controller
            .add_node(Box::new(game_resampler))
            .map_err(|e| anyhow::anyhow!(e))?;
        controller
            .add_node(Box::new(game_dsp))
            .map_err(|e| anyhow::anyhow!(e))?;

        controller
            .add_node(Box::new(chat_in))
            .map_err(|e| anyhow::anyhow!(e))?;
        controller
            .add_node(Box::new(chat_resampler))
            .map_err(|e| anyhow::anyhow!(e))?;
        controller
            .add_node(Box::new(chat_dsp))
            .map_err(|e| anyhow::anyhow!(e))?;

        controller
            .add_node(Box::new(master_out))
            .map_err(|e| anyhow::anyhow!(e))?;

        controller
            .connect("routune-game", "game-resampler")
            .map_err(|e| anyhow::anyhow!(e))?;
        controller
            .connect("game-resampler", "game-bypass")
            .map_err(|e| anyhow::anyhow!(e))?;
        controller
            .connect("game-bypass", "master-output")
            .map_err(|e| anyhow::anyhow!(e))?;

        controller
            .connect("routune-chat", "chat-resampler")
            .map_err(|e| anyhow::anyhow!(e))?;
        controller
            .connect("chat-resampler", "chat-bypass")
            .map_err(|e| anyhow::anyhow!(e))?;
        controller
            .connect("chat-bypass", "master-output")
            .map_err(|e| anyhow::anyhow!(e))?;

        // 2. Setup PipeWire channel for shutdown
        let (shutdown_sender, shutdown_receiver) = pw::channel::channel::<BridgeShutdown>();

        // Rendezvous channel to wait for thread startup outcome
        let (init_tx, init_rx) = std::sync::mpsc::sync_channel::<Result<()>>(0);

        let t_game_peak = game_peak.clone();
        let t_game_rms = game_rms.clone();
        let t_chat_peak = chat_peak.clone();
        let t_chat_rms = chat_rms.clone();
        let t_master_peak = master_peak.clone();
        let t_master_rms = master_rms.clone();
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
                let game_props = properties! {
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
                let chat_props = properties! {
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

                // Playback Output to default physical sink
                let playback_props = properties! {
                    *pw::keys::MEDIA_TYPE => "Audio",
                    *pw::keys::MEDIA_CATEGORY => "Playback",
                    *pw::keys::NODE_NAME => "routune-playback",
                    *pw::keys::NODE_DESCRIPTION => "Routune Playback Output",
                    *pw::keys::AUDIO_CHANNELS => "2",
                };

                let playback_stream = match pw::stream::StreamBox::new(&core, "routune-playback", playback_props) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = init_tx.send(Err(anyhow::anyhow!("Failed to create playback StreamBox: {:?}", e)));
                        return;
                    }
                };

                // Desktop Audio Monitor (capturing default physical sink monitor)
                let monitor_props = properties! {
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

                // Add listeners
                let game_user_data = GameSinkUserData {
                    producer: game_prod,
                    peak_db: t_game_peak,
                    rms_db: t_game_rms,
                };

                let _game_listener = match game_stream
                    .add_local_listener_with_user_data(game_user_data)
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
                                            for &sample in floats {
                                                let _ = data.producer.push(sample);
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
                        let _ = init_tx.send(Err(anyhow::anyhow!("Failed to register game listener: {:?}", e)));
                        return;
                    }
                };

                let chat_user_data = ChatSinkUserData {
                    producer: chat_prod,
                    peak_db: t_chat_peak,
                    rms_db: t_chat_rms,
                };

                let _chat_listener = match chat_stream
                    .add_local_listener_with_user_data(chat_user_data)
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
                                            for &sample in floats {
                                                let _ = data.producer.push(sample);
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
                        let _ = init_tx.send(Err(anyhow::anyhow!("Failed to register chat listener: {:?}", e)));
                        return;
                    }
                };

                let playback_user_data = PlaybackUserData {
                    game_consumer: game_cons,
                    chat_consumer: chat_cons,
                    graph,
                    game_scratch: vec![0.0f32; SCRATCH_BUFFER_SAMPLES],
                    chat_scratch: vec![0.0f32; SCRATCH_BUFFER_SAMPLES],
                    render_scratch: vec![0.0f32; SCRATCH_BUFFER_SAMPLES],
                    master_peak: t_master_peak,
                    master_rms: t_master_rms,
                    is_active: t_is_active.clone(),
                };

                let _playback_listener = match playback_stream
                    .add_local_listener_with_user_data(playback_user_data)
                    .process(|stream, data| {
                        if let Some(mut buf) = stream.dequeue_buffer() {
                            let datas = buf.datas_mut();
                            if !datas.is_empty() {
                                if let Some(slice) = datas[0].data() {
                                    let max_samples = slice.len() / 4;
                                    let target_samples = max_samples.min(data.render_scratch.len());
                                    let frames = target_samples / 2;
                                    if frames == 0 {
                                        return;
                                    }
                                    data.is_active.store(true, Ordering::Relaxed);

                                    // 1. Drain game ringbuffer
                                    let mut g_read = 0;
                                    while g_read < target_samples {
                                        if let Ok(sample) = data.game_consumer.pop() {
                                            data.game_scratch[g_read] = sample;
                                            g_read += 1;
                                        } else {
                                            break;
                                        }
                                    }
                                    if g_read < target_samples {
                                        data.game_scratch[g_read..target_samples].fill(0.0);
                                    }

                                    // 2. Drain chat ringbuffer
                                    let mut c_read = 0;
                                    while c_read < target_samples {
                                        if let Ok(sample) = data.chat_consumer.pop() {
                                            data.chat_scratch[c_read] = sample;
                                            c_read += 1;
                                        } else {
                                            break;
                                        }
                                    }
                                    if c_read < target_samples {
                                        data.chat_scratch[c_read..target_samples].fill(0.0);
                                    }

                                    // 3. Feed inputs to graph
                                    data.graph.feed_node_input("routune-game", &data.game_scratch[..target_samples]);
                                    data.graph.feed_node_input("routune-chat", &data.chat_scratch[..target_samples]);

                                    let context = ProcessContext {
                                        sample_rate: 48000,
                                        channels: 2,
                                        frames,
                                    };

                                    // 4. Render quantum
                                    data.graph.process_quantum(&mut data.render_scratch[..target_samples], &context);

                                    // 5. Calculate master metrics
                                    let (peak, rms) = calculate_metrics(&data.render_scratch[..target_samples]);
                                    if peak > -99.0 {
                                        data.master_peak.store(peak.to_bits(), Ordering::Relaxed);
                                        data.master_rms.store(rms.to_bits(), Ordering::Relaxed);
                                    }

                                    // 6. Copy rendered frames to PipeWire buffer
                                    let byte_count = target_samples * 4;
                                    let float_bytes = unsafe {
                                        std::slice::from_raw_parts(
                                            data.render_scratch.as_ptr() as *const u8,
                                            byte_count,
                                        )
                                    };
                                    slice[..byte_count].copy_from_slice(float_bytes);

                                    let chunk = datas[0].chunk_mut();
                                    *chunk.offset_mut() = 0;
                                    *chunk.size_mut() = byte_count as u32;
                                    *chunk.stride_mut() = 8;
                                }
                            }
                        }
                    })
                    .register()
                {
                    Ok(l) => l,
                    Err(e) => {
                        let _ = init_tx.send(Err(anyhow::anyhow!("Failed to register playback listener: {:?}", e)));
                        return;
                    }
                };

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
                let mut game_param = match Pod::from_bytes(&format_bytes) {
                    Some(p) => [p],
                    None => {
                        let _ = init_tx.send(Err(anyhow::anyhow!("Failed to construct Pod from format_bytes")));
                        return;
                    }
                };
                if let Err(e) = game_stream.connect(
                    spa::utils::Direction::Input,
                    None,
                    pw::stream::StreamFlags::MAP_BUFFERS | pw::stream::StreamFlags::RT_PROCESS,
                    &mut game_param,
                ) {
                    let _ = init_tx.send(Err(anyhow::anyhow!("Failed to connect game stream: {:?}", e)));
                    return;
                }

                let mut chat_param = match Pod::from_bytes(&format_bytes) {
                    Some(p) => [p],
                    None => {
                        let _ = init_tx.send(Err(anyhow::anyhow!("Failed to construct Pod from format_bytes")));
                        return;
                    }
                };
                if let Err(e) = chat_stream.connect(
                    spa::utils::Direction::Input,
                    None,
                    pw::stream::StreamFlags::MAP_BUFFERS | pw::stream::StreamFlags::RT_PROCESS,
                    &mut chat_param,
                ) {
                    let _ = init_tx.send(Err(anyhow::anyhow!("Failed to connect chat stream: {:?}", e)));
                    return;
                }

                let mut playback_param = match Pod::from_bytes(&format_bytes) {
                    Some(p) => [p],
                    None => {
                        let _ = init_tx.send(Err(anyhow::anyhow!("Failed to construct Pod from format_bytes")));
                        return;
                    }
                };
                if let Err(e) = playback_stream.connect(
                    spa::utils::Direction::Output,
                    None,
                    pw::stream::StreamFlags::AUTOCONNECT
                        | pw::stream::StreamFlags::MAP_BUFFERS
                        | pw::stream::StreamFlags::RT_PROCESS,
                    &mut playback_param,
                ) {
                    let _ = init_tx.send(Err(anyhow::anyhow!("Failed to connect playback stream: {:?}", e)));
                    return;
                }

                let mut monitor_param = match Pod::from_bytes(&format_bytes) {
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

                info!("PipeWire virtual audio bridge successfully initialized and connected");
                let _ = init_tx.send(Ok(()));

                // Run PipeWire loop until shutdown signal
                mainloop.run();
                info!("PipeWire mainloop exited");
            })
            .context("Failed to spawn PipeWire bridge thread")?;

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
            master_peak,
            master_rms,
            desktop_peak,
            desktop_rms,
        })
    }

    pub fn get_sink_metrics(&self) -> Vec<SinkMetric> {
        vec![
            SinkMetric {
                sink_id: "routune-game".into(),
                sink_name: "Routune Game".into(),
                peak_volume_db: f32::from_bits(self.game_peak.swap((-100.0f32).to_bits(), Ordering::Relaxed)),
                rms_volume_db: f32::from_bits(self.game_rms.swap((-100.0f32).to_bits(), Ordering::Relaxed)),
            },
            SinkMetric {
                sink_id: "routune-chat".into(),
                sink_name: "Routune Chat".into(),
                peak_volume_db: f32::from_bits(self.chat_peak.swap((-100.0f32).to_bits(), Ordering::Relaxed)),
                rms_volume_db: f32::from_bits(self.chat_rms.swap((-100.0f32).to_bits(), Ordering::Relaxed)),
            },
        ]
    }

    pub fn get_master_metrics(&self) -> (f32, f32) {
        let d_peak = f32::from_bits(self.desktop_peak.swap((-100.0f32).to_bits(), Ordering::Relaxed));
        let d_rms = f32::from_bits(self.desktop_rms.swap((-100.0f32).to_bits(), Ordering::Relaxed));
        let p_peak = f32::from_bits(self.master_peak.swap((-100.0f32).to_bits(), Ordering::Relaxed));
        let p_rms = f32::from_bits(self.master_rms.swap((-100.0f32).to_bits(), Ordering::Relaxed));

        (d_peak.max(p_peak), d_rms.max(p_rms))
    }

    pub fn get_topology(&self) -> GraphTopology {
        let controller = match self.graph_controller.lock() {
            Ok(c) => c,
            Err(poisoned) => poisoned.into_inner(),
        };

        let nodes = controller
            .node_infos()
            .iter()
            .map(|info| {
                let (peak, rms) = match info.id.as_str() {
                    "routune-game" => (
                        f32::from_bits(self.game_peak.load(Ordering::Relaxed)),
                        f32::from_bits(self.game_rms.load(Ordering::Relaxed)),
                    ),
                    "routune-chat" => (
                        f32::from_bits(self.chat_peak.load(Ordering::Relaxed)),
                        f32::from_bits(self.chat_rms.load(Ordering::Relaxed)),
                    ),
                    "master-output" => (
                        f32::from_bits(self.master_peak.load(Ordering::Relaxed)),
                        f32::from_bits(self.master_rms.load(Ordering::Relaxed)),
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

    #[test]
    fn test_pipewire_desktop_monitor_stream() {
        if std::env::var("CI").is_ok() {
            return;
        }

        pw::init();

        let mainloop = pw::main_loop::MainLoopRc::new(None).expect("MainLoop failed");
        let context = pw::context::ContextRc::new(&mainloop, None).expect("Context failed");
        let core = context.connect_rc(None).expect("Core failed");

        let monitor_props = properties! {
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

        match PipeWireBridgeService::start() {
            Ok(service) => {
                let metrics = service.get_sink_metrics();
                assert_eq!(metrics.len(), 2);
                assert_eq!(metrics[0].sink_id, "routune-game");
                assert_eq!(metrics[1].sink_id, "routune-chat");

                let topology = service.get_topology();
                assert_eq!(topology.nodes.len(), 7);
                assert_eq!(topology.edges.len(), 6);

                service.shutdown();
            }
            Err(err) => {
                // If PipeWire daemon is not running in test runner, skip
                eprintln!("PipeWire daemon not available in test environment: {err}");
            }
        }
    }
}