//! Protocol Mapping Trait and Core Types
//!
//! Defines the abstraction layer for mapping device capabilities to
//! protocol-specific addresses and data formats.

use std::collections::HashMap;
use std::sync::Arc;
use crate::mdl::{MetricValue, MetricDataType};
use serde_json::Value as JsonValue;

/// Protocol-agnostic device capability definition.
///
/// Describes what a device can do without specifying how to communicate
/// with it (protocol-specific details are handled by ProtocolMapping).
#[derive(Debug, Clone, PartialEq)]
pub struct Capability {
    /// Unique identifier for this capability
    pub name: String,
    /// Human-readable display name
    pub display_name: String,
    /// Type of capability (sensor, actuator, etc.)
    pub capability_type: CapabilityType,
    /// Data type for values
    pub data_type: MetricDataType,
    /// Unit of measurement (for numeric types)
    pub unit: Option<String>,
    /// Minimum valid value (for numeric types)
    pub min: Option<f64>,
    /// Maximum valid value (for numeric types)
    pub max: Option<f64>,
    /// Allowed values (for enum-like types)
    pub allowed_values: Option<Vec<MetricValue>>,
}

/// Type of device capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityType {
    /// Read-only sensor data
    Sensor,
    /// Writeable actuator control
    Actuator,
    /// Read and write capability
    Bidirectional,
    /// Command execution
    Command,
}

impl Capability {
    /// Create a new sensor capability.
    pub fn sensor(
        name: impl Into<String>,
        display_name: impl Into<String>,
        data_type: MetricDataType,
    ) -> Self {
        Self {
            name: name.into(),
            display_name: display_name.into(),
            capability_type: CapabilityType::Sensor,
            data_type,
            unit: None,
            min: None,
            max: None,
            allowed_values: None,
        }
    }

    /// Create a new actuator capability.
    pub fn actuator(
        name: impl Into<String>,
        display_name: impl Into<String>,
        data_type: MetricDataType,
    ) -> Self {
        Self {
            name: name.into(),
            display_name: display_name.into(),
            capability_type: CapabilityType::Actuator,
            data_type,
            unit: None,
            min: None,
            max: None,
            allowed_values: None,
        }
    }

    /// Set the unit for this capability.
    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// Set the range for this capability.
    pub fn with_range(mut self, min: f64, max: f64) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    /// Set allowed values for this capability.
    pub fn with_allowed_values(mut self, values: Vec<MetricValue>) -> Self {
        self.allowed_values = Some(values);
        self
    }
}

/// Protocol address - abstracts addressing for different protocols.
#[derive(Debug, Clone, PartialEq)]
pub enum Address {
    /// MQTT topic address
    MQTT {
        topic: String,
        qos: Option<u8>,
        retain: Option<bool>,
    },
    /// Modbus register address
    Modbus {
        slave: u8,
        register: u16,
        register_type: ModbusRegisterType,
        count: Option<u16>,
    },
    /// Home Assistant entity ID
    Hass {
        entity_id: String,
        attribute: Option<String>,
    },
    /// HTTP endpoint
    Http {
        url: String,
        method: String,
        headers: Option<HashMap<String, String>>,
    },
    /// Generic/custom address
    Custom {
        protocol: String,
        address: String,
        params: HashMap<String, String>,
    },
}

/// Modbus register types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModbusRegisterType {
    /// Coil (read-write, 1 bit)
    Coil,
    /// Discrete input (read-only, 1 bit)
    DiscreteInput,
    /// Holding register (read-write, 16 bits)
    HoldingRegister,
    /// Input register (read-only, 16 bits)
    InputRegister,
}

impl Address {
    /// Create an MQTT address.
    pub fn mqtt(topic: impl Into<String>) -> Self {
        Self::MQTT {
            topic: topic.into(),
            qos: None,
            retain: None,
        }
    }

    /// Create a Modbus address.
    pub fn modbus(slave: u8, register: u16, register_type: ModbusRegisterType) -> Self {
        Self::Modbus {
            slave,
            register,
            register_type,
            count: None,
        }
    }

    /// Create a Home Assistant address.
    pub fn hass(entity_id: impl Into<String>) -> Self {
        Self::Hass {
            entity_id: entity_id.into(),
            attribute: None,
        }
    }

    /// Create an HTTP address.
    pub fn http(url: impl Into<String>, method: impl Into<String>) -> Self {
        Self::Http {
            url: url.into(),
            method: method.into(),
            headers: None,
        }
    }
}

/// Result type for protocol mapping operations.
pub type MappingResult<T> = Result<T, MappingError>;

/// Errors that can occur during protocol mapping operations.
#[derive(Debug, thiserror::Error)]
pub enum MappingError {
    /// Capability not found in mapping
    #[error("Capability not found: {0}")]
    CapabilityNotFound(String),

    /// Command not found in mapping
    #[error("Command not found: {0}")]
    CommandNotFound(String),

    /// Invalid data format
    #[error("Invalid data format: {0}")]
    InvalidFormat(String),

    /// Parse error
    #[error("Parse error: {0}")]
    ParseError(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Template rendering error
    #[error("Template error: {0}")]
    TemplateError(String),

    /// Address resolution error
    #[error("Cannot resolve address: {0}")]
    AddressError(String),

    /// Protocol-specific error
    #[error("Protocol error: {0}")]
    ProtocolError(String),
}

/// Metric parser - extracts values from raw protocol data.
pub trait MetricParser: Send + Sync {
    /// Parse raw bytes into a MetricValue.
    fn parse(&self, data: &[u8], data_type: MetricDataType) -> MappingResult<MetricValue>;
}

/// Payload serializer - converts command parameters to protocol format.
pub trait PayloadSerializer: Send + Sync {
    /// Serialize command parameters into a sendable payload.
    fn serialize(
        &self,
        params: &HashMap<String, MetricValue>,
        template: &Option<String>,
    ) -> MappingResult<Vec<u8>>;
}

/// Protocol mapping trait - bridges device capabilities with protocol implementations.
///
/// Each protocol (MQTT, Modbus, HASS, etc.) implements this trait to provide:
/// 1. Address resolution for capabilities and commands
/// 2. Data parsing from protocol format to MetricValue
/// 3. Command serialization from MetricValue to protocol format
pub trait ProtocolMapping: Send + Sync {
    /// Get the protocol type identifier.
    fn protocol_type(&self) -> &'static str;

    /// Get the device type this mapping is for.
    fn device_type(&self) -> &str;

    /// Resolve the address for reading a metric/capability.
    ///
    /// Returns None if the capability is not mapped for this protocol.
    fn metric_address(&self, capability_name: &str) -> Option<Address>;

    /// Resolve the address for executing a command.
    ///
    /// Returns None if the command is not mapped for this protocol.
    fn command_address(&self, command_name: &str) -> Option<Address>;

    /// Parse raw protocol data into a MetricValue.
    ///
    /// This handles protocol-specific data formats (JSON, binary, etc.)
    fn parse_metric(
        &self,
        capability_name: &str,
        raw_data: &[u8],
    ) -> MappingResult<MetricValue>;

    /// Serialize command parameters into a protocol-specific payload.
    ///
    /// This generates the bytes that should be sent to execute a command.
    fn serialize_command(
        &self,
        command_name: &str,
        params: &HashMap<String, MetricValue>,
    ) -> MappingResult<Vec<u8>>;

    /// Get all capability names mapped for this protocol.
    fn mapped_capabilities(&self) -> Vec<String>;

    /// Get all command names mapped for this protocol.
    fn mapped_commands(&self) -> Vec<String>;

    /// Check if a capability is mapped.
    fn has_capability(&self, capability_name: &str) -> bool {
        self.metric_address(capability_name).is_some()
    }

    /// Check if a command is mapped.
    fn has_command(&self, command_name: &str) -> bool {
        self.command_address(command_name).is_some()
    }
}

/// Configuration for creating a protocol mapping.
#[derive(Debug, Clone)]
pub struct MappingConfig {
    /// Protocol type (mqtt, modbus, hass, etc.)
    pub protocol: String,
    /// Device type this mapping is for
    pub device_type: String,
    /// Metric mappings: capability name -> address template
    pub metric_mappings: HashMap<String, MetricMappingConfig>,
    /// Command mappings: command name -> command config
    pub command_mappings: HashMap<String, CommandMappingConfig>,
}

/// Configuration for a single metric mapping.
#[derive(Debug, Clone)]
pub struct MetricMappingConfig {
    /// Address template (supports ${device_id} variable)
    pub address_template: String,
    /// Parser type (json, binary, string, etc.)
    pub parser_type: String,
    /// JSON path for extracting value (for JSON payloads)
    pub value_path: Option<String>,
    /// Data type override
    pub data_type: Option<MetricDataType>,
}

/// Configuration for a single command mapping.
#[derive(Debug, Clone)]
pub struct CommandMappingConfig {
    /// Address template (supports ${device_id} variable)
    pub address_template: String,
    /// Payload template (supports variable substitution)
    pub payload_template: Option<String>,
    /// Response parser type
    pub response_parser: Option<String>,
}

impl MappingConfig {
    /// Create a new mapping config.
    pub fn new(
        protocol: impl Into<String>,
        device_type: impl Into<String>,
    ) -> Self {
        Self {
            protocol: protocol.into(),
            device_type: device_type.into(),
            metric_mappings: HashMap::new(),
            command_mappings: HashMap::new(),
        }
    }

    /// Add a metric mapping.
    pub fn with_metric_mapping(
        mut self,
        capability: impl Into<String>,
        address: impl Into<String>,
    ) -> Self {
        self.metric_mappings.insert(
            capability.into(),
            MetricMappingConfig {
                address_template: address.into(),
                parser_type: "auto".to_string(),
                value_path: None,
                data_type: None,
            },
        );
        self
    }

    /// Add a command mapping.
    pub fn with_command_mapping(
        mut self,
        command: impl Into<String>,
        address: impl Into<String>,
    ) -> Self {
        self.command_mappings.insert(
            command.into(),
            CommandMappingConfig {
                address_template: address.into(),
                payload_template: None,
                response_parser: None,
            },
        );
        self
    }

    /// Render a template by replacing ${device_id} with actual device ID.
    pub fn render_template(template: &str, device_id: &str) -> String {
        template.replace("${device_id}", device_id)
            .replace("${id}", device_id)
    }

    /// Render a template with additional variables.
    pub fn render_template_with_vars(
        template: &str,
        device_id: &str,
        vars: &HashMap<String, String>,
    ) -> String {
        let mut result = Self::render_template(template, device_id);
        for (key, value) in vars {
            result = result.replace(&format!("${{{}}}", key), value);
        }
        result
    }
}

/// Shared protocol mapping reference.
pub type SharedMapping = Arc<dyn ProtocolMapping>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_sensor() {
        let temp = Capability::sensor("temperature", "温度", MetricDataType::Float)
            .with_unit("°C")
            .with_range(-40.0, 80.0);

        assert_eq!(temp.name, "temperature");
        assert_eq!(temp.display_name, "温度");
        assert_eq!(temp.data_type, MetricDataType::Float);
        assert_eq!(temp.unit.as_deref(), Some("°C"));
        assert_eq!(temp.min, Some(-40.0));
        assert_eq!(temp.max, Some(80.0));
    }

    #[test]
    fn test_capability_actuator() {
        let relay = Capability::actuator("relay_state", "继电器状态", MetricDataType::Boolean);

        assert_eq!(relay.name, "relay_state");
        assert_eq!(relay.capability_type, CapabilityType::Actuator);
    }

    #[test]
    fn test_address_mqtt() {
        let addr = Address::mqtt("sensor/${device_id}/temperature");
        assert!(matches!(addr, Address::MQTT { .. }));
    }

    #[test]
    fn test_address_modbus() {
        let addr = Address::modbus(1, 0x0102, ModbusRegisterType::HoldingRegister);
        assert!(matches!(addr, Address::Modbus { .. }));
    }

    #[test]
    fn test_template_rendering() {
        let template = "sensor/${device_id}/temperature";
        let rendered = MappingConfig::render_template(template, "dht22_01");
        assert_eq!(rendered, "sensor/dht22_01/temperature");
    }

    #[test]
    fn test_template_rendering_with_vars() {
        let template = "${protocol}/${device_id}/${channel}";
        let mut vars = HashMap::new();
        vars.insert("channel".to_string(), "temperature".to_string());

        let rendered = MappingConfig::render_template_with_vars(template, "sensor01", &vars);
        assert_eq!(rendered, "${protocol}/sensor01/temperature");
    }

    #[test]
    fn test_mapping_config_builder() {
        let config = MappingConfig::new("mqtt", "dht22_sensor")
            .with_metric_mapping("temperature", "sensor/${device_id}/temperature")
            .with_metric_mapping("humidity", "sensor/${device_id}/humidity")
            .with_command_mapping("set_interval", "sensor/${device_id}/command");

        assert_eq!(config.device_type, "dht22_sensor");
        assert_eq!(config.metric_mappings.len(), 2);
        assert_eq!(config.command_mappings.len(), 1);
    }
}
