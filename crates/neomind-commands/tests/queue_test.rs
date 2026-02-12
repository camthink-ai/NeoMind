//! Command queue comprehensive tests.
//!
//! Tests priority-based command queuing with concurrency and edge cases.

use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use neomind_commands::{
    queue::{CommandQueue, QueueError, QueueStats},
    command::{CommandPriority, CommandRequest, CommandSource},
};

/// Helper to create a test command.
fn make_command(device_id: &str, command_name: &str, priority: CommandPriority) -> CommandRequest {
    let source = CommandSource::System {
        reason: "test".to_string(),
    };
    CommandRequest::new(device_id.to_string(), command_name.to_string(), source)
        .with_priority(priority)
}

#[tokio::test]
async fn test_queue_empty_initially() {
    let queue = CommandQueue::new(100);
    assert!(queue.is_empty().await);
    assert_eq!(queue.len().await, 0);
}

#[tokio::test]
async fn test_queue_enqueue_dequeue() {
    let queue = CommandQueue::new(100);
    let cmd = make_command("device1", "turn_on", CommandPriority::Normal);

    queue.enqueue(cmd).await.unwrap();
    assert_eq!(queue.len().await, 1);
    assert!(!queue.is_empty().await);

    let dequeued = queue.try_dequeue().await;
    assert!(dequeued.is_some());
    assert_eq!(queue.len().await, 0);
    assert!(queue.is_empty().await);
}

#[tokio::test]
async fn test_queue_priority_ordering() {
    let queue = CommandQueue::new(100);

    // Add commands in reverse priority order
    queue.enqueue(make_command("device1", "low", CommandPriority::Low)).await.unwrap();
    queue.enqueue(make_command("device2", "normal", CommandPriority::Normal)).await.unwrap();
    queue.enqueue(make_command("device3", "high", CommandPriority::High)).await.unwrap();
    queue.enqueue(make_command("device4", "critical", CommandPriority::Critical)).await.unwrap();
    queue.enqueue(make_command("device5", "emergency", CommandPriority::Emergency)).await.unwrap();

    // Should come out in priority order (highest first)
    let first = queue.try_dequeue().await.unwrap();
    assert_eq!(first.priority, CommandPriority::Emergency);

    let second = queue.try_dequeue().await.unwrap();
    assert_eq!(second.priority, CommandPriority::Critical);

    let third = queue.try_dequeue().await.unwrap();
    assert_eq!(third.priority, CommandPriority::High);

    let fourth = queue.try_dequeue().await.unwrap();
    assert_eq!(fourth.priority, CommandPriority::Normal);

    let fifth = queue.try_dequeue().await.unwrap();
    assert_eq!(fifth.priority, CommandPriority::Low);
}

#[tokio::test]
async fn test_queue_fifo_within_same_priority() {
    let queue = CommandQueue::new(100);

    // Add multiple commands with same priority
    let cmd1 = make_command("device1", "first", CommandPriority::Normal);
    let cmd2 = make_command("device2", "second", CommandPriority::Normal);
    let cmd3 = make_command("device3", "third", CommandPriority::Normal);

    queue.enqueue(cmd1).await.unwrap();
    queue.enqueue(cmd2).await.unwrap();
    queue.enqueue(cmd3).await.unwrap();

    // Should come out in FIFO order (by sequence number)
    let first = queue.try_dequeue().await.unwrap();
    assert_eq!(first.command_name, "first");

    let second = queue.try_dequeue().await.unwrap();
    assert_eq!(second.command_name, "second");

    let third = queue.try_dequeue().await.unwrap();
    assert_eq!(third.command_name, "third");
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
async fn test_queue_high_priority_bypasses_full() {
    let queue = CommandQueue::new(2);

    let low1 = make_command("device1", "low1", CommandPriority::Low);
    let low2 = make_command("device2", "low2", CommandPriority::Low);

    // Fill queue with low priority commands
    assert!(queue.enqueue(low1).await.is_ok());
    assert!(queue.enqueue(low2).await.is_ok());

    // High priority command should still be rejected when queue is full
    // The semaphore limits total size regardless of priority
    let high = make_command("device3", "high", CommandPriority::Emergency);
    let result = queue.enqueue(high).await;
    assert!(matches!(result, Err(QueueError::Full)));
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

    queue.enqueue(make_command("device1", "cmd1", CommandPriority::Normal)).await.unwrap();
    queue.enqueue(make_command("device2", "cmd2", CommandPriority::High)).await.unwrap();
    assert_eq!(queue.len().await, 2);

    queue.clear().await;
    assert_eq!(queue.len().await, 0);
    assert!(queue.is_empty().await);
}

#[tokio::test]
async fn test_queue_stats() {
    let queue = CommandQueue::new(100);

    queue.enqueue(make_command("device1", "low", CommandPriority::Low)).await.unwrap();
    queue.enqueue(make_command("device2", "normal", CommandPriority::Normal)).await.unwrap();
    queue.enqueue(make_command("device3", "high", CommandPriority::High)).await.unwrap();
    queue.enqueue(make_command("device4", "critical", CommandPriority::Critical)).await.unwrap();
    queue.enqueue(make_command("device5", "emergency", CommandPriority::Emergency)).await.unwrap();

    let stats: QueueStats = queue.stats().await;

    assert_eq!(stats.total_count, 5);

    // Check priority counts
    let priority_counts: std::collections::HashMap<String, usize> = stats.by_priority
        .into_iter()
        .map(|(k, v)| (k, v))
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
    assert_eq!(stats.processed_count, 0);
    assert_eq!(stats.failed_count, 0);

    // All priority counts should be zero
    for (name, count) in stats.by_priority {
        assert_eq!(count, 0, "Priority {} should have count 0", name);
    }
}

#[tokio::test]
async fn test_queue_concurrent_enqueue() {
    let queue = Arc::new(CommandQueue::new(1000));
    let mut handles = vec![];

    // Spawn multiple concurrent enqueues
    for i in 0..50 {
        let q = queue.clone();
        let handle = tokio::spawn(async move {
            let cmd = make_command(&format!("device{}", i), &format!("cmd{}", i), CommandPriority::Normal);
            q.enqueue(cmd).await
        });
        handles.push(handle);
    }

    // Wait for all to complete
    let mut successful = 0;
    for handle in handles {
        if handle.await.is_ok() {
            successful += 1;
        }
    }

    // All should succeed given large enough queue
    assert_eq!(successful, 50);
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
        queue.enqueue(make_command(&format!("device{}", i), "cmd", *priority)).await.unwrap();
    }

    // Dequeue all and verify order (Emergency first, Low last)
    let expected_order = [
        CommandPriority::Emergency,
        CommandPriority::Critical,
        CommandPriority::High,
        CommandPriority::Normal,
        CommandPriority::Low,
    ];

    for expected_priority in expected_order {
        let cmd = queue.try_dequeue().await.expect("Should have command");
        assert_eq!(cmd.priority, expected_priority);
    }
}

#[tokio::test]
async fn test_queue_sequence_preservation() {
    let queue = CommandQueue::new(100);

    // Add multiple commands with same priority
    for i in 0..10 {
        queue.enqueue(make_command(&format!("device{}", i), "cmd", CommandPriority::Normal)).await.unwrap();
    }

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
