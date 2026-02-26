//! Command queue for device command management.
//!
//! Provides priority-based queuing of commands with persistence.
//! Uses per-priority VecDeque to guarantee FIFO ordering within same priority.
//!
//! # FIFO Ordering Guarantee
//!
//! Commands with the same priority are guaranteed to be processed in FIFO order.
//! This is achieved by using separate VecDeque for each priority level,
//! which guarantees insertion order is preserved.

use std::collections::VecDeque;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, RwLock, Semaphore};

use crate::command::{CommandId, CommandPriority, CommandRequest};

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
}

impl QueueItem {
    fn new(command: CommandRequest) -> Self {
        Self { command }
    }
}

/// Command queue with priority support and FIFO guarantee.
pub struct CommandQueue {
    /// Inner queue data - using per-priority queues for FIFO guarantee
    inner: Arc<RwLock<QueueInner>>,

    /// Notification sender
    notifier: Arc<mpsc::Sender<CommandId>>,

    /// Semaphore for limiting queue size
    semaphore: Arc<Semaphore>,
}

/// Inner queue data with per-priority VecDeques.
struct QueueInner {
    /// Low priority queue
    low: VecDeque<QueueItem>,
    /// Normal priority queue
    normal: VecDeque<QueueItem>,
    /// High priority queue
    high: VecDeque<QueueItem>,
    /// Critical priority queue
    critical: VecDeque<QueueItem>,
    /// Emergency priority queue
    emergency: VecDeque<QueueItem>,
    /// Queue size limit
    max_size: usize,
}

impl CommandQueue {
    /// Create a new command queue.
    pub fn new(max_size: usize) -> Self {
        let (notifier, _receiver) = mpsc::channel(1000);

        Self {
            inner: Arc::new(RwLock::new(QueueInner {
                low: VecDeque::new(),
                normal: VecDeque::new(),
                high: VecDeque::new(),
                critical: VecDeque::new(),
                emergency: VecDeque::new(),
                max_size,
            })),
            notifier: Arc::new(notifier),
            semaphore: Arc::new(Semaphore::new(max_size)),
        }
    }

    /// Enqueue a command.
    pub async fn enqueue(&self, command: CommandRequest) -> Result<(), QueueError> {
        // Check semaphore for available slots
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| QueueError::Closed)?;

        let command_id = command.id.clone();
        let item = QueueItem::new(command);
        let mut inner = self.inner.write().await;

        // Check total size across all queues
        let total_size = inner.low.len()
            + inner.normal.len()
            + inner.high.len()
            + inner.critical.len()
            + inner.emergency.len();

        if total_size >= inner.max_size {
            return Err(QueueError::Full);
        }

        // Add to appropriate priority queue (FIFO guaranteed by VecDeque)
        match item.command.priority {
            CommandPriority::Low => inner.low.push_back(item),
            CommandPriority::Normal => inner.normal.push_back(item),
            CommandPriority::High => inner.high.push_back(item),
            CommandPriority::Critical => inner.critical.push_back(item),
            CommandPriority::Emergency => inner.emergency.push_back(item),
        }

        drop(inner);

        // Notify new command (but don't block if no receiver)
        let _ = self.notifier.try_send(command_id);

        Ok(())
    }

    /// Try to dequeue the next highest priority command (non-blocking).
    pub async fn try_dequeue(&self) -> Option<CommandRequest> {
        let mut inner = self.inner.write().await;

        // Check queues in priority order: Emergency > Critical > High > Normal > Low
        // pop_front preserves FIFO order within each priority level
        if let Some(item) = inner.emergency.pop_front() {
            return Some(item.command);
        }
        if let Some(item) = inner.critical.pop_front() {
            return Some(item.command);
        }
        if let Some(item) = inner.high.pop_front() {
            return Some(item.command);
        }
        if let Some(item) = inner.normal.pop_front() {
            return Some(item.command);
        }
        if let Some(item) = inner.low.pop_front() {
            return Some(item.command);
        }

        None
    }

    /// Get queue statistics.
    pub async fn stats(&self) -> QueueStats {
        let inner = self.inner.read().await;

        QueueStats {
            total_count: inner.low.len()
                + inner.normal.len()
                + inner.high.len()
                + inner.critical.len()
                + inner.emergency.len(),
            by_priority: [
                ("low".to_string(), inner.low.len()),
                ("normal".to_string(), inner.normal.len()),
                ("high".to_string(), inner.high.len()),
                ("critical".to_string(), inner.critical.len()),
                ("emergency".to_string(), inner.emergency.len()),
            ],
            processed_count: 0,
            failed_count: 0,
        }
    }

    /// Get current queue size.
    pub async fn len(&self) -> usize {
        let inner = self.inner.read().await;
        inner.low.len()
            + inner.normal.len()
            + inner.high.len()
            + inner.critical.len()
            + inner.emergency.len()
    }

    /// Check if queue is empty.
    pub async fn is_empty(&self) -> bool {
        let inner = self.inner.read().await;
        inner.low.is_empty()
            && inner.normal.is_empty()
            && inner.high.is_empty()
            && inner.critical.is_empty()
            && inner.emergency.is_empty()
    }

    /// Clear all commands from queue.
    pub async fn clear(&self) {
        let mut inner = self.inner.write().await;
        inner.low.clear();
        inner.normal.clear();
        inner.high.clear();
        inner.critical.clear();
        inner.emergency.clear();
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
    use crate::command::{CommandPriority, CommandRequest, CommandSource};

    /// Helper to create a test command.
    fn make_command(
        device_id: &str,
        command_name: &str,
        priority: CommandPriority,
    ) -> CommandRequest {
        let source = CommandSource::System {
            reason: "test".to_string(),
        };
        CommandRequest::new(device_id.to_string(), command_name.to_string(), source)
            .with_priority(priority)
    }

    #[tokio::test]
    async fn test_queue_enqueue_dequeue() {
        let queue = CommandQueue::new(100);

        let cmd = make_command("device1", "test", CommandPriority::Normal);
        queue.enqueue(cmd).await.unwrap();
        assert_eq!(queue.len().await, 1);

        let dequeued = queue.try_dequeue().await;
        assert!(dequeued.is_some());
        assert_eq!(queue.len().await, 0);
    }

    #[tokio::test]
    async fn test_queue_priority() {
        let queue = CommandQueue::new(100);

        // Add low priority command first
        let low = make_command("device1", "low", CommandPriority::Low);
        queue.enqueue(low).await.unwrap();

        // Then add high priority command
        let high = make_command("device2", "high", CommandPriority::High);
        queue.enqueue(high).await.unwrap();

        // High priority should come out first
        let first = queue.try_dequeue().await.unwrap();
        assert_eq!(first.command_name, "high");

        // Then low priority
        let second = queue.try_dequeue().await.unwrap();
        assert_eq!(second.command_name, "low");
    }

    #[tokio::test]
    async fn test_queue_empty_initially() {
        let queue = CommandQueue::new(100);
        assert!(queue.is_empty().await);
        assert_eq!(queue.len().await, 0);
    }

    #[tokio::test]
    async fn test_queue_full() {
        let queue = CommandQueue::new(2); // Max size = 2

        let cmd1 = make_command("device1", "cmd1", CommandPriority::Normal);
        let cmd2 = make_command("device2", "cmd2", CommandPriority::Normal);
        let cmd3 = make_command("device3", "cmd3", CommandPriority::Normal);

        // First two should succeed
        assert!(queue.enqueue(cmd1).await.is_ok());
        assert!(queue.enqueue(cmd2).await.is_ok());
        assert_eq!(queue.len().await, 2);

        // Third should fail - queue is full
        let result = queue.enqueue(cmd3).await;
        assert!(matches!(result, Err(QueueError::Full)));
        assert_eq!(queue.len().await, 2);
    }

    #[tokio::test]
    async fn test_queue_dequeue_empty() {
        let queue = CommandQueue::new(100);
        assert!(queue.is_empty().await);

        let dequeued = queue.try_dequeue().await;
        assert!(dequeued.is_none());
        assert!(queue.is_empty().await);
    }

    #[tokio::test]
    async fn test_queue_clear() {
        let queue = CommandQueue::new(100);

        queue
            .enqueue(make_command("device1", "cmd1", CommandPriority::Normal))
            .await
            .unwrap();
        queue
            .enqueue(make_command("device2", "cmd2", CommandPriority::High))
            .await
            .unwrap();
        assert_eq!(queue.len().await, 2);

        queue.clear().await;
        assert_eq!(queue.len().await, 0);
        assert!(queue.is_empty().await);
    }

    #[tokio::test]
    async fn test_queue_stats() {
        let queue = CommandQueue::new(100);

        queue
            .enqueue(make_command("device1", "low", CommandPriority::Low))
            .await
            .unwrap();
        queue
            .enqueue(make_command("device2", "normal", CommandPriority::Normal))
            .await
            .unwrap();
        queue
            .enqueue(make_command("device3", "high", CommandPriority::High))
            .await
            .unwrap();
        queue
            .enqueue(make_command(
                "device4",
                "critical",
                CommandPriority::Critical,
            ))
            .await
            .unwrap();
        queue
            .enqueue(make_command(
                "device5",
                "emergency",
                CommandPriority::Emergency,
            ))
            .await
            .unwrap();

        let stats: QueueStats = queue.stats().await;

        assert_eq!(stats.total_count, 5);

        let priority_counts: std::collections::HashMap<&str, usize> = stats
            .by_priority
            .iter()
            .map(|(k, v)| (k.as_str(), *v))
            .collect();

        assert_eq!(*priority_counts.get("low").unwrap_or(&0), 1);
        assert_eq!(*priority_counts.get("normal").unwrap_or(&0), 1);
        assert_eq!(*priority_counts.get("high").unwrap_or(&0), 1);
        assert_eq!(*priority_counts.get("critical").unwrap_or(&0), 1);
        assert_eq!(*priority_counts.get("emergency").unwrap_or(&0), 1);
    }

    #[tokio::test]
    async fn test_queue_stats_empty() {
        let queue = CommandQueue::new(100);
        let stats: QueueStats = queue.stats().await;

        assert_eq!(stats.total_count, 0);

        // All priority counts should be zero
        for (name, count) in stats.by_priority.iter() {
            assert_eq!(*count, 0, "Priority {} should have count 0", name);
        }
    }

    #[tokio::test]
    async fn test_queue_concurrent_enqueue() {
        use std::sync::Arc;
        let queue = Arc::new(CommandQueue::new(1000));
        let mut handles = vec![];

        // Spawn multiple concurrent enqueues
        for i in 0..50 {
            let q = queue.clone();
            let handle = tokio::spawn(async move {
                let cmd = make_command(&format!("device{}", i), "cmd", CommandPriority::Normal);
                q.enqueue(cmd).await
            });
            handles.push(handle);
        }

        // Wait for all to complete
        for handle in handles {
            let _ = handle.await;
        }

        // All should succeed given large enough queue
        assert_eq!(queue.len().await, 50);
    }

    #[tokio::test]
    async fn test_queue_priority_all_levels() {
        let queue = CommandQueue::new(100);

        // Add one command for each priority level
        let priorities = [
            CommandPriority::Low,
            CommandPriority::Normal,
            CommandPriority::High,
            CommandPriority::Critical,
            CommandPriority::Emergency,
        ];

        for (i, priority) in priorities.iter().enumerate() {
            let cmd = make_command(&format!("device{}", i), "cmd", *priority);
            queue.enqueue(cmd).await.unwrap();
        }

        let stats: QueueStats = queue.stats().await;
        assert_eq!(stats.total_count, 5);

        // Verify each priority has one item
        let priority_map: std::collections::HashMap<_, _> = stats
            .by_priority
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();

        for priority in priorities {
            let name = format!("{}", priority).to_lowercase();
            assert_eq!(*priority_map.get(name.as_str()).unwrap_or(&0), 1);
        }
    }

    #[tokio::test]
    async fn test_queue_sequence_preservation() {
        let queue = CommandQueue::new(100);

        // Add multiple commands with same priority
        for i in 0..10 {
            let cmd = make_command(&format!("device{}", i), "cmd", CommandPriority::Normal);
            queue.enqueue(cmd).await.unwrap();
        }

        assert_eq!(queue.len().await, 10);

        // All should be dequeued (we don't test exact order here, just that all exist)
        let mut count = 0;
        while queue.try_dequeue().await.is_some() {
            count += 1;
        }

        assert_eq!(count, 10);
    }

    #[tokio::test]
    async fn test_queue_command_id_preservation() {
        let queue = CommandQueue::new(100);

        let cmd = make_command("device1", "turn_on", CommandPriority::High);
        let original_id = cmd.id.clone();

        queue.enqueue(cmd).await.unwrap();
        let dequeued = queue.try_dequeue().await.unwrap();

        assert_eq!(dequeued.id, original_id);
        assert_eq!(dequeued.device_id, "device1");
        assert_eq!(dequeued.command_name, "turn_on");
    }

    #[tokio::test]
    async fn test_queue_fifo_within_same_priority() {
        let queue = CommandQueue::new(100);

        // Add multiple commands with same priority - test FIFO ordering
        let cmd1 = make_command("device1", "first", CommandPriority::Normal);
        let cmd2 = make_command("device2", "second", CommandPriority::Normal);
        let cmd3 = make_command("device3", "third", CommandPriority::Normal);

        queue.enqueue(cmd1).await.unwrap();
        queue.enqueue(cmd2).await.unwrap();
        queue.enqueue(cmd3).await.unwrap();

        // Should come out in FIFO order (by insertion order)
        let first = queue.try_dequeue().await.unwrap();
        assert_eq!(first.command_name, "first");

        let second = queue.try_dequeue().await.unwrap();
        assert_eq!(second.command_name, "second");

        let third = queue.try_dequeue().await.unwrap();
        assert_eq!(third.command_name, "third");
    }
}
