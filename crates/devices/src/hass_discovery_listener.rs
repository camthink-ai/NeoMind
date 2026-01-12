//! HASS MQTT Discovery Listener
//!
//! This module provides automatic discovery of HASS ecosystem devices
//! (Tasmota, Shelly, ESPHome, etc.) that publish to HASS MQTT Discovery topics.
//!
//! ## Architecture Note
//!
//! **This is the single source of truth for HASS MQTT device discovery.**
//! HASS discovery functionality is intentionally kept in the device module (not plugins)
//! because:
//! - It directly integrates with the MDL registry for device type registration
//! - It creates device instances that are managed by MqttDeviceManager
//! - It provides a fast path for discovering and adding devices to the device list
//!
//! For HASS REST API integration (state queries, webhook handling, etc.), see the
//! `adapters/hass.rs` module which is used by the plugin system.
//!
//! ## Discovery Flow
//!
//! 1. Subscribe to `homeassistant/+/config` topics
//! 2. Parse discovery messages
//! 3. Map to MDL DeviceTypeDefinition
//! 4. Register with MdlRegistry
//! 5. Create device instances

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, error, debug};

use super::hass_discovery::{
    HassDiscoveryMessage, parse_discovery_message, is_discovery_topic,
    HassDiscoveryError, is_supported_component,
};
use super::hass_discovery_mapper::{map_hass_to_mdl, register_hass_device_type};
use super::mdl_format::{MdlRegistry, DeviceInstance, ConnectionStatus};
use super::mdl::DeviceError;

/// HASS Discovery Listener configuration
#[derive(Debug, Clone)]
pub struct HassDiscoveryConfig {
    /// Enable HASS discovery
    pub enabled: bool,

    /// Discovery topic prefix (default: "homeassistant")
    pub topic_prefix: String,

    /// Auto-register discovered devices
    pub auto_register: bool,

    /// Components to discover (empty = all supported)
    pub components: Vec<String>,
}

impl Default for HassDiscoveryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            topic_prefix: "homeassistant".to_string(),
            auto_register: true,
            components: vec![],
        }
    }
}

impl HassDiscoveryConfig {
    /// Get the subscription topic pattern for HASS discovery
    pub fn subscription_topic(&self) -> String {
        if self.components.is_empty() {
            format!("{}/+/config", self.topic_prefix)
        } else {
            // Subscribe to specific components
            self.components.iter()
                .map(|c| format!("{}/{}/+/config", self.topic_prefix, c))
                .collect::<Vec<_>>()
                .join(", ")
        }
    }

    /// Check if a topic matches the discovery pattern
    pub fn matches_topic(&self, topic: &str) -> bool {
        is_discovery_topic(topic) && topic.starts_with(&format!("{}/", self.topic_prefix))
    }

    /// Check if a component should be processed
    pub fn should_process_component(&self, component: &str) -> bool {
        self.components.is_empty() || self.components.contains(&component.to_string())
    }
}

/// HASS Discovery Listener
///
/// Listens for HASS MQTT Discovery messages and automatically
/// registers discovered devices with the MDL registry.
pub struct HassDiscoveryListener {
    /// Configuration
    config: HassDiscoveryConfig,

    /// MDL Registry for registering device types
    registry: Arc<MdlRegistry>,

    /// Discovered devices (device_type -> DeviceInstance)
    devices: Arc<RwLock<HashMap<String, DeviceInstance>>>,
}

impl HassDiscoveryListener {
    /// Create a new HASS discovery listener
    pub fn new(config: HassDiscoveryConfig, registry: Arc<MdlRegistry>) -> Self {
        Self {
            config,
            registry,
            devices: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get the subscription topic for MQTT
    pub fn subscription_topic(&self) -> String {
        self.config.subscription_topic()
    }

    /// Handle an incoming MQTT message
    ///
    /// Returns true if the message was a HASS discovery message (even if processing failed)
    pub async fn handle_message(&self, topic: &str, payload: &[u8]) -> bool {
        // Check if this is a HASS discovery topic
        if !self.config.matches_topic(topic) {
            return false;
        }

        debug!("Received HASS discovery message on topic: {}", topic);

        // Parse the discovery message
        let msg = match parse_discovery_message(topic, payload) {
            Ok(msg) => msg,
            Err(e) => {
                warn!("Failed to parse HASS discovery message: {}", e);
                return true;
            }
        };

        // Check if we should process this component
        if !self.config.should_process_component(&msg.topic_parts.component) {
            debug!("Skipping component: {}", msg.topic_parts.component);
            return true;
        }

        // Check if component is supported
        if !is_supported_component(&msg.topic_parts.component) {
            debug!("Unsupported component: {}", msg.topic_parts.component);
            return true;
        }

        // Map to MDL and register
        match self.process_discovery_message(&msg).await {
            Ok(device_type) => {
                info!("Registered HASS device: {} ({})",
                    device_type, msg.topic_parts.component);
            }
            Err(e) => {
                error!("Failed to process HASS discovery message: {}", e);
            }
        }

        true
    }

    /// Process a discovery message and register the device type
    async fn process_discovery_message(
        &self,
        msg: &HassDiscoveryMessage,
    ) -> Result<String, HassDiscoveryError> {
        // Map HASS config to MDL
        let def = map_hass_to_mdl(msg)?;
        let device_type = def.device_type.clone();

        info!("Discovered HASS device: {} ({})",
            def.name, def.device_type);

        // Register the device type if auto-register is enabled
        if self.config.auto_register {
            register_hass_device_type(&self.registry, msg).await?;

            // Create a device instance
            let instance = DeviceInstance {
                device_type: def.device_type.clone(),
                device_id: def.device_type.clone(),
                name: Some(def.name.clone()),
                status: ConnectionStatus::Online,
                last_seen: chrono::Utc::now(),
                config: std::collections::HashMap::new(),
                current_values: std::collections::HashMap::new(),
                adapter_id: Some("hass-discovery".to_string()),
            };

            self.devices.write().await.insert(device_type.clone(), instance);
        }

        Ok(device_type)
    }

    /// Get all discovered devices
    pub async fn list_devices(&self) -> Vec<DeviceInstance> {
        self.devices.read().await.values().cloned().collect()
    }

    /// Get a specific discovered device
    pub async fn get_device(&self, device_type: &str) -> Option<DeviceInstance> {
        self.devices.read().await.get(device_type).cloned()
    }

    /// Remove a discovered device
    pub async fn remove_device(&self, device_type: &str) -> Result<(), DeviceError> {
        self.devices.write().await.remove(device_type);
        self.registry.unregister(device_type).await
            .map_err(|_| DeviceError::NotFound(super::mdl::DeviceId::new()))?;
        Ok(())
    }

    /// Get the number of discovered devices
    pub async fn device_count(&self) -> usize {
        self.devices.read().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscription_topic() {
        let config = HassDiscoveryConfig::default();
        assert_eq!(config.subscription_topic(), "homeassistant/+/config");

        let config = HassDiscoveryConfig {
            components: vec!["switch".to_string(), "sensor".to_string()],
            ..Default::default()
        };
        assert!(config.subscription_topic().contains("homeassistant/switch/+/config"));
        assert!(config.subscription_topic().contains("homeassistant/sensor/+/config"));
    }

    #[test]
    fn test_matches_topic() {
        let config = HassDiscoveryConfig::default();

        assert!(config.matches_topic("homeassistant/switch/lamp/config"));
        assert!(config.matches_topic("homeassistant/sensor/temp/config"));
        assert!(!config.matches_topic("homeassistant/switch/lamp/state"));
        assert!(!config.matches_topic("tele/sensor/SENSOR"));
        assert!(!config.matches_topic("stat/POWER"));
    }

    #[test]
    fn test_should_process_component() {
        let config = HassDiscoveryConfig {
            components: vec!["switch".to_string(), "light".to_string()],
            ..Default::default()
        };

        assert!(config.should_process_component("switch"));
        assert!(config.should_process_component("light"));
        assert!(!config.should_process_component("sensor"));
        assert!(!config.should_process_component("cover"));

        // Empty components means all
        let config_all = HassDiscoveryConfig {
            components: vec![],
            ..Default::default()
        };
        assert!(config_all.should_process_component("switch"));
        assert!(config_all.should_process_component("sensor"));
    }

    #[tokio::test]
    async fn test_handle_discovery_message() {
        let registry = Arc::new(MdlRegistry::new());
        let listener = HassDiscoveryListener::new(HassDiscoveryConfig::default(), registry.clone());

        let topic = "homeassistant/switch/lamp/config";
        let payload = r#"{
            "name": "Lamp",
            "state_topic": "stat/lamp/POWER",
            "command_topic": "cmnd/lamp/POWER",
            "payload_on": "ON",
            "payload_off": "OFF"
        }"#.as_bytes();

        let handled = listener.handle_message(topic, payload).await;
        assert!(handled);

        // Device should be registered
        let device = listener.get_device("hass_lamp").await;
        assert!(device.is_some());
    }
}
