//! Token budget management for context window optimization.
//!
//! This module provides token budget management to ensure the context doesn't overflow
//! while maximizing useful information density. It implements intelligent allocation
//! strategies based on relevance scoring.
//!
//! ## Example
//!
//! ```rust,no_run
//! use neomind_memory::{TokenBudget, ScoredMessage, Priority, PriorityFilter};
//! use neomind_core::llm::token_counter::{TokenCounter, CounterMode};
//!
//! let budget = TokenBudget::new(8000, 2000, 500);
//! let counter = TokenCounter::new(CounterMode::Auto);
//!
//! let messages = vec![
//!     ScoredMessage::with_priority("Important context", 0.9, Priority::High),
//!     ScoredMessage::with_priority("Less relevant", 0.3, Priority::Low),
//! ];
//!
//! let allocation = budget.allocate_messages(
//!     messages,
//!     &counter,
//!     PriorityFilter::MinPriority(Priority::Medium)
//! );
//! ```

use neomind_core::llm::token_counter::TokenCounter;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Token budget manager for context window optimization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBudget {
    /// Total context window size (e.g., 8000 for gpt-4)
    pub total_budget: usize,
    /// Tokens reserved for response generation
    pub reserved_for_response: usize,
    /// Tokens reserved for system prompt
    pub reserved_for_system: usize,
}

impl TokenBudget {
    /// Create a new token budget manager.
    ///
    /// # Arguments
    /// * `total` - Total context window size
    /// * `reserve_response` - Tokens to reserve for response generation
    /// * `reserve_system` - Tokens to reserve for system prompt
    ///
    /// # Example
    /// ```
    /// use neomind_memory::TokenBudget;
    ///
    /// let budget = TokenBudget::new(8000, 2000, 500);
    /// assert_eq!(budget.available_for_history(), 5500);
    /// ```
    pub fn new(total: usize, reserve_response: usize, reserve_system: usize) -> Self {
        Self {
            total_budget: total,
            reserved_for_response: reserve_response,
            reserved_for_system: reserve_system,
        }
    }

    /// Create budget for GPT-4 (8k context).
    pub fn gpt4() -> Self {
        Self::new(8192, 2048, 500)
    }

    /// Create budget for GPT-4-32k (32k context).
    pub fn gpt4_32k() -> Self {
        Self::new(32768, 4096, 500)
    }

    /// Create budget for GPT-3.5-Turbo (4k context).
    pub fn gpt35_turbo() -> Self {
        Self::new(4096, 1024, 500)
    }

    /// Create budget for Claude Opus (200k context).
    pub fn claude_opus() -> Self {
        Self::new(200000, 4096, 500)
    }

    /// Create budget for local models (typically 2k-4k context).
    pub fn local(context_size: usize) -> Self {
        let reserve_response = context_size / 4;
        let reserve_system = 500.min(context_size / 10);
        Self::new(context_size, reserve_response, reserve_system)
    }

    /// Get the available budget for history/conversation.
    ///
    /// # Example
    /// ```
    /// use neomind_memory::TokenBudget;
    ///
    /// let budget = TokenBudget::new(8000, 2000, 500);
    /// assert_eq!(budget.available_for_history(), 5500);
    /// ```
    pub fn available_for_history(&self) -> usize {
        self.total_budget
            .saturating_sub(self.reserved_for_response)
            .saturating_sub(self.reserved_for_system)
    }

    /// Check if estimated tokens exceed available budget.
    ///
    /// # Example
    /// ```
    /// use neomind_memory::TokenBudget;
    ///
    /// let budget = TokenBudget::new(8000, 2000, 500);
    /// assert!(!budget.needs_compaction(5000));
    /// assert!(budget.needs_compaction(6000));
    /// ```
    pub fn needs_compaction(&self, estimated: usize) -> bool {
        estimated > self.available_for_history()
    }

    /// Calculate the maximum tokens available for messages given estimated overhead.
    ///
    /// # Example
    /// ```
    /// use neomind_memory::TokenBudget;
    ///
    /// let budget = TokenBudget::new(8000, 2000, 500);
    /// let max_for_msgs = budget.max_for_messages(100); // 100 tokens overhead
    /// assert_eq!(max_for_msgs, 5400);
    /// ```
    pub fn max_for_messages(&self, overhead_tokens: usize) -> usize {
        self.available_for_history().saturating_sub(overhead_tokens)
    }

    /// Allocate messages within budget using relevance scoring.
    ///
    /// This selects the most relevant messages that fit within the token budget.
    /// Messages with relevance score below 0.15 are filtered out.
    ///
    /// # Arguments
    /// * `messages` - Messages with relevance scores
    /// * `counter` - Token counter for estimating message sizes
    /// * `filter` - Optional priority filter
    ///
    /// # Returns
    /// Allocation result with selected messages and token usage statistics.
    ///
    /// # Example
    /// ```
    /// use neomind_memory::{TokenBudget, ScoredMessage, Priority, PriorityFilter};
    /// use neomind_core::llm::token_counter::{TokenCounter, CounterMode};
    ///
    /// let budget = TokenBudget::new(8000, 2000, 500);
    /// let counter = TokenCounter::new(CounterMode::Auto);
    ///
    /// let messages = vec![
    ///     ScoredMessage {
    ///         content: "Important message".to_string(),
    ///         score: 0.9,
    ///         priority: Priority::High,
    ///         timestamp: 0,
    ///     },
    /// ];
    ///
    /// let allocation = budget.allocate_messages(messages, &counter, PriorityFilter::None);
    /// ```
    pub fn allocate_messages(
        &self,
        messages: Vec<ScoredMessage>,
        counter: &TokenCounter,
        filter: PriorityFilter,
    ) -> Allocation {
        let mut allocated = Vec::new();
        let mut used_tokens = self.reserved_for_system;
        let budget = self.available_for_history();

        // Filter and score messages
        let mut filtered: Vec<_> = messages
            .into_iter()
            .filter(|m| {
                // Apply priority filter
                if let PriorityFilter::MinPriority(min_prio) = filter {
                    if m.priority < min_prio {
                        return false;
                    }
                }
                // Filter out low relevance
                m.score >= 0.15
            })
            .collect();

        // Sort by priority first, then by relevance score
        filtered.sort_by(|a, b| {
            match (a.priority, b.priority) {
                (Priority::Critical, _) | (_, Priority::Critical) => b.priority.cmp(&a.priority),
                _ => {
                    // Within same priority, sort by relevance score
                    b.score
                        .partial_cmp(&a.score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }
            }
        });

        // Greedy allocation: select highest priority/score items that fit
        for msg in filtered {
            let tokens = counter.count(&msg.content);

            // Critical items always included if they fit
            let should_include = if msg.priority == Priority::Critical {
                used_tokens + tokens <= self.total_budget - self.reserved_for_response
            } else {
                used_tokens + tokens <= budget
            };

            if should_include {
                used_tokens += tokens;
                allocated.push(msg);
            }
        }

        let utilization = if budget > 0 {
            used_tokens as f64 / budget as f64
        } else {
            0.0
        };

        Allocation {
            messages: allocated,
            tokens_used: used_tokens,
            tokens_remaining: budget.saturating_sub(used_tokens - self.reserved_for_system),
            utilization,
        }
    }

    /// Get the total budget.
    pub fn total(&self) -> usize {
        self.total_budget
    }

    /// Get the reserved response tokens.
    pub fn reserved_response(&self) -> usize {
        self.reserved_for_response
    }

    /// Get the reserved system tokens.
    pub fn reserved_system(&self) -> usize {
        self.reserved_for_system
    }
}

impl fmt::Display for TokenBudget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TokenBudget(total={}, available={}, reserved_response={}, reserved_system={})",
            self.total_budget,
            self.available_for_history(),
            self.reserved_for_response,
            self.reserved_for_system
        )
    }
}

/// Message with relevance score for budget allocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredMessage {
    /// Message content
    pub content: String,
    /// Relevance score (0.0 - 1.0)
    pub score: f32,
    /// Message priority
    pub priority: Priority,
    /// Timestamp for recency calculation
    pub timestamp: i64,
}

impl ScoredMessage {
    /// Create a new scored message.
    pub fn new(content: impl Into<String>, score: f32) -> Self {
        Self {
            content: content.into(),
            score,
            priority: Priority::Medium,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Create a new scored message with priority.
    pub fn with_priority(content: impl Into<String>, score: f32, priority: Priority) -> Self {
        Self {
            content: content.into(),
            score,
            priority,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Create a new scored message with timestamp.
    pub fn with_timestamp(
        content: impl Into<String>,
        score: f32,
        priority: Priority,
        timestamp: i64,
    ) -> Self {
        Self {
            content: content.into(),
            score,
            priority,
            timestamp,
        }
    }
}

/// Message priority for allocation decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub enum Priority {
    /// Low priority - can be dropped first
    Low = 0,
    /// Medium priority - default
    #[default]
    Medium = 1,
    /// High priority - user messages, important context
    High = 2,
    /// Critical - must always include (system prompts)
    Critical = 3,
}

/// Filter for message allocation.
#[derive(Debug, Clone, Copy)]
pub enum PriorityFilter {
    /// No filtering
    None,
    /// Minimum priority level
    MinPriority(Priority),
}

/// Result of token budget allocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Allocation {
    /// Allocated messages
    pub messages: Vec<ScoredMessage>,
    /// Total tokens used (including reserved)
    pub tokens_used: usize,
    /// Remaining tokens for history
    pub tokens_remaining: usize,
    /// Budget utilization rate (0.0 - 1.0+)
    pub utilization: f64,
}

impl Allocation {
    /// Check if the allocation is over budget.
    pub fn is_over_budget(&self) -> bool {
        self.utilization > 1.0
    }

    /// Get the number of allocated messages.
    pub fn count(&self) -> usize {
        self.messages.len()
    }

    /// Check if allocation is empty.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Get compression ratio (if original size is known).
    ///
    /// Returns None if original_size is 0.
    pub fn compression_ratio(&self, original_size: usize) -> Option<f64> {
        if original_size == 0 {
            return None;
        }
        Some(self.messages.len() as f64 / original_size as f64)
    }
}

impl fmt::Display for Allocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Allocation(messages={}, tokens_used={}, remaining={}, utilization={:.2}%)",
            self.messages.len(),
            self.tokens_used,
            self.tokens_remaining,
            self.utilization * 100.0
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neomind_core::llm::token_counter::CounterMode;

    #[test]
    fn test_budget_creation() {
        let budget = TokenBudget::new(8000, 2000, 500);
        assert_eq!(budget.total(), 8000);
        assert_eq!(budget.available_for_history(), 5500);
        assert_eq!(budget.reserved_response(), 2000);
        assert_eq!(budget.reserved_system(), 500);
    }

    #[test]
    fn test_gpt4_budget() {
        let budget = TokenBudget::gpt4();
        assert_eq!(budget.total(), 8192);
        assert_eq!(budget.available_for_history(), 5644);
    }

    #[test]
    fn test_local_budget() {
        let budget = TokenBudget::local(4096);
        assert_eq!(budget.total(), 4096);
        // 4096 - 1024 (1/4 for response) - 409 (1/10 for system) = 2663
        assert!(budget.available_for_history() > 2000);
    }

    #[test]
    fn test_needs_compaction() {
        let budget = TokenBudget::new(8000, 2000, 500);
        assert!(!budget.needs_compaction(5000));
        assert!(budget.needs_compaction(6000));
    }

    #[test]
    fn test_max_for_messages() {
        let budget = TokenBudget::new(8000, 2000, 500);
        assert_eq!(budget.max_for_messages(100), 5400);
        assert_eq!(budget.max_for_messages(1000), 4500);
    }

    #[test]
    fn test_allocation_within_budget() {
        let budget = TokenBudget::new(8000, 2000, 500);
        let counter = TokenCounter::new(CounterMode::Auto);

        let messages = vec![
            ScoredMessage::with_priority("System prompt", 1.0, Priority::Critical),
            ScoredMessage::with_priority("User message", 0.9, Priority::High),
            ScoredMessage::with_priority("Low relevance", 0.1, Priority::Low),
            ScoredMessage::with_priority("Medium relevance", 0.5, Priority::Medium),
        ];

        let allocation = budget.allocate_messages(messages, &counter, PriorityFilter::None);

        // Critical and high priority should be included
        assert!(
            allocation
                .messages
                .iter()
                .any(|m| m.priority == Priority::Critical)
        );
        assert!(
            allocation
                .messages
                .iter()
                .any(|m| m.priority == Priority::High)
        );

        // Low relevance (below 0.15) should be filtered out
        assert!(!allocation.messages.iter().any(|m| m.score < 0.15));

        // Should not exceed budget
        assert!(!allocation.is_over_budget());
    }

    #[test]
    fn test_allocation_with_priority_filter() {
        let budget = TokenBudget::new(8000, 2000, 500);
        let counter = TokenCounter::new(CounterMode::Auto);

        let messages = vec![
            ScoredMessage::with_priority("High priority", 0.8, Priority::High),
            ScoredMessage::with_priority("Low priority", 0.9, Priority::Low),
        ];

        let allocation = budget.allocate_messages(
            messages,
            &counter,
            PriorityFilter::MinPriority(Priority::Medium),
        );

        // Low priority should be filtered out
        assert_eq!(allocation.messages.len(), 1);
        assert_eq!(allocation.messages[0].priority, Priority::High);
    }

    #[test]
    fn test_scored_message_creation() {
        let msg = ScoredMessage::new("test", 0.5);
        assert_eq!(msg.content, "test");
        assert_eq!(msg.score, 0.5);
        assert_eq!(msg.priority, Priority::Medium);

        let msg_with_prio = ScoredMessage::with_priority("test", 0.5, Priority::High);
        assert_eq!(msg_with_prio.priority, Priority::High);
    }

    #[test]
    fn test_allocation_display() {
        let allocation = Allocation {
            messages: vec![],
            tokens_used: 3000,
            tokens_remaining: 2500,
            utilization: 0.55,
        };

        let display = format!("{}", allocation);
        assert!(display.contains("messages="));
        assert!(display.contains("tokens_used=3000"));
        assert!(display.contains("55.00%"));
    }

    #[test]
    fn test_compression_ratio() {
        let allocation = Allocation {
            messages: vec![ScoredMessage::new("test", 0.5)],
            tokens_used: 1000,
            tokens_remaining: 4500,
            utilization: 0.18,
        };

        assert_eq!(allocation.compression_ratio(10), Some(0.1));
        assert_eq!(allocation.compression_ratio(0), None);
    }
}
