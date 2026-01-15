//! Device registry storage using redb.
//!
//! Provides persistent storage for device type templates and device configurations.

use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

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

/// Device type template (simplified version matching edge_ai_devices::mdl_format::DeviceTypeTemplate)
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

impl Default for DeviceTypeTemplate {
    fn default() -> Self {
        Self {
            device_type: String::new(),
            name: String::new(),
            description: String::new(),
            categories: Vec::new(),
            mode: DeviceTypeMode::default(),
            metrics: Vec::new(),
            uplink_samples: Vec::new(),
            commands: Vec::new(),
        }
    }
}

/// Metric definition (matches edge_ai_devices::mdl_format::MetricDefinition)
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MetricDataType {
    Float,
    Integer,
    Boolean,
    String,
    Binary,
    Enum { options: Vec<String> },
}

/// Command definition (matches edge_ai_devices::mdl_format::CommandDefinition)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandDefinition {
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub payload_template: String,
    #[serde(default)]
    pub parameters: Vec<ParameterDefinition>,
    /// Sample command payloads (for Simple mode / LLM reference)
    #[serde(default)]
    pub samples: Vec<serde_json::Value>,
    /// LLM hints for command usage
    #[serde(default)]
    pub llm_hints: String,
}

/// Parameter definition (matches edge_ai_devices::mdl_format::ParameterDefinition)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterDefinition {
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub data_type: MetricDataType,
    /// Default value (serialized as JSON)
    #[serde(default)]
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
}

impl Default for MetricDataType {
    fn default() -> Self {
        Self::String
    }
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

/// Device configuration (simplified version matching edge_ai_devices::registry::DeviceConfig)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    pub device_id: String,
    pub name: String,
    pub device_type: String,
    pub adapter_type: String,
    #[serde(default)]
    pub connection_config: ConnectionConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter_id: Option<String>,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            device_id: String::new(),
            name: String::new(),
            device_type: String::new(),
            adapter_type: String::new(),
            connection_config: ConnectionConfig::default(),
            adapter_id: None,
        }
    }
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
static REGISTRY_STORE_SINGLETON: StdMutex<Option<Arc<DeviceRegistryStore>>> = StdMutex::new(None);

impl DeviceRegistryStore {
    /// Open or create a device registry store at the given path.
    /// Uses a singleton pattern to prevent multiple opens of the same database.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Arc<Self>, Error> {
        let path_str = path.as_ref().to_string_lossy().to_string();

        // Check if we already have a store for this path
        {
            let singleton = REGISTRY_STORE_SINGLETON.lock().unwrap();
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

        *REGISTRY_STORE_SINGLETON.lock().unwrap() = Some(store.clone());
        Ok(store)
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
            table.remove(device_type)?.is_some()
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
            let devices_table = write_txn.open_table(DEVICES_TABLE)?;
            match devices_table.get(device_id)? {
                Some(value) => {
                    let config: DeviceConfig = serde_json::from_str(value.value())?;
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

    /// Delete a command history record.
    pub fn delete_command(&self, device_id: &str, command_id: &str) -> Result<bool, Error> {
        let write_txn = self.db.begin_write()?;
        let deleted = {
            let mut table = write_txn.open_table(COMMAND_HISTORY_TABLE)?;
            table.remove((device_id, command_id))?.is_some()
        };
        write_txn.commit()?;
        Ok(deleted)
    }

    /// Clear all command history for a device.
    pub fn clear_device_commands(&self, device_id: &str) -> Result<usize, Error> {
        let write_txn = self.db.begin_write()?;
        let count = {
            let mut table = write_txn.open_table(COMMAND_HISTORY_TABLE)?;
            let start_key = (device_id, "");
            let end_key = (device_id, "\x7F");

            // First collect all command IDs to delete
            let mut command_ids = Vec::new();
            {
                let range = table.range(start_key..=end_key)?;
                for result in range {
                    let (key, _) = result?;
                    command_ids.push(key.value().1.to_string());
                }
            }

            // Then delete each one
            let deleted = command_ids.len();
            for command_id in command_ids {
                table.remove((device_id, command_id.as_str()))?;
            }

            deleted
        };
        write_txn.commit()?;
        Ok(count)
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
}
