//! Unified Extension Service
//!
//! This module provides a unified interface for extension management that
//! combines in-process extensions (via ExtensionRegistry) and process-isolated
//! extensions (via IsolatedExtensionManager).
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                   UnifiedExtensionService                    │
//! │  - Provides single API for all extension operations         │
//! │  - Routes requests to appropriate backend                   │
//! │  - Handles lifecycle management                             │
//! └─────────────────────────────────────────────────────────────┘
//!           │                              │
//!           ▼                              ▼
//! ┌─────────────────────────┐    ┌─────────────────────────┐
//! │   ExtensionRegistry     │    │ IsolatedExtensionManager │
//! │ - In-process extensions │    │ - Process-isolated exts  │
//! │ - Direct calls          │    │ - IPC communication      │
//! └─────────────────────────┘    └─────────────────────────┘
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::extension::isolated::{
    IsolatedExtensionManager, IsolatedManagerConfig,
};
use crate::extension::registry::ExtensionRegistry;
use crate::extension::system::{
    ExtensionError, ExtensionMetadata, ExtensionMetricValue,
};

/// Configuration for the unified extension service
#[derive(Debug, Clone)]
pub struct UnifiedExtensionConfig {
    /// Configuration for isolated extensions
    pub isolated_config: IsolatedManagerConfig,
    /// Whether to use isolated mode by default for new extensions
    pub isolated_by_default: bool,
}

impl Default for UnifiedExtensionConfig {
    fn default() -> Self {
        Self {
            isolated_config: IsolatedManagerConfig::default(),
            isolated_by_default: true,
        }
    }
}

/// Unified information about an extension
#[derive(Debug, Clone)]
pub struct UnifiedExtensionInfo {
    /// Extension metadata
    pub metadata: ExtensionMetadata,
    /// Whether the extension is running in isolated mode
    pub is_isolated: bool,
    /// Whether the extension is currently running/healthy
    pub is_running: bool,
    /// Path to the extension binary
    pub path: Option<PathBuf>,
}

/// Unified extension service that manages both in-process and isolated extensions
pub struct UnifiedExtensionService {
    /// In-process extension registry
    registry: Arc<ExtensionRegistry>,
    /// Isolated extension manager
    isolated_manager: Arc<IsolatedExtensionManager>,
    /// Configuration
    config: UnifiedExtensionConfig,
}

impl UnifiedExtensionService {
    /// Create a new unified extension service
    pub fn new(
        registry: Arc<ExtensionRegistry>,
        config: UnifiedExtensionConfig,
    ) -> Self {
        let isolated_manager = Arc::new(IsolatedExtensionManager::new(config.isolated_config.clone()));

        Self {
            registry,
            isolated_manager,
            config,
        }
    }

    /// Create with default configuration
    pub fn with_defaults(registry: Arc<ExtensionRegistry>) -> Self {
        Self::new(registry, UnifiedExtensionConfig::default())
    }

    /// Load an extension from a path
    ///
    /// Automatically determines whether to use isolated mode based on configuration
    /// and extension metadata.
    pub async fn load(&self, path: &Path) -> Result<ExtensionMetadata, ExtensionError> {
        // First, get metadata to determine mode
        let use_isolated = self.should_use_isolated(path).await?;

        if use_isolated {
            tracing::info!(
                path = %path.display(),
                "Loading extension in ISOLATED mode"
            );

            self.isolated_manager
                .load(path)
                .await
                .map_err(|e| ExtensionError::LoadFailed(e.to_string()))
        } else {
            tracing::info!(
                path = %path.display(),
                "Loading extension in IN-PROCESS mode"
            );

            self.registry.load_from_path(path).await
        }
    }

    /// Determine if an extension should use isolated mode
    async fn should_use_isolated(&self, path: &Path) -> Result<bool, ExtensionError> {
        // Check if we can get metadata first
        // For now, just use the default setting
        // In the future, we could inspect the extension metadata for hints

        // Check file extension - WASM extensions are naturally sandboxed
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext == "wasm" {
                return Ok(false); // WASM doesn't need process isolation
            }
        }

        Ok(self.config.isolated_by_default)
    }

    /// Unload an extension
    pub async fn unload(&self, id: &str) -> Result<(), ExtensionError> {
        // Check if it's an isolated extension
        if self.isolated_manager.contains(id).await {
            self.isolated_manager
                .unload(id)
                .await
                .map_err(|e| ExtensionError::ExecutionFailed(e.to_string()))?;
        } else {
            self.registry.unregister(id).await?;
        }

        Ok(())
    }

    /// Execute a command on an extension
    pub async fn execute_command(
        &self,
        id: &str,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ExtensionError> {
        // Check if it's an isolated extension first
        if self.isolated_manager.contains(id).await {
            tracing::debug!(
                extension_id = %id,
                command = %command,
                "Executing command on ISOLATED extension"
            );

            self.isolated_manager
                .execute_command(id, command, args)
                .await
                .map_err(|e| ExtensionError::ExecutionFailed(e.to_string()))
        } else {
            self.registry.execute_command(id, command, args).await
        }
    }

    /// Get metrics from an extension
    pub async fn get_metrics(&self, id: &str) -> Vec<ExtensionMetricValue> {
        // Check if it's an isolated extension
        if self.isolated_manager.contains(id).await {
            self.isolated_manager
                .get_metrics(id)
                .await
                .unwrap_or_default()
        } else {
            self.registry.get_current_metrics(id).await
        }
    }

    /// Check health of an extension
    pub async fn health_check(&self, id: &str) -> Result<bool, ExtensionError> {
        // Check if it's an isolated extension
        if self.isolated_manager.contains(id).await {
            self.isolated_manager
                .health_check(id)
                .await
                .map_err(|e| ExtensionError::ExecutionFailed(e.to_string()))
        } else {
            self.registry.health_check(id).await
        }
    }

    /// Check if an extension is registered
    pub async fn contains(&self, id: &str) -> bool {
        self.registry.contains(id).await || self.isolated_manager.contains(id).await
    }

    /// Get extension info
    pub async fn get_info(&self, id: &str) -> Option<UnifiedExtensionInfo> {
        // Check isolated first
        if let Some(info) = self.isolated_manager.get_info(id) {
            return Some(UnifiedExtensionInfo {
                metadata: info.metadata,
                is_isolated: true,
                is_running: info.is_running,
                path: Some(info.path),
            });
        }

        // Check registry
        if let Some(info) = self.registry.get_info(id).await {
            let path = info.metadata.file_path.clone();
            return Some(UnifiedExtensionInfo {
                metadata: info.metadata,
                is_isolated: false,
                is_running: info.state == crate::extension::system::ExtensionState::Running,
                path,
            });
        }

        None
    }

    /// List all extensions
    pub async fn list(&self) -> Vec<UnifiedExtensionInfo> {
        let mut result = Vec::new();

        // Add in-process extensions
        for info in self.registry.list().await {
            let path = info.metadata.file_path.clone();
            result.push(UnifiedExtensionInfo {
                metadata: info.metadata,
                is_isolated: false,
                is_running: info.state == crate::extension::system::ExtensionState::Running,
                path,
            });
        }

        // Add isolated extensions
        for info in self.isolated_manager.list().await {
            result.push(UnifiedExtensionInfo {
                metadata: info.metadata,
                is_isolated: true,
                is_running: info.is_running,
                path: Some(info.path),
            });
        }

        result
    }

    /// Get count of all extensions
    pub async fn count(&self) -> usize {
        self.registry.count().await + self.isolated_manager.count().await
    }

    /// Check if an extension is running in isolated mode
    pub async fn is_isolated(&self, id: &str) -> bool {
        self.isolated_manager.contains(id).await
    }

    /// Stop all extensions
    pub async fn stop_all(&self) {
        self.isolated_manager.stop_all().await;
        // Registry doesn't have a stop_all method, extensions just get dropped
    }

    /// Get the underlying registry (for compatibility)
    pub fn registry(&self) -> Arc<ExtensionRegistry> {
        Arc::clone(&self.registry)
    }

    /// Get the isolated manager
    pub fn isolated_manager(&self) -> Arc<IsolatedExtensionManager> {
        Arc::clone(&self.isolated_manager)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = UnifiedExtensionConfig::default();
        assert!(config.isolated_by_default);
    }
}
