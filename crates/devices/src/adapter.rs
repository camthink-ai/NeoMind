//! Device adapter interface for NeoTalk event-driven architecture.
//!
//! This module defines the adapter pattern for integrating various data sources
//! (MQTT, HASS, HTTP, etc.) into the NeoTalk platform through a unified interface.

use crate::mdl::MetricValue;
use async_trait::async_trait;
use edge_ai_core::{EventBus, NeoTalkEvent};
use futures::{Stream, StreamExt};
use std::pin::Pin;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::broadcast;

/// Result type for adapter operations.
pub type AdapterResult<T> = Result<T, AdapterError>;

/// Error type for device adapter operations.
#[derive(Debug, Error)]
pub enum AdapterError {
    /// Adapter configuration error
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Connection error
    #[error("Connection error: {0}")]
    Connection(String),

    /// Communication error
    #[error("Communication error: {0}")]
    Communication(String),

    /// Device not found
    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    /// Operation timeout
    #[error("Operation timeout after {0}ms")]
    Timeout(u64),

    /// Adapter stopped
    #[error("Adapter is stopped")]
    Stopped,

    /// Other error
    #[error("Adapter error: {0}")]
    Other(#[from] anyhow::Error),
}

/// Device event emitted by adapters.
///
/// These events are the primary way adapters communicate with the rest of
/// the NeoTalk system. They are typically converted to NeoTalkEvent and
/// published on the event bus.
#[derive(Debug, Clone)]
pub enum DeviceEvent {
    /// Device metric update
    Metric {
        device_id: String,
        metric: String,
        value: MetricValue,
        timestamp: i64,
    },

    /// Device state change
    State {
        device_id: String,
        old_state: ConnectionStatus,
        new_state: ConnectionStatus,
        timestamp: i64,
    },

    /// Device discovery event
    Discovery { device: DiscoveredDeviceInfo },

    /// Device command result
    CommandResult {
        device_id: String,
        command: String,
        success: bool,
        result: Option<String>,
        timestamp: i64,
    },
}

impl DeviceEvent {
    /// Get the device ID for this event.
    pub fn device_id(&self) -> Option<&str> {
        match self {
            Self::Metric { device_id, .. }
            | Self::State { device_id, .. }
            | Self::CommandResult { device_id, .. } => Some(device_id),
            Self::Discovery { .. } => None,
        }
    }

    /// Get the timestamp for this event.
    pub fn timestamp(&self) -> i64 {
        match self {
            Self::Metric { timestamp, .. }
            | Self::State { timestamp, .. }
            | Self::CommandResult { timestamp, .. } => *timestamp,
            Self::Discovery { device } => device.timestamp,
        }
    }

    /// Convert to NeoTalkEvent.
    pub fn to_neotalk_event(self) -> NeoTalkEvent {
        match self {
            Self::Metric {
                device_id,
                metric,
                value,
                timestamp,
            } => NeoTalkEvent::DeviceMetric {
                device_id,
                metric,
                value: convert_metric_value(value),
                timestamp,
                quality: None,
            },
            Self::State {
                device_id,
                old_state: _,
                new_state,
                timestamp,
            } => {
                match new_state {
                    ConnectionStatus::Connected => NeoTalkEvent::DeviceOnline {
                        device_id,
                        device_type: "unknown".to_string(),
                        timestamp,
                    },
                    ConnectionStatus::Disconnected | ConnectionStatus::Error => {
                        NeoTalkEvent::DeviceOffline {
                            device_id,
                            reason: Some(format!("{:?}", new_state)),
                            timestamp,
                        }
                    }
                    _ => {
                        // Other state transitions don't generate online/offline events
                        NeoTalkEvent::DeviceOnline {
                            device_id,
                            device_type: "unknown".to_string(),
                            timestamp,
                        }
                    }
                }
            }
            Self::Discovery { device } => {
                // Discovery creates an online event
                NeoTalkEvent::DeviceOnline {
                    device_id: device.device_id,
                    device_type: device.device_type.clone(),
                    timestamp: device.timestamp,
                }
            }
            Self::CommandResult {
                device_id,
                command,
                success,
                result,
                timestamp,
            } => NeoTalkEvent::DeviceCommandResult {
                device_id,
                command,
                success,
                result: result.map(|r| serde_json::json!(r)),
                timestamp,
            },
        }
    }
}

/// Convert MDL MetricValue to core MetricValue.
fn convert_metric_value(value: MetricValue) -> edge_ai_core::MetricValue {
    use serde_json::json;
    match value {
        MetricValue::Integer(v) => edge_ai_core::MetricValue::Integer(v),
        MetricValue::Float(v) => edge_ai_core::MetricValue::Float(v),
        MetricValue::String(v) => edge_ai_core::MetricValue::String(v),
        MetricValue::Boolean(v) => edge_ai_core::MetricValue::Boolean(v),
        MetricValue::Array(arr) => {
            // Convert array to JSON for core metric value
            let json_arr: Vec<serde_json::Value> = arr.iter().map(|v| match v {
                MetricValue::Integer(i) => json!(*i),
                MetricValue::Float(f) => json!(*f),
                MetricValue::String(s) => json!(s),
                MetricValue::Boolean(b) => json!(*b),
                _ => json!(null),
            }).collect();
            edge_ai_core::MetricValue::Json(json!(json_arr))
        }
        MetricValue::Binary(_) => edge_ai_core::MetricValue::String("<binary>".to_string()),
        MetricValue::Null => edge_ai_core::MetricValue::String("null".to_string()),
    }
}

/// Device connection status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ConnectionStatus {
    /// Device is disconnected
    Disconnected,
    /// Currently connecting
    Connecting,
    /// Device is connected and operational
    Connected,
    /// Reconnecting after disconnect
    Reconnecting,
    /// Error state
    Error,
}

impl std::fmt::Display for ConnectionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disconnected => write!(f, "disconnected"),
            Self::Connecting => write!(f, "connecting"),
            Self::Connected => write!(f, "connected"),
            Self::Reconnecting => write!(f, "reconnecting"),
            Self::Error => write!(f, "error"),
        }
    }
}

/// Information about a discovered device.
#[derive(Debug, Clone)]
pub struct DiscoveredDeviceInfo {
    /// Unique device identifier
    pub device_id: String,
    /// Device type
    pub device_type: String,
    /// Device name (human-readable)
    pub name: Option<String>,
    /// Connection endpoint (e.g., MQTT topic, HASS entity ID)
    pub endpoint: Option<String>,
    /// Device capabilities
    pub capabilities: Vec<String>,
    /// Discovery timestamp
    pub timestamp: i64,
    /// Additional metadata
    pub metadata: serde_json::Value,
}

impl DiscoveredDeviceInfo {
    /// Create a new discovered device info.
    pub fn new(device_id: impl Into<String>, device_type: impl Into<String>) -> Self {
        Self {
            device_id: device_id.into(),
            device_type: device_type.into(),
            name: None,
            endpoint: None,
            capabilities: Vec::new(),
            timestamp: chrono::Utc::now().timestamp(),
            metadata: serde_json::json!({}),
        }
    }

    /// Set the device name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the endpoint.
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }
}

/// Device adapter trait.
///
/// All device adapters (MQTT, HASS, HTTP, etc.) implement this trait
/// to provide a unified interface for the adapter manager.
///
/// Simplified architecture: Adapters should automatically use templates from DeviceRegistry
/// to parse data. The adapter receives raw protocol data and uses the device's template
/// to convert it to DeviceEvents.
#[async_trait]
pub trait DeviceAdapter: Send + Sync {
    /// Get the adapter name.
    fn name(&self) -> &str;

    /// Get the adapter type identifier (e.g., "mqtt", "hass").
    fn adapter_type(&self) -> &'static str {
        "base"
    }

    /// Check if the adapter is running.
    fn is_running(&self) -> bool;

    /// Start the adapter.
    ///
    /// This should connect to the data source and begin emitting events.
    /// The adapter should use templates from DeviceRegistry to parse incoming data.
    async fn start(&self) -> AdapterResult<()>;

    /// Stop the adapter.
    ///
    /// This should disconnect from the data source and clean up resources.
    async fn stop(&self) -> AdapterResult<()>;

    /// Subscribe to device events from this adapter.
    ///
    /// Returns a stream of device events. Multiple subscribers are supported.
    /// Events should be automatically parsed using device templates.
    fn subscribe(&self) -> Pin<Box<dyn Stream<Item = DeviceEvent> + Send + '_>>;

    /// Set telemetry storage for this adapter (optional, default implementation does nothing).
    ///
    /// Adapters that can write directly to telemetry storage should implement this
    /// to enable direct data writing. If not set, data will only be published via EventBus.
    fn set_telemetry_storage(&self, _storage: Arc<crate::TimeSeriesStorage>) {
        // Default: do nothing - adapter will only publish via EventBus
    }

    /// Get the number of devices currently managed by this adapter.
    fn device_count(&self) -> usize;

    /// Get a list of device IDs managed by this adapter.
    fn list_devices(&self) -> Vec<String>;

    /// Get this adapter as `Any` for downcasting
    fn as_any(&self) -> &dyn std::any::Any;

    /// Send a command to a device via this adapter.
    ///
    /// The adapter should use the device's connection configuration and template
    /// to determine how to send the command (topic, payload format, etc.).
    ///
    /// # Arguments
    /// - `device_id`: The device to send the command to
    /// - `command_name`: The command name from the device template
    /// - `payload`: The pre-built command payload (from template)
    /// - `topic`: Optional topic override (if None, adapter should use device config)
    async fn send_command(
        &self,
        device_id: &str,
        command_name: &str,
        payload: String,
        topic: Option<String>,
    ) -> AdapterResult<()>;

    /// Get the connection status of this adapter.
    ///
    /// For adapters that manage connections (e.g., MQTT), this returns the
    /// current connection state. For stateless adapters, this may always return Connected.
    fn connection_status(&self) -> ConnectionStatus;

    /// Subscribe to a device's data stream (for protocols that support subscriptions).
    ///
    /// This is typically used for MQTT adapters to subscribe to device topics.
    /// Other adapters may implement this as a no-op.
    async fn subscribe_device(&self, device_id: &str) -> AdapterResult<()>;

    /// Unsubscribe from a device's data stream.
    ///
    /// This is typically used for MQTT adapters to unsubscribe from device topics.
    /// Other adapters may implement this as a no-op.
    async fn unsubscribe_device(&self, device_id: &str) -> AdapterResult<()>;
}

/// Adapter configuration.
#[derive(Debug, Clone)]
pub struct AdapterConfig {
    /// Adapter type identifier
    pub adapter_type: String,
    /// Adapter name (unique instance identifier)
    pub name: String,
    /// Whether to auto-start the adapter
    pub auto_start: bool,
    /// Additional configuration parameters
    pub params: serde_json::Value,
}

impl AdapterConfig {
    /// Create a new adapter configuration.
    pub fn new(adapter_type: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            adapter_type: adapter_type.into(),
            name: name.into(),
            auto_start: true,
            params: serde_json::json!({}),
        }
    }

    /// Set whether to auto-start.
    pub fn with_auto_start(mut self, auto_start: bool) -> Self {
        self.auto_start = auto_start;
        self
    }

    /// Set configuration parameters.
    pub fn with_params(mut self, params: serde_json::Value) -> Self {
        self.params = params;
        self
    }
}

/// Event publishing adapter.
///
/// Wraps a DeviceAdapter and automatically publishes events to the event bus.
pub struct EventPublishingAdapter {
    /// The underlying adapter
    adapter: Arc<dyn DeviceAdapter>,
    /// Event bus for publishing events
    event_bus: EventBus,
    /// Event channel sender
    event_tx: broadcast::Sender<DeviceEvent>,
    /// Running state
    running: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl EventPublishingAdapter {
    /// Create a new event-publishing wrapper for an adapter.
    pub fn new(adapter: Arc<dyn DeviceAdapter>, event_bus: EventBus) -> Self {
        let (event_tx, _) = broadcast::channel(1000);
        Self {
            adapter,
            event_bus,
            event_tx,
            running: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Get the underlying adapter.
    pub fn adapter(&self) -> &Arc<dyn DeviceAdapter> {
        &self.adapter
    }

    /// Check if the adapter is running.
    pub fn is_running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Start the adapter and begin publishing events.
    pub async fn start(&self) -> AdapterResult<()> {
        if self.is_running() {
            return Ok(());
        }

        self.adapter.start().await?;

        self.running
            .store(true, std::sync::atomic::Ordering::Relaxed);

        // Spawn event forwarding task
        let adapter = self.adapter.clone();
        let event_bus = self.event_bus.clone();
        let running = self.running.clone();

        tokio::spawn(async move {
            let mut rx = adapter.subscribe();
            while running.load(std::sync::atomic::Ordering::Relaxed) {
                match rx.next().await {
                    Some(event) => {
                        let device_id = event.device_id().unwrap_or("unknown").to_string();
                        let neotalk_event = event.to_neotalk_event();
                        let source = format!("adapter:{}", device_id);
                        event_bus.publish_with_source(neotalk_event, source).await;
                    }
                    None => break,
                }
            }
        });

        Ok(())
    }

    /// Stop the adapter.
    pub async fn stop(&self) -> AdapterResult<()> {
        self.running
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.adapter.stop().await
    }

    /// Subscribe to events from this adapter.
    pub fn subscribe(&self) -> broadcast::Receiver<DeviceEvent> {
        self.event_tx.subscribe()
    }
}

/// Mock adapter for testing.
pub struct MockAdapter {
    name: String,
    running: std::sync::Arc<std::sync::atomic::AtomicBool>,
    event_tx: broadcast::Sender<DeviceEvent>,
    devices: Vec<String>,
}

impl MockAdapter {
    /// Create a new mock adapter.
    pub fn new(name: impl Into<String>) -> Self {
        let (event_tx, _) = broadcast::channel(100);
        Self {
            name: name.into(),
            running: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            event_tx,
            devices: Vec::new(),
        }
    }

    /// Add a device to the mock adapter.
    pub fn with_device(mut self, device_id: impl Into<String>) -> Self {
        self.devices.push(device_id.into());
        self
    }

    /// Publish a test event.
    pub fn publish_event(
        &self,
        event: DeviceEvent,
    ) -> Result<(), broadcast::error::SendError<DeviceEvent>> {
        self.event_tx.send(event).map(|_| ())
    }
}

#[async_trait]
impl DeviceAdapter for MockAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn is_running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::Relaxed)
    }

    async fn start(&self) -> AdapterResult<()> {
        self.running
            .store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    async fn stop(&self) -> AdapterResult<()> {
        self.running
            .store(false, std::sync::atomic::Ordering::Relaxed);
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
        self.devices.len()
    }

    fn list_devices(&self) -> Vec<String> {
        self.devices.clone()
    }

    async fn send_command(
        &self,
        _device_id: &str,
        _command_name: &str,
        _payload: String,
        _topic: Option<String>,
    ) -> AdapterResult<()> {
        // Mock adapter always succeeds
        Ok(())
    }

    fn connection_status(&self) -> ConnectionStatus {
        if self.is_running() {
            ConnectionStatus::Connected
        } else {
            ConnectionStatus::Disconnected
        }
    }

    async fn subscribe_device(&self, _device_id: &str) -> AdapterResult<()> {
        // Mock adapter always succeeds
        Ok(())
    }

    async fn unsubscribe_device(&self, _device_id: &str) -> AdapterResult<()> {
        // Mock adapter always succeeds
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_status_display() {
        assert_eq!(ConnectionStatus::Connected.to_string(), "connected");
        assert_eq!(ConnectionStatus::Disconnected.to_string(), "disconnected");
    }

    #[test]
    fn test_discovered_device_info() {
        let device = DiscoveredDeviceInfo::new("sensor1", "temperature_sensor")
            .with_name("Temp Sensor 1")
            .with_endpoint("mqtt:/sensor1");

        assert_eq!(device.device_id, "sensor1");
        assert_eq!(device.device_type, "temperature_sensor");
        assert_eq!(device.name.as_deref(), Some("Temp Sensor 1"));
    }

    #[test]
    fn test_adapter_config() {
        let config = AdapterConfig::new("mqtt", "main_mqtt")
            .with_auto_start(false)
            .with_params(serde_json::json!({"host": "localhost"}));

        assert_eq!(config.adapter_type, "mqtt");
        assert_eq!(config.name, "main_mqtt");
        assert!(!config.auto_start);
    }

    #[test]
    fn test_device_event_device_id() {
        let event = DeviceEvent::Metric {
            device_id: "test".to_string(),
            metric: "temp".to_string(),
            value: MetricValue::Float(25.0),
            timestamp: 0,
        };

        assert_eq!(event.device_id(), Some("test"));
    }

    #[test]
    fn test_device_event_to_neotalk() {
        let event = DeviceEvent::Metric {
            device_id: "sensor1".to_string(),
            metric: "temperature".to_string(),
            value: MetricValue::Float(23.5),
            timestamp: 1234567890,
        };

        let neotalk = event.to_neotalk_event();
        assert!(matches!(neotalk, NeoTalkEvent::DeviceMetric { .. }));
    }

    #[test]
    fn test_device_event_state_to_neotalk() {
        let event = DeviceEvent::State {
            device_id: "sensor1".to_string(),
            old_state: ConnectionStatus::Disconnected,
            new_state: ConnectionStatus::Connected,
            timestamp: 0,
        };

        let neotalk = event.to_neotalk_event();
        assert!(matches!(neotalk, NeoTalkEvent::DeviceOnline { .. }));
    }

    #[tokio::test]
    async fn test_adapter_start_stop() {
        let adapter = MockAdapter::new("test");

        assert!(!adapter.is_running());
        adapter.start().await.unwrap();
        assert!(adapter.is_running());
        adapter.stop().await.unwrap();
        assert!(!adapter.is_running());
    }

    #[tokio::test]
    async fn test_mock_adapter_subscribe() {
        let adapter = MockAdapter::new("test");
        adapter.start().await.unwrap();

        let mut rx = adapter.subscribe();

        adapter
            .publish_event(DeviceEvent::Metric {
                device_id: "test".to_string(),
                metric: "temp".to_string(),
                value: MetricValue::Float(25.0),
                timestamp: 0,
            })
            .unwrap();

        let event = rx.next().await;
        assert!(event.is_some());
    }
}
