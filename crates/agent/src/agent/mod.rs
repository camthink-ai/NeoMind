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

pub mod fallback;
pub mod formatter;
pub mod semantic_mapper;
pub mod staged;
pub mod streaming;
pub mod tool_parser;
pub mod tokenizer;
pub mod types;
pub mod conversation_context;
pub mod smart_followup;
pub mod tool_confidence;
pub mod error_recovery;
pub mod model_selection;
pub mod intent_classifier;

use std::pin::Pin;
use std::sync::Arc;

use futures::{Stream, StreamExt};
use tokio::sync::RwLock;

// Re-export error types
pub use crate::error::NeoTalkError;
use serde_json::Value;

use super::error::Result;
use super::llm::{ChatConfig, LlmInterface};
use crate::context::ResourceIndex;
use edge_ai_core::{
    Message,
    llm::backend::LlmRuntime,
    config::agent_env_vars,
};
use edge_ai_llm::{CloudConfig, CloudRuntime, OllamaConfig, OllamaRuntime};

pub use fallback::{FallbackRule, default_fallback_rules, process_fallback};
pub use formatter::{format_summary, format_tool_result};
pub use semantic_mapper::{
    SemanticToolMapper, SemanticMapping, SemanticMatchType,
    DeviceMapping, RuleMapping, WorkflowMapping, MappingStats,
};
pub use streaming::{
    StreamSafeguards, events_to_string_stream, process_stream_events,
    process_stream_events_with_safeguards, process_multimodal_stream_events,
    process_multimodal_stream_events_with_safeguards,
};
pub use types::{
    AgentConfig, AgentEvent, AgentInternalState, AgentMessage, AgentMessageImage, AgentResponse,
    LlmBackend, SessionState, ToolCall,
};
pub use conversation_context::{
    ConversationContext, ConversationTopic, EntityReference, EntityType,
};
pub use smart_followup::{
    SmartFollowUpManager, FollowUpAnalysis, FollowUpItem, FollowUpType,
    FollowUpPriority, DetectedIntent, AvailableDevice,
};
pub use tool_confidence::{
    ToolConfidenceManager, ToolExecutionResult, ConfidenceLevel,
    ConfidenceConfig, ExecutionStatus,
};
pub use error_recovery::{
    ErrorRecoveryManager, ErrorInfo, ErrorCategory, RecoveryStrategy,
    RecoveryAction, FallbackPlan, ErrorRecoveryConfig, ErrorStats,
};
pub use model_selection::{
    ModelSelectionManager, ModelSelection, ModelInfo, ModelType,
    ModelCapabilities, ModelStatus, TaskComplexity, ModelSelectionConfig,
};

/// Maximum number of tool calls allowed per request to prevent infinite loops
/// Note: This constant is kept for backward compatibility. Config values take precedence.
const MAX_TOOL_CALLS_PER_REQUEST_DEFAULT: usize = 5;

/// === ANTHROPIC-STYLE IMPROVEMENT: Tool Result Clearing ===
///
/// Compacts old tool result messages into concise summaries.
/// This follows Anthropic's guidance: "One of the safest lightest touch forms
/// of compaction is tool result clearing – once a tool has been called deep
/// in the message history, why would the agent need to see the raw result again?"
///
/// Rules:
/// - Keep the most recent N tool results intact (configurable, default: 2)
/// - Older tool results are compressed to one-line summaries
/// - User and system messages are always kept
pub fn compact_tool_results(messages: &[AgentMessage], keep_recent: usize) -> Vec<AgentMessage> {
    let mut result = Vec::new();
    let mut tool_result_count = 0;

    for msg in messages.iter().rev() {
        // Always keep user and system messages
        if msg.role == "user" || msg.role == "system" {
            result.push(msg.clone());
            continue;
        }

        // Check if this is a tool result message (has tool_calls)
        if msg.tool_calls.is_some() && msg.tool_calls.as_ref().is_some_and(|t| !t.is_empty()) {
            tool_result_count += 1;

            // Keep recent tool results intact
            if tool_result_count <= keep_recent {
                result.push(msg.clone());
            } else {
                // Compress old tool results to a brief summary
                let tool_names: Vec<&str> = msg
                    .tool_calls
                    .as_ref()
                    .iter()
                    .flat_map(|calls| calls.iter().map(|t| t.name.as_str()))
                    .collect();

                // Create a compacted summary message
                let summary = if tool_names.len() == 1 {
                    format!("[之前调用了工具: {}]", tool_names[0])
                } else {
                    format!("[之前调用了工具: {}]", tool_names.join(", "))
                };

                result.push(AgentMessage {
                    role: msg.role.clone(),
                    content: summary,
                    tool_calls: None, // Remove actual tool data to save tokens
                    tool_call_id: None,
                    tool_call_name: None,
                    thinking: None, // Never keep thinking in compacted messages
                    images: None,
                    timestamp: msg.timestamp,
                });
            }
        } else {
            // Regular assistant message - keep it
            result.push(msg.clone());
        }
    }

    result.reverse();
    result
}

/// === ANTHROPIC-STYLE IMPROVEMENT: Context Window with Tool Result Clearing ===
///
/// Builds conversation context with:
/// 1. Tool result clearing for old messages
/// 2. Token-based windowing with accurate estimation
/// 3. Always keep recent messages (minimum 4) for context continuity
///
/// The `max_tokens` parameter allows dynamic context sizing based on the model's actual capacity.
/// This prevents wasting model capability (e.g., using 5k context with a 32k model) while
/// also preventing errors from exceeding the model's limit (e.g., using 12k context with an 8k model).
fn build_context_window(messages: &[AgentMessage], max_tokens: usize) -> Vec<AgentMessage> {
    // Use the improved tokenizer module for accurate token estimation
    use tokenizer::select_messages_within_token_limit;

    // First, apply tool result clearing
    let compacted = compact_tool_results(messages, 2); // Default: keep 2 recent results

    // Select messages within token limit using improved estimation
    let selected_refs = select_messages_within_token_limit(
        &compacted,
        max_tokens,
        4, // Always keep at least 4 recent messages
    );

    // Convert references to owned messages
    selected_refs.into_iter().cloned().collect()
}

/// Default context window size to use when model capacity is unknown.
/// This is a conservative value that works for most models.
const DEFAULT_CONTEXT_TOKENS: usize = 8_000;

/// AI Agent that orchestrates components.
pub struct Agent {
    /// Configuration
    config: AgentConfig,
    /// Session ID
    session_id: String,
    /// Tool registry
    tools: Arc<edge_ai_tools::ToolRegistry>,
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
    smart_conversation: Arc<tokio::sync::RwLock<crate::smart_conversation::SmartConversationManager>>,
    /// Semantic mapper - converts natural language to technical IDs
    semantic_mapper: Arc<semantic_mapper::SemanticToolMapper>,
    /// Conversation context - enables continuous dialogue with entity references
    conversation_context: Arc<tokio::sync::RwLock<ConversationContext>>,
}

impl Agent {
    /// Create a new agent with custom tool registry.
    pub fn with_tools(
        config: AgentConfig,
        session_id: String,
        tools: Arc<edge_ai_tools::ToolRegistry>,
    ) -> Self {
        let session_id_clone = session_id.clone();

        // Create LLM interface
        let llm_config = ChatConfig {
            model: config.model.clone(),
            temperature: config.temperature,
            top_p: 0.75,
            top_k: 20,  // Lowered for faster responses
            max_tokens: usize::MAX, // No artificial limit - let model decide
            concurrent_limit: 3,    // Default to 3 concurrent LLM requests
        };

        let llm_interface =
            Arc::new(LlmInterface::new(llm_config).with_system_prompt(&config.system_prompt));

        // Create semantic mapper with resource index
        let resource_index = Arc::new(RwLock::new(ResourceIndex::new()));
        let semantic_mapper = Arc::new(semantic_mapper::SemanticToolMapper::new(resource_index));

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
                crate::smart_conversation::SmartConversationManager::new()
            )),
            semantic_mapper,
            conversation_context: Arc::new(tokio::sync::RwLock::new(ConversationContext::new())),
        }
    }

    /// Get internal state for streaming (used by streaming module).
    pub fn internal_state(&self) -> Arc<tokio::sync::RwLock<AgentInternalState>> {
        self.internal_state.clone()
    }

    /// Get the LLM interface (for capability checks).
    pub fn llm_interface(&self) -> Arc<LlmInterface> {
        Arc::clone(&self.llm_interface)
    }

    /// Create a new agent with empty tool registry.
    /// Tools should be configured externally through the session manager.
    pub fn new(config: AgentConfig, session_id: String) -> Self {
        // Build tool registry - start empty, tools will be added by session manager
        let mut registry = edge_ai_tools::ToolRegistryBuilder::new()
            .build();

        // Add agent-specific tools
        use crate::tools::{ThinkTool, ToolSearchTool};
        use crate::tools::{AskUserTool, ConfirmActionTool, ClarifyIntentTool};

        // Create tool search tool (starts with empty tool list)
        let tool_search = ToolSearchTool::from_definitions(&[]);
        registry.register(std::sync::Arc::new(tool_search));

        // Create and register think tool
        let think_tool = ThinkTool::new();
        registry.register(std::sync::Arc::new(think_tool));

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

    /// Create with default config and empty tools.
    pub fn with_session(session_id: String) -> Self {
        Self::new(AgentConfig::default(), session_id)
    }

    /// Set custom fallback rules.
    pub fn with_fallback_rules(mut self, rules: Vec<FallbackRule>) -> Self {
        self.fallback_rules = rules;
        self
    }

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
            LlmBackend::Ollama { endpoint, model } => {
                tracing::info!(
                    endpoint = %endpoint, model = %model, timeout = ollama_timeout,
                    "Creating OllamaRuntime"
                );
                let config = OllamaConfig::new(&model)
                    .with_endpoint(&endpoint)
                    .with_timeout_secs(ollama_timeout);
                let runtime =
                    OllamaRuntime::new(config).map_err(|e| NeoTalkError::llm(e.to_string()))?;
                (Arc::new(runtime) as Arc<dyn LlmRuntime>, model)
            }
            LlmBackend::OpenAi {
                api_key,
                endpoint,
                model,
            } => {
                tracing::info!(
                    endpoint = %endpoint, model = %model, timeout = cloud_timeout,
                    "Creating CloudRuntime for OpenAI"
                );
                let config = CloudConfig::openai(&api_key)
                    .with_timeout_secs(cloud_timeout);
                let config = if endpoint.is_empty() {
                    config.with_model(&model)
                } else {
                    // Custom endpoint
                    CloudConfig::custom(&api_key, &endpoint)
                        .with_model(&model)
                        .with_timeout_secs(cloud_timeout)
                };
                let runtime =
                    CloudRuntime::new(config).map_err(|e| NeoTalkError::llm(e.to_string()))?;
                (Arc::new(runtime) as Arc<dyn LlmRuntime>, model)
            }
        };

        // Update model override
        self.llm_interface.update_model(model_name).await;

        self.llm_interface.set_llm(llm).await;
        self.internal_state.write().await.set_llm_ready(true);

        // Set tool definitions for function calling
        self.update_tool_definitions().await;

        Ok(())
    }

    /// Set a custom LLM runtime directly (for testing purposes).
    pub async fn set_custom_llm(&self, llm: Arc<dyn LlmRuntime>) {
        self.llm_interface.set_llm(llm).await;
        self.internal_state.write().await.set_llm_ready(true);
        self.update_tool_definitions().await;
    }

    /// Update tool definitions in the LLM interface.
    /// Uses simplified tool definitions for better LLM understanding.
    /// Also dynamically updates the system prompt to include tool descriptions.
    pub async fn update_tool_definitions(&self) {
        use edge_ai_core::llm::backend::ToolDefinition as CoreToolDefinition;
        use edge_ai_tools::simplified;

        // Use simplified tool definitions for LLM function calling
        let simplified_tools = simplified::get_simplified_tools();
        let core_defs: Vec<CoreToolDefinition> = simplified_tools
            .iter()
            .map(|tool| {
                // Build simplified parameters schema
                let mut properties = serde_json::Map::new();
                let mut required = Vec::new();

                for param in &tool.required {
                    required.push(param.clone());
                    properties.insert(param.clone(), serde_json::json!({
                        "type": "string",
                        "description": param
                    }));
                }
                for (param, info) in &tool.optional {
                    properties.insert(param.clone(), serde_json::json!({
                        "type": "string",
                        "description": info.description,
                        "default": info.default
                    }));
                }

                CoreToolDefinition {
                    name: tool.name.clone(),
                    description: format!("{} (别名: {})", tool.description, tool.aliases.join(", ")),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": properties,
                        "required": required
                    }),
                }
            })
            .collect();

        let tool_count = core_defs.len();
        self.llm_interface.set_tool_definitions(core_defs).await;

        // Dynamically update system prompt with tool descriptions
        let dynamic_prompt = self.generate_dynamic_system_prompt(&simplified_tools).await;
        self.llm_interface.set_system_prompt(&dynamic_prompt).await;

        tracing::debug!("Updated {} simplified tool definitions for LLM", tool_count);
    }

    /// Generate a dynamic system prompt with tool descriptions.
    /// This ensures the prompt always reflects the currently available tools.
    async fn generate_dynamic_system_prompt(&self, simplified_tools: &[edge_ai_tools::simplified::LlmToolDefinition]) -> String {
        // Generate base prompt (static parts: system_prompt + tools)
        let mut prompt = self.generate_base_prompt(simplified_tools);

        // === 动态注入系统资源上下文 ===
        // 这确保 LLM 能够感知当前系统中的实际设备、规则和工作流
        let resource_context = self.semantic_mapper.get_semantic_context().await;
        if !resource_context.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(&resource_context);
        }

        prompt
    }

    /// Generate base prompt (static parts: system_prompt + tools).
    /// This avoids rebuilding tool descriptions on every request.
    fn generate_base_prompt(&self, simplified_tools: &[edge_ai_tools::simplified::LlmToolDefinition]) -> String {
        let mut prompt = String::from(self.config.system_prompt.trim());

        prompt.push_str("\n\n## 可用工具\n\n");

        // Group tools by category for better organization
        let mut device_tools = Vec::new();
        let mut data_tools = Vec::new();
        let mut rule_tools = Vec::new();
        let mut system_tools = Vec::new();

        for tool in simplified_tools {
            if tool.name.contains("device") || tool.name.contains("control") {
                device_tools.push(tool);
            } else if tool.name.contains("data") || tool.name.contains("query") || tool.name.contains("metrics") {
                data_tools.push(tool);
            } else if tool.name.contains("rule") {
                rule_tools.push(tool);
            } else {
                system_tools.push(tool);
            }
        }

        // Add tool sections
        if !device_tools.is_empty() {
            prompt.push_str("### 设备管理\n");
            for tool in device_tools {
                prompt.push_str(&format!("- **{}**: {} (别名: {})\n",
                    tool.name, tool.description, tool.aliases.join(", ")));
            }
            prompt.push('\n');
        }

        if !data_tools.is_empty() {
            prompt.push_str("### 数据查询\n");
            for tool in data_tools {
                prompt.push_str(&format!("- **{}**: {} (别名: {})\n",
                    tool.name, tool.description, tool.aliases.join(", ")));
            }
            prompt.push('\n');
        }

        if !rule_tools.is_empty() {
            prompt.push_str("### 规则管理\n");
            for tool in rule_tools {
                prompt.push_str(&format!("- **{}**: {} (别名: {})\n",
                    tool.name, tool.description, tool.aliases.join(", ")));
            }
            prompt.push('\n');
        }

        if !system_tools.is_empty() {
            prompt.push_str("### 系统工具\n");
            for tool in system_tools {
                prompt.push_str(&format!("- **{}**: {} (别名: {})\n",
                    tool.name, tool.description, tool.aliases.join(", ")));
            }
            prompt.push('\n');
        }

        // Add usage guidance
        prompt.push_str("## 使用指南\n");
        prompt.push_str("- 多个工具调用可以并行执行，提高响应速度\n");
        prompt.push_str("- 设备别名和工具别名都可以使用，系统会自动识别\n");
        prompt.push_str("- 直接回答问题，不要过度思考或展开冗长的推理过程\n");

        prompt
    }

    /// Check if LLM is configured.
    pub async fn is_llm_configured(&self) -> bool {
        self.llm_interface.is_ready().await
    }

    /// Get the session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Get the session state.
    pub async fn state(&self) -> SessionState {
        self.internal_state.read().await.session.clone()
    }

    /// Get the conversation history.
    pub async fn history(&self) -> Vec<AgentMessage> {
        self.internal_state.read().await.memory.clone()
    }

    /// Restore conversation history from persisted data.
    pub async fn restore_history(&self, messages: Vec<AgentMessage>) {
        self.internal_state.write().await.restore_memory(messages);
    }

    /// Clear conversation history.
    pub async fn clear_history(&self) {
        self.internal_state.write().await.clear_memory();
    }

    /// Get available tools.
    pub fn available_tools(&self) -> Vec<String> {
        self.tools.list()
    }

    /// Get tool definitions for LLM.
    pub fn tool_definitions(&self) -> Value {
        self.tools.definitions_json()
    }

    /// Update the smart conversation manager with current devices.
    /// This enables better intent analysis based on available devices.
    pub async fn update_smart_context_devices(&self, devices: Vec<crate::smart_conversation::Device>) {
        let mut smart_conv = self.smart_conversation.write().await;
        smart_conv.update_devices(devices);
    }

    /// Update the smart conversation manager with current rules.
    /// This enables better intent analysis based on available rules.
    pub async fn update_smart_context_rules(&self, rules: Vec<crate::smart_conversation::Rule>) {
        let mut smart_conv = self.smart_conversation.write().await;
        smart_conv.update_rules(rules);
    }

    // === SEMANTIC MAPPING METHODS ===

    /// Register a device in the semantic mapper for natural language resolution.
    /// This allows LLM to reference devices by name instead of technical ID.
    pub async fn register_semantic_device(&self, device: crate::context::Resource) {
        let _ = self.semantic_mapper.register_device(device).await;
    }

    /// Update devices in the semantic mapper from smart conversation devices.
    pub async fn update_semantic_devices(&self, devices: Vec<crate::smart_conversation::Device>) {
        for device in devices {
            let resource = crate::context::Resource::device(
                &device.id,
                &device.name,
                &device.device_type
            )
                .with_location(&device.location);

            let _ = self.semantic_mapper.register_device(resource).await;
        }
    }

    /// Register rules in the semantic mapper.
    pub async fn register_semantic_rules(&self, rules: Vec<(String, String, bool)>) {
        self.semantic_mapper.register_rules(rules).await;
    }

    /// Update rules in the semantic mapper from smart conversation rules.
    pub async fn update_semantic_rules(&self, rules: Vec<crate::smart_conversation::Rule>) {
        let rules_data: Vec<(String, String, bool)> = rules.into_iter()
            .map(|r| (r.id, r.name, r.enabled))
            .collect();
        self.semantic_mapper.register_rules(rules_data).await;
    }

    /// Register workflows in the semantic mapper.
    pub async fn register_semantic_workflows(&self, workflows: Vec<(String, String, bool)>) {
        self.semantic_mapper.register_workflows(workflows).await;
    }

    /// Get semantic context for inclusion in LLM prompt.
    /// This provides LLM with available resource names without technical IDs.
    pub async fn get_semantic_context(&self) -> String {
        self.semantic_mapper.get_semantic_context().await
    }

    /// Get semantic mapper statistics.
    pub async fn get_semantic_mapping_stats(&self) -> semantic_mapper::MappingStats {
        self.semantic_mapper.get_stats().await
    }

    /// === FAST PATH: Check for simple responses BEFORE acquiring lock ===
    /// This improves latency for common queries like greetings and confirmations.
    fn try_fast_path(&self, user_message: &str) -> Option<AgentResponse> {
        let trimmed = user_message.trim().to_lowercase();
        let start = std::time::Instant::now();

        // Greeting patterns
        let greeting_responses: &[(&str, &str)] = &[
            ("你好", "你好！我是 NeoTalk 智能助手，有什么可以帮您？"),
            ("您好", "您好！我是 NeoTalk 智能助手，有什么可以帮您？"),
            ("hi", "Hello! I'm NeoTalk, your smart assistant. How can I help you?"),
            ("hello", "Hello! I'm NeoTalk, your smart assistant."),
            ("早上好", "早上好！今天有什么可以帮您的？"),
            ("下午好", "下午好！有什么可以帮您的？"),
            ("晚上好", "晚上好！有什么可以帮您的？"),
        ];

        // Confirmation patterns
        let confirmation_responses: &[(&str, &str)] = &[
            ("好的", "好的，我明白了。"),
            ("好的，", "好的。"),
            ("明白", "好的，我明白了。"),
            ("明白了", "好的，我明白了。"),
            ("知道了", "好的，我知道了。"),
            ("收到", "好的，收到了。"),
            ("嗯", "好的，我明白了。"),
            ("行", "好的，没问题。"),
            ("是", "是的，我明白了。"),
            ("对", "是的，正确。"),
            ("ok", "OK!"),
            ("好的ok", "好的！"),
            ("谢谢", "不客气！还有其他需要帮助的吗？"),
            ("thanks", "You're welcome! Is there anything else I can help with?"),
        ];

        // Check greetings
        for (pattern, response) in greeting_responses.iter() {
            if trimmed == *pattern || trimmed.starts_with(*pattern) {
                return Some(AgentResponse {
                    message: AgentMessage::assistant(*response),
                    tool_calls: vec![],
                    memory_context_used: false,
                    tools_used: vec![],
                    processing_time_ms: start.elapsed().as_millis() as u64,
                });
            }
        }

        // Check confirmations
        for (pattern, response) in confirmation_responses.iter() {
            if trimmed == *pattern || trimmed.starts_with(*pattern) {
                return Some(AgentResponse {
                    message: AgentMessage::assistant(*response),
                    tool_calls: vec![],
                    memory_context_used: false,
                    tools_used: vec![],
                    processing_time_ms: start.elapsed().as_millis() as u64,
                });
            }
        }

        None
    }

    /// Process a user message with real LLM.
    /// Uses session-level lock to prevent concurrent requests on the same session.
    pub async fn process(&self, user_message: &str) -> Result<AgentResponse> {
        tracing::debug!(message = %user_message, "Agent::process starting");

        // === FAST PATH: Try simple responses WITHOUT acquiring lock ===
        if let Some(response) = self.try_fast_path(user_message) {
            // Save to history for context continuity
            let user_msg = AgentMessage::user(user_message);
            self.internal_state
                .write()
                .await
                .push_message(user_msg);
            self.internal_state
                .write()
                .await
                .push_message(response.message.clone());

            return Ok(response);
        }

        // === NORMAL PATH: Acquire lock for complex processing ===
        let _lock = self.process_lock.lock().await;

        let start = std::time::Instant::now();

        // === SMART CONVERSATION INTERCEPTION ===
        // Check if we need to ask questions, confirm actions, or clarify intent
        // BEFORE calling the LLM - this guarantees intelligent behavior
        let smart_analysis = {
            let smart_conv = self.smart_conversation.read().await;
            smart_conv.analyze_input(user_message)
        };

        // Handle cases where we should intercept
        if !smart_analysis.can_proceed {
            let response_content = if let Some(question) = smart_analysis.missing_info {
                // Information missing - ask user
                format!("❓ {}", question)
            } else if let Some(confirm) = smart_analysis.requires_confirmation {
                // Dangerous operation - require confirmation
                format!("⚠️ {}", confirm)
            } else if let Some(clarify) = smart_analysis.ambiguous {
                // Intent unclear - ask for clarification
                format!("❓ {}", clarify)
            } else {
                // Should not reach here, but fallback
                "我明白您的请求，但需要更多信息。".to_string()
            };

            // Save user message and our response to history
            let user_msg = AgentMessage::user(user_message);
            let response_msg = AgentMessage::assistant(&response_content);

            self.internal_state
                .write()
                .await
                .push_message(user_msg);
            self.internal_state
                .write()
                .await
                .push_message(response_msg.clone());

            return Ok(AgentResponse {
                message: response_msg,
                tool_calls: vec![],
                memory_context_used: true,
                tools_used: vec![],
                processing_time_ms: start.elapsed().as_millis() as u64,
            });
        }

        // === PROCEED WITH NORMAL PROCESSING ===
        // === CONVERSATION CONTEXT: Enhance input with context ===
        // Try to resolve ambiguous commands and enhance with previous context
        let enhanced_input = {
            let ctx = self.conversation_context.read().await;
            // First, try to resolve ambiguous commands like "打开" -> "打开客厅的灯"
            if let Some(resolved) = ctx.resolve_ambiguous_command(user_message) {
                resolved
            } else {
                // Then enhance pronouns and add context
                ctx.enhance_input(user_message)
            }
        };

        // Add user message to history (use enhanced version for processing, but save original)
        let user_msg = AgentMessage::user(user_message);
        self.internal_state
            .write()
            .await
            .push_message(user_msg.clone());

        // Check if LLM is configured
        if !self.llm_interface.is_ready().await {
            // Fall back to simple keyword-based responses
            let (message, tool_calls, tools_used) =
                process_fallback(&self.tools, &self.fallback_rules, user_message).await;
            let processing_time = start.elapsed().as_millis() as u64;

            self.internal_state
                .write()
                .await
                .push_message(message.clone());

            return Ok(AgentResponse {
                message,
                tool_calls,
                memory_context_used: true,
                tools_used,
                processing_time_ms: processing_time,
            });
        }

        // === LLM PATH: Process with real LLM ===
        // Note: Fast path responses (greetings, confirmations) are handled in try_fast_path()
        // before acquiring the lock to improve latency.
        match self.process_with_llm(&enhanced_input).await {
            Ok(response) => {
                // === CONVERSATION CONTEXT: Update context after successful response ===
                {
                    let tool_results: Vec<(String, String)> = response.tool_calls
                        .iter()
                        .filter_map(|tc| {
                            tc.result.as_ref().map(|r| {
                                (tc.name.clone(), serde_json::to_string(r).unwrap_or_else(|_| "无结果".to_string()))
                            })
                        })
                        .collect();
                    let mut ctx = self.conversation_context.write().await;
                    ctx.update(user_message, &tool_results);
                }

                let processing_time = start.elapsed().as_millis() as u64;
                self.internal_state
                    .write()
                    .await
                    .session
                    .increment_messages();
                Ok(AgentResponse {
                    processing_time_ms: processing_time,
                    ..response
                })
            }
            Err(e) => {
                // On error, fall back to simple response
                tracing::error!(error = %e, "LLM error, using fallback");
                let (message, tool_calls, tools_used) =
                    process_fallback(&self.tools, &self.fallback_rules, user_message).await;
                let processing_time = start.elapsed().as_millis() as u64;

                self.internal_state
                    .write()
                    .await
                    .push_message(message.clone());

                Ok(AgentResponse {
                    message,
                    tool_calls,
                    memory_context_used: true,
                    tools_used,
                    processing_time_ms: processing_time,
                })
            }
        }
    }

    /// Process a user message with images (multimodal input).
    ///
    /// This method is used when the user sends images along with their text message.
    /// The images should be base64-encoded data URLs (e.g., "data:image/png;base64,...").
    pub async fn process_multimodal(
        &self,
        user_message: &str,
        images: Vec<String>, // Base64 data URLs
    ) -> Result<AgentResponse> {
        tracing::debug!(
            message = %user_message,
            image_count = images.len(),
            "Agent::process_multimodal starting"
        );

        // Create multimodal message content AND prepare images for storage
        let mut parts = vec![edge_ai_core::ContentPart::text(user_message)];
        let mut user_images = Vec::new();

        // Process images for both ContentPart and storage
        for image_data in &images {
            // Extract mime type from data URL
            let (mime_type_str, base64_part) = if image_data.starts_with("data:image/") {
                if let Some(pos) = image_data.find(',') {
                    let mime = if image_data.contains("data:image/png") {
                        "image/png"
                    } else if image_data.contains("data:image/jpeg") || image_data.contains("data:image/jpg") {
                        "image/jpeg"
                    } else if image_data.contains("data:image/webp") {
                        "image/webp"
                    } else if image_data.contains("data:image/gif") {
                        "image/gif"
                    } else {
                        "image/png"
                    };
                    (mime, &image_data[pos + 1..])
                } else {
                    ("image/png", image_data.as_str())
                }
            } else {
                ("image/png", image_data.as_str())
            };

            // Add to ContentPart for LLM
            parts.push(edge_ai_core::ContentPart::image_base64(
                base64_part,
                mime_type_str,
            ));

            // Add to storage as AgentMessageImage
            user_images.push(crate::agent::types::AgentMessageImage {
                data: image_data.clone(),
                mime_type: Some(mime_type_str.to_string()),
            });
        }

        let user_msg = edge_ai_core::Message::new(
            edge_ai_core::MessageRole::User,
            edge_ai_core::Content::Parts(parts),
        );

        // === Skip fast path for multimodal messages (always use LLM) ===
        let _lock = self.process_lock.lock().await;
        let start = std::time::Instant::now();

        // Add user message to history WITH images (for multimodal context in follow-up requests)
        let agent_user_msg = AgentMessage::user_with_images(user_message, user_images);
        self.internal_state
            .write()
            .await
            .push_message(agent_user_msg);

        // Check if LLM is configured (required for multimodal)
        if !self.llm_interface.is_ready().await {
            return Err(NeoTalkError::Llm(
                "Multimodal input requires LLM support".to_string(),
            ));
        }

        // === Get conversation history ===
        // Optimize: Clone only needed messages in one pass
        let history_without_last: Vec<AgentMessage> = {
            let state = self.internal_state.read().await;
            let memory = &state.memory;
            if memory.len() > 1 {
                memory.iter().take(memory.len() - 1).cloned().collect()
            } else {
                Vec::new()
            }
        };

        // Convert AgentMessage history to Message history
        let core_history: Vec<edge_ai_core::Message> =
            history_without_last.iter().map(|msg| msg.to_core()).collect();

        // === Process with LLM using multimodal message ===
        match self.llm_interface.chat_multimodal_with_history(user_msg, &core_history).await {
            Ok(llm_response) => {
                let response_msg = AgentMessage::assistant(&llm_response.text);

                self.internal_state
                    .write()
                    .await
                    .push_message(response_msg.clone());

                self.internal_state
                    .write()
                    .await
                    .session.increment_messages();

                let processing_time = start.elapsed().as_millis() as u64;

                Ok(AgentResponse {
                    message: response_msg,
                    tool_calls: vec![],
                    memory_context_used: false,
                    tools_used: vec![],
                    processing_time_ms: processing_time,
                })
            }
            Err(e) => {
                Err(NeoTalkError::Llm(format!("LLM processing failed: {}", e)))
            }
        }
    }

    /// Process a multimodal user message (text + images) with streaming response (returns AgentEvent stream).
    pub async fn process_multimodal_stream_events(
        &self,
        user_message: &str,
        images: Vec<String>, // Base64 data URLs
    ) -> Result<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> {
        tracing::debug!(
            message = %user_message,
            image_count = images.len(),
            "Agent::process_multimodal_stream_events starting"
        );

        let _lock = self.process_lock.lock().await;

        // Check if LLM is configured (required for multimodal)
        if !self.llm_interface.is_ready().await {
            // Fall back to simple response without LLM
            let (message, _, _) = process_fallback(&self.tools, &self.fallback_rules, user_message).await;
            self.internal_state
                .write()
                .await
                .push_message(message.clone());

            return Ok(Box::pin(async_stream::stream! {
                yield AgentEvent::content(message.content);
                yield AgentEvent::end();
            }));
        }

        match process_multimodal_stream_events(
            self.llm_interface.clone(),
            self.internal_state.clone(),
            self.tools.clone(),
            user_message,
            images,
        )
        .await
        {
            Ok(stream) => Ok(stream),
            Err(e) => {
                // On error, fall back to simple response
                tracing::error!(error = %e, "LLM multimodal stream error, using fallback");
                let (message, _, _) =
                    process_fallback(&self.tools, &self.fallback_rules, user_message).await;
                self.internal_state
                    .write()
                    .await
                    .push_message(message.clone());

                Ok(Box::pin(async_stream::stream! {
                    yield AgentEvent::content(message.content);
                    yield AgentEvent::end();
                }))
            }
        }
    }

    /// Process with real LLM.
    ///
    /// ## Safeguards:
    /// - Maximum tool calls per request limited to MAX_TOOL_CALLS_PER_REQUEST
    /// - Tool result clearing for old messages (Anthropic-style)
    /// - Token limit configured in ChatConfig
    async fn process_with_llm(&self, user_message: &str) -> Result<AgentResponse> {
        tracing::debug!(message = %user_message, "process_with_llm starting");
        use tool_parser::parse_tool_calls;

        // Get existing history (user message already added by caller in `process`)
        // Optimize: Clone only needed messages in one pass, avoiding double-clone
        let history_without_last: Vec<AgentMessage> = {
            let state = self.internal_state.read().await;
            let memory = &state.memory;
            if memory.len() > 1 {
                // Clone only what we need (skip last message)
                memory.iter().take(memory.len() - 1).cloned().collect()
            } else {
                Vec::new()
            }
        };

        // === DYNAMIC CONTEXT WINDOW: Get model's actual capacity ===
        // Query the LLM backend for the actual context window size.
        // This allows us to use the full capability of models like qwen3-vl:2b (32k)
        // while respecting the limits of smaller models like llama3:8b (8k).
        let max_context = self.llm_interface.max_context_length().await;
        // For models with large context (32k+), allow up to 16k for better long conversations
        // For smaller models (8k), use their full capacity
        let effective_max = if max_context >= 32000 {
            max_context.min(16_384) // Large models: cap at 16k
        } else {
            max_context.min(DEFAULT_CONTEXT_TOKENS) // Small models: use full capability
        };

        // === ANTHROPIC-STYLE IMPROVEMENT: Apply context window with tool result clearing ===
        // This prevents context bloat from old tool calls while maintaining conversation continuity
        let compacted_history = build_context_window(&history_without_last, effective_max);

        tracing::debug!(
            "Context: {} messages -> {} messages (after compaction)",
            history_without_last.len(),
            compacted_history.len()
        );

        // Build history for LLM (convert AgentMessage to Message)
        let mut core_history: Vec<Message> =
            compacted_history.iter().map(|msg| msg.to_core()).collect();

        // === CONVERSATION CONTEXT: Inject context summary before user message ===
        let context_summary = {
            let ctx = self.conversation_context.read().await;
            let summary = ctx.get_context_summary();
            if !summary.is_empty() {
                Some(format!("当前对话上下文：\n{}", summary))
            } else {
                None
            }
        };

        // If there's context, add it as a system message before the user message
        if let Some(summary) = context_summary {
            use edge_ai_core::message::{Content, MessageRole};
            core_history.push(Message::new(MessageRole::System, Content::text(&summary)));
            tracing::debug!("Injected conversation context into LLM history");
        }

        // Call LLM with conversation history (user message will be added by LLM interface)
        let chat_response = self
            .llm_interface
            .chat_with_history(user_message, &core_history)
            .await
            .map_err(|e| super::error::NeoTalkError::Llm(e.to_string()))?;

        // Parse response for tool calls
        tracing::debug!(response_text = %chat_response.text, "LLM response received");
        let (content, mut tool_calls) = parse_tool_calls(&chat_response.text)?;
        tracing::debug!(count = tool_calls.len(), "Parsed tool calls");
        for tc in &tool_calls {
            tracing::debug!(name = %tc.name, args = %tc.arguments, "  tool call");
        }

        // Extract thinking content if present
        let thinking = chat_response.thinking;

        // If no tool calls in response content, try parsing from thinking field
        // Some models (like qwen3 with thinking enabled) may put tool calls in thinking
        if tool_calls.is_empty()
            && let Some(ref thinking_content) = thinking
                && let Ok((_, thinking_tool_calls)) = parse_tool_calls(thinking_content)
                    && !thinking_tool_calls.is_empty() {
                        tracing::debug!("Found tool calls in thinking field, using them");
                        tool_calls = thinking_tool_calls;
                    }

        // If no tool calls, return the direct response
        if tool_calls.is_empty() {
            // Save assistant response with or without thinking
            let assistant_msg = if let Some(thinking_content) = thinking {
                // Apply cleanup to thinking if it's too long
                let cleaned_thinking = if thinking_content.len() > 200 {
                    crate::agent::streaming::cleanup_thinking_content(&thinking_content)
                } else {
                    thinking_content
                };
                AgentMessage::assistant_with_thinking(&content, &cleaned_thinking)
            } else {
                AgentMessage::assistant(&content)
            };
            self.internal_state
                .write()
                .await
                .push_message(assistant_msg.clone());
            return Ok(AgentResponse {
                message: assistant_msg,
                tool_calls: vec![],
                memory_context_used: true,
                tools_used: vec![],
                processing_time_ms: 0,
            });
        }

        // === SAFEGUARD: Limit number of tool calls to prevent infinite loops ===
        let max_calls = self.config.max_tool_calls;
        if tool_calls.len() > max_calls {
            tracing::warn!(
                "Too many tool calls ({}) in single request, limiting to {}",
                tool_calls.len(),
                max_calls
            );
            tool_calls.truncate(max_calls);
        }

        // === DEDUPLICATE: Remove duplicate tool calls to avoid redundant execution ===
        // Models sometimes output the same tool call multiple times
        // We keep the first occurrence of each unique (name, arguments) pair
        let original_count = tool_calls.len();
        let mut seen = std::collections::HashSet::new();
        tool_calls.retain(|tool_call| {
            // Create a unique key based on tool name and arguments
            let key = (
                tool_call.name.clone(),
                tool_call.arguments.to_string().chars().take(100).collect::<String>()
            );
            seen.insert(key)
        });
        let dedup_count = tool_calls.len();
        if original_count > dedup_count {
            tracing::info!(
                "Deduplicated tool calls: {} -> {} (removed {} duplicates)",
                original_count,
                dedup_count,
                original_count - dedup_count
            );
        }

        // Tool calls detected - DON'T save the initial assistant message yet
        // We'll save a complete message (with tool_calls and final response) after tool execution

        // Execute tools in PARALLEL for better performance
        // Independent tools can run simultaneously, dependent tools wait for results
        let mut tool_results = Vec::new();
        let mut tools_used = Vec::new();
        let mut tool_calls_with_results = Vec::new();

        // Clone tool_calls for parallel execution
        let tool_calls_clone = tool_calls.clone();

        // Use futures for parallel execution
        let futures: Vec<_> = tool_calls_clone
            .into_iter()
            .map(|tool_call| {
                let name = tool_call.name.clone();
                let arguments = tool_call.arguments.clone();
                let id = tool_call.id.clone();

                // Spawn each tool execution as a separate task
                async move {
                    let result = self.execute_tool(&name, &arguments).await;
                    (name, id, arguments, result)
                }
            })
            .collect();

        // Execute all tools in parallel and wait for completion
        let results = futures::future::join_all(futures).await;

        // Process results in original order
        for (name, id, arguments, result) in results {
            tracing::debug!(name = %name, result = ?result, "Tool execution result");
            match result {
                Ok(ok_result) => {
                    tools_used.push(name.clone());
                    tracing::debug!(name = %name, count = tools_used.len(), "Added to tools_used");
                    tool_results.push((name.clone(), ok_result.clone()));
                    tool_calls_with_results.push(ToolCall {
                        name,
                        id,
                        arguments,
                        result: Some(serde_json::json!(ok_result)),
                    });
                }
                Err(e) => {
                    let error_msg = format!("Error: {}", e);
                    tool_results.push((name.clone(), error_msg.clone()));
                    tool_calls_with_results.push(ToolCall {
                        name,
                        id,
                        arguments,
                        result: Some(serde_json::json!({ "error": error_msg })),
                    });
                }
            }
        }

        tracing::debug!(tools_used = ?tools_used, "Before format_tool_results");

        // Format tool results directly (without calling LLM again)
        // This prevents excessive thinking and model looping
        let final_text = crate::agent::streaming::format_tool_results(&tool_results);

        // Save a complete message with tool_calls, results, and optionally thinking
        let final_message = if let Some(thinking_content) = thinking {
            // Clean up thinking if it's too long
            let cleaned_thinking = if thinking_content.len() > 200 {
                crate::agent::streaming::cleanup_thinking_content(&thinking_content)
            } else {
                thinking_content
            };
            AgentMessage::assistant_with_tools_and_thinking(
                &final_text,
                tool_calls_with_results,
                &cleaned_thinking,
            )
        } else {
            AgentMessage::assistant_with_tools(&final_text, tool_calls_with_results)
        };
        self.internal_state
            .write()
            .await
            .push_message(final_message.clone());

        Ok(AgentResponse {
            message: final_message,
            tool_calls,
            memory_context_used: true,
            tools_used,
            processing_time_ms: 0,
        })
    }

    /// Map simplified parameter names to actual tool parameter names.
    ///
    /// This bridges the gap between the user-friendly simplified interface
    /// and the actual tool implementation parameters.
    fn map_simplified_parameters(&self, tool_name: &str, arguments: &Value) -> Value {
        if let Some(args_obj) = arguments.as_object() {
            // Special handling for create_rule: convert simplified (name, condition, action) to DSL
            if tool_name == "create_rule" || tool_name == "rule.from_context" {
                // Check if we have simplified parameters (condition + action) but no dsl
                let has_condition = args_obj.contains_key("condition");
                let has_action = args_obj.contains_key("action");
                let has_description = args_obj.contains_key("description");
                let has_dsl = args_obj.contains_key("dsl");

                if (has_condition || has_description) && !has_dsl {
                    // Convert simplified parameters to DSL
                    let name = args_obj.get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("未命名规则");

                    let dsl = if has_description {
                        // rule.from_context: extract structured rule from description
                        let description = args_obj.get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        // Try to parse the description to extract condition/action
                        // For now, generate a simple DSL with the description as context
                        format!(r#"RULE "{name}"
WHEN sensor.temperature > 30
DO
  NOTIFY "{description}"
END"#)
                    } else if has_condition && has_action {
                        // create_rule with simplified condition/action
                        let condition = args_obj.get("condition")
                            .and_then(|v| v.as_str())
                            .unwrap_or("sensor.temperature > 30");

                        let action = args_obj.get("action")
                            .and_then(|v| v.as_str())
                            .unwrap_or("通知管理员");

                        format!(r#"RULE "{name}"
WHEN {condition}
DO
  NOTIFY "{action}"
END"#)
                    } else {
                        // Fallback: just use the name
                        format!(r#"RULE "{name}"
WHEN sensor.temperature > 30
DO
  NOTIFY "规则触发"
END"#)
                    };

                    let mut mapped = serde_json::Map::new();
                    mapped.insert("name".to_string(), serde_json::json!(name));
                    mapped.insert("dsl".to_string(), serde_json::json!(dsl));

                    // Include description if present
                    if let Some(desc) = args_obj.get("description") {
                        mapped.insert("description".to_string(), desc.clone());
                    }

                    return serde_json::Value::Object(mapped);
                }
            }

            // Standard parameter mapping for other tools
            let mut mapped = serde_json::Map::new();

            for (key, value) in args_obj {
                // Map simplified names to actual parameter names based on tool
                let actual_key = match (tool_name, key.as_str()) {
                    // query_data mappings
                    ("query_data", "device") => "device_id",
                    ("query_data", "hours") => {
                        // Convert hours to start_time timestamp
                        if let Some(hours) = value.as_i64() {
                            let end_time = chrono::Utc::now().timestamp();
                            let start_time = end_time - (hours * 3600);
                            mapped.insert("end_time".to_string(), serde_json::json!(end_time));
                            mapped.insert("start_time".to_string(), serde_json::json!(start_time));
                            continue;
                        }
                        "start_time"
                    }
                    ("query_data", other) => other,

                    // control_device mappings - FIXED: action maps to command for real tool
                    ("control_device", "device") => "device_id",
                    ("control_device", "action") => "command",
                    ("control_device", "value") => "parameters",
                    ("control_device", other) => other,

                    // device.control mappings (simplified name, same as control_device)
                    ("device.control", "device") => "device_id",
                    ("device.control", "action") => "command",
                    ("device.control", "value") => "parameters",
                    ("device.control", other) => other,

                    // create_rule mappings (only used if DSL is already provided)
                    ("create_rule", "name") => "name",
                    ("create_rule", "dsl") => "dsl",
                    ("create_rule", "description") => "description",
                    ("create_rule", other) => other,

                    // disable_rule / enable_rule: rule -> rule_id
                    ("disable_rule", "rule") | ("enable_rule", "rule") => "rule_id",
                    ("disable_rule", other) | ("enable_rule", other) => other,

                    // list_devices mappings
                    ("list_devices", "type") => "device_type",
                    ("list_devices", "status") => "status",
                    ("list_devices", other) => other,

                    // list_rules mappings
                    ("list_rules", _) => key,

                    // Default: keep original key
                    (_, other) => other,
                };

                mapped.insert(actual_key.to_string(), value.clone());
            }

            serde_json::Value::Object(mapped)
        } else {
            arguments.clone()
        }
    }

    /// Map simplified tool names to real tool names.
    ///
    /// Simplified names are used in LLM prompts (e.g., "device.discover")
    /// while real names are used in ToolRegistry (e.g., "list_devices").
    ///
    /// This now uses the unified `ToolNameMapper` to ensure consistency
    /// across the codebase.
    fn resolve_tool_name(&self, simplified_name: &str) -> String {
        // Delegate to the unified mapper
        crate::tools::resolve_tool_name(simplified_name)
    }

    /// Execute a tool with retry logic.
    ///
    /// Retries up to 2 times for transient errors (network issues, timeouts).
    /// Returns a user-friendly error message if all retries fail.
    ///
    /// ## Production-ready error context:
    /// - Includes tool name, arguments, session ID for traceability
    /// - Categorizes errors (transient, validation, execution, timeout)
    /// - Logs detailed error information for debugging
    async fn execute_tool(&self, name: &str, arguments: &Value) -> Result<String> {
        const MAX_RETRIES: u32 = 2;
        let start = std::time::Instant::now();

        // Map simplified tool name to real tool name
        let real_tool_name = self.resolve_tool_name(name);

        // Convert simplified parameter names to actual tool parameters
        let mapped_arguments = self.map_simplified_parameters(name, arguments);

        // === SEMANTIC MAPPING: Convert natural language to technical IDs ===
        // This maps "客厅灯" -> "light_living_main" for device_id parameters
        let semantically_mapped = self.semantic_mapper
            .map_tool_parameters(&real_tool_name, mapped_arguments.clone())
            .await
            .unwrap_or(mapped_arguments);

        // Sanitize arguments for logging (limit size to avoid log spam)
        let args_preview = if semantically_mapped.to_string().len() > 200 {
            format!("{}...", &semantically_mapped.to_string().chars().take(200).collect::<String>())
        } else {
            semantically_mapped.to_string()
        };

        tracing::debug!(
            session_id = %self.session_id,
            tool = %real_tool_name,
            arguments = %args_preview,
            "Executing tool"
        );

        for attempt in 0..=MAX_RETRIES {
            match self.tools.execute(&real_tool_name, semantically_mapped.clone()).await {
                Ok(output) => {
                    let elapsed = start.elapsed();

                    // Check if tool execution itself failed
                    if !output.success {
                        let error_msg = output.error.unwrap_or_else(|| "Unknown error".to_string());

                        // Log detailed error with context
                        tracing::error!(
                            session_id = %self.session_id,
                            tool = %real_tool_name,
                            arguments = %args_preview,
                            error = %error_msg,
                            attempt = attempt,
                            elapsed_ms = elapsed.as_millis(),
                            error_category = "tool_execution_failed",
                            "Tool execution returned failure"
                        );

                        // Don't retry on logical errors (like invalid input)
                        return Ok(format!(
                            "工具 {} 执行失败: {}",
                            real_tool_name, error_msg
                        ));
                    }

                    tracing::debug!(
                        session_id = %self.session_id,
                        tool = %real_tool_name,
                        elapsed_ms = elapsed.as_millis(),
                        "Tool executed successfully"
                    );

                    return Ok(serde_json::to_string_pretty(&output.data)
                        .unwrap_or_else(|_| "Success".to_string()));
                }
                Err(e) => {
                    let last_error = e.to_string();
                    let elapsed = start.elapsed();

                    // Categorize the error for better debugging
                    let error_category = if last_error.contains("not_found")
                        || last_error.contains("unknown") {
                        "tool_not_found"
                    } else if last_error.contains("timeout") {
                        "timeout"
                    } else if last_error.contains("network")
                        || last_error.contains("connection") {
                        "network"
                    } else if last_error.contains("parse")
                        || last_error.contains("invalid") {
                        "validation"
                    } else {
                        "unknown"
                    };

                    // Check if error is transient (worth retrying)
                    let is_transient = matches!(error_category, "timeout" | "network");

                    tracing::warn!(
                        session_id = %self.session_id,
                        tool = %real_tool_name,
                        arguments = %args_preview,
                        error = %last_error,
                        attempt = attempt,
                        elapsed_ms = elapsed.as_millis(),
                        error_category = %error_category,
                        is_transient = is_transient,
                        "Tool execution error"
                    );

                    if is_transient && attempt < MAX_RETRIES {
                        // Exponential backoff: 100ms, 200ms
                        let delay_ms = 100 * (2_u64.pow(attempt));
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        continue;
                    }
                }
            }
        }

        // All retries failed - return detailed error with context
        let elapsed = start.elapsed();
        tracing::error!(
            session_id = %self.session_id,
            tool = %real_tool_name,
            arguments = %args_preview,
            elapsed_ms = elapsed.as_millis(),
            max_retries = MAX_RETRIES,
            error_category = "all_retries_failed",
            "Tool execution failed after all retries"
        );

        Err(super::error::NeoTalkError::Tool(format!(
            "工具 {} 执行失败 (session: {}, 尝试: {}次, 耗时: {}ms)",
            real_tool_name,
            self.session_id,
            MAX_RETRIES + 1,
            elapsed.as_millis()
        )))
    }

    /// Process a tool call result.
    pub async fn process_tool_result(
        &self,
        tool_call_id: &str,
        result: &str,
    ) -> Result<AgentResponse> {
        // Add tool result to history
        let tool_msg = AgentMessage::tool_result(tool_call_id, result);
        self.internal_state.write().await.push_message(tool_msg);

        // Get LLM response based on tool result
        let response_content = format!("工具执行完成。结果: {}", result);

        let response = AgentMessage::assistant(response_content);
        self.internal_state
            .write()
            .await
            .push_message(response.clone());

        Ok(AgentResponse {
            message: response,
            tool_calls: Vec::new(),
            memory_context_used: true,
            tools_used: Vec::new(),
            processing_time_ms: 0,
        })
    }

    /// Process a user message with streaming response (returns AgentEvent stream).
    pub async fn process_stream_events(
        &self,
        user_message: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> {
        // Add user message to history
        let user_msg = AgentMessage::user(user_message);
        self.internal_state.write().await.push_message(user_msg);

        // Check if LLM is configured
        if !self.llm_interface.is_ready().await {
            // Fall back to simple response
            let (message, _, _) =
                process_fallback(&self.tools, &self.fallback_rules, user_message).await;
            self.internal_state
                .write()
                .await
                .push_message(message.clone());

            // Return a single-item stream with the fallback response
            let content = message.content;
            return Ok(Box::pin(async_stream::stream! {
                yield AgentEvent::content(content);
                yield AgentEvent::end();
            }));
        }

        match process_stream_events(
            self.llm_interface.clone(),
            self.internal_state.clone(),
            self.tools.clone(),
            user_message,
        )
        .await
        {
            Ok(stream) => Ok(stream),
            Err(e) => {
                // On error, fall back to simple response
                tracing::error!(error = %e, "LLM stream error, using fallback");
                let (message, _, _) =
                    process_fallback(&self.tools, &self.fallback_rules, user_message).await;
                self.internal_state
                    .write()
                    .await
                    .push_message(message.clone());

                Ok(Box::pin(async_stream::stream! {
                    yield AgentEvent::content(message.content);
                    yield AgentEvent::end();
                }))
            }
        }
    }

    /// Process a user message with streaming response (legacy, returns String stream).
    pub async fn process_stream(
        &self,
        user_message: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = String> + Send>>> {
        let event_stream = self.process_stream_events(user_message).await?;
        Ok(events_to_string_stream(event_stream))
    }
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

        tracing::info!(
            session_id = %self.session_id,
            agent_name = %self.config.name,
            model = %self.config.model,
            tools_count = self.tools.list().len(),
            "Agent instance dropped (session destroyed)"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edge_ai_tools::{Tool, Result, ToolOutput, ToolError};
    use serde_json::json;

    /// Simple mock list_devices tool for testing
    struct MockListDevicesTool;
    #[async_trait::async_trait]
    impl Tool for MockListDevicesTool {
        fn name(&self) -> &str { "list_devices" }
        fn description(&self) -> &str { "List all devices (mock for testing)" }
        fn parameters(&self) -> serde_json::Value { json!({}) }
        async fn execute(&self, _args: serde_json::Value) -> Result<ToolOutput> {
            let data = json!({"devices": [{"id": "mock_device_1", "name": "Mock Device"}]});
            Ok(ToolOutput::success(data))
        }
    }

    /// Simple mock list_rules tool for testing
    struct MockListRulesTool;
    #[async_trait::async_trait]
    impl Tool for MockListRulesTool {
        fn name(&self) -> &str { "list_rules" }
        fn description(&self) -> &str { "List all rules (mock for testing)" }
        fn parameters(&self) -> serde_json::Value { json!({}) }
        async fn execute(&self, _args: serde_json::Value) -> Result<ToolOutput> {
            let data = json!({"rules": [{"id": "mock_rule_1", "name": "Mock Rule"}]});
            Ok(ToolOutput::success(data))
        }
    }

    /// Simple mock query_data tool for testing
    struct MockQueryDataTool;
    #[async_trait::async_trait]
    impl Tool for MockQueryDataTool {
        fn name(&self) -> &str { "query_data" }
        fn description(&self) -> &str { "Query metric data (mock for testing)" }
        fn parameters(&self) -> serde_json::Value { json!({"type": "object"}) }
        async fn execute(&self, _args: serde_json::Value) -> Result<ToolOutput> {
            let data = json!({"data": [{"metric": "temperature", "value": 25.5}]});
            Ok(ToolOutput::success(data))
        }
    }

    /// Simple mock greet tool for testing
    struct MockGreetTool;
    #[async_trait::async_trait]
    impl Tool for MockGreetTool {
        fn name(&self) -> &str { "greet" }
        fn description(&self) -> &str { "Greet the user (mock for testing)" }
        fn parameters(&self) -> serde_json::Value { json!({}) }
        async fn execute(&self, _args: serde_json::Value) -> Result<ToolOutput> {
            let data = json!({"message": "Hello there!"});
            Ok(ToolOutput::success(data))
        }
    }

    /// Create a test agent with mock tools registered
    fn create_test_agent_with_mocks(session_id: String) -> Agent {
        use edge_ai_tools::ToolRegistryBuilder;

        let mut registry = ToolRegistryBuilder::new().build();

        // Register mock tools
        registry.register(std::sync::Arc::new(MockListDevicesTool));
        registry.register(std::sync::Arc::new(MockListRulesTool));
        registry.register(std::sync::Arc::new(MockQueryDataTool));
        registry.register(std::sync::Arc::new(MockGreetTool));

        // Add default agent tools
        use crate::tools::{ThinkTool, ToolSearchTool};
        use crate::tools::{AskUserTool, ConfirmActionTool, ClarifyIntentTool};

        let tool_search = ToolSearchTool::from_definitions(&[]);
        registry.register(std::sync::Arc::new(tool_search));

        let think_tool = ThinkTool::new();
        registry.register(std::sync::Arc::new(think_tool));

        let ask_user_tool = AskUserTool::new();
        registry.register(std::sync::Arc::new(ask_user_tool));

        let confirm_tool = ConfirmActionTool::new();
        registry.register(std::sync::Arc::new(confirm_tool));

        let clarify_tool = ClarifyIntentTool::new();
        registry.register(std::sync::Arc::new(clarify_tool));

        Agent::with_tools(AgentConfig::default(), session_id, std::sync::Arc::new(registry))
    }

    #[tokio::test]
    async fn test_agent_creation() {
        let agent = Agent::with_session("test_session".to_string());
        assert_eq!(agent.session_id(), "test_session");

        let state = agent.state().await;
        assert_eq!(state.id, "test_session");
    }

    #[tokio::test]
    async fn test_agent_history() {
        let agent = Agent::with_session("test_session".to_string());

        // Initially empty
        assert!(agent.history().await.is_empty());

        // Clear should work
        agent.clear_history().await;
        assert!(agent.history().await.is_empty());
    }

    #[tokio::test]
    async fn test_available_tools() {
        let agent = create_test_agent_with_mocks("test_session".to_string());
        let tools = agent.available_tools();

        assert!(!tools.is_empty());
        assert!(tools.contains(&"list_devices".to_string()));
        assert!(tools.contains(&"list_rules".to_string()));
    }

    #[tokio::test]
    async fn test_process_fallback() {
        let agent = create_test_agent_with_mocks("test_session".to_string());
        let response = agent.process("列出所有设备").await.unwrap();

        assert!(response.message.content.contains("设备"));
        assert!(response.tools_used.contains(&"list_devices".to_string()));
    }

    #[tokio::test]
    async fn test_process_list_rules() {
        let agent = create_test_agent_with_mocks("test_session".to_string());
        let response = agent.process("列出规则").await.unwrap();

        assert!(response.message.content.contains("规则"));
        assert!(response.tools_used.contains(&"list_rules".to_string()));
    }

    #[tokio::test]
    async fn test_process_query_data() {
        let agent = create_test_agent_with_mocks("test_session".to_string());
        let response = agent.process("查询温度数据").await.unwrap();

        assert!(response.message.content.contains("数据"));
        assert!(response.tools_used.contains(&"query_data".to_string()));
    }

    #[tokio::test]
    async fn test_process_default() {
        let agent = Agent::with_session("test_session".to_string());
        let response = agent.process("你好").await.unwrap();

        // Should get a helpful response
        assert!(!response.message.content.is_empty());
    }

    #[tokio::test]
    async fn test_history_persistence() {
        let agent = Agent::with_session("test_session".to_string());

        // Send a message
        agent.process("列出设备").await.unwrap();

        // Check history
        let history = agent.history().await;
        assert!(history.len() >= 2); // user + assistant
    }

    #[tokio::test]
    async fn test_custom_fallback_rules() {
        // Test custom rules with mock greet tool
        // Note: Use a keyword that doesn't match fast-path greetings
        let custom_rules = vec![
            FallbackRule::new(vec!["greet", "greeting"], "greet")
                .with_response_template("Greeting from fallback!"),
        ];
        let agent = create_test_agent_with_mocks("test_session".to_string())
            .with_fallback_rules(custom_rules);

        // Use "greet me" which won't match fast-path patterns
        let response = agent.process("greet me").await.unwrap();
        // The greet tool should be used
        assert!(response.tools_used.contains(&"greet".to_string()),
            "Expected 'greet' in tools_used, got: {:?}", response.tools_used);
    }
}
