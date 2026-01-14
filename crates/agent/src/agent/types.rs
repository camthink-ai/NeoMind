//! Agent types - Events, Messages, Responses, and Configuration.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use edge_ai_core::{Message, SessionId};

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
    /// Stream ended
    End,
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
    pub fn tool_call_end(tool: impl Into<String>, result: impl Into<String>, success: bool) -> Self {
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

    /// Create an end event.
    pub fn end() -> Self {
        Self::End
    }

    /// Check if this event ends the stream.
    pub fn is_end(&self) -> bool {
        matches!(self, Self::End)
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
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            name: "NeoTalk Agent".to_string(),
            // IMPORTANT: Keep system prompt SHORT to prevent excessive thinking
            // Long prompts can trigger infinite thinking loops in reasoning models
            system_prompt: r#"你是NeoTalk物联网助手。帮助用户管理设备、查询数据和配置规则。

回复要求：简洁明了，直接回答问题。"#.to_string(),
            max_context_tokens: 8000,
            temperature: 0.4,
            enable_tools: true,
            enable_memory: true,
            model: "qwen3-vl:2b".to_string(),
            api_endpoint: std::env::var("OLLAMA_ENDPOINT").ok()
                .or_else(|| std::env::var("OPENAI_ENDPOINT").ok())
                .or_else(|| Some("http://localhost:11434/v1".to_string())),
            api_key: std::env::var("OPENAI_API_KEY").ok(),
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
    /// Timestamp
    pub timestamp: i64,
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
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Create an assistant message with thinking.
    pub fn assistant_with_thinking(content: impl Into<String>, thinking: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            tool_call_name: None,
            thinking: Some(thinking.into()),
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
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Convert to core Message.
    /// IMPORTANT: When tool_calls exist, include them in content for LLM context.
    /// This ensures the model knows what tools were called in previous turns.
    pub fn to_core(&self) -> Message {
        match self.role.as_str() {
            "user" => Message::user(&self.content),
            "assistant" => {
                // If this message has tool calls, include them in the content
                // so the LLM knows what tools were called in the conversation
                if let Some(ref calls) = self.tool_calls {
                    if !calls.is_empty() {
                        let mut full_content = self.content.clone();
                        // Don't append tool calls if content already ends with them (from streaming)
                        if !full_content.contains("<tool_calls>") {
                            full_content.push_str("\n\n[Tools called: ");
                            for (i, call) in calls.iter().enumerate() {
                                if i > 0 {
                                    full_content.push_str(", ");
                                }
                                full_content.push_str(&call.name);
                            }
                            full_content.push(']');
                        }
                        Message::assistant(&full_content)
                    } else {
                        Message::assistant(&self.content)
                    }
                } else {
                    Message::assistant(&self.content)
                }
            },
            "system" => Message::system(&self.content),
            "tool" => {
                // Tool result messages - include which tool was called
                if let Some(ref tool_name) = self.tool_call_name {
                    let tool_content = format!("[Tool: {} returned]\n{}", tool_name, self.content);
                    Message::user(&tool_content)
                } else {
                    Message::user(&self.content)
                }
            },
            _ => Message::user(&self.content),
        }
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

/// LLM backend type with configuration.
#[derive(Debug, Clone)]
pub enum LlmBackend {
    /// Ollama (local)
    Ollama {
        endpoint: String,
        model: String,
    },
    /// OpenAI-compatible API
    OpenAi {
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
        assert_eq!(config.name, "NeoTalk Agent");
        assert_eq!(config.model, "qwen3-vl:2b");
        assert!(config.enable_tools);
        assert!(config.enable_memory);
    }
}
