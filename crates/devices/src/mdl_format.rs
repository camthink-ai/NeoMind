//! Machine Description Language (MDL) - Device Type Definition
//!
//! MDL is a JSON-based format for describing device types in the NeoTalk IoT platform.
//!
//! ## MDL Structure
//!
//! ```json
//! {
//!   "device_type": "dht22_sensor",
//!   "name": "DHT22 温湿度传感器",
//!   "description": "基于 DHT22 的温湿度传感器",
//!   "categories": ["sensor", "climate"],
//!
//!   "uplink": {
//!     "metrics": [
//!       {
//!         "name": "temperature",
//!         "display_name": "温度",
//!         "data_type": "Float",
//!         "unit": "°C",
//!         "min": -40,
//!         "max": 80
//!       },
//!       {
//!         "name": "humidity",
//!         "display_name": "湿度",
//!         "data_type": "Float",
//!         "unit": "%",
//!         "min": 0,
//!         "max": 100
//!       }
//!     ]
//!   },
//!
//!   "downlink": {
//!     "commands": [
//!       {
//!         "name": "set_interval",
//!         "display_name": "设置上报间隔",
//!         "payload_template": "{\"action\": \"set_interval\", \"interval\": \"${{interval}}\"}",
//!         "parameters": [
//!           {
//!             "name": "interval",
//!             "display_name": "间隔时间",
//!             "data_type": "Integer",
//!             "default_value": 60,
//!             "min": 10,
//!             "max": 3600,
//!             "unit": "秒"
//!           }
//!         ]
//!       }
//!     ]
//!   }
//! }
//! ```
//!
//! ## MQTT Topic Format
//!
//! Topics are auto-generated based on device type and device ID:
//! - **Uplink** (device → system): `device/{device_type}/${device_id}/uplink`
//! - **Downlink** (system → device): `device/{device_type}/${device_id}/downlink`
//!
//! For example, a device with type `dht22_sensor` and ID `sensor_001` will:
//! - Publish to: `device/dht22_sensor/sensor_001/uplink`
//! - Receive commands on: `device/dht22_sensor/sensor_001/downlink`

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::RwLock;
use redb::ReadableTable;

use super::mdl::{MetricDataType, MetricValue, DeviceError};

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

    /// Uplink configuration (device -> system)
    #[serde(default)]
    pub uplink: UplinkConfig,

    /// Downlink configuration (system -> device)
    #[serde(default)]
    pub downlink: DownlinkConfig,
}

/// Uplink configuration - data flowing from device to system
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UplinkConfig {
    /// Metrics that this device publishes
    #[serde(default)]
    pub metrics: Vec<MetricDefinition>,
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

    /// Command parameters
    #[serde(default)]
    pub parameters: Vec<ParameterDefinition>,
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
}

/// Extract a value from JSON using dot notation path (e.g., "values.temperature")
fn extract_json_value(json: &serde_json::Value, path: &str) -> Result<serde_json::Value, DeviceError> {
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
                            index, arr.len()
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
fn json_value_to_metric(json: serde_json::Value, expected_type: &MetricDataType) -> Result<MetricValue, DeviceError> {
    match (json, expected_type) {
        // Number types
        (serde_json::Value::Number(n), MetricDataType::Integer) => {
            if let Some(i) = n.as_i64() {
                Ok(MetricValue::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(MetricValue::Integer(f as i64))
            } else {
                Err(DeviceError::InvalidParameter("Number out of range for Integer".into()))
            }
        }
        (serde_json::Value::Number(n), MetricDataType::Float) => {
            if let Some(f) = n.as_f64() {
                Ok(MetricValue::Float(f))
            } else {
                Err(DeviceError::InvalidParameter("Number cannot be converted to Float".into()))
            }
        }

        // Boolean type
        (serde_json::Value::Bool(b), MetricDataType::Boolean) => {
            Ok(MetricValue::Boolean(b))
        }

        // String type (also handles fallback for other types)
        (serde_json::Value::String(s), MetricDataType::String) => {
            Ok(MetricValue::String(s))
        }

        // Null type
        (serde_json::Value::Null, _) => {
            Ok(MetricValue::Null)
        }

        // Type coercion: try to convert to expected type
        (serde_json::Value::String(s), MetricDataType::Integer) => {
            s.trim().parse::<i64>()
                .map(MetricValue::Integer)
                .map_err(|_| DeviceError::InvalidParameter(format!("Cannot convert string '{}' to Integer", s)))
        }
        (serde_json::Value::String(s), MetricDataType::Float) => {
            s.trim().parse::<f64>()
                .map(MetricValue::Float)
                .map_err(|_| DeviceError::InvalidParameter(format!("Cannot convert string '{}' to Float", s)))
        }
        (serde_json::Value::String(s), MetricDataType::Boolean) => {
            let lower = s.to_lowercase();
            match lower.as_str() {
                "true" | "1" | "yes" | "on" => Ok(MetricValue::Boolean(true)),
                "false" | "0" | "no" | "off" => Ok(MetricValue::Boolean(false)),
                _ => Err(DeviceError::InvalidParameter(format!("Cannot convert string '{}' to Boolean", s))),
            }
        }

        // Number to boolean conversion
        (serde_json::Value::Number(n), MetricDataType::Boolean) => {
            if let Some(i) = n.as_i64() {
                Ok(MetricValue::Boolean(i != 0))
            } else if let Some(f) = n.as_f64() {
                Ok(MetricValue::Boolean(f != 0.0))
            } else {
                Err(DeviceError::InvalidParameter("Number cannot be converted to Boolean".into()))
            }
        }

        // Any value to string conversion
        (v, MetricDataType::String) => {
            Ok(MetricValue::String(v.to_string()))
        }

        // Type mismatch
        (v, expected) => {
            Err(DeviceError::InvalidParameter(format!(
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
            )))
        }
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
            return Err(DeviceError::InvalidParameter("device_type cannot be empty".into()));
        }

        // Validate device_type only contains alphanumeric, underscore, hyphen
        if !def.device_type.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
            return Err(DeviceError::InvalidParameter(
                "device_type can only contain alphanumeric, underscore, and hyphen".into()
            ));
        }

        // Validate metric definitions
        for metric in &def.uplink.metrics {
            if metric.name.is_empty() {
                return Err(DeviceError::InvalidParameter("metric name cannot be empty".into()));
            }
        }

        // Validate command definitions
        for command in &def.downlink.commands {
            if command.name.is_empty() {
                return Err(DeviceError::InvalidParameter("command name cannot be empty".into()));
            }
            if command.payload_template.is_empty() {
                return Err(DeviceError::InvalidParameter("command payload_template cannot be empty".into()));
            }
        }

        Ok(())
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
    pub fn parse_metric_value(&self, metric: &MetricDefinition, payload: &[u8]) -> Result<MetricValue, DeviceError> {
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
                let value = payload_str.trim().parse::<f64>()
                    .map_err(|_| DeviceError::InvalidParameter(format!("Invalid float: {}", payload_str)))?;
                Ok(MetricValue::Float(value))
            }
            MetricDataType::Integer => {
                let value = payload_str.trim().parse::<i64>()
                    .map_err(|_| DeviceError::InvalidParameter(format!("Invalid integer: {}", payload_str)))?;
                Ok(MetricValue::Integer(value))
            }
            MetricDataType::Boolean => {
                let value = payload_str.trim().to_lowercase();
                let bool_val = match value.as_str() {
                    "true" | "1" | "on" | "yes" => true,
                    "false" | "0" | "off" | "no" => false,
                    _ => return Err(DeviceError::InvalidParameter(format!("Invalid boolean: {}", payload_str))),
                };
                Ok(MetricValue::Boolean(bool_val))
            }
            MetricDataType::String => {
                Ok(MetricValue::String(payload_str.to_string()))
            }
            MetricDataType::Binary => {
                Ok(MetricValue::Binary(payload.to_vec()))
            }
        }
    }

    /// Build command payload from parameters
    pub fn build_command_payload(&self, command: &CommandDefinition, params: &HashMap<String, MetricValue>) -> Result<Vec<u8>, DeviceError> {
        let mut payload = command.payload_template.clone();

        // Replace ${param} with actual values
        for (key, value) in params {
            let placeholder = format!("${{{{{}}}}}", key);
            let value_str = match value {
                MetricValue::Integer(v) => v.to_string(),
                MetricValue::Float(v) => v.to_string(),
                MetricValue::String(v) => format!("\"{}\"", v),
                MetricValue::Boolean(v) => v.to_string(),
                MetricValue::Binary(_) => return Err(DeviceError::InvalidParameter("Binary values not supported in command payload".into())),
                MetricValue::Null => "null".to_string(),
            };
            payload = payload.replace(&placeholder, &value_str);
        }

        // Validate that all placeholders were replaced
        if payload.contains("${") {
            return Err(DeviceError::InvalidParameter("Not all required parameters were provided".into()));
        }

        // Validate as JSON only if it looks like JSON (starts with { or [)
        // This allows simple string payloads like "ON", "OFF" for HASS devices
        let trimmed = payload.trim();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            serde_json::from_str::<serde_json::Value>(&payload)
                .map_err(|_| DeviceError::InvalidParameter("Generated payload is not valid JSON".into()))?;
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
const MDL_TABLE: redb::TableDefinition<&str, &[u8]> =
    redb::TableDefinition::new("mdl_definitions");

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
            let singleton = MDL_STORAGE_SINGLETON.lock().unwrap();
            if let Some(storage) = singleton.as_ref() {
                if storage.path == path_str {
                    return Ok(storage.clone());
                }
            }
        }

        // Create new storage and save to singleton
        let path_ref = path.as_ref();
        let db = if path_ref.exists() {
            redb::Database::open(path_ref)
                .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?
        } else {
            redb::Database::create(path_ref)
                .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?
        };

        let storage = Arc::new(MdlStorage {
            db,
            path: path_str,
        });

        // Ensure tables exist
        storage.ensure_tables()?;

        *MDL_STORAGE_SINGLETON.lock().unwrap() = Some(storage.clone());
        Ok(storage)
    }

    /// Ensure all required tables exist
    fn ensure_tables(&self) -> Result<(), DeviceError> {
        let write_txn = self.db.begin_write()
            .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        // Create tables if they don't exist (redb creates them on first open_table)
        {
            let _ = write_txn.open_table(MDL_TABLE)
                .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
        }
        {
            let _ = write_txn.open_table(DEVICE_INSTANCES_TABLE)
                .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
        }

        write_txn.commit()
            .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        Ok(())
    }

    /// Create an in-memory MDL storage
    pub fn memory() -> Result<Arc<Self>, DeviceError> {
        let temp_path = std::env::temp_dir()
            .join(format!("mdl_test_{}.redb", uuid::Uuid::new_v4()));
        // Use create for in-memory temp file
        let db = redb::Database::create(&temp_path)
            .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
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
        let value = serde_json::to_vec(def)
            .map_err(|e| DeviceError::Serialization(e.to_string()))?;

        let write_txn = self.db.begin_write()
            .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        {
            let mut table = write_txn.open_table(MDL_TABLE)
                .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
            table.insert(key.as_str(), value.as_slice())
                .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
        }

        write_txn.commit()
            .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        Ok(())
    }

    /// Load a device type definition
    pub async fn load(&self, device_type: &str) -> Result<Option<DeviceTypeDefinition>, DeviceError> {
        let key = self.key(device_type);

        let read_txn = self.db.begin_read()
            .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        let table = read_txn.open_table(MDL_TABLE)
            .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        match table.get(key.as_str()) {
            Ok(Some(value)) => {
                let def: DeviceTypeDefinition = serde_json::from_slice(value.value())
                    .map_err(|e| DeviceError::Serialization(e.to_string()))?;
                Ok(Some(def))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))),
        }
    }

    /// Delete a device type definition
    pub async fn delete(&self, device_type: &str) -> Result<(), DeviceError> {
        let key = self.key(device_type);

        let write_txn = self.db.begin_write()
            .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        {
            let mut table = write_txn.open_table(MDL_TABLE)
                .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
            table.remove(key.as_str())
                .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
        }

        write_txn.commit()
            .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        Ok(())
    }

    /// Load all device type definitions
    pub async fn load_all(&self) -> Result<Vec<DeviceTypeDefinition>, DeviceError> {
        let mut definitions = Vec::new();

        let read_txn = self.db.begin_read()
            .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        let table = read_txn.open_table(MDL_TABLE)
            .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        let mut iter = table.iter()
            .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        while let Some(result) = iter.next() {
            let (_key, value) = result
                .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

            let def: DeviceTypeDefinition = serde_json::from_slice(value.value())
                .map_err(|e| DeviceError::Serialization(e.to_string()))?;

            definitions.push(def);
        }

        Ok(definitions)
    }

    /// List all device type IDs
    pub async fn list_ids(&self) -> Result<Vec<String>, DeviceError> {
        let mut ids = Vec::new();

        let read_txn = self.db.begin_read()
            .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        let table = read_txn.open_table(MDL_TABLE)
            .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        let mut iter = table.iter()
            .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        while let Some(result) = iter.next() {
            let (key, _value) = result
                .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

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
        let value = serde_json::to_vec(instance)
            .map_err(|e| DeviceError::Serialization(e.to_string()))?;

        let write_txn = self.db.begin_write()
            .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        {
            let mut table = write_txn.open_table(DEVICE_INSTANCES_TABLE)
                .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
            table.insert(key.as_str(), value.as_slice())
                .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
        }

        write_txn.commit()
            .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        Ok(())
    }

    /// Load a device instance
    pub async fn load_device_instance(&self, device_id: &str) -> Result<Option<DeviceInstance>, DeviceError> {
        let key = format!("device:{}", device_id);

        let read_txn = self.db.begin_read()
            .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        let table = read_txn.open_table(DEVICE_INSTANCES_TABLE)
            .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        match table.get(key.as_str()) {
            Ok(Some(value)) => {
                let instance: DeviceInstance = serde_json::from_slice(value.value())
                    .map_err(|e| DeviceError::Serialization(e.to_string()))?;
                Ok(Some(instance))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))),
        }
    }

    /// Delete a device instance
    pub async fn delete_device_instance(&self, device_id: &str) -> Result<(), DeviceError> {
        let key = format!("device:{}", device_id);

        let write_txn = self.db.begin_write()
            .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        {
            let mut table = write_txn.open_table(DEVICE_INSTANCES_TABLE)
                .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
            table.remove(key.as_str())
                .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
        }

        write_txn.commit()
            .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        Ok(())
    }

    /// Load all device instances
    pub async fn load_all_device_instances(&self) -> Result<Vec<DeviceInstance>, DeviceError> {
        let mut instances = Vec::new();

        let read_txn = self.db.begin_read()
            .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        let table = read_txn.open_table(DEVICE_INSTANCES_TABLE)
            .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        let mut iter = table.iter()
            .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        while let Some(result) = iter.next() {
            let (_key, value) = result
                .map_err(|e| DeviceError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

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

    /// Adapter/Plugin ID that manages this device (e.g., "hass-discovery", "modbus-1", "external-mqtt-2")
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
                        "data_type": "Float",
                        "unit": "°C"
                    },
                    {
                        "name": "humidity",
                        "display_name": "湿度",
                        "data_type": "Float",
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
                                "data_type": "String",
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
}
