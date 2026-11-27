//! Graceful shutdown handling for the web server.
//!
//! This module provides signal handling and resource cleanup for clean shutdown.

use std::time::Duration;

use crate::server::ServerState;

/// Shutdown timeout in seconds.
const SHUTDOWN_TIMEOUT: u64 = 30;

/// `shutdown_signal`, plus a test-only deadline: when
/// `NEOMIND_EXIT_AFTER_READY_MS` is set, the server exits gracefully that
/// many milliseconds after it starts serving. Lets the CLI integration
/// tests assert a FULL startup (bind → stores → services → ready) by simply
/// waiting for exit code 0 instead of "did it stay alive", which is what
/// made those tests environment-sensitive (they needed free ports for a
/// server that never exits).
pub async fn shutdown_signal_or_test_deadline() {
    let exit_after_ms = std::env::var("NEOMIND_EXIT_AFTER_READY_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok());
    tokio::select! {
        _ = shutdown_signal() => {}
        _ = async {
            match exit_after_ms {
                Some(ms) => {
                    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
                    tracing::info!(
                        exit_after_ms = ms,
                        "NEOMIND_EXIT_AFTER_READY_MS elapsed — graceful exit (test mode)"
                    );
                }
                None => std::future::pending::<()>().await,
            }
        } => {}
    }
}

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

    // Ensure all shutdown-aware background tasks, WS/SSE streams, schedulers,
    // watchdogs, and the TLS proxy are signalled to terminate.
    state.shutdown_token.cancel();

    // 0. Stop the clean-slate Cron/Interval ticker (in-flight submits finish
    // under their own timeout; there is no separate event-task abort map).
    if let Some(rt) = state.agents.clean_slate.read().await.as_ref() {
        rt.stop();
        tracing::info!("Stopped clean-slate agent scheduler at shutdown");
    }

    // 0b. Stop DataPush scheduler tasks gracefully (await final batch flushes
    // up to 5s without disabling persisted target configs in `data-push.redb`).
    if let Some(pm) = state.data_push.write().await.take() {
        match tokio::time::timeout(Duration::from_secs(5), pm.stop_all()).await {
            Ok(()) => tracing::info!("Stopped DataPush scheduler at shutdown"),
            Err(_) => tracing::warn!("DataPush scheduler stop timed out after 5s"),
        }
    }

    // 0c. Stop all registered IM bridges.
    if let Some(router) = state.im_router.read().await.clone() {
        let platforms = router.registry.list().await;
        for platform in platforms {
            if let Some(bridge) = router.registry.remove(&platform).await {
                match tokio::time::timeout(Duration::from_secs(5), bridge.stop()).await {
                    Ok(Ok(())) => tracing::info!(platform = ?platform, "Stopped IM bridge"),
                    Ok(Err(e)) => {
                        tracing::warn!(platform = ?platform, error = %e, "IM bridge stop error")
                    }
                    Err(_) => tracing::warn!(platform = ?platform, "IM bridge stop timed out"),
                }
            }
        }
    }

    // 1. Stop the builtin llama-server children EXPLICITLY. kill_on_drop
    // covers abnormal exits; this makes graceful shutdown immediate and
    // observable instead of waiting for process-exit reap. Without it, every
    // `systemctl restart` left a model-loaded llama-server (~2 GB) orphaned
    // and relied on the NEXT boot's port-conflict detection to reclaim it.
    crate::builtin_llm::server::stop_all_llama_servers();

    // 1b. Stop isolated extension runner processes (with timeout).
    match tokio::time::timeout(Duration::from_secs(10), state.extensions.runtime.stop_all()).await {
        Ok(()) => tracing::info!("Stopped isolated extensions at shutdown"),
        Err(_) => tracing::warn!("Stopping isolated extensions timed out after 10s"),
    }

    // 2. Stop ALL registered device adapters through DeviceService (with timeout)
    let device_service = state.devices.service.clone();
    let adapters_task = tokio::spawn(async move {
        let adapters = device_service.list_adapters().await;
        for adapter in adapters {
            if let Err(e) = device_service.stop_adapter(&adapter.id).await {
                tracing::warn!(
                    adapter_id = %adapter.id,
                    error = %e,
                    "Device adapter stop error"
                );
            }
        }
    });
    let _ = tokio::time::timeout(Duration::from_secs(5), adapters_task).await;

    // 3. Stop embedded broker (feature-gated)
    #[cfg(feature = "embedded-broker")]
    {
        let broker = state
            .devices
            .embedded_broker
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(broker) = broker.as_ref() {
            if broker.is_running() {
                tracing::info!("Stopping embedded MQTT broker...");
                if let Err(e) = broker.stop().await {
                    tracing::warn!("Embedded broker stop error: {}", e);
                }
            }
        }
    }

    // 4. Flush any pending database writes
    tracing::info!("Flushing storage...");

    // The timeseries WriteBuffer is a plain in-memory Vec — redb's Drop
    // knows nothing about it. Without this flush, Ctrl+C / SIGTERM lost
    // every point still sitting in the 200-point / 500ms buffer even
    // though write() had already reported success for them. open() hits
    // the process-wide singleton for the live path, so this flushes the
    // SAME buffer the server has been writing through.
    // Route through the LIVE telemetry wrapper, not a fresh open(): the
    // persistent store opens DEFERRED in the background, and when shutdown
    // races that open the singleton is unset — open() here would create a
    // second redb handle, hit the exclusive file lock, and fail, leaving the
    // placeholder store that actually holds the buffered points undrained.
    // state.devices.telemetry always addresses the store currently receiving
    // writes (placeholder or persistent), whichever won the race.
    match state.devices.telemetry.flush() {
        Ok(()) => {
            tracing::info!("Telemetry write buffer flushed");
        }
        Err(e) => {
            tracing::warn!(error = %e, "Telemetry final flush failed");
            // Fallback for paths that bypass the wrapper (tests, embedded use).
            if let Ok(ts) = neomind_storage::TimeSeriesStore::open(neomind_core::paths::store_path(
                "telemetry.redb",
            )) {
                ts.shutdown();
                tracing::info!("Telemetry write buffer flushed (singleton fallback)");
            }
        }
    }

    // 4b. Stale atomic-write temps: unique per process now, so a crashed
    // run's leftovers are pure garbage — sweep them at shutdown (the fn's
    // own doc suggested init-time, but shutdown is when the directory is
    // guaranteed quiescent for OUR writes).
    {
        let dir = neomind_core::paths::data_dir();
        let removed = neomind_storage::atomic_write::cleanup_stale_temps(&dir);
        if removed > 0 {
            tracing::info!(count = removed, "Swept stale atomic-write temp files");
        }
    }

    // 5. Log session counts
    let sessions = state.agents.session_manager.list_sessions().await;
    tracing::info!("Shutdown complete. Active sessions: {}", sessions.len());

    // 6. Log uptime
    let uptime = chrono::Utc::now().timestamp() - state.started_at;
    tracing::info!("Server uptime: {} seconds", uptime);
}

/// Run graceful shutdown with timeout.
pub async fn shutdown_with_timeout(state: &ServerState) {
    // Run cleanup directly instead of spawning, since we need to wait for it anyway
    // This avoids the 'static lifetime requirement
    match tokio::time::timeout(
        Duration::from_secs(SHUTDOWN_TIMEOUT),
        cleanup_resources(state),
    )
    .await
    {
        Ok(_) => {
            tracing::info!("Resources cleaned up successfully");
        }
        Err(_) => {
            tracing::warn!("Cleanup timed out after {} seconds", SHUTDOWN_TIMEOUT);
        }
    }
}
