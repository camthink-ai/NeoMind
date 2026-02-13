//! Message/Notification storage using redb.
//!
//! Provides persistent storage for messages and notifications.

use std::path::Path;
use std::sync::Arc;

use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use serde_json;

use crate::Error;

// Messages table: key = message_id, value = Message (serialized as JSON)
const MESSAGES_TABLE: TableDefinition<&str, &str> = TableDefinition::new("messages");

// Message history table: key = timestamp_id, value = Message (serialized)
// Keeps historical record of all messages even after deletion
const HISTORY_TABLE: TableDefinition<&str, &str> = TableDefinition::new("messages_history");

// Active messages index: key = message_id, value = "1" if active
const ACTIVE_TABLE: TableDefinition<&str, &str> = TableDefinition::new("messages_active");

/// Stored message representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMessage {
    /// Unique message ID
    pub id: String,
    /// Message category (alert, system, business)
    pub category: String,
    /// Message severity (info, warning, critical, emergency)
    pub severity: String,
    /// Message title
    pub title: String,
    /// Message content
    pub message: String,
    /// Source of the message
    pub source: String,
    /// Source type (device, rule, agent, etc.)
    pub source_type: Option<String>,
    /// Message status (active, acknowledged, resolved, archived)
    pub status: String,
    /// Tags for categorization
    pub tags: Option<Vec<String>>,
    /// Additional metadata
    pub metadata: Option<serde_json::Value>,
    /// Creation timestamp
    pub timestamp: i64,
    /// When was it acknowledged (if applicable)
    pub acknowledged_at: Option<i64>,
    /// When was it resolved (if applicable)
    pub resolved_at: Option<i64>,
    /// Who acknowledged it
    pub acknowledged_by: Option<String>,
}

impl StoredMessage {
    /// Create a new stored message.
    pub fn new(
        id: String,
        category: String,
        severity: String,
        title: String,
        message: String,
        source: String,
    ) -> Self {
        Self {
            id,
            category,
            severity,
            title,
            message,
            source,
            source_type: None,
            status: "active".to_string(),
            tags: None,
            metadata: None,
            timestamp: chrono::Utc::now().timestamp(),
            acknowledged_at: None,
            resolved_at: None,
            acknowledged_by: None,
        }
    }

    /// Check if the message is active.
    pub fn is_active(&self) -> bool {
        self.status == "active"
    }

    /// Mark as acknowledged.
    pub fn acknowledge(&mut self, by: Option<String>) {
        self.status = "acknowledged".to_string();
        self.acknowledged_at = Some(chrono::Utc::now().timestamp());
        self.acknowledged_by = by;
    }

    /// Mark as resolved.
    pub fn resolve(&mut self) {
        self.status = "resolved".to_string();
        self.resolved_at = Some(chrono::Utc::now().timestamp());
    }

    /// Mark as archived.
    pub fn archive(&mut self) {
        self.status = "archived".to_string();
    }
}

/// Message store for persistent storage.
pub struct MessageStore {
    db: Arc<Database>,
}

impl MessageStore {
    /// Open a message store at the given path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let db = Database::create(path)
            .map_err(|e| Error::Storage(format!("Failed to open message database: {}", e)))?;

        // Create tables
        let write_txn = db
            .begin_write()
            .map_err(|e| Error::Storage(format!("Failed to begin write: {}", e)))?;

        {
            write_txn
                .open_table(MESSAGES_TABLE)
                .map_err(|e| Error::Storage(format!("Failed to open messages table: {}", e)))?;
            write_txn
                .open_table(HISTORY_TABLE)
                .map_err(|e| Error::Storage(format!("Failed to open history table: {}", e)))?;
            write_txn
                .open_table(ACTIVE_TABLE)
                .map_err(|e| Error::Storage(format!("Failed to open active table: {}", e)))?;
        }

        write_txn
            .commit()
            .map_err(|e| Error::Storage(format!("Failed to commit: {}", e)))?;

        Ok(Self { db: Arc::new(db) })
    }

    /// Create an in-memory message store for testing.
    pub fn memory() -> Result<Self, Error> {
        // redb doesn't have true in-memory mode, so use a temp file
        let temp_dir = std::env::temp_dir();
        let db_path = temp_dir.join(format!("messages_test_{}.redb", std::process::id()));
        let db = Database::create(&db_path)
            .map_err(|e| Error::Storage(format!("Failed to create test database: {}", e)))?;

        // Create tables
        let write_txn = db
            .begin_write()
            .map_err(|e| Error::Storage(format!("Failed to begin write: {}", e)))?;

        {
            write_txn
                .open_table(MESSAGES_TABLE)
                .map_err(|e| Error::Storage(format!("Failed to open messages table: {}", e)))?;
            write_txn
                .open_table(HISTORY_TABLE)
                .map_err(|e| Error::Storage(format!("Failed to open history table: {}", e)))?;
            write_txn
                .open_table(ACTIVE_TABLE)
                .map_err(|e| Error::Storage(format!("Failed to open active table: {}", e)))?;
        }

        write_txn
            .commit()
            .map_err(|e| Error::Storage(format!("Failed to commit: {}", e)))?;

        Ok(Self { db: Arc::new(db) })
    }

    /// Insert a message.
    pub fn insert(&self, msg: &StoredMessage) -> Result<(), Error> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| Error::Storage(format!("Failed to begin write: {}", e)))?;

        let json = serde_json::to_string(msg)
            .map_err(|e| Error::Storage(format!("Failed to serialize message: {}", e)))?;

        {
            let mut messages_table = write_txn
                .open_table(MESSAGES_TABLE)
                .map_err(|e| Error::Storage(format!("Failed to open messages table: {}", e)))?;
            messages_table
                .insert(msg.id.as_str(), json.as_str())
                .map_err(|e| Error::Storage(format!("Failed to insert message: {}", e)))?;

            // Add to active index if active
            if msg.is_active() {
                let mut active_table = write_txn
                    .open_table(ACTIVE_TABLE)
                    .map_err(|e| Error::Storage(format!("Failed to open active table: {}", e)))?;
                active_table.insert(msg.id.as_str(), "1").map_err(|e| {
                    Error::Storage(format!("Failed to index active message: {}", e))
                })?;
            }

            // Add to history
            let history_key = format!("{}_{}", msg.timestamp, msg.id);
            let mut history_table = write_txn
                .open_table(HISTORY_TABLE)
                .map_err(|e| Error::Storage(format!("Failed to open history table: {}", e)))?;
            history_table
                .insert(history_key.as_str(), json.as_str())
                .map_err(|e| Error::Storage(format!("Failed to add to history: {}", e)))?;
        }

        write_txn
            .commit()
            .map_err(|e| Error::Storage(format!("Failed to commit: {}", e)))?;

        Ok(())
    }

    /// Get a message by ID.
    pub fn get(&self, id: &str) -> Result<Option<StoredMessage>, Error> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| Error::Storage(format!("Failed to begin read: {}", e)))?;

        let messages_table = read_txn
            .open_table(MESSAGES_TABLE)
            .map_err(|e| Error::Storage(format!("Failed to open messages table: {}", e)))?;

        match messages_table.get(id) {
            Ok(Some(value)) => {
                let json = value.value();
                let msg: StoredMessage = serde_json::from_str(json)
                    .map_err(|e| Error::Storage(format!("Failed to deserialize message: {}", e)))?;
                Ok(Some(msg))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(Error::Storage(format!("Failed to read message: {}", e))),
        }
    }

    /// Update a message.
    pub fn update(&self, msg: &StoredMessage) -> Result<(), Error> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| Error::Storage(format!("Failed to begin write: {}", e)))?;

        let json = serde_json::to_string(msg)
            .map_err(|e| Error::Storage(format!("Failed to serialize message: {}", e)))?;

        // Check if it was active before
        let was_active = {
            let messages_table = write_txn
                .open_table(MESSAGES_TABLE)
                .map_err(|e| Error::Storage(format!("Failed to open messages table: {}", e)))?;

            let result = messages_table
                .get(msg.id.as_str())
                .map_err(|e| Error::Storage(format!("Failed to read message: {}", e)))?;
            match result {
                Some(v) => serde_json::from_str::<StoredMessage>(v.value())
                    .ok()
                    .map(|m| m.is_active())
                    .unwrap_or(false),
                None => false,
            }
        };

        // Update the message
        {
            let mut messages_table = write_txn
                .open_table(MESSAGES_TABLE)
                .map_err(|e| Error::Storage(format!("Failed to open messages table: {}", e)))?;

            messages_table
                .insert(msg.id.as_str(), json.as_str())
                .map_err(|e| Error::Storage(format!("Failed to update message: {}", e)))?;
        }

        // Update active index
        {
            let mut active_table = write_txn
                .open_table(ACTIVE_TABLE)
                .map_err(|e| Error::Storage(format!("Failed to open active table: {}", e)))?;

            if msg.is_active() {
                active_table.insert(msg.id.as_str(), "1").map_err(|e| {
                    Error::Storage(format!("Failed to index active message: {}", e))
                })?;
            } else if was_active {
                active_table
                    .remove(msg.id.as_str())
                    .map_err(|e| Error::Storage(format!("Failed to update active index: {}", e)))?;
            }
        }

        write_txn
            .commit()
            .map_err(|e| Error::Storage(format!("Failed to commit: {}", e)))?;

        Ok(())
    }

    /// Delete a message.
    pub fn delete(&self, id: &str) -> Result<bool, Error> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| Error::Storage(format!("Failed to begin write: {}", e)))?;

        let existed = {
            let mut messages_table = write_txn
                .open_table(MESSAGES_TABLE)
                .map_err(|e| Error::Storage(format!("Failed to open messages table: {}", e)))?;

            let result = messages_table
                .remove(id)
                .map_err(|e| Error::Storage(format!("Failed to remove message: {}", e)))?
                .is_some();
            result
        };

        if existed {
            let mut active_table = write_txn
                .open_table(ACTIVE_TABLE)
                .map_err(|e| Error::Storage(format!("Failed to open active table: {}", e)))?;
            active_table
                .remove(id)
                .map_err(|e| Error::Storage(format!("Failed to update active index: {}", e)))?;
        }

        write_txn
            .commit()
            .map_err(|e| Error::Storage(format!("Failed to commit: {}", e)))?;

        Ok(existed)
    }

    /// List all messages.
    pub fn list(&self) -> Result<Vec<StoredMessage>, Error> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| Error::Storage(format!("Failed to begin read: {}", e)))?;

        let messages_table = read_txn
            .open_table(MESSAGES_TABLE)
            .map_err(|e| Error::Storage(format!("Failed to open messages table: {}", e)))?;

        let mut messages = Vec::new();
        let iter = messages_table
            .iter()
            .map_err(|e| Error::Storage(format!("Failed to iterate: {}", e)))?;
        for result in iter {
            let (_id, value) =
                result.map_err(|e| Error::Storage(format!("Failed to read entry: {}", e)))?;
            let msg: StoredMessage = serde_json::from_str(value.value())
                .map_err(|e| Error::Storage(format!("Failed to deserialize: {}", e)))?;
            messages.push(msg);
        }

        // Sort by timestamp descending
        messages.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        Ok(messages)
    }

    /// List active messages only.
    pub fn list_active(&self) -> Result<Vec<StoredMessage>, Error> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| Error::Storage(format!("Failed to begin read: {}", e)))?;

        let active_table = read_txn
            .open_table(ACTIVE_TABLE)
            .map_err(|e| Error::Storage(format!("Failed to open active table: {}", e)))?;

        let messages_table = read_txn
            .open_table(MESSAGES_TABLE)
            .map_err(|e| Error::Storage(format!("Failed to open messages table: {}", e)))?;

        let mut messages = Vec::new();
        let iter = active_table
            .iter()
            .map_err(|e| Error::Storage(format!("Failed to iterate: {}", e)))?;
        for result in iter {
            let (id, _) =
                result.map_err(|e| Error::Storage(format!("Failed to read entry: {}", e)))?;
            if let Some(value) = messages_table
                .get(id.value())
                .map_err(|e| Error::Storage(format!("Failed to read message: {}", e)))?
            {
                let msg: StoredMessage = serde_json::from_str(value.value())
                    .map_err(|e| Error::Storage(format!("Failed to deserialize: {}", e)))?;
                messages.push(msg);
            }
        }

        // Sort by timestamp descending
        messages.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        Ok(messages)
    }

    /// List messages by status.
    pub fn list_by_status(&self, status: &str) -> Result<Vec<StoredMessage>, Error> {
        let all = self.list()?;
        Ok(all.into_iter().filter(|m| m.status == status).collect())
    }

    /// List messages by category.
    pub fn list_by_category(&self, category: &str) -> Result<Vec<StoredMessage>, Error> {
        let all = self.list()?;
        Ok(all.into_iter().filter(|m| m.category == category).collect())
    }

    /// List messages by severity.
    pub fn list_by_severity(&self, severity: &str) -> Result<Vec<StoredMessage>, Error> {
        let all = self.list()?;
        Ok(all.into_iter().filter(|m| m.severity == severity).collect())
    }

    /// Get message statistics.
    pub fn get_stats(&self) -> Result<MessageStats, Error> {
        let all = self.list()?;
        let active = self.list_active()?;

        let mut by_category = std::collections::HashMap::new();
        let mut by_severity = std::collections::HashMap::new();
        let mut by_status = std::collections::HashMap::new();

        for msg in &all {
            *by_category.entry(msg.category.clone()).or_insert(0) += 1;
            *by_severity.entry(msg.severity.clone()).or_insert(0) += 1;
            *by_status.entry(msg.status.clone()).or_insert(0) += 1;
        }

        Ok(MessageStats {
            total: all.len(),
            active: active.len(),
            by_category,
            by_severity,
            by_status,
        })
    }

    /// Cleanup old messages.
    pub fn cleanup_old(&self, older_than_days: i64) -> Result<usize, Error> {
        let cutoff = chrono::Utc::now().timestamp() - (older_than_days * 86400);

        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| Error::Storage(format!("Failed to begin write: {}", e)))?;

        let to_remove = {
            let messages_table = write_txn
                .open_table(MESSAGES_TABLE)
                .map_err(|e| Error::Storage(format!("Failed to open messages table: {}", e)))?;

            let mut to_remove = Vec::new();
            let iter = messages_table
                .iter()
                .map_err(|e| Error::Storage(format!("Failed to iterate: {}", e)))?;
            for result in iter {
                let (id, value) =
                    result.map_err(|e| Error::Storage(format!("Failed to read entry: {}", e)))?;
                let msg: StoredMessage = serde_json::from_str(value.value())
                    .map_err(|e| Error::Storage(format!("Failed to deserialize: {}", e)))?;
                if msg.timestamp < cutoff {
                    to_remove.push(id.value().to_string());
                }
            }
            to_remove
        };

        // Delete messages
        {
            let mut messages_table = write_txn
                .open_table(MESSAGES_TABLE)
                .map_err(|e| Error::Storage(format!("Failed to open messages table: {}", e)))?;

            for id in &to_remove {
                messages_table
                    .remove(id.as_str())
                    .map_err(|e| Error::Storage(format!("Failed to remove message: {}", e)))?;
            }
        }

        // Delete from active index
        {
            let mut active_table = write_txn
                .open_table(ACTIVE_TABLE)
                .map_err(|e| Error::Storage(format!("Failed to open active table: {}", e)))?;

            for id in &to_remove {
                active_table.remove(id.as_str()).ok();
            }
        }

        write_txn
            .commit()
            .map_err(|e| Error::Storage(format!("Failed to commit: {}", e)))?;

        Ok(to_remove.len())
    }

    /// Clear all messages.
    pub fn clear(&self) -> Result<(), Error> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| Error::Storage(format!("Failed to begin write: {}", e)))?;

        // Collect all keys to delete first
        let mut keys_to_delete = Vec::new();
        {
            let messages_table = write_txn
                .open_table(MESSAGES_TABLE)
                .map_err(|e| Error::Storage(format!("Failed to open messages table: {}", e)))?;
            let iter = messages_table
                .iter()
                .map_err(|e| Error::Storage(format!("Failed to iterate: {}", e)))?;
            for (id, _) in iter.flatten() {
                keys_to_delete.push(id.value().to_string());
            }
        }

        // Collect active keys
        let mut active_keys = Vec::new();
        {
            let active_table = write_txn
                .open_table(ACTIVE_TABLE)
                .map_err(|e| Error::Storage(format!("Failed to open active table: {}", e)))?;
            let iter = active_table
                .iter()
                .map_err(|e| Error::Storage(format!("Failed to iterate: {}", e)))?;
            for (id, _) in iter.flatten() {
                active_keys.push(id.value().to_string());
            }
        }

        // Delete all messages
        {
            let mut messages_table = write_txn
                .open_table(MESSAGES_TABLE)
                .map_err(|e| Error::Storage(format!("Failed to open messages table: {}", e)))?;
            for key in &keys_to_delete {
                messages_table.remove(key.as_str()).ok();
            }
        }

        // Clear active index
        {
            let mut active_table = write_txn
                .open_table(ACTIVE_TABLE)
                .map_err(|e| Error::Storage(format!("Failed to open active table: {}", e)))?;
            for key in &active_keys {
                active_table.remove(key.as_str()).ok();
            }
        }

        write_txn
            .commit()
            .map_err(|e| Error::Storage(format!("Failed to commit: {}", e)))?;

        Ok(())
    }
}

/// Message statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageStats {
    pub total: usize,
    pub active: usize,
    pub by_category: std::collections::HashMap<String, usize>,
    pub by_severity: std::collections::HashMap<String, usize>,
    pub by_status: std::collections::HashMap<String, usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stored_message_creation() {
        let msg = StoredMessage::new(
            "test-1".to_string(),
            "alert".to_string(),
            "warning".to_string(),
            "Test Alert".to_string(),
            "This is a test".to_string(),
            "sensor1".to_string(),
        );

        assert_eq!(msg.id, "test-1");
        assert_eq!(msg.category, "alert");
        assert!(msg.is_active());
    }

    #[test]
    fn test_stored_message_acknowledge() {
        let mut msg = StoredMessage::new(
            "test-1".to_string(),
            "alert".to_string(),
            "warning".to_string(),
            "Test Alert".to_string(),
            "This is a test".to_string(),
            "sensor1".to_string(),
        );

        msg.acknowledge(Some("user1".to_string()));
        assert_eq!(msg.status, "acknowledged");
        assert!(msg.acknowledged_at.is_some());
        assert_eq!(msg.acknowledged_by, Some("user1".to_string()));
        assert!(!msg.is_active());
    }

    #[test]
    fn test_stored_message_resolve() {
        let mut msg = StoredMessage::new(
            "test-1".to_string(),
            "alert".to_string(),
            "warning".to_string(),
            "Test Alert".to_string(),
            "This is a test".to_string(),
            "sensor1".to_string(),
        );

        msg.resolve();
        assert_eq!(msg.status, "resolved");
        assert!(msg.resolved_at.is_some());
        assert!(!msg.is_active());
    }

    #[test]
    fn test_stored_message_archive() {
        let mut msg = StoredMessage::new(
            "test-1".to_string(),
            "alert".to_string(),
            "warning".to_string(),
            "Test Alert".to_string(),
            "This is a test".to_string(),
            "sensor1".to_string(),
        );

        msg.archive();
        assert_eq!(msg.status, "archived");
        assert!(!msg.is_active());
    }
}
