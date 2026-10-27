use anyhow::Result;
use routune_core::RoutingGraph;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    info!("Starting Routune background daemon");

    let initial_graph = RoutingGraph::default();
    info!(
        "Initialized audio graph scaffold with {} nodes",
        initial_graph.nodes.len()
    );

    info!("Daemon running. Awaiting IPC connections...");

    // Placeholder lifecycle loop for Phase 1 scaffolding
    tokio::signal::ctrl_c().await?;
    info!("Shutting down Routune daemon");

    Ok(())
}
