//! MQTT device adapter.
//!
//! Implements the Device trait for devices communicating via MQTT protocol.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::mdl::{
    Device, Command, ConnectionStatus, DeviceCapability, DeviceError, DeviceId,
    DeviceInfo, DeviceState, DeviceType, MetricDefinition, MetricDataType, MetricValue,
};

/// Configuration for an MQTT device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttConfig {
    /// MQTT broker address
    pub broker: String,

    /// MQTT broker port
    pub port: u16,

    /// Client ID (auto-generated if not provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Username for authentication
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    /// Password for authentication
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,

    /// Keep-alive interval in seconds
    #[serde(default = "default_keep_alive")]
    pub keep_alive: u64,

    /// Clean session flag
    #[serde(default = "default_clean_session")]
    pub clean_session: bool,

    /// QoS level for subscriptions
    #[serde(default = "default_qos")]
    pub qos: u8,

    /// Topic prefix for this device's metrics
    pub topic_prefix: String,

    /// Command topic (device receives commands here)
    pub command_topic: String,
}

fn default_keep_alive() -> u64 { 60 }
fn default_clean_session() -> bool { true }
fn default_qos() -> u8 { 1 }

impl MqttConfig {
    pub fn new(broker: impl Into<String>, topic_prefix: impl Into<String>) -> Self {
        Self {
            broker: broker.into(),
            port: 1883,
            client_id: None,
            username: None,
            password: None,
            keep_alive: 60,
            clean_session: true,
            qos: 1,
            topic_prefix: topic_prefix.into(),
            command_topic: "command".to_string(),
        }
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn with_auth(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        self.username = Some(username.into());
        self.password = Some(password.into());
        self
    }

    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = Some(client_id.into());
        self
    }

    pub fn full_broker_addr(&self) -> String {
        format!("{}:{}", self.broker, self.port)
    }
}

/// MQTT message received from the broker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttMessage {
    pub topic: String,
    pub payload: Vec<u8>,
    pub qos: u8,
    pub retain: bool,
    pub timestamp: i64,
}

/// Device that communicates via MQTT.
pub struct MqttDevice {
    /// Unique device identifier
    id: DeviceId,
    /// Human-readable name
    name: String,
    /// MQTT configuration
    config: MqttConfig,
    /// Metric definitions this device provides
    metrics: HashMap<String, MetricDefinition>,
    /// Cached metric values (last received)
    cached_values: Arc<RwLock<HashMap<String, MetricValue>>>,
    /// Device state
    state: Arc<RwLock<DeviceState>>,
    /// Available commands
    available_commands: Vec<String>,
    /// Device location (optional)
    location: Option<String>,
}

impl MqttDevice {
    /// Create a new MQTT device.
    pub fn new(
        name: impl Into<String>,
        config: MqttConfig,
        metrics: Vec<MetricDefinition>,
    ) -> Self {
        let id = DeviceId::new();
        let name = name.into();

        let metrics_map = metrics
            .into_iter()
            .map(|m| (m.name.clone(), m))
            .collect();

        let state = DeviceState {
            status: ConnectionStatus::Disconnected,
            last_seen: None,
            error: None,
        };

        Self {
            id,
            name,
            config,
            metrics: metrics_map,
            cached_values: Arc::new(RwLock::new(HashMap::new())),
            state: Arc::new(RwLock::new(state)),
            available_commands: Vec::new(),
            location: None,
        }
    }

    /// Create a sensor device (read-only).
    pub fn sensor(
        name: impl Into<String>,
        broker: impl Into<String>,
        topic_prefix: impl Into<String>,
        metrics: Vec<MetricDefinition>,
    ) -> Self {
        let config = MqttConfig::new(broker, topic_prefix);
        Self::new(name, config, metrics)
    }

    /// Create an actuator device (can receive commands).
    pub fn actuator(
        name: impl Into<String>,
        broker: impl Into<String>,
        topic_prefix: impl Into<String>,
        metrics: Vec<MetricDefinition>,
        commands: Vec<String>,
    ) -> Self {
        let config = MqttConfig::new(broker, topic_prefix);
        let mut device = Self::new(name, config, metrics);
        device.available_commands = commands;
        device
    }

    /// Set the device location.
    pub fn with_location(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }

    /// Add an available command.
    pub fn add_command(mut self, command: impl Into<String>) -> Self {
        self.available_commands.push(command.into());
        self
    }

    /// Get the MQTT topic for a specific metric.
    pub fn metric_topic(&self, metric_name: &str) -> String {
        format!("{}/{}", self.config.topic_prefix, metric_name)
    }

    /// Get the MQTT topic for commands.
    pub fn command_topic(&self) -> String {
        format!("{}/{}", self.config.topic_prefix, self.config.command_topic)
    }

    /// Process an incoming MQTT message and update cached values.
    pub async fn process_message(&self, topic: &str, payload: &[u8]) -> Result<(), DeviceError> {
        // Extract metric name from topic
        let metric_name = topic
            .strip_prefix(&format!("{}/", self.config.topic_prefix))
            .or_else(|| topic.strip_prefix(&self.config.topic_prefix))
            .unwrap_or(topic);

        // Find metric definition
        if let Some(metric_def) = self.metrics.get(metric_name) {
            let value = match metric_def.data_type {
                MetricDataType::Integer => {
                    let s = std::str::from_utf8(payload)
                        .map_err(|e| DeviceError::Serialization(format!("UTF-8 error: {}", e)))?;
                    MetricValue::Integer(s.parse().unwrap_or(0))
                }
                MetricDataType::Float => {
                    let s = std::str::from_utf8(payload)
                        .map_err(|e| DeviceError::Serialization(format!("UTF-8 error: {}", e)))?;
                    MetricValue::Float(s.parse().unwrap_or(0.0))
                }
                MetricDataType::Boolean => {
                    let s = std::str::from_utf8(payload)
                        .map_err(|e| DeviceError::Serialization(format!("UTF-8 error: {}", e)))?;
                    MetricValue::Boolean(s.eq_ignore_ascii_case("true") || s == "1")
                }
                MetricDataType::String => {
                    MetricValue::String(String::from_utf8(payload.to_vec())
                        .unwrap_or_else(|_| String::from_utf8_lossy(payload).to_string()))
                }
                MetricDataType::Binary => MetricValue::Binary(payload.to_vec()),
            };

            let mut values = self.cached_values.write().await;
            values.insert(metric_name.to_string(), value);

            // Update last seen
            let mut state = self.state.write().await;
            state.last_seen = Some(chrono::Utc::now());

            Ok(())
        } else {
            Err(DeviceError::InvalidMetric(format!("Unknown metric: {}", metric_name)))
        }
    }

    /// Publish a command to the device.
    pub async fn publish_command(&self, command: &Command) -> Result<Vec<u8>, DeviceError> {
        // This would publish to MQTT broker
        // For now, we simulate by creating the payload
        let payload = serde_json::to_vec(command)
            .map_err(|e| DeviceError::Serialization(e.to_string()))?;

        // Update last seen
        let mut state = self.state.write().await;
        state.last_seen = Some(chrono::Utc::now());

        Ok(payload)
    }
}

#[async_trait]
impl Device for MqttDevice {
    fn id(&self) -> &DeviceId {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn device_type(&self) -> DeviceType {
        if self.available_commands.is_empty() {
            DeviceType::Sensor
        } else if !self.metrics.is_empty() {
            DeviceType::Hybrid
        } else {
            DeviceType::Actuator
        }
    }

    fn capabilities(&self) -> Vec<DeviceCapability> {
        let mut caps = vec![DeviceCapability::ReadNumeric];
        if !self.available_commands.is_empty() {
            caps.push(DeviceCapability::ExecuteCommand);
        }
        caps.push(DeviceCapability::StreamData);
        caps
    }

    fn metrics(&self) -> Vec<MetricDefinition> {
        self.metrics.values().cloned().collect()
    }

    fn commands(&self) -> Vec<String> {
        self.available_commands.clone()
    }

    async fn read_metric(&self, metric: &str) -> Result<MetricValue, DeviceError> {
        if !self.metrics.contains_key(metric) {
            return Err(DeviceError::InvalidMetric(format!("Unknown metric: {}", metric)));
        }

        let values = self.cached_values.read().await;
        values
            .get(metric)
            .cloned()
            .ok_or_else(|| DeviceError::Communication(format!("No value for metric: {}", metric)))
    }

    async fn write_command(&self, command: &Command) -> Result<Option<MetricValue>, DeviceError> {
        if !self.available_commands.contains(&command.name) {
            return Err(DeviceError::InvalidCommand(format!("Unknown command: {}", command.name)));
        }

        let _payload = self.publish_command(command).await?;
        Ok(None)
    }

    async fn connect(&mut self) -> Result<(), DeviceError> {
        let mut state = self.state.write().await;
        state.status = ConnectionStatus::Connecting;
        drop(state);

        // Simulate connection (in real implementation, would connect to MQTT broker)
        tokio::time::sleep(Duration::from_millis(100)).await;

        let mut state = self.state.write().await;
        state.status = ConnectionStatus::Connected;
        state.last_seen = Some(chrono::Utc::now());
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), DeviceError> {
        let mut state = self.state.write().await;
        state.status = ConnectionStatus::Disconnected;
        state.last_seen = Some(chrono::Utc::now());
        Ok(())
    }

    async fn is_connected(&self) -> bool {
        let state = self.state.read().await;
        matches!(state.status, ConnectionStatus::Connected)
    }

    async fn state(&self) -> DeviceState {
        let state = self.state.read().await;
        state.clone()
    }

    fn info(&self) -> DeviceInfo {
        // Use try_read to avoid blocking; fall back to default state if locked
        let current_state = self.state.try_read()
            .ok()
            .map(|s| s.clone())
            .unwrap_or_else(|| DeviceState {
                status: ConnectionStatus::Disconnected,
                last_seen: None,
                error: None,
            });

        DeviceInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            device_type: self.device_type(),
            protocol: "mqtt".to_string(),
            status: current_state.status,
            capabilities: self.capabilities(),
            metrics: self.metrics(),
            commands: self.available_commands.clone(),
            location: self.location.clone(),
            metadata: {
                let mut meta = HashMap::new();
                meta.insert("broker".to_string(), self.config.broker.clone());
                meta.insert("port".to_string(), self.config.port.to_string());
                meta.insert("topic_prefix".to_string(), self.config.topic_prefix.clone());
                meta
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mqtt_config() {
        let config = MqttConfig::new("localhost", "sensors/temp")
            .with_port(1883)
            .with_auth("user", "pass");

        assert_eq!(config.broker, "localhost");
        assert_eq!(config.port, 1883);
        assert_eq!(config.topic_prefix, "sensors/temp");
        assert_eq!(config.username, Some("user".to_string()));
        assert_eq!(config.password, Some("pass".to_string()));
    }

    #[tokio::test]
    async fn test_mqtt_sensor() {
        let metrics = vec![
            MetricDefinition {
                name: "temperature".to_string(),
                description: "Temperature in Celsius".to_string(),
                data_type: MetricDataType::Float,
                unit: Some("°C".to_string()),
                read_only: true,
                min: Some(-40.0),
                max: Some(100.0),
            },
        ];

        let device = MqttDevice::sensor("TempSensor1", "localhost", "sensors/temp1", metrics);

        assert_eq!(device.name(), "TempSensor1");
        assert_eq!(device.device_type(), DeviceType::Sensor);

        // Process a message
        device.process_message("sensors/temp1/temperature", b"23.5").await.unwrap();

        // Read the cached value
        let value = device.read_metric("temperature").await.unwrap();
        assert_eq!(value, MetricValue::Float(23.5));
    }

    #[tokio::test]
    async fn test_mqtt_connect_disconnect() {
        let metrics = vec![];
        let mut device = MqttDevice::sensor("TestDevice", "localhost", "test", metrics);

        assert!(!device.is_connected().await);

        device.connect().await.unwrap();
        assert!(device.is_connected().await);

        device.disconnect().await.unwrap();
        assert!(!device.is_connected().await);
    }
}
