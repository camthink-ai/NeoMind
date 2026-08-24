//! Agent types - Events, Messages, Responses, and Configuration.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use neomind_core::{
    config::{agent_env_vars, endpoints, models},
    Message,
};

/// Agent event emitted during streaming processing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentEvent {
    /// Thinking/reasoning content chunk
    Thinking {
        /// Content chunk
        content: String,
    },
    /// Actual response content chunk
    Content {
        /// Content chunk
        content: String,
    },
    /// Tool call is starting
    ToolCallStart {
        /// Tool name
        tool: String,
        /// Tool arguments
        arguments: Value,
        /// Round number (1-based) for multi-round tool calling
        #[serde(skip_serializing_if = "Option::is_none")]
        round: Option<usize>,
    },
    /// Tool call completed with result
    ToolCallEnd {
        /// Tool name
        tool: String,
        /// Result (success or error)
        result: String,
        /// Whether it succeeded
        success: bool,
        /// Round number (1-based) for multi-round tool calling
        #[serde(skip_serializing_if = "Option::is_none")]
        round: Option<usize>,
    },
    /// Error occurred
    Error {
        /// Error message
        message: String,
    },
    /// Warning message (non-fatal)
    Warning {
        /// Warning message
        message: String,
    },
    /// Stream ended
    End {
        /// Prompt tokens used in this request (from LLM backend)
        #[serde(skip_serializing_if = "Option::is_none")]
        prompt_tokens: Option<u32>,
    },
    /// Intermediate end (for multi-round tool calling)
    /// Indicates the current round is complete but more processing is coming
    IntermediateEnd,
    /// Intent classification result
    Intent {
        /// Intent category (e.g., "Device", "Rule", "Data")
        category: String,
        /// Display name (e.g., "设备管理", "自动化规则")
        display_name: String,
        /// Confidence score
        #[serde(skip_serializing_if = "Option::is_none")]
        confidence: Option<f32>,
        /// Keywords that matched
        #[serde(skip_serializing_if = "Option::is_none")]
        keywords: Option<Vec<String>>,
    },
    /// Execution plan step
    Plan {
        /// Step description
        step: String,
        /// Stage name
        stage: String,
    },
    /// Heartbeat to keep connection alive
    Heartbeat {
        /// Timestamp when heartbeat was sent
        timestamp: i64,
    },
    /// Progress update
    Progress {
        /// Progress message
        message: String,
        /// Stage name
        #[serde(skip_serializing_if = "Option::is_none")]
        stage: Option<String>,
        /// Elapsed time in ms
        #[serde(rename = "elapsedMs", skip_serializing_if = "Option::is_none")]
        elapsed_ms: Option<u64>,
    },
}

impl AgentEvent {
    /// Create a thinking chunk event.
    pub fn thinking(content: impl Into<String>) -> Self {
        Self::Thinking {
            content: content.into(),
        }
    }

    /// Create a content chunk event.
    pub fn content(content: impl AsRef<str>) -> Self {
        Self::Content {
            content: content.as_ref().to_string(),
        }
    }

    /// Create a tool call start event.
    pub fn tool_call_start(tool: impl Into<String>, arguments: Value) -> Self {
        Self::ToolCallStart {
            tool: tool.into(),
            arguments,
            round: None,
        }
    }

    /// Create a tool call end event.
    pub fn tool_call_end(
        tool: impl Into<String>,
        result: impl Into<String>,
        success: bool,
    ) -> Self {
        Self::ToolCallEnd {
            tool: tool.into(),
            result: result.into(),
            success,
            round: None,
        }
    }

    /// Create a tool call start event with round number.
    pub fn tool_call_start_round(tool: impl Into<String>, arguments: Value, round: usize) -> Self {
        Self::ToolCallStart {
            tool: tool.into(),
            arguments,
            round: Some(round),
        }
    }

    /// Create a tool call end event with round number.
    pub fn tool_call_end_round(
        tool: impl Into<String>,
        result: impl Into<String>,
        success: bool,
        round: usize,
    ) -> Self {
        Self::ToolCallEnd {
            tool: tool.into(),
            result: result.into(),
            success,
            round: Some(round),
        }
    }

    /// Create an error event.
    pub fn error(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
        }
    }

    /// Create a warning event.
    pub fn warning(message: impl Into<String>) -> Self {
        Self::Warning {
            message: message.into(),
        }
    }

    /// Create an end event.
    pub fn end() -> Self {
        Self::End {
            prompt_tokens: None,
        }
    }

    /// Create an end event with token usage data.
    pub fn end_with_tokens(prompt_tokens: u32) -> Self {
        Self::End {
            prompt_tokens: Some(prompt_tokens),
        }
    }

    /// Create an intent event.
    pub fn intent(
        category: impl Into<String>,
        display_name: impl Into<String>,
        confidence: impl Into<Option<f32>>,
        keywords: impl Into<Option<Vec<String>>>,
    ) -> Self {
        Self::Intent {
            category: category.into(),
            display_name: display_name.into(),
            confidence: confidence.into(),
            keywords: keywords.into(),
        }
    }

    /// Create a plan event.
    pub fn plan(step: impl Into<String>, stage: impl Into<String>) -> Self {
        Self::Plan {
            step: step.into(),
            stage: stage.into(),
        }
    }

    /// Create a heartbeat event.
    pub fn heartbeat() -> Self {
        Self::Heartbeat {
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Create a progress event.
    pub fn progress(message: impl Into<String>, stage: impl Into<String>, elapsed: u64) -> Self {
        Self::Progress {
            message: message.into(),
            stage: Some(stage.into()),
            elapsed_ms: Some(elapsed),
        }
    }

    /// Check if this event ends the stream.
    pub fn is_end(&self) -> bool {
        matches!(self, Self::End { .. })
    }

    /// Check if this event is any kind of end (End or IntermediateEnd).
    pub fn is_any_end(&self) -> bool {
        matches!(self, Self::End { .. } | Self::IntermediateEnd)
    }

    /// Convert to JSON for WebSocket transmission.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// Agent configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Agent name
    pub name: String,
    /// System prompt
    pub system_prompt: String,
    /// Session-scoped suffix appended to the STREAMING chat system prompt
    /// (page-scoped focus). Kept separate from `system_prompt` so the
    /// streaming builder can inject it without dragging in the legacy full
    /// prompt that `system_prompt` carries for the non-streaming paths.
    pub system_prompt_suffix: Option<String>,
    /// Maximum tokens in context
    pub max_context_tokens: usize,
    /// Temperature for LLM
    pub temperature: f32,
    /// Enable tool calling
    pub enable_tools: bool,
    /// Enable memory
    pub enable_memory: bool,
    /// Model to use
    pub model: String,
    /// API endpoint for cloud LLM
    pub api_endpoint: Option<String>,
    /// API key for cloud LLM
    pub api_key: Option<String>,
    /// Maximum tool calls per request (default: 5)
    #[serde(default = "default_max_tool_calls")]
    pub max_tool_calls: usize,
    /// Number of recent tool results to keep intact (default: 2)
    #[serde(default = "default_keep_tool_results")]
    pub keep_recent_tool_results: usize,
    /// Per-session tool allowlist (empty = all available tools). Applied to
    /// both the function-calling schema and the text quick-reference prompt.
    /// The user-interaction tools (ask_user / confirm_action / clarify_intent)
    /// are always kept — they are UX, not domain capability.
    #[serde(default)]
    pub allowed_tools: Vec<String>,
}

/// Default value for max tool calls per request.
fn default_max_tool_calls() -> usize {
    100
}

/// Default value for keep recent tool results.
fn default_keep_tool_results() -> usize {
    2
}

/// Generate the default system prompt with language adaptation support.
/// The prompt instructs the LLM to respond in the same language as the user.
fn default_system_prompt() -> String {
    use crate::prompts::PromptBuilder;
    PromptBuilder::new()
        .with_thinking(true)
        .build_system_prompt()
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            name: "NeoMind Agent".to_string(),
            system_prompt: default_system_prompt(),
            system_prompt_suffix: None,
            // Load from environment variables with fallback to defaults
            max_context_tokens: agent_env_vars::max_context_tokens(),
            temperature: agent_env_vars::temperature(),
            enable_tools: true,
            enable_memory: true,
            model: models::OLLAMA_DEFAULT.to_string(),
            api_endpoint: std::env::var("OLLAMA_ENDPOINT")
                .ok()
                .or_else(|| std::env::var("OPENAI_ENDPOINT").ok())
                .or_else(|| Some(endpoints::OLLAMA.to_string())),
            api_key: std::env::var("OPENAI_API_KEY").ok(),
            max_tool_calls: default_max_tool_calls(),
            keep_recent_tool_results: default_keep_tool_results(),
            allowed_tools: Vec::new(),
        }
    }
}

/// User message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    /// Role (user, assistant, system, tool)
    pub role: String,
    /// Content — Arc<str> for cheap cloning across context windows
    pub content: std::sync::Arc<str>,
    /// Tool calls (if any)
    pub tool_calls: Option<Vec<ToolCall>>,
    /// Tool call ID (for tool responses)
    pub tool_call_id: Option<String>,
    /// Tool call name (for tracking which tool was called)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_name: Option<String>,
    /// Thinking content (for AI reasoning process)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    /// Images attached to the message (base64 data URLs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<AgentMessageImage>>,
    /// Per-round intermediate text for multi-round tool calling
    #[serde(skip_serializing_if = "Option::is_none")]
    pub round_contents: Option<Value>,
    /// Per-round thinking content for grouped rendering
    #[serde(skip_serializing_if = "Option::is_none")]
    pub round_thinking: Option<Value>,
    /// Timestamp
    pub timestamp: i64,
}

/// An image attached to a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessageImage {
    /// Base64 data URL (e.g., "data:image/png;base64,...")
    pub data: String,
    /// MIME type (e.g., "image/png")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

impl AgentMessage {
    /// Create a user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into().into(),
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

    /// Create a user message with images.
    pub fn user_with_images(content: impl Into<String>, images: Vec<AgentMessageImage>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into().into(),
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

    /// Create an assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into().into(),
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

    /// Create an assistant message with thinking.
    pub fn assistant_with_thinking(
        content: impl Into<String>,
        thinking: impl Into<String>,
    ) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into().into(),
            tool_calls: None,
            tool_call_id: None,
            tool_call_name: None,
            thinking: Some(thinking.into()),
            images: None,
            round_contents: None,
            round_thinking: None,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Create a system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into().into(),
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

    /// Create a tool result message.
    pub fn tool_result(tool_name: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: content.into().into(),
            tool_calls: None,
            tool_call_id: None,
            tool_call_name: Some(tool_name.into()),
            thinking: None,
            images: None,
            round_contents: None,
            round_thinking: None,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Create an assistant message with tool calls.
    pub fn assistant_with_tools(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into().into(),
            tool_calls: Some(tool_calls),
            tool_call_id: None,
            tool_call_name: None,
            thinking: None,
            images: None,
            round_contents: None,
            round_thinking: None,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Create an assistant message with tool calls and thinking.
    pub fn assistant_with_tools_and_thinking(
        content: impl Into<String>,
        tool_calls: Vec<ToolCall>,
        thinking: impl Into<String>,
    ) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into().into(),
            tool_calls: Some(tool_calls),
            tool_call_id: None,
            tool_call_name: None,
            thinking: Some(thinking.into()),
            images: None,
            round_contents: None,
            round_thinking: None,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Convert to core Message.
    /// IMPORTANT: When tool_calls exist, include them in content for LLM context.
    /// This ensures the model knows what tools were called in previous turns.
    /// IMPORTANT: Images are preserved for multimodal context in follow-up requests.
    pub fn to_core(&self) -> Message {
        match self.role.as_str() {
            "user" => {
                // Check if this message has images - create multimodal message
                if let Some(ref images) = self.images {
                    if !images.is_empty() {
                        return self.to_core_multimodal();
                    }
                }
                Message::user(self.content.as_ref())
            }
            "assistant" => {
                let mut content = self.content.to_string();
                // When tool_calls exist and content is empty, inject a concise summary
                // so the LLM knows what tools were called in previous turns.
                if let Some(ref tool_calls) = self.tool_calls {
                    if !tool_calls.is_empty() && content.is_empty() {
                        let summaries: Vec<String> = tool_calls
                            .iter()
                            .map(|tc| {
                                let args_summary = summarize_tool_args(&tc.name, &tc.arguments);
                                let result_preview = tc
                                    .result
                                    .as_ref()
                                    .map(|r| {
                                        if let Some(s) = r.as_str() {
                                            s.chars().take(120).collect::<String>()
                                        } else {
                                            let s = r.to_string();
                                            s.chars().take(120).collect::<String>()
                                        }
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
                        content = format!("In a previous turn I called {}. These results are from earlier and should not be repeated.", summaries.join(", then "));
                    }
                }
                Message::assistant(&content)
            }
            "system" => Message::system(self.content.as_ref()),
            "tool" => {
                // Tool result messages - include which tool was called
                if let Some(ref tool_name) = self.tool_call_name {
                    let tool_content = format!("[Tool: {} returned]\n{}", tool_name, self.content);
                    Message::user(&tool_content)
                } else {
                    Message::user(self.content.as_ref())
                }
            }
            _ => Message::user(self.content.as_ref()),
        }
    }

    /// Convert to core Message with multimodal content (text + images).
    /// This preserves image context for follow-up LLM requests.
    fn to_core_multimodal(&self) -> Message {
        use neomind_core::{Content, ContentPart, MessageRole};

        let images = match &self.images {
            Some(imgs) if !imgs.is_empty() => imgs,
            _ => return Message::user(self.content.as_ref()),
        };

        // Build content parts: text + images
        let mut parts = vec![ContentPart::text(self.content.to_string())];

        for image in images {
            // Prefer the mime type stored with the message (set at upload time).
            // If missing, parse from the data URL header or infer from magic bytes.
            let parsed = crate::image_utils::parse_image_data(&image.data);
            let mime_type = image
                .mime_type
                .clone()
                .or_else(|| parsed.map(|p| p.mime_type.to_string()))
                .unwrap_or_else(|| "image/png".to_string());
            let base64_data = parsed.map(|p| p.base64).unwrap_or(image.data.as_str());

            parts.push(ContentPart::image_base64(base64_data, mime_type));
        }

        Message::new(MessageRole::User, Content::Parts(parts))
    }

    /// Convert from core Message.
    pub fn from_core(msg: &Message) -> Self {
        Self {
            role: msg.role.to_string(),
            content: msg.content.as_text().into(),
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
}

/// Tool call from LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Tool name
    pub name: String,
    /// Call ID
    pub id: String,
    /// Arguments
    pub arguments: Value,
    /// Execution result (populated after tool execution)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Round number for multi-round tool calling (1-based)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub round: Option<usize>,
}

/// Agent response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    /// Response message
    pub message: AgentMessage,
    /// Tool calls made (if any)
    pub tool_calls: Vec<ToolCall>,
    /// Memory used
    pub memory_context_used: bool,
    /// Tools used
    pub tools_used: Vec<String>,
    /// Processing time in milliseconds
    pub processing_time_ms: u64,
}

/// Session state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    /// Session ID
    pub id: String,
    /// Started at
    pub started_at: i64,
    /// Last activity
    pub last_activity: i64,
    /// Message count
    pub message_count: usize,
    /// Metadata
    pub metadata: Value,
}

impl SessionState {
    /// Create a new session state.
    pub fn new(id: String) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            id,
            started_at: now,
            last_activity: now,
            message_count: 0,
            metadata: Value::Object(serde_json::Map::new()),
        }
    }

    /// Update activity.
    pub fn touch(&mut self) {
        self.last_activity = chrono::Utc::now().timestamp();
    }

    /// Increment message count.
    pub fn increment_messages(&mut self) {
        self.message_count += 1;
        self.touch();
    }
}

/// Find the array of items from a tool response JSON.
/// Tries: `val[list_key]` → `val["data"][list_key]` → `val["data"]` (if array) → `val` (if array)
fn find_json_list<'a>(
    val: &'a serde_json::Value,
    list_key: &str,
) -> Option<&'a Vec<serde_json::Value>> {
    val.get(list_key)
        .and_then(|v| v.as_array())
        .or_else(|| {
            val.get("data")
                .and_then(|d| d.get(list_key))
                .and_then(|v| v.as_array())
        })
        .or_else(|| val.get("data").and_then(|v| v.as_array()))
        .or_else(|| val.as_array())
}

/// Summarize tool call arguments into a concise string.
/// E.g., `{"action":"list"}` → `"list"`,
///       `{"action":"history","device_id":"b626d0","metric":"battery"}` → `"history b626d0/battery"`
pub fn summarize_tool_args(tool_name: &str, args: &Value) -> String {
    let obj = match args.as_object() {
        Some(o) => o,
        None => return args.to_string().chars().take(80).collect(),
    };

    // Extract the action field if present
    let action = obj.get("action").and_then(|v| v.as_str()).unwrap_or("");

    match tool_name {
        "device" => match action {
            "list" => "list".to_string(),
            "get" | "query" | "history" => {
                let device_id = obj.get("device_id").and_then(|v| v.as_str()).unwrap_or("?");
                let metric = obj.get("metric").and_then(|v| v.as_str()).unwrap_or("");
                if metric.is_empty() {
                    format!("{} {}", action, device_id)
                } else {
                    format!("{} {}/{}", action, device_id, metric)
                }
            }
            _ => action.to_string(),
        },
        "agent" => match action {
            "list" => "list".to_string(),
            _ => action.to_string(),
        },
        _ => {
            // Generic: show action + up to 2 key params
            let mut parts = vec![];
            if !action.is_empty() {
                parts.push(action.to_string());
            }
            for (k, v) in obj.iter() {
                if k == "action" {
                    continue;
                }
                if parts.len() >= 3 {
                    break;
                }
                if let Some(s) = v.as_str() {
                    parts.push(format!("{}={}", k, s.chars().take(20).collect::<String>()));
                }
            }
            if parts.is_empty() {
                "?".to_string()
            } else {
                parts.join(",")
            }
        }
    }
}

/// Cached large tool result data (images, big base64 blobs, etc.).
/// This data is stored outside LLM message history to prevent token bloat.
#[derive(Debug, Clone)]
pub struct CachedLargeResult {
    /// MIME type or content description (e.g., "image/jpeg", "application/json")
    pub content_type: String,
    /// Full data (base64 for images, raw string for JSON/text)
    pub data: String,
    /// Size of the data in bytes
    pub size_bytes: usize,
    /// Timestamp when cached
    pub cached_at: i64,
}

/// Cache for large tool results that must not enter LLM context.
/// Stores full data keyed by tool_name. Data is retrieved only when
/// needed — for tool calls that accept base64 or multimodal LLM calls.
#[derive(Debug, Clone, Default)]
pub struct LargeDataCache {
    entries: std::collections::HashMap<String, CachedLargeResult>,
}

/// Threshold below which we don't cache — just pass through.
/// Set to 32KB: compact time-series format (2000 points ≈ 14KB) should reach LLM intact.
/// Only truly large payloads (images, huge JSON) get cached with a summary reference.
const CACHE_THRESHOLD_BYTES: usize = 32 * 1024;

/// Threshold used by `slim_large_strings_in_json` to decide when a
/// non-image string is large enough to cache-and-replace. Kept higher
/// than `CACHE_THRESHOLD_BYTES` (which gates `store()`) so that (a)
/// anything slim decides to cache is guaranteed to actually be stored,
/// and (b) legitimate large text payloads (configs, compact logs,
/// multi-row query results) still reach the LLM verbatim instead of
/// being hidden behind a `$cached:` reference the LLM can't "read" as
/// text.
const SLIM_THRESHOLD_BYTES: usize = 64 * 1024;

/// Maximum number of cache entries. Oldest entries are evicted when exceeded.
const MAX_CACHE_ENTRIES: usize = 20;

/// Maximum total cache size in bytes (50MB). Oldest entries are evicted when exceeded.
const MAX_CACHE_TOTAL_BYTES: usize = 50 * 1024 * 1024;

impl LargeDataCache {
    /// Create a new empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Evict oldest entries if cache exceeds limits.
    fn evict_if_needed(&mut self) {
        // Evict by count
        while self.entries.len() > MAX_CACHE_ENTRIES {
            if let Some(oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, v)| v.cached_at)
                .map(|(k, _)| k.clone())
            {
                tracing::debug!("Evicting cache entry '{}' (count limit)", oldest_key);
                self.entries.remove(&oldest_key);
            } else {
                break;
            }
        }

        // Evict by total size
        let total_size: usize = self.entries.values().map(|e| e.size_bytes).sum();
        if total_size > MAX_CACHE_TOTAL_BYTES {
            // Sort by age (oldest first) and evict until under budget
            let mut sorted: Vec<(String, i64)> = self
                .entries
                .iter()
                .map(|(k, v)| (k.clone(), v.cached_at))
                .collect();
            sorted.sort_by_key(|(_, ts)| *ts);

            let mut current_size = total_size;
            for (key, _) in sorted {
                if current_size <= MAX_CACHE_TOTAL_BYTES {
                    break;
                }
                if let Some(entry) = self.entries.remove(&key) {
                    tracing::debug!(
                        "Evicting cache entry '{}' (size limit, freed {} bytes)",
                        key,
                        entry.size_bytes
                    );
                    current_size -= entry.size_bytes;
                }
            }
        }
    }

    /// Store a tool result. Returns a summary string to use in message history.
    /// - Small results (<32KB): not cached, returned as-is
    /// - Large/image/base64 results: cached, returns summary with reference key
    pub fn store(&mut self, tool_name: &str, result: &str) -> String {
        let size_bytes = result.len();

        // Small results: pass through unchanged
        if size_bytes < CACHE_THRESHOLD_BYTES {
            return result.to_string();
        }

        // Detect content type
        let content_type = Self::detect_content_type(result);

        let cached = CachedLargeResult {
            content_type: content_type.clone(),
            data: result.to_string(),
            size_bytes,
            cached_at: chrono::Utc::now().timestamp(),
        };
        self.entries.insert(tool_name.to_string(), cached);

        // Evict oldest entries if cache exceeds size/count limits
        self.evict_if_needed();

        // Return a concise summary for message history — NO raw base64 preview
        let human_size = Self::humanize_bytes(size_bytes);
        let cached_ref = format!("$cached:{}", tool_name);
        if content_type.starts_with("image/") || content_type == "application/json+base64" {
            // Show JSON structure without base64 content
            let structure_preview = Self::describe_structure(result);
            format!(
                "[Image data, {}. Use \"{}\" to reference this data in subsequent tool calls. Structure: {}]",
                human_size, cached_ref, structure_preview
            )
        } else if content_type == "application/json" {
            // JSON data: extract key summary values for LLM context
            let summary = Self::smart_json_summary(tool_name, result, 500);
            format!(
                "[Data: {}, {}. Use \"{}\" to reference. {}]",
                content_type, human_size, cached_ref, summary
            )
        } else {
            let preview: String = result.chars().take(300).collect();
            format!(
                "[Data: {}, {}. Use \"{}\" to reference. Preview: {}]",
                content_type, human_size, cached_ref, preview
            )
        }
    }

    /// Get the most recently cached image-like data entry.
    /// Used for auto-injection when the LLM fails to pass valid image arguments.
    /// Returns the extracted image data (base64) and the cache key.
    pub fn get_latest_image(&self) -> Option<(String, String)> {
        let data_dir = Self::image_data_dir();
        let data_dir = std::path::Path::new(&data_dir);
        // Priority 1: user-uploaded images
        if let Some(user_img) = self.entries.get("user_image") {
            return Some((
                Self::extract_image_data(&user_img.data, data_dir),
                "user_image".to_string(),
            ));
        }
        // Priority 2: most recent image-type cached entry by timestamp
        let mut best: Option<(&String, &CachedLargeResult)> = None;
        for (key, entry) in &self.entries {
            let is_image = entry.content_type.starts_with("image/")
                || entry.content_type == "application/json+base64";
            if is_image {
                let is_better = match best {
                    None => true,
                    Some((_, prev)) => entry.cached_at > prev.cached_at,
                };
                if is_better {
                    best = Some((key, entry));
                }
            }
        }
        best.map(|(key, entry)| (Self::extract_image_data(&entry.data, data_dir), key.clone()))
    }

    /// Seed the cache with the agent's bound device image so the existing
    /// `$cached:` reference resolution + auto-inject machinery
    /// (`resolve_cached_arguments`) can hand the FULL image to image-aware
    /// tool calls — including extension tools (e.g. YOLO / grounding) whose
    /// `image`/`image_base64` arg the LLM cannot fill with real bytes.
    ///
    /// Why this exists: scheduled/event agents inject their bound image
    /// directly into the multimodal user message (`build_tool_messages`)
    /// rather than via `store()`. Without seeding, `get_latest_image()`
    /// returns `None`, the auto-inject never fires, and tools receive the
    /// LLM's truncated base64 fragment (a recognizable JPEG header + junk
    /// body) → the extension decodes garbage and returns `null`.
    ///
    /// `data_url` must be a `data:image/<mime>;base64,<...>` string. Unlike
    /// `store()`, this bypasses the 32KB size threshold — even a small bound
    /// image must be injectable. Keyed as `agent_image` so it is distinct from
    /// chat's `user_image` and surfaced by `get_latest_image()`'s image scan.
    pub fn seed_bound_image(&mut self, data_url: &str) {
        if !data_url.starts_with("data:image/") {
            tracing::warn!(
                len = data_url.len(),
                "seed_bound_image: ignoring non-image data URL"
            );
            return;
        }
        let cached = CachedLargeResult {
            content_type: Self::detect_content_type(data_url),
            data: data_url.to_string(),
            size_bytes: data_url.len(),
            cached_at: chrono::Utc::now().timestamp(),
        };
        self.entries.insert("agent_image".to_string(), cached);
        self.evict_if_needed();
    }

    /// Store a user-uploaded chat image UNCONDITIONALLY — no size threshold.
    ///
    /// `store()` passes anything under `CACHE_THRESHOLD_BYTES` (32KB) through
    /// without caching, so a compressed user image (frontend uploads are
    /// routinely ≤24KB binary ≈ 32KB base64) never entered the cache:
    /// `$cached:user_image` then failed to resolve AND `get_latest_image()`
    /// returned None, so image-shaped tool args received the LLM's literal
    /// placeholder string and the tool errored ("Invalid base64 data").
    /// Found via 2026-08-18 vision-pipeline debugging: the model SAW the
    /// image in the prompt, but every image tool call failed — mirroring the
    /// agent-path lesson already encoded in `seed_bound_image` ("must work
    /// even for a small bound image").
    pub fn store_user_image(&mut self, key: &str, data_url: &str) {
        if !data_url.starts_with("data:image/") {
            tracing::warn!(
                len = data_url.len(),
                key = %key,
                "store_user_image: ignoring non-image data URL"
            );
            return;
        }
        let cached = CachedLargeResult {
            content_type: Self::detect_content_type(data_url),
            data: data_url.to_string(),
            size_bytes: data_url.len(),
            cached_at: chrono::Utc::now().timestamp(),
        };
        self.entries.insert(key.to_string(), cached);
        self.evict_if_needed();
    }

    /// Detect content type from content heuristics.
    fn detect_content_type(content: &str) -> String {
        // Check for data URL images (data:image/png;base64,...)
        if content.starts_with("data:image/") {
            // Extract MIME from data URL
            let end = content.find(';').unwrap_or(15);
            return content[..end].trim_start_matches("data:").to_string();
        }
        // Internal image URL form (/api/images/...) — image-bearing regardless
        // of size (the URL is tiny but points at a real stored image), so
        // get_latest_image() can surface it for vision auto-injection. Require
        // it to be the whole value (bare URL) or a JSON string value (preceded
        // by a quote) — a mere mention in prose (e.g. an error message) must
        // NOT classify as image, or it'd falsely trigger vision auto-inject.
        if content.starts_with("/api/images/") || content.contains("\"/api/images/") {
            return "image/url".to_string();
        }
        // Check for raw base64 image data (long string of base64 chars)
        if content.len() > 10_000 && Self::looks_like_base64(content) {
            return "image/base64".to_string();
        }
        // Check for JSON with embedded base64 image
        if content.starts_with('{') || content.starts_with('[') {
            if content.contains("base64,")
                || content.contains("\"data:image/")
                || content.contains("\"base64_data\"")
                || content.contains("\"data_type\":\"image\"")
            {
                return "application/json+base64".to_string();
            }
            return "application/json".to_string();
        }
        "text/plain".to_string()
    }

    /// Quick heuristic: does this look like base64 data?
    fn looks_like_base64(s: &str) -> bool {
        // Sample first 200 chars (NOT bytes — byte slicing panics on
        // multi-byte UTF-8, see sanitize.rs fix for the same bug).
        let sample: String = s.chars().take(200).collect();
        sample.chars().all(|c| {
            c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=' || c == '\n' || c == '\r'
        })
    }

    /// Resolve a $cached reference to the actual data.
    /// Format: "$cached:tool_name" — extracts the relevant image/base64 data
    /// from the cached tool result. Returns the full cached data as fallback.
    pub fn resolve_reference(&self, reference: &str) -> Option<String> {
        let tool_name = reference.strip_prefix("$cached:")?;
        let cached = self.entries.get(tool_name)?;
        let data_dir = Self::image_data_dir();
        Some(Self::extract_image_data(
            &cached.data,
            std::path::Path::new(&data_dir),
        ))
    }

    /// Extract image/base64 data from a cached result string.
    /// Recursively walks the JSON tree to find base64_data regardless of nesting depth,
    /// so it works for any tool response structure (device get, query, extensions, etc.).
    fn extract_image_data(data: &str, data_dir: &std::path::Path) -> String {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
            if let Some(found) = Self::find_base64_in_value(&json, data_dir) {
                return found;
            }
        }
        // Fallback: return raw data (works for pure base64 or a bare /api/images/ URL,
        // which the downstream vision tool's resolve_image handles directly).
        data.to_string()
    }

    /// Resolve the data dir for internal image lookups.
    /// Mirrors the agent data collector / extension tool normalization.
    fn image_data_dir() -> String {
        std::env::var("NEOMIND_DATA_DIR").unwrap_or_else(|_| "data".to_string())
    }

    /// Resolve an internal `/api/images/` URL to raw base64 (no `data:` prefix),
    /// matching the form `find_base64_in_value` returns for its other image sources.
    ///
    /// Lets the `$cached:` reference + auto-inject path feed vision tools from
    /// device/tool results that carry the v0.9.6 URL form instead of base64.
    /// Returns `None` for non-URL input or unresolvable files (missing/too large/
    /// non-image) so the caller keeps searching or falls back.
    fn resolve_api_images_url(url: &str, data_dir: &std::path::Path) -> Option<String> {
        if !url.starts_with("/api/images/") {
            return None;
        }
        let data_url =
            neomind_devices::image_storage::resolve_internal_image_to_data_url(url, data_dir)?;
        // data:<mime>;base64,<b64> → raw <b64> (base64 alphabet has no comma).
        data_url.split(',').nth(1).map(|s| s.to_string())
    }

    /// Recursively search a JSON value for base64 image data.
    /// Priority: base64_data fields > /api/images/ URLs > data:image URLs > large string values.
    fn find_base64_in_value(
        value: &serde_json::Value,
        data_dir: &std::path::Path,
    ) -> Option<String> {
        match value {
            serde_json::Value::Object(map) => {
                // Direct base64_data field (any depth)
                if let Some(b64) = map.get("base64_data").and_then(|v| v.as_str()) {
                    return Some(b64.to_string());
                }
                // data:image URL (extract base64 portion)
                for key in &["data", "image", "content", "url"] {
                    if let Some(s) = map.get(*key).and_then(|v| v.as_str()) {
                        // Internal /api/images/ URL → resolve to raw base64.
                        if let Some(b64) = Self::resolve_api_images_url(s, data_dir) {
                            return Some(b64);
                        }
                        if let Some(b64) = s.strip_prefix("data:image/") {
                            if let Some(after_comma) = b64.split(',').nth(1) {
                                return Some(after_comma.to_string());
                            }
                        }
                        // Large string likely to be raw base64 image.
                        // Require substantial size (>10KB) and valid base64 alphabet to avoid
                        // false positives on non-image base64 data (certificates, tokens, etc.).
                        if s.len() > 10000
                            && s.len() % 4 <= 1 // base64 length is always 4n, 4n+2, 4n+3
                            && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
                        {
                            return Some(s.to_string());
                        }
                    }
                }
                // Recurse into child values
                for v in map.values() {
                    if let Some(found) = Self::find_base64_in_value(v, data_dir) {
                        return Some(found);
                    }
                }
                None
            }
            serde_json::Value::Array(arr) => {
                for v in arr {
                    if let Some(found) = Self::find_base64_in_value(v, data_dir) {
                        return Some(found);
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Describe JSON structure without exposing raw base64 content.
    /// Shows field names, types, and sizes instead of actual data.
    fn describe_structure(data: &str) -> String {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
            Self::describe_value(&json, 0)
        } else {
            format!("<{} bytes>", data.len())
        }
    }

    /// Generate a smart summary of JSON data preserving key identifiers.
    /// Falls back to structure description if data shape is unrecognized.
    fn smart_json_summary(tool_name: &str, data: &str, max_len: usize) -> String {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(data) {
            let summary = match tool_name {
                "device" => Self::extract_device_summary(&val),
                "agent" => Self::extract_agent_summary(&val),
                "rule" => Self::extract_rule_summary(&val),
                "message" | "alert" => Self::extract_message_summary(&val),
                "extension" => Self::extract_extension_summary(&val),
                _ => Self::generic_json_summary(&val),
            };
            // Truncate to max_len
            let chars: Vec<char> = summary.chars().collect();
            if chars.len() > max_len {
                format!("{}...", chars[..max_len].iter().collect::<String>())
            } else {
                summary
            }
        } else {
            // Not valid JSON, just truncate
            let s: String = data.chars().take(max_len).collect();
            format!("Preview: {}", s)
        }
    }

    /// Extract device names and IDs from device tool results.
    /// E.g., "6 devices: NE101-Shelf(b62730), NE101-Refrigerator(b65020), ..."
    fn extract_device_summary(val: &serde_json::Value) -> String {
        Self::extract_list_summary(
            val,
            "devices",
            &["name", "device_name"],
            &["id", "device_id"],
            "devices",
        )
    }

    /// Extract agent names and IDs from agent tool results.
    fn extract_agent_summary(val: &serde_json::Value) -> String {
        Self::extract_list_summary(
            val,
            "agents",
            &["name", "agent_name"],
            &["id", "agent_id"],
            "agents",
        )
    }

    /// Extract rule names and IDs from rule tool results.
    fn extract_rule_summary(val: &serde_json::Value) -> String {
        Self::extract_list_summary(val, "rules", &["name"], &["id", "rule_id"], "rules")
    }

    /// Generic list summary extractor.
    /// Looks for an array under `list_key` (or "data"), extracts name+id pairs.
    fn extract_list_summary(
        val: &serde_json::Value,
        list_key: &str,
        name_keys: &[&str],
        id_keys: &[&str],
        label: &str,
    ) -> String {
        let arr = find_json_list(val, list_key);
        if let Some(arr) = arr {
            let count = arr.len();
            let items: Vec<String> = arr
                .iter()
                .take(10)
                .map(|item: &serde_json::Value| {
                    let name = name_keys
                        .iter()
                        .find_map(|k| item.get(*k).and_then(|v| v.as_str()))
                        .unwrap_or("?");
                    let id = id_keys
                        .iter()
                        .find_map(|k| item.get(*k).and_then(|v| v.as_str()))
                        .unwrap_or("?");
                    format!("{}({})", name, id)
                })
                .collect();
            if items.is_empty() {
                return format!("{} {}", count, label);
            }
            let suffix = if count > 10 { ", ..." } else { "" };
            format!("{} {}: {}{}", count, label, items.join(", "), suffix)
        } else {
            Self::describe_value(val, 0)
        }
    }

    /// Extract message/alert titles and IDs from message tool results.
    fn extract_message_summary(val: &serde_json::Value) -> String {
        let arr = find_json_list(val, "messages");
        if let Some(arr) = arr {
            let count = arr.len();
            let items: Vec<String> = arr
                .iter()
                .take(10)
                .map(|item: &serde_json::Value| {
                    let title = item
                        .get("title")
                        .or_else(|| item.get("subject"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    let level = item
                        .get("level")
                        .and_then(|v| v.as_str())
                        .map(|l| format!("[{}]", l))
                        .unwrap_or_default();
                    format!("{}{}", title, level)
                })
                .collect();
            if items.is_empty() {
                return format!("{} messages", count);
            }
            let suffix = if count > 10 { ", ..." } else { "" };
            format!("{} messages: {}{}", count, items.join(", "), suffix)
        } else {
            Self::describe_value(val, 0)
        }
    }

    /// Extract extension names, IDs, and state from extension tool results.
    fn extract_extension_summary(val: &serde_json::Value) -> String {
        let arr = find_json_list(val, "extensions");
        if let Some(arr) = arr {
            let count = arr.len();
            let items: Vec<String> = arr
                .iter()
                .take(10)
                .map(|item: &serde_json::Value| {
                    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                    let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                    let state = item
                        .get("state")
                        .and_then(|v| v.as_str())
                        .map(|s| format!(":{}", s))
                        .unwrap_or_default();
                    format!("{}({}){}", name, id, state)
                })
                .collect();
            if items.is_empty() {
                return format!("{} extensions", count);
            }
            let suffix = if count > 10 { ", ..." } else { "" };
            format!("{} extensions: {}{}", count, items.join(", "), suffix)
        } else {
            Self::describe_value(val, 0)
        }
    }

    /// Generic JSON summary: show first N items' key fields.
    fn generic_json_summary(val: &serde_json::Value) -> String {
        match val {
            serde_json::Value::Array(arr) => {
                let count = arr.len();
                if count == 0 {
                    return "[]".to_string();
                }
                // Show first 3 items concisely
                let previews: Vec<String> = arr
                    .iter()
                    .take(3)
                    .map(|item| match item {
                        serde_json::Value::Object(map) => {
                            let fields: Vec<String> = map
                                .iter()
                                .take(3)
                                .map(|(k, v)| {
                                    let vs = match v {
                                        serde_json::Value::String(s) => {
                                            s.chars().take(20).collect::<String>()
                                        }
                                        other => {
                                            other.to_string().chars().take(20).collect::<String>()
                                        }
                                    };
                                    format!("{}={}", k, vs)
                                })
                                .collect();
                            format!("{{{}}}", fields.join(", "))
                        }
                        other => other.to_string().chars().take(60).collect(),
                    })
                    .collect();
                let suffix = if count > 3 {
                    format!(", ... ({} items)", count)
                } else {
                    "".to_string()
                };
                format!("[{}{}]", previews.join(", "), suffix)
            }
            serde_json::Value::Object(map) => {
                let fields: Vec<String> = map
                    .iter()
                    .take(5)
                    .map(|(k, v)| {
                        let vs = match v {
                            serde_json::Value::String(s) if s.len() > 40 => {
                                let end = s.floor_char_boundary(40);
                                format!("{}...", &s[..end])
                            }
                            serde_json::Value::Array(a) => format!("Array({})", a.len()),
                            serde_json::Value::Object(_) => "{...}".to_string(),
                            other => other.to_string(),
                        };
                        format!("{}: {}", k, vs)
                    })
                    .collect();
                format!("{{{}}}", fields.join(", "))
            }
            other => {
                let s = other.to_string();
                s.chars().take(200).collect()
            }
        }
    }

    /// Recursively describe a JSON value's structure.
    fn describe_value(value: &serde_json::Value, depth: usize) -> String {
        if depth > 3 {
            return "{...}".to_string();
        }
        match value {
            serde_json::Value::Object(map) => {
                let fields: Vec<String> = map
                    .iter()
                    .map(|(k, v)| {
                        let val_desc = match v {
                            serde_json::Value::String(s) if s.len() > 100 => {
                                format!("String({} bytes)", s.len())
                            }
                            serde_json::Value::Array(arr) => {
                                format!("Array({} items)", arr.len())
                            }
                            serde_json::Value::Object(_) => Self::describe_value(v, depth + 1),
                            other => {
                                let s = other.to_string();
                                if s.len() > 50 {
                                    let end = s.floor_char_boundary(50);
                                    format!("{}...", &s[..end])
                                } else {
                                    s
                                }
                            }
                        };
                        format!("\"{}\": {}", k, val_desc)
                    })
                    .collect();
                format!("{{{}}}", fields.join(", "))
            }
            serde_json::Value::Array(arr) => {
                if arr.is_empty() {
                    "[]".to_string()
                } else {
                    let first = Self::describe_value(&arr[0], depth + 1);
                    format!("[{}... ({} items)]", first, arr.len())
                }
            }
            serde_json::Value::String(s) if s.len() > 100 => {
                format!("String({} bytes)", s.len())
            }
            other => other.to_string(),
        }
    }

    /// Format bytes as human-readable string.
    fn humanize_bytes(bytes: usize) -> String {
        const KB: usize = 1024;
        const MB: usize = 1024 * KB;
        if bytes >= MB {
            format!("{:.1}MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.1}KB", bytes as f64 / KB as f64)
        } else {
            format!("{}B", bytes)
        }
    }

    /// Walk a JSON tree and replace large string values IN PLACE with a
    /// one-line natural-language summary that contains: data kind, MIME
    /// type (if image), size, `$cached:` reference, and a concrete action
    /// hint. The actual data is stored in cache under a path-derived key.
    ///
    /// Unlike [`store`](Self::store) (which caches the WHOLE tool result as
    /// one entry when it exceeds 32KB), this method preserves all small
    /// fields — only the large string values get replaced. This keeps
    /// metadata visible to the LLM while preventing base64 from polluting
    /// the prompt.
    ///
    /// Each large value gets its own cache key, so multiple images from the
    /// same or different tools coexist without overwriting each other (the
    /// 8-hex-char content hash disambiguates).
    ///
    /// Returns the number of values slimmed.
    pub fn slim_large_strings_in_json(
        &mut self,
        value: &mut serde_json::Value,
        tool_name: &str,
    ) -> usize {
        let mut count = 0;
        self.slim_value_recursive(value, tool_name, &mut count);
        if count > 0 {
            self.evict_if_needed();
        }
        count
    }

    fn slim_value_recursive(
        &mut self,
        value: &mut serde_json::Value,
        path: &str,
        count: &mut usize,
    ) {
        match value {
            serde_json::Value::String(s) => {
                if Self::should_slim_string(s) {
                    let key = Self::make_cache_key(path, s);
                    let (kind, mime) = Self::classify_string(s);
                    let bytes = s.len();
                    let size_str = Self::humanize_bytes(bytes);
                    let cached_ref = format!("$cached:{}", key);

                    // Insert cache entry directly (we already know the data).
                    let cached = CachedLargeResult {
                        content_type: mime.clone(),
                        data: s.clone(),
                        size_bytes: bytes,
                        cached_at: chrono::Utc::now().timestamp(),
                    };
                    self.entries.insert(key, cached);

                    // Build one complete sentence with: kind, mime, size,
                    // $cached ref, and a single concrete action pointing at
                    // the common `vision` tool. We intentionally avoid naming
                    // specific extensions — the LLM picks the right one based
                    // on what's installed and what the user asked for.
                    let summary = if mime.starts_with("image/") {
                        format!(
                            "{kind} data ({mime}, {size}) cached as {ref} — pass this reference to the `vision` tool's `image` argument to analyze the content.",
                            kind = kind,
                            mime = mime,
                            size = size_str,
                            ref = cached_ref
                        )
                    } else {
                        format!(
                            "{kind} ({size}) cached as {ref} — pass this reference to downstream tools that accept `$cached:` references to access the full data.",
                            kind = kind,
                            size = size_str,
                            ref = cached_ref
                        )
                    };

                    *value = serde_json::Value::String(summary);
                    *count += 1;
                } else if (s.contains("data:image/") || s.len() > SLIM_THRESHOLD_BYTES)
                    && (s.starts_with('{') || s.starts_with('['))
                {
                    // Stringified-JSON nesting (the shell tool's `stdout`
                    // field): the shell tool wraps a `neomind` CliResponse as
                    // a JSON *string* inside `result.stdout` (double-encoded).
                    // An image metric buried in that string is invisible to
                    // the direct check above — `stdout` is one opaque string
                    // that neither starts with `data:image/` nor exceeds the
                    // size ceiling — so without this descent the `$cached`
                    // flow never fires and `sanitize` later textually mangles
                    // the base64 into `[image data, N]`, destroying the bytes
                    // (the `device get --metric image_data` → vision failure).
                    // Parse the inner JSON, recurse so every slimmable leaf
                    // becomes a `$cached` ref + cache entry, then store the
                    // re-serialized string back. `stdout` stays a string, so
                    // the shell result contract and `result_format` are
                    // untouched.
                    if let Ok(mut inner) = serde_json::from_str::<serde_json::Value>(s.as_str()) {
                        let before = *count;
                        self.slim_value_recursive(&mut inner, path, count);
                        if *count > before {
                            if let Ok(reserialized) = serde_json::to_string(&inner) {
                                *value = serde_json::Value::String(reserialized);
                            }
                        }
                    }
                }
            }
            serde_json::Value::Object(map) => {
                for (k, v) in map.iter_mut() {
                    let child_path = if path.is_empty() {
                        k.clone()
                    } else {
                        format!("{}.{}", path, k)
                    };
                    self.slim_value_recursive(v, &child_path, count);
                }
            }
            serde_json::Value::Array(arr) => {
                for (i, v) in arr.iter_mut().enumerate() {
                    let child_path = format!("{}[{}]", path, i);
                    self.slim_value_recursive(v, &child_path, count);
                }
            }
            _ => {}
        }
    }

    /// Decide whether a string is large enough (or image-like enough) to
    /// warrant slimming. Image data URLs always trigger (keeping base64 out
    /// of the prompt is an invariant). Other strings must exceed the
    /// `SLIM_THRESHOLD_BYTES` ceiling — kept higher than `CACHE_THRESHOLD_BYTES`
    /// so legitimate large text (configs, compact logs) still reaches the LLM
    /// while truly bloated values get cached.
    fn should_slim_string(s: &str) -> bool {
        s.starts_with("data:image/") || s.len() > SLIM_THRESHOLD_BYTES
    }

    /// Classify a large string into (kind, mime) for the summary.
    fn classify_string(s: &str) -> (&'static str, String) {
        if let Some(rest) = s.strip_prefix("data:image/") {
            let subtype = rest.split(';').next().unwrap_or("jpeg");
            let canonical = crate::image_utils::normalize_mime_subtype(subtype)
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("image/{}", subtype));
            ("Image", canonical)
        } else if s.len() > 10_000 && Self::looks_like_base64(s) {
            ("Large base64 data", "application/octet-stream".to_string())
        } else {
            ("Large string data", "text/plain".to_string())
        }
    }

    /// Build a deterministic, readable cache key.
    /// Format: `<path>#<8-hex-content-hash>` — path makes it debuggable,
    /// hash makes it unique per distinct payload (so two different images
    /// at the same JSON path don't collide).
    fn make_cache_key(path: &str, data: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        let hash = format!("{:x}", hasher.finish());
        // Sanitize path — replace characters that would mangle `$cached:` parsing.
        // The resolver splits on `:` so any `:` in path must be replaced.
        let safe_path = path.replace(':', "_");
        format!("{}#{}", safe_path, &hash[..8])
    }
}

/// Unified internal state for the agent.
/// Combines session state with LLM readiness and other runtime state.
#[derive(Debug, Clone)]
pub struct AgentInternalState {
    /// Session ID
    pub session_id: String,
    /// Whether LLM is ready
    pub llm_ready: bool,
    /// Session state
    pub session: SessionState,
    /// Message history for this session
    pub memory: Vec<AgentMessage>,
    /// Recent assistant response hashes (for cross-turn repetition detection)
    pub recent_response_hashes: Vec<u64>,
    /// Cache for large tool results (images, base64) that must not enter LLM context
    pub large_data_cache: LargeDataCache,
    /// Session-level tool result cache for deduplication across turns.
    /// Shared via Arc<RwLock<>> so streaming code can access it concurrently.
    pub tool_result_cache: std::sync::Arc<tokio::sync::RwLock<super::streaming::ToolResultCache>>,
    /// Cached compaction: (message_count_at_cache_time, max_tokens, compacted_messages)
    /// Invalidated when messages change or max_tokens differs.
    pub compaction_cache: Option<(usize, usize, Vec<AgentMessage>)>,
}

impl AgentInternalState {
    /// Create a new internal state.
    pub fn new(session_id: String) -> Self {
        let session = SessionState::new(session_id.clone());
        Self {
            session_id,
            llm_ready: false,
            session,
            memory: Vec::new(),
            recent_response_hashes: Vec::new(),
            large_data_cache: LargeDataCache::new(),
            tool_result_cache: std::sync::Arc::new(tokio::sync::RwLock::new(
                super::streaming::ToolResultCache::new(std::time::Duration::from_secs(300)),
            )),
            compaction_cache: None,
        }
    }

    /// Calculate similarity hash for a response content.
    fn hash_response(content: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        // Normalize content for hashing:
        // - Remove extra whitespace
        // - Convert to lowercase for case-insensitive comparison
        // - Remove common filler phrases
        let normalized = content
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");

        let mut h = DefaultHasher::new();
        normalized.hash(&mut h);
        h.finish()
    }

    /// Register a new assistant response for repetition detection.
    pub fn register_response(&mut self, content: &str) {
        let hash = Self::hash_response(content);
        self.recent_response_hashes.push(hash);

        // Keep only the last 10 response hashes
        if self.recent_response_hashes.len() > 10 {
            self.recent_response_hashes.remove(0);
        }
    }

    /// Update LLM readiness.
    pub fn set_llm_ready(&mut self, ready: bool) {
        self.llm_ready = ready;
    }

    /// Touch the session (update activity).
    pub fn touch(&mut self) {
        self.session.touch();
    }

    /// Push a message to the memory.
    pub fn push_message(&mut self, message: AgentMessage) {
        self.memory.push(message);
        self.session.increment_messages();
    }

    /// Restore memory from a list of messages.
    pub fn restore_memory(&mut self, messages: Vec<AgentMessage>) {
        self.memory = messages;
    }

    /// Clear the memory.
    pub fn clear_memory(&mut self) {
        self.memory.clear();
    }
}

/// LLM backend type with configuration.
#[derive(Debug, Clone)]
pub enum LlmBackend {
    /// Ollama (local)
    Ollama {
        endpoint: String,
        model: String,
        capabilities: Option<neomind_core::BackendCapabilities>,
    },
    /// OpenAI-compatible API
    OpenAi {
        api_key: String,
        endpoint: String,
        model: String,
        capabilities: Option<neomind_core::BackendCapabilities>,
    },
    /// Anthropic API
    Anthropic {
        api_key: String,
        endpoint: String,
        model: String,
        capabilities: Option<neomind_core::BackendCapabilities>,
    },
    /// Google AI API
    Google {
        api_key: String,
        endpoint: String,
        model: String,
        capabilities: Option<neomind_core::BackendCapabilities>,
    },
    /// xAI (Grok) API
    XAi {
        api_key: String,
        endpoint: String,
        model: String,
        capabilities: Option<neomind_core::BackendCapabilities>,
    },
    /// Qwen (Alibaba DashScope)
    Qwen {
        api_key: String,
        endpoint: String,
        model: String,
        capabilities: Option<neomind_core::BackendCapabilities>,
    },
    /// DeepSeek API
    DeepSeek {
        api_key: String,
        endpoint: String,
        model: String,
        capabilities: Option<neomind_core::BackendCapabilities>,
    },
    /// Zhipu GLM API
    GLM {
        api_key: String,
        endpoint: String,
        model: String,
        capabilities: Option<neomind_core::BackendCapabilities>,
    },
    /// MiniMax API
    MiniMax {
        api_key: String,
        endpoint: String,
        model: String,
        capabilities: Option<neomind_core::BackendCapabilities>,
    },
    /// llama.cpp standalone server (local)
    LlamaCpp {
        endpoint: String,
        model: String,
        capabilities: Option<neomind_core::BackendCapabilities>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_event_creation() {
        let thinking = AgentEvent::thinking("test thinking");
        assert!(matches!(thinking, AgentEvent::Thinking { .. }));

        let content = AgentEvent::content("test content");
        assert!(matches!(content, AgentEvent::Content { .. }));

        let end = AgentEvent::end();
        assert!(end.is_end());
    }

    #[test]
    fn test_agent_message_creation() {
        let user_msg = AgentMessage::user("Hello");
        assert_eq!(user_msg.role, "user");
        assert_eq!(&*user_msg.content, "Hello");

        let assistant_msg = AgentMessage::assistant("Hi there!");
        assert_eq!(assistant_msg.role, "assistant");

        let sys_msg = AgentMessage::system("You are helpful");
        assert_eq!(sys_msg.role, "system");

        let tool_msg = AgentMessage::tool_result("list_devices", "Success");
        assert_eq!(tool_msg.role, "tool");
        assert_eq!(tool_msg.tool_call_name, Some("list_devices".to_string()));
    }

    #[test]
    fn test_session_state() {
        let mut state = SessionState::new("session_1".to_string());
        assert_eq!(state.message_count, 0);

        state.increment_messages();
        assert_eq!(state.message_count, 1);

        state.touch();
        assert!(state.last_activity > 0);
    }

    #[test]
    fn test_agent_config_default() {
        let config = AgentConfig::default();
        assert_eq!(config.name, "NeoMind Agent");
        assert_eq!(config.model, models::OLLAMA_DEFAULT);
        assert!(config.enable_tools);
        assert!(config.enable_memory);
    }

    // --- slim_large_strings_in_json coverage ---

    /// Build a fake JPEG data URL of the requested byte size. Used to push
    /// payloads past the 32KB slim threshold.
    fn fake_jpeg_data_url(bytes: usize) -> String {
        // "data:image/jpeg;base64," is 23 bytes; fill the rest with 'A'.
        let prefix = "data:image/jpeg;base64,";
        let pad = bytes.saturating_sub(prefix.len());
        format!("{}{}", prefix, "A".repeat(pad))
    }

    /// Single image value gets replaced with a one-line summary that
    /// mentions kind, mime, size, $cached ref, and the vision tool.
    #[test]
    fn test_store_user_image_bypasses_threshold() {
        // Regression (2026-08-18): a compressed user image below the 32KB
        // CACHE_THRESHOLD used to be silently dropped by store(), leaving
        // $cached:user_image unresolvable and image tool calls failing with
        // "Invalid base64 data" while the model HAD seen the image.
        let small_png = concat!(
            "data:image/png;base64,",
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJ",
            "AAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="
        );
        let mut cache = LargeDataCache::new();
        cache.store_user_image("user_image", small_png);
        // Resolvable as a $cached: reference (what the LLM emits)…
        let resolved = cache
            .resolve_reference("$cached:user_image")
            .expect("small user image must resolve");
        assert!(resolved.starts_with("data:image/png;base64,"));
        // …and surfaced to the auto-inject path.
        let (latest, source) = cache.get_latest_image().expect("latest image must exist");
        assert_eq!(source, "user_image");
        assert_eq!(latest, resolved);
    }

    #[test]
    fn test_slim_single_image_value_replaced() {
        let mut cache = LargeDataCache::new();
        let mut value = serde_json::json!({
            "device": {"id": "abc", "battery": 100},
            "image": fake_jpeg_data_url(40_000)
        });

        let n = cache.slim_large_strings_in_json(&mut value, "shell");
        assert_eq!(n, 1, "exactly one value should be slimmed");

        // Small sibling fields preserved untouched.
        assert_eq!(value["device"]["battery"], 100);
        assert_eq!(value["device"]["id"], "abc");

        // The image value is now a summary string.
        let summary = value["image"].as_str().expect("image replaced with string");
        assert!(
            summary.contains("Image"),
            "summary should mention kind: {}",
            summary
        );
        assert!(
            summary.contains("image/jpeg"),
            "summary should mention mime: {}",
            summary
        );
        assert!(
            summary.contains("$cached:"),
            "summary should include ref: {}",
            summary
        );
        assert!(
            summary.contains("vision"),
            "summary should hint at vision tool: {}",
            summary
        );

        // No raw base64 left in the JSON.
        let serialized = serde_json::to_string(&value).unwrap();
        assert!(
            !serialized.contains("data:image/jpeg;base64,AAAA"),
            "no raw base64 should remain in the slimmed JSON"
        );
    }

    /// Two different images at the same JSON path get distinct cache keys
    /// (content-hash disambiguates), so both stay accessible. This is the
    /// multi-image coexistence guarantee.
    #[test]
    fn test_slim_multiple_images_get_distinct_keys() {
        let mut cache = LargeDataCache::new();
        let img_a = fake_jpeg_data_url(40_000);
        let img_b = fake_jpeg_data_url(40_000) + "B"; // different content → different hash

        let mut value = serde_json::json!({
            "a": img_a,
            "b": img_b
        });

        let n = cache.slim_large_strings_in_json(&mut value, "shell");
        assert_eq!(n, 2, "both images should be slimmed");

        let summary_a = value["a"].as_str().unwrap();
        let summary_b = value["b"].as_str().unwrap();

        // Extract the $cached: refs and confirm they differ.
        let ref_a = summary_a
            .split("$cached:")
            .nth(1)
            .and_then(|s| s.split_whitespace().next())
            .expect("ref a present");
        let ref_b = summary_b
            .split("$cached:")
            .nth(1)
            .and_then(|s| s.split_whitespace().next())
            .expect("ref b present");
        assert_ne!(ref_a, ref_b, "refs must differ for different payloads");

        // Both refs must resolve to their respective payloads.
        let resolved_a = cache
            .resolve_reference(&format!("$cached:{}", ref_a))
            .expect("a resolves");
        let resolved_b = cache
            .resolve_reference(&format!("$cached:{}", ref_b))
            .expect("b resolves");
        assert!(resolved_a.ends_with("AAAA"));
        assert!(resolved_b.ends_with("AAAAB"));
    }

    /// Regression for the `device get --metric image_data` → `[image data, N]`
    /// failure. The shell tool wraps a `neomind` CliResponse as a JSON
    /// *string* inside `result.stdout` (double-encoded). An image metric
    /// buried in that string must still become a `$cached` ref — otherwise
    /// slim never fires (one opaque string: no `data:image/` prefix, under
    /// the size ceiling) and `sanitize` later textually mangles the base64,
    /// so the `vision` tool can never see the bytes.
    #[test]
    fn test_slim_descends_into_stringified_json_stdout() {
        let mut cache = LargeDataCache::new();
        // ~7KB — under SLIM_THRESHOLD_BYTES (64KB), so size alone must NOT
        // trigger slim; only the nested `data:image/` leaf should.
        let img = fake_jpeg_data_url(7_000);

        // Inner CliResponse, serialized to a string exactly as the in-process
        // shell dispatcher does, then placed under `stdout`.
        let inner = serde_json::json!({
            "success": true,
            "data": {
                "device_id": "ne301-1",
                "metrics": {
                    "image_data": {
                        "value": img,
                        "unit": "image",
                        "timestamp": "2026-07-15T00:00:00Z"
                    }
                }
            }
        });
        let stdout_str = serde_json::to_string(&inner).unwrap();
        assert!(
            stdout_str.contains("data:image/jpeg;base64,"),
            "precondition: raw base64 present in stringified stdout"
        );

        let mut value = serde_json::json!({
            "exit_code": 0,
            "stdout": stdout_str,
            "stderr": "",
            "command": "neomind device get ne301-1 --metric image_data",
            "timed_out": false
        });

        let n = cache.slim_large_strings_in_json(&mut value, "shell");
        assert!(
            n >= 1,
            "buried image must be slimmed via stdout descent; n={}",
            n
        );

        // stdout stays a STRING (shell contract + result_format rely on it),
        // but its content no longer holds raw base64 and carries a $cached ref.
        let slimmed_stdout = value["stdout"].as_str().expect("stdout stays a string");
        assert!(
            slimmed_stdout.contains("$cached:"),
            "stdout must carry a $cached ref: {}",
            slimmed_stdout
        );
        assert!(
            !slimmed_stdout.contains("data:image/jpeg;base64,"),
            "no raw base64 should remain: {}",
            slimmed_stdout
        );

        // The ref must resolve back to the FULL image bytes — that is what the
        // vision tool reads. If this resolves, the image is no longer lost.
        let ref_key = slimmed_stdout
            .split("$cached:")
            .nth(1)
            .and_then(|s| s.split_whitespace().next())
            .expect("a $cached ref is present");
        let resolved = cache
            .resolve_reference(&format!("$cached:{}", ref_key))
            .expect("vision must be able to resolve the full bytes");
        assert!(
            resolved.starts_with("data:image/jpeg;base64,"),
            "resolved bytes must be the original image, got: {}...",
            &resolved[..resolved.len().min(40)]
        );
    }

    /// Strings under the threshold that aren't data URLs are NOT slimmed —
    /// they flow through unchanged. (Note: `data:image/` URLs are slimmed
    /// regardless of size — see `test_slim_small_data_url_still_replaced`
    /// for that invariant. The "no base64 in prompt ever" guarantee must
    /// hold for all image data, even tiny icons.)
    #[test]
    fn test_slim_small_strings_pass_through() {
        let mut cache = LargeDataCache::new();
        let mut value = serde_json::json!({
            "short": "hello world",
            "medium_text": "x".repeat(500),  // well below 32KB threshold
            "nested": {"deep": "also short"}
        });

        let n = cache.slim_large_strings_in_json(&mut value, "tool");
        assert_eq!(n, 0, "no values should be slimmed");
        assert_eq!(value["short"], "hello world");
        assert_eq!(value["medium_text"], "x".repeat(500));
    }

    /// Even tiny `data:image/` URLs get slimmed — invariant: no base64
    /// (no matter how small) should ever leak into the prompt as raw text.
    /// This defends against edge cases like 50-byte favicon data URLs
    /// confusing the LLM.
    #[test]
    fn test_slim_small_data_url_still_replaced() {
        let mut cache = LargeDataCache::new();
        let mut value = serde_json::json!({
            "icon": "data:image/png;base64,iVBORw0KGgo="  // 31 bytes
        });

        let n = cache.slim_large_strings_in_json(&mut value, "tool");
        assert_eq!(n, 1, "small data URLs must still be slimmed");
        let summary = value["icon"].as_str().unwrap();
        assert!(summary.contains("$cached:"));
    }

    /// Threshold guard: a NON-image string under `SLIM_THRESHOLD_BYTES`
    /// (64KB) must reach the LLM verbatim. Previously the threshold was
    /// 32KB and would hide mid-size payloads (compact JSON configs, short
    /// logs). After bumping to 64KB, only truly bloated values get cached.
    #[test]
    fn test_slim_large_text_under_threshold_passes_through() {
        let mut cache = LargeDataCache::new();
        // 40KB plain text — above the old 32KB ceiling, below the new 64KB one.
        let big_text = "x".repeat(40_000);
        let mut value = serde_json::json!({ "config": big_text.clone() });

        let n = cache.slim_large_strings_in_json(&mut value, "tool");
        assert_eq!(
            n, 0,
            "40KB non-image text should pass through under the 64KB threshold"
        );
        assert_eq!(
            value["config"].as_str().unwrap().len(),
            40_000,
            "text must be untouched"
        );
    }

    /// Threshold guard: a NON-image string ABOVE `SLIM_THRESHOLD_BYTES`
    /// (64KB) IS slimmed — that's the whole point of raising (not removing)
    /// the ceiling.
    #[test]
    fn test_slim_large_text_above_threshold_replaced() {
        let mut cache = LargeDataCache::new();
        let big_text = "y".repeat(70_000); // 70KB > 64KB threshold
        let mut value = serde_json::json!({ "log": big_text });

        let n = cache.slim_large_strings_in_json(&mut value, "tool");
        assert_eq!(n, 1, "70KB non-image text should be slimmed");
        let summary = value["log"].as_str().unwrap();
        assert!(summary.contains("$cached:"), "should reference the cache");
        // Verify the cache actually stored it (70KB > CACHE_THRESHOLD_BYTES 32KB)
        // by extracting the `$cached:<key>` token and resolving it.
        let key = summary
            .find("$cached:")
            .map(|start| &summary[start..])
            .and_then(|s| s.split_whitespace().next())
            .unwrap();
        let resolved = cache
            .resolve_reference(key)
            .expect("cache should have the data");
        assert_eq!(
            resolved.len(),
            70_000,
            "resolved value should be the full 70KB text"
        );
    }

    /// Non-JSON tool outputs must not crash the slim path — but slim is
    /// only called when the caller has already parsed JSON, so this test
    /// documents that a JSON string value containing plain text (not a
    /// data URL, not large) passes through untouched.
    #[test]
    fn test_slim_handles_plain_json_without_large_data() {
        let mut cache = LargeDataCache::new();
        let mut value = serde_json::json!({
            "success": true,
            "data": {"devices": [{"id": "x", "name": "Sensor"}]}
        });

        let n = cache.slim_large_strings_in_json(&mut value, "device_list");
        assert_eq!(n, 0);
        assert!(value["success"].is_boolean());
    }

    /// Arrays of images (e.g. a tool returning a list of frames) all get
    /// slimmed, each with its own cache entry.
    #[test]
    fn test_slim_handles_array_of_images() {
        let mut cache = LargeDataCache::new();
        let mut value = serde_json::json!({
            "frames": [
                fake_jpeg_data_url(40_000),
                fake_jpeg_data_url(40_000) + "X",
                fake_jpeg_data_url(40_000) + "Y",
            ]
        });

        let n = cache.slim_large_strings_in_json(&mut value, "stream");
        assert_eq!(n, 3, "all three frames should be slimmed");

        // Each frame should now be a summary string (no raw base64).
        for (i, frame) in value["frames"].as_array().unwrap().iter().enumerate() {
            let s = frame
                .as_str()
                .unwrap_or_else(|| panic!("frame {} not a string", i));
            assert!(s.contains("$cached:"), "frame {} missing ref", i);
        }
    }

    /// A cached tool result carrying an `/api/images/` URL (the v0.9.6 image
    /// storage form) must resolve to raw base64 — so `$cached:` references and
    /// vision auto-inject still feed vision tools the actual bytes, not a
    /// hostless path the tool can't read.
    #[test]
    fn test_extract_image_data_resolves_api_images_url() {
        let temp =
            std::env::temp_dir().join(format!("neomind_test_cache_img_{}", uuid::Uuid::new_v4()));
        let images_dir = temp.join("images").join("cam1").join("image");
        std::fs::create_dir_all(&images_dir).unwrap();
        let png = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52,
        ];
        std::fs::write(images_dir.join("1700000000.png"), png).unwrap();

        // URL nested under a recognized image key, inside a JSON tool result.
        let data = r#"{"device":"cam1","image":"/api/images/cam1/image/1700000000.png"}"#;
        let resolved = LargeDataCache::extract_image_data(data, &temp);

        use base64::Engine as _;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&resolved)
            .expect("should resolve to raw base64");
        assert_eq!(decoded, png, "got: {resolved}");

        let _ = std::fs::remove_dir_all(&temp);
    }

    /// A bare `/api/images/` URL (not wrapped in JSON) falls back to the raw
    /// string — the downstream vision tool's `resolve_image` handles it, so the
    /// cache must not drop or mangle it.
    #[test]
    fn test_extract_image_data_bare_api_images_url_passthrough() {
        let temp =
            std::env::temp_dir().join(format!("neomind_test_cache_bare_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp).unwrap();
        let url = "/api/images/cam1/image/1700000000.png";
        let resolved = LargeDataCache::extract_image_data(url, &temp);
        assert_eq!(resolved, url, "bare URL should pass through unchanged");
        let _ = std::fs::remove_dir_all(&temp);
    }

    /// Content containing an `/api/images/` URL is image-bearing (regardless of
    /// the tiny URL size) and must be classified as an image type so
    /// `get_latest_image()` can surface it for vision auto-injection.
    #[test]
    fn test_detect_content_type_api_images_url() {
        assert_eq!(
            LargeDataCache::detect_content_type("/api/images/cam1/image/1.png"),
            "image/url"
        );
        assert_eq!(
            LargeDataCache::detect_content_type(
                r#"{"device":"cam1","image":"/api/images/cam1/image/1.png"}"#
            ),
            "image/url"
        );
    }

    /// A result that merely MENTIONS `/api/images/` in prose (e.g. an error
    /// message) must NOT be classified as image — it would falsely trigger
    /// vision auto-injection of a non-image string. Only a bare URL or a JSON
    /// string value counts.
    #[test]
    fn test_detect_content_type_prose_mention_not_image() {
        assert_eq!(
            LargeDataCache::detect_content_type(r#"{"error":"failed to fetch /api/images/x"}"#),
            "application/json"
        );
        // A value that starts with /api/images/ (even under a non-image key)
        // still looks like an image URL value → image/url.
        assert_eq!(
            LargeDataCache::detect_content_type(r#"{"note":"/api/images/cam/1.png"}"#),
            "image/url"
        );
    }

    /// Repeated slim calls (simulating two `device latest` invocations in
    /// the same session) accumulate distinct cache entries — no overwrite.
    #[test]
    fn test_slim_repeated_calls_accumulate() {
        let mut cache = LargeDataCache::new();

        // First call: device A's image.
        let mut v1 = serde_json::json!({"image": fake_jpeg_data_url(40_000)});
        cache.slim_large_strings_in_json(&mut v1, "shell");

        // Second call: device B's image (different content).
        let mut v2 = serde_json::json!({"image": fake_jpeg_data_url(40_000) + "Z"});
        cache.slim_large_strings_in_json(&mut v2, "shell");

        // Both should be resolvable.
        let ref1 = v1["image"]
            .as_str()
            .unwrap()
            .split("$cached:")
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap();
        let ref2 = v2["image"]
            .as_str()
            .unwrap()
            .split("$cached:")
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap();
        assert_ne!(ref1, ref2);
        assert!(cache
            .resolve_reference(&format!("$cached:{}", ref1))
            .is_some());
        assert!(cache
            .resolve_reference(&format!("$cached:{}", ref2))
            .is_some());
    }
}
