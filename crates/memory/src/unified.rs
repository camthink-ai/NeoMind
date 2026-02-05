//! Unified Memory Interface
//!
//! This module provides a unified interface to all memory layers (short-term,
//! mid-term, and long-term) with automatic data promotion and transparent
//! query routing.
//!
//! ## Architecture
//!
//! The unified memory interface provides:
//! - **Single Entry Point**: One API for all memory operations
//! - **Automatic Promotion**: Data automatically moves between layers
//! - **Intelligent Routing**: Queries route to the most appropriate layer
//! - **Transparent Fallback**: Fall through to lower layers if needed
//!
//! ## Data Flow
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     Unified Memory                           │
//! ├─────────────────────────────────────────────────────────────┤
//! │  Query ──► Relevance Check ──► Layer Selection ──► Result   │
//!     │                                                │
//!     ▼                                                ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │  Promotion/Demotion                                          │
//! │  - Short-term ──► Mid-term (on consolidate)                 │
//! │  - Mid-term ──► Long-term (on importance threshold)         │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Example
//!
//! ```rust,no_run
//! use edge_ai_memory::{UnifiedMemory, MemoryQuery};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let memory = UnifiedMemory::new();
//!
//!     // Add to memory (automatically routed to short-term)
//!     memory.add("user", "Hello").await?;
//!
//!     // Query across all layers
//!     let results = memory.query(MemoryQuery::new("greeting")).await?;
//!
//!     // Consolidate short-term to mid-term
//!     memory.consolidate("session_123").await?;
//!
//!     Ok(())
//! }
//! ```

use crate::budget::TokenBudget;
use crate::error::{MemoryError, Result};
use crate::long_term::{KnowledgeEntry, KnowledgeCategory, LongTermMemory};
use crate::mid_term::{ConversationEntry, MidTermMemory};
use crate::short_term::{MemoryMessage, ShortTermMemory};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Memory layer identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemoryLayer {
    /// Short-term memory (current conversation)
    ShortTerm,
    /// Mid-term memory (recent conversations)
    MidTerm,
    /// Long-term memory (persistent knowledge)
    LongTerm,
    /// All layers
    All,
}

impl std::fmt::Display for MemoryLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryLayer::ShortTerm => write!(f, "short-term"),
            MemoryLayer::MidTerm => write!(f, "mid-term"),
            MemoryLayer::LongTerm => write!(f, "long-term"),
            MemoryLayer::All => write!(f, "all"),
        }
    }
}

/// Memory query with layer targeting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryQuery {
    /// Query text
    pub query: String,
    /// Target layer (None = auto-select)
    pub layer: Option<MemoryLayer>,
    /// Maximum results per layer
    pub max_results: usize,
    /// Minimum relevance score (0.0 - 1.0)
    pub min_score: f32,
    /// Include metadata in results
    pub include_metadata: bool,
}

impl MemoryQuery {
    /// Create a new memory query.
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            layer: None,
            max_results: 10,
            min_score: 0.1,
            include_metadata: false,
        }
    }

    /// Target a specific layer.
    pub fn target_layer(mut self, layer: MemoryLayer) -> Self {
        self.layer = Some(layer);
        self
    }

    /// Set maximum results.
    pub fn max_results(mut self, max: usize) -> Self {
        self.max_results = max;
        self
    }

    /// Set minimum relevance score.
    pub fn min_score(mut self, score: f32) -> Self {
        self.min_score = score;
        self
    }

    /// Include metadata in results.
    pub fn with_metadata(mut self) -> Self {
        self.include_metadata = true;
        self
    }
}

/// Result from a memory query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryItem {
    /// Content
    pub content: String,
    /// Source layer
    pub source_layer: MemoryLayer,
    /// Relevance score (0.0 - 1.0)
    pub score: f64,
    /// Timestamp
    pub timestamp: i64,
    /// Optional metadata
    pub metadata: Option<serde_json::Value>,
    /// Session ID (for mid-term)
    pub session_id: Option<String>,
}

impl MemoryItem {
    /// Create a new memory item.
    pub fn new(
        content: impl Into<String>,
        source_layer: MemoryLayer,
        score: f64,
    ) -> Self {
        Self {
            content: content.into(),
            source_layer,
            score,
            timestamp: chrono::Utc::now().timestamp(),
            metadata: None,
            session_id: None,
        }
    }

    /// Create from short-term message.
    pub fn from_short_term(msg: MemoryMessage, score: f64) -> Self {
        Self {
            content: msg.content,
            source_layer: MemoryLayer::ShortTerm,
            score,
            timestamp: msg.timestamp,
            metadata: msg.metadata,
            session_id: None,
        }
    }

    /// Create from mid-term entry.
    pub fn from_mid_term(entry: ConversationEntry, score: f64) -> Self {
        Self {
            content: format!("Q: {}\nA: {}", entry.user_input, entry.assistant_response),
            source_layer: MemoryLayer::MidTerm,
            score,
            timestamp: entry.timestamp,
            metadata: Some(serde_json::json!({
                "question": entry.user_input,
                "answer": entry.assistant_response,
            })),
            session_id: Some(entry.session_id),
        }
    }

    /// Create from long-term entry.
    pub fn from_long_term(entry: KnowledgeEntry, score: f64) -> Self {
        Self {
            content: entry.content.clone(),
            source_layer: MemoryLayer::LongTerm,
            score,
            timestamp: entry.created_at,
            metadata: Some(serde_json::json!({
                "title": entry.title,
                "category": format!("{:?}", entry.category),
                "tags": entry.tags,
            })),
            session_id: None,
        }
    }
}

/// Query results from unified memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryResults {
    /// Items from all queried layers
    pub items: Vec<MemoryItem>,
    /// Total items found
    pub total_count: usize,
    /// Items per layer
    pub layer_counts: HashMap<String, usize>,
    /// Query execution time (ms)
    pub query_time_ms: u64,
}

impl MemoryResults {
    /// Create empty results.
    pub fn empty() -> Self {
        Self {
            items: Vec::new(),
            total_count: 0,
            layer_counts: HashMap::new(),
            query_time_ms: 0,
        }
    }

    /// Get items from a specific layer.
    pub fn from_layer(&self, layer: MemoryLayer) -> Vec<&MemoryItem> {
        self.items
            .iter()
            .filter(|item| item.source_layer == layer)
            .collect()
    }

    /// Sort by relevance score.
    pub fn sort_by_relevance(mut self) -> Self {
        self.items.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        self
    }
}

/// Promotion policy for moving data between layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PromotionPolicy {
    /// Never promote automatically
    Never,
    /// Promote on consolidate (short-term -> mid-term)
    OnConsolidate,
    /// Promote when importance threshold is reached (mid-term -> long-term)
    OnImportance,
    /// Promote automatically based on heuristics
    Auto,
}

/// Configuration for unified memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedMemoryConfig {
    /// Promotion policy
    pub promotion_policy: PromotionPolicy,
    /// Minimum access count for promotion to long-term
    pub min_access_count: usize,
    /// Minimum age (hours) for promotion to long-term
    pub min_age_hours: i64,
    /// Token budget for context building
    pub token_budget: Option<TokenBudget>,
}

impl Default for UnifiedMemoryConfig {
    fn default() -> Self {
        Self {
            promotion_policy: PromotionPolicy::OnConsolidate,
            min_access_count: 3,
            min_age_hours: 24,
            token_budget: None,
        }
    }
}

/// Unified memory interface.
///
/// Provides a single API for all memory layers with automatic
/// promotion and intelligent query routing.
#[derive(Clone)]
pub struct UnifiedMemory {
    /// Short-term memory
    short_term: Arc<RwLock<ShortTermMemory>>,
    /// Mid-term memory
    mid_term: Arc<RwLock<MidTermMemory>>,
    /// Long-term memory
    long_term: Arc<RwLock<LongTermMemory>>,
    /// Configuration
    config: UnifiedMemoryConfig,
}

impl UnifiedMemory {
    /// Create a new unified memory instance.
    pub fn new() -> Self {
        Self::with_config(UnifiedMemoryConfig::default())
    }

    /// Create with custom configuration.
    pub fn with_config(config: UnifiedMemoryConfig) -> Self {
        Self {
            short_term: Arc::new(RwLock::new(ShortTermMemory::new())),
            mid_term: Arc::new(RwLock::new(MidTermMemory::new())),
            long_term: Arc::new(RwLock::new(LongTermMemory::new())),
            config,
        }
    }

    /// Get the configuration.
    pub fn config(&self) -> &UnifiedMemoryConfig {
        &self.config
    }

    /// Update configuration.
    pub async fn set_config(&mut self, config: UnifiedMemoryConfig) {
        self.config = config;
    }

    // === Short-term Operations ===

    /// Add a message to short-term memory.
    pub async fn add(&self, role: impl Into<String>, content: impl Into<String>) -> Result<()> {
        let mut memory = self.short_term.write().await;
        memory.add(role, content)}

    /// Add a pre-built message to short-term memory.
    pub async fn add_message(&self, message: MemoryMessage) -> Result<()> {
        let mut memory = self.short_term.write().await;
        memory.add_message(message)}

    /// Get all short-term messages.
    pub async fn get_short_term(&self) -> Vec<MemoryMessage> {
        let memory = self.short_term.read().await;
        memory.get_messages()
    }

    /// Get last N messages from short-term.
    pub async fn get_last_n(&self, n: usize) -> Vec<MemoryMessage> {
        let memory = self.short_term.read().await;
        memory.get_last_n(n)
    }

    /// Clear short-term memory.
    pub async fn clear_short_term(&self) {
        let mut memory = self.short_term.write().await;
        memory.clear();
    }

    // === Mid-term Operations ===

    /// Add a conversation to mid-term memory.
    pub async fn add_conversation(
        &self,
        session_id: impl Into<String>,
        question: impl Into<String>,
        answer: impl Into<String>,
    ) -> Result<()> {
        let memory = self.mid_term.write().await;
        memory
            .add_conversation(session_id, question, answer)
            .await}

    /// Get conversations for a session.
    pub async fn get_session(&self, session_id: &str) -> Vec<ConversationEntry> {
        let memory = self.mid_term.read().await;
        memory.get_by_session(session_id).await
    }

    /// Remove a session from mid-term memory.
    pub async fn remove_session(&self, session_id: &str) -> Result<usize> {
        let memory = self.mid_term.write().await;
        memory
            .remove_session(session_id)
            .await}

    /// Clear mid-term memory.
    pub async fn clear_mid_term(&self) {
        let memory = self.mid_term.write().await;
        memory.clear().await;
    }

    // === Long-term Operations ===

    /// Add knowledge to long-term memory.
    pub async fn add_knowledge(&self, entry: KnowledgeEntry) -> Result<()> {
        let memory = self.long_term.write().await;
        memory.add(entry).await}

    /// Search long-term memory.
    pub async fn search_long_term(&self, query: &str) -> Vec<KnowledgeEntry> {
        let memory = self.long_term.read().await;
        memory.search(query).await
    }

    /// Get knowledge by category.
    pub async fn get_by_category(&self, category: KnowledgeCategory) -> Vec<KnowledgeEntry> {
        let memory = self.long_term.read().await;
        memory.get_by_category(&category).await
    }

    /// Clear long-term memory.
    pub async fn clear_long_term(&self) {
        let memory = self.long_term.write().await;
        memory.clear().await;
    }

    // === Consolidation ===

    /// Consolidate short-term memory to mid-term.
    ///
    /// Moves all messages from short-term to mid-term memory for the given session.
    pub async fn consolidate(&self, session_id: impl Into<String>) -> Result<usize> {
        let session = session_id.into();

        // Get messages from short-term
        let messages = {
            let memory = self.short_term.read().await;
            memory.get_messages()
        };

        if messages.is_empty() {
            return Ok(0);
        }

        // Convert messages to conversation entries
        // Group consecutive user-assistant pairs
        let mut consolidated = 0;
        let mut current_question: Option<String> = None;

        for msg in messages {
            match msg.role.as_str() {
                "user" => {
                    current_question = Some(msg.content);
                }
                "assistant" => {
                    if let Some(question) = current_question.take() {
                        self.add_conversation(&session, question, msg.content)
                            .await?;
                        consolidated += 1;
                    }
                }
                _ => {}
            }
        }

        // Clear short-term after consolidation
        if consolidated > 0 && self.config.promotion_policy != PromotionPolicy::Never {
            self.clear_short_term().await;
        }

        Ok(consolidated)
    }

    /// Promote mid-term entries to long-term based on importance.
    ///
    /// Entries are promoted if they meet the configured importance threshold.
    pub async fn promote_to_long_term(&self) -> Result<usize> {
        if self.config.promotion_policy == PromotionPolicy::Never {
            return Ok(0);
        }

        let mut promoted = 0;
        let now = chrono::Utc::now().timestamp();
        let min_age = self.config.min_age_hours * 3600;
        let _min_access = self.config.min_access_count;

        // Get all entries from mid-term
        let entries = {
            let memory = self.mid_term.read().await;
            memory.get_all().await
        };

        // Collect unique session IDs
        let sessions: HashSet<String> = entries
            .iter()
            .map(|e| e.session_id.clone())
            .collect();

        for session_id in sessions {
            let session_entries: Vec<_> = entries
                .iter()
                .filter(|e| e.session_id == session_id)
                .collect();

            for entry in session_entries {
                let age = now - entry.timestamp;

                // Check promotion criteria
                // Note: access_count is not tracked in ConversationEntry,
                // so we use age as the primary criterion
                if age >= min_age {
                    // Create knowledge entry
                    let knowledge = KnowledgeEntry::new(
                        format!("Conversation: {}", session_id),
                        format!("Q: {}\n\nA: {}", entry.user_input, entry.assistant_response),
                        KnowledgeCategory::BestPractice,
                    )
                    .with_tags(vec![session_id.clone()]);

                    // Add to long-term
                    {
                        let memory = self.long_term.write().await;
                        memory.add(knowledge).await?;
                    }

                    promoted += 1;
                }
            }
        }

        Ok(promoted)
    }

    // === Unified Query ===

    /// Query across memory layers.
    ///
    /// If no layer is specified in the query, automatically routes to the
    /// most appropriate layer based on query characteristics.
    pub async fn query(&self, query: MemoryQuery) -> Result<MemoryResults> {
        let start = std::time::Instant::now();

        // Determine target layers
        let target_layers = match query.layer {
            Some(MemoryLayer::All) => vec![MemoryLayer::ShortTerm, MemoryLayer::MidTerm, MemoryLayer::LongTerm],
            Some(layer) => vec![layer],
            None => self.auto_select_layers(&query.query),
        };

        let mut items = Vec::new();
        let mut layer_counts = HashMap::new();

        // Query each target layer
        for layer in &target_layers {
            let mut layer_items = self.query_layer(layer, &query).await?;
            layer_counts.insert(layer.to_string(), layer_items.len());
            items.append(&mut layer_items);
        }

        // Filter by minimum score
        items.retain(|item| item.score >= query.min_score as f64);

        // Sort by relevance
        items.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        // Limit results
        items.truncate(query.max_results);

        let elapsed = start.elapsed().as_millis() as u64;

        Ok(MemoryResults {
            items,
            total_count: layer_counts.values().copied().sum(),
            layer_counts,
            query_time_ms: elapsed,
        })
    }

    /// Query with a simple string (creates default query).
    pub async fn query_str(&self, query: &str) -> Result<MemoryResults> {
        self.query(MemoryQuery::new(query)).await
    }

    /// Auto-select appropriate layers based on query.
    fn auto_select_layers(&self, query: &str) -> Vec<MemoryLayer> {
        // Simple heuristic: recent/short queries go to short-term,
        // specific keywords go to mid-term, general knowledge goes to long-term

        let query_lower = query.to_lowercase();

        // Recent/current context indicators
        let is_recent = query_lower.contains("last")
            || query_lower.contains("recent")
            || query_lower.contains("current")
            || query_lower.contains("just")
            || query.len() < 20;

        // Knowledge indicators
        let is_knowledge = query_lower.contains("how")
            || query_lower.contains("what is")
            || query_lower.contains("explain")
            || query_lower.contains("manual")
            || query_lower.contains("guide");

        if is_recent {
            vec![MemoryLayer::ShortTerm, MemoryLayer::MidTerm]
        } else if is_knowledge {
            vec![MemoryLayer::LongTerm, MemoryLayer::MidTerm]
        } else {
            vec![MemoryLayer::MidTerm, MemoryLayer::ShortTerm]
        }
    }

    /// Query a specific layer.
    async fn query_layer(&self, layer: &MemoryLayer, query: &MemoryQuery) -> Result<Vec<MemoryItem>> {
        match layer {
            MemoryLayer::ShortTerm => {
                let memory = self.short_term.read().await;
                let messages = memory.get_messages();

                let items: Vec<_> = messages
                    .into_iter()
                    .map(|msg| {
                        let score = simple_relevance(&query.query, &msg.content);
                        MemoryItem::from_short_term(msg, score)
                    })
                    .collect();

                Ok(items)
            }
            MemoryLayer::MidTerm => {
                let memory = self.mid_term.read().await;
                let results = memory.search(&query.query, query.max_results).await;

                let items: Vec<_> = results
                    .into_iter()
                    .map(|r| {
                        MemoryItem::from_mid_term(r.entry, r.score as f64)
                    })
                    .collect();

                Ok(items)
            }
            MemoryLayer::LongTerm => {
                let memory = self.long_term.read().await;
                let results = memory.search(&query.query).await;

                let items: Vec<_> = results
                    .into_iter()
                    .map(|entry| {
                        let score = simple_relevance(&query.query, &entry.content);
                        MemoryItem::from_long_term(entry, score)
                    })
                    .collect();

                Ok(items)
            }
            MemoryLayer::All => {
                // Shouldn't happen, but handle gracefully
                Ok(Vec::new())
            }
        }
    }

    // === Statistics ===

    /// Get statistics for all memory layers.
    pub async fn stats(&self) -> UnifiedMemoryStats {
        let short_term = self.short_term.read().await;
        let mid_term = self.mid_term.read().await;
        let long_term = self.long_term.read().await;

        // Get entry and session counts for mid-term
        let all_entries = mid_term.get_all().await;
        let session_ids: HashSet<_> = all_entries
            .iter()
            .map(|e| e.session_id.clone())
            .collect();

        // Get long-term entry count
        let long_term_count = long_term.len().await;

        UnifiedMemoryStats {
            short_term_messages: short_term.len(),
            short_term_tokens: short_term.token_count(),
            mid_term_entries: all_entries.len(),
            mid_term_sessions: session_ids.len(),
            long_term_entries: long_term_count,
        }
    }

    /// Get memory usage for context building with token budget.
    pub async fn build_context(
        &self,
        system_prompt: Option<&str>,
        max_tokens: usize,
    ) -> Result<String> {
        use edge_ai_core::llm::token_counter::TokenCounter;

        let counter = TokenCounter::default();
        let mut context = String::new();
        let mut used_tokens = 0;

        // Add system prompt
        if let Some(prompt) = system_prompt {
            let prompt_tokens = counter.count(prompt);
            if prompt_tokens > max_tokens {
                return Err(MemoryError::CapacityExceeded(
                    "System prompt exceeds budget".to_string(),
                ));
            }
            context.push_str(prompt);
            context.push_str("\n\n");
            used_tokens += prompt_tokens;
        }

        // Get recent messages from short-term
        let messages = {
            let memory = self.short_term.read().await;
            memory.get_messages()
        };

        for msg in messages {
            let msg_tokens = counter.count(&msg.content);
            if used_tokens + msg_tokens > max_tokens {
                break;
            }
            context.push_str(&format!("{}: {}\n", msg.role, msg.content));
            used_tokens += msg_tokens;
        }

        Ok(context)
    }
}

impl Default for UnifiedMemory {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics for unified memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedMemoryStats {
    /// Number of short-term messages
    pub short_term_messages: usize,
    /// Tokens in short-term memory
    pub short_term_tokens: usize,
    /// Number of mid-term entries
    pub mid_term_entries: usize,
    /// Number of mid-term sessions
    pub mid_term_sessions: usize,
    /// Number of long-term entries
    pub long_term_entries: usize,
}

/// Simple relevance scoring for keyword matching.
fn simple_relevance(query: &str, content: &str) -> f64 {
    let query_lower = query.to_lowercase();
    let content_lower = content.to_lowercase();

    // Exact match
    if content_lower.contains(&query_lower) {
        return 0.9;
    }

    // Word overlap
    let query_words: std::collections::HashSet<_> =
        query_lower.split_whitespace().collect();
    let content_words: std::collections::HashSet<_> =
        content_lower.split_whitespace().collect();

    if query_words.is_empty() {
        return 0.0;
    }

    let intersection = query_words.intersection(&content_words).count();
    let union = query_words.union(&content_words).count();

    if union == 0 {
        return 0.0;
    }

    // Jaccard similarity
    intersection as f64 / union as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_layer_display() {
        assert_eq!(MemoryLayer::ShortTerm.to_string(), "short-term");
        assert_eq!(MemoryLayer::MidTerm.to_string(), "mid-term");
        assert_eq!(MemoryLayer::LongTerm.to_string(), "long-term");
        assert_eq!(MemoryLayer::All.to_string(), "all");
    }

    #[test]
    fn test_memory_query_builder() {
        let query = MemoryQuery::new("test")
            .target_layer(MemoryLayer::ShortTerm)
            .max_results(5)
            .min_score(0.3)
            .with_metadata();

        assert_eq!(query.query, "test");
        assert_eq!(query.layer, Some(MemoryLayer::ShortTerm));
        assert_eq!(query.max_results, 5);
        assert_eq!(query.min_score, 0.3);
        assert!(query.include_metadata);
    }

    #[test]
    fn test_memory_item_creation() {
        let item = MemoryItem::new("test content", MemoryLayer::ShortTerm, 0.8);
        assert_eq!(item.content, "test content");
        assert_eq!(item.source_layer, MemoryLayer::ShortTerm);
        assert_eq!(item.score, 0.8);
    }

    #[test]
    fn test_memory_results_empty() {
        let results = MemoryResults::empty();
        assert!(results.items.is_empty());
        assert_eq!(results.total_count, 0);
    }

    #[test]
    fn test_memory_results_sort() {
        let mut results = MemoryResults::empty();
        results.items = vec![
            MemoryItem::new("low", MemoryLayer::ShortTerm, 0.3),
            MemoryItem::new("high", MemoryLayer::ShortTerm, 0.9),
            MemoryItem::new("mid", MemoryLayer::ShortTerm, 0.6),
        ];

        let sorted = results.sort_by_relevance();
        assert_eq!(sorted.items[0].score, 0.9);
        assert_eq!(sorted.items[2].score, 0.3);
    }

    #[test]
    fn test_config_default() {
        let config = UnifiedMemoryConfig::default();
        assert_eq!(config.promotion_policy, PromotionPolicy::OnConsolidate);
        assert_eq!(config.min_access_count, 3);
        assert_eq!(config.min_age_hours, 24);
    }

    #[tokio::test]
    async fn test_unified_memory_creation() {
        let memory = UnifiedMemory::new();
        let stats = memory.stats().await;
        assert_eq!(stats.short_term_messages, 0);
        assert_eq!(stats.mid_term_entries, 0);
        assert_eq!(stats.long_term_entries, 0);
    }

    #[test]
    fn test_promotion_policy_serialize() {
        let policy = PromotionPolicy::Auto;
        let json = serde_json::to_string(&policy).unwrap();
        assert_eq!(json, "\"Auto\"");
    }

    #[tokio::test]
    async fn test_add_and_query() {
        let memory = UnifiedMemory::new();

        // Add to short-term
        memory.add("user", "Hello world").await.unwrap();
        memory.add("assistant", "Hi there").await.unwrap();

        // Query
        let results = memory.query_str("hello").await.unwrap();
        assert!(!results.items.is_empty());
    }

    #[tokio::test]
    async fn test_consolidate() {
        let memory = UnifiedMemory::new();

        // Add messages
        memory.add("user", "Question 1").await.unwrap();
        memory.add("assistant", "Answer 1").await.unwrap();
        memory.add("user", "Question 2").await.unwrap();
        memory.add("assistant", "Answer 2").await.unwrap();

        // Consolidate
        let count = memory.consolidate("test_session").await.unwrap();
        assert_eq!(count, 2);

        // Short-term should be cleared
        let short_term = memory.get_short_term().await;
        assert!(short_term.is_empty());

        // Mid-term should have the session
        let session = memory.get_session("test_session").await;
        assert_eq!(session.len(), 2);
    }
}
