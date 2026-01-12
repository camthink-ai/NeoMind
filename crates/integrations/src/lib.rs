//! Edge AI Integrations Crate
//!
//! This crate provides a unified framework for integrating NeoTalk with external systems.
//!
//! ## Architecture
//!
//! ```text
//! External System          Integration Framework          NeoTalk
//! ┌─────────────┐          ┌─────────────────────┐          ┌──────────┐
//! │             │  Ingest  │                     │  Event   │          │
//! │   HASS/MQTT │──────────▶│  Integration        │──────────▶│ EventBus │
//! │             │          │  - Connector         │          │          │
//! │             │  Egress  │  - Transformer      │  Command │          │
//! │             │◀─────────│  - Protocol Adapter  │◀─────────│  Agent   │
//! └─────────────┘          └─────────────────────┘          └──────────┘
//! ```
//!
//! ## Features
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `mqtt` | ❌ | MQTT broker integration |
//! | `websocket` | ❌ | WebSocket client integration |
//! | `http` | ❌ | HTTP/REST API integration |
//! | `modbus` | ❌ | Modbus TCP integration |
//! | `hass` | ❌ | Home Assistant integration (HTTP+WebSocket) |
//! | `hass-mqtt` | ❌ | Home Assistant via MQTT Discovery |
//! | `tasmota` | ❌ | Tasmota device integration |
//! | `zigbee` | ❌ | Zigbee2MQTT integration |
//! | `all` | ❌ | All integrations |
//!
//! ## Example
//!
//! ```rust,no_run
//! use edge_ai_integrations::IntegrationRegistry;
//! use edge_ai_core::EventBus;
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let event_bus = EventBus::new();
//! // Create a registry
//! let registry = IntegrationRegistry::new(event_bus);
//!
//! // Start all integrations
//! registry.start_all().await?;
//! # Ok(())
//! # }
//! ```

pub mod registry;
pub mod connectors;
pub mod protocols;

// Re-exports from core
pub use edge_ai_core::integration::{
    Integration, DynIntegration, IntegrationType, IntegrationState,
    IntegrationMetadata, IntegrationEvent, IntegrationCommand,
    IntegrationResponse, IntegrationConfig, IntegrationError,
    DiscoveredInfo, Result as IntegrationResult,
    // Connector exports
    connector::{
        Connector, DynConnector, ConnectorError, ConnectorConfig,
        ConnectionMetrics, Result as ConnectorResult, BaseConnector,
    },
    // Transformer exports
    transformer::{
        Transformer, DynTransformer, TransformationError,
        TransformationContext, EntityMapping, MappingConfig,
        ValueTransform, TransformType, UnitConversion,
        ConversionFunction, Result as TransformerResult, BaseTransformer,
    },
};

// Re-exports from registry
pub use registry::{IntegrationRegistry, RegistryEvent, RegistryError, Result as RegistryResult};

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
