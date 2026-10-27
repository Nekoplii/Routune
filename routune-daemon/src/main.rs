mod audio;
mod server;

use anyhow::Result;
use std::net::SocketAddr;
use tokio::sync::broadcast;
use tonic::transport::Server;
use tracing::info;

use server::proto::routune_control_server::RoutuneControlServer;
use server::ControlServiceImpl;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    info!("Starting Routune audio daemon service");

    // 1. Initialize lock-free audio capture
    let (audio_service, consumer) = audio::initialize_audio_capture();

    // 2. Setup lock-free telemetry dispatcher
    let (telemetry_tx, _) = broadcast::channel(128);
    let dispatcher_handle = tokio::spawn(server::run_telemetry_dispatcher(
        consumer,
        telemetry_tx.clone(),
    ));

    // 3. Initialize gRPC control service
    let control_service = ControlServiceImpl::new(audio_service.is_active.clone(), telemetry_tx);
    let control_server = RoutuneControlServer::new(control_service);
    let web_control_server = tonic_web::enable(control_server);

    let addr: SocketAddr = std::env::var("ROUTUNE_DAEMON_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:50051".to_string())
        .parse()?;

    info!("gRPC Control server listening on http://{}", addr);

    // 4. Run server with graceful shutdown
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

    dispatcher_handle.abort();
    info!("Routune daemon shutdown complete.");

    Ok(())
}
