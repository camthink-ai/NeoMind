//! Device Registry - Unified storage for device type templates and device instance configurations
//!
//! This module provides a simplified, unified registry that stores both:
//! - Device type templates (simplified DeviceTypeDefinition)
//! - Device instance configurations (DeviceConfig)
//!
//! ## Persistence
//!
//! The registry supports optional disk persistence using redb storage:
//! ```rust,no_run
//! use neomind_devices::DeviceRegistry;
//!
//! // Create registry with persistence
//! let registry = DeviceRegistry::with_persistence("./data/devices.redb").await?;
//!
//! // Or create in-memory registry
//! let registry = DeviceRegistry::new();
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

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
    Simple,
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

/// Unified Device Registry
/// Stores both templates and device configurations with optional persistence
pub struct DeviceRegistry {
    /// Device type templates indexed by device_type
    templates: Arc<RwLock<HashMap<String, DeviceTypeTemplate>>>,
    /// Device instance configurations indexed by device_id
    devices: Arc<RwLock<HashMap<String, DeviceConfig>>>,
    /// Index: device_type -> set of device_ids
    type_index: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// Optional persistent storage backend
    storage: Option<Arc<DeviceRegistryStore>>,
    /// Whether to auto-save after modifications
    auto_save: Arc<RwLock<bool>>,
}

impl DeviceRegistry {
    /// Create a new in-memory device registry (no persistence)
    pub fn new() -> Self {
        Self {
            templates: Arc::new(RwLock::new(HashMap::new())),
            devices: Arc::new(RwLock::new(HashMap::new())),
            type_index: Arc::new(RwLock::new(HashMap::new())),
            storage: None,
            auto_save: Arc::new(RwLock::new(false)),
        }
    }

    /// Create a new device registry with disk persistence
    ///
    /// This will load existing templates and devices from the storage file.
    /// If the file doesn't exist, it will be created.
    pub async fn with_persistence<P: AsRef<Path>>(path: P) -> Result<Self, DeviceError> {
        let store = DeviceRegistryStore::open(path)
            .map_err(|e| DeviceError::Storage(format!("Failed to open storage: {}", e)))?;

        let registry = Self {
            templates: Arc::new(RwLock::new(HashMap::new())),
            devices: Arc::new(RwLock::new(HashMap::new())),
            type_index: Arc::new(RwLock::new(HashMap::new())),
            storage: Some(store), // Already an Arc
            auto_save: Arc::new(RwLock::new(true)),
        };

        // Load existing data from storage
        registry.load_from_storage().await?;

        Ok(registry)
    }

    /// Load all data from storage into memory
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
                        llm_hints: c.llm_hints,
                        parameter_groups: c
                            .parameter_groups
                            .into_iter()
                            .map(convert_parameter_group_from_storage)
                            .collect(),
                    })
                    .collect(),
            };

            let mut templates = self.templates.write().await;
            templates.insert(template.device_type.clone(), template);
        }

        // Load devices
        let storage_devices = store
            .list_devices()
            .map_err(|e| DeviceError::Storage(format!("Failed to load devices: {}", e)))?;

        for storage_device in storage_devices {
            let config = DeviceConfig {
                device_id: storage_device.device_id,
                name: storage_device.name,
                device_type: storage_device.device_type,
                adapter_type: storage_device.adapter_type,
                connection_config: convert_connection_config(storage_device.connection_config),
                adapter_id: storage_device.adapter_id,
            };

            let device_id = config.device_id.clone();
            let device_type = config.device_type.clone();

            // Store device
            {
                let mut devices = self.devices.write().await;
                devices.insert(device_id.clone(), config);
            }

            // Update type index
            {
                let mut type_index = self.type_index.write().await;
                type_index
                    .entry(device_type)
                    .or_insert_with(Vec::new)
                    .push(device_id);
            }
        }

        tracing::info!(
            "Loaded {} templates and {} devices from storage",
            self.templates.read().await.len(),
            self.devices.read().await.len()
        );

        Ok(())
    }

    /// Save all data to storage (manual save)
    pub async fn save_to_storage(&self) -> Result<(), DeviceError> {
        let Some(store) = &self.storage else {
            return Err(DeviceError::Storage("No storage configured".to_string()));
        };

        // Convert and save templates
        let templates = self.templates.read().await;
        for template in templates.values() {
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
                        llm_hints: c.llm_hints.clone(),
                        parameter_groups: c
                            .parameter_groups
                            .iter()
                            .map(convert_parameter_group_to_storage)
                            .collect(),
                    })
                    .collect(),
            };
            store
                .save_template(&storage_template)
                .map_err(|e| DeviceError::Storage(format!("Failed to save template: {}", e)))?;
        }
        drop(templates);

        // Convert and save devices
        let devices = self.devices.read().await;
        for device in devices.values() {
            let storage_config = StorageConfig {
                device_id: device.device_id.clone(),
                name: device.name.clone(),
                device_type: device.device_type.clone(),
                adapter_type: device.adapter_type.clone(),
                connection_config: convert_connection_config_to_storage(
                    device.connection_config.clone(),
                ),
                adapter_id: device.adapter_id.clone(),
            };
            store
                .save_device(&storage_config)
                .map_err(|e| DeviceError::Storage(format!("Failed to save device: {}", e)))?;
        }

        tracing::debug!(
            "Saved {} templates and {} devices to storage",
            self.templates.read().await.len(),
            self.devices.read().await.len()
        );

        Ok(())
    }

    /// Enable or disable auto-save
    pub async fn set_auto_save(&self, enabled: bool) {
        let mut auto_save = self.auto_save.write().await;
        *auto_save = enabled;
    }

    /// Check if storage is enabled
    pub fn has_storage(&self) -> bool {
        self.storage.is_some()
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
                    llm_hints: c.llm_hints.clone(),
                    parameter_groups: c
                        .parameter_groups
                        .iter()
                        .map(convert_parameter_group_to_storage)
                        .collect(),
                })
                .collect(),
        };

        let mut templates = self.templates.write().await;
        templates.insert(device_type.clone(), template);

        // Save to storage if enabled
        drop(templates);
        if self.storage.is_some() {
            if *self.auto_save.read().await {
                if let Some(store) = &self.storage {
                    if let Err(e) = store.save_template(&storage_template) {
                        tracing::warn!("Failed to save template to storage: {}", e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Get a device type template
    pub async fn get_template(&self, device_type: &str) -> Option<DeviceTypeTemplate> {
        let templates = self.templates.read().await;
        templates.get(device_type).cloned()
    }

    /// List all device type templates
    pub async fn list_templates(&self) -> Vec<DeviceTypeTemplate> {
        let templates = self.templates.read().await;
        templates.values().cloned().collect()
    }

    /// Unregister a device type template
    pub async fn unregister_template(&self, device_type: &str) -> Result<(), DeviceError> {
        // Check if any devices are using this template
        let type_index = self.type_index.read().await;
        if let Some(device_ids) = type_index.get(device_type) {
            if !device_ids.is_empty() {
                return Err(DeviceError::InvalidParameter(format!(
                    "Cannot unregister template '{}': {} devices still use it",
                    device_type,
                    device_ids.len()
                )));
            }
        }
        drop(type_index);

        let mut templates = self.templates.write().await;
        templates.remove(device_type);
        drop(templates);

        // Delete from storage if enabled
        if self.storage.is_some() {
            if *self.auto_save.read().await {
                if let Some(store) = &self.storage {
                    if let Err(e) = store.delete_template(device_type) {
                        tracing::warn!("Failed to delete template from storage: {}", e);
                    }
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
        let _template = self
            .get_template(&config.device_type)
            .await
            .ok_or_else(|| {
                DeviceError::NotFoundStr(format!(
                    "Device type template '{}' not found",
                    config.device_type
                ))
            })?;

        // Validate device_id format
        if config.device_id.is_empty() {
            return Err(DeviceError::InvalidParameter(
                "device_id cannot be empty".into(),
            ));
        }

        let device_id = config.device_id.clone();
        let device_type = config.device_type.clone();

        // Check if device already exists
        {
            let devices = self.devices.read().await;
            if devices.contains_key(&device_id) {
                return Err(DeviceError::AlreadyExists(device_id));
            }
        }

        // Prepare storage config (cloned before moving config)
        let storage_config = if self.storage.is_some() && *self.auto_save.read().await {
            Some(StorageConfig {
                device_id: config.device_id.clone(),
                name: config.name.clone(),
                device_type: config.device_type.clone(),
                adapter_type: config.adapter_type.clone(),
                connection_config: convert_connection_config_to_storage(
                    config.connection_config.clone(),
                ),
                adapter_id: config.adapter_id.clone(),
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
        {
            let mut devices = self.devices.write().await;
            devices.insert(device_id.clone(), config);
        }

        // Update type index
        {
            let mut type_index = self.type_index.write().await;
            type_index
                .entry(device_type)
                .or_insert_with(Vec::new)
                .push(device_id);
        }

        Ok(())
    }

    /// Get a device configuration
    pub async fn get_device(&self, device_id: &str) -> Option<DeviceConfig> {
        let devices = self.devices.read().await;
        devices.get(device_id).cloned()
    }

    /// List all device configurations
    pub async fn list_devices(&self) -> Vec<DeviceConfig> {
        let devices = self.devices.read().await;
        devices.values().cloned().collect()
    }

    /// Find a device by its telemetry topic
    /// This is used by MQTT adapters to route messages from custom topics
    pub async fn find_device_by_telemetry_topic(
        &self,
        topic: &str,
    ) -> Option<(String, DeviceConfig)> {
        let devices = self.devices.read().await;
        for (device_id, config) in devices.iter() {
            if let Some(ref telemetry_topic) = config.connection_config.telemetry_topic {
                if telemetry_topic == topic {
                    return Some((device_id.clone(), config.clone()));
                }
            }
        }
        None
    }

    /// List devices by type
    pub async fn list_devices_by_type(&self, device_type: &str) -> Vec<DeviceConfig> {
        let type_index = self.type_index.read().await;
        let device_ids = type_index.get(device_type).cloned().unwrap_or_default();
        drop(type_index);

        let devices = self.devices.read().await;
        device_ids
            .iter()
            .filter_map(|id| devices.get(id).cloned())
            .collect()
    }

    /// Unregister a device configuration
    pub async fn unregister_device(&self, device_id: &str) -> Result<(), DeviceError> {
        // Get device to find its type
        let device_type = {
            let devices = self.devices.read().await;
            devices.get(device_id).map(|d| d.device_type.clone())
        };

        if device_type.is_none() {
            return Err(DeviceError::NotFoundStr(device_id.to_string()));
        }

        let device_type = device_type.unwrap();

        // Remove device
        {
            let mut devices = self.devices.write().await;
            devices.remove(device_id);
        }

        // Update type index
        {
            let mut type_index = self.type_index.write().await;
            if let Some(device_ids) = type_index.get_mut(&device_type) {
                device_ids.retain(|id| id != device_id);
                if device_ids.is_empty() {
                    type_index.remove(&device_type);
                }
            }
        }

        // Delete from storage if enabled
        if self.storage.is_some() {
            if *self.auto_save.read().await {
                if let Some(store) = &self.storage {
                    if let Err(e) = store.delete_device(device_id) {
                        tracing::warn!("Failed to delete device from storage: {}", e);
                    }
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
        let storage_config = if self.storage.is_some() && *self.auto_save.read().await {
            Some(StorageConfig {
                device_id: config.device_id.clone(),
                name: config.name.clone(),
                device_type: config.device_type.clone(),
                adapter_type: config.adapter_type.clone(),
                connection_config: convert_connection_config_to_storage(
                    config.connection_config.clone(),
                ),
                adapter_id: config.adapter_id.clone(),
            })
        } else {
            None
        };

        // Validate template exists
        self.get_template(&new_device_type).await.ok_or_else(|| {
            DeviceError::NotFoundStr(format!(
                "Device type template '{}' not found",
                new_device_type
            ))
        })?;

        // Update device (type change handling if needed)
        let old_device_type = {
            let devices = self.devices.read().await;
            devices.get(device_id).map(|d| d.device_type.clone())
        };

        {
            let mut devices = self.devices.write().await;
            devices.insert(device_id.to_string(), config);
        }

        // Update type index if type changed
        if let Some(old_type) = old_device_type {
            if old_type != new_device_type {
                // Remove from old type index
                {
                    let mut type_index = self.type_index.write().await;
                    if let Some(device_ids) = type_index.get_mut(&old_type) {
                        device_ids.retain(|id| id != device_id);
                        if device_ids.is_empty() {
                            type_index.remove(&old_type);
                        }
                    }
                }

                // Add to new type index
                {
                    let mut type_index = self.type_index.write().await;
                    type_index
                        .entry(new_device_type)
                        .or_insert_with(Vec::new)
                        .push(device_id.to_string());
                }
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
    pub async fn device_count(&self) -> usize {
        let devices = self.devices.read().await;
        devices.len()
    }

    /// Get template count
    pub async fn template_count(&self) -> usize {
        let templates = self.templates.read().await;
        templates.len()
    }

    /// Get reference to storage backend (for command history, etc.)
    pub fn storage(&self) -> Option<&Arc<DeviceRegistryStore>> {
        self.storage.as_ref()
    }
}

impl Default for DeviceRegistry {
    fn default() -> Self {
        Self::new()
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

        let retrieved = registry.get_template("test_sensor").await;
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
        };

        registry.register_device(config).await.unwrap();

        let retrieved = registry.get_device("sensor1").await;
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
        };

        let result = registry.register_device(config).await;
        assert!(result.is_err());
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
            };
            registry.register_device(config).await.unwrap();
        }

        let devices = registry.list_devices_by_type("test_sensor").await;
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
        };
        registry.register_device(config).await.unwrap();

        // Try to unregister template - should fail
        let result = registry.unregister_template("test_sensor").await;
        assert!(result.is_err());
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
        assert_eq!(
            enable_ai_param.unwrap().required,
            false,
            "enable_ai should not be required"
        );
    }
}
