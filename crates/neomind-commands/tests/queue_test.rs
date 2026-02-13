//! Command queue comprehensive tests.
//!
//! Tests priority-based command queuing with concurrency and edge cases.

use std::sync::Arc;

use neomind_commands::{
    command::{CommandPriority, CommandRequest, CommandSource},
    queue::{CommandQueue, QueueError, QueueStats},
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
            let cmd = make_command(&format!("device{}", i), "cmd", CommandPriority::Normal);
            q.enqueue(cmd).await
        });
        handles.push(handle);
    }

    // Wait for all to complete
    for handle in handles {
        handle.await.unwrap();
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
    let priority_map: std::collections::HashMap<_, _> = stats.by_priority.into_iter().collect();

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
