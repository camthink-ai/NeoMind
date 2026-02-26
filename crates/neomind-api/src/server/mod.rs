//! Web server for Edge AI Agent.
//!
//! This provides a web interface with WebSocket support for chat
//! and REST API for devices, rules, alerts, and session management.

pub mod assets;
pub mod extension_metrics;
pub mod middleware;
pub mod router;
pub mod state;
pub mod types;

// Re-export commonly used types
pub use middleware::rate_limit_middleware;
pub use router::{create_router, create_router_with_state};
pub use state::DeviceStatusUpdate;
pub use types::{ServerState, MAX_REQUEST_BODY_SIZE};

use std::net::SocketAddr;
use std::time::Duration;

/// Start the web server on a specific address.
/// This is the main entry point for running the server.
pub async fn run(bind: SocketAddr) -> anyhow::Result<()> {
    use crate::startup::{ServiceStatus, StartupLogger};

    // Note: V2 extension system doesn't require panic hook installation
    // The V2 system uses safer FFI boundaries directly

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

    // Initialize transform event service
    state.init_transform_event_service().await;
    startup.service("Transform event service", ServiceStatus::Started);

    // Initialize tools
    state.init_tools().await;
    startup.service("AI tools", ServiceStatus::Started);

    // Initialize rule engine event service
    state.init_rule_engine_events().await;
    startup.service("Rule engine events", ServiceStatus::Started);

    // Initialize auto-onboarding event listener
    state.init_auto_onboarding_events().await;
    startup.service("Auto-onboarding events", ServiceStatus::Started);

    // Initialize extension metrics collector (decoupled from device system)
    let extension_registry = state.extensions.registry.clone();
    let metrics_storage = state.extensions.metrics_storage.clone();
    let event_bus = state.core.event_bus.clone();
    let _metrics_task = tokio::spawn(async move {
        use crate::server::extension_metrics::ExtensionMetricsCollector;
        use std::time::Duration;

        let collector =
            ExtensionMetricsCollector::new(extension_registry, metrics_storage, event_bus)
                .with_interval(Duration::from_secs(60));

        collector.run().await;
    });
    startup.service("Extension metrics collector", ServiceStatus::Started);

    // Initialize extensions from persistent storage
    // This loads all extensions marked with auto_start=true
    state.init_extensions().await;

    // Initialize AI Agent manager
    let _ = state.start_agent_manager().await;
    startup.service("AI Agent manager", ServiceStatus::Started);

    // Initialize AI Agent event listener
    state.init_agent_events().await;
    startup.service("AI Agent events", ServiceStatus::Started);

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

    // P0: Spawn pending stream cleanup task (runs every 5 minutes)
    // Cleans up stale pending stream states that weren't properly cleared
    let state_for_cleanup_task = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(300));
        loop {
            interval.tick().await;
            let session_store = state_for_cleanup_task
                .agents
                .session_manager
                .session_store();
            match session_store.cleanup_stale_pending_streams() {
                Ok(count) => {
                    if count > 0 {
                        tracing::info!("Cleaned up {} stale pending stream states", count);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to cleanup stale pending streams: {}", e);
                }
            }
        }
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

/// Start the server with default configuration.
/// This function is designed to be called from Tauri or other embedded contexts.
/// It starts the server in the background and returns immediately.
///
/// Uses port 9375 to avoid conflicts with common applications.
/// Binds to 0.0.0.0 to allow LAN access.
pub async fn start_server() -> anyhow::Result<()> {
    let bind: SocketAddr = "0.0.0.0:9375".parse()?;
    run(bind).await
}
