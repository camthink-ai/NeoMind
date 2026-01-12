//! Message conversion utilities.
//!
//! This module provides standardized conversions between different message types.

use super::{Content, Message, MessageRole};
use serde_json::Value;

/// Extended message with additional metadata.
#[derive(Debug, Clone)]
pub struct ExtendedMessage {
    /// Role of the sender
    pub role: MessageRole,
    /// Content
    pub content: Content,
    /// Tool calls (if any)
    pub tool_calls: Option<Vec<ToolCall>>,
    /// Tool call ID (for tool responses)
    pub tool_call_id: Option<String>,
    /// Thinking content (for AI reasoning process)
    pub thinking: Option<String>,
    /// Timestamp
    pub timestamp: i64,
}

/// Tool call from LLM.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolCall {
    /// Tool name
    pub name: String,
    /// Call ID
    pub id: String,
    /// Arguments
    pub arguments: Value,
}

impl ExtendedMessage {
    /// Create a new extended message.
    pub fn new(role: MessageRole, content: impl Into<Content>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            thinking: None,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Create a user message.
    pub fn user(content: impl Into<Content>) -> Self {
        Self::new(MessageRole::User, content)
    }

    /// Create an assistant message.
    pub fn assistant(content: impl Into<Content>) -> Self {
        Self::new(MessageRole::Assistant, content)
    }

    /// Create a system message.
    pub fn system(content: impl Into<Content>) -> Self {
        Self::new(MessageRole::System, content)
    }

    /// Create an assistant message with thinking.
    pub fn assistant_with_thinking(content: impl Into<Content>, thinking: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            thinking: Some(thinking.into()),
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Create a tool result message.
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<Content>) -> Self {
        Self {
            role: MessageRole::User, // Tool results are sent as user messages
            content: content.into(),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            thinking: None,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Create an assistant message with tool calls.
    pub fn assistant_with_tools(
        content: impl Into<Content>,
        tool_calls: Vec<ToolCall>,
    ) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
            tool_calls: Some(tool_calls),
            tool_call_id: None,
            thinking: None,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Convert to core Message.
    pub fn to_core(&self) -> Message {
        Message::new(self.role, self.content.clone())
    }

    /// Convert from core Message.
    pub fn from_core(msg: &Message) -> Self {
        Self {
            role: msg.role,
            content: msg.content.clone(),
            tool_calls: None,
            tool_call_id: None,
            thinking: None,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Set the thinking content.
    pub fn with_thinking(mut self, thinking: impl Into<String>) -> Self {
        self.thinking = Some(thinking.into());
        self
    }

    /// Set tool calls.
    pub fn with_tool_calls(mut self, tool_calls: Vec<ToolCall>) -> Self {
        self.tool_calls = Some(tool_calls);
        self
    }

    /// Set tool call ID.
    pub fn with_tool_call_id(mut self, id: impl Into<String>) -> Self {
        self.tool_call_id = Some(id.into());
        self
    }
}

impl From<Message> for ExtendedMessage {
    fn from(msg: Message) -> Self {
        Self {
            role: msg.role,
            content: msg.content,
            tool_calls: None,
            tool_call_id: None,
            thinking: None,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }
}

impl From<ExtendedMessage> for Message {
    fn from(msg: ExtendedMessage) -> Self {
        Message::new(msg.role, msg.content)
    }
}

impl From<&Message> for ExtendedMessage {
    fn from(msg: &Message) -> Self {
        Self {
            role: msg.role,
            content: msg.content.clone(),
            tool_calls: None,
            tool_call_id: None,
            thinking: None,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extended_message_creation() {
        let user_msg = ExtendedMessage::user("Hello");
        assert_eq!(user_msg.role, MessageRole::User);

        let assistant_msg = ExtendedMessage::assistant("Hi there!");
        assert_eq!(assistant_msg.role, MessageRole::Assistant);

        let sys_msg = ExtendedMessage::system("You are helpful");
        assert_eq!(sys_msg.role, MessageRole::System);
    }

    #[test]
    fn test_extended_message_with_thinking() {
        let msg = ExtendedMessage::assistant_with_thinking("Answer", "My reasoning");
        assert_eq!(msg.role, MessageRole::Assistant);
        assert_eq!(msg.thinking, Some("My reasoning".to_string()));
    }

    #[test]
    fn test_extended_message_to_core() {
        let ext = ExtendedMessage::user("Test message");
        let core = ext.to_core();
        assert_eq!(core.role, MessageRole::User);
    }

    #[test]
    fn test_extended_message_from_core() {
        let core = Message::user("Test message");
        let ext = ExtendedMessage::from_core(&core);
        assert_eq!(ext.role, MessageRole::User);
    }

    #[test]
    fn test_extended_message_conversions() {
        let core = Message::assistant("Hello");
        let ext: ExtendedMessage = (&core).into();
        let back: Message = ext.into();
        assert_eq!(back.role, MessageRole::Assistant);
    }
}
