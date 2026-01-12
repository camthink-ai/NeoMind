//! Device adapter plugin registry.
//!
//! This module provides a registry for managing device adapters as plugins,
//! bridging between the adapter system and the unified plugin system.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

use crate::adapter::DeviceAdapter;
use crate::plugin_adapter::{
    DeviceAdapterPlugin, DeviceAdapterPluginFactory,
    AdapterPluginInfo, DeviceAdapterStats,
};
use edge_ai_core::{EventBus, plugin::{PluginType, PluginLoadOptions, UnifiedPlugin}};
use anyhow::Result;

/// Configuration for registering a device adapter plugin.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AdapterPluginConfig {
    /// Unique plugin identifier
    pub id: String,
    /// Display name
    pub name: String,
    /// Adapter type (mqtt, modbus, hass, etc.)
    pub adapter_type: String,
    /// Adapter-specific configuration
    pub config: serde_json::Value,
    /// Whether to auto-start the adapter
    #[serde(default)]
    pub auto_start: bool,
    /// Whether the plugin is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl AdapterPluginConfig {
    /// Create a new adapter plugin configuration.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        adapter_type: impl Into<String>,
        config: serde_json::Value,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            adapter_type: adapter_type.into(),
            config,
            auto_start: false,
            enabled: true,
        }
    }

    /// Create a configuration for an MQTT adapter.
    pub fn mqtt(
        id: impl Into<String>,
        name: impl Into<String>,
        broker: impl Into<String>,
        topics: Vec<String>,
    ) -> Self {
        let id_str = id.into();
        let client_id = format!("neotalk-{}", id_str);
        Self {
            id: id_str.clone(),
            name: name.into(),
            adapter_type: "mqtt".to_string(),
            config: serde_json::json!({
                "name": id_str,
                "mqtt": {
                    "broker": broker.into(),
                    "client_id": client_id,
                },
                "subscribe_topics": topics,
            }),
            auto_start: false,
            enabled: true,
        }
    }

    /// Create a configuration for a Modbus adapter.
    pub fn modbus(
        id: impl Into<String>,
        name: impl Into<String>,
        host: impl Into<String>,
        port: u16,
        slave_id: u8,
    ) -> Self {
        let id_str = id.into();
        Self {
            id: id_str.clone(),
            name: name.into(),
            adapter_type: "modbus".to_string(),
            config: serde_json::json!({
                "name": id_str,
                "host": host.into(),
                "port": port,
                "slave_id": slave_id,
            }),
            auto_start: false,
            enabled: true,
        }
    }
}

/// Registry for device adapter plugins.
pub struct DeviceAdapterPluginRegistry {
    /// Registered adapter plugins
    plugins: RwLock<HashMap<String, Arc<tokio::sync::RwLock<DeviceAdapterPlugin>>>>,
    /// Plugin configurations
    configs: RwLock<HashMap<String, AdapterPluginConfig>>,
    /// Event bus for publishing events
    event_bus: EventBus,
    /// NeoTalk version for compatibility
    neotalk_version: String,
}

impl DeviceAdapterPluginRegistry {
    /// Create a new device adapter plugin registry.
    pub fn new(event_bus: EventBus) -> Self {
        Self {
            plugins: RwLock::new(HashMap::new()),
            configs: RwLock::new(HashMap::new()),
            event_bus,
            neotalk_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Register a device adapter as a plugin.
    pub async fn register_adapter(
        &self,
        adapter: Arc<dyn DeviceAdapter>,
        config: AdapterPluginConfig,
    ) -> Result<()> {
        let plugin = DeviceAdapterPluginFactory::create_plugin(adapter, self.event_bus.clone());

        // Store the plugin
        {
            let mut plugins = self.plugins.write().await;
            plugins.insert(config.id.clone(), plugin);
        }

        // Store the configuration
        {
            let mut configs = self.configs.write().await;
            configs.insert(config.id.clone(), config);
        }

        Ok(())
    }

    /// Create and register an adapter from configuration.
    pub async fn register_from_config(
        &self,
        config: AdapterPluginConfig,
    ) -> Result<()> {
        // Create the adapter based on type
        let adapter = self.create_adapter(&config).await?;

        // Register as plugin
        self.register_adapter(adapter, config).await?;

        Ok(())
    }

    /// Create an adapter from configuration.
    async fn create_adapter(
        &self,
        config: &AdapterPluginConfig,
    ) -> Result<Arc<dyn DeviceAdapter>> {
        use crate::adapters::create_adapter;

        match config.adapter_type.as_str() {
            "mqtt" => {
                #[cfg(feature = "mqtt")]
                {
                    create_adapter("mqtt", &config.config, &self.event_bus)
                        .map_err(|e| anyhow::anyhow!("Failed to create MQTT adapter: {}", e))
                }
                #[cfg(not(feature = "mqtt"))]
                {
                    Err(anyhow::anyhow!("MQTT feature not enabled"))
                }
            }
            "modbus" => {
                #[cfg(feature = "modbus")]
                {
                    create_adapter("modbus", &config.config, &self.event_bus)
                        .map_err(|e| anyhow::anyhow!("Failed to create Modbus adapter: {}", e))
                }
                #[cfg(not(feature = "modbus"))]
                {
                    Err(anyhow::anyhow!("Modbus feature not enabled"))
                }
            }
            "hass" => {
                #[cfg(feature = "hass")]
                {
                    create_adapter("hass", &config.config, &self.event_bus)
                        .map_err(|e| anyhow::anyhow!("Failed to create HASS adapter: {}", e))
                }
                #[cfg(not(feature = "hass"))]
                {
                    Err(anyhow::anyhow!("HASS feature not enabled"))
                }
            }
            _ => Err(anyhow::anyhow!("Unknown adapter type: {}", config.adapter_type)),
        }
    }

    /// Unregister a device adapter plugin.
    pub async fn unregister(&self, id: &str) -> Result<()> {
        // Stop the plugin if running
        if let Some(plugin) = self.plugins.write().await.remove(id) {
            let mut p = plugin.write().await;
            let _ = p.shutdown().await;
        }

        // Remove configuration
        self.configs.write().await.remove(id);

        Ok(())
    }

    /// Get a plugin by ID.
    pub async fn get_plugin(&self, id: &str) -> Option<Arc<tokio::sync::RwLock<DeviceAdapterPlugin>>> {
        self.plugins.read().await.get(id).cloned()
    }

    /// List all device adapter plugins.
    pub async fn list_adapters(&self) -> Vec<AdapterPluginInfo> {
        let plugins = self.plugins.read().await;
        let configs = self.configs.read().await;

        plugins
            .iter()
            .map(|(id, plugin)| {
                let config = configs.get(id).unwrap();
                let plugin_ref = futures::executor::block_on(async { plugin.read().await });

                AdapterPluginInfo {
                    id: id.clone(),
                    name: config.name.clone(),
                    adapter_type: config.adapter_type.clone(),
                    enabled: config.enabled,
                    running: plugin_ref.adapter().is_running(),
                    device_count: plugin_ref.device_count(),
                    state: format!("{:?}", plugin_ref.get_state()),
                    version: plugin_ref.metadata().version.to_string(),
                    uptime_secs: plugin_ref.get_stats().start_count.try_into().ok(),
                    last_activity: chrono::Utc::now().timestamp(),
                }
            })
            .collect()
    }

    /// Get statistics for all device adapter plugins.
    pub async fn get_stats(&self) -> DeviceAdapterStats {
        let adapters = self.list_adapters().await;
        let total_adapters = adapters.len();
        let running_adapters = adapters.iter().filter(|a| a.running).count();
        let total_devices = adapters.iter().map(|a| a.device_count).sum();

        DeviceAdapterStats {
            total_adapters,
            running_adapters,
            total_devices,
            adapters,
        }
    }

    /// Start a device adapter plugin.
    pub async fn start_plugin(&self, id: &str) -> Result<()> {
        let plugin = self.get_plugin(id).await
            .ok_or_else(|| anyhow::anyhow!("Plugin not found: {}", id))?;

        let mut p = plugin.write().await;
        p.start().await
            .map_err(|e| anyhow::anyhow!("Failed to start plugin: {}", e))?;

        Ok(())
    }

    /// Stop a device adapter plugin.
    pub async fn stop_plugin(&self, id: &str) -> Result<()> {
        let plugin = self.get_plugin(id).await
            .ok_or_else(|| anyhow::anyhow!("Plugin not found: {}", id))?;

        let mut p = plugin.write().await;
        p.stop().await
            .map_err(|e| anyhow::anyhow!("Failed to stop plugin: {}", e))?;

        Ok(())
    }

    /// Get devices managed by a specific adapter plugin.
    pub async fn get_adapter_devices(&self, id: &str) -> Result<Vec<String>> {
        let plugin = self.get_plugin(id).await
            .ok_or_else(|| anyhow::anyhow!("Plugin not found: {}", id))?;

        let p = plugin.read().await;
        Ok(p.list_devices())
    }

    /// Check if a plugin exists.
    pub async fn contains(&self, id: &str) -> bool {
        self.plugins.read().await.contains_key(id)
    }

    /// Get the number of registered adapters.
    pub async fn count(&self) -> usize {
        self.plugins.read().await.len()
    }

    /// Get all plugin IDs.
    pub async fn list_ids(&self) -> Vec<String> {
        self.plugins.read().await.keys().cloned().collect()
    }

    /// Get configuration for a plugin.
    pub async fn get_config(&self, id: &str) -> Option<AdapterPluginConfig> {
        self.configs.read().await.get(id).cloned()
    }

    /// Update configuration for a plugin.
    pub async fn update_config(&self, id: &str, config: serde_json::Value) -> Result<()> {
        let mut configs = self.configs.write().await;
        if let Some(cfg) = configs.get_mut(id) {
            cfg.config = config;
            Ok(())
        } else {
            Err(anyhow::anyhow!("Plugin not found: {}", id))
        }
    }

    /// Start all enabled adapters.
    pub async fn start_all_enabled(&self) -> Result<()> {
        let configs = self.configs.read().await;
        let enabled_ids: Vec<String> = configs
            .iter()
            .filter(|(_, cfg)| cfg.enabled)
            .map(|(id, _)| id.clone())
            .collect();

        drop(configs);

        for id in enabled_ids {
            if let Err(e) = self.start_plugin(&id).await {
                tracing::warn!("Failed to start adapter plugin {}: {}", id, e);
            }
        }

        Ok(())
    }

    /// Stop all running adapters.
    pub async fn stop_all(&self) -> Result<()> {
        let ids = self.list_ids().await;

        for id in ids {
            let _ = self.stop_plugin(&id).await;
        }

        Ok(())
    }
}

impl Clone for DeviceAdapterPluginRegistry {
    fn clone(&self) -> Self {
        Self {
            plugins: RwLock::new(HashMap::new()),
            configs: RwLock::new(HashMap::new()),
            event_bus: self.event_bus.clone(),
            neotalk_version: self.neotalk_version.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::MockAdapter;

    #[tokio::test]
    async fn test_registry_creation() {
        let event_bus = EventBus::new();
        let registry = DeviceAdapterPluginRegistry::new(event_bus);

        assert_eq!(registry.count().await, 0);
        assert!(registry.list_ids().await.is_empty());
    }

    #[tokio::test]
    async fn test_adapter_config_mqtt() {
        let config = AdapterPluginConfig::mqtt(
            "test-mqtt",
            "Test MQTT",
            "localhost:1883",
            vec!["sensors/#".to_string()],
        );

        assert_eq!(config.id, "test-mqtt");
        assert_eq!(config.adapter_type, "mqtt");
        assert!(config.enabled);
    }

    #[tokio::test]
    async fn test_adapter_config_modbus() {
        let config = AdapterPluginConfig::modbus(
            "test-modbus",
            "Test Modbus",
            "192.168.1.100",
            502,
            1,
        );

        assert_eq!(config.id, "test-modbus");
        assert_eq!(config.adapter_type, "modbus");
        assert!(config.enabled);
    }

    #[tokio::test]
    async fn test_register_adapter() {
        let event_bus = EventBus::new();
        let registry = DeviceAdapterPluginRegistry::new(event_bus);

        let mock_adapter = Arc::new(MockAdapter::new("test-adapter").with_device("sensor1"));
        let config = AdapterPluginConfig::new(
            "test-adapter",
            "Test Adapter",
            "mock",
            serde_json::json!({}),
        );

        registry.register_adapter(mock_adapter, config).await.unwrap();

        assert_eq!(registry.count().await, 1);
        assert!(registry.contains("test-adapter").await);
    }

    #[tokio::test]
    async fn test_unregister_adapter() {
        let event_bus = EventBus::new();
        let registry = DeviceAdapterPluginRegistry::new(event_bus);

        let mock_adapter = Arc::new(MockAdapter::new("test-adapter"));
        let config = AdapterPluginConfig::new(
            "test-adapter",
            "Test Adapter",
            "mock",
            serde_json::json!({}),
        );

        registry.register_adapter(mock_adapter, config).await.unwrap();
        assert_eq!(registry.count().await, 1);

        registry.unregister("test-adapter").await.unwrap();
        assert_eq!(registry.count().await, 0);
    }

    #[tokio::test]
    async fn test_get_adapter_devices() {
        let event_bus = EventBus::new();
        let registry = DeviceAdapterPluginRegistry::new(event_bus);

        let mock_adapter = Arc::new(MockAdapter::new("test-adapter")
            .with_device("sensor1")
            .with_device("sensor2"));

        let config = AdapterPluginConfig::new(
            "test-adapter",
            "Test Adapter",
            "mock",
            serde_json::json!({}),
        );

        registry.register_adapter(mock_adapter, config).await.unwrap();

        let devices = registry.get_adapter_devices("test-adapter").await.unwrap();
        assert_eq!(devices.len(), 2);
        assert!(devices.contains(&"sensor1".to_string()));
        assert!(devices.contains(&"sensor2".to_string()));
    }

    #[tokio::test]
    async fn test_get_stats() {
        let event_bus = EventBus::new();
        let registry = DeviceAdapterPluginRegistry::new(event_bus);

        let mock_adapter = Arc::new(MockAdapter::new("test-adapter")
            .with_device("sensor1"));

        let config = AdapterPluginConfig::new(
            "test-adapter",
            "Test Adapter",
            "mock",
            serde_json::json!({}),
        );

        registry.register_adapter(mock_adapter, config).await.unwrap();

        let stats = registry.get_stats().await;
        assert_eq!(stats.total_adapters, 1);
        assert_eq!(stats.total_devices, 1);
    }

    #[tokio::test]
    async fn test_start_stop_plugin() {
        let event_bus = EventBus::new();
        let registry = DeviceAdapterPluginRegistry::new(event_bus);

        let mock_adapter = Arc::new(MockAdapter::new("test-adapter"));
        let config = AdapterPluginConfig::new(
            "test-adapter",
            "Test Adapter",
            "mock",
            serde_json::json!({}),
        );

        registry.register_adapter(mock_adapter, config).await.unwrap();

        // Initialize the plugin first (via UnifiedPluginRegistry would do this)
        let plugin = registry.get_plugin("test-adapter").await.unwrap();
        plugin.write().await.initialize(&serde_json::json!({})).await.unwrap();

        // Start the plugin
        registry.start_plugin("test-adapter").await.unwrap();

        let plugins = registry.list_adapters().await;
        assert_eq!(plugins[0].running, true);

        // Stop the plugin
        registry.stop_plugin("test-adapter").await.unwrap();

        let plugins = registry.list_adapters().await;
        assert_eq!(plugins[0].running, false);
    }
}
