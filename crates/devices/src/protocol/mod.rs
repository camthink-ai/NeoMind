//! Protocol Mapping Layer
//!
//! This module provides the abstraction layer for mapping device capabilities
//! to protocol-specific addresses and data formats. It decouples the device
//! type definitions (MDL) from protocol implementations (MQTT, Modbus, HASS, etc.).
//!
//! ## Architecture
//!
//! ```text
//! Device Type Definition (MDL)       Protocol Mapping
//! ├─ temperature capability  ──────→  ├─ MQTT: sensor/${id}/temperature
//! ├─ humidity capability     ──────→  ├─ Modbus: register 0x0102, holding
//! └─ relay_state capability  ──────→  └─ HASS: switch.relay123
//! ```

pub mod mapping;
pub mod mqtt_mapping;
pub mod modbus_mapping;
pub mod hass_mapping;

// Re-exports
pub use mapping::{
    ProtocolMapping, Address, ModbusRegisterType,
    MetricParser, PayloadSerializer,
    Capability, CapabilityType, MappingConfig, MappingError,
    SharedMapping,
};
pub use mqtt_mapping::{MqttMapping, MqttMappingBuilder, MqttValueParser, BinaryFormat};
pub use modbus_mapping::{ModbusMapping, ModbusMappingBuilder, ModbusDataType};
pub use hass_mapping::{HassMapping, HassMappingBuilder, HassDomain, HassEntityId};
