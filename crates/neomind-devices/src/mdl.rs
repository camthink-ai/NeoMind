//! Machine Description Language (MDL) - Device Abstraction Layer
//!
//! Provides a unified interface for interacting with various types of devices
//! regardless of their underlying communication protocol.

use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

// Custom serialization module for binary data as base64
mod metric_value_serde {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        STANDARD.decode(&s).map_err(serde::de::Error::custom)
    }
}

// Custom deserialization module for MetricDataType
// Handles both string form ("Array") and object form ({ "Array": { "element_type": ... }})
mod metric_data_type_serde {
    use super::*;
    use serde::de::{Error, MapAccess, Visitor};
    use std::fmt;

    /// Helper to parse MetricDataType from a string value
    fn parse_from_str(s: &str) -> Option<MetricDataType> {
        match s.to_lowercase().as_str() {
            "integer" => Some(MetricDataType::Integer),
            "float" => Some(MetricDataType::Float),
            "string" => Some(MetricDataType::String),
            "boolean" => Some(MetricDataType::Boolean),
            "array" => Some(MetricDataType::Array { element_type: None }),
            "binary" => Some(MetricDataType::Binary),
            // "enum" can't be created from string alone - needs options
            _ => None,
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<MetricDataType, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct MetricDataTypeVisitor;

        impl<'de> Visitor<'de> for MetricDataTypeVisitor {
            type Value = MetricDataType;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string or an object representing MetricDataType")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: Error,
            {
                parse_from_str(value).ok_or_else(|| {
                    E::custom(format!(
                        "unknown data type '{}', expected one of: integer, float, string, boolean, array, binary",
                        value
                    ))
                })
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                // Expecting a single key like "Array", "Enum", etc.
                let key = map.next_key::<String>()?.ok_or_else(|| {
                    serde::de::Error::custom("expected a non-empty object for MetricDataType")
                })?;

                let key_lower = key.to_lowercase();

                match key_lower.as_str() {
                    "integer" => Ok(MetricDataType::Integer),
                    "float" => Ok(MetricDataType::Float),
                    "string" => Ok(MetricDataType::String),
                    "boolean" => Ok(MetricDataType::Boolean),
                    "binary" => Ok(MetricDataType::Binary),
                    "array" => {
                        // Parse optional element_type field
                        #[derive(Deserialize)]
                        struct ArrayData {
                            #[serde(default)]
                            element_type: Option<Box<MetricDataType>>,
                        }
                        let data = map.next_value::<ArrayData>()?;
                        Ok(MetricDataType::Array {
                            element_type: data.element_type,
                        })
                    }
                    "enum" => {
                        // Parse required options field
                        #[derive(Deserialize)]
                        struct EnumData {
                            options: Vec<String>,
                        }
                        let data = map.next_value::<EnumData>()?;
                        Ok(MetricDataType::Enum {
                            options: data.options,
                        })
                    }
                    _ => Err(serde::de::Error::custom(format!(
                        "unknown data type '{}'",
                        key
                    ))),
                }
            }
        }

        deserializer.deserialize_any(MetricDataTypeVisitor)
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
    /// Array of values (heterogeneous arrays supported)
    Array(Vec<MetricValue>),
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
            Self::Array(_) => "array",
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
///
/// Supports both lowercase (e.g., "string", "integer") and Title Case (e.g., "String", "Integer")
/// for compatibility with external device type definitions like neomind-device-types.
///
/// The Array variant can be deserialized from either:
/// - A string: "Array" or "array" → Array { element_type: None }
/// - An object: { "Array": { "element_type": "String" } } → Array with specific element type
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum MetricDataType {
    /// Integer type - supports both "integer" and "Integer"
    Integer,
    /// Float type - supports both "float" and "Float"
    Float,
    /// String type - supports both "string" and "String"
    #[default]
    String,
    /// Boolean type - supports both "boolean" and "Boolean"
    Boolean,
    /// Array type (optionally with element type hint)
    /// Can be deserialized from string "Array" or "array"
    /// or object { "Array": { "element_type": ... } }
    Array {
        /// Element type hint (for homogeneous arrays)
        element_type: Option<Box<MetricDataType>>,
    },
    /// Binary type - supports both "binary" and "Binary"
    Binary,
    /// Enum type with fixed set of string options
    /// Must be deserialized from object: { "Enum": { "options": [...] } }
    Enum { options: Vec<String> },
}

impl<'de> Deserialize<'de> for MetricDataType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        metric_data_type_serde::deserialize(deserializer)
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
        self.parameters.insert(
            key.into(),
            CommandParameter {
                value: value.into(),
                data_type: None,
            },
        );
        self
    }

    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }
}

/// Device state information.
// Note: ConnectionStatus is now defined in the adapter module
pub use crate::adapter::ConnectionStatus;

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

    /// Storage error
    #[error("Storage error: {0}")]
    Storage(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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

    #[test]
    fn test_metric_data_type_deserialize_array_string() {
        // Test GitHub JSON format - plain string "Array"
        // This is the format used by neomind-device-types repository
        let json = json!({"name": "detections", "data_type": "Array"});
        let result: serde_json::Value = serde_json::from_str(&json.to_string()).unwrap();

        // Deserialize to MetricDataType
        let data_type: MetricDataType =
            serde_json::from_value(result["data_type"].clone()).unwrap();

        match data_type {
            MetricDataType::Array { element_type } => {
                assert!(
                    element_type.is_none(),
                    "element_type should be None for plain string"
                );
            }
            _ => panic!("Expected Array variant, got {:?}", data_type),
        }
    }

    #[test]
    fn test_metric_data_type_deserialize_lowercase_strings() {
        // Test lowercase variants
        for (s, expected) in [
            ("integer", MetricDataType::Integer),
            ("float", MetricDataType::Float),
            ("string", MetricDataType::String),
            ("boolean", MetricDataType::Boolean),
            ("binary", MetricDataType::Binary),
            ("array", MetricDataType::Array { element_type: None }),
        ] {
            let result: MetricDataType = serde_json::from_str(&format!("\"{}\"", s)).unwrap();
            assert_eq!(result, expected, "Failed for '{}'", s);
        }
    }

    #[test]
    fn test_metric_data_type_deserialize_titlecase_strings() {
        // Test TitleCase variants (for GitHub compatibility)
        for (s, expected) in [
            ("Integer", MetricDataType::Integer),
            ("Float", MetricDataType::Float),
            ("String", MetricDataType::String),
            ("Boolean", MetricDataType::Boolean),
            ("Binary", MetricDataType::Binary),
            ("Array", MetricDataType::Array { element_type: None }),
        ] {
            let result: MetricDataType = serde_json::from_str(&format!("\"{}\"", s)).unwrap();
            assert_eq!(result, expected, "Failed for '{}'", s);
        }
    }

    #[test]
    fn test_metric_data_type_deserialize_array_with_element_type() {
        // Test object form with element_type
        let json = r#"{"array": {"element_type": "String"}}"#;
        let result: MetricDataType = serde_json::from_str(json).unwrap();

        match result {
            MetricDataType::Array { element_type } => {
                assert!(element_type.is_some(), "element_type should be Some");
                match *element_type.unwrap() {
                    MetricDataType::String => {}
                    _ => panic!("Expected String element_type"),
                }
            }
            _ => panic!("Expected Array variant"),
        }
    }

    #[test]
    fn test_metric_data_type_deserialize_enum_with_options() {
        // Test Enum variant with options
        let json = r#"{"enum": {"options": ["on", "off", "auto"]}}"#;
        let result: MetricDataType = serde_json::from_str(json).unwrap();

        match result {
            MetricDataType::Enum { options } => {
                assert_eq!(options, vec!["on", "off", "auto"]);
            }
            _ => panic!("Expected Enum variant"),
        }
    }
}
