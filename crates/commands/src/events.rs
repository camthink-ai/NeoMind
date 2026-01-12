//! Event integration for command system.
//!
//! Provides event publishing and subscription for command lifecycle events.

use std::sync::Arc;
use tokio::sync::{RwLock, mpsc, broadcast};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

use crate::command::{CommandId, CommandStatus, CommandResult, CommandPriority};
use crate::ack::AckStatus;

/// Command lifecycle event types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CommandEventType {
    /// Command created
    Created,
    /// Command queued
    Queued,
    /// Command sending
    Sending,
    /// Command sent, waiting for acknowledgment
    Sent,
    /// Acknowledgment received
    Acknowledged,
    /// Command completed successfully
    Completed,
    /// Command failed
    Failed,
    /// Command cancelled
    Cancelled,
    /// Command timed out
    Timeout,
    /// Command retry initiated
    Retry,
}

/// Command event with context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandEvent {
    /// Event ID
    pub id: String,
    /// Event type
    pub event_type: CommandEventType,
    /// Command ID
    pub command_id: CommandId,
    /// Device ID
    pub device_id: String,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Previous status (if applicable)
    pub previous_status: Option<CommandStatus>,
    /// Current status
    pub current_status: CommandStatus,
    /// Result (if completed/failed)
    pub result: Option<CommandResult>,
    /// Acknowledgment status
    pub ack_status: Option<AckStatus>,
    /// Additional event data
    pub data: Option<serde_json::Value>,
}

impl CommandEvent {
    /// Create a new command event.
    pub fn new(
        event_type: CommandEventType,
        command_id: CommandId,
        device_id: String,
        current_status: CommandStatus,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            event_type,
            command_id,
            device_id,
            timestamp: Utc::now(),
            previous_status: None,
            current_status,
            result: None,
            ack_status: None,
            data: None,
        }
    }

    /// Set previous status.
    pub fn with_previous_status(mut self, status: CommandStatus) -> Self {
        self.previous_status = Some(status);
        self
    }

    /// Set result.
    pub fn with_result(mut self, result: CommandResult) -> Self {
        self.result = Some(result);
        self
    }

    /// Set acknowledgment status.
    pub fn with_ack_status(mut self, status: AckStatus) -> Self {
        self.ack_status = Some(status);
        self
    }

    /// Set additional data.
    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = Some(data);
        self
    }
}

/// Event filter for subscriptions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventFilter {
    /// Filter by device ID
    pub device_id: Option<String>,
    /// Filter by command status
    pub status: Option<CommandStatus>,
    /// Filter by event types
    pub event_types: Option<Vec<CommandEventType>>,
    /// Filter by priority
    pub priority: Option<CommandPriority>,
}

impl EventFilter {
    /// Create a new empty filter.
    pub fn new() -> Self {
        Self {
            device_id: None,
            status: None,
            event_types: None,
            priority: None,
        }
    }

    /// Check if an event matches this filter.
    pub fn matches(&self, event: &CommandEvent) -> bool {
        if let Some(ref device_id) = self.device_id {
            if &event.device_id != device_id {
                return false;
            }
        }

        if let Some(ref status) = self.status {
            if event.current_status != *status {
                return false;
            }
        }

        if let Some(ref event_types) = self.event_types {
            if !event_types.contains(&event.event_type) {
                return false;
            }
        }

        true
    }
}

impl Default for EventFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// Event subscriber configuration.
#[derive(Debug, Clone)]
pub struct SubscriberConfig {
    /// Event filter
    pub filter: EventFilter,
    /// Channel buffer size
    pub buffer_size: usize,
}

impl Default for SubscriberConfig {
    fn default() -> Self {
        Self {
            filter: EventFilter::new(),
            buffer_size: 100,
        }
    }
}

/// Event bus for command events.
pub struct CommandEventBus {
    /// Broadcast sender for all events
    broadcast_tx: broadcast::Sender<CommandEvent>,
    /// Event storage (recent events)
    recent_events: Arc<RwLock<Vec<CommandEvent>>>,
    /// Maximum events to store
    max_stored: usize,
}

impl CommandEventBus {
    /// Create a new event bus.
    pub fn new(max_stored: usize) -> Self {
        let (broadcast_tx, _) = broadcast::channel(1000);

        Self {
            broadcast_tx,
            recent_events: Arc::new(RwLock::new(Vec::new())),
            max_stored,
        }
    }

    /// Publish an event.
    pub async fn publish(&self, event: CommandEvent) {
        // Store recent events
        let mut events = self.recent_events.write().await;
        events.push(event.clone());
        if events.len() > self.max_stored {
            events.remove(0);
        }
        drop(events);

        // Broadcast to all subscribers
        let _ = self.broadcast_tx.send(event);
    }

    /// Subscribe to all events.
    pub fn subscribe(&self) -> broadcast::Receiver<CommandEvent> {
        self.broadcast_tx.subscribe()
    }

    /// Subscribe with a filter.
    pub fn subscribe_filtered(&self, config: SubscriberConfig) -> FilteredSubscriber {
        let rx = self.broadcast_tx.subscribe();
        FilteredSubscriber {
            rx,
            filter: config.filter,
        }
    }

    /// Get recent events.
    pub async fn get_recent(&self, limit: usize) -> Vec<CommandEvent> {
        let events = self.recent_events.read().await;
        let len = events.len();
        let start = if len > limit { len - limit } else { 0 };
        events[start..].to_vec()
    }

    /// Get events by device.
    pub async fn get_by_device(&self, device_id: &str, limit: usize) -> Vec<CommandEvent> {
        let events = self.recent_events.read().await;
        events.iter()
            .filter(|e| e.device_id == device_id)
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }

    /// Get events for a command.
    pub async fn get_for_command(&self, command_id: &CommandId) -> Vec<CommandEvent> {
        let events = self.recent_events.read().await;
        events.iter()
            .filter(|e| &e.command_id == command_id)
            .cloned()
            .collect()
    }

    /// Clear all stored events.
    pub async fn clear(&self) {
        self.recent_events.write().await.clear();
    }
}

/// Filtered event subscriber.
pub struct FilteredSubscriber {
    rx: broadcast::Receiver<CommandEvent>,
    filter: EventFilter,
}

impl FilteredSubscriber {
    /// Receive next filtered event.
    pub async fn recv(&mut self) -> Result<CommandEvent, broadcast::error::RecvError> {
        loop {
            match self.rx.recv().await {
                Ok(event) => {
                    if self.filter.matches(&event) {
                        return Ok(event);
                    }
                    // Otherwise, continue waiting for next event
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Try to receive next filtered event without blocking.
    pub fn try_recv(&mut self) -> Result<CommandEvent, broadcast::error::TryRecvError> {
        loop {
            match self.rx.try_recv() {
                Ok(event) => {
                    if self.filter.matches(&event) {
                        return Ok(event);
                    }
                    // Otherwise, continue trying
                }
                Err(broadcast::error::TryRecvError::Empty) => {
                    return Err(broadcast::error::TryRecvError::Empty);
                }
                Err(broadcast::error::TryRecvError::Lagged(_n)) => {
                    return Err(broadcast::error::TryRecvError::Lagged(0));
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// Event integration that bridges command state changes to events.
pub struct EventIntegration {
    /// Event bus
    event_bus: Arc<CommandEventBus>,
    /// Running flag
    running: Arc<RwLock<bool>>,
}

impl EventIntegration {
    /// Create a new event integration.
    pub fn new(event_bus: Arc<CommandEventBus>) -> Self {
        Self {
            event_bus,
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Get the event bus.
    pub fn event_bus(&self) -> Arc<CommandEventBus> {
        self.event_bus.clone()
    }

    /// Publish command created event.
    pub async fn publish_created(
        &self,
        command_id: CommandId,
        device_id: String,
    ) {
        let event = CommandEvent::new(
            CommandEventType::Created,
            command_id,
            device_id,
            CommandStatus::Pending,
        );
        self.event_bus.publish(event).await;
    }

    /// Publish command status change event.
    pub async fn publish_status_change(
        &self,
        command_id: CommandId,
        device_id: String,
        previous: CommandStatus,
        current: CommandStatus,
    ) {
        let event_type = match current {
            CommandStatus::Queued => CommandEventType::Queued,
            CommandStatus::Sending => CommandEventType::Sending,
            CommandStatus::Completed => CommandEventType::Completed,
            CommandStatus::Failed => CommandEventType::Failed,
            CommandStatus::Cancelled => CommandEventType::Cancelled,
            CommandStatus::Timeout => CommandEventType::Timeout,
            _ => return, // Skip other status changes
        };

        let event = CommandEvent::new(
            event_type,
            command_id,
            device_id,
            current,
        ).with_previous_status(previous);

        self.event_bus.publish(event).await;
    }

    /// Publish command sent event.
    pub async fn publish_sent(
        &self,
        command_id: CommandId,
        device_id: String,
    ) {
        let event = CommandEvent::new(
            CommandEventType::Sent,
            command_id,
            device_id,
            CommandStatus::Sending,
        );
        self.event_bus.publish(event).await;
    }

    /// Publish acknowledgment event.
    pub async fn publish_acknowledged(
        &self,
        command_id: CommandId,
        device_id: String,
        ack_status: AckStatus,
    ) {
        let event = CommandEvent::new(
            CommandEventType::Acknowledged,
            command_id,
            device_id,
            CommandStatus::WaitingAck,
        ).with_ack_status(ack_status);

        self.event_bus.publish(event).await;
    }

    /// Publish command retry event.
    pub async fn publish_retry(
        &self,
        command_id: CommandId,
        device_id: String,
        attempt: u32,
    ) {
        let event = CommandEvent::new(
            CommandEventType::Retry,
            command_id,
            device_id,
            CommandStatus::Queued,
        ).with_data(serde_json::json!({"attempt": attempt}));

        self.event_bus.publish(event).await;
    }
}

impl Default for EventIntegration {
    fn default() -> Self {
        Self::new(Arc::new(CommandEventBus::new(1000)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_event_new() {
        let event = CommandEvent::new(
            CommandEventType::Created,
            "cmd1".to_string(),
            "device1".to_string(),
            CommandStatus::Pending,
        );

        assert_eq!(event.event_type, CommandEventType::Created);
        assert_eq!(event.command_id, "cmd1");
        assert_eq!(event.device_id, "device1");
        assert_eq!(event.current_status, CommandStatus::Pending);
    }

    #[test]
    fn test_event_filter_matches() {
        let filter = EventFilter {
            device_id: Some("device1".to_string()),
            status: Some(CommandStatus::Completed),
            event_types: Some(vec![CommandEventType::Completed]),
            priority: None,
        };

        let matching_event = CommandEvent::new(
            CommandEventType::Completed,
            "cmd1".to_string(),
            "device1".to_string(),
            CommandStatus::Completed,
        );
        assert!(filter.matches(&matching_event));

        let wrong_device = CommandEvent::new(
            CommandEventType::Completed,
            "cmd1".to_string(),
            "device2".to_string(),
            CommandStatus::Completed,
        );
        assert!(!filter.matches(&wrong_device));

        let wrong_status = CommandEvent::new(
            CommandEventType::Completed,
            "cmd1".to_string(),
            "device1".to_string(),
            CommandStatus::Pending,
        );
        assert!(!filter.matches(&wrong_status));
    }

    #[tokio::test]
    async fn test_event_bus_publish_subscribe() {
        let bus = CommandEventBus::new(100);

        // Subscribe before publishing
        let mut rx = bus.subscribe();

        let event = CommandEvent::new(
            CommandEventType::Created,
            "cmd1".to_string(),
            "device1".to_string(),
            CommandStatus::Pending,
        );

        bus.publish(event.clone()).await;

        let received = rx.recv().await.unwrap();
        assert_eq!(received.command_id, "cmd1");
    }

    #[tokio::test]
    async fn test_event_bus_recent() {
        let bus = CommandEventBus::new(10);

        for i in 0..5 {
            let event = CommandEvent::new(
                CommandEventType::Created,
                format!("cmd{}", i),
                "device1".to_string(),
                CommandStatus::Pending,
            );
            bus.publish(event).await;
        }

        let recent = bus.get_recent(3).await;
        assert_eq!(recent.len(), 3);
        // Should get the last 3 events
        assert_eq!(recent[0].command_id, "cmd2");
        assert_eq!(recent[1].command_id, "cmd3");
        assert_eq!(recent[2].command_id, "cmd4");
    }

    #[tokio::test]
    async fn test_filtered_subscriber() {
        let bus = CommandEventBus::new(100);

        let filter = EventFilter {
            device_id: Some("device1".to_string()),
            status: None,
            event_types: Some(vec![CommandEventType::Created]),
            priority: None,
        };

        let mut subscriber = bus.subscribe_filtered(SubscriberConfig {
            filter,
            buffer_size: 10,
        });

        // Publish matching event
        let event1 = CommandEvent::new(
            CommandEventType::Created,
            "cmd1".to_string(),
            "device1".to_string(),
            CommandStatus::Pending,
        );
        bus.publish(event1).await;

        // Publish non-matching event
        let event2 = CommandEvent::new(
            CommandEventType::Completed,
            "cmd2".to_string(),
            "device1".to_string(),
            CommandStatus::Completed,
        );
        bus.publish(event2).await;

        // Should only receive the matching event
        let received = subscriber.recv().await.unwrap();
        assert_eq!(received.command_id, "cmd1");
        assert_eq!(received.event_type, CommandEventType::Created);
    }

    #[tokio::test]
    async fn test_event_integration() {
        let integration = EventIntegration::default();

        let mut rx = integration.event_bus().subscribe();

        integration.publish_created(
            "cmd1".to_string(),
            "device1".to_string(),
        ).await;

        let received = rx.recv().await.unwrap();
        assert_eq!(received.event_type, CommandEventType::Created);
        assert_eq!(received.device_id, "device1");
    }
}
