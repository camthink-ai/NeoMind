//! Integration Registry for managing multiple integrations.
//!
//! The registry provides:
//! - Integration registration and lifecycle management
//! - Event aggregation from all integrations
//! - Command routing to specific integrations
//! - Health monitoring and automatic reconnection

use crate::{DynIntegration, IntegrationCommand, IntegrationEvent, IntegrationResponse};
use edge_ai_core::event::{MetricValue, NeoTalkEvent};
use edge_ai_core::eventbus::EventBus;
use edge_ai_core::integration::IntegrationType;
use futures::StreamExt;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Result type for registry operations.
pub type Result<T> = std::result::Result<T, RegistryError>;

/// Registry error types.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    /// Integration already registered.
    #[error("Integration already registered: {0}")]
    AlreadyRegistered(String),

    /// Integration not found.
    #[error("Integration not found: {0}")]
    NotFound(String),

    /// Integration failed to start.
    #[error("Integration failed to start: {0}")]
    StartFailed(String),

    /// Integration failed to stop.
    #[error("Integration failed to stop: {0}")]
    StopFailed(String),

    /// Command routing failed.
    #[error("Command routing failed: {0}")]
    RouteFailed(String),

    /// Other error.
    #[error("Registry error: {0}")]
    Other(#[from] anyhow::Error),
}

/// Events from the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RegistryEvent {
    /// Integration registered.
    Registered { id: String, timestamp: i64 },

    /// Integration unregistered.
    Unregistered { id: String, timestamp: i64 },

    /// Integration started.
    Started { id: String, timestamp: i64 },

    /// Integration stopped.
    Stopped { id: String, timestamp: i64 },

    /// Integration error.
    Error {
        id: String,
        error: String,
        timestamp: i64,
    },
}

impl RegistryEvent {
    /// Get the event timestamp.
    pub fn timestamp(&self) -> i64 {
        match self {
            Self::Registered { timestamp, .. }
            | Self::Unregistered { timestamp, .. }
            | Self::Started { timestamp, .. }
            | Self::Stopped { timestamp, .. }
            | Self::Error { timestamp, .. } => *timestamp,
        }
    }
}

/// Integration state tracking.
struct RuntimeState {
    /// The integration instance.
    integration: DynIntegration,

    /// Whether the integration is running.
    running: Arc<std::sync::atomic::AtomicBool>,

    /// Event stream task handle.
    task_handle: Option<tokio::task::JoinHandle<()>>,
}

// Safety: We only access integration through Arc which is thread-safe
unsafe impl Send for RuntimeState {}
unsafe impl Sync for RuntimeState {}

/// Integration Registry for managing multiple integrations.
///
/// The registry manages the lifecycle of all integrations, aggregates
/// their events, and routes commands to the appropriate integration.
pub struct IntegrationRegistry {
    /// Registered integrations by ID.
    integrations: Arc<RwLock<HashMap<String, RuntimeState>>>,

    /// EventBus for publishing integration events.
    event_bus: EventBus,

    /// Registry event sender.
    registry_sender: tokio::sync::mpsc::UnboundedSender<RegistryEvent>,
}

impl IntegrationRegistry {
    /// Create a new integration registry.
    pub fn new(event_bus: EventBus) -> Self {
        let (registry_sender, _) = tokio::sync::mpsc::unbounded_channel();

        Self {
            integrations: Arc::new(RwLock::new(HashMap::new())),
            event_bus,
            registry_sender,
        }
    }

    /// Register an integration.
    ///
    /// # Arguments
    /// * `integration` - The integration to register
    ///
    /// # Returns
    /// The integration ID
    pub async fn register(&self, integration: DynIntegration) -> Result<String> {
        let id = integration.metadata().id.clone();

        {
            let mut integrations = self.integrations.write();
            if integrations.contains_key(&id) {
                return Err(RegistryError::AlreadyRegistered(id));
            }

            integrations.insert(
                id.clone(),
                RuntimeState {
                    integration,
                    running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                    task_handle: None,
                },
            );
        }

        // Emit registry event
        self.emit_registry_event(RegistryEvent::Registered {
            id: id.clone(),
            timestamp: chrono::Utc::now().timestamp(),
        });

        Ok(id)
    }

    /// Unregister an integration.
    ///
    /// # Arguments
    /// * `id` - The integration ID
    pub async fn unregister(&self, id: &str) -> Result<()> {
        // Stop the integration first if running
        let needs_stop = {
            let integrations = self.integrations.read();
            integrations
                .get(id)
                .is_some_and(|s| s.running.load(std::sync::atomic::Ordering::Relaxed))
        };

        if needs_stop {
            self.stop(id).await?;
        }

        let mut integrations = self.integrations.write();
        integrations
            .remove(id)
            .ok_or_else(|| RegistryError::NotFound(id.to_string()))?;

        // Emit registry event
        self.emit_registry_event(RegistryEvent::Unregistered {
            id: id.to_string(),
            timestamp: chrono::Utc::now().timestamp(),
        });

        Ok(())
    }

    /// Start an integration.
    ///
    /// # Arguments
    /// * `id` - The integration ID
    pub async fn start(&self, id: &str) -> Result<()> {
        let (integration, event_bus, running_flag, id_string) = {
            let mut integrations = self.integrations.write();
            let state = integrations
                .get_mut(id)
                .ok_or_else(|| RegistryError::NotFound(id.to_string()))?;

            if state.running.load(std::sync::atomic::Ordering::Relaxed) {
                return Ok(()); // Already running
            }

            state
                .running
                .store(true, std::sync::atomic::Ordering::Relaxed);
            (
                state.integration.clone(),
                self.event_bus.clone(),
                state.running.clone(),
                id.to_string(),
            )
        };

        // Start the integration
        integration
            .start()
            .await
            .map_err(|e| RegistryError::StartFailed(e.to_string()))?;

        // Spawn event forwarding task
        let registry_sender = self.registry_sender.clone();
        let id_for_task = id_string.clone();

        let handle = tokio::spawn(async move {
            let mut stream = integration.subscribe();

            while let Some(event) = stream.next().await {
                if let IntegrationEvent::Error { message, .. } = &event {
                    let _ = registry_sender.send(RegistryEvent::Error {
                        id: id_for_task.clone(),
                        error: message.clone(),
                        timestamp: chrono::Utc::now().timestamp(),
                    });
                }

                // Convert integration event to NeoTalk event and publish
                if let Ok(neotalk_event) = Self::integration_to_neotalk_event(event, &id_for_task) {
                    let _ = event_bus.publish(neotalk_event);
                }
            }

            // Mark as not running when stream ends
            running_flag.store(false, std::sync::atomic::Ordering::Relaxed);
        });

        {
            let mut integrations = self.integrations.write();
            if let Some(state) = integrations.get_mut(&id_string) {
                state.task_handle = Some(handle);
            }
        }

        // Emit registry event
        self.emit_registry_event(RegistryEvent::Started {
            id: id_string,
            timestamp: chrono::Utc::now().timestamp(),
        });

        Ok(())
    }

    /// Stop an integration.
    ///
    /// # Arguments
    /// * `id` - The integration ID
    pub async fn stop(&self, id: &str) -> Result<()> {
        let integration = {
            let mut integrations = self.integrations.write();
            let state = integrations
                .get_mut(id)
                .ok_or_else(|| RegistryError::NotFound(id.to_string()))?;

            if !state.running.load(std::sync::atomic::Ordering::Relaxed) {
                return Ok(()); // Already stopped
            }

            state
                .running
                .store(false, std::sync::atomic::Ordering::Relaxed);

            // Abort the event stream task
            if let Some(handle) = state.task_handle.take() {
                handle.abort();
            }

            state.integration.clone()
        };

        integration
            .stop()
            .await
            .map_err(|e| RegistryError::StopFailed(e.to_string()))?;

        // Emit registry event
        self.emit_registry_event(RegistryEvent::Stopped {
            id: id.to_string(),
            timestamp: chrono::Utc::now().timestamp(),
        });

        Ok(())
    }

    /// Start all registered integrations.
    pub async fn start_all(&self) -> Result<()> {
        let ids: Vec<String> = self.integrations.read().keys().cloned().collect();

        for id in ids {
            if let Err(e) = self.start(&id).await {
                tracing::warn!("Failed to start integration {}: {}", id, e);
            }
        }

        Ok(())
    }

    /// Stop all running integrations.
    pub async fn stop_all(&self) -> Result<()> {
        let ids: Vec<String> = self.integrations.read().keys().cloned().collect();

        for id in ids {
            let _ = self.stop(&id).await;
        }

        Ok(())
    }

    /// Send a command to an integration.
    ///
    /// # Arguments
    /// * `id` - The integration ID
    /// * `command` - The command to send
    pub async fn send_command(
        &self,
        id: &str,
        command: IntegrationCommand,
    ) -> Result<IntegrationResponse> {
        // Clone integration to avoid holding lock across await
        let integration = {
            let integrations = self.integrations.read();
            let state = integrations
                .get(id)
                .ok_or_else(|| RegistryError::NotFound(id.to_string()))?;
            state.integration.clone()
        };

        integration
            .send_command(command)
            .await
            .map_err(|e| RegistryError::RouteFailed(e.to_string()))
    }

    /// Get an integration by ID.
    pub fn get(&self, id: &str) -> Option<DynIntegration> {
        self.integrations
            .read()
            .get(id)
            .map(|state| state.integration.clone())
    }

    /// List all integration IDs.
    pub fn list(&self) -> Vec<String> {
        self.integrations.read().keys().cloned().collect()
    }

    /// Get integrations by type.
    pub fn get_by_type(&self, integration_type: &IntegrationType) -> Vec<DynIntegration> {
        self.integrations
            .read()
            .values()
            .filter(|state| &state.integration.metadata().integration_type == integration_type)
            .map(|state| state.integration.clone())
            .collect()
    }

    /// Get running integrations.
    pub fn running(&self) -> Vec<String> {
        self.integrations
            .read()
            .iter()
            .filter(|(_, state)| state.running.load(std::sync::atomic::Ordering::Relaxed))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get the count of registered integrations.
    pub fn len(&self) -> usize {
        self.integrations.read().len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.integrations.read().is_empty()
    }

    /// Subscribe to registry events.
    ///
    /// Note: This replaces the internal sender and any pending events will be lost.
    /// In a production system, you'd want a more robust event broadcasting mechanism.
    pub fn subscribe_registry_events(&self) -> tokio::sync::mpsc::UnboundedReceiver<RegistryEvent> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        // We need to get a mutable reference to replace the sender
        // Since we can't do that through &self, we use Arc::into_inner trick
        // This is a limitation of the current design
        let _ = tx; // TODO: Implement proper event broadcasting
        rx
    }

    /// Emit a registry event.
    fn emit_registry_event(&self, event: RegistryEvent) {
        let _ = self.registry_sender.send(event);
    }

    /// Convert an integration event to a NeoTalk event.
    fn integration_to_neotalk_event(
        event: IntegrationEvent,
        source_id: &str,
    ) -> Result<NeoTalkEvent> {
        // Get timestamp first to avoid borrow issues
        let timestamp = event.timestamp();

        Ok(match event {
            IntegrationEvent::Data {
                source,
                data_type,
                payload,
                ..
            } => {
                // Try to parse as metric and create DeviceMetric event
                if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&payload) {
                    NeoTalkEvent::DeviceMetric {
                        device_id: source,
                        metric: data_type,
                        value: Self::json_to_metric_value(json),
                        timestamp,
                        quality: None,
                    }
                } else {
                    // Fallback: create a DeviceOnline event for unknown data
                    NeoTalkEvent::DeviceOnline {
                        device_id: source,
                        device_type: data_type,
                        timestamp,
                    }
                }
            }
            IntegrationEvent::Discovery {
                discovered_id,
                discovery_type,
                ..
            } => NeoTalkEvent::DeviceOnline {
                device_id: discovered_id,
                device_type: discovery_type,
                timestamp,
            },
            IntegrationEvent::StateChanged { .. } => NeoTalkEvent::LlmResponse {
                session_id: source_id.to_string(),
                content: "Integration state changed".to_string(),
                tools_used: vec![],
                processing_time_ms: 0,
                timestamp,
            },
            IntegrationEvent::Error { message, .. } => NeoTalkEvent::ToolExecutionFailure {
                tool_name: source_id.to_string(),
                arguments: serde_json::json!({}),
                error: message,
                error_type: "IntegrationError".to_string(),
                duration_ms: 0,
                session_id: None,
                timestamp,
            },
        })
    }

    /// Convert JSON to MetricValue.
    fn json_to_metric_value(json: serde_json::Value) -> MetricValue {
        match json {
            serde_json::Value::Number(n) => {
                if let Some(f) = n.as_f64() {
                    MetricValue::float(f)
                } else if let Some(i) = n.as_i64() {
                    MetricValue::integer(i)
                } else {
                    MetricValue::json(serde_json::Value::Number(n))
                }
            }
            serde_json::Value::Bool(b) => MetricValue::boolean(b),
            serde_json::Value::String(s) => MetricValue::string(s),
            serde_json::Value::Null => MetricValue::json(serde_json::Value::Null),
            _ => MetricValue::json(json),
        }
    }
}

impl Clone for IntegrationRegistry {
    fn clone(&self) -> Self {
        Self {
            integrations: self.integrations.clone(),
            event_bus: self.event_bus.clone(),
            registry_sender: self.registry_sender.clone(),
        }
    }
}

/// Builder for creating an IntegrationRegistry.
pub struct IntegrationRegistryBuilder {
    event_bus: Option<EventBus>,
}

impl Default for IntegrationRegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl IntegrationRegistryBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self { event_bus: None }
    }

    /// Set the EventBus.
    pub fn with_event_bus(mut self, event_bus: EventBus) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    /// Build the registry.
    pub fn build(self) -> IntegrationRegistry {
        let event_bus = self.event_bus.unwrap_or_default();
        IntegrationRegistry::new(event_bus)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IntegrationMetadata;
    use edge_ai_core::integration::IntegrationState;
    use futures::Stream;
    use std::pin::Pin;

    // Mock integration for testing
    struct MockIntegration {
        metadata: IntegrationMetadata,
    }

    #[async_trait::async_trait]
    impl crate::Integration for MockIntegration {
        fn metadata(&self) -> &IntegrationMetadata {
            &self.metadata
        }

        fn state(&self) -> IntegrationState {
            IntegrationState::Disconnected
        }

        async fn start(&self) -> crate::IntegrationResult<()> {
            Ok(())
        }

        async fn stop(&self) -> crate::IntegrationResult<()> {
            Ok(())
        }

        fn subscribe(&self) -> Pin<Box<dyn Stream<Item = IntegrationEvent> + Send + '_>> {
            Box::pin(futures::stream::empty())
        }

        async fn send_command(
            &self,
            _command: IntegrationCommand,
        ) -> crate::IntegrationResult<IntegrationResponse> {
            Ok(IntegrationResponse::success(serde_json::json!({})))
        }
    }

    #[tokio::test]
    async fn test_registry_register() {
        let event_bus = EventBus::new();
        let registry = IntegrationRegistry::new(event_bus);

        let metadata = IntegrationMetadata::new("test", "Test Integration", IntegrationType::Mqtt);
        let integration: DynIntegration = std::sync::Arc::new(MockIntegration { metadata });

        let id = registry.register(integration).await.unwrap();
        assert_eq!(id, "test");
        assert_eq!(registry.len(), 1);
    }

    #[tokio::test]
    async fn test_registry_list() {
        let event_bus = EventBus::new();
        let registry = IntegrationRegistry::new(event_bus);

        let metadata = IntegrationMetadata::new("test1", "Test 1", IntegrationType::Mqtt);
        let _ = registry
            .register(std::sync::Arc::new(MockIntegration { metadata }))
            .await;

        let metadata = IntegrationMetadata::new("test2", "Test 2", IntegrationType::Http);
        let _ = registry
            .register(std::sync::Arc::new(MockIntegration { metadata }))
            .await;

        let ids = registry.list();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"test1".to_string()));
        assert!(ids.contains(&"test2".to_string()));
    }

    #[test]
    fn test_registry_builder() {
        let event_bus = EventBus::new();
        let registry = IntegrationRegistryBuilder::new()
            .with_event_bus(event_bus)
            .build();

        assert_eq!(registry.len(), 0);
    }
}
