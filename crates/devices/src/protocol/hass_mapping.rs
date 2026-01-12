//! Home Assistant (HASS) Protocol Mapping Implementation
//!
//! Maps device capabilities to Home Assistant entities and handles
//! the Home Assistant WebSocket/REST API.

use std::collections::HashMap;
use std::sync::Arc;
use crate::mdl::{MetricValue, MetricDataType};
use crate::protocol::mapping::{
    ProtocolMapping, Address, MappingConfig, MappingResult,
    MappingError,
};

/// HASS domain (platform) types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HassDomain {
    Sensor,
    BinarySensor,
    Switch,
    Light,
    Cover,
    Climate,
    Camera,
    InputBoolean,
    InputNumber,
    Number,
    Select,
    Text,
}

impl HassDomain {
    /// Parse from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "sensor" => Some(Self::Sensor),
            "binary_sensor" => Some(Self::BinarySensor),
            "switch" => Some(Self::Switch),
            "light" => Some(Self::Light),
            "cover" => Some(Self::Cover),
            "climate" => Some(Self::Climate),
            "camera" => Some(Self::Camera),
            "input_boolean" => Some(Self::InputBoolean),
            "input_number" => Some(Self::InputNumber),
            "number" => Some(Self::Number),
            "select" => Some(Self::Select),
            "text" => Some(Self::Text),
            _ => None,
        }
    }

    /// Get the string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Sensor => "sensor",
            Self::BinarySensor => "binary_sensor",
            Self::Switch => "switch",
            Self::Light => "light",
            Self::Cover => "cover",
            Self::Climate => "climate",
            Self::Camera => "camera",
            Self::InputBoolean => "input_boolean",
            Self::InputNumber => "input_number",
            Self::Number => "number",
            Self::Select => "select",
            Self::Text => "text",
        }
    }
}

/// HASS entity ID (e.g., "sensor.temperature_1").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HassEntityId {
    /// Domain/platform
    pub domain: HassDomain,
    /// Entity identifier (without domain prefix)
    pub entity: String,
}

impl HassEntityId {
    /// Parse from entity ID string.
    pub fn parse(entity_id: &str) -> Option<Self> {
        let parts: Vec<&str> = entity_id.split('.').collect();
        if parts.len() != 2 {
            return None;
        }

        let domain = HassDomain::from_str(parts[0])?;
        Some(Self {
            domain,
            entity: parts[1].to_string(),
        })
    }

    /// Create from domain and entity name.
    pub fn new(domain: HassDomain, entity: impl Into<String>) -> Self {
        Self {
            domain,
            entity: entity.into(),
        }
    }

    /// Get the full entity ID string.
    pub fn as_string(&self) -> String {
        format!("{}.{}", self.domain.as_str(), self.entity)
    }

    /// Get a specific attribute from this entity.
    pub fn attribute(&self, attr: impl Into<String>) -> Address {
        Address::Hass {
            entity_id: self.as_string(),
            attribute: Some(attr.into()),
        }
    }
}

impl std::fmt::Display for HassEntityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_string())
    }
}

/// HASS attribute or property access.
#[derive(Debug, Clone, PartialEq)]
pub enum HassValueAccess {
    /// Get the entity's state value
    State,
    /// Get a specific attribute
    Attribute(String),
    /// Get a nested property from attributes
    AttributePath(Vec<String>),
}

impl HassValueAccess {
    /// Create a state accessor.
    pub fn state() -> Self {
        Self::State
    }

    /// Create an attribute accessor.
    pub fn attribute(name: impl Into<String>) -> Self {
        Self::Attribute(name.into())
    }

    /// Create a nested attribute path accessor.
    pub fn attribute_path(path: Vec<impl Into<String>>) -> Self {
        Self::AttributePath(path.into_iter().map(|s| s.into()).collect())
    }
}

/// Configuration for a single HASS metric mapping.
#[derive(Debug, Clone)]
pub struct HassMetricMapping {
    /// Entity ID
    pub entity_id: String,
    /// How to access the value
    pub access: HassValueAccess,
    /// Data type override
    pub data_type: Option<MetricDataType>,
    /// Unit conversion (from HASS unit to target unit)
    pub unit_conversion: Option<UnitConversion>,
}

/// Unit conversion types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitConversion {
    CelsiusToFahrenheit,
    FahrenheitToCelsius,
    None,
}

impl UnitConversion {
    /// Apply conversion to a value.
    pub fn apply(&self, value: f64) -> f64 {
        match self {
            Self::CelsiusToFahrenheit => value * 9.0 / 5.0 + 32.0,
            Self::FahrenheitToCelsius => (value - 32.0) * 5.0 / 9.0,
            Self::None => value,
        }
    }
}

/// Configuration for a single HASS command mapping.
#[derive(Debug, Clone)]
pub struct HassCommandMapping {
    /// Entity ID to control
    pub entity_id: String,
    /// Service to call (e.g., "turn_on", "set_temperature")
    pub service: String,
    /// Service domain (defaults to entity's domain)
    pub service_domain: Option<String>,
    /// Parameter mapping (command params -> service data)
    pub param_mapping: HashMap<String, String>,
}

/// HASS protocol mapping configuration.
#[derive(Debug, Clone)]
pub struct HassMappingConfig {
    /// Device type this mapping is for
    pub device_type: String,
    /// Metric name -> HASS entity mapping
    pub metric_mappings: HashMap<String, HassMetricMapping>,
    /// Command name -> HASS service mapping
    pub command_mappings: HashMap<String, HassCommandMapping>,
}

/// HASS protocol mapping implementation.
pub struct HassMapping {
    config: HassMappingConfig,
}

impl HassMapping {
    /// Create a new HASS mapping from configuration.
    pub fn new(config: HassMappingConfig) -> Self {
        Self { config }
    }

    /// Parse a HASS state value (string) to appropriate MetricValue.
    fn parse_state_value(value: &str, data_type: Option<MetricDataType>) -> MappingResult<MetricValue> {
        let target_type = data_type.unwrap_or(MetricDataType::String);

        match target_type {
            MetricDataType::Boolean | MetricDataType::Binary => {
                match value.to_lowercase().as_str() {
                    "on" | "true" | "yes" | "1" => Ok(MetricValue::Boolean(true)),
                    "off" | "false" | "no" | "0" => Ok(MetricValue::Boolean(false)),
                    _ => Ok(MetricValue::Boolean(!value.is_empty())),
                }
            }
            MetricDataType::Integer => {
                value.parse::<i64>()
                    .map(MetricValue::Integer)
                    .map_err(|_| MappingError::ParseError(format!("Not an integer: {}", value)))
            }
            MetricDataType::Float => {
                value.parse::<f64>()
                    .map(MetricValue::Float)
                    .map_err(|_| MappingError::ParseError(format!("Not a float: {}", value)))
            }
            MetricDataType::String => Ok(MetricValue::String(value.to_string())),
        }
    }
}

impl ProtocolMapping for HassMapping {
    fn protocol_type(&self) -> &'static str {
        "hass"
    }

    fn device_type(&self) -> &str {
        &self.config.device_type
    }

    fn metric_address(&self, capability_name: &str) -> Option<Address> {
        self.config.metric_mappings.get(capability_name)
            .map(|mapping| match &mapping.access {
                HassValueAccess::State => Address::Hass {
                    entity_id: mapping.entity_id.clone(),
                    attribute: None,
                },
                HassValueAccess::Attribute(attr) => Address::Hass {
                    entity_id: mapping.entity_id.clone(),
                    attribute: Some(attr.clone()),
                },
                HassValueAccess::AttributePath(path) => Address::Hass {
                    entity_id: mapping.entity_id.clone(),
                    attribute: Some(path.join(".")),
                },
            })
    }

    fn command_address(&self, command_name: &str) -> Option<Address> {
        self.config.command_mappings.get(command_name)
            .map(|mapping| Address::Hass {
                entity_id: format!("service:{}/{}", mapping.service_domain.clone()
                    .unwrap_or_else(|| {
                        // Extract domain from entity_id
                        mapping.entity_id.split('.').next().unwrap_or("homeassistant").to_string()
                    }), mapping.service),
                attribute: None,
            })
    }

    fn parse_metric(&self, capability_name: &str, raw_data: &[u8]) -> MappingResult<MetricValue> {
        let mapping = self.config.metric_mappings.get(capability_name)
            .ok_or_else(|| MappingError::CapabilityNotFound(capability_name.to_string()))?;

        // HASS data is typically JSON with a "state" field and "attributes" object
        let json: serde_json::Value = serde_json::from_slice(raw_data)
            .map_err(|e| MappingError::ParseError(format!("Invalid HASS JSON: {}", e)))?;

        let value_str = match &mapping.access {
            HassValueAccess::State => {
                json.get("state")
                    .or_else(|| json.get("value"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| MappingError::ParseError("No state in HASS response".to_string()))?
            }
            HassValueAccess::Attribute(attr) => {
                json.get("attributes")
                    .and_then(|attrs| attrs.get(attr))
                    .and_then(|v| v.as_str())
                    .or_else(|| json.get(attr).and_then(|v| v.as_str()))
                    .ok_or_else(|| MappingError::ParseError(
                        format!("Attribute '{}' not found", attr)
                    ))?
            }
            HassValueAccess::AttributePath(path) => {
                let mut current = json.get("attributes").unwrap_or(&json);
                for segment in path {
                    current = current.get(segment)
                        .ok_or_else(|| MappingError::ParseError(
                            format!("Path segment '{}' not found", segment)
                        ))?;
                }
                current.as_str()
                    .ok_or_else(|| MappingError::ParseError("Value is not a string".to_string()))?
            }
        };

        let mut value = Self::parse_state_value(value_str, mapping.data_type)?;

        // Apply unit conversion if configured
        if let Some(conversion) = mapping.unit_conversion {
            if let MetricValue::Float(f) = value {
                value = MetricValue::Float(conversion.apply(f));
            }
        }

        Ok(value)
    }

    fn serialize_command(
        &self,
        command_name: &str,
        params: &HashMap<String, MetricValue>,
    ) -> MappingResult<Vec<u8>> {
        let mapping = self.config.command_mappings.get(command_name)
            .ok_or_else(|| MappingError::CommandNotFound(command_name.to_string()))?;

        // Build service data from parameters
        let mut service_data = serde_json::Map::new();

        // Add entity_id
        service_data.insert(
            "entity_id".to_string(),
            serde_json::Value::String(mapping.entity_id.clone())
        );

        // Map parameters according to param_mapping
        for (param_key, service_key) in &mapping.param_mapping {
            if let Some(value) = params.get(param_key) {
                let json_value = match value {
                    MetricValue::String(s) => serde_json::Value::String(s.clone()),
                    MetricValue::Integer(i) => serde_json::Value::Number(
                        serde_json::Number::from(*i)
                    ),
                    MetricValue::Float(f) => serde_json::Value::Number(
                        serde_json::Number::from_f64(*f)
                            .unwrap_or_else(|| serde_json::Number::from(0))
                    ),
                    MetricValue::Boolean(b) => serde_json::Value::Bool(*b),
                    MetricValue::Null => serde_json::Value::Null,
                    MetricValue::Binary(_) => serde_json::Value::String("<binary>".to_string()),
                };
                service_data.insert(service_key.clone(), json_value);
            }
        }

        // If no explicit mapping, copy all params
        if mapping.param_mapping.is_empty() {
            for (key, value) in params {
                let json_value = match value {
                    MetricValue::String(s) => serde_json::Value::String(s.clone()),
                    MetricValue::Integer(i) => serde_json::Value::Number(
                        serde_json::Number::from(*i)
                    ),
                    MetricValue::Float(f) => serde_json::Value::Number(
                        serde_json::Number::from_f64(*f)
                            .unwrap_or_else(|| serde_json::Number::from(0))
                    ),
                    MetricValue::Boolean(b) => serde_json::Value::Bool(*b),
                    MetricValue::Null => serde_json::Value::Null,
                    MetricValue::Binary(_) => serde_json::Value::String("<binary>".to_string()),
                };
                service_data.insert(key.clone(), json_value);
            }
        }

        serde_json::to_vec(&service_data)
            .map_err(|e| MappingError::SerializationError(format!("{}", e)))
    }

    fn mapped_capabilities(&self) -> Vec<String> {
        self.config.metric_mappings.keys().cloned().collect()
    }

    fn mapped_commands(&self) -> Vec<String> {
        self.config.command_mappings.keys().cloned().collect()
    }
}

/// Builder for creating HASS mappings.
pub struct HassMappingBuilder {
    device_type: String,
    metric_mappings: HashMap<String, HassMetricMapping>,
    command_mappings: HashMap<String, HassCommandMapping>,
}

impl HassMappingBuilder {
    /// Create a new builder for a device type.
    pub fn new(device_type: impl Into<String>) -> Self {
        Self {
            device_type: device_type.into(),
            metric_mappings: HashMap::new(),
            command_mappings: HashMap::new(),
        }
    }

    /// Add a sensor entity mapping.
    pub fn add_sensor(
        mut self,
        name: impl Into<String>,
        entity_id: impl Into<String>,
    ) -> Self {
        self.metric_mappings.insert(
            name.into(),
            HassMetricMapping {
                entity_id: entity_id.into(),
                access: HassValueAccess::State,
                data_type: None,
                unit_conversion: None,
            },
        );
        self
    }

    /// Add an attribute mapping.
    pub fn add_attribute(
        mut self,
        name: impl Into<String>,
        entity_id: impl Into<String>,
        attribute: impl Into<String>,
    ) -> Self {
        self.metric_mappings.insert(
            name.into(),
            HassMetricMapping {
                entity_id: entity_id.into(),
                access: HassValueAccess::Attribute(attribute.into()),
                data_type: None,
                unit_conversion: None,
            },
        );
        self
    }

    /// Add a switch command (turn_on/turn_off).
    pub fn add_switch_command(
        mut self,
        name: impl Into<String>,
        entity_id: impl Into<String>,
        service: impl Into<String>,
    ) -> Self {
        let entity = entity_id.into();
        let domain = entity.split('.').next().unwrap_or("switch").to_string();

        self.command_mappings.insert(
            name.into(),
            HassCommandMapping {
                entity_id: entity,
                service: service.into(),
                service_domain: Some(domain),
                param_mapping: HashMap::new(),
            },
        );
        self
    }

    /// Add a command with parameter mapping.
    pub fn add_command_with_params(
        mut self,
        name: impl Into<String>,
        entity_id: impl Into<String>,
        service: impl Into<String>,
        service_domain: Option<String>,
        param_mapping: HashMap<String, String>,
    ) -> Self {
        self.command_mappings.insert(
            name.into(),
            HassCommandMapping {
                entity_id: entity_id.into(),
                service: service.into(),
                service_domain,
                param_mapping,
            },
        );
        self
    }

    /// Build the mapping.
    pub fn build(self) -> HassMapping {
        HassMapping::new(HassMappingConfig {
            device_type: self.device_type,
            metric_mappings: self.metric_mappings,
            command_mappings: self.command_mappings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hass_entity_id_parse() {
        let entity = HassEntityId::parse("sensor.temperature_1").unwrap();
        assert_eq!(entity.domain, HassDomain::Sensor);
        assert_eq!(entity.entity, "temperature_1");
        assert_eq!(entity.as_string(), "sensor.temperature_1");
    }

    #[test]
    fn test_hass_entity_id_new() {
        let entity = HassEntityId::new(HassDomain::Sensor, "temperature_1");
        assert_eq!(entity.as_string(), "sensor.temperature_1");
    }

    #[test]
    fn test_hass_entity_id_attribute() {
        let entity = HassEntityId::new(HassDomain::Sensor, "temperature_1");
        let addr = entity.attribute("unit_of_measurement");
        assert!(matches!(addr, Address::Hass { entity_id, attribute: Some(_) }
            if entity_id == "sensor.temperature_1"));
    }

    #[test]
    fn test_hass_domain_from_str() {
        assert_eq!(HassDomain::from_str("sensor"), Some(HassDomain::Sensor));
        assert_eq!(HassDomain::from_str("binary_sensor"), Some(HassDomain::BinarySensor));
        assert_eq!(HassDomain::from_str("switch"), Some(HassDomain::Switch));
        assert_eq!(HassDomain::from_str("unknown"), None);
    }

    #[test]
    fn test_builder_pattern() {
        let mapping = HassMappingBuilder::new("climate_sensor")
            .add_sensor("temperature", "sensor.indoor_temperature")
            .add_sensor("humidity", "sensor.indoor_humidity")
            .add_switch_command("turn_on_fan", "switch.bedroom_fan", "turn_on")
            .add_switch_command("turn_off_fan", "switch.bedroom_fan", "turn_off")
            .build();

        assert_eq!(mapping.device_type(), "climate_sensor");
        assert_eq!(mapping.mapped_capabilities().len(), 2);
        assert_eq!(mapping.mapped_commands().len(), 2);
    }

    #[test]
    fn test_parse_state_boolean() {
        assert!(matches!(
            HassMapping::parse_state_value("on", Some(MetricDataType::Boolean)),
            Ok(MetricValue::Boolean(true))
        ));
        assert!(matches!(
            HassMapping::parse_state_value("off", Some(MetricDataType::Boolean)),
            Ok(MetricValue::Boolean(false))
        ));
        assert!(matches!(
            HassMapping::parse_state_value("true", Some(MetricDataType::Boolean)),
            Ok(MetricValue::Boolean(true))
        ));
    }

    #[test]
    fn test_parse_state_number() {
        assert!(matches!(
            HassMapping::parse_state_value("23.5", Some(MetricDataType::Float)),
            Ok(MetricValue::Float(23.5))
        ));
        assert!(matches!(
            HassMapping::parse_state_value("42", Some(MetricDataType::Integer)),
            Ok(MetricValue::Integer(42))
        ));
    }

    #[test]
    fn test_serialize_command_simple() {
        let mapping = HassMappingBuilder::new("test")
            .add_switch_command("toggle", "switch.test", "toggle")
            .build();

        let result = mapping.serialize_command("toggle", &HashMap::new()).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(json["entity_id"], "switch.test");
    }

    #[test]
    fn test_serialize_command_with_params() {
        let mut param_map = HashMap::new();
        param_map.insert("brightness".to_string(), MetricValue::Integer(255));

        let mapping = HassMappingBuilder::new("test")
            .add_command_with_params(
                "set_brightness",
                "light.test",
                "turn_on",
                Some("light".to_string()),
                [("brightness".to_string(), "brightness".to_string())].into(),
            )
            .build();

        let result = mapping.serialize_command("set_brightness", &param_map).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(json["entity_id"], "light.test");
        assert_eq!(json["brightness"], 255);
    }

    #[test]
    fn test_parse_hass_state_response() {
        let mapping = HassMappingBuilder::new("test")
            .add_sensor("temp", "sensor.test")
            .build();

        let data = r#"{"state": "23.5", "attributes": {"unit_of_measurement": "C"}}"#.as_bytes();
        let result = mapping.parse_metric("temp", data);
        // When data_type is not specified, it defaults to String
        assert!(matches!(result, Ok(MetricValue::String(s)) if s == "23.5"));
    }

    #[test]
    fn test_parse_hass_attribute() {
        let mapping = HassMappingBuilder::new("test")
            .add_attribute("unit", "sensor.test", "unit_of_measurement")
            .build();

        let data = r#"{"state": "23.5", "attributes": {"unit_of_measurement": "C"}}"#.as_bytes();
        let result = mapping.parse_metric("unit", data);
        if let Ok(MetricValue::String(s)) = result {
            assert_eq!(s, "C");
        } else {
            panic!("Expected String value");
        }
    }
}
