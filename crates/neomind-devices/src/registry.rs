//! Device Registry - Unified storage for device type templates and device instance configurations
//!
//! This module provides a simplified, unified registry that stores both:
//! - Device type templates (simplified DeviceTypeDefinition)
//! - Device instance configurations (DeviceConfig)
//!
//! ## Persistence
//!
//! The registry supports optional disk persistence using redb storage:
//! ```no_run
//! use neomind_devices::DeviceRegistry;
//! # async fn _doc() -> Result<(), Box<dyn std::error::Error>> {
//! // With persistence (async — requires a runtime)
//! let _registry = DeviceRegistry::with_persistence("./data/devices.redb").await?;
//! // In-memory (synchronous)
//! let _in_memory = DeviceRegistry::new();
//! # Ok(())
//! # }
//! ```

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use super::mdl::DeviceError;
use super::mdl::MetricDataType;
use super::mdl::MetricValue;
use super::mdl_format::{CommandDefinition, MetricDefinition, ParameterDefinition};

// Storage types conversion
use neomind_storage::device_registry::{
    CommandDefinition as StorageCommandDefinition,
    DeviceConfig as StorageConfig,
    DeviceRegistryStore,
    DeviceTypeTemplate as StorageTemplate,
    MetricDataType as StorageMetricDataType,
    // The types below now match, so we can use them directly
    MetricDefinition as StorageMetricDefinition,
    ParamMetricValue as StorageMetricValue,
    ParameterDefinition as StorageParameterDefinition,
    ParameterGroup as StorageParameterGroup,
    ValidationRule as StorageValidationRule,
};

// Conversion function for ValidationRule
fn convert_validation_rule_to_storage(
    vr: &super::mdl_format::ValidationRule,
) -> StorageValidationRule {
    match vr {
        super::mdl_format::ValidationRule::Pattern {
            regex,
            error_message,
        } => StorageValidationRule::Pattern {
            regex: regex.clone(),
            error_message: error_message.clone(),
        },
        super::mdl_format::ValidationRule::Range {
            min,
            max,
            error_message,
        } => StorageValidationRule::Range {
            min: *min,
            max: *max,
            error_message: error_message.clone(),
        },
        super::mdl_format::ValidationRule::Length {
            min,
            max,
            error_message,
        } => StorageValidationRule::Length {
            min: *min,
            max: *max,
            error_message: error_message.clone(),
        },
        super::mdl_format::ValidationRule::Custom { validator, params } => {
            StorageValidationRule::Custom {
                validator: validator.clone(),
                params: params.clone(),
            }
        }
    }
}

fn convert_validation_rule_from_storage(
    vr: StorageValidationRule,
) -> super::mdl_format::ValidationRule {
    match vr {
        StorageValidationRule::Pattern {
            regex,
            error_message,
        } => super::mdl_format::ValidationRule::Pattern {
            regex,
            error_message,
        },
        StorageValidationRule::Range {
            min,
            max,
            error_message,
        } => super::mdl_format::ValidationRule::Range {
            min,
            max,
            error_message,
        },
        StorageValidationRule::Length {
            min,
            max,
            error_message,
        } => super::mdl_format::ValidationRule::Length {
            min,
            max,
            error_message,
        },
        StorageValidationRule::Custom { validator, params } => {
            super::mdl_format::ValidationRule::Custom { validator, params }
        }
    }
}

fn convert_parameter_group_to_storage(
    pg: &super::mdl_format::ParameterGroup,
) -> StorageParameterGroup {
    StorageParameterGroup {
        id: pg.id.clone(),
        display_name: pg.display_name.clone(),
        description: pg.description.clone(),
        collapsed: pg.collapsed,
        parameters: pg.parameters.clone(),
        order: pg.order,
    }
}

fn convert_parameter_group_from_storage(
    pg: StorageParameterGroup,
) -> super::mdl_format::ParameterGroup {
    super::mdl_format::ParameterGroup {
        id: pg.id,
        display_name: pg.display_name,
        description: pg.description,
        collapsed: pg.collapsed,
        parameters: pg.parameters,
        order: pg.order,
    }
}

/// Device type mode: simple (raw data + LLM) or full (structured definitions)
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DeviceTypeMode {
    #[default]
    #[serde(alias = "Simple")]
    Simple,
    #[serde(alias = "Full")]
    Full,
}

/// Simplified device type definition (template)
/// Removed uplink/downlink separation - directly lists metrics and commands
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceTypeTemplate {
    /// Unique identifier for this device type (e.g., "dht22_sensor")
    pub device_type: String,
    /// Human-readable name
    pub name: String,
    /// Description
    #[serde(default)]
    pub description: String,
    /// Categories for grouping
    #[serde(default)]
    pub categories: Vec<String>,
    /// Definition mode
    #[serde(default)]
    pub mode: DeviceTypeMode,
    /// Metrics that this device provides (simplified - no uplink wrapper)
    #[serde(default)]
    pub metrics: Vec<MetricDefinition>,
    /// Sample uplink data for Simple mode
    #[serde(default)]
    pub uplink_samples: Vec<serde_json::Value>,
    /// Commands that this device accepts (simplified - no downlink wrapper)
    #[serde(default)]
    pub commands: Vec<CommandDefinition>,
    /// Suggested offline threshold (seconds) for devices of this type.
    /// Applied to `DeviceConfig::offline_timeout_secs` when a device is
    /// registered through the template, unless the caller explicitly
    /// overrides it. `None` = use the global default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_offline_timeout_secs: Option<u64>,
    /// If `Some(false)`, skip storing the `_raw` metric (full payload snapshot)
    /// for devices of this type. `None` (default) = inherit the extractor's
    /// `store_raw` config. Set `Some(false)` for large-payload devices (e.g.
    /// cameras) whose structured metrics already cover all fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store_raw: Option<bool>,
    /// Origin marker: `Some(version)` for templates seeded from the built-in
    /// bundle (NE101/NE301/NE302…), `None` for user-created / imported ones.
    /// Informational — seeding is insert-only either way; user edits to a
    /// builtin type are preserved (and keep this marker).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin_version: Option<String>,
    /// Optional CamThink CT-Prov discovery/provision metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ct_prov: Option<neomind_storage::device_registry::CtProvMetadata>,
}

impl DeviceTypeTemplate {
    /// Create a new device type template
    pub fn new(device_type: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            device_type: device_type.into(),
            name: name.into(),
            description: String::new(),
            categories: Vec::new(),
            mode: DeviceTypeMode::Simple,
            metrics: Vec::new(),
            uplink_samples: Vec::new(),
            commands: Vec::new(),
            default_offline_timeout_secs: None,
            store_raw: None,
            builtin_version: None,
            ct_prov: None,
        }
    }

    /// Add a metric to the template
    pub fn with_metric(mut self, metric: MetricDefinition) -> Self {
        self.metrics.push(metric);
        self
    }

    /// Add a command to the template
    pub fn with_command(mut self, command: CommandDefinition) -> Self {
        self.commands.push(command);
        self
    }

    /// Set description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Add a category
    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.categories.push(category.into());
        self
    }
}

/// Device instance configuration
/// Contains only connection information - device capabilities come from the template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    /// Unique device identifier
    pub device_id: String,
    /// Human-readable device name
    pub name: String,
    /// Device type template reference
    pub device_type: String,
    /// Adapter type (mqtt, hass, etc.)
    pub adapter_type: String,
    /// Connection configuration (protocol-specific)
    pub connection_config: ConnectionConfig,
    /// Adapter/Plugin ID that manages this device
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter_id: Option<String>,
    /// Last seen timestamp (persisted, 0 = never connected)
    #[serde(default)]
    pub last_seen: i64,
    /// Per-device offline threshold (seconds). When `None`, falls back to
    /// the global `HeartbeatConfig::offline_timeout`. Use `Some(n)` for
    /// low-frequency devices (battery sensors, LoRaWAN, NB-IoT) so they
    /// don't flicker to "Offline" between reports. Honored by
    /// `effective_offline_timeout()` in `service.rs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offline_timeout_secs: Option<u64>,
}

/// Unified connection configuration for different protocols
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ConnectionConfig {
    // MQTT-specific
    /// Telemetry topic (data from device)
    pub telemetry_topic: Option<String>,
    /// Command topic (commands to device)
    pub command_topic: Option<String>,
    /// JSON path for extracting values (optional)
    pub json_path: Option<String>,

    // HASS-specific
    /// Home Assistant entity ID
    pub entity_id: Option<String>,

    // Generic metadata
    /// Additional protocol-specific parameters
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl ConnectionConfig {
    /// Create a new empty connection config
    pub fn new() -> Self {
        Self::default()
    }

    /// Create MQTT connection config
    pub fn mqtt(
        telemetry_topic: impl Into<String>,
        command_topic: Option<impl Into<String>>,
    ) -> Self {
        Self {
            telemetry_topic: Some(telemetry_topic.into()),
            command_topic: command_topic.map(|t| t.into()),
            ..Default::default()
        }
    }

    /// Create HASS connection config
    pub fn hass(entity_id: impl Into<String>) -> Self {
        Self {
            entity_id: Some(entity_id.into()),
            ..Default::default()
        }
    }
}

// ========== Conversion Functions for Storage ==========

/// Convert storage metric data type to local metric data type
fn convert_metric_data_type(storage_type: StorageMetricDataType) -> MetricDataType {
    match storage_type {
        StorageMetricDataType::Float => MetricDataType::Float,
        StorageMetricDataType::Integer => MetricDataType::Integer,
        StorageMetricDataType::Boolean => MetricDataType::Boolean,
        StorageMetricDataType::String => MetricDataType::String,
        StorageMetricDataType::Binary => MetricDataType::Binary,
        StorageMetricDataType::Enum { options } => MetricDataType::Enum { options },
    }
}

/// Convert local metric data type to storage metric data type
fn convert_metric_data_type_to_storage(local_type: MetricDataType) -> StorageMetricDataType {
    match local_type {
        MetricDataType::Float => StorageMetricDataType::Float,
        MetricDataType::Integer => StorageMetricDataType::Integer,
        MetricDataType::Boolean => StorageMetricDataType::Boolean,
        MetricDataType::String => StorageMetricDataType::String,
        MetricDataType::Binary => StorageMetricDataType::Binary,
        // For Array types, store as String (JSON serialized)
        MetricDataType::Array { .. } => StorageMetricDataType::String,
        MetricDataType::Enum { options } => StorageMetricDataType::Enum { options },
    }
}

/// Convert storage metric value to local metric value
fn convert_metric_value(storage_value: StorageMetricValue) -> MetricValue {
    match storage_value {
        StorageMetricValue::Integer(n) => MetricValue::Integer(n),
        StorageMetricValue::Float(f) => MetricValue::Float(f),
        StorageMetricValue::String(s) => MetricValue::String(s),
        StorageMetricValue::Boolean(b) => MetricValue::Boolean(b),
        StorageMetricValue::Null => MetricValue::Null,
    }
}

/// Convert local metric value to storage metric value
fn convert_metric_value_to_storage(local_value: MetricValue) -> Option<StorageMetricValue> {
    match local_value {
        MetricValue::Integer(n) => Some(StorageMetricValue::Integer(n)),
        MetricValue::Float(f) => Some(StorageMetricValue::Float(f)),
        MetricValue::String(s) => Some(StorageMetricValue::String(s)),
        MetricValue::Boolean(b) => Some(StorageMetricValue::Boolean(b)),
        MetricValue::Array(_) => None, // Array not supported for parameter defaults
        MetricValue::Null => Some(StorageMetricValue::Null),
        MetricValue::Binary(_) => None, // Binary not supported for parameter defaults
    }
}

/// Convert storage connection config to local connection config
fn convert_connection_config(
    storage_config: neomind_storage::device_registry::ConnectionConfig,
) -> ConnectionConfig {
    ConnectionConfig {
        telemetry_topic: storage_config.telemetry_topic,
        command_topic: storage_config.command_topic,
        json_path: storage_config.json_path,
        entity_id: storage_config.entity_id,
        extra: storage_config.extra,
    }
}

/// Convert local connection config to storage connection config
fn convert_connection_config_to_storage(
    local_config: ConnectionConfig,
) -> neomind_storage::device_registry::ConnectionConfig {
    neomind_storage::device_registry::ConnectionConfig {
        telemetry_topic: local_config.telemetry_topic,
        command_topic: local_config.command_topic,
        json_path: local_config.json_path,
        host: None,
        port: None,
        slave_id: None,
        register_map: None,
        entity_id: local_config.entity_id,
        extra: local_config.extra,
    }
}

/// Maximum allowed length for a device identifier.
pub const MAX_DEVICE_ID_LEN: usize = 128;

/// Validate a device identifier against length limits and path/control-char injection.
///
/// Allows common device ID characters such as `[A-Za-z0-9._:-]` while rejecting
/// empty IDs, IDs longer than 128 characters, `..`, `/`, `\`, `\0`, and control characters.
pub fn validate_device_id(device_id: &str) -> Result<(), DeviceError> {
    if device_id.is_empty() {
        return Err(DeviceError::InvalidParameter(
            "device_id cannot be empty".into(),
        ));
    }
    if device_id.len() > MAX_DEVICE_ID_LEN || device_id.chars().count() > MAX_DEVICE_ID_LEN {
        return Err(DeviceError::InvalidParameter(format!(
            "device_id exceeds maximum length of {} characters",
            MAX_DEVICE_ID_LEN
        )));
    }
    if device_id.contains("..")
        || device_id.contains('/')
        || device_id.contains('\\')
        || device_id.contains('\0')
        || device_id.chars().any(|c| c.is_control())
    {
        return Err(DeviceError::InvalidParameter(
            "device_id contains forbidden characters ('..', '/', '\\', NUL, or control characters are not allowed)".into(),
        ));
    }
    Ok(())
}

/// Unified Device Registry
/// Stores both templates and device configurations with optional persistence
pub struct DeviceRegistry {
    /// Device type templates indexed by device_type
    templates: DashMap<String, DeviceTypeTemplate>,
    /// Device instance configurations indexed by device_id
    devices: DashMap<String, DeviceConfig>,
    /// Index: device_type -> set of device_ids
    type_index: DashMap<String, Vec<String>>,
    /// Reverse index: telemetry_topic → registered device ids (shared-topic safe).
    topic_index: DashMap<String, Vec<String>>,
    /// Optional persistent storage backend
    storage: Option<Arc<DeviceRegistryStore>>,
    /// Whether to auto-save after modifications
    auto_save: AtomicBool,
}

impl Default for DeviceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceRegistry {
    /// Create a new in-memory device registry (no persistence)
    pub fn new() -> Self {
        Self {
            templates: DashMap::new(),
            devices: DashMap::new(),
            type_index: DashMap::new(),
            topic_index: DashMap::new(),
            storage: None,
            auto_save: AtomicBool::new(false),
        }
    }

    /// Create a new device registry with disk persistence
    ///
    /// This will load existing templates and devices from the storage file.
    /// If the file doesn't exist, it will be created.
    pub async fn with_persistence<P: AsRef<Path>>(path: P) -> Result<Self, DeviceError> {
        let store = DeviceRegistryStore::open(path)
            .map_err(|e| DeviceError::Storage(format!("Failed to open storage: {}", e)))?;

        // Seed built-in device type templates (NE101, NE301, ...) BEFORE
        // loading into memory. Without this, a fresh-install server boots
        // with an empty in-memory template cache, and `init_device_storage()`
        // (called after ServerState::new()) writes templates to disk but
        // never refreshes the cache — so API device registration against
        // built-in templates fails with "template not found" until the
        // server is restarted. Seeding here is idempotent
        // (`seed_builtin_templates` is a no-op if templates already exist)
        // and makes the in-memory load pick them up on first boot.
        match store.seed_builtin_templates() {
            Ok(n) if n > 0 => tracing::info!(
                category = "storage",
                seeded = n,
                "Seeded built-in device type templates during registry init"
            ),
            Ok(_) => {}
            Err(e) => tracing::warn!(
                category = "storage",
                error = %e,
                "Failed to seed built-in device type templates during registry init"
            ),
        }

        let registry = Self {
            templates: DashMap::new(),
            devices: DashMap::new(),
            type_index: DashMap::new(),
            topic_index: DashMap::new(),
            storage: Some(store),
            auto_save: AtomicBool::new(true),
        };

        // Load existing data from storage
        registry.load_from_storage().await?;

        Ok(registry)
    }

    /// Load all data from storage into memory
    /// Seed built-in device type templates (NE101, NE301, ...) via this
    /// registry's OWN storage handle — the same store the in-memory cache
    /// reads from. Seeding through a separately-opened `DeviceRegistryStore`
    /// races with the registry's reload: the seed writes one handle, the
    /// reload reads another, and the write may not be visible to the read on
    /// the same boot (seen as intermittent "template not found"). Going
    /// through `self.storage` keeps seed + reload on the same handle.
    pub async fn seed_builtin_templates(&self) -> Result<usize, DeviceError> {
        let Some(store) = &self.storage else {
            return Err(DeviceError::Storage("No storage configured".to_string()));
        };
        store
            .seed_builtin_templates()
            .map_err(|e| DeviceError::Storage(format!("seed_builtin_templates failed: {}", e)))
    }

    pub async fn load_from_storage(&self) -> Result<(), DeviceError> {
        let Some(store) = &self.storage else {
            return Err(DeviceError::Storage("No storage configured".to_string()));
        };

        // Load templates
        let storage_templates = store
            .list_templates()
            .map_err(|e| DeviceError::Storage(format!("Failed to load templates: {}", e)))?;

        for storage_template in storage_templates {
            // Convert storage mode to local mode
            let mode = match storage_template.mode {
                neomind_storage::device_registry::DeviceTypeMode::Simple => DeviceTypeMode::Simple,
                neomind_storage::device_registry::DeviceTypeMode::Full => DeviceTypeMode::Full,
            };

            let template = DeviceTypeTemplate {
                device_type: storage_template.device_type,
                name: storage_template.name,
                description: storage_template.description,
                categories: storage_template.categories,
                mode,
                metrics: storage_template
                    .metrics
                    .into_iter()
                    .map(|m| MetricDefinition {
                        name: m.name,
                        display_name: m.display_name,
                        data_type: convert_metric_data_type(m.data_type),
                        unit: m.unit,
                        min: m.min,
                        max: m.max,
                        required: m.required,
                    })
                    .collect(),
                uplink_samples: storage_template.uplink_samples,
                commands: storage_template
                    .commands
                    .into_iter()
                    .map(|c| CommandDefinition {
                        name: c.name,
                        display_name: c.display_name,
                        payload_template: c.payload_template,
                        fixed_values: c.fixed_values,
                        parameters: c
                            .parameters
                            .into_iter()
                            .map(|p| {
                                let default_value = p.default_value.map(convert_metric_value);
                                let allowed_values = p
                                    .allowed_values
                                    .into_iter()
                                    .map(convert_metric_value)
                                    .collect();
                                ParameterDefinition {
                                    name: p.name,
                                    display_name: p.display_name,
                                    data_type: convert_metric_data_type(p.data_type),
                                    default_value,
                                    min: p.min,
                                    max: p.max,
                                    unit: p.unit,
                                    allowed_values,
                                    required: p.required,
                                    visible_when: p.visible_when,
                                    group: p.group,
                                    help_text: p.help_text,
                                    validation: p
                                        .validation
                                        .into_iter()
                                        .map(convert_validation_rule_from_storage)
                                        .collect(),
                                }
                            })
                            .collect(),
                        samples: c.samples,
                        description: c.description,
                        parameter_groups: c
                            .parameter_groups
                            .into_iter()
                            .map(convert_parameter_group_from_storage)
                            .collect(),
                    })
                    .collect(),
                default_offline_timeout_secs: storage_template.default_offline_timeout_secs,
                store_raw: storage_template.store_raw,
                builtin_version: storage_template.builtin_version,
                ct_prov: storage_template.ct_prov,
            };

            self.templates
                .insert(template.device_type.clone(), template);
        }

        // Load devices
        let storage_devices = store
            .list_devices()
            .map_err(|e| DeviceError::Storage(format!("Failed to load devices: {}", e)))?;

        // Track devices that need last_seen migration (old data without the field)
        let mut migrated_devices = Vec::new();

        for storage_device in storage_devices {
            // Backward compat: old stored data has no last_seen field → defaults to 0.
            // These devices existed before this feature, so they were likely connected.
            // Use sentinel value 1 to mean "was connected before tracking was added".
            let last_seen = if storage_device.last_seen == 0 {
                migrated_devices.push(storage_device.device_id.clone());
                1
            } else {
                storage_device.last_seen
            };

            let config = DeviceConfig {
                device_id: storage_device.device_id,
                name: storage_device.name,
                device_type: storage_device.device_type,
                adapter_type: storage_device.adapter_type,
                connection_config: convert_connection_config(storage_device.connection_config),
                adapter_id: storage_device.adapter_id,
                last_seen,
                offline_timeout_secs: storage_device.offline_timeout_secs,
            };

            let device_id = config.device_id.clone();
            let device_type = config.device_type.clone();
            let telemetry_topic = config.connection_config.telemetry_topic.clone();

            self.devices.insert(device_id.clone(), config);

            self.type_index
                .entry(device_type)
                .or_default()
                .push(device_id.clone());

            if let Some(topic) = telemetry_topic {
                self.topic_index_insert(&topic, &device_id);
            }
        }

        // Persist last_seen migration for old devices
        if !migrated_devices.is_empty() {
            tracing::info!(
                "Migrating last_seen for {} existing devices (backward compat)",
                migrated_devices.len()
            );
            for device_id in &migrated_devices {
                if let Err(e) = store.update_last_seen(device_id, 1) {
                    tracing::warn!("Failed to migrate last_seen for {}: {}", device_id, e);
                }
            }
        }

        tracing::info!(
            "Loaded {} templates and {} devices from storage",
            self.templates.len(),
            self.devices.len()
        );

        Ok(())
    }

    /// Save all data to storage (manual save)
    pub async fn save_to_storage(&self) -> Result<(), DeviceError> {
        let Some(store) = &self.storage else {
            return Err(DeviceError::Storage("No storage configured".to_string()));
        };

        // Convert and save templates
        for entry in self.templates.iter() {
            let template = entry.value();

            // Convert local mode to storage mode
            let mode = match template.mode {
                DeviceTypeMode::Simple => neomind_storage::device_registry::DeviceTypeMode::Simple,
                DeviceTypeMode::Full => neomind_storage::device_registry::DeviceTypeMode::Full,
            };

            let storage_template = StorageTemplate {
                device_type: template.device_type.clone(),
                name: template.name.clone(),
                description: template.description.clone(),
                categories: template.categories.clone(),
                mode,
                metrics: template
                    .metrics
                    .iter()
                    .map(|m| StorageMetricDefinition {
                        name: m.name.clone(),
                        display_name: m.display_name.clone(),
                        data_type: convert_metric_data_type_to_storage(m.data_type.clone()),
                        unit: m.unit.clone(),
                        min: m.min,
                        max: m.max,
                        required: m.required,
                    })
                    .collect(),
                uplink_samples: template.uplink_samples.clone(),
                commands: template
                    .commands
                    .iter()
                    .map(|c| StorageCommandDefinition {
                        name: c.name.clone(),
                        display_name: c.display_name.clone(),
                        payload_template: c.payload_template.clone(),
                        fixed_values: c.fixed_values.clone(),
                        parameters: c
                            .parameters
                            .iter()
                            .map(|p| StorageParameterDefinition {
                                name: p.name.clone(),
                                display_name: p.display_name.clone(),
                                data_type: convert_metric_data_type_to_storage(p.data_type.clone()),
                                default_value: p
                                    .default_value
                                    .as_ref()
                                    .and_then(|v| convert_metric_value_to_storage(v.clone())),
                                min: p.min,
                                max: p.max,
                                unit: p.unit.clone(),
                                allowed_values: p
                                    .allowed_values
                                    .iter()
                                    .filter_map(|v| convert_metric_value_to_storage(v.clone()))
                                    .collect(),
                                required: p.required,
                                visible_when: p.visible_when.clone(),
                                group: p.group.clone(),
                                help_text: p.help_text.clone(),
                                validation: p
                                    .validation
                                    .iter()
                                    .map(convert_validation_rule_to_storage)
                                    .collect(),
                            })
                            .collect(),
                        samples: c.samples.clone(),
                        description: c.description.clone(),
                        parameter_groups: c
                            .parameter_groups
                            .iter()
                            .map(convert_parameter_group_to_storage)
                            .collect(),
                    })
                    .collect(),
                default_offline_timeout_secs: template.default_offline_timeout_secs,
                store_raw: template.store_raw,
                builtin_version: template.builtin_version.clone(),
                ct_prov: template.ct_prov.clone(),
            };
            store
                .save_template(&storage_template)
                .map_err(|e| DeviceError::Storage(format!("Failed to save template: {}", e)))?;
        }

        // Convert and save devices
        for entry in self.devices.iter() {
            let device = entry.value();

            let storage_config = StorageConfig {
                device_id: device.device_id.clone(),
                name: device.name.clone(),
                device_type: device.device_type.clone(),
                adapter_type: device.adapter_type.clone(),
                connection_config: convert_connection_config_to_storage(
                    device.connection_config.clone(),
                ),
                adapter_id: device.adapter_id.clone(),
                last_seen: device.last_seen,
                offline_timeout_secs: device.offline_timeout_secs,
            };
            store
                .save_device(&storage_config)
                .map_err(|e| DeviceError::Storage(format!("Failed to save device: {}", e)))?;
        }

        tracing::debug!(
            "Saved {} templates and {} devices to storage",
            self.templates.len(),
            self.devices.len()
        );

        Ok(())
    }

    /// Enable or disable auto-save
    pub fn set_auto_save(&self, enabled: bool) {
        self.auto_save
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    /// Check if storage is enabled
    pub fn has_storage(&self) -> bool {
        self.storage.is_some()
    }

    /// Helper: check if auto-save is enabled
    fn is_auto_save(&self) -> bool {
        self.auto_save.load(std::sync::atomic::Ordering::Relaxed)
    }

    // ========== Template Management ==========

    /// Register a device type template
    pub async fn register_template(&self, template: DeviceTypeTemplate) -> Result<(), DeviceError> {
        self.validate_template(&template)?;

        let device_type = template.device_type.clone();

        // Convert local mode to storage mode
        let mode = match template.mode {
            DeviceTypeMode::Simple => neomind_storage::device_registry::DeviceTypeMode::Simple,
            DeviceTypeMode::Full => neomind_storage::device_registry::DeviceTypeMode::Full,
        };

        // Convert to storage template for saving
        let storage_template = StorageTemplate {
            device_type: template.device_type.clone(),
            name: template.name.clone(),
            description: template.description.clone(),
            categories: template.categories.clone(),
            mode,
            metrics: template
                .metrics
                .iter()
                .map(|m| StorageMetricDefinition {
                    name: m.name.clone(),
                    display_name: m.display_name.clone(),
                    data_type: convert_metric_data_type_to_storage(m.data_type.clone()),
                    unit: m.unit.clone(),
                    min: m.min,
                    max: m.max,
                    required: m.required,
                })
                .collect(),
            uplink_samples: template.uplink_samples.clone(),
            commands: template
                .commands
                .iter()
                .map(|c| StorageCommandDefinition {
                    name: c.name.clone(),
                    display_name: c.display_name.clone(),
                    payload_template: c.payload_template.clone(),
                    fixed_values: c.fixed_values.clone(),
                    parameters: c
                        .parameters
                        .iter()
                        .map(|p| StorageParameterDefinition {
                            name: p.name.clone(),
                            display_name: p.display_name.clone(),
                            data_type: convert_metric_data_type_to_storage(p.data_type.clone()),
                            default_value: p
                                .default_value
                                .as_ref()
                                .and_then(|v| convert_metric_value_to_storage(v.clone())),
                            min: p.min,
                            max: p.max,
                            unit: p.unit.clone(),
                            allowed_values: p
                                .allowed_values
                                .iter()
                                .filter_map(|v| convert_metric_value_to_storage(v.clone()))
                                .collect(),
                            required: p.required,
                            visible_when: p.visible_when.clone(),
                            group: p.group.clone(),
                            help_text: p.help_text.clone(),
                            validation: p
                                .validation
                                .iter()
                                .map(convert_validation_rule_to_storage)
                                .collect(),
                        })
                        .collect(),
                    samples: c.samples.clone(),
                    description: c.description.clone(),
                    parameter_groups: c
                        .parameter_groups
                        .iter()
                        .map(convert_parameter_group_to_storage)
                        .collect(),
                })
                .collect(),
            default_offline_timeout_secs: template.default_offline_timeout_secs,
            store_raw: template.store_raw,
            builtin_version: template.builtin_version.clone(),
            ct_prov: template.ct_prov.clone(),
        };

        self.templates.insert(device_type, template);

        // Save to storage if enabled
        if self.is_auto_save() {
            if let Some(store) = &self.storage {
                if let Err(e) = store.save_template(&storage_template) {
                    tracing::warn!("Failed to save template to storage: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Get a device type template
    pub fn get_template(&self, device_type: &str) -> Option<DeviceTypeTemplate> {
        self.templates.get(device_type).map(|v| v.value().clone())
    }

    /// List all device type templates
    pub fn list_templates(&self) -> Vec<DeviceTypeTemplate> {
        self.templates.iter().map(|e| e.value().clone()).collect()
    }

    /// Unregister a device type template
    pub async fn unregister_template(&self, device_type: &str) -> Result<(), DeviceError> {
        // Check if any devices are using this template
        if let Some(type_entry) = self.type_index.get(device_type) {
            if !type_entry.is_empty() {
                return Err(DeviceError::InvalidParameter(format!(
                    "Cannot unregister template '{}': {} devices still use it",
                    device_type,
                    type_entry.len()
                )));
            }
        }

        self.templates.remove(device_type);

        // Delete from storage if enabled
        if self.is_auto_save() {
            if let Some(store) = &self.storage {
                if let Err(e) = store.delete_template(device_type) {
                    tracing::warn!("Failed to delete template from storage: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Validate a template
    fn validate_template(&self, template: &DeviceTypeTemplate) -> Result<(), DeviceError> {
        if template.device_type.is_empty() {
            return Err(DeviceError::InvalidParameter(
                "device_type cannot be empty".into(),
            ));
        }

        // Validate device_type only contains alphanumeric, underscore, hyphen
        if !template
            .device_type
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(DeviceError::InvalidParameter(
                "device_type can only contain alphanumeric, underscore, and hyphen".into(),
            ));
        }

        // Validate metric definitions
        for metric in &template.metrics {
            if metric.name.is_empty() {
                return Err(DeviceError::InvalidParameter(
                    "metric name cannot be empty".into(),
                ));
            }
        }

        // Validate command definitions
        for command in &template.commands {
            if command.name.is_empty() {
                return Err(DeviceError::InvalidParameter(
                    "command name cannot be empty".into(),
                ));
            }
        }

        Ok(())
    }

    // ========== Device Configuration Management ==========

    /// Register a device configuration
    pub async fn register_device(&self, config: DeviceConfig) -> Result<(), DeviceError> {
        // Validate that the template exists
        let _template = self.get_template(&config.device_type).ok_or_else(|| {
            DeviceError::NotFoundStr(format!(
                "Device type template '{}' not found",
                config.device_type
            ))
        })?;

        // Validate device_id format
        validate_device_id(&config.device_id)?;

        let device_id = config.device_id.clone();
        let device_type = config.device_type.clone();
        let telemetry_topic = config.connection_config.telemetry_topic.clone();

        // Idempotent re-registration (e.g. ordinary approve retry after
        // BindIncomplete): apply the full landed config — including device_type
        // and connection_config — via update_device. The old partial patch kept
        // the first type/topic and made retry returns lie about what was saved.
        if self.devices.contains_key(&device_id) {
            tracing::debug!(
                "Device '{}' re-registered — updating type+config in place",
                device_id
            );
            return self.update_device(&device_id, config).await;
        }

        // Prepare storage config (cloned before moving config)
        let storage_config = if self.is_auto_save() {
            Some(StorageConfig {
                device_id: config.device_id.clone(),
                name: config.name.clone(),
                device_type: config.device_type.clone(),
                adapter_type: config.adapter_type.clone(),
                connection_config: convert_connection_config_to_storage(
                    config.connection_config.clone(),
                ),
                adapter_id: config.adapter_id.clone(),
                last_seen: config.last_seen,
                offline_timeout_secs: config.offline_timeout_secs,
            })
        } else {
            None
        };

        // Save to storage FIRST (before modifying memory)
        // This ensures atomicity - if storage fails, nothing is modified in memory
        if let Some(storage_config) = &storage_config {
            if let Some(store) = &self.storage {
                store.save_device(storage_config).map_err(|e| {
                    DeviceError::Storage(format!("Failed to save device to storage: {}", e))
                })?;
            }
        }

        // Store device configuration in memory
        self.devices.insert(device_id.clone(), config);

        // Update type index
        self.type_index
            .entry(device_type)
            .or_default()
            .push(device_id.clone());

        // Shared-topic reverse index (topic → many device ids).
        if let Some(topic) = telemetry_topic {
            self.topic_index_insert(&topic, &device_id);
        }

        Ok(())
    }

    /// Get a device configuration
    pub fn get_device(&self, device_id: &str) -> Option<DeviceConfig> {
        self.devices.get(device_id).map(|v| v.value().clone())
    }

    /// Update only the `last_seen` field for a device.
    /// Lightweight alternative to `update_device` — no template validation or type index updates.
    pub async fn update_last_seen(&self, device_id: &str, last_seen: i64) {
        // [ingest hot path] This fires on EVERY DeviceMetric event — one
        // redb write txn per metric per report (10-metric device at 1 Hz =
        // 10 txns/s, each a read-modify-write of the whole DeviceConfig).
        // Debounce the PERSIST side to ≥15s per device (in line with
        // heartbeat granularity — is_connected_within works from the
        // in-memory value, which still updates every event, so status
        // semantics are unchanged; restart survival loses at most ~15s of
        // last_seen precision, far inside any offline_timeout floor of 30s).
        let should_persist = {
            let mut entry = match self.devices.get_mut(device_id) {
                Some(c) => c,
                None => return,
            };
            let prev = entry.last_seen;
            entry.last_seen = last_seen;
            // First sighting (prev == 0), clock regression, or ≥15s advance.
            prev == 0 || last_seen - prev >= 15
        };

        if !should_persist {
            return;
        }
        if let Some(store) = &self.storage {
            if let Err(e) = store.update_last_seen(device_id, last_seen) {
                tracing::warn!("Failed to persist last_seen for {}: {}", device_id, e);
            }
        }
    }

    /// List all device configurations
    pub fn list_devices(&self) -> Vec<DeviceConfig> {
        self.devices.iter().map(|e| e.value().clone()).collect()
    }

    fn topic_index_insert(&self, topic: &str, device_id: &str) {
        let mut entry = self.topic_index.entry(topic.to_string()).or_default();
        if !entry.iter().any(|id| id == device_id) {
            entry.push(device_id.to_string());
        }
    }

    fn topic_index_remove_device(&self, topic: &str, device_id: &str) {
        let empty = if let Some(mut entry) = self.topic_index.get_mut(topic) {
            entry.retain(|id| id != device_id);
            entry.is_empty()
        } else {
            false
        };
        if empty {
            self.topic_index.remove(topic);
        }
    }

    /// All devices registered on a telemetry topic (shared-topic safe).
    pub fn find_devices_by_telemetry_topic(&self, topic: &str) -> Vec<(String, DeviceConfig)> {
        let mut out = Vec::new();
        if let Some(ids_ref) = self.topic_index.get(topic) {
            let ids = ids_ref.value().clone();
            drop(ids_ref);
            let mut still_valid = Vec::new();
            for id in ids {
                if let Some(config) = self.devices.get(&id) {
                    if config.connection_config.telemetry_topic.as_deref() == Some(topic) {
                        still_valid.push(id.clone());
                        out.push((id, config.clone()));
                    }
                }
            }
            if still_valid.is_empty() {
                self.topic_index.remove(topic);
            } else {
                self.topic_index.insert(topic.to_string(), still_valid);
            }
            if !out.is_empty() {
                return out;
            }
        }
        // Fallback linear scan — rebuilds the multi-device index.
        let mut rebuilt = Vec::new();
        for entry in self.devices.iter() {
            if entry.value().connection_config.telemetry_topic.as_deref() == Some(topic) {
                rebuilt.push(entry.key().clone());
                out.push((entry.key().clone(), entry.value().clone()));
            }
        }
        if !rebuilt.is_empty() {
            self.topic_index.insert(topic.to_string(), rebuilt);
        }
        out
    }

    /// Find a device by its telemetry topic.
    ///
    /// For shared topics this returns **one** registrant (first id) — suitable
    /// for transport presence client_id learning. Telemetry routing must use
    /// identity resolution + the multi-device topic map instead.
    pub fn find_device_by_telemetry_topic(&self, topic: &str) -> Option<(String, DeviceConfig)> {
        self.find_devices_by_telemetry_topic(topic)
            .into_iter()
            .next()
    }

    /// List devices by type
    pub fn list_devices_by_type(&self, device_type: &str) -> Vec<DeviceConfig> {
        let device_ids = self
            .type_index
            .get(device_type)
            .map(|e| e.value().clone())
            .unwrap_or_default();

        device_ids
            .iter()
            .filter_map(|id| self.devices.get(id).map(|e| e.value().clone()))
            .collect()
    }

    /// Unregister a device configuration
    pub fn unregister_device(&self, device_id: &str) -> Result<(), DeviceError> {
        // Get device to find its type + telemetry_topic (for index cleanup)
        let device = self
            .devices
            .get(device_id)
            .ok_or_else(|| DeviceError::NotFoundStr(device_id.to_string()))?;
        let device_type = device.device_type.clone();
        let telemetry_topic = device.connection_config.telemetry_topic.clone();
        drop(device);

        // Remove device
        self.devices.remove(device_id);

        // Drop only this device's topic association (shared topics keep others).
        if let Some(topic) = telemetry_topic {
            self.topic_index_remove_device(&topic, device_id);
        }

        // Update type index
        if let Some(mut type_entry) = self.type_index.get_mut(&device_type) {
            type_entry.retain(|id| id != device_id);
        }
        // Clean up empty type index entries
        if self
            .type_index
            .get(&device_type)
            .is_some_and(|e| e.is_empty())
        {
            self.type_index.remove(&device_type);
        }

        // Delete from storage if enabled
        if self.is_auto_save() {
            if let Some(store) = &self.storage {
                if let Err(e) = store.delete_device(device_id) {
                    tracing::warn!("Failed to delete device from storage: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Update a device configuration
    pub async fn update_device(
        &self,
        device_id: &str,
        config: DeviceConfig,
    ) -> Result<(), DeviceError> {
        // Validate device_id matches
        if config.device_id != device_id {
            return Err(DeviceError::InvalidParameter(
                "device_id in config must match the parameter".into(),
            ));
        }

        // Save new device_type before moving config
        let new_device_type = config.device_type.clone();

        // Prepare storage config (cloned before moving config)
        let storage_config = if self.is_auto_save() {
            Some(StorageConfig {
                device_id: config.device_id.clone(),
                name: config.name.clone(),
                device_type: config.device_type.clone(),
                adapter_type: config.adapter_type.clone(),
                connection_config: convert_connection_config_to_storage(
                    config.connection_config.clone(),
                ),
                adapter_id: config.adapter_id.clone(),
                last_seen: config.last_seen,
                offline_timeout_secs: config.offline_timeout_secs,
            })
        } else {
            None
        };

        // Validate template exists
        self.get_template(&new_device_type).ok_or_else(|| {
            DeviceError::NotFoundStr(format!(
                "Device type template '{}' not found",
                new_device_type
            ))
        })?;

        // Get old device type / topic for index update
        let (old_device_type, old_topic) = self
            .devices
            .get(device_id)
            .map(|d| {
                (
                    d.device_type.clone(),
                    d.connection_config.telemetry_topic.clone(),
                )
            })
            .unwrap_or((String::new(), None));
        let new_topic = config.connection_config.telemetry_topic.clone();

        // Update device
        self.devices.insert(device_id.to_string(), config);

        // Update type index if type changed
        if !old_device_type.is_empty() && old_device_type != new_device_type {
            // Remove from old type index
            if let Some(mut type_entry) = self.type_index.get_mut(&old_device_type) {
                type_entry.retain(|id| id != device_id);
            }
            if self
                .type_index
                .get(&old_device_type)
                .is_some_and(|e| e.is_empty())
            {
                self.type_index.remove(&old_device_type);
            }

            // Add to new type index
            self.type_index
                .entry(new_device_type)
                .or_default()
                .push(device_id.to_string());
        }

        // Maintain multi-device topic index when telemetry_topic changes.
        if old_topic.as_deref() != new_topic.as_deref() {
            if let Some(old) = old_topic {
                self.topic_index_remove_device(&old, device_id);
            }
            if let Some(new) = new_topic {
                self.topic_index_insert(&new, device_id);
            }
        }

        // Update storage if enabled
        if let Some(storage_config) = storage_config {
            if let Some(store) = &self.storage {
                if let Err(e) = store.update_device(device_id, &storage_config) {
                    tracing::warn!("Failed to update device in storage: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Get device count
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }

    /// Get template count
    pub fn template_count(&self) -> usize {
        self.templates.len()
    }

    /// Get reference to storage backend (for command history, etc.)
    pub fn storage(&self) -> Option<&Arc<DeviceRegistryStore>> {
        self.storage.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_template_registration() {
        let registry = DeviceRegistry::new();

        let template = DeviceTypeTemplate::new("test_sensor", "Test Sensor")
            .with_description("A test sensor")
            .with_metric(MetricDefinition {
                name: "temperature".to_string(),
                display_name: "Temperature".to_string(),
                data_type: MetricDataType::Float,
                unit: "°C".to_string(),
                min: Some(-40.0),
                max: Some(80.0),
                required: false,
            });

        registry.register_template(template).await.unwrap();

        let retrieved = registry.get_template("test_sensor");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "Test Sensor");
    }

    #[tokio::test]
    async fn test_device_registration() {
        let registry = DeviceRegistry::new();

        // First register a template
        let template = DeviceTypeTemplate::new("test_sensor", "Test Sensor");
        registry.register_template(template).await.unwrap();

        // Then register a device
        let config = DeviceConfig {
            device_id: "sensor1".to_string(),
            name: "Sensor 1".to_string(),
            device_type: "test_sensor".to_string(),
            adapter_type: "mqtt".to_string(),
            connection_config: ConnectionConfig::mqtt("sensors/sensor1/data", None::<String>),
            adapter_id: None,
            last_seen: 0,
            offline_timeout_secs: None,
        };

        registry.register_device(config).await.unwrap();

        let retrieved = registry.get_device("sensor1");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "Sensor 1");
    }

    #[tokio::test]
    async fn test_device_without_template_fails() {
        let registry = DeviceRegistry::new();

        let config = DeviceConfig {
            device_id: "sensor1".to_string(),
            name: "Sensor 1".to_string(),
            device_type: "nonexistent".to_string(),
            adapter_type: "mqtt".to_string(),
            connection_config: ConnectionConfig::new(),
            adapter_id: None,
            last_seen: 0,
            offline_timeout_secs: None,
        };

        let result = registry.register_device(config).await;
        assert!(result.is_err());
    }

    /// NM-206: ordinary approve retry after BindIncomplete with a changed
    /// device_type must land the new type+config (not the old partial upsert).
    #[tokio::test]
    async fn register_device_retry_updates_type_and_config() {
        let registry = DeviceRegistry::new();
        registry
            .register_template(DeviceTypeTemplate::new("old", "Old"))
            .await
            .unwrap();
        registry
            .register_template(DeviceTypeTemplate::new("new", "New"))
            .await
            .unwrap();

        let mut first = DeviceConfig {
            device_id: "retry-device".into(),
            name: "Retry".into(),
            device_type: "old".into(),
            adapter_type: "mqtt".into(),
            connection_config: ConnectionConfig {
                telemetry_topic: Some("sensors/old".into()),
                ..Default::default()
            },
            adapter_id: Some("test".into()),
            last_seen: 1,
            offline_timeout_secs: None,
        };
        registry.register_device(first.clone()).await.unwrap();
        assert_eq!(
            registry.get_device("retry-device").unwrap().device_type,
            "old"
        );

        first.device_type = "new".into();
        first.connection_config.telemetry_topic = Some("sensors/retry-device".into());
        first.name = "Retry renamed".into();
        registry.register_device(first).await.unwrap();

        let landed = registry.get_device("retry-device").unwrap();
        assert_eq!(landed.device_type, "new", "retry must update device_type");
        assert_eq!(landed.name, "Retry renamed");
        assert_eq!(
            landed.connection_config.telemetry_topic.as_deref(),
            Some("sensors/retry-device"),
            "retry must update connection_config"
        );
        assert!(registry
            .list_devices_by_type("new")
            .iter()
            .any(|d| d.device_id == "retry-device"));
        assert!(
            !registry
                .list_devices_by_type("old")
                .iter()
                .any(|d| d.device_id == "retry-device"),
            "type index must move off old type"
        );
    }

    #[tokio::test]
    async fn test_list_devices_by_type() {
        let registry = DeviceRegistry::new();

        // Register template
        let template = DeviceTypeTemplate::new("test_sensor", "Test Sensor");
        registry.register_template(template).await.unwrap();

        // Register multiple devices
        for i in 1..=3 {
            let config = DeviceConfig {
                device_id: format!("sensor{}", i),
                name: format!("Sensor {}", i),
                device_type: "test_sensor".to_string(),
                adapter_type: "mqtt".to_string(),
                connection_config: ConnectionConfig::new(),
                adapter_id: None,
                last_seen: 0,
                offline_timeout_secs: None,
            };
            registry.register_device(config).await.unwrap();
        }

        let devices = registry.list_devices_by_type("test_sensor");
        assert_eq!(devices.len(), 3);
    }

    #[tokio::test]
    async fn test_unregister_template_with_devices_fails() {
        let registry = DeviceRegistry::new();

        // Register template and device
        let template = DeviceTypeTemplate::new("test_sensor", "Test Sensor");
        registry.register_template(template).await.unwrap();

        let config = DeviceConfig {
            device_id: "sensor1".to_string(),
            name: "Sensor 1".to_string(),
            device_type: "test_sensor".to_string(),
            adapter_type: "mqtt".to_string(),
            connection_config: ConnectionConfig::new(),
            adapter_id: None,
            last_seen: 0,
            offline_timeout_secs: None,
        };
        registry.register_device(config).await.unwrap();

        // Try to unregister template - should fail
        let result = registry.unregister_template("test_sensor").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_register_device_validates_device_id() {
        let registry = DeviceRegistry::new();
        registry
            .register_template(DeviceTypeTemplate::new("test_sensor", "Test Sensor"))
            .await
            .unwrap();

        // Valid device IDs (including dots, colons, dashes, underscores) must pass
        for valid_id in [
            "sensor-01",
            "aa:bb:cc:dd:ee:ff",
            "device.room_1",
            &"a".repeat(MAX_DEVICE_ID_LEN),
        ] {
            let cfg = DeviceConfig {
                device_id: valid_id.to_string(),
                name: "Valid".to_string(),
                device_type: "test_sensor".to_string(),
                adapter_type: "mqtt".to_string(),
                connection_config: ConnectionConfig::new(),
                adapter_id: None,
                last_seen: 0,
                offline_timeout_secs: None,
            };
            assert!(
                registry.register_device(cfg).await.is_ok(),
                "expected valid device_id to succeed: {valid_id}"
            );
        }

        // Invalid device IDs must fail with InvalidParameter
        for bad_id in [
            "",
            &"x".repeat(MAX_DEVICE_ID_LEN + 1),
            "../etc/passwd",
            "foo..bar",
            "dir/sensor",
            "dir\\sensor",
            "bad\0id",
            "bad\nid",
            "bad\x1bid",
        ] {
            let cfg = DeviceConfig {
                device_id: bad_id.to_string(),
                name: "Bad".to_string(),
                device_type: "test_sensor".to_string(),
                adapter_type: "mqtt".to_string(),
                connection_config: ConnectionConfig::new(),
                adapter_id: None,
                last_seen: 0,
                offline_timeout_secs: None,
            };
            assert!(
                matches!(
                    registry.register_device(cfg).await,
                    Err(DeviceError::InvalidParameter(_))
                ),
                "expected invalid device_id to be rejected: {bad_id:?}"
            );
        }
    }

    /// Test that GitHub device type JSON files can be deserialized correctly
    /// This validates compatibility with the camthink-ai/NeoMind-DeviceTypes repository
    #[test]
    fn test_github_device_types_deserialization() {
        // ne101_camera.json - uses TitleCase types (String, Integer)
        let ne101_json = r#"{
            "device_type": "ne101_camera",
            "name": "CamThink Sensing Camera",
            "description": "Test camera",
            "categories": ["Camera", "Sensing"],
            "mode": "simple",
            "metrics": [
                {"name": "ts", "display_name": "Timestamp", "data_type": "Integer", "required": false},
                {"name": "values.devName", "display_name": "Device Name", "data_type": "String", "required": false},
                {"name": "values.battery", "display_name": "Battery Level", "data_type": "Integer", "unit": "%", "min": 0, "max": 100, "required": false}
            ],
            "uplink_samples": [
                {"ts": 1740640441620, "values": {"devName": "NE101", "battery": 84}}
            ],
            "commands": []
        }"#;

        let ne101_result: Result<DeviceTypeTemplate, _> = serde_json::from_str(ne101_json);
        assert!(
            ne101_result.is_ok(),
            "ne101_camera deserialization failed: {:?}",
            ne101_result.err()
        );

        // ne301_camera.json - uses TitleCase "Array" which was problematic
        let ne301_json = r#"{
            "device_type": "ne301_camera",
            "name": "CamThink Edge AI Camera",
            "description": "Test AI camera",
            "categories": ["Camera", "AI"],
            "mode": "simple",
            "metrics": [
                {"name": "metadata.image_id", "display_name": "Image ID", "data_type": "String", "required": false},
                {"name": "ai_result.ai_result.detections", "display_name": "Detections", "data_type": "Array", "required": false},
                {"name": "ai_result.ai_result.poses", "display_name": "Poses", "data_type": "Array", "required": false}
            ],
            "uplink_samples": [],
            "commands": [
                {
                    "name": "capture",
                    "display_name": "Capture",
                    "description": "Capture an image",
                    "payload_template": "{\"cmd\": \"capture\"}",
                    "parameters": [
                        {"name": "request_id", "display_name": "Request ID", "data_type": "String", "required": true},
                        {"name": "enable_ai", "display_name": "Enable AI", "data_type": "Boolean", "default_value": true, "required": false}
                    ]
                }
            ]
        }"#;

        let ne301_result: Result<DeviceTypeTemplate, _> = serde_json::from_str(ne301_json);
        assert!(
            ne301_result.is_ok(),
            "ne301_camera deserialization failed: {:?}",
            ne301_result.err()
        );

        // Verify the Array type was deserialized correctly
        let template = ne301_result.unwrap();
        let detections_metric = template
            .metrics
            .iter()
            .find(|m| m.name == "ai_result.ai_result.detections");
        assert!(detections_metric.is_some(), "detections metric not found");
        match &detections_metric.unwrap().data_type {
            MetricDataType::Array { element_type } => {
                assert!(
                    element_type.is_none(),
                    "element_type should be None for plain 'Array' string"
                );
            }
            _ => panic!(
                "Expected Array variant, got {:?}",
                detections_metric.unwrap().data_type
            ),
        }

        // Verify command parameter with default_value was deserialized
        let capture_cmd = template.commands.iter().find(|c| c.name == "capture");
        assert!(capture_cmd.is_some(), "capture command not found");
        let enable_ai_param = capture_cmd
            .unwrap()
            .parameters
            .iter()
            .find(|p| p.name == "enable_ai");
        assert!(enable_ai_param.is_some(), "enable_ai parameter not found");
        assert!(
            !enable_ai_param.unwrap().required,
            "enable_ai should not be required"
        );
    }

    #[tokio::test]
    async fn shared_topic_index_keeps_both_devices() {
        let registry = DeviceRegistry::new();
        registry
            .register_template(DeviceTypeTemplate::new("gw_sensor", "Gateway Sensor"))
            .await
            .unwrap();

        for id in ["A", "B"] {
            registry
                .register_device(DeviceConfig {
                    device_id: id.to_string(),
                    name: format!("Device {id}"),
                    device_type: "gw_sensor".to_string(),
                    adapter_type: "mqtt".to_string(),
                    connection_config: ConnectionConfig::mqtt("gateway/data", None::<String>),
                    adapter_id: None,
                    last_seen: 0,
                    offline_timeout_secs: None,
                })
                .await
                .unwrap();
        }

        let found = registry.find_devices_by_telemetry_topic("gateway/data");
        let ids: Vec<_> = found.iter().map(|(id, _)| id.as_str()).collect();
        assert!(ids.contains(&"A"));
        assert!(ids.contains(&"B"));
        assert_eq!(found.len(), 2);

        registry.unregister_device("A").unwrap();
        let found = registry.find_devices_by_telemetry_topic("gateway/data");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "B");
    }
}
