//! Core AI Agent that orchestrates LLM, memory, and tools.
//!
//! ## Architecture
//!
//! The `Agent` is a high-level AI agent that integrates LLM, tools, and memory.
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │                    Agent                            │
//! │  ┌────────────────────────────────────────────────┐ │
//! │  │  LlmInterface (LLM wrapper)                    │ │
//! │  │  - LLM runtime management                     │ │
//! │  │  - chat() / chat_stream()                     │ │
//! │  └────────────────────────────────────────────────┘ │
//! │                                                       │
//! │  + ToolRegistry (function calling)                 │
//! │  + Memory (conversation history)                    │
//! │  + SessionState (metadata tracking)                 │
//! └─────────────────────────────────────────────────────┘
//! ```

pub mod conversation_context;
pub mod fallback;
pub mod semantic_mapper;
pub mod smart_followup;
pub mod staged;
pub mod streaming;
pub mod tokenizer;
pub mod tool_parser;
pub mod types;

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Semaphore;

use futures::Stream;
use tokio::sync::RwLock;

use crate::error::NeoMindError;

use super::error::Result;
use super::llm::{ChatConfig, LlmInterface};
use crate::context::ResourceIndex;
use crate::llm_backends::{CloudConfig, CloudRuntime, OllamaConfig, OllamaRuntime};
use neomind_core::{config::agent_env_vars, llm::backend::LlmRuntime};

// Type aliases to reduce complexity
pub type SharedToolRegistry = Arc<crate::toolkit::ToolRegistry>;
pub type SharedLlmInterface = Arc<LlmInterface>;
pub type SharedSessionState = Arc<RwLock<SessionState>>;
pub type SharedSmartConversation =
    Arc<tokio::sync::RwLock<crate::smart_conversation::SmartConversationManager>>;
pub type SharedSemanticMapper = Arc<semantic_mapper::SemanticToolMapper>;
pub type EventStream = Pin<Box<dyn Stream<Item = AgentEvent> + Send>>;
pub type MessageStream = Pin<Box<dyn Stream<Item = (String, bool)> + Send>>;

pub use conversation_context::ConversationContext;
pub use fallback::{default_fallback_rules, process_fallback, FallbackRule};
pub use smart_followup::SmartFollowUpManager;
pub use streaming::{
    process_multimodal_stream_events_with_safeguards, process_stream_events_with_safeguards,
    StreamSafeguards,
};
pub use types::{
    AgentConfig, AgentEvent, AgentInternalState, AgentMessage, AgentMessageImage, AgentResponse,
    LlmBackend, SessionState, ToolCall,
};

struct ToolResultCache {
    entries: HashMap<(String, String), (String, std::time::Instant)>,
    default_ttl_seconds: u64,
}

impl ToolResultCache {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            default_ttl_seconds: 30,
        }
    }

    fn get_ttl_for_tool(&self, tool_name: &str) -> std::time::Duration {
        match tool_name {
            // Device data changes frequently - shorter TTL
            t if t.contains("device_discover") => std::time::Duration::from_secs(60),
            t if t.contains("device_query") => std::time::Duration::from_secs(30),
            t if t.contains("query_data") => std::time::Duration::from_secs(30),
            // Static data - longer TTL
            t if t.contains("list_rules") => std::time::Duration::from_secs(300),
            t if t.contains("list_agents") => std::time::Duration::from_secs(300),
            t if t.contains("get_agent") => std::time::Duration::from_secs(120),
            // Default TTL
            _ => std::time::Duration::from_secs(self.default_ttl_seconds),
        }
    }

    fn get(&self, tool_name: &str, args: &str) -> Option<String> {
        let key = (tool_name.to_string(), args.to_string());
        if let Some((result, timestamp)) = self.entries.get(&key) {
            let ttl = self.get_ttl_for_tool(tool_name);
            if timestamp.elapsed() < ttl {
                tracing::debug!(tool = %tool_name, "Cache hit");
                return Some(result.clone());
            } else {
                tracing::debug!(tool = %tool_name, "Cache entry expired");
            }
        }
        None
    }

    fn put(&mut self, tool_name: &str, args: String, result: String) {
        let key = (tool_name.to_string(), args);
        self.entries
            .insert(key, (result, std::time::Instant::now()));

        // Clean up expired entries periodically
        self.cleanup();
    }

    fn cleanup(&mut self) {
        let now = std::time::Instant::now();
        // Remove entries older than 5 minutes (max TTL)
        self.entries.retain(|_, (_, timestamp)| {
            now.duration_since(*timestamp) < std::time::Duration::from_secs(300)
        });
    }

    /// Invalidate all cache entries for a specific tool or prefix.
    fn invalidate(&mut self, tool_prefix: &str) {
        let keys_to_remove: Vec<_> = self
            .entries
            .keys()
            .filter(|(name, _)| name.starts_with(tool_prefix))
            .cloned()
            .collect();
        let count = keys_to_remove.len();
        for key in keys_to_remove {
            self.entries.remove(&key);
        }
        tracing::debug!(prefix = %tool_prefix, count = count, "Invalidated cache entries");
    }
}

/// Check if a tool action modifies state and should trigger cache invalidation.
///
/// Write actions (create, update, delete, control, send, acknowledge, etc.)
/// modify persisted state. After these operations, any cached read results
/// for the same tool are stale and must be evicted so subsequent reads
/// reflect the updated state.
fn is_write_action(arguments: &serde_json::Value) -> bool {
    let action = match arguments.get("action").and_then(|v| v.as_str()) {
        Some(a) => a,
        None => return false,
    };

    matches!(
        action,
        // CRUD write operations
        "create" | "update" | "delete"
        // Device state changes
        | "control" | "write_metric"
        // Messaging
        | "send" | "send_message" | "read"
        // Alert acknowledgment
        | "acknowledge"
    )
}

/// AI Agent that orchestrates components.
/// Shared conversation state — merged from 3 independent locks into one.
/// This reduces lock contention: conversation_context, smart_followup, and
/// last_injected_context_hash are always used together.
struct AgentSharedState {
    conversation_context: ConversationContext,
    smart_followup: SmartFollowUpManager,
    last_injected_context_hash: u64,
}

pub struct Agent {
    /// Configuration
    config: AgentConfig,
    /// Session ID
    session_id: String,
    /// Tool registry
    tools: Arc<crate::toolkit::ToolRegistry>,
    /// LLM interface
    llm_interface: Arc<LlmInterface>,
    /// Unified internal state (memory + session + llm_ready)
    /// Single lock reduces contention compared to multiple Arc<RwLock<...>>
    internal_state: Arc<tokio::sync::RwLock<AgentInternalState>>,
    /// Fallback rules for when LLM is unavailable
    fallback_rules: Vec<FallbackRule>,
    /// Process lock to prevent concurrent requests on the same session
    process_lock: Arc<tokio::sync::Mutex<()>>,
    /// Smart conversation manager - intercepts input for追问/确认
    smart_conversation:
        Arc<tokio::sync::RwLock<crate::smart_conversation::SmartConversationManager>>,
    /// Semantic mapper - converts natural language to technical IDs
    semantic_mapper: Arc<semantic_mapper::SemanticToolMapper>,
    /// Resident system capability index (CLI tree + data conventions + device-type snapshot).
    capability_index: Arc<crate::prompts::CapabilityIndex>,
    /// Shared conversation state (merged: context + followup + hash)
    shared_state: Arc<tokio::sync::RwLock<AgentSharedState>>,
    /// Tool result cache - caches recent tool executions to avoid redundant calls
    tool_result_cache: Arc<tokio::sync::RwLock<ToolResultCache>>,
    /// Memory snapshot (re-read on each user message so the agent sees its own
    /// memory writes without restarting the session).
    memory_snapshot: tokio::sync::RwLock<Option<crate::memory::MemorySnapshot>>,
    /// Semaphore limiting parallel tool executions within a single agent step
    tool_concurrency_limit: Arc<Semaphore>,
}

impl Agent {
    /// Create a new agent with custom tool registry.
    pub fn with_tools(
        config: AgentConfig,
        session_id: String,
        tools: Arc<crate::toolkit::ToolRegistry>,
    ) -> Self {
        let session_id_clone = session_id.clone();

        // Create LLM interface
        let llm_config = ChatConfig {
            model: config.model.clone(),
            temperature: config.temperature,
            top_p: 0.75,
            top_k: 20,              // Lowered for faster responses
            max_tokens: usize::MAX, // No artificial limit - let model decide
            concurrent_limit: 3,    // Default to 3 concurrent LLM requests
        };

        let llm_interface = Arc::new(
            LlmInterface::new(llm_config)
                .with_system_prompt(&config.system_prompt)
                .with_system_prompt_suffix(config.system_prompt_suffix.clone()),
        );

        // Create semantic mapper with resource index
        let resource_index = Arc::new(RwLock::new(ResourceIndex::new()));
        let semantic_mapper = Arc::new(semantic_mapper::SemanticToolMapper::new(
            resource_index.clone(),
        ));
        // Capability index shares the same ResourceIndex as the semantic mapper
        // (zero new service wiring) — see prompts/capability_index.rs.
        let capability_index =
            Arc::new(crate::prompts::CapabilityIndex::new(resource_index.clone()));

        Self {
            config,
            session_id,
            tools,
            llm_interface,
            internal_state: Arc::new(tokio::sync::RwLock::new(AgentInternalState::new(
                session_id_clone,
            ))),
            fallback_rules: default_fallback_rules(),
            process_lock: Arc::new(tokio::sync::Mutex::new(())),
            smart_conversation: Arc::new(tokio::sync::RwLock::new(
                crate::smart_conversation::SmartConversationManager::new(),
            )),
            semantic_mapper,
            capability_index,
            shared_state: Arc::new(tokio::sync::RwLock::new(AgentSharedState {
                conversation_context: ConversationContext::new(),
                smart_followup: SmartFollowUpManager::new(),
                last_injected_context_hash: 0,
            })),
            tool_result_cache: Arc::new(tokio::sync::RwLock::new(ToolResultCache::new())),
            memory_snapshot: tokio::sync::RwLock::new(None),
            tool_concurrency_limit: Arc::new(Semaphore::new(
                std::env::var("NEOMIND_TOOL_CONCURRENCY")
                    .ok()
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .filter(|&n| n >= 1)
                    .unwrap_or(5),
            )),
        }
    }
}
impl Agent {
    /// Get the LLM interface (for capability checks).
    pub fn llm_interface(&self) -> Arc<LlmInterface> {
        Arc::clone(&self.llm_interface)
    }
}
impl Agent {
    /// Set pinned skill IDs for this session (user-selected skills).
    pub async fn set_pinned_skills(&self, skills: Vec<String>) {
        self.llm_interface.set_pinned_skills(skills).await;
    }
}
impl Agent {
    /// Create a new agent with empty tool registry.
    /// Tools should be configured externally through the session manager.
    pub fn new(config: AgentConfig, session_id: String) -> Self {
        // Build tool registry - start empty, tools will be added by session manager
        let mut registry = crate::toolkit::ToolRegistryBuilder::new().build();

        // Add agent-specific tools
        use crate::tools::{AskUserTool, ClarifyIntentTool, ConfirmActionTool};

        // === 添加用户交互工具 ===
        // ask_user: 向用户询问缺失信息
        let ask_user_tool = AskUserTool::new();
        registry.register(std::sync::Arc::new(ask_user_tool));

        // confirm_action: 二次确认危险操作
        let confirm_tool = ConfirmActionTool::new();
        registry.register(std::sync::Arc::new(confirm_tool));

        // clarify_intent: 澄清模糊意图
        let clarify_tool = ClarifyIntentTool::new();
        registry.register(std::sync::Arc::new(clarify_tool));

        Self::with_tools(config, session_id, Arc::new(registry))
    }
}
impl Agent {
    /// Create with default config and empty tools.
    pub fn with_session(session_id: String) -> Self {
        Self::new(AgentConfig::default(), session_id)
    }
}
impl Agent {
    /// Configure the LLM backend.
    pub async fn configure_llm(&self, backend: LlmBackend) -> Result<()> {
        tracing::debug!(backend = ?backend, "Agent::configure_llm called");

        // Load timeout from environment variable (or use defaults)
        let ollama_timeout = agent_env_vars::ollama_timeout_secs();
        let cloud_timeout = agent_env_vars::cloud_timeout_secs();

        tracing::debug!(
            ollama_timeout_secs = ollama_timeout,
            cloud_timeout_secs = cloud_timeout,
            "Configuring LLM with timeout values"
        );

        let (llm, model_name) = match backend {
            LlmBackend::Ollama {
                endpoint,
                model,
                capabilities,
            } => {
                tracing::debug!(
                    endpoint = %endpoint, model = %model, timeout = ollama_timeout,
                    capabilities = ?capabilities,
                    "Creating OllamaRuntime"
                );
                let config = OllamaConfig::new(&model)
                    .with_endpoint(&endpoint)
                    .with_timeout_secs(ollama_timeout);
                let mut runtime =
                    OllamaRuntime::new(config).map_err(|e| NeoMindError::llm(e.to_string()))?;

                // [fresh runtime probe] The direct chat runtime built here is
                // cached for the process lifetime and is NOT rebuilt when the
                // instance manager later refreshes stored capabilities — so a
                // creation-time registry default (max_context = 128000 for
                // unknown models) would survive forever in the prompt budget
                // while the server enforces its real window (measured
                // 2026-08-17: every turn sent a full-history prompt that
                // overflowed and got rescued by the compact-retry ladder).
                // Probe /api/show NOW; the probe is authoritative except for
                // an explicit user override (gotcha #3).
                // The probe is authoritative for max_context/tools (no user
                // override channel exists for those); multimodal/thinking keep
                // the stored value when present — the effective stored value
                // may already encode a user override we cannot see from here
                // (BackendCapabilities carries no override marker; that lives
                // on the instance record) — gotcha #3.
                let detected = runtime.fetch_capabilities_from_api().await;
                match (&detected, &capabilities) {
                    (Some(d), stored) => {
                        let (multimodal, thinking) = match stored {
                            Some(c) => (c.multimodal, c.thinking_display),
                            None => (d.supports_multimodal, d.supports_thinking),
                        };
                        tracing::info!(
                            multimodal,
                            thinking,
                            tools = d.supports_tools,
                            max_ctx = d.max_context,
                            "OllamaRuntime capabilities resolved from /api/show"
                        );
                        runtime = runtime.with_capabilities_override(
                            multimodal,
                            thinking,
                            d.supports_tools,
                            d.max_context,
                        );
                    }
                    (None, Some(caps)) => {
                        // Probe failed — stored values. Conservative context
                        // fallback: OVER-claiming context turns every turn into
                        // an overflow-then-retry (wasted full prefill);
                        // under-claiming only trims history.
                        tracing::debug!(
                            multimodal = %caps.multimodal,
                            thinking_display = %caps.thinking_display,
                            function_calling = %caps.function_calling,
                            max_context = %caps.max_context.unwrap_or(8192),
                            "Applying capabilities override to OllamaRuntime (probe failed)"
                        );
                        runtime = runtime.with_capabilities_override(
                            caps.multimodal,
                            caps.thinking_display,
                            caps.function_calling,
                            caps.max_context.unwrap_or(8192),
                        );
                    }
                    (None, None) => {
                        tracing::debug!(
                            "No capabilities and no probe for OllamaRuntime, using runtime defaults"
                        );
                    }
                }

                (Arc::new(runtime) as Arc<dyn LlmRuntime>, model)
            }
            LlmBackend::Qwen {
                api_key,
                endpoint,
                model,
                capabilities,
            } => {
                tracing::debug!(
                    endpoint = %endpoint, model = %model, timeout = cloud_timeout,
                    "Creating CloudRuntime for Qwen"
                );
                // Always use Qwen provider to ensure correct vision model detection
                let config = CloudConfig::qwen(&api_key)
                    .with_model(&model)
                    .with_timeout_secs(cloud_timeout)
                    .with_base_url_opt(if endpoint.is_empty() {
                        None
                    } else {
                        Some(endpoint.clone())
                    });
                let runtime = CloudRuntime::new(config).map_err(
                    |e: neomind_core::llm::backend::LlmError| NeoMindError::llm(e.to_string()),
                )?;
                let runtime = apply_cloud_capabilities(runtime, capabilities);
                (Arc::new(runtime) as Arc<dyn LlmRuntime>, model)
            }
            LlmBackend::DeepSeek {
                api_key,
                endpoint,
                model,
                capabilities,
            } => {
                tracing::debug!(
                    endpoint = %endpoint, model = %model, timeout = cloud_timeout,
                    "Creating CloudRuntime for DeepSeek"
                );
                // Always use DeepSeek provider to ensure correct vision model detection
                let config = CloudConfig::deepseek(&api_key)
                    .with_model(&model)
                    .with_timeout_secs(cloud_timeout)
                    .with_base_url_opt(if endpoint.is_empty() {
                        None
                    } else {
                        Some(endpoint.clone())
                    });
                let runtime = CloudRuntime::new(config).map_err(
                    |e: neomind_core::llm::backend::LlmError| NeoMindError::llm(e.to_string()),
                )?;
                let runtime = apply_cloud_capabilities(runtime, capabilities);
                (Arc::new(runtime) as Arc<dyn LlmRuntime>, model)
            }
            LlmBackend::GLM {
                api_key,
                endpoint,
                model,
                capabilities,
            } => {
                tracing::debug!(
                    endpoint = %endpoint, model = %model, timeout = cloud_timeout,
                    "Creating CloudRuntime for GLM"
                );
                // Always use GLM provider to ensure correct vision model detection
                let config = CloudConfig::glm(&api_key)
                    .with_model(&model)
                    .with_timeout_secs(cloud_timeout)
                    .with_base_url_opt(if endpoint.is_empty() {
                        None
                    } else {
                        Some(endpoint.clone())
                    });
                let runtime = CloudRuntime::new(config).map_err(
                    |e: neomind_core::llm::backend::LlmError| NeoMindError::llm(e.to_string()),
                )?;
                let runtime = apply_cloud_capabilities(runtime, capabilities);
                (Arc::new(runtime) as Arc<dyn LlmRuntime>, model)
            }
            LlmBackend::MiniMax {
                api_key,
                endpoint,
                model,
                capabilities,
            } => {
                tracing::debug!(
                    endpoint = %endpoint, model = %model, timeout = cloud_timeout,
                    "Creating CloudRuntime for MiniMax"
                );
                // Always use MiniMax provider to ensure correct vision model detection
                let config = CloudConfig::minimax(&api_key)
                    .with_model(&model)
                    .with_timeout_secs(cloud_timeout)
                    .with_base_url_opt(if endpoint.is_empty() {
                        None
                    } else {
                        Some(endpoint.clone())
                    });
                let runtime = CloudRuntime::new(config).map_err(
                    |e: neomind_core::llm::backend::LlmError| NeoMindError::llm(e.to_string()),
                )?;
                let runtime = apply_cloud_capabilities(runtime, capabilities);
                (Arc::new(runtime) as Arc<dyn LlmRuntime>, model)
            }
            LlmBackend::Anthropic {
                api_key,
                endpoint,
                model,
                capabilities,
            } => {
                tracing::debug!(
                    endpoint = %endpoint, model = %model, timeout = cloud_timeout,
                    "Creating CloudRuntime for Anthropic"
                );
                // Always use Anthropic provider to ensure correct vision model detection
                let config = CloudConfig::anthropic(&api_key)
                    .with_model(&model)
                    .with_timeout_secs(cloud_timeout)
                    .with_base_url_opt(if endpoint.is_empty() {
                        None
                    } else {
                        Some(endpoint.clone())
                    });
                let runtime = CloudRuntime::new(config).map_err(
                    |e: neomind_core::llm::backend::LlmError| NeoMindError::llm(e.to_string()),
                )?;
                let runtime = apply_cloud_capabilities(runtime, capabilities);
                (Arc::new(runtime) as Arc<dyn LlmRuntime>, model)
            }
            LlmBackend::Google {
                api_key,
                endpoint,
                model,
                capabilities,
            } => {
                tracing::debug!(
                    endpoint = %endpoint, model = %model, timeout = cloud_timeout,
                    "Creating CloudRuntime for Google"
                );
                // Always use Google provider to ensure correct vision model detection
                let config = CloudConfig::google(&api_key)
                    .with_model(&model)
                    .with_timeout_secs(cloud_timeout)
                    .with_base_url_opt(if endpoint.is_empty() {
                        None
                    } else {
                        Some(endpoint.clone())
                    });
                let runtime = CloudRuntime::new(config).map_err(
                    |e: neomind_core::llm::backend::LlmError| NeoMindError::llm(e.to_string()),
                )?;
                let runtime = apply_cloud_capabilities(runtime, capabilities);
                (Arc::new(runtime) as Arc<dyn LlmRuntime>, model)
            }
            LlmBackend::XAi {
                api_key,
                endpoint,
                model,
                capabilities,
            } => {
                tracing::debug!(
                    endpoint = %endpoint, model = %model, timeout = cloud_timeout,
                    "Creating CloudRuntime for xAI"
                );
                // Always use Grok provider to ensure correct vision model detection
                let config = CloudConfig::grok(&api_key)
                    .with_model(&model)
                    .with_timeout_secs(cloud_timeout)
                    .with_base_url_opt(if endpoint.is_empty() {
                        None
                    } else {
                        Some(endpoint.clone())
                    });
                let runtime = CloudRuntime::new(config).map_err(
                    |e: neomind_core::llm::backend::LlmError| NeoMindError::llm(e.to_string()),
                )?;
                let runtime = apply_cloud_capabilities(runtime, capabilities);
                (Arc::new(runtime) as Arc<dyn LlmRuntime>, model)
            }
            LlmBackend::OpenAi {
                api_key,
                endpoint,
                model,
                capabilities,
            } => {
                tracing::debug!(
                    endpoint = %endpoint, model = %model, timeout = cloud_timeout,
                    "Creating CloudRuntime for OpenAI"
                );
                // Always use OpenAI provider to ensure correct vision model detection
                let config = CloudConfig::openai(&api_key)
                    .with_model(&model)
                    .with_timeout_secs(cloud_timeout)
                    .with_base_url_opt(if endpoint.is_empty() {
                        None
                    } else {
                        Some(endpoint.clone())
                    });
                let runtime = CloudRuntime::new(config).map_err(
                    |e: neomind_core::llm::backend::LlmError| NeoMindError::llm(e.to_string()),
                )?;
                let runtime = apply_cloud_capabilities(runtime, capabilities);
                (Arc::new(runtime) as Arc<dyn LlmRuntime>, model)
            }
            #[cfg(feature = "llamacpp")]
            LlmBackend::LlamaCpp {
                endpoint,
                model,
                capabilities,
            } => {
                tracing::info!(
                    endpoint = %endpoint, model = %model, timeout = ollama_timeout,
                    capabilities = ?capabilities,
                    "Creating LlamaCppRuntime"
                );
                let config = crate::llm_backends::backends::llamacpp::LlamaCppConfig::new(&model)
                    .with_endpoint(&endpoint)
                    .with_timeout_secs(ollama_timeout);
                let mut runtime =
                    crate::llm_backends::backends::llamacpp::LlamaCppRuntime::new(config)
                        .map_err(|e| NeoMindError::llm(e.to_string()))?;

                // [fresh runtime probe] Same rationale as the Ollama arm: the
                // direct chat runtime is cached for the process lifetime, so
                // bake the REAL window in at creation instead of trusting the
                // stored registry default (128000). /props is authoritative
                // for a local llama-server; only a user override wins over it.
                // Same authority split as the Ollama arm: probe wins for
                // max_context/tools (no override channel); multimodal/thinking
                // keep stored when present (stored may encode a user override
                // invisible at this layer) — gotcha #3.
                let detected = runtime.detect_capabilities().await;
                match (&detected, &capabilities) {
                    (Some(d), stored) => {
                        let (multimodal, thinking) = match stored {
                            Some(c) => (c.multimodal, c.thinking_display),
                            None => (d.supports_multimodal, d.supports_thinking),
                        };
                        tracing::info!(
                            multimodal,
                            thinking,
                            tools = d.supports_tools,
                            max_ctx = d.max_context,
                            "LlamaCppRuntime capabilities resolved from /props"
                        );
                        runtime = runtime.with_capabilities_override(
                            multimodal,
                            thinking,
                            d.supports_tools,
                            d.max_context,
                        );
                    }
                    (None, Some(caps)) => {
                        // Probe failed — stored values + conservative context
                        // fallback (over-claiming = overflow-then-retry churn;
                        // under-claiming = harmless trimming).
                        tracing::debug!(
                            multimodal = %caps.multimodal,
                            thinking_display = %caps.thinking_display,
                            function_calling = %caps.function_calling,
                            max_context = %caps.max_context.unwrap_or(8192),
                            "Applying capabilities override to LlamaCppRuntime (probe failed)"
                        );
                        runtime = runtime.with_capabilities_override(
                            caps.multimodal,
                            caps.thinking_display,
                            caps.function_calling,
                            caps.max_context.unwrap_or(8192),
                        );
                    }
                    (None, None) => {
                        tracing::debug!(
                            "No capabilities and no probe for LlamaCppRuntime, using runtime defaults"
                        );
                    }
                }

                (Arc::new(runtime) as Arc<dyn LlmRuntime>, model)
            }
            #[cfg(not(feature = "llamacpp"))]
            LlmBackend::LlamaCpp { .. } => {
                return Err(NeoMindError::llm(
                    "llama.cpp backend is not available (feature not enabled)".to_string(),
                ));
            }
        };

        // Update model override
        self.llm_interface.update_model(model_name).await;

        tracing::debug!(
            backend_id = %llm.backend_id().as_str(),
            model = %llm.model_name(),
            "Setting LLM runtime on interface"
        );

        self.llm_interface.set_llm(llm).await;
        self.internal_state.write().await.set_llm_ready(true);

        // Set tool definitions for function calling
        self.update_tool_definitions().await;

        Ok(())
    }
}
impl Agent {
    /// Set a custom LLM runtime directly (for testing purposes).
    pub async fn set_custom_llm(&self, llm: Arc<dyn LlmRuntime>) {
        self.llm_interface.set_llm(llm).await;
        self.internal_state.write().await.set_llm_ready(true);
        self.update_tool_definitions().await;
    }
}
impl Agent {
    /// Update tool definitions in the LLM interface.
    /// Uses tool definitions from the actual tool registry, filtered through
    /// the disabled set so tools the user turned off on the Extensions page
    /// never reach the LLM (covers both scheduled and chat/session paths).
    /// Also dynamically updates the system prompt to include tool descriptions.
    pub async fn update_tool_definitions(&self) {
        use neomind_core::llm::backend::ToolDefinition as CoreToolDefinition;

        // allowed_tool_defs() already filters out disabled tools AND applies
        // the per-session allowlist. Use it instead of iterating list() + get()
        // so the chat path can't leak disabled or out-of-profile tools.
        let core_defs: Vec<CoreToolDefinition> = self
            .allowed_tool_defs()
            .into_iter()
            .map(|def| CoreToolDefinition {
                name: def.name,
                description: def.description,
                parameters: def.parameters,
            })
            .collect();

        let tool_count = core_defs.len();
        self.llm_interface.set_tool_definitions(core_defs).await;

        // Dynamically update system prompt with tool descriptions
        let dynamic_prompt = self.generate_dynamic_system_prompt().await;
        self.llm_interface.set_system_prompt(&dynamic_prompt).await;

        tracing::debug!(
            "Updated {} tool definitions for LLM (from registry)",
            tool_count
        );
    }
}
impl Agent {
    /// Tool definitions visible to this session's LLM: the registry's
    /// definitions (disabled tools already excluded) filtered through
    /// `config.allowed_tools`. Empty allowlist = all tools. The
    /// user-interaction tools (ask_user / confirm_action / clarify_intent)
    /// are never filtered out — they are UX, not domain capability.
    fn allowed_tool_defs(&self) -> Vec<crate::toolkit::tool::ToolDefinition> {
        let defs = self.tools.definitions_for_llm();
        if self.config.allowed_tools.is_empty() {
            return defs;
        }
        const ALWAYS_KEEP: [&str; 3] = ["ask_user", "confirm_action", "clarify_intent"];
        defs.into_iter()
            .filter(|d| {
                ALWAYS_KEEP.contains(&d.name.as_str())
                    || self.config.allowed_tools.iter().any(|a| a == &d.name)
            })
            .collect()
    }
}
impl Agent {
    /// Generate a dynamic system prompt with tool descriptions.
    /// This ensures the prompt always reflects the currently available tools.
    async fn generate_dynamic_system_prompt(&self) -> String {
        // Generate base prompt (static parts: system_prompt + tools)
        let mut prompt = self.generate_base_prompt();

        // === 动态注入系统资源上下文 ===
        // 这确保 LLM 能够感知当前系统中的实际设备、规则和工作流
        let resource_context = self.semantic_mapper.get_semantic_context().await;
        if !resource_context.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(&resource_context);
        }

        // === System capability index (command tree + data conventions + device-type snapshot) ===
        let capability = self.capability_index.build().await;
        if !capability.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(&capability);
        }

        // === Memory snapshot injection (re-read each user message) ===
        if let Some(snapshot) = self.memory_snapshot.read().await.as_ref() {
            let section = snapshot.to_prompt_section();
            if !section.is_empty() {
                prompt.push_str(&section);
            }
        }

        prompt
    }
}
impl Agent {
    /// Generate base prompt (static parts: system_prompt + tools).
    /// This avoids rebuilding tool descriptions on every request.
    fn generate_base_prompt(&self) -> String {
        let mut prompt = String::from(self.config.system_prompt.trim());

        prompt.push_str("\n\n## Available Tools (Quick Reference)\n\n");

        // allowed_tool_defs() filters out disabled tools and applies the
        // per-session allowlist so the text prompt stays in sync with the
        // function-calling schema.
        let defs = self.allowed_tool_defs();
        let extension_defs: Vec<_> = defs.iter().filter(|d| d.name.contains(':')).collect();

        for def in defs.iter().filter(|d| !d.name.contains(':')) {
            prompt.push_str(&format!("**{}**: {}\n", def.name, def.description));
        }

        if !extension_defs.is_empty() {
            prompt.push_str("\n### Extension Tools\n");
            prompt.push_str("These tools are provided by installed extensions. Use them when users ask about related functionality.\n\n");
            for def in &extension_defs {
                prompt.push_str(&format!("**{}**: {}\n", def.name, def.description));
                if let Some(params) = def.parameters.get("properties") {
                    prompt.push_str("  Parameters:\n");
                    if let Some(obj) = params.as_object() {
                        for (pname, pschema) in obj {
                            let desc = pschema
                                .get("description")
                                .and_then(|d| d.as_str())
                                .unwrap_or("");
                            prompt.push_str(&format!("    - `{}`: {}\n", pname, desc));
                        }
                    }
                }
                prompt.push('\n');
            }
        }

        prompt.push_str("## Usage Guide\n");
        prompt.push_str(
            "- Use `shell` tool with `neomind` CLI commands for all platform operations\n",
        );
        prompt.push_str("- Multiple tool calls can be executed in parallel for faster response\n");

        prompt
    }
}
impl Agent {
    /// Get the session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
}
impl Agent {
    /// Set the memory snapshot for this session. Called on each user message
    /// (not just the first) so the agent sees memory writes from the previous
    /// turn. Also pushes the snapshot's prompt section to the LLM interface.
    pub async fn set_memory_snapshot(&self, snapshot: crate::memory::MemorySnapshot) {
        let section = snapshot.to_prompt_section();
        if !section.is_empty() {
            self.llm_interface.set_memory_context(Some(section)).await;
        }
        *self.memory_snapshot.write().await = Some(snapshot);
    }
}
impl Agent {
    /// Check if a memory snapshot has been loaded.
    pub async fn has_memory_snapshot(&self) -> bool {
        self.memory_snapshot.read().await.is_some()
    }
}
impl Agent {
    /// Get the session state.
    pub async fn state(&self) -> SessionState {
        self.internal_state.read().await.session.clone()
    }
}
impl Agent {
    /// Get the conversation history.
    pub async fn history(&self) -> Vec<AgentMessage> {
        self.internal_state.read().await.memory.clone()
    }
}
impl Agent {
    /// Restore conversation history from persisted data.
    pub async fn restore_history(&self, messages: Vec<AgentMessage>) {
        self.internal_state.write().await.restore_memory(messages);
    }
}
impl Agent {
    /// Clear conversation history.
    pub async fn clear_history(&self) {
        self.internal_state.write().await.clear_memory();
    }
}
/// Apply storage-derived `BackendCapabilities` to a freshly-constructed
/// `CloudRuntime`, so that cloud backends in the chat / configure_llm flow
/// honor the same layered capability detection (registry → heuristic → user
/// override) that Ollama and llama.cpp already honor.
///
/// Without this, cloud backends would fall back to the static
/// `is_vision_model()` pattern match in `openai.rs`, which can disagree with
/// `detect_vision_capability()` and silently drops user overrides.
fn apply_cloud_capabilities(
    mut runtime: CloudRuntime,
    capabilities: Option<neomind_core::BackendCapabilities>,
) -> CloudRuntime {
    if let Some(caps) = capabilities {
        tracing::debug!(
            multimodal = %caps.multimodal,
            thinking_display = %caps.thinking_display,
            function_calling = %caps.function_calling,
            max_context = %caps.max_context.unwrap_or(128000),
            "Applying capabilities override to CloudRuntime"
        );
        runtime = runtime.with_capabilities_override(
            caps.multimodal,
            caps.thinking_display,
            caps.function_calling,
            caps.max_context.unwrap_or(128000),
        );
    } else {
        tracing::debug!(
            "No capabilities provided for CloudRuntime, using provider-default detection"
        );
    }
    runtime
}

/// Drop implementation for Agent to log session lifecycle for observability.
///
/// This helps with production debugging by tracking:
/// - When sessions are destroyed
/// - Session duration and message count
/// - Resource cleanup verification
impl Drop for Agent {
    fn drop(&mut self) {
        // Note: This is a synchronous drop, so we can't access the async internal_state
        // However, we can log basic information about the session being destroyed

        tracing::debug!(
            session_id = %self.session_id,
            agent_name = %self.config.name,
            model = %self.config.model,
            tools_count = self.tools.list().len(),
            "Agent instance dropped (session destroyed)"
        );
    }
}

// Domain submodules — impl Agent blocks live across these files.
mod context;
mod execution;
mod process;
mod tools;
pub use context::*;

#[cfg(test)]
mod tests;
