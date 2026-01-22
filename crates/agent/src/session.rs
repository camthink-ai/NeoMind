//! Session manager for multiple agent sessions with persistence.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures::Stream;
use tokio::sync::RwLock;
use uuid::Uuid;

use edge_ai_storage::SessionStore;

use super::agent::{Agent, AgentConfig, AgentEvent, AgentMessage, LlmBackend};
use super::error::{AgentError, Result};

// Re-export instance manager for convenience
pub use edge_ai_llm::instance_manager::{
    BackendTypeDefinition, LlmBackendInstanceManager, get_instance_manager,
};

use edge_ai_storage::LlmBackendInstance;

/// Convert an LlmBackendInstance to LlmBackend enum for agent configuration.
fn instance_to_llm_backend(instance: &LlmBackendInstance) -> Result<LlmBackend> {
    use edge_ai_storage::LlmBackendType;

    Ok(match instance.backend_type {
        LlmBackendType::Ollama => LlmBackend::Ollama {
            endpoint: instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "http://localhost:11434".to_string()),
            model: instance.model.clone(),
        },
        LlmBackendType::OpenAi => LlmBackend::OpenAi {
            api_key: instance.api_key.clone().unwrap_or_default(),
            endpoint: instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
            model: instance.model.clone(),
        },
        LlmBackendType::Anthropic => LlmBackend::OpenAi {
            api_key: instance.api_key.clone().unwrap_or_default(),
            endpoint: instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "https://api.anthropic.com/v1".to_string()),
            model: instance.model.clone(),
        },
        LlmBackendType::Google => LlmBackend::OpenAi {
            api_key: instance.api_key.clone().unwrap_or_default(),
            endpoint: instance.endpoint.clone().unwrap_or_else(|| {
                "https://generativelanguage.googleapis.com/v1beta".to_string()
            }),
            model: instance.model.clone(),
        },
        LlmBackendType::XAi => LlmBackend::OpenAi {
            api_key: instance.api_key.clone().unwrap_or_default(),
            endpoint: instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "https://api.x.ai/v1".to_string()),
            model: instance.model.clone(),
        },
    })
}

/// Configuration for session cleanup
#[derive(Debug, Clone)]
pub struct SessionCleanupConfig {
    /// Enable automatic cleanup
    pub enabled: bool,
    /// Maximum session age in seconds before cleanup
    pub max_age_seconds: i64,
    /// Cleanup interval in seconds
    pub cleanup_interval_seconds: u64,
    /// Maximum empty session age in seconds before cleanup
    pub max_empty_age_seconds: i64,
}

impl Default for SessionCleanupConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_age_seconds: 7 * 24 * 3600, // 7 days
            cleanup_interval_seconds: 3600,  // 1 hour
            max_empty_age_seconds: 24 * 3600, // 1 day for empty sessions
        }
    }
}

impl SessionCleanupConfig {
    /// Create a new cleanup config.
    pub fn new(max_age_seconds: i64, cleanup_interval_seconds: u64) -> Self {
        Self {
            enabled: true,
            max_age_seconds,
            cleanup_interval_seconds,
            max_empty_age_seconds: 24 * 3600,
        }
    }

    /// Get the cleanup interval as Duration.
    pub fn cleanup_interval(&self) -> Duration {
        Duration::from_secs(self.cleanup_interval_seconds)
    }

    /// Disable automatic cleanup.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }
}

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
    /// Session cleanup configuration
    cleanup_config: SessionCleanupConfig,
    /// Whether cleanup task is running
    cleanup_running: Arc<RwLock<bool>>,
}

impl SessionManager {
    /// Create a new session manager with persistent storage.
    pub fn new() -> Result<Self> {
        Self::with_path("data/sessions.redb")
    }

    /// Create a new session manager with in-memory storage.
    /// This does not open any database files, avoiding lock conflicts.
    pub fn memory() -> Self {
        tracing::debug!(message = "Creating memory SessionManager (fallback mode)");
        let store = SessionStore::open(":memory:").unwrap_or_else(|e| {
            // Fallback to temp file if :memory: fails
            tracing::error!(error = %e, ":memory: failed, using temp file");
            let temp_path = std::env::temp_dir()
                .join(format!("sessions_fallback_{}.redb", uuid::Uuid::new_v4()));
            tracing::debug!(path = ?temp_path, "Using fallback path for session store");
            SessionStore::open(&temp_path).expect("Failed to create fallback session store")
        });
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            session_messages: Arc::new(RwLock::new(HashMap::new())),
            store,
            default_config: AgentConfig::default(),
            default_llm_backend: Arc::new(RwLock::new(None)),
            tool_registry: Arc::new(RwLock::new(None)),
            cleanup_config: SessionCleanupConfig::default(),
            cleanup_running: Arc::new(RwLock::new(false)),
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
            cleanup_config: SessionCleanupConfig::default(),
            cleanup_running: Arc::new(RwLock::new(false)),
        };

        // Restore sessions from database on startup
        // Note: This requires LLM backend to be configured later via set_llm_backend
        let session_ids = manager.store.list_sessions().unwrap_or_else(|e| {
            tracing::error!(error = %e, message = "Failed to list sessions from database");
            Vec::new()
        });

        if !session_ids.is_empty() {
            tracing::info!(count = session_ids.len(), "Found persisted sessions, will restore lazily");
        }

        tracing::info!(message = "SessionManager initialized with persistent storage");

        Ok(manager)
    }

    /// Save a session ID to persistent storage.
    fn save_session_id(&self, session_id: &str) -> Result<()> {
        self.store
            .save_session_id(session_id)
            .map_err(|e| AgentError::Storage(format!("Failed to save session: {}", e)))
    }

    /// Delete a session from persistent storage.
    fn delete_session_id(&self, session_id: &str) -> Result<()> {
        tracing::debug!(" delete_session_id called for: {}", session_id);
        let result = self.store.delete_session(session_id);
        tracing::debug!(" delete_session result: {:?}", result);
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
                    calls
                        .iter()
                        .map(|call| {
                            let mut obj = serde_json::json!({
                                "name": call.name,
                                "id": call.id,
                                "arguments": call.arguments,
                            });
                            // Add result field if present
                            if let Some(ref result) = call.result
                                && let Some(obj_map) = obj.as_object_mut() {
                                    obj_map.insert("result".to_string(), result.clone());
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

        self.store
            .save_history(session_id, &session_messages)
            .map_err(|e| AgentError::Storage(format!("Failed to save history: {}", e)))
    }

    /// Load message history for a session from persistent storage.
    fn load_history(&self, session_id: &str) -> Result<Vec<AgentMessage>> {
        let session_messages = self
            .store
            .load_history(session_id)
            .map_err(|e| AgentError::Storage(format!("Failed to load history: {}", e)))?;

        // Debug: Log loaded messages
        tracing::debug!(" Loaded {} messages from DB for session {}", session_messages.len(), session_id);
        for (i, sm) in session_messages.iter().enumerate() {
            if sm.role == "assistant" {
                tracing::debug!(" Message {}: role={}, content_len={}, has_thinking={}, tool_calls_count={}",
                    i, sm.role, sm.content.len(), sm.thinking.is_some(),
                    sm.tool_calls.as_ref().map_or(0, |c| c.len()));
            }
        }

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
        

        // Get the instance manager
        let manager = get_instance_manager()
            .map_err(|e| AgentError::Llm(format!("Failed to get instance manager: {}", e)))?;

        // Get the active backend instance
        let active_instance = manager
            .get_active_instance()
            .ok_or_else(|| AgentError::Llm("No active LLM backend configured".to_string()))?;

        // Convert to LlmBackend enum based on backend type
        let backend = instance_to_llm_backend(&active_instance)?;

        // Configure using the standard method
        self.set_llm_backend(backend).await
    }

    /// Configure LLM using a specific backend ID from the instance manager.
    /// Returns the LlmBackend for direct agent configuration.
    pub fn get_backend_by_id(backend_id: &str) -> Result<LlmBackend> {
        let manager = get_instance_manager()
            .map_err(|e| AgentError::Llm(format!("Failed to get instance manager: {}", e)))?;

        // Get the instance by ID using the public method
        let instance = manager.get_instance(backend_id)
            .ok_or_else(|| AgentError::Llm(format!("Backend '{}' not found", backend_id)))?;

        instance_to_llm_backend(&instance)
    }

    /// Configure LLM using a specific backend ID from the instance manager.
    /// This configures the specified backend for the current session agent.
    pub async fn configure_agent_by_backend_id(
        &self,
        session_id: &str,
        backend_id: &str,
    ) -> Result<()> {
        let backend = Self::get_backend_by_id(backend_id)?;
        let agent = self.get_session(session_id).await?;
        agent.configure_llm(backend).await
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
            Arc::new(Agent::with_tools(
                self.default_config.clone(),
                session_id.clone(),
                tools,
            ))
        } else {
            Arc::new(Agent::new(self.default_config.clone(), session_id.clone()))
        };

        // Configure LLM if a default backend is set
        let llm_backend = self.default_llm_backend.read().await.clone();
        if let Some(backend) = llm_backend {
            let _ = agent.configure_llm(backend).await;
        }

        self.sessions
            .write()
            .await
            .insert(session_id.clone(), agent);
        self.session_messages
            .write()
            .await
            .insert(session_id.clone(), Vec::new());

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
        let in_db = self.store.session_exists(session_id).map_err(|e| {
            AgentError::Storage(format!("Failed to check session existence: {}", e))
        })?;

        if in_db {
            // Session exists in database, restore it
            self.restore_session_from_db(session_id).await
        } else {
            Err(AgentError::NotFound(format!("Session: {}", session_id)))
        }
    }

    /// Restore a session from the database into memory.
    async fn restore_session_from_db(&self, session_id: &str) -> Result<Arc<Agent>> {
        tracing::info!(session_id = %session_id, message = "Restoring session from database");

        // Use tool registry if set, otherwise create default agent
        let tool_registry = self.tool_registry.read().await.clone();
        let agent = if let Some(tools) = tool_registry {
            Arc::new(Agent::with_tools(
                self.default_config.clone(),
                session_id.to_string(),
                tools,
            ))
        } else {
            Arc::new(Agent::new(
                self.default_config.clone(),
                session_id.to_string(),
            ))
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
            tracing::debug!(session_id = %session_id, count = history.len(), "Restored messages for session");
        }

        // Save to in-memory cache
        self.sessions
            .write()
            .await
            .insert(session_id.to_string(), agent.clone());
        self.session_messages
            .write()
            .await
            .insert(session_id.to_string(), history);

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
                        tracing::info!(session_id = %id, "Restoring session from database");

                        // Use tool registry if set, otherwise create default mock tools
                        let tool_registry = self.tool_registry.read().await.clone();
                        let agent = if let Some(tools) = tool_registry {
                            Arc::new(Agent::with_tools(
                                self.default_config.clone(),
                                id.clone(),
                                tools,
                            ))
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
                            tracing::error!(session_id = %id, error = %e, message = "Failed to load history");
                            Vec::new()
                        });

                        // Restore history to agent's memory
                        if !history.is_empty() {
                            agent.restore_history(history.clone()).await;
                            tracing::debug!(session_id = %id, count = history.len(), "Restored messages for session");
                        }

                        // Save to in-memory cache
                        self.sessions.write().await.insert(id.clone(), agent);
                        self.session_messages
                            .write()
                            .await
                            .insert(id.clone(), history);

                        id
                    } else {
                        // Create a new session
                        tracing::info!(session_id = %id, message = "Session not found in database, creating new session");
                        
                        self
                            .create_session()
                            .await
                            .unwrap_or_else(|_| Uuid::new_v4().to_string())
                    }
                }
            }
            None => {
                // No session ID provided, create a new session
                self.create_session()
                    .await
                    .unwrap_or_else(|_| Uuid::new_v4().to_string())
            }
        }
    }

    /// Remove a session.
    /// Removes from both memory and database, even if not currently loaded in memory.
    pub async fn remove_session(&self, session_id: &str) -> Result<()> {
        tracing::debug!(" remove_session called for: {}", session_id);

        // Check if session exists (in memory or database)
        let in_memory = self.sessions.read().await.contains_key(session_id);
        tracing::debug!(" in_memory: {}", in_memory);

        let in_db = self
            .store
            .session_exists(session_id)
            .map_err(|e| AgentError::Storage(format!("Failed to check session: {}", e)))?;
        tracing::debug!(" in_db: {}", in_db);

        if !in_memory && !in_db {
            tracing::debug!(" Session not found in memory or database");
            return Err(AgentError::NotFound(format!("Session: {}", session_id)));
        }

        // Remove from memory (if present)
        self.sessions.write().await.remove(session_id);
        self.session_messages.write().await.remove(session_id);

        // Remove from database
        tracing::debug!(" Deleting from database...");
        self.delete_session_id(session_id)?;
        tracing::debug!(" Session deleted successfully");

        Ok(())
    }

    /// Update session title.
    pub async fn update_session_title(
        &self,
        session_id: &str,
        title: Option<String>,
    ) -> Result<()> {
        // Check if session exists (in memory or database)
        let in_memory = self.sessions.read().await.contains_key(session_id);
        let in_db = self
            .store
            .session_exists(session_id)
            .map_err(|e| AgentError::Storage(format!("Failed to check session: {}", e)))?;

        if !in_memory && !in_db {
            return Err(AgentError::NotFound(format!("Session: {}", session_id)));
        }

        // Save the metadata
        let metadata = edge_ai_storage::SessionMetadata {
            title: title.filter(|t| !t.trim().is_empty()), // Filter out empty titles
        };

        self.store
            .save_session_metadata(session_id, &metadata)
            .map_err(|e| AgentError::Storage(format!("Failed to save session metadata: {}", e)))?;

        Ok(())
    }

    /// Get session title.
    pub async fn get_session_title(&self, session_id: &str) -> Result<Option<String>> {
        self.store
            .get_session_metadata(session_id)
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
                tracing::error!(error = %e, message = "Failed to list sessions from database");
                // Fallback to memory-only sessions
                self.sessions.read().await.keys().cloned().collect()
            }
        };

        for session_id in db_session_ids {
            // Get timestamp from store (in seconds)
            let timestamp_seconds = self
                .store
                .get_session_timestamp(&session_id)
                .ok()
                .and_then(|r| r);

            // Convert seconds to milliseconds for frontend compatibility
            let timestamp_ms =
                timestamp_seconds.unwrap_or_else(|| chrono::Utc::now().timestamp()) * 1000;

            // Try to get messages from memory first, then from database
            let message_count =
                if let Some(msgs) = self.session_messages.read().await.get(&session_id) {
                    msgs.len() as u32
                } else {
                    // Load from database to get message count
                    self.load_history(&session_id)
                        .map(|msgs| msgs.len() as u32)
                        .unwrap_or(0)
                };

            // Get preview from database (first user message)
            let preview = self.load_history(&session_id).ok().and_then(|msgs| {
                msgs.iter().find(|m| m.role == "user").map(|m| {
                    // Truncate content to 50 chars (using char boundary for Unicode safety)
                    let content = m.content.trim();
                    if content.chars().count() > 50 {
                        format!("{}...", content.chars().take(50).collect::<String>())
                    } else {
                        content.to_string()
                    }
                })
            });

            // Get title from metadata
            let title = self
                .store
                .get_session_metadata(&session_id)
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
                tracing::error!(error = %e, message = "Failed to list sessions from database");
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
        tracing::debug!(session_id = %session_id, message = %message, "SessionManager::process_message");
        let agent = self.get_session(session_id).await?;
        let response = agent.process(message).await?;

        // Update message history
        let messages = agent.history().await;
        self.session_messages
            .write()
            .await
            .insert(session_id.to_string(), messages.clone());

        // Persist history to database
        if let Err(e) = self.save_history(session_id, &messages) {
            tracing::error!(session_id = %session_id, error = %e, message = "Failed to save history");
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

    /// Process a message in a session with optional LLM backend override.
    pub async fn process_message_with_backend(
        &self,
        session_id: &str,
        message: &str,
        backend_id: Option<&str>,
    ) -> Result<super::agent::AgentResponse> {
        // If a specific backend is requested, configure the agent with it
        if let Some(backend) = backend_id {
            let _ = self.configure_agent_by_backend_id(session_id, backend).await;
        }
        self.process_message(session_id, message).await
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

    /// Process a message in a session with event streaming and optional LLM backend override.
    pub async fn process_message_events_with_backend(
        &self,
        session_id: &str,
        message: &str,
        backend_id: Option<&str>,
    ) -> Result<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> {
        // If a specific backend is requested, configure the agent with it
        if let Some(backend) = backend_id {
            let _ = self.configure_agent_by_backend_id(session_id, backend).await;
        }
        self.process_message_events(session_id, message).await
    }

    /// Process a multimodal message with images in a session.
    pub async fn process_message_multimodal(
        &self,
        session_id: &str,
        message: &str,
        images: Vec<String>, // Base64 data URLs
    ) -> Result<super::agent::AgentResponse> {
        tracing::debug!(
            session_id = %session_id,
            image_count = images.len(),
            "SessionManager::process_message_multimodal"
        );
        let agent = self.get_session(session_id).await?;
        let response = agent.process_multimodal(message, images).await?;

        // Update message history
        let messages = agent.history().await;
        self.session_messages
            .write()
            .await
            .insert(session_id.to_string(), messages.clone());

        // Persist history to database
        if let Err(e) = self.save_history(session_id, &messages) {
            tracing::error!(session_id = %session_id, error = %e, message = "Failed to save history");
        }

        Ok(response)
    }

    /// Process a multimodal message with optional LLM backend override.
    pub async fn process_message_multimodal_with_backend(
        &self,
        session_id: &str,
        message: &str,
        images: Vec<String>,
        backend_id: Option<&str>,
    ) -> Result<super::agent::AgentResponse> {
        // If a specific backend is requested, configure the agent with it
        if let Some(backend) = backend_id {
            let _ = self.configure_agent_by_backend_id(session_id, backend).await;
        }
        self.process_message_multimodal(session_id, message, images).await
    }

    /// Process a multimodal message (text + images) with streaming response and optional backend override.
    pub async fn process_message_multimodal_with_backend_stream(
        &self,
        session_id: &str,
        message: &str,
        images: Vec<String>,
        backend_id: Option<&str>,
    ) -> Result<Pin<Box<dyn Stream<Item = super::agent::AgentEvent> + Send>>> {
        // If a specific backend is requested, configure the agent with it
        if let Some(backend) = backend_id {
            let _ = self.configure_agent_by_backend_id(session_id, backend).await;
        }
        self.process_message_multimodal_stream(session_id, message, images).await
    }

    /// Process a multimodal message (text + images) with streaming response.
    pub async fn process_message_multimodal_stream(
        &self,
        session_id: &str,
        message: &str,
        images: Vec<String>,
    ) -> Result<Pin<Box<dyn Stream<Item = super::agent::AgentEvent> + Send>>> {
        let agent = self.get_session(session_id).await?;
        agent.process_multimodal_stream_events(message, images).await
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
                tracing::warn!(session_id = %session_id, "Session not found, returning empty history");
                Ok(Vec::new())
            }
            Err(AgentError::Storage(e)) => {
                // Storage error (database corrupted, etc.) - try to load directly from store
                tracing::error!(session_id = %session_id, error = %e, "Storage error, trying direct load");
                // Try to load history directly from storage as a fallback
                match self.load_history(session_id) {
                    Ok(messages) => {
                        tracing::debug!(count = messages.len(), "Successfully loaded messages via direct load");
                        Ok(messages)
                    }
                    Err(load_err) => {
                        tracing::error!(error = %load_err, "Direct load also failed, returning empty history");
                        Ok(Vec::new())
                    }
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Clear conversation history for a session.
    pub async fn clear_history(&self, session_id: &str) -> Result<()> {
        let agent = self.get_session(session_id).await?;
        agent.clear_history().await;

        // Update in-memory cache
        self.session_messages
            .write()
            .await
            .insert(session_id.to_string(), Vec::new());

        // Clear persisted history using the dedicated clear method
        if let Err(e) = self.store.clear_history(session_id) {
            tracing::error!(session_id = %session_id, error = %e, message = "Failed to clear history");
        }

        Ok(())
    }

    /// Persist the current history for a session to the database.
    pub async fn persist_history(&self, session_id: &str) -> Result<()> {
        let messages = if let Ok(agent) = self.get_session(session_id).await {
            agent.history().await
        } else if let Some(cached) = self.session_messages.read().await.get(session_id) {
            cached.clone()
        } else {
            return Ok(());
        };

        if let Err(e) = self.save_history(session_id, &messages) {
            tracing::debug!(
                session_id = %session_id,
                error = %e,
                "Failed to persist history"
            );
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
                                tracing::debug!(session_id = %session_id, age = age_seconds, "Found empty session, removing");
                                let _ = self.delete_session_id(&session_id);
                                invalid_count += 1;
                            } else {
                                tracing::debug!(session_id = %session_id, age = age_seconds, "Skipping recent empty session");
                            }
                        }
                    }
                    // Session has messages, keep it
                }
                Err(e) => {
                    // Failed to load history - corrupted data
                    tracing::warn!(session_id = %session_id, error = %e, "Found corrupted session, removing");
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

    /// Start the automatic session cleanup task.
    /// This runs in the background and periodically cleans up old sessions.
    pub async fn start_cleanup_task(&self) {
        if !self.cleanup_config.enabled {
            tracing::info!("Session cleanup is disabled");
            return;
        }

        // Check if already running
        {
            let running = self.cleanup_running.read().await;
            if *running {
                tracing::info!("Session cleanup task is already running");
                return;
            }
        }

        // Mark as running
        *self.cleanup_running.write().await = true;

        let sessions = self.sessions.clone();
        let session_messages = self.session_messages.clone();
        let store = self.store.clone();
        let cleanup_config = self.cleanup_config.clone();
        let cleanup_running = self.cleanup_running.clone();
        let cleanup_interval = cleanup_config.cleanup_interval();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(cleanup_interval);
            let mut first_tick = true;

            while *cleanup_running.read().await {
                if first_tick {
                    first_tick = false;
                } else {
                    // Perform cleanup
                    let now = chrono::Utc::now().timestamp();

                    // Clean up sessions from database (both inactive and empty)
                    let db_session_ids = match store.list_sessions() {
                        Ok(ids) => ids,
                        Err(e) => {
                            tracing::error!("Failed to list sessions for cleanup: {}", e);
                            continue;
                        }
                    };

                    let mut removed_count = 0;
                    for session_id in db_session_ids {
                        let should_remove = match store.get_session_timestamp(&session_id) {
                            Ok(Some(timestamp)) => {
                                let age = now - timestamp;

                                // Check if session is empty or too old
                                if age > cleanup_config.max_age_seconds {
                                    tracing::info!(
                                        "Removing old session {} (age: {}s, max: {}s)",
                                        session_id,
                                        age,
                                        cleanup_config.max_age_seconds
                                    );
                                    true
                                } else {
                                    // Check if empty session
                                    match store.load_history(&session_id) {
                                        Ok(messages) if messages.is_empty() => {
                                            if age > cleanup_config.max_empty_age_seconds {
                                                tracing::info!(
                                                    "Removing empty session {} (age: {}s)",
                                                    session_id,
                                                    age
                                                );
                                                true
                                            } else {
                                                false
                                            }
                                        }
                                        _ => false,
                                    }
                                }
                            }
                            Ok(None) => true, // No timestamp = corrupted, remove
                            Err(_) => true,  // Error = corrupted, remove
                        };

                        if should_remove {
                            // Remove from memory
                            sessions.write().await.remove(&session_id);
                            session_messages.write().await.remove(&session_id);

                            // Remove from database
                            if let Err(e) = store.delete_session(&session_id) {
                                tracing::error!("Failed to delete session {}: {}", session_id, e);
                            } else {
                                removed_count += 1;
                            }
                        }
                    }

                    if removed_count > 0 {
                        tracing::info!("Session cleanup completed: removed {} sessions", removed_count);
                    }
                }

                tokio::select! {
                    _ = interval.tick() => {}
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {
                        // Check if we should stop
                        if !*cleanup_running.read().await {
                            break;
                        }
                    }
                }
            }

            tracing::info!("Session cleanup task stopped");
        });

        tracing::info!(
            "Session cleanup task started (interval: {}s, max_age: {}s)",
            cleanup_config.cleanup_interval_seconds,
            cleanup_config.max_age_seconds
        );
    }

    /// Stop the automatic session cleanup task.
    pub async fn stop_cleanup_task(&self) {
        *self.cleanup_running.write().await = false;
        tracing::info!("Session cleanup task stop requested");
    }

    /// Set the cleanup configuration.
    pub async fn set_cleanup_config(&mut self, config: SessionCleanupConfig) {
        self.cleanup_config = config;

        // Restart cleanup task if enabled
        if self.cleanup_config.enabled {
            self.start_cleanup_task().await;
        } else {
            self.stop_cleanup_task().await;
        }
    }

    /// Get the current cleanup configuration.
    pub fn cleanup_config(&self) -> &SessionCleanupConfig {
        &self.cleanup_config
    }

    /// Perform an immediate cleanup pass.
    /// Returns the number of sessions removed.
    pub async fn perform_cleanup(&self) -> usize {
        let mut total_removed = 0;

        // Clean up inactive sessions from memory
        total_removed += self.cleanup_inactive(self.cleanup_config.max_age_seconds).await;

        // Clean up invalid/empty sessions from database
        total_removed += self.cleanup_invalid_sessions().await;

        total_removed
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new().unwrap_or_else(|e| {
            tracing::error!(error = %e, "Failed to create SessionManager, using in-memory only");
            Self {
                sessions: Arc::new(RwLock::new(HashMap::new())),
                session_messages: Arc::new(RwLock::new(HashMap::new())),
                store: SessionStore::open(":memory:").unwrap_or_else(|_| {
                    // Fallback to temp file if :memory: fails
                    let temp_path = std::env::temp_dir()
                        .join(format!("sessions_fallback_{}.redb", uuid::Uuid::new_v4()));
                    SessionStore::open(&temp_path).expect("Failed to create fallback session store")
                }),
                default_config: AgentConfig::default(),
                default_llm_backend: Arc::new(RwLock::new(None)),
                tool_registry: Arc::new(RwLock::new(None)),
                cleanup_config: SessionCleanupConfig::default(),
                cleanup_running: Arc::new(RwLock::new(false)),
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
        let new_id = manager
            .get_or_create_session(Some("non-existent-id".to_string()))
            .await;
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

        let response = manager
            .process_message(&session_id, "列出设备")
            .await
            .unwrap();
        assert!(!response.message.content.is_empty());
    }

    #[tokio::test]
    async fn test_get_history() {
        let manager = create_temp_manager();
        let session_id = manager.create_session().await.unwrap();

        manager
            .process_message(&session_id, "列出设备")
            .await
            .unwrap();

        let history = manager.get_history(&session_id).await.unwrap();
        assert!(history.len() >= 2); // user + assistant
    }

    #[tokio::test]
    async fn test_clear_history() {
        let manager = create_temp_manager();
        let session_id = manager.create_session().await.unwrap();

        manager
            .process_message(&session_id, "列出设备")
            .await
            .unwrap();
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
        manager
            .process_message(&session1, "列出设备")
            .await
            .unwrap();
        manager
            .process_message(&session2, "列出规则")
            .await
            .unwrap();

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
