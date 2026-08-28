//! Session storage using redb.
//!
//! Provides persistent storage for chat sessions and message history.
//!
//! NOTE: Uses JSON serialization instead of Bincode for better schema compatibility.
//! JSON is more forgiving when fields are added/removed from structs over time.

use parking_lot::Mutex;
use std::path::Path;
use std::sync::Arc;

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
const PENDING_STREAM_TABLE: TableDefinition<&str, Vec<u8>> =
    TableDefinition::new("pending_streams");

/// Default value for memory_enabled: true.
const fn default_memory_enabled() -> bool {
    true
}

/// Session metadata (title, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// User-defined title for the session
    #[serde(default)]
    pub title: Option<String>,
    /// Whether memory injection is enabled for this session (defaults to true)
    #[serde(default = "default_memory_enabled")]
    pub memory_enabled: bool,
    /// Conversation summary for context compression (injected when context exceeds threshold)
    #[serde(default)]
    pub conversation_summary: Option<String>,
    /// Index of the last message that has been summarized (messages up to this index can be removed)
    #[serde(default)]
    pub summary_up_to_index: Option<u64>,
    /// Preview text derived from the first user message (truncated)
    #[serde(default)]
    pub preview: Option<String>,
}

impl Default for SessionMetadata {
    fn default() -> Self {
        Self {
            title: None,
            memory_enabled: true,
            conversation_summary: None,
            summary_up_to_index: None,
            preview: None,
        }
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
    /// Images attached to the message (base64 data URLs).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<SessionMessageImage>>,
    /// Round contents for multi-step tool calls (round number → intermediate text).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub round_contents: Option<serde_json::Value>,
    /// Per-round thinking content for grouped rendering (round number → thinking text).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub round_thinking: Option<serde_json::Value>,
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
            round_contents: None,
            round_thinking: None,
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
            round_contents: None,
            round_thinking: None,
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
            round_contents: None,
            round_thinking: None,
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
            round_contents: None,
            round_thinking: None,
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
            round_contents: None,
            round_thinking: None,
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
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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

    /// Check if the state is stale (older than 10 minutes).
    pub fn is_stale(&self) -> bool {
        let now = chrono::Utc::now().timestamp();
        now - self.updated_at > 600 // 10 minutes
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
static SESSION_STORE_SINGLETON: Mutex<Option<Arc<SessionStore>>> = Mutex::new(None);

impl SessionStore {
    /// Open or create a session store at the given path.
    /// Uses a singleton pattern to prevent multiple opens of the same database.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Arc<Self>, Error> {
        let path_str = path.as_ref().to_string_lossy().to_string();

        // Check if we already have a store for this path
        {
            let singleton = SESSION_STORE_SINGLETON.lock();
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
        // Rollback guard: refuse databases stamped by a newer build (see schema.rs).
        crate::schema::check_or_stamp(&db)
            .map_err(|e| Error::Storage(format!("schema version: {e}")))?;

        let store = Arc::new(SessionStore {
            db: Arc::new(db),
            path: path_str,
        });

        {
            let mut singleton = SESSION_STORE_SINGLETON.lock();
            *singleton = Some(store.clone());
        }
        tracing::debug!("[SessionStore] Opened session store at: {}", store.path);
        Ok(store)
    }

    /// Open a session store WITHOUT registering the global singleton.
    ///
    /// Each call creates a fresh, independent instance. Use in tests / isolated
    /// runs where sharing the singleton would cause cross-test interference
    /// (e.g. parallel sessions-handler tests racing on one shared store).
    /// `":memory:"` maps to redb's in-memory backend so every call is truly
    /// independent; other paths open/create that file directly. Production
    /// code should keep using [`open`](Self::open).
    pub fn open_isolated<P: AsRef<Path>>(path: P) -> Result<Arc<Self>, Error> {
        let path_str = path.as_ref().to_string_lossy().to_string();
        // ":memory:" → unique temp file. redb 2.1 has no in-memory backend,
        // and the global singleton would make parallel tests share one store.
        // A unique path guarantees each call gets an independent store
        // (matches TimeSeriesStorage::memory's pattern).
        let db_path: std::path::PathBuf = if path_str == ":memory:" {
            std::env::temp_dir().join(format!("session_isolated_{}.redb", uuid::Uuid::new_v4()))
        } else {
            path.as_ref().to_path_buf()
        };
        let db = if db_path.exists() {
            Database::open(&db_path)?
        } else {
            Database::create(&db_path)?
        };
        Ok(Arc::new(SessionStore {
            db: Arc::new(db),
            path: path_str,
        }))
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
                if let Ok(mut r) = range {
                    if r.next().is_some() {
                        // Existing history found, don't clear it
                        tracing::debug!(
                            "[save_history] Refusing to clear existing history for session {}",
                            session_id
                        );
                        return Ok(());
                    }
                }
            }
        }

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(HISTORY_TABLE)?;

            // If messages is empty, don't delete existing data
            // This prevents accidental data loss
            if messages.is_empty() {
                tracing::warn!(
                    "[save_history] Warning: Attempting to save empty message list for session {}, skipping to avoid data loss",
                    session_id
                );
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

            tracing::debug!(
                "[save_history] Saved {} messages for session {}",
                messages.len(),
                session_id
            );
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
        tracing::debug!("[SessionStore] Deleting session: {}", session_id);
        let write_txn = self.db.begin_write()?;

        // Delete from sessions table
        {
            let mut sessions_table = write_txn.open_table(SESSIONS_TABLE)?;
            sessions_table.remove(session_id)?;
        }

        // Delete from metadata table
        {
            let mut meta_table = write_txn.open_table(SESSIONS_META_TABLE)?;
            let _ = meta_table.remove(session_id); // Ignore error if not exists
        }

        // Delete from history table - we need to collect the actual key tuples
        {
            let mut history_table = write_txn.open_table(HISTORY_TABLE)?;
            let start_key = (session_id, 0u64);
            let end_key = (session_id, u64::MAX);

            let mut keys_to_delete: Vec<(String, u64)> = Vec::new();
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
        let table = match read_txn.open_table(SESSIONS_TABLE) {
            Ok(t) => t,
            Err(_) => return Ok(Vec::new()),
        };

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
        let table = match read_txn.open_table(SESSIONS_TABLE) {
            Ok(t) => t,
            Err(_) => return Ok(false),
        };
        Ok(table.get(session_id)?.is_some())
    }

    /// Get session timestamp.
    pub fn get_session_timestamp(&self, session_id: &str) -> Result<Option<i64>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = match read_txn.open_table(SESSIONS_TABLE) {
            Ok(t) => t,
            Err(_) => return Ok(None),
        };
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

    /// Toggle memory enabled state for a session.
    pub fn toggle_memory(&self, session_id: &str, enabled: bool) -> Result<(), Error> {
        let mut metadata = self.get_session_metadata(session_id)?;
        metadata.memory_enabled = enabled;
        self.save_session_metadata(session_id, &metadata)
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
    /// Returns Ok(()) even if the table doesn't exist (creates it automatically).
    pub fn save_pending_stream(&self, state: &PendingStreamState) -> Result<(), Error> {
        let serialized = serde_json::to_vec(state).map_err(|e| {
            Error::Storage(format!("Failed to serialize pending stream state: {}", e))
        })?;

        let write_txn = self.db.begin_write()?;
        {
            // Use open_table which creates the table if it doesn't exist
            let mut table = write_txn.open_table(PENDING_STREAM_TABLE).map_err(|e| {
                Error::Storage(format!("Failed to open pending_streams table: {}", e))
            })?;
            table.insert(state.session_id.as_str(), serialized)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Get the pending stream state for a session (if any).
    /// Returns Ok(None) if the table doesn't exist or session not found.
    pub fn get_pending_stream(
        &self,
        session_id: &str,
    ) -> Result<Option<PendingStreamState>, Error> {
        let read_txn = match self.db.begin_read() {
            Ok(txn) => txn,
            Err(_) => return Ok(None), // Database error, return None
        };

        let table = match read_txn.open_table(PENDING_STREAM_TABLE) {
            Ok(t) => t,
            Err(e) => {
                // Table doesn't exist yet - this is normal for new databases
                let error_msg = e.to_string();
                if error_msg.contains("does not exist") || error_msg.contains("Table") {
                    return Ok(None);
                }
                return Err(Error::Storage(format!("Redb table error: {}", e)));
            }
        };

        match table.get(session_id)? {
            Some(value) => {
                let value_vec = value.value();
                let state = serde_json::from_slice::<PendingStreamState>(value_vec.as_slice())
                    .map_err(|e| {
                        Error::Storage(format!("Failed to deserialize pending stream state: {}", e))
                    })?;
                Ok(Some(state))
            }
            None => Ok(None),
        }
    }

    /// Delete the pending stream state for a session.
    /// Returns Ok(()) even if the table doesn't exist.
    pub fn delete_pending_stream(&self, session_id: &str) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = match write_txn.open_table(PENDING_STREAM_TABLE) {
                Ok(t) => t,
                Err(e) => {
                    // Table doesn't exist yet - nothing to delete
                    let error_msg = e.to_string();
                    if error_msg.contains("does not exist") || error_msg.contains("Table") {
                        return Ok(());
                    }
                    return Err(Error::Storage(format!("Redb table error: {}", e)));
                }
            };
            table.remove(session_id)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Get all pending stream states (e.g., for recovery after server restart).
    /// Returns empty vec if the table doesn't exist yet (graceful handling for new databases).
    pub fn get_all_pending_streams(&self) -> Result<Vec<PendingStreamState>, Error> {
        let read_txn = match self.db.begin_read() {
            Ok(txn) => txn,
            Err(_) => return Ok(vec![]), // Database error, return empty
        };

        let table = match read_txn.open_table(PENDING_STREAM_TABLE) {
            Ok(t) => t,
            Err(e) => {
                // Table doesn't exist yet - this is normal for new databases
                let error_msg = e.to_string();
                if error_msg.contains("does not exist") || error_msg.contains("Table") {
                    return Ok(vec![]);
                }
                return Err(Error::Storage(format!("Redb table error: {}", e)));
            }
        };

        let mut states = Vec::new();
        for result in table.iter()? {
            let (key, value) = result?;
            let value_vec = value.value();
            match serde_json::from_slice::<PendingStreamState>(value_vec.as_slice()) {
                Ok(state) => {
                    states.push(state);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to deserialize pending stream state for key '{}': {}",
                        key.value(),
                        e
                    );
                }
            }
        }
        Ok(states)
    }

    /// Clean up stale pending stream states (older than 10 minutes).
    /// Returns Ok(0) if the table doesn't exist yet (graceful handling for new databases).
    pub fn cleanup_stale_pending_streams(&self) -> Result<usize, Error> {
        let read_txn = match self.db.begin_read() {
            Ok(txn) => txn,
            Err(_) => return Ok(0), // Database error, nothing to clean
        };

        let table = match read_txn.open_table(PENDING_STREAM_TABLE) {
            Ok(t) => t,
            Err(e) => {
                // Table doesn't exist yet - this is normal for new databases
                let error_msg = e.to_string();
                if error_msg.contains("does not exist") || error_msg.contains("Table") {
                    return Ok(0);
                }
                return Err(Error::Storage(format!("Redb table error: {}", e)));
            }
        };

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
            round_contents: None,
            round_thinking: None,
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
            round_contents: None,
            round_thinking: None,
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
                    ..Default::default()
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
                    ..Default::default()
                },
            )
            .unwrap();
        let meta = store.get_session_metadata("test-session").unwrap();
        assert_eq!(meta.title, Some("New Title".to_string()));

        // Clear title
        store
            .save_session_metadata(
                "test-session",
                &SessionMetadata {
                    title: None,
                    ..Default::default()
                },
            )
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
            round_contents: None,
            round_thinking: None,
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

    #[test]
    fn test_append_message() {
        let store = create_temp_store();

        // Save session
        store.save_session_id("test-session").unwrap();

        // Append single message
        let msg = SessionMessage::user("Hello");
        let index = store.append_message("test-session", &msg).unwrap();
        assert_eq!(index, 0);

        // Append another message
        let msg2 = SessionMessage::assistant("Hi there!");
        let index2 = store.append_message("test-session", &msg2).unwrap();
        assert_eq!(index2, 1);

        // Verify both messages are stored
        let loaded = store.load_history("test-session").unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].content, "Hello");
        assert_eq!(loaded[1].content, "Hi there!");
    }

    #[test]
    fn test_append_messages() {
        let store = create_temp_store();

        // Save session
        store.save_session_id("test-session").unwrap();

        // Append batch of messages
        let messages = vec![
            SessionMessage::user("First"),
            SessionMessage::assistant("Response 1"),
            SessionMessage::user("Second"),
        ];
        let count = store.append_messages("test-session", &messages).unwrap();
        assert_eq!(count, 3);

        // Append another batch
        let more_messages = vec![
            SessionMessage::assistant("Response 2"),
            SessionMessage::user("Third"),
        ];
        let count2 = store
            .append_messages("test-session", &more_messages)
            .unwrap();
        assert_eq!(count2, 2);

        // Verify all messages are stored
        let loaded = store.load_history("test-session").unwrap();
        assert_eq!(loaded.len(), 5);
        assert_eq!(loaded[0].content, "First");
        assert_eq!(loaded[4].content, "Third");
    }

    #[test]
    fn test_append_messages_empty() {
        let store = create_temp_store();

        // Save session (this creates the sessions table)
        store.save_session_id("test-session").unwrap();

        // First, create the history table by saving at least one message
        store
            .append_message("test-session", &SessionMessage::user("First"))
            .unwrap();

        // Now test appending empty batch
        let count = store.append_messages("test-session", &[]).unwrap();
        assert_eq!(count, 0);

        // Verify only the first message exists
        let loaded = store.load_history("test-session").unwrap();
        assert_eq!(loaded.len(), 1);
    }

    #[test]
    fn test_clear_history() {
        let store = create_temp_store();

        // Save session with messages
        store.save_session_id("test-session").unwrap();
        let messages = vec![
            SessionMessage::user("Hello"),
            SessionMessage::assistant("Hi"),
        ];
        store.save_history("test-session", &messages).unwrap();

        // Verify messages exist
        let loaded = store.load_history("test-session").unwrap();
        assert_eq!(loaded.len(), 2);

        // Clear history
        store.clear_history("test-session").unwrap();

        // Verify history is cleared
        let loaded = store.load_history("test-session").unwrap();
        assert_eq!(loaded.len(), 0);

        // Session should still exist
        assert!(store.session_exists("test-session").unwrap());
    }

    #[test]
    fn test_message_count() {
        let store = create_temp_store();

        // Save session (this creates the sessions table)
        store.save_session_id("test-session").unwrap();

        // Add messages (this creates the history table)
        let messages = vec![
            SessionMessage::user("First"),
            SessionMessage::assistant("Response 1"),
            SessionMessage::user("Second"),
        ];
        store.save_history("test-session", &messages).unwrap();

        // Count should be 3
        let count = store.message_count("test-session").unwrap();
        assert_eq!(count, 3);

        // Add more messages via append
        store
            .append_message("test-session", &SessionMessage::assistant("Response 2"))
            .unwrap();

        // Count should be 4
        let count = store.message_count("test-session").unwrap();
        assert_eq!(count, 4);
    }

    #[test]
    fn test_save_history_empty_prevents_data_loss() {
        let store = create_temp_store();

        // Save session with initial messages
        store.save_session_id("test-session").unwrap();
        let messages = vec![
            SessionMessage::user("Original message"),
            SessionMessage::assistant("Original response"),
        ];
        store.save_history("test-session", &messages).unwrap();

        // Verify messages exist
        let loaded = store.load_history("test-session").unwrap();
        assert_eq!(loaded.len(), 2);

        // Try to save empty history - should be rejected to prevent data loss
        store.save_history("test-session", &[]).unwrap();

        // Original messages should still be there
        let loaded = store.load_history("test-session").unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].content, "Original message");
    }

    #[test]
    fn test_get_session_timestamp() {
        let store = create_temp_store();

        // Save session
        store.save_session_id("test-session").unwrap();

        // Get timestamp
        let timestamp = store
            .get_session_timestamp("test-session")
            .unwrap()
            .expect("Timestamp should exist");
        assert!(timestamp > 0);

        // Non-existent session should return None
        let timestamp = store.get_session_timestamp("non-existent").unwrap();
        assert!(timestamp.is_none());
    }

    #[test]
    fn test_toggle_memory() {
        let store = create_temp_store();

        // Save session
        store.save_session_id("test-session").unwrap();

        // Initially memory_enabled should be true (default)
        let meta = store.get_session_metadata("test-session").unwrap();
        assert!(meta.memory_enabled);

        // Enable memory
        store.toggle_memory("test-session", true).unwrap();
        let meta = store.get_session_metadata("test-session").unwrap();
        assert!(meta.memory_enabled);

        // Disable memory
        store.toggle_memory("test-session", false).unwrap();
        let meta = store.get_session_metadata("test-session").unwrap();
        assert!(!meta.memory_enabled);
    }

    #[test]
    fn test_delete_session_metadata() {
        let store = create_temp_store();

        // Save session with metadata
        store.save_session_id("test-session").unwrap();
        let metadata = SessionMetadata {
            title: Some("Test Session".to_string()),
            memory_enabled: true,
            ..Default::default()
        };
        store
            .save_session_metadata("test-session", &metadata)
            .unwrap();

        // Verify metadata exists
        let meta = store.get_session_metadata("test-session").unwrap();
        assert_eq!(meta.title, Some("Test Session".to_string()));
        assert!(meta.memory_enabled);

        // Delete metadata
        store.delete_session_metadata("test-session").unwrap();

        // Metadata should be default
        let meta = store.get_session_metadata("test-session").unwrap();
        assert!(meta.title.is_none());
        assert!(meta.memory_enabled); // defaults to true now
    }

    #[test]
    fn test_session_not_found() {
        let store = create_temp_store();

        // First create a session to ensure tables exist
        store.save_session_id("dummy-session").unwrap();
        store
            .append_message("dummy-session", &SessionMessage::user("Dummy"))
            .unwrap();

        // Now test operations on non-existent session
        assert!(!store.session_exists("non-existent").unwrap());

        // Load history should return empty vec
        let loaded = store.load_history("non-existent").unwrap();
        assert_eq!(loaded.len(), 0);

        // Message count should be 0
        let count = store.message_count("non-existent").unwrap();
        assert_eq!(count, 0);

        // Delete should succeed (no-op)
        store.delete_session("non-existent").unwrap();

        // Clear history should succeed (no-op)
        store.clear_history("non-existent").unwrap();
    }

    #[test]
    fn test_multiple_sessions() {
        let store = create_temp_store();

        // Create multiple sessions
        store.save_session_id("session-1").unwrap();
        store.save_session_id("session-2").unwrap();
        store.save_session_id("session-3").unwrap();

        // List all sessions
        let sessions = store.list_sessions().unwrap();
        assert_eq!(sessions.len(), 3);
        assert!(sessions.contains(&"session-1".to_string()));
        assert!(sessions.contains(&"session-2".to_string()));
        assert!(sessions.contains(&"session-3".to_string()));

        // Add messages to each session
        store
            .save_history("session-1", &[SessionMessage::user("Message 1")])
            .unwrap();
        store
            .save_history("session-2", &[SessionMessage::user("Message 2")])
            .unwrap();
        store
            .save_history("session-3", &[SessionMessage::user("Message 3")])
            .unwrap();

        // Verify each session has its own messages
        let loaded1 = store.load_history("session-1").unwrap();
        let loaded2 = store.load_history("session-2").unwrap();
        let loaded3 = store.load_history("session-3").unwrap();
        assert_eq!(loaded1.len(), 1);
        assert_eq!(loaded2.len(), 1);
        assert_eq!(loaded3.len(), 1);
        assert_eq!(loaded1[0].content, "Message 1");
        assert_eq!(loaded2[0].content, "Message 2");
        assert_eq!(loaded3[0].content, "Message 3");

        // Delete one session
        store.delete_session("session-2").unwrap();

        // Verify only session-2 is deleted
        assert!(store.session_exists("session-1").unwrap());
        assert!(!store.session_exists("session-2").unwrap());
        assert!(store.session_exists("session-3").unwrap());

        let sessions = store.list_sessions().unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn test_save_history_overwrites() {
        let store = create_temp_store();

        // Save session with initial messages
        store.save_session_id("test-session").unwrap();
        let messages1 = vec![
            SessionMessage::user("Original 1"),
            SessionMessage::assistant("Original 2"),
        ];
        store.save_history("test-session", &messages1).unwrap();

        // Overwrite with new messages
        let messages2 = vec![
            SessionMessage::user("New 1"),
            SessionMessage::assistant("New 2"),
            SessionMessage::user("New 3"),
        ];
        store.save_history("test-session", &messages2).unwrap();

        // Verify new messages replaced old ones
        let loaded = store.load_history("test-session").unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0].content, "New 1");
        assert_eq!(loaded[1].content, "New 2");
        assert_eq!(loaded[2].content, "New 3");
    }

    #[test]
    fn test_session_metadata_with_all_fields() {
        let store = create_temp_store();

        // Save session
        store.save_session_id("test-session").unwrap();

        // Set all metadata fields
        let metadata = SessionMetadata {
            title: Some("Test Session".to_string()),
            memory_enabled: true,
            conversation_summary: Some("This is a summary".to_string()),
            summary_up_to_index: Some(5),
            preview: None,
        };
        store
            .save_session_metadata("test-session", &metadata)
            .unwrap();

        // Retrieve and verify
        let loaded = store.get_session_metadata("test-session").unwrap();
        assert_eq!(loaded.title, Some("Test Session".to_string()));
        assert!(loaded.memory_enabled);
        assert_eq!(
            loaded.conversation_summary,
            Some("This is a summary".to_string())
        );
        assert_eq!(loaded.summary_up_to_index, Some(5));
    }

    #[test]
    fn test_pending_stream_state() {
        let store = create_temp_store();

        // Create and save pending stream state
        let mut state = PendingStreamState::new("test-session".to_string(), "Hello".to_string());
        state.update_content("Response so far");
        state.update_thinking("Thinking...");
        state.set_stage(StreamStage::Generating);

        store.save_pending_stream(&state).unwrap();

        // Retrieve state
        let loaded = store.get_pending_stream("test-session").unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.session_id, "test-session");
        assert_eq!(loaded.user_message, "Hello");
        assert_eq!(loaded.content, "Response so far");
        assert_eq!(loaded.thinking, "Thinking...");
        assert!(matches!(loaded.stage, StreamStage::Generating));

        // Delete state
        store.delete_pending_stream("test-session").unwrap();

        // Verify deleted
        let loaded = store.get_pending_stream("test-session").unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_pending_stream_state_not_found() {
        let store = create_temp_store();

        // Non-existent session should return None
        let loaded = store.get_pending_stream("non-existent").unwrap();
        assert!(loaded.is_none());

        // Delete should succeed (no-op)
        store.delete_pending_stream("non-existent").unwrap();
    }

    #[test]
    fn test_get_all_pending_streams() {
        let store = create_temp_store();

        // Initially empty
        let all = store.get_all_pending_streams().unwrap();
        assert_eq!(all.len(), 0);

        // Add multiple pending streams
        let state1 = PendingStreamState::new("session-1".to_string(), "Message 1".to_string());
        let state2 = PendingStreamState::new("session-2".to_string(), "Message 2".to_string());
        let state3 = PendingStreamState::new("session-3".to_string(), "Message 3".to_string());

        store.save_pending_stream(&state1).unwrap();
        store.save_pending_stream(&state2).unwrap();
        store.save_pending_stream(&state3).unwrap();

        // Retrieve all
        let all = store.get_all_pending_streams().unwrap();
        assert_eq!(all.len(), 3);

        let session_ids: Vec<&str> = all.iter().map(|s| s.session_id.as_str()).collect();
        assert!(session_ids.contains(&"session-1"));
        assert!(session_ids.contains(&"session-2"));
        assert!(session_ids.contains(&"session-3"));
    }

    #[test]
    fn test_cleanup_stale_pending_streams() {
        let store = create_temp_store();

        // Create a stale stream state (manually set old timestamp)
        let mut stale_state =
            PendingStreamState::new("stale-session".to_string(), "Old".to_string());
        stale_state.updated_at = chrono::Utc::now().timestamp() - 700; // 11.6 minutes ago
        store.save_pending_stream(&stale_state).unwrap();

        // Create a fresh stream state
        let fresh_state = PendingStreamState::new("fresh-session".to_string(), "New".to_string());
        store.save_pending_stream(&fresh_state).unwrap();

        // Cleanup should remove the stale one
        let cleaned = store.cleanup_stale_pending_streams().unwrap();
        assert_eq!(cleaned, 1);

        // Verify stale is gone, fresh remains
        let stale = store.get_pending_stream("stale-session").unwrap();
        assert!(stale.is_none());

        let fresh = store.get_pending_stream("fresh-session").unwrap();
        assert!(fresh.is_some());
    }

    #[test]
    fn test_session_message_with_images() {
        let store = create_temp_store();

        // Create message with images
        let images = vec![
            SessionMessageImage {
                data: "data:image/png;base64,iVBORw0KG...".to_string(),
                mime_type: Some("image/png".to_string()),
            },
            SessionMessageImage {
                data: "data:image/jpeg;base64,/9j/4AAQ...".to_string(),
                mime_type: Some("image/jpeg".to_string()),
            },
        ];

        let msg = SessionMessage::user_with_images("Look at these images", images);
        store.save_session_id("test-session").unwrap();
        store.append_message("test-session", &msg).unwrap();

        // Retrieve and verify
        let loaded = store.load_history("test-session").unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].content, "Look at these images");
        assert!(loaded[0].images.is_some());
        let images = loaded[0].images.as_ref().unwrap();
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].mime_type, Some("image/png".to_string()));
        assert_eq!(images[1].mime_type, Some("image/jpeg".to_string()));
    }

    #[test]
    fn test_tool_message() {
        let store = create_temp_store();

        // Create tool message
        let msg = SessionMessage::tool("call_123", "Tool execution result");
        store.save_session_id("test-session").unwrap();
        store.append_message("test-session", &msg).unwrap();

        // Retrieve and verify
        let loaded = store.load_history("test-session").unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].role, "tool");
        assert_eq!(loaded[0].tool_call_id, Some("call_123".to_string()));
        assert_eq!(loaded[0].content, "Tool execution result");
    }

    #[test]
    fn test_system_message() {
        let store = create_temp_store();

        // Create system message
        let msg = SessionMessage::system("You are a helpful assistant");
        store.save_session_id("test-session").unwrap();
        store.append_message("test-session", &msg).unwrap();

        // Retrieve and verify
        let loaded = store.load_history("test-session").unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].role, "system");
        assert_eq!(loaded[0].content, "You are a helpful assistant");
    }

    #[tokio::test]
    async fn test_concurrent_session_access() {
        let store = create_temp_store();

        // Create session
        store.save_session_id("test-session").unwrap();

        // Spawn multiple tasks that append messages concurrently
        let store_clone = store.clone();
        let handle1 = tokio::spawn(async move {
            for i in 0..5 {
                store_clone
                    .append_message("test-session", &SessionMessage::user(format!("Msg1-{}", i)))
                    .unwrap();
            }
        });

        let store_clone = store.clone();
        let handle2 = tokio::spawn(async move {
            for i in 0..5 {
                store_clone
                    .append_message(
                        "test-session",
                        &SessionMessage::assistant(format!("Msg2-{}", i)),
                    )
                    .unwrap();
            }
        });

        let store_clone = store.clone();
        let handle3 = tokio::spawn(async move {
            for i in 0..5 {
                store_clone
                    .append_message("test-session", &SessionMessage::user(format!("Msg3-{}", i)))
                    .unwrap();
            }
        });

        // Wait for all tasks to complete
        let results = tokio::join!(handle1, handle2, handle3);
        assert!(results.0.is_ok());
        assert!(results.1.is_ok());
        assert!(results.2.is_ok());

        // Verify all messages were saved
        let loaded = store.load_history("test-session").unwrap();
        assert_eq!(loaded.len(), 15);

        // Verify message count
        let count = store.message_count("test-session").unwrap();
        assert_eq!(count, 15);
    }

    #[tokio::test]
    async fn test_concurrent_different_sessions() {
        let store = create_temp_store();

        let store_clone = store.clone();
        let handle1 = tokio::spawn(async move {
            for i in 0..3 {
                let session_id = format!("session-1-{}", i);
                store_clone.save_session_id(&session_id).unwrap();
                store_clone
                    .append_message(&session_id, &SessionMessage::user(format!("Msg {}", i)))
                    .unwrap();
            }
        });

        let store_clone = store.clone();
        let handle2 = tokio::spawn(async move {
            for i in 0..3 {
                let session_id = format!("session-2-{}", i);
                store_clone.save_session_id(&session_id).unwrap();
                store_clone
                    .append_message(&session_id, &SessionMessage::user(format!("Msg {}", i)))
                    .unwrap();
            }
        });

        // Wait for completion
        let results = tokio::join!(handle1, handle2);
        assert!(results.0.is_ok());
        assert!(results.1.is_ok());

        // Verify all sessions exist
        let sessions = store.list_sessions().unwrap();
        assert_eq!(sessions.len(), 6);

        // Verify each session has its messages
        for session_id in sessions {
            let loaded = store.load_history(&session_id).unwrap();
            assert_eq!(loaded.len(), 1);
        }
    }

    #[test]
    fn test_session_metadata_default_values() {
        let store = create_temp_store();

        // Save session
        store.save_session_id("test-session").unwrap();

        // Get metadata without setting it first
        let meta = store.get_session_metadata("test-session").unwrap();

        // Should have default values
        assert!(meta.title.is_none());
        assert!(meta.memory_enabled); // defaults to true
        assert!(meta.conversation_summary.is_none());
        assert!(meta.summary_up_to_index.is_none());
    }

    #[test]
    fn test_duplicate_session_id() {
        let store = create_temp_store();

        // Save session twice with same ID
        store.save_session_id("test-session").unwrap();
        store.save_session_id("test-session").unwrap();

        // Should only have one session
        let sessions = store.list_sessions().unwrap();
        assert_eq!(sessions.len(), 1);

        // Session should exist
        assert!(store.session_exists("test-session").unwrap());

        // Save messages and verify
        store
            .save_history("test-session", &[SessionMessage::user("Test")])
            .unwrap();
        let loaded = store.load_history("test-session").unwrap();
        assert_eq!(loaded.len(), 1);
    }

    #[test]
    fn test_large_message_history() {
        let store = create_temp_store();

        // Save session
        store.save_session_id("test-session").unwrap();

        // Add many messages (100 messages)
        let messages: Vec<SessionMessage> = (0..100)
            .map(|i| {
                SessionMessage::user(format!("This is message number {} with some content", i))
                    .with_thinking(format!("Thinking about message {}", i))
            })
            .collect();

        store.save_history("test-session", &messages).unwrap();

        // Verify all messages are saved
        let loaded = store.load_history("test-session").unwrap();
        assert_eq!(loaded.len(), 100);

        // Verify message count
        let count = store.message_count("test-session").unwrap();
        assert_eq!(count, 100);

        // Verify first and last messages
        assert_eq!(
            loaded[0].content,
            "This is message number 0 with some content"
        );
        assert_eq!(
            loaded[99].content,
            "This is message number 99 with some content"
        );
        assert_eq!(
            loaded[0].thinking,
            Some("Thinking about message 0".to_string())
        );
        assert_eq!(
            loaded[99].thinking,
            Some("Thinking about message 99".to_string())
        );
    }

    #[test]
    fn test_save_history_updates_timestamp() {
        let store = create_temp_store();

        // Save session
        store.save_session_id("test-session").unwrap();

        // Get initial timestamp
        let timestamp1 = store
            .get_session_timestamp("test-session")
            .unwrap()
            .unwrap();

        // Wait a bit and save again
        std::thread::sleep(std::time::Duration::from_millis(10));
        store.save_session_id("test-session").unwrap();

        // Get updated timestamp
        let timestamp2 = store
            .get_session_timestamp("test-session")
            .unwrap()
            .unwrap();

        // Timestamp should be updated
        assert!(timestamp2 >= timestamp1);
    }

    #[test]
    fn test_pending_stream_state_default_values() {
        let state = PendingStreamState::new("test-session".to_string(), "Hello".to_string());

        // Check default values
        assert_eq!(state.session_id, "test-session");
        assert_eq!(state.user_message, "Hello");
        assert!(state.content.is_empty());
        assert!(state.thinking.is_empty());
        assert!(matches!(state.stage, StreamStage::Waiting));
        assert!(state.tool_calls.is_none());
        assert!(!state.interrupted);

        // Elapsed time should be small
        let elapsed = state.elapsed_secs();
        assert!((0..2).contains(&elapsed));

        // Should not be stale
        assert!(!state.is_stale());
    }

    #[test]
    fn test_pending_stream_state_stage_transitions() {
        let mut state = PendingStreamState::new("test".to_string(), "Msg".to_string());

        // Initial stage
        assert!(matches!(state.stage, StreamStage::Waiting));

        // Transition to thinking
        state.update_thinking("Thinking...");
        assert!(matches!(state.stage, StreamStage::Thinking));

        // Transition to generating
        state.set_stage(StreamStage::Generating);
        assert!(matches!(state.stage, StreamStage::Generating));

        // Transition to tool execution
        state.tool_calls = Some(vec![serde_json::json!({"name": "test"})]);
        state.set_stage(StreamStage::ToolExecution);
        assert!(matches!(state.stage, StreamStage::ToolExecution));

        // Transition to complete
        state.set_stage(StreamStage::Complete);
        assert!(matches!(state.stage, StreamStage::Complete));
    }

    #[test]
    fn test_cleanup_stale_pending_streams_empty() {
        let store = create_temp_store();

        // Cleanup on empty database should return 0
        let cleaned = store.cleanup_stale_pending_streams().unwrap();
        assert_eq!(cleaned, 0);
    }
}
