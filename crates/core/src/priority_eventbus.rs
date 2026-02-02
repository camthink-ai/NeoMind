//! Priority-based event handling extension for EventBus.
//!
//! This module provides a priority queue wrapper around the standard EventBus
//! to ensure critical events are processed first during high load.

use crate::event::{EventMetadata, NeoTalkEvent};
use crate::eventbus::EventBus;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Event priority levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[derive(Default)]
pub enum EventPriority {
    /// Low priority - informational events that can be delayed
    Low = 0,
    /// Normal priority - regular events (default)
    #[default]
    Normal = 1,
    /// High priority - important events that should be processed soon
    High = 2,
    /// Critical priority - urgent events that must be processed immediately
    Critical = 3,
}


/// Wrapper that combines an event with its priority for ordering.
#[derive(Debug, Clone)]
struct PrioritizedEvent {
    /// The event itself
    event: NeoTalkEvent,
    /// Event metadata
    metadata: EventMetadata,
    /// Priority level (higher = more important)
    priority: EventPriority,
    /// Sequence number for FIFO ordering within same priority
    sequence: u64,
}

// Implement reverse ordering for BinaryHeap (max heap)
impl PartialEq for PrioritizedEvent {
    fn eq(&self, other: &Self) -> bool {
        self.sequence == other.sequence
    }
}

impl Eq for PrioritizedEvent {}

impl PartialOrd for PrioritizedEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrioritizedEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        // First compare by priority (higher priority first)
        match self.priority.cmp(&other.priority) {
            Ordering::Equal => {
                // Same priority, use sequence number (FIFO)
                self.sequence.cmp(&other.sequence)
            }
            other => other,
        }
    }
}

/// Priority-based event bus wrapper.
///
/// This wraps the standard EventBus and adds priority queue functionality.
/// Events are buffered and processed based on their priority level.
pub struct PriorityEventBus {
    /// Underlying event bus
    event_bus: EventBus,
    /// Priority queue for pending events
    queue: Arc<Mutex<BinaryHeap<PrioritizedEvent>>>,
    /// Next sequence number for FIFO ordering
    sequence: Arc<Mutex<u64>>,
    /// Maximum queue size before dropping low-priority events
    max_queue_size: usize,
}

impl PriorityEventBus {
    /// Create a new priority event bus wrapping the given event bus.
    pub fn new(event_bus: EventBus) -> Self {
        Self {
            event_bus,
            queue: Arc::new(Mutex::new(BinaryHeap::new())),
            sequence: Arc::new(Mutex::new(0)),
            max_queue_size: 10_000,
        }
    }

    /// Set the maximum queue size.
    pub fn with_max_queue_size(mut self, size: usize) -> Self {
        self.max_queue_size = size;
        self
    }

    /// Publish an event with a specific priority.
    pub async fn publish_with_priority(
        &self,
        event: NeoTalkEvent,
        priority: EventPriority,
    ) -> bool {
        let seq = {
            let mut seq = self.sequence.lock().await;
            let current = *seq;
            *seq = seq.wrapping_add(1);
            current
        };

        let prioritized = PrioritizedEvent {
            event,
            metadata: EventMetadata::new("priority_bus"),
            priority,
            sequence: seq,
        };

        // Check queue size and drop low-priority events if necessary
        let mut queue = self.queue.lock().await;
        if queue.len() >= self.max_queue_size {
            if priority < EventPriority::High {
                // Drop low/normal priority events when queue is full
                return false;
            }
            // For high/critical events, remove lowest priority items
            while queue.len() >= self.max_queue_size {
                if let Some(removed) = queue.pop() {
                    if removed.priority >= priority {
                        // Put it back and fail
                        queue.push(removed);
                        return false;
                    }
                } else {
                    break;
                }
            }
        }
        queue.push(prioritized);
        true
    }

    /// Publish a critical event (highest priority).
    pub async fn publish_critical(&self, event: NeoTalkEvent) -> bool {
        self.publish_with_priority(event, EventPriority::Critical).await
    }

    /// Publish a high-priority event.
    pub async fn publish_high(&self, event: NeoTalkEvent) -> bool {
        self.publish_with_priority(event, EventPriority::High).await
    }

    /// Process pending events from the priority queue.
    ///
    /// This should be called periodically (e.g., in a background task)
    /// to drain the queue and publish events to the underlying event bus.
    pub async fn process_queue(&self, limit: usize) -> usize {
        let mut queue = self.queue.lock().await;
        let mut count = 0;

        while count < limit {
            if let Some(prioritized) = queue.pop() {
                drop(queue); // Release lock before publishing
                self.event_bus
                    .publish_with_metadata(prioritized.event, prioritized.metadata)
                    .await;
                count += 1;
                queue = self.queue.lock().await;
            } else {
                break;
            }
        }

        count
    }

    /// Get the number of pending events in the queue.
    pub async fn pending_count(&self) -> usize {
        self.queue.lock().await.len()
    }

    /// Get the underlying event bus.
    pub fn inner(&self) -> &EventBus {
        &self.event_bus
    }

    /// Start a background task to process the queue.
    ///
    /// Returns a handle that can be used to stop the processor.
    pub fn spawn_processor(&self, interval_ms: u64, batch_size: usize) -> EventProcessorHandle {
        let queue = self.queue.clone();
        let event_bus = self.event_bus.clone();
        let running = Arc::new(Mutex::new(true));
        let running_clone = running.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(interval_ms));
            loop {
                interval.tick().await;
                if !*running_clone.lock().await {
                    break;
                }

                let mut queue_guard = queue.lock().await;
                let mut count = 0;
                while count < batch_size {
                    if let Some(prioritized) = queue_guard.pop() {
                        drop(queue_guard);
                        event_bus
                            .publish_with_metadata(prioritized.event, prioritized.metadata)
                            .await;
                        count += 1;
                        queue_guard = queue.lock().await;
                    } else {
                        break;
                    }
                }
            }
        });

        EventProcessorHandle { running }
    }
}

impl Clone for PriorityEventBus {
    fn clone(&self) -> Self {
        Self {
            event_bus: self.event_bus.clone(),
            queue: self.queue.clone(),
            sequence: self.sequence.clone(),
            max_queue_size: self.max_queue_size,
        }
    }
}

/// Handle to control an event processor task.
pub struct EventProcessorHandle {
    running: Arc<Mutex<bool>>,
}

impl EventProcessorHandle {
    /// Stop the event processor task.
    pub async fn stop(self) {
        *self.running.lock().await = false;
    }
}

/// Helper to determine event priority based on event type.
pub fn event_priority(event: &NeoTalkEvent) -> EventPriority {
    match event {
        // Critical events - device failures, alerts, security issues
        NeoTalkEvent::DeviceOffline { .. }
        | NeoTalkEvent::AlertCreated { .. } => EventPriority::Critical,

        // High priority - device online, rule triggered, workflow triggered
        NeoTalkEvent::DeviceOnline { .. }
        | NeoTalkEvent::RuleTriggered { .. }
        | NeoTalkEvent::WorkflowTriggered { .. }
        | NeoTalkEvent::DeviceCommandResult { success: false, .. } => EventPriority::High,

        // Normal priority - regular events
        NeoTalkEvent::DeviceMetric { .. }
        | NeoTalkEvent::RuleEvaluated { .. }
        | NeoTalkEvent::WorkflowStepCompleted { .. }
        | NeoTalkEvent::WorkflowCompleted { .. }
        | NeoTalkEvent::DeviceCommandResult { success: true, .. } => EventPriority::Normal,

        // Low priority - informational events
        NeoTalkEvent::LlmDecisionProposed { .. }
        | NeoTalkEvent::LlmDecisionExecuted { .. }
        | NeoTalkEvent::PeriodicReviewTriggered { .. }
        | NeoTalkEvent::RuleExecuted { .. }
        | NeoTalkEvent::AgentExecutionCompleted { .. }
        | NeoTalkEvent::UserMessage { .. }
        | NeoTalkEvent::AgentThinking { .. } => EventPriority::Low,

        // Default to normal priority for all other events
        _ => EventPriority::Normal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_priority_ordering() {
        assert!(EventPriority::Critical > EventPriority::High);
        assert!(EventPriority::High > EventPriority::Normal);
        assert!(EventPriority::Normal > EventPriority::Low);
    }

    #[test]
    fn test_prioritized_event_ordering() {
        let e1 = PrioritizedEvent {
            event: NeoTalkEvent::DeviceOnline {
                device_id: "test".to_string(),
                device_type: "sensor".to_string(),
                timestamp: 0,
            },
            metadata: EventMetadata::new("test"),
            priority: EventPriority::Normal,
            sequence: 1,
        };
        let e2 = PrioritizedEvent {
            event: NeoTalkEvent::DeviceOffline {
                device_id: "test".to_string(),
                reason: None,
                timestamp: 0,
            },
            metadata: EventMetadata::new("test"),
            priority: EventPriority::Critical,
            sequence: 0,
        };

        // e2 has higher priority (Critical vs Normal)
        assert!(e2 > e1);
    }
}
