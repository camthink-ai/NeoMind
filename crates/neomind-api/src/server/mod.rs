//! Web server for Edge AI Agent.
//!
//! This provides a web interface with WebSocket support for chat
//! and REST API for devices, rules, alerts, and session management.

pub mod assets;
pub mod extension_metrics;
pub mod image_cleanup;
pub mod install_service;
pub mod middleware;
pub mod router;
pub mod state;
pub mod system_context;
pub mod tools;
pub mod types;
pub mod uninstall_service;

// Re-export commonly used types
pub use install_service::ExtensionInstallService;
pub use uninstall_service::{ExtensionUninstallService, UninstallReport};

// Re-export tools
pub use tools::TransformTool;

// Re-export commonly used types
pub use middleware::rate_limit_middleware;
pub use router::{create_router, create_router_with_state};
pub use state::DeviceStatusUpdate;
pub use types::{ServerState, MAX_REQUEST_BODY_SIZE};

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use neomind_storage::ExtensionStore;
use tower_http::timeout::{RequestBodyTimeoutLayer, TimeoutLayer};

/// Start the web server on a specific address.
/// This is the main entry point for running the server.
pub async fn run(bind: SocketAddr) -> anyhow::Result<()> {
    use crate::startup::{ServiceStatus, StartupLogger};

    // Note: V2 extension system doesn't require panic hook installation
    // The V2 system uses safer FFI boundaries directly

    let mut startup = StartupLogger::new();
    startup.banner();

    // ── Phase A: Core init + HTTP listener (fast path to serving) ──

    let t_start = std::time::Instant::now();
    let state = ServerState::new().await;
    tracing::info!(
        elapsed_ms = t_start.elapsed().as_millis() as u64,
        "ServerState::new() completed"
    );

    // Initialization phase
    startup.phase_init();

    // Initialize device type storage (must be before init_device_adapters)
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

    // Start enabled data push targets (must be after event bus is ready)
    state.init_data_push_targets().await;
    startup.service("Data push targets", ServiceStatus::Started);

    // Configuration phase
    startup.phase_config();

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

    let app = create_router_with_state(state.clone());

    let app = app
        .layer(RequestBodyTimeoutLayer::new(Duration::from_secs(20)))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(120),
        ));

    let listener = tokio::net::TcpListener::bind(bind).await?;

    // Ready phase — HTTP listener is bound
    startup.phase_ready();
    startup.ready_info(&bind.to_string());

    tracing::info!(
        elapsed_ms = t_start.elapsed().as_millis() as u64,
        "HTTP listener ready (time to serve)"
    );

    // ── Phase B: Deferred background services (after listener starts serving) ──
    // These run in the background and do not block HTTP serving.

    // Initialize extension metrics collector (decoupled from device system)
    let runtime = state.extensions.runtime.clone();
    let metrics_storage = state.extensions.metrics_storage.clone();
    let event_bus_for_metrics = state.core.event_bus.clone();
    tokio::spawn(async move {
        use crate::server::extension_metrics::ExtensionMetricsCollector;
        use std::time::Duration;

        let mut collector = ExtensionMetricsCollector::new(runtime, metrics_storage)
            .with_interval(Duration::from_secs(60));

        if let Some(bus) = event_bus_for_metrics {
            collector = collector.with_event_bus(bus);
        }

        collector.run().await;
    });

    // Start telemetry retention cleanup background task
    {
        tokio::spawn(async move {
            use neomind_storage::{SettingsStore, TimeSeriesStore};

            // Wait for server to initialize
            tokio::time::sleep(Duration::from_secs(10)).await;

            const SETTINGS_DB_PATH: &str = "data/settings.redb";
            const TELEMETRY_DB_PATH: &str = "data/telemetry.redb";

            loop {
                // Load config on each cycle so runtime changes take effect.
                // [observability] Both reopen failures used to be silent —
                // retention could no-op forever with no trace of why.
                let config = SettingsStore::open(SETTINGS_DB_PATH)
                    .map(|s| s.get_retention_config())
                    .unwrap_or_else(|e| {
                        tracing::warn!(
                            category = "storage",
                            error = %e,
                            "Retention: settings store unavailable — retention disabled this cycle"
                        );
                        Default::default()
                    });

                let interval_secs = config.interval_hours * 3600;

                if config.enabled {
                    let policy = config.to_retention_policy();
                    let ts_store = match TimeSeriesStore::open(TELEMETRY_DB_PATH) {
                        Ok(store) => store,
                        Err(e) => {
                            tracing::warn!(
                                category = "storage",
                                error = %e,
                                "Retention: telemetry store unavailable — skipping this cycle"
                            );
                            tokio::time::sleep(std::time::Duration::from_secs(
                                interval_secs.max(3600),
                            ))
                            .await;
                            continue;
                        }
                    };
                    {
                        ts_store.set_retention_policy(policy).await;
                        match ts_store.apply_retention().await {
                            Ok(result) => {
                                if result.points_removed > 0 {
                                    tracing::info!(
                                        points_removed = result.points_removed,
                                        metrics_cleaned = result.metrics_cleaned.len(),
                                        "Retention cleanup completed"
                                    );
                                }
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "Retention cleanup failed");
                            }
                        }
                    }

                    // Clean up expired image files
                    if let Some(image_retention_hours) = config.image_retention {
                        let data_dir = std::env::var("NEOMIND_DATA_DIR")
                            .unwrap_or_else(|_| "data".to_string());
                        let images_dir = PathBuf::from(&data_dir).join("images");

                        match crate::server::image_cleanup::cleanup_expired_images(
                            &images_dir,
                            image_retention_hours,
                        )
                        .await
                        {
                            Ok((files_deleted, dirs_cleaned)) => {
                                if files_deleted > 0 || dirs_cleaned > 0 {
                                    tracing::info!(
                                        files_deleted = files_deleted,
                                        dirs_cleaned = dirs_cleaned,
                                        retention_hours = image_retention_hours,
                                        "Image retention cleanup completed"
                                    );
                                }
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "Image retention cleanup failed");
                            }
                        }
                    }
                }

                tokio::time::sleep(Duration::from_secs(interval_secs)).await;
            }
        });
    }

    // Start data-push delivery log cleanup background task.
    // Without this, data-push.redb grows unbounded in high-frequency push
    // scenarios and can fill the disk. Default retention: 30 days. Re-runs
    // every 24h. Failures are logged and retried next cycle — non-fatal.
    {
        let dp_state = state.clone();
        tokio::spawn(async move {
            // Wait for server to initialize (matches telemetry retention task)
            tokio::time::sleep(Duration::from_secs(15)).await;

            const DATA_PUSH_LOG_RETENTION_DAYS: u32 = 30;
            const RUN_INTERVAL_SECS: u64 = 24 * 60 * 60;

            loop {
                let push_manager_guard = dp_state.data_push.read().await;
                if let Some(pm) = push_manager_guard.as_ref() {
                    match pm.cleanup_logs(DATA_PUSH_LOG_RETENTION_DAYS) {
                        Ok(0) => tracing::debug!(
                            days = DATA_PUSH_LOG_RETENTION_DAYS,
                            "DataPush log cleanup: no old entries removed"
                        ),
                        Ok(n) => tracing::info!(
                            removed = n,
                            days = DATA_PUSH_LOG_RETENTION_DAYS,
                            "DataPush log cleanup removed old entries"
                        ),
                        Err(e) => tracing::warn!(
                            category = "data_push",
                            error = %e,
                            "DataPush log cleanup failed (will retry next cycle)"
                        ),
                    }
                }
                drop(push_manager_guard);
                tokio::time::sleep(Duration::from_secs(RUN_INTERVAL_SECS)).await;
            }
        });
    }

    // Start rule execution history cleanup background task.
    // Without this, rule_history.redb grows unbounded for long-running
    // deployments — `cleanup_history` was previously only called once at
    // startup (types.rs), so a server up for months would accumulate
    // months of trigger history. Default retention: 30 days. Re-runs
    // every 24h. Mirrors the data-push log cleanup task above. Failures
    // are logged and retried next cycle — non-fatal.
    {
        let rule_state = state.clone();
        tokio::spawn(async move {
            // Wait for server to initialize (matches data-push retention task).
            tokio::time::sleep(Duration::from_secs(20)).await;

            const RULE_HISTORY_RETENTION_DAYS: u64 = 30;
            const RUN_INTERVAL_SECS: u64 = 24 * 60 * 60;

            loop {
                if let Some(store) = rule_state.rule_store() {
                    match store.cleanup_history(RULE_HISTORY_RETENTION_DAYS) {
                        Ok(0) => tracing::debug!(
                            days = RULE_HISTORY_RETENTION_DAYS,
                            "Rule history cleanup: no old entries removed"
                        ),
                        Ok(n) => tracing::info!(
                            removed = n,
                            days = RULE_HISTORY_RETENTION_DAYS,
                            "Rule history cleanup removed old entries"
                        ),
                        Err(e) => tracing::warn!(
                            category = "rules",
                            error = %e,
                            "Rule history cleanup failed (will retry next cycle)"
                        ),
                    }
                }
                tokio::time::sleep(Duration::from_secs(RUN_INTERVAL_SECS)).await;
            }
        });
    }

    // Automation execution retention — this table was the one unbounded-growth
    // store the 0.9.12 sweep missed (messages/agent-executions/data-push/
    // rule-history all got tasks; automations were forgotten). 30 days,
    // daily, mirroring the rule-history task above.
    {
        let automation_state = state.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(20)).await;

            const AUTOMATION_EXEC_RETENTION_DAYS: u64 = 30;
            const RUN_INTERVAL_SECS: u64 = 24 * 60 * 60;

            loop {
                if let Some(store) = automation_state.automation.automation_store.clone() {
                    match store
                        .cleanup_executions(AUTOMATION_EXEC_RETENTION_DAYS)
                        .await
                    {
                        Ok(0) => {}
                        Ok(n) => tracing::info!(
                            removed = n,
                            days = AUTOMATION_EXEC_RETENTION_DAYS,
                            "Automation execution cleanup removed old records"
                        ),
                        Err(e) => tracing::warn!(
                            category = "automation",
                            error = %e,
                            "Automation execution cleanup failed (will retry next cycle)"
                        ),
                    }
                }
                tokio::time::sleep(Duration::from_secs(RUN_INTERVAL_SECS)).await;
            }
        });
    }

    // Heavy background services — extension loading, agent manager, MQTT
    {
        let bg_state = state.clone();
        tokio::spawn(async move {
            let t_bg = std::time::Instant::now();

            // Start embedded MQTT broker immediately so devices can connect
            // while extensions and other services load in parallel.
            #[cfg(feature = "embedded-broker")]
            {
                bg_state.start_embedded_broker().await;
            }

            // Kill orphaned extension runner processes from a previous session.
            // MUST run before init_extensions() to avoid killing newly spawned runners.
            // Orphaned runners hold dylib files open and cause dlopen() hangs.
            neomind_core::extension::isolated::IsolatedExtensionManager::cleanup_orphaned_runners();
            tracing::info!(
                elapsed_ms = t_bg.elapsed().as_millis() as u64,
                "Extension orphan cleanup done"
            );

            // Initialize extensions from persistent storage
            bg_state.init_extensions().await;

            // Refresh tool registry now that extensions are loaded
            bg_state.refresh_extension_tools().await;

            // Start extension death monitoring for auto-restart
            {
                let runtime = bg_state.extensions.runtime.clone();
                bg_state.extensions.runtime.set_on_crash_recovery_restart(Arc::new(
                    move |extension_id: &str, _path: &std::path::Path| {
                        let ext_id = extension_id.to_string();
                        let rt = runtime.clone();
                        tokio::spawn(async move {
                            if let Ok(store) = ExtensionStore::open("data/extensions.redb") {
                                // Clear error status after successful crash recovery
                                if let Ok(Some(mut record)) = store.load(&ext_id) {
                                    record.health_status = "ok".to_string();
                                    record.last_error = None;
                                    record.last_error_at = None;
                                    let _ = store.save(&record);

                                    // Apply saved config via ConfigUpdate IPC (NOT
                                    // execute_command, because "configure" is a
                                    // lifecycle method, not a registered command).
                                    if let Some(ref config) = record.config {
                                        tracing::info!(
                                            extension_id = %ext_id,
                                            "Applying saved config to extension after crash recovery"
                                        );
                                        if let Err(e) = rt
                                            .send_config_update(&ext_id, config)
                                            .await
                                        {
                                            tracing::warn!(
                                                extension_id = %ext_id,
                                                error = %e,
                                                "Failed to apply saved config after crash recovery"
                                            );
                                        }
                                    }
                                }
                            }
                        });
                    },
                ));
            }

            // Record error in storage when crash recovery restart fails
            bg_state
                .extensions
                .runtime
                .set_on_crash_recovery_failed(Arc::new(move |extension_id: &str, error: &str| {
                    let ext_id = extension_id.to_string();
                    let err_msg = error.to_string();
                    tokio::spawn(async move {
                        if let Ok(store) = ExtensionStore::open("data/extensions.redb") {
                            let _ = store.update_error_status(&ext_id, &err_msg);
                        }
                    });
                }));

            bg_state.extensions.runtime.clone().start_death_monitoring();

            // Initialize extension event subscription
            bg_state.init_extension_event_subscription().await;

            // Initialize AI Agent manager.
            // A failure here means the scheduler never starts: all scheduled and
            // event-triggered agents silently stop running (HTTP/health stay green).
            // Log at error! so it is visible — do not swallow with `let _ =`.
            if let Err(e) = bg_state.start_agent_manager().await {
                tracing::error!(
                    category = "agent",
                    error = %e,
                    "Failed to start agent manager — scheduled and event-triggered agents will not run"
                );
            }

            // Start the IM bridge router.
            // A failure here means all inbound IM messages (Telegram/Feishu/...)
            // are silently dropped — the EventBus subscription never starts, so
            // inbound events go nowhere. Surface at error! so it is visible;
            // do NOT swallow with `let _ =` (P0 pattern).
            if let Err(e) = bg_state.start_im_router().await {
                tracing::error!(
                    category = "im",
                    error = %e,
                    "Failed to start IM router — inbound IM messages will not be processed"
                );
            }

            // Initialize AI Agent event listener
            bg_state.init_agent_events().await;

            // Auto-register a reachable llama.cpp server (if none exists yet),
            // then refresh capabilities from /props for all llama.cpp backends.
            {
                let mut retry_interval = tokio::time::interval(Duration::from_secs(5));
                for _ in 0..12 {
                    retry_interval.tick().await;
                    if let Ok(instance_manager) = neomind_agent::get_instance_manager() {
                        instance_manager.auto_register_llamacpp().await;
                        instance_manager.detect_llamacpp_capabilities().await;
                        break;
                    }
                }
            }

            // Start memory scheduler (temp file cleanup)
            {
                let agents_state = bg_state.agents.clone();
                tokio::spawn(async move {
                    if let Err(e) = agents_state.start_memory_scheduler().await {
                        tracing::warn!(
                            category = "memory",
                            error = %e,
                            "Failed to start memory scheduler"
                        );
                    }
                });
            }

            // Start periodic system context + LLM summarization
            {
                let ctx_state = bg_state.clone();
                tokio::spawn(async move {
                    use neomind_storage::MemoryConfig;

                    // Wait for system to stabilize
                    tokio::time::sleep(Duration::from_secs(30)).await;

                    let config = MemoryConfig::load();
                    let context_interval = config.system_context_interval_secs.max(60);
                    let summary_interval = config.summary_interval_secs.max(600);

                    let mut context_timer =
                        tokio::time::interval(Duration::from_secs(context_interval));
                    let mut summary_timer =
                        tokio::time::interval(Duration::from_secs(summary_interval));

                    tracing::info!(
                        context_interval_secs = context_interval,
                        summary_interval_secs = summary_interval,
                        "System context background task started"
                    );

                    loop {
                        tokio::select! {
                            _ = context_timer.tick() => {
                                let context = crate::server::system_context::gather_system_context(&ctx_state).await;

                                if let Err(e) = ctx_state.agents.system_memory_store
                                    .replace_marker_section("knowledge", "system-context", &context)
                                    .await
                                {
                                    tracing::warn!(error = %e, "Failed to update system context");
                                }
                            }
                            _ = summary_timer.tick() => {
                                // Reload config each tick to pick up runtime changes
                                let current_config = MemoryConfig::load();
                                let llm_result = async {
                                    let manager = neomind_agent::get_instance_manager().ok()?;
                                    match &current_config.summary_backend_id {
                                        Some(id) => manager.get_runtime(id).await.ok(),
                                        None => manager.get_active_runtime().await.ok(),
                                    }
                                }.await;

                                let llm = match llm_result {
                                    Some(rt) => rt,
                                    None => {
                                        tracing::debug!("No active LLM runtime, skipping summary");
                                        continue;
                                    }
                                };

                                let session_store = ctx_state.agents.session_manager.session_store();
                                let _ = crate::server::system_context::summarize_chat_context(
                                    &session_store,
                                    &llm,
                                    &ctx_state.agents.system_memory_store,
                                ).await;

                                let _ = crate::server::system_context::summarize_agent_context(
                                    &ctx_state.agents.agent_store,
                                    &llm,
                                    &ctx_state.agents.system_memory_store,
                                ).await;
                            }
                        }
                    }
                });
            }

            // Initialize device adapters (MQTT, Webhook, etc.)
            bg_state.init_device_adapters().await;

            tracing::info!(
                elapsed_ms = t_bg.elapsed().as_millis() as u64,
                "Background services init complete"
            );
        });
    }

    // Services phase
    startup.phase_services();

    // Run with graceful shutdown.
    // `into_make_service_with_connect_info` populates `ConnectInfo<SocketAddr>` for
    // all handlers — required by webhook IP allow/block lists, rate-limit client_id
    // extraction, and per-IP discovery throttling. Without this, Optional
    // `ConnectInfo` extractors silently receive `None` and every IP-based security
    // control degrades to no-op.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
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
/// Uses port 9375 by default to avoid conflicts with common applications.
/// Binds to 0.0.0.0 to allow LAN access.
/// Port can be configured via config.toml [server] section or NEOMIND_PORT env var.
pub async fn start_server() -> anyhow::Result<()> {
    // rustls 0.23 (via reqwest's rustls-tls-...-no-provider build) does NOT
    // auto-select a process-level CryptoProvider. Install ring before any TLS
    // use (MQTT adapter, HTTPS LLM calls) or the first TLS op panics:
    // "Could not automatically determine the process-level CryptoProvider".
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (host, port) = crate::config::get_server_config();
    let bind: SocketAddr = format!("{}:{}", host, port).parse()?;
    run(bind).await
}
