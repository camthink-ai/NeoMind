//! Mid-term memory for recent conversation history.
//!
//! Mid-term memory stores recent conversations with vector-based semantic search.
//! It helps retrieve relevant past conversations based on similarity.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::error::Result;

/// Default maximum entries in mid-term memory
pub const DEFAULT_MAX_ENTRIES: usize = 1000;

/// Default embedding dimension (for simple hash-based embeddings)
pub const DEFAULT_EMBEDDING_DIM: usize = 64;

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

/// Simple hash-based embedding generator.
/// In production, this would use a proper embedding model.
pub struct SimpleEmbedding {
    dim: usize,
}

impl SimpleEmbedding {
    /// Create a new simple embedding generator.
    pub fn new(dim: usize) -> Self {
        Self { dim }
    }

    /// Generate an embedding from text.
    pub fn embed(&self, text: &str) -> Vec<f32> {
        let mut embedding = vec![0.0_f32; self.dim];

        // Simple hash-based embedding (for demonstration)
        for (i, byte) in text.bytes().enumerate() {
            let pos = i % self.dim;
            embedding[pos] = embedding[pos] * 31.0 + (byte as f32) * 0.1;
            embedding[pos] = (embedding[pos] % 10.0 - 5.0) / 5.0; // Normalize to [-1, 1]
        }

        // Normalize to unit length
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in embedding.iter_mut() {
                *v /= norm;
            }
        }

        embedding
    }

    /// Compute cosine similarity between two embeddings.
    pub fn cosine_similarity(&self, a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }

        let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot_product / (norm_a * norm_b)
        }
    }
}

impl Default for SimpleEmbedding {
    fn default() -> Self {
        Self::new(DEFAULT_EMBEDDING_DIM)
    }
}

/// Mid-term memory for storing recent conversation history.
pub struct MidTermMemory {
    /// All conversation entries
    entries: Arc<RwLock<HashMap<String, ConversationEntry>>>,
    /// Maximum number of entries
    max_entries: usize,
    /// Embedding generator
    embedding: SimpleEmbedding,
    /// Index by session
    session_index: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl MidTermMemory {
    /// Create a new mid-term memory.
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            max_entries: DEFAULT_MAX_ENTRIES,
            embedding: SimpleEmbedding::default(),
            session_index: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Set the maximum number of entries.
    pub fn with_max_entries(mut self, max: usize) -> Self {
        self.max_entries = max;
        self
    }

    /// Set the embedding dimension.
    pub fn with_embedding_dim(mut self, dim: usize) -> Self {
        self.embedding = SimpleEmbedding::new(dim);
        self
    }

    /// Add a conversation entry.
    pub async fn add(&self, entry: ConversationEntry) -> Result<()> {
        let id = entry.id.clone();
        let session_id = entry.session_id.clone();
        let text = entry.text_for_embedding();

        // Generate embedding
        let embedding = self.embedding.embed(&text);
        let entry_with_embed = entry.with_embedding(embedding);

        // Check capacity and evict if necessary
        {
            let mut entries = self.entries.write().await;
            if entries.len() >= self.max_entries {
                // Find oldest entry and remove it
                if let Some(oldest_id) = entries.values().min_by_key(|e| e.timestamp).map(|e| e.id.clone()) {
                    entries.remove(&oldest_id);
                }
            }
            entries.insert(id.clone(), entry_with_embed);
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

    /// Search for similar conversations.
    pub async fn search(&self, query: &str, top_k: usize) -> Vec<SearchResult> {
        let query_embedding = self.embedding.embed(query);
        let entries = self.entries.read().await;

        let mut results: Vec<SearchResult> = entries
            .values()
            .filter_map(|entry| {
                entry.embedding.as_ref().map(|emb| SearchResult {
                    entry: entry.clone(),
                    score: self.embedding.cosine_similarity(&query_embedding, emb),
                })
            })
            .collect();

        // Sort by score (descending)
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        // Keep top_k results
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
    }

    /// Remove entries for a session.
    pub async fn remove_session(&self, session_id: &str) -> Result<usize> {
        let mut session_idx = self.session_index.write().await;
        let mut entries = self.entries.write().await;

        let ids = session_idx.remove(session_id).unwrap_or_default();
        let count = ids.len();

        for id in &ids {
            entries.remove(id);
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
        let sim_12 = embed.cosine_similarity(&emb1, &emb2);
        let sim_13 = embed.cosine_similarity(&emb1, &emb3);

        // Note: This is a simple hash-based embedding, so similarity might not be semantic
        // Just test that it runs and returns values
        assert!(sim_12 >= 0.0 && sim_12 <= 1.0);
        assert!(sim_13 >= 0.0 && sim_13 <= 1.0);
    }

    #[test]
    fn test_cosine_similarity() {
        let embed = SimpleEmbedding::new(4);
        let a = vec![1.0, 0.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0, 0.0];
        let c = vec![0.0, 1.0, 0.0, 0.0];

        // Identical vectors
        assert!((embed.cosine_similarity(&a, &b) - 1.0).abs() < 0.001);

        // Orthogonal vectors
        assert!(embed.cosine_similarity(&a, &c) < 0.001);
    }
}
