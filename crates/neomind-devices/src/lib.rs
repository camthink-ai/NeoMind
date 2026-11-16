//! Edge AI Device Management Crate
//!
//! This crate provides device abstraction and management capabilities for the NeoMind platform.
//!
//! ## Features
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `mqtt` | ✅ | MQTT protocol support |
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

pub mod image_storage;
pub mod mdl;
pub mod mdl_format;
pub mod mqtt;
pub mod payload_template;
pub mod telemetry;

// Simplified device management
pub mod association_bind;
pub mod registry;
pub mod service;

// Protocol mapping layer - decouples MDL from protocol implementations
pub mod protocol;

// Adapter interface for event-driven architecture
pub mod adapter;

// Device adapters implementing the adapter interface
pub mod adapters;

// Unified data extraction for all adapters
pub mod unified_extractor;

#[cfg(feature = "embedded-broker")]
pub mod embedded_broker;

// Re-exports (only types used externally via crate-root shortcut path)
pub use adapter::{AdapterResult, ConnectionStatus, DeviceAdapter, DeviceEvent};
pub use association_bind::{AssociationBindInput, AssociationBindOutcome};
pub use mdl::{DeviceError, MetricDataType, MetricValue};
pub use mdl_format::{CommandDefinition, MetricDefinition as MdlMetricDefinition};
pub use registry::{
    ConnectionConfig, DeviceConfig, DeviceRegistry, DeviceTypeMode, DeviceTypeTemplate,
};
pub use service::{CommandStatus, DeviceService, ExtensionCommandRouterFn};
pub use telemetry::{DataPoint, TimeSeriesStorage};

#[cfg(feature = "embedded-broker")]
pub use embedded_broker::{EmbeddedBroker, EmbeddedBrokerConfig, TopicResolverFn};

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }
}
