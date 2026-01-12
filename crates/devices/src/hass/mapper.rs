//! Home Assistant entity to NeoTalk device mapper.

use super::entities::{HassEntityState, HassDomain, HassDeviceClass, HassDeviceInfo};
use super::templates::HassDeviceTemplate;
use crate::mdl::{
    Device, DeviceId, DeviceType, MetricValue, MetricDefinition, MetricDataType,
    DeviceCapability, Command, DeviceState,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

/// Errors that can occur during entity mapping.
#[derive(Debug, Error)]
pub enum MappingError {
    #[error("Unsupported entity type: {0}")]
    UnsupportedEntityType(String),

    #[error("Invalid metric value: {0}")]
    InvalidValue(String),

    #[error("Template not found: {0}")]
    TemplateNotFound(String),

    #[error("Mapping configuration error: {0}")]
    ConfigError(String),
}

/// Result type for mapping operations.
pub type MappingResult<T> = Result<T, MappingError>;

/// Helper function to get friendly name or entity ID.
fn get_friendly_name_or_id(entity: &HassEntityState) -> String {
    if !entity.attributes.friendly_name.is_empty() {
        entity.attributes.friendly_name.clone()
    } else {
        entity.entity_id.clone()
    }
}

/// Configuration for mapping a HASS entity to a NeoTalk device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityMapping {
    /// HASS entity ID
    pub entity_id: String,

    /// NeoTalk device ID (generated if not provided)
    pub device_id: Option<String>,

    /// Device name
    pub name: Option<String>,

    /// Template to use for mapping
    pub template: Option<String>,

    /// Custom metric mappings
    pub metric_mappings: Vec<MetricMapping>,

    /// Custom command mappings
    pub command_mappings: Vec<CommandMapping>,
}

/// Mapping for a single metric.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricMapping {
    /// Metric name in NeoTalk
    pub name: String,

    /// Source in HASS entity (e.g., "state", "attributes.temperature")
    pub source: String,

    /// Data type
    pub data_type: MetricDataType,

    /// Unit of measurement
    pub unit: Option<String>,

    /// Whether this metric is read-only
    pub read_only: bool,
}

/// Mapping for a single command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandMapping {
    /// Command name in NeoTalk
    pub name: String,

    /// HASS service domain
    pub domain: String,

    /// HASS service name
    pub service: String,

    /// Default service data template
    pub service_data: Option<serde_json::Value>,
}

/// A mapped NeoTalk device from Home Assistant.
#[derive(Debug, Clone)]
pub struct MappedDevice {
    /// Device information
    pub device: MappedDeviceInfo,

    /// HASS entities that belong to this device
    pub entities: Vec<HassEntityState>,

    /// Template used for mapping
    pub template: Option<String>,
}

/// Information about a mapped device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappedDeviceInfo {
    /// NeoTalk device ID
    pub id: String,

    /// Device name
    pub name: String,

    /// Device type
    pub device_type: String,

    /// HASS device ID (if available)
    pub hass_device_id: Option<String>,

    /// Associated HASS entity IDs
    pub entity_ids: Vec<String>,

    /// Metrics defined for this device
    pub metrics: Vec<MetricMapping>,

    /// Commands defined for this device
    pub commands: Vec<CommandMapping>,

    /// Device attributes
    pub attributes: HashMap<String, String>,

    /// Whether device is enabled
    pub enabled: bool,
}

/// Mapper that converts HASS entities to NeoTalk devices.
pub struct HassEntityMapper {
    /// Available templates
    templates: HashMap<String, HassDeviceTemplate>,

    /// Custom entity mappings
    custom_mappings: HashMap<String, EntityMapping>,
}

impl HassEntityMapper {
    /// Create a new mapper with default templates.
    pub fn new() -> Self {
        let templates = super::templates::builtin_templates();
        Self {
            templates,
            custom_mappings: HashMap::new(),
        }
    }

    /// Create a new mapper with custom templates.
    pub fn with_templates(templates: HashMap<String, HassDeviceTemplate>) -> Self {
        Self {
            templates,
            custom_mappings: HashMap::new(),
        }
    }

    /// Add a custom mapping.
    pub fn add_mapping(&mut self, mapping: EntityMapping) {
        self.custom_mappings.insert(mapping.entity_id.clone(), mapping);
    }

    /// Add a custom template.
    pub fn add_template(&mut self, name: String, template: HassDeviceTemplate) {
        self.templates.insert(name, template);
    }

    /// Map a single HASS entity to a MappedDevice.
    pub fn map_entity(&self, entity: &HassEntityState) -> MappingResult<MappedDevice> {
        // Check for custom mapping first
        if let Some(mapping) = self.custom_mappings.get(&entity.entity_id) {
            return self.map_with_custom_mapping(entity, mapping);
        }

        // Auto-detect template based on entity domain and device class
        let template = self.find_template_for_entity(entity)?;

        self.map_with_template(entity, &template)
    }

    /// Map multiple entities, grouping by device.
    pub fn map_entities(&self, entities: Vec<HassEntityState>) -> MappingResult<Vec<MappedDevice>> {
        // Group entities by device
        let mut device_groups: HashMap<String, Vec<HassEntityState>> = HashMap::new();

        for entity in entities {
            // Skip disabled entities
            if entity.attributes.disabled {
                continue;
            }

            let device_key = if let Some(device) = &entity.attributes.device {
                // Use device ID if available
                if let Some(id) = device.identifiers.first() {
                    id.to_string()
                } else {
                    entity.entity_id.clone()
                }
            } else {
                entity.entity_id.clone()
            };

            device_groups
                .entry(device_key)
                .or_insert_with(Vec::new)
                .push(entity);
        }

        // Map each device group
        let mut mapped_devices = Vec::new();
        for (_key, device_entities) in device_groups {
            if let Some(device) = self.map_device_group(device_entities)? {
                mapped_devices.push(device);
            }
        }

        Ok(mapped_devices)
    }

    /// Map a group of entities that belong to the same device.
    fn map_device_group(&self, entities: Vec<HassEntityState>) -> MappingResult<Option<MappedDevice>> {
        if entities.is_empty() {
            return Ok(None);
        }

        // Use the first entity's device info
        let first_entity = &entities[0];
        let device_info = first_entity.attributes.device.clone();

        // Determine device type from entities
        let device_type = self.determine_device_type(&entities)?;

        // Generate device ID
        let device_id = if let Some(device) = &device_info {
            if let Some(id) = device.identifiers.first() {
                self.sanitize_device_id(&id.to_string())
            } else if let Some(name) = &device.name {
                format!("hass_{}", self.slugify(name))
            } else {
                format!("hass_{}", self.slugify(&entities[0].entity_id))
            }
        } else {
            format!("hass_{}", self.slugify(&entities[0].entity_id))
        };

        // Get device name
        let device_name = if let Some(device) = &device_info {
            if let Some(name) = &device.name {
                if !name.is_empty() {
                    name.clone()
                } else {
                    get_friendly_name_or_id(&entities[0])
                }
            } else {
                get_friendly_name_or_id(&entities[0])
            }
        } else {
            get_friendly_name_or_id(&entities[0])
        };

        // Collect entity IDs
        let entity_ids: Vec<String> = entities.iter().map(|e| e.entity_id.clone()).collect();

        // Get HASS device ID
        let hass_device_id = device_info.as_ref().and_then(|d| {
            d.identifiers
                .first()
                .and_then(|v| v.as_str().map(String::from))
        });

        // Build metrics and commands from entities
        let (metrics, commands) = self.extract_mappings_from_entities(&entities)?;

        // Build attributes
        let mut attributes = HashMap::new();
        if let Some(device) = &device_info {
            if let Some(manufacturer) = &device.manufacturer {
                attributes.insert("manufacturer".to_string(), manufacturer.clone());
            }
            if let Some(model) = &device.model {
                attributes.insert("model".to_string(), model.clone());
            }
            if let Some(sw_version) = &device.sw_version {
                attributes.insert("sw_version".to_string(), sw_version.clone());
            }
        }

        let info = MappedDeviceInfo {
            id: device_id,
            name: device_name,
            device_type,
            hass_device_id,
            entity_ids,
            metrics,
            commands,
            attributes,
            enabled: true,
        };

        Ok(Some(MappedDevice {
            device: info,
            entities,
            template: None,
        }))
    }

    /// Find the appropriate template for an entity.
    fn find_template_for_entity(&self, entity: &HassEntityState) -> MappingResult<HassDeviceTemplate> {
        let domain = HassDomain::from_entity_id(&entity.entity_id);

        // Match by domain and device class
        let template_key = match domain {
            HassDomain::Sensor => {
                if let Some(dc) = &entity.attributes.device_class {
                    format!("sensor_{}", dc)
                } else {
                    "sensor_generic".to_string()
                }
            }
            HassDomain::BinarySensor => "binary_sensor".to_string(),
            HassDomain::Switch => "switch".to_string(),
            HassDomain::Light => "light".to_string(),
            HassDomain::Cover => "cover".to_string(),
            HassDomain::Climate => "climate".to_string(),
            HassDomain::Camera => "camera".to_string(),
            HassDomain::Fan => "fan".to_string(),
            HassDomain::Lock => "lock".to_string(),
            HassDomain::MediaPlayer => "media_player".to_string(),
            _ => {
                return Err(MappingError::UnsupportedEntityType(format!(
                    "{}: {}",
                    domain.as_str(),
                    entity.entity_id
                )))
            }
        };

        self.templates
            .get(&template_key)
            .cloned()
            .or_else(|| self.templates.get("generic").cloned())
            .ok_or_else(|| MappingError::TemplateNotFound(template_key))
    }

    /// Map an entity using a specific template.
    fn map_with_template(
        &self,
        entity: &HassEntityState,
        template: &HassDeviceTemplate,
    ) -> MappingResult<MappedDevice> {
        let device_id = format!("hass_{}", self.slugify(&entity.entity_id));
        let device_name = get_friendly_name_or_id(entity);

        let (metrics, commands) = self.extract_mappings_from_template(entity, template)?;

        let info = MappedDeviceInfo {
            id: device_id,
            name: device_name,
            device_type: template.device_type.clone(),
            hass_device_id: None,
            entity_ids: vec![entity.entity_id.clone()],
            metrics,
            commands,
            attributes: HashMap::new(),
            enabled: !entity.attributes.disabled,
        };

        Ok(MappedDevice {
            device: info,
            entities: vec![entity.clone()],
            template: Some(template.name.clone()),
        })
    }

    /// Map an entity using a custom mapping.
    fn map_with_custom_mapping(
        &self,
        entity: &HassEntityState,
        mapping: &EntityMapping,
    ) -> MappingResult<MappedDevice> {
        let device_id = mapping
            .device_id
            .clone()
            .unwrap_or_else(|| format!("hass_{}", self.slugify(&entity.entity_id)));

        let device_name = if let Some(name) = &mapping.name {
            if !name.is_empty() {
                name.clone()
            } else {
                get_friendly_name_or_id(entity)
            }
        } else {
            get_friendly_name_or_id(entity)
        };

        let info = MappedDeviceInfo {
            id: device_id,
            name: device_name,
            device_type: "generic".to_string(),
            hass_device_id: None,
            entity_ids: vec![entity.entity_id.clone()],
            metrics: mapping.metric_mappings.clone(),
            commands: mapping.command_mappings.clone(),
            attributes: HashMap::new(),
            enabled: true,
        };

        Ok(MappedDevice {
            device: info,
            entities: vec![entity.clone()],
            template: mapping.template.clone(),
        })
    }

    /// Extract metric and command mappings from a template.
    fn extract_mappings_from_template(
        &self,
        entity: &HassEntityState,
        template: &HassDeviceTemplate,
    ) -> MappingResult<(Vec<MetricMapping>, Vec<CommandMapping>)> {
        let mut metrics = Vec::new();
        let mut commands = Vec::new();

        // Add template metrics
        for metric in &template.metrics {
            metrics.push(MetricMapping {
                name: metric.name.clone(),
                source: "state".to_string(),
                data_type: metric.data_type.clone(),
                unit: metric.unit.clone(),
                read_only: metric.read_only,
            });
        }

        // Add template commands
        for cmd in &template.commands {
            commands.push(CommandMapping {
                name: cmd.name.clone(),
                domain: cmd.domain.clone(),
                service: cmd.service.clone(),
                service_data: cmd.data.clone(),
            });
        }

        Ok((metrics, commands))
    }

    /// Extract metrics and commands from a list of entities.
    fn extract_mappings_from_entities(
        &self,
        entities: &[HassEntityState],
    ) -> MappingResult<(Vec<MetricMapping>, Vec<CommandMapping>)> {
        let mut metrics = Vec::new();
        let mut commands = Vec::new();

        for entity in entities {
            let domain = HassDomain::from_entity_id(&entity.entity_id);

            // Add metric for sensor entities
            match domain {
                HassDomain::Sensor | HassDomain::BinarySensor => {
                    let data_type = self.infer_data_type(entity);
                    let name = if !entity.attributes.friendly_name.is_empty() {
                        entity.attributes.friendly_name.clone()
                    } else {
                        entity
                            .entity_id
                            .split('.')
                            .nth(1)
                            .unwrap_or("value")
                            .to_string()
                    };

                    metrics.push(MetricMapping {
                        name: name.clone(),
                        source: "state".to_string(),
                        data_type,
                        unit: entity.attributes.unit_of_measurement.clone(),
                        read_only: true,
                    });
                }
                HassDomain::Switch | HassDomain::Light | HassDomain::Fan => {
                    // Add state metric
                    metrics.push(MetricMapping {
                        name: "state".to_string(),
                        source: "state".to_string(),
                        data_type: MetricDataType::Boolean,
                        unit: None,
                        read_only: false,
                    });

                    // Add commands
                    commands.push(CommandMapping {
                        name: "turn_on".to_string(),
                        domain: domain.as_str().to_string(),
                        service: "turn_on".to_string(),
                        service_data: None,
                    });

                    commands.push(CommandMapping {
                        name: "turn_off".to_string(),
                        domain: domain.as_str().to_string(),
                        service: "turn_off".to_string(),
                        service_data: None,
                    });

                    commands.push(CommandMapping {
                        name: "toggle".to_string(),
                        domain: domain.as_str().to_string(),
                        service: "toggle".to_string(),
                        service_data: None,
                    });
                }
                HassDomain::Cover => {
                    metrics.push(MetricMapping {
                        name: "state".to_string(),
                        source: "state".to_string(),
                        data_type: MetricDataType::String,
                        unit: None,
                        read_only: false,
                    });

                    commands.push(CommandMapping {
                        name: "open".to_string(),
                        domain: "cover".to_string(),
                        service: "open_cover".to_string(),
                        service_data: None,
                    });

                    commands.push(CommandMapping {
                        name: "close".to_string(),
                        domain: "cover".to_string(),
                        service: "close_cover".to_string(),
                        service_data: None,
                    });

                    commands.push(CommandMapping {
                        name: "stop".to_string(),
                        domain: "cover".to_string(),
                        service: "stop_cover".to_string(),
                        service_data: None,
                    });
                }
                HassDomain::Climate => {
                    metrics.push(MetricMapping {
                        name: "temperature".to_string(),
                        source: "attributes.temperature".to_string(),
                        data_type: MetricDataType::Float,
                        unit: Some("°C".to_string()),
                        read_only: false,
                    });

                    metrics.push(MetricMapping {
                        name: "hvac_mode".to_string(),
                        source: "state".to_string(),
                        data_type: MetricDataType::String,
                        unit: None,
                        read_only: false,
                    });
                }
                _ => {}
            }
        }

        Ok((metrics, commands))
    }

    /// Determine the device type from a list of entities.
    fn determine_device_type(&self, entities: &[HassEntityState]) -> MappingResult<String> {
        // Count domains
        let mut domain_counts: HashMap<HassDomain, usize> = HashMap::new();

        for entity in entities {
            let domain = HassDomain::from_entity_id(&entity.entity_id);
            *domain_counts.entry(domain).or_insert(0) += 1;
        }

        // Determine primary type
        if domain_counts.contains_key(&HassDomain::Light) {
            return Ok("light".to_string());
        }

        if domain_counts.contains_key(&HassDomain::Switch) {
            return Ok("switch".to_string());
        }

        if domain_counts.contains_key(&HassDomain::Sensor) {
            return Ok("sensor".to_string());
        }

        if domain_counts.contains_key(&HassDomain::Climate) {
            return Ok("climate".to_string());
        }

        if domain_counts.contains_key(&HassDomain::Cover) {
            return Ok("cover".to_string());
        }

        if domain_counts.contains_key(&HassDomain::MediaPlayer) {
            return Ok("media_player".to_string());
        }

        Ok("generic".to_string())
    }

    /// Infer data type from entity state.
    fn infer_data_type(&self, entity: &HassEntityState) -> MetricDataType {
        // Try to parse as number
        if let Ok(_) = entity.state.parse::<f64>() {
            return MetricDataType::Float;
        }

        // Check for boolean states
        let lower = entity.state.to_lowercase();
        if matches!(lower.as_str(), "on" | "off" | "true" | "false") {
            return MetricDataType::Boolean;
        }

        // Default to string
        MetricDataType::String
    }

    /// Sanitize device ID for safe use.
    fn sanitize_device_id(&self, id: &str) -> String {
        id.chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    }

    /// Convert string to slug format.
    fn slugify(&self, s: &str) -> String {
        s.to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect::<String>()
            .split('_')
            .filter(|s| !s.is_empty())
            .collect::<Vec<&str>>()
            .join("_")
    }
}

impl Default for HassEntityMapper {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mapper_creation() {
        let mapper = HassEntityMapper::new();
        assert!(!mapper.templates.is_empty());
    }

    #[test]
    fn test_slugify() {
        let mapper = HassEntityMapper::new();
        assert_eq!(mapper.slugify("Living Room Temperature"), "living_room_temperature");
        assert_eq!(mapper.slugify("sensor.temp_188"), "sensor_temp_188");
    }

    #[test]
    fn test_sanitize_device_id() {
        let mapper = HassEntityMapper::new();
        assert_eq!(mapper.sanitize_device_id("device:123"), "device_123");
        assert_eq!(mapper.sanitize_device_id("device@123"), "device_123");
    }

    #[test]
    fn test_infer_data_type() {
        let mapper = HassEntityMapper::new();

        let float_entity = HassEntityState {
            entity_id: "sensor.temp".to_string(),
            state: "23.5".to_string(),
            attributes: crate::hass::entities::HassEntityAttributes::default(),
            last_changed: "".to_string(),
            last_updated: "".to_string(),
            context: None,
        };

        let bool_entity = HassEntityState {
            entity_id: "switch.test".to_string(),
            state: "on".to_string(),
            attributes: crate::hass::entities::HassEntityAttributes::default(),
            last_changed: "".to_string(),
            last_updated: "".to_string(),
            context: None,
        };

        assert_eq!(mapper.infer_data_type(&float_entity), MetricDataType::Float);
        assert_eq!(mapper.infer_data_type(&bool_entity), MetricDataType::Boolean);
    }
}
