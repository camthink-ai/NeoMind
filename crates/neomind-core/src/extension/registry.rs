//! Extension registry for managing in-host extension proxies.
//!
//! The registry provides:
//! - Extension registration and lifecycle management
//! - Extension discovery from filesystem
//! - Health monitoring
//! - Safety management (circuit breaker, panic isolation)
//!
//! Note: Real extension execution is handled by the isolated runner/runtime.
//! This registry now only stores host-side proxy objects for streaming support.

use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::event::NeoMindEvent;
use crate::eventbus::EventBus;
use crate::extension::event_dispatcher::EventDispatcher;
use crate::extension::loader::NativeExtensionMetadataLoader;
use crate::extension::safety::ExtensionSafetyManager;
use crate::extension::system::{
    DynExtension, ExtensionError, ExtensionMetadata, ExtensionState, ExtensionStats,
};
use crate::extension::types::Result;

/// Information about a registered extension.
#[derive(Debug, Clone)]
pub struct ExtensionInfo {
    /// Extension metadata
    pub metadata: ExtensionMetadata,
    /// Current state
    pub state: ExtensionState,
    /// Runtime statistics
    pub stats: ExtensionStats,
    /// When the extension was loaded
    pub loaded_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Metrics provided by this extension
    pub metrics: Vec<super::system::MetricDescriptor>,
    /// Commands provided by this extension
    pub commands: Vec<super::system::ExtensionCommand>,
}

/// Registry for managing extensions.
pub struct ExtensionRegistry {
    /// Registered extensions.
    /// Using parking_lot::RwLock for better performance and no potential for
    /// blocking the async runtime (unlike std::sync::RwLock).
    extensions: RwLock<HashMap<String, DynExtension>>,
    /// Extension information cache.
    /// Using parking_lot::RwLock for consistent locking strategy.
    info_cache: RwLock<HashMap<String, ExtensionInfo>>,
    /// Native extension metadata loader
    native_loader: NativeExtensionMetadataLoader,
    /// Extension directories to scan
    extension_dirs: Vec<PathBuf>,
    /// Safety manager for circuit breaking and panic isolation
    safety_manager: std::sync::Arc<ExtensionSafetyManager>,
    /// Event bus for publishing lifecycle events (optional)
    event_bus: Option<std::sync::Arc<EventBus>>,
    /// Command execution timeout in seconds (default: 300)
    command_timeout_secs: u64,
    /// Event dispatcher for pushing events to extensions (optional)
    /// Using Option<Arc<>> for interior mutability
    event_dispatcher: parking_lot::RwLock<Option<std::sync::Arc<EventDispatcher>>>,
}

impl ExtensionRegistry {
    /// Create a new extension registry.
    pub fn new() -> Self {
        Self {
            extensions: RwLock::new(HashMap::new()),
            info_cache: RwLock::new(HashMap::new()),
            native_loader: NativeExtensionMetadataLoader::new(),
            extension_dirs: vec![],
            safety_manager: std::sync::Arc::new(ExtensionSafetyManager::new()),
            event_bus: None,
            event_dispatcher: parking_lot::RwLock::new(None),
            command_timeout_secs: 300,
        }
    }

    /// Set the command execution timeout in seconds.
    pub fn with_command_timeout_secs(mut self, secs: u64) -> Self {
        self.command_timeout_secs = secs;
        self
    }

    /// Set the event bus for publishing lifecycle events.
    pub fn set_event_bus(&mut self, event_bus: std::sync::Arc<EventBus>) {
        self.event_bus = Some(event_bus);
    }

    /// Set the event dispatcher for pushing events to extensions.
    pub fn set_event_dispatcher(&self, event_dispatcher: std::sync::Arc<EventDispatcher>) {
        *self.event_dispatcher.write() = Some(event_dispatcher);
    }

    /// Add an extension directory to scan.
    pub fn add_extension_dir(&mut self, path: PathBuf) {
        self.extension_dirs.push(path);
    }

    /// Register an extension instance.
    pub async fn register(&self, id: String, extension: DynExtension) -> Result<()> {
        self.register_with_path(id, extension, None).await
    }

    /// Register an extension with an optional file path.
    ///
    /// The file path is used to locate the extension's manifest.json for dashboard components.
    pub async fn register_with_path(
        &self,
        id: String,
        extension: DynExtension,
        file_path: Option<PathBuf>,
    ) -> Result<()> {
        let ext = extension.read().await;
        let mut metadata = ext.metadata().clone();
        let metrics = ext.metrics().to_vec();
        let commands = ext.commands().to_vec();
        drop(ext);

        // Set file path if provided
        metadata.file_path = file_path;

        // Check if already registered
        if self.extensions.read().contains_key(&id) {
            return Err(ExtensionError::AlreadyRegistered(id));
        }

        // Store extension
        self.extensions
            .write()
            .insert(id.clone(), extension.clone());

        // Register with safety manager for circuit breaking and panic tracking
        self.safety_manager.register_extension(id.clone()).await;

        // Store info
        let stats = ExtensionStats {
            start_count: 1, // First registration counts as a start
            ..Default::default()
        };

        self.info_cache.write().insert(
            id.clone(),
            ExtensionInfo {
                metadata,
                state: ExtensionState::Running,
                stats,
                loaded_at: Some(chrono::Utc::now()),
                metrics,
                commands,
            },
        );

        // Register with event dispatcher for event subscriptions
        // This allows in-process extensions to receive events via handle_event()
        if let Some(ref dispatcher) = *self.event_dispatcher.read() {
            let dispatcher = std::sync::Arc::clone(dispatcher);
            let extension_clone = extension.clone();
            let id_clone = id.clone();
            // Use spawn instead of block_in_place — block_in_place panics on
            // current-thread runtimes (Tauri main thread). The registration is
            // fire-and-forget: the extension starts receiving events within ms.
            tokio::spawn(async move {
                dispatcher
                    .register_in_process_extension(id_clone, extension_clone)
                    .await;
            });
        }

        // Publish ExtensionLifecycle { state: "registered" } event
        // Use sync version to avoid issues with non-Tokio contexts
        if let Some(ref event_bus) = self.event_bus {
            let _ = event_bus.publish_with_source_sync(
                NeoMindEvent::ExtensionLifecycle {
                    extension_id: id.clone(),
                    state: "registered".to_string(),
                    message: Some(format!("Extension {} registered", id)),
                    timestamp: chrono::Utc::now().timestamp(),
                },
                "extension",
            );
        }

        tracing::info!("Extension registered: {}", id);
        Ok(())
    }

    /// Unregister an extension.
    pub async fn unregister(&self, id: &str) -> Result<()> {
        // Update stats before removing
        if let Some(info) = self.info_cache.write().get_mut(id) {
            info.stats.stop_count += 1;
        }

        // Publish ExtensionLifecycle { state: "unregistered" } event BEFORE removing
        // Use sync version to avoid issues with non-Tokio contexts
        if let Some(ref event_bus) = self.event_bus {
            let _ = event_bus.publish_with_source_sync(
                NeoMindEvent::ExtensionLifecycle {
                    extension_id: id.to_string(),
                    state: "unregistered".to_string(),
                    message: Some(format!("Extension {} unregistered", id)),
                    timestamp: chrono::Utc::now().timestamp(),
                },
                "extension",
            );
        }

        // Remove from memory
        self.extensions.write().remove(id);
        self.info_cache.write().remove(id);

        // Unregister from safety manager
        self.safety_manager.unregister_extension(id).await;

        // ✅ FIX: Unregister from event dispatcher to prevent sending events to unloaded extension
        if let Some(ref dispatcher) = *self.event_dispatcher.read() {
            dispatcher.unregister_extension(id);
        }

        tracing::info!("Extension unregistered: {}", id);
        Ok(())
    }

    /// Get an extension by ID.
    pub async fn get(&self, id: &str) -> Option<DynExtension> {
        self.extensions.read().get(id).cloned()
    }

    /// Get extension info by ID.
    pub async fn get_info(&self, id: &str) -> Option<ExtensionInfo> {
        self.info_cache.read().get(id).cloned()
    }

    /// Get current metric values from an extension.
    /// This calls the extension's `produce_metrics()` method and returns the current values.
    pub async fn get_current_metrics(&self, id: &str) -> Vec<super::system::ExtensionMetricValue> {
        if let Some(ext) = self.get(id).await {
            let ext = ext.read().await;
            // Call produce_metrics with panic handling
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ext.produce_metrics())) {
                Ok(Ok(metrics)) => metrics,
                Ok(Err(e)) => {
                    tracing::warn!(
                        extension_id = %id,
                        error = %e,
                        "[ExtensionRegistry] Extension failed to produce metrics"
                    );
                    Vec::new()
                }
                Err(_) => {
                    tracing::error!(
                        extension_id = %id,
                        "[ExtensionRegistry] Extension panicked while producing metrics"
                    );
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        }
    }

    /// Get extension statistics.
    /// This calls the extension's `get_stats()` method and returns the statistics.
    pub async fn get_stats(
        &self,
        id: &str,
    ) -> std::result::Result<super::system::ExtensionStats, ExtensionError> {
        if let Some(ext) = self.get(id).await {
            let ext = ext.read().await;
            // Call get_stats with panic handling
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ext.get_stats())) {
                Ok(stats) => Ok(stats),
                Err(_) => {
                    tracing::error!(
                        extension_id = %id,
                        "[ExtensionRegistry] Extension panicked while getting stats"
                    );
                    Err(ExtensionError::ExecutionFailed(
                        "Extension panicked while getting stats".to_string(),
                    ))
                }
            }
        } else {
            Err(ExtensionError::NotFound(format!(
                "Extension {} not found",
                id
            )))
        }
    }

    /// List all extensions.
    pub async fn list(&self) -> Vec<ExtensionInfo> {
        self.info_cache.read().values().cloned().collect()
    }

    /// Load an extension from a file path and register it.
    ///
    /// Note: WASM extensions should be loaded via the extension-runner process,
    /// not directly through this registry.
    pub async fn load_from_path(&self, path: &Path) -> Result<ExtensionMetadata> {
        let extension = path.extension().and_then(|e| e.to_str());

        match extension {
            Some("so") | Some("dylib") | Some("dll") => {
                Err(ExtensionError::InvalidFormat(
                    "Direct native loading has been removed; use ExtensionRuntime for isolated execution"
                        .to_string(),
                ))
            }
            Some("wasm") => {
                // WASM extensions should be loaded via extension-runner
                // Return an error pointing users to the isolated extension path
                Err(ExtensionError::InvalidFormat(
                    "WASM extensions must be loaded via ExtensionRuntime for process isolation".to_string()
                ))
            }
            _ => Err(ExtensionError::InvalidFormat(format!(
                "Unsupported extension format: {:?}",
                path
            ))),
        }
    }

    /// Discover extensions in configured directories.
    ///
    /// Returns a list of (path, metadata) tuples for discovered extensions.
    /// Note: Only native extensions are discovered directly. WASM extensions
    /// should be discovered via the extension-runner process.
    pub async fn discover(&self) -> Vec<(PathBuf, ExtensionMetadata)> {
        let mut discovered = Vec::new();

        tracing::debug!(
            "Extension discover: starting, dirs: {:?}",
            self.extension_dirs
        );

        for dir in &self.extension_dirs {
            if !dir.exists() {
                tracing::debug!("Extension discover: directory does not exist: {:?}", dir);
                continue;
            }

            tracing::debug!("Extension discover: scanning directory: {:?}", dir);

            // Use the loader's discover method
            let native_found = self.native_loader.discover(dir).await;
            tracing::debug!(
                "Extension discover: found {} native extensions",
                native_found.len()
            );
            for (path, metadata) in native_found {
                discovered.push((path, metadata));
            }

            // Note: WASM extensions are discovered by the extension-runner,
            // not here in the registry
        }

        tracing::debug!(
            "Extension discover: complete, total found: {}",
            discovered.len()
        );
        discovered
    }

    /// Execute a command on an extension.
    ///
    /// Includes a 30-second timeout to prevent hanging on slow or buggy extensions.
    pub async fn execute_command(
        &self,
        id: &str,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        // Check safety manager before executing
        if !self.safety_manager.is_allowed(id).await {
            tracing::warn!(
                extension_id = %id,
                command = %command,
                "[ExtensionRegistry] Extension execution blocked by safety manager"
            );
            return Err(ExtensionError::SecurityError(format!(
                "Extension '{}' is temporarily disabled by safety policy",
                id
            )));
        }

        let ext = self
            .get(id)
            .await
            .ok_or_else(|| ExtensionError::NotFound(id.to_string()))?;

        // Clone the Arc to avoid holding the lock across the await
        let ext_clone = Arc::clone(&ext);

        // Record start time for execution stats
        let start_time = std::time::Instant::now();

        // Execute with timeout protection
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(self.command_timeout_secs),
            async {
                let ext_guard = ext_clone.read().await;
                ext_guard.execute_command(command, args).await
            },
        )
        .await;

        // Calculate execution time
        let execution_time_ms = start_time.elapsed().as_millis() as u64;

        match result {
            Ok(Ok(value)) => {
                // Record success with safety manager
                self.safety_manager.record_success(id).await;

                // Update stats for successful execution
                if let Some(info) = self.info_cache.write().get_mut(id) {
                    info.stats.commands_executed += 1;
                    info.stats.total_execution_time_ms += execution_time_ms;
                    info.stats.last_execution_time_ms = Some(chrono::Utc::now().timestamp_millis());
                }

                Ok(value)
            }
            Ok(Err(e)) => {
                // Record logical failure
                self.safety_manager.record_failure(id).await;

                // Update error stats
                if let Some(info) = self.info_cache.write().get_mut(id) {
                    info.stats.error_count += 1;
                    info.stats.last_error = Some(e.to_string());
                }

                tracing::warn!(
                    extension_id = %id,
                    command = %command,
                    error = %e,
                    "[ExtensionRegistry] Extension command failed"
                );
                Err(e)
            }
            Err(_) => {
                // Timeout is treated as a failure for safety manager
                self.safety_manager.record_failure(id).await;

                // Update error stats for timeout
                let error_msg = format!(
                    "Command '{}' timed out after {} seconds",
                    command, self.command_timeout_secs
                );
                if let Some(info) = self.info_cache.write().get_mut(id) {
                    info.stats.error_count += 1;
                    info.stats.last_error = Some(error_msg.clone());
                }

                tracing::error!(
                    extension_id = %id,
                    command = %command,
                    timeout_secs = self.command_timeout_secs,
                    "[ExtensionRegistry] Extension command timed out"
                );
                Err(ExtensionError::Timeout(format!(
                    "Command '{}' on extension '{}' timed out",
                    command, id
                )))
            }
        }
    }

    /// Perform health check on an extension.
    pub async fn health_check(&self, id: &str) -> Result<bool> {
        let ext = self
            .get(id)
            .await
            .ok_or_else(|| ExtensionError::NotFound(id.to_string()))?;

        let ext_clone = Arc::clone(&ext);
        let result = {
            let ext_guard = ext_clone.read().await;
            ext_guard.health_check().await
        };
        result
    }

    /// Check if an extension is registered.
    pub async fn contains(&self, id: &str) -> bool {
        self.extensions.read().contains_key(id)
    }

    /// Get the number of registered extensions.
    pub async fn count(&self) -> usize {
        self.extensions.read().len()
    }

    /// Get all registered extensions (alias for get_extensions() from trait).
    pub async fn get_all(&self) -> Vec<DynExtension> {
        self.extensions.read().values().cloned().collect()
    }

    /// Get the safety manager for this registry.
    pub fn safety_manager(&self) -> Arc<ExtensionSafetyManager> {
        Arc::clone(&self.safety_manager)
    }
}

impl Default for ExtensionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait for registries that manage extensions.
#[async_trait::async_trait]
pub trait ExtensionRegistryTrait: Send + Sync {
    /// Get all registered extensions.
    async fn get_extensions(&self) -> Vec<DynExtension>;

    /// Get a specific extension by ID.
    async fn get_extension(&self, id: &str) -> Option<DynExtension>;

    /// Execute a command on an extension.
    async fn execute_command(
        &self,
        extension_id: &str,
        command: &str,
        args: &serde_json::Value,
    ) -> std::result::Result<serde_json::Value, String>;

    /// Get metrics from an extension.
    async fn get_metrics(&self, extension_id: &str) -> Vec<super::system::MetricDescriptor>;
}

#[async_trait::async_trait]
impl ExtensionRegistryTrait for ExtensionRegistry {
    async fn get_extensions(&self) -> Vec<DynExtension> {
        self.extensions.read().values().cloned().collect()
    }

    async fn get_extension(&self, id: &str) -> Option<DynExtension> {
        self.get(id).await
    }

    async fn execute_command(
        &self,
        extension_id: &str,
        command: &str,
        args: &serde_json::Value,
    ) -> std::result::Result<serde_json::Value, String> {
        // Delegate to the main registry execute_command which includes timeout
        // and safety manager integration. This ensures all callers (including
        // tools and automation) go through the same protection layer.
        self.execute_command(extension_id, command, args)
            .await
            .map_err(|e| e.to_string())
    }

    async fn get_metrics(&self, extension_id: &str) -> Vec<super::system::MetricDescriptor> {
        if let Some(ext) = self.get(extension_id).await {
            // Clone the Arc to avoid holding the lock
            let ext_clone = Arc::clone(&ext);
            let metrics = {
                let ext_guard = ext_clone.read().await;
                ext_guard.metrics().to_vec()
            };
            metrics
        } else {
            vec![]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_registry_creation() {
        let registry = ExtensionRegistry::new();
        assert_eq!(registry.count().await, 0);
    }
}
