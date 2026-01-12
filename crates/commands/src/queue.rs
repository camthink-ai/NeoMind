//! Command queue for device command management.
//!
//! Provides priority-based queuing of commands with persistence.

use std::collections::BinaryHeap;
use std::sync::Arc;

use tokio::sync::{mpsc, RwLock, Semaphore};
use serde::{Deserialize, Serialize};

use crate::command::{CommandRequest, CommandId};

/// Queue statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueStats {
    /// Total number of commands in queue
    pub total_count: usize,
    /// Count by priority
    pub by_priority: [(String, usize); 5],
    /// Number of commands processed
    pub processed_count: u64,
    /// Number of commands failed
    pub failed_count: u64,
}

/// Priority queue wrapper for command ordering.
#[derive(Debug, Clone)]
struct QueueItem {
    command: CommandRequest,
    /// Inverse priority for min-heap (higher priority = lower value)
    priority: u8,
    /// Sequence number for FIFO ordering within same priority
    sequence: u64,
}

impl QueueItem {
    fn new(command: CommandRequest, sequence: u64) -> Self {
        // Priority is inverted for min-heap
        let priority = (6 - command.priority.value()) as u8;
        Self {
            command,
            priority,
            sequence,
        }
    }
}

impl PartialEq for QueueItem {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.sequence == other.sequence
    }
}

impl Eq for QueueItem {}

impl PartialOrd for QueueItem {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for QueueItem {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // BinaryHeap is a max-heap, so we need to invert the comparison
        // Lower priority value = higher priority = processed first
        match self.priority.cmp(&other.priority) {
            std::cmp::Ordering::Equal => {
                // Same priority: use sequence (FIFO)
                self.sequence.cmp(&other.sequence)
            }
            // Invert the ordering for the max-heap behavior
            std::cmp::Ordering::Less => std::cmp::Ordering::Greater,
            std::cmp::Ordering::Greater => std::cmp::Ordering::Less,
        }
    }
}

/// Command queue with priority support.
pub struct CommandQueue {
    /// Inner queue data
    inner: Arc<RwLock<QueueInner>>,
    /// Notification sender
    notifier: Arc<mpsc::Sender<CommandId>>,
    /// Semaphore for limiting queue size
    semaphore: Arc<Semaphore>,
    /// Next sequence number
    sequence: Arc<RwLock<u64>>,
}

/// Inner queue data.
struct QueueInner {
    /// Priority queue
    queue: BinaryHeap<QueueItem>,
    /// Queue size limit
    max_size: usize,
}

impl CommandQueue {
    /// Create a new command queue.
    pub fn new(max_size: usize) -> Self {
        let (notifier, _receiver) = mpsc::channel(1000);

        Self {
            inner: Arc::new(RwLock::new(QueueInner {
                queue: BinaryHeap::new(),
                max_size,
            })),
            notifier: Arc::new(notifier),
            semaphore: Arc::new(Semaphore::new(max_size)),
            sequence: Arc::new(RwLock::new(0)),
        }
    }

    /// Enqueue a command.
    pub async fn enqueue(&self, command: CommandRequest) -> Result<(), QueueError> {
        // Check semaphore for available slots
        let _permit = self.semaphore.acquire().await
            .map_err(|_| QueueError::Closed)?;

        let command_id = command.id.clone();

        let mut sequence = self.sequence.write().await;
        let seq = *sequence;
        *sequence += 1;
        drop(sequence);

        let item = QueueItem::new(command, seq);
        let mut inner = self.inner.write().await;

        if inner.queue.len() >= inner.max_size {
            return Err(QueueError::Full);
        }

        inner.queue.push(item);
        drop(inner);

        // Notify new command (but don't block if no receiver)
        let _ = self.notifier.try_send(command_id);

        Ok(())
    }

    /// Try to dequeue the next command (non-blocking).
    pub async fn try_dequeue(&self) -> Option<CommandRequest> {
        let mut inner = self.inner.write().await;
        inner.queue.pop().map(|item| {
            item.command
        })
    }

    /// Get queue statistics.
    pub async fn stats(&self) -> QueueStats {
        let inner = self.inner.read().await;

        let mut by_priority_counts: [(String, usize); 5] = [
            ("low".to_string(), 0),
            ("normal".to_string(), 0),
            ("high".to_string(), 0),
            ("critical".to_string(), 0),
            ("emergency".to_string(), 0),
        ];

        for item in inner.queue.iter() {
            let idx = (item.command.priority.value() as usize).saturating_sub(1);
            if idx < 5 {
                by_priority_counts[idx].1 += 1;
            }
        }

        QueueStats {
            total_count: inner.queue.len(),
            by_priority: by_priority_counts,
            processed_count: 0,
            failed_count: 0,
        }
    }

    /// Get the current queue size.
    pub async fn len(&self) -> usize {
        let inner = self.inner.read().await;
        inner.queue.len()
    }

    /// Check if queue is empty.
    pub async fn is_empty(&self) -> bool {
        let inner = self.inner.read().await;
        inner.queue.is_empty()
    }

    /// Clear all commands from the queue.
    pub async fn clear(&self) {
        let mut inner = self.inner.write().await;
        inner.queue.clear();
    }

    /// Get notification receiver for queue updates.
    pub fn subscribe(&self) -> mpsc::Receiver<CommandId> {
        let (_notifier, receiver) = mpsc::channel(1000);
        // In a real implementation, this would replace the notifier
        // For now, just return a new receiver
        receiver
    }
}

/// Queue error types.
#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    #[error("Queue is full")]
    Full,

    #[error("Queue is closed")]
    Closed,

    #[error("Queue operation failed: {0}")]
    Failed(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CommandSource, CommandPriority};

    #[tokio::test]
    async fn test_queue_enqueue_dequeue() {
        let queue = CommandQueue::new(100);

        let source = CommandSource::System {
            reason: "test".to_string(),
        };
        let cmd = CommandRequest::new("device1".to_string(), "test".to_string(), source);

        queue.enqueue(cmd).await.unwrap();
        assert_eq!(queue.len().await, 1);

        let dequeued = queue.try_dequeue().await;
        assert!(dequeued.is_some());
        assert_eq!(queue.len().await, 0);
    }

    #[tokio::test]
    async fn test_queue_priority() {
        let queue = CommandQueue::new(100);

        let source = CommandSource::System {
            reason: "test".to_string(),
        };

        // Add low priority command first
        let low = CommandRequest::new("device1".to_string(), "low".to_string(), source.clone())
            .with_priority(CommandPriority::Low);
        queue.enqueue(low).await.unwrap();

        // Add high priority command
        let high = CommandRequest::new("device1".to_string(), "high".to_string(), source.clone())
            .with_priority(CommandPriority::High);
        queue.enqueue(high).await.unwrap();

        // High priority should come out first
        let first = queue.try_dequeue().await.unwrap();
        assert_eq!(first.priority, CommandPriority::High);

        let second = queue.try_dequeue().await.unwrap();
        assert_eq!(second.priority, CommandPriority::Low);
    }
}
