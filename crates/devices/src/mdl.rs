//! Machine Description Language (MDL) - Device Abstraction Layer
//!
//! Provides a unified interface for interacting with various types of devices
//! regardless of their underlying communication protocol.

use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;

use serde::{Deserialize, Serialize, Serializer, Deserializer};
use uuid::Uuid;

// Custom serialization module for binary data as base64
mod metric_value_serde {
    use super::*;

    pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&base64::encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        base64::decode(&s).map_err(serde::de::Error::custom)
    }
}

/// Unique identifier for a device.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId(pub Uuid);

impl DeviceId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn from_string(s: &str) -> Result<Self, uuid::Error> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl Default for DeviceId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for DeviceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Device type classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceType {
    /// Sensor device (reads data only)
    Sensor,
    /// Actuator device (executes commands)
    Actuator,
    /// Controller device (reads and writes, manages other devices)
    Controller,
    /// Gateway device (routes communication between protocols)
    Gateway,
    /// Combined sensor and actuator
    Hybrid,
}

impl std::fmt::Display for DeviceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sensor => write!(f, "sensor"),
            Self::Actuator => write!(f, "actuator"),
            Self::Controller => write!(f, "controller"),
            Self::Gateway => write!(f, "gateway"),
            Self::Hybrid => write!(f, "hybrid"),
        }
    }
}

/// Device capability - what operations a device can perform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeviceCapability {
    /// Read numeric values
    ReadNumeric,
    /// Read string/binary data
    ReadData,
    /// Write numeric values
    WriteNumeric,
    /// Execute commands
    ExecuteCommand,
    /// Stream data continuously
    StreamData,
    /// Historical data access
    ReadHistory,
    /// Custom capability with name
    Custom(String),
}

/// Metric value that can be read from a device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MetricValue {
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
    /// Binary data serialized as base64 string
    #[serde(with = "metric_value_serde")]
    Binary(Vec<u8>),
    Null,
}

impl MetricValue {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Float(v) => Some(*v),
            Self::Integer(v) => Some(*v as f64),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Integer(v) => Some(*v),
            Self::Float(v) => Some(*v as i64),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Boolean(v) => Some(*v),
            _ => None,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Integer(_) => "integer",
            Self::Float(_) => "float",
            Self::String(_) => "string",
            Self::Boolean(_) => "boolean",
            Self::Binary(_) => "binary",
            Self::Null => "null",
        }
    }
}

impl From<i64> for MetricValue {
    fn from(v: i64) -> Self {
        Self::Integer(v)
    }
}

impl From<f64> for MetricValue {
    fn from(v: f64) -> Self {
        Self::Float(v)
    }
}

impl From<String> for MetricValue {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}

impl From<&str> for MetricValue {
    fn from(v: &str) -> Self {
        Self::String(v.to_string())
    }
}

impl From<bool> for MetricValue {
    fn from(v: bool) -> Self {
        Self::Boolean(v)
    }
}

/// Definition of a metric that a device provides.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricDefinition {
    /// Metric identifier
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// Data type
    pub data_type: MetricDataType,
    /// Unit of measurement
    pub unit: Option<String>,
    /// Whether this metric is read-only
    pub read_only: bool,
    /// Minimum value (for numeric types)
    pub min: Option<f64>,
    /// Maximum value (for numeric types)
    pub max: Option<f64>,
}

/// Data type for metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetricDataType {
    Integer,
    Float,
    String,
    Boolean,
    Binary,
}

impl Default for MetricDataType {
    fn default() -> Self {
        Self::String
    }
}

/// Command that can be sent to a device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    /// Command identifier
    pub name: String,
    /// Command parameters
    pub parameters: HashMap<String, CommandParameter>,
    /// Timeout for command execution
    pub timeout_ms: Option<u64>,
}

/// Command parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandParameter {
    /// Parameter value
    pub value: MetricValue,
    /// Optional data type hint
    pub data_type: Option<MetricDataType>,
}

impl Command {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            parameters: HashMap::new(),
            timeout_ms: None,
        }
    }

    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<MetricValue>) -> Self {
        self.parameters.insert(key.into(), CommandParameter {
            value: value.into(),
            data_type: None,
        });
        self
    }

    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }
}

/// Connection status of a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Error,
}

/// Device state information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceState {
    /// Current connection status
    pub status: ConnectionStatus,
    /// Last successful communication timestamp
    pub last_seen: Option<chrono::DateTime<chrono::Utc>>,
    /// Error message if status is Error
    pub error: Option<String>,
}

/// Information about a device (without exposing the full device object).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Unique device identifier
    pub id: DeviceId,
    /// Human-readable device name
    pub name: String,
    /// Device type
    pub device_type: DeviceType,
    /// Protocol/backend type
    pub protocol: String,
    /// Connection status
    pub status: ConnectionStatus,
    /// Device capabilities
    pub capabilities: Vec<DeviceCapability>,
    /// Available metrics
    pub metrics: Vec<MetricDefinition>,
    /// Available commands
    pub commands: Vec<String>,
    /// Device location (optional)
    pub location: Option<String>,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

/// Errors that can occur during device operations.
#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    /// Device not found
    #[error("Device not found: {0}")]
    NotFound(DeviceId),

    /// Generic not found error (for brokers, managers, etc.)
    #[error("Not found: {0}")]
    NotFoundStr(String),

    /// Already exists error
    #[error("Already exists: {0}")]
    AlreadyExists(String),

    /// Device is not connected
    #[error("Device not connected: {0}")]
    NotConnected(DeviceId),

    /// Operation timed out
    #[error("Operation timed out after {0}ms")]
    Timeout(u64),

    /// Communication error
    #[error("Communication error: {0}")]
    Communication(String),

    /// Invalid metric name
    #[error("Invalid metric: {0}")]
    InvalidMetric(String),

    /// Invalid command
    #[error("Invalid command: {0}")]
    InvalidCommand(String),

    /// Invalid parameter value
    #[error("Invalid parameter value: {0}")]
    InvalidParameter(String),

    /// Protocol-specific error
    #[error("Protocol error: {0}")]
    Protocol(String),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),
}

/// Core device trait - all devices must implement this.
#[async_trait]
pub trait Device: Send + Sync {
    /// Get the unique device identifier.
    fn id(&self) -> &DeviceId;

    /// Get the human-readable device name.
    fn name(&self) -> &str;

    /// Get the device type.
    fn device_type(&self) -> DeviceType;

    /// Get the device's capabilities.
    fn capabilities(&self) -> Vec<DeviceCapability> {
        Vec::new()
    }

    /// Get available metric definitions.
    fn metrics(&self) -> Vec<MetricDefinition> {
        Vec::new()
    }

    /// Get available command names.
    fn commands(&self) -> Vec<String> {
        Vec::new()
    }

    /// Read a metric value from the device.
    async fn read_metric(&self, metric: &str) -> Result<MetricValue, DeviceError>;

    /// Read multiple metrics at once.
    async fn read_metrics(&self, metrics: &[&str]) -> Result<HashMap<String, MetricValue>, DeviceError> {
        let mut result = HashMap::new();
        for metric in metrics {
            let value = self.read_metric(metric).await?;
            result.insert(metric.to_string(), value);
        }
        Ok(result)
    }

    /// Write a command to the device.
    async fn write_command(&self, command: &Command) -> Result<Option<MetricValue>, DeviceError>;

    /// Connect to the device.
    async fn connect(&mut self) -> Result<(), DeviceError>;

    /// Disconnect from the device.
    async fn disconnect(&mut self) -> Result<(), DeviceError>;

    /// Check if the device is currently connected.
    async fn is_connected(&self) -> bool;

    /// Get the current device state.
    async fn state(&self) -> DeviceState;

    /// Get device information.
    fn info(&self) -> DeviceInfo;
}

/// Dynamic device wrapper for heterogeneous device storage.
pub type DynDevice = Arc<dyn Device>;

/// Helper trait for downcasting device references.
pub trait DeviceDowncast {
    /// Attempt to downcast to a specific device type.
    fn downcast_ref<T: Device + 'static>(&self) -> Option<&T>;

    /// Attempt to downcast mutably to a specific device type.
    fn downcast_mut<T: Device + 'static>(&mut self) -> Option<&mut T>;
}

/// Convert any Device implementer into a DeviceInfo.
pub fn device_to_info(device: &DynDevice) -> DeviceInfo {
    device.info()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_id() {
        let id1 = DeviceId::new();
        let id2 = DeviceId::new();
        assert_ne!(id1, id2);
        assert_eq!(id1.to_string().len(), 36); // UUID format
    }

    #[test]
    fn test_metric_value_conversions() {
        let int_val = MetricValue::Integer(42);
        assert_eq!(int_val.as_i64(), Some(42));
        assert_eq!(int_val.as_f64(), Some(42.0));

        let float_val = MetricValue::Float(3.14);
        assert_eq!(float_val.as_f64(), Some(3.14));

        let string_val = MetricValue::String("hello".to_string());
        assert_eq!(string_val.as_str(), Some("hello"));

        let bool_val = MetricValue::Boolean(true);
        assert_eq!(bool_val.as_bool(), Some(true));
    }

    #[test]
    fn test_command_builder() {
        let cmd = Command::new("set_speed")
            .with_param("speed", 100)
            .with_param("direction", "forward")
            .with_timeout(5000);

        assert_eq!(cmd.name, "set_speed");
        assert_eq!(cmd.parameters.len(), 2);
        assert_eq!(cmd.timeout_ms, Some(5000));
    }
}
