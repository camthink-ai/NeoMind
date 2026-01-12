//! Modbus Protocol Mapping Implementation
//!
//! Maps device capabilities to Modbus registers and handles data type conversions.

use std::collections::HashMap;
use std::sync::Arc;
use crate::mdl::{MetricValue, MetricDataType};
use crate::protocol::mapping::{
    ProtocolMapping, Address, MappingConfig, MappingResult,
    MappingError, ModbusRegisterType,
};

/// Modbus register mapping configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModbusDataType {
    /// 16-bit integer (1 register)
    Int16,
    /// 32-bit integer (2 registers)
    Int32,
    /// 16-bit unsigned integer (1 register)
    Uint16,
    /// 32-bit unsigned integer (2 registers)
    Uint32,
    /// 32-bit float (2 registers)
    Float32,
    /// 64-bit float (4 registers)
    Float64,
    /// Boolean (coil/discrete input)
    Bool,
}

impl ModbusDataType {
    /// Get the number of registers required for this data type.
    pub fn register_count(&self) -> u16 {
        match self {
            Self::Int16 | Self::Uint16 | Self::Bool => 1,
            Self::Int32 | Self::Uint32 | Self::Float32 => 2,
            Self::Float64 => 4,
        }
    }

    /// Parse from bytes (big-endian Modbus standard).
    pub fn from_be_bytes(&self, data: &[u8]) -> MappingResult<MetricValue> {
        match self {
            Self::Int16 => {
                if data.len() < 2 {
                    return Err(MappingError::ParseError("Insufficient data for i16".to_string()));
                }
                let value = i16::from_be_bytes([data[0], data[1]]);
                Ok(MetricValue::Integer(value as i64))
            }
            Self::Int32 => {
                if data.len() < 4 {
                    return Err(MappingError::ParseError("Insufficient data for i32".to_string()));
                }
                let value = i32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                Ok(MetricValue::Integer(value as i64))
            }
            Self::Uint16 => {
                if data.len() < 2 {
                    return Err(MappingError::ParseError("Insufficient data for u16".to_string()));
                }
                let value = u16::from_be_bytes([data[0], data[1]]);
                Ok(MetricValue::Integer(value as i64))
            }
            Self::Uint32 => {
                if data.len() < 4 {
                    return Err(MappingError::ParseError("Insufficient data for u32".to_string()));
                }
                let value = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                Ok(MetricValue::Integer(value as i64))
            }
            Self::Float32 => {
                if data.len() < 4 {
                    return Err(MappingError::ParseError("Insufficient data for f32".to_string()));
                }
                let bytes: [u8; 4] = [data[0], data[1], data[2], data[3]];
                let value = f32::from_be_bytes(bytes);
                Ok(MetricValue::Float(value as f64))
            }
            Self::Float64 => {
                if data.len() < 8 {
                    return Err(MappingError::ParseError("Insufficient data for f64".to_string()));
                }
                let bytes: [u8; 8] = [
                    data[0], data[1], data[2], data[3],
                    data[4], data[5], data[6], data[7],
                ];
                let value = f64::from_be_bytes(bytes);
                Ok(MetricValue::Float(value))
            }
            Self::Bool => {
                if data.is_empty() {
                    return Err(MappingError::ParseError("No data for bool".to_string()));
                }
                Ok(MetricValue::Boolean(data[0] != 0))
            }
        }
    }

    /// Serialize to bytes (big-endian Modbus standard).
    pub fn to_be_bytes(&self, value: &MetricValue) -> MappingResult<Vec<u8>> {
        match (self, value) {
            (Self::Int16, MetricValue::Integer(v)) if *v >= i16::MIN as i64 && *v <= i16::MAX as i64 => {
                Ok((*v as i16).to_be_bytes().to_vec())
            }
            (Self::Int32, MetricValue::Integer(v)) if *v >= i32::MIN as i64 && *v <= i32::MAX as i64 => {
                Ok((*v as i32).to_be_bytes().to_vec())
            }
            (Self::Uint16, MetricValue::Integer(v)) if *v >= 0 && *v <= u16::MAX as i64 => {
                Ok((*v as u16).to_be_bytes().to_vec())
            }
            (Self::Uint32, MetricValue::Integer(v)) if *v >= 0 && *v <= u32::MAX as i64 => {
                Ok((*v as u32).to_be_bytes().to_vec())
            }
            (Self::Float32, MetricValue::Float(v)) => {
                Ok((*v as f32).to_be_bytes().to_vec())
            }
            (Self::Float64, MetricValue::Float(v)) => {
                Ok(v.to_be_bytes().to_vec())
            }
            (Self::Bool, MetricValue::Boolean(v)) => {
                Ok(vec![if *v { 1 } else { 0 }])
            }
            (_, v) => Err(MappingError::SerializationError(format!(
                "Cannot convert {:?} to {:?}", v, self
            ))),
        }
    }
}

/// Configuration for a single Modbus metric mapping.
#[derive(Debug, Clone)]
pub struct ModbusMetricMapping {
    /// Slave/unit ID
    pub slave: u8,
    /// Register address
    pub register: u16,
    /// Register type
    pub register_type: ModbusRegisterType,
    /// Data type for parsing
    pub data_type: ModbusDataType,
    /// Register count (auto-calculated from data_type if None)
    pub count: Option<u16>,
    /// Scale factor (value is multiplied by this after reading)
    pub scale: Option<f64>,
    /// Offset (added after scaling)
    pub offset: Option<f64>,
}

/// Configuration for a single Modbus command mapping.
#[derive(Debug, Clone)]
pub struct ModbusCommandMapping {
    /// Slave/unit ID
    pub slave: u8,
    /// Register address
    pub register: u16,
    /// Register type
    pub register_type: ModbusRegisterType,
    /// Data type for serialization
    pub data_type: ModbusDataType,
    /// Parameter to register mapping
    pub param_mapping: HashMap<String, ModbusRegisterMapping>,
}

/// Maps a command parameter to a specific register.
#[derive(Debug, Clone)]
pub struct ModbusRegisterMapping {
    pub register: u16,
    pub data_type: ModbusDataType,
}

/// Modbus protocol mapping configuration.
#[derive(Debug, Clone)]
pub struct ModbusMappingConfig {
    /// Device type this mapping is for
    pub device_type: String,
    /// Default slave ID
    pub default_slave: u8,
    /// Metric name -> Modbus address
    pub metric_mappings: HashMap<String, ModbusMetricMapping>,
    /// Command name -> Modbus command config
    pub command_mappings: HashMap<String, ModbusCommandMapping>,
}

/// Modbus protocol mapping implementation.
pub struct ModbusMapping {
    config: ModbusMappingConfig,
}

impl ModbusMapping {
    /// Create a new Modbus mapping from configuration.
    pub fn new(config: ModbusMappingConfig) -> Self {
        Self { config }
    }

    /// Get the register info for a metric.
    pub fn metric_register(&self, capability_name: &str) -> Option<&ModbusMetricMapping> {
        self.config.metric_mappings.get(capability_name)
    }

    /// Get the register info for a command.
    pub fn command_register(&self, command_name: &str) -> Option<&ModbusCommandMapping> {
        self.config.command_mappings.get(command_name)
    }

    /// Apply scale and offset to a value.
    fn apply_transform(
        value: f64,
        scale: Option<f64>,
        offset: Option<f64>,
    ) -> MetricValue {
        let mut result = value;
        if let Some(s) = scale {
            result *= s;
        }
        if let Some(o) = offset {
            result += o;
        }
        MetricValue::Float(result)
    }

    /// Reverse transform for writing.
    fn reverse_transform(
        value: &MetricValue,
        scale: Option<f64>,
        offset: Option<f64>,
    ) -> MetricValue {
        let base = match value {
            MetricValue::Float(f) => *f,
            MetricValue::Integer(i) => *i as f64,
            _ => return value.clone(),
        };

        let mut result = base;
        if let Some(o) = offset {
            result -= o;
        }
        if let Some(s) = scale {
            result /= s;
        }

        // Try to return as integer if close to whole number
        if (result - result.round()).abs() < 0.0001 {
            MetricValue::Integer(result.round() as i64)
        } else {
            MetricValue::Float(result)
        }
    }
}

impl ProtocolMapping for ModbusMapping {
    fn protocol_type(&self) -> &'static str {
        "modbus"
    }

    fn device_type(&self) -> &str {
        &self.config.device_type
    }

    fn metric_address(&self, capability_name: &str) -> Option<Address> {
        self.config.metric_mappings.get(capability_name)
            .map(|mapping| Address::Modbus {
                slave: mapping.slave,
                register: mapping.register,
                register_type: mapping.register_type,
                count: Some(mapping.data_type.register_count()),
            })
    }

    fn command_address(&self, command_name: &str) -> Option<Address> {
        self.config.command_mappings.get(command_name)
            .map(|mapping| Address::Modbus {
                slave: mapping.slave,
                register: mapping.register,
                register_type: mapping.register_type,
                count: None,
            })
    }

    fn parse_metric(&self, capability_name: &str, raw_data: &[u8]) -> MappingResult<MetricValue> {
        let mapping = self.config.metric_mappings.get(capability_name)
            .ok_or_else(|| MappingError::CapabilityNotFound(capability_name.to_string()))?;

        let value = mapping.data_type.from_be_bytes(raw_data)?;

        // Apply scale and offset if configured
        if let (Some(scale), Some(offset)) = (mapping.scale, mapping.offset) {
            match value {
                MetricValue::Float(f) => Ok(Self::apply_transform(f, Some(scale), Some(offset))),
                MetricValue::Integer(i) => Ok(Self::apply_transform(i as f64, Some(scale), Some(offset))),
                _ => Ok(value),
            }
        } else if let Some(scale) = mapping.scale {
            match value {
                MetricValue::Float(f) => Ok(Self::apply_transform(f, Some(scale), None)),
                MetricValue::Integer(i) => Ok(Self::apply_transform(i as f64, Some(scale), None)),
                _ => Ok(value),
            }
        } else if let Some(offset) = mapping.offset {
            match value {
                MetricValue::Float(f) => Ok(Self::apply_transform(f, None, Some(offset))),
                MetricValue::Integer(i) => Ok(Self::apply_transform(i as f64, None, Some(offset))),
                _ => Ok(value),
            }
        } else {
            Ok(value)
        }
    }

    fn serialize_command(
        &self,
        command_name: &str,
        params: &HashMap<String, MetricValue>,
    ) -> MappingResult<Vec<u8>> {
        let mapping = self.config.command_mappings.get(command_name)
            .ok_or_else(|| MappingError::CommandNotFound(command_name.to_string()))?;

        // For commands with parameter mapping, serialize the primary parameter
        if let Some((param_name, register_mapping)) = mapping.param_mapping.iter().next() {
            let value = params.get(param_name)
                .or_else(|| params.values().next())
                .ok_or_else(|| MappingError::SerializationError("No parameter provided".to_string()))?;

            register_mapping.data_type.to_be_bytes(value)
        } else {
            // Use the command's data type with the first parameter
            let value = params.values().next()
                .ok_or_else(|| MappingError::SerializationError("No parameter provided".to_string()))?;

            mapping.data_type.to_be_bytes(value)
        }
    }

    fn mapped_capabilities(&self) -> Vec<String> {
        self.config.metric_mappings.keys().cloned().collect()
    }

    fn mapped_commands(&self) -> Vec<String> {
        self.config.command_mappings.keys().cloned().collect()
    }
}

/// Builder for creating Modbus mappings.
pub struct ModbusMappingBuilder {
    device_type: String,
    default_slave: u8,
    metric_mappings: HashMap<String, ModbusMetricMapping>,
    command_mappings: HashMap<String, ModbusCommandMapping>,
}

impl ModbusMappingBuilder {
    /// Create a new builder for a device type.
    pub fn new(device_type: impl Into<String>) -> Self {
        Self {
            device_type: device_type.into(),
            default_slave: 1,
            metric_mappings: HashMap::new(),
            command_mappings: HashMap::new(),
        }
    }

    /// Set the default slave ID.
    pub fn default_slave(mut self, slave: u8) -> Self {
        self.default_slave = slave;
        self
    }

    /// Add a metric mapping (holding register).
    pub fn add_holding_register(
        mut self,
        name: impl Into<String>,
        register: u16,
        data_type: ModbusDataType,
    ) -> Self {
        self.metric_mappings.insert(
            name.into(),
            ModbusMetricMapping {
                slave: self.default_slave,
                register,
                register_type: ModbusRegisterType::HoldingRegister,
                data_type,
                count: None,
                scale: None,
                offset: None,
            },
        );
        self
    }

    /// Add a metric mapping (input register).
    pub fn add_input_register(
        mut self,
        name: impl Into<String>,
        register: u16,
        data_type: ModbusDataType,
    ) -> Self {
        self.metric_mappings.insert(
            name.into(),
            ModbusMetricMapping {
                slave: self.default_slave,
                register,
                register_type: ModbusRegisterType::InputRegister,
                data_type,
                count: None,
                scale: None,
                offset: None,
            },
        );
        self
    }

    /// Add a metric mapping with scale and offset.
    pub fn add_register_with_transform(
        mut self,
        name: impl Into<String>,
        register: u16,
        register_type: ModbusRegisterType,
        data_type: ModbusDataType,
        scale: f64,
        offset: f64,
    ) -> Self {
        self.metric_mappings.insert(
            name.into(),
            ModbusMetricMapping {
                slave: self.default_slave,
                register,
                register_type,
                data_type,
                count: None,
                scale: Some(scale),
                offset: Some(offset),
            },
        );
        self
    }

    /// Add a coil mapping.
    pub fn add_coil(mut self, name: impl Into<String>, register: u16) -> Self {
        self.metric_mappings.insert(
            name.into(),
            ModbusMetricMapping {
                slave: self.default_slave,
                register,
                register_type: ModbusRegisterType::Coil,
                data_type: ModbusDataType::Bool,
                count: None,
                scale: None,
                offset: None,
            },
        );
        self
    }

    /// Add a command mapping.
    pub fn add_command(
        mut self,
        name: impl Into<String>,
        register: u16,
        data_type: ModbusDataType,
    ) -> Self {
        self.command_mappings.insert(
            name.into(),
            ModbusCommandMapping {
                slave: self.default_slave,
                register,
                register_type: ModbusRegisterType::HoldingRegister,
                data_type,
                param_mapping: HashMap::new(),
            },
        );
        self
    }

    /// Build the mapping.
    pub fn build(self) -> ModbusMapping {
        ModbusMapping::new(ModbusMappingConfig {
            device_type: self.device_type,
            default_slave: self.default_slave,
            metric_mappings: self.metric_mappings,
            command_mappings: self.command_mappings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_pattern() {
        let mapping = ModbusMappingBuilder::new("energy_meter")
            .default_slave(1)
            .add_input_register("voltage", 0x0000, ModbusDataType::Float32)
            .add_input_register("current", 0x0002, ModbusDataType::Float32)
            .add_holding_register("power_alarm", 0x0100, ModbusDataType::Uint16)
            .add_coil("reset", 0x0200)
            .build();

        assert_eq!(mapping.device_type(), "energy_meter");
        assert_eq!(mapping.mapped_capabilities().len(), 4);
    }

    #[test]
    fn test_parse_int16() {
        let data: [u8; 2] = [0x00, 0x64]; // 100 in big-endian
        let result = ModbusDataType::Int16.from_be_bytes(&data);
        assert!(matches!(result, Ok(MetricValue::Integer(100))));
    }

    #[test]
    fn test_parse_float32() {
        let data: [u8; 4] = [0x41, 0xBE, 0x00, 0x00]; // 23.75 in big-endian
        let result = ModbusDataType::Float32.from_be_bytes(&data);
        if let Ok(MetricValue::Float(f)) = result {
            assert!((f - 23.75).abs() < 0.01);
        } else {
            panic!("Expected Float value");
        }
    }

    #[test]
    fn test_parse_with_scale() {
        let mapping = ModbusMappingBuilder::new("test")
            .add_register_with_transform(
                "voltage",
                0x0000,
                ModbusRegisterType::InputRegister,
                ModbusDataType::Int16,
                0.1,  // scale
                0.0,  // offset
            )
            .build();

        // Value is 240 (unscaled), should become 24.0
        let data: [u8; 2] = [0x00, 0xF0]; // 240 in big-endian
        let result = mapping.parse_metric("voltage", &data);
        assert!(matches!(result, Ok(MetricValue::Float(f)) if (f - 24.0).abs() < 0.1));
    }

    #[test]
    fn test_serialize_int16() {
        let bytes = ModbusDataType::Int16.to_be_bytes(&MetricValue::Integer(1000)).unwrap();
        assert_eq!(bytes, vec![0x03, 0xE8]); // 1000 in big-endian
    }

    #[test]
    fn test_serialize_float32() {
        let bytes = ModbusDataType::Float32.to_be_bytes(&MetricValue::Float(25.5)).unwrap();
        assert_eq!(bytes.len(), 4);
    }
}
