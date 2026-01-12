//! Session storage using redb.
//!
//! Provides persistent storage for chat sessions and message history.

use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

use crate::Error;

// Session table: key = session_id, value = timestamp
const SESSIONS_TABLE: TableDefinition<&str, i64> =
    TableDefinition::new("sessions");

// History table: key = (session_id, message_index), value = Message (serialized)
const HISTORY_TABLE: TableDefinition<(&str, u64), &[u8]> =
    TableDefinition::new("history");

/// A message in a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    /// Message role (user, assistant, system, tool).
    pub role: String,
    /// Message content.
    pub content: String,
    /// Tool calls made by the assistant.
    pub tool_calls: Option<Vec<serde_json::Value>>,
    /// Tool call ID for tool responses.
    pub tool_call_id: Option<String>,
    /// Thinking/reasoning content.
    pub thinking: Option<String>,
    /// Message timestamp.
    pub timestamp: i64,
}

impl SessionMessage {
    /// Create a new user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            thinking: None,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Create a new assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            thinking: None,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Create a new system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            thinking: None,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Create a new tool message.
    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            thinking: None,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Add tool calls to an assistant message.
    pub fn with_tool_calls(mut self, tool_calls: Vec<serde_json::Value>) -> Self {
        self.tool_calls = Some(tool_calls);
        self
    }

    /// Add thinking content.
    pub fn with_thinking(mut self, thinking: impl Into<String>) -> Self {
        self.thinking = Some(thinking.into());
        self
    }

    /// Set timestamp.
    pub fn with_timestamp(mut self, timestamp: i64) -> Self {
        self.timestamp = timestamp;
        self
    }
}

/// Session storage using redb.
pub struct SessionStore {
    db: Arc<Database>,
    path: String,
}

/// Global session store singleton (thread-safe).
static SESSION_STORE_SINGLETON: StdMutex<Option<Arc<SessionStore>>> = StdMutex::new(None);

impl SessionStore {
    /// Open or create a session store at the given path.
    /// Uses a singleton pattern to prevent multiple opens of the same database.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Arc<Self>, Error> {
        let path_str = path.as_ref().to_string_lossy().to_string();

        // Check if we already have a store for this path
        {
            let singleton = SESSION_STORE_SINGLETON.lock().unwrap();
            if let Some(store) = singleton.as_ref() {
                if store.path == path_str {
                    return Ok(store.clone());
                }
            }
        }

        // Create new store and save to singleton
        let path_ref = path.as_ref();
        let db = if path_ref.exists() {
            Database::open(path_ref)?
        } else {
            Database::create(path_ref)?
        };

        let store = Arc::new(SessionStore {
            db: Arc::new(db),
            path: path_str,
        });

        *SESSION_STORE_SINGLETON.lock().unwrap() = Some(store.clone());
        Ok(store)
    }

    /// Save a session ID.
    pub fn save_session_id(&self, session_id: &str) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(SESSIONS_TABLE)?;
            let timestamp = chrono::Utc::now().timestamp();
            table.insert(session_id, timestamp)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Save message history for a session.
    pub fn save_history(
        &self,
        session_id: &str,
        messages: &[SessionMessage],
    ) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(HISTORY_TABLE)?;

            // Delete old records for this session
            let start_key = (session_id, 0u64);
            let end_key = (session_id, u64::MAX);

            // Collect keys as owned tuples
            let mut keys_to_delete: Vec<(String, u64)> = Vec::new();
            let mut range = table.range(start_key..=end_key)?;
            while let Some(result) = range.next() {
                let (key_ref, _val_ref) = result?;
                let sid: &str = key_ref.value().0;
                let idx: u64 = key_ref.value().1;
                keys_to_delete.push((sid.to_string(), idx));
            }
            drop(range);

            for key in &keys_to_delete {
                table.remove((key.0.as_str(), key.1))?;
            }

            // Insert new messages
            for (index, message) in messages.iter().enumerate() {
                let key = (session_id, index as u64);
                let value = bincode::serialize(message)?;
                table.insert(key, value.as_slice())?;
            }
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Load message history for a session.
    pub fn load_history(&self, session_id: &str) -> Result<Vec<SessionMessage>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(HISTORY_TABLE)?;

        let start_key = (session_id, 0u64);
        let end_key = (session_id, u64::MAX);

        let mut messages = Vec::new();
        for result in table.range(start_key..=end_key)? {
            let (_key, value) = result?;
            let message: SessionMessage = bincode::deserialize(value.value())?;
            messages.push(message);
        }

        Ok(messages)
    }

    /// Delete a session.
    pub fn delete_session(&self, session_id: &str) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;

        // Delete from sessions table
        {
            let mut sessions_table = write_txn.open_table(SESSIONS_TABLE)?;
            sessions_table.remove(session_id)?;
        }

        // Delete from history table - we need to collect the actual key tuples
        {
            let mut history_table = write_txn.open_table(HISTORY_TABLE)?;
            let start_key = (session_id, 0u64);
            let end_key = (session_id, u64::MAX);

            let mut keys_to_delete: Vec<(String, u64)> = Vec::new();
            let mut range = history_table.range(start_key..=end_key)?;
            while let Some(result) = range.next() {
                let (key_ref, _val_ref) = result?;
                let sid: &str = key_ref.value().0;
                let idx: u64 = key_ref.value().1;
                keys_to_delete.push((sid.to_string(), idx));
            }
            drop(range);

            for key in &keys_to_delete {
                history_table.remove((key.0.as_str(), key.1))?;
            }
        }

        write_txn.commit()?;
        Ok(())
    }

    /// List all session IDs.
    pub fn list_sessions(&self) -> Result<Vec<String>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SESSIONS_TABLE)?;

        let mut sessions = Vec::new();
        for result in table.iter()? {
            let key = result?.0;
            sessions.push(key.value().to_string());
        }

        Ok(sessions)
    }

    /// Check if a session exists.
    pub fn session_exists(&self, session_id: &str) -> Result<bool, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SESSIONS_TABLE)?;
        Ok(table.get(session_id)?.is_some())
    }

    /// Get session timestamp.
    pub fn get_session_timestamp(&self, session_id: &str) -> Result<Option<i64>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SESSIONS_TABLE)?;
        Ok(table.get(session_id)?.map(|v| v.value()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a temporary SessionStore for tests
    fn create_temp_store() -> Arc<SessionStore> {
        let temp_dir = std::env::temp_dir().join(format!("session_test_{}.redb", uuid::Uuid::new_v4()));
        SessionStore::open(temp_dir).unwrap()
    }

    #[test]
    fn test_session_store() {
        let store = create_temp_store();

        // Save session
        store.save_session_id("test-session").unwrap();

        // Check exists
        assert!(store.session_exists("test-session").unwrap());
        assert!(!store.session_exists("non-existent").unwrap());

        // Save messages
        let messages = vec![
            SessionMessage::user("Hello"),
            SessionMessage::assistant("Hi there!"),
        ];
        store.save_history("test-session", &messages).unwrap();

        // Load messages
        let loaded = store.load_history("test-session").unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].role, "user");
        assert_eq!(loaded[0].content, "Hello");
        assert_eq!(loaded[1].role, "assistant");
        assert_eq!(loaded[1].content, "Hi there!");

        // List sessions
        let sessions = store.list_sessions().unwrap();
        assert!(sessions.contains(&"test-session".to_string()));

        // Delete session
        store.delete_session("test-session").unwrap();
        assert!(!store.session_exists("test-session").unwrap());
    }

    #[test]
    fn test_session_message_builder() {
        let msg = SessionMessage::user("test")
            .with_timestamp(12345);
        assert_eq!(msg.content, "test");
        assert_eq!(msg.timestamp, 12345);

        let msg = SessionMessage::assistant("response")
            .with_thinking("Let me think...");
        assert_eq!(msg.thinking, Some("Let me think...".to_string()));

        let tool_calls = vec![serde_json::json!({"name": "test"})];
        let msg = SessionMessage::assistant("I'll use a tool")
            .with_tool_calls(tool_calls);
        assert!(msg.tool_calls.is_some());
    }
}
