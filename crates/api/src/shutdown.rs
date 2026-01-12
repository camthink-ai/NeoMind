//! Graceful shutdown handling for the web server.
//!
//! This module provides signal handling and resource cleanup for clean shutdown.

use std::time::Duration;

use crate::server::ServerState;

/// Shutdown timeout in seconds.
const SHUTDOWN_TIMEOUT: u64 = 30;

/// Wait for shutdown signal (Ctrl+C or SIGTERM).
pub async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received Ctrl+C, starting graceful shutdown");
        }
        _ = terminate => {
            tracing::info!("Received SIGTERM, starting graceful shutdown");
        }
    }
}

/// Clean up resources before shutdown.
pub async fn cleanup_resources(state: &ServerState) {
    tracing::info!("Cleaning up resources...");

    // 1. Disconnect MQTT (with timeout)
    let mqtt = state.mqtt_device_manager.clone();
    let mqtt_task = tokio::spawn(async move {
        if let Err(e) = mqtt.disconnect().await {
            tracing::warn!("MQTT disconnect error: {}", e);
        }
    });
    let _ = tokio::time::timeout(Duration::from_secs(5), mqtt_task).await;

    // 2. Note embedded broker status (feature-gated)
    #[cfg(feature = "embedded-broker")]
    if let Some(broker) = &state.embedded_broker {
        if broker.is_running() {
            tracing::info!("Embedded MQTT broker was running");
            // Note: EmbeddedBroker doesn't have a stop method,
            // it will be cleaned up when the process exits
        }
    }

    // 3. Flush any pending database writes
    tracing::info!("Flushing storage...");

    // Note: TimeSeriesStorage doesn't have explicit flush/close
    // The redb database handles this via Drop

    // 4. Log session counts
    let sessions = state.session_manager.list_sessions().await;
    tracing::info!("Shutdown complete. Active sessions: {}", sessions.len());

    // 5. Log uptime
    let uptime = chrono::Utc::now().timestamp() - state.started_at;
    tracing::info!("Server uptime: {} seconds", uptime);
}

/// Run graceful shutdown with timeout.
pub async fn shutdown_with_timeout(state: &ServerState) {
    // Run cleanup directly instead of spawning, since we need to wait for it anyway
    // This avoids the 'static lifetime requirement
    match tokio::time::timeout(Duration::from_secs(SHUTDOWN_TIMEOUT), cleanup_resources(state)).await {
        Ok(_) => {
            tracing::info!("Resources cleaned up successfully");
        }
        Err(_) => {
            tracing::warn!("Cleanup timed out after {} seconds", SHUTDOWN_TIMEOUT);
        }
    }
}
