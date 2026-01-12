//! MQTT device adapter for NeoTalk event-driven architecture.
//!
//! This adapter connects to an MQTT broker, subscribes to device topics,
//! and publishes device events to the event bus.
//!
//! ## Protocol Mapping Integration
//!
//! The adapter can use a `ProtocolMapping` for flexible topic and payload handling:
//! ```text
//! Device Type Definition       MQTT Mapping
//! ├─ temperature capability  ──→ sensor/${id}/temperature
//! ├─ humidity capability     ──→ sensor/${id}/humidity
//! └─ set_interval command    ──→ sensor/${id}/command
//! ```

use crate::adapter::{DeviceAdapter, DeviceEvent, DiscoveredDeviceInfo, AdapterResult};
use crate::mqtt::MqttConfig;
use crate::protocol::{ProtocolMapping, Address};
use edge_ai_core::EventBus;
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use serde_json::Value;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

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
}

impl MqttAdapterConfig {
    /// Create a new MQTT adapter configuration.
    pub fn new(name: impl Into<String>, broker: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            mqtt: MqttConfig::new(broker, "neotalk"),
            subscribe_topics: Vec::new(),
            discovery_topic: None,
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

/// MQTT device adapter.
///
/// Connects to an MQTT broker and publishes device events.
/// Can optionally use a ProtocolMapping for flexible topic and payload handling.
pub struct MqttAdapter {
    /// Adapter configuration
    config: MqttAdapterConfig,
    /// Event channel sender
    event_tx: broadcast::Sender<DeviceEvent>,
    /// Running state
    running: Arc<std::sync::atomic::AtomicBool>,
    /// Connected devices
    devices: Arc<tokio::sync::RwLock<Vec<String>>>,
    /// Optional protocol mapping for flexible topic/payload handling
    protocol_mapping: Option<Arc<dyn ProtocolMapping>>,
    /// Device ID to device type mapping (used with protocol mapping)
    device_types: Arc<tokio::sync::RwLock<HashMap<String, String>>>,
}

impl MqttAdapter {
    /// Create a new MQTT adapter.
    pub fn new(config: MqttAdapterConfig) -> Self {
        let (event_tx, _) = broadcast::channel(1000);
        Self {
            config,
            event_tx,
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            devices: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            protocol_mapping: None,
            device_types: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Create a new MQTT adapter with a protocol mapping.
    pub fn with_mapping(
        config: MqttAdapterConfig,
        mapping: Arc<dyn ProtocolMapping>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(1000);
        Self {
            config,
            event_tx,
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            devices: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            protocol_mapping: Some(mapping),
            device_types: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Register a device type with its ID (for protocol mapping).
    pub async fn register_device_type(&self, device_id: String, device_type: String) {
        let mut types = self.device_types.write().await;
        types.insert(device_id, device_type);
    }

    /// Set the protocol mapping.
    pub fn set_mapping(&mut self, mapping: Arc<dyn ProtocolMapping>) {
        self.protocol_mapping = Some(mapping);
    }

    /// Get the current protocol mapping.
    pub fn mapping(&self) -> Option<&Arc<dyn ProtocolMapping>> {
        self.protocol_mapping.as_ref()
    }

    /// Extract device ID from topic using pattern matching.
    fn extract_device_id(&self, topic: &str) -> Option<String> {
        // Try to match against subscription patterns
        for pattern in &self.config.subscribe_topics {
            if let Some(id) = Self::match_topic_pattern(topic, pattern) {
                return Some(id);
            }
        }

        // Fallback: extract from common patterns
        let parts: Vec<&str> = topic.split('/').collect();
        if parts.len() >= 2 {
            // Common pattern: prefix/{device_id}/...
            Some(parts[1].to_string())
        } else {
            None
        }
    }

    /// Match a topic against a pattern and extract device ID.
    fn match_topic_pattern(topic: &str, pattern: &str) -> Option<String> {
        // Convert MQTT wildcard patterns to regex
        // + matches one level, # matches multiple levels
        let pattern_parts: Vec<&str> = pattern.split('/').collect();
        let topic_parts: Vec<&str> = topic.split('/').collect();

        if pattern_parts.len() != topic_parts.len() {
            return None;
        }

        let mut device_id = None;
        let mut matches = true;

        for (i, (p, t)) in pattern_parts.iter().zip(topic_parts.iter()).enumerate() {
            match *p {
                "+" => {
                    // Single-level wildcard - this is the device ID
                    if i == 1 {
                        device_id = Some(t.to_string());
                    }
                }
                "#" => {
                    // Multi-level wildcard - not implemented at same level count
                }
                _ => {
                    if p != t {
                        matches = false;
                        break;
                    }
                }
            }
        }

        if matches {
            device_id
        } else {
            None
        }
    }

    /// Extract metric name from topic.
    fn extract_metric_name(&self, topic: &str) -> Option<String> {
        let parts: Vec<&str> = topic.split('/').collect();
        if parts.len() >= 3 {
            Some(parts[2].to_string())
        } else {
            topic.split('/').last().map(|s| s.to_string())
        }
    }

    /// Parse MQTT payload to MetricValue.
    /// Uses protocol mapping if available, otherwise falls back to default parsing.
    fn parse_payload(
        &self,
        device_id: &str,
        metric_name: &str,
        payload: &[u8],
    ) -> crate::mdl::MetricValue {
        // Try protocol mapping first
        if let Some(ref mapping) = self.protocol_mapping {
            if let Ok(value) = mapping.parse_metric(metric_name, payload) {
                return value;
            }
            // Fall through to default parsing on error
        }

        // Default payload parsing
        Self::default_parse_value(payload)
    }

    /// Default value parsing (when no protocol mapping is available).
    fn default_parse_value(payload: &[u8]) -> crate::mdl::MetricValue {
        // Try JSON first
        if let Ok(json) = serde_json::from_slice::<Value>(payload) {
            if let Some(num) = json.as_f64() {
                return crate::mdl::MetricValue::Float(num);
            } else if let Some(s) = json.as_str() {
                return crate::mdl::MetricValue::String(s.to_string());
            } else if let Some(b) = json.as_bool() {
                return crate::mdl::MetricValue::Boolean(b);
            }
        }

        // Try to parse as number
        let str_payload = String::from_utf8_lossy(payload);
        if let Ok(num) = str_payload.trim().parse::<f64>() {
            return crate::mdl::MetricValue::Float(num);
        }

        // Try boolean
        match str_payload.trim().to_lowercase().as_str() {
            "true" | "on" | "1" => return crate::mdl::MetricValue::Boolean(true),
            "false" | "off" | "0" => return crate::mdl::MetricValue::Boolean(false),
            _ => {}
        }

        // Default to string
        crate::mdl::MetricValue::String(str_payload.to_string())
    }

    /// Handle incoming MQTT message.
    fn handle_message(&self, topic: String, payload: Vec<u8>) {
        let device_id = match self.extract_device_id(&topic) {
            Some(id) => id,
            None => {
                warn!("Could not parse device ID from topic: {}", topic);
                return;
            }
        };

        // Check for discovery messages
        if self.config.discovery_topic.as_ref().map_or(false, |dt| topic.contains(dt)) {
            if let Ok(info) = serde_json::from_slice::<Value>(&payload) {
                let device_info = DiscoveredDeviceInfo::new(&device_id, info.get("type")
                    .and_then(|v| v.as_str()).unwrap_or("unknown"))
                    .with_name(info.get("name").and_then(|v| v.as_str()).unwrap_or(&device_id))
                    .with_endpoint(&topic);

                // If protocol mapping is available, try to get device type from it
                if let Some(ref mapping) = self.protocol_mapping {
                    if let Some(dt) = info.get("device_type").and_then(|v| v.as_str()) {
                        if dt == mapping.device_type() {
                            // Register this device with its type
                            let device_id_clone = device_id.clone();
                            let dt_owned = dt.to_string(); // Clone to own the data
                            tokio::spawn(async move {
                                // In a real implementation, this would persist the device type
                                debug!("Discovered device {} of type {}", device_id_clone, dt_owned);
                            });
                        }
                    }
                }

                let _ = self.event_tx.send(DeviceEvent::Discovery { device: device_info });
                return;
            }
        }

        // Parse as metric event
        let metric_name = match self.extract_metric_name(&topic) {
            Some(name) => name,
            None => "value".to_string(),
        };

        let value = self.parse_payload(&device_id, &metric_name, &payload);

        let event = DeviceEvent::Metric {
            device_id,
            metric: metric_name,
            value,
            timestamp: chrono::Utc::now().timestamp(),
        };

        let _ = self.event_tx.send(event);
    }

    /// Send a command to a device using protocol mapping.
    pub async fn send_command(
        &self,
        device_id: &str,
        command: &str,
        params: &HashMap<String, crate::mdl::MetricValue>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(ref mapping) = self.protocol_mapping {
            // Serialize command using protocol mapping
            let payload = mapping.serialize_command(command, params)?;
            let address = mapping.command_address(command)
                .ok_or("Command not found in mapping")?;

            if let Address::MQTT { topic, .. } = address {
                let full_topic = topic.replace("${device_id}", device_id);
                info!("Sending MQTT command to {}: {}", full_topic,
                    String::from_utf8_lossy(&payload));
                // In a real implementation, this would publish to MQTT
                return Ok(());
            }

            Err("Command address is not MQTT type".into())
        } else {
            // Fallback: construct topic from device_id and command
            let topic = format!("{}/command/{}", device_id, command);
            info!("Sending MQTT command to {} (no mapping)", topic);
            // In a real implementation, this would publish to MQTT
            Ok(())
        }
    }
}

#[async_trait]
impl DeviceAdapter for MqttAdapter {
    fn name(&self) -> &str {
        &self.config.name
    }

    fn adapter_type(&self) -> &'static str {
        "mqtt"
    }

    fn is_running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::Relaxed)
    }

    async fn start(&self) -> AdapterResult<()> {
        if self.is_running() {
            return Ok(());
        }

        info!("Starting MQTT adapter: {}", self.config.name);

        // In a real implementation, this would connect to the MQTT broker
        // For now, we simulate the adapter running
        self.running.store(true, std::sync::atomic::Ordering::Relaxed);

        // Spawn a task to simulate incoming messages
        // In production, this would be the MQTT client event loop
        let running = self.running.clone();
        let _event_tx = self.event_tx.clone();
        let name = self.config.name.clone();

        tokio::spawn(async move {
            while running.load(std::sync::atomic::Ordering::Relaxed) {
                // Simulate message processing
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            debug!("MQTT adapter '{}' stopped", name);
        });

        info!("MQTT adapter '{}' started", self.config.name);
        Ok(())
    }

    async fn stop(&self) -> AdapterResult<()> {
        info!("Stopping MQTT adapter: {}", self.config.name);
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

/// Create an MQTT adapter connected to an event bus.
pub fn create_mqtt_adapter(
    config: MqttAdapterConfig,
    event_bus: &EventBus,
) -> Arc<MqttAdapter> {
    let adapter = Arc::new(MqttAdapter::new(config));
    let adapter_clone = adapter.clone();
    let event_bus = event_bus.clone();

    // Spawn event forwarding task
    tokio::spawn(async move {
        let mut rx = adapter_clone.subscribe();
        while let Some(event) = rx.next().await {
            let device_id = event.device_id().unwrap_or("unknown").to_string();
            let neotalk_event = event.to_neotalk_event();
            let source = format!("adapter:mqtt:{}", device_id);
            event_bus.publish_with_source(neotalk_event, source).await;
        }
    });

    adapter
}

/// Create an MQTT adapter with protocol mapping.
pub fn create_mqtt_adapter_with_mapping(
    config: MqttAdapterConfig,
    mapping: Arc<dyn ProtocolMapping>,
    event_bus: &EventBus,
) -> Arc<MqttAdapter> {
    let adapter = Arc::new(MqttAdapter::with_mapping(config, mapping));
    let adapter_clone = adapter.clone();
    let event_bus = event_bus.clone();

    // Spawn event forwarding task
    tokio::spawn(async move {
        let mut rx = adapter_clone.subscribe();
        while let Some(event) = rx.next().await {
            let device_id = event.device_id().unwrap_or("unknown").to_string();
            let neotalk_event = event.to_neotalk_event();
            let source = format!("adapter:mqtt:{}", device_id);
            event_bus.publish_with_source(neotalk_event, source).await;
        }
    });

    adapter
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_topic_pattern_single_wildcard() {
        assert_eq!(
            MqttAdapter::match_topic_pattern("sensor/temp01/temperature", "sensor/+/temperature"),
            Some("temp01".to_string())
        );
        assert_eq!(
            MqttAdapter::match_topic_pattern("sensor/temp02/humidity", "sensor/+/temperature"),
            None
        );
    }

    #[test]
    fn test_extract_device_id() {
        let config = MqttAdapterConfig::new("test", "localhost")
            .with_topic("sensor/+/temperature");
        let adapter = MqttAdapter::new(config);

        assert_eq!(
            adapter.extract_device_id("sensor/temp01/temperature"),
            Some("temp01".to_string())
        );
    }

    #[test]
    fn test_extract_metric_name() {
        let config = MqttAdapterConfig::new("test", "localhost");
        let adapter = MqttAdapter::new(config);

        assert_eq!(
            adapter.extract_metric_name("sensor/temp01/temperature"),
            Some("temperature".to_string())
        );
    }

    #[test]
    fn test_default_parse_value_float() {
        let payload = b"23.5";
        let result = MqttAdapter::default_parse_value(payload);
        assert!(matches!(result, crate::mdl::MetricValue::Float(23.5)));
    }

    #[test]
    fn test_default_parse_value_json() {
        let payload = br#"{"value": 42.0}"#;
        let result = MqttAdapter::default_parse_value(payload);
        assert!(matches!(result, crate::mdl::MetricValue::Float(42.0)));
    }

    #[test]
    fn test_default_parse_value_boolean() {
        let payload = b"true";
        let result = MqttAdapter::default_parse_value(payload);
        assert!(matches!(result, crate::mdl::MetricValue::Boolean(true)));
    }
}
