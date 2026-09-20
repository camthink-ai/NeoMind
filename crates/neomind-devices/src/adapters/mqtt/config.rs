//! `config` — split from the former mqtt.rs monolith.

use crate::mqtt::MqttConfig;

/// MQTT device adapter configuration.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct MqttAdapterConfig {
    /// Adapter name
    pub name: String,
    /// MQTT broker configuration
    pub mqtt: MqttConfig,
    /// Topic patterns to subscribe to (e.g., ["sensors/+/temperature", "sensors/+/humidity"])
    pub subscribe_topics: Vec<String>,
    /// Topic pattern for device discovery (e.g., "devices/+/discovery")
    pub discovery_topic: Option<String>,
    /// Discovery prefix for auto-discovery
    pub discovery_prefix: String,
    /// Enable auto-discovery
    pub auto_discovery: bool,
    /// Payload field to use as the device identity when the topic cannot
    /// uniquely identify a device (e.g. a gateway forwarding many devices on
    /// one topic). `None`/empty → auto-detect common fields (device_id, sn,
    /// mac, ...). Explicit value wins over auto-detection.
    #[serde(default)]
    pub device_id_field: Option<String>,
    /// Storage directory for persistence
    pub storage_dir: Option<String>,
}

impl MqttAdapterConfig {
    /// Create a new MQTT adapter configuration.
    pub fn new(name: impl Into<String>, broker: impl Into<String>) -> Self {
        let mqtt_config = MqttConfig::new(broker, "neomind");
        Self {
            name: name.into(),
            mqtt: mqtt_config,
            subscribe_topics: Vec::new(),
            discovery_topic: None,
            discovery_prefix: "neomind".to_string(),
            auto_discovery: true,
            device_id_field: None,
            storage_dir: None,
        }
    }

    /// Add a subscription topic pattern.
    pub fn with_topic(mut self, topic: impl Into<String>) -> Self {
        self.subscribe_topics.push(topic.into());
        self
    }

    /// Add multiple subscription topics.
    pub fn with_topics(mut self, topics: Vec<String>) -> Self {
        self.subscribe_topics = topics;
        self
    }

    /// Set the discovery topic.
    pub fn with_discovery(mut self, topic: impl Into<String>) -> Self {
        self.discovery_topic = Some(topic.into());
        self
    }

    /// Set the discovery prefix.
    pub fn with_discovery_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.discovery_prefix = prefix.into();
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
}
