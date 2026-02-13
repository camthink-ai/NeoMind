//! Mid-term memory for recent conversation history.
//!
//! Mid-term memory stores recent conversations with vector-based semantic search.
//! It helps retrieve relevant past conversations based on similarity.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::bm25::{extract_text_for_bm25, BM25Index};
use super::embeddings::{
    cosine_similarity, create_embedding_model, EmbeddingConfig, EmbeddingModel, SimpleEmbedding,
};
use super::error::Result;

/// Wrapper to make SimpleEmbedding implement EmbeddingModel.
pub struct SimpleEmbeddingWrapper(pub SimpleEmbedding);

#[async_trait::async_trait]
impl EmbeddingModel for SimpleEmbeddingWrapper {
    async fn embed(&self, text: &str) -> std::result::Result<Vec<f32>, super::error::MemoryError> {
        Ok(self.0.embed(text))
    }

    async fn embed_batch(
        &self,
        texts: &[String],
    ) -> std::result::Result<Vec<Vec<f32>>, super::error::MemoryError> {
        Ok(texts.iter().map(|t| self.0.embed(t)).collect())
    }

    fn dimension(&self) -> usize {
        self.0.dimension()
    }

    fn model_name(&self) -> &str {
        "simple"
    }
}

/// Default maximum entries in mid-term memory
pub const DEFAULT_MAX_ENTRIES: usize = 1000;

/// A conversation entry in mid-term memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationEntry {
    /// Unique ID
    pub id: String,
    /// Session ID
    pub session_id: String,
    /// User input
    pub user_input: String,
    /// Assistant response
    pub assistant_response: String,
    /// Timestamp
    pub timestamp: i64,
    /// Embedding vector (for semantic search)
    pub embedding: Option<Vec<f32>>,
    /// Additional metadata
    pub metadata: Option<serde_json::Value>,
}

impl ConversationEntry {
    /// Create a new conversation entry.
    pub fn new(
        session_id: impl Into<String>,
        user_input: impl Into<String>,
        assistant_response: impl Into<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            user_input: user_input.into(),
            assistant_response: assistant_response.into(),
            timestamp: chrono::Utc::now().timestamp(),
            embedding: None,
            metadata: None,
        }
    }

    /// Set the embedding.
    pub fn with_embedding(mut self, embedding: Vec<f32>) -> Self {
        self.embedding = Some(embedding);
        self
    }

    /// Set metadata.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Get combined text for embedding.
    pub fn text_for_embedding(&self) -> String {
        format!("Q: {}\nA: {}", self.user_input, self.assistant_response)
    }

    /// Get a summary.
    pub fn summary(&self) -> String {
        let response_preview = if self.assistant_response.len() > 100 {
            format!("{}...", &self.assistant_response[..100])
        } else {
            self.assistant_response.clone()
        };
        format!("Q: {} | A: {}", self.user_input, response_preview)
    }
}

/// Result from a similarity search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// The conversation entry
    pub entry: ConversationEntry,
    /// Similarity score (0-1, higher is more similar)
    pub score: f32,
}

/// Mid-term memory for storing recent conversation history.
pub struct MidTermMemory {
    /// All conversation entries
    entries: Arc<RwLock<HashMap<String, ConversationEntry>>>,
    /// Maximum number of entries
    max_entries: usize,
    /// Embedding model (boxed trait object)
    embedding: Arc<dyn EmbeddingModel>,
    /// Index by session
    session_index: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// BM25 full-text search index
    bm25_index: Arc<RwLock<BM25Index>>,
}

impl MidTermMemory {
    /// Create a new mid-term memory with default (simple) embedding.
    pub fn new() -> Self {
        Self::with_embedding_config(EmbeddingConfig::simple())
    }

    /// Create a new mid-term memory with custom embedding configuration.
    pub fn with_embedding_config(config: EmbeddingConfig) -> Self {
        let embedding = create_embedding_model(config).unwrap_or_else(|_| {
            // Fallback to simple embedding on error
            Box::new(SimpleEmbeddingWrapper(SimpleEmbedding::default()))
        });

        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            max_entries: DEFAULT_MAX_ENTRIES,
            embedding: Arc::from(embedding),
            session_index: Arc::new(RwLock::new(HashMap::new())),
            bm25_index: Arc::new(RwLock::new(BM25Index::new())),
        }
    }

    /// Set the maximum number of entries.
    pub fn with_max_entries(mut self, max: usize) -> Self {
        self.max_entries = max;
        self
    }

    /// Set the embedding dimension (only works with Simple embedding).
    pub fn with_embedding_dim(mut self, dim: usize) -> Self {
        self.embedding = Arc::new(SimpleEmbeddingWrapper(SimpleEmbedding::new(dim)));
        self
    }

    /// Add a conversation entry.
    pub async fn add(&self, entry: ConversationEntry) -> Result<()> {
        let id = entry.id.clone();
        let session_id = entry.session_id.clone();
        let text = entry.text_for_embedding();
        let user_input = entry.user_input.clone();
        let assistant_response = entry.assistant_response.clone();

        // Generate embedding asynchronously
        let embedding = self.embedding.embed(&text).await?;
        let entry_with_embed = entry.with_embedding(embedding);

        // Check capacity and evict if necessary
        let evicted_id = {
            let mut entries = self.entries.write().await;
            if entries.len() >= self.max_entries {
                // Find oldest entry and remove it
                if let Some(oldest) = entries.values().min_by_key(|e| e.timestamp) {
                    let oldest_id = oldest.id.clone();
                    entries.remove(&oldest_id);
                    Some(oldest_id)
                } else {
                    None
                }
            } else {
                None
            }
        };

        // Insert new entry
        {
            let mut entries = self.entries.write().await;
            entries.insert(id.clone(), entry_with_embed);
        }

        // Update BM25 index
        {
            let text_for_bm25 = extract_text_for_bm25(&user_input, &assistant_response);
            let mut bm25 = self.bm25_index.write().await;
            bm25.add_document(&id, &text_for_bm25);
        }

        // Remove evicted entry from BM25
        if let Some(evicted) = evicted_id {
            let mut bm25 = self.bm25_index.write().await;
            bm25.remove_document(&evicted);
        }

        // Update session index
        {
            let mut session_idx = self.session_index.write().await;
            session_idx.entry(session_id).or_default().push(id);
        }

        Ok(())
    }

    /// Add a conversation (user input + response).
    pub async fn add_conversation(
        &self,
        session_id: impl Into<String>,
        user_input: impl Into<String>,
        assistant_response: impl Into<String>,
    ) -> Result<()> {
        let entry = ConversationEntry::new(session_id, user_input, assistant_response);
        self.add(entry).await
    }

    /// Get an entry by ID.
    pub async fn get(&self, id: &str) -> Option<ConversationEntry> {
        self.entries.read().await.get(id).cloned()
    }

    /// Get all entries for a session.
    pub async fn get_by_session(&self, session_id: &str) -> Vec<ConversationEntry> {
        let session_idx = self.session_index.read().await;
        if let Some(ids) = session_idx.get(session_id) {
            let entries = self.entries.read().await;
            ids.iter()
                .filter_map(|id| entries.get(id).cloned())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Search for similar conversations using semantic search.
    pub async fn search(&self, query: &str, top_k: usize) -> Vec<SearchResult> {
        let query_embedding: Vec<f32> = self.embedding.embed(query).await.unwrap_or_default();
        let entries = self.entries.read().await;

        let mut results: Vec<SearchResult> = entries
            .values()
            .filter_map(|entry| {
                entry.embedding.as_ref().map(|emb| SearchResult {
                    entry: entry.clone(),
                    score: cosine_similarity(&query_embedding, emb),
                })
            })
            .collect();

        // Sort by score (descending)
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        // Keep top_k results
        results.truncate(top_k);
        results
    }

    /// Search for conversations using BM25 full-text search.
    pub async fn search_bm25(&self, query: &str, top_k: usize) -> Vec<SearchResult> {
        let bm25_results = {
            let bm25 = self.bm25_index.read().await;
            bm25.search(query, top_k)
        };

        let entries = self.entries.read().await;
        bm25_results
            .into_iter()
            .filter_map(|bm25_result| {
                entries.get(&bm25_result.id).map(|entry| SearchResult {
                    entry: entry.clone(),
                    score: bm25_result.score as f32,
                })
            })
            .collect()
    }

    /// Search for conversations using hybrid search (semantic + BM25).
    ///
    /// Combines semantic search and BM25 full-text search with configurable weights.
    /// - `semantic_weight`: Weight for semantic similarity (0.0 - 1.0)
    /// - `bm25_weight`: Weight for BM25 score (0.0 - 1.0)
    ///
    /// The final score is: `semantic_score * semantic_weight + bm25_score * bm25_weight`
    pub async fn search_hybrid(
        &self,
        query: &str,
        top_k: usize,
        semantic_weight: f32,
        bm25_weight: f32,
    ) -> Vec<SearchResult> {
        // Get semantic search results
        let semantic_results = self.search(query, top_k * 2).await;

        // Get BM25 search results
        let bm25_results = self.search_bm25(query, top_k * 2).await;

        // Combine scores using a map
        let mut combined_scores: HashMap<String, (ConversationEntry, f32)> = HashMap::new();

        // Add semantic scores
        for result in semantic_results {
            let entry_id = result.entry.id.clone();
            let score = result.score * semantic_weight;
            combined_scores
                .entry(entry_id)
                .or_insert((result.entry, 0.0))
                .1 += score;
        }

        // Add BM25 scores (normalize BM25 scores first)
        if !bm25_results.is_empty() {
            let max_bm25: f32 = bm25_results
                .iter()
                .map(|r| r.score)
                .fold(0.0_f32, |a, b| a.max(b));

            if max_bm25 > 0.0 {
                for result in bm25_results {
                    let entry_id = result.entry.id.clone();
                    let normalized_score = (result.score / max_bm25) * bm25_weight;
                    combined_scores
                        .entry(entry_id)
                        .or_insert((result.entry, 0.0))
                        .1 += normalized_score;
                }
            }
        }

        // Convert to results and sort
        let mut results: Vec<SearchResult> = combined_scores
            .into_iter()
            .map(|(_id, (entry, score))| SearchResult { entry, score })
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        results.truncate(top_k);
        results
    }

    /// Get the number of entries.
    pub async fn len(&self) -> usize {
        self.entries.read().await.len()
    }

    /// Check if empty.
    pub async fn is_empty(&self) -> bool {
        self.entries.read().await.is_empty()
    }

    /// Clear all entries.
    pub async fn clear(&self) {
        self.entries.write().await.clear();
        self.session_index.write().await.clear();
        self.bm25_index.write().await.clear();
    }

    /// Remove entries for a session.
    pub async fn remove_session(&self, session_id: &str) -> Result<usize> {
        let mut session_idx = self.session_index.write().await;
        let mut entries = self.entries.write().await;
        let mut bm25 = self.bm25_index.write().await;

        let ids = session_idx.remove(session_id).unwrap_or_default();
        let count = ids.len();

        for id in &ids {
            entries.remove(id);
            bm25.remove_document(id);
        }

        Ok(count)
    }

    /// Get all entries.
    pub async fn get_all(&self) -> Vec<ConversationEntry> {
        self.entries.read().await.values().cloned().collect()
    }

    /// Get recent entries (limited by n).
    pub async fn get_recent(&self, n: usize) -> Vec<ConversationEntry> {
        let entries = self.entries.read().await;
        let mut sorted: Vec<_> = entries.values().cloned().collect();
        sorted.sort_by_key(|e| std::cmp::Reverse(e.timestamp));
        sorted.truncate(n);
        sorted
    }
}

impl Default for MidTermMemory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_conversation_entry() {
        let entry = ConversationEntry::new("session1", "Hello", "Hi there!");
        assert_eq!(entry.session_id, "session1");
        assert_eq!(entry.user_input, "Hello");
        assert_eq!(entry.assistant_response, "Hi there!");
        assert!(entry.embedding.is_none());
    }

    #[tokio::test]
    async fn test_mid_term_memory_add() {
        let memory = MidTermMemory::new();

        memory
            .add_conversation("session1", "Hello", "Hi there!")
            .await
            .unwrap();

        assert_eq!(memory.len().await, 1);
    }

    #[tokio::test]
    async fn test_get_by_session() {
        let memory = MidTermMemory::new();

        memory
            .add_conversation("session1", "Q1", "A1")
            .await
            .unwrap();
        memory
            .add_conversation("session1", "Q2", "A2")
            .await
            .unwrap();
        memory
            .add_conversation("session2", "Q3", "A3")
            .await
            .unwrap();

        let session1_entries = memory.get_by_session("session1").await;
        assert_eq!(session1_entries.len(), 2);

        let session2_entries = memory.get_by_session("session2").await;
        assert_eq!(session2_entries.len(), 1);
    }

    #[tokio::test]
    async fn test_search() {
        let memory = MidTermMemory::new();

        memory
            .add_conversation("session1", "What is the temperature?", "It's 25 degrees")
            .await
            .unwrap();
        memory
            .add_conversation("session1", "How about humidity?", "About 60%")
            .await
            .unwrap();

        // Search for temperature-related content
        let results = memory.search("temperature", 5).await;
        assert!(!results.is_empty());
    }

    #[tokio::test]
    async fn test_max_entries() {
        let memory = MidTermMemory::new().with_max_entries(3);

        for i in 0..5 {
            memory
                .add_conversation("session1", format!("Q{}", i), format!("A{}", i))
                .await
                .unwrap();
        }

        // Should have max 3 entries
        assert_eq!(memory.len().await, 3);
    }

    #[tokio::test]
    async fn test_remove_session() {
        let memory = MidTermMemory::new();

        memory
            .add_conversation("session1", "Q1", "A1")
            .await
            .unwrap();
        memory
            .add_conversation("session2", "Q2", "A2")
            .await
            .unwrap();

        let count = memory.remove_session("session1").await.unwrap();
        assert_eq!(count, 1);
        assert_eq!(memory.len().await, 1);
    }

    #[tokio::test]
    async fn test_get_recent() {
        let memory = MidTermMemory::new();

        for i in 0..5 {
            memory
                .add_conversation("session1", format!("Q{}", i), format!("A{}", i))
                .await
                .unwrap();
            // Small delay to ensure different timestamps
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }

        let recent = memory.get_recent(3).await;
        assert_eq!(recent.len(), 3);
    }

    #[tokio::test]
    async fn test_clear() {
        let memory = MidTermMemory::new();

        memory
            .add_conversation("session1", "Q1", "A1")
            .await
            .unwrap();

        assert_eq!(memory.len().await, 1);

        memory.clear().await;
        assert!(memory.is_empty().await);
    }

    #[test]
    fn test_simple_embedding() {
        let embed = SimpleEmbedding::new(64);
        let text1 = "Hello world";
        let text2 = "Hello there";
        let text3 = "Completely different";

        let emb1 = embed.embed(text1);
        let emb2 = embed.embed(text2);
        let emb3 = embed.embed(text3);

        // Similar texts should have higher similarity
        let sim_12 = cosine_similarity(&emb1, &emb2);
        let sim_13 = cosine_similarity(&emb1, &emb3);

        // Note: This is a simple hash-based embedding, so similarity might not be semantic
        // Just test that it runs and returns values
        assert!(sim_12 >= 0.0 && sim_12 <= 1.0);
        assert!(sim_13 >= 0.0 && sim_13 <= 1.0);
    }

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0, 0.0];
        let c = vec![0.0, 1.0, 0.0, 0.0];

        // Identical vectors
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.001);

        // Orthogonal vectors
        assert!(cosine_similarity(&a, &c) < 0.001);
    }
}
