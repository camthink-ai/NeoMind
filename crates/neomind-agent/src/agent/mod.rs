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
use serde_json::Value;

use super::error::Result;
use super::llm::{ChatConfig, LlmInterface};
use super::tools::mapper::map_tool_parameters;
use crate::context::ResourceIndex;
use crate::llm_backends::{CloudConfig, CloudRuntime, OllamaConfig, OllamaRuntime};
use neomind_core::{config::agent_env_vars, llm::backend::LlmRuntime, Message};

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
    events_to_string_stream, process_multimodal_stream_events_with_safeguards,
    process_stream_events_with_safeguards, StreamSafeguards,
};
pub use types::{
    AgentConfig, AgentEvent, AgentInternalState, AgentMessage, AgentMessageImage, AgentResponse,
    LlmBackend, SessionState, ToolCall,
};

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

        // Handle role:tool messages (tool result messages from LargeDataCache)
        if msg.role == "tool" {
            tool_result_count += 1;
            if tool_result_count <= keep_recent && msg.content.len() <= 8000 {
                result.push(msg.clone());
            } else {
                // Compress tool result — preserve meaningful preview
                let summary: Arc<str> = if msg.content.len() > 800 {
                    let name = msg.tool_call_name.as_deref().unwrap_or("unknown");
                    let preview: String = msg.content.chars().take(500).collect();
                    format!(
                        "[Tool: {} result ({} chars): {}...]",
                        name,
                        msg.content.len(),
                        preview
                    )
                    .into()
                } else {
                    msg.content.clone()
                };
                result.push(AgentMessage {
                    content: summary,
                    ..msg.clone()
                });
            }
            continue;
        }

        // Check if this is a tool result message (has tool_calls)
        if msg.tool_calls.is_some() && msg.tool_calls.as_ref().is_some_and(|t| !t.is_empty()) {
            tool_result_count += 1;

            // Keep recent tool results intact
            if tool_result_count <= keep_recent {
                result.push(msg.clone());
            } else {
                // Compress old tool results to a descriptive summary
                // that preserves action type, key arguments, and result preview
                let summaries: Vec<String> = msg
                    .tool_calls
                    .as_ref()
                    .iter()
                    .flat_map(|calls| calls.iter())
                    .map(|tc| {
                        let args_summary = types::summarize_tool_args(&tc.name, &tc.arguments);
                        let result_preview = tc
                            .result
                            .as_ref()
                            .map(|r| {
                                let s = if let Some(s) = r.as_str() {
                                    s.to_string()
                                } else {
                                    r.to_string()
                                };
                                // Read actions need more preview to preserve data
                                let is_data_action = args_summary.contains("list")
                                    || args_summary.contains("get")
                                    || args_summary.contains("history");
                                let preview_len = if is_data_action { 300 } else { 80 };
                                s.chars().take(preview_len).collect::<String>()
                            })
                            .unwrap_or_default();
                        if result_preview.is_empty() {
                            format!("the {} tool with {}", tc.name, args_summary)
                        } else {
                            format!(
                                "the {} tool with {} and received: {}",
                                tc.name, args_summary, result_preview
                            )
                        }
                    })
                    .collect();

                let summary = format!(
                    "Previously called {}. These are past results, do not repeat.",
                    summaries.join(", then ")
                );

                result.push(AgentMessage {
                    role: msg.role.clone(),
                    content: summary.into(),
                    tool_calls: None,
                    tool_call_id: None,
                    tool_call_name: None,
                    thinking: None,
                    images: None,
                    round_contents: None,
                    round_thinking: None,
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
    let mut _current_tokens = 0;

    // First pass: keep recent messages intact
    let recent_start = messages.len().saturating_sub(keep_recent);
    for msg in &messages[recent_start..] {
        result.push(msg.clone());
        _current_tokens += tokenizer::estimate_message_tokens(msg);
    }

    // If we're already under the token limit, return early
    if _current_tokens <= target_tokens {
        // Still need to add older messages in reverse order
        for msg in messages[..recent_start].iter().rev() {
            let msg_tokens = tokenizer::estimate_message_tokens(msg);
            if _current_tokens + msg_tokens > target_tokens {
                break;
            }
            result.insert(0, msg.clone());
            _current_tokens += msg_tokens;
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
            _current_tokens += tokenizer::estimate_message_tokens(msg);
            continue;
        }

        // Keep user messages verbatim (they contain critical intent)
        if msg.role == "user" {
            // Truncate very long user messages
            let truncated_content: Arc<str> = if msg.content.len() > 200 {
                let s: String = msg.content.chars().take(200).collect();
                format!("{}... (message truncated)", s).into()
            } else {
                msg.content.clone()
            };
            compressed_older.push(AgentMessage {
                content: truncated_content,
                ..msg.clone()
            });
            _current_tokens += tokenizer::estimate_message_tokens(msg);
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
            format!("[Previous conversation: {}]", topic_batches.join("; "))
        } else {
            format!(
                "[Previous conversation: {} rounds, topics include {}]",
                topic_batches.len(),
                if topic_batches.len() > 5 {
                    "multiple topics"
                } else {
                    "related content"
                }
            )
        };

        // Create a synthetic summary message
        let timestamp = messages
            .first()
            .map(|m| m.timestamp)
            .unwrap_or_else(|| chrono::Utc::now().timestamp());

        compressed_older.push(AgentMessage {
            role: "system".to_string(),
            content: old_summary.into(),
            tool_calls: None,
            tool_call_id: None,
            tool_call_name: None,
            thinking: None,
            images: None,
            round_contents: None,
            round_thinking: None,
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
    if content.contains("成功")
        || content.contains("已完成")
        || content.contains("success")
        || content.contains("completed")
    {
        if let Some(tool_name) = &msg.tool_call_name {
            return format!("Executed {}", tool_name);
        }
        return "Operation completed".to_string();
    }

    if content.contains("失败")
        || content.contains("错误")
        || content.contains("failed")
        || content.contains("error")
    {
        return format!("Operation failed: {}", extract_first_phrase(content, 30));
    }

    if content.contains("查询到")
        || content.contains("数据显示")
        || content.contains("found")
        || content.contains("data shows")
    {
        return format!("Queried data: {}", extract_first_phrase(content, 30));
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

    // Hard truncate with ellipsis (UTF-8 safe — byte slicing panics on
    // multi-byte chars; floor_char_boundary lands on a valid boundary).
    let end = trimmed.floor_char_boundary(max_len.saturating_sub(3));
    format!("{}...", &trimmed[..end])
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

    // Adaptive compaction: scale parameters with model context capacity
    // Larger contexts (32k+) should preserve far more history
    let (keep_tools, compress_threshold, keep_recent, min_recent) = if max_tokens > 16000 {
        (6, 30, 14, 8) // Large context: very gentle
    } else if max_tokens > 8000 {
        (4, 20, 10, 6) // Medium context: moderate
    } else {
        (2, 12, 6, 4) // Small context: aggressive (original)
    };

    // First, apply tool result clearing
    let tool_compacted = compact_tool_results(messages, keep_tools);

    // Then apply conversation-level compression if we have many messages
    let conversation_compacted = if tool_compacted.len() > compress_threshold {
        compact_conversation(&tool_compacted, keep_recent, max_tokens)
    } else {
        tool_compacted
    };

    // Use importance-based selection for better context quality (P1.2)
    // This prioritizes system messages, user intent, and error handling
    let selected_refs = select_messages_with_importance(
        &conversation_compacted,
        max_tokens,
        min_recent,
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
        recent.iter().map(|m| m.content.as_ref()).collect();
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

    /// Get the LLM interface (for capability checks).
    pub fn llm_interface(&self) -> Arc<LlmInterface> {
        Arc::clone(&self.llm_interface)
    }

    /// Set pinned skill IDs for this session (user-selected skills).
    pub async fn set_pinned_skills(&self, skills: Vec<String>) {
        self.llm_interface.set_pinned_skills(skills).await;
    }

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

    /// Create with default config and empty tools.
    pub fn with_session(session_id: String) -> Self {
        Self::new(AgentConfig::default(), session_id)
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

    /// Set a custom LLM runtime directly (for testing purposes).
    pub async fn set_custom_llm(&self, llm: Arc<dyn LlmRuntime>) {
        self.llm_interface.set_llm(llm).await;
        self.internal_state.write().await.set_llm_ready(true);
        self.update_tool_definitions().await;
    }

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

    /// Get the session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

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

    /// Check if a memory snapshot has been loaded.
    pub async fn has_memory_snapshot(&self) -> bool {
        self.memory_snapshot.read().await.is_some()
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

        // Check greetings — EXACT match only. Prefix matching (starts_with)
        // misfired on substantive messages: "ok here's my question..." -> "OK!",
        // "行业发展" -> "好的，没问题." (matched "行"), "对称性" -> "是的，正确."
        // (matched "对"). Anything that isn't exactly the greeting falls through
        // to the LLM, which handles greetings contextually AND preserves intent
        // (e.g. "ok, create a device" no longer short-circuits to a canned "OK!").
        for (pattern, response) in greeting_responses.iter() {
            if trimmed == *pattern {
                return Some(AgentResponse {
                    message: AgentMessage::assistant(*response),
                    tool_calls: vec![],
                    memory_context_used: false,
                    tools_used: vec![],
                    processing_time_ms: start.elapsed().as_millis() as u64,
                });
            }
        }

        // Check confirmations — EXACT match only (same rationale as greetings).
        for (pattern, response) in confirmation_responses.iter() {
            if trimmed == *pattern {
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

        // Refresh tool definitions on every turn so mid-session toggles
        // (user disabled a tool via the Extensions page) take effect on the
        // next chat turn instead of requiring a new session. Cheap: filters
        // an in-memory Vec and only re-pushes if the LLM interface changed.
        self.update_tool_definitions().await;

        // Set session ID on memory tool to avoid cross-session contamination
        // (must happen under process_lock so concurrent calls to THIS session serialize)
        self.tools
            .set_memory_session_id(self.session_id.clone())
            .await;

        let start = std::time::Instant::now();

        // === SMART FOLLOWUP INTERCEPTION (Context-Aware) ===
        // Single lock acquisition for both context and followup
        let followup_analysis = {
            let mut shared = self.shared_state.write().await;
            let ctx_snapshot = shared.conversation_context.clone();
            shared
                .smart_followup
                .analyze_input(user_message, &ctx_snapshot)
        };

        // Handle smart followup cases
        if !followup_analysis.can_proceed {
            let response_content = if let Some(first_followup) = followup_analysis.followups.first()
            {
                // Use the highest priority followup
                let mut content = first_followup.question.clone();

                // Add suggestions if available
                if !first_followup.suggestions.is_empty() {
                    content.push_str("\n\nSuggested options:");
                    for (i, suggestion) in first_followup.suggestions.iter().enumerate() {
                        content.push_str(&format!("\n{}. {}", i + 1, suggestion));
                    }
                }

                content
            } else {
                // Should not reach here, but fallback
                "I understand your request, but need more information.".to_string()
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

        // === CONVERSATION CONTEXT: Enhance input with conversation state ===
        let enhanced_input = {
            let shared = self.shared_state.read().await;
            if let Some(resolved) = shared
                .conversation_context
                .resolve_ambiguous_command(user_message)
            {
                resolved
            } else {
                shared.conversation_context.enhance_input(user_message)
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
                    let mut shared = self.shared_state.write().await;
                    shared
                        .conversation_context
                        .update(user_message, &tool_results);
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
            // Parse via shared utility — handles data URLs, raw base64, and
            // jpg/jpeg aliasing consistently across all multimodal paths.
            let parsed = crate::image_utils::parse_image_data(image_data).unwrap_or(
                crate::image_utils::ParsedImage {
                    mime_type: "image/png",
                    base64: image_data.as_str(),
                },
            );
            let mime_type_str = parsed.mime_type;
            let base64_part = parsed.base64;

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
        self.process_multimodal_stream_events_with_safeguards(
            user_message,
            images,
            StreamSafeguards::default(),
        )
        .await
    }

    /// Process a multimodal user message with streaming response and custom safeguards.
    pub async fn process_multimodal_stream_events_with_safeguards(
        &self,
        user_message: &str,
        images: Vec<String>, // Base64 data URLs
        safeguards: StreamSafeguards,
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

        match process_multimodal_stream_events_with_safeguards(
            self.llm_interface.clone(),
            self.internal_state.clone(),
            self.tools.clone(),
            user_message,
            images,
            safeguards,
            None,
            None,
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
        // Uses compaction cache for incremental updates when only a few messages changed
        let compacted_history = {
            let mut state = self.internal_state.write().await;
            let current_count = state.memory.len().saturating_sub(1); // without last

            // Check cache validity: same max_tokens and small message delta
            let cached = state.compaction_cache.take();
            let compacted = if let Some((cached_count, cached_max, ref cached_msgs)) = cached {
                if cached_max == effective_max
                    && current_count > cached_count
                    && current_count <= cached_count + 4
                {
                    // Incremental: only compact the new messages and append
                    let new_msgs: Vec<AgentMessage> = history_without_last
                        .iter()
                        .skip(cached_count)
                        .cloned()
                        .collect();
                    let mut base = cached_msgs.clone();
                    if !new_msgs.is_empty() {
                        // Re-compact the tail with existing context
                        // For small deltas, just append (tool compaction will handle on next full run)
                        base.extend(new_msgs);
                        build_context_window(&base, effective_max)
                    } else {
                        base
                    }
                } else {
                    // Full recompaction needed
                    build_context_window(&history_without_last, effective_max)
                }
            } else {
                build_context_window(&history_without_last, effective_max)
            };

            // Update cache
            state.compaction_cache = Some((current_count, effective_max, compacted.clone()));
            compacted
        };

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
            let shared = self.shared_state.read().await;
            let summary = shared.conversation_context.get_context_summary();
            if !summary.is_empty() {
                Some(format!("Current conversation context:\n{}", summary))
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

            let mut shared = self.shared_state.write().await;
            if shared.last_injected_context_hash != summary_hash {
                // Context has changed, inject it
                shared.last_injected_context_hash = summary_hash;
                drop(shared); // Release lock before proceeding

                use neomind_core::message::{Content, MessageRole};
                core_history.push(Message::new(MessageRole::System, Content::text(&summary)));
                tracing::debug!(
                    "Injected conversation context into LLM history (changed from previous)"
                );
            } else {
                drop(shared); // Release lock
                tracing::debug!("Skipping context injection - unchanged from previous");
            }
        }

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
            tracing::debug!(
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
            let batch_clone: Vec<_> = batch.to_vec();
            let semaphore = self.tool_concurrency_limit.clone();

            // Use futures for parallel execution within batch (limited by semaphore)
            let futures: Vec<_> = batch_clone
                .into_iter()
                .map(|tool_call| {
                    let name = tool_call.name.clone();
                    let arguments = tool_call.arguments.clone();
                    let id = tool_call.id.clone();
                    let sem = semaphore.clone();

                    async move {
                        let _permit = match sem.acquire().await {
                            Ok(p) => p,
                            Err(e) => {
                                tracing::warn!("tool concurrency semaphore closed: {}", e);
                                return (
                                    name,
                                    id,
                                    arguments,
                                    Err(NeoMindError::Tool(
                                        "tool concurrency semaphore closed".to_string(),
                                    )),
                                );
                            }
                        };
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
                // Push the resolved tool name (e.g., "rule" instead of "list_rules")
                let resolved = self.resolve_tool_name(&name);
                tools_used.push(resolved);
                tracing::debug!(name = %name, count = tools_used.len(), "Added to tools_used");
                match result {
                    Ok(ok_result) => {
                        tool_results.push((name.clone(), ok_result.clone()));
                        tool_calls_with_results.push(ToolCall {
                            name,
                            id,
                            arguments,
                            result: Some(serde_json::json!(ok_result)),
                            round: None,
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
                            round: None,
                        });
                    }
                }
            }
        }

        tracing::debug!(tools_used = ?tools_used, "Formatting tool results");

        // Format tool results directly (unified ReAct loop — no Phase 2)
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
                    tracing::debug!(
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

        // Map simplified tool name to real tool name (for execution routing)
        let real_tool_name = self.resolve_tool_name(name);

        // Convert simplified parameter names to actual tool parameters.
        // Pass original name (not resolved) so domain-specific mapping works.
        let mapped_arguments = self.map_simplified_parameters(name, arguments);

        // === SEMANTIC MAPPING: Convert natural language to technical IDs ===
        // This maps "客厅灯" -> "light_living_main" for device_id parameters.
        // Use original name for domain matching (semantic_mapper expects "device", not "shell").
        let domain_name = crate::tools::mapper::resolve_domain_name(name);
        let semantically_mapped = self
            .semantic_mapper
            .map_tool_parameters(&domain_name, mapped_arguments.clone())
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

        // If mapper resolved a CLI domain name to "shell", convert structured args
        // to a CLI command string that ShellTool expects: {"command": "neomind <domain> ..."}
        let exec_args = if real_tool_name == "shell" && name != "shell" {
            crate::tools::mapper::build_cli_command(name, &semantically_mapped)
                .unwrap_or(semantically_mapped.clone())
        } else {
            semantically_mapped.clone()
        };

        let mut last_error = String::new();
        let mut last_attempt = 0u32;

        for attempt in 0..=MAX_RETRIES {
            match self.tools.execute(&real_tool_name, exec_args.clone()).await {
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
                            "Tool {} execution failed: {}",
                            real_tool_name, error_msg
                        ));
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

                    // === CACHE: Handle write vs read operations ===
                    if is_write_action(arguments) {
                        // Invalidate all cached reads for this tool so subsequent
                        // queries reflect the updated state
                        self.tool_result_cache.write().await.invalidate(name);
                    } else {
                        // Cache read results to avoid redundant calls
                        self.tool_result_cache
                            .write()
                            .await
                            .put(name, args_key, sanitized.clone());
                    }

                    return Ok(sanitized);
                }
                Err(e) => {
                    last_error = e.to_string();
                    last_attempt = attempt;
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

                    // Non-transient errors: fail immediately, no retry
                    break;
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
            "Tool {} failed (session: {}, attempts: {}, elapsed: {}ms): {}",
            real_tool_name,
            self.session_id,
            last_attempt + 1,
            elapsed.as_millis(),
            last_error
        )))
    }

    /// Process a user message with streaming response (returns AgentEvent stream).
    pub async fn process_stream_events(
        &self,
        user_message: &str,
        conversation_summary: Option<String>,
        summary_up_to_index: Option<u64>,
    ) -> Result<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> {
        self.process_stream_events_with_safeguards(
            user_message,
            conversation_summary,
            summary_up_to_index,
            StreamSafeguards::default(),
        )
        .await
    }

    /// Process a user message with streaming response and custom safeguards (e.g., interrupt signal).
    pub async fn process_stream_events_with_safeguards(
        &self,
        user_message: &str,
        conversation_summary: Option<String>,
        summary_up_to_index: Option<u64>,
        safeguards: StreamSafeguards,
    ) -> Result<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> {
        // Add user message to history
        let user_msg = AgentMessage::user(user_message);
        self.internal_state.write().await.push_message(user_msg);

        // Set session ID on memory tool to avoid cross-session contamination
        self.tools
            .set_memory_session_id(self.session_id.clone())
            .await;

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

        match process_stream_events_with_safeguards(
            self.llm_interface.clone(),
            self.internal_state.clone(),
            self.tools.clone(),
            user_message,
            safeguards,
            conversation_summary,
            summary_up_to_index,
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

    /// Process a user message with streaming response (returns String stream).
    pub async fn process_stream(
        &self,
        user_message: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = String> + Send>>> {
        let event_stream = self.process_stream_events(user_message, None, None).await?;
        Ok(events_to_string_stream(event_stream))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolkit::{Result, Tool, ToolOutput};
    use serde_json::json;

    /// Simple mock shell tool for testing (replaces individual mock tools)
    struct MockShellTool;
    #[async_trait::async_trait]
    impl Tool for MockShellTool {
        fn name(&self) -> &str {
            "shell"
        }
        fn description(&self) -> &str {
            "Execute CLI commands (mock for testing)"
        }
        fn parameters(&self) -> serde_json::Value {
            json!({"type": "object", "properties": {"command": {"type": "string"}}})
        }
        async fn execute(&self, _args: serde_json::Value) -> Result<ToolOutput> {
            let data =
                json!({"devices": [{"id": "mock_device_1", "name": "模拟设备"}], "count": 1});
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

    /// Create a test agent with mock tools registered
    fn create_test_agent_with_mocks(session_id: String) -> Agent {
        use crate::toolkit::ToolRegistryBuilder;

        let mut registry = ToolRegistryBuilder::new().build();

        // Register mock tools
        registry.register(std::sync::Arc::new(MockShellTool));
        registry.register(std::sync::Arc::new(MockListRulesTool));

        // Add default agent tools
        use crate::tools::{AskUserTool, ClarifyIntentTool, ConfirmActionTool};

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
    async fn allowed_tools_filters_definitions_but_keeps_interaction_tools() {
        // A per-session allowlist trims domain tools from BOTH the
        // function-calling schema and the text quick-reference prompt, while
        // the user-interaction tools always survive (they are UX, not domain
        // capability).
        use crate::toolkit::ToolRegistryBuilder;
        let mut registry = ToolRegistryBuilder::new().build();
        registry.register(std::sync::Arc::new(MockShellTool));
        registry.register(std::sync::Arc::new(MockListRulesTool));
        use crate::tools::{AskUserTool, ClarifyIntentTool, ConfirmActionTool};
        registry.register(std::sync::Arc::new(AskUserTool::new()));
        registry.register(std::sync::Arc::new(ConfirmActionTool::new()));
        registry.register(std::sync::Arc::new(ClarifyIntentTool::new()));

        let mut config = AgentConfig::default();
        config.allowed_tools = vec!["shell".to_string()];
        let agent = Agent::with_tools(
            config,
            "allowlist-test".to_string(),
            std::sync::Arc::new(registry),
        );
        agent.update_tool_definitions().await;

        let defs = agent.llm_interface().get_tool_definitions().await;
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"shell"), "allowlisted tool must be kept");
        for ux in ["ask_user", "confirm_action", "clarify_intent"] {
            assert!(
                names.contains(&ux),
                "interaction tool {ux} must survive allowlisting"
            );
        }
        assert!(
            !names.contains(&"list_rules"),
            "out-of-profile tool must be filtered"
        );

        // Text quick-reference prompt stays in sync with the schema
        let prompt = agent.generate_dynamic_system_prompt().await;
        assert!(prompt.contains("**shell**"));
        assert!(!prompt.contains("**list_rules**"));
    }

    #[tokio::test]
    async fn dynamic_prompt_contains_capability_index() {
        let agent = Agent::with_session("test_capability".to_string());
        let prompt = agent.generate_dynamic_system_prompt().await;
        assert!(
            prompt.contains("## System Capability Index"),
            "capability index missing from dynamic prompt"
        );
        assert!(
            prompt.contains("### CLI Commands"),
            "cli tree missing from dynamic prompt"
        );
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
    async fn test_process_fallback() {
        let agent = create_test_agent_with_mocks("test_session".to_string());
        let response = agent.process("列出所有设备").await.unwrap();

        assert!(response.message.content.contains("设备"));
        assert!(response.tools_used.contains(&"shell".to_string()));
    }

    #[tokio::test]
    async fn test_process_list_rules() {
        let agent = create_test_agent_with_mocks("test_session".to_string());
        let response = agent.process("列出规则").await.unwrap();

        assert!(response.message.content.contains("规则"));
        assert!(response.tools_used.contains(&"shell".to_string()));
    }

    #[tokio::test]
    async fn test_process_query_data() {
        let agent = create_test_agent_with_mocks("test_session".to_string());
        let response = agent.process("查询温度数据").await.unwrap();

        assert!(response.message.content.contains("数据"));
        assert!(response.tools_used.contains(&"shell".to_string()));
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
}
