//! Context Health Monitoring (P3.1)
//!
//! This module provides health metrics for detecting context degradation
//! and context rot in long-running agent conversations.
//!
//! ## Health Metrics
//!
//! - **Completeness**: Are critical entities still present in context?
//! - **Recency**: How recent is the context on average?
//! - **Diversity**: Entropy of topics in the conversation
//! - **Redundancy**: Ratio of duplicate/similar content
//! - **Overall Score**: Weighted composite (0-1)
//!
//! ## Example
//!
//! ```rust,no_run
//! use neomind_agent::context::{ContextHealth, calculate_health};
//! use neomind_agent::AgentMessage;
//!
//! # fn example(messages: Vec<AgentMessage>) {
//! let health = calculate_health(&messages);
//! if health.overall_score < 0.6 {
//!     println!("Context degraded, refresh needed");
//! }
//! # }
//! ```

use crate::agent::AgentMessage;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Context health metrics (0-1 scale).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextHealth {
    /// Completeness: Are critical entities present?
    /// 1.0 = all referenced entities in context, 0.0 = missing entities
    pub completeness: f64,

    /// Recency: How recent is the context?
    /// 1.0 = very recent, 0.0 = very old
    pub recency: f64,

    /// Diversity: Topic variety in conversation
    /// 1.0 = high diversity, 0.0 = repetitive
    pub diversity: f64,

    /// Redundancy: Duplicate content ratio
    /// 1.0 = no redundancy, 0.0 = highly redundant
    pub redundancy: f64,

    /// Overall health score (weighted composite)
    /// 0-1 scale, <0.6 indicates degraded context
    pub overall_score: f64,

    /// Health status category
    pub status: HealthStatus,

    /// Timestamp when health was calculated
    pub calculated_at: i64,
}

/// Health status category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    /// Context is healthy (score >= 0.8)
    Excellent,
    /// Context is good (score >= 0.6)
    Good,
    /// Context is degraded (score >= 0.4)
    Degraded,
    /// Context is critical (score < 0.4)
    Critical,
}

impl HealthStatus {
    /// Create from score.
    fn from_score(score: f64) -> Self {
        if score >= 0.8 {
            HealthStatus::Excellent
        } else if score >= 0.6 {
            HealthStatus::Good
        } else if score >= 0.4 {
            HealthStatus::Degraded
        } else {
            HealthStatus::Critical
        }
    }
}

/// Health check configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckConfig {
    /// Weight for completeness metric
    pub completeness_weight: f64,
    /// Weight for recency metric
    pub recency_weight: f64,
    /// Weight for diversity metric
    pub diversity_weight: f64,
    /// Weight for redundancy metric
    pub redundancy_weight: f64,
    /// Threshold for "healthy" context
    pub healthy_threshold: f64,
    /// Maximum message age for full recency score (seconds)
    pub max_message_age: i64,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            completeness_weight: 0.3,
            recency_weight: 0.25,
            diversity_weight: 0.2,
            redundancy_weight: 0.25,
            healthy_threshold: 0.6,
            max_message_age: 3600, // 1 hour
        }
    }
}

/// Calculate context health metrics for a message list.
pub fn calculate_health(messages: &[AgentMessage]) -> ContextHealth {
    calculate_health_with_config(messages, &HealthCheckConfig::default())
}

/// Calculate context health with custom configuration.
pub fn calculate_health_with_config(
    messages: &[AgentMessage],
    config: &HealthCheckConfig,
) -> ContextHealth {
    let now = chrono::Utc::now().timestamp();

    let completeness = calculate_completeness(messages);
    let recency = calculate_recency(messages, now, config.max_message_age);
    let diversity = calculate_diversity(messages);
    let redundancy = calculate_redundancy(messages);

    // Weighted overall score
    let overall_score = (completeness * config.completeness_weight
        + recency * config.recency_weight
        + diversity * config.diversity_weight
        + redundancy * config.redundancy_weight)
        .clamp(0.0, 1.0);

    let status = HealthStatus::from_score(overall_score);

    ContextHealth {
        completeness,
        recency,
        diversity,
        redundancy,
        overall_score,
        status,
        calculated_at: now,
    }
}

/// Calculate completeness: check if referenced entities are in context.
fn calculate_completeness(messages: &[AgentMessage]) -> f64 {
    if messages.is_empty() {
        return 1.0;
    }

    // Count unique entities mentioned
    let mut entity_keywords = Vec::new();
    for msg in messages {
        let content = msg.content.to_lowercase();
        // Common entity indicators in Chinese/English
        if content.contains("设备") || content.contains("device") {
            entity_keywords.push("device");
        }
        if content.contains("规则") || content.contains("rule") {
            entity_keywords.push("rule");
        }
        if content.contains("智能体") || content.contains("agent") {
            entity_keywords.push("agent");
        }
        if content.contains("房间") || content.contains("客厅") || content.contains("卧室") {
            entity_keywords.push("location");
        }
    }

    // High completeness if entities are mentioned in recent messages
    let recent_has_entities = messages
        .iter()
        .rev()
        .take(5)
        .any(|msg| {
            let content = msg.content.to_lowercase();
            entity_keywords.iter().any(|kw| content.contains(kw))
        });

    if recent_has_entities || entity_keywords.is_empty() {
        1.0
    } else {
        // Entities were mentioned but not recently
        0.7
    }
}

/// Calculate recency: average age of messages.
fn calculate_recency(messages: &[AgentMessage], now: i64, max_age: i64) -> f64 {
    if messages.is_empty() {
        return 1.0;
    }

    // Calculate average age as ratio of max_age
    let total_age: i64 = messages
        .iter()
        .map(|msg| (now - msg.timestamp).clamp(0, max_age))
        .sum();

    let avg_age = total_age as f64 / messages.len() as f64;
    let max_age_f = max_age as f64;

    // Recency is inverse of age ratio
    1.0 - (avg_age / max_age_f).min(1.0)
}

/// Calculate diversity: topic variety in conversation.
fn calculate_diversity(messages: &[AgentMessage]) -> f64 {
    if messages.len() < 3 {
        return 1.0; // Too few messages to judge
    }

    // Count topic keywords
    let mut topics = HashMap::new();
    for msg in messages {
        let content = msg.content.to_lowercase();

        if content.contains("温度") || content.contains("temperature") {
            *topics.entry("temperature").or_insert(0) += 1;
        }
        if content.contains("灯") || content.contains("light") {
            *topics.entry("lighting").or_insert(0) += 1;
        }
        if content.contains("湿度") || content.contains("humidity") {
            *topics.entry("humidity").or_insert(0) += 1;
        }
        if content.contains("创建") || content.contains("create") {
            *topics.entry("creation").or_insert(0) += 1;
        }
        if content.contains("查询") || content.contains("query") {
            *topics.entry("query").or_insert(0) += 1;
        }
    }

    if topics.is_empty() {
        return 0.5; // Neutral
    }

    // Diversity based on topic count vs message count
    let topic_count = topics.len() as f64;
    let msg_count = messages.len() as f64;

    // More topics relative to messages = higher diversity
    (topic_count / msg_count).min(1.0) * 2.0
}

/// Calculate redundancy: duplicate content detection.
fn calculate_redundancy(messages: &[AgentMessage]) -> f64 {
    if messages.len() < 2 {
        return 1.0; // No redundancy with single message
    }

    let mut redundancy_count = 0;

    // Check for consecutive similar messages
    for i in 0..messages.len().saturating_sub(1) {
        let curr = &messages[i].content.to_lowercase();
        let next = &messages[i + 1].content.to_lowercase();

        // Check for similarity
        if curr.len() > 10 && next.len() > 10 {
            let similarity = calculate_similarity(curr, next);
            if similarity > 0.8 {
                redundancy_count += 1;
            }
        }
    }

    // Redundancy score: 1.0 = no redundancy, 0.0 = high redundancy
    let redundancy_ratio = redundancy_count as f64 / messages.len() as f64;
    1.0 - (redundancy_ratio * 2.0).min(1.0)
}

/// Calculate string similarity (0-1).
fn calculate_similarity(a: &str, b: &str) -> f64 {
    let len_a = a.chars().count();
    let len_b = b.chars().count();
    let max_len = len_a.max(len_b);

    if max_len == 0 {
        return 1.0;
    }

    // Simple character overlap
    let mut common = 0;
    for ch in a.chars() {
        if b.contains(ch) {
            common += 1;
        }
    }

    common as f64 / max_len as f64
}

impl ContextHealth {
    /// Check if context is healthy.
    pub fn is_healthy(&self) -> bool {
        self.overall_score >= 0.6
    }

    /// Check if context needs refresh.
    pub fn needs_refresh(&self) -> bool {
        self.overall_score < 0.6 || self.status == HealthStatus::Critical
    }

    /// Get a summary description of the health status.
    pub fn summary(&self) -> String {
        match self.status {
            HealthStatus::Excellent => "Context is excellent".to_string(),
            HealthStatus::Good => {
                format!("Context is good (score: {:.2})", self.overall_score)
            }
            HealthStatus::Degraded => {
                format!("Context is degraded (score: {:.2}), consider refresh", self.overall_score)
            }
            HealthStatus::Critical => {
                format!("Context is critical (score: {:.2}), refresh recommended", self.overall_score)
            }
        }
    }

    /// Get recommendation for improving context health.
    pub fn recommendation(&self) -> Option<&'static str> {
        if self.completeness < 0.5 {
            Some("Re-inject entity references into context")
        } else if self.recency < 0.4 {
            Some("Refresh context with recent messages")
        } else if self.diversity < 0.4 {
            Some("Introduce new topics to reduce repetition")
        } else if self.redundancy < 0.5 {
            Some("Summarize redundant messages")
        } else if self.overall_score < 0.6 {
            Some("Consider context compression or refresh")
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_message(role: &str, content: &str, timestamp: i64) -> AgentMessage {
        AgentMessage {
            role: role.to_string(),
            content: content.to_string(),
            tool_calls: None,
            tool_call_id: None,
            tool_call_name: None,
            thinking: None,
            images: None,
            timestamp,
        }
    }

    #[test]
    fn test_health_calculation() {
        let messages = vec![
            make_message("user", "Check temperature", 1000),
            make_message("assistant", "Temperature is 22°C", 1001),
        ];

        let health = calculate_health(&messages);
        assert!(health.overall_score > 0.5);
    }

    #[test]
    fn test_health_status_from_score() {
        assert_eq!(HealthStatus::from_score(0.9), HealthStatus::Excellent);
        assert_eq!(HealthStatus::from_score(0.7), HealthStatus::Good);
        assert_eq!(HealthStatus::from_score(0.5), HealthStatus::Degraded);
        assert_eq!(HealthStatus::from_score(0.2), HealthStatus::Critical);
    }

    #[test]
    fn test_health_is_healthy() {
        let mut health = ContextHealth {
            completeness: 0.8,
            recency: 0.8,
            diversity: 0.8,
            redundancy: 0.8,
            overall_score: 0.8,
            status: HealthStatus::Excellent,
            calculated_at: 0,
        };

        assert!(health.is_healthy());

        health.overall_score = 0.5;
        health.status = HealthStatus::Degraded;
        assert!(!health.is_healthy());
    }

    #[test]
    fn test_redundancy_detection() {
        let messages = vec![
            make_message("user", "Check temperature", 1000),
            make_message("assistant", "Temperature is 22°C", 1001),
            make_message("assistant", "Temperature is 22°C", 1002), // Duplicate
        ];

        let health = calculate_health(&messages);
        assert!(health.redundancy < 1.0); // Should detect redundancy
    }
}
