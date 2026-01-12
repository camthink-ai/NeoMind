//! Plugin system for NeoTalk.
//!
//! This module defines the plugin interface and registry for extending
//! the NeoTalk platform with custom implementations at compile time or
//! runtime (via WASM or native libraries).
//!
//! ## Plugin Types
//!
//! - **LLM Backends**: Custom LLM implementations
//! - **Storage Backends**: Custom storage implementations
//! - **Device Adapters**: Custom device/protocol adapters
//! - **Tools**: Custom function calling tools
//! - **Alert Channels**: Custom notification channels
//!
//! ## Dynamic Loading
//!
//! - **WASM Plugins**: WebAssembly modules for secure sandboxed execution
//! - **Native Plugins**: Dynamic libraries (.so, .dylib, .dll) via libloading
//! - **Config Hot Reload**: File watching for configuration changes

use serde_json::Value;
use std::sync::Arc;

// Core plugin types and traits
pub mod native;
pub mod wasm;
pub mod types;
pub mod watcher;
pub mod registry;

// Re-exports
pub use native::{NativePluginLoader, LoadedNativePlugin, NativePluginWrapper};
pub use wasm::{WasmPluginLoader, WasmPlugin, LoadedWasmPlugin, ValidationResult};
pub use types::{
    PluginType, PluginState, ExtendedPluginMetadata, PluginDependency, ResourceLimits,
    PluginPermission, StateMachine, UnifiedPlugin, DynUnifiedPlugin, PluginStats,
    PluginRegistryEvent,
};
pub use watcher::{ConfigWatcher, ConfigChangeCallback, HotConfig, ConfigReloadManager};
pub use registry::{UnifiedPluginRegistry, PluginInstance, PluginInfo, PluginLoadOptions};

/// Result type for plugin operations.
pub type Result<T> = std::result::Result<T, PluginError>;

/// Plugin error types.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    /// Plugin not found.
    #[error("Plugin not found: {0}")]
    NotFound(String),

    /// Plugin failed to initialize.
    #[error("Plugin initialization failed: {0}")]
    InitializationFailed(String),

    /// Plugin execution failed.
    #[error("Plugin execution failed: {0}")]
    ExecutionFailed(String),

    /// Invalid plugin configuration.
    #[error("Invalid plugin configuration: {0}")]
    InvalidConfiguration(String),

    /// Plugin version mismatch.
    #[error("Plugin version mismatch: expected {expected}, found {found}")]
    VersionMismatch { expected: String, found: String },

    /// WASM loading failed.
    #[error("WASM loading failed: {0}")]
    WasmLoadFailed(String),

    /// Other error.
    #[error("Plugin error: {0}")]
    Other(#[from] anyhow::Error),
}

/// Plugin metadata.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PluginMetadata {
    /// Unique plugin identifier.
    pub id: String,

    /// Plugin name.
    pub name: String,

    /// Plugin version.
    pub version: String,

    /// Required NeoTalk version (semver).
    pub required_neotalk_version: String,

    /// Plugin description.
    pub description: String,

    /// Plugin author.
    pub author: Option<String>,

    /// Plugin types (llm, storage, device, tool, alert, etc.).
    pub types: Vec<String>,

    /// Additional metadata.
    #[serde(flatten)]
    pub extra: Value,
}

impl PluginMetadata {
    /// Create a new plugin metadata.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        version: impl Into<String>,
        required_neotalk_version: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            version: version.into(),
            required_neotalk_version: required_neotalk_version.into(),
            description: String::new(),
            author: None,
            types: Vec::new(),
            extra: Value::Object(Default::default()),
        }
    }

    /// Set the description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set the author.
    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    /// Add a plugin type.
    pub fn with_type(mut self, plugin_type: impl Into<String>) -> Self {
        self.types.push(plugin_type.into());
        self
    }
}

/// Plugin trait for compile-time registered plugins.
///
/// Plugins can implement this trait to provide custom functionality
/// such as LLM backends, storage backends, device adapters, etc.
pub trait Plugin: Send + Sync {
    /// Get the plugin metadata.
    fn metadata(&self) -> &PluginMetadata;

    /// Initialize the plugin with the given configuration.
    fn initialize(&mut self, config: &Value) -> Result<()>;

    /// Check if the plugin is initialized.
    fn is_initialized(&self) -> bool;

    /// Shutdown the plugin and clean up resources.
    fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Dynamic plugin wrapper for trait objects.
pub type DynPlugin = Arc<std::sync::Mutex<dyn Plugin>>;

/// Plugin registry for managing plugins.
///
/// The registry supports both compile-time registered plugins
/// and runtime-loaded WASM plugins (future feature).
pub struct PluginRegistry {
    /// Registered plugins by ID.
    plugins: std::collections::HashMap<String, DynPlugin>,

    /// NeoTalk version for compatibility checking.
    neotalk_version: String,
}

impl PluginRegistry {
    /// Create a new plugin registry.
    pub fn new(neotalk_version: impl Into<String>) -> Self {
        Self {
            plugins: std::collections::HashMap::new(),
            neotalk_version: neotalk_version.into(),
        }
    }

    /// Register a plugin.
    ///
    /// # Example
    /// ```no_run
    /// use edge_ai_core::plugin::{PluginRegistry, Plugin, PluginMetadata};
    /// use std::sync::Arc;
    ///
    /// struct MyPlugin;
    ///
    /// impl Plugin for MyPlugin {
    ///     fn metadata(&self) -> &PluginMetadata {
    ///         // Return metadata
    ///         &metadata
    ///     }
    ///
    ///     fn initialize(&mut self, config: &serde_json::Value) -> edge_ai_core::plugin::Result<()> {
    ///         // Initialize
    ///         Ok(())
    ///     }
    ///
    ///     fn is_initialized(&self) -> bool {
    ///         true
    ///     }
    /// }
    /// ```
    pub fn register(&mut self, plugin: DynPlugin) -> Result<()> {
        let metadata = plugin.lock().unwrap().metadata().clone();

        // Check version compatibility
        if !self.check_version_compatibility(&metadata)? {
            return Err(PluginError::VersionMismatch {
                expected: self.neotalk_version.clone(),
                found: metadata.version,
            });
        }

        self.plugins.insert(metadata.id.clone(), plugin);
        Ok(())
    }

    /// Get a plugin by ID.
    pub fn get(&self, id: &str) -> Option<DynPlugin> {
        self.plugins.get(id).cloned()
    }

    /// List all registered plugin IDs.
    pub fn list(&self) -> Vec<String> {
        self.plugins.keys().cloned().collect()
    }

    /// Unregister a plugin.
    pub fn unregister(&mut self, id: &str) -> Result<()> {
        self.plugins
            .remove(id)
            .ok_or_else(|| PluginError::NotFound(id.to_string()))?;
        Ok(())
    }

    /// Get plugins by type.
    pub fn get_by_type(&self, plugin_type: &str) -> Vec<DynPlugin> {
        self.plugins
            .values()
            .filter(|p| {
                let guard = p.lock().unwrap();
                guard.metadata().types.contains(&plugin_type.to_string())
            })
            .cloned()
            .collect()
    }

    /// Shutdown all plugins.
    pub fn shutdown_all(&mut self) -> Result<()> {
        for plugin in self.plugins.values_mut() {
            let mut guard = plugin.lock().unwrap();
            guard.shutdown()?;
        }
        self.plugins.clear();
        Ok(())
    }

    /// Check version compatibility.
    fn check_version_compatibility(&self, metadata: &PluginMetadata) -> Result<bool> {
        // Simple version check - in production, use semver crate
        // For now, just check if major version matches
        let required = &metadata.required_neotalk_version;
        let current = &self.neotalk_version;

        if required == "*" || required == "any" {
            return Ok(true);
        }

        // Check if current version satisfies requirement
        // This is a simplified check - use semver crate in production
        if required.starts_with('^') {
            // Caret requirement - same logic as Cargo
            let req_ver = &required[1..];
            Ok(current.starts_with(req_ver))
        } else if required.starts_with('~') {
            // Tilde requirement
            let req_ver = &required[1..];
            Ok(current.starts_with(req_ver))
        } else if required.contains('=') {
            // Exact version
            Ok(current == required)
        } else {
            // Default to compatible
            Ok(current.starts_with(required))
        }
    }

    /// Load a WASM plugin (placeholder for future implementation).
    ///
    /// This method is a placeholder for future WASM plugin loading.
    /// When implemented, it will:
    /// 1. Load the WASM file
    /// 2. Extract metadata from the WASM module
    /// 3. Create a WASM plugin wrapper
    /// 4. Register the plugin
    #[allow(dead_code)]
    async fn load_wasm_plugin(
        &mut self,
        _wasm_path: impl AsRef<std::path::Path>,
    ) -> Result<()> {
        // TODO: Implement WASM plugin loading
        Err(PluginError::Other(anyhow::anyhow!(
            "WASM plugin loading not yet implemented"
        )))
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new(env!("CARGO_PKG_VERSION"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockPlugin {
        metadata: PluginMetadata,
        initialized: bool,
    }

    impl MockPlugin {
        fn new(id: &str) -> Self {
            Self {
                metadata: PluginMetadata::new(id, "Mock Plugin", "1.0.0", "*")
                    .with_description("A mock plugin for testing")
                    .with_type("test"),
                initialized: false,
            }
        }
    }

    impl Plugin for MockPlugin {
        fn metadata(&self) -> &PluginMetadata {
            &self.metadata
        }

        fn initialize(&mut self, _config: &Value) -> Result<()> {
            self.initialized = true;
            Ok(())
        }

        fn is_initialized(&self) -> bool {
            self.initialized
        }
    }

    #[test]
    fn test_plugin_registry() {
        let mut registry = PluginRegistry::new("1.0.0");

        let plugin = Arc::new(std::sync::Mutex::new(MockPlugin::new("test-plugin")));
        registry.register(plugin).unwrap();

        assert_eq!(registry.list().len(), 1);
        assert!(registry.get("test-plugin").is_some());
    }

    #[test]
    fn test_plugin_metadata() {
        let metadata = PluginMetadata::new("test", "Test Plugin", "1.0.0", "1.0.0")
            .with_description("A test plugin")
            .with_author("Test Author")
            .with_type("llm");

        assert_eq!(metadata.id, "test");
        assert_eq!(metadata.name, "Test Plugin");
        assert_eq!(metadata.description, "A test plugin");
        assert_eq!(metadata.author, Some("Test Author".to_string()));
        assert!(metadata.types.contains(&"llm".to_string()));
    }
}
