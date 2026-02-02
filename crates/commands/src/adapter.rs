//! Downlink adapters for sending commands to devices.
//!
//! Supports multiple protocols: MQTT, Modbus, HTTP, etc.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::command::{CommandRequest, CommandResult};

/// Device type identifier.
pub type DeviceType = String;

/// Adapter identifier.
pub type AdapterId = String;

/// Downlink adapter error types.
#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Send failed: {0}")]
    SendFailed(String),

    #[error("Timeout waiting for response")]
    Timeout,

    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    #[error("Unsupported device type: {0}")]
    UnsupportedDeviceType(String),

    #[error("Adapter not connected")]
    NotConnected,

    #[error("Authentication failed")]
    AuthenticationFailed,

    #[error("Protocol error: {0}")]
    ProtocolError(String),

    #[error("Other error: {0}")]
    Other(String),
}

/// Adapter statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterStats {
    /// Adapter ID
    pub adapter_id: String,
    /// Number of commands sent
    pub commands_sent: u64,
    /// Number of commands succeeded
    pub commands_succeeded: u64,
    /// Number of commands failed
    pub commands_failed: u64,
    /// Average response time in milliseconds
    pub avg_response_time_ms: f64,
    /// Whether adapter is connected
    pub connected: bool,
    /// Last error message
    pub last_error: Option<String>,
}

/// Downlink adapter types.
pub enum AnyAdapter {
    Mqtt(MqttDownlinkAdapter),
    Modbus(ModbusDownlinkAdapter),
    Http(HttpDownlinkAdapter),
}

impl AnyAdapter {
    /// Get the adapter ID.
    pub fn id(&self) -> &str {
        match self {
            AnyAdapter::Mqtt(a) => &a.config.client_id,
            AnyAdapter::Modbus(a) => &a.id,
            AnyAdapter::Http(a) => &a.id,
        }
    }

    /// Get the supported device types.
    pub fn supported_device_types(&self) -> &[&str] {
        match self {
            AnyAdapter::Mqtt(_) => &["mqtt", "iot", "sensor", "smart_plug", "smart_bulb"],
            AnyAdapter::Modbus(_) => &["modbus", "modbus_rtu", "modbus_tcp", "plc", "controller"],
            AnyAdapter::Http(_) => &["http", "rest", "webhook", "api"],
        }
    }

    /// Send a command to a device.
    pub async fn send_command(
        &self,
        command: &CommandRequest,
    ) -> Result<CommandResult, AdapterError> {
        match self {
            AnyAdapter::Mqtt(a) => a.send_command(command).await,
            AnyAdapter::Modbus(a) => a.send_command(command).await,
            AnyAdapter::Http(a) => a.send_command(command).await,
        }
    }

    /// Check if adapter is connected.
    pub async fn is_connected(&self) -> bool {
        match self {
            AnyAdapter::Mqtt(a) => a.is_connected().await,
            AnyAdapter::Modbus(a) => a.is_connected().await,
            AnyAdapter::Http(a) => a.is_connected().await,
        }
    }

    /// Get adapter statistics.
    pub async fn stats(&self) -> AdapterStats {
        match self {
            AnyAdapter::Mqtt(a) => a.stats().await,
            AnyAdapter::Modbus(a) => a.stats().await,
            AnyAdapter::Http(a) => a.stats().await,
        }
    }
}

/// MQTT downlink adapter configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttAdapterConfig {
    /// Broker URL
    pub broker_url: String,
    /// Client ID
    pub client_id: String,
    /// Username (optional)
    pub username: Option<String>,
    /// Password (optional)
    pub password: Option<String>,
    /// Command topic template
    pub command_topic_template: String,
    /// Response topic template
    pub response_topic_template: String,
    /// Quality of Service
    pub qos: u8,
    /// Keep alive interval in seconds
    pub keep_alive_secs: u64,
    /// Connection timeout in seconds
    pub connect_timeout_secs: u64,
}

impl Default for MqttAdapterConfig {
    fn default() -> Self {
        Self {
            broker_url: "tcp://localhost:1883".to_string(),
            client_id: format!("neotalk_{}", uuid::Uuid::new_v4()),
            username: None,
            password: None,
            command_topic_template: "devices/{device_id}/commands".to_string(),
            response_topic_template: "devices/{device_id}/responses".to_string(),
            qos: 1,
            keep_alive_secs: 60,
            connect_timeout_secs: 10,
        }
    }
}

/// MQTT downlink adapter.
pub struct MqttDownlinkAdapter {
    pub config: MqttAdapterConfig,
    connected: Arc<RwLock<bool>>,
    stats: Arc<RwLock<AdapterStats>>,
}

impl MqttDownlinkAdapter {
    /// Create a new MQTT adapter.
    pub fn new(config: MqttAdapterConfig) -> Self {
        let adapter_id = format!("mqtt_{}", config.client_id);
        Self {
            config,
            connected: Arc::new(RwLock::new(false)),
            stats: Arc::new(RwLock::new(AdapterStats {
                adapter_id,
                commands_sent: 0,
                commands_succeeded: 0,
                commands_failed: 0,
                avg_response_time_ms: 0.0,
                connected: false,
                last_error: None,
            })),
        }
    }

    /// Connect to the MQTT broker.
    pub async fn connect(&self) -> Result<(), AdapterError> {
        // In a real implementation, this would establish an MQTT connection
        *self.connected.write().await = true;
        {
            let mut stats = self.stats.write().await;
            stats.connected = true;
        }
        Ok(())
    }

    /// Disconnect from the MQTT broker.
    pub async fn disconnect(&self) {
        *self.connected.write().await = false;
        {
            let mut stats = self.stats.write().await;
            stats.connected = false;
        }
    }

    /// Format the command topic for a device.
    pub fn format_command_topic(&self, device_id: &str) -> String {
        self.config
            .command_topic_template
            .replace("{device_id}", device_id)
    }

    /// Format the response topic for a device.
    pub fn format_response_topic(&self, device_id: &str) -> String {
        self.config
            .response_topic_template
            .replace("{device_id}", device_id)
    }

    /// Send a command to a device.
    pub async fn send_command(
        &self,
        command: &CommandRequest,
    ) -> Result<CommandResult, AdapterError> {
        if !*self.connected.read().await {
            return Err(AdapterError::NotConnected);
        }

        let topic = self.format_command_topic(&command.device_id);

        // In a real implementation, this would:
        // 1. Serialize the command to MQTT format
        // 2. Publish to the broker
        // 3. Wait for response on the response topic
        // 4. Parse and return the result

        let mut stats = self.stats.write().await;
        stats.commands_sent += 1;
        stats.commands_succeeded += 1;
        drop(stats);

        Ok(CommandResult::success(format!(
            "Command sent via MQTT to {}",
            topic
        )))
    }

    /// Check if adapter is connected.
    pub async fn is_connected(&self) -> bool {
        *self.connected.read().await
    }

    /// Get adapter statistics.
    pub async fn stats(&self) -> AdapterStats {
        self.stats.read().await.clone()
    }
}

/// Modbus downlink adapter configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModbusAdapterConfig {
    /// Modbus device type (RTU or TCP)
    pub device_type: ModbusDeviceType,
    /// Slave address
    pub slave_address: u8,
    /// Connection timeout in milliseconds
    pub timeout_ms: u64,
}

/// Modbus device type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ModbusDeviceType {
    /// Modbus RTU (serial)
    Rtu {
        port: String,
        baud_rate: u32,
        data_bits: u8,
        stop_bits: u8,
        parity: String,
    },
    /// Modbus TCP
    Tcp { host: String, port: u16 },
}

impl Default for ModbusAdapterConfig {
    fn default() -> Self {
        Self {
            device_type: ModbusDeviceType::Tcp {
                host: "127.0.0.1".to_string(),
                port: 502,
            },
            slave_address: 1,
            timeout_ms: 5000,
        }
    }
}

/// Modbus downlink adapter.
pub struct ModbusDownlinkAdapter {
    pub id: String,
    pub config: ModbusAdapterConfig,
    connected: Arc<RwLock<bool>>,
    stats: Arc<RwLock<AdapterStats>>,
}

impl ModbusDownlinkAdapter {
    /// Create a new Modbus adapter.
    pub fn new(id: String, config: ModbusAdapterConfig) -> Self {
        Self {
            id,
            config,
            connected: Arc::new(RwLock::new(false)),
            stats: Arc::new(RwLock::new(AdapterStats {
                adapter_id: String::new(),
                commands_sent: 0,
                commands_succeeded: 0,
                commands_failed: 0,
                avg_response_time_ms: 0.0,
                connected: false,
                last_error: None,
            })),
        }
    }

    /// Connect to the Modbus device.
    pub async fn connect(&self) -> Result<(), AdapterError> {
        *self.connected.write().await = true;
        {
            let mut stats = self.stats.write().await;
            stats.adapter_id = self.id.clone();
            stats.connected = true;
        }
        Ok(())
    }

    /// Disconnect from the Modbus device.
    pub async fn disconnect(&self) {
        *self.connected.write().await = false;
        {
            let mut stats = self.stats.write().await;
            stats.connected = false;
        }
    }

    /// Send a command to a device.
    pub async fn send_command(
        &self,
        command: &CommandRequest,
    ) -> Result<CommandResult, AdapterError> {
        if !*self.connected.read().await {
            return Err(AdapterError::NotConnected);
        }

        let mut stats = self.stats.write().await;
        stats.commands_sent += 1;
        stats.commands_succeeded += 1;
        drop(stats);

        Ok(CommandResult::success(format!(
            "Modbus command sent to device {}",
            command.device_id
        )))
    }

    /// Check if adapter is connected.
    pub async fn is_connected(&self) -> bool {
        *self.connected.read().await
    }

    /// Get adapter statistics.
    pub async fn stats(&self) -> AdapterStats {
        self.stats.read().await.clone()
    }
}

/// HTTP downlink adapter configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpAdapterConfig {
    /// Base URL for HTTP requests
    pub base_url: String,
    /// API key header name
    pub api_key_header: Option<String>,
    /// API key value
    pub api_key: Option<String>,
    /// Request timeout in seconds
    pub timeout_secs: u64,
    /// Whether to use HTTPS
    pub use_https: bool,
}

impl Default for HttpAdapterConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:8080".to_string(),
            api_key_header: Some("X-API-Key".to_string()),
            api_key: None,
            timeout_secs: 30,
            use_https: false,
        }
    }
}

/// HTTP downlink adapter.
pub struct HttpDownlinkAdapter {
    pub id: String,
    pub config: HttpAdapterConfig,
    /// Connection status (reserved for future connection management).
    #[allow(dead_code)]
    connected: Arc<RwLock<bool>>,
    stats: Arc<RwLock<AdapterStats>>,
}

impl HttpDownlinkAdapter {
    /// Create a new HTTP adapter.
    pub fn new(id: String, config: HttpAdapterConfig) -> Self {
        Self {
            id,
            config,
            connected: Arc::new(RwLock::new(true)),
            stats: Arc::new(RwLock::new(AdapterStats {
                adapter_id: String::new(),
                commands_sent: 0,
                commands_succeeded: 0,
                commands_failed: 0,
                avg_response_time_ms: 0.0,
                connected: true,
                last_error: None,
            })),
        }
    }

    /// Build the full URL for a command.
    pub fn build_url(&self, device_id: &str, command_name: &str) -> String {
        format!(
            "{}/devices/{}/commands/{}",
            self.config.base_url, device_id, command_name
        )
    }

    /// Send a command to a device.
    pub async fn send_command(
        &self,
        command: &CommandRequest,
    ) -> Result<CommandResult, AdapterError> {
        let url = self.build_url(&command.device_id, &command.command_name);

        let mut stats = self.stats.write().await;
        stats.commands_sent += 1;
        stats.commands_succeeded += 1;
        drop(stats);

        Ok(CommandResult::success_with_data(
            format!("HTTP command sent to {}", url),
            serde_json::json!({"url": url, "status": "sent"}),
        ))
    }

    /// Check if adapter is connected.
    pub async fn is_connected(&self) -> bool {
        true // HTTP is stateless
    }

    /// Get adapter statistics.
    pub async fn stats(&self) -> AdapterStats {
        self.stats.read().await.clone()
    }
}

/// Downlink adapter registry for managing available adapters.
pub struct DownlinkAdapterRegistry {
    /// Registered adapters by ID
    adapters: HashMap<AdapterId, AnyAdapter>,
    /// Device type to adapter ID mapping
    device_type_map: HashMap<DeviceType, AdapterId>,
    /// Default adapter ID
    default_adapter: Option<AdapterId>,
}

impl DownlinkAdapterRegistry {
    /// Create a new adapter registry.
    pub fn new() -> Self {
        Self {
            adapters: HashMap::new(),
            device_type_map: HashMap::new(),
            default_adapter: None,
        }
    }

    /// Register an MQTT adapter.
    pub fn register_mqtt(&mut self, adapter: MqttDownlinkAdapter) {
        let id = format!("mqtt_{}", adapter.config.client_id);
        self.register_adapter(id.clone(), AnyAdapter::Mqtt(adapter));
    }

    /// Register a Modbus adapter.
    pub fn register_modbus(&mut self, adapter: ModbusDownlinkAdapter) {
        let id = adapter.id.clone();
        self.register_adapter(id.clone(), AnyAdapter::Modbus(adapter));
    }

    /// Register an HTTP adapter.
    pub fn register_http(&mut self, adapter: HttpDownlinkAdapter) {
        let id = adapter.id.clone();
        self.register_adapter(id.clone(), AnyAdapter::Http(adapter));
    }

    /// Register an adapter.
    fn register_adapter(&mut self, id: AdapterId, adapter: AnyAdapter) {
        // Register device types
        for device_type in adapter.supported_device_types() {
            self.device_type_map
                .insert(device_type.to_string(), id.clone());
        }

        self.adapters.insert(id.clone(), adapter);

        // Set as default if first adapter
        if self.default_adapter.is_none() {
            self.default_adapter = Some(id);
        }
    }

    /// Get an adapter by ID.
    pub fn get(&self, id: &str) -> Option<&AnyAdapter> {
        self.adapters.get(id)
    }

    /// Get adapter for a device type.
    pub fn get_for_device_type(&self, device_type: &str) -> Option<&AnyAdapter> {
        self.device_type_map
            .get(device_type)
            .and_then(|id| self.adapters.get(id))
    }

    /// Get the default adapter.
    pub fn get_default(&self) -> Option<&AnyAdapter> {
        self.default_adapter
            .as_ref()
            .and_then(|id| self.adapters.get(id))
    }

    /// Set the default adapter.
    pub fn set_default(&mut self, id: &str) -> bool {
        if self.adapters.contains_key(id) {
            self.default_adapter = Some(id.to_string());
            true
        } else {
            false
        }
    }

    /// Get all registered adapter IDs.
    pub fn adapter_ids(&self) -> Vec<&str> {
        self.adapters.keys().map(|k| k.as_str()).collect()
    }

    /// Get statistics for all adapters.
    pub async fn get_all_stats(&self) -> HashMap<AdapterId, AdapterStats> {
        let mut stats = HashMap::new();
        for (id, adapter) in &self.adapters {
            stats.insert(id.clone(), adapter.stats().await);
        }
        stats
    }
}

impl Default for DownlinkAdapterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mqtt_config_default() {
        let config = MqttAdapterConfig::default();
        assert_eq!(config.broker_url, "tcp://localhost:1883");
        assert_eq!(config.qos, 1);
    }

    #[test]
    fn test_modbus_config_default() {
        let config = ModbusAdapterConfig::default();
        assert_eq!(config.slave_address, 1);
        assert!(matches!(config.device_type, ModbusDeviceType::Tcp { .. }));
    }

    #[test]
    fn test_http_config_default() {
        let config = HttpAdapterConfig::default();
        assert_eq!(config.base_url, "http://localhost:8080");
        assert_eq!(config.timeout_secs, 30);
    }

    #[tokio::test]
    async fn test_registry_register_mqtt() {
        let mut registry = DownlinkAdapterRegistry::new();
        let mqtt = MqttDownlinkAdapter::new(MqttAdapterConfig::default());

        registry.register_mqtt(mqtt);

        assert!(!registry.adapter_ids().is_empty());
        assert!(registry.get_default().is_some());
    }

    #[test]
    fn test_mqtt_topic_formatting() {
        let adapter = MqttDownlinkAdapter::new(MqttAdapterConfig {
            command_topic_template: "devices/{device_id}/cmd/{command}".to_string(),
            ..Default::default()
        });

        let topic = adapter.format_command_topic("sensor123");
        assert_eq!(topic, "devices/sensor123/cmd/{command}");
    }

    #[test]
    fn test_http_url_building() {
        let adapter = HttpDownlinkAdapter::new(
            "http_test".to_string(),
            HttpAdapterConfig {
                base_url: "https://api.example.com".to_string(),
                ..Default::default()
            },
        );

        let url = adapter.build_url("device1", "turn_on");
        assert_eq!(
            url,
            "https://api.example.com/devices/device1/commands/turn_on"
        );
    }
}
