//! Short-term memory for current conversation context.
//!
//! Short-term memory holds the current conversation context with a limited size.
//! It's used for maintaining the immediate chat history.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::error::{MemoryError, Result};

/// Maximum default size of short-term memory
pub const DEFAULT_MAX_MESSAGES: usize = 100;

/// Default max tokens for short-term memory
pub const DEFAULT_MAX_TOKENS: usize = 4000;

/// A message in short-term memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMessage {
    /// Unique ID
    pub id: String,
    /// Role (user, assistant, system)
    pub role: String,
    /// Content
    pub content: String,
    /// Timestamp
    pub timestamp: i64,
    /// Estimated token count
    pub token_count: usize,
    /// Additional metadata
    pub metadata: Option<serde_json::Value>,
}

impl MemoryMessage {
    /// Create a new memory message.
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        let content_str = content.into();
        Self {
            id: Uuid::new_v4().to_string(),
            role: role.into(),
            token_count: estimate_tokens(&content_str),
            timestamp: chrono::Utc::now().timestamp(),
            content: content_str,
            metadata: None,
        }
    }

    /// Create a new memory message with metadata.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Get a summary of the message.
    pub fn summary(&self) -> String {
        let content_preview = if self.content.len() > 50 {
            format!("{}...", &self.content[..50])
        } else {
            self.content.clone()
        };
        format!("[{}] {}", self.role, content_preview)
    }
}

/// Estimate token count (rough approximation: ~4 chars per token).
fn estimate_tokens(text: &str) -> usize {
    (text.len() + 3) / 4
}

/// Short-term memory for current conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortTermMemory {
    /// Messages in the conversation
    messages: VecDeque<MemoryMessage>,
    /// Maximum number of messages
    max_messages: usize,
    /// Maximum tokens allowed
    max_tokens: usize,
    /// Current token count
    current_tokens: usize,
    /// System prompt (if any)
    system_prompt: Option<String>,
}

impl ShortTermMemory {
    /// Create a new short-term memory.
    pub fn new() -> Self {
        Self {
            messages: VecDeque::new(),
            max_messages: DEFAULT_MAX_MESSAGES,
            max_tokens: DEFAULT_MAX_TOKENS,
            current_tokens: 0,
            system_prompt: None,
        }
    }

    /// Set the maximum number of messages.
    pub fn with_max_messages(mut self, max: usize) -> Self {
        self.max_messages = max;
        self
    }

    /// Set the maximum tokens.
    pub fn with_max_tokens(mut self, max: usize) -> Self {
        self.max_tokens = max;
        self
    }

    /// Set the system prompt.
    pub fn with_system_prompt(mut self, prompt: String) -> Self {
        self.system_prompt = Some(prompt);
        self
    }

    /// Add a message to memory.
    pub fn add(&mut self, role: impl Into<String>, content: impl Into<String>) -> Result<()> {
        let message = MemoryMessage::new(role, content);
        self.add_message(message)
    }

    /// Add a message to memory.
    pub fn add_message(&mut self, message: MemoryMessage) -> Result<()> {
        // Check if adding would exceed token limit
        if self.current_tokens + message.token_count > self.max_tokens {
            // Try to evict oldest messages to make room
            while !self.messages.is_empty()
                && self.current_tokens + message.token_count > self.max_tokens
            {
                if let Some(removed) = self.messages.pop_front() {
                    self.current_tokens -= removed.token_count;
                }
            }

            // If still too large, return error
            if self.current_tokens + message.token_count > self.max_tokens {
                return Err(MemoryError::CapacityExceeded(format!(
                    "Message token count {} exceeds remaining capacity {}",
                    message.token_count,
                    self.max_tokens - self.current_tokens
                )));
            }
        }

        // Enforce message count limit
        while self.messages.len() >= self.max_messages {
            if let Some(removed) = self.messages.pop_front() {
                self.current_tokens -= removed.token_count;
            }
        }

        self.current_tokens += message.token_count;
        self.messages.push_back(message);
        Ok(())
    }

    /// Get all messages.
    pub fn get_messages(&self) -> Vec<MemoryMessage> {
        self.messages.iter().cloned().collect()
    }

    /// Get the last N messages.
    pub fn get_last_n(&self, n: usize) -> Vec<MemoryMessage> {
        self.messages
            .iter()
            .rev()
            .take(n)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    /// Clear all messages.
    pub fn clear(&mut self) {
        self.messages.clear();
        self.current_tokens = 0;
    }

    /// Get the number of messages.
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Check if memory is empty.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Get the current token count.
    pub fn token_count(&self) -> usize {
        self.current_tokens
    }

    /// Get the system prompt.
    pub fn system_prompt(&self) -> Option<&str> {
        self.system_prompt.as_deref()
    }

    /// Set the system prompt.
    pub fn set_system_prompt(&mut self, prompt: String) {
        self.system_prompt = Some(prompt);
    }

    /// Remove the system prompt.
    pub fn clear_system_prompt(&mut self) {
        self.system_prompt = None;
    }

    /// Get a formatted prompt for LLM.
    pub fn to_llm_prompt(&self) -> String {
        let mut result = String::new();

        if let Some(system) = &self.system_prompt {
            result.push_str(&format!("System: {}\n\n", system));
        }

        for msg in &self.messages {
            result.push_str(&format!("{}: {}\n", msg.role, msg.content));
        }

        result
    }

    /// Find messages by role.
    pub fn find_by_role(&self, role: &str) -> Vec<MemoryMessage> {
        self.messages
            .iter()
            .filter(|m| m.role == role)
            .cloned()
            .collect()
    }

    /// Get the last message.
    pub fn last_message(&self) -> Option<MemoryMessage> {
        self.messages.back().cloned()
    }
}

impl Default for ShortTermMemory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_message_creation() {
        let msg = MemoryMessage::new("user", "Hello, world!");
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "Hello, world!");
        assert!(!msg.id.is_empty());
    }

    #[test]
    fn test_short_term_memory() {
        let mut memory = ShortTermMemory::new();
        assert!(memory.is_empty());

        memory.add("user", "Hello").unwrap();
        memory.add("assistant", "Hi there!").unwrap();

        assert_eq!(memory.len(), 2);
        assert!(!memory.is_empty());
    }

    #[test]
    fn test_max_messages_limit() {
        let mut memory = ShortTermMemory::new().with_max_messages(3);

        for i in 0..5 {
            memory.add("user", format!("Message {}", i)).unwrap();
        }

        assert_eq!(memory.len(), 3);
        // Should have last 3 messages
        let messages = memory.get_messages();
        assert_eq!(messages[0].content, "Message 2");
        assert_eq!(messages[2].content, "Message 4");
    }

    #[test]
    fn test_get_last_n() {
        let mut memory = ShortTermMemory::new();

        for i in 0..5 {
            memory.add("user", format!("Message {}", i)).unwrap();
        }

        let last_2 = memory.get_last_n(2);
        assert_eq!(last_2.len(), 2);
        assert_eq!(last_2[0].content, "Message 3");
        assert_eq!(last_2[1].content, "Message 4");
    }

    #[test]
    fn test_clear() {
        let mut memory = ShortTermMemory::new();
        memory.add("user", "Test").unwrap();
        assert_eq!(memory.len(), 1);

        memory.clear();
        assert!(memory.is_empty());
        assert_eq!(memory.token_count(), 0);
    }

    #[test]
    fn test_system_prompt() {
        let mut memory = ShortTermMemory::new().with_system_prompt("You are a helpful assistant.".to_string());
        assert_eq!(memory.system_prompt(), Some("You are a helpful assistant."));

        memory.set_system_prompt("New prompt".to_string());
        assert_eq!(memory.system_prompt(), Some("New prompt"));

        memory.clear_system_prompt();
        assert!(memory.system_prompt().is_none());
    }

    #[test]
    fn test_find_by_role() {
        let mut memory = ShortTermMemory::new();
        memory.add("user", "User message 1").unwrap();
        memory.add("assistant", "Assistant message").unwrap();
        memory.add("user", "User message 2").unwrap();

        let user_msgs = memory.find_by_role("user");
        assert_eq!(user_msgs.len(), 2);

        let assistant_msgs = memory.find_by_role("assistant");
        assert_eq!(assistant_msgs.len(), 1);
    }

    #[test]
    fn test_to_llm_prompt() {
        let mut memory = ShortTermMemory::new().with_system_prompt("System prompt".to_string());
        memory.add("user", "Hello").unwrap();

        let prompt = memory.to_llm_prompt();
        assert!(prompt.contains("System prompt"));
        assert!(prompt.contains("user: Hello"));
    }

    #[test]
    fn test_memory_message_summary() {
        let msg = MemoryMessage::new("user", "This is a very long message that should be truncated in the summary");
        let summary = msg.summary();
        assert!(summary.contains("[user]"));
        assert!(summary.contains("..."));
    }
}
