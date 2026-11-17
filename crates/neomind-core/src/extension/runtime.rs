//! Extension runtime service.
//!
//! This is the single host-side entry point for extension lifecycle and command
//! execution. All extensions run in isolated mode; the registry is only kept as
//! an internal proxy store for streaming support.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::extension::isolated::{IsolatedExtensionManager, IsolatedManagerConfig};
use crate::extension::registry::ExtensionRegistry;
use crate::extension::system::{ExtensionError, ExtensionMetadata, ExtensionMetricValue};

/// Runtime configuration for isolated extensions.
#[derive(Debug, Clone)]
pub struct ExtensionRuntimeConfig {
    /// Configuration for isolated extensions.
    pub isolated_config: IsolatedManagerConfig,
}

impl Default for ExtensionRuntimeConfig {
    fn default() -> Self {
        Self {
            isolated_config: IsolatedManagerConfig::default(),
        }
    }
}

/// Runtime view of a loaded extension (load snapshot).
#[derive(Debug, Clone)]
pub struct ExtensionRuntimeInfo {
    /// Extension metadata.
    pub metadata: ExtensionMetadata,
    /// Always `true` in the new runtime.
    pub is_isolated: bool,
    /// Whether the extension is currently running.
    pub is_running: bool,
    /// Path to the extension binary.
    pub path: Option<PathBuf>,
    /// Metrics provided by this extension.
    pub metrics: Vec<super::system::MetricDescriptor>,
    /// Commands provided by this extension.
    pub commands: Vec<super::system::ExtensionCommand>,
    /// Capability generation captured with this load (same snapshot as commands).
    pub capability_generation: super::CapabilityGeneration,
    /// Consecutive crashes (crash-loop counter, reset on a stable start).
    /// Non-zero while stopped ⇒ the extension crashed rather than being
    /// stopped on purpose.
    pub consecutive_crashes: u32,
    /// Human-readable reason for the last crash.
    pub last_crash_reason: Option<String>,
}

/// Single-path extension runtime.
pub struct ExtensionRuntime {
    proxy_registry: Arc<ExtensionRegistry>,
    isolated_manager: Arc<IsolatedExtensionManager>,
    config: ExtensionRuntimeConfig,
}

impl ExtensionRuntime {
    /// Create a new runtime.
    pub fn new(proxy_registry: Arc<ExtensionRegistry>, config: ExtensionRuntimeConfig) -> Self {
        let isolated_manager = Arc::new(IsolatedExtensionManager::new(
            config.isolated_config.clone(),
        ));

        let event_dispatcher = isolated_manager.event_dispatcher();
        proxy_registry.set_event_dispatcher(event_dispatcher);

        Self {
            proxy_registry,
            isolated_manager,
            config,
        }
    }

    /// Create with default configuration.
    pub fn with_defaults(proxy_registry: Arc<ExtensionRegistry>) -> Self {
        Self::new(proxy_registry, ExtensionRuntimeConfig::default())
    }

    /// Start background crash monitoring.
    pub fn start_death_monitoring(self: Arc<Self>) {
        self.isolated_manager.clone().start_death_monitoring();
    }

    /// Set a callback to be invoked after crash recovery restart.
    /// The callback receives (extension_id, extension_path) and can apply saved config, etc.
    #[allow(clippy::type_complexity)]
    pub fn set_on_crash_recovery_restart(
        &self,
        callback: Arc<dyn Fn(&str, &std::path::Path) + Send + Sync>,
    ) {
        self.isolated_manager
            .set_on_crash_recovery_restart(callback);
    }

    /// Set a callback to be invoked when crash recovery restart fails.
    /// The callback receives (extension_id, error_message).
    #[allow(clippy::type_complexity)]
    pub fn set_on_crash_recovery_failed(&self, callback: Arc<dyn Fn(&str, &str) + Send + Sync>) {
        self.isolated_manager.set_on_crash_recovery_failed(callback);
    }

    /// Get the event dispatcher used for extension event delivery.
    pub fn get_event_dispatcher(&self) -> Arc<crate::extension::EventDispatcher> {
        self.isolated_manager.event_dispatcher()
    }

    /// Load an extension and register a streaming proxy if available.
    pub async fn load(&self, path: &Path) -> Result<ExtensionMetadata, ExtensionError> {
        let metadata = self
            .isolated_manager
            .load(path)
            .await
            .map_err(|e| ExtensionError::LoadFailed(e.to_string()))?;

        if let Some(isolated) = self.isolated_manager.get(&metadata.id).await {
            let descriptor = isolated.descriptor().await;
            let proxy = if let Some(desc) = descriptor {
                super::proxy::create_proxy_with_descriptor(isolated, desc)
            } else {
                super::proxy::create_proxy(isolated)
            };
            self.proxy_registry
                .register(metadata.id.clone(), proxy)
                .await?;
            // `register` mints the capability generation token.
        }

        Ok(metadata)
    }

    /// Unload an extension and remove its streaming proxy.
    pub async fn unload(&self, id: &str) -> Result<(), ExtensionError> {
        // Drop version-bound write-gate cache so a reload/upgrade cannot
        // reuse a stale ReadOnly grant against a newer mutating command.
        super::invalidate_instance_capability(id);

        self.isolated_manager
            .unload(id)
            .await
            .map_err(|e| ExtensionError::ExecutionFailed(e.to_string()))?;

        if self.proxy_registry.contains(id).await {
            let _ = self.proxy_registry.unregister(id).await;
        }

        Ok(())
    }

    /// Rebuild + re-register an extension's proxy after the isolated
    /// manager restarted its runner process. The manager's crash-restart
    /// path reloads into ITS map only — the proxy registry keeps serving
    /// the OLD (dead) instance, so every stream-session call fails
    /// "Extension not running" until the whole serve restarts (observed
    /// repeatedly on 2026-09-09: runner respawned fine, sessions dead).
    pub async fn refresh_proxy(&self, id: &str) -> Result<(), ExtensionError> {
        let file_path = self
            .proxy_registry
            .get_info(id)
            .await
            .and_then(|i| i.metadata.file_path.clone());

        let Some(isolated) = self.isolated_manager.get(id).await else {
            return Err(ExtensionError::LoadFailed(format!(
                "extension {id} not present in isolated manager"
            )));
        };

        let descriptor = isolated.descriptor().await;
        let proxy = if let Some(desc) = descriptor {
            super::proxy::create_proxy_with_descriptor(isolated, desc)
        } else {
            super::proxy::create_proxy(isolated)
        };

        // Swap under unregister+register: `register` refuses existing ids.
        if self.proxy_registry.contains(id).await {
            let _ = self.proxy_registry.unregister(id).await;
        }
        self.proxy_registry
            .register_with_path(id.to_string(), proxy, file_path)
            .await
    }

    /// Alias for unload used by API handlers.
    pub async fn unregister(&self, id: &str) -> Result<(), ExtensionError> {
        self.unload(id).await
    }

    /// Execute a command on an extension (isolated process by id).
    ///
    /// Prefer [`Self::execute_prepared`] when a [`super::PreparedInvoke`] is
    /// already in hand — that path refuses stale generations after reload.
    pub async fn execute_command(
        &self,
        id: &str,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ExtensionError> {
        self.isolated_manager
            .execute_command(id, command, args)
            .await
            .map_err(|e| ExtensionError::ExecutionFailed(e.to_string()))
    }

    /// Execute a prepared invoke against the **bound** load-snapshot instance.
    ///
    /// Delegates to [`ExtensionRegistry::execute_prepared`] on the proxy
    /// registry (same Arc as host `ExtensionState.registry`). That path
    /// checks generation and then calls the snapshot's `instance` handle —
    /// it must **never** re-query `isolated_manager` by id after the check,
    /// or a reload between check and lookup would enter the new process.
    pub async fn execute_prepared(
        &self,
        prepared: &super::PreparedInvoke,
    ) -> Result<serde_json::Value, ExtensionError> {
        self.proxy_registry.execute_prepared(prepared).await
    }

    /// Get metrics from an extension.
    pub async fn get_metrics(&self, id: &str) -> Vec<ExtensionMetricValue> {
        match self.isolated_manager.get_metrics(id).await {
            Ok(metrics) => metrics,
            Err(e) => {
                tracing::warn!(
                    extension_id = %id,
                    error = %e,
                    "Failed to get extension metrics"
                );
                vec![]
            }
        }
    }

    /// Refresh an extension's cached descriptor so runtime-discovered
    /// dynamic metrics become visible through `/api/extensions`.
    /// Failures are logged and swallowed — the caller (collector) treats
    /// this as best-effort and continues with the previous cache.
    pub async fn refresh_descriptor(&self, id: &str) -> Result<(), ExtensionError> {
        match self.isolated_manager.refresh_descriptor(id).await {
            Ok(()) => Ok(()),
            Err(e) => {
                tracing::warn!(
                    extension_id = %id,
                    error = %e,
                    "Failed to refresh descriptor (using cached)"
                );
                Err(ExtensionError::ExecutionFailed(e.to_string()))
            }
        }
    }

    /// Check extension health.
    pub async fn health_check(&self, id: &str) -> Result<bool, ExtensionError> {
        self.isolated_manager
            .health_check(id)
            .await
            .map_err(|e| ExtensionError::ExecutionFailed(e.to_string()))
    }

    /// Send config hot-reload update to a running extension.
    pub async fn send_config_update(
        &self,
        id: &str,
        config: &serde_json::Value,
    ) -> Result<(), ExtensionError> {
        self.isolated_manager
            .send_config_update(id, config)
            .await
            .map_err(|e| ExtensionError::ExecutionFailed(e.to_string()))
    }

    /// Get extension statistics.
    pub async fn get_stats(
        &self,
        id: &str,
    ) -> Result<crate::extension::system::ExtensionStats, ExtensionError> {
        self.isolated_manager
            .get_stats(id)
            .await
            .map_err(|e| ExtensionError::ExecutionFailed(e.to_string()))
    }

    /// Get extension log entries.
    pub async fn get_logs(
        &self,
        id: &str,
    ) -> Result<Vec<crate::extension::isolated::ExtensionLogEntry>, ExtensionError> {
        self.isolated_manager
            .get_logs(id)
            .await
            .map_err(|e| ExtensionError::ExecutionFailed(e.to_string()))
    }

    /// Clear extension log entries.
    pub async fn clear_logs(&self, id: &str) -> Result<(), ExtensionError> {
        self.isolated_manager
            .clear_logs(id)
            .await
            .map_err(|e| ExtensionError::ExecutionFailed(e.to_string()))
    }

    /// Get active stream sessions for an extension.
    pub async fn get_active_sessions(&self, id: &str) -> Vec<String> {
        self.isolated_manager
            .get_active_sessions(id)
            .await
            .unwrap_or_default()
    }

    /// Get event subscriptions for an extension.
    pub async fn get_event_subscriptions(&self, id: &str) -> Vec<String> {
        self.isolated_manager
            .get_event_subscriptions(id)
            .await
            .unwrap_or_default()
    }

    /// Check if an extension is loaded.
    pub async fn contains(&self, id: &str) -> bool {
        self.isolated_manager.contains(id).await
    }

    /// Get extension info.
    ///
    /// Descriptor fields (metadata/metrics/commands) AND the capability
    /// generation are taken from ONE registry entry — the previous shape
    /// read the descriptor here and the token from the registry with an
    /// await in between, so a reload could return "old descriptor + new
    /// token" in a single info object. Runtime status (is_running/crashes)
    /// is separately-sourced liveness data, which never feeds the
    /// capability gate.
    pub async fn get_info(&self, id: &str) -> Option<ExtensionRuntimeInfo> {
        let entry = self.proxy_registry.get_info(id).await?;
        let runtime_status = self.isolated_manager.get_info(id)?;
        Some(ExtensionRuntimeInfo {
            metadata: entry.metadata,
            is_isolated: true,
            is_running: runtime_status.runtime.is_running,
            path: Some(runtime_status.path),
            metrics: entry.metrics,
            commands: entry.commands,
            capability_generation: entry.capability_generation,
            consecutive_crashes: runtime_status.runtime.consecutive_crashes,
            last_crash_reason: runtime_status.runtime.last_crash_reason.clone(),
        })
    }

    /// List all loaded extensions.
    ///
    /// Same single-entry rule as [`Self::get_info`]: descriptor + generation
    /// from one registry entry per extension; runtime status separately.
    pub async fn list(&self) -> Vec<ExtensionRuntimeInfo> {
        let mut out = Vec::new();
        for entry in self.proxy_registry.list().await {
            let id = entry.metadata.id.clone();
            let runtime_status = self.isolated_manager.get_info(&id);
            out.push(ExtensionRuntimeInfo {
                metadata: entry.metadata,
                is_isolated: true,
                is_running: runtime_status
                    .as_ref()
                    .map(|i| i.runtime.is_running)
                    .unwrap_or(false),
                path: runtime_status.as_ref().map(|i| i.path.clone()),
                metrics: entry.metrics,
                commands: entry.commands,
                capability_generation: entry.capability_generation,
                consecutive_crashes: runtime_status
                    .as_ref()
                    .map(|i| i.runtime.consecutive_crashes)
                    .unwrap_or(0),
                last_crash_reason: runtime_status
                    .as_ref()
                    .and_then(|i| i.runtime.last_crash_reason.clone()),
            });
        }
        out
    }

    /// Get a specific extension by ID.
    pub async fn get(&self, id: &str) -> Option<ExtensionRuntimeInfo> {
        self.get_info(id).await
    }

    /// Get the number of loaded extensions.
    pub async fn count(&self) -> usize {
        self.isolated_manager.count().await
    }

    /// Extensions are always isolated in the new runtime.
    pub async fn is_isolated(&self, id: &str) -> bool {
        self.contains(id).await
    }

    /// Stop all extensions and clear proxy registrations.
    pub async fn stop_all(&self) {
        let proxy_ids: Vec<String> = self
            .list()
            .await
            .into_iter()
            .map(|info| info.metadata.id)
            .collect();

        self.isolated_manager.stop_all().await;

        for id in proxy_ids {
            if self.proxy_registry.contains(&id).await {
                let _ = self.proxy_registry.unregister(&id).await;
            }
        }
    }

    /// Set the capability provider for extensions.
    pub async fn set_capability_provider(
        &self,
        provider: Arc<dyn super::context::ExtensionCapabilityProvider>,
    ) {
        self.isolated_manager
            .set_capability_provider(provider)
            .await;
    }

    /// Get a proxy extension for streaming operations.
    pub async fn get_extension(
        &self,
        id: &str,
    ) -> Option<Arc<tokio::sync::RwLock<Box<dyn crate::extension::system::Extension>>>> {
        self.proxy_registry.get(id).await
    }

    /// Access the underlying isolated manager.
    pub fn isolated_manager(&self) -> Arc<IsolatedExtensionManager> {
        Arc::clone(&self.isolated_manager)
    }

    /// Get runtime configuration.
    pub fn config(&self) -> &ExtensionRuntimeConfig {
        &self.config
    }
}

#[cfg(test)]
mod bound_execute_tests {
    use super::*;
    use crate::extension::system::{
        Extension, ExtensionCommand, ExtensionMetadata, MetricDefinition,
    };
    use crate::extension::ExtensionRegistry;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::OnceLock;

    /// HTTP/CLI consume `ExtensionRuntime::execute_prepared`. Prove it uses the
    /// registry-bound instance (not isolated_manager id re-lookup): mid-execute
    /// reload must leave the new instance at call count 0.
    #[tokio::test]
    async fn runtime_execute_prepared_survives_mid_execute_reload() {
        static ENTERED_V1: AtomicBool = AtomicBool::new(false);
        static RELEASE_V1: AtomicBool = AtomicBool::new(false);
        static CALLS_V2: AtomicUsize = AtomicUsize::new(0);
        ENTERED_V1.store(false, Ordering::SeqCst);
        RELEASE_V1.store(false, Ordering::SeqCst);
        CALLS_V2.store(0, Ordering::SeqCst);

        struct V1;
        #[async_trait::async_trait]
        impl Extension for V1 {
            fn metadata(&self) -> &ExtensionMetadata {
                static M: OnceLock<ExtensionMetadata> = OnceLock::new();
                M.get_or_init(|| ExtensionMetadata::new("rt-bind", "RT Bind", "1.0.0"))
            }
            fn metrics(&self) -> Vec<MetricDefinition> {
                Vec::new()
            }
            fn commands(&self) -> Vec<ExtensionCommand> {
                vec![ExtensionCommand {
                    name: "ping".into(),
                    display_name: "Ping".into(),
                    description: String::new(),
                    payload_template: String::new(),
                    parameters: Vec::new(),
                    fixed_values: std::collections::HashMap::new(),
                    samples: Vec::new(),
                    parameter_groups: Vec::new(),
                    ai_exposed: true,
                    effect: Some(crate::extension::system::CommandEffect::ReadOnly),
                    replay_safety: Some(crate::extension::system::CommandReplaySafety::Safe),
                    execution: Some(crate::extension::system::CommandExecutionKind::Immediate),
                }]
            }
            async fn execute_command(
                &self,
                _command: &str,
                _args: &serde_json::Value,
            ) -> Result<serde_json::Value, ExtensionError> {
                ENTERED_V1.store(true, Ordering::SeqCst);
                while !RELEASE_V1.load(Ordering::SeqCst) {
                    tokio::task::yield_now().await;
                }
                Ok(serde_json::json!({"v": 1}))
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        struct V2;
        #[async_trait::async_trait]
        impl Extension for V2 {
            fn metadata(&self) -> &ExtensionMetadata {
                static M: OnceLock<ExtensionMetadata> = OnceLock::new();
                M.get_or_init(|| ExtensionMetadata::new("rt-bind", "RT Bind", "2.0.0"))
            }
            fn metrics(&self) -> Vec<MetricDefinition> {
                Vec::new()
            }
            fn commands(&self) -> Vec<ExtensionCommand> {
                vec![ExtensionCommand {
                    name: "ping".into(),
                    display_name: "Ping".into(),
                    description: String::new(),
                    payload_template: String::new(),
                    parameters: Vec::new(),
                    fixed_values: std::collections::HashMap::new(),
                    samples: Vec::new(),
                    parameter_groups: Vec::new(),
                    ai_exposed: true,
                    effect: Some(crate::extension::system::CommandEffect::ReadOnly),
                    replay_safety: Some(crate::extension::system::CommandReplaySafety::Safe),
                    execution: Some(crate::extension::system::CommandExecutionKind::Immediate),
                }]
            }
            async fn execute_command(
                &self,
                _command: &str,
                _args: &serde_json::Value,
            ) -> Result<serde_json::Value, ExtensionError> {
                CALLS_V2.fetch_add(1, Ordering::SeqCst);
                Ok(serde_json::json!({"v": 2}))
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        let registry = Arc::new(ExtensionRegistry::new().with_command_timeout_secs(30));
        let runtime = ExtensionRuntime::with_defaults(Arc::clone(&registry));
        registry
            .register(
                "rt-bind".into(),
                Arc::new(tokio::sync::RwLock::new(Box::new(V1))),
            )
            .await
            .unwrap();
        let info = registry.get_info("rt-bind").await.unwrap();
        let cmd = info.commands.iter().find(|c| c.name == "ping").unwrap();
        let prepared = crate::extension::prepare_invoke(crate::extension::PrepareInvokeRequest {
            instance_id: "rt-bind",
            command: cmd,
            raw_params: &serde_json::json!({}),
            extension_version: Some(info.metadata.version.as_str()),
            package_digest: None,
            capability_generation: &info.capability_generation,
        })
        .unwrap();

        let rt = Arc::new(runtime);
        let rt_exec = Arc::clone(&rt);
        let prepared_clone = prepared.clone();
        let join = tokio::spawn(async move { rt_exec.execute_prepared(&prepared_clone).await });

        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        while !ENTERED_V1.load(Ordering::SeqCst) {
            assert!(
                tokio::time::Instant::now() < deadline,
                "V1 never entered execute_command via runtime path"
            );
            tokio::task::yield_now().await;
        }

        registry.unregister("rt-bind").await.unwrap();
        registry
            .register(
                "rt-bind".into(),
                Arc::new(tokio::sync::RwLock::new(Box::new(V2))),
            )
            .await
            .unwrap();

        RELEASE_V1.store(true, Ordering::SeqCst);
        let out = join.await.expect("join").expect("bound V1 should finish");
        assert_eq!(out, serde_json::json!({"v": 1}));
        assert_eq!(
            CALLS_V2.load(Ordering::SeqCst),
            0,
            "runtime execute_prepared must not re-query by id into the new instance"
        );
    }
}
