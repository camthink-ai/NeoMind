//! Dynamic plugin wrapper.
//!
//! This module provides a wrapper around dynamically loaded plugin instances,
//! implementing the UnifiedPlugin trait for safe interaction.

use std::fmt::{self, Display, Formatter};

use async_trait::async_trait;
use serde_json::Value;

use super::descriptor::{PluginDestroyFn, ParsedPluginDescriptor};
use crate::plugin::{
    ExtendedPluginMetadata, PluginError, PluginMetadata, PluginState, PluginStats,
    PluginType, Result, UnifiedPlugin,
};

/// Wrapper for a dynamically loaded plugin instance.
pub struct DynamicPluginWrapper {
    /// Parsed descriptor from the plugin file
    descriptor: ParsedPluginDescriptor,

    /// Metadata
    metadata: ExtendedPluginMetadata,

    /// Current state
    state: PluginState,

    /// Statistics
    stats: PluginStats,

    /// Instance pointer (opaque, from the plugin)
    instance: Option<*mut ()>,

    /// Destroy function pointer
    destroy_fn: PluginDestroyFn,

    /// Whether the plugin has been initialized
    initialized: bool,

    /// Whether the plugin is currently running
    running: bool,
}

// SAFETY: The DynamicPluginWrapper is Send+Sync because:
// 1. The instance pointer is only accessed through mutex-protected methods
// 2. All operations are serialized through the async trait methods
// 3. The underlying plugin is expected to be thread-safe (checked via capabilities)
unsafe impl Send for DynamicPluginWrapper {}
unsafe impl Sync for DynamicPluginWrapper {}

impl DynamicPluginWrapper {
    /// Create a new wrapper from a parsed descriptor.
    pub fn new(mut descriptor: ParsedPluginDescriptor) -> Result<Self> {
        // Create base metadata
        let base = PluginMetadata::new(&descriptor.id, &descriptor.name, &descriptor.version, &descriptor.required_neotalk)
            .with_description(descriptor.description.clone())
            .with_author(descriptor.author.clone().unwrap_or_default())
            .with_type(&descriptor.plugin_type);

        // Determine plugin type
        let plugin_type = plugin_type_from_str(&descriptor.plugin_type)?;

        // Parse version requirement to get minimum version
        // Extract version number from requirement (e.g., ">=1.0.0" -> "1.0.0")
        let version_str = descriptor.required_neotalk
            .trim_start_matches(">=")
            .trim_start_matches("^")
            .trim_start_matches("~")
            .trim_start_matches("=")
            .trim();

        // Create extended metadata
        let metadata = ExtendedPluginMetadata {
            base,
            plugin_type,
            version: semver::Version::parse(&descriptor.version)
                .unwrap_or_else(|_| semver::Version::new(1, 0, 0)),
            required_neotalk_version: semver::Version::parse(version_str)
                .unwrap_or_else(|_| semver::Version::new(1, 0, 0)),
            dependencies: vec![],
            config_schema: descriptor.config_schema.clone(),
            resource_limits: None,
            permissions: vec![],
            homepage: descriptor.homepage.clone(),
            repository: descriptor.repository.clone(),
            license: descriptor.license.clone(),
        };

        // Store destroy function pointer
        let destroy_fn = unsafe {
            if descriptor.destroy_fn.is_null() {
                return Err(PluginError::InitializationFailed(
                    "Plugin destroy function is null".into()
                ));
            }
            std::mem::transmute::<*const (), PluginDestroyFn>(descriptor.destroy_fn)
        };

        Ok(Self {
            descriptor,
            metadata,
            state: PluginState::Loaded,
            stats: PluginStats::default(),
            instance: None,
            destroy_fn,
            initialized: false,
            running: false,
        })
    }

    /// Get the descriptor.
    pub fn descriptor(&self) -> &ParsedPluginDescriptor {
        &self.descriptor
    }

    /// Check if the plugin is loaded (has an instance).
    pub fn is_loaded(&self) -> bool {
        self.instance.is_some()
    }

    /// Load the plugin instance by calling the create function.
    pub fn load(&mut self, config: &Value) -> Result<()> {
        if self.instance.is_some() {
            return Err(PluginError::InitializationFailed(
                "Plugin already loaded".into()
            ));
        }

        // Serialize config to JSON
        let config_json = serde_json::to_string(config)
            .map_err(|e| PluginError::SerializationError(e.to_string()))?;

        // Call the plugin's create function
        let instance_ptr = unsafe {
            let create_fn: super::descriptor::PluginCreateFn =
                std::mem::transmute(self.descriptor.create_fn);

            create_fn(config_json.as_ptr(), config_json.len())
        };

        if instance_ptr.is_null() {
            return Err(PluginError::InitializationFailed(
                "Plugin create function returned null".into()
            ));
        }

        self.instance = Some(instance_ptr);
        self.state = PluginState::Initialized;
        self.initialized = true;

        Ok(())
    }

    /// Unload the plugin instance.
    pub fn unload(&mut self) -> Result<()> {
        if let Some(instance) = self.instance.take() {
            unsafe {
                (self.destroy_fn)(instance);
            }
            self.state = PluginState::Loaded;
            self.initialized = false;
            self.running = false;
        }
        Ok(())
    }
}

impl Drop for DynamicPluginWrapper {
    fn drop(&mut self) {
        // Automatically unload when dropped
        let _ = self.unload();
    }
}

impl Display for DynamicPluginWrapper {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} v{} ({})",
            self.descriptor.name,
            self.descriptor.version,
            self.descriptor.id
        )
    }
}

#[async_trait]
impl UnifiedPlugin for DynamicPluginWrapper {
    fn metadata(&self) -> &ExtendedPluginMetadata {
        &self.metadata
    }

    async fn initialize(&mut self, config: &Value) -> Result<()> {
        if self.initialized {
            return Ok(());
        }

        self.load(config)?;
        self.state = PluginState::Initialized;
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        if !self.initialized {
            self.initialize(&Value::Null).await?;
        }

        self.running = true;
        self.state = PluginState::Running;
        self.stats.record_start();
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        self.running = false;
        self.state = PluginState::Stopped;
        self.stats.record_stop(0);
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        if self.running {
            self.stop().await?;
        }
        self.unload()?;
        self.state = PluginState::Loaded;
        Ok(())
    }

    fn get_state(&self) -> PluginState {
        self.state.clone()
    }

    async fn health_check(&self) -> Result<()> {
        if !self.initialized {
            return Err(PluginError::ExecutionFailed(
                "Plugin not initialized".into()
            ));
        }

        if !self.running {
            return Err(PluginError::ExecutionFailed("Plugin not running".into()));
        }

        Ok(())
    }

    fn get_stats(&self) -> PluginStats {
        self.stats.clone()
    }

    async fn handle_command(&self, command: &str, _args: &Value) -> Result<Value> {
        match command {
            "get_info" => {
                let info = serde_json::json!({
                    "id": self.descriptor.id,
                    "name": self.descriptor.name,
                    "version": self.descriptor.version,
                    "description": self.descriptor.description,
                    "plugin_type": self.descriptor.plugin_type,
                    "author": self.descriptor.author,
                    "homepage": self.descriptor.homepage,
                    "repository": self.descriptor.repository,
                    "license": self.descriptor.license,
                });
                Ok(info)
            }
            "get_config_schema" => Ok(self.descriptor.config_schema.clone().unwrap_or(Value::Null)),
            _ => Err(PluginError::ExecutionFailed(format!(
                "Unknown command: {}",
                command
            ))),
        }
    }
}

/// Convert a string to PluginType.
fn plugin_type_from_str(s: &str) -> Result<PluginType> {
    match s {
        "llm_backend" => Ok(PluginType::LlmBackend),
        "device_adapter" => Ok(PluginType::DeviceAdapter),
        "storage_backend" => Ok(PluginType::StorageBackend),
        "tool" => Ok(PluginType::Tool),
        "integration" => Ok(PluginType::Integration),
        "alert_channel" => Ok(PluginType::AlertChannel),
        _ => Ok(PluginType::Custom(s.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_type_from_str() {
        assert!(matches!(
            plugin_type_from_str("llm_backend").unwrap(),
            PluginType::LlmBackend
        ));
        assert!(matches!(
            plugin_type_from_str("device_adapter").unwrap(),
            PluginType::DeviceAdapter
        ));
        assert!(matches!(
            plugin_type_from_str("custom_type").unwrap(),
            PluginType::Custom(s) if s == "custom_type"
        ));
    }
}
