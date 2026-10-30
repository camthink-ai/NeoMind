//! Web server for Edge AI Agent.
//!
//! This provides a web interface with WebSocket support for chat
//! and REST API for devices, rules, alerts, and session management.

pub mod assets;
pub mod extension_metrics;
pub mod image_cleanup;
pub mod install_service;
pub mod middleware;
pub mod paths;
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
use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use neomind_storage::ExtensionStore;
use tower_http::timeout::{RequestBodyTimeoutLayer, TimeoutLayer};

/// Start the web server on a specific address.
/// This is the main entry point for running the server.
/// Recorded at startup so URL resolution can tell whether the server is
/// reachable on the LAN (0.0.0.0) or only locally (127.0.0.1/localhost).
static HTTP_BIND_LOOPBACK: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static HTTP_BIND_PORT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);

/// Whether the HTTP server is bound to loopback only.
pub fn http_bind_is_loopback() -> bool {
    HTTP_BIND_LOOPBACK.load(std::sync::atomic::Ordering::Relaxed)
}

/// The bound HTTP port (0 if not yet bound).
pub fn http_bind_port() -> u16 {
    HTTP_BIND_PORT.load(std::sync::atomic::Ordering::Relaxed)
}

pub async fn run(bind: SocketAddr) -> anyhow::Result<()> {
    // rustls 0.23 has no auto-selected process CryptoProvider; install ring
    // up front (idempotent) — the CLI `serve` path reaches run() directly and
    // would otherwise panic on first TLS use (e.g. the TLS proxy below).
    let _ = rustls::crypto::ring::default_provider().install_default();

    HTTP_BIND_LOOPBACK.store(
        bind.ip().is_loopback(),
        std::sync::atomic::Ordering::Relaxed,
    );
    HTTP_BIND_PORT.store(bind.port(), std::sync::atomic::Ordering::Relaxed);
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

    // Bootstrap the agent's platform-command auth: provision an internal
    // wildcard API key if none exists, so the chat agent's HTTP-path CLI
    // commands work out of the box on fresh installs (see
    // AuthState::ensure_internal_api_key).
    state.auth.api_key_state.ensure_internal_api_key().await;

    // Session retention: prune chat sessions older than the configured
    // window (settings → retention → session_retention_hours; default OFF).
    // Hourly cadence, config re-read each cycle so runtime changes apply.
    {
        let state_for_retention = state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(3600));
            loop {
                interval.tick().await;
                let hours = neomind_storage::SettingsStore::open_default()
                    .ok()
                    .and_then(|s| s.get_retention_config().session_retention_hours);
                let Some(hours) = hours else { continue };
                let session_store = state_for_retention.agents.session_manager.session_store();
                match session_store.cleanup_old_sessions(hours as i64 * 3600) {
                    Ok(n) if n > 0 => tracing::info!(
                        category = "storage",
                        n,
                        hours,
                        "Session retention pass complete"
                    ),
                    Ok(_) => {}
                    Err(e) => tracing::warn!(
                        category = "storage",
                        error = %e,
                        "Session retention pass failed"
                    ),
                }
            }
        });
    }

    // Split-brain guard: with NEOMIND_DATA_DIR set, stores whose file only
    // exists at the legacy cwd-relative data/ get silently redirected there
    // (upgrade compat — see neomind_core::paths). Half-env/half-legacy is a
    // data-loss illusion waiting to happen, so surface it as ONE prominent
    // error-level banner instead of scattered per-file warns. Strict mode
    // (NEOMIND_STRICT_DATA_DIR) never redirects at all.
    // NOTE: placed after ALL init_* phases — stores resolve lazily through
    // init (device/telemetry/llm/rules/automations/data-push among them), so
    // checking earlier under-reports.
    let redirected = neomind_core::paths::legacy_redirected_stores();
    if !redirected.is_empty() {
        tracing::error!(
            files = ?redirected,
            data_dir = %neomind_core::paths::data_dir().display(),
            "DATA-DIR SPLIT: NEOMIND_DATA_DIR is set, but the stores above were \
             found ONLY at the legacy cwd-relative ./data/ and are being used \
             from there. Set NEOMIND_STRICT_DATA_DIR to disable the fallback, \
             or move the files into the data dir to consolidate."
        );
    }

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

    // TCP_NODELAY on the LISTENING socket: accepted connections inherit it on
    // Linux/macOS. Without it, per-message WebSocket writes on the video push
    // path interlock with the receiver's delayed ACKs (~40-200ms stall per
    // message) — a fast link delivers only ~6fps of 20fps pushed frames.
    let socket = socket2::Socket::new(
        socket2::Domain::for_address(bind),
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )?;
    socket.set_reuse_address(true)?;
    socket.set_nodelay(true)?;
    socket.bind(&bind.into())?;
    socket.listen(1024)?;
    // std/socket2 sockets are blocking by default; tokio requires a
    // non-blocking fd or registration panics ("Registering a blocking socket
    // with the tokio runtime is unsupported" — seen on macOS 15+).
    socket.set_nonblocking(true)?;
    let listener = tokio::net::TcpListener::from_std(socket.into())?;

    // Ready phase — HTTP listener is bound
    startup.phase_ready();
    startup.ready_info(&bind.to_string());

    tracing::info!(
        elapsed_ms = t_start.elapsed().as_millis() as u64,
        "HTTP listener ready (time to serve)"
    );

    // ── Optional TLS front (secure-context enablement) ─────────────────────
    // Browsers gate getUserMedia (device camera) and the clipboard API on
    // secure contexts. Edge deployments are usually reached over plain HTTP
    // on a LAN IP, so we offer an in-process TLS reverse proxy: TLS on
    // NEOMIND_TLS_PORT (default 9376) → loopback to the plaintext listener.
    // Enable by setting NEOMIND_TLS_CERT + NEOMIND_TLS_KEY (PEM paths).
    if let (Ok(cert), Ok(key)) = (
        std::env::var("NEOMIND_TLS_CERT"),
        std::env::var("NEOMIND_TLS_KEY"),
    ) {
        let tls_port: u16 = std::env::var("NEOMIND_TLS_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(9376);
        let target = format!("127.0.0.1:{}", bind.port());
        tokio::spawn(async move {
            match run_tls_proxy(&cert, &key, tls_port, &target).await {
                Ok(()) => unreachable!("tls proxy loop never returns Ok"),
                Err(e) => tracing::error!(error = %e, port = tls_port, "TLS proxy terminated"),
            }
        });
    }

    // ── Phase B: Deferred background services (after listener starts serving) ──
    // These run in the background and do not block HTTP serving.

    // Initialize extension metrics collector (decoupled from device system)
    let runtime = state.extensions.runtime.clone();
    let metrics_storage = state.extensions.metrics_storage.clone();
    // Saved extension configs so the collector's self-heal reload restores
    // configuration instead of resetting the extension to defaults.
    let extension_config_store = state.extensions.store.clone();
    let event_bus_for_metrics = state.core.event_bus.clone();
    tokio::spawn(async move {
        use crate::server::extension_metrics::ExtensionMetricsCollector;
        use std::time::Duration;

        let mut collector = ExtensionMetricsCollector::new(runtime, metrics_storage)
            .with_interval(Duration::from_secs(60))
            .with_config_store(extension_config_store);

        if let Some(bus) = event_bus_for_metrics {
            collector = collector.with_event_bus(bus);
        }

        collector.run().await;
    });

    // Start telemetry retention cleanup background task
    {
        let session_store = state.agents.session_manager.session_store();
        let agent_store = state.agents.agent_store.clone();
        tokio::spawn(async move {
            use neomind_storage::{SettingsStore, TimeSeriesStore};

            // Wait for server to initialize
            tokio::time::sleep(Duration::from_secs(10)).await;

            loop {
                // Load config on each cycle so runtime changes take effect.
                // [observability] Both reopen failures used to be silent —
                // retention could no-op forever with no trace of why.
                let config = SettingsStore::open_default()
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
                    let ts_store = match TimeSeriesStore::open(neomind_core::paths::store_path(
                        "telemetry.redb",
                    )) {
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

                    // Clean up expired image files. Files still referenced by a
                    // conversation or agent journal survive — deleting them on the
                    // image schedule turned history markdown into 404s.
                    if let Some(image_retention_hours) = config.image_retention {
                        // Canonical data dir (paths::data_dir treats an
                        // EMPTY env var as unset; a raw env read here treated
                        // it as "" and joined "./images").
                        let images_dir = neomind_core::paths::data_dir().join("images");

                        let protected =
                            crate::server::image_cleanup::collect_referenced_image_files(
                                &session_store,
                                &agent_store,
                            )
                            .await;

                        match crate::server::image_cleanup::cleanup_expired_images_with_protection(
                            &images_dir,
                            image_retention_hours,
                            &protected,
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
                            // The manager's crash-restart rebuilt its own map;
                            // swap the proxy registry to the fresh instance
                            // FIRST — otherwise session/init calls keep
                            // hitting the dead proxy ("Extension not
                            // running") and even send_config_update below
                            // goes through the stale handle.
                            if let Err(e) = rt.refresh_proxy(&ext_id).await {
                                tracing::warn!(
                                    extension_id = %ext_id,
                                    error = %e,
                                    "Failed to refresh proxy after crash recovery"
                                );
                            }
                            if let Ok(store) = ExtensionStore::open(crate::server::paths::extension_store_path()) {
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

            // Record error in storage when crash recovery restart fails, and
            // surface it through the notification channels — without the
            // message, a circuit-broken extension just quietly stops working
            // ("feature silently gone" from the extension-system review).
            let crash_notify_state = bg_state.clone();
            bg_state
                .extensions
                .runtime
                .set_on_crash_recovery_failed(Arc::new(
                    move |extension_id: &str, error: &str| {
                        let ext_id = extension_id.to_string();
                        let err_msg = error.to_string();
                        let notify = crash_notify_state.clone();
                        tokio::spawn(async move {
                            if let Ok(store) = ExtensionStore::open(crate::server::paths::extension_store_path()) {
                                let _ = store.update_error_status(&ext_id, &err_msg);
                            }
                            let title = format!("Extension '{ext_id}' stopped auto-restarting");
                            let body = format!(
                                "Extension '{ext_id}' crashed repeatedly and auto-restart has been \
                                 disabled ({err_msg}). It will not recover on its own — check \
                                 Settings → Extensions to view the crash reason or restart it manually."
                            );
                            if let Err(e) = notify
                                .core
                                .message_manager
                                .system_message(title, body)
                                .await
                            {
                                tracing::warn!(
                                    extension_id = %ext_id,
                                    error = %e,
                                    "Failed to send crash-recovery notification"
                                );
                            }
                        });
                    },
                ));

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

            // Builtin LLM(内置 LFM2.5-2.6B):定位已下载模型 → spawn → 注册实例。
            // 独立 task 运行:不阻塞后续启动(wait_healthy 最多等 60s)。
            {
                let bg_state_for_builtin = bg_state.clone();
                tokio::spawn(async move {
                    // API-owned engine knobs persist here and win over the
                    // systemd env on service restart (2026-10-30: ctx 32768
                    // reverted to 8192 and every turn failed).
                    crate::builtin_llm::config::set_engine_overrides_path(
                        bg_state_for_builtin.data_dir.join("builtin_engine.json"),
                    );
                    let mut cfg = crate::builtin_llm::config::BuiltinConfig::from_env();
                    let data_dir = bg_state_for_builtin.data_dir.clone();
                    // 镜像 auto_register 守卫:get_instance_manager 启动初期可能暂不可用,
                    // 重试到可用(与上面 llama.cpp 块相同的 12×5s 节奏)。
                    let mut retry_interval = tokio::time::interval(Duration::from_secs(5));
                    for _ in 0..12 {
                        retry_interval.tick().await;
                        let _engine_change =
                            crate::builtin_llm::config::ENGINE_CHANGE_LOCK.lock().await;
                        cfg = crate::builtin_llm::config::BuiltinConfig::from_env();
                        if let Ok(manager) = neomind_agent::get_instance_manager() {
                            match crate::builtin_llm::state::bootstrap(&data_dir, &cfg, &manager)
                                .await
                            {
                                crate::builtin_llm::state::BootstrapOutcome::ServerReady {
                                    endpoint,
                                } => {
                                    tracing::info!(endpoint = %endpoint, "Builtin LLM ready");
                                }
                                crate::builtin_llm::state::BootstrapOutcome::ModelMissing => {
                                    tracing::info!("Builtin LLM model not downloaded yet; UI will guide download");
                                }
                                crate::builtin_llm::state::BootstrapOutcome::Failed(e) => {
                                    tracing::warn!(error = %e, "Builtin LLM bootstrap failed");
                                }
                                _ => {}
                            }
                            break;
                        }
                    }

                    // [llama watchdog] OOM self-heal (2026-10-15 incident on
                    // NG4520: llama-server maps ~47 GB of CUDA address space,
                    // tops the oom_score ranking, and was killed 5× in 17 h;
                    // each death left chat dead with connection-refused until
                    // a MANUAL service restart). bootstrap() is already
                    // idempotent with exactly the right semantics (healthy →
                    // no-op; instance present but server dead → respawn), so
                    // the watchdog just drives it on a slow cadence. The
                    // health-first check keeps the common case one HTTP GET;
                    // consecutive failures back off to cap log noise, and a
                    // missing model never triggers respawn attempts.
                    let mut poll = Duration::from_secs(30);
                    let mut backoff = Duration::from_secs(30);
                    // Consecutive alive-but-unhealthy cycles. A LIVE process
                    // with a dead port is "loading / box under load", not an
                    // OOM death — respawning it on a probe timeout would
                    // double-spawn two ~47 GB mappings and OOM the box
                    // (2026-10-15 near-miss). Only a genuinely wedged server
                    // (5 min unresponsive) gets the kill-then-respawn
                    // escalation.
                    let mut wedged_cycles: u32 = 0;
                    // Consecutive failed starts while an operator pin is in
                    // effect. A pin that cannot start is a proven-impossible
                    // configuration, not a transient; retrying it forever was
                    // the 2026-10-30 crash loop. After 2 failures this process
                    // drops the pin so the auto-sizer can degrade (partial
                    // offload / CPU) — the systemd file is NOT modified, so a
                    // service restart still honors the operator's pin.
                    let mut pinned_failures: u32 = 0;
                    let mut recovery = crate::builtin_llm::config::WatchdogRecovery::new(&cfg);
                    loop {
                        tokio::time::sleep(poll).await;
                        let _engine_change =
                            crate::builtin_llm::config::ENGINE_CHANGE_LOCK.lock().await;
                        let (latest, pins_changed) =
                            recovery.refresh(crate::builtin_llm::config::BuiltinConfig::from_env());
                        cfg = latest;
                        if pins_changed {
                            pinned_failures = 0;
                        }
                        if !cfg.enabled {
                            continue;
                        }
                        if crate::builtin_llm::server::health_check(cfg.port).await {
                            backoff = Duration::from_secs(30);
                            wedged_cycles = 0;
                            continue;
                        }
                        // Port dead. Only an incident when a model is
                        // actually installed — otherwise "stopped" is the
                        // normal not-configured state.
                        let mdir = crate::builtin_llm::handlers::models_dir(&data_dir);
                        if crate::builtin_llm::handlers::installed_model(&mdir).is_none() {
                            continue;
                        }
                        if crate::builtin_llm::server::any_llama_server_alive() {
                            wedged_cycles += 1;
                            if wedged_cycles < 10 {
                                // Alive but not answering yet — wait it out.
                                tracing::debug!(
                                    port = cfg.port,
                                    wedged_cycles,
                                    "builtin llm watchdog: llama-server alive but not answering — waiting"
                                );
                                continue;
                            }
                            tracing::warn!(
                                port = cfg.port,
                                "builtin llm watchdog: llama-server alive but unresponsive for ~5min — killing and respawning"
                            );
                            crate::builtin_llm::server::stop_all_llama_servers();
                            wedged_cycles = 0;
                        } else {
                            wedged_cycles = 0;
                            // Process gone (the OOM case): clear its stale
                            // registry entry before the replacement lands.
                            crate::builtin_llm::server::stop_all_llama_servers();
                        }
                        let Ok(manager) = neomind_agent::get_instance_manager() else {
                            continue;
                        };
                        tracing::warn!(
                            port = cfg.port,
                            next_backoff_secs = backoff.as_secs(),
                            "builtin llm watchdog: llama-server not answering — attempting respawn"
                        );
                        match crate::builtin_llm::state::bootstrap(&data_dir, &cfg, &manager).await
                        {
                            crate::builtin_llm::state::BootstrapOutcome::ServerReady {
                                endpoint,
                            } => {
                                tracing::info!(
                                    endpoint = %endpoint,
                                    "builtin llm watchdog: llama-server revived"
                                );
                                pinned_failures = 0;
                                backoff = Duration::from_secs(30);
                            }
                            crate::builtin_llm::state::BootstrapOutcome::ServerAlreadyRunning => {
                                // Someone else (restart API / race) fixed it.
                                pinned_failures = 0;
                                backoff = Duration::from_secs(30);
                            }
                            crate::builtin_llm::state::BootstrapOutcome::Failed(e) => {
                                if cfg.ngl.is_some() || cfg.vlm_ngl.is_some() {
                                    pinned_failures = pinned_failures.saturating_add(1);
                                    if pinned_failures >= 2 {
                                        tracing::error!(
                                            pinned_ngl = ?cfg.ngl,
                                            pinned_vlm_ngl = ?cfg.vlm_ngl,
                                            failures = pinned_failures,
                                            error = %e,
                                            "builtin llm watchdog: pinned NGL failed repeated starts — \
                                             clearing the pin for respawns in THIS process so the \
                                             auto-sizer can degrade; the systemd config is unchanged \
                                             (remove NEOMIND_BUILTIN_MODEL_NGL/_VLM_NGL to fix it \
                                             permanently)"
                                        );
                                        recovery.degrade();
                                        cfg.ngl = None;
                                        cfg.vlm_ngl = None;
                                        pinned_failures = 0;
                                    }
                                }
                                tracing::warn!(
                                    error = %e,
                                    next_backoff_secs = backoff.as_secs(),
                                    "builtin llm watchdog: respawn failed — backing off"
                                );
                                backoff = std::cmp::min(backoff * 2, Duration::from_secs(300));
                            }
                            // ModelMissing mid-loop: model was uninstalled —
                            // stop attempting, the outer enabled/model checks
                            // handle re-arming.
                            _ => {
                                backoff = Duration::from_secs(30);
                            }
                        }
                        poll = backoff;
                    }
                });
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

                    // Both deadlines start "now", so the first gather and the
                    // first summary run right after stabilization.
                    let mut next_context_at = tokio::time::Instant::now();
                    let mut next_summary_at = tokio::time::Instant::now();

                    tracing::info!(
                        "System context background task started (intervals hot-reload from memory config)"
                    );

                    loop {
                        // Reload the config EVERY iteration so runtime changes
                        // to the intervals take effect on the next tick. The
                        // previous interval()-based timers captured both
                        // intervals once at startup — while a comment inside
                        // the tick claimed reloads worked.
                        let config = MemoryConfig::load();
                        let context_interval =
                            Duration::from_secs(config.system_context_interval_secs.max(60));
                        let summary_interval =
                            Duration::from_secs(config.summary_interval_secs.max(600));

                        tokio::select! {
                            _ = tokio::time::sleep_until(next_context_at) => {
                                next_context_at = tokio::time::Instant::now() + context_interval;
                                let context = crate::server::system_context::gather_system_context(&ctx_state).await;

                                if let Err(e) = ctx_state.agents.system_memory_store
                                    .replace_marker_section("knowledge", "system-context", &context)
                                    .await
                                {
                                    tracing::warn!(error = %e, "Failed to update system context");
                                }
                            }
                            _ = tokio::time::sleep_until(next_summary_at) => {
                                next_summary_at = tokio::time::Instant::now() + summary_interval;

                                let llm_result = async {
                                    let manager = neomind_agent::get_instance_manager().ok()?;
                                    match &config.summary_backend_id {
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

    // Periodic data-dir backup (see neomind_storage::backup). Runs the same
    // create_backup path as POST /api/settings/backup.
    spawn_backup_scheduler(state.data_dir.clone());

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
    .with_graceful_shutdown(crate::shutdown::shutdown_signal_or_test_deadline())
    .await?;

    // Clean up resources after server shuts down
    crate::shutdown::cleanup_resources(&state_for_cleanup).await;

    tracing::info!("Server shutdown complete");
    Ok(())
}

/// Periodic data-directory backup. The schedule lives in settings.redb
/// (`/api/settings/backup-config`, editable in Settings → Preferences) and
/// is re-read every tick so changes apply within a minute — env vars only
/// seed the default before anything is saved. The first full interval after
/// boot is skipped so a fresh start does not copy databases while services
/// are still warming up.
fn spawn_backup_scheduler(data_dir: std::path::PathBuf) {
    tracing::info!(category = "backup", "Periodic backup scheduler started");

    tokio::spawn(async move {
        // Read the schedule off the (blocking) settings store. The settings
        // path derives from the SAME data_dir being backed up — the store
        // path used to be hardcoded "data/settings.redb", so a deployment
        // with NEOMIND_DATA_DIR pointing elsewhere read one directory's
        // config while backing up another (found by the pre-release audit).
        let settings_dir = data_dir.clone();
        let read_config = move || {
            let settings_path = settings_dir.join("settings.redb");
            tokio::task::spawn_blocking(move || {
                use neomind_storage::settings::BackupConfig;
                neomind_storage::SettingsStore::open(settings_path)
                    .ok()
                    .and_then(|s| s.load_backup_config().ok().flatten())
                    .unwrap_or_else(BackupConfig::from_env_or_default)
            })
        };

        let initial = read_config().await.unwrap_or_default();
        // Grace: first backup one full interval after boot, not immediately.
        let mut next_run =
            tokio::time::Instant::now() + std::time::Duration::from_secs(initial.interval_secs);
        let mut last_config = initial;

        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;

            let config = read_config().await.unwrap_or(last_config.clone());
            if config != last_config {
                // Schedule edit: re-arm the timer off the NEW interval so a
                // shortened interval doesn't wait out the old one.
                next_run = tokio::time::Instant::now()
                    + std::time::Duration::from_secs(config.interval_secs);
                tracing::info!(
                    category = "backup",
                    enabled = config.enabled,
                    interval_secs = config.interval_secs,
                    keep = config.keep,
                    "Backup schedule changed; timer re-armed"
                );
                last_config = config.clone();
            }
            if !config.enabled || tokio::time::Instant::now() < next_run {
                continue;
            }
            next_run =
                tokio::time::Instant::now() + std::time::Duration::from_secs(config.interval_secs);

            let dir = data_dir.clone();
            let keep = config.keep;
            let result = tokio::task::spawn_blocking(move || {
                match neomind_storage::backup::create_backup(&dir, env!("CARGO_PKG_VERSION")) {
                    Ok(manifest) => {
                        let pruned = neomind_storage::backup::prune_backups(&dir, keep);
                        Ok((manifest, pruned))
                    }
                    Err(e) => Err(e),
                }
            })
            .await;
            match result {
                Ok(Ok((manifest, pruned))) => tracing::info!(
                    category = "backup",
                    id = %manifest.id,
                    pruned,
                    "Scheduled backup complete"
                ),
                Ok(Err(e)) => tracing::warn!(
                    category = "backup",
                    error = %e,
                    "Scheduled backup failed"
                ),
                Err(e) => tracing::warn!(
                    category = "backup",
                    error = %e,
                    "Scheduled backup task join error"
                ),
            }
        }
    });
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

// ============================================================================
// TLS reverse proxy (secure-context enablement for LAN deployments)
// ============================================================================

/// In-process TLS front: accepts rustls connections on `port`, forwards them
/// byte-for-byte to the plaintext listener at `target` (loopback). Same
/// TCP_NODELAY-on-listener trick as the main listener — WS push latency
/// depends on it.
async fn run_tls_proxy(
    cert_pem: &str,
    key_pem: &str,
    port: u16,
    target: &str,
) -> anyhow::Result<()> {
    let certs: Vec<rustls::pki_types::CertificateDer<'static>> = {
        let f = std::fs::File::open(cert_pem)?;
        let mut r = std::io::BufReader::new(f);
        rustls_pemfile::certs(&mut r).collect::<Result<_, _>>()?
    };
    let key = {
        let f = std::fs::File::open(key_pem)?;
        let mut r = std::io::BufReader::new(f);
        rustls_pemfile::private_key(&mut r)?
            .ok_or_else(|| anyhow::anyhow!("no private key in {key_pem}"))?
    };
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    let acceptor = tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(config));

    let addr: SocketAddr = format!("0.0.0.0:{port}").parse()?;
    let socket = socket2::Socket::new(
        socket2::Domain::for_address(addr),
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )?;
    socket.set_reuse_address(true)?;
    socket.set_nodelay(true)?;
    socket.bind(&addr.into())?;
    socket.listen(1024)?;
    // std/socket2 sockets are blocking by default; tokio requires a
    // non-blocking fd or registration panics INSIDE the spawned task —
    // the server keeps running, the proxy dies silently with no log. Same
    // bug as the main listener (fixed there in a474ef03); this second
    // call site was missed and left the whole TLS front non-functional.
    socket.set_nonblocking(true)?;
    let listener = tokio::net::TcpListener::from_std(socket.into())?;

    tracing::info!(port, %target, "TLS proxy listening (secure-context front)");
    loop {
        let (stream, _peer) = listener.accept().await?;
        let _ = stream.set_nodelay(true);
        let acceptor = acceptor.clone();
        let target = target.to_string();
        tokio::spawn(async move {
            let tls = match acceptor.accept(stream).await {
                Ok(t) => t,
                Err(_) => return,
            };
            let mut upstream = match tokio::net::TcpStream::connect(&target).await {
                Ok(u) => u,
                Err(_) => return,
            };
            let _ = upstream.set_nodelay(true);
            let mut tls = tls;
            let _ = tokio::io::copy_bidirectional(&mut tls, &mut upstream).await;
        });
    }
}
