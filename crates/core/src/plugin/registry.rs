//! Unified plugin registry for managing all plugin types.
//!
//! This module provides a centralized registry for managing plugins
//! of different types (LLM backends, storage backends, device adapters, etc.)
//! with support for dynamic loading, lifecycle management, and state tracking.

use super::{PluginError, Result, ExtendedPluginMetadata, PluginType, PluginState, PluginStats, StateMachine, UnifiedPlugin, DynUnifiedPlugin, PluginRegistryEvent};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{RwLock, Mutex};

/// Options for loading a plugin.
#[derive(Debug, Clone)]
pub struct PluginLoadOptions {
    /// Whether to auto-start the plugin after loading
    pub auto_start: bool,

    /// Plugin configuration
    pub config: Option<Value>,

    /// Whether the plugin is enabled
    pub enabled: bool,

    /// Load timeout in seconds
    pub timeout_secs: Option<u64>,
}

impl Default for PluginLoadOptions {
    fn default() -> Self {
        Self {
            auto_start: false,
            config: None,
            enabled: true,
            timeout_secs: None,
        }
    }
}

/// Information about a registered plugin.
#[derive(Debug, Clone)]
pub struct PluginInfo {
    /// Plugin metadata
    pub metadata: ExtendedPluginMetadata,

    /// Current plugin state
    pub state: PluginState,

    /// Plugin statistics
    pub stats: PluginStats,

    /// Whether the plugin is enabled
    pub enabled: bool,

    /// Path to the plugin file (for dynamically loaded plugins)
    pub path: Option<PathBuf>,

    /// Load timestamp
    pub loaded_at: i64,

    /// Plugin type (determined by metadata)
    pub plugin_type: PluginType,
}

impl PluginInfo {
    /// Create a new plugin info.
    pub fn new(metadata: ExtendedPluginMetadata, plugin_type: PluginType) -> Self {
        Self {
            metadata,
            state: PluginState::Loaded,
            stats: PluginStats::default(),
            enabled: true,
            path: None,
            loaded_at: chrono::Utc::now().timestamp(),
            plugin_type,
        }
    }

    /// Check if the plugin is active (running or initialized).
    pub fn is_active(&self) -> bool {
        self.enabled && self.state.is_active()
    }
}

/// A plugin instance with its wrapper and metadata.
pub struct PluginInstance {
    /// The actual plugin
    pub plugin: DynUnifiedPlugin,

    /// Plugin information
    pub info: PluginInfo,

    /// State machine for tracking transitions
    pub state_machine: StateMachine,
}

impl PluginInstance {
    /// Create a new plugin instance.
    pub fn new(plugin: DynUnifiedPlugin, info: PluginInfo) -> Self {
        let state_machine = StateMachine::new();
        Self {
            plugin,
            info,
            state_machine,
        }
    }

    /// Get the current plugin state.
    pub async fn state(&self) -> PluginState {
        self.plugin.read().await.get_state()
    }
}

/// Unified plugin registry for managing all plugin types.
///
/// This registry provides:
/// - Plugin registration and discovery
/// - Lifecycle management (initialize, start, stop, shutdown)
/// - State tracking and transitions
/// - Health monitoring
pub struct UnifiedPluginRegistry {
    /// Registered plugins by ID
    plugins: Arc<RwLock<HashMap<String, PluginInstance>>>,

    /// Native plugin loader for dynamic libraries
    native_loader: Arc<Mutex<super::NativePluginLoader>>,

    /// Config watcher for hot-reload
    config_watcher: Arc<RwLock<Option<super::ConfigWatcher>>>,

    /// Plugin search paths
    search_paths: Arc<RwLock<Vec<PathBuf>>>,

    /// NeoTalk version for compatibility checking
    neotalk_version: String,
}

impl UnifiedPluginRegistry {
    /// Create a new unified plugin registry.
    pub fn new(neotalk_version: impl Into<String>) -> Self {
        Self {
            plugins: Arc::new(RwLock::new(HashMap::new())),
            native_loader: Arc::new(Mutex::new(super::NativePluginLoader::new())),
            config_watcher: Arc::new(RwLock::new(None)),
            search_paths: Arc::new(RwLock::new(Vec::new())),
            neotalk_version: neotalk_version.into(),
        }
    }

    /// Add a search path for plugins.
    pub async fn add_search_path(&self, path: impl AsRef<Path>) {
        let mut paths = self.search_paths.write().await;
        paths.push(path.as_ref().to_path_buf());
    }

    /// Register a plugin.
    pub async fn register(
        &self,
        id: String,
        plugin: DynUnifiedPlugin,
        plugin_type: PluginType,
    ) -> Result<()> {
        let metadata = plugin.read().await.metadata().clone();

        // Verify ID matches
        if metadata.base.id != id {
            return Err(PluginError::InitializationFailed(format!(
                "Plugin ID mismatch: expected {}, got {}",
                id, metadata.base.id
            )));
        }

        let info = PluginInfo::new(metadata, plugin_type);
        let instance = PluginInstance::new(plugin, info);

        let mut plugins = self.plugins.write().await;
        plugins.insert(id.clone(), instance);

        tracing::info!("Plugin registered: {}", id);
        Ok(())
    }

    /// Unregister a plugin.
    pub async fn unregister(&self, id: &str) -> Result<()> {
        let mut plugins = self.plugins.write().await;

        if let Some(mut instance) = plugins.remove(id) {
            // Shutdown the plugin
            let mut plugin = instance.plugin.write().await;
            if let Err(e) = plugin.shutdown().await {
                tracing::warn!("Failed to shutdown plugin {}: {}", id, e);
            }

            tracing::info!("Plugin unregistered: {}", id);
            Ok(())
        } else {
            Err(PluginError::NotFound(id.to_string()))
        }
    }

    /// Get a plugin by ID.
    pub async fn get(&self, id: &str) -> Option<DynUnifiedPlugin> {
        let plugins = self.plugins.read().await;
        plugins.get(id).map(|p| p.plugin.clone())
    }

    /// Get plugin info by ID.
    pub async fn get_info(&self, id: &str) -> Option<PluginInfo> {
        let plugins = self.plugins.read().await;
        plugins.get(id).map(|p| p.info.clone())
    }

    /// List all registered plugins.
    pub async fn list(&self) -> Vec<PluginInfo> {
        let plugins = self.plugins.read().await;
        plugins.values().map(|p| p.info.clone()).collect()
    }

    /// List plugins by type.
    pub async fn list_by_type(&self, plugin_type: PluginType) -> Vec<PluginInfo> {
        let plugins = self.plugins.read().await;
        plugins.values()
            .filter(|p| p.info.plugin_type == plugin_type)
            .map(|p| p.info.clone())
            .collect()
    }

    /// Initialize a plugin.
    pub async fn initialize(&self, id: &str, config: &Value) -> Result<()> {
        let plugins = self.plugins.read().await;
        let instance = plugins.get(id)
            .ok_or_else(|| PluginError::NotFound(id.to_string()))?;

        let mut plugin = instance.plugin.write().await;
        plugin.initialize(config).await?;

        // Update state
        drop(plugin);
        drop(plugins);

        let mut plugins = self.plugins.write().await;
        if let Some(instance) = plugins.get_mut(id) {
            instance.state_machine.transition(PluginState::Initialized, "Initialize called".to_string())?;
            instance.info.state = PluginState::Initialized;
        }

        tracing::info!("Plugin initialized: {}", id);
        Ok(())
    }

    /// Start a plugin.
    pub async fn start(&self, id: &str) -> Result<()> {
        let plugins = self.plugins.read().await;
        let instance = plugins.get(id)
            .ok_or_else(|| PluginError::NotFound(id.to_string()))?;

        if !instance.info.enabled {
            return Err(PluginError::InitializationFailed(format!(
                "Plugin {} is disabled", id
            )));
        }

        let mut plugin = instance.plugin.write().await;
        plugin.start().await?;

        // Update state and stats
        drop(plugin);
        drop(plugins);

        let mut plugins = self.plugins.write().await;
        if let Some(instance) = plugins.get_mut(id) {
            instance.state_machine.transition(PluginState::Running, "Start called".to_string())?;
            instance.info.state = PluginState::Running;
            instance.info.stats.record_start();
        }

        tracing::info!("Plugin started: {}", id);
        Ok(())
    }

    /// Stop a plugin.
    pub async fn stop(&self, id: &str) -> Result<()> {
        let plugins = self.plugins.read().await;
        let instance = plugins.get(id)
            .ok_or_else(|| PluginError::NotFound(id.to_string()))?;

        let start_time = instance.info.stats.last_start_time;
        let mut plugin = instance.plugin.write().await;
        plugin.stop().await?;

        // Update state and stats
        drop(plugin);
        drop(plugins);

        let mut plugins = self.plugins.write().await;
        if let Some(instance) = plugins.get_mut(id) {
            instance.state_machine.transition(PluginState::Stopped, "Stop called".to_string())?;
            instance.info.state = PluginState::Stopped;

            // Calculate duration
            if let Some(start) = start_time {
                let duration_ms = (chrono::Utc::now().timestamp_millis() - start.timestamp_millis()) as u64;
                instance.info.stats.record_stop(duration_ms);
            }
        }

        tracing::info!("Plugin stopped: {}", id);
        Ok(())
    }

    /// Enable a plugin.
    pub async fn enable(&self, id: &str) -> Result<()> {
        let mut plugins = self.plugins.write().await;
        let instance = plugins.get_mut(id)
            .ok_or_else(|| PluginError::NotFound(id.to_string()))?;

        instance.info.enabled = true;
        tracing::info!("Plugin enabled: {}", id);
        Ok(())
    }

    /// Disable a plugin.
    pub async fn disable(&self, id: &str) -> Result<()> {
        // Stop if running
        if self.is_running(id).await {
            let _ = self.stop(id).await;
        }

        let mut plugins = self.plugins.write().await;
        let instance = plugins.get_mut(id)
            .ok_or_else(|| PluginError::NotFound(id.to_string()))?;

        instance.info.enabled = false;
        tracing::info!("Plugin disabled: {}", id);
        Ok(())
    }

    /// Check if a plugin is running.
    pub async fn is_running(&self, id: &str) -> bool {
        if let Some(info) = self.get_info(id).await {
            matches!(info.state, PluginState::Running)
        } else {
            false
        }
    }

    /// Health check for a plugin.
    pub async fn health_check(&self, id: &str) -> Result<()> {
        let plugins = self.plugins.read().await;
        let instance = plugins.get(id)
            .ok_or_else(|| PluginError::NotFound(id.to_string()))?;

        let plugin = instance.plugin.read().await;
        plugin.health_check().await
    }

    /// Get plugin statistics.
    pub async fn get_stats(&self, id: &str) -> Option<PluginStats> {
        let plugins = self.plugins.read().await;
        plugins.get(id).map(|p| {
            let mut stats = p.info.stats.clone();
            // Update with live stats from plugin
            let plugin = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(p.plugin.read())
            });
            let live_stats = plugin.get_stats();
            // Merge stats
            stats.start_count = live_stats.start_count.max(stats.start_count);
            stats.stop_count = live_stats.stop_count.max(stats.stop_count);
            stats.error_count = live_stats.error_count.max(stats.error_count);
            stats
        })
    }

    /// Execute a plugin command.
    pub async fn execute_command(&self, id: &str, command: &str, args: &Value) -> Result<Value> {
        let plugins = self.plugins.read().await;
        let instance = plugins.get(id)
            .ok_or_else(|| PluginError::NotFound(id.to_string()))?;

        let plugin = instance.plugin.read().await;
        plugin.handle_command(command, args).await
    }

    /// Discover and load native plugins from search paths.
    pub async fn discover_native_plugins(&self) -> Result<usize> {
        let paths = self.search_paths.read().await;
        let loader = self.native_loader.lock().await;

        let mut loaded = 0;

        for search_path in paths.iter() {
            let entries = std::fs::read_dir(search_path)
                .map_err(|e| PluginError::InitializationFailed(format!("Failed to read search path {:?}: {}", search_path, e)))?;

            for entry in entries.flatten() {
                let path = entry.path();

                // Check file extension based on platform
                let is_plugin = cfg!(target_os = "macos")
                    .then(|| path.extension().and_then(|e| e.to_str()) == Some("dylib"))
                    .unwrap_or_else(|| {
                        cfg!(target_os = "linux")
                            .then(|| path.extension().and_then(|e| e.to_str()) == Some("so"))
                            .unwrap_or_else(|| {
                                cfg!(target_os = "windows")
                                    .then(|| path.extension().and_then(|e| e.to_str()) == Some("dll"))
                                    .unwrap_or(false)
                            })
                    });

                if is_plugin {
                    match self.load_native_plugin(&path).await {
                        Ok(_) => loaded += 1,
                        Err(e) => {
                            tracing::warn!("Failed to load plugin {:?}: {}", path, e);
                        }
                    }
                }
            }
        }

        Ok(loaded)
    }

    /// Load a native plugin from a file path.
    pub async fn load_native_plugin(&self, path: &Path) -> Result<String> {
        let _loader = self.native_loader.lock().await;
        let loaded = _loader.load_from_path(path)?;

        // Create a wrapper plugin that implements UnifiedPlugin
        let _wrapper = unsafe { super::NativePluginWrapper::from_loaded(&loaded)? };

        // TODO: Convert native plugin to UnifiedPlugin trait object
        // For now, we need a bridge implementation
        tracing::info!("Native plugin loaded: {}", loaded.metadata.id);

        Ok(loaded.metadata.id.clone())
    }

    /// Reload a plugin (for hot-reload support).
    pub async fn reload(&self, id: &str) -> Result<()> {
        let info = self.get_info(id).await
            .ok_or_else(|| PluginError::NotFound(id.to_string()))?;

        // If it's a native plugin with a path, reload it
        if let Some(path) = &info.path {
            let was_running = self.is_running(id).await;

            // Unregister and reload
            self.unregister(id).await?;
            self.load_native_plugin(path).await?;

            // Restart if it was running
            if was_running {
                self.start(id).await?;
            }

            tracing::info!("Plugin reloaded: {}", id);
        }

        Ok(())
    }

    /// Shutdown all plugins.
    pub async fn shutdown_all(&self) -> Result<()> {
        let plugins = self.plugins.read().await;
        let ids: Vec<String> = plugins.keys().cloned().collect();
        drop(plugins);

        for id in ids {
            if let Err(e) = self.unregister(&id).await {
                tracing::warn!("Failed to unregister plugin {}: {}", id, e);
            }
        }

        Ok(())
    }
}

impl Default for UnifiedPluginRegistry {
    fn default() -> Self {
        Self::new(env!("CARGO_PKG_VERSION"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = UnifiedPluginRegistry::new("1.0.0");
        assert_eq!(registry.neotalk_version, "1.0.0");
    }

    #[test]
    fn test_load_options_default() {
        let opts = PluginLoadOptions::default();
        assert!(!opts.auto_start);
        assert!(opts.enabled);
        assert!(opts.config.is_none());
    }
}
