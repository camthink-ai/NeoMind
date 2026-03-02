//! Isolated Extension Manager
//!
//! This module provides a manager for process-isolated extensions that works
//! alongside the standard ExtensionRegistry. It allows extensions to be loaded
//! in isolated mode without modifying the core registry structure.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     API Layer                                │
//! │  (checks IsolatedExtensionManager first, then Registry)     │
//! └─────────────────────────────────────────────────────────────┘
//!           │                              │
//!           ▼                              ▼
//! ┌─────────────────────────┐    ┌─────────────────────────┐
//! │ IsolatedExtensionManager │    │   ExtensionRegistry     │
//! │ - Manages isolated exts  │    │ - Manages in-process    │
//! │ - Process lifecycle      │    │ - Standard loading      │
//! │ - IPC communication      │    │ - Direct calls          │
//! └─────────────────────────┘    └─────────────────────────┘
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::RwLock as AsyncRwLock;

use super::process::{IsolatedExtension, IsolatedExtensionConfig};
use super::{IsolatedExtensionError, IsolatedResult};
use crate::extension::loader::{IsolatedExtensionLoader, IsolatedLoaderConfig};
use crate::extension::system::{ExtensionMetadata, ExtensionMetricValue};

/// Configuration for the isolated extension manager
#[derive(Debug, Clone)]
pub struct IsolatedManagerConfig {
    /// Base configuration for isolated extensions
    pub extension_config: IsolatedExtensionConfig,
    /// Whether to use isolated mode by default
    pub isolated_by_default: bool,
    /// Extensions that should always run in isolated mode
    pub force_isolated: Vec<String>,
    /// Extensions that should always run in-process
    pub force_in_process: Vec<String>,
}

impl Default for IsolatedManagerConfig {
    fn default() -> Self {
        Self {
            extension_config: IsolatedExtensionConfig::default(),
            // Default to isolated mode for safety
            isolated_by_default: true,
            force_isolated: Vec::new(),
            force_in_process: Vec::new(),
        }
    }
}

/// Information about a loaded isolated extension
#[derive(Debug, Clone)]
pub struct IsolatedExtensionInfo {
    /// Extension descriptor (unified capabilities)
    pub descriptor: crate::extension::system::ExtensionDescriptor,
    /// Path to extension binary
    pub path: PathBuf,
    /// Runtime state
    pub runtime: crate::extension::system::ExtensionRuntimeState,
}

// Keep backward-compatible accessor fields
impl IsolatedExtensionInfo {
    /// Get extension metadata
    pub fn metadata(&self) -> &ExtensionMetadata {
        &self.descriptor.metadata
    }

    /// Get extension commands
    pub fn commands(&self) -> &[crate::extension::system::ExtensionCommand] {
        &self.descriptor.commands
    }

    /// Get extension metrics
    pub fn metrics(&self) -> &[crate::extension::system::MetricDescriptor] {
        &self.descriptor.metrics
    }

    /// Check if running
    pub fn is_running(&self) -> bool {
        self.runtime.is_running
    }

    /// Get restart count
    pub fn restart_count(&self) -> u64 {
        self.runtime.restart_count
    }
}

/// Manager for process-isolated extensions
///
/// This manager handles extensions that run in separate processes,
/// providing complete isolation from the main NeoMind process.
pub struct IsolatedExtensionManager {
    /// Isolated extensions by ID
    extensions: AsyncRwLock<HashMap<String, Arc<IsolatedExtension>>>,
    /// Extension info cache
    info_cache: RwLock<HashMap<String, IsolatedExtensionInfo>>,
    /// Configuration
    config: IsolatedManagerConfig,
    /// Loader for isolated extensions
    loader: IsolatedExtensionLoader,
}

impl IsolatedExtensionManager {
    /// Create a new isolated extension manager
    pub fn new(config: IsolatedManagerConfig) -> Self {
        let loader_config = IsolatedLoaderConfig {
            isolated_config: config.extension_config.clone(),
            use_isolated_by_default: config.isolated_by_default,
            force_isolated: config.force_isolated.clone(),
            force_in_process: config.force_in_process.clone(),
        };

        Self {
            extensions: AsyncRwLock::new(HashMap::new()),
            info_cache: RwLock::new(HashMap::new()),
            config,
            loader: IsolatedExtensionLoader::new(loader_config),
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(IsolatedManagerConfig::default())
    }

    /// Check if an extension should use isolated mode
    pub fn should_use_isolated(&self, extension_id: &str) -> bool {
        self.loader.should_use_isolated(extension_id)
    }

    /// Load an extension in isolated mode
    pub async fn load(&self, path: &Path) -> IsolatedResult<ExtensionMetadata> {
        tracing::info!(
            path = %path.display(),
            "Loading extension in isolated mode"
        );

        let loaded = self.loader.load_isolated(path).await?;

        // Get the complete descriptor
        let descriptor = loaded.descriptor().await.ok_or_else(|| {
            IsolatedExtensionError::SpawnFailed("Failed to get extension descriptor".to_string())
        })?;

        let id = descriptor.id().to_string();

        // Store extension
        self.extensions.write().await.insert(id.clone(), loaded.clone());

        // Create runtime state
        let mut runtime = crate::extension::system::ExtensionRuntimeState::isolated();
        runtime.is_running = loaded.is_alive();
        runtime.loaded_at = Some(chrono::Utc::now().timestamp());

        // Store info
        self.info_cache.write().insert(
            id.clone(),
            IsolatedExtensionInfo {
                descriptor,
                path: path.to_path_buf(),
                runtime,
            },
        );

        tracing::info!(
            extension_id = %id,
            "Extension loaded in isolated mode"
        );

        // Return metadata from the info cache
        let info = self.info_cache.read().get(&id).cloned();
        Ok(info.map(|i| i.descriptor.metadata).unwrap())
    }

    /// Unload an extension
    pub async fn unload(&self, id: &str) -> IsolatedResult<()> {
        let mut extensions = self.extensions.write().await;

        if let Some(isolated) = extensions.remove(id) {
            // Stop the extension process
            isolated.stop().await?;
            self.info_cache.write().remove(id);

            tracing::info!(
                extension_id = %id,
                "Extension unloaded"
            );
        }

        Ok(())
    }

    /// Execute a command on an isolated extension
    pub async fn execute_command(
        &self,
        id: &str,
        command: &str,
        args: &serde_json::Value,
    ) -> IsolatedResult<serde_json::Value> {
        let extensions = self.extensions.read().await;

        let isolated = extensions.get(id).ok_or_else(|| {
            IsolatedExtensionError::IpcError(format!("Extension {} not found", id))
        })?;

        isolated.execute_command(command, args).await
    }

    /// Get metrics from an isolated extension
    pub async fn get_metrics(&self, id: &str) -> IsolatedResult<Vec<ExtensionMetricValue>> {
        let extensions = self.extensions.read().await;

        let isolated = extensions.get(id).ok_or_else(|| {
            IsolatedExtensionError::IpcError(format!("Extension {} not found", id))
        })?;

        isolated.produce_metrics().await
    }

    /// Check health of an isolated extension
    pub async fn health_check(&self, id: &str) -> IsolatedResult<bool> {
        let extensions = self.extensions.read().await;

        let isolated = extensions.get(id).ok_or_else(|| {
            IsolatedExtensionError::IpcError(format!("Extension {} not found", id))
        })?;

        isolated.health_check().await
    }

    /// Check if an extension is registered
    pub async fn contains(&self, id: &str) -> bool {
        self.extensions.read().await.contains_key(id)
    }

    /// Get extension info
    pub fn get_info(&self, id: &str) -> Option<IsolatedExtensionInfo> {
        self.info_cache.read().get(id).cloned()
    }

    /// List all isolated extensions
    pub async fn list(&self) -> Vec<IsolatedExtensionInfo> {
        self.info_cache.read().values().cloned().collect()
    }

    /// Get count of isolated extensions
    pub async fn count(&self) -> usize {
        self.extensions.read().await.len()
    }

    /// Check if an extension is running
    pub async fn is_running(&self, id: &str) -> bool {
        let extensions = self.extensions.read().await;
        extensions.get(id).map(|e| e.is_alive()).unwrap_or(false)
    }

    /// Get an isolated extension by ID
    pub async fn get(&self, id: &str) -> Option<Arc<IsolatedExtension>> {
        self.extensions.read().await.get(id).cloned()
    }

    /// Stop all extensions
    pub async fn stop_all(&self) {
        let mut extensions = self.extensions.write().await;

        for (id, isolated) in extensions.iter() {
            if let Err(e) = isolated.stop().await {
                tracing::warn!(
                    extension_id = %id,
                    error = %e,
                    "Failed to stop extension"
                );
            }
        }

        extensions.clear();
        self.info_cache.write().clear();

        tracing::info!("All isolated extensions stopped");
    }

    /// Get the loader configuration
    pub fn config(&self) -> &IsolatedManagerConfig {
        &self.config
    }
}

impl Drop for IsolatedExtensionManager {
    fn drop(&mut self) {
        // Attempt to stop all extensions on drop
        // Note: This is a best-effort cleanup
        if let Ok(mut extensions) = self.extensions.try_write() {
            for (id, isolated) in extensions.iter() {
                // Use kill_process for synchronous cleanup
                if isolated.is_alive() {
                    tracing::warn!(
                        extension_id = %id,
                        "Extension still running during drop, killing"
                    );
                }
            }
            extensions.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = IsolatedManagerConfig::default();
        assert!(config.isolated_by_default);
        assert!(config.force_isolated.is_empty());
        assert!(config.force_in_process.is_empty());
    }

    #[test]
    fn test_manager_creation() {
        let manager = IsolatedExtensionManager::with_defaults();
        assert_eq!(tokio::runtime::Runtime::new().unwrap().block_on(async {
            manager.count().await
        }), 0);
    }
}
