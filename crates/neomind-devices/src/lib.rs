//! Edge AI Device Management Crate
//!
//! This crate provides device abstraction and management capabilities for the NeoMind platform.
//!
//! ## Features
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `mqtt` | ✅ | MQTT protocol support |
//! | `discovery` | ❌ | mDNS device discovery |
//! | `embedded-broker` | ❌ | Embedded MQTT broker |
//! | `all` | ❌ | All features |
//!
//! ## Architecture
//!
//! The device management system uses a simplified architecture:
//! - **DeviceRegistry**: Storage for device configurations and type templates
//! - **DeviceService**: Unified interface for all device operations
//! - **DeviceAdapter**: Protocol-specific adapter interface (MQTT)
//! - **DeviceAdapterPluginRegistry**: Plugin system for managing adapters
//!
//! Devices are configured using `DeviceConfig` and accessed through `DeviceService`.
//! Protocol adapters are registered as plugins for unified management.

pub mod builtin_types;
pub mod discovery;
pub mod mdl;
pub mod mdl_format;
pub mod mqtt;
pub mod mqtt_v2;
pub mod telemetry;

// Simplified device management
pub mod registry;
pub mod service;
pub mod service_types;  // Shared types for device operations

// Protocol mapping layer - decouples MDL from protocol implementations
pub mod protocol;

// Adapter interface for event-driven architecture
pub mod adapter;

// Protocol mapping re-exports
pub use protocol::{
    Address, BinaryFormat, Capability, CapabilityType,
    MappingConfig, MqttMapping, MqttMappingBuilder, ProtocolMapping, SharedMapping,
};

// Re-export protocol mapping functions
pub use builtin_types::{
    builtin_device_types, builtin_mqtt_mappings,
};

// Device adapters implementing the adapter interface
pub mod adapters;

// Unified data extraction for all adapters
pub mod unified_extractor;

#[cfg(feature = "embedded-broker")]
pub mod embedded_broker;

// Re-exports for convenience
pub use mdl::{
    Command, DeviceCapability, DeviceError, DeviceId, DeviceInfo, DeviceState, DeviceType,
    MetricDataType, MetricDefinition, MetricValue,
};
// ConnectionStatus is now defined in the adapter module
pub use adapter::ConnectionStatus;
pub use mdl_format::{
    CommandDefinition, DeviceInstance, DeviceTypeDefinition, DownlinkConfig, MdlRegistry,
    MdlStorage, MetricDefinition as MdlMetricDefinition, ParameterDefinition, UplinkConfig,
};

// New architecture exports
pub use discovery::{DeviceDiscovery, DiscoveredDevice, DiscoveryResult};
pub use registry::{ConnectionConfig, DeviceConfig, DeviceRegistry, DeviceTypeTemplate, DeviceTypeMode};
pub use service::{AdapterInfo, AdapterStats, CommandHistoryRecord, CommandStatus, DeviceHealth, DeviceService, DeviceStatus, HeartbeatConfig};
pub use telemetry::{AggregatedData, DataPoint, MetricCache, TimeSeriesStorage};

// Unified data extraction re-exports
pub use unified_extractor::{
    ExtractionConfig, ExtractionMode, ExtractionResult, ExtractedMetric, UnifiedExtractor,
};

#[cfg(feature = "embedded-broker")]
pub use embedded_broker::{BrokerMode, EmbeddedBroker, EmbeddedBrokerConfig};

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

// Re-export core adapter types from local adapter module
pub use adapter::{
    AdapterConfig, AdapterError, AdapterResult, DeviceAdapter, DeviceEvent, DiscoveredDeviceInfo,
    EventPublishingAdapter,
};

// Adapter creation utilities
pub use adapters::{available_adapters, create_adapter};
