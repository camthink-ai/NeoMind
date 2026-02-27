//! Extension registry for managing dynamically loaded extensions.
//!
//! The registry provides:
//! - Extension registration and lifecycle management
//! - Extension discovery from filesystem
//! - Health monitoring
//! - Safety management (circuit breaker, panic isolation)

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::RwLock;

use crate::event::NeoMindEvent;
use crate::eventbus::EventBus;
use crate::extension::loader::{NativeExtensionLoader, WasmExtensionLoader};
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
    /// Registered extensions (using std::sync::RwLock for spawn_blocking compatibility)
    extensions: RwLock<HashMap<String, DynExtension>>,
    /// Extension information cache (using std::sync::RwLock for spawn_blocking compatibility)
    info_cache: RwLock<HashMap<String, ExtensionInfo>>,
    /// Native extension loader
    native_loader: NativeExtensionLoader,
    /// WASM extension loader
    wasm_loader: WasmExtensionLoader,
    /// Extension directories to scan
    extension_dirs: Vec<PathBuf>,
    /// Loaded libraries (kept alive to prevent unloading)
    _loaded_libraries: Vec<libloading::Library>,
    /// Safety manager for circuit breaking and panic isolation
    safety_manager: Arc<ExtensionSafetyManager>,
    /// Event bus for publishing lifecycle events (optional)
    event_bus: Option<Arc<EventBus>>,
}

impl ExtensionRegistry {
    /// Create a new extension registry.
    pub fn new() -> Self {
        // Create WASM loader, but avoid panicking the whole process on failure.
        // If the sandbox cannot be created, we log an error and disable WASM extension loading
        // while still allowing native extensions to function.
        let wasm_loader = match WasmExtensionLoader::new() {
            Ok(loader) => loader,
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "[ExtensionRegistry] Failed to create WASM loader, WASM extensions will be unavailable"
                );
                // Fallback to a default loader that will report errors on use.
                // This uses Default, which is expected to succeed in normal configurations.
                WasmExtensionLoader::default()
            }
        };

        Self {
            extensions: RwLock::new(HashMap::new()),
            info_cache: RwLock::new(HashMap::new()),
            native_loader: NativeExtensionLoader::new(),
            wasm_loader,
            extension_dirs: vec![],
            _loaded_libraries: vec![],
            safety_manager: Arc::new(ExtensionSafetyManager::new()),
            event_bus: None,
        }
    }

    /// Set the event bus for publishing lifecycle events.
    pub fn set_event_bus(&mut self, event_bus: Arc<EventBus>) {
        self.event_bus = Some(event_bus);
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
        if self.extensions.read().unwrap().contains_key(&id) {
            return Err(ExtensionError::AlreadyRegistered(id));
        }

        // Store extension
        self.extensions
            .write()
            .unwrap()
            .insert(id.clone(), extension.clone());

        // Register with safety manager for circuit breaking and panic tracking
        self.safety_manager
            .register_extension(id.clone())
            .await;

        // Store info
        self.info_cache
            .write()
            .unwrap()
            .insert(
                id.clone(),
                ExtensionInfo {
                    metadata,
                    state: ExtensionState::Running,
                    stats: ExtensionStats::default(),
                    loaded_at: Some(chrono::Utc::now()),
                    metrics,
                    commands,
                },
            );

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
        self.extensions.write().unwrap().remove(id);
        self.info_cache.write().unwrap().remove(id);

        // Unregister from safety manager
        self.safety_manager.unregister_extension(id).await;
        
        tracing::info!("Extension unregistered: {}", id);
        Ok(())
    }

    /// Get an extension by ID.
    pub async fn get(&self, id: &str) -> Option<DynExtension> {
        self.extensions.read().unwrap().get(id).cloned()
    }

    /// Get extension info by ID.
    pub async fn get_info(&self, id: &str) -> Option<ExtensionInfo> {
        self.info_cache.read().unwrap().get(id).cloned()
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

    /// List all extensions.
    pub async fn list(&self) -> Vec<ExtensionInfo> {
        self.info_cache.read().unwrap().values().cloned().collect()
    }

    /// Load an extension from a file path and register it.
    pub async fn load_from_path(&self, path: &Path) -> Result<ExtensionMetadata> {
        let extension = path.extension().and_then(|e| e.to_str());

        match extension {
            Some("so") | Some("dylib") | Some("dll") => {
                // Load the native extension
                let loaded = self.native_loader.load(path)?;

                // Get metadata and metrics/commands
                let ext = loaded.extension.read().await;
                let mut metadata = ext.metadata().clone();
                let _metrics = ext.metrics().to_vec();
                let _commands = ext.commands().to_vec();
                drop(ext);

                // Set file path for component loading
                metadata.file_path = Some(path.to_path_buf());

                // Register the extension with file path
                let id = metadata.id.clone();
                self.register_with_path(id, loaded.extension, Some(path.to_path_buf())).await?;

                Ok(metadata)
            }
            Some("wasm") => {
                // Load the WASM extension
                let loaded = self.wasm_loader.load(path).await?;

                // Get metadata and metrics/commands
                let ext = loaded.extension.read().await;
                let mut metadata = ext.metadata().clone();
                let _metrics = ext.metrics().to_vec();
                let _commands = ext.commands().to_vec();
                drop(ext);

                // Set file path for component loading
                metadata.file_path = Some(path.to_path_buf());

                // Register the extension with file path
                let id = metadata.id.clone();
                self.register_with_path(id, loaded.extension, Some(path.to_path_buf())).await?;

                Ok(metadata)
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

            // Discover WASM extensions
            let wasm_found = self.wasm_loader.discover(dir).await;
            tracing::debug!(
                "Extension discover: found {} wasm extensions",
                wasm_found.len()
            );
            for (path, metadata) in wasm_found {
                discovered.push((path, metadata));
            }
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

        // Execute with timeout protection (30 seconds)
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            async {
                let ext_guard = ext_clone.read().await;
                ext_guard.execute_command(command, args).await
            }
        ).await;

        match result {
            Ok(Ok(value)) => {
                // Record success with safety manager
                self.safety_manager.record_success(id).await;
                Ok(value)
            }
            Ok(Err(e)) => {
                // Record logical failure
                self.safety_manager.record_failure(id).await;
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
                tracing::error!(
                    extension_id = %id,
                    command = %command,
                    "[ExtensionRegistry] Extension command timed out after 30 seconds"
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
        self.extensions.read().unwrap().contains_key(id)
    }

    /// Get the number of registered extensions.
    pub async fn count(&self) -> usize {
        self.extensions.read().unwrap().len()
    }

    /// Get all registered extensions (alias for get_extensions() from trait).
    pub async fn get_all(&self) -> Vec<DynExtension> {
        self.extensions.read().unwrap().values().cloned().collect()
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
        self.extensions.read().unwrap().values().cloned().collect()
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
