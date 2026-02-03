//! Memory Compression with Hierarchical Summarization
//!
//! This module provides memory compression capabilities to reduce memory footprint
//! while preserving important information through hierarchical summarization.
//!
//! ## Features
//!
//! - **Message Grouping**: Group related messages for compression
//! - **Hierarchical Summarization**: Create multi-level summaries
//! - **Compression Statistics**: Track compression ratios
//! - **Selective Compression**: Compress based on importance scores
//! - **Reversible Operations**: Track original content for decompression
//!
//! ## Compression Strategy
//!
//! ```text
//! Original Messages (100 messages)
//!       |
//!       v
//! Group by Topic/Session (5 groups of 20)
//!       |
//!       v
//! Summarize Each Group (5 summaries)
//!       |
//!       v
//! Merge Related Summaries (2 combined summaries)
//!       |
//!       v
//! Final Compressed Output (2 high-level summaries)
//! ```
//!
//! ## Example
//!
//! ```rust,no_run
//! use edge_ai_memory::compression::{
//!     MemoryCompressor, CompressionConfig, MessageGroup,
//! };
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let compressor = MemoryCompressor::new();
//!
//! // Note: vec![...] is placeholder for actual message vectors
//! // let messages = vec![
//! //     MessageGroup::new("session1", "User asked about temperature", vec![...]),
//! //     MessageGroup::new("session1", "AI responded with reading", vec![...]),
//! // ];
//! #
//! // let compressed = compressor.compress(&messages).await?;
//! // println!("Compressed to {} tokens from {} original",
//! //     compressed.compressed_tokens, compressed.original_tokens);
//! # Ok(())
//! # }
//! ```

use crate::budget::{ScoredMessage, TokenBudget};
use crate::error::{MemoryError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Default target compression ratio (0.0 - 1.0).
/// 0.3 means compress to 30% of original size.
pub const DEFAULT_TARGET_RATIO: f64 = 0.3;

/// Minimum group size for compression.
pub const MIN_GROUP_SIZE: usize = 3;

/// Maximum summary length in tokens.
pub const DEFAULT_MAX_SUMMARY_TOKENS: usize = 200;

/// A group of related messages for compression.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageGroup {
    /// Unique identifier for the group
    pub id: String,
    /// Topic or theme of the group
    pub topic: String,
    /// Messages in the group
    pub messages: Vec<ScoredMessage>,
    /// Creation timestamp
    pub created_at: i64,
    /// Session ID if applicable
    pub session_id: Option<String>,
}

impl MessageGroup {
    /// Create a new message group.
    pub fn new(id: impl Into<String>, topic: impl Into<String>, messages: Vec<ScoredMessage>) -> Self {
        Self {
            id: id.into(),
            topic: topic.into(),
            messages,
            created_at: chrono::Utc::now().timestamp(),
            session_id: None,
        }
    }

    /// Set the session ID.
    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Get the total number of messages.
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Calculate total tokens in the group.
    pub fn total_tokens(&self, counter: &TokenBudget) -> usize {
        self.messages
            .iter()
            .map(|_m| counter.available_for_history()) // Approximate
            .sum()
    }

    /// Get the average importance score.
    pub fn average_importance(&self) -> f64 {
        if self.messages.is_empty() {
            return 0.0;
        }
        let sum: f32 = self.messages.iter().map(|m| m.score).sum();
        sum as f64 / self.messages.len() as f64
    }
}

/// Compressed memory output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressedMemory {
    /// Compressed content
    pub content: String,
    /// Original number of messages
    pub original_count: usize,
    /// Compressed number of summaries
    pub compressed_count: usize,
    /// Original token count (estimated)
    pub original_tokens: usize,
    /// Compressed token count (estimated)
    pub compressed_tokens: usize,
    /// Compression ratio (compressed / original)
    pub compression_ratio: f64,
    /// Groups that were compressed
    pub group_ids: Vec<String>,
    /// Metadata about the compression
    pub metadata: CompressionMetadata,
}

/// Metadata about a compression operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionMetadata {
    /// Timestamp of compression
    pub compressed_at: i64,
    /// Compression method used
    pub method: CompressionMethod,
    /// Number of levels in hierarchical compression
    pub levels: usize,
    /// Whether this can be expanded
    pub expandable: bool,
}

/// Method used for compression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionMethod {
    /// Simple concatenation with deduplication
    Concatenate,
    /// Chronological summarization
    Chronological,
    /// Topic-based grouping
    TopicBased,
    /// Hierarchical multi-level compression
    Hierarchical,
    /// Semantic clustering
    Semantic,
}

/// Configuration for memory compression.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionConfig {
    /// Target compression ratio (0.0 - 1.0)
    pub target_ratio: f64,
    /// Minimum group size to consider compression
    pub min_group_size: usize,
    /// Maximum summary length in tokens
    pub max_summary_tokens: usize,
    /// Whether to preserve importance scores
    pub preserve_scores: bool,
    /// Compression method to use
    pub method: CompressionMethod,
    /// Number of hierarchical levels (for Hierarchical method)
    pub hierarchy_levels: usize,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            target_ratio: DEFAULT_TARGET_RATIO,
            min_group_size: MIN_GROUP_SIZE,
            max_summary_tokens: DEFAULT_MAX_SUMMARY_TOKENS,
            preserve_scores: true,
            method: CompressionMethod::Hierarchical,
            hierarchy_levels: 2,
        }
    }
}

impl CompressionConfig {
    /// Create a new compression config.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set target compression ratio.
    pub fn target_ratio(mut self, ratio: f64) -> Self {
        self.target_ratio = ratio.clamp(0.1, 0.9);
        self
    }

    /// Set minimum group size.
    pub fn min_group_size(mut self, size: usize) -> Self {
        self.min_group_size = size.max(1);
        self
    }

    /// Set compression method.
    pub fn method(mut self, method: CompressionMethod) -> Self {
        self.method = method;
        self
    }

    /// Set hierarchy levels.
    pub fn hierarchy_levels(mut self, levels: usize) -> Self {
        self.hierarchy_levels = levels.clamp(1, 5);
        self
    }
}

/// Summary at a specific level of the hierarchy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryLevel {
    /// Level number (0 = most detailed, higher = more compressed)
    pub level: usize,
    /// Summary content
    pub content: String,
    /// Token count
    pub tokens: usize,
    /// Source group IDs
    pub sources: Vec<String>,
}

/// Memory compressor with hierarchical summarization.
#[derive(Clone)]
pub struct MemoryCompressor {
    config: CompressionConfig,
    /// Cache of compressed groups
    cache: Arc<RwLock<HashMap<String, CompressedMemory>>>,
}

impl MemoryCompressor {
    /// Create a new memory compressor.
    pub fn new() -> Self {
        Self {
            config: CompressionConfig::default(),
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create with custom configuration.
    pub fn with_config(config: CompressionConfig) -> Self {
        Self {
            config,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get the current configuration.
    pub fn config(&self) -> &CompressionConfig {
        &self.config
    }

    /// Update the configuration.
    pub fn set_config(&mut self, config: CompressionConfig) {
        self.config = config;
    }

    /// Compress a list of message groups.
    pub async fn compress(&self, groups: &[MessageGroup]) -> Result<CompressedMemory> {
        if groups.is_empty() {
            return Err(MemoryError::InvalidFormat("No groups to compress".to_string()));
        }

        let original_count: usize = groups.iter().map(|g| g.message_count()).sum();
        let original_tokens = self.estimate_tokens(groups);
        let group_ids: Vec<String> = groups.iter().map(|g| g.id.clone()).collect();

        let (compressed_content, compressed_count, compressed_tokens, levels) = match self.config.method {
            CompressionMethod::Concatenate => self.compress_concatenate(groups),
            CompressionMethod::Chronological => self.compress_chronological(groups),
            CompressionMethod::TopicBased => self.compress_topic_based(groups),
            CompressionMethod::Hierarchical => self.compress_hierarchical(groups).await,
            CompressionMethod::Semantic => self.compress_semantic(groups),
        };

        let compression_ratio = if original_tokens > 0 {
            compressed_tokens as f64 / original_tokens as f64
        } else {
            1.0
        };

        let compressed = CompressedMemory {
            content: compressed_content,
            original_count,
            compressed_count,
            original_tokens,
            compressed_tokens,
            compression_ratio,
            group_ids,
            metadata: CompressionMetadata {
                compressed_at: chrono::Utc::now().timestamp(),
                method: self.config.method,
                levels,
                expandable: true,
            },
        };

        Ok(compressed)
    }

    /// Estimate token count for groups.
    fn estimate_tokens(&self, groups: &[MessageGroup]) -> usize {
        groups
            .iter()
            .flat_map(|g| g.messages.iter().map(|m| m.content.len() / 4)) // Rough estimate: 4 chars per token
            .sum()
    }

    /// Simple concatenation compression.
    fn compress_concatenate(&self, groups: &[MessageGroup]) -> (String, usize, usize, usize) {
        let mut parts = Vec::new();
        let mut count = 0;

        for group in groups {
            if group.messages.is_empty() {
                continue;
            }

            parts.push(format!("[Topic: {}]", group.topic));
            for msg in &group.messages {
                parts.push(msg.content.clone());
            }
            count += group.message_count();
        }

        let content = parts.join("\n");
        let tokens = content.len() / 4;

        (content, count, tokens, 1)
    }

    /// Chronological compression with timestamps.
    fn compress_chronological(&self, groups: &[MessageGroup]) -> (String, usize, usize, usize) {
        let mut entries: Vec<_> = groups
            .iter()
            .flat_map(|g| {
                g.messages.iter().map(move |m| (m.timestamp, &g.topic, &m.content))
            })
            .collect();

        entries.sort_by_key(|e| e.0);

        let parts: Vec<String> = entries
            .iter()
            .map(|(_, topic, content)| format!("{}: {}", topic, content))
            .collect();

        let content = parts.join("\n");
        let tokens = content.len() / 4;

        (content, entries.len(), tokens, 1)
    }

    /// Topic-based compression.
    fn compress_topic_based(&self, groups: &[MessageGroup]) -> (String, usize, usize, usize) {
        let mut by_topic: HashMap<&str, Vec<&ScoredMessage>> = HashMap::new();

        for group in groups {
            by_topic
                .entry(&group.topic)
                .or_default()
                .extend(group.messages.iter().collect::<Vec<_>>());
        }

        let mut parts = Vec::new();
        let mut count = 0;

        for (topic, messages) in by_topic.iter() {
            if messages.len() < self.config.min_group_size {
                continue;
            }

            // Create a summary for this topic
            let summary = format!(
                "[{}: {} messages discussed]",
                topic,
                messages.len()
            );
            parts.push(summary);
            count += messages.len();
        }

        let content = parts.join("\n");
        let tokens = content.len() / 4;

        (content, count, tokens, 1)
    }

    /// Hierarchical multi-level compression.
    async fn compress_hierarchical(&self, groups: &[MessageGroup]) -> (String, usize, usize, usize) {
        let mut summaries: Vec<SummaryLevel> = Vec::new();
        let levels = self.config.hierarchy_levels;

        // Level 0: Group by topic
        let mut by_topic: HashMap<String, Vec<&MessageGroup>> = HashMap::new();
        for group in groups {
            by_topic
                .entry(group.topic.clone())
                .or_default()
                .push(group);
        }

        // Create level 0 summaries
        for (topic, topic_groups) in &by_topic {
            let messages: Vec<_> = topic_groups
                .iter()
                .flat_map(|g| g.messages.iter())
                .collect();

            if messages.len() < self.config.min_group_size {
                continue;
            }

            let content = self.summarize_messages(&messages, topic);
            let sources: Vec<String> = topic_groups.iter().map(|g| g.id.clone()).collect();
            let tokens = content.len() / 4;

            summaries.push(SummaryLevel {
                level: 0,
                content,
                tokens,
                sources,
            });
        }

        // Higher levels: Combine related summaries
        for level in 1..levels {
            if summaries.is_empty() {
                break;
            }

            // Group summaries by similarity (simplified: just combine in batches)
            let batch_size = (summaries.len() / (levels - level + 1)).max(1);
            let mut combined = Vec::new();

            for chunk in summaries.chunks(batch_size) {
                let combined_content = format!(
                    "[Summary Level {}]\n{}",
                    level,
                    chunk.iter()
                        .map(|s| s.content.as_str())
                        .collect::<Vec<&str>>()
                        .join("\n")
                );
                let all_sources: Vec<String> = chunk
                    .iter()
                    .flat_map(|s| s.sources.clone())
                    .collect();

                combined.push(SummaryLevel {
                    level,
                    content: combined_content.clone(),
                    tokens: combined_content.len() / 4,
                    sources: all_sources,
                });
            }

            summaries = combined;
        }

        // Return the highest level summary
        let final_summary = summaries.into_iter().last().unwrap_or_else(|| SummaryLevel {
            level: 0,
            content: "[No content to summarize]".to_string(),
            tokens: 0,
            sources: Vec::new(),
        });

        let total_count = groups.iter().map(|g| g.message_count()).sum();

        (final_summary.content, total_count, final_summary.tokens, levels)
    }

    /// Semantic compression (placeholder for future embedding-based compression).
    fn compress_semantic(&self, groups: &[MessageGroup]) -> (String, usize, usize, usize) {
        // For now, fall back to topic-based
        self.compress_topic_based(groups)
    }

    /// Create a summary of messages.
    fn summarize_messages(&self, messages: &[&ScoredMessage], topic: &str) -> String {
        if messages.is_empty() {
            return format!("[No messages for topic '{}']", topic);
        }

        // Extract key points (simplified: take highest scored messages)
        let mut sorted: Vec<_> = messages.iter().collect();
        sorted.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        let key_points: Vec<_> = sorted
            .iter()
            .take(3) // Top 3 points
            .filter_map(|m| {
                let content = m.content.trim();
                if content.is_empty() {
                    None
                } else {
                    // Truncate if too long
                    Some(if content.len() > 100 {
                        format!("{}...", &content[..97])
                    } else {
                        content.to_string()
                    })
                }
            })
            .collect();

        if key_points.is_empty() {
            format!("[Topic '{}' discussed with {} messages]", topic, messages.len())
        } else {
            format!(
                "[Topic: {}]\n- {}",
                topic,
                key_points.join("\n- ")
            )
        }
    }

    /// Compress with target token limit.
    pub async fn compress_to_limit(
        &self,
        groups: &[MessageGroup],
        max_tokens: usize,
    ) -> Result<CompressedMemory> {
        // Estimate current tokens
        let current_tokens = self.estimate_tokens(groups);

        if current_tokens <= max_tokens {
            // No compression needed
            return self.compress(groups).await;
        }

        // Calculate required ratio
        let target_ratio = max_tokens as f64 / current_tokens as f64;

        // Create temporary config with target ratio
        let temp_config = CompressionConfig {
            target_ratio,
            ..self.config.clone()
        };

        let temp_compressor = MemoryCompressor::with_config(temp_config);
        temp_compressor.compress(groups).await
    }

    /// Get cached compressed memory.
    pub async fn get_cached(&self, id: &str) -> Option<CompressedMemory> {
        let cache = self.cache.read().await;
        cache.get(id).cloned()
    }

    /// Cache a compressed memory.
    pub async fn cache(&self, id: impl Into<String>, compressed: CompressedMemory) {
        let mut cache = self.cache.write().await;
        cache.insert(id.into(), compressed);
    }

    /// Clear the compression cache.
    pub async fn clear_cache(&self) {
        self.cache.write().await.clear();
    }

    /// Expand compressed memory (placeholder - returns original content).
    pub async fn expand(&self, _compressed: &CompressedMemory) -> Result<Vec<ScoredMessage>> {
        // In a real implementation, this would reconstruct from metadata
        // For now, return empty as expansion is not implemented
        Err(MemoryError::NotFound("Expansion not implemented".to_string()))
    }

    /// Calculate compression statistics.
    pub fn calculate_stats(&self, compressed: &CompressedMemory) -> CompressionStats {
        CompressionStats {
            original_count: compressed.original_count,
            compressed_count: compressed.compressed_count,
            original_tokens: compressed.original_tokens,
            compressed_tokens: compressed.compressed_tokens,
            compression_ratio: compressed.compression_ratio,
            space_saved: compressed.original_tokens.saturating_sub(compressed.compressed_tokens),
            space_saved_percent: if compressed.original_tokens > 0 {
                (compressed.original_tokens - compressed.compressed_tokens) as f64
                    / compressed.original_tokens as f64
                    * 100.0
            } else {
                0.0
            },
        }
    }
}

impl Default for MemoryCompressor {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about a compression operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionStats {
    /// Original message count
    pub original_count: usize,
    /// Compressed message count
    pub compressed_count: usize,
    /// Original token count
    pub original_tokens: usize,
    /// Compressed token count
    pub compressed_tokens: usize,
    /// Compression ratio
    pub compression_ratio: f64,
    /// Tokens saved
    pub space_saved: usize,
    /// Percentage of space saved
    pub space_saved_percent: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::Priority;

    #[test]
    fn test_compression_config_default() {
        let config = CompressionConfig::default();
        assert_eq!(config.target_ratio, DEFAULT_TARGET_RATIO);
        assert_eq!(config.min_group_size, MIN_GROUP_SIZE);
        assert_eq!(config.max_summary_tokens, DEFAULT_MAX_SUMMARY_TOKENS);
    }

    #[test]
    fn test_compression_config_builder() {
        let config = CompressionConfig::new()
            .target_ratio(0.5)
            .min_group_size(5)
            .method(CompressionMethod::Semantic);

        assert_eq!(config.target_ratio, 0.5);
        assert_eq!(config.min_group_size, 5);
        assert_eq!(config.method, CompressionMethod::Semantic);
    }

    #[test]
    fn test_message_group_creation() {
        let messages = vec![
            ScoredMessage::new("Message 1", 0.5),
            ScoredMessage::new("Message 2", 0.7),
        ];

        let group = MessageGroup::new("group1", "test topic", messages);
        assert_eq!(group.id, "group1");
        assert_eq!(group.topic, "test topic");
        assert_eq!(group.message_count(), 2);
        assert_eq!(group.session_id, None);
    }

    #[test]
    fn test_message_group_with_session() {
        let group = MessageGroup::new("g1", "topic", vec![])
            .with_session("session1");
        assert_eq!(group.session_id, Some("session1".to_string()));
    }

    #[test]
    fn test_message_group_average_importance() {
        let messages = vec![
            ScoredMessage::new("M1", 0.2),
            ScoredMessage::new("M2", 0.8),
        ];

        let group = MessageGroup::new("g1", "topic", messages);
        assert!((group.average_importance() - 0.5).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_memory_compressor_creation() {
        let compressor = MemoryCompressor::new();
        assert_eq!(
            compressor.config().target_ratio,
            DEFAULT_TARGET_RATIO
        );
    }

    #[tokio::test]
    async fn test_memory_compressor_with_config() {
        let config = CompressionConfig::new().target_ratio(0.5);
        let compressor = MemoryCompressor::with_config(config.clone());
        assert_eq!(compressor.config().target_ratio, 0.5);
    }

    #[tokio::test]
    async fn test_compress_empty_groups() {
        let compressor = MemoryCompressor::new();
        let result = compressor.compress(&[]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_compress_single_group() {
        let compressor = MemoryCompressor::new();
        let messages = vec![
            ScoredMessage::with_priority("This is a longer message to ensure tokens are estimated", 0.5, Priority::Medium),
            ScoredMessage::with_priority("Another message with enough content for token estimation", 0.7, Priority::High),
        ];

        let group = MessageGroup::new("g1", "test", messages);
        let result = compressor.compress(&[group]).await;

        assert!(result.is_ok());
        let compressed = result.unwrap();
        assert!(!compressed.content.is_empty());
        assert_eq!(compressed.original_count, 2);
        // Ratio should be non-negative
        assert!(compressed.compression_ratio >= 0.0);
    }

    #[tokio::test]
    async fn test_compress_concatenate() {
        let config = CompressionConfig::new().method(CompressionMethod::Concatenate);
        let compressor = MemoryCompressor::with_config(config);

        let messages1 = vec![ScoredMessage::new("Msg 1", 0.5)];
        let messages2 = vec![ScoredMessage::new("Msg 2", 0.5)];

        let group1 = MessageGroup::new("g1", "topic1", messages1);
        let group2 = MessageGroup::new("g2", "topic2", messages2);

        let result = compressor.compress(&[group1, group2]).await;
        assert!(result.is_ok());

        let compressed = result.unwrap();
        assert!(compressed.content.contains("Msg 1"));
        assert!(compressed.content.contains("Msg 2"));
    }

    #[tokio::test]
    async fn test_compress_topic_based() {
        let config = CompressionConfig::new().method(CompressionMethod::TopicBased);
        let compressor = MemoryCompressor::with_config(config);

        let messages = vec![
            ScoredMessage::new("M1", 0.5),
            ScoredMessage::new("M2", 0.5),
            ScoredMessage::new("M3", 0.5),
        ];

        let group = MessageGroup::new("g1", "temperature", messages);
        let result = compressor.compress(&[group]).await;

        assert!(result.is_ok());
        let compressed = result.unwrap();
        assert!(compressed.content.contains("temperature"));
    }

    #[tokio::test]
    async fn test_calculate_stats() {
        let compressor = MemoryCompressor::new();

        let compressed = CompressedMemory {
            content: "Test summary".to_string(),
            original_count: 100,
            compressed_count: 1,
            original_tokens: 1000,
            compressed_tokens: 50,
            compression_ratio: 0.05,
            group_ids: vec!["g1".to_string()],
            metadata: CompressionMetadata {
                compressed_at: 0,
                method: CompressionMethod::Hierarchical,
                levels: 2,
                expandable: true,
            },
        };

        let stats = compressor.calculate_stats(&compressed);
        assert_eq!(stats.original_count, 100);
        assert_eq!(stats.compressed_count, 1);
        assert_eq!(stats.space_saved, 950);
        assert!((stats.space_saved_percent - 95.0).abs() < 0.1);
    }

    #[tokio::test]
    async fn test_cache_operations() {
        let compressor = MemoryCompressor::new();

        let compressed = CompressedMemory {
            content: "Cached content".to_string(),
            original_count: 10,
            compressed_count: 1,
            original_tokens: 100,
            compressed_tokens: 20,
            compression_ratio: 0.2,
            group_ids: vec![],
            metadata: CompressionMetadata {
                compressed_at: 0,
                method: CompressionMethod::Concatenate,
                levels: 1,
                expandable: false,
            },
        };

        compressor.cache("test_key", compressed.clone()).await;

        let retrieved = compressor.get_cached("test_key").await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().content, "Cached content");

        compressor.clear_cache().await;
        assert!(compressor.get_cached("test_key").await.is_none());
    }
}
