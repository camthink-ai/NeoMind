//! Memory consolidation for conversation history.
//!
//! This module provides memory consolidation strategies to periodically
//! compress old conversation history into summaries, following the
//! moltbot design patterns.
//!
//! ## Example
//!
//! ```rust
//! use edge_ai_core::llm::memory_consolidation::{MemoryConfig, MemoryConsolidator};
//!
//! let config = MemoryConfig::default()
//!     .with_max_messages_before_consolidation(50)
//!     .with_consolidation_ratio(0.3);
//!
//! let consolidator = MemoryConsolidator::new(config);
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for memory consolidation.
///
/// Controls when and how conversation history is consolidated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Maximum number of messages before triggering consolidation.
    /// Default: 50 messages
    pub max_messages_before_consolidation: usize,

    /// Target ratio after consolidation (0.0 - 1.0).
    /// e.g., 0.3 means keep 30% of original messages.
    /// Default: 0.3
    pub consolidation_ratio: f64,

    /// Minimum number of recent messages to always keep.
    /// Default: 10 messages
    pub min_recent_messages: usize,

    /// Whether to consolidate system messages.
    /// Default: false (always keep system messages)
    pub consolidate_system_messages: bool,

    /// Whether to preserve tool calls in consolidated history.
    /// Default: true
    pub preserve_tool_calls: bool,

    /// Maximum age of messages to keep (in seconds).
    /// None means no age limit.
    /// Default: None
    pub max_message_age_seconds: Option<i64>,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            max_messages_before_consolidation: 50,
            consolidation_ratio: 0.3,
            min_recent_messages: 10,
            consolidate_system_messages: false,
            preserve_tool_calls: true,
            max_message_age_seconds: None,
        }
    }
}

impl MemoryConfig {
    /// Create a new memory config with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the maximum messages before consolidation.
    pub fn with_max_messages_before_consolidation(mut self, count: usize) -> Self {
        self.max_messages_before_consolidation = count;
        self
    }

    /// Set the consolidation ratio (0.0 - 1.0).
    pub fn with_consolidation_ratio(mut self, ratio: f64) -> Self {
        assert!((0.0..=1.0).contains(&ratio), "consolidation_ratio must be between 0.0 and 1.0");
        self.consolidation_ratio = ratio;
        self
    }

    /// Set the minimum number of recent messages to keep.
    pub fn with_min_recent_messages(mut self, count: usize) -> Self {
        self.min_recent_messages = count;
        self
    }

    /// Set whether to consolidate system messages.
    pub fn with_consolidate_system_messages(mut self, consolidate: bool) -> Self {
        self.consolidate_system_messages = consolidate;
        self
    }

    /// Set whether to preserve tool calls.
    pub fn with_preserve_tool_calls(mut self, preserve: bool) -> Self {
        self.preserve_tool_calls = preserve;
        self
    }

    /// Set the maximum message age in seconds.
    pub fn with_max_message_age_seconds(mut self, age_seconds: i64) -> Self {
        self.max_message_age_seconds = Some(age_seconds);
        self
    }

    /// Calculate the target message count after consolidation.
    pub fn target_message_count(&self, current_count: usize) -> usize {
        let target = (current_count as f64 * self.consolidation_ratio) as usize;
        target.max(self.min_recent_messages)
    }

    /// Check if consolidation is needed for the given message count.
    pub fn needs_consolidation(&self, message_count: usize) -> bool {
        message_count >= self.max_messages_before_consolidation
    }

    /// Create a conservative config (less aggressive consolidation).
    pub fn conservative() -> Self {
        Self {
            max_messages_before_consolidation: 100,
            consolidation_ratio: 0.5,
            min_recent_messages: 20,
            consolidate_system_messages: false,
            preserve_tool_calls: true,
            max_message_age_seconds: None,
        }
    }

    /// Create an aggressive config (more aggressive consolidation).
    pub fn aggressive() -> Self {
        Self {
            max_messages_before_consolidation: 30,
            consolidation_ratio: 0.2,
            min_recent_messages: 5,
            consolidate_system_messages: true,
            preserve_tool_calls: false,
            max_message_age_seconds: Some(7 * 24 * 3600), // 7 days
        }
    }
}

/// Result of memory consolidation.
#[derive(Debug, Clone)]
pub struct ConsolidationResult {
    /// The consolidated messages
    pub messages: Vec<ConsolidatedMessage>,
    /// Original message count
    pub original_count: usize,
    /// Consolidated message count
    pub consolidated_count: usize,
    /// Number of messages removed
    pub messages_removed: usize,
    /// Summary of removed content
    pub summary: String,
}

/// A consolidated message that may represent multiple original messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidatedMessage {
    /// The message role
    pub role: String,
    /// The message content (may be a summary)
    pub content: String,
    /// Whether this is a consolidated summary
    pub is_summary: bool,
    /// Number of original messages represented by this summary
    pub represents_count: usize,
    /// Timestamp
    pub timestamp: i64,
}

/// Memory consolidator for conversation history.
pub struct MemoryConsolidator {
    config: MemoryConfig,
}

impl Default for MemoryConsolidator {
    fn default() -> Self {
        Self::new(MemoryConfig::default())
    }
}

impl MemoryConsolidator {
    /// Create a new memory consolidator.
    pub fn new(config: MemoryConfig) -> Self {
        Self { config }
    }

    /// Create with default config.
    pub fn with_default_config() -> Self {
        Self::default()
    }

    /// Consolidate a list of messages.
    ///
    /// This returns a new list of messages with old history compressed
    /// into summaries, following the consolidation strategy.
    pub fn consolidate(&self, messages: Vec<ConsolidatedMessage>) -> ConsolidationResult {
        let original_count = messages.len();

        // Fast path: if under threshold, return as-is
        if !self.config.needs_consolidation(original_count) {
            return ConsolidationResult {
                messages,
                original_count,
                consolidated_count: original_count,
                messages_removed: 0,
                summary: "No consolidation needed".to_string(),
            };
        }

        let target_count = self.config.target_message_count(original_count);
        let mut result = Vec::new();
        let mut removed_count = 0;
        let mut summaries = Vec::new();

        // Keep recent messages
        let recent_start = original_count.saturating_sub(target_count);
        let (old_messages, recent_messages) = messages.split_at(recent_start);

        // Process old messages into summaries
        let (kept_old, removed, summary) = self.consolidate_old_messages(old_messages);
        removed_count += removed;
        if !summary.is_empty() {
            summaries.push(summary);
        }

        result.extend(kept_old);
        result.extend(recent_messages.iter().cloned());

        let consolidated_count = result.len();

        ConsolidationResult {
            messages: result,
            original_count,
            consolidated_count,
            messages_removed: removed_count,
            summary: summaries.join("; "),
        }
    }

    /// Consolidate old messages into summaries.
    fn consolidate_old_messages(
        &self,
        messages: &[ConsolidatedMessage],
    ) -> (Vec<ConsolidatedMessage>, usize, String) {
        let mut kept = Vec::new();
        let mut removed = 0;
        let mut summary_parts = Vec::new();

        // Group messages by role for summarization
        let mut by_role: HashMap<String, Vec<&ConsolidatedMessage>> = HashMap::new();

        for msg in messages {
            if !self.config.consolidate_system_messages && msg.role == "system" {
                kept.push(msg.clone());
                continue;
            }

            by_role
                .entry(msg.role.clone())
                .or_default()
                .push(msg);
        }

        // Create summaries for each role group
        for (role, role_messages) in by_role {
            if role_messages.len() <= self.config.min_recent_messages {
                // Keep all messages for this role
                kept.extend(role_messages.into_iter().cloned());
            } else {
                // Consolidate this role group
                removed += role_messages.len();

                let count = role_messages.len();
                let first_timestamp = role_messages.first().map(|m| m.timestamp).unwrap_or(0);
                let last_timestamp = role_messages.last().map(|m| m.timestamp).unwrap_or(0);

                // Create a summary message
                let summary_content = self.create_summary(&role, role_messages);
                summary_parts.push(format!("{}: {} messages", role, count));

                kept.push(ConsolidatedMessage {
                    role: role.clone(),
                    content: summary_content,
                    is_summary: true,
                    represents_count: count,
                    timestamp: (first_timestamp + last_timestamp) / 2,
                });
            }
        }

        let summary = if summary_parts.is_empty() {
            String::new()
        } else {
            format!("Consolidated: {}", summary_parts.join(", "))
        };

        (kept, removed, summary)
    }

    /// Create a summary for a group of messages.
    fn create_summary(&self, role: &str, messages: Vec<&ConsolidatedMessage>) -> String {
        let count = messages.len();

        if role == "user" {
            // Summarize user messages
            let topics: Vec<&str> = messages
                .iter()
                .filter_map(|m| {
                    // Extract first few words as topic hint
                    m.content.split_whitespace().next()
                })
                .collect();

            if topics.len() > 3 {
                format!(
                    "[用户此前问了{}个问题，涉及: {}等话题]",
                    count,
                    topics[..3].join(", ")
                )
            } else {
                format!("[用户此前问了{}个问题]", count)
            }
        } else if role == "assistant" {
            // Summarize assistant messages
            let has_tools = messages.iter().any(|m| m.content.contains("tool"));
            if has_tools {
                format!("[此前提供了{}次回复，包含工具调用]", count)
            } else {
                format!("[此前提供了{}次回复]", count)
            }
        } else {
            format!("[此前有{}条{}消息]", count, role)
        }
    }

    /// Check if a message is too old based on max_message_age_seconds.
    #[allow(dead_code)]
    fn is_message_too_old(&self, timestamp: i64, current_time: i64) -> bool {
        if let Some(max_age) = self.config.max_message_age_seconds {
            let age = current_time - timestamp;
            age > max_age
        } else {
            false
        }
    }

    /// Get the consolidation config.
    pub fn config(&self) -> &MemoryConfig {
        &self.config
    }

    /// Update the consolidation config.
    pub fn set_config(&mut self, config: MemoryConfig) {
        self.config = config;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_message(role: &str, content: &str, timestamp: i64) -> ConsolidatedMessage {
        ConsolidatedMessage {
            role: role.to_string(),
            content: content.to_string(),
            is_summary: false,
            represents_count: 1,
            timestamp,
        }
    }

    #[test]
    fn test_memory_config_default() {
        let config = MemoryConfig::default();
        assert_eq!(config.max_messages_before_consolidation, 50);
        assert_eq!(config.consolidation_ratio, 0.3);
        assert_eq!(config.min_recent_messages, 10);
    }

    #[test]
    fn test_needs_consolidation() {
        let config = MemoryConfig::default()
            .with_max_messages_before_consolidation(10);

        assert!(!config.needs_consolidation(5));
        assert!(config.needs_consolidation(10));
        assert!(config.needs_consolidation(20));
    }

    #[test]
    fn test_target_message_count() {
        let config = MemoryConfig::default()
            .with_consolidation_ratio(0.5)
            .with_min_recent_messages(5);

        assert_eq!(config.target_message_count(100), 50);
        assert_eq!(config.target_message_count(10), 5); // min_recent_messages
    }

    #[test]
    fn test_consolidate_nothing_needed() {
        let consolidator = MemoryConsolidator::default();
        let messages = vec![
            make_message("user", "Hello", 1000),
            make_message("assistant", "Hi there!", 1001),
        ];

        let result = consolidator.consolidate(messages);
        assert_eq!(result.messages_removed, 0);
        assert_eq!(result.consolidated_count, 2);
    }

    #[test]
    fn test_consolidate_creates_summary() {
        let config = MemoryConfig::default()
            .with_max_messages_before_consolidation(5)
            .with_consolidation_ratio(0.4)  // Lower ratio to ensure consolidation happens
            .with_min_recent_messages(1);     // Lower to ensure more consolidation

        let consolidator = MemoryConsolidator::new(config);
        let messages = vec![
            make_message("user", "Message 1", 1000),
            make_message("user", "Message 2", 1001),
            make_message("assistant", "Response 1", 1002),
            make_message("user", "Message 3", 1003),
            make_message("assistant", "Response 2", 1004),
            make_message("user", "Message 4", 1005),
        ];

        let result = consolidator.consolidate(messages);
        assert!(result.messages_removed > 0);
        assert!(result.consolidated_count < 6);
    }

    #[test]
    fn test_conservative_config() {
        let config = MemoryConfig::conservative();
        assert_eq!(config.max_messages_before_consolidation, 100);
        assert_eq!(config.consolidation_ratio, 0.5);
    }

    #[test]
    fn test_aggressive_config() {
        let config = MemoryConfig::aggressive();
        assert_eq!(config.max_messages_before_consolidation, 30);
        assert_eq!(config.consolidation_ratio, 0.2);
        assert!(config.max_message_age_seconds.is_some());
    }
}
