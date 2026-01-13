//! Session manager for multiple agent sessions with persistence.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use futures::Stream;
use tokio::sync::RwLock;
use uuid::Uuid;

use edge_ai_storage::SessionStore;

use super::agent::{Agent, AgentConfig, AgentEvent, AgentMessage, LlmBackend};
use super::error::{AgentError, Result};

// Re-export instance manager for convenience
pub use edge_ai_llm::instance_manager::{
    LlmBackendInstanceManager, get_instance_manager, BackendTypeDefinition,
};

/// Information about a session for listing.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionInfo {
    /// Session ID
    #[serde(rename = "sessionId")]
    pub session_id: String,
    /// Creation timestamp
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    /// Number of messages in the session
    #[serde(rename = "messageCount")]
    pub message_count: u32,
    /// User-defined title
    pub title: Option<String>,
    /// Preview of the first user message
    pub preview: Option<String>,
}

/// Session manager for managing multiple agent sessions with persistence.
pub struct SessionManager {
    /// Active sessions (in-memory cache)
    sessions: Arc<RwLock<HashMap<String, Arc<Agent>>>>,
    /// Message history for sessions
    session_messages: Arc<RwLock<HashMap<String, Vec<AgentMessage>>>>,
    /// Persistent storage for sessions
    store: Arc<SessionStore>,
    /// Default agent config
    default_config: AgentConfig,
    /// Default LLM backend (configured for new sessions)
    default_llm_backend: Arc<RwLock<Option<LlmBackend>>>,
    /// Tool registry for all sessions
    tool_registry: Arc<RwLock<Option<Arc<edge_ai_tools::ToolRegistry>>>>,
}

impl SessionManager {
    /// Create a new session manager with persistent storage.
    pub fn new() -> Result<Self> {
        Self::with_path("data/sessions.redb")
    }

    /// Create a new session manager with in-memory storage.
    /// This does not open any database files, avoiding lock conflicts.
    pub fn memory() -> Self {
        eprintln!("[DEBUG SessionManager] Creating memory SessionManager (fallback mode)");
        let store = SessionStore::open(":memory:")
            .unwrap_or_else(|e| {
                // Fallback to temp file if :memory: fails
                eprintln!("[DEBUG SessionManager] :memory: failed, using temp file: {}", e);
                let temp_path = std::env::temp_dir().join(
                    format!("sessions_fallback_{}.redb", uuid::Uuid::new_v4())
                );
                eprintln!("[DEBUG SessionManager] Fallback path: {:?}", temp_path);
                SessionStore::open(&temp_path)
                    .expect("Failed to create fallback session store")
            });
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            session_messages: Arc::new(RwLock::new(HashMap::new())),
            store,
            default_config: AgentConfig::default(),
            default_llm_backend: Arc::new(RwLock::new(None)),
            tool_registry: Arc::new(RwLock::new(None)),
        }
    }

    /// Create a new session manager with a custom database path.
    pub fn with_path(path: impl AsRef<std::path::Path>) -> Result<Self> {
        // Create or open the database
        let store = SessionStore::open(path)
            .map_err(|e| AgentError::Storage(format!("Failed to open session store: {}", e)))?;

        let manager = Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            session_messages: Arc::new(RwLock::new(HashMap::new())),
            store,
            default_config: AgentConfig::default(),
            default_llm_backend: Arc::new(RwLock::new(None)),
            tool_registry: Arc::new(RwLock::new(None)),
        };

        // Restore sessions from database on startup
        // Note: This requires LLM backend to be configured later via set_llm_backend
        let session_ids = manager.store.list_sessions()
            .unwrap_or_else(|e| {
                eprintln!("Failed to list sessions from database: {}", e);
                Vec::new()
            });

        if !session_ids.is_empty() {
            eprintln!("SessionManager: Found {} persisted sessions, restoring...", session_ids.len());

            // Use a blocking task since we're in a sync context
            let rt = tokio::runtime::Handle::try_current();
            if rt.is_ok() {
                // We're in a tokio runtime, restore sessions asynchronously
                // For now, just log that sessions will be restored lazily
                eprintln!("SessionManager: Sessions will be restored on first access (lazy restoration)");
            } else {
                eprintln!("SessionManager: No tokio runtime available, sessions will be restored lazily");
            }
        }

        eprintln!("SessionManager initialized with persistent storage");

        Ok(manager)
    }

    /// Save a session ID to persistent storage.
    fn save_session_id(&self, session_id: &str) -> Result<()> {
        self.store.save_session_id(session_id)
            .map_err(|e| AgentError::Storage(format!("Failed to save session: {}", e)))
    }

    /// Delete a session from persistent storage.
    fn delete_session_id(&self, session_id: &str) -> Result<()> {
        eprintln!("[DEBUG] delete_session_id called for: {}", session_id);
        let result = self.store.delete_session(session_id);
        eprintln!("[DEBUG] delete_session result: {:?}", result);
        result.map_err(|e| AgentError::Storage(format!("Failed to delete session: {}", e)))
    }

    /// Save message history for a session to persistent storage.
    fn save_history(&self, session_id: &str, messages: &[AgentMessage]) -> Result<()> {
        // Convert AgentMessage to SessionMessage
        let session_messages: Vec<edge_ai_storage::SessionMessage> = messages
            .iter()
            .map(|msg| {
                // Convert ToolCall to serde_json::Value, including result field
                let tool_calls = msg.tool_calls.as_ref().map(|calls| {
                    calls.iter()
                        .map(|call| {
                            let mut obj = serde_json::json!({
                                "name": call.name,
                                "id": call.id,
                                "arguments": call.arguments,
                            });
                            // Add result field if present
                            if let Some(ref result) = call.result {
                                if let Some(obj_map) = obj.as_object_mut() {
                                    obj_map.insert("result".to_string(), result.clone());
                                }
                            }
                            obj
                        })
                        .collect()
                });

                edge_ai_storage::SessionMessage {
                    role: msg.role.clone(),
                    content: msg.content.clone(),
                    tool_calls,
                    tool_call_id: msg.tool_call_id.clone(),
                    tool_call_name: msg.tool_call_name.clone(),
                    thinking: msg.thinking.clone(),
                    timestamp: msg.timestamp,
                }
            })
            .collect();

        self.store.save_history(session_id, &session_messages)
            .map_err(|e| AgentError::Storage(format!("Failed to save history: {}", e)))
    }

    /// Load message history for a session from persistent storage.
    fn load_history(&self, session_id: &str) -> Result<Vec<AgentMessage>> {
        let session_messages = self.store.load_history(session_id)
            .map_err(|e| AgentError::Storage(format!("Failed to load history: {}", e)))?;

        // Convert SessionMessage back to AgentMessage
        let messages = session_messages
            .into_iter()
            .map(|sm| {
                // Convert serde_json::Value to ToolCall, including result field
                let tool_calls = sm.tool_calls.map(|values| {
                    values
                        .into_iter()
                        .filter_map(|v| {
                            if let (Some(name), Some(id), Some(args)) = (
                                v.get("name").and_then(|n| n.as_str()),
                                v.get("id").and_then(|i| i.as_str()),
                                v.get("arguments"),
                            ) {
                                // Extract result field if present
                                let result = v.get("result").cloned();
                                Some(super::agent::ToolCall {
                                    name: name.to_string(),
                                    id: id.to_string(),
                                    arguments: args.clone(),
                                    result,
                                })
                            } else {
                                None
                            }
                        })
                        .collect()
                });

                AgentMessage {
                    role: sm.role,
                    content: sm.content,
                    tool_calls,
                    tool_call_id: sm.tool_call_id,
                    tool_call_name: sm.tool_call_name,
                    thinking: sm.thinking,
                    timestamp: sm.timestamp,
                }
            })
            .collect();

        Ok(messages)
    }

    /// Set the default agent config.
    pub fn with_config(mut self, config: AgentConfig) -> Self {
        self.default_config = config;
        self
    }

    /// Set the default LLM backend for all new and existing sessions.
    pub async fn set_llm_backend(&self, backend: LlmBackend) -> Result<()> {
        // Store as default for new sessions
        *self.default_llm_backend.write().await = Some(backend.clone());

        // Configure LLM for all existing sessions
        let sessions = self.sessions.read().await;
        for agent in sessions.values() {
            let _ = agent.configure_llm(backend.clone()).await;
        }

        Ok(())
    }

    /// Configure LLM using the LlmBackendInstanceManager.
    /// This fetches the active backend from the instance manager and configures it for all sessions.
    pub async fn configure_llm_from_instance_manager(&self) -> Result<()> {
        use edge_ai_storage::LlmBackendType;

        // Get the instance manager
        let manager = get_instance_manager().map_err(|e| {
            AgentError::Llm(format!("Failed to get instance manager: {}", e))
        })?;

        // Get the active backend instance
        let active_instance = manager.get_active_instance().ok_or_else(|| {
            AgentError::Llm("No active LLM backend configured".to_string())
        })?;

        // Convert to LlmBackend enum based on backend type
        let backend = match active_instance.backend_type {
            LlmBackendType::Ollama => LlmBackend::Ollama {
                endpoint: active_instance.endpoint.clone().unwrap_or_else(|| "http://localhost:11434".to_string()),
                model: active_instance.model.clone(),
            },
            LlmBackendType::OpenAi => LlmBackend::OpenAi {
                api_key: active_instance.api_key.clone().unwrap_or_default(),
                endpoint: active_instance.endpoint.clone().unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
                model: active_instance.model.clone(),
            },
            LlmBackendType::Anthropic => LlmBackend::OpenAi {
                api_key: active_instance.api_key.clone().unwrap_or_default(),
                endpoint: active_instance.endpoint.clone().unwrap_or_else(|| "https://api.anthropic.com/v1".to_string()),
                model: active_instance.model.clone(),
            },
            LlmBackendType::Google => LlmBackend::OpenAi {
                api_key: active_instance.api_key.clone().unwrap_or_default(),
                endpoint: active_instance.endpoint.clone().unwrap_or_else(|| "https://generativelanguage.googleapis.com/v1beta".to_string()),
                model: active_instance.model.clone(),
            },
            LlmBackendType::XAi => LlmBackend::OpenAi {
                api_key: active_instance.api_key.clone().unwrap_or_default(),
                endpoint: active_instance.endpoint.clone().unwrap_or_else(|| "https://api.x.ai/v1".to_string()),
                model: active_instance.model.clone(),
            },
        };

        // Configure using the standard method
        self.set_llm_backend(backend).await
    }

    /// Set the tool registry for all new sessions.
    pub async fn set_tool_registry(&self, registry: Arc<edge_ai_tools::ToolRegistry>) {
        *self.tool_registry.write().await = Some(registry);
    }

    /// Create a new session.
    pub async fn create_session(&self) -> Result<String> {
        let session_id = Uuid::new_v4().to_string();

        // Use tool registry if set, otherwise create default mock tools
        let tool_registry = self.tool_registry.read().await.clone();
        let agent = if let Some(tools) = tool_registry {
            Arc::new(Agent::with_tools(self.default_config.clone(), session_id.clone(), tools))
        } else {
            Arc::new(Agent::new(self.default_config.clone(), session_id.clone()))
        };

        // Configure LLM if a default backend is set
        let llm_backend = self.default_llm_backend.read().await.clone();
        if let Some(backend) = llm_backend {
            let _ = agent.configure_llm(backend).await;
        }

        self.sessions.write().await.insert(session_id.clone(), agent);
        self.session_messages.write().await.insert(session_id.clone(), Vec::new());

        // Save session ID to database
        self.save_session_id(&session_id)?;

        Ok(session_id)
    }

    /// Get an existing session.
    /// If the session is not in memory but exists in the database, it will be restored.
    pub async fn get_session(&self, session_id: &str) -> Result<Arc<Agent>> {
        // First check if session is in memory
        if let Some(agent) = self.sessions.read().await.get(session_id).cloned() {
            return Ok(agent);
        }

        // Session not in memory, check if it exists in database
        let in_db = self.store.session_exists(session_id)
            .map_err(|e| AgentError::Storage(format!("Failed to check session existence: {}", e)))?;

        if in_db {
            // Session exists in database, restore it
            self.restore_session_from_db(session_id).await
        } else {
            Err(AgentError::NotFound(format!("Session: {}", session_id)))
        }
    }

    /// Restore a session from the database into memory.
    async fn restore_session_from_db(&self, session_id: &str) -> Result<Arc<Agent>> {
        eprintln!("Restoring session {} from database", session_id);

        // Use tool registry if set, otherwise create default agent
        let tool_registry = self.tool_registry.read().await.clone();
        let agent = if let Some(tools) = tool_registry {
            Arc::new(Agent::with_tools(self.default_config.clone(), session_id.to_string(), tools))
        } else {
            Arc::new(Agent::new(self.default_config.clone(), session_id.to_string()))
        };

        // Configure LLM if a default backend is set
        let llm_backend = self.default_llm_backend.read().await.clone();
        if let Some(backend) = llm_backend {
            let _ = agent.configure_llm(backend).await;
        }

        // Load message history from database
        let history = self.load_history(session_id)?;

        // Restore history to agent's memory
        if !history.is_empty() {
            agent.restore_history(history.clone()).await;
            eprintln!("Restored {} messages for session {}", history.len(), session_id);
        }

        // Save to in-memory cache
        self.sessions.write().await.insert(session_id.to_string(), agent.clone());
        self.session_messages.write().await.insert(session_id.to_string(), history);

        Ok(agent)
    }

    /// Get or create a session (never fails).
    /// If the session exists, returns it. If not, creates a new one.
    pub async fn get_or_create_session(&self, session_id: Option<String>) -> String {
        match session_id {
            Some(id) => {
                // Check if session exists in memory
                if self.sessions.read().await.contains_key(&id) {
                    // Session exists, return it
                    id
                } else {
                    // Session doesn't exist, check if it's in the database
                    let in_db = self.store.session_exists(&id).unwrap_or(false);

                    if in_db {
                        // Session is in database but not in memory (server restart)
                        // Recreate the agent
                        eprintln!("Restoring session {} from database", id);

                        // Use tool registry if set, otherwise create default mock tools
                        let tool_registry = self.tool_registry.read().await.clone();
                        let agent = if let Some(tools) = tool_registry {
                            Arc::new(Agent::with_tools(self.default_config.clone(), id.clone(), tools))
                        } else {
                            Arc::new(Agent::new(self.default_config.clone(), id.clone()))
                        };

                        // Configure LLM if a default backend is set
                        let llm_backend = self.default_llm_backend.read().await.clone();
                        if let Some(backend) = llm_backend {
                            let _ = agent.configure_llm(backend).await;
                        }

                        // Load message history from database
                        let history = self.load_history(&id).unwrap_or_else(|e| {
                            eprintln!("Failed to load history for session {}: {}", id, e);
                            Vec::new()
                        });

                        // Restore history to agent's memory
                        if !history.is_empty() {
                            agent.restore_history(history.clone()).await;
                            eprintln!("Restored {} messages for session {}", history.len(), id);
                        }

                        // Save to in-memory cache
                        self.sessions.write().await.insert(id.clone(), agent);
                        self.session_messages.write().await.insert(id.clone(), history);

                        id
                    } else {
                        // Create a new session
                        eprintln!("Session {} not found in database, creating new session", id);
                        let new_id = self.create_session().await.unwrap_or_else(|_| {
                            Uuid::new_v4().to_string()
                        });
                        new_id
                    }
                }
            }
            None => {
                // No session ID provided, create a new session
                self.create_session().await.unwrap_or_else(|_| {
                    Uuid::new_v4().to_string()
                })
            }
        }
    }

    /// Remove a session.
    /// Removes from both memory and database, even if not currently loaded in memory.
    pub async fn remove_session(&self, session_id: &str) -> Result<()> {
        eprintln!("[DEBUG] remove_session called for: {}", session_id);

        // Check if session exists (in memory or database)
        let in_memory = self.sessions.read().await.contains_key(session_id);
        eprintln!("[DEBUG] in_memory: {}", in_memory);

        let in_db = self.store.session_exists(session_id)
            .map_err(|e| AgentError::Storage(format!("Failed to check session: {}", e)))?;
        eprintln!("[DEBUG] in_db: {}", in_db);

        if !in_memory && !in_db {
            eprintln!("[DEBUG] Session not found in memory or database");
            return Err(AgentError::NotFound(format!("Session: {}", session_id)));
        }

        // Remove from memory (if present)
        self.sessions.write().await.remove(session_id);
        self.session_messages.write().await.remove(session_id);

        // Remove from database
        eprintln!("[DEBUG] Deleting from database...");
        self.delete_session_id(session_id)?;
        eprintln!("[DEBUG] Session deleted successfully");

        Ok(())
    }

    /// Update session title.
    pub async fn update_session_title(&self, session_id: &str, title: Option<String>) -> Result<()> {
        // Check if session exists (in memory or database)
        let in_memory = self.sessions.read().await.contains_key(session_id);
        let in_db = self.store.session_exists(session_id)
            .map_err(|e| AgentError::Storage(format!("Failed to check session: {}", e)))?;

        if !in_memory && !in_db {
            return Err(AgentError::NotFound(format!("Session: {}", session_id)));
        }

        // Save the metadata
        let metadata = edge_ai_storage::SessionMetadata {
            title: title.filter(|t| !t.trim().is_empty()), // Filter out empty titles
        };

        self.store.save_session_metadata(session_id, &metadata)
            .map_err(|e| AgentError::Storage(format!("Failed to save session metadata: {}", e)))?;

        Ok(())
    }

    /// Get session title.
    pub async fn get_session_title(&self, session_id: &str) -> Result<Option<String>> {
        self.store.get_session_metadata(session_id)
            .map_err(|e| AgentError::Storage(format!("Failed to get session metadata: {}", e)))
            .map(|meta| meta.title)
    }

    /// List all active sessions with their metadata.
/// Returns sessions from both memory and database (for persistence after restart).
pub async fn list_sessions_with_info(&self) -> Vec<SessionInfo> {
    let mut infos = Vec::new();

    // Get all session IDs from database (including those not in memory)
    let db_session_ids = match self.store.list_sessions() {
        Ok(ids) => ids,
        Err(e) => {
            eprintln!("Failed to list sessions from database: {}", e);
            // Fallback to memory-only sessions
            self.sessions.read().await.keys().cloned().collect()
        }
    };

    for session_id in db_session_ids {
        // Get timestamp from store (in seconds)
        let timestamp_seconds = self.store.get_session_timestamp(&session_id).ok()
            .and_then(|r| r);

        // Convert seconds to milliseconds for frontend compatibility
        let timestamp_ms = timestamp_seconds
            .unwrap_or_else(|| chrono::Utc::now().timestamp())
            * 1000;

        // Try to get messages from memory first, then from database
        let message_count = if let Some(msgs) = self.session_messages.read().await.get(&session_id) {
            msgs.len() as u32
        } else {
            // Load from database to get message count
            self.load_history(&session_id)
                .map(|msgs| msgs.len() as u32)
                .unwrap_or(0)
        };

        // Get preview from database (first user message)
        let preview = self.load_history(&session_id)
            .ok()
            .and_then(|msgs| {
                msgs.iter()
                    .find(|m| m.role == "user")
                    .map(|m| {
                        // Truncate content to 50 chars
                        let content = m.content.trim();
                        if content.len() > 50 {
                            format!("{}...", &content[..50])
                        } else {
                            content.to_string()
                        }
                    })
            });

        // Get title from metadata
        let title = self.store.get_session_metadata(&session_id)
            .ok()
            .and_then(|meta| meta.title)
            .filter(|t| !t.is_empty());

        infos.push(SessionInfo {
            session_id: session_id.clone(),
            created_at: timestamp_ms,
            message_count,
            title,
            preview,
        });
    }

    // Sort by created_at descending (newest first)
    infos.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    infos
}

    /// List all active sessions (IDs only).
    /// Returns sessions from both memory and database (for persistence after restart).
    pub async fn list_sessions(&self) -> Vec<String> {
        // Get all session IDs from database (including those not in memory)
        match self.store.list_sessions() {
            Ok(ids) => ids,
            Err(e) => {
                eprintln!("Failed to list sessions from database: {}", e);
                // Fallback to memory-only sessions
                self.sessions.read().await.keys().cloned().collect()
            }
        }
    }

    /// Get the number of active sessions.
    pub async fn session_count(&self) -> usize {
        self.sessions.read().await.len()
    }

    /// Process a message in a session.
    pub async fn process_message(
        &self,
        session_id: &str,
        message: &str,
    ) -> Result<super::agent::AgentResponse> {
        let agent = self.get_session(session_id).await?;
        let response = agent.process(message).await?;

        // Update message history
        let messages = agent.history().await;
        self.session_messages.write().await.insert(session_id.to_string(), messages.clone());

        // Persist history to database
        if let Err(e) = self.save_history(session_id, &messages) {
            eprintln!("Failed to save history for session {}: {}", session_id, e);
        }

        Ok(response)
    }

    /// Process a message in a session with streaming response.
    pub async fn process_message_stream(
        &self,
        session_id: &str,
        message: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = String> + Send>>> {
        let agent = self.get_session(session_id).await?;
        agent.process_stream(message).await
    }

    /// Process a message in a session with event streaming (rich response).
    pub async fn process_message_events(
        &self,
        session_id: &str,
        message: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> {
        let agent = self.get_session(session_id).await?;
        agent.process_stream_events(message).await
    }

    /// Get conversation history for a session.
    /// If session doesn't exist, returns empty history (soft fail for dirty data).
    pub async fn get_history(&self, session_id: &str) -> Result<Vec<AgentMessage>> {
        // Try to get the session - this will restore from DB if needed
        match self.get_session(session_id).await {
            Ok(agent) => Ok(agent.history().await),
            Err(AgentError::NotFound(_)) => {
                // Session not found in memory or DB - might be dirty data
                // Return empty history instead of error
                eprintln!("Session {} not found, returning empty history (may be dirty data)", session_id);
                Ok(Vec::new())
            }
            Err(e) => Err(e),
        }
    }

    /// Clear conversation history for a session.
    pub async fn clear_history(&self, session_id: &str) -> Result<()> {
        let agent = self.get_session(session_id).await?;
        agent.clear_history().await;

        // Update in-memory cache
        self.session_messages.write().await.insert(session_id.to_string(), Vec::new());

        // Clear persisted history
        if let Err(e) = self.save_history(session_id, &[]) {
            eprintln!("Failed to clear history for session {}: {}", session_id, e);
        }

        Ok(())
    }

    /// Persist the current history for a session to the database.
    pub async fn persist_history(&self, session_id: &str) -> Result<()> {
        eprintln!("[DEBUG] persist_history called for session: {}", session_id);
        let messages = if let Ok(agent) = self.get_session(session_id).await {
            eprintln!("[DEBUG] Got agent, fetching history...");
            agent.history().await
        } else if let Some(cached) = self.session_messages.read().await.get(session_id) {
            eprintln!("[DEBUG] Got cached messages, count: {}", cached.len());
            cached.clone()
        } else {
            eprintln!("[DEBUG] No agent or cached messages found for session");
            return Ok(());
        };

        eprintln!("[DEBUG] Saving {} messages for session {}", messages.len(), session_id);
        if let Err(e) = self.save_history(session_id, &messages) {
            eprintln!("Failed to persist history for session {}: {}", session_id, e);
        }

        Ok(())
    }

    /// Validate a session - check if it actually exists in the database.
    pub fn validate_session(&self, session_id: &str) -> bool {
        self.store.session_exists(session_id).unwrap_or(false)
    }

    /// Clean up invalid/empty sessions.
    /// Removes sessions that either:
    /// 1. Have corrupted history (can't load), or
    /// 2. Have no messages (empty sessions older than 1 hour)
    /// Returns the number of sessions cleaned up.
    pub async fn cleanup_invalid_sessions(&self) -> usize {
        let db_session_ids = match self.store.list_sessions() {
            Ok(ids) => ids,
            Err(_) => return 0,
        };

        let now = chrono::Utc::now().timestamp();
        let empty_session_threshold = 3600; // 1 hour in seconds

        let mut invalid_count = 0;
        for session_id in db_session_ids {
            // Check if session has valid history
            match self.load_history(&session_id) {
                Ok(messages) => {
                    // Session loaded successfully
                    if messages.is_empty() {
                        // Empty session - check if it's old enough to delete
                        if let Ok(Some(timestamp)) = self.store.get_session_timestamp(&session_id) {
                            let age_seconds = now - timestamp;
                            if age_seconds > empty_session_threshold {
                                eprintln!("Found empty session {} (age: {}s), removing", session_id, age_seconds);
                                let _ = self.delete_session_id(&session_id);
                                invalid_count += 1;
                            } else {
                                eprintln!("Skipping recent empty session {} (age: {}s)", session_id, age_seconds);
                            }
                        }
                    }
                    // Session has messages, keep it
                }
                Err(e) => {
                    // Failed to load history - corrupted data
                    eprintln!("Found corrupted session {} (error: {}), removing", session_id, e);
                    let _ = self.delete_session_id(&session_id);
                    invalid_count += 1;
                }
            }
        }

        invalid_count
    }

    /// Clean up inactive sessions (older than specified seconds).
    pub async fn cleanup_inactive(&self, max_age_seconds: i64) -> usize {
        let now = chrono::Utc::now().timestamp();
        let mut sessions = self.sessions.write().await;
        let mut to_remove = Vec::new();

        for (id, agent) in sessions.iter() {
            let state = agent.state().await;
            if now - state.last_activity > max_age_seconds {
                to_remove.push(id.clone());
            }
        }

        for id in &to_remove {
            sessions.remove(id);
            self.session_messages.write().await.remove(id);
            let _ = self.delete_session_id(id);
        }

        to_remove.len()
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new().unwrap_or_else(|e| {
            eprintln!("Failed to create SessionManager: {}, using in-memory only", e);
            Self {
                sessions: Arc::new(RwLock::new(HashMap::new())),
                session_messages: Arc::new(RwLock::new(HashMap::new())),
                store: SessionStore::open(":memory:").unwrap_or_else(|_| {
                    // Fallback to temp file if :memory: fails
                    let temp_path = std::env::temp_dir().join(format!("sessions_fallback_{}.redb", uuid::Uuid::new_v4()));
                    SessionStore::open(&temp_path).expect("Failed to create fallback session store")
                }),
                default_config: AgentConfig::default(),
                default_llm_backend: Arc::new(RwLock::new(None)),
                tool_registry: Arc::new(RwLock::new(None)),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a temporary SessionManager for tests
    fn create_temp_manager() -> SessionManager {
        let temp_dir = std::env::temp_dir();
        let test_path = temp_dir.join(format!("neotalk_test_{}", uuid::Uuid::new_v4()));
        SessionManager::with_path(test_path).unwrap()
    }

    #[tokio::test]
    async fn test_session_manager_creation() {
        let manager = create_temp_manager();
        assert_eq!(manager.session_count().await, 0);
    }

    #[tokio::test]
    async fn test_create_session() {
        let manager = create_temp_manager();
        let session_id = manager.create_session().await.unwrap();

        assert_eq!(manager.session_count().await, 1);
        assert!(manager.get_session(&session_id).await.is_ok());
    }

    #[tokio::test]
    async fn test_get_or_create_session() {
        let manager = create_temp_manager();

        // Create a session with an ID that doesn't exist - should create new
        let new_id = manager.get_or_create_session(Some("non-existent-id".to_string())).await;
        assert!(manager.get_session(&new_id).await.is_ok());

        // Get existing session
        let existing_id = manager.get_or_create_session(Some(new_id.clone())).await;
        assert_eq!(existing_id, new_id);
    }

    #[tokio::test]
    async fn test_remove_session() {
        let manager = create_temp_manager();
        let session_id = manager.create_session().await.unwrap();

        manager.remove_session(&session_id).await.unwrap();
        assert_eq!(manager.session_count().await, 0);
        assert!(manager.get_session(&session_id).await.is_err());
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let manager = create_temp_manager();

        manager.create_session().await.unwrap();
        manager.create_session().await.unwrap();

        let sessions = manager.list_sessions().await;
        assert_eq!(sessions.len(), 2);
    }

    #[tokio::test]
    async fn test_process_message() {
        let manager = create_temp_manager();
        let session_id = manager.create_session().await.unwrap();

        let response = manager.process_message(&session_id, "列出设备").await.unwrap();
        assert!(!response.message.content.is_empty());
    }

    #[tokio::test]
    async fn test_get_history() {
        let manager = create_temp_manager();
        let session_id = manager.create_session().await.unwrap();

        manager.process_message(&session_id, "列出设备").await.unwrap();

        let history = manager.get_history(&session_id).await.unwrap();
        assert!(history.len() >= 2); // user + assistant
    }

    #[tokio::test]
    async fn test_clear_history() {
        let manager = create_temp_manager();
        let session_id = manager.create_session().await.unwrap();

        manager.process_message(&session_id, "列出设备").await.unwrap();
        manager.clear_history(&session_id).await.unwrap();

        let history = manager.get_history(&session_id).await.unwrap();
        assert_eq!(history.len(), 0);
    }

    #[tokio::test]
    async fn test_cleanup_inactive() {
        let manager = create_temp_manager();

        let _session_id = manager.create_session().await.unwrap();

        // Cleanup sessions older than 1 second (shouldn't remove active session)
        let removed = manager.cleanup_inactive(1).await;
        assert_eq!(removed, 0);
        assert_eq!(manager.session_count().await, 1);
    }

    #[tokio::test]
    async fn test_multiple_sessions_independent() {
        let manager = create_temp_manager();

        let session1 = manager.create_session().await.unwrap();
        let session2 = manager.create_session().await.unwrap();

        // Send different messages
        manager.process_message(&session1, "列出设备").await.unwrap();
        manager.process_message(&session2, "列出规则").await.unwrap();

        // Check histories are independent
        let history1 = manager.get_history(&session1).await.unwrap();
        let history2 = manager.get_history(&session2).await.unwrap();

        assert!(history1.len() >= 2);
        assert!(history2.len() >= 2);

        // Contents should be different
        let last_msg1 = &history1[history1.len() - 1];
        let last_msg2 = &history2[history2.len() - 1];

        assert_ne!(last_msg1.content, last_msg2.content);
    }
}
