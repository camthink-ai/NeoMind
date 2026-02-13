//! Semantic Search with Hybrid Approach
//!
//! This module provides semantic search combining:
//! - **Vector Similarity**: Cosine similarity on embeddings
//! - **BM25 Keyword**: Traditional keyword matching
//! - **Hybrid Score**: Weighted combination of both approaches
//!
//! ## Architecture
//!
//! ```text
//! Query ---> Embedding + BM25 ---> Dual Scoring ---> Hybrid Result
//!             |                       |                  |
//!             v                       v                  v
//!       Vector Search           Keyword Match       Combined
//!       (Semantic)              (Lexical)            Score
//! ```
//!
//! ## Example
//!
//! ```rust,no_run
//! use neomind_memory::semantic::{SemanticSearch, SearchConfig};
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let search = SemanticSearch::new();
//!
//! // Index documents
//! search.index("doc1", "Temperature sensor readings").await?;
//! search.index("doc2", "Humidity levels in the greenhouse").await?;
//!
//! // Hybrid search
//! let results = search
//!     .search("sensor temperature")
//!     .with_config(SearchConfig::default().hybrid_alpha(0.7))
//!     .execute()
//!     .await?;
//!
//! for result in results {
//!     println!("{}: {:.2}", result.id, result.score);
//! }
//! # Ok(())
//! # }
//! ```

use crate::bm25::BM25Index;
use crate::embeddings::{
    create_embedding_model, EmbeddingConfig, EmbeddingModel, EmbeddingProvider,
};
use crate::error::{MemoryError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Default hybrid alpha (vector weight).
pub const DEFAULT_HYBRID_ALPHA: f64 = 0.7;

/// Maximum number of results to return.
pub const DEFAULT_MAX_RESULTS: usize = 10;

/// Document with embedding for semantic search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticDocument {
    /// Unique document ID
    pub id: String,
    /// Document content
    pub content: String,
    /// Embedding vector (if available)
    pub embedding: Option<Vec<f32>>,
    /// Metadata
    pub metadata: Option<serde_json::Value>,
    /// Timestamp
    pub timestamp: i64,
}

impl SemanticDocument {
    /// Create a new semantic document.
    pub fn new(id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            content: content.into(),
            embedding: None,
            metadata: None,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Create with metadata.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Create with embedding.
    pub fn with_embedding(mut self, embedding: Vec<f32>) -> Self {
        self.embedding = Some(embedding);
        self
    }
}

/// Search result from semantic search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticSearchResult {
    /// Document ID
    pub id: String,
    /// Document content
    pub content: String,
    /// Combined relevance score (0.0 - 1.0)
    pub score: f64,
    /// Vector similarity score (0.0 - 1.0)
    pub vector_score: f64,
    /// BM25 keyword score (0.0 - 1.0)
    pub keyword_score: f64,
    /// Metadata
    pub metadata: Option<serde_json::Value>,
}

/// Configuration for semantic search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    /// Hybrid alpha: weight for vector similarity (0.0 - 1.0)
    /// Remaining weight (1 - alpha) goes to BM25.
    pub hybrid_alpha: f64,
    /// Maximum results to return
    pub max_results: usize,
    /// Minimum score threshold
    pub min_score: f64,
    /// Include scores in results
    pub include_scores: bool,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            hybrid_alpha: DEFAULT_HYBRID_ALPHA,
            max_results: DEFAULT_MAX_RESULTS,
            min_score: 0.1,
            include_scores: true,
        }
    }
}

impl SearchConfig {
    /// Create a new search config.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the hybrid alpha (vector weight).
    pub fn hybrid_alpha(mut self, alpha: f64) -> Self {
        self.hybrid_alpha = alpha.clamp(0.0, 1.0);
        self
    }

    /// Set max results.
    pub fn max_results(mut self, max: usize) -> Self {
        self.max_results = max;
        self
    }

    /// Set minimum score.
    pub fn min_score(mut self, score: f64) -> Self {
        self.min_score = score.clamp(0.0, 1.0);
        self
    }

    /// Include detailed scores.
    pub fn with_scores(mut self) -> Self {
        self.include_scores = true;
        self
    }
}

/// Cosine similarity between two vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot_product: f64 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| *x as f64 * *y as f64)
        .sum();

    let norm_a: f64 = a
        .iter()
        .map(|x| (*x as f64) * (*x as f64))
        .sum::<f64>()
        .sqrt();

    let norm_b: f64 = b
        .iter()
        .map(|x| (*x as f64) * (*x as f64))
        .sum::<f64>()
        .sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot_product / (norm_a * norm_b)
}

/// Dot product similarity between two vectors.
pub fn dot_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    a.iter()
        .zip(b.iter())
        .map(|(x, y)| *x as f64 * *y as f64)
        .sum()
}

/// Semantic search engine with hybrid approach.
///
/// Combines vector similarity (semantic) with BM25 (keyword) for
/// optimal search results across different query types.
#[derive(Clone)]
pub struct SemanticSearch {
    /// Document store
    documents: Arc<RwLock<HashMap<String, SemanticDocument>>>,
    /// BM25 index for keyword search
    bm25: Arc<RwLock<BM25Index>>,
    /// Embedding model (boxed for thread-safe storage)
    embedding: Arc<RwLock<Option<Box<dyn EmbeddingModel>>>>,
    /// Configuration
    config: SearchConfig,
}

impl SemanticSearch {
    /// Create a new semantic search engine.
    pub fn new() -> Self {
        Self {
            documents: Arc::new(RwLock::new(HashMap::new())),
            bm25: Arc::new(RwLock::new(BM25Index::new())),
            embedding: Arc::new(RwLock::new(None)),
            config: SearchConfig::default(),
        }
    }

    /// Create with embedding model.
    pub async fn with_embedding(model: Box<dyn EmbeddingModel>) -> Self {
        let search = Self::new();
        {
            let mut embedding = search.embedding.write().await;
            *embedding = Some(model);
        }
        search
    }

    /// Create with provider configuration.
    pub async fn with_provider(provider: EmbeddingProvider) -> Result<Self> {
        let config = match provider {
            EmbeddingProvider::Local => EmbeddingConfig::local("bge-small-zh-v1.5"),
            EmbeddingProvider::Ollama => EmbeddingConfig::ollama("nomic-embed-text"),
            EmbeddingProvider::OpenAI => EmbeddingConfig::openai("text-embedding-ada-002", ""),
            EmbeddingProvider::Simple => EmbeddingConfig::simple(),
        };

        match create_embedding_model(config) {
            Ok(model) => {
                let search = Self::new();
                {
                    let mut embedding = search.embedding.write().await;
                    *embedding = Some(model);
                }
                Ok(search)
            }
            Err(e) => Err(MemoryError::Config(format!(
                "Failed to create embedding model: {}",
                e
            ))),
        }
    }

    /// Get the current configuration.
    pub fn config(&self) -> &SearchConfig {
        &self.config
    }

    /// Set configuration.
    pub fn set_config(&mut self, config: SearchConfig) {
        self.config = config;
    }

    /// Index a document for search.
    pub async fn index(&self, id: impl Into<String>, content: impl Into<String>) -> Result<()> {
        let id = id.into();
        let content = content.into();

        // Create document
        let mut doc = SemanticDocument::new(&id, &content);

        // Generate embedding if model is available
        {
            let embedding_model = self.embedding.read().await;
            if let Some(model) = embedding_model.as_ref() {
                let emb = model.embed(&content).await.map_err(|e| {
                    MemoryError::Embedding(format!("Failed to generate embedding: {}", e))
                })?;
                doc.embedding = Some(emb);
            }
        }

        // Store document
        {
            let mut docs = self.documents.write().await;
            docs.insert(id.clone(), doc.clone());
        }

        // Update BM25 index
        {
            let mut bm25 = self.bm25.write().await;
            bm25.add_document(&id, &content);
        }

        Ok(())
    }

    /// Index multiple documents.
    pub async fn index_batch(&self, docs: Vec<(String, String)>) -> Result<()> {
        for (id, content) in docs {
            self.index(id, content).await?;
        }
        Ok(())
    }

    /// Remove a document from the index.
    pub async fn remove(&self, id: &str) -> bool {
        // Remove from documents
        let removed = {
            let mut docs = self.documents.write().await;
            docs.remove(id).is_some()
        };

        if removed {
            // Remove from BM25
            let mut bm25 = self.bm25.write().await;
            bm25.remove_document(id);
        }

        removed
    }

    /// Clear all documents.
    pub async fn clear(&self) {
        let mut docs = self.documents.write().await;
        docs.clear();

        let mut bm25 = self.bm25.write().await;
        bm25.clear();
    }

    /// Get document count.
    pub async fn count(&self) -> usize {
        let docs = self.documents.read().await;
        docs.len()
    }

    /// Check if empty.
    pub async fn is_empty(&self) -> bool {
        let docs = self.documents.read().await;
        docs.is_empty()
    }

    /// Search for documents using hybrid approach.
    ///
    /// Returns results ranked by combined vector + keyword score.
    pub fn search<'a>(&'a self, query: &'a str) -> SearchExecutor<'a> {
        SearchExecutor {
            search: self,
            query,
            config: self.config.clone(),
        }
    }

    // Internal: Execute the search.
    async fn execute_search(
        &self,
        query: &str,
        config: &SearchConfig,
    ) -> Result<Vec<SemanticSearchResult>> {
        // Get query embedding
        let query_embedding = {
            let embedding_model = self.embedding.read().await;
            if let Some(model) = embedding_model.as_ref() {
                Some(model.embed(query).await.map_err(|e| {
                    MemoryError::Embedding(format!("Failed to generate query embedding: {}", e))
                })?)
            } else {
                None
            }
        };

        // Get documents
        let docs = {
            let docs = self.documents.read().await;
            docs.iter()
                .map(|(id, doc)| (id.clone(), doc.clone()))
                .collect::<Vec<_>>()
        };

        // Get BM25 scores
        let bm25_scores = {
            let bm25 = self.bm25.read().await;
            bm25.search(query, docs.len())
        };

        // Calculate scores
        let mut results: Vec<SemanticSearchResult> = docs
            .into_iter()
            .map(|(id, doc)| {
                // Vector similarity
                let vector_score = if let (Some(q_emb), Some(d_emb)) =
                    (&query_embedding, doc.embedding.as_ref())
                {
                    cosine_similarity(q_emb, d_emb)
                } else {
                    0.0
                };

                // BM25 keyword score
                let keyword_score = bm25_scores
                    .iter()
                    .find(|result| result.id == id)
                    .map(|result| result.score)
                    .unwrap_or(0.0);

                // Hybrid score
                let score = config.hybrid_alpha * vector_score
                    + (1.0 - config.hybrid_alpha) * keyword_score;

                SemanticSearchResult {
                    id: id.clone(),
                    content: doc.content.clone(),
                    score,
                    vector_score,
                    keyword_score,
                    metadata: doc.metadata.clone(),
                }
            })
            .collect();

        // Filter by minimum score
        results.retain(|r| r.score >= config.min_score);

        // Sort by score (descending)
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        // Limit results
        results.truncate(config.max_results);

        Ok(results)
    }
}

impl Default for SemanticSearch {
    fn default() -> Self {
        Self::new()
    }
}

/// Search executor for building search queries.
pub struct SearchExecutor<'a> {
    search: &'a SemanticSearch,
    query: &'a str,
    config: SearchConfig,
}

impl<'a> SearchExecutor<'a> {
    /// Set custom config for this search.
    pub fn with_config(mut self, config: SearchConfig) -> Self {
        self.config = config;
        self
    }

    /// Set hybrid alpha for this search.
    pub fn hybrid_alpha(mut self, alpha: f64) -> Self {
        self.config.hybrid_alpha = alpha.clamp(0.0, 1.0);
        self
    }

    /// Set max results for this search.
    pub fn max_results(mut self, max: usize) -> Self {
        self.config.max_results = max;
        self
    }

    /// Execute the search.
    pub async fn execute(self) -> Result<Vec<SemanticSearchResult>> {
        self.search.execute_search(self.query, &self.config).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semantic_document_creation() {
        let doc = SemanticDocument::new("doc1", "test content");
        assert_eq!(doc.id, "doc1");
        assert_eq!(doc.content, "test content");
        assert!(doc.embedding.is_none());
    }

    #[test]
    fn test_semantic_document_with_embedding() {
        let doc = SemanticDocument::new("doc1", "test content").with_embedding(vec![0.1, 0.2, 0.3]);
        assert_eq!(doc.embedding, Some(vec![0.1, 0.2, 0.3]));
    }

    #[test]
    fn test_search_config_default() {
        let config = SearchConfig::default();
        assert_eq!(config.hybrid_alpha, 0.7);
        assert_eq!(config.max_results, 10);
        assert_eq!(config.min_score, 0.1);
    }

    #[test]
    fn test_search_config_builder() {
        let config = SearchConfig::new()
            .hybrid_alpha(0.5)
            .max_results(20)
            .min_score(0.2);

        assert_eq!(config.hybrid_alpha, 0.5);
        assert_eq!(config.max_results, 20);
        assert_eq!(config.min_score, 0.2);
    }

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0];
        let b = vec![1.0, 0.0];
        // Same vectors = 1.0
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.001);

        let c = vec![0.0, 1.0];
        // Orthogonal = 0.0
        assert!((cosine_similarity(&a, &c) - 0.0).abs() < 0.001);

        let d = vec![0.707, 0.707];
        // 45 degrees = cos(45°) ≈ 0.707
        let sim = cosine_similarity(&a, &d);
        assert!((sim - 0.707).abs() < 0.01);
    }

    #[test]
    fn test_dot_similarity() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![2.0, 3.0, 4.0];
        // 1*2 + 2*3 + 3*4 = 2 + 6 + 12 = 20
        assert!((dot_similarity(&a, &b) - 20.0).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_semantic_search_creation() {
        let search = SemanticSearch::new();
        assert!(search.is_empty().await);
        assert_eq!(search.count().await, 0);
    }

    #[tokio::test]
    async fn test_index_and_search() {
        let search = SemanticSearch::new();

        // Index documents
        search.index("doc1", "temperature sensor").await.unwrap();
        search.index("doc2", "humidity level").await.unwrap();
        search.index("doc3", "temperature reading").await.unwrap();

        assert_eq!(search.count().await, 3);

        // Search
        let results = search.search("temperature").execute().await.unwrap();
        assert!(!results.is_empty());
    }

    #[tokio::test]
    async fn test_remove_document() {
        let search = SemanticSearch::new();

        search.index("doc1", "test content").await.unwrap();
        assert_eq!(search.count().await, 1);

        assert!(search.remove("doc1").await);
        assert_eq!(search.count().await, 0);

        assert!(!search.remove("nonexistent").await);
    }

    #[tokio::test]
    async fn test_clear() {
        let search = SemanticSearch::new();

        search.index("doc1", "test").await.unwrap();
        search.index("doc2", "test").await.unwrap();

        search.clear().await;
        assert!(search.is_empty().await);
    }

    #[tokio::test]
    async fn test_search_with_config() {
        let search = SemanticSearch::new();

        search
            .index("doc1", "temperature sensor high")
            .await
            .unwrap();
        search.index("doc2", "humidity sensor low").await.unwrap();

        // Search with custom config
        let results = search
            .search("sensor")
            .max_results(1)
            .execute()
            .await
            .unwrap();

        assert!(results.len() <= 1);
    }
}
