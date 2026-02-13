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

pub mod cache;
pub mod conversation_context;
pub mod fallback;
pub mod formatter;
pub mod scheduler;
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

use futures::Stream;
use tokio::sync::RwLock;

// Re-export error types
pub use crate::error::NeoMindError;
use serde_json::Value;

use super::error::Result;
use super::llm::{ChatConfig, LlmInterface};
use super::tools::mapper::map_tool_parameters;
use crate::context::ResourceIndex;
use neomind_core::{config::agent_env_vars, llm::backend::LlmRuntime, Message};
use neomind_llm::{CloudConfig, CloudRuntime, OllamaConfig, OllamaRuntime};

// Type aliases to reduce complexity
pub type SharedToolRegistry = Arc<neomind_tools::ToolRegistry>;
pub type SharedLlmInterface = Arc<LlmInterface>;
pub type SharedSessionState = Arc<RwLock<SessionState>>;
pub type SharedResourceIndex = Arc<RwLock<ResourceIndex>>;
pub type SharedSmartConversation =
    Arc<tokio::sync::RwLock<crate::smart_conversation::SmartConversationManager>>;
pub type SharedSemanticMapper = Arc<semantic_mapper::SemanticToolMapper>;
pub type EventStream = Pin<Box<dyn Stream<Item = AgentEvent> + Send>>;
pub type MessageStream = Pin<Box<dyn Stream<Item = (String, bool)> + Send>>;

pub use crate::context_selector::{
    CommandReference, ContextBundle, ContextScope, ContextSelector, DeviceTypeReference, Entity,
    IntentAnalysis, IntentAnalyzer, IntentType, RuleReference,
};
pub use conversation_context::{
    ConversationContext, ConversationTopic, EntityReference, EntityType,
};
pub use fallback::{default_fallback_rules, process_fallback, FallbackRule};
pub use formatter::{format_summary, format_tool_result};
pub use semantic_mapper::{
    DeviceMapping, MappingStats, RuleMapping, SemanticMapping, SemanticMatchType,
    SemanticToolMapper, WorkflowMapping,
};
pub use smart_followup::{
    AvailableDevice, DetectedIntent, FollowUpAnalysis, FollowUpItem, FollowUpPriority,
    FollowUpType, SmartFollowUpManager,
};
pub use streaming::{
    events_to_string_stream, process_multimodal_stream_events,
    process_multimodal_stream_events_with_safeguards, process_stream_events,
    process_stream_events_with_safeguards, StreamSafeguards,
};
pub use types::{
    AgentConfig, AgentEvent, AgentInternalState, AgentMessage, AgentMessageImage, AgentResponse,
    LlmBackend, SessionState, ToolCall,
};

/// Maximum number of tool calls allowed per request to prevent infinite loops
/// Note: This constant is kept for backward compatibility. Config values take precedence.
#[allow(dead_code)]
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

/// === ENHANCED: Conversation-Level Compression ===
///
/// Extends compression beyond tool results to include conversation messages.
/// This follows the "tiered compression" strategy from LangChain:
/// - Level 1: Keep recent messages intact
/// - Level 2: Summarize older assistant messages to key points
/// - Level 3: Compress very old messages to brief topic markers
///
/// Rules:
/// - Keep N most recent messages intact (default: 6)
/// - Preserve user messages verbatim (higher priority for user intent)
/// - Compress assistant messages to key points (remove verbose explanations)
/// - Group very old messages into topic summaries
/// - Never compress system messages
///
/// Expected impact: 30-50% token reduction for long conversations.
pub fn compact_conversation(
    messages: &[AgentMessage],
    keep_recent: usize,
    target_tokens: usize,
) -> Vec<AgentMessage> {
    if messages.len() <= keep_recent {
        return messages.to_vec();
    }

    let mut result = Vec::new();
    #[allow(unused_assignments)]
    let mut current_tokens = 0;

    // First pass: keep recent messages intact
    let recent_start = messages.len().saturating_sub(keep_recent);
    for msg in &messages[recent_start..] {
        result.push(msg.clone());
        current_tokens += tokenizer::estimate_message_tokens(msg);
    }

    // If we're already under the token limit, return early
    if current_tokens <= target_tokens {
        // Still need to add older messages in reverse order
        for msg in messages[..recent_start].iter().rev() {
            let msg_tokens = tokenizer::estimate_message_tokens(msg);
            if current_tokens + msg_tokens > target_tokens {
                break;
            }
            result.insert(0, msg.clone());
            current_tokens += msg_tokens;
        }
        return result;
    }

    // Second pass: compress older messages
    let mut compressed_older = Vec::new();
    let mut topic_batches: Vec<String> = Vec::new();

    for msg in messages[..recent_start].iter() {
        // Always keep system messages
        if msg.role == "system" {
            compressed_older.push(msg.clone());
            current_tokens += tokenizer::estimate_message_tokens(msg);
            continue;
        }

        // Keep user messages verbatim (they contain critical intent)
        if msg.role == "user" {
            // Truncate very long user messages
            let truncated_content = if msg.content.len() > 200 {
                format!("{}... (消息过长，已截断)", &msg.content[..200])
            } else {
                msg.content.clone()
            };
            compressed_older.push(AgentMessage {
                content: truncated_content,
                ..msg.clone()
            });
            current_tokens += tokenizer::estimate_message_tokens(msg);
            continue;
        }

        // Assistant messages: create brief summary
        if msg.role == "assistant" {
            let summary = summarize_assistant_message(msg);
            topic_batches.push(summary);
        }
    }

    // Create a single summary for old conversation
    if !topic_batches.is_empty() {
        let old_summary = if topic_batches.len() <= 3 {
            format!("[之前的对话: {}]", topic_batches.join("; "))
        } else {
            format!(
                "[之前的对话: 包含{}轮交流，主题涉及{}]",
                topic_batches.len(),
                if topic_batches.len() > 5 {
                    "多个话题"
                } else {
                    "相关内容"
                }
            )
        };

        // Create a synthetic summary message
        let timestamp = messages
            .first()
            .and_then(|m| Some(m.timestamp))
            .unwrap_or_else(|| chrono::Utc::now().timestamp());

        compressed_older.push(AgentMessage {
            role: "system".to_string(),
            content: old_summary,
            tool_calls: None,
            tool_call_id: None,
            tool_call_name: None,
            thinking: None,
            images: None,
            timestamp,
        });
    }

    // Combine compressed older messages with recent messages
    let mut final_result = compressed_older;
    final_result.extend(result);

    final_result
}

/// Summarize an assistant message to its key points.
fn summarize_assistant_message(msg: &AgentMessage) -> String {
    let content = msg.content.trim();

    // Early return for short content
    if content.len() <= 50 {
        return content.to_string();
    }

    // Check for common patterns and extract key info
    if content.contains("成功") || content.contains("已完成") {
        if let Some(tool_name) = &msg.tool_call_name {
            return format!("执行了{}", tool_name);
        }
        return "操作已完成".to_string();
    }

    if content.contains("失败") || content.contains("错误") {
        return format!("操作失败: {}", extract_first_phrase(content, 30));
    }

    if content.contains("查询到") || content.contains("数据显示") {
        return format!("查询了数据: {}", extract_first_phrase(content, 30));
    }

    // Generic: extract first meaningful phrase
    extract_first_phrase(content, 40)
}

/// Extract the first meaningful phrase from text.
fn extract_first_phrase(text: &str, max_len: usize) -> String {
    let trimmed = text.trim();
    if trimmed.len() <= max_len {
        return trimmed.to_string();
    }

    // Try to break at sentence boundary
    if let Some(pos) = trimmed.find('。') {
        if pos <= max_len {
            return trimmed[..pos + 1].to_string();
        }
    }

    if let Some(pos) = trimmed.find('.') {
        if pos <= max_len {
            return trimmed[..pos + 1].to_string();
        }
    }

    if let Some(pos) = trimmed.find('，') {
        if pos <= max_len {
            return trimmed[..pos].to_string();
        }
    }

    if let Some(pos) = trimmed.find(',') {
        if pos <= max_len {
            return trimmed[..pos].to_string();
        }
    }

    // Hard truncate with ellipsis
    format!("{}...", &trimmed[..max_len.saturating_sub(3)])
}

/// === ENHANCED: Context Window with Importance-Based Selection ===
///
/// Builds conversation context with:
/// 1. Tool result clearing for old messages
/// 2. Conversation-level compression for long histories
/// 3. Importance-based message selection (P1.2)
/// 4. Token-based windowing with accurate estimation
/// 5. Always keep recent messages (minimum 4) for context continuity
///
/// The `max_tokens` parameter allows dynamic context sizing based on the model's actual capacity.
/// This prevents wasting model capability (e.g., using 5k context with a 32k model) while
/// also preventing errors from exceeding the model's limit (e.g., using 12k context with an 8k model).
fn build_context_window(messages: &[AgentMessage], max_tokens: usize) -> Vec<AgentMessage> {
    // Use the improved tokenizer module for accurate token estimation
    use tokenizer::select_messages_with_importance;

    // First, apply tool result clearing
    let tool_compacted = compact_tool_results(messages, 2); // Default: keep 2 recent results

    // Then apply conversation-level compression if we have many messages
    let conversation_compacted = if tool_compacted.len() > 12 {
        // Only compress if we have more than 12 messages after tool clearing
        compact_conversation(&tool_compacted, 6, max_tokens)
    } else {
        tool_compacted
    };

    // Use importance-based selection for better context quality (P1.2)
    // This prioritizes system messages, user intent, and error handling
    let selected_refs = select_messages_with_importance(
        &conversation_compacted,
        max_tokens,
        4,    // Always keep at least 4 recent messages
        0.15, // Minimum importance threshold
    );

    // Convert references to owned messages
    selected_refs.into_iter().cloned().collect()
}

/// Calculate adaptive context size adjustment based on conversation complexity.
///
/// Returns a multiplier (0.9 to 1.2) that adjusts the context window:
/// - 1.2: High complexity (many entities, topics, recent errors)
/// - 1.0: Normal complexity
/// - 0.9: Low complexity (simple greetings, repetitive)
///
/// Complexity factors:
/// - High entity diversity: +10%
/// - Multiple active topics: +10%
/// - Recent errors: +15%
/// - Simple greetings: -10%
fn calculate_adaptive_context_adjustment(messages: &[AgentMessage]) -> f64 {
    if messages.is_empty() {
        return 1.0;
    }

    let mut adjustment = 1.0f64;

    // Analyze recent messages (last 10) for complexity
    let recent_count = messages.len().min(10);
    let recent = &messages[messages.len().saturating_sub(recent_count)..];

    // 1. Entity diversity: Count unique device/rule/agent mentions
    let mut entities = std::collections::HashSet::new();
    for msg in recent {
        let content = msg.content.to_lowercase();

        // Extract device IDs
        for word in content.split_whitespace() {
            if word.starts_with("device_") || word.starts_with("设备") {
                entities.insert(word.to_string());
            }
            // Rule mentions
            if word.contains("rule") || word.contains("规则") {
                entities.insert("rule".to_string());
            }
            // Agent mentions
            if word.contains("agent") || word.contains("智能体") {
                entities.insert("agent".to_string());
            }
            // Location mentions
            if word.contains("客厅")
                || word.contains("卧室")
                || word.contains("厨房")
                || word.contains("living")
                || word.contains("bedroom")
                || word.contains("kitchen")
            {
                entities.insert("location".to_string());
            }
        }
    }

    // High entity diversity: +10%
    if entities.len() >= 4 {
        adjustment += 0.1;
        tracing::debug!(
            "Adaptive context: +10% for high entity diversity ({})",
            entities.len()
        );
    }

    // 2. Topic variety: Count distinct topics
    let mut topics = std::collections::HashSet::new();
    for msg in recent {
        let content = msg.content.to_lowercase();

        if content.contains("温度") || content.contains("temperature") {
            topics.insert("temperature");
        }
        if content.contains("灯") || content.contains("light") {
            topics.insert("lighting");
        }
        if content.contains("湿度") || content.contains("humidity") {
            topics.insert("humidity");
        }
        if content.contains("创建") || content.contains("create") {
            topics.insert("creation");
        }
        if content.contains("查询") || content.contains("query") || content.contains("list") {
            topics.insert("query");
        }
        if content.contains("控制") || content.contains("control") {
            topics.insert("control");
        }
    }

    // Multiple active topics: +10%
    if topics.len() >= 3 {
        adjustment += 0.1;
        tracing::debug!(
            "Adaptive context: +10% for multiple topics ({})",
            topics.len()
        );
    }

    // 3. Recent errors: +15%
    let has_recent_errors = recent.iter().any(|msg| {
        let content = msg.content.to_lowercase();
        content.contains("错误")
            || content.contains("失败")
            || content.contains("error")
            || content.contains("fail")
            || msg.role == "tool" && content.contains("\"success\":false")
    });
    if has_recent_errors {
        adjustment += 0.15;
        tracing::debug!("Adaptive context: +15% for recent errors");
    }

    // 4. Simple greetings: -10%
    let is_simple_greeting = messages.len() <= 3
        && recent.iter().all(|msg| {
            let content = msg.content.to_lowercase();
            let tokens = content.split_whitespace().count();
            tokens <= 5
                || content.contains("你好")
                || content.contains("hello")
                || content.contains("hi")
                || content.contains("嗨")
        });
    if is_simple_greeting {
        adjustment -= 0.1;
        tracing::debug!("Adaptive context: -10% for simple greeting");
    }

    // 5. Repetitive content penalty: -5%
    let unique_contents: std::collections::HashSet<_> =
        recent.iter().map(|m| m.content.as_str()).collect();
    if recent.len() > 3 && unique_contents.len() < recent.len() / 2 {
        adjustment -= 0.05;
        tracing::debug!("Adaptive context: -5% for repetitive content");
    }

    // Clamp adjustment to reasonable bounds
    let adjustment = adjustment.clamp(0.9, 1.2);

    tracing::debug!(
        "Adaptive context adjustment: {:.2} (entities={}, topics={}, has_errors={}, is_greeting={})",
        adjustment,
        entities.len(),
        topics.len(),
        has_recent_errors,
        is_simple_greeting
    );

    adjustment
}

/// Default context window size to use when model capacity is unknown.
/// This is a conservative value that works for most models.
#[allow(dead_code)]
const DEFAULT_CONTEXT_TOKENS: usize = 8_000;

/// Tool result cache to avoid redundant executions.
///
/// Caches tool results with TTL-based expiration:
/// - Device queries: 60 seconds TTL
/// - Static data (list_rules, list_agents): 300 seconds TTL
/// - Other tools: 30 seconds default TTL
///
/// Cache key format: (tool_name, serialized_arguments)
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

    fn clear(&mut self) {
        self.entries.clear();
    }
}

impl Default for ToolResultCache {
    fn default() -> Self {
        Self::new()
    }
}

/// AI Agent that orchestrates components.
pub struct Agent {
    /// Configuration
    config: AgentConfig,
    /// Session ID
    session_id: String,
    /// Tool registry
    tools: Arc<neomind_tools::ToolRegistry>,
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
    /// Conversation context - enables continuous dialogue with entity references
    conversation_context: Arc<tokio::sync::RwLock<ConversationContext>>,
    /// Smart followup manager - intelligent question generation when input is incomplete
    smart_followup: Arc<tokio::sync::RwLock<SmartFollowUpManager>>,
    /// Context selector - intelligent context selection based on intent analysis
    context_selector: Arc<tokio::sync::RwLock<crate::context_selector::ContextSelector>>,
    /// Last injected context summary hash (for deduplication)
    last_injected_context_hash: Arc<tokio::sync::RwLock<u64>>,
    /// Tool result cache - caches recent tool executions to avoid redundant calls
    tool_result_cache: Arc<tokio::sync::RwLock<ToolResultCache>>,
}

impl Agent {
    /// Create a new agent with custom tool registry.
    pub fn with_tools(
        config: AgentConfig,
        session_id: String,
        tools: Arc<neomind_tools::ToolRegistry>,
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

        let llm_interface =
            Arc::new(LlmInterface::new(llm_config).with_system_prompt(&config.system_prompt));

        // Create semantic mapper with resource index
        let resource_index = Arc::new(RwLock::new(ResourceIndex::new()));
        let semantic_mapper = Arc::new(semantic_mapper::SemanticToolMapper::new(
            resource_index.clone(),
        ));

        // Create smart followup manager with resource index for device-aware followups
        let smart_followup = Arc::new(tokio::sync::RwLock::new(
            SmartFollowUpManager::with_resource_index(resource_index.clone()),
        ));

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
            conversation_context: Arc::new(tokio::sync::RwLock::new(ConversationContext::new())),
            smart_followup,
            context_selector: Arc::new(tokio::sync::RwLock::new(
                crate::context_selector::ContextSelector::new(),
            )),
            last_injected_context_hash: Arc::new(tokio::sync::RwLock::new(0)),
            tool_result_cache: Arc::new(tokio::sync::RwLock::new(ToolResultCache::new())),
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
        let mut registry = neomind_tools::ToolRegistryBuilder::new().build();

        // Add agent-specific tools
        use crate::tools::{AskUserTool, ClarifyIntentTool, ConfirmActionTool};
        use crate::tools::{ThinkTool, ToolSearchTool};

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
                    OllamaRuntime::new(config).map_err(|e| NeoMindError::llm(e.to_string()))?;
                (Arc::new(runtime) as Arc<dyn LlmRuntime>, model)
            }
            LlmBackend::Qwen {
                api_key,
                endpoint,
                model,
            } => {
                tracing::info!(
                    endpoint = %endpoint, model = %model, timeout = cloud_timeout,
                    "Creating CloudRuntime for Qwen"
                );
                let config = if endpoint.is_empty() {
                    CloudConfig::qwen(&api_key)
                        .with_model(&model)
                        .with_timeout_secs(cloud_timeout)
                } else {
                    CloudConfig::custom(&api_key, &endpoint)
                        .with_model(&model)
                        .with_timeout_secs(cloud_timeout)
                };
                let runtime =
                    CloudRuntime::new(config).map_err(|e| NeoMindError::llm(e.to_string()))?;
                (Arc::new(runtime) as Arc<dyn LlmRuntime>, model)
            }
            LlmBackend::DeepSeek {
                api_key,
                endpoint,
                model,
            } => {
                tracing::info!(
                    endpoint = %endpoint, model = %model, timeout = cloud_timeout,
                    "Creating CloudRuntime for DeepSeek"
                );
                let config = if endpoint.is_empty() {
                    CloudConfig::deepseek(&api_key)
                        .with_model(&model)
                        .with_timeout_secs(cloud_timeout)
                } else {
                    CloudConfig::custom(&api_key, &endpoint)
                        .with_model(&model)
                        .with_timeout_secs(cloud_timeout)
                };
                let runtime =
                    CloudRuntime::new(config).map_err(|e| NeoMindError::llm(e.to_string()))?;
                (Arc::new(runtime) as Arc<dyn LlmRuntime>, model)
            }
            LlmBackend::GLM {
                api_key,
                endpoint,
                model,
            } => {
                tracing::info!(
                    endpoint = %endpoint, model = %model, timeout = cloud_timeout,
                    "Creating CloudRuntime for GLM"
                );
                let config = if endpoint.is_empty() {
                    CloudConfig::glm(&api_key)
                        .with_model(&model)
                        .with_timeout_secs(cloud_timeout)
                } else {
                    CloudConfig::custom(&api_key, &endpoint)
                        .with_model(&model)
                        .with_timeout_secs(cloud_timeout)
                };
                let runtime =
                    CloudRuntime::new(config).map_err(|e| NeoMindError::llm(e.to_string()))?;
                (Arc::new(runtime) as Arc<dyn LlmRuntime>, model)
            }
            LlmBackend::MiniMax {
                api_key,
                endpoint,
                model,
            } => {
                tracing::info!(
                    endpoint = %endpoint, model = %model, timeout = cloud_timeout,
                    "Creating CloudRuntime for MiniMax"
                );
                let config = if endpoint.is_empty() {
                    CloudConfig::minimax(&api_key)
                        .with_model(&model)
                        .with_timeout_secs(cloud_timeout)
                } else {
                    CloudConfig::custom(&api_key, &endpoint)
                        .with_model(&model)
                        .with_timeout_secs(cloud_timeout)
                };
                let runtime =
                    CloudRuntime::new(config).map_err(|e| NeoMindError::llm(e.to_string()))?;
                (Arc::new(runtime) as Arc<dyn LlmRuntime>, model)
            }
            LlmBackend::Anthropic {
                api_key,
                endpoint,
                model,
            } => {
                tracing::info!(
                    endpoint = %endpoint, model = %model, timeout = cloud_timeout,
                    "Creating CloudRuntime for Anthropic"
                );
                let config = CloudConfig::anthropic(&api_key).with_timeout_secs(cloud_timeout);
                let config = if endpoint.is_empty() {
                    config.with_model(&model)
                } else {
                    CloudConfig::custom(&api_key, &endpoint)
                        .with_model(&model)
                        .with_timeout_secs(cloud_timeout)
                };
                let runtime =
                    CloudRuntime::new(config).map_err(|e| NeoMindError::llm(e.to_string()))?;
                (Arc::new(runtime) as Arc<dyn LlmRuntime>, model)
            }
            LlmBackend::Google {
                api_key,
                endpoint,
                model,
            } => {
                tracing::info!(
                    endpoint = %endpoint, model = %model, timeout = cloud_timeout,
                    "Creating CloudRuntime for Google"
                );
                let config = CloudConfig::google(&api_key).with_timeout_secs(cloud_timeout);
                let config = if endpoint.is_empty() {
                    config.with_model(&model)
                } else {
                    CloudConfig::custom(&api_key, &endpoint)
                        .with_model(&model)
                        .with_timeout_secs(cloud_timeout)
                };
                let runtime =
                    CloudRuntime::new(config).map_err(|e| NeoMindError::llm(e.to_string()))?;
                (Arc::new(runtime) as Arc<dyn LlmRuntime>, model)
            }
            LlmBackend::XAi {
                api_key,
                endpoint,
                model,
            } => {
                tracing::info!(
                    endpoint = %endpoint, model = %model, timeout = cloud_timeout,
                    "Creating CloudRuntime for xAI"
                );
                let config = CloudConfig::grok(&api_key).with_timeout_secs(cloud_timeout);
                let config = if endpoint.is_empty() {
                    config.with_model(&model)
                } else {
                    CloudConfig::custom(&api_key, &endpoint)
                        .with_model(&model)
                        .with_timeout_secs(cloud_timeout)
                };
                let runtime =
                    CloudRuntime::new(config).map_err(|e| NeoMindError::llm(e.to_string()))?;
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
                let config = CloudConfig::openai(&api_key).with_timeout_secs(cloud_timeout);
                let config = if endpoint.is_empty() {
                    config.with_model(&model)
                } else {
                    // Custom endpoint
                    CloudConfig::custom(&api_key, &endpoint)
                        .with_model(&model)
                        .with_timeout_secs(cloud_timeout)
                };
                let runtime =
                    CloudRuntime::new(config).map_err(|e| NeoMindError::llm(e.to_string()))?;
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
        use neomind_core::llm::backend::ToolDefinition as CoreToolDefinition;
        use neomind_tools::simplified;

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
                    properties.insert(
                        param.clone(),
                        serde_json::json!({
                            "type": "string",
                            "description": param
                        }),
                    );
                }
                for (param, info) in &tool.optional {
                    properties.insert(
                        param.clone(),
                        serde_json::json!({
                            "type": "string",
                            "description": info.description,
                            "default": info.default
                        }),
                    );
                }

                CoreToolDefinition {
                    name: tool.name.clone(),
                    description: format!(
                        "{} (别名: {})",
                        tool.description,
                        tool.aliases.join(", ")
                    ),
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
    async fn generate_dynamic_system_prompt(
        &self,
        simplified_tools: &[neomind_tools::simplified::LlmToolDefinition],
    ) -> String {
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
    fn generate_base_prompt(
        &self,
        simplified_tools: &[neomind_tools::simplified::LlmToolDefinition],
    ) -> String {
        let mut prompt = String::from(self.config.system_prompt.trim());

        prompt.push_str("\n\n## 可用工具\n\n");

        // Group tools by category for better organization
        let mut device_tools = Vec::new();
        let mut data_tools = Vec::new();
        let mut rule_tools = Vec::new();
        let mut agent_tools = Vec::new();
        let mut system_tools = Vec::new();

        for tool in simplified_tools {
            if tool.name.contains("device_discover") || tool.name.contains("device_control") {
                device_tools.push(tool);
            } else if tool.name.contains("device") || tool.name.contains("control") {
                device_tools.push(tool);
            } else if tool.name.contains("data")
                || tool.name.contains("query")
                || tool.name.contains("metrics")
            {
                data_tools.push(tool);
            } else if tool.name.contains("rule") {
                rule_tools.push(tool);
            } else if tool.name.contains("agent") {
                agent_tools.push(tool);
            } else {
                system_tools.push(tool);
            }
        }

        // Add tool sections with examples
        if !device_tools.is_empty() {
            prompt.push_str("### 设备管理\n");
            for tool in device_tools {
                prompt.push_str(&format!(
                    "**{}**: {} (别名: {})\n",
                    tool.name,
                    tool.description,
                    tool.aliases.join(", ")
                ));

                // Add use_when conditions
                if !tool.use_when.is_empty() {
                    prompt.push_str("  *调用条件*: ");
                    for (i, condition) in tool.use_when.iter().enumerate() {
                        if i > 0 {
                            prompt.push_str(" 或 ");
                        }
                        prompt.push_str(condition);
                    }
                    prompt.push('\n');
                }

                // Add examples (especially important for chains)
                if !tool.examples.is_empty() {
                    prompt.push_str("  *示例*:\n");
                    for example in &tool.examples {
                        prompt.push_str(&format!("    - 用户: \"{}\"\n", example.user_query));
                        prompt.push_str(&format!("      调用: {}\n", example.tool_call));
                        if !example.explanation.is_empty() {
                            prompt.push_str(&format!("      说明: {}\n", example.explanation));
                        }
                    }
                }
                prompt.push('\n');
            }
        }

        if !data_tools.is_empty() {
            prompt.push_str("### 数据查询\n");
            for tool in data_tools {
                prompt.push_str(&format!(
                    "**{}**: {} (别名: {})\n",
                    tool.name,
                    tool.description,
                    tool.aliases.join(", ")
                ));

                if !tool.use_when.is_empty() {
                    prompt.push_str("  *调用条件*: ");
                    for (i, condition) in tool.use_when.iter().enumerate() {
                        if i > 0 {
                            prompt.push_str(" 或 ");
                        }
                        prompt.push_str(condition);
                    }
                    prompt.push('\n');
                }

                if !tool.examples.is_empty() {
                    prompt.push_str("  *示例*:\n");
                    for example in &tool.examples {
                        prompt.push_str(&format!("    - 用户: \"{}\"\n", example.user_query));
                        prompt.push_str(&format!("      调用: {}\n", example.tool_call));
                        if !example.explanation.is_empty() {
                            prompt.push_str(&format!("      说明: {}\n", example.explanation));
                        }
                    }
                }
                prompt.push('\n');
            }
        }

        if !rule_tools.is_empty() {
            prompt.push_str("### 规则管理\n");
            for tool in rule_tools {
                prompt.push_str(&format!(
                    "**{}**: {} (别名: {})\n",
                    tool.name,
                    tool.description,
                    tool.aliases.join(", ")
                ));

                if !tool.examples.is_empty() {
                    prompt.push_str("  *示例*:\n");
                    for example in &tool.examples {
                        prompt.push_str(&format!(
                            "    - 用户: \"{}\" → {}\n",
                            example.user_query, example.tool_call
                        ));
                    }
                }
                prompt.push('\n');
            }
        }

        if !agent_tools.is_empty() {
            prompt.push_str("### AI Agent\n");
            for tool in agent_tools {
                prompt.push_str(&format!(
                    "**{}**: {} (别名: {})\n",
                    tool.name,
                    tool.description,
                    tool.aliases.join(", ")
                ));

                if !tool.examples.is_empty() {
                    prompt.push_str("  *示例*:\n");
                    for example in &tool.examples {
                        prompt.push_str(&format!(
                            "    - 用户: \"{}\" → {}\n",
                            example.user_query, example.tool_call
                        ));
                    }
                }
                prompt.push('\n');
            }
        }

        if !system_tools.is_empty() {
            prompt.push_str("### 系统工具\n");
            for tool in system_tools {
                prompt.push_str(&format!(
                    "**{}**: {} (别名: {})\n",
                    tool.name,
                    tool.description,
                    tool.aliases.join(", ")
                ));
            }
            prompt.push('\n');
        }

        // Add usage guidance with chain instructions
        prompt.push_str("## 调用顺序规则\n\n");

        prompt.push_str("### 设备数据查询链路\n");
        prompt.push_str("**快速路径**（推荐）：\n");
        prompt.push_str("- **get_device_data(device_id='设备名称')** → 工具会自动解析名称为ID，返回所有当前指标\n");
        prompt.push_str("- **query_data(device_id='设备名称', metric='实际指标名')** → 查询历史趋势（必须先用get_device_data确认指标名）\n\n");

        prompt.push_str("**完整路径**（当用户问\"有哪些设备\"时）：\n");
        prompt.push_str("- **device_discover** → 显示所有设备列表\n");
        prompt.push_str("- **get_device_data(device_id)** → 获取设备详细数据\n\n");

        prompt.push_str("⚠️ 关键要点：\n");
        prompt.push_str("- ✅ get_device_data 和 query_data 都支持直接使用设备名称（如 'ne101'、'ne101 test'）\n");
        prompt
            .push_str("- ❌ 不要跳过 get_device_data 直接调用 query_data（需要先确认指标名称）\n");
        prompt.push_str("- ❌ query_data 的 metric 参数必须是实际的指标名（如 values.battery），不能是 'battery' 或 '温度'\n\n");

        prompt.push_str("## 使用指南\n");
        prompt.push_str("- 多个工具调用可以并行执行，提高响应速度\n");
        prompt.push_str("- 设备别名和工具别名都可以使用，系统会自动识别\n");

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
    pub async fn update_smart_context_devices(
        &self,
        devices: Vec<crate::smart_conversation::Device>,
    ) {
        let mut smart_conv = self.smart_conversation.write().await;
        smart_conv.update_devices(devices);
    }

    /// Update the smart conversation manager with current rules.
    /// This enables better intent analysis based on available rules.
    pub async fn update_smart_context_rules(&self, rules: Vec<crate::smart_conversation::Rule>) {
        let mut smart_conv = self.smart_conversation.write().await;
        smart_conv.update_rules(rules);
    }

    // === SMART FOLLOWUP METHODS ===

    /// Update smart followup manager with current devices.
    /// This enables intelligent followup questions based on available devices.
    pub async fn update_followup_devices(&self, devices: Vec<AvailableDevice>) {
        let mut followup = self.smart_followup.write().await;
        followup.set_available_devices(devices);
    }

    /// Refresh smart followup devices from resource index.
    pub async fn refresh_followup_devices(&self) {
        let mut followup = self.smart_followup.write().await;
        followup.refresh_devices().await;
    }

    // === CONTEXT SELECTOR METHODS ===

    /// Get the context selector reference.
    pub async fn context_selector(
        &self,
    ) -> Arc<tokio::sync::RwLock<crate::context_selector::ContextSelector>> {
        Arc::clone(&self.context_selector)
    }

    /// Analyze query intent and get suggested context bundle.
    pub async fn analyze_intent(&self, query: &str) -> (IntentAnalysis, ContextBundle) {
        let selector = self.context_selector.read().await;
        selector.select_context(query).await
    }

    /// Update device types in the context selector.
    pub async fn update_context_device_types(
        &self,
        device_types: Vec<neomind_devices::mdl_format::DeviceTypeDefinition>,
    ) {
        let selector = self.context_selector.read().await;
        selector.set_device_types(device_types).await;
        selector.register_with_analyzer().await;
    }

    /// Update rule engine in the context selector.
    pub async fn update_context_rule_engine(&self, engine: Arc<neomind_rules::RuleEngine>) {
        let selector = self.context_selector.read().await;
        selector.set_rule_engine(engine).await;
    }

    // === P4.2: INTELLIGENT MEMORY INJECTION HELPERS ===

    /// Extract entities and topics from conversation for memory retrieval.
    ///
    /// Analyzes recent messages to identify:
    /// - Device mentions (device IDs, locations)
    /// - Topic keywords (temperature, lighting, etc.)
    /// - Rule/automation mentions
    fn extract_conversation_entities_topics(
        &self,
        messages: &[AgentMessage],
    ) -> (Vec<String>, Vec<String>) {
        use std::collections::HashSet;

        let mut entities = HashSet::new();
        let mut topics = HashSet::new();

        // Analyze last 10 messages
        let recent_count = messages.len().min(10);
        let recent = &messages[messages.len().saturating_sub(recent_count)..];

        for msg in recent {
            let content = msg.content.to_lowercase();

            // Extract device entities
            for word in content.split_whitespace() {
                if word.starts_with("device_") || word.starts_with("设备") {
                    entities.insert(word.to_string());
                }
                // Location mentions
                if word.contains("客厅") || word.contains("卧室") || word.contains("厨房") {
                    entities.insert(word.to_string());
                }
                if word.contains("living") || word.contains("bedroom") || word.contains("kitchen") {
                    entities.insert(word.to_string());
                }
            }

            // Extract topics
            if content.contains("温度") || content.contains("temperature") {
                topics.insert("temperature".to_string());
            }
            if content.contains("灯") || content.contains("light") {
                topics.insert("lighting".to_string());
            }
            if content.contains("湿度") || content.contains("humidity") {
                topics.insert("humidity".to_string());
            }
            if content.contains("规则") || content.contains("自动化") || content.contains("rule")
            {
                topics.insert("automation".to_string());
            }
        }

        (entities.into_iter().collect(), topics.into_iter().collect())
    }

    /// Build memory injection hint based on entities, topics, and health status.
    ///
    /// Creates a system prompt that guides the LLM to recall relevant context
    /// from long-term memory. This is a framework that can be extended with
    /// actual memory queries when a persistent memory service is integrated.
    fn build_memory_injection_hint(
        &self,
        entities: &[String],
        topics: &[String],
        health: &crate::context::ContextHealth,
    ) -> String {
        if entities.is_empty() && topics.is_empty() {
            return String::new();
        }

        let mut hint = String::from("[Memory Recall] ");

        // Add health-based guidance
        if let Some(recommendation) = health.recommendation() {
            hint.push_str(&format!("Context hint: {}. ", recommendation));
        }

        // Add entity recall hints
        if !entities.is_empty() {
            hint.push_str(&format!("Related entities: {}. ", entities.join(", ")));
        }

        // Add topic recall hints
        if !topics.is_empty() {
            hint.push_str(&format!(
                "Previous topics discussed: {}. ",
                topics.join(", ")
            ));
        }

        // Add guidance for memory retrieval
        hint.push_str("Consider any relevant information from earlier in the conversation that might help with the current request.");

        hint
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
            let resource =
                crate::context::Resource::device(&device.id, &device.name, &device.device_type)
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
        let rules_data: Vec<(String, String, bool)> = rules
            .into_iter()
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
            ("你好", "你好！我是 NeoMind 智能助手，有什么可以帮您？"),
            ("您好", "您好！我是 NeoMind 智能助手，有什么可以帮您？"),
            (
                "hi",
                "Hello! I'm NeoMind, your smart assistant. How can I help you?",
            ),
            ("hello", "Hello! I'm NeoMind, your smart assistant."),
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
            (
                "thanks",
                "You're welcome! Is there anything else I can help with?",
            ),
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
            self.internal_state.write().await.push_message(user_msg);
            self.internal_state
                .write()
                .await
                .push_message(response.message.clone());

            return Ok(response);
        }

        // === NORMAL PATH: Acquire lock for complex processing ===
        let _lock = self.process_lock.lock().await;

        let start = std::time::Instant::now();

        // === SMART FOLLOWUP INTERCEPTION (Context-Aware) ===
        // More advanced interception with conversation context awareness
        let followup_analysis = {
            let ctx = self.conversation_context.read().await;
            let mut followup = self.smart_followup.write().await;
            followup.analyze_input(user_message, &ctx)
        };

        // Handle smart followup cases
        if !followup_analysis.can_proceed {
            let response_content = if let Some(first_followup) = followup_analysis.followups.first()
            {
                // Use the highest priority followup
                let mut content = first_followup.question.clone();

                // Add suggestions if available
                if !first_followup.suggestions.is_empty() {
                    content.push_str("\n\n建议选项：");
                    for (i, suggestion) in first_followup.suggestions.iter().enumerate() {
                        content.push_str(&format!("\n{}. {}", i + 1, suggestion));
                    }
                }

                content
            } else {
                // Should not reach here, but fallback
                "我明白您的请求，但需要更多信息。".to_string()
            };

            // Save user message and our response to history
            let user_msg = AgentMessage::user(user_message);
            let response_msg = AgentMessage::assistant(&response_content);

            self.internal_state.write().await.push_message(user_msg);
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

        // === SMART CONVERSATION INTERCEPTION (Simple, Fallback) ===
        // Simple pattern-based interception for backward compatibility
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

            self.internal_state.write().await.push_message(user_msg);
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

        // === INTENT ANALYSIS: Use ContextSelector to analyze user intent ===
        let (intent_analysis, _context_bundle) = self.analyze_intent(user_message).await;
        tracing::info!(
            intent = ?intent_analysis.intent_type,
            confidence = intent_analysis.confidence,
            entities = intent_analysis.entities.len(),
            scope = ?intent_analysis.context_scope,
            "Intent analysis completed"
        );

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
                    let tool_results: Vec<(String, String)> = response
                        .tool_calls
                        .iter()
                        .filter_map(|tc| {
                            tc.result.as_ref().map(|r| {
                                (
                                    tc.name.clone(),
                                    serde_json::to_string(r)
                                        .unwrap_or_else(|_| "无结果".to_string()),
                                )
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
        let mut parts = vec![neomind_core::ContentPart::text(user_message)];
        let mut user_images = Vec::new();

        // Process images for both ContentPart and storage
        for image_data in &images {
            // Extract mime type from data URL
            let (mime_type_str, base64_part) = if image_data.starts_with("data:image/") {
                if let Some(pos) = image_data.find(',') {
                    let mime = if image_data.contains("data:image/png") {
                        "image/png"
                    } else if image_data.contains("data:image/jpeg")
                        || image_data.contains("data:image/jpg")
                    {
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
            parts.push(neomind_core::ContentPart::image_base64(
                base64_part,
                mime_type_str,
            ));

            // Add to storage as AgentMessageImage
            user_images.push(crate::agent::types::AgentMessageImage {
                data: image_data.clone(),
                mime_type: Some(mime_type_str.to_string()),
            });
        }

        let user_msg = neomind_core::Message::new(
            neomind_core::MessageRole::User,
            neomind_core::Content::Parts(parts),
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
            return Err(NeoMindError::Llm(
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
        let core_history: Vec<neomind_core::Message> = history_without_last
            .iter()
            .map(|msg| msg.to_core())
            .collect();

        // === Process with LLM using multimodal message ===
        match self
            .llm_interface
            .chat_multimodal_with_history(user_msg, &core_history)
            .await
        {
            Ok(llm_response) => {
                let response_msg = AgentMessage::assistant(&llm_response.text);

                self.internal_state
                    .write()
                    .await
                    .push_message(response_msg.clone());

                self.internal_state
                    .write()
                    .await
                    .session
                    .increment_messages();

                let processing_time = start.elapsed().as_millis() as u64;

                Ok(AgentResponse {
                    message: response_msg,
                    tool_calls: vec![],
                    memory_context_used: false,
                    tools_used: vec![],
                    processing_time_ms: processing_time,
                })
            }
            Err(e) => Err(NeoMindError::Llm(format!("LLM processing failed: {}", e))),
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
            let (message, _, _) =
                process_fallback(&self.tools, &self.fallback_rules, user_message).await;
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
        // Reserve space for: system prompt, user message, and generation
        let max_context = self.llm_interface.max_context_length().await;

        // Calculate space needed for non-history components
        // System prompt (~500 tokens) + user message (~200 tokens) + context injection (~200 tokens) + generation reserve (~1000 tokens)
        const NON_HISTORY_TOKENS: usize = 1900;
        let safe_max_context = max_context.saturating_sub(NON_HISTORY_TOKENS);

        // === P3.2: ADAPTIVE CONTEXT SIZING ===
        // Adjust context size based on conversation complexity:
        // - High entity diversity: +10%
        // - Multiple active topics: +10%
        // - Recent errors: +15%
        // - Simple greetings: -10%
        let adaptive_adjustment = calculate_adaptive_context_adjustment(&history_without_last);

        // Calculate effective max with adaptive adjustment, ensuring we don't exceed safe limit
        let effective_max = ((safe_max_context as f64) * adaptive_adjustment) as usize;

        // Enforce reasonable bounds (minimum 1024 tokens for history, maximum safe limit)
        let effective_max = effective_max.clamp(1024, safe_max_context);

        tracing::debug!(
            "Context window: model_capacity={}, safe_max={}, adjustment={:.2}, effective_max={}",
            max_context,
            safe_max_context,
            adaptive_adjustment,
            effective_max
        );

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

        // === CONVERSATION CONTEXT: Inject context summary ONLY if it changed ===
        // This prevents repeatedly injecting the same context which can cause
        // the LLM to generate repetitive responses
        let context_summary = {
            let ctx = self.conversation_context.read().await;
            let summary = ctx.get_context_summary();
            if !summary.is_empty() {
                Some(format!("当前对话上下文：\n{}", summary))
            } else {
                None
            }
        };

        // Only inject context if it has changed since last time
        // Use a simple hash to detect changes
        if let Some(summary) = context_summary {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};

            let summary_hash = {
                let mut h = DefaultHasher::new();
                summary.hash(&mut h);
                h.finish()
            };

            let mut last_hash = self.last_injected_context_hash.write().await;
            if *last_hash != summary_hash {
                // Context has changed, inject it
                *last_hash = summary_hash;
                drop(last_hash); // Release lock before proceeding

                use neomind_core::message::{Content, MessageRole};
                core_history.push(Message::new(MessageRole::System, Content::text(&summary)));
                tracing::debug!(
                    "Injected conversation context into LLM history (changed from previous)"
                );
            } else {
                drop(last_hash); // Release lock
                tracing::debug!("Skipping context injection - unchanged from previous");
            }
        }

        // === P4.2: INTELLIGENT MEMORY INJECTION (TEMPORARILY DISABLED FOR DEBUGGING) ===
        // Detect stale context and inject relevant long-term memory summaries
        // This improves context continuity in long conversations
        /*
        use crate::context::{calculate_health, ContextHealth};
        let health = calculate_health(&history_without_last);

        // Inject memory if context is degraded
        if health.needs_refresh() {
            tracing::debug!(
                "Context health degraded (score: {:.2}), injecting memory hints",
                health.overall_score
            );

            // Extract entities and topics from recent conversation
            let (entities, topics) = self.extract_conversation_entities_topics(&history_without_last);

            if !entities.is_empty() || !topics.is_empty() {
                // Build memory hint prompt
                let memory_hint = self.build_memory_injection_hint(&entities, &topics, &health);

                if !memory_hint.is_empty() {
                    use neomind_core::message::{Content, MessageRole};
                    core_history.push(Message::new(MessageRole::System, Content::text(&memory_hint)));
                    tracing::debug!("Injected memory hint into context");
                }
            }
        }
        */

        // Call LLM with conversation history (user message will be added by LLM interface)
        let chat_response = self
            .llm_interface
            .chat_with_history(user_message, &core_history)
            .await
            .map_err(|e| super::error::NeoMindError::Llm(e.to_string()))?;

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
        if tool_calls.is_empty() {
            if let Some(ref thinking_content) = thinking {
                if let Ok((_, thinking_tool_calls)) = parse_tool_calls(thinking_content) {
                    if !thinking_tool_calls.is_empty() {
                        tracing::debug!("Found tool calls in thinking field, using them");
                        tool_calls = thinking_tool_calls;
                    }
                }
            }
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

            // === SAFEGUARD: Register response for cross-turn repetition detection ===
            {
                let mut state = self.internal_state.write().await;
                state.register_response(&content);
                state.push_message(assistant_msg.clone());
            }

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
                tool_call
                    .arguments
                    .to_string()
                    .chars()
                    .take(100)
                    .collect::<String>(),
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

        // === DEPENDENCY-AWARE TOOL SCHEDULING ===
        // Group tools into execution batches based on dependencies:
        // - Tools in the same batch can execute in parallel
        // - Tools in later batches wait for results from earlier batches
        // - Mutually exclusive tools are detected and handled
        let execution_batches = self.build_execution_batches(&tool_calls).await;

        tracing::debug!(
            count = execution_batches.len(),
            batches = ?execution_batches.iter().map(|b| b.len()).collect::<Vec<_>>(),
            "Tool execution batches (dependency-aware)"
        );

        let mut tool_results = Vec::new();
        let mut tools_used = Vec::new();
        let mut tool_calls_with_results = Vec::new();

        // Execute each batch in sequence
        for (batch_idx, batch) in execution_batches.iter().enumerate() {
            if batch.len() > 1 {
                tracing::debug!(
                    batch = batch_idx,
                    size = batch.len(),
                    "Executing batch in parallel"
                );
            }

            // Clone tool_calls for parallel execution within this batch
            let batch_clone: Vec<_> = batch.iter().cloned().collect();

            // Use futures for parallel execution within batch
            let futures: Vec<_> = batch_clone
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

            // Execute all tools in this batch in parallel and wait for completion
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
            if tool_name == "create_rule" || tool_name == "rule_from_context" {
                // Check if we have simplified parameters (condition + action) but no dsl
                let has_condition = args_obj.contains_key("condition");
                let has_action = args_obj.contains_key("action");
                let has_description = args_obj.contains_key("description");
                let has_dsl = args_obj.contains_key("dsl");

                if (has_condition || has_description) && !has_dsl {
                    // Convert simplified parameters to DSL
                    let name = args_obj
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("未命名规则");

                    let dsl = if has_description {
                        // rule_from_context: extract structured rule from description
                        let description = args_obj
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        // Try to parse the description to extract condition/action
                        // For now, generate a simple DSL with the description as context
                        format!(
                            r#"RULE "{name}"
WHEN sensor.temperature > 30
DO
  NOTIFY "{description}"
END"#
                        )
                    } else if has_condition && has_action {
                        // create_rule with simplified condition/action
                        let condition = args_obj
                            .get("condition")
                            .and_then(|v| v.as_str())
                            .unwrap_or("sensor.temperature > 30");

                        let action = args_obj
                            .get("action")
                            .and_then(|v| v.as_str())
                            .unwrap_or("通知管理员");

                        format!(
                            r#"RULE "{name}"
WHEN {condition}
DO
  NOTIFY "{action}"
END"#
                        )
                    } else {
                        // Fallback: just use the name
                        format!(
                            r#"RULE "{name}"
WHEN sensor.temperature > 30
DO
  NOTIFY "规则触发"
END"#
                        )
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

            // Delegate to mapper.rs for standard parameter mapping
            map_tool_parameters(tool_name, arguments)
        } else {
            arguments.clone()
        }
    }

    /// Map simplified tool names to real tool names.
    ///
    /// Simplified names are used in LLM prompts (e.g., "device_discover")
    /// Real names are the same as simplified names since we unified to underscore naming.
    ///
    /// This now uses the unified `ToolNameMapper` to ensure consistency
    /// across the codebase.
    fn resolve_tool_name(&self, simplified_name: &str) -> String {
        // Delegate to the unified mapper
        crate::tools::resolve_tool_name(simplified_name)
    }

    /// Build execution batches based on tool dependencies.
    ///
    /// Returns a Vec of batches where:
    /// - Each batch contains tools that can be executed in parallel
    /// - Later batches depend on results from earlier batches
    /// - Tools with dependencies are scheduled after their prerequisites
    ///
    /// Uses ToolRelationships metadata from tool definitions:
    /// - `call_after`: Prerequisites that must execute first
    /// - `output_to`: Tools that depend on this tool's output
    /// - `exclusive_with`: Tools that cannot run together
    async fn build_execution_batches(&self, tool_calls: &[ToolCall]) -> Vec<Vec<ToolCall>> {
        use std::collections::{HashMap, HashSet};

        if tool_calls.is_empty() {
            return vec![];
        }

        // If only one tool, no batching needed
        if tool_calls.len() == 1 {
            return vec![tool_calls.to_vec()];
        }

        // Build maps for normalized tool names and relationships
        // We need to track both original names (for ToolCall cloning) and resolved names (for dependency resolution)
        let mut original_to_resolved: HashMap<String, String> = HashMap::new();
        let mut resolved_to_original: HashMap<String, String> = HashMap::new();

        // First pass: resolve all tool names
        for tool_call in tool_calls {
            let real_name = self.resolve_tool_name(&tool_call.name);
            original_to_resolved.insert(tool_call.name.clone(), real_name.clone());
            resolved_to_original
                .entry(real_name)
                .or_insert_with(|| tool_call.name.clone());
        }

        // Build relationships map using RESOLVED names as keys
        let mut relationships: HashMap<String, neomind_core::tools::ToolRelationships> =
            HashMap::new();
        for tool_call in tool_calls {
            let real_name = self.resolve_tool_name(&tool_call.name);
            if let Some(tool) = self.tools.get(&real_name) {
                relationships.insert(real_name, tool.definition().relationships);
            } else {
                // No relationships found, use default
                relationships.insert(real_name, neomind_core::tools::ToolRelationships::default());
            }
        }

        // Create a resolved name to ToolCall map
        let mut resolved_to_call: HashMap<String, ToolCall> = HashMap::new();
        for tool_call in tool_calls {
            let real_name = self.resolve_tool_name(&tool_call.name);
            resolved_to_call.insert(real_name, tool_call.clone());
        }

        // Track which tools are in each batch (using resolved names)
        let mut batches: Vec<Vec<ToolCall>> = vec![];
        let mut placed_resolved: HashSet<String> = HashSet::new();

        // Kahn's algorithm for topological sorting
        loop {
            let mut current_batch = Vec::new();
            let mut current_batch_resolved: HashSet<String> = HashSet::new();

            for tool_call in tool_calls {
                let resolved_name = &original_to_resolved[&tool_call.name];

                // Skip if already placed
                if placed_resolved.contains(resolved_name) {
                    continue;
                }

                // Get dependencies (call_after)
                let deps = relationships
                    .get(resolved_name)
                    .map(|r| r.call_after.clone())
                    .unwrap_or_default();

                // Check if all dependencies are satisfied
                // Dependencies are specified using resolved tool names
                let deps_satisfied = deps.iter().all(|dep| {
                    // Resolve the dependency name to handle any aliases in the dependency spec
                    let resolved_dep = self.resolve_tool_name(dep);
                    // Dependency is satisfied if:
                    // 1. It's not in our current tool_calls (not being executed)
                    // 2. Or it's already been placed in a previous batch
                    !resolved_to_call.contains_key(&resolved_dep)
                        || placed_resolved.contains(&resolved_dep)
                });

                if !deps_satisfied {
                    continue; // Wait for dependencies
                }

                // Check for mutual exclusivity with tools in current batch
                let exclusive_with = relationships
                    .get(resolved_name)
                    .map(|r| r.exclusive_with.clone())
                    .unwrap_or_default();

                let conflicts_with_batch = exclusive_with.iter().any(|excl| {
                    let resolved_excl = self.resolve_tool_name(excl);
                    current_batch_resolved.contains(&resolved_excl)
                });

                if conflicts_with_batch {
                    continue; // Can't run with current batch, try next batch
                }

                // Can add to current batch
                current_batch.push(tool_call.clone());
                current_batch_resolved.insert(resolved_name.clone());
            }

            if current_batch.is_empty() {
                // No more tools can be placed
                break;
            }

            // Mark tools as placed
            for name in current_batch_resolved {
                placed_resolved.insert(name);
            }

            batches.push(current_batch);

            // Exit if all tools are placed
            if placed_resolved.len() == tool_calls.len() {
                break;
            }
        }

        // Handle circular dependency case (tools still not placed)
        if placed_resolved.len() < tool_calls.len() {
            tracing::warn!(
                placed = placed_resolved.len(),
                total = tool_calls.len(),
                "Circular dependency detected, remaining tools will execute in single batch"
            );
            // Add remaining tools as a final batch
            let remaining: Vec<_> = tool_calls
                .iter()
                .filter(|tc| !placed_resolved.contains(&original_to_resolved[&tc.name]))
                .cloned()
                .collect();
            if !remaining.is_empty() {
                batches.push(remaining);
            }
        }

        // If batching didn't work (shouldn't happen), fall back to single batch
        if batches.is_empty() {
            batches.push(tool_calls.to_vec());
        }

        batches
    }

    /// Sanitize tool output for LLM consumption.
    ///
    /// If the output is too large (e.g., base64 images, large files),
    /// intelligently truncates while preserving useful structure.
    ///
    /// ## Strategy
    /// - **Small outputs** (<10KB): Return as-is
    /// - **Base64 strings**: Replace with metadata only (LLM can't use them)
    /// - **Large strings**: Truncate with preview
    /// - **Arrays**: Keep first N items + count of remaining
    /// - **Objects**: Keep small fields, truncate large fields
    /// - **device_discover**: Always preserve summary and device list with id/name/type
    ///
    /// This preserves data structure while avoiding token waste on unusable data.
    fn sanitize_tool_output_for_llm(&self, tool_name: &str, data: &serde_json::Value) -> String {
        const MAX_SIZE: usize = 10_240; // 10KB
        const MAX_PREVIEW_ITEMS: usize = 5; // Keep first 5 array items
        const MAX_STRING_PREVIEW: usize = 500; // 500 chars for string preview

        // Special handling for device_discover - preserve critical structure
        if tool_name == "device_discover" {
            if let Some(obj) = data.as_object() {
                // Always preserve summary - it has accurate counts
                let summary = obj
                    .get("summary")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));

                // Extract minimal device info: id, name, type, status
                let devices: Vec<serde_json::Value> = obj.get("groups")
                    .and_then(|g| g.as_array())
                    .iter()
                    .flat_map(|groups| groups.iter())
                    .filter_map(|g| g.get("devices").and_then(|d| d.as_array()))
                    .flat_map(|devices| devices.iter())
                    .map(|device| {
                        serde_json::json!({
                            "id": device.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                            "name": device.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                            "device_type": device.get("device_type").and_then(|v| v.as_str()).unwrap_or(""),
                            "status": device.get("status").and_then(|v| v.as_str()).unwrap_or("unknown")
                        })
                    })
                    .collect();

                let compact = serde_json::json!({
                    "summary": summary,
                    "device_count": devices.len(),
                    "devices": devices
                });

                let result = serde_json::to_string(&compact).unwrap_or_else(|_| "null".to_string());
                let original_size = serde_json::to_string(data).unwrap_or_default().len();

                if result.len() != original_size {
                    tracing::info!(
                        session_id = %self.session_id,
                        tool = %tool_name,
                        original_bytes = original_size,
                        compressed_bytes = result.len(),
                        "device_discover output compressed for LLM"
                    );
                }

                return result;
            }
        }

        // Recursively truncate value to fit within MAX_SIZE
        let truncated = self.truncate_value(data, MAX_SIZE, MAX_PREVIEW_ITEMS, MAX_STRING_PREVIEW);

        let result = serde_json::to_string(&truncated).unwrap_or_else(|_| "null".to_string());
        let original_size = serde_json::to_string(data).unwrap_or_default().len();

        // Log if truncation occurred
        if result.len() != original_size {
            tracing::warn!(
                session_id = %self.session_id,
                tool = %tool_name,
                original_bytes = original_size,
                truncated_bytes = result.len(),
                "Tool output truncated for LLM"
            );
        }

        result
    }

    /// Recursively truncate a JSON value to fit within size constraints.
    ///
    /// This preserves data structure while trimming large content.
    fn truncate_value(
        &self,
        value: &serde_json::Value,
        max_size: usize,
        max_items: usize,
        max_string: usize,
    ) -> serde_json::Value {
        match value {
            // Base64 detection: long string with only base64 chars
            serde_json::Value::String(s) if s.len() > 500 && self.is_likely_base64(s) => {
                // Replace base64 with metadata
                serde_json::json!({
                    "_truncated": true,
                    "_type": "base64_data",
                    "size_bytes": s.len()
                })
            }

            // Regular long string - truncate with preview
            serde_json::Value::String(s) if s.len() > max_string => {
                let preview: String = s.chars().take(max_string).collect();
                serde_json::json!({
                    "_truncated": true,
                    "_original_length": s.len(),
                    "preview": format!("{}...", preview)
                })
            }

            // Array - keep first N items + count
            serde_json::Value::Array(arr) => {
                if arr.len() <= max_items {
                    // Check each element recursively
                    let truncated: Vec<serde_json::Value> = arr
                        .iter()
                        .map(|v| {
                            self.truncate_value(
                                v,
                                max_size / arr.len().max(1),
                                max_items,
                                max_string,
                            )
                        })
                        .collect();
                    serde_json::Value::Array(truncated)
                } else {
                    let kept: Vec<serde_json::Value> = arr
                        .iter()
                        .take(max_items)
                        .map(|v| {
                            self.truncate_value(v, max_size / max_items, max_items, max_string)
                        })
                        .collect();

                    serde_json::json!({
                        "_truncated": true,
                        "_total_count": arr.len(),
                        "_showing_first": max_items,
                        "items": kept
                    })
                }
            }

            // Object - process each field, truncate large ones
            serde_json::Value::Object(obj) => {
                let mut result = serde_json::Map::new();

                for (key, val) in obj.iter() {
                    // Skip known large fields (like full data content)
                    if matches!(key.as_str(), "data" | "content" | "base64" | "image")
                        && serde_json::to_string(val).unwrap_or_default().len() > 1000
                    {
                        result.insert(
                            format!("_{}_truncated", key),
                            serde_json::json!({
                                "_size_bytes": serde_json::to_string(val).unwrap_or_default().len(),
                                "_note": "Large data omitted"
                            }),
                        );
                        continue;
                    }

                    let truncated = self.truncate_value(
                        val,
                        max_size / obj.len().max(1),
                        max_items,
                        max_string,
                    );
                    result.insert(key.clone(), truncated);

                    // Early exit if we're getting too big
                    if serde_json::to_string(&result).unwrap_or_default().len() > max_size {
                        break;
                    }
                }

                if !result.is_empty() {
                    serde_json::Value::Object(result)
                } else {
                    serde_json::json!({"_truncated": true, "_note": "All fields were too large"})
                }
            }

            // Other types pass through
            _ => value.clone(),
        }
    }

    /// Check if a string is likely base64 encoded data.
    fn is_likely_base64(&self, s: &str) -> bool {
        // Base64 contains only these chars
        let base64_chars = s.chars().take(1000).all(|c| {
            c.is_alphanumeric() || c == '+' || c == '/' || c == '=' || c == '\n' || c == '\r'
        });
        // Must be reasonably long and look like base64
        base64_chars && s.len() > 100
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

        // === CACHE: Check if result is cached ===
        let args_key = arguments.to_string();
        if let Some(cached_result) = self.tool_result_cache.read().await.get(name, &args_key) {
            let elapsed = start.elapsed();
            tracing::debug!(
                session_id = %self.session_id,
                tool = %name,
                elapsed_ms = elapsed.as_millis(),
                "Tool result served from cache"
            );
            return Ok(cached_result);
        }

        // Map simplified tool name to real tool name
        let real_tool_name = self.resolve_tool_name(name);

        // Convert simplified parameter names to actual tool parameters
        let mapped_arguments = self.map_simplified_parameters(name, arguments);

        // === SEMANTIC MAPPING: Convert natural language to technical IDs ===
        // This maps "客厅灯" -> "light_living_main" for device_id parameters
        let semantically_mapped = self
            .semantic_mapper
            .map_tool_parameters(&real_tool_name, mapped_arguments.clone())
            .await
            .unwrap_or(mapped_arguments);

        // Sanitize arguments for logging (limit size to avoid log spam)
        let args_preview = if semantically_mapped.to_string().len() > 200 {
            format!(
                "{}...",
                &semantically_mapped
                    .to_string()
                    .chars()
                    .take(200)
                    .collect::<String>()
            )
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
            match self
                .tools
                .execute(&real_tool_name, semantically_mapped.clone())
                .await
            {
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
                        return Ok(format!("工具 {} 执行失败: {}", real_tool_name, error_msg));
                    }

                    tracing::debug!(
                        session_id = %self.session_id,
                        tool = %real_tool_name,
                        elapsed_ms = elapsed.as_millis(),
                        "Tool executed successfully"
                    );

                    // Sanitize output to avoid sending large data (base64, files, etc.) to LLM
                    let sanitized =
                        self.sanitize_tool_output_for_llm(&real_tool_name, &output.data);

                    // === CACHE: Store successful result ===
                    self.tool_result_cache
                        .write()
                        .await
                        .put(name, args_key, sanitized.clone());

                    return Ok(sanitized);
                }
                Err(e) => {
                    let last_error = e.to_string();
                    let elapsed = start.elapsed();

                    // Categorize the error for better debugging
                    let error_category = if last_error.contains("not_found")
                        || last_error.contains("unknown")
                    {
                        "tool_not_found"
                    } else if last_error.contains("timeout") {
                        "timeout"
                    } else if last_error.contains("network") || last_error.contains("connection") {
                        "network"
                    } else if last_error.contains("parse") || last_error.contains("invalid") {
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

        Err(super::error::NeoMindError::Tool(format!(
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
    use neomind_tools::{Result, Tool, ToolError, ToolOutput};
    use serde_json::json;

    /// Simple mock device_discover tool for testing
    struct MockDeviceDiscoverTool;
    #[async_trait::async_trait]
    impl Tool for MockDeviceDiscoverTool {
        fn name(&self) -> &str {
            "device_discover"
        }
        fn description(&self) -> &str {
            "List all devices (mock for testing)"
        }
        fn parameters(&self) -> serde_json::Value {
            json!({})
        }
        async fn execute(&self, _args: serde_json::Value) -> Result<ToolOutput> {
            let data = json!({"devices": [{"id": "mock_device_1", "name": "模拟设备"}]});
            Ok(ToolOutput::success(data))
        }
    }

    /// Simple mock list_rules tool for testing
    struct MockListRulesTool;
    #[async_trait::async_trait]
    impl Tool for MockListRulesTool {
        fn name(&self) -> &str {
            "list_rules"
        }
        fn description(&self) -> &str {
            "List all rules (mock for testing)"
        }
        fn parameters(&self) -> serde_json::Value {
            json!({})
        }
        async fn execute(&self, _args: serde_json::Value) -> Result<ToolOutput> {
            let data = json!({"rules": [{"id": "mock_rule_1", "name": "Mock Rule"}]});
            Ok(ToolOutput::success(data))
        }
    }

    /// Simple mock query_data tool for testing
    struct MockQueryDataTool;
    #[async_trait::async_trait]
    impl Tool for MockQueryDataTool {
        fn name(&self) -> &str {
            "query_data"
        }
        fn description(&self) -> &str {
            "Query metric data (mock for testing)"
        }
        fn parameters(&self) -> serde_json::Value {
            json!({"type": "object"})
        }
        async fn execute(&self, _args: serde_json::Value) -> Result<ToolOutput> {
            let data = json!({"data": [{"metric": "temperature", "value": 25.5}]});
            Ok(ToolOutput::success(data))
        }
    }

    /// Simple mock greet tool for testing
    struct MockGreetTool;
    #[async_trait::async_trait]
    impl Tool for MockGreetTool {
        fn name(&self) -> &str {
            "greet"
        }
        fn description(&self) -> &str {
            "Greet the user (mock for testing)"
        }
        fn parameters(&self) -> serde_json::Value {
            json!({})
        }
        async fn execute(&self, _args: serde_json::Value) -> Result<ToolOutput> {
            let data = json!({"message": "Hello there!"});
            Ok(ToolOutput::success(data))
        }
    }

    /// Create a test agent with mock tools registered
    fn create_test_agent_with_mocks(session_id: String) -> Agent {
        use neomind_tools::ToolRegistryBuilder;

        let mut registry = ToolRegistryBuilder::new().build();

        // Register mock tools
        registry.register(std::sync::Arc::new(MockDeviceDiscoverTool));
        registry.register(std::sync::Arc::new(MockListRulesTool));
        registry.register(std::sync::Arc::new(MockQueryDataTool));
        registry.register(std::sync::Arc::new(MockGreetTool));

        // Add default agent tools
        use crate::tools::{AskUserTool, ClarifyIntentTool, ConfirmActionTool};
        use crate::tools::{ThinkTool, ToolSearchTool};

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

        Agent::with_tools(
            AgentConfig::default(),
            session_id,
            std::sync::Arc::new(registry),
        )
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
        assert!(tools.contains(&"device_discover".to_string()));
        assert!(tools.contains(&"list_rules".to_string()));
    }

    #[tokio::test]
    async fn test_process_fallback() {
        let agent = create_test_agent_with_mocks("test_session".to_string());
        let response = agent.process("列出所有设备").await.unwrap();

        assert!(response.message.content.contains("设备"));
        assert!(response.tools_used.contains(&"device_discover".to_string()));
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
        let custom_rules = vec![FallbackRule::new(vec!["greet", "greeting"], "greet")
            .with_response_template("Greeting from fallback!")];
        let agent = create_test_agent_with_mocks("test_session".to_string())
            .with_fallback_rules(custom_rules);

        // Use "greet me" which won't match fast-path patterns
        let response = agent.process("greet me").await.unwrap();
        // The greet tool should be used
        assert!(
            response.tools_used.contains(&"greet".to_string()),
            "Expected 'greet' in tools_used, got: {:?}",
            response.tools_used
        );
    }
}
