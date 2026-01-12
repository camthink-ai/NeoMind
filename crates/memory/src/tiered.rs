//! Tiered memory combining short-term, mid-term, and long-term memory.
//!
//! This module provides a unified interface to all three memory layers.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::error::Result;
use super::long_term::{KnowledgeEntry, KnowledgeCategory, TroubleshootingCase};
use super::mid_term::{ConversationEntry, SearchResult};
use super::short_term::{MemoryMessage, ShortTermMemory};

/// Configuration for tiered memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TieredMemoryConfig {
    /// Maximum messages in short-term memory
    pub max_short_term_messages: usize,
    /// Maximum tokens in short-term memory
    pub max_short_term_tokens: usize,
    /// Maximum entries in mid-term memory
    pub max_mid_term_entries: usize,
    /// Embedding dimension for mid-term memory
    pub embedding_dim: usize,
    /// Maximum knowledge entries in long-term memory
    pub max_long_term_knowledge: usize,
}

impl Default for TieredMemoryConfig {
    fn default() -> Self {
        Self {
            max_short_term_messages: 100,
            max_short_term_tokens: 4000,
            max_mid_term_entries: 1000,
            embedding_dim: 64,
            max_long_term_knowledge: 10000,
        }
    }
}

/// Memory query result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryQueryResult {
    /// Results from short-term memory
    pub short_term: Vec<MemoryMessage>,
    /// Results from mid-term memory
    pub mid_term: Vec<SearchResult>,
    /// Results from long-term memory
    pub long_term: Vec<KnowledgeEntry>,
}

/// Tiered memory combining all three layers.
pub struct TieredMemory {
    /// Short-term memory
    short_term: ShortTermMemory,
    /// Mid-term memory
    mid_term: Arc<super::mid_term::MidTermMemory>,
    /// Long-term memory
    long_term: Arc<super::long_term::LongTermMemory>,
    /// Configuration
    _config: TieredMemoryConfig,
}

impl TieredMemory {
    /// Create a new tiered memory with default config.
    pub fn new() -> Self {
        Self::with_config(TieredMemoryConfig::default())
    }

    /// Create a new tiered memory with custom config.
    pub fn with_config(config: TieredMemoryConfig) -> Self {
        Self {
            short_term: ShortTermMemory::new()
                .with_max_messages(config.max_short_term_messages)
                .with_max_tokens(config.max_short_term_tokens),
            mid_term: Arc::new(
                super::mid_term::MidTermMemory::new()
                    .with_max_entries(config.max_mid_term_entries)
                    .with_embedding_dim(config.embedding_dim),
            ),
            long_term: Arc::new(
                super::long_term::LongTermMemory::new()
                    .with_max_knowledge(config.max_long_term_knowledge),
            ),
            _config: config,
        }
    }

    // ===== Short-term memory operations =====

    /// Add a message to short-term memory.
    pub fn add_message(&mut self, role: impl Into<String>, content: impl Into<String>) -> Result<()> {
        self.short_term.add(role, content)
    }

    /// Get all short-term messages.
    pub fn get_short_term(&self) -> Vec<MemoryMessage> {
        self.short_term.get_messages()
    }

    /// Get the last N messages from short-term memory.
    pub fn get_last_messages(&self, n: usize) -> Vec<MemoryMessage> {
        self.short_term.get_last_n(n)
    }

    /// Clear short-term memory.
    pub fn clear_short_term(&mut self) {
        self.short_term.clear();
    }

    /// Get the formatted prompt for LLM.
    pub fn get_llm_prompt(&self) -> String {
        self.short_term.to_llm_prompt()
    }

    // ===== Mid-term memory operations =====

    /// Add a conversation to mid-term memory.
    pub async fn add_conversation(
        &self,
        session_id: impl Into<String>,
        user_input: impl Into<String>,
        assistant_response: impl Into<String>,
    ) -> Result<()> {
        self.mid_term
            .add_conversation(session_id, user_input, assistant_response)
            .await
    }

    /// Search mid-term memory for similar conversations.
    pub async fn search_mid_term(&self, query: &str, top_k: usize) -> Vec<SearchResult> {
        self.mid_term.search(query, top_k).await
    }

    /// Get conversations by session ID.
    pub async fn get_session_history(&self, session_id: &str) -> Vec<ConversationEntry> {
        self.mid_term.get_by_session(session_id).await
    }

    /// Clear mid-term memory.
    pub async fn clear_mid_term(&self) {
        self.mid_term.clear().await;
    }

    // ===== Long-term memory operations =====

    /// Add a knowledge entry to long-term memory.
    pub async fn add_knowledge(&self, entry: KnowledgeEntry) -> Result<()> {
        self.long_term.add(entry).await
    }

    /// Search long-term memory.
    pub async fn search_knowledge(&self, query: &str) -> Vec<KnowledgeEntry> {
        self.long_term.search(query).await
    }

    /// Get knowledge by category.
    pub async fn get_knowledge_by_category(&self, category: &KnowledgeCategory) -> Vec<KnowledgeEntry> {
        self.long_term.get_by_category(category).await
    }

    /// Get knowledge by device.
    pub async fn get_device_knowledge(&self, device_id: &str) -> Vec<KnowledgeEntry> {
        self.long_term.get_by_device(device_id).await
    }

    /// Add a troubleshooting case.
    pub async fn add_troubleshooting_case(&self, case: TroubleshootingCase) -> Result<()> {
        self.long_term.add_case(case).await
    }

    /// Find troubleshooting cases.
    pub async fn find_troubleshooting(&self, symptoms: &[String]) -> Vec<TroubleshootingCase> {
        self.long_term.find_cases(symptoms).await
    }

    /// Get most accessed knowledge.
    pub async fn get_popular_knowledge(&self, n: usize) -> Vec<KnowledgeEntry> {
        self.long_term.get_most_accessed(n).await
    }

    /// Clear long-term memory.
    pub async fn clear_long_term(&self) {
        self.long_term.clear().await;
    }

    // ===== Combined operations =====

    /// Query all memory layers.
    pub async fn query_all(&self, query: &str, top_k: usize) -> MemoryQueryResult {
        // Short-term: filter by keyword match
        let short_term: Vec<MemoryMessage> = self
            .short_term
            .get_messages()
            .into_iter()
            .filter(|m| m.content.to_lowercase().contains(&query.to_lowercase()))
            .collect();

        // Mid-term: semantic search
        let mid_term = self.mid_term.search(query, top_k).await;

        // Long-term: keyword search
        let long_term = self.long_term.search(query).await;

        MemoryQueryResult {
            short_term,
            mid_term,
            long_term,
        }
    }

    /// Consolidate short-term to mid-term memory.
    /// Call this periodically to preserve important conversations.
    pub async fn consolidate(&self, session_id: &str) -> Result<()> {
        let messages = self.short_term.get_messages();

        // Pair up messages (user + assistant)
        let mut i = 0;
        while i + 1 < messages.len() {
            let user_msg = &messages[i];
            let assistant_msg = &messages[i + 1];

            if (user_msg.role == "user" || user_msg.role == "User")
                && (assistant_msg.role == "assistant" || assistant_msg.role == "Assistant")
            {
                self.mid_term
                    .add_conversation(session_id, &user_msg.content, &assistant_msg.content)
                    .await?;
            }

            i += 2;
        }

        Ok(())
    }

    /// Get memory statistics.
    pub async fn get_stats(&self) -> MemoryStats {
        MemoryStats {
            short_term_messages: self.short_term.len(),
            short_term_tokens: self.short_term.token_count(),
            mid_term_entries: self.mid_term.len().await,
            long_term_entries: self.long_term.len().await,
        }
    }

    /// Get the short-term memory reference.
    pub fn short_term_ref(&self) -> &ShortTermMemory {
        &self.short_term
    }

    /// Get mutable short-term memory reference.
    pub fn short_term_mut(&mut self) -> &mut ShortTermMemory {
        &mut self.short_term
    }

    /// Get the mid-term memory reference.
    pub fn mid_term_ref(&self) -> &Arc<super::mid_term::MidTermMemory> {
        &self.mid_term
    }

    /// Get the long-term memory reference.
    pub fn long_term_ref(&self) -> &Arc<super::long_term::LongTermMemory> {
        &self.long_term
    }
}

impl Default for TieredMemory {
    fn default() -> Self {
        Self::new()
    }
}

/// Memory statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStats {
    /// Number of messages in short-term memory
    pub short_term_messages: usize,
    /// Token count in short-term memory
    pub short_term_tokens: usize,
    /// Number of entries in mid-term memory
    pub mid_term_entries: usize,
    /// Number of entries in long-term memory
    pub long_term_entries: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tiered_memory_creation() {
        let memory = TieredMemory::new();
        let stats = memory.get_stats().await;

        assert_eq!(stats.short_term_messages, 0);
        assert_eq!(stats.mid_term_entries, 0);
        assert_eq!(stats.long_term_entries, 0);
    }

    #[tokio::test]
    async fn test_short_term_operations() {
        let mut memory = TieredMemory::new();

        memory.add_message("user", "Hello").unwrap();
        memory.add_message("assistant", "Hi there!").unwrap();

        assert_eq!(memory.short_term_ref().len(), 2);

        let messages = memory.get_short_term();
        assert_eq!(messages.len(), 2);
    }

    #[tokio::test]
    async fn test_mid_term_operations() {
        let memory = TieredMemory::new();

        memory
            .add_conversation("session1", "Hello", "Hi there!")
            .await
            .unwrap();

        let stats = memory.get_stats().await;
        assert_eq!(stats.mid_term_entries, 1);
    }

    #[tokio::test]
    async fn test_long_term_operations() {
        let memory = TieredMemory::new();

        let entry = KnowledgeEntry::new(
            "Test Knowledge",
            "Test content",
            KnowledgeCategory::BestPractice,
        );

        memory.add_knowledge(entry).await.unwrap();

        let stats = memory.get_stats().await;
        assert_eq!(stats.long_term_entries, 1);
    }

    #[tokio::test]
    async fn test_query_all() {
        let mut memory = TieredMemory::new();

        // Add to short-term
        memory.add_message("user", "What is temperature?").unwrap();
        memory
            .add_message("assistant", "Temperature is 25 degrees")
            .unwrap();

        // Add to mid-term
        memory
            .add_conversation(
                "session1",
                "How do I check humidity?",
                "Use the humidity sensor command.",
            )
            .await
            .unwrap();

        // Add to long-term
        let entry = KnowledgeEntry::new(
            "Temperature Guide",
            "Temperature measures how hot or cold something is.",
            KnowledgeCategory::DeviceManual,
        );
        memory.add_knowledge(entry).await.unwrap();

        let results = memory.query_all("temperature", 5).await;
        // Should find results in at least one layer
        assert!(
            !results.short_term.is_empty()
                || !results.mid_term.is_empty()
                || !results.long_term.is_empty()
        );
    }

    #[tokio::test]
    async fn test_consolidate() {
        let mut memory = TieredMemory::new();

        memory.add_message("user", "Question 1").unwrap();
        memory
            .add_message("assistant", "Answer 1")
            .unwrap();

        memory.consolidate("test_session").await.unwrap();

        let stats = memory.get_stats().await;
        assert_eq!(stats.mid_term_entries, 1);
    }

    #[tokio::test]
    async fn test_troubleshooting() {
        let memory = TieredMemory::new();

        let case = TroubleshootingCase::new("Device not working")
            .with_symptom("No power")
            .with_solution(super::super::long_term::SolutionStep::new(
                1,
                "Check power cable",
            ));

        memory.add_troubleshooting_case(case).await.unwrap();

        let results = memory
            .find_troubleshooting(&["no power".to_string()])
            .await;
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_get_device_knowledge() {
        let memory = TieredMemory::new();

        let entry = KnowledgeEntry::new(
            "Device Manual",
            "Device documentation",
            KnowledgeCategory::DeviceManual,
        )
        .with_devices(vec!["device1".to_string()]);

        memory.add_knowledge(entry).await.unwrap();

        let device_knowledge = memory.get_device_knowledge("device1").await;
        assert_eq!(device_knowledge.len(), 1);
    }

    #[tokio::test]
    async fn test_clear_operations() {
        let mut memory = TieredMemory::new();

        memory.add_message("user", "Test").unwrap();
        memory
            .add_conversation("s1", "Q", "A")
            .await
            .unwrap();

        let entry = KnowledgeEntry::new("Test", "Content", KnowledgeCategory::BestPractice);
        memory.add_knowledge(entry).await.unwrap();

        // Clear short-term
        memory.clear_short_term();
        assert_eq!(memory.short_term_ref().len(), 0);

        // Clear mid-term
        memory.clear_mid_term().await;
        let stats = memory.get_stats().await;
        assert_eq!(stats.mid_term_entries, 0);

        // Clear long-term
        memory.clear_long_term().await;
        let stats = memory.get_stats().await;
        assert_eq!(stats.long_term_entries, 0);
    }

    #[tokio::test]
    async fn test_config() {
        let config = TieredMemoryConfig {
            max_short_term_messages: 50,
            max_short_term_tokens: 2000,
            max_mid_term_entries: 500,
            embedding_dim: 128,
            max_long_term_knowledge: 5000,
        };

        let mut memory = TieredMemory::with_config(config.clone());

        // Verify config is applied (check token limit)
        for _ in 0..100 {
            memory.add_message("user", "x".repeat(100)).unwrap();
        }

        // Should be limited by token count
        assert!(memory.short_term_ref().token_count() <= 2000);
    }
}
