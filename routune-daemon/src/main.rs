mod audio;
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

use server::proto::routune_control_server::RoutuneControlServer;
use server::{ControlServiceImpl, TelemetryMetricsProvider};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    info!("Starting Routune audio daemon service");

    // 1. Initialize Sonar per-channel router and pushers
    let (channel_router, pusher) = audio::ChannelRouter::new();
    let is_active = Arc::new(AtomicBool::new(false));
    let channel_router = Arc::new(tokio::sync::Mutex::new(channel_router));

    // 2. Initialize Linux PipeWire virtual audio routing and node graph
    #[cfg(target_os = "linux")]
    let (pipewire_service, provider): (
        Option<Arc<pipewire_bridge::PipeWireBridgeService>>,
        Option<Arc<dyn TelemetryMetricsProvider>>,
    ) = match pipewire_bridge::PipeWireBridgeService::start(pusher) {
        Ok(service) => {
            info!("PipeWire virtual audio bridge running with 4 virtual sinks ('routune-game', 'routune-chat', 'routune-media', 'routune-aux')");
            let arc_service = Arc::new(service);
            (Some(arc_service.clone()), Some(arc_service as Arc<dyn TelemetryMetricsProvider>))
        }
        Err(err) => {
            warn!("PipeWire virtual bridge unavailable ({err}). Continuing in fallback mode.");
            (None, None)
        }
    };

    #[cfg(not(target_os = "linux"))]
    let provider: Option<Arc<dyn TelemetryMetricsProvider>> = None;

    // 3. Setup lock-free telemetry dispatcher
    let (telemetry_tx, _) = broadcast::channel(128);
    let dispatcher_handle = tokio::spawn(server::run_telemetry_dispatcher(
        None,
        telemetry_tx.clone(),
        provider.clone(),
        Some(channel_router.clone()),
    ));

    // 4. Initialize gRPC control service
    let control_service = ControlServiceImpl::new(
        is_active,
        telemetry_tx,
        provider,
        channel_router.clone(),
    );
    let control_server = RoutuneControlServer::new(control_service);
    let web_control_server = tonic_web::enable(control_server);

    let addr: SocketAddr = std::env::var("ROUTUNE_DAEMON_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:50051".to_string())
        .parse()?;

    info!("gRPC Control server listening on http://{}", addr);

    // 5. Spawn background stream recovery supervisor task
    let recovery_router = channel_router.clone();
    let supervisor_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
        loop {
            interval.tick().await;
            let death_signal = {
                let router = recovery_router.lock().await;
                router.try_recv_death_signal()
            };

            if let Some(signal) = death_signal {
                warn!("Channel stream death signal received: {:?}. Audio routing handle released.", signal);
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