//! Session storage using redb.
//!
//! Provides persistent storage for chat sessions and message history.
//!
//! NOTE: Uses JSON serialization instead of Bincode for better schema compatibility.
//! JSON is more forgiving when fields are added/removed from structs over time.

use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use serde_json;

use crate::Error;

// Session table: key = session_id, value = timestamp
const SESSIONS_TABLE: TableDefinition<&str, i64> = TableDefinition::new("sessions");

// Session metadata table: key = session_id, value = JSON metadata (title, etc.)
const SESSIONS_META_TABLE: TableDefinition<&str, Vec<u8>> = TableDefinition::new("sessions_meta");

// History table: key = (session_id, message_index), value = Message (serialized)
const HISTORY_TABLE: TableDefinition<(&str, u64), Vec<u8>> = TableDefinition::new("history");

// Pending stream states: key = session_id, value = PendingStreamState (serialized)
// P0.3: Track in-progress streaming responses for recovery after disconnection
const PENDING_STREAM_TABLE: TableDefinition<&str, Vec<u8>> = TableDefinition::new("pending_streams");

/// Session metadata (title, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct SessionMetadata {
    /// User-defined title for the session
    pub title: Option<String>,
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
    /// Images attached to the message (base64 data URLs).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<SessionMessageImage>>,
    /// Message timestamp.
    pub timestamp: i64,
}

/// An image attached to a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessageImage {
    /// Base64 data URL (e.g., "data:image/png;base64,...").
    pub data: String,
    /// MIME type (e.g., "image/png").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
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
            images: None,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Create a new user message with images.
    pub fn user_with_images(content: impl Into<String>, images: Vec<SessionMessageImage>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            tool_call_name: None,
            thinking: None,
            images: Some(images),
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
            images: None,
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
            images: None,
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
            images: None,
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

/// P0.3: Pending stream state for tracking in-progress streaming responses.
///
/// This is used to recover from disconnections or page refreshes during
/// long-running LLM responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingStreamState {
    /// Session ID
    pub session_id: String,
    /// User message that triggered the stream
    pub user_message: String,
    /// Accumulated content so far
    pub content: String,
    /// Accumulated thinking content so far
    pub thinking: String,
    /// Current processing stage
    pub stage: StreamStage,
    /// When the stream started
    pub started_at: i64,
    /// Last update timestamp
    pub updated_at: i64,
    /// Tool calls detected so far (if any)
    pub tool_calls: Option<Vec<serde_json::Value>>,
    /// Whether the stream was intentionally interrupted
    pub interrupted: bool,
}

/// Current stage of stream processing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub enum StreamStage {
    /// Initial stage - waiting for response
    #[serde(rename = "waiting")]
    #[default]
    Waiting,
    /// Model is thinking/reasoning
    #[serde(rename = "thinking")]
    Thinking,
    /// Generating actual response content
    #[serde(rename = "generating")]
    Generating,
    /// Executing tools
    #[serde(rename = "tool_execution")]
    ToolExecution,
    /// Stream complete
    #[serde(rename = "complete")]
    Complete,
}


impl PendingStreamState {
    /// Create a new pending stream state.
    pub fn new(session_id: String, user_message: String) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            session_id,
            user_message,
            content: String::new(),
            thinking: String::new(),
            stage: StreamStage::Waiting,
            started_at: now,
            updated_at: now,
            tool_calls: None,
            interrupted: false,
        }
    }

    /// Update the content and timestamp.
    pub fn update_content(&mut self, content: impl Into<String>) {
        self.content = content.into();
        self.updated_at = chrono::Utc::now().timestamp();
    }

    /// Update the thinking content and timestamp.
    pub fn update_thinking(&mut self, thinking: impl Into<String>) {
        self.thinking = thinking.into();
        self.stage = StreamStage::Thinking;
        self.updated_at = chrono::Utc::now().timestamp();
    }

    /// Update the processing stage.
    pub fn set_stage(&mut self, stage: StreamStage) {
        self.stage = stage;
        self.updated_at = chrono::Utc::now().timestamp();
    }

    /// Add tool calls.
    pub fn set_tool_calls(&mut self, tool_calls: Vec<serde_json::Value>) {
        self.tool_calls = Some(tool_calls);
        self.stage = StreamStage::ToolExecution;
        self.updated_at = chrono::Utc::now().timestamp();
    }

    /// Mark as interrupted.
    pub fn mark_interrupted(&mut self) {
        self.interrupted = true;
        self.updated_at = chrono::Utc::now().timestamp();
    }

    /// Check if the state is stale (older than 10 minutes).
    pub fn is_stale(&self) -> bool {
        let now = chrono::Utc::now().timestamp();
        now - self.updated_at > 600  // 10 minutes
    }

    /// Get elapsed time in seconds.
    pub fn elapsed_secs(&self) -> i64 {
        chrono::Utc::now().timestamp() - self.started_at
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
        tracing::debug!(
            "[SessionStore::open] Opening session store at: {}",
            path_str
        );

        // Check if we already have a store for this path
        {
            let Ok(singleton) = SESSION_STORE_SINGLETON.lock() else {
                return Err(Error::Storage("Failed to acquire session store lock".to_string()));
            };
            if let Some(store) = singleton.as_ref()
                && store.path == path_str {
                    tracing::debug!(
                        "[SessionStore::open] Returning cached store for: {}",
                        path_str
                    );
                    return Ok(store.clone());
                }
        }

        // Create new store and save to singleton
        let path_ref = path.as_ref();
        tracing::debug!(
            "[SessionStore::open] Path exists: {}, is_file: {}",
            path_ref.exists(),
            path_ref.is_file()
        );
        let db = if path_ref.exists() {
            tracing::debug!("[SessionStore::open] Opening existing database");
            Database::open(path_ref)?
        } else {
            tracing::debug!("[SessionStore::open] Creating new database");
            Database::create(path_ref)?
        };

        let store = Arc::new(SessionStore {
            db: Arc::new(db),
            path: path_str,
        });

        {
            let Ok(mut singleton) = SESSION_STORE_SINGLETON.lock() else {
                return Err(Error::Storage("Failed to acquire session store lock".to_string()));
            };
            *singleton = Some(store.clone());
        }
        tracing::debug!("[SessionStore::open] Session store created/loaded successfully");
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
    /// NOTE: If messages is empty and history exists, this will NOT clear the history.
    /// This prevents accidental data loss when called with stale/incomplete data.
    pub fn save_history(&self, session_id: &str, messages: &[SessionMessage]) -> Result<(), Error> {
        // If messages is empty, don't overwrite existing history
        // This prevents accidental data loss
        if messages.is_empty() {
            // Check if there's existing history
            let read_txn = self.db.begin_read()?;
            let table = read_txn.open_table(HISTORY_TABLE);
            if let Ok(t) = table {
                let start_key = (session_id, 0u64);
                let end_key = (session_id, u64::MAX);
                let range = t.range(start_key..=end_key);
                if let Ok(mut r) = range
                    && r.next().is_some() {
                        // Existing history found, don't clear it
                        tracing::debug!(
                            "[save_history] Refusing to clear existing history for session {}",
                            session_id
                        );
                        return Ok(());
                    }
            }
        }

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(HISTORY_TABLE)?;

            // If messages is empty, don't delete existing data
            // This prevents accidental data loss
            if messages.is_empty() {
                tracing::warn!("[save_history] Warning: Attempting to save empty message list for session {}, skipping to avoid data loss", session_id);
                return Ok(());
            }

            // Delete old records for this session
            let start_key = (session_id, 0u64);
            let end_key = (session_id, u64::MAX);

            // Collect keys as owned tuples
            let mut keys_to_delete: Vec<(String, u64)> = Vec::new();
            let mut range = table.range(start_key..=end_key)?;
            for result in range.by_ref() {
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
                let value = serde_json::to_vec(message)?;
                table.insert(key, value)?;
            }

            tracing::debug!("[save_history] Saved {} messages for session {}", messages.len(), session_id);
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Clear message history for a session.
    /// This is the ONLY method that should be used to intentionally clear history.
    pub fn clear_history(&self, session_id: &str) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(HISTORY_TABLE)?;

            let start_key = (session_id, 0u64);
            let end_key = (session_id, u64::MAX);

            // Collect keys to delete
            let mut keys_to_delete: Vec<(String, u64)> = Vec::new();
            let mut range = table.range(start_key..=end_key)?;
            for result in range.by_ref() {
                let (key_ref, _val_ref) = result?;
                let sid: &str = key_ref.value().0;
                let idx: u64 = key_ref.value().1;
                keys_to_delete.push((sid.to_string(), idx));
            }
            drop(range);

            for key in &keys_to_delete {
                table.remove((key.0.as_str(), key.1))?;
            }
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Load message history for a session.
    /// Skips corrupted messages and logs warnings instead of failing completely.
    pub fn load_history(&self, session_id: &str) -> Result<Vec<SessionMessage>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(HISTORY_TABLE)?;

        let start_key = (session_id, 0u64);
        let end_key = (session_id, u64::MAX);

        let mut messages = Vec::new();
        let mut skipped_count = 0;
        for result in table.range(start_key..=end_key)? {
            let (_key, value) = result?;
            let value_vec = value.value(); // This is now &Vec<u8>

            // Handle deserialization errors gracefully
            match serde_json::from_slice::<SessionMessage>(value_vec.as_slice()) {
                Ok(message) => messages.push(message),
                Err(e) => {
                    skipped_count += 1;
                    // Only log the first few errors to avoid spam
                    if skipped_count <= 3 {
                        tracing::warn!(
                            session_id = %session_id,
                            error = %e,
                            "Failed to deserialize session message (schema mismatch or corruption), skipping"
                        );
                    }
                }
            }
        }

        if skipped_count > 0 {
            tracing::info!(
                session_id = %session_id,
                total = messages.len() + skipped_count,
                skipped = skipped_count,
                loaded = messages.len(),
                "Session history loaded with some skipped messages due to corruption"
            );
        }

        Ok(messages)
    }

    /// Append a single message to session history (incremental save).
    /// This is more efficient than save_history for adding new messages.
    pub fn append_message(&self, session_id: &str, message: &SessionMessage) -> Result<u64, Error> {
        let write_txn = self.db.begin_write()?;
        let index = {
            let mut table = write_txn.open_table(HISTORY_TABLE)?;

            // Find the next available index
            let start_key = (session_id, 0u64);
            let end_key = (session_id, u64::MAX);

            let mut max_index: u64 = 0;
            for result in table.range(start_key..=end_key)? {
                let (key, _) = result?;
                let idx = key.value().1;
                if idx >= max_index {
                    max_index = idx + 1;
                }
            }

            // Insert the new message
            let key = (session_id, max_index);
            let value = serde_json::to_vec(message)?;
            table.insert(key, value)?;

            max_index
        };
        write_txn.commit()?;
        Ok(index)
    }

    /// Append multiple messages to session history (batch incremental save).
    /// More efficient than save_history when adding new messages.
    pub fn append_messages(
        &self,
        session_id: &str,
        messages: &[SessionMessage],
    ) -> Result<usize, Error> {
        if messages.is_empty() {
            return Ok(0);
        }

        let write_txn = self.db.begin_write()?;
        let count = {
            let mut table = write_txn.open_table(HISTORY_TABLE)?;

            // Find the next available index
            let start_key = (session_id, 0u64);
            let end_key = (session_id, u64::MAX);

            let mut next_index: u64 = 0;
            for result in table.range(start_key..=end_key)? {
                let (key, _) = result?;
                let idx = key.value().1;
                if idx >= next_index {
                    next_index = idx + 1;
                }
            }

            // Insert all messages
            for message in messages {
                let key = (session_id, next_index);
                let value = serde_json::to_vec(message)?;
                table.insert(key, value)?;
                next_index += 1;
            }

            messages.len()
        };
        write_txn.commit()?;
        Ok(count)
    }

    /// Get the message count for a session without loading all messages.
    pub fn message_count(&self, session_id: &str) -> Result<usize, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(HISTORY_TABLE)?;

        let start_key = (session_id, 0u64);
        let end_key = (session_id, u64::MAX);

        let count = table.range(start_key..=end_key)?.count();
        Ok(count)
    }

    /// Delete a session.
    pub fn delete_session(&self, session_id: &str) -> Result<(), Error> {
        tracing::debug!(
            "[SessionStore] delete_session called for: {}",
            session_id
        );
        let write_txn = self.db.begin_write()?;
        tracing::debug!("[SessionStore] write transaction started");

        // Delete from sessions table
        {
            let mut sessions_table = write_txn.open_table(SESSIONS_TABLE)?;
            tracing::debug!("[SessionStore] removing from SESSIONS_TABLE");
            sessions_table.remove(session_id)?;
            tracing::debug!("[SessionStore] removed from SESSIONS_TABLE");
        }

        // Delete from metadata table
        {
            let mut meta_table = write_txn.open_table(SESSIONS_META_TABLE)?;
            let _ = meta_table.remove(session_id); // Ignore error if not exists
            tracing::debug!("[SessionStore] removed from SESSIONS_META_TABLE");
        }

        // Delete from history table - we need to collect the actual key tuples
        {
            tracing::debug!("[SessionStore] opening HISTORY_TABLE");
            let mut history_table = write_txn.open_table(HISTORY_TABLE)?;
            let start_key = (session_id, 0u64);
            let end_key = (session_id, u64::MAX);

            let mut keys_to_delete: Vec<(String, u64)> = Vec::new();
            tracing::debug!("[SessionStore] collecting history keys to delete");
            let mut range = history_table.range(start_key..=end_key)?;
            for result in range.by_ref() {
                let (key_ref, _val_ref) = result?;
                let sid: &str = key_ref.value().0;
                let idx: u64 = key_ref.value().1;
                keys_to_delete.push((sid.to_string(), idx));
            }
            drop(range);
            tracing::debug!(
                "[SessionStore] found {} history keys to delete",
                keys_to_delete.len()
            );

            for key in &keys_to_delete {
                history_table.remove((key.0.as_str(), key.1))?;
            }
            tracing::debug!("[SessionStore] removed history entries");
        }

        tracing::debug!("[SessionStore] committing transaction");
        write_txn.commit()?;
        tracing::debug!("[SessionStore] delete_session complete");
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
    pub fn save_session_metadata(
        &self,
        session_id: &str,
        metadata: &SessionMetadata,
    ) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(SESSIONS_META_TABLE)?;
            let value = serde_json::to_vec(metadata)?;
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
                let metadata: SessionMetadata = serde_json::from_slice(value.value().as_slice())?;
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

    // ========== P0.3: Pending Stream State Management ==========

    /// Save or update a pending stream state for a session.
    pub fn save_pending_stream(&self, state: &PendingStreamState) -> Result<(), Error> {
        let serialized = serde_json::to_vec(state)
            .map_err(|e| Error::Storage(format!("Failed to serialize pending stream state: {}", e)))?;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(PENDING_STREAM_TABLE)?;
            table.insert(state.session_id.as_str(), serialized)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Get the pending stream state for a session (if any).
    pub fn get_pending_stream(&self, session_id: &str) -> Result<Option<PendingStreamState>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(PENDING_STREAM_TABLE)?;

        match table.get(session_id)? {
            Some(value) => {
                let value_vec = value.value();
                let state = serde_json::from_slice::<PendingStreamState>(value_vec.as_slice())
                    .map_err(|e| Error::Storage(format!("Failed to deserialize pending stream state: {}", e)))?;
                Ok(Some(state))
            }
            None => Ok(None),
        }
    }

    /// Delete the pending stream state for a session.
    pub fn delete_pending_stream(&self, session_id: &str) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(PENDING_STREAM_TABLE)?;
            table.remove(session_id)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Get all pending stream states (e.g., for recovery after server restart).
    pub fn get_all_pending_streams(&self) -> Result<Vec<PendingStreamState>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(PENDING_STREAM_TABLE)?;

        let mut states = Vec::new();
        for result in table.iter()? {
            let (_key, value) = result?;
            let value_vec = value.value();
            match serde_json::from_slice::<PendingStreamState>(value_vec.as_slice()) {
                Ok(state) => {
                    // Skip stale states
                    if !state.is_stale() {
                        states.push(state);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to deserialize pending stream state: {}", e);
                }
            }
        }
        Ok(states)
    }

    /// Clean up stale pending stream states (older than 10 minutes).
    pub fn cleanup_stale_pending_streams(&self) -> Result<usize, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(PENDING_STREAM_TABLE)?;

        let mut stale_session_ids = Vec::new();
        for result in table.iter()? {
            let (key, value) = result?;
            let key_str = key.value().to_string();
            let value_vec = value.value();
            match serde_json::from_slice::<PendingStreamState>(value_vec.as_slice()) {
                Ok(state) => {
                    if state.is_stale() {
                        stale_session_ids.push(key_str);
                    }
                }
                Err(_) => {
                    // Corrupted state, mark for cleanup
                    stale_session_ids.push(key_str);
                }
            }
        }
        drop(read_txn);

        // Delete stale states
        let count = stale_session_ids.len();
        if !stale_session_ids.is_empty() {
            let write_txn = self.db.begin_write()?;
            {
                let mut table = write_txn.open_table(PENDING_STREAM_TABLE)?;
                for session_id in stale_session_ids {
                    let _ = table.remove(session_id.as_str());
                }
            }
            write_txn.commit()?;
        }

        Ok(count)
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
            images: None,
            timestamp: chrono::Utc::now().timestamp(),
        };
        let msg2 = SessionMessage {
            role: "assistant".to_string(),
            content: "Hi there!".to_string(),
            tool_calls: None,
            tool_call_id: None,
            tool_call_name: None,
            thinking: None,
            images: None,
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
        let meta = store
            .get_session_metadata("test-session")
            .unwrap_or_else(|_| SessionMetadata::default());
        assert!(meta.title.is_none());

        // Set title
        store
            .save_session_metadata(
                "test-session",
                &SessionMetadata {
                    title: Some("My Chat Session".to_string()),
                },
            )
            .unwrap();

        // Get title
        let meta = store.get_session_metadata("test-session").unwrap();
        assert_eq!(meta.title, Some("My Chat Session".to_string()));

        // Update with different title
        store
            .save_session_metadata(
                "test-session",
                &SessionMetadata {
                    title: Some("New Title".to_string()),
                },
            )
            .unwrap();
        let meta = store.get_session_metadata("test-session").unwrap();
        assert_eq!(meta.title, Some("New Title".to_string()));

        // Clear title
        store
            .save_session_metadata("test-session", &SessionMetadata { title: None })
            .unwrap();
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
            images: None,
            timestamp: 12345,
        };

        let serialized = serde_json::to_vec(&msg).unwrap();
        let deserialized: SessionMessage = serde_json::from_slice(&serialized).unwrap();
        assert_eq!(deserialized.role, "user");
        assert_eq!(deserialized.content, "Hello");
        assert_eq!(deserialized.timestamp, 12345);
    }

    #[test]
    fn test_session_message_builder() {
        let msg = SessionMessage::user("test").with_timestamp(12345);
        assert_eq!(msg.content, "test");
        assert_eq!(msg.timestamp, 12345);

        let msg = SessionMessage::assistant("response").with_thinking("Let me think...");
        assert_eq!(msg.thinking, Some("Let me think...".to_string()));

        let tool_calls = vec![serde_json::json!({"name": "test"})];
        let msg = SessionMessage::assistant("I'll use a tool").with_tool_calls(tool_calls);
        assert!(msg.tool_calls.is_some());
    }
}
