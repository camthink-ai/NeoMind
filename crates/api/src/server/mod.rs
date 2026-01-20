//! Web server for Edge AI Agent.
//!
//! This provides a web interface with WebSocket support for chat
//! and REST API for devices, rules, alerts, and session management.

pub mod middleware;
pub mod router;
pub mod types;

// Re-export commonly used types
pub use middleware::rate_limit_middleware;
pub use router::{create_router, create_router_with_state};
pub use types::{DeviceStatusUpdate, MAX_REQUEST_BODY_SIZE, ServerState};

use std::net::SocketAddr;
use std::time::Duration;

/// Run the web server with graceful shutdown.
pub async fn run(bind: SocketAddr) -> anyhow::Result<()> {
    use crate::startup::{ServiceStatus, StartupLogger};

    let mut startup = StartupLogger::new();
    startup.banner();

    let state = ServerState::new().await;

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

    // Initialize tools (must be after workflow engine)
    state.init_tools().await;
    startup.service("AI tools", ServiceStatus::Started);

    // Initialize event persistence
    state.init_event_log().await;
    startup.service("Event persistence", ServiceStatus::Started);

    // Initialize rule engine event service
    state.init_rule_engine_events().await;
    startup.service("Rule engine events", ServiceStatus::Started);

    // Initialize auto-onboarding event listener
    state.init_auto_onboarding_events().await;
    startup.service("Auto-onboarding events", ServiceStatus::Started);

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
