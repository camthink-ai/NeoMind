//! Device registry storage using redb.
//!
//! Provides persistent storage for device type templates and device configurations.

use parking_lot::Mutex;
use std::path::Path;
use std::sync::Arc;

use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

use crate::Error;

// Templates table: key = device_type, value = DeviceTypeTemplate (JSON)
const TEMPLATES_TABLE: TableDefinition<&str, &str> = TableDefinition::new("device_templates");

// Devices table: key = device_id, value = DeviceConfig (JSON)
const DEVICES_TABLE: TableDefinition<&str, &str> = TableDefinition::new("device_configs");

// Type index table: key = device_type, value = comma-separated device_ids
const TYPE_INDEX_TABLE: TableDefinition<&str, &str> = TableDefinition::new("device_type_index");

// Command history table: key = (device_id, command_id), value = CommandHistoryRecord (JSON)
const COMMAND_HISTORY_TABLE: TableDefinition<(&str, &str), &str> =
    TableDefinition::new("command_history");

/// Device type mode: simple (raw data + LLM) or full (structured definitions)
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DeviceTypeMode {
    #[default]
    Simple,
    Full,
}

/// Device type template (simplified version matching neomind_devices::mdl_format::DeviceTypeTemplate)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeviceTypeTemplate {
    pub device_type: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub categories: Vec<String>,
    /// Definition mode
    #[serde(default)]
    pub mode: DeviceTypeMode,
    #[serde(default)]
    pub metrics: Vec<MetricDefinition>,
    /// Sample uplink data for Simple mode
    #[serde(default)]
    pub uplink_samples: Vec<serde_json::Value>,
    #[serde(default)]
    pub commands: Vec<CommandDefinition>,
    /// Suggested offline threshold (seconds) for devices of this type.
    /// `None` = use global default. Forward-compatible via `#[serde(default)]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_offline_timeout_secs: Option<u64>,
    /// If `Some(false)`, skip storing the `_raw` metric (full payload snapshot)
    /// for devices of this type. `None` (default) = inherit the extractor's
    /// `store_raw` config (currently always true). Set `Some(false)` for
    /// large-payload devices (e.g. cameras) whose structured metrics already
    /// cover all fields. Forward-compatible via `#[serde(default)]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store_raw: Option<bool>,
    /// Builtin template version — historical/provenance marker only.
    /// Older NeoMind builds set this when seeding built-in templates; the
    /// seeder is now insert-only (never overwrites existing templates), so
    /// this field drives no behavior. Kept for backward-compatible
    /// deserialization of records written by older installs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin_version: Option<String>,
}

/// Metric definition (matches neomind_devices::mdl_format::MetricDefinition)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricDefinition {
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub data_type: MetricDataType,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub required: bool,
}

/// Metric data type.
/// Accepts both lowercase ("integer") and PascalCase ("Integer") forms.
/// "Array" is mapped to String (stored as JSON string).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum MetricDataType {
    #[serde(alias = "Float")]
    Float,
    #[serde(alias = "Integer")]
    Integer,
    #[serde(alias = "Boolean")]
    Boolean,
    #[serde(alias = "String", alias = "Array")]
    #[default]
    String,
    #[serde(alias = "Binary")]
    Binary,
    Enum {
        options: Vec<String>,
    },
}

/// Command definition (matches neomind_devices::mdl_format::CommandDefinition)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandDefinition {
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub payload_template: String,
    #[serde(default)]
    pub parameters: Vec<ParameterDefinition>,
    /// Fixed values - parameters that are always sent with the same value
    #[serde(default)]
    pub fixed_values: std::collections::HashMap<String, serde_json::Value>,
    /// Sample command payloads (for Simple mode / LLM reference)
    #[serde(default)]
    pub samples: Vec<serde_json::Value>,
    /// Parameter groups for organizing related parameters
    #[serde(default)]
    pub parameter_groups: Vec<ParameterGroup>,
}

/// Parameter definition (matches neomind_devices::mdl_format::ParameterDefinition)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterDefinition {
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub data_type: MetricDataType,
    /// Default value — accepts both tagged format `{"boolean": true}` and bare values `true`, `0`.
    #[serde(default, deserialize_with = "deserialize_default_value")]
    pub default_value: Option<ParamMetricValue>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub unit: String,
    /// Allowed values (for enum types)
    #[serde(default)]
    pub allowed_values: Vec<ParamMetricValue>,
    /// Whether this parameter is required
    #[serde(default)]
    pub required: bool,
    /// Conditional visibility - show this parameter only when condition is met
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
    Pattern {
        regex: String,
        error_message: String,
    },
    /// Range validation for numbers
    Range {
        min: f64,
        max: f64,
        error_message: String,
    },
    /// Length validation for strings/arrays
    Length {
        min: usize,
        max: usize,
        error_message: String,
    },
    /// Custom validation (by name)
    Custom {
        validator: String,
        params: serde_json::Value,
    },
}

/// Parameter group for organizing parameters in the UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterGroup {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub collapsed: bool,
    pub parameters: Vec<String>,
    #[serde(default)]
    pub order: i32,
}

/// Metric value for parameter defaults (renamed to avoid conflict with device_state::MetricValue)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParamMetricValue {
    #[serde(rename = "integer")]
    Integer(i64),
    #[serde(rename = "float")]
    Float(f64),
    #[serde(rename = "string")]
    String(String),
    #[serde(rename = "boolean")]
    Boolean(bool),
    #[serde(rename = "null")]
    Null,
}

/// Custom deserializer for `default_value` that accepts both:
/// - Externally-tagged: `{"boolean": true}` (standard serde format)
/// - Bare JSON values: `true`, `0`, `"hello"` (official DeviceTypes format)
fn deserialize_default_value<'de, D>(deserializer: D) -> Result<Option<ParamMetricValue>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(json_val) => {
            // Try externally-tagged format first: {"boolean": true}
            if let Ok(v) = serde_json::from_value::<ParamMetricValue>(json_val.clone()) {
                return Ok(Some(v));
            }
            // Fallback: interpret as bare JSON value
            Ok(match json_val {
                serde_json::Value::Bool(b) => Some(ParamMetricValue::Boolean(b)),
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        Some(ParamMetricValue::Integer(i))
                    } else {
                        n.as_f64().map(ParamMetricValue::Float)
                    }
                }
                serde_json::Value::String(s) => Some(ParamMetricValue::String(s)),
                serde_json::Value::Null => Some(ParamMetricValue::Null),
                _ => None,
            })
        }
    }
}

/// Device configuration (simplified version matching neomind_devices::registry::DeviceConfig)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeviceConfig {
    pub device_id: String,
    pub name: String,
    pub device_type: String,
    pub adapter_type: String,
    #[serde(default)]
    pub connection_config: ConnectionConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter_id: Option<String>,
    #[serde(default)]
    pub last_seen: i64,
    /// Per-device offline threshold (seconds). None = use global default.
    /// Forward-compatible: old stored devices without this field deserialize
    /// to None via `#[serde(default)]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offline_timeout_secs: Option<u64>,
}

/// Connection configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ConnectionConfig {
    #[serde(rename = "telemetryTopic")]
    pub telemetry_topic: Option<String>,
    #[serde(rename = "commandTopic")]
    pub command_topic: Option<String>,
    #[serde(rename = "jsonPath")]
    pub json_path: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    #[serde(rename = "slaveId")]
    pub slave_id: Option<u8>,
    #[serde(rename = "registerMap")]
    pub register_map: Option<::std::collections::HashMap<String, u16>>,
    #[serde(rename = "entityId")]
    pub entity_id: Option<String>,
    #[serde(flatten)]
    pub extra: ::std::collections::HashMap<String, serde_json::Value>,
}

/// Command history record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandHistoryRecord {
    pub command_id: String,
    pub device_id: String,
    pub command_name: String,
    pub parameters: ::std::collections::HashMap<String, serde_json::Value>,
    pub status: CommandStatus,
    pub result: Option<String>,
    pub error: Option<String>,
    pub created_at: i64,
    pub completed_at: Option<i64>,
}

/// Command status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum CommandStatus {
    Pending,
    Sent,
    Completed,
    Failed,
    Timeout,
}

/// Device registry store using redb.
pub struct DeviceRegistryStore {
    db: Arc<Database>,
    path: String,
}

/// Global device registry store singleton (thread-safe).
static REGISTRY_STORE_SINGLETON: Mutex<Option<Arc<DeviceRegistryStore>>> = Mutex::new(None);

impl DeviceRegistryStore {
    /// Open or create a device registry store at the given path.
    /// Uses a singleton pattern to prevent multiple opens of the same database.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Arc<Self>, Error> {
        let path_str = path.as_ref().to_string_lossy().to_string();

        // Check if we already have a store for this path
        {
            let singleton = REGISTRY_STORE_SINGLETON.lock();
            if let Some(store) = singleton.as_ref() {
                if store.path == path_str {
                    return Ok(store.clone());
                }
            }
        }

        // Create new store
        let path_ref = path.as_ref();
        if let Some(parent) = path_ref.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let is_new = !path_ref.exists();
        let db = if is_new {
            Database::create(path_ref)?
        } else {
            Database::open(path_ref)?
        };
        // Rollback guard: refuse databases stamped by a newer build (see schema.rs).
        crate::schema::check_or_stamp(&db)
            .map_err(|e| Error::Storage(format!("schema version: {e}")))?;

        // Create tables if this is a new database OR verify/create for existing databases
        // This handles cases where a database file exists but tables weren't created properly
        let _tables_created = if is_new {
            let write_txn = db.begin_write()?;
            {
                // Create all tables
                let _templates = write_txn.open_table(TEMPLATES_TABLE)?;
                let _devices = write_txn.open_table(DEVICES_TABLE)?;
                let _type_index = write_txn.open_table(TYPE_INDEX_TABLE)?;
                let _commands = write_txn.open_table(COMMAND_HISTORY_TABLE)?;
            }
            write_txn.commit()?;
            true
        } else {
            // For existing databases, try to open tables to verify they exist
            // If tables don't exist, recreate the database
            match db.begin_read()?.open_table(TEMPLATES_TABLE) {
                Ok(_) => false, // Tables exist, no need to create
                Err(_) => {
                    // Tables don't exist - need to recreate
                    drop(db);
                    std::fs::remove_file(path_ref)?;
                    let new_db = Database::create(path_ref)?;
                    let write_txn = new_db.begin_write()?;
                    {
                        let _templates = write_txn.open_table(TEMPLATES_TABLE)?;
                        let _devices = write_txn.open_table(DEVICES_TABLE)?;
                        let _type_index = write_txn.open_table(TYPE_INDEX_TABLE)?;
                        let _commands = write_txn.open_table(COMMAND_HISTORY_TABLE)?;
                    }
                    write_txn.commit()?;
                    return Ok(Arc::new(DeviceRegistryStore {
                        db: Arc::new(new_db),
                        path: path_str,
                    }));
                }
            }
        };

        let store = Arc::new(DeviceRegistryStore {
            db: Arc::new(db),
            path: path_str,
        });

        *REGISTRY_STORE_SINGLETON.lock() = Some(store.clone());
        Ok(store)
    }

    /// Clear the process-global singleton cache and release the file lock.
    ///
    /// redb only allows one process to hold a database file open at a time.
    /// `open()` caches the store in a global singleton so that repeated opens
    /// within the same process (e.g. ServerState::new() followed by
    /// init_device_storage) don't fight over the lock. Test harnesses that
    /// need to pre-seed a database file and then hand it off to a child
    /// process must call this to drop the cached Arc and release the lock.
    pub fn close_singleton() {
        *REGISTRY_STORE_SINGLETON.lock() = None;
    }

    // ========== Template Management ==========

    /// Save a device type template.
    pub fn save_template(&self, template: &DeviceTypeTemplate) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(TEMPLATES_TABLE)?;
            let json = serde_json::to_string(template)?;
            table.insert(template.device_type.as_str(), json.as_str())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Load a device type template.
    pub fn load_template(&self, device_type: &str) -> Result<Option<DeviceTypeTemplate>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(TEMPLATES_TABLE)?;

        match table.get(device_type)? {
            Some(value) => {
                let json = value.value();
                let template: DeviceTypeTemplate = serde_json::from_str(json)?;
                Ok(Some(template))
            }
            None => Ok(None),
        }
    }

    /// List all device type templates.
    pub fn list_templates(&self) -> Result<Vec<DeviceTypeTemplate>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(TEMPLATES_TABLE)?;

        let mut templates = Vec::new();
        for result in table.iter()? {
            let (_key, value) = result?;
            let json = value.value();
            if let Ok(template) = serde_json::from_str::<DeviceTypeTemplate>(json) {
                templates.push(template);
            }
        }

        Ok(templates)
    }

    /// Delete a device type template.
    pub fn delete_template(&self, device_type: &str) -> Result<bool, Error> {
        let write_txn = self.db.begin_write()?;
        let deleted = {
            let mut table = write_txn.open_table(TEMPLATES_TABLE)?;
            let result = table.remove(device_type)?.is_some();
            result
        };
        write_txn.commit()?;
        Ok(deleted)
    }

    /// Check if a template exists.
    pub fn template_exists(&self, device_type: &str) -> Result<bool, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(TEMPLATES_TABLE)?;
        Ok(table.get(device_type)?.is_some())
    }

    // ========== Device Configuration Management ==========

    /// Save a device configuration.
    pub fn save_device(&self, config: &DeviceConfig) -> Result<(), Error> {
        let device_id = config.device_id.clone();
        let device_type = config.device_type.clone();

        let write_txn = self.db.begin_write()?;
        {
            // Save device config
            let mut devices_table = write_txn.open_table(DEVICES_TABLE)?;
            let json = serde_json::to_string(config)?;
            devices_table.insert(device_id.as_str(), json.as_str())?;

            // Update type index
            let mut index_table = write_txn.open_table(TYPE_INDEX_TABLE)?;
            let key = device_type.as_str();

            // Get existing device IDs for this type
            let mut device_ids: Vec<String> = match index_table.get(key)? {
                Some(value) => value
                    .value()
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect(),
                None => Vec::new(),
            };

            // Add this device if not already present
            if !device_ids.contains(&device_id) {
                device_ids.push(device_id);
            }

            // Save updated index
            let index_value = device_ids.join(",");
            index_table.insert(key, index_value.as_str())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Load a device configuration.
    pub fn load_device(&self, device_id: &str) -> Result<Option<DeviceConfig>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(DEVICES_TABLE)?;

        match table.get(device_id)? {
            Some(value) => {
                let json = value.value();
                let config: DeviceConfig = serde_json::from_str(json)?;
                Ok(Some(config))
            }
            None => Ok(None),
        }
    }

    /// List all device configurations.
    pub fn list_devices(&self) -> Result<Vec<DeviceConfig>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(DEVICES_TABLE)?;

        let mut devices = Vec::new();
        for result in table.iter()? {
            let (_key, value) = result?;
            let json = value.value();
            if let Ok(config) = serde_json::from_str::<DeviceConfig>(json) {
                devices.push(config);
            }
        }

        Ok(devices)
    }

    /// List devices by type.
    pub fn list_devices_by_type(&self, device_type: &str) -> Result<Vec<DeviceConfig>, Error> {
        let read_txn = self.db.begin_read()?;

        // Get device IDs for this type from index
        let device_ids: Vec<String> = {
            let index_table = read_txn.open_table(TYPE_INDEX_TABLE)?;
            match index_table.get(device_type)? {
                Some(value) => value
                    .value()
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect(),
                None => return Ok(Vec::new()),
            }
        };

        if device_ids.is_empty() {
            return Ok(Vec::new());
        }

        // Load device configs
        let devices_table = read_txn.open_table(DEVICES_TABLE)?;
        let mut devices = Vec::new();

        for device_id in device_ids {
            if let Some(value) = devices_table.get(device_id.as_str())? {
                let json = value.value();
                if let Ok(config) = serde_json::from_str::<DeviceConfig>(json) {
                    devices.push(config);
                }
            }
        }

        Ok(devices)
    }

    /// Delete a device configuration.
    pub fn delete_device(&self, device_id: &str) -> Result<Option<String>, Error> {
        let write_txn = self.db.begin_write()?;

        // First get the device to find its type
        let device_type = {
            match write_txn.open_table(DEVICES_TABLE)?.get(device_id)? {
                Some(value) => {
                    let json = value.value().as_bytes();
                    let config: DeviceConfig = serde_json::from_slice(json)?;
                    Some(config.device_type)
                }
                None => None,
            }
        };

        if device_type.is_none() {
            return Ok(None);
        }

        let device_type = device_type.unwrap();

        // Delete device
        {
            let mut devices_table = write_txn.open_table(DEVICES_TABLE)?;
            devices_table.remove(device_id)?;
        }

        // Update type index
        {
            let mut index_table = write_txn.open_table(TYPE_INDEX_TABLE)?;
            let key = device_type.as_str();

            // Read the value first, then drop the borrow
            let device_ids: Vec<String> = match index_table.get(key)? {
                Some(value) => value
                    .value()
                    .split(',')
                    .filter(|s| !s.is_empty() && *s != device_id)
                    .map(String::from)
                    .collect(),
                None => Vec::new(),
            };

            if device_ids.is_empty() {
                index_table.remove(key)?;
            } else {
                index_table.insert(key, device_ids.join(",").as_str())?;
            }
        }

        write_txn.commit()?;
        Ok(Some(device_type))
    }

    /// Update a device configuration.
    pub fn update_device(&self, device_id: &str, config: &DeviceConfig) -> Result<(), Error> {
        // Ensure device_id matches
        if config.device_id != device_id {
            return Err(Error::InvalidInput("device_id mismatch".to_string()));
        }

        // Check if device exists to handle type change
        let old_device_type = self.load_device(device_id)?.map(|d| d.device_type);

        let new_device_type = config.device_type.clone();

        let write_txn = self.db.begin_write()?;
        {
            // Update device config
            let mut devices_table = write_txn.open_table(DEVICES_TABLE)?;
            let json = serde_json::to_string(config)?;
            devices_table.insert(device_id, json.as_str())?;

            // Update type index if type changed
            if let Some(old_type) = old_device_type {
                if old_type != new_device_type {
                    let mut index_table = write_txn.open_table(TYPE_INDEX_TABLE)?;

                    // Remove from old type
                    let old_type_key = old_type.as_str();
                    let old_device_ids: Vec<String> = match index_table.get(old_type_key)? {
                        Some(value) => value
                            .value()
                            .split(',')
                            .filter(|s| !s.is_empty() && *s != device_id)
                            .map(String::from)
                            .collect(),
                        None => Vec::new(),
                    };

                    if old_device_ids.is_empty() {
                        index_table.remove(old_type_key)?;
                    } else {
                        index_table.insert(old_type_key, old_device_ids.join(",").as_str())?;
                    }

                    // Add to new type
                    let key = new_device_type.as_str();
                    let mut device_ids: Vec<String> = match index_table.get(key)? {
                        Some(value) => value
                            .value()
                            .split(',')
                            .filter(|s| !s.is_empty())
                            .map(String::from)
                            .collect(),
                        None => Vec::new(),
                    };

                    if !device_ids.contains(&device_id.to_string()) {
                        device_ids.push(device_id.to_string());
                    }

                    index_table.insert(key, device_ids.join(",").as_str())?;
                }
            }
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Update only the `last_seen` field for a device. Lightweight alternative to `update_device`.
    pub fn update_last_seen(&self, device_id: &str, last_seen: i64) -> Result<(), Error> {
        let mut config = self
            .load_device(device_id)?
            .ok_or_else(|| Error::NotFound(device_id.to_string()))?;
        config.last_seen = last_seen;
        let json = serde_json::to_string(&config)?;
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(DEVICES_TABLE)?;
            table.insert(device_id, json.as_str())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Check if a device exists.
    pub fn device_exists(&self, device_id: &str) -> Result<bool, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(DEVICES_TABLE)?;
        Ok(table.get(device_id)?.is_some())
    }

    /// Get device count.
    pub fn device_count(&self) -> Result<usize, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(DEVICES_TABLE)?;
        Ok(table.iter()?.count())
    }

    /// Get template count.
    pub fn template_count(&self) -> Result<usize, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(TEMPLATES_TABLE)?;
        Ok(table.iter()?.count())
    }

    // ========== Command History Management ==========

    /// Save a command history record.
    pub fn save_command(&self, record: &CommandHistoryRecord) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(COMMAND_HISTORY_TABLE)?;
            let json = serde_json::to_string(record)?;
            table.insert(
                (record.device_id.as_str(), record.command_id.as_str()),
                json.as_str(),
            )?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Load a command history record.
    pub fn load_command(
        &self,
        device_id: &str,
        command_id: &str,
    ) -> Result<Option<CommandHistoryRecord>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(COMMAND_HISTORY_TABLE)?;

        match table.get((device_id, command_id))? {
            Some(value) => {
                let json = value.value();
                let record: CommandHistoryRecord = serde_json::from_str(json)?;
                Ok(Some(record))
            }
            None => Ok(None),
        }
    }

    /// List command history for a device.
    pub fn list_commands(
        &self,
        device_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<CommandHistoryRecord>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(COMMAND_HISTORY_TABLE)?;

        let mut commands = Vec::new();
        let start_key = (device_id, "");
        let end_key = (device_id, "\x7F");

        for result in table
            .range(start_key..=end_key)?
            .take(limit.unwrap_or(usize::MAX))
        {
            let (_key, value) = result?;
            let json = value.value();
            if let Ok(record) = serde_json::from_str::<CommandHistoryRecord>(json) {
                commands.push(record);
            }
        }

        // Sort by created_at descending (newest first)
        commands.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Ok(commands)
    }

    /// List all command history records.
    pub fn list_all_commands(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<CommandHistoryRecord>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(COMMAND_HISTORY_TABLE)?;

        let mut commands = Vec::new();
        for result in table.iter()?.take(limit.unwrap_or(usize::MAX)) {
            let (_key, value) = result?;
            let json = value.value();
            if let Ok(record) = serde_json::from_str::<CommandHistoryRecord>(json) {
                commands.push(record);
            }
        }

        // Sort by created_at descending (newest first)
        commands.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Ok(commands)
    }

    // ========== Builtin Templates ==========

    /// Seed built-in device type templates (NE101, NE301, etc.).
    ///
    /// Insert-only: a template is written **only when it does not yet exist**.
    /// Existing templates are never overwritten — software upgrades must not
    /// clobber a user's local device types (whether from a prior seed, a cloud
    /// import, or an edit). To update a type to the latest official definition,
    /// the user re-imports it from the cloud repo, which overwrites.
    pub fn seed_builtin_templates(&self) -> Result<usize, Error> {
        let templates: &[(&str, &str)] = &[
            (
                "ne101_camera",
                include_str!("builtin_types/ne101_camera.json"),
            ),
            (
                "ne301_camera",
                include_str!("builtin_types/ne301_camera.json"),
            ),
        ];

        let mut changed = 0;
        for (device_type, json) in templates {
            // Already exists (prior seed / cloud import / user edit) → never
            // overwrite. Upgrades must not touch local device types.
            if self.load_template(device_type)?.is_some() {
                continue;
            }
            let bundled: DeviceTypeTemplate = serde_json::from_str(json).map_err(|e| {
                Error::Serialization(format!("Invalid builtin template {}: {}", device_type, e))
            })?;
            self.save_template(&bundled)?;
            changed += 1;
        }
        Ok(changed)
    }

    // ========== Bulk Operations ==========

    /// Load all templates into memory.
    pub fn load_all_templates(
        &self,
    ) -> Result<::std::collections::HashMap<String, DeviceTypeTemplate>, Error> {
        let templates = self.list_templates()?;
        let mut map = ::std::collections::HashMap::new();
        for template in templates {
            map.insert(template.device_type.clone(), template);
        }
        Ok(map)
    }

    /// Load all devices into memory.
    pub fn load_all_devices(
        &self,
    ) -> Result<::std::collections::HashMap<String, DeviceConfig>, Error> {
        let devices = self.list_devices()?;
        let mut map = ::std::collections::HashMap::new();
        for device in devices {
            map.insert(device.device_id.clone(), device);
        }
        Ok(map)
    }

    /// Load type index into memory.
    pub fn load_type_index(
        &self,
    ) -> Result<::std::collections::HashMap<String, Vec<String>>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(TYPE_INDEX_TABLE)?;

        let mut index = ::std::collections::HashMap::new();
        for result in table.iter()? {
            let (key, value) = result?;
            let device_type = key.value().to_string();
            let device_ids: Vec<String> = value
                .value()
                .split(',')
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect();
            index.insert(device_type, device_ids);
        }

        Ok(index)
    }

    /// Save all data from memory (bulk backup).
    pub fn save_all(
        &self,
        templates: &::std::collections::HashMap<String, DeviceTypeTemplate>,
        devices: &::std::collections::HashMap<String, DeviceConfig>,
        type_index: &::std::collections::HashMap<String, Vec<String>>,
    ) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            // Save templates
            let mut templates_table = write_txn.open_table(TEMPLATES_TABLE)?;
            for template in templates.values() {
                let json = serde_json::to_string(template)?;
                templates_table.insert(template.device_type.as_str(), json.as_str())?;
            }

            // Save devices
            let mut devices_table = write_txn.open_table(DEVICES_TABLE)?;
            for device in devices.values() {
                let json = serde_json::to_string(device)?;
                devices_table.insert(device.device_id.as_str(), json.as_str())?;
            }

            // Save type index
            let mut index_table = write_txn.open_table(TYPE_INDEX_TABLE)?;
            for (device_type, device_ids) in type_index {
                let index_value = device_ids.join(",");
                index_table.insert(device_type.as_str(), index_value.as_str())?;
            }
        }
        write_txn.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn create_temp_store() -> Arc<DeviceRegistryStore> {
        let temp_dir =
            std::env::temp_dir().join(format!("device_registry_test_{}", uuid::Uuid::new_v4()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("devices.redb");
        DeviceRegistryStore::open(&db_path).unwrap()
    }

    #[test]
    fn test_template_crud() {
        let store = create_temp_store();

        let template = DeviceTypeTemplate {
            device_type: "dht22".to_string(),
            name: "DHT22 Sensor".to_string(),
            description: "Temperature and humidity sensor".to_string(),
            categories: vec!["sensor".to_string(), "climate".to_string()],
            mode: DeviceTypeMode::Full,
            metrics: vec![MetricDefinition {
                name: "temperature".to_string(),
                display_name: "Temperature".to_string(),
                data_type: MetricDataType::Float,
                unit: "°C".to_string(),
                min: Some(-40.0),
                max: Some(80.0),
                required: false,
            }],
            uplink_samples: vec![],
            commands: vec![],
            default_offline_timeout_secs: None,
            store_raw: None,
            builtin_version: None,
        };

        store.save_template(&template).unwrap();

        let loaded = store.load_template("dht22").unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.device_type, "dht22");
        assert_eq!(loaded.name, "DHT22 Sensor");
        assert_eq!(loaded.metrics.len(), 1);

        assert!(store.template_exists("dht22").unwrap());

        store.delete_template("dht22").unwrap();
        assert!(!store.template_exists("dht22").unwrap());
    }

    #[test]
    fn test_device_crud() {
        let store = create_temp_store();

        let config = DeviceConfig {
            device_id: "sensor1".to_string(),
            name: "Sensor 1".to_string(),
            device_type: "dht22".to_string(),
            adapter_type: "mqtt".to_string(),
            connection_config: ConnectionConfig {
                telemetry_topic: Some("sensors/sensor1".to_string()),
                ..Default::default()
            },
            adapter_id: Some("main-mqtt".to_string()),
            offline_timeout_secs: None,
            last_seen: 0,
        };

        store.save_device(&config).unwrap();

        let loaded = store.load_device("sensor1").unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.device_id, "sensor1");
        assert_eq!(loaded.name, "Sensor 1");

        assert!(store.device_exists("sensor1").unwrap());

        let deleted = store.delete_device("sensor1").unwrap();
        assert_eq!(deleted, Some("dht22".to_string()));
        assert!(!store.device_exists("sensor1").unwrap());
    }

    #[test]
    fn test_list_devices_by_type() {
        let store = create_temp_store();

        store
            .save_device(&DeviceConfig {
                device_id: "sensor1".to_string(),
                name: "Sensor 1".to_string(),
                device_type: "dht22".to_string(),
                adapter_type: "mqtt".to_string(),
                connection_config: Default::default(),
                adapter_id: None,
                last_seen: 0,
                offline_timeout_secs: None,
            })
            .unwrap();

        store
            .save_device(&DeviceConfig {
                device_id: "sensor2".to_string(),
                name: "Sensor 2".to_string(),
                device_type: "dht22".to_string(),
                adapter_type: "mqtt".to_string(),
                connection_config: Default::default(),
                adapter_id: None,
                last_seen: 0,
                offline_timeout_secs: None,
            })
            .unwrap();

        store
            .save_device(&DeviceConfig {
                device_id: "light1".to_string(),
                name: "Light 1".to_string(),
                device_type: "switch".to_string(),
                adapter_type: "mqtt".to_string(),
                connection_config: Default::default(),
                adapter_id: None,
                last_seen: 0,
                offline_timeout_secs: None,
            })
            .unwrap();

        let dht22_devices = store.list_devices_by_type("dht22").unwrap();
        assert_eq!(dht22_devices.len(), 2);

        let switch_devices = store.list_devices_by_type("switch").unwrap();
        assert_eq!(switch_devices.len(), 1);
    }

    #[test]
    fn test_command_history() {
        let store = create_temp_store();

        let record = CommandHistoryRecord {
            command_id: "cmd1".to_string(),
            device_id: "sensor1".to_string(),
            command_name: "read".to_string(),
            parameters: std::collections::HashMap::new(),
            status: CommandStatus::Completed,
            result: Some("{\"temperature\": 23.5}".to_string()),
            error: None,
            created_at: 1234567890,
            completed_at: Some(1234567895),
        };

        store.save_command(&record).unwrap();

        let loaded = store.load_command("sensor1", "cmd1").unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().status, CommandStatus::Completed);

        let commands = store.list_commands("sensor1", None).unwrap();
        assert_eq!(commands.len(), 1);
    }

    #[test]
    fn test_bulk_operations() {
        let store = create_temp_store();

        let mut templates = std::collections::HashMap::new();
        templates.insert(
            "dht22".to_string(),
            DeviceTypeTemplate {
                device_type: "dht22".to_string(),
                name: "DHT22".to_string(),
                ..Default::default()
            },
        );

        let mut devices = std::collections::HashMap::new();
        devices.insert(
            "sensor1".to_string(),
            DeviceConfig {
                device_id: "sensor1".to_string(),
                name: "Sensor 1".to_string(),
                device_type: "dht22".to_string(),
                adapter_type: "mqtt".to_string(),
                ..Default::default()
            },
        );

        let mut type_index = std::collections::HashMap::new();
        type_index.insert("dht22".to_string(), vec!["sensor1".to_string()]);

        store.save_all(&templates, &devices, &type_index).unwrap();

        assert_eq!(store.template_count().unwrap(), 1);
        assert_eq!(store.device_count().unwrap(), 1);

        let loaded_templates = store.load_all_templates().unwrap();
        assert_eq!(loaded_templates.len(), 1);
        assert!(loaded_templates.contains_key("dht22"));
    }

    #[test]
    fn test_seed_builtin_templates() {
        let store = create_temp_store();

        // First seed should insert NE101 and NE301
        let seeded = store.seed_builtin_templates().unwrap();
        assert_eq!(seeded, 2);

        // Templates should exist
        assert!(store.template_exists("ne101_camera").unwrap());
        assert!(store.template_exists("ne301_camera").unwrap());

        // Verify NE101 content
        let ne101 = store.load_template("ne101_camera").unwrap().unwrap();
        assert_eq!(ne101.device_type, "ne101_camera");
        assert!(ne101.name.contains("NE101") || ne101.name.contains("Sensing"));
        assert!(!ne101.metrics.is_empty());

        // Verify NE301 content
        let ne301 = store.load_template("ne301_camera").unwrap().unwrap();
        assert_eq!(ne301.device_type, "ne301_camera");
        assert!(ne301.name.contains("NE301") || ne301.name.contains("Edge AI"));
        assert!(!ne301.metrics.is_empty());
        assert!(!ne301.commands.is_empty()); // NE301 has capture/sleep commands

        // Second seed is a no-op (insert-only: existing templates are never touched)
        let seeded_again = store.seed_builtin_templates().unwrap();
        assert_eq!(seeded_again, 0);
    }

    #[test]
    fn test_seed_never_overwrites_existing_template() {
        // Software upgrades must not overwrite a user's local device types.
        // Pre-seed NE101 with custom content; the seeder must leave it
        // untouched and only insert the still-absent NE301. Users update a
        // type explicitly via cloud import — never via the builtin seeder.
        let store = create_temp_store();

        // Pre-seed NE101 with custom user content (store_raw unset on purpose)
        let custom = DeviceTypeTemplate {
            device_type: "ne101_camera".to_string(),
            name: "My Custom NE101".to_string(),
            description: "user-modified".to_string(),
            categories: vec![],
            mode: DeviceTypeMode::Full,
            metrics: vec![],
            uplink_samples: vec![],
            commands: vec![],
            default_offline_timeout_secs: None,
            store_raw: None,
            builtin_version: None,
        };
        store.save_template(&custom).unwrap();

        // Seed must NOT overwrite the existing NE101; only insert NE301 fresh
        let seeded = store.seed_builtin_templates().unwrap();
        assert_eq!(seeded, 1, "only NE301 inserted; existing NE101 untouched");

        // Existing NE101 is preserved verbatim (not reset to the bundled one)
        let loaded = store.load_template("ne101_camera").unwrap().unwrap();
        assert_eq!(loaded.name, "My Custom NE101");
        assert_eq!(loaded.store_raw, None);
    }

    #[test]
    fn test_seed_builtin_preserves_user_templates() {
        let store = create_temp_store();

        // Pre-insert a user-created template with the same device_type as a builtin
        let user_template = DeviceTypeTemplate {
            device_type: "ne101_camera".to_string(),
            name: "My Custom Camera".to_string(),
            description: "User-modified".to_string(),
            categories: vec![],
            mode: DeviceTypeMode::Full,
            metrics: vec![],
            uplink_samples: vec![],
            commands: vec![],
            default_offline_timeout_secs: None,
            builtin_version: None, // No version = user-created
            store_raw: None,
        };
        store.save_template(&user_template).unwrap();

        // Seed should NOT overwrite the user template
        let seeded = store.seed_builtin_templates().unwrap();
        assert_eq!(seeded, 1); // Only NE301 was inserted, NE101 was skipped

        // Verify user template is preserved
        let loaded = store.load_template("ne101_camera").unwrap().unwrap();
        assert_eq!(loaded.name, "My Custom Camera");
        assert!(loaded.builtin_version.is_none());
    }
}
