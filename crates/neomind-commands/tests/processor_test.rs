//! Command processor tests.
//!
//! Tests command processing lifecycle including start, stop, and running state.

use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use neomind_commands::{
    processor::{CommandProcessor, ProcessorConfig},
    queue::CommandQueue,
    adapter::DownlinkAdapterRegistry,
    command::{CommandPriority, CommandRequest, CommandStatus, CommandSource},
};

/// Helper to create a test processor.
fn create_test_processor(queue_size: usize) -> CommandProcessor {
    let queue = Arc::new(CommandQueue::new(queue_size));
    let adapters = Arc::new(tokio::sync::RwLock::new(DownlinkAdapterRegistry::new()));
    let config = ProcessorConfig::default();

    CommandProcessor::new(queue, adapters, config)
}

#[tokio::test]
async fn test_processor_creation() {
    let processor = create_test_processor(100);

    // Should not be running initially
    assert!(!processor.is_running().await);
}

#[tokio::test]
async fn test_processor_config_default() {
    let config = ProcessorConfig::default();

    assert_eq!(config.poll_interval_ms, 100);
    assert_eq!(config.max_concurrent, 10);
    assert_eq!(config.default_timeout_secs, 30);
}

#[tokio::test]
async fn test_processor_start() {
    let processor = create_test_processor(100);

    assert!(!processor.is_running().await);

    processor.start().await;
    // Give it a moment to start
    sleep(Duration::from_millis(50)).await;

    // Should now be running
    assert!(processor.is_running().await);
}

#[tokio::test]
async fn test_processor_stop() {
    let processor = create_test_processor(100);

    processor.start().await;
    sleep(Duration::from_millis(50)).await;
    assert!(processor.is_running().await);

    processor.stop().await;
    // Give it a moment to stop
    sleep(Duration::from_millis(100)).await;

    // Should no longer be running
    assert!(!processor.is_running().await);
}

#[tokio::test]
async fn test_processor_start_idempotent() {
    let processor = create_test_processor(100);

    // Start multiple times
    processor.start().await;
    sleep(Duration::from_millis(50)).await;
    processor.start().await;
    sleep(Duration::from_millis(50)).await;

    assert!(processor.is_running().await);

    // Stop should work normally
    processor.stop().await;
    sleep(Duration::from_millis(100)).await;
    assert!(!processor.is_running().await);
}

#[tokio::test]
async fn test_processor_stop_idempotent() {
    let processor = create_test_processor(100);

    // Stop without starting should not panic
    processor.stop().await;
    assert!(!processor.is_running().await);

    // Second stop should also be safe
    processor.stop().await;
    assert!(!processor.is_running().await);
}

#[tokio::test]
async fn test_processor_stop_while_stopped() {
    let processor = create_test_processor(100);

    // Stop when not running should be safe
    processor.stop().await;
    sleep(Duration::from_millis(50)).await;

    assert!(!processor.is_running().await);

    // Now start and stop again
    processor.start().await;
    sleep(Duration::from_millis(50)).await;
    assert!(processor.is_running().await);

    processor.stop().await;
    sleep(Duration::from_millis(100)).await;
    assert!(!processor.is_running().await);
}

#[tokio::test]
async fn test_processor_processes_commands() {
    let queue = Arc::new(CommandQueue::new(100));
    let adapters = Arc::new(tokio::sync::RwLock::new(DownlinkAdapterRegistry::new()));
    let config = ProcessorConfig {
        poll_interval_ms: 10, // Fast poll for testing
        max_concurrent: 10,
        default_timeout_secs: 30,
    };

    let processor = CommandProcessor::new(queue.clone(), adapters, config);

    // Enqueue a command before starting
    let source = CommandSource::System {
        reason: "test".to_string(),
    };
    let cmd = CommandRequest::new("device1".to_string(), "turn_on".to_string(), source);
    queue.enqueue(cmd).await.unwrap();

    processor.start().await;

    // Wait for processor to pick up the command
    sleep(Duration::from_millis(50)).await;

    // Command should have been processed (removed from queue)
    let remaining = queue.len().await;
    assert!(remaining < 1, "Command should have been processed");

    processor.stop().await;
}

#[tokio::test]
async fn test_processor_custom_config() {
    let queue = Arc::new(CommandQueue::new(100));
    let adapters = Arc::new(tokio::sync::RwLock::new(DownlinkAdapterRegistry::new()));

    let custom_config = ProcessorConfig {
        poll_interval_ms: 500,
        max_concurrent: 5,
        default_timeout_secs: 60,
    };

    let processor = CommandProcessor::new(queue, adapters, custom_config);

    processor.start().await;
    sleep(Duration::from_millis(50)).await;

    assert!(processor.is_running().await);

    processor.stop().await;
}

#[tokio::test]
async fn test_processor_concurrent_start() {
    let processor = create_test_processor(100);

    // Try starting from multiple tasks
    let handle1 = tokio::spawn(async move {
        processor.start().await
    });

    sleep(Duration::from_millis(10)).await;

    let handle2 = tokio::spawn(async move {
        processor.start().await
    });

    // Both should complete without panicking
    let (r1, r2) = tokio::join!(handle1, handle2).await;
    assert!(r1.is_ok());
    assert!(r2.is_ok());

    assert!(processor.is_running().await);

    processor.stop().await;
}
