//! Command state store comprehensive tests.
//!
//! Tests command persistence, cache eviction, retry logic, and cleanup.

use std::sync::Arc;

use neomind_commands::{
    command::{CommandPriority, CommandRequest, CommandResult, CommandSource, CommandStatus},
    state::{CommandStateStore, StateError, StoreStats},
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

    store
        .update_status(&id, CommandStatus::Queued)
        .await
        .unwrap();
    assert_eq!(store.get(&id).await.unwrap().status, CommandStatus::Queued);

    store
        .update_status(&id, CommandStatus::Sending)
        .await
        .unwrap();
    assert_eq!(store.get(&id).await.unwrap().status, CommandStatus::Sending);

    store
        .update_status(&id, CommandStatus::Completed)
        .await
        .unwrap();
    assert_eq!(
        store.get(&id).await.unwrap().status,
        CommandStatus::Completed
    );
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
    store
        .update_status(&id, CommandStatus::Queued)
        .await
        .unwrap();
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
    let mut cmd1 = make_command("device1", "cmd1", CommandPriority::Normal);
    let mut cmd2 = make_command("device2", "cmd2", CommandPriority::Normal);
    let cmd3 = make_command("device3", "cmd3", CommandPriority::Normal);
    let id1 = cmd1.id.clone();
    let id2 = cmd2.id.clone();
    let id3 = cmd3.id.clone();

    store.store(&cmd1).await.unwrap();
    store.store(&cmd2).await.unwrap();
    store.store(&cmd3).await.unwrap();

    store
        .update_status(&id1, CommandStatus::Completed)
        .await
        .unwrap();
    store
        .update_status(&id2, CommandStatus::Pending)
        .await
        .unwrap();
    store
        .update_status(&id3, CommandStatus::Completed)
        .await
        .unwrap();

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

    let mut cmd1 = make_command("device1", "cmd1", CommandPriority::Normal);
    let mut cmd2 = make_command("device2", "cmd2", CommandPriority::Normal);
    let cmd3 = make_command("device3", "cmd3", CommandPriority::Normal);
    let id1 = cmd1.id.clone();
    let id2 = cmd2.id.clone();
    let id3 = cmd3.id.clone();

    store.store(&cmd1).await.unwrap();
    store.store(&cmd2).await.unwrap();
    store.store(&cmd3).await.unwrap();

    // cmd1: Failed (retryable)
    store
        .update_status(&id1, CommandStatus::Failed)
        .await
        .unwrap();
    cmd1.update_status(CommandStatus::Failed);
    store.store(&cmd1).await.unwrap();

    // cmd2: Completed (not retryable)
    store
        .update_status(&id2, CommandStatus::Completed)
        .await
        .unwrap();
    cmd2.update_status(CommandStatus::Completed);
    store.store(&cmd2).await.unwrap();

    // cmd3: Pending (retryable but not yet failed)
    store.store(&cmd3).await.unwrap();

    let retryable = store.get_retryable_commands().await;
    assert_eq!(retryable.len(), 1);
    assert_eq!(retryable[0].id, id1);
}

#[tokio::test]
async fn test_state_store_cleanup_old() {
    let store = CommandStateStore::new(100);

    // Add old completed command with short timeout
    let mut old_cmd = make_command("device1", "cmd1", CommandPriority::Normal);
    let old_id = old_cmd.id.clone();

    store.store(&old_cmd).await.unwrap();
    old_cmd.timeout_secs = 1; // Very short timeout
    store
        .update_status(&old_id, CommandStatus::Completed)
        .await
        .unwrap();
    let old_result = CommandResult::success("Done");
    store.set_result(&old_id, old_result).await.unwrap();

    // Wait for command to expire (completed_at is set to now(), need >67 seconds for cleanup with 2s buffer)
    tokio::time::sleep(tokio::time::Duration::from_secs(68)).await;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Add recent command (triggers cache update for old_cmd)
    let recent_cmd = make_command("device2", "cmd2", CommandPriority::Normal);
    store.store(&recent_cmd).await.unwrap();

    assert_eq!(store.len().await, 2);

    // Cleanup commands older than 2 seconds (should remove old_cmd)
    let cleaned = store.cleanup_old_completed(2).await;
    assert_eq!(cleaned, 1, "Expected 1 old command to be cleaned up");
    assert_eq!(store.len().await, 1, "Expected 1 command remaining");

    // Old command should be gone
    let result = store.get(&old_id).await;
    assert!(
        matches!(result, Err(StateError::NotFound(_))),
        "Expected old command to be removed"
    );

    // Recent command should still exist
    assert!(
        store.get(&recent_cmd.id).await.is_ok(),
        "Expected recent command to still exist"
    );
}
