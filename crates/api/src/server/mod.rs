//! Web server for Edge AI Agent.
//!
//! This provides a web interface with WebSocket support for chat
//! and REST API for devices, rules, alerts, and session management.

pub mod types;
pub mod middleware;
pub mod router;

// Re-export commonly used types
pub use types::{ServerState, DeviceStatusUpdate, MAX_REQUEST_BODY_SIZE};
pub use router::{create_router, create_router_with_state};
pub use middleware::rate_limit_middleware;

use std::net::SocketAddr;
use std::time::Duration;

/// Run the web server with graceful shutdown.
pub async fn run(bind: SocketAddr) -> anyhow::Result<()> {
    use crate::startup::{StartupLogger, ServiceStatus};

    let mut startup = StartupLogger::new();
    startup.banner();

    let state = ServerState::new();

    // Initialization phase
    startup.phase_init();

    // Initialize device type storage (must be before init_mqtt)
    state.init_device_storage().await;
    startup.service("Device storage", ServiceStatus::Started);

    // Initialize LLM
    state.init_llm().await;

    // Initialize workflow engine
    state.init_workflow_engine().await;
    startup.service("Workflow engine", ServiceStatus::Started);

    // Configuration phase
    startup.phase_config();

    // Initialize MQTT and register device types
    state.init_mqtt().await;

    // Services phase
    startup.phase_services();

    // Clone state for cleanup (move into shutdown task)
    let state_for_cleanup = state.clone();

    // Spawn rate limit cleanup task (runs every 5 minutes)
    let rate_limiter = state.rate_limiter.clone();
    tokio::spawn(async move {
        crate::rate_limit::cleanup_task(rate_limiter, Duration::from_secs(300)).await;
    });

    let app = create_router_with_state(state);

    let listener = tokio::net::TcpListener::bind(bind).await?;

    // Ready phase
    startup.phase_ready();
    startup.ready_info(&bind.to_string());

    // Run with graceful shutdown
    axum::serve(listener, app)
        .with_graceful_shutdown(crate::shutdown::shutdown_signal())
        .await?;

    // Clean up resources after server shuts down
    crate::shutdown::cleanup_resources(&state_for_cleanup).await;

    tracing::info!("Server shutdown complete");
    Ok(())
}
