//! Machine Description Language (MDL) - Device Type Definition
//!
//! MDL is a JSON-based format for describing device types in the NeoTalk IoT platform.
//!
//! ## MDL Structure
//!
//! MDL supports two modes:
//!
//! ### Simple Mode (Raw data + LLM)
//! ```json
//! {
//!   "device_type": "camera",
//!   "name": "AI Camera",
//!   "mode": "simple",
//!   "description": "AI摄像头，支持拍照和休眠",
//!   "uplink": {
//!     "samples": [
//!       {"cmd": "frame", "data": "base64...", "ts": 1234567890},
//!       {"cmd": "status", "ai_enabled": true}
//!     ]
//!   },
//!   "downlink": {
//!     "commands": [
//!       {
//!         "name": "capture",
//!         "display_name": "Capture",
//!         "payload_template": "{\"cmd\": \"capture\", \"request_id\": \"${uuid}\", \"params\": ${params}}",
//!         "samples": [
//!           {"cmd": "capture", "params": {"enable_ai": true}}
//!         ],
//!         "llm_hints": "拍照命令，params.enable_ai 控制是否开启AI识别"
//!       }
//!     ]
//!   }
//! }
//! ```
//!
//! ### Full Mode (Structured templates)
//! ```json
//! {
//!   "device_type": "dht22_sensor",
//!   "mode": "full",
//!   "uplink": {
//!     "metrics": [
//!       {
//!         "name": "temperature",
//!         "display_name": "温度",
//!         "data_type": "Float",
//!         "unit": "°C"
//!       }
//!     ]
//!   },
//!   "downlink": {
//!     "commands": [
//!       {
//!         "name": "set_interval",
//!         "parameters": [
//!           {"name": "interval", "data_type": "Integer", "default_value": 60}
//!         ]
//!       }
//!     ]
//!   }
//! }
//! ```

use redb::ReadableTable;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::RwLock;

use super::mdl::{DeviceError, MetricDataType, MetricValue};

/// MDL Device Type Definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceTypeDefinition {
    /// Unique identifier for this device type (e.g., "dht22_sensor")
    /// Only alphanumeric, underscore, and hyphen allowed
    pub device_type: String,

    /// Human-readable name
    pub name: String,

    /// Description of this device type
    #[serde(default)]
    pub description: String,

    /// Categories for grouping (e.g., ["sensor", "climate"])
    #[serde(default)]
    pub categories: Vec<String>,

    /// Device type mode
    /// - "simple": Raw data storage, LLM interprets
    /// - "full": Structured metrics and parameters
    #[serde(default = "default_device_mode")]
    pub mode: DeviceTypeMode,

    /// Uplink configuration (device -> system)
    #[serde(default)]
    pub uplink: UplinkConfig,

    /// Downlink configuration (system -> device)
    #[serde(default)]
    pub downlink: DownlinkConfig,
}

/// Default device mode is "full" for backward compatibility
fn default_device_mode() -> DeviceTypeMode {
    DeviceTypeMode::Full
}

/// Device type mode
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DeviceTypeMode {
    /// Simple mode: Store raw JSON, LLM interprets
    Simple,
    /// Full mode: Structured metrics and parameters
    Full,
}

/// Uplink configuration - data flowing from device to system
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UplinkConfig {
    /// Metrics that this device publishes (full mode)
    #[serde(default)]
    pub metrics: Vec<MetricDefinition>,

    /// Sample uplink data for LLM reference (simple mode)
    #[serde(default)]
    pub samples: Vec<serde_json::Value>,
}

/// Downlink configuration - commands flowing from system to device
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DownlinkConfig {
    /// Commands that this device accepts
    #[serde(default)]
    pub commands: Vec<CommandDefinition>,
}

/// Metric definition in MDL
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricDefinition {
    /// Metric identifier (unique within device type)
    /// Can use dot notation for nested JSON fields (e.g., "values.temperature")
    pub name: String,

    /// Display name
    #[serde(default)]
    pub display_name: String,

    /// Data type
    #[serde(default)]
    pub data_type: MetricDataType,

    /// Unit of measurement
    #[serde(default)]
    pub unit: String,

    /// Minimum value (for numeric types)
    #[serde(default)]
    pub min: Option<f64>,

    /// Maximum value (for numeric types)
    #[serde(default)]
    pub max: Option<f64>,

    /// Whether this metric is required
    #[serde(default)]
    pub required: bool,
}

/// Command definition in MDL
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandDefinition {
    /// Command identifier
    pub name: String,

    /// Display name
    #[serde(default)]
    pub display_name: String,

    /// Payload template (supports ${param} variables)
    /// This is protocol-specific; use protocol mappings for multi-protocol support.
    #[serde(default)]
    pub payload_template: String,

    /// Command parameters (full mode)
    #[serde(default)]
    pub parameters: Vec<ParameterDefinition>,

    /// Fixed values - parameters that are always sent with the same value
    /// These values are not visible to the user and are automatically included
    /// Example: {"protocol_version": 2, "device_type": "sensor_v1"}
    #[serde(default)]
    pub fixed_values: HashMap<String, serde_json::Value>,

    /// Sample command payloads for LLM reference (simple mode)
    #[serde(default)]
    pub samples: Vec<serde_json::Value>,

    /// LLM hints - natural language description for LLM
    #[serde(default)]
    pub llm_hints: String,

    /// Parameter groups - organizes parameters into collapsible sections
    #[serde(default)]
    pub parameter_groups: Vec<ParameterGroup>,
}

/// Parameter definition for commands
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterDefinition {
    /// Parameter name
    pub name: String,

    /// Display name
    #[serde(default)]
    pub display_name: String,

    /// Data type
    #[serde(default)]
    pub data_type: MetricDataType,

    /// Default value
    #[serde(default)]
    pub default_value: Option<MetricValue>,

    /// Minimum value (for numeric types)
    #[serde(default)]
    pub min: Option<f64>,

    /// Maximum value (for numeric types)
    #[serde(default)]
    pub max: Option<f64>,

    /// Unit
    #[serde(default)]
    pub unit: String,

    /// Allowed values (for enum types)
    #[serde(default)]
    pub allowed_values: Vec<MetricValue>,

    /// Whether this parameter is required
    #[serde(default)]
    pub required: bool,

    /// Conditional visibility - show this parameter only when condition is met
    /// Example: "mode == 'advanced'" or "brightness > 50"
    #[serde(default)]
    pub visible_when: Option<String>,

    /// Parameter group for organizing related parameters
    #[serde(default)]
    pub group: Option<String>,

    /// Help text for this parameter
    #[serde(default)]
    pub help_text: String,

    /// Validation rules
    #[serde(default)]
    pub validation: Vec<ValidationRule>,
}

/// Validation rule for parameter values
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ValidationRule {
    /// Pattern validation for strings (regex)
    Pattern { regex: String, error_message: String },
    /// Range validation for numbers
    Range { min: f64, max: f64, error_message: String },
    /// Length validation for strings/arrays
    Length { min: usize, max: usize, error_message: String },
    /// Custom validation (by name)
    Custom { validator: String, params: serde_json::Value },
}

/// Parameter group for organizing parameters in the UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterGroup {
    /// Group identifier
    pub id: String,

    /// Display name
    pub display_name: String,

    /// Group description
    #[serde(default)]
    pub description: String,

    /// Whether this group is collapsed by default
    #[serde(default)]
    pub collapsed: bool,

    /// Parameter names in this group
    pub parameters: Vec<String>,

    /// Order for this group (lower = higher in the list)
    #[serde(default)]
    pub order: i32,
}

/// Extract a value from JSON using dot notation path (e.g., "values.temperature")
fn extract_json_value(
    json: &serde_json::Value,
    path: &str,
) -> Result<serde_json::Value, DeviceError> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = json;

    for (i, part) in parts.iter().enumerate() {
        match current {
            serde_json::Value::Object(map) => {
                if let Some(value) = map.get(*part) {
                    current = value;
                } else {
                    return Err(DeviceError::InvalidParameter(format!(
                        "Path '{}' not found in JSON (part '{}' at index {})",
                        path, part, i
                    )));
                }
            }
            serde_json::Value::Array(arr) => {
                // Try to parse as array index
                if let Ok(index) = part.parse::<usize>() {
                    if index < arr.len() {
                        current = &arr[index];
                    } else {
                        return Err(DeviceError::InvalidParameter(format!(
                            "Array index {} out of bounds (length: {})",
                            index,
                            arr.len()
                        )));
                    }
                } else {
                    return Err(DeviceError::InvalidParameter(format!(
                        "Cannot use string key '{}' on array at path part '{}'",
                        part, i
                    )));
                }
            }
            _ => {
                return Err(DeviceError::InvalidParameter(format!(
                    "Cannot traverse into primitive value at path part '{}'",
                    part
                )));
            }
        }
    }

    Ok(current.clone())
}

/// Convert a JSON value to MetricValue based on the expected data type
fn json_value_to_metric(
    json: serde_json::Value,
    expected_type: &MetricDataType,
) -> Result<MetricValue, DeviceError> {
    match (json, expected_type) {
        // Number types
        (serde_json::Value::Number(n), MetricDataType::Integer) => {
            if let Some(i) = n.as_i64() {
                Ok(MetricValue::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(MetricValue::Integer(f as i64))
            } else {
                Err(DeviceError::InvalidParameter(
                    "Number out of range for Integer".into(),
                ))
            }
        }
        (serde_json::Value::Number(n), MetricDataType::Float) => {
            if let Some(f) = n.as_f64() {
                Ok(MetricValue::Float(f))
            } else {
                Err(DeviceError::InvalidParameter(
                    "Number cannot be converted to Float".into(),
                ))
            }
        }

        // Boolean type
        (serde_json::Value::Bool(b), MetricDataType::Boolean) => Ok(MetricValue::Boolean(b)),

        // String type (also handles fallback for other types)
        (serde_json::Value::String(s), MetricDataType::String) => Ok(MetricValue::String(s)),

        // Null type
        (serde_json::Value::Null, _) => Ok(MetricValue::Null),

        // Type coercion: try to convert to expected type
        (serde_json::Value::String(s), MetricDataType::Integer) => s
            .trim()
            .parse::<i64>()
            .map(MetricValue::Integer)
            .map_err(|_| {
                DeviceError::InvalidParameter(format!("Cannot convert string '{}' to Integer", s))
            }),
        (serde_json::Value::String(s), MetricDataType::Float) => s
            .trim()
            .parse::<f64>()
            .map(MetricValue::Float)
            .map_err(|_| {
                DeviceError::InvalidParameter(format!("Cannot convert string '{}' to Float", s))
            }),
        (serde_json::Value::String(s), MetricDataType::Boolean) => {
            let lower = s.to_lowercase();
            match lower.as_str() {
                "true" | "1" | "yes" | "on" => Ok(MetricValue::Boolean(true)),
                "false" | "0" | "no" | "off" => Ok(MetricValue::Boolean(false)),
                _ => Err(DeviceError::InvalidParameter(format!(
                    "Cannot convert string '{}' to Boolean",
                    s
                ))),
            }
        }

        // Number to boolean conversion
        (serde_json::Value::Number(n), MetricDataType::Boolean) => {
            if let Some(i) = n.as_i64() {
                Ok(MetricValue::Boolean(i != 0))
            } else if let Some(f) = n.as_f64() {
                Ok(MetricValue::Boolean(f != 0.0))
            } else {
                Err(DeviceError::InvalidParameter(
                    "Number cannot be converted to Boolean".into(),
                ))
            }
        }

        // Array type - convert JSON array to MetricValue::Array
        (serde_json::Value::Array(arr), MetricDataType::Array { .. }) => {
            let mut result = Vec::new();
            for item in arr {
                result.push(json_value_to_metric(item, &MetricDataType::String)?);
            }
            Ok(MetricValue::Array(result))
        }

        // Any value to string conversion
        (v, MetricDataType::String) => Ok(MetricValue::String(v.to_string())),

        // Type mismatch
        (v, expected) => Err(DeviceError::InvalidParameter(format!(
            "Type mismatch: expected {:?}, got {}",
            expected,
            match v {
                serde_json::Value::Null => "null",
                serde_json::Value::Bool(_) => "boolean",
                serde_json::Value::Number(_) => "number",
                serde_json::Value::String(_) => "string",
                serde_json::Value::Array(_) => "array",
                serde_json::Value::Object(_) => "object",
            }
        ))),
    }
}

/// MDL Registry - manages device type definitions
pub struct MdlRegistry {
    /// Registered device types indexed by device_type identifier
    device_types: Arc<RwLock<HashMap<String, DeviceTypeDefinition>>>,

    /// Storage backend for persistence (public for device instance persistence)
    pub storage: Arc<RwLock<Option<Arc<MdlStorage>>>>,
}

impl MdlRegistry {
    /// Create a new MDL registry
    pub fn new() -> Self {
        Self {
            device_types: Arc::new(RwLock::new(HashMap::new())),
            storage: Arc::new(RwLock::new(None)),
        }
    }

    /// Initialize with storage
    pub async fn with_storage(self, storage: Arc<MdlStorage>) -> Self {
        *self.storage.write().await = Some(storage);
        self
    }

    /// Set storage backend
    pub async fn set_storage(&self, storage: Arc<MdlStorage>) {
        *self.storage.write().await = Some(storage);
    }

    /// Register a device type definition
    pub async fn register(&self, def: DeviceTypeDefinition) -> Result<(), DeviceError> {
        // Validate the definition
        self.validate(&def)?;

        // Store in memory
        let mut types = self.device_types.write().await;
        types.insert(def.device_type.clone(), def.clone());

        // Persist to storage
        if let Some(storage) = self.storage.read().await.as_ref() {
            storage.save(&def).await?;
        }

        Ok(())
    }

    /// Get a device type definition
    pub async fn get(&self, device_type: &str) -> Option<DeviceTypeDefinition> {
        let types = self.device_types.read().await;
        types.get(device_type).cloned()
    }

    /// List all registered device types
    pub async fn list(&self) -> Vec<DeviceTypeDefinition> {
        let types = self.device_types.read().await;
        types.values().cloned().collect()
    }

    /// Unregister a device type
    pub async fn unregister(&self, device_type: &str) -> Result<(), DeviceError> {
        let mut types = self.device_types.write().await;

        if !types.contains_key(device_type) {
            return Err(DeviceError::NotFound(super::mdl::DeviceId::new()));
        }

        types.remove(device_type);

        // Remove from storage
        if let Some(storage) = self.storage.read().await.as_ref() {
            storage.delete(device_type).await?;
        }

        Ok(())
    }

    /// Load all definitions from storage
    pub async fn load_from_storage(&self) -> Result<(), DeviceError> {
        let storage_guard = self.storage.read().await;
        if let Some(storage) = storage_guard.as_ref() {
            let definitions = storage.load_all().await?;
            let mut types = self.device_types.write().await;

            for def in definitions {
                types.insert(def.device_type.clone(), def);
            }
        }
        Ok(())
    }

    /// Validate a device type definition
    fn validate(&self, def: &DeviceTypeDefinition) -> Result<(), DeviceError> {
        // Check device_type format
        if def.device_type.is_empty() {
            return Err(DeviceError::InvalidParameter(
                "device_type cannot be empty".into(),
            ));
        }

        // Validate device_type only contains alphanumeric, underscore, hyphen
        if !def
            .device_type
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(DeviceError::InvalidParameter(
                "device_type can only contain alphanumeric, underscore, and hyphen".into(),
            ));
        }

        // Validate metric definitions
        for metric in &def.uplink.metrics {
            if metric.name.is_empty() {
                return Err(DeviceError::InvalidParameter(
                    "metric name cannot be empty".into(),
                ));
            }
            // Validate min/max for numeric types
            if matches!(
                metric.data_type,
                MetricDataType::Integer | MetricDataType::Float
            )
                && let (Some(min), Some(max)) = (metric.min, metric.max)
                    && min > max {
                        return Err(DeviceError::InvalidParameter(format!(
                            "metric '{}': min ({}) cannot be greater than max ({})",
                            metric.name, min, max
                        )));
                    }
        }

        // Validate command definitions
        for command in &def.downlink.commands {
            if command.name.is_empty() {
                return Err(DeviceError::InvalidParameter(
                    "command name cannot be empty".into(),
                ));
            }
            if command.payload_template.is_empty() {
                return Err(DeviceError::InvalidParameter(
                    "command payload_template cannot be empty".into(),
                ));
            }

            // Validate parameters
            for param in &command.parameters {
                if param.name.is_empty() {
                    return Err(DeviceError::InvalidParameter(format!(
                        "command '{}': parameter name cannot be empty",
                        command.name
                    )));
                }

                // Validate data_type-specific constraints
                match &param.data_type {
                    MetricDataType::Integer | MetricDataType::Float => {
                        if let (Some(min), Some(max)) = (param.min, param.max)
                            && min > max {
                                return Err(DeviceError::InvalidParameter(format!(
                                    "command '{}', parameter '{}': min ({}) cannot be greater than max ({})",
                                    command.name, param.name, min, max
                                )));
                            }
                    }
                    MetricDataType::Enum { options } => {
                        if !param.allowed_values.is_empty() && !options.is_empty() {
                            // Check that allowed_values match the enum options
                            for allowed in &param.allowed_values {
                                let allowed_str = match allowed {
                                    MetricValue::String(s) => s.as_str(),
                                    _ => continue,
                                };
                                if !options.iter().any(|v| v == allowed_str) {
                                    return Err(DeviceError::InvalidParameter(format!(
                                        "command '{}', parameter '{}': allowed_value '{}' is not in enum options",
                                        command.name, param.name, allowed_str
                                    )));
                                }
                            }
                        }
                    }
                    _ => {}
                }

                // Validate default value type matches data_type
                if let Some(default_value) = &param.default_value
                    && !self.validate_metric_value_type(default_value, &param.data_type) {
                        return Err(DeviceError::InvalidParameter(format!(
                            "command '{}', parameter '{}': default value type does not match data_type",
                            command.name, param.name
                        )));
                    }
            }

            // Validate parameter groups reference valid parameters
            for group in &command.parameter_groups {
                for param_name in &group.parameters {
                    if !command.parameters.iter().any(|p| &p.name == param_name) {
                        return Err(DeviceError::InvalidParameter(format!(
                            "command '{}': parameter group '{}' references unknown parameter '{}'",
                            command.name, group.id, param_name
                        )));
                    }
                }
            }
        }

        Ok(())
    }

    /// Validate that a MetricValue matches the expected MetricDataType
    fn validate_metric_value_type(&self, value: &MetricValue, expected_type: &MetricDataType) -> bool {
        match (value, expected_type) {
            (MetricValue::Integer(_), MetricDataType::Integer) => true,
            (MetricValue::Float(_), MetricDataType::Float) => true,
            (MetricValue::String(_), MetricDataType::String) => true,
            (MetricValue::Boolean(_), MetricDataType::Boolean) => true,
            (MetricValue::Binary(_), MetricDataType::Binary) => true,
            (MetricValue::Array(_), MetricDataType::Array { .. }) => true,
            (MetricValue::String(_), MetricDataType::Enum { .. }) => true,
            // Allow numeric coercion
            (MetricValue::Integer(_), MetricDataType::Float) => true,
            (MetricValue::Float(_), MetricDataType::Integer) => true,
            _ => false,
        }
    }

    /// Get the uplink topic for a device
    pub fn uplink_topic(&self, device_type: &str, device_id: &str) -> String {
        format!("device/{}/{}/uplink", device_type, device_id)
    }

    /// Get the downlink topic for a device
    pub fn downlink_topic(&self, device_type: &str, device_id: &str) -> String {
        format!("device/{}/{}/downlink", device_type, device_id)
    }

    /// Parse incoming MQTT payload based on metric definition
    /// The metric name can use dot notation to extract nested JSON values (e.g., "values.temperature")
    pub fn parse_metric_value(
        &self,
        metric: &MetricDefinition,
        payload: &[u8],
    ) -> Result<MetricValue, DeviceError> {
        // First, try to parse as JSON
        if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(payload) {
            // Extract value using metric name as path (supports dot notation)
            let extracted = extract_json_value(&json_value, &metric.name)?;

            // Convert extracted JSON value to MetricValue based on data_type
            return json_value_to_metric(extracted, &metric.data_type);
        }

        // Fall back to raw string parsing for non-JSON payloads
        let payload_str = std::str::from_utf8(payload)
            .map_err(|_| DeviceError::InvalidParameter("Invalid UTF-8 payload".into()))?;

        // Try to parse based on data type
        match metric.data_type {
            MetricDataType::Float => {
                let value = payload_str.trim().parse::<f64>().map_err(|_| {
                    DeviceError::InvalidParameter(format!("Invalid float: {}", payload_str))
                })?;
                Ok(MetricValue::Float(value))
            }
            MetricDataType::Integer => {
                let value = payload_str.trim().parse::<i64>().map_err(|_| {
                    DeviceError::InvalidParameter(format!("Invalid integer: {}", payload_str))
                })?;
                Ok(MetricValue::Integer(value))
            }
            MetricDataType::Boolean => {
                let value = payload_str.trim().to_lowercase();
                let bool_val = match value.as_str() {
                    "true" | "1" | "on" | "yes" => true,
                    "false" | "0" | "off" | "no" => false,
                    _ => {
                        return Err(DeviceError::InvalidParameter(format!(
                            "Invalid boolean: {}",
                            payload_str
                        )));
                    }
                };
                Ok(MetricValue::Boolean(bool_val))
            }
            MetricDataType::String => Ok(MetricValue::String(payload_str.to_string())),
            MetricDataType::Binary => Ok(MetricValue::Binary(payload.to_vec())),
            MetricDataType::Array { .. } => {
                // Try to parse as JSON array
                if let Ok(json_val) = serde_json::from_slice::<serde_json::Value>(payload) {
                    if let Some(arr) = json_val.as_array() {
                        let converted: Vec<MetricValue> = arr.iter().map(|v| match v {
                            serde_json::Value::Number(n) => {
                                if let Some(i) = n.as_i64() {
                                    MetricValue::Integer(i)
                                } else {
                                    MetricValue::Float(n.as_f64().unwrap_or(0.0))
                                }
                            }
                            serde_json::Value::String(s) => MetricValue::String(s.clone()),
                            serde_json::Value::Bool(b) => MetricValue::Boolean(*b),
                            _ => MetricValue::Null,
                        }).collect();
                        Ok(MetricValue::Array(converted))
                    } else {
                        Ok(MetricValue::String(payload_str.to_string()))
                    }
                } else {
                    Ok(MetricValue::String(payload_str.to_string()))
                }
            }
            // For Enum types, treat as String
            MetricDataType::Enum { .. } => Ok(MetricValue::String(payload_str.to_string())),
        }
    }

    /// Build command payload from parameters
    pub fn build_command_payload(
        &self,
        command: &CommandDefinition,
        params: &HashMap<String, MetricValue>,
    ) -> Result<Vec<u8>, DeviceError> {
        let mut payload = command.payload_template.clone();

        // First, replace fixed values (these are always included)
        for (key, value) in &command.fixed_values {
            let placeholder = format!("${{{{{}}}}}", key);
            let value_str = match value {
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        i.to_string()
                    } else {
                        n.as_f64().unwrap_or(0.0).to_string()
                    }
                }
                serde_json::Value::String(s) => format!("\"{}\"", s),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Null => "null".to_string(),
                serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                    // For complex types, serialize without extra escaping
                    value.to_string()
                }
            };
            payload = payload.replace(&placeholder, &value_str);
        }

        // Then, replace user-provided parameters
        for (key, value) in params {
            let placeholder = format!("${{{{{}}}}}", key);
            let value_str = match value {
                MetricValue::Integer(v) => v.to_string(),
                MetricValue::Float(v) => v.to_string(),
                MetricValue::String(v) => format!("\"{}\"", v),
                MetricValue::Boolean(v) => v.to_string(),
                MetricValue::Array(arr) => {
                    // Serialize array as JSON
                    serde_json::to_string(arr).map_err(|_| {
                        DeviceError::InvalidParameter("Failed to serialize array value".into())
                    })?
                }
                MetricValue::Binary(_) => {
                    return Err(DeviceError::InvalidParameter(
                        "Binary values not supported in command payload".into(),
                    ));
                }
                MetricValue::Null => "null".to_string(),
            };
            payload = payload.replace(&placeholder, &value_str);
        }

        // Validate that all placeholders were replaced (except those in fixed_values which were already handled)
        if payload.contains("${") {
            return Err(DeviceError::InvalidParameter(
                "Not all required parameters were provided".into(),
            ));
        }

        // Validate as JSON only if it looks like JSON (starts with { or [)
        // This allows simple string payloads like "ON", "OFF" for HASS devices
        let trimmed = payload.trim();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            serde_json::from_str::<serde_json::Value>(&payload).map_err(|_| {
                DeviceError::InvalidParameter("Generated payload is not valid JSON".into())
            })?;
        }

        Ok(payload.into_bytes())
    }
}

impl Default for MdlRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// MDL Storage - handles persistence of device type definitions
///
/// Uses redb for efficient key-value storage.
pub struct MdlStorage {
    db: redb::Database,
    /// Storage path for singleton
    path: String,
}

// Table definition for MDL storage
const MDL_TABLE: redb::TableDefinition<&str, &[u8]> = redb::TableDefinition::new("mdl_definitions");

/// Global MDL storage singleton (thread-safe).
static MDL_STORAGE_SINGLETON: StdMutex<Option<Arc<MdlStorage>>> = StdMutex::new(None);

impl MdlStorage {
    /// Create a new MDL storage at the given path
    /// If the database doesn't exist, it will be created.
    /// If it exists, it will be opened and tables will be created if missing.
    /// Uses a singleton pattern to prevent multiple opens of the same database.
    pub fn open<P: AsRef<std::path::Path>>(path: P) -> Result<Arc<Self>, DeviceError> {
        let path_str = path.as_ref().to_string_lossy().to_string();

        // Check if we already have a storage for this path
        {
            let Ok(singleton) = MDL_STORAGE_SINGLETON.lock() else {
                return Err(DeviceError::Io(std::io::Error::other(
                    "Failed to acquire MDL storage lock".to_string(),
                )));
            };
            if let Some(storage) = singleton.as_ref()
                && storage.path == path_str {
                    return Ok(storage.clone());
                }
        }

        // Create new storage and save to singleton
        let path_ref = path.as_ref();
        let db = if path_ref.exists() {
            redb::Database::open(path_ref).map_err(|e| {
                DeviceError::Io(std::io::Error::other(
                    e.to_string(),
                ))
            })?
        } else {
            redb::Database::create(path_ref).map_err(|e| {
                DeviceError::Io(std::io::Error::other(
                    e.to_string(),
                ))
            })?
        };

        let storage = Arc::new(MdlStorage { db, path: path_str });

        // Ensure tables exist
        storage.ensure_tables()?;

        {
            let Ok(mut singleton) = MDL_STORAGE_SINGLETON.lock() else {
                return Err(DeviceError::Io(std::io::Error::other(
                    "Failed to acquire MDL storage lock".to_string(),
                )));
            };
            *singleton = Some(storage.clone());
        }
        Ok(storage)
    }

    /// Ensure all required tables exist
    fn ensure_tables(&self) -> Result<(), DeviceError> {
        let write_txn = self.db.begin_write().map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        // Create tables if they don't exist (redb creates them on first open_table)
        {
            let _ = write_txn.open_table(MDL_TABLE).map_err(|e| {
                DeviceError::Io(std::io::Error::other(
                    e.to_string(),
                ))
            })?;
        }
        {
            let _ = write_txn.open_table(DEVICE_INSTANCES_TABLE).map_err(|e| {
                DeviceError::Io(std::io::Error::other(
                    e.to_string(),
                ))
            })?;
        }

        write_txn.commit().map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        Ok(())
    }

    /// Create an in-memory MDL storage
    pub fn memory() -> Result<Arc<Self>, DeviceError> {
        let temp_path =
            std::env::temp_dir().join(format!("mdl_test_{}.redb", uuid::Uuid::new_v4()));
        // Use create for in-memory temp file
        let db = redb::Database::create(&temp_path).map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;
        let storage = Arc::new(MdlStorage {
            db,
            path: temp_path.to_string_lossy().to_string(),
        });
        storage.ensure_tables()?;
        Ok(storage)
    }

    /// Key for a device type
    fn key(&self, device_type: &str) -> String {
        format!("mdl:{}", device_type)
    }

    /// Save a device type definition
    pub async fn save(&self, def: &DeviceTypeDefinition) -> Result<(), DeviceError> {
        let key = self.key(&def.device_type);
        let value =
            serde_json::to_vec(def).map_err(|e| DeviceError::Serialization(e.to_string()))?;

        let write_txn = self.db.begin_write().map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        {
            let mut table = write_txn.open_table(MDL_TABLE).map_err(|e| {
                DeviceError::Io(std::io::Error::other(
                    e.to_string(),
                ))
            })?;
            table.insert(key.as_str(), value.as_slice()).map_err(|e| {
                DeviceError::Io(std::io::Error::other(
                    e.to_string(),
                ))
            })?;
        }

        write_txn.commit().map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        Ok(())
    }

    /// Load a device type definition
    pub async fn load(
        &self,
        device_type: &str,
    ) -> Result<Option<DeviceTypeDefinition>, DeviceError> {
        let key = self.key(device_type);

        let read_txn = self.db.begin_read().map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        let table = read_txn.open_table(MDL_TABLE).map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        match table.get(key.as_str()) {
            Ok(Some(value)) => {
                let def: DeviceTypeDefinition = serde_json::from_slice(value.value())
                    .map_err(|e| DeviceError::Serialization(e.to_string()))?;
                Ok(Some(def))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))),
        }
    }

    /// Delete a device type definition
    pub async fn delete(&self, device_type: &str) -> Result<(), DeviceError> {
        let key = self.key(device_type);

        let write_txn = self.db.begin_write().map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        {
            let mut table = write_txn.open_table(MDL_TABLE).map_err(|e| {
                DeviceError::Io(std::io::Error::other(
                    e.to_string(),
                ))
            })?;
            table.remove(key.as_str()).map_err(|e| {
                DeviceError::Io(std::io::Error::other(
                    e.to_string(),
                ))
            })?;
        }

        write_txn.commit().map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        Ok(())
    }

    /// Load all device type definitions
    pub async fn load_all(&self) -> Result<Vec<DeviceTypeDefinition>, DeviceError> {
        let mut definitions = Vec::new();

        let read_txn = self.db.begin_read().map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        let table = read_txn.open_table(MDL_TABLE).map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        let iter = table.iter().map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        for result in iter {
            let (_key, value) = result.map_err(|e| {
                DeviceError::Io(std::io::Error::other(
                    e.to_string(),
                ))
            })?;

            let def: DeviceTypeDefinition = serde_json::from_slice(value.value())
                .map_err(|e| DeviceError::Serialization(e.to_string()))?;

            definitions.push(def);
        }

        Ok(definitions)
    }

    /// List all device type IDs
    pub async fn list_ids(&self) -> Result<Vec<String>, DeviceError> {
        let mut ids = Vec::new();

        let read_txn = self.db.begin_read().map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        let table = read_txn.open_table(MDL_TABLE).map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        let iter = table.iter().map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        for result in iter {
            let (key, _value) = result.map_err(|e| {
                DeviceError::Io(std::io::Error::other(
                    e.to_string(),
                ))
            })?;

            // Extract device type from key (remove "mdl:" prefix)
            let key_str = key.value();
            if let Some(id) = key_str.strip_prefix("mdl:") {
                ids.push(id.to_string());
            }
        }

        Ok(ids)
    }

    /// Save a device instance
    pub async fn save_device_instance(&self, instance: &DeviceInstance) -> Result<(), DeviceError> {
        let key = format!("device:{}", instance.device_id);
        let value =
            serde_json::to_vec(instance).map_err(|e| DeviceError::Serialization(e.to_string()))?;

        let write_txn = self.db.begin_write().map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        {
            let mut table = write_txn.open_table(DEVICE_INSTANCES_TABLE).map_err(|e| {
                DeviceError::Io(std::io::Error::other(
                    e.to_string(),
                ))
            })?;
            table.insert(key.as_str(), value.as_slice()).map_err(|e| {
                DeviceError::Io(std::io::Error::other(
                    e.to_string(),
                ))
            })?;
        }

        write_txn.commit().map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        Ok(())
    }

    /// Load a device instance
    pub async fn load_device_instance(
        &self,
        device_id: &str,
    ) -> Result<Option<DeviceInstance>, DeviceError> {
        let key = format!("device:{}", device_id);

        let read_txn = self.db.begin_read().map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        let table = read_txn.open_table(DEVICE_INSTANCES_TABLE).map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        match table.get(key.as_str()) {
            Ok(Some(value)) => {
                let instance: DeviceInstance = serde_json::from_slice(value.value())
                    .map_err(|e| DeviceError::Serialization(e.to_string()))?;
                Ok(Some(instance))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))),
        }
    }

    /// Delete a device instance
    pub async fn delete_device_instance(&self, device_id: &str) -> Result<(), DeviceError> {
        let key = format!("device:{}", device_id);

        let write_txn = self.db.begin_write().map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        {
            let mut table = write_txn.open_table(DEVICE_INSTANCES_TABLE).map_err(|e| {
                DeviceError::Io(std::io::Error::other(
                    e.to_string(),
                ))
            })?;
            table.remove(key.as_str()).map_err(|e| {
                DeviceError::Io(std::io::Error::other(
                    e.to_string(),
                ))
            })?;
        }

        write_txn.commit().map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        Ok(())
    }

    /// Load all device instances
    pub async fn load_all_device_instances(&self) -> Result<Vec<DeviceInstance>, DeviceError> {
        let mut instances = Vec::new();

        let read_txn = self.db.begin_read().map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        let table = read_txn.open_table(DEVICE_INSTANCES_TABLE).map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        let iter = table.iter().map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        for result in iter {
            let (_key, value) = result.map_err(|e| {
                DeviceError::Io(std::io::Error::other(
                    e.to_string(),
                ))
            })?;

            let instance: DeviceInstance = serde_json::from_slice(value.value())
                .map_err(|e| DeviceError::Serialization(e.to_string()))?;

            instances.push(instance);
        }

        Ok(instances)
    }
}

/// Device instance - created from a device type definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInstance {
    /// Device type this instance belongs to
    pub device_type: String,

    /// Unique device ID
    pub device_id: String,

    /// Device name (optional override)
    pub name: Option<String>,

    /// Connection status
    pub status: ConnectionStatus,

    /// Last seen timestamp
    pub last_seen: chrono::DateTime<chrono::Utc>,

    /// Instance-specific configuration
    pub config: HashMap<String, String>,

    /// Current metric values
    pub current_values: HashMap<String, (MetricValue, chrono::DateTime<chrono::Utc>)>,

    /// Adapter/Plugin ID that manages this device (e.g., "hass-discovery", "external-mqtt-1")
    /// None means manually added via internal MQTT
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter_id: Option<String>,
}

/// Connection status for device instances
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Online,
    Offline,
    Error,
}

impl ConnectionStatus {
    /// Convert to lowercase string for API responses
    pub fn as_str(&self) -> &'static str {
        match self {
            ConnectionStatus::Disconnected => "disconnected",
            ConnectionStatus::Connecting => "connecting",
            ConnectionStatus::Online => "online",
            ConnectionStatus::Offline => "offline",
            ConnectionStatus::Error => "error",
        }
    }
}

// Table definition for device instances storage
const DEVICE_INSTANCES_TABLE: redb::TableDefinition<&str, &[u8]> =
    redb::TableDefinition::new("device_instances");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mdl::MetricDataType;

    #[test]
    fn test_device_type_deserialization() {
        let json = r#"
        {
            "device_type": "dht22_sensor",
            "name": "DHT22 温湿度传感器",
            "description": "基于 DHT22 的温湿度传感器",
            "categories": ["sensor", "climate"],
            "uplink": {
                "metrics": [
                    {
                        "name": "temperature",
                        "display_name": "温度",
                        "data_type": "float",
                        "unit": "°C"
                    },
                    {
                        "name": "humidity",
                        "display_name": "湿度",
                        "data_type": "float",
                        "unit": "%"
                    }
                ]
            },
            "downlink": {
                "commands": []
            }
        }
        "#;

        let def: DeviceTypeDefinition = serde_json::from_str(json).unwrap();
        assert_eq!(def.device_type, "dht22_sensor");
        assert_eq!(def.uplink.metrics.len(), 2);
        assert_eq!(def.uplink.metrics[0].name, "temperature");
        assert_eq!(def.uplink.metrics[1].name, "humidity");
    }

    #[test]
    fn test_topic_generation() {
        let registry = MdlRegistry::new();
        let uplink = registry.uplink_topic("dht22_sensor", "sensor_001");
        let downlink = registry.downlink_topic("dht22_sensor", "sensor_001");
        assert_eq!(uplink, "device/dht22_sensor/sensor_001/uplink");
        assert_eq!(downlink, "device/dht22_sensor/sensor_001/downlink");
    }

    #[test]
    fn test_command_with_parameters() {
        let json = r#"
        {
            "device_type": "smart_switch",
            "name": "智能开关",
            "downlink": {
                "commands": [
                    {
                        "name": "set_state",
                        "display_name": "设置状态",
                        "payload_template": "{\"state\": \"${{state}}\"}",
                        "parameters": [
                            {
                                "name": "state",
                                "display_name": "状态",
                                "data_type": "string",
                                "allowed_values": [{"String": "on"}, {"String": "off"}]
                            }
                        ]
                    }
                ]
            }
        }
        "#;

        let def: DeviceTypeDefinition = serde_json::from_str(json).unwrap();
        assert_eq!(def.device_type, "smart_switch");
        assert_eq!(def.downlink.commands.len(), 1);
        assert_eq!(def.downlink.commands[0].name, "set_state");
        assert_eq!(def.downlink.commands[0].parameters.len(), 1);
    }

    #[test]
    fn test_extract_json_value_nested_2_levels() {
        let json = serde_json::json!({
            "temp": {
                "value": 25.5
            }
        });

        let result = extract_json_value(&json, "temp.value").unwrap();
        assert_eq!(result, serde_json::json!(25.5));
    }

    #[test]
    fn test_extract_json_value_nested_5_levels() {
        let json = serde_json::json!({
            "data": {
                "sensor": {
                    "reading": {
                        "temperature": 22.3
                    }
                }
            }
        });

        let result = extract_json_value(&json, "data.sensor.reading.temperature").unwrap();
        assert_eq!(result, serde_json::json!(22.3));
    }

    #[test]
    fn test_extract_json_value_nested_10_levels() {
        let json = serde_json::json!({
            "l1": {
                "l2": {
                    "l3": {
                        "l4": {
                            "l5": {
                                "l6": {
                                    "l7": {
                                        "l8": {
                                            "l9": {
                                                "l10": 999
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        let result = extract_json_value(&json, "l1.l2.l3.l4.l5.l6.l7.l8.l9.l10").unwrap();
        assert_eq!(result, serde_json::json!(999));
    }

    #[test]
    fn test_extract_json_value_with_array_index() {
        let json = serde_json::json!({
            "values": [
                {"temp": 20},
                {"temp": 25},
                {"temp": 30}
            ]
        });

        let result = extract_json_value(&json, "values.1.temp").unwrap();
        assert_eq!(result, serde_json::json!(25));
    }

    #[test]
    fn test_extract_json_value_mixed_array_and_object() {
        let json = serde_json::json!({
            "data": {
                "readings": [
                    {"value": 10},
                    {"value": 20}
                ]
            }
        });

        let result = extract_json_value(&json, "data.readings.0.value").unwrap();
        assert_eq!(result, serde_json::json!(10));
    }

    #[test]
    fn test_parse_metric_with_nested_path() {
        let registry = MdlRegistry::new();

        let metric = MetricDefinition {
            name: "data.sensor.temperature".to_string(),
            display_name: "Temperature".to_string(),
            data_type: MetricDataType::Float,
            unit: "°C".to_string(),
            min: None,
            max: None,
            required: false,
        };

        let payload = br#"{"data": {"sensor": {"temperature": 23.5}}}"#;
        let result = registry.parse_metric_value(&metric, payload).unwrap();

        assert!(matches!(result, MetricValue::Float(23.5)));
    }
}
