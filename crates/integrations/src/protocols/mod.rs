//! Protocol adapters for external systems.
//!
//! This module provides concrete implementations of the `Integration` trait
//! for various external systems and protocols.

#[cfg(feature = "http")]
pub mod hass;

pub mod modbus;

// Re-exports
#[cfg(feature = "hass")]
pub use hass::{HassIntegration, HassConfig, create_hass_integration};

#[cfg(feature = "modbus")]
pub use modbus::{ModbusIntegration, ModbusConfig, create_modbus_integration};

use crate::{IntegrationMetadata, IntegrationType, IntegrationState};
use edge_ai_core::integration::IntegrationEvent;
use futures::Stream;
use std::pin::Pin;

/// Base integration implementation with common functionality.
pub struct BaseIntegration {
    /// Metadata.
    pub metadata: IntegrationMetadata,

    /// State.
    state: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl BaseIntegration {
    /// Create a new base integration.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        integration_type: IntegrationType,
    ) -> Self {
        Self {
            metadata: IntegrationMetadata::new(id, name, integration_type),
            state: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Check if running.
    pub fn is_running(&self) -> bool {
        self.state.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Set running state.
    pub fn set_running(&self, running: bool) {
        self.state.store(running, std::sync::atomic::Ordering::Relaxed);
    }

    /// Get the state as IntegrationState.
    pub fn to_integration_state(&self) -> IntegrationState {
        if self.is_running() {
            IntegrationState::Connected
        } else {
            IntegrationState::Disconnected
        }
    }

    /// Create an empty stream (for integrations without events).
    pub fn empty_stream(&self) -> Pin<Box<dyn Stream<Item = IntegrationEvent> + Send + '_>> {
        Box::pin(futures::stream::empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_integration() {
        let integration = BaseIntegration::new("test", "Test Integration", IntegrationType::Mqtt);
        assert_eq!(integration.metadata.id, "test");
        assert!(!integration.is_running());
        assert_eq!(integration.to_integration_state(), IntegrationState::Disconnected);

        integration.set_running(true);
        assert!(integration.is_running());
        assert_eq!(integration.to_integration_state(), IntegrationState::Connected);
    }
}
