//! Context Compaction Strategy for LLM
//!
//! Provides intelligent context window management following moltbot design patterns:
//! - reserve_tokens_floor: Minimum tokens to always keep available for generation
//! - max_history_share: Maximum percentage of context window for history
//! - Smart message selection with priority (system > user > assistant > tool)
//! - Truncation with ellipsis for long messages
//!
//! ## Example
//!
//! ```rust
//! use edge_ai_core::llm::compaction::{CompactionConfig, compact_messages};
//! use edge_ai_core::message::Message;
//!
//! let config = CompactionConfig::default()
//!     .with_reserve_tokens_floor(1024)
//!     .with_max_history_share(0.75);
//!
//! let messages = vec![
//!     Message::system("You are a helpful assistant.".to_string()),
//!     Message::user("Hello!".to_string()),
//! ];
//! let compacted = compact_messages(&messages, &config, 4096);
//! ```

use crate::message::{Message, MessageRole};
use serde::{Deserialize, Serialize};

/// Configuration for context compaction.
///
/// Based on moltbot's context management strategy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionConfig {
    /// Minimum tokens to always keep available for response generation.
    /// This ensures the model has room to generate responses.
    /// Default: 1024 tokens (4KB for most models)
    pub reserve_tokens_floor: usize,

    /// Maximum percentage of context window that can be used for history.
    /// The rest is reserved for system prompt and response generation.
    /// Range: 0.0 - 1.0, Default: 0.75 (75%)
    pub max_history_share: f64,

    /// Minimum number of recent messages to always keep.
    /// Ensures conversation continuity.
    /// Default: 4 messages
    pub min_recent_messages: usize,

    /// Maximum message length before truncation.
    /// Messages longer than this are truncated with an ellipsis.
    /// Default: 4096 characters
    pub max_message_length: usize,

    /// Whether to enable smart tool result compaction.
    /// Tool results beyond the recent threshold are summarized.
    /// Default: true
    pub compact_tool_results: bool,

    /// Number of recent tool results to keep in full.
    /// Default: 2
    pub keep_recent_tool_results: usize,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            reserve_tokens_floor: 1024,
            max_history_share: 0.75,
            min_recent_messages: 4,
            max_message_length: 4096,
            compact_tool_results: true,
            keep_recent_tool_results: 2,
        }
    }
}

impl CompactionConfig {
    /// Create a new compaction config with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the reserve tokens floor.
    pub fn with_reserve_tokens_floor(mut self, tokens: usize) -> Self {
        self.reserve_tokens_floor = tokens;
        self
    }

    /// Set the maximum history share (0.0 - 1.0).
    pub fn with_max_history_share(mut self, share: f64) -> Self {
        assert!((0.0..=1.0).contains(&share), "max_history_share must be between 0.0 and 1.0");
        self.max_history_share = share;
        self
    }

    /// Set the minimum number of recent messages to keep.
    pub fn with_min_recent_messages(mut self, count: usize) -> Self {
        self.min_recent_messages = count;
        self
    }

    /// Set the maximum message length before truncation.
    pub fn with_max_message_length(mut self, length: usize) -> Self {
        self.max_message_length = length;
        self
    }

    /// Enable or disable tool result compaction.
    pub fn with_compact_tool_results(mut self, enabled: bool) -> Self {
        self.compact_tool_results = enabled;
        self
    }

    /// Set the number of recent tool results to keep.
    pub fn with_keep_recent_tool_results(mut self, count: usize) -> Self {
        self.keep_recent_tool_results = count;
        self
    }

    /// Calculate the maximum tokens available for history given a context window size.
    pub fn max_history_tokens(&self, context_window: usize) -> usize {
        let history_budget = (context_window as f64 * self.max_history_share) as usize;
        let with_reserve = history_budget.saturating_sub(self.reserve_tokens_floor);
        with_reserve.max(self.min_recent_messages * 100) // At least 100 tokens per min message
    }

    /// Create a conservative config for small context windows.
    pub fn conservative() -> Self {
        Self {
            reserve_tokens_floor: 512,
            max_history_share: 0.6,
            min_recent_messages: 2,
            max_message_length: 2048,
            compact_tool_results: true,
            keep_recent_tool_results: 1,
        }
    }

    /// Create an aggressive config for large context windows.
    pub fn aggressive() -> Self {
        Self {
            reserve_tokens_floor: 2048,
            max_history_share: 0.85,
            min_recent_messages: 6,
            max_message_length: 8192,
            compact_tool_results: true,
            keep_recent_tool_results: 3,
        }
    }
}

/// Priority level for message selection during compaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MessagePriority {
    /// System prompts - highest priority
    System = 4,
    /// User messages - high priority
    User = 3,
    /// Assistant messages - medium priority
    Assistant = 2,
    /// Tool calls/results - lowest priority
    Tool = 1,
}

impl MessagePriority {
    /// Get the priority for a message role.
    pub fn from_role(role: &MessageRole) -> Self {
        match role {
            MessageRole::System => Self::System,
            MessageRole::User => Self::User,
            MessageRole::Assistant => Self::Assistant,
        }
    }
}

/// Result of message compaction.
#[derive(Debug, Clone)]
pub struct CompactionResult {
    /// The compacted messages
    pub messages: Vec<Message>,
    /// Original token count estimate
    pub original_tokens: usize,
    /// Compacted token count estimate
    pub compacted_tokens: usize,
    /// Number of messages removed
    pub messages_removed: usize,
    /// Number of messages truncated
    pub messages_truncated: usize,
}

impl CompactionResult {
    /// Calculate the compression ratio.
    pub fn compression_ratio(&self) -> f64 {
        if self.original_tokens == 0 {
            return 1.0;
        }
        self.compacted_tokens as f64 / self.original_tokens as f64
    }
}

/// Compact a list of messages to fit within a token budget.
///
/// This function:
/// 1. Keeps system messages and recent messages
/// 2. Truncates long messages
/// 3. Compacts old tool results if enabled
/// 4. Selects messages by priority when over budget
pub fn compact_messages(
    messages: &[Message],
    config: &CompactionConfig,
    context_window: usize,
) -> CompactionResult {
    let max_tokens = config.max_history_tokens(context_window);
    let original_tokens = estimate_messages_tokens(messages);

    // Fast path: if we're already under budget, just return
    if original_tokens <= max_tokens {
        return CompactionResult {
            messages: messages.to_vec(),
            original_tokens,
            compacted_tokens: original_tokens,
            messages_removed: 0,
            messages_truncated: 0,
        };
    }

    let mut result = Vec::new();
    let mut tool_result_count = 0;
    let mut removed_count = 0;
    let mut truncated_count = 0;
    let mut current_tokens = 0;

    // Process in reverse (most recent first)
    for msg in messages.iter().rev() {
        let priority = MessagePriority::from_role(&msg.role);
        let msg_tokens = estimate_message_tokens(&msg.content);

        // Always keep system messages and recent min messages
        let is_recent = result.len() < config.min_recent_messages;
        let should_keep = priority == MessagePriority::System || is_recent;

        if !should_keep {
            // Check if adding this would exceed budget
            if current_tokens + msg_tokens > max_tokens {
                removed_count += 1;
                continue;
            }
        }

        // Handle tool result compaction
        if config.compact_tool_results && priority == MessagePriority::Assistant {
            // Check if this looks like a tool result (has tool_call_id in text)
            let content_text = content_as_text(&msg.content);
            if content_text.contains("tool_call_id") {
                tool_result_count += 1;
                if tool_result_count > config.keep_recent_tool_results {
                    // Summarize old tool result
                    let truncated = truncate_text(&content_text, 200);
                    let summary_msg = Message {
                        role: msg.role,
                        content: crate::message::Content::Text(
                            format!("[Previous tool result: {}]", truncated)
                        ),
                        timestamp: msg.timestamp,
                    };
                    current_tokens += estimate_message_tokens(&summary_msg.content);
                    result.push(summary_msg);
                    continue;
                }
            }
        }

        // Truncate if too long
        let final_msg = if msg_tokens > config.max_message_length {
            truncated_count += 1;
            truncate_message(msg, config.max_message_length)
        } else {
            msg.clone()
        };

        current_tokens += estimate_message_tokens(&final_msg.content);
        result.push(final_msg);
    }

    result.reverse();

    CompactionResult {
        messages: result,
        original_tokens,
        compacted_tokens: current_tokens,
        messages_removed: removed_count,
        messages_truncated: truncated_count,
    }
}

/// Estimate token count for a text string.
///
/// Uses a heuristic approach:
/// - Chinese characters: ~1.8 tokens each
/// - English words: ~0.25 tokens per character (4 chars = 1 token)
/// - Special characters: ~0.5 tokens each
pub fn estimate_tokens(text: &str) -> usize {
    let mut tokens = 0f64;

    for line in text.lines() {
        let chinese_count = line.chars().filter(|c| is_chinese(*c)).count() as f64;
        let english_count = line.chars().filter(|c| c.is_ascii_alphabetic()).count() as f64;
        let number_count = line.chars().filter(|c| c.is_ascii_digit()).count() as f64;
        let special_count = line.chars().filter(|c| !c.is_alphanumeric()).count() as f64;

        tokens += chinese_count * 1.8;
        tokens += english_count * 0.25;
        tokens += number_count * 0.3;
        tokens += special_count * 0.5;
    }

    (tokens * 1.1).ceil() as usize
}

/// Check if a character is CJK.
fn is_chinese(c: char) -> bool {
    let cp = c as u32;
    (0x4E00..=0x9FFF).contains(&cp)
        || (0x3400..=0x4DBF).contains(&cp)
        || (0xF900..=0xFAFF).contains(&cp)
        || (0xFF00..=0xFFEF).contains(&cp)
        || (0x3040..=0x309F).contains(&cp)
        || (0x30A0..=0x30FF).contains(&cp)
}

/// Extract text content from Content enum.
fn content_as_text(content: &crate::message::Content) -> String {
    match content {
        crate::message::Content::Text(text) => text.clone(),
        crate::message::Content::Parts(parts) => {
            // Extract text from parts and note multimodal content
            let mut result = String::new();
            let mut has_multimodal = false;
            for part in parts {
                match part {
                    crate::message::ContentPart::Text { text } => {
                        result.push_str(text);
                    }
                    crate::message::ContentPart::ImageUrl { .. } |
                    crate::message::ContentPart::ImageBase64 { .. } => {
                        has_multimodal = true;
                        result.push_str("[image]");
                    }
                }
            }
            if has_multimodal {
                result = format!("[multimodal] {}", result);
            }
            result
        }
    }
}

/// Estimate tokens for message content.
fn estimate_message_tokens(content: &crate::message::Content) -> usize {
    match content {
        crate::message::Content::Text(text) => estimate_tokens(text),
        crate::message::Content::Parts(parts) => {
            // Estimate tokens for multimodal content
            let mut total = 0;
            for part in parts {
                match part {
                    crate::message::ContentPart::Text { text } => {
                        total += estimate_tokens(text);
                    }
                    crate::message::ContentPart::ImageUrl { .. } |
                    crate::message::ContentPart::ImageBase64 { .. } => {
                        total += 85; // Approximate token cost for images
                    }
                }
            }
            total
        }
    }
}

/// Estimate tokens for multiple messages.
pub fn estimate_messages_tokens(messages: &[Message]) -> usize {
    messages
        .iter()
        .map(|m| estimate_message_tokens(&m.content))
        .sum()
}

/// Truncate text to a maximum length, adding ellipsis if truncated.
pub fn truncate_text(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        return text.to_string();
    }

    // Try to truncate at a word boundary
    let truncated = &text[..max_len];
    if let Some(last_space) = truncated.rfind(' ') {
        format!("{}...", &text[..last_space])
    } else {
        format!("{}...", truncated)
    }
}

/// Truncate a message's content to fit within max length.
pub fn truncate_message(msg: &Message, max_len: usize) -> Message {
    let mut truncated = msg.clone();
    truncated.content = match &msg.content {
        crate::message::Content::Text(text) => {
            crate::message::Content::Text(truncate_text(text, max_len))
        }
        // For non-text content, keep as is (could add placeholder truncation in future)
        other => other.clone(),
    };
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens_chinese() {
        let tokens = estimate_tokens("你好世界");
        assert!(tokens > 4);
        assert!(tokens < 15);
    }

    #[test]
    fn test_estimate_tokens_english() {
        let tokens = estimate_tokens("Hello world");
        assert!(tokens > 0);
        assert!(tokens < 10);
    }

    #[test]
    fn test_truncate_text() {
        let text = "This is a long text that should be truncated";
        let truncated = truncate_text(text, 20);
        assert!(truncated.len() <= text.len() + 3); // +3 for "..."
        assert!(truncated.contains("..."));
    }

    #[test]
    fn test_compaction_config_default() {
        let config = CompactionConfig::default();
        assert_eq!(config.reserve_tokens_floor, 1024);
        assert_eq!(config.max_history_share, 0.75);
        assert_eq!(config.min_recent_messages, 4);
    }

    #[test]
    fn test_compaction_config_builder() {
        let config = CompactionConfig::default()
            .with_reserve_tokens_floor(2048)
            .with_max_history_share(0.8);

        assert_eq!(config.reserve_tokens_floor, 2048);
        assert_eq!(config.max_history_share, 0.8);
    }

    #[test]
    fn test_max_history_tokens() {
        let config = CompactionConfig::default();
        // 4096 window, 75% share, 1024 reserve = 4096 * 0.75 - 1024 = 2048
        let max = config.max_history_tokens(4096);
        assert!(max > 2000 && max < 2100);
    }

    #[test]
    fn test_conservative_config() {
        let config = CompactionConfig::conservative();
        assert_eq!(config.reserve_tokens_floor, 512);
        assert_eq!(config.max_history_share, 0.6);
    }

    #[test]
    fn test_aggressive_config() {
        let config = CompactionConfig::aggressive();
        assert_eq!(config.reserve_tokens_floor, 2048);
        assert_eq!(config.max_history_share, 0.85);
    }
}
