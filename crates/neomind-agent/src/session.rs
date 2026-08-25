//! Session manager for multiple agent sessions with persistence.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use futures::{Stream, StreamExt};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::llm::LlmInterface;

use neomind_storage::SessionStore;

use super::agent::{Agent, AgentConfig, AgentEvent, AgentMessage, LlmBackend};
use super::error::{NeoMindError, Result};

// Re-export instance manager for convenience
pub use crate::llm_backends::{
    get_instance_manager, BackendTypeDefinition, LlmBackendInstanceManager,
};

use neomind_storage::LlmBackendInstance;

/// Optional overrides applied on top of `SessionManager::default_config` when
/// creating a new session. All fields are `Option<...>`; `None` means "inherit
/// the default". Used by callers (e.g. the `chat_session_open` capability, the
/// WS chat auto-create path) that need to customize a session at creation time
/// — most notably to inject a `system_prompt` for the voice-assistant without
/// polluting every user message via `pageContext`.
///
/// Only applied to **newly-created** sessions. Existing sessions (reused via
/// `get_or_create_session` with a known id) keep their original config; the
/// override is silently ignored for them.
#[derive(Debug, Clone, Default)]
pub struct CreateSessionOptions {
    /// Override the agent's system prompt.
    pub system_prompt: Option<String>,
    /// Append to the agent's system prompt (keeps the platform default as
    /// the base). Ignored when `system_prompt` is also set — an explicit
    /// full override wins.
    pub system_prompt_suffix: Option<String>,
    /// Override the LLM sampling temperature.
    pub temperature: Option<f32>,
    /// Override the model identifier (e.g. "qwen3:1.7b").
    pub model: Option<String>,
    /// Enable or disable tool calling.
    pub enable_tools: Option<bool>,
    /// Restrict the session to these tools (empty/absent = all tools).
    /// The user-interaction tools are always kept regardless.
    pub allowed_tools: Vec<String>,
}

/// Convert an LlmBackendInstance to LlmBackend enum for agent configuration.
/// Convert storage BackendCapabilities to core BackendCapabilities
fn convert_capabilities(
    storage_caps: &neomind_storage::BackendCapabilities,
) -> neomind_core::BackendCapabilities {
    neomind_core::BackendCapabilities {
        streaming: storage_caps.supports_streaming,
        multimodal: storage_caps.supports_multimodal,
        function_calling: storage_caps.supports_tools,
        thinking_display: storage_caps.supports_thinking,
        max_context: Some(storage_caps.max_context),
        multiple_models: false,
        modalities: Vec::new(),
        supports_images: storage_caps.supports_multimodal,
        reasoning: neomind_core::ReasoningCapabilities::default(),
    }
}

fn instance_to_llm_backend(instance: &LlmBackendInstance) -> Result<LlmBackend> {
    use neomind_storage::LlmBackendType;

    // Convert capabilities from storage type to core type
    let capabilities = Some(convert_capabilities(&instance.capabilities));

    Ok(match instance.backend_type {
        LlmBackendType::Ollama => LlmBackend::Ollama {
            endpoint: instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "http://localhost:11434".to_string()),
            model: instance.model.clone(),
            capabilities,
        },
        LlmBackendType::LlamaCpp => LlmBackend::LlamaCpp {
            endpoint: instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "http://127.0.0.1:8080".to_string()),
            model: instance.model.clone(),
            capabilities,
        },
        LlmBackendType::OpenAi => LlmBackend::OpenAi {
            api_key: instance.api_key.clone().unwrap_or_default(),
            endpoint: instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
            model: instance.model.clone(),
            capabilities,
        },
        LlmBackendType::Anthropic => LlmBackend::Anthropic {
            api_key: instance.api_key.clone().unwrap_or_default(),
            endpoint: instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "https://api.anthropic.com/v1".to_string()),
            model: instance.model.clone(),
            capabilities,
        },
        LlmBackendType::Google => LlmBackend::Google {
            api_key: instance.api_key.clone().unwrap_or_default(),
            endpoint: instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "https://generativelanguage.googleapis.com/v1beta".to_string()),
            model: instance.model.clone(),
            capabilities,
        },
        LlmBackendType::XAi => LlmBackend::XAi {
            api_key: instance.api_key.clone().unwrap_or_default(),
            endpoint: instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "https://api.x.ai/v1".to_string()),
            model: instance.model.clone(),
            capabilities,
        },
        LlmBackendType::Qwen => LlmBackend::Qwen {
            api_key: instance.api_key.clone().unwrap_or_default(),
            endpoint: instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "https://dashscope.aliyuncs.com/compatible-mode/v1".to_string()),
            model: instance.model.clone(),
            capabilities,
        },
        LlmBackendType::DeepSeek => LlmBackend::DeepSeek {
            api_key: instance.api_key.clone().unwrap_or_default(),
            endpoint: instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "https://api.deepseek.com".to_string()),
            model: instance.model.clone(),
            capabilities,
        },
        LlmBackendType::GLM => LlmBackend::GLM {
            api_key: instance.api_key.clone().unwrap_or_default(),
            endpoint: instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "https://open.bigmodel.cn/api/paas/v4".to_string()),
            model: instance.model.clone(),
            capabilities,
        },
        LlmBackendType::MiniMax => LlmBackend::MiniMax {
            api_key: instance.api_key.clone().unwrap_or_default(),
            endpoint: instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "https://api.minimax.chat/v1".to_string()),
            model: instance.model.clone(),
            capabilities,
        },
    })
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
    /// Whether memory injection is enabled
    #[serde(rename = "memoryEnabled", default)]
    pub memory_enabled: bool,
}

/// Type alias for the cancel-sender map shared between manager and stream wrappers.
type CancelSenderMap = Arc<RwLock<HashMap<String, tokio::sync::watch::Sender<bool>>>>;

/// RAII guard that ensures a registered cancel sender is removed from the
/// `cancel_senders` map when the stream wrapper is dropped — regardless of
/// whether the consumer fully drained the stream.
///
/// Without this guard, a client disconnect / downstream error that drops the
/// stream early would leak the sender entry, blocking future cancel attempts
/// and pinning memory until process exit.
///
/// `Drop` cannot await on `tokio::sync::RwLock`, so cleanup is dispatched to
/// a background task. This is cheap (cleanup runs once per session turn) and
/// avoids holding the runtime's worker threads.
struct CancelSenderGuard {
    senders: CancelSenderMap,
    session_id: String,
    /// Set to true once the natural-end cleanup has run, so `Drop` doesn't
    /// double-spawn a redundant removal.
    cleaned_up: bool,
}

impl CancelSenderGuard {
    fn new(senders: CancelSenderMap, session_id: String) -> Self {
        Self {
            senders,
            session_id,
            cleaned_up: false,
        }
    }

    /// Mark as cleaned up so `Drop` becomes a no-op. Callers that already
    /// performed the removal explicitly should invoke this before going out
    /// of scope.
    fn mark_cleaned_up(&mut self) {
        self.cleaned_up = true;
    }
}

impl Drop for CancelSenderGuard {
    fn drop(&mut self) {
        if self.cleaned_up {
            return;
        }
        let senders = self.senders.clone();
        let session_id = std::mem::take(&mut self.session_id);
        tokio::spawn(async move {
            senders.write().await.remove(&session_id);
        });
    }
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
    tool_registry: Arc<RwLock<Option<Arc<crate::toolkit::ToolRegistry>>>>,
    /// Skill registry for scenario-driven prompt injection
    skill_registry: crate::skills::SharedSkillRegistry,
    /// Cancel signal senders for active streaming sessions (session_id → watch::Sender)
    cancel_senders: Arc<RwLock<HashMap<String, tokio::sync::watch::Sender<bool>>>>,
    /// Per-session event subscribers. ChatSessionCapabilityProvider
    /// registers one mpsc::Sender per session-stream subscription. Each
    /// AgentEvent yielded by `process_message_events` is teed (best-effort,
    /// `try_send`) to all subscribers of the session. Voice-assistant and
    /// other low-latency consumers use this to skip the EventBus hop; the
    /// EventBus path remains as a fallback for ordinary ChatStream callers.
    ///
    /// Typically 0 or 1 subscribers per session; fan-out is a future
    /// extension. A slow subscriber that fills its channel just drops events
    /// (deliberate: voice workloads should never accumulate backlog).
    event_subscribers: Arc<RwLock<HashMap<String, Vec<tokio::sync::mpsc::Sender<AgentEvent>>>>>,
    /// Per-session frozen memory snapshot (loaded once, cached for the session).
    /// `MemorySnapshot::load` reads 3 files synchronously + token-truncates on
    /// every call; the "frozen snapshot" design (snapshot.rs) loads once per
    /// session, so re-reading on each message was redundant work on the hot
    /// path. Memory-tool writes intentionally do NOT invalidate this — the
    /// snapshot stays frozen for the session and is re-read on the next one.
    memory_snapshots: Arc<RwLock<HashMap<String, crate::memory::MemorySnapshot>>>,
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
        let store = SessionStore::open_isolated(":memory:").unwrap_or_else(|e| {
            // Fallback to temp file if :memory: fails
            tracing::error!(error = %e, ":memory: failed, using temp file");
            let temp_path = std::env::temp_dir()
                .join(format!("sessions_fallback_{}.redb", uuid::Uuid::new_v4()));
            tracing::debug!(path = ?temp_path, "Using fallback path for session store");
            SessionStore::open_isolated(&temp_path)
                .expect("Failed to create fallback session store")
        });
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            session_messages: Arc::new(RwLock::new(HashMap::new())),
            store,
            default_config: AgentConfig::default(),
            default_llm_backend: Arc::new(RwLock::new(None)),
            tool_registry: Arc::new(RwLock::new(None)),
            skill_registry: crate::skills::create_shared_registry(None),
            cancel_senders: Arc::new(RwLock::new(HashMap::new())),
            event_subscribers: Arc::new(RwLock::new(HashMap::new())),
            memory_snapshots: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new session manager with a custom database path.
    pub fn with_path(path: impl AsRef<std::path::Path>) -> Result<Self> {
        // Create or open the database
        let store = SessionStore::open(path)
            .map_err(|e| NeoMindError::Storage(format!("Failed to open session store: {}", e)))?;

        let data_dir = std::path::Path::new("data");
        let manager = Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            session_messages: Arc::new(RwLock::new(HashMap::new())),
            store,
            default_config: AgentConfig::default(),
            default_llm_backend: Arc::new(RwLock::new(None)),
            tool_registry: Arc::new(RwLock::new(None)),
            skill_registry: crate::skills::create_shared_registry(Some(data_dir)),
            cancel_senders: Arc::new(RwLock::new(HashMap::new())),
            event_subscribers: Arc::new(RwLock::new(HashMap::new())),
            memory_snapshots: Arc::new(RwLock::new(HashMap::new())),
        };

        // Restore sessions from database on startup
        // Note: This requires LLM backend to be configured later via set_llm_backend
        let session_ids = manager.store.list_sessions().unwrap_or_else(|e| {
            tracing::error!(error = %e, message = "Failed to list sessions from database");
            Vec::new()
        });

        if !session_ids.is_empty() {
            tracing::info!(
                count = session_ids.len(),
                "Found persisted sessions, will restore lazily"
            );
        }

        tracing::info!(message = "SessionManager initialized with persistent storage");

        Ok(manager)
    }

    /// Save a session ID to persistent storage.
    fn save_session_id(&self, session_id: &str) -> Result<()> {
        self.store
            .save_session_id(session_id)
            .map_err(|e| NeoMindError::Storage(format!("Failed to save session: {}", e)))
    }

    /// Delete a session from persistent storage.
    fn delete_session_id(&self, session_id: &str) -> Result<()> {
        tracing::debug!(" delete_session_id called for: {}", session_id);
        let result = self.store.delete_session(session_id);
        tracing::debug!(" delete_session result: {:?}", result);
        result.map_err(|e| NeoMindError::Storage(format!("Failed to delete session: {}", e)))
    }

    /// Save message history for a session to persistent storage.
    fn save_history(&self, session_id: &str, messages: &[AgentMessage]) -> Result<()> {
        // Convert AgentMessage to SessionMessage
        let session_messages: Vec<neomind_storage::SessionMessage> = messages
            .iter()
            .map(|msg| {
                // Convert ToolCall to serde_json::Value, including result and round fields
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
                            if let Some(ref result) = call.result {
                                if let Some(obj_map) = obj.as_object_mut() {
                                    obj_map.insert("result".to_string(), result.clone());
                                }
                            }
                            // Add round field if present
                            if let Some(ref round) = call.round {
                                if let Some(obj_map) = obj.as_object_mut() {
                                    obj_map.insert("round".to_string(), serde_json::json!(round));
                                }
                            }
                            obj
                        })
                        .collect()
                });

                // Convert images from AgentMessageImage to SessionMessageImage
                let images = msg.images.as_ref().map(|imgs| {
                    imgs.iter()
                        .map(|img| neomind_storage::SessionMessageImage {
                            data: img.data.clone(),
                            mime_type: img.mime_type.clone(),
                        })
                        .collect()
                });

                neomind_storage::SessionMessage {
                    role: msg.role.clone(),
                    content: msg.content.to_string(),
                    tool_calls,
                    tool_call_id: msg.tool_call_id.clone(),
                    tool_call_name: msg.tool_call_name.clone(),
                    thinking: msg.thinking.clone(),
                    images,
                    round_contents: msg.round_contents.clone(),
                    round_thinking: msg.round_thinking.clone(),
                    timestamp: msg.timestamp,
                }
            })
            .collect();

        self.store
            .save_history(session_id, &session_messages)
            .map_err(|e| NeoMindError::Storage(format!("Failed to save history: {}", e)))
    }

    /// Load message history for a session from persistent storage.
    fn load_history(&self, session_id: &str) -> Result<Vec<AgentMessage>> {
        let session_messages = self
            .store
            .load_history(session_id)
            .map_err(|e| NeoMindError::Storage(format!("Failed to load history: {}", e)))?;

        // Debug: Log loaded messages
        tracing::debug!(
            " Loaded {} messages from DB for session {}",
            session_messages.len(),
            session_id
        );
        for (i, sm) in session_messages.iter().enumerate() {
            if sm.role == "assistant" {
                tracing::debug!(
                    " Message {}: role={}, content_len={}, has_thinking={}, tool_calls_count={}",
                    i,
                    sm.role,
                    sm.content.len(),
                    sm.thinking.is_some(),
                    sm.tool_calls.as_ref().map_or(0, |c| c.len())
                );
            }
        }

        // Convert SessionMessage back to AgentMessage
        let messages = session_messages
            .into_iter()
            .map(|sm| {
                // Convert serde_json::Value to ToolCall, including result and round fields
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
                                // Extract round field if present
                                let round =
                                    v.get("round").and_then(|r| r.as_u64()).map(|r| r as usize);
                                Some(super::agent::ToolCall {
                                    name: name.to_string(),
                                    id: id.to_string(),
                                    arguments: args.clone(),
                                    result,
                                    round,
                                })
                            } else {
                                None
                            }
                        })
                        .collect()
                });

                // Convert images from SessionMessageImage to AgentMessageImage
                let images = sm.images.map(|imgs| {
                    imgs.into_iter()
                        .map(|img| super::agent::AgentMessageImage {
                            data: img.data,
                            mime_type: img.mime_type,
                        })
                        .collect()
                });

                AgentMessage {
                    role: sm.role,
                    content: sm.content.into(),
                    tool_calls,
                    tool_call_id: sm.tool_call_id,
                    tool_call_name: sm.tool_call_name,
                    thinking: sm.thinking,
                    images,
                    round_contents: sm.round_contents,
                    round_thinking: sm.round_thinking,
                    timestamp: sm.timestamp,
                }
            })
            .collect();

        Ok(messages)
    }

    /// Set the default LLM backend for NEW sessions only.
    /// Does NOT modify existing sessions (they may have their own backend configuration).
    pub async fn set_default_llm_backend(&self, backend: LlmBackend) {
        *self.default_llm_backend.write().await = Some(backend);
    }

    /// Set the default LLM backend for all new and existing sessions.
    /// WARNING: This will override backend configuration for ALL existing sessions,
    /// including AI Agents with their own llm_backend_id.
    /// Use set_default_llm_backend() instead unless you specifically need this behavior.
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

    /// Get the default LLM backend if configured.
    pub async fn get_llm_backend(&self) -> Result<Option<LlmBackend>> {
        Ok(self.default_llm_backend.read().await.clone())
    }

    /// Configure LLM using the LlmBackendInstanceManager.
    /// This sets the default backend for NEW sessions only.
    pub async fn configure_llm_from_instance_manager(&self) -> Result<()> {
        // Get the instance manager
        let manager = get_instance_manager()
            .map_err(|e| NeoMindError::Llm(format!("Failed to get instance manager: {}", e)))?;

        // Get the active backend instance
        let active_instance = manager
            .get_active_instance()
            .ok_or_else(|| NeoMindError::Llm("No active LLM backend configured".to_string()))?;

        // Convert to LlmBackend enum based on backend type
        let backend = instance_to_llm_backend(&active_instance)?;

        // Only set as default for new sessions, don't modify existing sessions
        self.set_default_llm_backend(backend).await;
        Ok(())
    }

    /// Configure LLM using a specific backend ID from the instance manager.
    /// Returns the LlmBackend for direct agent configuration.
    pub fn get_backend_by_id(backend_id: &str) -> Result<LlmBackend> {
        tracing::debug!(backend_id = %backend_id, "get_backend_by_id called");

        let manager = get_instance_manager().map_err(|e| {
            tracing::error!(error = %e, "Failed to get instance manager");
            NeoMindError::Llm(format!("Failed to get instance manager: {}", e))
        })?;

        // Get the instance by ID using the public method
        let instance = manager.get_instance(backend_id).ok_or_else(|| {
            tracing::error!(backend_id = %backend_id, "Backend not found");
            NeoMindError::Llm(format!("Backend '{}' not found", backend_id))
        })?;

        tracing::debug!(
            backend_id = %backend_id,
            model = %instance.model,
            supports_multimodal = %instance.capabilities.supports_multimodal,
            "Found backend instance"
        );

        instance_to_llm_backend(&instance)
    }

    /// Configure LLM using a specific backend ID from the instance manager.
    /// This configures the specified backend for the current session agent.
    pub async fn configure_agent_by_backend_id(
        &self,
        session_id: &str,
        backend_id: &str,
    ) -> Result<()> {
        tracing::debug!(session_id = %session_id, backend_id = %backend_id, "configure_agent_by_backend_id called");

        let backend = Self::get_backend_by_id(backend_id)?;

        tracing::debug!(
            session_id = %session_id,
            backend_type = ?std::mem::discriminant(&backend),
            "Got backend, configuring agent"
        );

        let agent = self.get_session(session_id).await?;
        agent.configure_llm(backend).await
    }

    /// Set the tool registry for all new sessions.
    pub async fn set_tool_registry(&self, registry: Arc<crate::toolkit::ToolRegistry>) {
        *self.tool_registry.write().await = Some(registry);
    }

    /// Get a clone of the tool registry (if set).
    pub async fn get_tool_registry(&self) -> Option<Arc<crate::toolkit::ToolRegistry>> {
        self.tool_registry.read().await.clone()
    }

    /// Get the shared skill registry.
    pub fn skill_registry(&self) -> crate::skills::SharedSkillRegistry {
        self.skill_registry.clone()
    }

    /// P0.3: Get the session store for direct access (for pending stream state management).
    pub fn session_store(&self) -> Arc<SessionStore> {
        self.store.clone()
    }

    /// Register a cancel signal sender for an active streaming session.
    /// The sender is stored so that `cancel_session` can interrupt the stream.
    ///
    /// [single-stream mutex] Returns `false` WITHOUT registering when a
    /// stream is already active for the session — the registration doubles
    /// as the per-session mutual exclusion. Previously a second concurrent
    /// stream silently OVERWROTE the first's sender (making it
    /// uncancellable) and interleaved history writes into the same
    /// AgentInternalState.
    pub async fn register_cancel_sender(
        &self,
        session_id: &str,
        sender: tokio::sync::watch::Sender<bool>,
    ) -> bool {
        let mut map = self.cancel_senders.write().await;
        if map.contains_key(session_id) {
            return false;
        }
        map.insert(session_id.to_string(), sender);
        true
    }

    /// Remove a cancel sender (called when streaming ends naturally).
    pub async fn remove_cancel_sender(&self, session_id: &str) {
        self.cancel_senders.write().await.remove(session_id);
    }

    // ---- Direct event subscribers (Phase 2: ChatSession direct routing) ----

    /// Register a direct subscriber for events on this session. Returns the
    /// `Receiver` end. The subscriber receives every `AgentEvent` yielded by
    /// any future `process_message_events` call on this session, until
    /// dropped via `remove_subscriber` (or until the Sender half is dropped
    /// when the subscriber holder tears down).
    ///
    /// `buffer` is the mpsc channel capacity. Use a small bounded channel
    /// (e.g. 256): a slow consumer just drops events via `try_send` in
    /// `publish_to_subscribers` — voice workloads should never accumulate
    /// backlog, and a stuck subscriber must not wedge the agent stream.
    pub async fn subscribe_events(
        &self,
        session_id: &str,
        buffer: usize,
    ) -> tokio::sync::mpsc::Receiver<AgentEvent> {
        let (tx, rx) = tokio::sync::mpsc::channel(buffer);
        self.event_subscribers
            .write()
            .await
            .entry(session_id.to_string())
            .or_insert_with(Vec::new)
            .push(tx);
        rx
    }

    /// Drop the most recently registered subscriber for this session
    /// (LIFO; today we assume at most one subscriber per session — the
    /// ChatSessionCapabilityProvider). The dropped `Sender` closes the
    /// channel; the holder's `Receiver::recv` will return `None`.
    pub async fn remove_subscriber(&self, session_id: &str) {
        // [deadlock fix] The old body held the write guard from the
        // `if let` scrutinee and then acquired `.write()` AGAIN on the same
        // non-reentrant tokio RwLock whenever the popped vec became empty —
        // a permanent deadlock wedging `event_subscribers` globally the
        // moment a subscriber ever registered and disconnected. Remove
        // through the same guard instead.
        let mut subs = self.event_subscribers.write().await;
        if let Some(v) = subs.get_mut(session_id) {
            v.pop();
            if v.is_empty() {
                subs.remove(session_id);
            }
        }
    }

    /// Best-effort tee of one AgentEvent to all direct subscribers of this
    /// session. Never blocks the agent stream on a slow subscriber: uses
    /// `try_send`; channel-full / closed errors are silently dropped
    /// (the subscriber misses that event).
    pub async fn publish_to_subscribers(&self, session_id: &str, event: AgentEvent) {
        let subs = self.event_subscribers.read().await;
        if let Some(v) = subs.get(session_id) {
            for tx in v.iter() {
                let _ = tx.try_send(event.clone());
            }
        }
    }

    /// Cancel an active streaming session by session ID.
    /// Returns true if a stream was active and interrupted.
    pub async fn cancel_session(&self, session_id: &str) -> bool {
        let mut senders = self.cancel_senders.write().await;
        if let Some(sender) = senders.remove(session_id) {
            // Send the interrupt signal
            let _ = sender.send(true);
            tracing::info!(session_id = %session_id, "Sent interrupt signal to streaming session");
            true
        } else {
            tracing::debug!(session_id = %session_id, "No active stream to cancel");
            false
        }
    }

    /// Get the LLM interface for a session's agent (for background tasks like summarization).
    pub async fn get_agent_llm(&self, session_id: &str) -> Option<Arc<LlmInterface>> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).map(|agent| agent.llm_interface())
    }

    /// Create a new session using the manager's `default_config`.
    ///
    /// Equivalent to `create_session_with_config(self.default_config.clone())`.
    /// Existing callers (CLI, capability providers, tests) keep this behavior.
    pub async fn create_session(&self) -> Result<String> {
        self.create_session_with_config(self.default_config.clone())
            .await
    }

    /// Create a new session with the given (full) `AgentConfig`.
    ///
    /// This is the canonical implementation; the no-arg and options variants
    /// are thin wrappers. The provided config is used verbatim — no merging
    /// with `default_config` happens here.
    pub async fn create_session_with_config(&self, config: AgentConfig) -> Result<String> {
        let session_id = Uuid::new_v4().to_string();

        // Use tool registry if set, otherwise create default mock tools
        let tool_registry = self.tool_registry.read().await.clone();

        let agent = if let Some(tools) = tool_registry {
            Agent::with_tools(config, session_id.clone(), tools)
        } else {
            Agent::new(config, session_id.clone())
        };

        // Configure LLM if a default backend is set
        let llm_backend = self.default_llm_backend.read().await.clone();
        if let Some(backend) = llm_backend {
            let _ = agent.configure_llm(backend).await;
        }

        // Inject skill registry into agent's LLM interface
        agent
            .llm_interface()
            .set_skill_registry(self.skill_registry.clone())
            .await;

        let agent = Arc::new(agent);

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

    /// Create a new session with the supplied options merged on top of the
    /// manager's `default_config`. Fields left `None` in `opts` fall back to
    /// the default. Use this when you need per-session overrides (e.g. a
    /// voice-assistant extension baking in a system prompt) without changing
    /// global defaults.
    pub async fn create_session_with_options(&self, opts: CreateSessionOptions) -> Result<String> {
        let config = self.merge_options(opts);
        self.create_session_with_config(config).await
    }

    /// Merge `CreateSessionOptions` on top of `default_config`. Any `None`
    /// field inherits the default; `Some(v)` overrides that single field.
    fn merge_options(&self, opts: CreateSessionOptions) -> AgentConfig {
        let mut cfg = self.default_config.clone();
        if let Some(sp) = opts.system_prompt {
            cfg.system_prompt = sp;
        } else if let Some(suffix) = opts.system_prompt_suffix {
            if !suffix.trim().is_empty() {
                // Append on top of the platform default — page-scoped focus
                // without losing the base agent instructions. Both slots get
                // it: `system_prompt` feeds the non-streaming paths, the new
                // dedicated suffix field feeds the streaming chat builder
                // (which builds its own slim-template prompt and never read
                // `system_prompt` — the suffix used to die there silently).
                let trimmed = suffix.trim().to_string();
                cfg.system_prompt = format!("{}\n\n{}", cfg.system_prompt.trim_end(), trimmed);
                cfg.system_prompt_suffix = Some(trimmed);
            }
        }
        if let Some(t) = opts.temperature {
            cfg.temperature = t;
        }
        if let Some(m) = opts.model {
            cfg.model = m;
        }
        if let Some(et) = opts.enable_tools {
            cfg.enable_tools = et;
        }
        if !opts.allowed_tools.is_empty() {
            cfg.allowed_tools = opts.allowed_tools;
        }
        cfg
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
            NeoMindError::Storage(format!("Failed to check session existence: {}", e))
        })?;

        if in_db {
            // Session exists in database, restore it
            self.restore_session_from_db(session_id).await
        } else {
            Err(NeoMindError::NotFound(format!("Session: {}", session_id)))
        }
    }

    /// Restore a session from the database into memory.
    async fn restore_session_from_db(&self, session_id: &str) -> Result<Arc<Agent>> {
        tracing::info!(session_id = %session_id, message = "Restoring session from database");

        // Use tool registry if set, otherwise create default agent
        let tool_registry = self.tool_registry.read().await.clone();

        let agent = if let Some(tools) = tool_registry {
            Agent::with_tools(self.default_config.clone(), session_id.to_string(), tools)
        } else {
            Agent::new(self.default_config.clone(), session_id.to_string())
        };

        // Configure LLM if a default backend is set
        let llm_backend = self.default_llm_backend.read().await.clone();
        if let Some(backend) = llm_backend {
            let _ = agent.configure_llm(backend).await;
        }

        // Inject skill registry into agent's LLM interface
        agent
            .llm_interface()
            .set_skill_registry(self.skill_registry.clone())
            .await;

        let agent = Arc::new(agent);

        // Load message history from database
        let history = self.load_history(session_id)?;

        // Restore history to agent's memory
        if !history.is_empty() {
            agent.restore_history(history.clone()).await;
            tracing::debug!(session_id = %session_id, count = history.len(), "Restored messages for session");
        }

        // Save to in-memory cache.
        //
        // Double-checked locking: between the read-lock check in `get_session`
        // and this write, a concurrent restore for the same session_id may
        // have already inserted its own Agent. If we blindly insert, we'd
        // overwrite the winner — leaving the loser's caller with a stale Arc
        // whose state updates (memory, history) silently evaporate.
        //
        // If a concurrent restore won, return THAT agent and discard ours.
        let mut sessions = self.sessions.write().await;
        if let Some(existing) = sessions.get(session_id).cloned() {
            tracing::debug!(
                session_id = %session_id,
                "Concurrent restore won — discarding duplicate agent"
            );
            drop(sessions);
            return Ok(existing);
        }
        sessions.insert(session_id.to_string(), agent.clone());
        drop(sessions);

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
                    id
                } else {
                    // Session doesn't exist in memory, check if it's in the database
                    let in_db = self.store.session_exists(&id).unwrap_or(false);

                    if in_db {
                        // Restore from database (reuses restore_session_from_db which
                        // includes tool registry, LLM config, skill registry, and history)
                        match self.restore_session_from_db(&id).await {
                            Ok(_) => id,
                            Err(e) => {
                                tracing::error!(session_id = %id, error = %e, "Failed to restore session, creating new");
                                self.create_session()
                                    .await
                                    .unwrap_or_else(|_| Uuid::new_v4().to_string())
                            }
                        }
                    } else {
                        // Create a new session
                        tracing::info!(session_id = %id, "Session not found in database, creating new session");
                        self.create_session()
                            .await
                            .unwrap_or_else(|_| Uuid::new_v4().to_string())
                    }
                }
            }
            None => self
                .create_session()
                .await
                .unwrap_or_else(|_| Uuid::new_v4().to_string()),
        }
    }

    /// Get or create a session with optional per-session config overrides.
    ///
    /// **Override semantics**: `opts` is applied **only when a new session is
    /// created** in this call. If `session_id` references an existing session
    /// (in memory or DB), the existing session's config is preserved — `opts`
    /// is silently ignored. This is intentional: callers like the
    /// `chat_session_open` capability may pass `system_prompt` on every call,
    /// but we must not silently rewrite the prompt of a long-running session
    /// just because a client retried the open handshake.
    ///
    /// Use this when you want per-session defaults (e.g. voice-assistant
    /// baking in a system prompt) without losing the idempotent "give me this
    /// session or make one" property of `get_or_create_session`.
    pub async fn get_or_create_session_with_options(
        &self,
        session_id: Option<String>,
        opts: CreateSessionOptions,
    ) -> String {
        match session_id {
            // Existing session — explicitly DO NOT apply opts (see doc above).
            Some(id) => self.get_or_create_session(Some(id)).await,
            None => self
                .create_session_with_options(opts)
                .await
                .unwrap_or_else(|_| Uuid::new_v4().to_string()),
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
            .map_err(|e| NeoMindError::Storage(format!("Failed to check session: {}", e)))?;
        tracing::debug!(" in_db: {}", in_db);

        if !in_memory && !in_db {
            tracing::debug!(" Session not found in memory or database");
            return Err(NeoMindError::NotFound(format!("Session: {}", session_id)));
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
            .map_err(|e| NeoMindError::Storage(format!("Failed to check session: {}", e)))?;

        if !in_memory && !in_db {
            return Err(NeoMindError::NotFound(format!("Session: {}", session_id)));
        }

        // Save the metadata
        let mut metadata = self
            .store
            .get_session_metadata(session_id)
            .unwrap_or_default();
        metadata.title = title.filter(|t| !t.trim().is_empty()); // Filter out empty titles

        self.store
            .save_session_metadata(session_id, &metadata)
            .map_err(|e| {
                NeoMindError::Storage(format!("Failed to save session metadata: {}", e))
            })?;

        Ok(())
    }

    /// Toggle memory enabled state for a session.
    pub async fn toggle_memory(&self, session_id: &str, enabled: bool) -> Result<bool> {
        // Check if session exists
        let in_memory = self.sessions.read().await.contains_key(session_id);
        let in_db = self
            .store
            .session_exists(session_id)
            .map_err(|e| NeoMindError::Storage(format!("Failed to check session: {}", e)))?;

        if !in_memory && !in_db {
            return Err(NeoMindError::NotFound(format!("Session: {}", session_id)));
        }

        self.store
            .toggle_memory(session_id, enabled)
            .map_err(|e| NeoMindError::Storage(format!("Failed to toggle memory: {}", e)))?;

        Ok(enabled)
    }

    /// Get whether memory is enabled for a session.
    /// Lightweight background memory extraction after a chat turn.
    ///
    /// Chat had NO automatic memory write — only what the model chose to
    /// write via the memory tool, which small models almost never do — so
    /// USER.md/KNOWLEDGE.md stayed empty and cross-session memory was dead
    /// in practice. This runs a small thinking-disabled LLM call over the
    /// last exchange, merges durable facts into the snapshot files
    /// (deduped, budget-capped), and invalidates the frozen-snapshot cache
    /// so the next turn sees them. Fire-and-forget; never blocks the reply.
    pub async fn maybe_extract_memory(
        &self,
        session_id: &str,
        user_message: &str,
        assistant_reply: &str,
    ) {
        if !self.is_memory_enabled(session_id).await {
            return;
        }
        let um = user_message.trim().to_string();
        let ar = assistant_reply.trim().to_string();
        // A substantive exchange only — greetings/pings shouldn't mint memory.
        if um.is_empty() || ar.chars().count() < 40 {
            return;
        }
        let agent = match self.get_session(session_id).await {
            Ok(a) => a,
            Err(_) => return,
        };
        let runtime = match agent.llm_interface().get_runtime().await {
            Ok(r) => r,
            Err(_) => return,
        };
        let model = runtime.model_name().to_string();

        // The snapshot cache is invalidated after a write so the next turn
        // re-loads the updated files. Rebuild from the store fresh each time.
        let store = neomind_storage::MarkdownMemoryStore::new("data/memory");
        let snapshots = self.memory_snapshots.clone();
        let session_id = session_id.to_string();

        tokio::spawn(async move {
            use neomind_core::llm::backend::{GenerationParams, LlmInput, LlmRuntime as _};
            use neomind_core::message::{Content, Message, MessageRole};

            let prompt = format!(
                "You are the memory extractor for an IoT edge platform assistant. \
From this user turn and the assistant's reply, extract AT MOST 3 durable, reusable \
facts that would help FUTURE conversations. Two kinds, tagged exactly:\n\
[user] fact        — durable facts about the user (preferences, identity, context)\n\
[knowledge] fact   — durable facts about the system/domain (device names, locations, conventions)\n\
Output ONLY bullet lines starting with \"- \", one per fact, each tagged. \
Skip transient or session-specific chatter. Empty output if nothing durable.\n\n\
User: {um}\n\
Assistant: {ar}\n"
            );

            let input = LlmInput {
                messages: vec![Message::new(MessageRole::User, Content::text(prompt))],
                params: GenerationParams {
                    temperature: Some(0.2),
                    max_tokens: Some(800),
                    thinking_enabled: Some(false), // gotcha #7 — no wasted thinking tokens
                    ..Default::default()
                },
                model: Some(model),
                stream: false,
                tools: None,
            };
            let out = match runtime.generate(input).await {
                Ok(o) => o,
                Err(e) => {
                    tracing::debug!(session_id, error = %e, "memory extraction skipped (LLM error)");
                    return;
                }
            };

            // Parse tagged bullets: `- [user] ...` / `- [knowledge] ...`
            let mut new_user: Vec<String> = Vec::new();
            let mut new_knowledge: Vec<String> = Vec::new();
            for line in out.text.lines() {
                let line = line.trim();
                let rest = line.strip_prefix("- ").or_else(|| line.strip_prefix("• "));
                let Some(rest) = rest else { continue };
                if let Some(fact) = rest.strip_prefix("[user]") {
                    let f = fact.trim().to_string();
                    if !f.is_empty() {
                        new_user.push(f);
                    }
                } else if let Some(fact) = rest.strip_prefix("[knowledge]") {
                    let f = fact.trim().to_string();
                    if !f.is_empty() {
                        new_knowledge.push(f);
                    }
                }
            }
            if new_user.is_empty() && new_knowledge.is_empty() {
                return;
            }

            // Merge: exact-normalized dedup against existing bullets; cap to
            // the file budget keeping the OLDEST facts (durable base facts
            // outrank recent chatter).
            let norm =
                |s: &str| -> String { s.to_lowercase().split_whitespace().collect::<String>() };
            let merged = |target: &str, additions: &[String]| -> Option<String> {
                let existing = store.read_file_sync(target);
                let existing_norms: std::collections::HashSet<String> = existing
                    .lines()
                    .filter_map(|l| {
                        let l = l.trim().trim_start_matches("- ").trim_start_matches("• ");
                        if l.is_empty() {
                            None
                        } else {
                            Some(norm(l))
                        }
                    })
                    .collect();
                let fresh: Vec<&String> = additions
                    .iter()
                    .filter(|f| !existing_norms.contains(&norm(f)))
                    .collect();
                if fresh.is_empty() {
                    return None;
                }
                let mut merged = existing.trim_end().to_string();
                for f in &fresh {
                    if !merged.is_empty() {
                        merged.push('\n');
                    }
                    merged.push_str("- ");
                    merged.push_str(f);
                }
                merged.push('\n');
                let limit = match target {
                    "user" => 2000,
                    _ => 3000,
                };
                if merged.chars().count() > limit {
                    let mut kept = String::new();
                    for line in merged.lines() {
                        if kept.chars().count() + line.chars().count() + 1 > limit {
                            break;
                        }
                        if !kept.is_empty() {
                            kept.push('\n');
                        }
                        kept.push_str(line);
                    }
                    merged = kept;
                    merged.push('\n');
                }
                Some(merged)
            };

            let mut wrote_any = false;
            if let Some(content) = merged("user", &new_user) {
                if store.write_file("user", &content).await.is_ok() {
                    wrote_any = true;
                }
            }
            if let Some(content) = merged("knowledge", &new_knowledge) {
                if store.write_file("knowledge", &content).await.is_ok() {
                    wrote_any = true;
                }
            }

            if wrote_any {
                // Invalidate the frozen snapshot so the next turn re-loads.
                snapshots.write().await.remove(&session_id);
                tracing::info!(
                    session_id,
                    user_facts = new_user.len(),
                    knowledge_facts = new_knowledge.len(),
                    "chat memory extraction: merged durable facts"
                );
            }
        });
    }

    pub async fn is_memory_enabled(&self, session_id: &str) -> bool {
        self.store
            .get_session_metadata(session_id)
            .map(|m| m.memory_enabled)
            .unwrap_or(false)
    }

    /// Ensure the session's frozen memory snapshot is loaded and set on the
    /// agent. Loads once per session and caches the result — `MemorySnapshot`
    /// is a "frozen snapshot" (see `memory/snapshot.rs`) that reads 3 files
    /// synchronously per load, so reloading on every message was redundant
    /// work on the hot path. Memory-tool writes deliberately do NOT invalidate
    /// this cache: the snapshot stays frozen for the session and is re-read on
    /// the next session, matching the original frozen-snapshot semantics.
    async fn ensure_memory_snapshot(&self, session_id: &str, agent: &Arc<Agent>) {
        if !self.is_memory_enabled(session_id).await {
            return;
        }
        {
            let cache = self.memory_snapshots.read().await;
            if let Some(snapshot) = cache.get(session_id) {
                if !snapshot.is_empty() {
                    agent.set_memory_snapshot(snapshot.clone()).await;
                }
                return;
            }
        }
        let memory_store = neomind_storage::MarkdownMemoryStore::new("data/memory");
        let snapshot = crate::memory::MemorySnapshot::load(&memory_store);
        if !snapshot.is_empty() {
            tracing::info!(
                session_id = %session_id,
                "Loaded memory snapshot for session"
            );
            self.memory_snapshots
                .write()
                .await
                .insert(session_id.to_string(), snapshot.clone());
            agent.set_memory_snapshot(snapshot).await;
        }
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

            // Get title and memory_enabled from metadata
            let metadata = self
                .store
                .get_session_metadata(&session_id)
                .unwrap_or_default();
            let title = metadata.title.filter(|t| !t.is_empty());

            infos.push(SessionInfo {
                session_id: session_id.clone(),
                created_at: timestamp_ms,
                message_count,
                title,
                preview,
                memory_enabled: metadata.memory_enabled,
            });
        }

        // Sort by created_at descending (newest first)
        infos.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        infos
    }

    /// List sessions with basic info (lightweight version).
    ///
    /// Performance optimization: Does NOT load message count or preview.
    /// Use this for listing sessions where full details aren't needed.
    /// This is significantly faster than `list_sessions_with_info` for large session counts.
    pub async fn list_sessions_with_info_light(&self) -> Vec<SessionInfo> {
        let mut infos = Vec::new();

        // Get all session IDs from database
        let db_session_ids = match self.store.list_sessions() {
            Ok(ids) => ids,
            Err(e) => {
                tracing::error!(error = %e, message = "Failed to list sessions from database");
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

            // Get title and memory_enabled from metadata
            let metadata = self
                .store
                .get_session_metadata(&session_id)
                .unwrap_or_default();
            let title = metadata.title.filter(|t| !t.is_empty());
            let preview = metadata.preview.filter(|p| !p.is_empty());

            infos.push(SessionInfo {
                session_id: session_id.clone(),
                created_at: timestamp_ms,
                message_count: 0, // Not loaded in light version
                title,
                preview,
                memory_enabled: metadata.memory_enabled,
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

        // Load memory snapshot if enabled and not yet loaded (parity with process_message_events)
        self.ensure_memory_snapshot(session_id, &agent).await;

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
        // If a specific backend is requested, configure the agent with it.
        // Propagate the error (mirrors process_message_multimodal_with_backend):
        // if the user explicitly chose a backend and it can't be applied, fail
        // loud rather than silently running on the previously-configured one.
        if let Some(backend) = backend_id {
            self.configure_agent_by_backend_id(session_id, backend)
                .await
                .map_err(|e| {
                    tracing::error!(
                        backend_id = %backend,
                        error = ?e,
                        "Failed to configure agent with backend"
                    );
                    e
                })?;
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

        // Load memory snapshot if enabled and not yet loaded
        self.ensure_memory_snapshot(session_id, &agent).await;

        // Read conversation summary from session metadata for context compression
        let (conversation_summary, summary_up_to_index) = self
            .store
            .get_session_metadata(session_id)
            .map(|m| (m.conversation_summary, m.summary_up_to_index))
            .unwrap_or((None, None));

        // Create a cancel signal channel so the stream can be interrupted
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        if !self.register_cancel_sender(session_id, cancel_tx).await {
            return Err(NeoMindError::validation(
                "A response is already being generated for this session — wait for it to finish or cancel it first",
            ));
        }

        let safeguards = super::agent::StreamSafeguards::default().with_interrupt_signal(cancel_rx);

        let session_id_owned = session_id.to_string();
        let cancel_senders = self.cancel_senders.clone();

        let stream = agent
            .process_stream_events_with_safeguards(
                message,
                conversation_summary,
                summary_up_to_index,
                safeguards,
            )
            .await?;

        // Wrap the stream with a scopeguard so the cancel sender is removed
        // even if the consumer drops the stream early (client disconnect,
        // downstream error). The guard lives inside the async block, so it
        // drops naturally on both natural-end and early-drop paths.
        let cleanup_stream = Box::pin(async_stream::stream! {
            let mut guard = CancelSenderGuard::new(cancel_senders.clone(), session_id_owned.clone());
            let mut stream = stream;
            while let Some(event) = stream.next().await {
                yield event;
            }
            // Stream ended naturally — remove the cancel sender inline.
            cancel_senders.write().await.remove(&session_id_owned);
            guard.mark_cleaned_up();
        });

        Ok(cleanup_stream)
    }

    /// Process a message in a session with event streaming and optional LLM backend override.
    pub async fn process_message_events_with_backend(
        &self,
        session_id: &str,
        message: &str,
        backend_id: Option<&str>,
    ) -> Result<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> {
        self.process_message_events_with_backend_and_skills(session_id, message, backend_id, None)
            .await
    }

    /// Process a message in a session with event streaming, optional backend override, and pinned skills.
    ///
    /// `selected_skills`: `None` = leave the session's pins untouched;
    /// `Some(list)` = replace the pins (an empty list clears them).
    pub async fn process_message_events_with_backend_and_skills(
        &self,
        session_id: &str,
        message: &str,
        backend_id: Option<&str>,
        selected_skills: Option<&[String]>,
    ) -> Result<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> {
        // If a specific backend is requested, configure the agent with it
        if let Some(backend) = backend_id {
            if let Err(e) = self
                .configure_agent_by_backend_id(session_id, backend)
                .await
            {
                tracing::error!(
                    session_id = %session_id,
                    backend_id = %backend,
                    error = %e,
                    "Failed to configure agent backend for streaming — falling back to current runtime"
                );
            }
        }

        // Update pinned skills on the agent if provided
        if let Some(skills) = selected_skills {
            if let Ok(agent) = self.get_session(session_id).await {
                agent.set_pinned_skills(skills.to_vec()).await;
            }
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
            self.configure_agent_by_backend_id(session_id, backend)
                .await
                .map_err(|e| {
                    tracing::error!(backend_id = %backend, error = ?e, "Failed to configure agent with backend");
                    e
                })?;
        }

        // Check if images are provided and model supports vision
        if !images.is_empty() {
            let agent = self.get_session(session_id).await?;
            if !agent.llm_interface().supports_multimodal().await {
                return Err(super::error::NeoMindError::Validation(
                    "Current model does not support image input. Please select a vision-capable model or remove images before retrying."
                        .to_string(),
                ));
            }
        }

        self.process_message_multimodal(session_id, message, images)
            .await
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
            self.configure_agent_by_backend_id(session_id, backend)
                .await
                .map_err(|e| {
                    tracing::error!(backend_id = %backend, error = ?e, "Failed to configure agent with backend");
                    e
                })?;
        }

        // Check if images are provided and model supports vision
        if !images.is_empty() {
            let agent = self.get_session(session_id).await?;
            if !agent.llm_interface().supports_multimodal().await {
                return Err(super::error::NeoMindError::Validation(
                    "Current model does not support image input. Please select a vision-capable model or remove images before retrying."
                        .to_string(),
                ));
            }
        }

        self.process_message_multimodal_stream(session_id, message, images)
            .await
    }

    /// Process a multimodal message (text + images) with streaming response.
    pub async fn process_message_multimodal_stream(
        &self,
        session_id: &str,
        message: &str,
        images: Vec<String>,
    ) -> Result<Pin<Box<dyn Stream<Item = super::agent::AgentEvent> + Send>>> {
        // Check if images are provided and model supports vision
        if !images.is_empty() {
            let agent = self.get_session(session_id).await?;
            if !agent.llm_interface().supports_multimodal().await {
                return Err(super::error::NeoMindError::Validation(
                    "Current model does not support image input. Please select a vision-capable model or remove images before retrying."
                        .to_string(),
                ));
            }
        }
        let agent = self.get_session(session_id).await?;

        // Create a cancel signal channel
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        if !self.register_cancel_sender(session_id, cancel_tx).await {
            return Err(NeoMindError::validation(
                "A response is already being generated for this session — wait for it to finish or cancel it first",
            ));
        }

        let safeguards = super::agent::StreamSafeguards::default().with_interrupt_signal(cancel_rx);

        let session_id_owned = session_id.to_string();
        let cancel_senders = self.cancel_senders.clone();

        let stream = agent
            .process_multimodal_stream_events_with_safeguards(message, images, safeguards)
            .await?;

        // Wrap with scopeguard so cancel sender is removed on both natural
        // stream end and early drop (matches the text streaming path above).
        let cleanup_stream = Box::pin(async_stream::stream! {
            let mut guard = CancelSenderGuard::new(cancel_senders.clone(), session_id_owned.clone());
            let mut stream = stream;
            while let Some(event) = stream.next().await {
                yield event;
            }
            cancel_senders.write().await.remove(&session_id_owned);
            guard.mark_cleaned_up();
        });

        Ok(cleanup_stream)
    }

    /// Get conversation history for a session.
    /// If session doesn't exist, returns empty history (soft fail for dirty data).
    pub async fn get_history(&self, session_id: &str) -> Result<Vec<AgentMessage>> {
        // Try to get the session - this will restore from DB if needed
        match self.get_session(session_id).await {
            Ok(agent) => Ok(agent.history().await),
            Err(NeoMindError::NotFound(_)) => {
                // Session not found in memory or DB - might be dirty data
                // Return empty history instead of error
                tracing::warn!(session_id = %session_id, "Session not found, returning empty history");
                Ok(Vec::new())
            }
            Err(NeoMindError::Storage(e)) => {
                // Storage error (database corrupted, etc.) - try to load directly from store
                tracing::error!(session_id = %session_id, error = %e, "Storage error, trying direct load");
                // Try to load history directly from storage as a fallback
                match self.load_history(session_id) {
                    Ok(messages) => {
                        tracing::debug!(
                            count = messages.len(),
                            "Successfully loaded messages via direct load"
                        );
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

        // [ghost-summary fix] The conversation summary survives a history
        // clear unless reset — the next turn then injected a summary of the
        // DELETED conversation as a system message. Reset both fields so a
        // cleared session starts truly fresh.
        match self.store.get_session_metadata(session_id) {
            Ok(mut meta) => {
                if meta.conversation_summary.is_some() || meta.summary_up_to_index.is_some() {
                    meta.conversation_summary = None;
                    meta.summary_up_to_index = None;
                    if let Err(e) = self.store.save_session_metadata(session_id, &meta) {
                        tracing::error!(
                            session_id = %session_id,
                            error = %e,
                            message = "Failed to reset conversation summary on clear"
                        );
                    }
                }
            }
            Err(e) => {
                tracing::debug!(
                    session_id = %session_id,
                    error = %e,
                    "No session metadata to reset on clear"
                );
            }
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

        // Update preview from first user message if not already set
        if let Ok(mut metadata) = self.store.get_session_metadata(session_id) {
            if metadata.preview.is_none() || metadata.preview.as_ref().is_none_or(|p| p.is_empty())
            {
                if let Some(first_user_msg) = messages.iter().find(|m| m.role == "user") {
                    let content = first_user_msg.content.trim();
                    let preview = if content.chars().count() > 50 {
                        format!("{}...", content.chars().take(50).collect::<String>())
                    } else {
                        content.to_string()
                    };
                    if !preview.is_empty() {
                        metadata.preview = Some(preview);
                        if let Err(e) = self.store.save_session_metadata(session_id, &metadata) {
                            tracing::debug!(
                                session_id = %session_id,
                                error = %e,
                                "Failed to save session preview"
                            );
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Clean up invalid/empty sessions.
    ///
    /// Removes sessions that either:
    /// 1. Have corrupted history (can't load), or
    /// 2. Have no messages (empty sessions older than 1 hour)
    ///
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
                skill_registry: crate::skills::create_shared_registry(None),
                cancel_senders: Arc::new(RwLock::new(HashMap::new())),
                event_subscribers: Arc::new(RwLock::new(HashMap::new())),
                memory_snapshots: Arc::new(RwLock::new(HashMap::new())),
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
        let test_path = temp_dir.join(format!("neomind_test_{}", uuid::Uuid::new_v4()));
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
    async fn test_create_session_with_options_overrides_only_provided_fields() {
        // Verify that CreateSessionOptions acts as a patch on top of
        // default_config: only Some(_) fields override, the rest inherits.
        let manager = create_temp_manager();

        let opts = CreateSessionOptions {
            system_prompt: Some("voice-assistant mode: short replies".to_string()),
            temperature: Some(0.1),
            model: None,        // inherit default
            enable_tools: None, // inherit default
            ..Default::default()
        };

        let session_id = manager.create_session_with_options(opts).await.unwrap();
        let agent = manager.get_session(&session_id).await.unwrap();
        let llm = agent.llm_interface();
        let sp = llm.get_system_prompt().await;

        assert_eq!(sp, "voice-assistant mode: short replies");
        // We don't have a direct temperature getter on the agent's LLM here
        // without poking at backend internals; the override is exercised
        // end-to-end in the API integration tests. The system_prompt check
        // above is the load-bearing assertion for the patch mechanism.
    }

    #[tokio::test]
    async fn test_create_session_with_options_suffix_appends_to_default() {
        // system_prompt_suffix appends to the platform default instead of
        // replacing it — page-scoped focus without losing the base prompt.
        let manager = create_temp_manager();
        let default_prompt = manager.default_config.system_prompt.clone();

        let opts = CreateSessionOptions {
            system_prompt_suffix: Some("## Current page focus: devices".to_string()),
            ..Default::default()
        };
        let session_id = manager.create_session_with_options(opts).await.unwrap();
        let agent = manager.get_session(&session_id).await.unwrap();
        let sp = agent.llm_interface().get_system_prompt().await;

        assert!(
            sp.starts_with(&default_prompt),
            "suffix must keep the platform default as the base"
        );
        assert!(sp.ends_with("## Current page focus: devices"));
        // And the merge must not mutate the manager default
        assert_eq!(manager.default_config.system_prompt, default_prompt);
    }

    #[tokio::test]
    async fn test_suffix_survives_into_streaming_chat_prompt() {
        // Regression (0.9.20): the streaming chat path builds its own
        // slim-template prompt and never read `system_prompt` — the
        // page-scoped suffix silently died in tool-enabled sessions. The
        // dedicated suffix slot must be appended by the streaming builder.
        let manager = create_temp_manager();
        let opts = CreateSessionOptions {
            system_prompt_suffix: Some("## Current page focus: dashboards".to_string()),
            ..Default::default()
        };
        let session_id = manager.create_session_with_options(opts).await.unwrap();
        let agent = manager.get_session(&session_id).await.unwrap();

        let streaming_prompt = agent
            .llm_interface()
            .build_system_prompt_with_tools(Some("list my boards"))
            .await;
        assert!(
            streaming_prompt.contains("## Current page focus: dashboards"),
            "streaming chat prompt must carry the session suffix"
        );
    }

    #[tokio::test]
    async fn test_create_session_with_options_full_override_wins_over_suffix() {
        // An explicit system_prompt replaces wholesale; the suffix is ignored.
        let manager = create_temp_manager();
        let opts = CreateSessionOptions {
            system_prompt: Some("full override".to_string()),
            system_prompt_suffix: Some("appended?".to_string()),
            ..Default::default()
        };
        let session_id = manager.create_session_with_options(opts).await.unwrap();
        let agent = manager.get_session(&session_id).await.unwrap();
        assert_eq!(
            agent.llm_interface().get_system_prompt().await,
            "full override"
        );
    }

    #[tokio::test]
    async fn test_create_session_default_unchanged() {
        // Regression guard: callers using the original create_session()
        // path must observe identical behavior — default_config flows
        // through to the agent's system prompt.
        let manager = create_temp_manager();
        let session_id = manager.create_session().await.unwrap();
        let agent = manager.get_session(&session_id).await.unwrap();
        let sp = agent.llm_interface().get_system_prompt().await;
        // AgentConfig::default() hardcodes this string in agent/types.rs.
        assert_eq!(sp, AgentConfig::default().system_prompt);
    }

    #[tokio::test]
    async fn test_get_or_create_session_with_options_ignores_opts_for_existing() {
        // Critical safety property: when an existing session_id is passed,
        // opts MUST be ignored — otherwise a client retrying an open()
        // handshake could silently overwrite a long-running session's
        // system_prompt.
        let manager = create_temp_manager();

        // First call: create with custom prompt
        let sid = manager
            .get_or_create_session_with_options(
                None,
                CreateSessionOptions {
                    system_prompt: Some("original prompt".to_string()),
                    ..Default::default()
                },
            )
            .await;

        // Second call: same id, DIFFERENT prompt. Must NOT override.
        let sid_again = manager
            .get_or_create_session_with_options(
                Some(sid.clone()),
                CreateSessionOptions {
                    system_prompt: Some("different prompt".to_string()),
                    ..Default::default()
                },
            )
            .await;

        assert_eq!(sid, sid_again);
        let agent = manager.get_session(&sid).await.unwrap();
        let sp = agent.llm_interface().get_system_prompt().await;
        assert_eq!(sp, "original prompt");
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

        // Two different sessions should have distinct IDs
        assert_ne!(session1, session2);

        // Both should be retrievable independently
        let agent1 = manager.get_session(&session1).await.unwrap();
        let agent2 = manager.get_session(&session2).await.unwrap();

        // Histories should start empty and be independent
        let history1 = agent1.history().await;
        let history2 = agent2.history().await;
        assert_eq!(history1.len(), 0);
        assert_eq!(history2.len(), 0);

        // Verify session count reflects both sessions
        assert_eq!(manager.session_count().await, 2);

        // Removing one session should not affect the other
        manager.remove_session(&session1).await.unwrap();
        assert!(manager.get_session(&session2).await.is_ok());
        assert_eq!(manager.session_count().await, 1);
    }

    #[tokio::test]
    async fn test_cancel_session_mechanism() {
        let manager = create_temp_manager();

        // 1. No active stream → cancel returns false
        let cancelled = manager.cancel_session("nonexistent").await;
        assert!(
            !cancelled,
            "Canceling nonexistent session should return false"
        );

        // 2. Register a cancel sender, then cancel it
        let (tx, rx) = tokio::sync::watch::channel(false);
        manager.register_cancel_sender("test-session", tx).await;

        // Receiver should initially be false
        assert!(!*rx.borrow(), "Initial value should be false");

        // Cancel the session
        let cancelled = manager.cancel_session("test-session").await;
        assert!(cancelled, "Canceling registered session should return true");

        // Receiver should now be true
        assert!(*rx.borrow(), "After cancel, receiver should be true");

        // 3. Second cancel should return false (sender was removed)
        let cancelled = manager.cancel_session("test-session").await;
        assert!(
            !cancelled,
            "Second cancel should return false (sender removed)"
        );
    }

    #[tokio::test]
    async fn test_cancel_sender_cleanup_on_stream_end() {
        let manager = create_temp_manager();

        // Simulate: register sender, then remove it (as stream cleanup would do)
        let (tx, _rx) = tokio::sync::watch::channel(false);
        manager.register_cancel_sender("stream-session", tx).await;

        // Verify registered
        let cancelled = manager.cancel_session("stream-session").await;
        assert!(cancelled, "Should be able to cancel registered session");

        // Re-register and remove manually (simulating stream end)
        let (tx2, _rx2) = tokio::sync::watch::channel(false);
        manager.register_cancel_sender("stream-session", tx2).await;
        manager.remove_cancel_sender("stream-session").await;

        // Now cancel should return false
        let cancelled = manager.cancel_session("stream-session").await;
        assert!(!cancelled, "After removal, cancel should return false");
    }

    /// Regression: if a stream wrapper is dropped WITHOUT being fully drained
    /// (client disconnect, downstream error), the cancel sender must still be
    /// removed from the map so a later `cancel_session` doesn't see a stale
    /// entry. Before the `CancelSenderGuard` fix, the cleanup code lived at
    /// the tail of `async_stream::stream! { ... }` and never ran on early drop.
    /// Regression: two concurrent `get_session` calls for the same session
    /// that's in the DB but not in memory MUST return the SAME `Arc<Agent>`.
    ///
    /// Before the double-checked-locking fix in `restore_session_from_db`,
    /// both calls would pass the read-lock check (session not present),
    /// both would build separate Agents, and both would insert. The second
    /// insert would win — the loser's caller would be left with a stale Arc
    /// whose subsequent state changes (memory updates, history appends)
    /// silently never reached the canonical map entry.
    #[tokio::test]
    async fn test_get_session_concurrent_restore_returns_same_arc() {
        let manager = create_temp_manager();

        // 1. Create a session so it's in the DB
        let session_id = manager.create_session().await.unwrap();

        // 2. Seed the history table (otherwise load_history fails on never-
        //    used sessions — unrelated to the race we're testing)
        manager
            .save_history(&session_id, &[AgentMessage::user("seed")])
            .unwrap();

        // 3. Evict it from the in-memory cache so the next get_session hits
        //    the restore path
        manager.sessions.write().await.remove(&session_id);

        // 3. Spawn N concurrent get_session calls — they'll all race through
        //    the read-lock miss and into restore_session_from_db
        let manager_arc = std::sync::Arc::new(manager);
        let mut handles = Vec::new();
        for _ in 0..8 {
            let m = manager_arc.clone();
            let sid = session_id.clone();
            handles.push(tokio::spawn(async move {
                m.get_session(&sid).await.expect("session must restore")
            }));
        }

        let mut agents = Vec::new();
        for h in handles {
            agents.push(h.await.unwrap());
        }

        // 4. All callers MUST have gotten the same Arc — race-condition-free
        let first_addr = Arc::as_ptr(&agents[0]);
        for (i, a) in agents.iter().enumerate() {
            assert_eq!(
                Arc::as_ptr(a),
                first_addr,
                "concurrent get_session caller {i} got a different Arc — TOCTOU regression",
            );
        }

        // 5. The map must hold that same Arc as the canonical entry
        let canonical = manager_arc
            .sessions
            .read()
            .await
            .get(&session_id)
            .cloned()
            .expect("session must be cached after restore");
        assert_eq!(
            Arc::as_ptr(&canonical),
            first_addr,
            "canonical map entry differs from what callers received",
        );
    }

    #[tokio::test]
    async fn test_cancel_sender_guard_removes_on_drop() {
        let manager = create_temp_manager();

        // Register a sender
        let (tx, _rx) = tokio::sync::watch::channel(false);
        manager.register_cancel_sender("dropped-session", tx).await;

        // Drop a guard WITHOUT calling mark_cleaned_up — simulating early drop
        {
            let _guard = CancelSenderGuard::new(
                manager.cancel_senders.clone(),
                "dropped-session".to_string(),
            );
            // _guard goes out of scope here → Drop spawns a removal task
        }

        // Give the spawned cleanup task a chance to run
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // The sender should have been removed by the Drop handler
        let cancelled = manager.cancel_session("dropped-session").await;
        assert!(
            !cancelled,
            "After guard drop, sender should be removed (cancel returns false)"
        );
    }

    /// Regression: when the stream completes naturally and we call
    /// `mark_cleaned_up`, the Drop handler must NOT double-spawn a removal.
    #[tokio::test]
    async fn test_cancel_sender_guard_mark_cleaned_up_is_noop_on_drop() {
        let manager = create_temp_manager();

        let (tx, _rx) = tokio::sync::watch::channel(false);
        manager.register_cancel_sender("clean-session", tx).await;

        // Simulate natural stream end: explicit removal + mark_cleaned_up
        {
            let mut guard =
                CancelSenderGuard::new(manager.cancel_senders.clone(), "clean-session".to_string());
            manager.remove_cancel_sender("clean-session").await;
            guard.mark_cleaned_up();
            // guard drops here — Drop should be a no-op
        }

        // Map should remain empty for this session
        assert!(
            !manager
                .cancel_senders
                .read()
                .await
                .contains_key("clean-session"),
            "mark_cleaned_up should suppress the Drop-spawned removal"
        );
    }
}
