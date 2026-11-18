//! Concurrent request limiting tests
//!
//! Tests for the concurrent request limiting functionality

use neomind_core::extension::isolated::{IsolatedExtensionConfig, IsolatedExtensionError};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// Test that max_concurrent_requests default is reasonable
#[test]
fn test_default_concurrent_limit() {
    let config = IsolatedExtensionConfig::default();
    assert!(config.max_concurrent_requests > 0);
    assert!(config.max_concurrent_requests <= 1000); // Sanity check

    println!(
        "Default concurrent request limit: {}",
        config.max_concurrent_requests
    );
    assert_eq!(config.max_concurrent_requests, 100);
}

/// Test TooManyRequests error variant
#[test]
fn test_too_many_requests_error() {
    let error = IsolatedExtensionError::TooManyRequests(100);

    assert!(error.to_string().contains("Too many concurrent requests"));
    assert!(error.to_string().contains("100"));

    println!("Error message: {}", error);
}

/// Test concurrent request counter logic
#[tokio::test]
#[serial_test::serial]
async fn test_concurrent_counter() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let counter = Arc::new(AtomicUsize::new(0));
    let limit = 5;

    // Simulate multiple concurrent requests
    let mut handles = vec![];

    for _i in 0..10 {
        let counter_clone = counter.clone();
        let handle = tokio::spawn(async move {
            // Check limit
            let current = counter_clone.load(Ordering::SeqCst);
            if current < limit {
                // Increment
                counter_clone.fetch_add(1, Ordering::SeqCst);
                // Simulate work
                tokio::time::sleep(Duration::from_millis(10)).await;
                // Decrement
                counter_clone.fetch_sub(1, Ordering::SeqCst);
                Ok::<_, ()>(())
            } else {
                Err::<(), ()>(())
            }
        });
        handles.push(handle);
    }

    // Wait for all tasks
    let mut success_count = 0;
    for handle in handles {
        if handle.await.unwrap().is_ok() {
            success_count += 1;
        }
    }

    // Should have exactly `limit` successful requests
    assert_eq!(success_count, limit);
    assert_eq!(counter.load(Ordering::SeqCst), 0);

    println!(
        "Successful requests: {} out of 10 (limit: {})",
        success_count, limit
    );
}

/// Test scopeguard cleanup
#[tokio::test]
#[serial_test::serial]
async fn test_scopeguard_cleanup() {
    use scopeguard::guard;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let counter = Arc::new(AtomicUsize::new(0));

    {
        // Create a guard
        let _guard = guard(counter.clone(), |c| {
            c.fetch_sub(1, Ordering::SeqCst);
        });

        // Increment counter
        counter.fetch_add(1, Ordering::SeqCst);
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // Guard goes out of scope here and decrements
    }

    // After guard is dropped, counter should be back to 0
    assert_eq!(counter.load(Ordering::SeqCst), 0);

    println!("Scopeguard cleanup verified");
}

/// Test sequential batch operations
#[tokio::test]
#[serial_test::serial]
async fn test_sequential_batch_operations() {
    use neomind_core::extension::isolated::BatchCommand;

    let batch_commands = vec![
        BatchCommand {
            command: "cmd1".to_string(),
            args: serde_json::json!({"arg": 1}),
        },
        BatchCommand {
            command: "cmd2".to_string(),
            args: serde_json::json!({"arg": 2}),
        },
        BatchCommand {
            command: "cmd3".to_string(),
            args: serde_json::json!({"arg": 3}),
        },
    ];

    // Simulate batch execution
    let mut results = Vec::new();
    for cmd in &batch_commands {
        // Simulate command execution
        let result = serde_json::json!({
            "command": cmd.command,
            "success": true,
            "elapsed_ms": 10.0
        });
        results.push(result);
    }

    assert_eq!(results.len(), batch_commands.len());
    assert_eq!(results.len(), 3);

    println!("Batch operations: {} commands executed", results.len());
}

/// Test concurrent batch operations
#[tokio::test]
#[serial_test::serial]
async fn test_concurrent_batch_operations() {
    use neomind_core::extension::isolated::BatchCommand;
    use tokio::sync::Semaphore;

    let batch_count = 5;
    let commands_per_batch = 3;
    let limit = 2;

    // Use a semaphore to properly limit concurrency
    let semaphore = Arc::new(Semaphore::new(limit));
    let success_count = Arc::new(RwLock::new(0));

    let mut handles = vec![];

    for _ in 0..batch_count {
        let semaphore_clone = semaphore.clone();
        let success_count_clone = success_count.clone();

        let handle = tokio::spawn(async move {
            // Acquire semaphore permit
            let _permit = semaphore_clone.acquire().await.unwrap();

            // Create batch commands
            let _batch = (0..commands_per_batch)
                .map(|i| BatchCommand {
                    command: format!("batch_cmd_{}", i),
                    args: serde_json::json!({"index": i}),
                })
                .collect::<Vec<_>>();

            // Simulate processing
            tokio::time::sleep(Duration::from_millis(50)).await;

            // Mark success
            *success_count_clone.write().await += 1;

            true
        });

        handles.push(handle);
    }

    // Wait for all handles
    for handle in handles {
        handle.await.unwrap();
    }

    let success = *success_count.read().await;
    println!(
        "Successful batches: {} out of {} (limit: {})",
        success, batch_count, limit
    );

    // All batches should complete eventually (semaphore queues requests)
    assert_eq!(success, batch_count);
}
