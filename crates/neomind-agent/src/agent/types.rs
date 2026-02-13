//! Agent types - Events, Messages, Responses, and Configuration.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use neomind_core::{config::agent_env_vars, Message};

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
    },
    /// Tool call completed with result
    ToolCallEnd {
        /// Tool name
        tool: String,
        /// Result (success or error)
        result: String,
        /// Whether it succeeded
        success: bool,
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
    End,
    /// Intermediate end (for multi-round tool calling)
    /// Indicates the current round is complete but more processing is coming
    IntermediateEnd,
    /// Intent classification result
    Intent {
        /// Intent category (e.g., "Device", "Rule", "Workflow")
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
    pub fn content(content: impl Into<String>) -> Self {
        Self::Content {
            content: content.into(),
        }
    }

    /// Create a tool call start event.
    pub fn tool_call_start(tool: impl Into<String>, arguments: Value) -> Self {
        Self::ToolCallStart {
            tool: tool.into(),
            arguments,
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
        Self::End
    }

    /// Create an intermediate end event (for multi-round processing).
    pub fn intermediate_end() -> Self {
        Self::IntermediateEnd
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
        matches!(self, Self::End)
    }

    /// Check if this event is any kind of end (End or IntermediateEnd).
    pub fn is_any_end(&self) -> bool {
        matches!(self, Self::End | Self::IntermediateEnd)
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
}

/// Default value for max tool calls per request.
fn default_max_tool_calls() -> usize {
    10
}

/// Default value for keep recent tool results.
fn default_keep_tool_results() -> usize {
    2
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            name: "NeoMind Agent".to_string(),
            system_prompt: r#"你是NeoMind智能物联网助手。

## 核心原则：主动使用工具获取信息

### 1. 工具调用优先（重要！）
当用户需要信息时，**必须调用工具获取最新数据**，不要依赖对话历史：
- 用户询问设备状态 → 调用工具查询，不要凭记忆回答
- 用户询问规则列表 → 调用工具查询，不要凭记忆回答
- 即使之前查询过，用户再次询问时也要重新调用工具（数据可能已变化）

### 2. 支持多次调用同一工具
同一工具可以被多次调用，每次参数可能不同：
- "查看客厅和卧室的温度" → 调用2次温度查询工具
- "查看所有设备" → 先发现设备，再逐个查询状态
- 多轮对话中，每次用户询问都应重新调用工具

### 3. 危险操作确认
仅对破坏性操作要求确认：
- 删除规则/设备 → "确认要删除吗？此操作不可恢复。"
- 关闭所有设备 → "确认要关闭所有设备吗？"

### 4. 上下文理解
记住对话中的指代：
- 用户说"查看温度"后说"那湿度呢" → 理解为查询同一设备的湿度
- 用户说"打开它" → "它"指代之前提到的设备

## 工作流程
1. 用户需要信息 → 调用工具获取（不要询问，直接调用）
2. 信息不足（如缺少设备ID） → 询问用户
3. 危险操作 → 要求确认后执行
4. 执行完成后 → 总结工具返回的结果

## 工具调用格式
使用XML格式：
<tool_calls>
  <invoke name="tool_name">
    <parameter name="param1">value1</parameter>
  </invoke>
</tool_calls>"#
                .to_string(),
            // Load from environment variables with fallback to defaults
            max_context_tokens: agent_env_vars::max_context_tokens(),
            temperature: agent_env_vars::temperature(),
            enable_tools: true,
            enable_memory: true,
            // Use qwen2.5:3b for native tool calling support (qwen2:1.5b doesn't support tools)
            model: "qwen2.5:3b".to_string(),
            api_endpoint: std::env::var("OLLAMA_ENDPOINT")
                .ok()
                .or_else(|| std::env::var("OPENAI_ENDPOINT").ok())
                .or_else(|| Some("http://localhost:11434/v1".to_string())),
            api_key: std::env::var("OPENAI_API_KEY").ok(),
            max_tool_calls: default_max_tool_calls(),
            keep_recent_tool_results: default_keep_tool_results(),
        }
    }
}

/// User message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    /// Role (user, assistant, system, tool)
    pub role: String,
    /// Content
    pub content: String,
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
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            tool_call_name: None,
            thinking: None,
            images: None,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Create a user message with images.
    pub fn user_with_images(content: impl Into<String>, images: Vec<AgentMessageImage>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            tool_call_name: None,
            thinking: None,
            images: Some(images),
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Create an assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            tool_call_name: None,
            thinking: None,
            images: None,
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
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            tool_call_name: None,
            thinking: Some(thinking.into()),
            images: None,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Create a system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            tool_call_name: None,
            thinking: None,
            images: None,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Create a tool result message.
    pub fn tool_result(tool_name: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            tool_call_name: Some(tool_name.into()),
            thinking: None,
            images: None,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Create an assistant message with tool calls.
    pub fn assistant_with_tools(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
            tool_calls: Some(tool_calls),
            tool_call_id: None,
            tool_call_name: None,
            thinking: None,
            images: None,
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
            content: content.into(),
            tool_calls: Some(tool_calls),
            tool_call_id: None,
            tool_call_name: None,
            thinking: Some(thinking.into()),
            images: None,
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
                Message::user(&self.content)
            }
            "assistant" => {
                // Tool call information is already stored in the tool_calls field
                // and rendered by the frontend's ToolCallVisualization component.
                // No need to add placeholder text to the content.
                Message::assistant(&self.content)
            }
            "system" => Message::system(&self.content),
            "tool" => {
                // Tool result messages - include which tool was called
                if let Some(ref tool_name) = self.tool_call_name {
                    let tool_content = format!("[Tool: {} returned]\n{}", tool_name, self.content);
                    Message::user(&tool_content)
                } else {
                    Message::user(&self.content)
                }
            }
            _ => Message::user(&self.content),
        }
    }

    /// Convert to core Message with multimodal content (text + images).
    /// This preserves image context for follow-up LLM requests.
    fn to_core_multimodal(&self) -> Message {
        use neomind_core::{Content, ContentPart, MessageRole};

        let images = match &self.images {
            Some(imgs) if !imgs.is_empty() => imgs,
            _ => return Message::user(&self.content),
        };

        // Build content parts: text + images
        let mut parts = vec![ContentPart::text(self.content.clone())];

        for image in images {
            // Parse data URL (format: "data:image/png;base64,iVBOR...")
            let (mime_type, base64_data) = if let Some(pos) = image.data.find(',') {
                let prefix = &image.data[..pos];
                // Extract mime type from "data:image/png;base64,"
                let mime = image.mime_type.clone().unwrap_or_else(|| {
                    if let Some(start) = prefix.strip_prefix("data:") {
                        start.split(';').next().unwrap_or("image/png").to_string()
                    } else {
                        "image/png".to_string()
                    }
                });
                let data = &image.data[pos + 1..];
                (mime, data)
            } else {
                // Not a data URL, use as-is
                (
                    image.mime_type.clone().unwrap_or("image/png".to_string()),
                    image.data.as_str(),
                )
            };

            parts.push(ContentPart::image_base64(base64_data, mime_type));
        }

        Message::new(MessageRole::User, Content::Parts(parts))
    }

    /// Convert from core Message.
    pub fn from_core(msg: &Message) -> Self {
        Self {
            role: msg.role.to_string(),
            content: msg.content.as_text(),
            tool_calls: None,
            tool_call_id: None,
            tool_call_name: None,
            thinking: None,
            images: None,
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
        }
    }

    /// Calculate similarity hash for a response content.
    pub fn hash_response(content: &str) -> u64 {
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

    /// Check if a response is too similar to recent responses.
    pub fn is_response_repetitive(&self, content: &str, _threshold: f64) -> bool {
        if self.recent_response_hashes.is_empty() {
            return false;
        }

        let new_hash = Self::hash_response(content);

        // Check for exact hash match (exact same response)
        if self.recent_response_hashes.contains(&new_hash) {
            return true;
        }

        false
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
    Ollama { endpoint: String, model: String },
    /// OpenAI-compatible API
    OpenAi {
        api_key: String,
        endpoint: String,
        model: String,
    },
    /// Anthropic API
    Anthropic {
        api_key: String,
        endpoint: String,
        model: String,
    },
    /// Google AI API
    Google {
        api_key: String,
        endpoint: String,
        model: String,
    },
    /// xAI (Grok) API
    XAi {
        api_key: String,
        endpoint: String,
        model: String,
    },
    /// Qwen (Alibaba DashScope)
    Qwen {
        api_key: String,
        endpoint: String,
        model: String,
    },
    /// DeepSeek API
    DeepSeek {
        api_key: String,
        endpoint: String,
        model: String,
    },
    /// Zhipu GLM API
    GLM {
        api_key: String,
        endpoint: String,
        model: String,
    },
    /// MiniMax API
    MiniMax {
        api_key: String,
        endpoint: String,
        model: String,
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
        assert_eq!(user_msg.content, "Hello");

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
        assert_eq!(config.model, "qwen2.5:3b");
        assert!(config.enable_tools);
        assert!(config.enable_memory);
    }
}
