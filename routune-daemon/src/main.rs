mod audio;
mod config;
#[cfg(target_os = "linux")]
mod pipewire_bridge;
mod server;

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::broadcast;
use tonic::transport::Server;
use tracing::{info, warn};

use routune_core::graph::VirtualChannel;
use server::proto::routune_control_server::RoutuneControlServer;
use server::{ControlServiceImpl, TelemetryMetricsProvider};

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if handle_cli(&args).await? {
        return Ok(());
    }

    audio::silence_alsa_logging();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stdout)
        .init();

    info!("Starting Routune audio daemon service");

    // 1. Load persistent configuration from ~/.config/routune/config.json
    let config_manager = Arc::new(config::ConfigManager::init()?);
    let initial_config = config_manager.get_config().await;

    // 2. Initialize Sonar per-channel router and pushers
    let (mut channel_router, pusher) = audio::ChannelRouter::new();
    let is_active = Arc::new(AtomicBool::new(false));

    // 2b. Initialize native virtual mic pipeline (native PipeWire source, CPAL DSP output stream, auto-link)
    #[cfg(target_os = "linux")]
    if let Err(e) = channel_router.init_virtual_mic_pipeline() {
        warn!("Failed to initialize virtual mic pipeline: {e}");
    }

    // 3. Map physical hardware outputs from persistent configuration with startup retry backoff
    let routes = [
        (server::proto::AudioChannel::Game as i32, initial_config.get_route(VirtualChannel::Game)),
        (server::proto::AudioChannel::Chat as i32, initial_config.get_route(VirtualChannel::Chat)),
        (server::proto::AudioChannel::Media as i32, initial_config.get_route(VirtualChannel::Media)),
        (server::proto::AudioChannel::Aux as i32, initial_config.get_route(VirtualChannel::Aux)),
        (server::proto::AudioChannel::Mic as i32, initial_config.get_route(VirtualChannel::Mic)),
    ];
    for (ch, target) in routes {
        if !target.is_empty() && !target.eq_ignore_ascii_case("none") && !target.eq_ignore_ascii_case("unassigned") {
            for attempt in 1..=4 {
                match channel_router.set_channel_routing(ch, target) {
                    Ok(act) => {
                        info!("Restored persisted hardware route for channel {ch} -> {act}");
                        break;
                    }
                    Err(e) => {
                        if attempt < 4 {
                            std::thread::sleep(std::time::Duration::from_millis(150 * attempt as u64));
                        } else {
                            warn!("Failed to restore route for channel {ch} to {target} after {attempt} attempts: {e}");
                        }
                    }
                }
            }
        }
    }
    let channel_router = Arc::new(tokio::sync::Mutex::new(channel_router));

    // 4. Build PipeWire virtual audio routing and inject preloaded EqNodes
    #[cfg(target_os = "linux")]
    let (pipewire_service, provider): (
        Option<Arc<pipewire_bridge::PipeWireBridgeService>>,
        Option<Arc<dyn TelemetryMetricsProvider>>,
    ) = match pipewire_bridge::PipeWireBridgeService::start_with_config(pusher, Some(&initial_config)) {
        Ok(service) => {
            info!("PipeWire virtual audio bridge running with 4 virtual sinks ('routune-game', 'routune-chat', 'routune-media', 'routune-aux')");
            let arc_service = Arc::new(service);
            let g = initial_config.get_noise_gate();
            let _ = arc_service.set_noise_gate(
                VirtualChannel::Mic,
                g.enabled,
                g.threshold_db,
                g.attack_ms,
                g.release_ms,
            );
            let c = initial_config.get_compressor();
            let _ = arc_service.set_compressor(
                VirtualChannel::Mic,
                c.enabled,
                c.threshold_db,
                c.ratio,
                c.attack_ms,
                c.release_ms,
                c.makeup_gain_db,
            );
            (Some(arc_service.clone()), Some(arc_service as Arc<dyn TelemetryMetricsProvider>))
        }
        Err(err) => {
            warn!("PipeWire virtual bridge unavailable ({err}). Continuing in fallback mode.");
            (None, None)
        }
    };

    #[cfg(target_os = "linux")]
    audio::unlink_unwanted_mic_dsp_links();

    #[cfg(not(target_os = "linux"))]
    let provider: Option<Arc<dyn TelemetryMetricsProvider>> = None;

    // 5. Setup lock-free telemetry dispatcher
    let (telemetry_tx, _) = broadcast::channel(128);
    let dispatcher_handle = tokio::spawn(server::run_telemetry_dispatcher(
        None,
        telemetry_tx.clone(),
        provider.clone(),
        Some(channel_router.clone()),
    ));

    // 6. Initialize gRPC control service with persistence manager
    let control_service = ControlServiceImpl::new_with_config(
        is_active,
        telemetry_tx,
        provider,
        channel_router.clone(),
        Some(config_manager.clone()),
    );
    let control_server = RoutuneControlServer::new(control_service);
    let web_control_server = tonic_web::enable(control_server);

    let addr: SocketAddr = std::env::var("ROUTUNE_DAEMON_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:50051".to_string())
        .parse()?;

    info!("gRPC Control server listening on http://{}", addr);

    // 5. Spawn background stream recovery supervisor task
    let recovery_router = channel_router.clone();
    let recovery_config = config_manager.clone();
    let supervisor_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
        let mut loop_count = 0u32;
        loop {
            interval.tick().await;
            loop_count = loop_count.wrapping_add(1);

            let death_signal = {
                let router = recovery_router.lock().await;
                router.try_recv_death_signal()
            };

            if let Some(ref signal) = death_signal {
                warn!("Channel stream death signal received: {:?}. Audio routing handle released.", signal);
            }

            // Periodic auto-recovery check (on death signal or every 3 seconds)
            if death_signal.is_some() || loop_count.is_multiple_of(6) {
                #[cfg(target_os = "linux")]
                tokio::task::spawn_blocking(audio::unlink_unwanted_mic_dsp_links);

                let cfg = recovery_config.get_config().await;
                let ch_targets = [
                    (server::proto::AudioChannel::Game as i32, cfg.get_route(VirtualChannel::Game)),
                    (server::proto::AudioChannel::Chat as i32, cfg.get_route(VirtualChannel::Chat)),
                    (server::proto::AudioChannel::Media as i32, cfg.get_route(VirtualChannel::Media)),
                    (server::proto::AudioChannel::Aux as i32, cfg.get_route(VirtualChannel::Aux)),
                    (server::proto::AudioChannel::Mic as i32, cfg.get_route(VirtualChannel::Mic)),
                ];

                let mut router = recovery_router.lock().await;
                for (ch_id, target) in ch_targets {
                    if !target.is_empty() && !target.eq_ignore_ascii_case("none") && !target.eq_ignore_ascii_case("unassigned") {
                        if !router.is_channel_stream_active(ch_id) {
                            match router.set_channel_routing(ch_id, target) {
                                Ok(act) => {
                                    info!("Supervisor recovered/restored channel {ch_id} route -> {act}");
                                }
                                Err(e) => {
                                    tracing::debug!("Supervisor recovery attempt for channel {ch_id} to {target}: {e}");
                                }
                            }
                        }
                    }
                }
            }
        }
    });

    // 6. Run server with graceful shutdown
    Server::builder()
        .accept_http1(true)
        .add_service(web_control_server)
        .serve_with_shutdown(addr, async {
            if let Err(err) = tokio::signal::ctrl_c().await {
                tracing::error!("Failed to listen for ctrl+c: {err}");
            }
            info!("Received shutdown signal. Stopping Routune daemon...");
        })
        .await?;

    supervisor_handle.abort();
    dispatcher_handle.abort();

    #[cfg(target_os = "linux")]
    if let Some(service) = pipewire_service {
        if let Ok(service) = Arc::try_unwrap(service) {
            service.shutdown();
        }
    }

    info!("Routune daemon shutdown complete.");

    Ok(())
}

fn parse_channel(s: &str) -> Result<server::proto::AudioChannel> {
    match s.to_lowercase().as_str() {
        "master" => Ok(server::proto::AudioChannel::Master),
        "game" => Ok(server::proto::AudioChannel::Game),
        "chat" => Ok(server::proto::AudioChannel::Chat),
        "media" => Ok(server::proto::AudioChannel::Media),
        "aux" => Ok(server::proto::AudioChannel::Aux),
        "mic" => Ok(server::proto::AudioChannel::Mic),
        other => anyhow::bail!("Unknown channel: '{other}'. Expected: master, game, chat, media, aux, mic"),
    }
}

async fn handle_cli(args: &[String]) -> Result<bool> {
    if args.len() <= 1 {
        return Ok(false);
    }

    let cmd = args[1].as_str();
    match cmd {
        "--toggle-mute" | "-m" => {
            let channel_name = args.get(2).map(|s| s.as_str()).unwrap_or("mic");
            let channel = parse_channel(channel_name)?;
            let mut client = server::proto::routune_control_client::RoutuneControlClient::connect("http://127.0.0.1:50051").await?;
            let config = client.get_daemon_config(server::proto::Empty {}).await?.into_inner();
            let current_muted = config.channels.iter().find(|m| m.channel == channel as i32).map(|m| m.muted).unwrap_or(false);
            let next_muted = !current_muted;
            client.set_channel_mute(server::proto::ChannelMuteRequest {
                channel: channel as i32,
                muted: next_muted,
            }).await?;
            println!("{channel_name} mute: {next_muted}");
            Ok(true)
        }
        "--toggle-noise" | "-n" => {
            let channel_name = args.get(2).map(|s| s.as_str()).unwrap_or("mic");
            let channel = parse_channel(channel_name)?;
            let mut client = server::proto::routune_control_client::RoutuneControlClient::connect("http://127.0.0.1:50051").await?;
            let config = client.get_daemon_config(server::proto::Empty {}).await?.into_inner();
            let current_noise = config.channels.iter().find(|n| n.channel == channel as i32).map(|n| n.noise_cancellation).unwrap_or(false);
            let next_noise = !current_noise;
            client.toggle_ai_noise(server::proto::AiNoiseRequest {
                channel: channel as i32,
                enabled: next_noise,
                level: 1.0,
            }).await?;
            println!("{channel_name} noise cancellation: {next_noise}");
            Ok(true)
        }
        "--help" | "-h" => {
            println!("Routune Audio Daemon CLI");
            println!("Usage: routune-daemon [OPTIONS]");
            println!();
            println!("Options:");
            println!("  --toggle-mute <channel>   Toggle channel mute (mic, master, game, chat, media, aux)");
            println!("  --toggle-noise <channel>  Toggle AI noise suppression (mic)");
            println!("  --help, -h                Show this help");
            println!();
            println!("When invoked without arguments, starts the background audio routing daemon.");
            Ok(true)
        }
        _ => Ok(false),
    }
}