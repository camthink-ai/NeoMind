//! Home Assistant (HASS) REST API adapter for NeoTalk event-driven architecture.
//!
//! ## Architecture Note
//!
//! This adapter provides **REST API integration** with Home Assistant:
//! - Query device states via HASS REST API
//! - Send commands via HASS REST API
//! - Receive webhook updates from HASS
//!
//! **This is NOT the HASS MQTT discovery handler.** For MQTT-based device discovery,
//! see `hass_discovery_listener.rs` in the device module, which handles the
//! `homeassistant/+/config` discovery topics for auto-discovering Tasmota, Shelly,
//! ESPHome, and other HASS ecosystem devices.
//!
//! ## Purpose
//!
//! The HASS adapter is used when you want to:
//! - Integrate with an existing Home Assistant instance via REST API
//! - Query/control devices that are already managed by HASS
//! - Use HASS as the central hub while NeoTalk provides AI/automation features
//!
//! ## Usage
//!
//! ```no_run
//! use edge_ai_devices::adapters::{HassAdapter, HassAdapterConfig, create_hass_adapter};
//! use edge_ai_core::EventBus;
//!
//! # #[tokio::main]
//! # async fn main() -> anyhow::Result<()> {
//! let event_bus = EventBus::new();
//! let config = HassAdapterConfig {
//!     url: "http://localhost:8123".to_string(),
//!     token: "your_long_lived_access_token".to_string(),
//! };
//! let adapter = create_hass_adapter(config, &event_bus);
//! # Ok(())
//! # }
//! ```

use crate::adapter::{DeviceAdapter, DeviceEvent, DiscoveredDeviceInfo, AdapterResult};
use crate::mqtt::MqttConfig;
use edge_ai_core::EventBus;
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use serde::Deserialize;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

/// HASS discovery message.
#[derive(Debug, Deserialize, Clone)]
pub struct HassDiscoveryMessage {
    /// Unique identifier for this device
    #[serde(rename = "unique_id")]
    pub unique_id: String,

    /// Device name
    #[serde(default)]
    pub name: Option<String>,

    /// Device type (component)
    #[serde(rename = "component")]
    pub component: String,

    /// Device state topic
    #[serde(rename = "state_topic")]
    #[serde(default)]
    pub state_topic: Option<String>,

    /// Device command topic
    #[serde(rename = "command_topic")]
    #[serde(default)]
    pub command_topic: Option<String>,

    /// Device class
    #[serde(rename = "device_class")]
    #[serde(default)]
    pub device_class: Option<String>,

    /// Device information
    #[serde(default)]
    pub device: Option<HassDeviceInfo>,

    /// Additional configuration
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

/// Device information from HASS.
#[derive(Debug, Deserialize, Clone)]
pub struct HassDeviceInfo {
    /// List of identifiers for this device
    pub identifiers: Vec<String>,

    /// Device name
    #[serde(default)]
    pub name: Option<String>,

    /// Device model
    #[serde(default)]
    pub model: Option<String>,

    /// Device manufacturer
    #[serde(default)]
    pub manufacturer: Option<String>,

    /// Software version
    #[serde(default)]
    pub sw_version: Option<String>,

    /// Connection information
    #[serde(default)]
    pub connections: Option<Vec<Vec<String>>>,
}

/// HASS device adapter configuration.
#[derive(Debug, Clone)]
pub struct HassAdapterConfig {
    /// Adapter name
    pub name: String,
    /// MQTT broker configuration (HASS uses MQTT for discovery)
    pub mqtt: MqttConfig,
    /// Discovery topic pattern (default: homeassistant/+/+/config)
    pub discovery_topic: String,
    /// Auto-discover devices on startup
    pub auto_discover: bool,
}

impl HassAdapterConfig {
    /// Create a new HASS adapter configuration.
    pub fn new(name: impl Into<String>, broker: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            mqtt: MqttConfig::new(broker, "homeassistant"),
            discovery_topic: "homeassistant/+/+/config".to_string(),
            auto_discover: true,
        }
    }

    /// Set custom discovery topic.
    pub fn with_discovery_topic(mut self, topic: impl Into<String>) -> Self {
        self.discovery_topic = topic.into();
        self
    }

    /// Set MQTT authentication.
    pub fn with_auth(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        self.mqtt = self.mqtt.with_auth(username, password);
        self
    }

    /// Set MQTT port.
    pub fn with_port(mut self, port: u16) -> Self {
        self.mqtt = self.mqtt.with_port(port);
        self
    }

    /// Disable auto-discovery.
    pub fn without_auto_discover(mut self) -> Self {
        self.auto_discover = false;
        self
    }
}

/// HASS device adapter.
///
/// Subscribes to Home Assistant MQTT discovery topics and publishes device discovery events.
pub struct HassAdapter {
    /// Adapter configuration
    config: HassAdapterConfig,
    /// Event channel sender
    event_tx: broadcast::Sender<DeviceEvent>,
    /// Running state
    running: Arc<std::sync::atomic::AtomicBool>,
    /// Discovered devices
    devices: Arc<tokio::sync::RwLock<Vec<String>>>,
}

impl HassAdapter {
    /// Create a new HASS adapter.
    pub fn new(config: HassAdapterConfig) -> Self {
        let (event_tx, _) = broadcast::channel(1000);
        Self {
            config,
            event_tx,
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            devices: Arc::new(tokio::sync::RwLock::new(Vec::new())),
        }
    }

    /// Parse HASS component to device type.
    fn component_to_device_type(&self, component: &str) -> String {
        match component {
            "sensor" => "sensor".to_string(),
            "binary_sensor" => "binary_sensor".to_string(),
            "switch" => "switch".to_string(),
            "light" => "light".to_string(),
            "climate" => "thermostat".to_string(),
            "cover" => "cover".to_string(),
            "lock" => "lock".to_string(),
            "camera" => "camera".to_string(),
            "vacuum" => "vacuum".to_string(),
            _ => format!("{}_device", component),
        }
    }

    /// Handle HASS discovery message.
    fn handle_discovery_message(&self, topic: String, payload: &[u8]) {
        // Parse the discovery message
        let discovery_msg: HassDiscoveryMessage = match serde_json::from_slice(payload) {
            Ok(msg) => msg,
            Err(e) => {
                warn!("Failed to parse HASS discovery message: {}", e);
                return;
            }
        };

        // Extract device information
        let device_id = discovery_msg.unique_id.clone();
        let device_type = self.component_to_device_type(&discovery_msg.component);
        let name = discovery_msg.name.as_deref()
            .or(discovery_msg.device.as_ref().and_then(|d| d.name.as_deref()))
            .unwrap_or(&device_id);

        let device_info = DiscoveredDeviceInfo::new(&device_id, &device_type)
            .with_name(name)
            .with_endpoint(&topic);

        // Publish discovery event
        let _ = self.event_tx.send(DeviceEvent::Discovery { device: device_info });

        // Store discovered device
        if let Ok(mut devices) = self.devices.try_write() {
            if !devices.contains(&device_id) {
                devices.push(device_id);
            }
        }
    }

    /// Get the discovery topic subscription patterns.
    fn get_discovery_topics(&self) -> Vec<String> {
        let topic = &self.config.discovery_topic;
        // Convert wildcard pattern to actual subscription topics
        // For HASS, the main discovery topic is "homeassistant/+/+/config"
        vec![topic.clone()]
    }
}

#[async_trait]
impl DeviceAdapter for HassAdapter {
    fn name(&self) -> &str {
        &self.config.name
    }

    fn adapter_type(&self) -> &'static str {
        "hass"
    }

    fn is_running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::Relaxed)
    }

    async fn start(&self) -> AdapterResult<()> {
        if self.is_running() {
            return Ok(());
        }

        info!("Starting HASS adapter: {}", self.config.name);

        // In a real implementation, this would:
        // 1. Connect to the MQTT broker
        // 2. Subscribe to the discovery topic
        // 3. Process incoming discovery messages
        // For now, we simulate the adapter running
        self.running.store(true, std::sync::atomic::Ordering::Relaxed);

        // Spawn a task to simulate message processing
        let running = self.running.clone();
        let name = self.config.name.clone();

        tokio::spawn(async move {
            while running.load(std::sync::atomic::Ordering::Relaxed) {
                // Simulate message processing
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            debug!("HASS adapter '{}' stopped", name);
        });

        info!("HASS adapter '{}' started, discovery topic: {}",
              self.config.name, self.config.discovery_topic);
        Ok(())
    }

    async fn stop(&self) -> AdapterResult<()> {
        info!("Stopping HASS adapter: {}", self.config.name);
        self.running.store(false, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    fn subscribe(&self) -> Pin<Box<dyn Stream<Item = DeviceEvent> + Send + '_>> {
        let rx = self.event_tx.subscribe();
        Box::pin(async_stream::stream! {
            let mut rx = rx;
            while let Ok(event) = rx.recv().await {
                yield event;
            }
        })
    }

    fn device_count(&self) -> usize {
        self.devices.try_read().map(|v| v.len()).unwrap_or(0)
    }

    fn list_devices(&self) -> Vec<String> {
        self.devices.try_read().map(|v| v.clone()).unwrap_or_default()
    }
}

/// Create a HASS adapter connected to an event bus.
pub fn create_hass_adapter(
    config: HassAdapterConfig,
    event_bus: &EventBus,
) -> Arc<HassAdapter> {
    let adapter = Arc::new(HassAdapter::new(config));
    let adapter_clone = adapter.clone();
    let event_bus = event_bus.clone();

    // Spawn event forwarding task
    tokio::spawn(async move {
        let mut rx = adapter_clone.subscribe();
        while let Some(event) = rx.next().await {
            let device_id = event.device_id().unwrap_or("unknown").to_string();
            let neotalk_event = event.to_neotalk_event();
            let source = format!("adapter:hass:{}", device_id);
            event_bus.publish_with_source(neotalk_event, source).await;
        }
    });

    adapter
}

/// Convert HASS component to NeoTalk device type.
pub fn map_hass_component_to_device_type(component: &str) -> String {
    match component {
        "sensor" => "sensor".to_string(),
        "binary_sensor" => "binary_sensor".to_string(),
        "switch" => "switch".to_string(),
        "light" => "light".to_string(),
        "climate" => "thermostat".to_string(),
        "cover" => "cover".to_string(),
        "lock" => "lock".to_string(),
        "camera" => "camera".to_string(),
        "vacuum" => "vacuum".to_string(),
        "fan" => "fan".to_string(),
        "input_boolean" => "input_boolean".to_string(),
        _ => format!("hass_{}", component),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_builder() {
        let config = HassAdapterConfig::new("hass_mqtt", "localhost")
            .with_port(1883)
            .with_auth("user", "pass")
            .without_auto_discover();

        assert_eq!(config.name, "hass_mqtt");
        assert_eq!(config.discovery_topic, "homeassistant/+/+/config");
        assert!(!config.auto_discover);
    }

    #[test]
    fn test_component_mapping() {
        let adapter = HassAdapter::new(HassAdapterConfig::new("test", "localhost"));

        assert_eq!(adapter.component_to_device_type("sensor"), "sensor");
        assert_eq!(adapter.component_to_device_type("switch"), "switch");
        assert_eq!(adapter.component_to_device_type("climate"), "thermostat");
        assert_eq!(adapter.component_to_device_type("unknown"), "unknown_device");
    }

    #[test]
    fn test_map_hass_component_function() {
        assert_eq!(map_hass_component_to_device_type("sensor"), "sensor");
        assert_eq!(map_hass_component_to_device_type("light"), "light");
        assert_eq!(map_hass_component_to_device_type("climate"), "thermostat");
    }

    #[test]
    fn test_discovery_topic_default() {
        let config = HassAdapterConfig::new("test", "localhost");
        assert_eq!(config.discovery_topic, "homeassistant/+/+/config");
    }

    #[tokio::test]
    async fn test_adapter_lifecycle() {
        let config = HassAdapterConfig::new("test", "localhost");
        let adapter = HassAdapter::new(config);

        assert!(!adapter.is_running());
        adapter.start().await.unwrap();
        assert!(adapter.is_running());
        adapter.stop().await.unwrap();
        assert!(!adapter.is_running());
    }

    #[tokio::test]
    async fn test_name_and_type() {
        let config = HassAdapterConfig::new("my_hass", "localhost");
        let adapter = HassAdapter::new(config);

        assert_eq!(adapter.name(), "my_hass");
        assert_eq!(adapter.adapter_type(), "hass");
    }

    #[tokio::test]
    async fn test_discovery_message_parsing() {
        let json = r#"{
            "unique_id": "sensor_temp_123",
            "name": "Living Room Temperature",
            "component": "sensor",
            "state_topic": "homeassistant/sensor/temp/state",
            "device_class": "temperature"
        }"#;

        let msg: HassDiscoveryMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.unique_id, "sensor_temp_123");
        assert_eq!(msg.name, Some("Living Room Temperature".to_string()));
        assert_eq!(msg.component, "sensor");
        assert_eq!(msg.device_class, Some("temperature".to_string()));
    }

    #[tokio::test]
    async fn test_discovery_message_with_device_info() {
        let json = r#"{
            "unique_id": "switch_light_1",
            "component": "switch",
            "state_topic": "homeassistant/switch/light1/state",
            "command_topic": "homeassistant/switch/light1/set",
            "device": {
                "identifiers": ["light_1"],
                "name": "Living Room Light",
                "model": "Smart Bulb",
                "manufacturer": "Philips"
            }
        }"#;

        let msg: HassDiscoveryMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.unique_id, "switch_light_1");
        assert!(msg.device.is_some());
        let device = msg.device.unwrap();
        assert_eq!(device.identifiers, vec!["light_1"]);
        assert_eq!(device.name, Some("Living Room Light".to_string()));
        assert_eq!(device.manufacturer, Some("Philips".to_string()));
    }

    #[tokio::test]
    async fn test_device_tracking() {
        let config = HassAdapterConfig::new("test", "localhost");
        let adapter = HassAdapter::new(config);

        assert_eq!(adapter.device_count(), 0);
        assert!(adapter.list_devices().is_empty());
    }

    #[tokio::test]
    async fn test_handle_discovery_message() {
        let config = HassAdapterConfig::new("test", "localhost");
        let adapter = HassAdapter::new(config);

        let discovery_json = r#"{
            "unique_id": "sensor_temp_123",
            "name": "Living Room Temperature",
            "component": "sensor",
            "state_topic": "homeassistant/sensor/temp/state"
        }"#;

        adapter.handle_discovery_message(
            "homeassistant/sensor/temp/config".to_string(),
            discovery_json.as_bytes(),
        );

        assert_eq!(adapter.device_count(), 1);
        let devices = adapter.list_devices();
        assert_eq!(devices[0], "sensor_temp_123");
    }
}
