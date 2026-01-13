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

// Session metadata table: key = session_id, value = JSON metadata (title, etc.)
const SESSIONS_META_TABLE: TableDefinition<&str, Vec<u8>> =
    TableDefinition::new("sessions_meta");

// History table: key = (session_id, message_index), value = Message (serialized)
const HISTORY_TABLE: TableDefinition<(&str, u64), Vec<u8>> =
    TableDefinition::new("history");

/// Session metadata (title, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// User-defined title for the session
    pub title: Option<String>,
}

impl Default for SessionMetadata {
    fn default() -> Self {
        Self { title: None }
    }
}

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
    /// Tool call name for tracking which tool was called.
    pub tool_call_name: Option<String>,
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
            tool_call_name: None,
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
            tool_call_name: None,
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
            tool_call_name: None,
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
            tool_call_name: None,
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
        eprintln!("[DEBUG SessionStore::open] Opening session store at: {}", path_str);

        // Check if we already have a store for this path
        {
            let singleton = SESSION_STORE_SINGLETON.lock().unwrap();
            if let Some(store) = singleton.as_ref() {
                if store.path == path_str {
                    eprintln!("[DEBUG SessionStore::open] Returning cached store for: {}", path_str);
                    return Ok(store.clone());
                }
            }
        }

        // Create new store and save to singleton
        let path_ref = path.as_ref();
        eprintln!("[DEBUG SessionStore::open] Path exists: {}, is_file: {}", path_ref.exists(), path_ref.is_file());
        let db = if path_ref.exists() {
            eprintln!("[DEBUG SessionStore::open] Opening existing database");
            Database::open(path_ref)?
        } else {
            eprintln!("[DEBUG SessionStore::open] Creating new database");
            Database::create(path_ref)?
        };

        let store = Arc::new(SessionStore {
            db: Arc::new(db),
            path: path_str,
        });

        *SESSION_STORE_SINGLETON.lock().unwrap() = Some(store.clone());
        eprintln!("[DEBUG SessionStore::open] Session store created/loaded successfully");
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
                table.insert(key, value)?;
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
            let value_vec = value.value(); // This is now &Vec<u8>
            let message: SessionMessage = bincode::deserialize(value_vec.as_slice())?;
            messages.push(message);
        }

        Ok(messages)
    }

    /// Delete a session.
    pub fn delete_session(&self, session_id: &str) -> Result<(), Error> {
        eprintln!("[DEBUG SessionStore] delete_session called for: {}", session_id);
        let write_txn = self.db.begin_write()?;
        eprintln!("[DEBUG SessionStore] write transaction started");

        // Delete from sessions table
        {
            let mut sessions_table = write_txn.open_table(SESSIONS_TABLE)?;
            eprintln!("[DEBUG SessionStore] removing from SESSIONS_TABLE");
            sessions_table.remove(session_id)?;
            eprintln!("[DEBUG SessionStore] removed from SESSIONS_TABLE");
        }

        // Delete from metadata table
        {
            let mut meta_table = write_txn.open_table(SESSIONS_META_TABLE)?;
            let _ = meta_table.remove(session_id); // Ignore error if not exists
            eprintln!("[DEBUG SessionStore] removed from SESSIONS_META_TABLE");
        }

        // Delete from history table - we need to collect the actual key tuples
        {
            eprintln!("[DEBUG SessionStore] opening HISTORY_TABLE");
            let mut history_table = write_txn.open_table(HISTORY_TABLE)?;
            let start_key = (session_id, 0u64);
            let end_key = (session_id, u64::MAX);

            let mut keys_to_delete: Vec<(String, u64)> = Vec::new();
            eprintln!("[DEBUG SessionStore] collecting history keys to delete");
            let mut range = history_table.range(start_key..=end_key)?;
            while let Some(result) = range.next() {
                let (key_ref, _val_ref) = result?;
                let sid: &str = key_ref.value().0;
                let idx: u64 = key_ref.value().1;
                keys_to_delete.push((sid.to_string(), idx));
            }
            drop(range);
            eprintln!("[DEBUG SessionStore] found {} history keys to delete", keys_to_delete.len());

            for key in &keys_to_delete {
                history_table.remove((key.0.as_str(), key.1))?;
            }
            eprintln!("[DEBUG SessionStore] removed history entries");
        }

        eprintln!("[DEBUG SessionStore] committing transaction");
        write_txn.commit()?;
        eprintln!("[DEBUG SessionStore] delete_session complete");
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

    /// Save session metadata (title, etc.).
    pub fn save_session_metadata(&self, session_id: &str, metadata: &SessionMetadata) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(SESSIONS_META_TABLE)?;
            let value = bincode::serialize(metadata)?;
            table.insert(session_id, value)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Get session metadata.
    pub fn get_session_metadata(&self, session_id: &str) -> Result<SessionMetadata, Error> {
        let read_txn = self.db.begin_read()?;

        // Table might not exist in older databases, handle gracefully
        let table = match read_txn.open_table(SESSIONS_META_TABLE) {
            Ok(t) => t,
            Err(_) => return Ok(SessionMetadata::default()),
        };

        match table.get(session_id)? {
            Some(value) => {
                let metadata: SessionMetadata = bincode::deserialize(value.value().as_slice())?;
                Ok(metadata)
            }
            None => Ok(SessionMetadata::default()),
        }
    }

    /// Delete session metadata.
    pub fn delete_session_metadata(&self, session_id: &str) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(SESSIONS_META_TABLE)?;
            table.remove(session_id)?;
        }
        write_txn.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a temporary SessionStore for tests
    fn create_temp_store() -> Arc<SessionStore> {
        let temp_dir = std::env::temp_dir().join(format!("session_test_{}", uuid::Uuid::new_v4()));
        // Remove existing directory if it exists
        let _ = std::fs::remove_dir_all(&temp_dir);
        // Create the directory
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("sessions.redb");
        SessionStore::open(&db_path).unwrap()
    }

    #[test]
    fn test_session_store() {
        let store = create_temp_store();

        // Save session
        store.save_session_id("test-session").unwrap();

        // Check exists
        assert!(store.session_exists("test-session").unwrap());
        assert!(!store.session_exists("non-existent").unwrap());

        // Save messages - without tool_calls to test basic functionality
        let msg1 = SessionMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
            tool_calls: None,
            tool_call_id: None,
            tool_call_name: None,
            thinking: None,
            timestamp: chrono::Utc::now().timestamp(),
        };
        let msg2 = SessionMessage {
            role: "assistant".to_string(),
            content: "Hi there!".to_string(),
            tool_calls: None,
            tool_call_id: None,
            tool_call_name: None,
            thinking: None,
            timestamp: chrono::Utc::now().timestamp(),
        };
        let messages = vec![msg1, msg2];

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
    fn test_session_metadata() {
        let store = create_temp_store();

        // Save session
        store.save_session_id("test-session").unwrap();

        // Initially no metadata - table might not exist, which is fine
        let meta = store.get_session_metadata("test-session").unwrap_or_else(|_| SessionMetadata::default());
        assert!(meta.title.is_none());

        // Set title
        store.save_session_metadata("test-session", &SessionMetadata {
            title: Some("My Chat Session".to_string()),
        }).unwrap();

        // Get title
        let meta = store.get_session_metadata("test-session").unwrap();
        assert_eq!(meta.title, Some("My Chat Session".to_string()));

        // Update with different title
        store.save_session_metadata("test-session", &SessionMetadata {
            title: Some("New Title".to_string()),
        }).unwrap();
        let meta = store.get_session_metadata("test-session").unwrap();
        assert_eq!(meta.title, Some("New Title".to_string()));

        // Clear title
        store.save_session_metadata("test-session", &SessionMetadata {
            title: None,
        }).unwrap();
        let meta = store.get_session_metadata("test-session").unwrap();
        assert!(meta.title.is_none());
    }

    #[test]
    fn test_session_message_serialization() {
        let msg = SessionMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
            tool_calls: None,
            tool_call_id: None,
            tool_call_name: None,
            thinking: None,
            timestamp: 12345,
        };

        let serialized = bincode::serialize(&msg).unwrap();
        let deserialized: SessionMessage = bincode::deserialize(&serialized).unwrap();
        assert_eq!(deserialized.role, "user");
        assert_eq!(deserialized.content, "Hello");
        assert_eq!(deserialized.timestamp, 12345);
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
