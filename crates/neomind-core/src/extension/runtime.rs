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

/// Runtime view of a loaded extension.
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
        }

        Ok(metadata)
    }

    /// Unload an extension and remove its streaming proxy.
    pub async fn unload(&self, id: &str) -> Result<(), ExtensionError> {
        self.isolated_manager
            .unload(id)
            .await
            .map_err(|e| ExtensionError::ExecutionFailed(e.to_string()))?;

        if self.proxy_registry.contains(id).await {
            let _ = self.proxy_registry.unregister(id).await;
        }

        Ok(())
    }

    /// Alias for unload used by API handlers.
    pub async fn unregister(&self, id: &str) -> Result<(), ExtensionError> {
        self.unload(id).await
    }

    /// Execute a command on an extension.
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
    pub async fn get_info(&self, id: &str) -> Option<ExtensionRuntimeInfo> {
        self.isolated_manager
            .get_info(id)
            .map(|info| ExtensionRuntimeInfo {
                metadata: info.descriptor.metadata,
                is_isolated: true,
                is_running: info.runtime.is_running,
                path: Some(info.path),
                metrics: info.descriptor.metrics,
                commands: info.descriptor.commands,
                consecutive_crashes: info.runtime.consecutive_crashes,
                last_crash_reason: info.runtime.last_crash_reason.clone(),
            })
    }

    /// List all loaded extensions.
    pub async fn list(&self) -> Vec<ExtensionRuntimeInfo> {
        self.isolated_manager
            .list()
            .await
            .into_iter()
            .map(|info| ExtensionRuntimeInfo {
                metadata: info.descriptor.metadata,
                is_isolated: true,
                is_running: info.runtime.is_running,
                path: Some(info.path),
                metrics: info.descriptor.metrics,
                commands: info.descriptor.commands,
                consecutive_crashes: info.runtime.consecutive_crashes,
                last_crash_reason: info.runtime.last_crash_reason.clone(),
            })
            .collect()
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
