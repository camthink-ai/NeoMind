//! Dynamic plugin factory.
//!
//! This module provides a factory for creating and managing dynamic plugin instances,
//! integrating with the plugin registry for seamless plugin lifecycle management.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use libloading::Library;
use serde_json::Value;
use tokio::sync::RwLock as TokioRwLock;

use super::{DynamicPluginLoader, DynamicPluginWrapper, LoadedPlugin, SecurityContext};
use crate::plugin::{
    DynUnifiedPlugin, PluginError, PluginState, PluginStats, Result, UnifiedPlugin,
};

/// Factory for creating and managing dynamic plugins.
pub struct DynamicPluginFactory {
    /// Plugin loader for loading plugins from files
    loader: Arc<TokioRwLock<DynamicPluginLoader>>,

    /// Loaded libraries (kept alive to prevent unloading)
    libraries: Arc<TokioRwLock<HashMap<PathBuf, Library>>>,

    /// Active plugin instances
    plugins: Arc<TokioRwLock<HashMap<String, DynamicPluginEntry>>>,

    /// Event callback for plugin lifecycle events
    event_callback: Arc<TokioRwLock<Option<Box<dyn Fn(PluginFactoryEvent) + Send + Sync>>>>,
}

/// Entry for an active plugin instance.
struct DynamicPluginEntry {
    /// The wrapper instance
    wrapper: Arc<TokioRwLock<DynamicPluginWrapper>>,

    /// Path to the plugin file
    path: PathBuf,

    /// When the plugin was loaded
    loaded_at: chrono::DateTime<chrono::Utc>,
}

/// Events emitted by the plugin factory.
#[derive(Debug, Clone)]
pub enum PluginFactoryEvent {
    /// Plugin was loaded
    Loaded { plugin_id: String, path: PathBuf },

    /// Plugin was initialized
    Initialized { plugin_id: String },

    /// Plugin was started
    Started { plugin_id: String },

    /// Plugin was stopped
    Stopped { plugin_id: String },

    /// Plugin was unloaded
    Unloaded { plugin_id: String },

    /// Plugin encountered an error
    Error { plugin_id: String, error: String },
}

impl DynamicPluginFactory {
    /// Create a new factory with default settings.
    pub fn new() -> Self {
        Self {
            loader: Arc::new(TokioRwLock::new(DynamicPluginLoader::new())),
            libraries: Arc::new(TokioRwLock::new(HashMap::new())),
            plugins: Arc::new(TokioRwLock::new(HashMap::new())),
            event_callback: Arc::new(TokioRwLock::new(None)),
        }
    }

    /// Create a factory with a custom security context.
    pub fn with_security(security: SecurityContext) -> Self {
        Self {
            loader: Arc::new(TokioRwLock::new(DynamicPluginLoader::with_security(security))),
            libraries: Arc::new(TokioRwLock::new(HashMap::new())),
            plugins: Arc::new(TokioRwLock::new(HashMap::new())),
            event_callback: Arc::new(TokioRwLock::new(None)),
        }
    }

    /// Set the event callback.
    pub fn with_event_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(PluginFactoryEvent) + Send + Sync + 'static,
    {
        let boxed: Box<dyn Fn(PluginFactoryEvent) + Send + Sync> = Box::new(callback);
        self.event_callback = Arc::new(TokioRwLock::new(Some(boxed)));
        self
    }

    /// Add a search path for plugins.
    pub async fn add_search_path(&self, path: impl AsRef<Path>) {
        let mut loader = self.loader.write().await;
        loader.add_search_path(path);
    }

    /// Discover all plugins in the search paths.
    pub async fn discover(&self) -> Vec<LoadedPlugin> {
        let mut loader = self.loader.write().await;
        loader.discover()
    }

    /// Get search paths.
    pub async fn search_paths(&self) -> Vec<PathBuf> {
        let loader = self.loader.read().await;
        loader.search_paths().to_vec()
    }

    /// Load a plugin from a file path.
    pub async fn load_from_path(&self, path: impl AsRef<Path>) -> Result<String> {
        let path = path.as_ref();

        // Load the plugin using the loader
        let (wrapper, library) = {
            let mut loader = self.loader.write().await;
            loader.load_from_path(path)?
        };

        let plugin_id = wrapper.descriptor().id.clone();

        // Store the library to keep it alive
        let mut libraries = self.libraries.write().await;
        libraries.insert(path.to_path_buf(), library);

        // Create the entry
        let entry = DynamicPluginEntry {
            wrapper: Arc::new(TokioRwLock::new(wrapper)),
            path: path.to_path_buf(),
            loaded_at: chrono::Utc::now(),
        };

        // Store the plugin
        let mut plugins = self.plugins.write().await;
        plugins.insert(plugin_id.clone(), entry);

        // Emit event
        self.emit_event(PluginFactoryEvent::Loaded {
            plugin_id: plugin_id.clone(),
            path: path.to_path_buf(),
        })
        .await;

        Ok(plugin_id)
    }

    /// Load a plugin from bytes (for uploads).
    ///
    /// This writes the bytes to a temporary file and loads it.
    pub async fn load_from_bytes(&self, bytes: &[u8], filename: &str) -> Result<String> {
        // Create a temporary file
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join(format!("neotalk_plugin_{}", filename));

        // Write the bytes to the file
        std::fs::write(&temp_path, bytes)
            .map_err(|e| PluginError::LoadFailed(format!("Failed to write plugin: {}", e)))?;

        // Load from the temporary path
        self.load_from_path(&temp_path).await
    }

    /// Initialize a plugin.
    pub async fn initialize(&self, plugin_id: &str, config: &Value) -> Result<()> {
        let entry = {
            let plugins = self.plugins.read().await;
            plugins
                .get(plugin_id)
                .ok_or_else(|| PluginError::NotFound(plugin_id.to_string()))?
                .wrapper
                .clone()
        };

        let mut wrapper = entry.write().await;
        wrapper.initialize(config).await?;

        // Emit event
        self.emit_event(PluginFactoryEvent::Initialized {
            plugin_id: plugin_id.to_string(),
        })
        .await;

        Ok(())
    }

    /// Start a plugin.
    pub async fn start(&self, plugin_id: &str) -> Result<()> {
        let entry = {
            let plugins = self.plugins.read().await;
            plugins
                .get(plugin_id)
                .ok_or_else(|| PluginError::NotFound(plugin_id.to_string()))?
                .wrapper
                .clone()
        };

        let mut wrapper = entry.write().await;
        wrapper.start().await?;

        // Emit event
        self.emit_event(PluginFactoryEvent::Started {
            plugin_id: plugin_id.to_string(),
        })
        .await;

        Ok(())
    }

    /// Stop a plugin.
    pub async fn stop(&self, plugin_id: &str) -> Result<()> {
        let entry = {
            let plugins = self.plugins.read().await;
            plugins
                .get(plugin_id)
                .ok_or_else(|| PluginError::NotFound(plugin_id.to_string()))?
                .wrapper
                .clone()
        };

        let mut wrapper = entry.write().await;
        wrapper.stop().await?;

        // Emit event
        self.emit_event(PluginFactoryEvent::Stopped {
            plugin_id: plugin_id.to_string(),
        })
        .await;

        Ok(())
    }

    /// Unload a plugin.
    pub async fn unload(&self, plugin_id: &str) -> Result<()> {
        let entry = {
            let mut plugins = self.plugins.write().await;
            plugins
                .remove(plugin_id)
                .ok_or_else(|| PluginError::NotFound(plugin_id.to_string()))?
        };

        // Shutdown the wrapper
        let mut wrapper = entry.wrapper.write().await;
        wrapper.shutdown().await?;

        // Drop the wrapper and remove the library
        drop(wrapper);

        // Remove the library (this will unload it when dropped)
        let mut libraries = self.libraries.write().await;
        libraries.remove(&entry.path);

        // Emit event
        self.emit_event(PluginFactoryEvent::Unloaded {
            plugin_id: plugin_id.to_string(),
        })
        .await;

        Ok(())
    }

    /// Get a plugin wrapper by ID.
    pub async fn get_plugin(&self, plugin_id: &str) -> Result<Arc<TokioRwLock<DynamicPluginWrapper>>> {
        let plugins = self.plugins.read().await;
        let entry = plugins
            .get(plugin_id)
            .ok_or_else(|| PluginError::NotFound(plugin_id.to_string()))?;

        Ok(entry.wrapper.clone())
    }

    /// Get all loaded plugin IDs.
    pub async fn plugin_ids(&self) -> Vec<String> {
        let plugins = self.plugins.read().await;
        plugins.keys().cloned().collect()
    }

    /// Get the state of a plugin.
    pub async fn get_state(&self, plugin_id: &str) -> Result<PluginState> {
        let wrapper = self.get_plugin(plugin_id).await?;
        let wrapper = wrapper.read().await;
        Ok(wrapper.get_state())
    }

    /// Get the stats of a plugin.
    pub async fn get_stats(&self, plugin_id: &str) -> Result<PluginStats> {
        let wrapper = self.get_plugin(plugin_id).await?;
        let wrapper = wrapper.read().await;
        Ok(wrapper.get_stats())
    }

    /// Execute a command on a plugin.
    pub async fn execute_command(
        &self,
        plugin_id: &str,
        command: &str,
        args: &Value,
    ) -> Result<Value> {
        let wrapper = self.get_plugin(plugin_id).await?;
        let wrapper = wrapper.read().await;
        wrapper.handle_command(command, args).await
    }

    /// Perform health check on a plugin.
    pub async fn health_check(&self, plugin_id: &str) -> Result<()> {
        let wrapper = self.get_plugin(plugin_id).await?;
        let wrapper = wrapper.read().await;
        wrapper.health_check().await
    }

    /// Reload a plugin.
    pub async fn reload(&self, plugin_id: &str) -> Result<()> {
        // Get the current entry
        let (path, _config) = {
            let plugins = self.plugins.read().await;
            let entry = plugins
                .get(plugin_id)
                .ok_or_else(|| PluginError::NotFound(plugin_id.to_string()))?;

            // Get current config from metadata
            let wrapper = entry.wrapper.read().await;
            let config = serde_json::json!({}); // Plugins use empty config on reload

            (entry.path.clone(), config)
        };

        // Unload the plugin
        self.unload(plugin_id).await?;

        // Reload from path
        let new_id = self.load_from_path(&path).await?;

        // Initialize and start
        self.initialize(&new_id, &serde_json::json!({})).await?;
        self.start(&new_id).await?;

        Ok(())
    }

    /// Peek at a plugin without loading it.
    pub async fn peek(&self, path: &Path) -> Result<super::ParsedPluginDescriptor> {
        let loader = self.loader.read().await;
        // Need to call peek which requires &self, so we need to clone loader
        // or use a different approach. For now, we'll use unsafe to extend lifetime.
        // In practice, peek doesn't store any references, so this is safe.
        // A better approach would be to make loader peek take &self and not modify.
        let loader_ref = &*loader;
        // Peek requires &self but doesn't modify, so we need to work around
        // the RwLock guard. The cleanest way is to use a separate method
        // that doesn't require mutable access.

        // For now, let's create a temporary loader just for peeking
        drop(loader_ref);
        let temp_loader = DynamicPluginLoader::new();
        temp_loader.peek(path)
    }

    /// Emit an event if a callback is set.
    async fn emit_event(&self, event: PluginFactoryEvent) {
        let callback = self.event_callback.read().await;
        if let Some(cb) = callback.as_ref() {
            cb(event);
        }
    }

    /// Get the loader (for advanced usage).
    pub async fn get_loader(&self) -> Arc<TokioRwLock<DynamicPluginLoader>> {
        self.loader.clone()
    }
}

impl Default for DynamicPluginFactory {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert a DynamicPluginWrapper reference to a DynUnifiedPlugin.
///
/// This is a convenience function for when you need to pass a dynamic plugin
/// to code that expects a DynUnifiedPlugin.
pub fn wrapper_to_unified(
    wrapper: Arc<TokioRwLock<DynamicPluginWrapper>>,
) -> DynUnifiedPlugin {
    wrapper
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_factory_creation() {
        let factory = DynamicPluginFactory::new();
        // Factory created successfully
    }

    #[tokio::test]
    async fn test_empty_plugins() {
        let factory = DynamicPluginFactory::new();
        let ids = factory.plugin_ids().await;
        assert!(ids.is_empty());
    }

    #[tokio::test]
    async fn test_search_paths() {
        let factory = DynamicPluginFactory::new();
        factory.add_search_path("/tmp/test").await;
        let paths = factory.search_paths().await;
        assert!(paths.iter().any(|p| p.ends_with("test")));
    }
}
