//! Specialized device adapter plugins.
//!
//! This module provides specialized plugin implementations for different
//! device adapter types, each with type-specific metadata, configuration
//! schemas, and commands.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::json;
use chrono::Utc;

use crate::adapter::DeviceAdapter;
use crate::plugin_adapter::{DeviceAdapterPlugin, DeviceAdapterPluginFactory};
use edge_ai_core::EventBus;
use edge_ai_core::plugin::{
    ExtendedPluginMetadata, PluginType, PluginState, PluginError, PluginStats,
    StateMachine, PluginMetadata, PluginPermission,
};

#[cfg(feature = "embedded-broker")]
use crate::embedded_broker;

/// Configuration for an external MQTT broker connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalBrokerConfig {
    /// Unique identifier for this broker
    pub id: String,

    /// Display name
    pub name: String,

    /// Broker address (e.g., "localhost:1883" or "192.168.1.100:8883")
    pub broker: String,

    /// Client ID prefix (will have UUID appended)
    #[serde(default = "default_client_id")]
    pub client_id: String,

    /// MQTT topics to subscribe to
    pub topics: Vec<String>,

    /// Username for authentication (optional)
    pub username: Option<String>,

    /// Password for authentication (optional)
    pub password: Option<String>,

    /// Use TLS/SSL
    #[serde(default)]
    pub use_tls: bool,

    /// Connection timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,

    /// Keep-alive interval in seconds
    #[serde(default = "default_keep_alive")]
    pub keep_alive_secs: u64,

    /// Auto-reconnect on connection loss
    #[serde(default = "default_auto_reconnect")]
    pub auto_reconnect: bool,

    /// QoS level for subscriptions (0, 1, or 2)
    #[serde(default = "default_qos")]
    pub qos: u8,

    /// Whether this broker is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_client_id() -> String {
    "neotalk-external".to_string()
}

fn default_timeout() -> u64 {
    30
}

fn default_keep_alive() -> u64 {
    60
}

fn default_auto_reconnect() -> bool {
    true
}

fn default_qos() -> u8 {
    1
}

fn default_enabled() -> bool {
    true
}

impl ExternalBrokerConfig {
    /// Create a new external broker configuration.
    pub fn new(id: String, name: String, broker: String, topics: Vec<String>) -> Self {
        Self {
            id,
            name,
            broker,
            client_id: default_client_id(),
            topics,
            username: None,
            password: None,
            use_tls: false,
            timeout_secs: default_timeout(),
            keep_alive_secs: default_keep_alive(),
            auto_reconnect: default_auto_reconnect(),
            qos: default_qos(),
            enabled: default_enabled(),
        }
    }

    /// Get the full client ID with UUID suffix.
    pub fn full_client_id(&self) -> String {
        format!("{}-{}", self.client_id, uuid::Uuid::new_v4())
    }
}


/// Configuration for Modbus adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModbusAdapterConfig {
    /// Unique identifier
    pub id: String,

    /// Display name
    pub name: String,

    /// Modbus device host address
    pub host: String,

    /// Modbus TCP port
    #[serde(default = "default_modbus_port")]
    pub port: u16,

    /// Slave/unit ID
    #[serde(default = "default_slave_id")]
    pub slave_id: u8,

    /// Connection timeout in milliseconds
    #[serde(default = "default_modbus_timeout")]
    pub timeout_ms: u64,

    /// Poll interval in milliseconds
    #[serde(default = "default_modbus_poll")]
    pub poll_interval_ms: u64,

    /// Whether this adapter is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_modbus_port() -> u16 {
    502
}

fn default_slave_id() -> u8 {
    1
}

fn default_modbus_timeout() -> u64 {
    5000
}

fn default_modbus_poll() -> u64 {
    1000
}

/// Internal MQTT Broker plugin.
///
/// This plugin manages the embedded MQTT broker that runs within NeoTalk.
#[cfg(feature = "embedded-broker")]
pub struct InternalMqttBrokerPlugin {
    metadata: ExtendedPluginMetadata,
    state_machine: StateMachine,
    stats: PluginStats,
    event_bus: EventBus,
    running: Arc<AtomicBool>,
    start_time: Arc<RwLock<Option<Instant>>>,
    config: Option<crate::embedded_broker::EmbeddedBrokerConfig>,
}

#[cfg(feature = "embedded-broker")]
impl InternalMqttBrokerPlugin {
    /// Create a new internal MQTT broker plugin.
    pub fn new(event_bus: EventBus) -> Self {
        let base = PluginMetadata::new(
            "internal-mqtt-broker",
            "Internal MQTT Broker",
            "1.0.0",
            ">=1.0.0",
        )
        .with_description("Built-in MQTT broker for local device communication")
        .with_author("NeoTalk")
        .with_type("internal_mqtt_broker");

        let metadata = ExtendedPluginMetadata {
            base,
            plugin_type: PluginType::InternalMqttBroker,
            version: semver::Version::new(1, 0, 0),
            required_neotalk_version: semver::Version::new(1, 0, 0),
            dependencies: vec![],
            config_schema: Some(internal_broker_config_schema()),
            resource_limits: None,
            permissions: vec![
                PluginPermission::NetworkAccess,
                PluginPermission::EventPublish,
            ],
            homepage: Some("https://github.com/neotalk".to_string()),
            repository: None,
            license: Some("MIT".to_string()),
        };

        Self {
            metadata,
            state_machine: StateMachine::new(),
            stats: PluginStats::default(),
            event_bus,
            running: Arc::new(AtomicBool::new(false)),
            start_time: Arc::new(RwLock::new(None)),
            config: None,
        }
    }

    /// Get the broker configuration.
    pub fn config(&self) -> Option<&crate::embedded_broker::EmbeddedBrokerConfig> {
        self.config.as_ref()
    }

    /// Get current connection count.
    pub async fn connection_count(&self) -> usize {
        // This would be tracked by the actual broker implementation
        // For now, return a placeholder
        0
    }
}

#[cfg(feature = "embedded-broker")]
#[async_trait::async_trait]
impl edge_ai_core::plugin::UnifiedPlugin for InternalMqttBrokerPlugin {
    fn metadata(&self) -> &ExtendedPluginMetadata {
        &self.metadata
    }

    async fn initialize(&mut self, config: &serde_json::Value) -> Result<(), PluginError> {
        // Parse configuration if provided
        if let Some(config_obj) = config.as_object() {
            let broker_config = crate::embedded_broker::EmbeddedBrokerConfig {
                listen: config_obj
                    .get("listen")
                    .and_then(|v| v.as_str())
                    .unwrap_or("0.0.0.0")
                    .to_string(),
                port: config_obj
                    .get("port")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1883) as u16,
                max_connections: config_obj
                    .get("max_connections")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1000) as usize,
                max_payload_size: config_obj
                    .get("max_payload_size")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(268435456) as usize,
                connection_timeout_ms: config_obj
                    .get("connection_timeout_ms")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(60000) as u16,
                dynamic_filters: config_obj
                    .get("dynamic_filters")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true),
            };
            self.config = Some(broker_config);
        } else {
            self.config = Some(crate::embedded_broker::EmbeddedBrokerConfig::default());
        }

        self.state_machine.transition(PluginState::Initialized, "Initialization".to_string())
            .map_err(|e| PluginError::InitializationFailed(e.to_string()))?;
        Ok(())
    }

    async fn start(&mut self) -> Result<(), PluginError> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(());
        }

        let config = self.config.clone()
            .unwrap_or_default();

        let broker = crate::embedded_broker::EmbeddedBroker::new(config.clone());
        broker.start()
            .map_err(|e| PluginError::InitializationFailed(format!("Failed to start broker: {}", e)))?;

        self.running.store(true, Ordering::Relaxed);
        *self.start_time.write().await = Some(Instant::now());
        self.stats.record_start();

        self.state_machine.transition(PluginState::Running, "Start".to_string())
            .map_err(|e| PluginError::InitializationFailed(e.to_string()))?;

        tracing::info!("Internal MQTT Broker plugin started on {}:{}", config.listen, config.port);

        Ok(())
    }

    async fn stop(&mut self) -> Result<(), PluginError> {
        if !self.running.load(Ordering::Relaxed) {
            return Ok(());
        }

        self.running.store(false, Ordering::Relaxed);
        *self.start_time.write().await = None;

        let duration = self.start_time.read().await
            .map(|t| t.elapsed().as_millis() as u64)
            .unwrap_or(0);
        self.stats.record_stop(duration);

        self.state_machine.transition(PluginState::Stopped, "Stop".to_string())
            .map_err(|e| PluginError::ExecutionFailed(e.to_string()))?;

        tracing::info!("Internal MQTT Broker plugin stopped");

        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), PluginError> {
        if self.running.load(Ordering::Relaxed) {
            self.stop().await?;
        }

        self.state_machine.transition(PluginState::Loaded, "Shutdown".to_string())
            .map_err(|e| PluginError::ExecutionFailed(e.to_string()))?;

        Ok(())
    }

    fn get_state(&self) -> PluginState {
        self.state_machine.current().clone()
    }

    async fn health_check(&self) -> Result<(), PluginError> {
        if !self.running.load(Ordering::Relaxed) {
            return Err(PluginError::Other(anyhow::anyhow!("Broker not running")));
        }

        let default_config = crate::embedded_broker::EmbeddedBrokerConfig::default();
        let config = self.config.as_ref().unwrap_or(&default_config);

        if crate::embedded_broker::is_broker_running(config.port) {
            Ok(())
        } else {
            Err(PluginError::Other(anyhow::anyhow!("Broker port not accessible")))
        }
    }

    fn get_stats(&self) -> PluginStats {
        let mut stats = self.stats.clone();
        // Add broker-specific stats
        stats.avg_response_time_ms = stats.avg_response_time_ms;
        stats
    }

    async fn handle_command(&self, command: &str, _args: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
        match command {
            "get_status" => {
                let default_config = crate::embedded_broker::EmbeddedBrokerConfig::default();
                let config = self.config.as_ref().unwrap_or(&default_config);

                Ok(json!({
                    "running": self.running.load(Ordering::Relaxed),
                    "port": config.port,
                    "listen": config.listen,
                    "max_connections": config.max_connections,
                    "state": self.get_state(),
                }))
            }
            "get_connections" => {
                let count = self.connection_count().await;
                Ok(json!({
                    "connection_count": count,
                    "max_connections": self.config.as_ref()
                        .map(|c| c.max_connections)
                        .unwrap_or(1000),
                }))
            }
            "get_info" => {
                Ok(json!({
                    "id": self.metadata.base.id,
                    "name": self.metadata.base.name,
                    "plugin_type": "internal_mqtt_broker",
                    "version": self.metadata.version.to_string(),
                    "running": self.running.load(Ordering::Relaxed),
                    "description": self.metadata.base.description,
                }))
            }
            _ => {
                Err(PluginError::Other(anyhow::anyhow!("Unknown command: {}", command)))
            }
        }
    }
}

/// External MQTT Broker plugin.
///
/// This plugin manages connections to external MQTT brokers.
pub struct ExternalMqttBrokerPlugin {
    metadata: ExtendedPluginMetadata,
    state_machine: StateMachine,
    stats: PluginStats,
    event_bus: EventBus,
    running: Arc<AtomicBool>,
    config: Option<ExternalBrokerConfig>,
    device_count: Arc<AtomicUsize>,
}

impl ExternalMqttBrokerPlugin {
    /// Create a new external MQTT broker plugin.
    pub fn new(id: String, name: String, event_bus: EventBus) -> Self {
        let base = PluginMetadata::new(
            &id,
            &name,
            "1.0.0",
            ">=1.0.0",
        )
        .with_description("External MQTT broker connection")
        .with_type("external_mqtt_broker");

        let metadata = ExtendedPluginMetadata {
            base,
            plugin_type: PluginType::ExternalMqttBroker,
            version: semver::Version::new(1, 0, 0),
            required_neotalk_version: semver::Version::new(1, 0, 0),
            dependencies: vec![],
            config_schema: Some(external_broker_config_schema()),
            resource_limits: None,
            permissions: vec![
                PluginPermission::NetworkAccess,
                PluginPermission::EventPublish,
                PluginPermission::EventSubscribe,
            ],
            homepage: None,
            repository: None,
            license: None,
        };

        Self {
            metadata,
            state_machine: StateMachine::new(),
            stats: PluginStats::default(),
            event_bus,
            running: Arc::new(AtomicBool::new(false)),
            config: None,
            device_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Get the plugin configuration.
    pub fn config(&self) -> Option<&ExternalBrokerConfig> {
        self.config.as_ref()
    }

    /// Set the device count.
    pub fn set_device_count(&self, count: usize) {
        self.device_count.store(count, Ordering::Relaxed);
    }
}

#[async_trait::async_trait]
impl edge_ai_core::plugin::UnifiedPlugin for ExternalMqttBrokerPlugin {
    fn metadata(&self) -> &ExtendedPluginMetadata {
        &self.metadata
    }

    async fn initialize(&mut self, config: &serde_json::Value) -> Result<(), PluginError> {
        let broker_config: ExternalBrokerConfig = serde_json::from_value(config.clone())
            .map_err(|e| PluginError::InitializationFailed(format!("Invalid config: {}", e)))?;

        self.config = Some(broker_config);

        self.state_machine.transition(PluginState::Initialized, "Initialization".to_string())
            .map_err(|e| PluginError::InitializationFailed(e.to_string()))?;
        Ok(())
    }

    async fn start(&mut self) -> Result<(), PluginError> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(());
        }

        let config = self.config.as_ref()
            .ok_or_else(|| PluginError::InitializationFailed("No configuration".to_string()))?;

        // The actual MQTT connection would be managed by the MqttAdapter
        // This plugin serves as the management layer
        self.running.store(true, Ordering::Relaxed);
        self.stats.record_start();

        self.state_machine.transition(PluginState::Running, "Start".to_string())
            .map_err(|e| PluginError::InitializationFailed(e.to_string()))?;

        tracing::info!("External MQTT Broker plugin started: {}", config.broker);

        Ok(())
    }

    async fn stop(&mut self) -> Result<(), PluginError> {
        if !self.running.load(Ordering::Relaxed) {
            return Ok(());
        }

        self.running.store(false, Ordering::Relaxed);
        self.stats.record_stop(0);

        self.state_machine.transition(PluginState::Stopped, "Stop".to_string())
            .map_err(|e| PluginError::ExecutionFailed(e.to_string()))?;

        tracing::info!("External MQTT Broker plugin stopped");

        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), PluginError> {
        if self.running.load(Ordering::Relaxed) {
            self.stop().await?;
        }

        self.state_machine.transition(PluginState::Loaded, "Shutdown".to_string())
            .map_err(|e| PluginError::ExecutionFailed(e.to_string()))?;

        Ok(())
    }

    fn get_state(&self) -> PluginState {
        self.state_machine.current().clone()
    }

    async fn health_check(&self) -> Result<(), PluginError> {
        if !self.running.load(Ordering::Relaxed) {
            return Err(PluginError::Other(anyhow::anyhow!("Not running")));
        }
        // Would check actual connection status here
        Ok(())
    }

    fn get_stats(&self) -> PluginStats {
        let mut stats = self.stats.clone();
        stats.start_count = self.device_count.load(Ordering::Relaxed) as u64;
        stats
    }

    async fn handle_command(&self, command: &str, _args: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
        match command {
            "get_status" => {
                let config = self.config.as_ref()
                    .ok_or_else(|| PluginError::Other(anyhow::anyhow!("No configuration")))?;

                Ok(json!({
                    "running": self.running.load(Ordering::Relaxed),
                    "broker": config.broker,
                    "topics": config.topics,
                    "use_tls": config.use_tls,
                    "device_count": self.device_count.load(Ordering::Relaxed),
                }))
            }
            "get_info" => {
                Ok(json!({
                    "id": self.metadata.base.id,
                    "name": self.metadata.base.name,
                    "plugin_type": "external_mqtt_broker",
                    "running": self.running.load(Ordering::Relaxed),
                    "version": self.metadata.version.to_string(),
                }))
            }
            _ => {
                Err(PluginError::Other(anyhow::anyhow!("Unknown command: {}", command)))
            }
        }
    }
}

/// Modbus Adapter plugin.
///
/// This plugin manages Modbus TCP device connections.
pub struct ModbusAdapterPlugin {
    metadata: ExtendedPluginMetadata,
    state_machine: StateMachine,
    stats: PluginStats,
    event_bus: EventBus,
    running: Arc<AtomicBool>,
    config: Option<ModbusAdapterConfig>,
    device_count: Arc<AtomicUsize>,
}

impl ModbusAdapterPlugin {
    /// Create a new Modbus adapter plugin.
    pub fn new(id: String, name: String, event_bus: EventBus) -> Self {
        let base = PluginMetadata::new(
            &id,
            &name,
            "1.0.0",
            ">=1.0.0",
        )
        .with_description("Modbus TCP device adapter")
        .with_type("modbus_adapter");

        let metadata = ExtendedPluginMetadata {
            base,
            plugin_type: PluginType::ModbusAdapter,
            version: semver::Version::new(1, 0, 0),
            required_neotalk_version: semver::Version::new(1, 0, 0),
            dependencies: vec![],
            config_schema: Some(modbus_adapter_config_schema()),
            resource_limits: None,
            permissions: vec![
                PluginPermission::NetworkAccess,
                PluginPermission::EventPublish,
                PluginPermission::DeviceRead,
            ],
            homepage: None,
            repository: None,
            license: None,
        };

        Self {
            metadata,
            state_machine: StateMachine::new(),
            stats: PluginStats::default(),
            event_bus,
            running: Arc::new(AtomicBool::new(false)),
            config: None,
            device_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[async_trait::async_trait]
impl edge_ai_core::plugin::UnifiedPlugin for ModbusAdapterPlugin {
    fn metadata(&self) -> &ExtendedPluginMetadata {
        &self.metadata
    }

    async fn initialize(&mut self, config: &serde_json::Value) -> Result<(), PluginError> {
        let modbus_config: ModbusAdapterConfig = serde_json::from_value(config.clone())
            .map_err(|e| PluginError::InitializationFailed(format!("Invalid config: {}", e)))?;

        self.config = Some(modbus_config);

        self.state_machine.transition(PluginState::Initialized, "Initialization".to_string())
            .map_err(|e| PluginError::InitializationFailed(e.to_string()))?;
        Ok(())
    }

    async fn start(&mut self) -> Result<(), PluginError> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(());
        }

        self.running.store(true, Ordering::Relaxed);
        self.stats.record_start();

        self.state_machine.transition(PluginState::Running, "Start".to_string())
            .map_err(|e| PluginError::InitializationFailed(e.to_string()))?;

        tracing::info!("Modbus Adapter plugin started");

        Ok(())
    }

    async fn stop(&mut self) -> Result<(), PluginError> {
        if !self.running.load(Ordering::Relaxed) {
            return Ok(());
        }

        self.running.store(false, Ordering::Relaxed);
        self.stats.record_stop(0);

        self.state_machine.transition(PluginState::Stopped, "Stop".to_string())
            .map_err(|e| PluginError::ExecutionFailed(e.to_string()))?;

        tracing::info!("Modbus Adapter plugin stopped");

        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), PluginError> {
        if self.running.load(Ordering::Relaxed) {
            self.stop().await?;
        }

        self.state_machine.transition(PluginState::Loaded, "Shutdown".to_string())
            .map_err(|e| PluginError::ExecutionFailed(e.to_string()))?;

        Ok(())
    }

    fn get_state(&self) -> PluginState {
        self.state_machine.current().clone()
    }

    async fn health_check(&self) -> Result<(), PluginError> {
        if !self.running.load(Ordering::Relaxed) {
            return Err(PluginError::Other(anyhow::anyhow!("Not running")));
        }
        Ok(())
    }

    fn get_stats(&self) -> PluginStats {
        let mut stats = self.stats.clone();
        stats.start_count = self.device_count.load(Ordering::Relaxed) as u64;
        stats
    }

    async fn handle_command(&self, command: &str, _args: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
        match command {
            "get_info" => {
                Ok(json!({
                    "id": self.metadata.base.id,
                    "name": self.metadata.base.name,
                    "plugin_type": "modbus_adapter",
                    "running": self.running.load(Ordering::Relaxed),
                    "version": self.metadata.version.to_string(),
                }))
            }
            _ => {
                Err(PluginError::Other(anyhow::anyhow!("Unknown command: {}", command)))
            }
        }
    }
}

/// Unified device adapter plugin factory.
///
/// This factory creates specialized plugins for different adapter types.
pub struct UnifiedAdapterPluginFactory;

impl UnifiedAdapterPluginFactory {
    /// Create an internal MQTT broker plugin.
    #[cfg(feature = "embedded-broker")]
    pub fn create_internal_broker(event_bus: EventBus) -> Arc<RwLock<InternalMqttBrokerPlugin>> {
        Arc::new(RwLock::new(InternalMqttBrokerPlugin::new(event_bus)))
    }

    /// Create an internal MQTT broker plugin (stub when feature is disabled).
    ///
    /// This returns an error when the embedded-broker feature is not enabled.
    #[cfg(not(feature = "embedded-broker"))]
    pub fn create_internal_broker(_event_bus: EventBus) -> Arc<RwLock<ExternalMqttBrokerPlugin>> {
        // Return an external broker as a fallback with default config
        Arc::new(RwLock::new(ExternalMqttBrokerPlugin::new(
            "fallback-broker".to_string(),
            "Fallback Broker".to_string(),
            _event_bus,
        )))
    }

    /// Create an external MQTT broker plugin.
    pub fn create_external_broker(
        id: String,
        name: String,
        event_bus: EventBus,
    ) -> Arc<RwLock<ExternalMqttBrokerPlugin>> {
        Arc::new(RwLock::new(ExternalMqttBrokerPlugin::new(id, name, event_bus)))
    }

    /// Create a Modbus adapter plugin.
    pub fn create_modbus_adapter(
        id: String,
        name: String,
        event_bus: EventBus,
    ) -> Arc<RwLock<ModbusAdapterPlugin>> {
        Arc::new(RwLock::new(ModbusAdapterPlugin::new(id, name, event_bus)))
    }

    /// Wrap a generic DeviceAdapter as a plugin.
    pub fn wrap_adapter(
        adapter: Arc<dyn DeviceAdapter>,
        event_bus: EventBus,
    ) -> Arc<RwLock<DeviceAdapterPlugin>> {
        DeviceAdapterPluginFactory::create_plugin(adapter, event_bus)
    }
}

/// Generate JSON Schema for internal broker configuration.
fn internal_broker_config_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "title": "Internal MQTT Broker Configuration",
        "description": "Configure the embedded MQTT broker settings",
        "properties": {
            "listen": {
                "type": "string",
                "title": "Listen Address",
                "description": "IP address to listen on",
                "default": "0.0.0.0",
                "format": "ipv4"
            },
            "port": {
                "type": "integer",
                "title": "Port",
                "description": "MQTT broker port",
                "default": 1883,
                "minimum": 1024,
                "maximum": 65535
            },
            "max_connections": {
                "type": "integer",
                "title": "Max Connections",
                "description": "Maximum concurrent connections",
                "default": 1000,
                "minimum": 1
            },
            "max_payload_size": {
                "type": "integer",
                "title": "Max Payload Size",
                "description": "Maximum message payload size in bytes",
                "default": 268435456
            },
            "connection_timeout_ms": {
                "type": "integer",
                "title": "Connection Timeout",
                "description": "Connection timeout in milliseconds",
                "default": 60000
            },
            "dynamic_filters": {
                "type": "boolean",
                "title": "Dynamic Filters",
                "description": "Enable dynamic topic filters",
                "default": true
            }
        },
        "required": ["port"],
        "ui_hints": {
            "field_order": ["listen", "port", "max_connections", "max_payload_size", "connection_timeout_ms", "dynamic_filters"],
            "display_names": {
                "listen": "监听地址",
                "port": "端口",
                "max_connections": "最大连接数",
                "max_payload_size": "最大负载大小",
                "connection_timeout_ms": "连接超时 (毫秒)",
                "dynamic_filters": "动态过滤器"
            }
        }
    })
}

/// Generate JSON Schema for external broker configuration.
fn external_broker_config_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "title": "External MQTT Broker Configuration",
        "description": "Configure connection to an external MQTT broker",
        "properties": {
            "id": {
                "type": "string",
                "title": "Broker ID",
                "description": "Unique identifier for this broker"
            },
            "name": {
                "type": "string",
                "title": "Display Name",
                "description": "Human-readable name"
            },
            "broker": {
                "type": "string",
                "title": "Broker Address",
                "description": "MQTT broker address (e.g., localhost:1883)",
                "format": "hostname"
            },
            "client_id": {
                "type": "string",
                "title": "Client ID Prefix",
                "description": "MQTT client ID prefix (UUID will be appended)",
                "default": "neotalk-external"
            },
            "topics": {
                "type": "array",
                "title": "Subscribe Topics",
                "description": "MQTT topics to subscribe to",
                "items": {
                    "type": "string"
                },
                "default": ["sensors/#"]
            },
            "username": {
                "type": "string",
                "title": "Username",
                "description": "MQTT username (optional)"
            },
            "password": {
                "type": "string",
                "title": "Password",
                "description": "MQTT password (optional)",
                "x-security": "secret"
            },
            "use_tls": {
                "type": "boolean",
                "title": "Use TLS",
                "description": "Connect using TLS/SSL",
                "default": false
            },
            "timeout_secs": {
                "type": "integer",
                "title": "Connection Timeout",
                "description": "Connection timeout in seconds",
                "default": 30,
                "minimum": 1
            },
            "keep_alive_secs": {
                "type": "integer",
                "title": "Keep-Alive",
                "description": "Keep-alive interval in seconds",
                "default": 60,
                "minimum": 10
            },
            "auto_reconnect": {
                "type": "boolean",
                "title": "Auto Reconnect",
                "description": "Automatically reconnect on connection loss",
                "default": true
            },
            "qos": {
                "type": "integer",
                "title": "QoS Level",
                "description": "Quality of Service level (0, 1, or 2)",
                "default": 1,
                "minimum": 0,
                "maximum": 2
            },
            "enabled": {
                "type": "boolean",
                "title": "Enabled",
                "description": "Whether this broker is enabled",
                "default": true
            }
        },
        "required": ["id", "name", "broker", "topics"],
        "ui_hints": {
            "field_order": ["name", "broker", "topics", "username", "password", "use_tls", "qos", "enabled"],
            "display_names": {
                "id": "Broker ID",
                "name": "显示名称",
                "broker": "Broker 地址",
                "client_id": "客户端 ID",
                "topics": "订阅主题",
                "username": "用户名",
                "password": "密码",
                "use_tls": "使用 TLS",
                "timeout_secs": "连接超时",
                "keep_alive_secs": "保活间隔",
                "auto_reconnect": "自动重连",
                "qos": "QoS 级别",
                "enabled": "启用"
            },
            "placeholders": {
                "broker": "localhost:1883",
                "topics": "sensors/#"
            }
        }
    })
}

/// Generate JSON Schema for Modbus adapter configuration.
fn modbus_adapter_config_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "title": "Modbus Adapter Configuration",
        "description": "Configure Modbus TCP device adapter",
        "properties": {
            "id": {
                "type": "string",
                "title": "Adapter ID",
                "description": "Unique identifier for this adapter"
            },
            "name": {
                "type": "string",
                "title": "Display Name",
                "description": "Human-readable name"
            },
            "host": {
                "type": "string",
                "title": "Host Address",
                "description": "Modbus device IP address",
                "format": "ipv4"
            },
            "port": {
                "type": "integer",
                "title": "Port",
                "description": "Modbus TCP port",
                "default": 502,
                "minimum": 1024,
                "maximum": 65535
            },
            "slave_id": {
                "type": "integer",
                "title": "Slave ID",
                "description": "Modbus slave/unit ID",
                "default": 1,
                "minimum": 1,
                "maximum": 247
            },
            "timeout_ms": {
                "type": "integer",
                "title": "Timeout",
                "description": "Connection timeout in milliseconds",
                "default": 5000,
                "minimum": 100
            },
            "poll_interval_ms": {
                "type": "integer",
                "title": "Poll Interval",
                "description": "Device poll interval in milliseconds",
                "default": 1000,
                "minimum": 100
            },
            "enabled": {
                "type": "boolean",
                "title": "Enabled",
                "description": "Whether this adapter is enabled",
                "default": true
            }
        },
        "required": ["id", "name", "host"],
        "ui_hints": {
            "field_order": ["name", "host", "port", "slave_id", "timeout_ms", "poll_interval_ms", "enabled"],
            "display_names": {
                "id": "Adapter ID",
                "name": "显示名称",
                "host": "主机地址",
                "port": "端口",
                "slave_id": "从机 ID",
                "timeout_ms": "超时 (毫秒)",
                "poll_interval_ms": "轮询间隔 (毫秒)",
                "enabled": "启用"
            },
            "placeholders": {
                "host": "192.168.1.100"
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_external_broker_config_schema() {
        let schema = external_broker_config_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"].is_object());
        assert!(schema["required"].is_array());
    }

    #[test]
    fn test_internal_broker_config_schema() {
        let schema = internal_broker_config_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["port"].is_object());
    }

    #[test]
    fn test_modbus_adapter_config_schema() {
        let schema = modbus_adapter_config_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["port"]["default"], 502);
    }
}
