//! Edge AI Integrations Crate
//!
//! This crate provides a unified framework for integrating NeoMind with external systems.
//!
//! ## Architecture
//!
//! ```text
//! External System          Integration Framework          NeoMind
//! ┌─────────────┐          ┌─────────────────────┐          ┌──────────┐
//! │             │  Ingest  │                     │  Event   │          │
//! │    MQTT     │──────────▶│  Integration        │──────────▶│ EventBus │
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
//! | `all` | ❌ | All integrations |
//!
//! ## Example
//!
//! ```rust,no_run
//! use neomind_integrations::IntegrationRegistry;
//! use neomind_core::EventBus;
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

pub mod connectors;
pub mod protocols;
pub mod registry;

// Re-exports from core
pub use neomind_core::integration::{
    DiscoveredInfo,
    DynIntegration,
    Integration,
    IntegrationCommand,
    IntegrationConfig,
    IntegrationError,
    IntegrationEvent,
    IntegrationMetadata,
    IntegrationResponse,
    IntegrationState,
    IntegrationType,
    Result as IntegrationResult,
    // Connector exports
    connector::{
        BaseConnector, ConnectionMetrics, Connector, ConnectorConfig, ConnectorError, DynConnector,
        Result as ConnectorResult,
    },
    // Transformer exports
    transformer::{
        BaseTransformer, ConversionFunction, DynTransformer, EntityMapping, MappingConfig,
        Result as TransformerResult, TransformType, TransformationContext, TransformationError,
        Transformer, UnitConversion, ValueTransform,
    },
};

// Re-exports from registry
pub use registry::{IntegrationRegistry, RegistryError, RegistryEvent, Result as RegistryResult};

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
