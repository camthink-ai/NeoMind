//! Edge AI Device Management Crate
//!
//! This crate provides device abstraction and management capabilities for the NeoTalk platform.
//!
//! ## Features
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `mqtt` | ✅ | MQTT protocol support |
//! | `modbus` | ✅ | Modbus TCP protocol support |
//! | `hass` | ❌ | Home Assistant integration |
//! | `discovery` | ❌ | mDNS device discovery |
//! | `embedded-broker` | ❌ | Embedded MQTT broker |
//! | `all` | ❌ | All features |

pub mod mdl;
pub mod mdl_format;
pub mod mqtt;
pub mod mqtt_v2;
pub mod modbus;
pub mod manager;
pub mod discovery;
pub mod telemetry;
pub mod builtin_types;

// Multi-broker management
pub mod multi_broker;

// Protocol mapping layer - decouples MDL from protocol implementations
pub mod protocol;

// Adapter interface for event-driven architecture
pub mod adapter;
pub mod adapter_manager;

// Device adapter plugin integration
pub mod plugin_adapter;
pub mod plugin_registry;

// Protocol mapping re-exports
pub use protocol::{
    ProtocolMapping, Address, ModbusRegisterType,
    MqttMapping, ModbusMapping, HassMapping,
    Capability, CapabilityType, MappingConfig,
    SharedMapping,
    MqttMappingBuilder, ModbusMappingBuilder, HassMappingBuilder,
    ModbusDataType, BinaryFormat,
};

// Re-export protocol mapping functions
pub use builtin_types::{
    builtin_device_types, builtin_mqtt_mappings,
    builtin_modbus_mappings, builtin_hass_mappings,
};

// Device adapters implementing the adapter interface
pub mod adapters;

#[cfg(feature = "hass")]
pub mod hass;

// HASS MQTT Discovery protocol support (always available)
pub mod hass_discovery;
pub mod hass_discovery_mapper;
pub mod hass_discovery_listener;

#[cfg(feature = "embedded-broker")]
pub mod embedded_broker;

// Re-exports for convenience
pub use mdl::{
    Device, DeviceId, DeviceType, MetricValue, MetricDefinition,
    DeviceCapability, Command, DeviceError, ConnectionStatus, DeviceState,
    DeviceInfo,
};
pub use mdl_format::{
    DeviceTypeDefinition, MdlRegistry, MdlStorage, DeviceInstance,
    MetricDefinition as MdlMetricDefinition,
    CommandDefinition, ParameterDefinition,
    UplinkConfig, DownlinkConfig,
};
pub use mqtt_v2::{MqttDeviceManager, MqttManagerConfig, MqttDevice, DiscoveredHassDevice, AggregatedHassDevice, aggregate_hass_devices};
pub use multi_broker::MultiBrokerManager;
pub use manager::{DeviceManager, DeviceGroup, GroupManager};
pub use discovery::{DeviceDiscovery, DiscoveredDevice, DiscoveryResult};
pub use telemetry::{TimeSeriesStorage, MetricCache, DataPoint, AggregatedData};

#[cfg(feature = "embedded-broker")]
pub use embedded_broker::{EmbeddedBroker, EmbeddedBrokerConfig, BrokerMode};

#[cfg(feature = "hass")]
pub use hass::{
    HassClient, HassConnectionConfig, HassEntityMapper, HassSettings,
    HassWebSocketClient, MappedDevice, EntityMapping,
};

// HASS Discovery re-exports
pub use hass_discovery::{
    HassDiscoveryError, HassDiscoveryResult, HassDiscoveryConfig,
    HassDeviceInfo, HassDiscoveryMessage, HassTopicParts,
    parse_discovery_message, is_discovery_topic, discovery_subscription_pattern,
    is_supported_component, component_to_device_type,
};
pub use hass_discovery_mapper::{
    map_hass_to_mdl, register_hass_device_type, generate_uplink_config,
};
pub use hass_discovery_listener::{
    HassDiscoveryListener, HassDiscoveryConfig as HassDiscoveryListenerConfig,
};

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Build information
pub const BUILD_PROFILE: &str = if cfg!(debug_assertions) {
    "debug"
} else {
    "release"
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }
}

// Re-exports for adapter manager
pub use adapter_manager::{
    AdapterManager, AdapterManagerConfig, AdapterStatus, AdapterInfo,
    AdapterStats, ManagerEvent, ModifiedDeviceEvent,
};

// Re-export core adapter types from local adapter module
pub use adapter::{
    DeviceAdapter, DeviceEvent, DiscoveredDeviceInfo,
    AdapterError, AdapterResult, EventPublishingAdapter, AdapterConfig,
};

// Adapter creation utilities
pub use adapters::{create_adapter, available_adapters};

// Specialized adapter plugins
pub use adapters::plugins::{
    ExternalMqttBrokerPlugin, ModbusAdapterPlugin,
    UnifiedAdapterPluginFactory,
    ExternalBrokerConfig, ModbusAdapterConfig,
};

#[cfg(feature = "embedded-broker")]
pub use adapters::plugins::InternalMqttBrokerPlugin;

// Device adapter plugin integration
pub use plugin_adapter::{
    DeviceAdapterPlugin, DeviceAdapterPluginFactory,
    AdapterDeviceInfo, DeviceAdapterStats, AdapterPluginInfo,
};
pub use plugin_registry::{
    DeviceAdapterPluginRegistry, AdapterPluginConfig,
};
