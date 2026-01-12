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
}

impl SessionManager {
    /// Create a new session manager with persistent storage.
    pub fn new() -> Result<Self> {
        Self::with_path("data/sessions.redb")
    }

    /// Create a new session manager with in-memory storage.
    /// This does not open any database files, avoiding lock conflicts.
    pub fn memory() -> Self {
        let store = SessionStore::open(":memory:")
            .unwrap_or_else(|_| {
                // Fallback to temp file if :memory: fails
                let temp_path = std::env::temp_dir().join(
                    format!("sessions_fallback_{}.redb", uuid::Uuid::new_v4())
                );
                SessionStore::open(&temp_path)
                    .expect("Failed to create fallback session store")
            });
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            session_messages: Arc::new(RwLock::new(HashMap::new())),
            store,
            default_config: AgentConfig::default(),
            default_llm_backend: Arc::new(RwLock::new(None)),
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
        };

        // Note: We don't restore sessions on startup for now
        // The session IDs are persisted but message history is in-memory
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
        self.store.delete_session(session_id)
            .map_err(|e| AgentError::Storage(format!("Failed to delete session: {}", e)))
    }

    /// Save message history for a session to persistent storage.
    fn save_history(&self, session_id: &str, messages: &[AgentMessage]) -> Result<()> {
        // Convert AgentMessage to SessionMessage
        let session_messages: Vec<edge_ai_storage::SessionMessage> = messages
            .iter()
            .map(|msg| {
                // Convert ToolCall to serde_json::Value
                let tool_calls = msg.tool_calls.as_ref().map(|calls| {
                    calls.iter()
                        .map(|call| serde_json::json!({
                            "name": call.name,
                            "id": call.id,
                            "arguments": call.arguments,
                        }))
                        .collect()
                });

                edge_ai_storage::SessionMessage {
                    role: msg.role.clone(),
                    content: msg.content.clone(),
                    tool_calls,
                    tool_call_id: msg.tool_call_id.clone(),
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
                // Convert serde_json::Value to ToolCall
                let tool_calls = sm.tool_calls.map(|values| {
                    values
                        .into_iter()
                        .filter_map(|v| {
                            if let (Some(name), Some(id), Some(args)) = (
                                v.get("name").and_then(|n| n.as_str()),
                                v.get("id").and_then(|i| i.as_str()),
                                v.get("arguments"),
                            ) {
                                Some(super::agent::ToolCall {
                                    name: name.to_string(),
                                    id: id.to_string(),
                                    arguments: args.clone(),
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

    /// Create a new session.
    pub async fn create_session(&self) -> Result<String> {
        let session_id = Uuid::new_v4().to_string();
        let agent = Arc::new(Agent::new(self.default_config.clone(), session_id.clone()));

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
    pub async fn get_session(&self, session_id: &str) -> Result<Arc<Agent>> {
        self.sessions
            .read()
            .await
            .get(session_id)
            .cloned()
            .ok_or_else(|| AgentError::NotFound(format!("Session: {}", session_id)))
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
                        let agent = Arc::new(Agent::new(self.default_config.clone(), id.clone()));

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
    pub async fn remove_session(&self, session_id: &str) -> Result<()> {
        self.sessions
            .write()
            .await
            .remove(session_id)
            .ok_or_else(|| AgentError::NotFound(format!("Session: {}", session_id)))?;

        self.session_messages.write().await.remove(session_id);

        // Remove from database
        self.delete_session_id(session_id)?;

        Ok(())
    }

    /// List all active sessions with their metadata.
    pub async fn list_sessions_with_info(&self) -> Vec<SessionInfo> {
        let session_ids: Vec<String> = self.sessions.read().await.keys().cloned().collect();
        let mut infos = Vec::new();

        for session_id in session_ids {
            // Get timestamp from store
            let timestamp = self.store.get_session_timestamp(&session_id).ok()
                .and_then(|r| r);

            // Get message count and preview from memory
            let messages = self.session_messages.read().await.get(&session_id)
                .map(|msgs| msgs.clone());

            let message_count = messages.as_ref().map(|m| m.len()).unwrap_or(0);

            // Get preview from first user message
            let preview = messages.and_then(|msgs| {
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

            infos.push(SessionInfo {
                session_id: session_id.clone(),
                created_at: timestamp.unwrap_or_else(|| chrono::Utc::now().timestamp()),
                message_count: message_count as u32,
                preview,
            });
        }

        // Sort by created_at descending (newest first)
        infos.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        infos
    }

    /// List all active sessions (IDs only).
    pub async fn list_sessions(&self) -> Vec<String> {
        self.sessions.read().await.keys().cloned().collect()
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
    pub async fn get_history(&self, session_id: &str) -> Result<Vec<AgentMessage>> {
        let agent = self.get_session(session_id).await?;
        Ok(agent.history().await)
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
        let messages = if let Ok(agent) = self.get_session(session_id).await {
            agent.history().await
        } else if let Some(cached) = self.session_messages.read().await.get(session_id) {
            cached.clone()
        } else {
            return Ok(());
        };

        if let Err(e) = self.save_history(session_id, &messages) {
            eprintln!("Failed to persist history for session {}: {}", session_id, e);
        }

        Ok(())
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
