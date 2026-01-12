//! Session management for chat conversations.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::message::Message;

/// Unique identifier for a session.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

impl SessionId {
    /// Create a new random session ID.
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Create a session ID from a string.
    pub fn from_str(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Get the string representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<String> for SessionId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for SessionId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Metadata for a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// Optional title for the session.
    pub title: Option<String>,
    /// Model used for this session.
    pub model: Option<String>,
    /// Creation timestamp.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last update timestamp.
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Default for SessionMetadata {
    fn default() -> Self {
        let now = chrono::Utc::now();
        Self {
            title: None,
            model: None,
            created_at: now,
            updated_at: now,
        }
    }
}

/// A chat session storing conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique session identifier.
    pub id: SessionId,
    /// Session metadata.
    pub metadata: SessionMetadata,
    /// Message history.
    pub messages: Vec<Message>,
    /// Session context (key-value pairs for tool use, etc.).
    #[serde(default)]
    pub context: HashMap<String, serde_json::Value>,
}

impl Session {
    /// Create a new session.
    pub fn new() -> Self {
        Self::with_id(SessionId::new())
    }

    /// Create a session with a specific ID.
    pub fn with_id(id: SessionId) -> Self {
        Self {
            id,
            metadata: SessionMetadata::default(),
            messages: Vec::new(),
            context: HashMap::new(),
        }
    }

    /// Add a message to the session.
    pub fn add_message(&mut self, message: Message) {
        self.messages.push(message);
        self.metadata.updated_at = chrono::Utc::now();
    }

    /// Get the message history.
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// Clear all messages.
    pub fn clear(&mut self) {
        self.messages.clear();
        self.metadata.updated_at = chrono::Utc::now();
    }

    /// Set the session title.
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.metadata.title = Some(title.into());
        self.metadata.updated_at = chrono::Utc::now();
    }

    /// Set the model for this session.
    pub fn set_model(&mut self, model: impl Into<String>) {
        self.metadata.model = Some(model.into());
        self.metadata.updated_at = chrono::Utc::now();
    }

    /// Get a context value.
    pub fn get_context(&self, key: &str) -> Option<&serde_json::Value> {
        self.context.get(key)
    }

    /// Set a context value.
    pub fn set_context(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.context.insert(key.into(), value);
        self.metadata.updated_at = chrono::Utc::now();
    }

    /// Return the number of messages in the session.
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Check if the session is empty.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::MessageRole;

    #[test]
    fn test_session_creation() {
        let session = Session::new();
        assert!(session.is_empty());
        assert_eq!(session.len(), 0);
    }

    #[test]
    fn test_add_message() {
        let mut session = Session::new();
        session.add_message(Message::user("Hello"));
        assert_eq!(session.len(), 1);
        assert_eq!(session.messages[0].role, MessageRole::User);
    }
}
