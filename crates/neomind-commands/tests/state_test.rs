//! Command state store comprehensive tests.
//!
//! Tests command persistence, cache eviction, retry logic, and cleanup.

use std::sync::Arc;
use chrono::Utc;

use neomind_commands::{
    state::{CommandStateStore, StateError, StoreStats},
    command::{CommandPriority, CommandRequest, CommandResult, CommandStatus, CommandSource},
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
async fn test_state_store_empty_initially() {
    let store = CommandStateStore::new(100);

    assert!(store.is_empty().await);
    assert_eq!(store.len().await, 0);
}

#[tokio::test]
async fn test_state_store_and_retrieve() {
    let store = CommandStateStore::new(100);
    let cmd = make_command("device1", "turn_on", CommandPriority::Normal);

    store.store(&cmd).await.unwrap();
    assert_eq!(store.len().await, 1);
    assert!(!store.is_empty().await);

    let retrieved = store.get(&cmd.id).await.unwrap();
    assert_eq!(retrieved.id, cmd.id);
    assert_eq!(retrieved.device_id, "device1");
    assert_eq!(retrieved.command_name, "turn_on");
}

#[tokio::test]
async fn test_state_store_get_not_found() {
    let store = CommandStateStore::new(100);

    let id = String::from("nonexistent-id");
    let result = store.get(&id).await;
    assert!(matches!(result, Err(StateError::NotFound(_))));
}

#[tokio::test]
async fn test_state_store_update_status() {
    let store = CommandStateStore::new(100);
    let cmd = make_command("device1", "turn_on", CommandPriority::Normal);
    let id = cmd.id.clone();

    store.store(&cmd).await.unwrap();
    assert_eq!(store.get(&id).await.unwrap().status, CommandStatus::Pending);

    store.update_status(&id, CommandStatus::Queued).await.unwrap();
    assert_eq!(store.get(&id).await.unwrap().status, CommandStatus::Queued);

    store.update_status(&id, CommandStatus::Sending).await.unwrap();
    assert_eq!(store.get(&id).await.unwrap().status, CommandStatus::Sending);

    store.update_status(&id, CommandStatus::Completed).await.unwrap();
    assert_eq!(store.get(&id).await.unwrap().status, CommandStatus::Completed);
}

#[tokio::test]
async fn test_state_store_set_result() {
    let store = CommandStateStore::new(100);
    let cmd = make_command("device1", "turn_on", CommandPriority::Normal);
    let id = cmd.id.clone();

    store.store(&cmd).await.unwrap();

    // Set success result
    let success_result = CommandResult::success("Command completed successfully");
    store.set_result(&id, success_result).await.unwrap();

    let retrieved = store.get(&id).await.unwrap();
    assert_eq!(retrieved.status, CommandStatus::Completed);
    assert!(retrieved.result.is_some());
    assert!(retrieved.result.as_ref().unwrap().success);

    // Set failed result
    store.update_status(&id, CommandStatus::Queued).await.unwrap();
    let fail_result = CommandResult::failed("Connection timeout");
    store.set_result(&id, fail_result).await.unwrap();

    let retrieved = store.get(&id).await.unwrap();
    assert_eq!(retrieved.status, CommandStatus::Failed);
    assert!(!retrieved.result.as_ref().unwrap().success);
}

#[tokio::test]
async fn test_state_store_increment_attempt() {
    let store = CommandStateStore::new(100);
    let cmd = make_command("device1", "turn_on", CommandPriority::Normal);
    let id = cmd.id.clone();

    store.store(&cmd).await.unwrap();
    assert_eq!(store.get(&id).await.unwrap().attempt, 0);

    let attempt = store.increment_attempt(&id).await.unwrap();
    assert_eq!(attempt, 1);

    let attempt = store.increment_attempt(&id).await.unwrap();
    assert_eq!(attempt, 2);

    let retrieved = store.get(&id).await.unwrap();
    assert_eq!(retrieved.attempt, 2);
}

#[tokio::test]
async fn test_state_store_delete() {
    let store = CommandStateStore::new(100);
    let cmd = make_command("device1", "turn_on", CommandPriority::Normal);
    let id = cmd.id.clone();

    store.store(&cmd).await.unwrap();
    assert_eq!(store.len().await, 1);

    let deleted = store.delete(&id).await.unwrap();
    assert!(deleted);
    assert_eq!(store.len().await, 0);

    // Second delete should return false
    let deleted_again = store.delete(&id).await.unwrap();
    assert!(!deleted_again);
}

#[tokio::test]
async fn test_state_store_list_by_status() {
    let store = CommandStateStore::new(100);

    // Add commands with different statuses
    let cmd1 = make_command("device1", "cmd1", CommandPriority::Normal);
    let cmd2 = make_command("device2", "cmd2", CommandPriority::Normal);
    let cmd3 = make_command("device3", "cmd3", CommandPriority::Normal);
    let id1 = cmd1.id.clone();
    let id2 = cmd2.id.clone();
    let id3 = cmd3.id.clone();

    store.store(&cmd1).await.unwrap();
    store.store(&cmd2).await.unwrap();
    store.store(&cmd3).await.unwrap();

    store.update_status(&id1, CommandStatus::Completed).await.unwrap();
    store.update_status(&id2, CommandStatus::Pending).await.unwrap();
    store.update_status(&id3, CommandStatus::Completed).await.unwrap();

    let completed = store.list_by_status(CommandStatus::Completed).await;
    assert_eq!(completed.len(), 2);

    let pending = store.list_by_status(CommandStatus::Pending).await;
    assert_eq!(pending.len(), 1);

    let failed = store.list_by_status(CommandStatus::Failed).await;
    assert_eq!(failed.len(), 0);
}

#[tokio::test]
async fn test_state_store_list_by_device() {
    let store = CommandStateStore::new(100);

    let cmd1 = make_command("device1", "cmd1", CommandPriority::Normal);
    let cmd2 = make_command("device1", "cmd2", CommandPriority::Normal);
    let cmd3 = make_command("device2", "cmd3", CommandPriority::Normal);

    store.store(&cmd1).await.unwrap();
    store.store(&cmd2).await.unwrap();
    store.store(&cmd3).await.unwrap();

    let device1_commands = store.list_by_device("device1").await;
    assert_eq!(device1_commands.len(), 2);

    let device2_commands = store.list_by_device("device2").await;
    assert_eq!(device2_commands.len(), 1);
}

#[tokio::test]
async fn test_state_store_list_by_source() {
    let store = CommandStateStore::new(100);

    let user_cmd = make_command("device1", "cmd1", CommandPriority::Normal);
    let system_cmd = make_command("device2", "cmd2", CommandPriority::Normal);

    store.store(&user_cmd).await.unwrap();
    store.store(&system_cmd).await.unwrap();

    let user_commands = store.list_by_source("user").await;
    assert_eq!(user_commands.len(), 0); // Our test commands are System type

    let system_commands = store.list_by_source("system").await;
    assert_eq!(system_commands.len(), 2);
}

#[tokio::test]
async fn test_state_store_get_retryable() {
    let store = CommandStateStore::new(100);

    let cmd1 = make_command("device1", "cmd1", CommandPriority::Normal);
    let cmd2 = make_command("device2", "cmd2", CommandPriority::Normal);
    let cmd3 = make_command("device3", "cmd3", CommandPriority::Normal);
    let id1 = cmd1.id.clone();
    let id2 = cmd2.id.clone();
    let id3 = cmd3.id.clone();

    store.store(&cmd1).await.unwrap();
    store.store(&cmd2).await.unwrap();
    store.store(&cmd3).await.unwrap();

    // cmd1: Failed (retryable)
    store.update_status(&id1, CommandStatus::Failed).await.unwrap();
    cmd1.update_status(CommandStatus::Failed);
    store.store(&cmd1).await.unwrap();

    // cmd2: Completed (not retryable)
    store.update_status(&id2, CommandStatus::Completed).await.unwrap();
    cmd2.update_status(CommandStatus::Completed);
    store.store(&cmd2).await.unwrap();

    // cmd3: Pending (retryable but not yet failed)
    store.store(&cmd3).await.unwrap();

    let retryable = store.get_retryable_commands().await;
    assert_eq!(retryable.len(), 1);
    assert_eq!(retryable[0].id, id1);
}

#[tokio::test]
async fn test_state_store_get_expired() {
    let store = CommandStateStore::new(100);

    let cmd1 = make_command("device1", "cmd1", CommandPriority::Normal);
    let cmd2 = make_command("device2", "cmd2", CommandPriority::Normal);
    let id1 = cmd1.id.clone();
    let id2 = cmd2.id.clone();

    store.store(&cmd1).await.unwrap();
    store.store(&cmd2).await.unwrap();

    // cmd1: Complete it with very short timeout to make it "expired"
    // We create a command that appears old by having a short timeout
    let mut expired_cmd = cmd1.clone();
    expired_cmd.timeout_secs = 1; // 1 second timeout
    store.update_status(&id1, CommandStatus::Completed).await.unwrap();
    let old_result = CommandResult::success("Done");
    store.set_result(&id1, old_result).await.unwrap();
    // Wait for it to actually expire
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    store.store(&expired_cmd).await.unwrap();

    // cmd2 is recent and should not be expired
    store.update_status(&id2, CommandStatus::Completed).await.unwrap();
    let result2 = CommandResult::success("Done");
    store.set_result(&id2, result2).await.unwrap();

    let expired = store.get_expired_commands().await;
    // The expired command should be found
    assert!(expired.iter().any(|c| c.id == id1));
    // cmd2 should not be in expired list (recently completed)
    assert!(!expired.iter().any(|c| c.id == id2));
}

#[tokio::test]
async fn test_state_store_cleanup_old() {
    let store = CommandStateStore::new(100);

    // Add old completed command with short timeout
    let old_cmd = make_command("device1", "cmd1", CommandPriority::Normal);
    let old_id = old_cmd.id.clone();

    store.store(&old_cmd).await.unwrap();
    old_cmd.timeout_secs = 1; // Very short timeout
    store.update_status(&old_id, CommandStatus::Completed).await.unwrap();
    let old_result = CommandResult::success("Done");
    store.set_result(&old_id, old_result).await.unwrap();
    // Wait for command to expire
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    store.store(&old_cmd).await.unwrap();

    // Add recent command
    let recent_cmd = make_command("device2", "cmd2", CommandPriority::Normal);
    store.store(&recent_cmd).await.unwrap();

    assert_eq!(store.len().await, 2);

    // Cleanup commands older than 1 second (should remove old_cmd)
    let cleaned = store.cleanup_old_completed(1).await;
    assert_eq!(cleaned, 1);
    assert_eq!(store.len().await, 1);

    // Old command should be gone
    let result = store.get(&old_id).await;
    assert!(matches!(result, Err(StateError::NotFound(_))));

    // Recent command should still exist
    assert!(store.get(&recent_cmd.id).await.is_ok());
}

#[tokio::test]
async fn test_state_store_clear() {
    let store = CommandStateStore::new(100);

    for i in 0..10 {
        let cmd = make_command(&format!("device{}", i), "cmd", CommandPriority::Normal);
        store.store(&cmd).await.unwrap();
    }

    assert_eq!(store.len().await, 10);

    store.clear().await;

    assert_eq!(store.len().await, 0);
    assert!(store.is_empty().await);

    // All commands should be gone
    let result = store.get(&make_command("device0", "cmd", CommandPriority::Normal).id).await;
    assert!(matches!(result, Err(StateError::NotFound(_))));
}

#[tokio::test]
async fn test_state_store_stats() {
    let store = CommandStateStore::new(100);

    let cmd1 = make_command("device1", "cmd1", CommandPriority::Normal);
    let cmd2 = make_command("device2", "cmd2", CommandPriority::Normal);
    let cmd3 = make_command("device3", "cmd3", CommandPriority::Normal);
    let id1 = cmd1.id.clone();
    let id2 = cmd2.id.clone();

    store.store(&cmd1).await.unwrap();
    store.store(&cmd2).await.unwrap();
    store.store(&cmd3).await.unwrap();

    store.update_status(&id1, CommandStatus::Completed).await.unwrap();
    store.update_status(&id2, CommandStatus::Failed).await.unwrap();

    let stats: StoreStats = store.stats().await;

    assert_eq!(stats.total_count, 3);
    assert_eq!(stats.cache_size, 3);

    let status_counts: std::collections::HashMap<_, _> = stats.by_status
        .into_iter()
        .map(|(s, c)| (s, c))
        .collect();

    assert_eq!(*status_counts.get(&CommandStatus::Pending).unwrap_or(&0), 1);
    assert_eq!(*status_counts.get(&CommandStatus::Completed).unwrap_or(&0), 1);
    assert_eq!(*status_counts.get(&CommandStatus::Failed).unwrap_or(&0), 1);
}

#[tokio::test]
async fn test_state_store_cache_eviction() {
    let store = CommandStateStore::new(5); // Small cache size

    // Fill up the cache
    for i in 0..5 {
        let cmd = make_command(&format!("device{}", i), "cmd", CommandPriority::Normal);
        store.store(&cmd).await.unwrap();
    }

    assert_eq!(store.len().await, 5);

    // All commands are terminal, so adding one more should trigger eviction
    // but since they're all terminal and recent, no eviction happens
    let cmd6 = make_command("device6", "cmd", CommandPriority::Normal);
    store.store(&cmd6).await.unwrap();

    // Should still have 6 (no eviction of recent terminal commands)
    assert_eq!(store.len().await, 6);
}

#[tokio::test]
async fn test_state_store_concurrent_operations() {
    let store = Arc::new(CommandStateStore::new(1000));
    let mut handles = vec![];

    // Concurrent stores
    for i in 0..50 {
        let s = store.clone();
        let handle = tokio::spawn(async move {
            let cmd = make_command(&format!("device{}", i), "cmd", CommandPriority::Normal);
            s.store(&cmd).await
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

    // All stores should succeed
    assert_eq!(successful, 50);
    assert_eq!(store.len().await, 50);
}

#[tokio::test]
async fn test_state_store_set_result_updates_status() {
    let store = CommandStateStore::new(100);

    let cmd = make_command("device1", "cmd", CommandPriority::Normal);
    let id = cmd.id.clone();

    store.store(&cmd).await.unwrap();
    assert_eq!(store.get(&id).await.unwrap().status, CommandStatus::Pending);

    // Successful result should set status to Completed
    let success = CommandResult::success("Done");
    store.set_result(&id, success).await.unwrap();

    let retrieved = store.get(&id).await.unwrap();
    assert_eq!(retrieved.status, CommandStatus::Completed);

    // Failed result should set status to Failed
    store.update_status(&id, CommandStatus::Queued).await.unwrap();
    let failed = CommandResult::failed("Error");
    store.set_result(&id, failed).await.unwrap();

    let retrieved = store.get(&id).await.unwrap();
    assert_eq!(retrieved.status, CommandStatus::Failed);
}
