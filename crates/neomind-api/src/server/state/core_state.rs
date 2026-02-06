//! Core system services state.
//!
//! Contains fundamental services used across the application:
//! - EventBus for event-driven communication
//! - CommandManager for command history and retry
//! - MessageManager for unified messaging
//! - ExtensionRegistry for dynamically loaded extensions

use std::sync::Arc;
use tokio::sync::RwLock;

use neomind_commands::CommandManager;
use neomind_core::{EventBus, extension::ExtensionRegistry};
use neomind_messages::MessageManager;

/// Core system services state.
///
/// Provides access to fundamental cross-cutting services.
#[derive(Clone)]
pub struct CoreState {
    /// Event bus for system-wide event distribution.
    pub event_bus: Option<Arc<EventBus>>,

    /// Command manager for command history and retry.
    pub command_manager: Option<Arc<CommandManager>>,

    /// Message manager for unified messages/notifications system.
    pub message_manager: Arc<MessageManager>,

    /// Extension registry for managing dynamically loaded extensions (.so/.wasm).
    pub extension_registry: Arc<RwLock<ExtensionRegistry>>,
}

impl CoreState {
    /// Create a new core state.
    pub fn new(
        event_bus: Option<Arc<EventBus>>,
        command_manager: Option<Arc<CommandManager>>,
        message_manager: Arc<MessageManager>,
        extension_registry: Arc<RwLock<ExtensionRegistry>>,
    ) -> Self {
        Self {
            event_bus,
            command_manager,
            message_manager,
            extension_registry,
        }
    }

    /// Create a minimal core state (for testing).
    #[cfg(test)]
    pub fn minimal() -> Self {
        use neomind_core::EventBus;
        Self {
            event_bus: Some(Arc::new(EventBus::new())),
            command_manager: None,
            message_manager: Arc::new(MessageManager::new()),
            extension_registry: Arc::new(RwLock::new(ExtensionRegistry::new())),
        }
    }
}
