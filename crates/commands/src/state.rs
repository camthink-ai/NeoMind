//! Command state storage for persistence.
//!
//! Provides persistent storage of command state using the storage crate.

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::command::{CommandRequest, CommandId, CommandStatus, CommandResult, RetryPolicy, CommandSource, CommandPriority};
use crate::queue::{QueueError, CommandQueue};

/// Command state store error types.
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Command not found: {0}")]
    NotFound(CommandId),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Deserialization error: {0}")]
    Deserialization(String),
}

/// Command state store for persisting command state.
pub struct CommandStateStore {
    /// In-memory cache of commands
    cache: Arc<RwLock<dashmap::DashMap<CommandId, CommandRequest>>>,
    /// Maximum cache size
    max_cache_size: usize,
}

impl CommandStateStore {
    /// Create a new command state store.
    pub fn new(max_cache_size: usize) -> Self {
        Self {
            cache: Arc::new(RwLock::new(dashmap::DashMap::new())),
            max_cache_size,
        }
    }

    /// Store a command.
    pub async fn store(&self, command: &CommandRequest) -> Result<(), StateError> {
        let mut cache = self.cache.write().await;

        // Evict old entries if cache is full
        if cache.len() >= self.max_cache_size {
            self.evict_expired(&mut cache);
        }

        cache.insert(command.id.clone(), command.clone());
        Ok(())
    }

    /// Retrieve a command by ID.
    pub async fn get(&self, id: &CommandId) -> Result<CommandRequest, StateError> {
        let cache = self.cache.read().await;
        cache.get(id)
            .map(|c| c.clone())
            .ok_or_else(|| StateError::NotFound(id.clone()))
    }

    /// Update command status.
    pub async fn update_status(&self, id: &CommandId, status: CommandStatus) -> Result<(), StateError> {
        let mut cache = self.cache.write().await;
        let mut cmd = cache.get_mut(id)
            .ok_or_else(|| StateError::NotFound(id.clone()))?;

        cmd.update_status(status);
        Ok(())
    }

    /// Update command result.
    pub async fn set_result(&self, id: &CommandId, result: CommandResult) -> Result<(), StateError> {
        let mut cache = self.cache.write().await;
        let mut cmd = cache.get_mut(id)
            .ok_or_else(|| StateError::NotFound(id.clone()))?;

        cmd.set_result(result);
        Ok(())
    }

    /// Increment command attempt counter.
    pub async fn increment_attempt(&self, id: &CommandId) -> Result<u32, StateError> {
        let mut cache = self.cache.write().await;
        let mut cmd = cache.get_mut(id)
            .ok_or_else(|| StateError::NotFound(id.clone()))?;

        cmd.increment_attempt();
        Ok(cmd.attempt)
    }

    /// Delete a command.
    pub async fn delete(&self, id: &CommandId) -> Result<bool, StateError> {
        let mut cache = self.cache.write().await;
        Ok(cache.remove(id).is_some())
    }

    /// List commands by status.
    pub async fn list_by_status(&self, status: CommandStatus) -> Vec<CommandRequest> {
        let cache = self.cache.read().await;
        cache.iter()
            .filter(|c| c.status == status)
            .map(|c| c.clone())
            .collect()
    }

    /// List commands by device.
    pub async fn list_by_device(&self, device_id: &str) -> Vec<CommandRequest> {
        let cache = self.cache.read().await;
        cache.iter()
            .filter(|c| c.device_id == device_id)
            .map(|c| c.clone())
            .collect()
    }

    /// List commands by source.
    pub async fn list_by_source(&self, source_type: &str) -> Vec<CommandRequest> {
        let cache = self.cache.read().await;
        cache.iter()
            .filter(|c| c.source.type_name() == source_type)
            .map(|c| c.clone())
            .collect()
    }

    /// Get pending commands for retry.
    pub async fn get_retryable_commands(&self) -> Vec<CommandRequest> {
        let cache = self.cache.read().await;
        cache.iter()
            .filter(|c| c.can_retry() && !c.is_expired())
            .map(|c| c.clone())
            .collect()
    }

    /// Get expired commands.
    pub async fn get_expired_commands(&self) -> Vec<CommandRequest> {
        let cache = self.cache.read().await;
        cache.iter()
            .filter(|c| c.is_expired() && !c.status.is_terminal())
            .map(|c| c.clone())
            .collect()
    }

    /// Get store statistics.
    pub async fn stats(&self) -> StoreStats {
        let cache = self.cache.read().await;

        let mut by_status: Vec<(CommandStatus, usize)> = vec![
            (CommandStatus::Pending, 0),
            (CommandStatus::Queued, 0),
            (CommandStatus::Sending, 0),
            (CommandStatus::WaitingAck, 0),
            (CommandStatus::Completed, 0),
            (CommandStatus::Failed, 0),
            (CommandStatus::Cancelled, 0),
            (CommandStatus::Timeout, 0),
        ];

        for cmd in cache.iter() {
            let idx = match cmd.status {
                CommandStatus::Pending => 0,
                CommandStatus::Queued => 1,
                CommandStatus::Sending => 2,
                CommandStatus::WaitingAck => 3,
                CommandStatus::Completed => 4,
                CommandStatus::Failed => 5,
                CommandStatus::Cancelled => 6,
                CommandStatus::Timeout => 7,
            };
            by_status[idx].1 += 1;
        }

        StoreStats {
            total_count: cache.len(),
            by_status,
            cache_size: cache.len(),
        }
    }

    /// Clear all completed commands older than specified seconds.
    pub async fn cleanup_old_completed(&self, older_than_secs: i64) -> usize {
        let cutoff = chrono::Utc::now() - chrono::Duration::seconds(older_than_secs);

        let cache = self.cache.write().await;
        let initial_len = cache.len();

        cache.retain(|_, cmd| {
            if cmd.status.is_terminal() {
                if let Some(result) = &cmd.result {
                    result.completed_at > cutoff
                } else {
                    true
                }
            } else {
                true
            }
        });

        initial_len - cache.len()
    }

    /// Evict expired commands from cache.
    fn evict_expired(&self, cache: &mut dashmap::DashMap<CommandId, CommandRequest>) {
        let cutoff = chrono::Utc::now() - chrono::Duration::hours(24);

        cache.retain(|_, cmd| {
            if cmd.status.is_terminal() {
                if let Some(result) = &cmd.result {
                    result.completed_at > cutoff
                } else {
                    true
                }
            } else {
                true
            }
        });
    }

    /// Get cache size.
    pub async fn len(&self) -> usize {
        self.cache.read().await.len()
    }

    /// Check if cache is empty.
    pub async fn is_empty(&self) -> bool {
        self.cache.read().await.is_empty()
    }

    /// Clear all commands from cache.
    pub async fn clear(&self) {
        self.cache.write().await.clear();
    }
}

impl Default for CommandStateStore {
    fn default() -> Self {
        Self::new(10000)
    }
}

/// Store statistics.
#[derive(Debug, Clone)]
pub struct StoreStats {
    /// Total number of commands
    pub total_count: usize,
    /// Count by status
    pub by_status: Vec<(CommandStatus, usize)>,
    /// Current cache size
    pub cache_size: usize,
}

impl Default for StoreStats {
    fn default() -> Self {
        Self {
            total_count: 0,
            by_status: vec![
                (CommandStatus::Pending, 0),
                (CommandStatus::Queued, 0),
                (CommandStatus::Sending, 0),
                (CommandStatus::WaitingAck, 0),
                (CommandStatus::Completed, 0),
                (CommandStatus::Failed, 0),
                (CommandStatus::Cancelled, 0),
                (CommandStatus::Timeout, 0),
            ],
            cache_size: 0,
        }
    }
}

/// Command manager for coordinating queue and state.
pub struct CommandManager {
    /// Command queue
    pub queue: Arc<CommandQueue>,
    /// State store
    pub state: Arc<CommandStateStore>,
}

impl CommandManager {
    /// Create a new command manager.
    pub fn new(queue: Arc<CommandQueue>, state: Arc<CommandStateStore>) -> Self {
        Self { queue, state }
    }

    /// Submit a new command.
    pub async fn submit(&self, mut command: CommandRequest) -> Result<CommandId, QueueError> {
        // Update status to queued
        command.update_status(crate::command::CommandStatus::Queued);

        // Store the command
        self.state.store(&command).await
            .map_err(|e| QueueError::Failed(e.to_string()))?;

        // Enqueue the command
        let id = command.id.clone();
        self.queue.enqueue(command).await?;

        Ok(id)
    }

    /// Get command status.
    pub async fn get_status(&self, id: &CommandId) -> Result<CommandStatus, StateError> {
        let cmd = self.state.get(id).await?;
        Ok(cmd.status)
    }

    /// Get command result.
    pub async fn get_result(&self, id: &CommandId) -> Result<Option<CommandResult>, StateError> {
        let cmd = self.state.get(id).await?;
        Ok(cmd.result)
    }

    /// Cancel a command.
    pub async fn cancel(&self, id: &CommandId) -> Result<(), StateError> {
        self.state.update_status(id, crate::command::CommandStatus::Cancelled).await
    }

    /// Retry a failed command.
    pub async fn retry(&self, id: &CommandId) -> Result<(), QueueError> {
        let mut cmd = self.state.get(id).await
            .map_err(|e| QueueError::Failed(e.to_string()))?;

        if !cmd.can_retry() {
            return Err(QueueError::Failed("Command cannot be retried".to_string()));
        }

        // Reset for retry
        cmd.increment_attempt();
        cmd.update_status(crate::command::CommandStatus::Queued);

        // Store updated command
        self.state.store(&cmd).await
            .map_err(|e| QueueError::Failed(e.to_string()))?;

        // Re-enqueue
        self.queue.enqueue(cmd).await
    }

    /// Get queue statistics.
    pub async fn queue_stats(&self) -> crate::queue::QueueStats {
        self.queue.stats().await
    }

    /// Get state statistics.
    pub async fn state_stats(&self) -> StoreStats {
        self.state.stats().await
    }

    /// List commands by device.
    pub async fn list_device_commands(&self, device_id: &str) -> Vec<CommandRequest> {
        self.state.list_by_device(device_id).await
    }

    /// Get pending retryable commands.
    pub async fn get_retryable(&self) -> Vec<CommandRequest> {
        self.state.get_retryable_commands().await
    }

    /// Clean up old completed commands.
    pub async fn cleanup(&self, older_than_secs: i64) -> usize {
        self.state.cleanup_old_completed(older_than_secs).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::CommandSource;

    #[tokio::test]
    async fn test_state_store() {
        let store = CommandStateStore::new(100);

        let source = CommandSource::System {
            reason: "test".to_string(),
        };
        let cmd = CommandRequest::new("device1".to_string(), "test".to_string(), source);

        store.store(&cmd).await.unwrap();
        assert_eq!(store.len().await, 1);

        let retrieved = store.get(&cmd.id).await.unwrap();
        assert_eq!(retrieved.id, cmd.id);
        assert_eq!(retrieved.device_id, "device1");
    }

    #[tokio::test]
    async fn test_state_store_update_status() {
        let store = CommandStateStore::new(100);

        let source = CommandSource::System {
            reason: "test".to_string(),
        };
        let cmd = CommandRequest::new("device1".to_string(), "test".to_string(), source);
        let id = cmd.id.clone();

        store.store(&cmd).await.unwrap();
        store.update_status(&id, CommandStatus::Completed).await.unwrap();

        let retrieved = store.get(&id).await.unwrap();
        assert_eq!(retrieved.status, CommandStatus::Completed);
    }

    #[tokio::test]
    async fn test_state_store_list_by_status() {
        let store = CommandStateStore::new(100);

        let source = CommandSource::System {
            reason: "test".to_string(),
        };

        let cmd1 = CommandRequest::new("device1".to_string(), "test1".to_string(), source.clone());
        let cmd2 = CommandRequest::new("device1".to_string(), "test2".to_string(), source);

        store.store(&cmd1).await.unwrap();
        store.store(&cmd2).await.unwrap();

        store.update_status(&cmd1.id, CommandStatus::Completed).await.unwrap();

        let pending = store.list_by_status(CommandStatus::Pending).await;
        assert_eq!(pending.len(), 1);

        let completed = store.list_by_status(CommandStatus::Completed).await;
        assert_eq!(completed.len(), 1);
    }

    #[tokio::test]
    async fn test_state_store_delete() {
        let store = CommandStateStore::new(100);

        let source = CommandSource::System {
            reason: "test".to_string(),
        };
        let cmd = CommandRequest::new("device1".to_string(), "test".to_string(), source);
        let id = cmd.id.clone();

        store.store(&cmd).await.unwrap();
        assert_eq!(store.len().await, 1);

        let deleted = store.delete(&id).await.unwrap();
        assert!(deleted);
        assert_eq!(store.len().await, 0);

        let deleted_again = store.delete(&id).await.unwrap();
        assert!(!deleted_again);
    }

    #[tokio::test]
    async fn test_state_store_not_found() {
        let store = CommandStateStore::new(100);

        let result = store.get(&"nonexistent".to_string()).await;
        assert!(matches!(result, Err(StateError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_command_manager_submit() {
        let queue = Arc::new(CommandQueue::new(100));
        let state = Arc::new(CommandStateStore::new(100));
        let manager = CommandManager::new(queue, state);

        let source = CommandSource::System {
            reason: "test".to_string(),
        };
        let cmd = CommandRequest::new("device1".to_string(), "test".to_string(), source);

        let id = manager.submit(cmd).await.unwrap();
        assert!(!id.is_empty());

        let status = manager.get_status(&id).await.unwrap();
        assert_eq!(status, CommandStatus::Queued);
    }
}
