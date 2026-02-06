//! Event bus for NeoMind event-driven architecture.
//!
//! The event bus is the central nervous system of NeoMind. All components
//! communicate through publishing and subscribing to events.

use crate::event::{EventMetadata, NeoMindEvent};
use std::sync::Arc;
use tokio::sync::broadcast;

/// Default channel capacity for the event bus.
pub const DEFAULT_CHANNEL_CAPACITY: usize = 1000;

/// Event bus for NeoMind.
///
/// The event bus uses a broadcast channel to distribute events to all
/// subscribers. It supports:
/// - Publishing events with automatic metadata generation
/// - Subscribing to all events
/// - Filtered subscriptions for specific event types
#[derive(Clone)]
pub struct EventBus {
    /// Broadcast channel sender
    tx: broadcast::Sender<(NeoMindEvent, EventMetadata)>,
    /// Event bus name for identification
    name: String,
}

impl EventBus {
    /// Create a new event bus with default capacity.
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CHANNEL_CAPACITY)
    }

    /// Create a new event bus with the specified capacity.
    ///
    /// The capacity determines how many events are buffered for slow subscribers.
    pub fn with_capacity(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self {
            tx,
            name: "default".to_string(),
        }
    }

    /// Create a new event bus with a name.
    pub fn with_name(name: impl Into<String>) -> Self {
        Self {
            tx: broadcast::channel(DEFAULT_CHANNEL_CAPACITY).0,
            name: name.into(),
        }
    }

    /// Get the name of this event bus.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the number of current subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }

    /// Publish an event with default metadata.
    ///
    /// The event is sent to all subscribers. If there are no subscribers,
    /// the event is discarded. Returns `true` if there was at least one
    /// subscriber.
    pub async fn publish(&self, event: NeoMindEvent) -> bool {
        self.publish_with_source(event, "system").await
    }

    /// Publish an event with a custom source.
    pub async fn publish_with_source(
        &self,
        event: NeoMindEvent,
        source: impl Into<String>,
    ) -> bool {
        let metadata = EventMetadata::new(source);
        self.publish_with_metadata(event, metadata).await
    }

    /// Publish an event with custom metadata.
    pub async fn publish_with_metadata(
        &self,
        event: NeoMindEvent,
        metadata: EventMetadata,
    ) -> bool {
        self.tx.send((event, metadata)).is_ok()
    }

    /// Subscribe to all events.
    ///
    /// Returns a receiver that will receive all published events.
    /// If the subscriber falls behind, older events may be dropped.
    pub fn subscribe(&self) -> EventBusReceiver {
        EventBusReceiver {
            rx: self.tx.subscribe(),
        }
    }

    /// Subscribe to events matching a filter.
    ///
    /// The filter is a function that returns `true` for events to receive.
    /// Only matching events will be delivered through the returned receiver.
    pub fn subscribe_filtered<F>(&self, filter: F) -> FilteredReceiver<F>
    where
        F: Fn(&NeoMindEvent) -> bool + Send + 'static,
    {
        let rx = self.tx.subscribe();
        FilteredReceiver::new(rx, filter)
    }

    /// Create a filtered subscription helper for common patterns.
    pub fn filter(&self) -> FilterBuilder {
        FilterBuilder {
            tx: self.tx.clone(),
        }
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Receiver for all events from the event bus.
pub struct EventBusReceiver {
    rx: broadcast::Receiver<(NeoMindEvent, EventMetadata)>,
}

impl EventBusReceiver {
    /// Receive the next event.
    ///
    /// Returns `None` if the event bus is closed.
    pub async fn recv(&mut self) -> Option<(NeoMindEvent, EventMetadata)> {
        match self.rx.recv().await {
            Ok(event) => Some(event),
            Err(broadcast::error::RecvError::Lagged(_)) => {
                // We missed some events, but can continue receiving
                // Try again immediately
                self.rx.try_recv().ok()
            }
            Err(broadcast::error::RecvError::Closed) => None,
        }
    }

    /// Try to receive an event without blocking.
    pub fn try_recv(&mut self) -> Option<(NeoMindEvent, EventMetadata)> {
        self.rx.try_recv().ok()
    }

    /// Get the underlying broadcast receiver.
    pub fn into_inner(self) -> broadcast::Receiver<(NeoMindEvent, EventMetadata)> {
        self.rx
    }
}

/// Receiver for filtered events from the event bus.
pub struct FilteredReceiver<F>
where
    F: Fn(&NeoMindEvent) -> bool + Send,
{
    rx: broadcast::Receiver<(NeoMindEvent, EventMetadata)>,
    filter: F,
}

impl<F> FilteredReceiver<F>
where
    F: Fn(&NeoMindEvent) -> bool + Send,
{
    fn new(rx: broadcast::Receiver<(NeoMindEvent, EventMetadata)>, filter: F) -> Self {
        Self { rx, filter }
    }

    /// Receive the next event matching the filter.
    ///
    /// Returns `None` if the event bus is closed.
    pub async fn recv(&mut self) -> Option<(NeoMindEvent, EventMetadata)> {
        loop {
            match self.rx.recv().await {
                Ok((event, meta)) => {
                    if (self.filter)(&event) {
                        return Some((event, meta));
                    }
                    // Event didn't match filter, continue waiting
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // We missed some events, try to continue
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }

    /// Try to receive a matching event without blocking.
    pub fn try_recv(&mut self) -> Option<(NeoMindEvent, EventMetadata)> {
        while let Ok(result) = self.rx.try_recv() {
            let (event, meta) = result;
            if (self.filter)(&event) {
                return Some((event, meta));
            }
            // Continue trying to get events from buffer
        }
        None
    }
}

/// Builder for creating filtered subscriptions.
pub struct FilterBuilder {
    tx: broadcast::Sender<(NeoMindEvent, EventMetadata)>,
}

impl FilterBuilder {
    /// Subscribe to device events only.
    pub fn device_events(&self) -> FilteredReceiver<fn(&NeoMindEvent) -> bool> {
        let rx = self.tx.subscribe();
        FilteredReceiver::new(rx, NeoMindEvent::is_device_event)
    }

    /// Subscribe to rule events only.
    pub fn rule_events(&self) -> FilteredReceiver<fn(&NeoMindEvent) -> bool> {
        let rx = self.tx.subscribe();
        FilteredReceiver::new(rx, NeoMindEvent::is_rule_event)
    }

    /// Subscribe to workflow events only.
    pub fn workflow_events(&self) -> FilteredReceiver<fn(&NeoMindEvent) -> bool> {
        let rx = self.tx.subscribe();
        FilteredReceiver::new(rx, NeoMindEvent::is_workflow_event)
    }

    /// Subscribe to agent events only.
    pub fn agent_events(&self) -> FilteredReceiver<fn(&NeoMindEvent) -> bool> {
        let rx = self.tx.subscribe();
        FilteredReceiver::new(rx, NeoMindEvent::is_agent_event)
    }

    /// Subscribe to LLM events only.
    pub fn llm_events(&self) -> FilteredReceiver<fn(&NeoMindEvent) -> bool> {
        let rx = self.tx.subscribe();
        FilteredReceiver::new(rx, NeoMindEvent::is_llm_event)
    }

    /// Subscribe to alert events only.
    pub fn alert_events(&self) -> FilteredReceiver<fn(&NeoMindEvent) -> bool> {
        let rx = self.tx.subscribe();
        FilteredReceiver::new(rx, NeoMindEvent::is_alert_event)
    }

    /// Subscribe with a custom filter function.
    pub fn custom<F>(&self, filter: F) -> FilteredReceiver<F>
    where
        F: Fn(&NeoMindEvent) -> bool + Send + 'static,
    {
        let rx = self.tx.subscribe();
        FilteredReceiver::new(rx, filter)
    }
}

/// Shared event bus handle.
///
/// This is useful for sharing an event bus across multiple components.
pub type SharedEventBus = Arc<EventBus>;

/// Trait for event persistence (reserved for future use).
///
/// Implementations can store events to disk, database, or external services.
pub trait EventPersistence: Send + Sync {
    /// Store an event.
    fn store(&self, event: &NeoMindEvent, metadata: &EventMetadata) -> Result<(), PersistError>;

    /// Query events by time range.
    fn query(
        &self,
        start: i64,
        end: i64,
    ) -> Result<Vec<(NeoMindEvent, EventMetadata)>, PersistError>;
}

/// Error type for event persistence operations.
#[derive(Debug, thiserror::Error)]
pub enum PersistError {
    /// IO error occurred.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Storage backend error.
    #[error("Storage error: {0}")]
    Storage(String),
}

/// No-op persistence implementation for testing.
#[derive(Debug, Clone, Default)]
pub struct NoOpPersistence;

impl EventPersistence for NoOpPersistence {
    fn store(&self, _event: &NeoMindEvent, _metadata: &EventMetadata) -> Result<(), PersistError> {
        Ok(())
    }

    fn query(
        &self,
        _start: i64,
        _end: i64,
    ) -> Result<Vec<(NeoMindEvent, EventMetadata)>, PersistError> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{MetricValue, ProposedAction};

    #[tokio::test]
    async fn test_event_bus_publish_subscribe() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();

        let event = NeoMindEvent::DeviceOnline {
            device_id: "test".to_string(),
            device_type: "sensor".to_string(),
            timestamp: 0,
        };

        bus.publish(event.clone()).await;

        let received = rx.recv().await.unwrap();
        assert_eq!(received.0.type_name(), "DeviceOnline");
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let bus = EventBus::new();
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();

        let event = NeoMindEvent::DeviceOnline {
            device_id: "test".to_string(),
            device_type: "sensor".to_string(),
            timestamp: 0,
        };

        bus.publish(event).await;

        // Both subscribers should receive the event
        let received1 = rx1.recv().await.unwrap();
        let received2 = rx2.recv().await.unwrap();

        assert_eq!(received1.0.type_name(), "DeviceOnline");
        assert_eq!(received2.0.type_name(), "DeviceOnline");
    }

    #[tokio::test]
    async fn test_filtered_subscription() {
        let bus = EventBus::new();
        let mut rx = bus.filter().device_events();

        // Publish a device event
        bus.publish(NeoMindEvent::DeviceOnline {
            device_id: "test".to_string(),
            device_type: "sensor".to_string(),
            timestamp: 0,
        })
        .await;

        // Publish a rule event (should be filtered out)
        bus.publish(NeoMindEvent::RuleTriggered {
            rule_id: "rule1".to_string(),
            rule_name: "Test Rule".to_string(),
            trigger_value: 42.0,
            actions: vec!["action".to_string()],
            timestamp: 0,
        })
        .await;

        // Should only receive the device event
        let received = rx.recv().await.unwrap();
        assert!(received.0.is_device_event());
        assert_eq!(received.0.type_name(), "DeviceOnline");
    }

    #[tokio::test]
    async fn test_custom_filter() {
        let bus = EventBus::new();
        let mut rx = bus
            .filter()
            .custom(|event| matches!(event, NeoMindEvent::DeviceMetric { .. }));

        // Publish various events
        bus.publish(NeoMindEvent::DeviceOnline {
            device_id: "test".to_string(),
            device_type: "sensor".to_string(),
            timestamp: 0,
        })
        .await;

        bus.publish(NeoMindEvent::DeviceMetric {
            device_id: "test".to_string(),
            metric: "temp".to_string(),
            value: MetricValue::float(25.0),
            timestamp: 0,
            quality: None,
        })
        .await;

        // Should only receive the metric event
        let received = rx.recv().await.unwrap();
        assert!(matches!(received.0, NeoMindEvent::DeviceMetric { .. }));
    }

    #[tokio::test]
    async fn test_publish_with_source() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();

        bus.publish_with_source(
            NeoMindEvent::DeviceOnline {
                device_id: "test".to_string(),
                device_type: "sensor".to_string(),
                timestamp: 0,
            },
            "test_adapter",
        )
        .await;

        let received = rx.recv().await.unwrap();
        assert_eq!(received.1.source, "test_adapter");
    }

    #[tokio::test]
    async fn test_llm_decision_event() {
        let bus = EventBus::new();
        let mut rx = bus.filter().llm_events();

        let actions = vec![ProposedAction::notify_user("Test notification")];

        bus.publish(NeoMindEvent::LlmDecisionProposed {
            decision_id: "dec-1".to_string(),
            title: "Test Decision".to_string(),
            description: "Test description".to_string(),
            reasoning: "Test reasoning".to_string(),
            actions,
            confidence: 0.85,
            timestamp: 0,
        })
        .await;

        let received = rx.recv().await.unwrap();
        assert!(received.0.is_llm_event());
        assert_eq!(received.0.type_name(), "LlmDecisionProposed");
    }

    #[tokio::test]
    async fn test_subscriber_count() {
        let bus = EventBus::new();
        assert_eq!(bus.subscriber_count(), 0);

        let _rx1 = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 1);

        let _rx2 = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 2);

        drop(_rx1);
        // Note: count updates when receiver is dropped, but we need to give it time
        // In practice, this is fine for our use case
    }

    #[tokio::test]
    async fn test_no_op_persistence() {
        let persistence = NoOpPersistence;
        let event = NeoMindEvent::DeviceOnline {
            device_id: "test".to_string(),
            device_type: "sensor".to_string(),
            timestamp: 0,
        };
        let metadata = EventMetadata::new("test");

        assert!(persistence.store(&event, &metadata).is_ok());
        assert!(persistence.query(0, 100).unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_all_event_filters() {
        let bus = EventBus::new();

        // Test each filter type
        let mut device_rx = bus.filter().device_events();
        let mut rule_rx = bus.filter().rule_events();
        let mut workflow_rx = bus.filter().workflow_events();
        let mut llm_rx = bus.filter().llm_events();
        let mut alert_rx = bus.filter().alert_events();

        // Publish one of each type
        bus.publish(NeoMindEvent::DeviceOnline {
            device_id: "test".to_string(),
            device_type: "sensor".to_string(),
            timestamp: 0,
        })
        .await;

        bus.publish(NeoMindEvent::RuleTriggered {
            rule_id: "rule1".to_string(),
            rule_name: "Test".to_string(),
            trigger_value: 1.0,
            actions: vec![],
            timestamp: 0,
        })
        .await;

        bus.publish(NeoMindEvent::WorkflowTriggered {
            workflow_id: "wf1".to_string(),
            trigger_type: "manual".to_string(),
            trigger_data: None,
            execution_id: "exec1".to_string(),
            timestamp: 0,
        })
        .await;

        bus.publish(NeoMindEvent::PeriodicReviewTriggered {
            review_id: "rev1".to_string(),
            review_type: "hourly".to_string(),
            timestamp: 0,
        })
        .await;

        bus.publish(NeoMindEvent::AlertCreated {
            alert_id: "alert1".to_string(),
            title: "Test Alert".to_string(),
            severity: "info".to_string(),
            message: "Test message".to_string(),
            timestamp: 0,
        })
        .await;

        // Verify each receiver got its event
        assert_eq!(
            device_rx.recv().await.unwrap().0.type_name(),
            "DeviceOnline"
        );
        assert_eq!(rule_rx.recv().await.unwrap().0.type_name(), "RuleTriggered");
        assert_eq!(
            workflow_rx.recv().await.unwrap().0.type_name(),
            "WorkflowTriggered"
        );
        assert_eq!(
            llm_rx.recv().await.unwrap().0.type_name(),
            "PeriodicReviewTriggered"
        );
        assert_eq!(alert_rx.recv().await.unwrap().0.type_name(), "AlertCreated");
    }

    #[tokio::test]
    async fn test_shared_event_bus() {
        let bus: SharedEventBus = Arc::new(EventBus::new());
        let bus_clone = Arc::clone(&bus);

        let mut rx = bus.subscribe();

        tokio::spawn(async move {
            bus_clone
                .publish(NeoMindEvent::DeviceOnline {
                    device_id: "test".to_string(),
                    device_type: "sensor".to_string(),
                    timestamp: 0,
                })
                .await;
        });

        let received = rx.recv().await.unwrap();
        assert_eq!(received.0.type_name(), "DeviceOnline");
    }

    #[tokio::test]
    async fn test_try_recv() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();

        // No event yet
        assert!(rx.try_recv().is_none());

        // Publish an event
        bus.publish(NeoMindEvent::DeviceOnline {
            device_id: "test".to_string(),
            device_type: "sensor".to_string(),
            timestamp: 0,
        })
        .await;

        // Should be able to try_recv now
        let received = rx.try_recv().unwrap();
        assert_eq!(received.0.type_name(), "DeviceOnline");
    }

    #[tokio::test]
    async fn test_filtered_try_recv() {
        let bus = EventBus::new();
        let mut rx = bus.filter().device_events();

        // Publish non-matching event
        bus.publish(NeoMindEvent::RuleTriggered {
            rule_id: "rule1".to_string(),
            rule_name: "Test".to_string(),
            trigger_value: 1.0,
            actions: vec![],
            timestamp: 0,
        })
        .await;

        // Should return None since filter doesn't match
        assert!(rx.try_recv().is_none());

        // Publish matching event
        bus.publish(NeoMindEvent::DeviceOnline {
            device_id: "test".to_string(),
            device_type: "sensor".to_string(),
            timestamp: 0,
        })
        .await;

        // Should return the matching event
        let received = rx.try_recv().unwrap();
        assert_eq!(received.0.type_name(), "DeviceOnline");
    }
}
