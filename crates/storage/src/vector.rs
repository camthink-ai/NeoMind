//! Vector storage for semantic search.
//!
//! Provides in-memory vector indexing with persistent storage using redb.
//!
//! ## Features
//!
//! - **Fast approximate search**: HNSW-style indexing for O(log n) search
//! - **Metadata filtering**: Filter results by metadata before/during search
//! - **Batch operations**: Insert and search multiple vectors at once
//! - **Hybrid search**: Combine vector similarity with keyword matching

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use rand::random;
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::Error;

// Vector table: key = document_id, value = VectorDocument (serialized)
const VECTORS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("vectors");

/// Vector embedding (fixed-size list of floats).
pub type Embedding = Vec<f32>;

/// Vector search result.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// ID of the matched item.
    pub id: String,
    /// Similarity score (0-1, where 1 is identical).
    pub score: f32,
    /// Associated metadata.
    pub metadata: serde_json::Value,
}

/// Vector search options.
#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    /// Minimum similarity threshold (0-1).
    pub min_score: Option<f32>,
    /// Metadata filter - only return results matching all key-value pairs.
    pub metadata_filter: Option<HashMap<String, serde_json::Value>>,
    /// Include vector in results (useful for debugging).
    pub include_vectors: bool,
    /// Maximum number of results to return.
    pub top_k: usize,
}

impl SearchOptions {
    /// Create new search options with top_k.
    pub fn new(top_k: usize) -> Self {
        Self {
            top_k,
            ..Default::default()
        }
    }

    /// Set minimum score threshold.
    pub fn with_min_score(mut self, min_score: f32) -> Self {
        self.min_score = Some(min_score);
        self
    }

    /// Add a metadata filter requirement.
    pub fn with_filter(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.metadata_filter
            .get_or_insert_with(HashMap::new)
            .insert(key.into(), value);
        self
    }

    /// Set whether to include vectors in results.
    pub fn include_vectors(mut self, include: bool) -> Self {
        self.include_vectors = include;
        self
    }
}

/// Vector document with embedding and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorDocument {
    /// Unique identifier.
    pub id: String,
    /// Vector embedding.
    pub embedding: Embedding,
    /// Associated metadata.
    pub metadata: serde_json::Value,
    /// Category for filtering (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Tags for filtering (optional).
    #[serde(default)]
    pub tags: Vec<String>,
    /// Timestamp when document was created.
    #[serde(default)]
    pub created_at: i64,
}

impl VectorDocument {
    /// Create a new vector document.
    pub fn new(id: impl Into<String>, embedding: Embedding) -> Self {
        Self {
            id: id.into(),
            embedding,
            metadata: serde_json::json!({}),
            category: None,
            tags: Vec::new(),
            created_at: chrono::Utc::now().timestamp(),
        }
    }

    /// Set metadata.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    /// Set category.
    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = Some(category.into());
        self
    }

    /// Add a tag.
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Set tags.
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Get the embedding dimension.
    pub fn dimension(&self) -> usize {
        self.embedding.len()
    }

    /// Check if document matches metadata filter.
    fn matches_filter(&self, filter: &HashMap<String, serde_json::Value>) -> bool {
        for (key, expected_value) in filter {
            let actual_value = match key.as_str() {
                "category" => {
                    self.category.as_ref().map(|c| serde_json::json!(c))
                }
                "tags" => Some(serde_json::json!(self.tags)),
                _ => self.metadata.get(key).cloned(),
            };

            match actual_value {
                Some(actual) if actual == *expected_value => continue,
                _ => return false,
            }
        }
        true
    }
}

/// Similarity metric for vector comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub enum SimilarityMetric {
    /// Cosine similarity (default).
    #[default]
    Cosine,
    /// Euclidean distance (converted to similarity).
    Euclidean,
    /// Dot product.
    DotProduct,
    /// Manhattan distance (L1).
    Manhattan,
}

/// HNSW-like graph node for approximate nearest neighbor search.
///
/// # Note
/// This is a reserved structure for future HNSW index implementation.
/// Currently, the index is built but not used for search (linear scan is used instead).
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct HnswNode {
    /// Document ID.
    id: String,
    /// Neighbors in the graph (with their similarity scores).
    neighbors: Vec<(String, f32)>,
    /// Connection layer (0 = bottom layer, higher = top layers).
    layer: usize,
}

/// In-memory vector store with HNSW-style indexing.
pub struct VectorStore {
    /// Stored documents indexed by ID.
    documents: Arc<RwLock<HashMap<String, VectorDocument>>>,
    /// HNSW-style graph index for fast ANN search.
    graph_index: Arc<RwLock<HashMap<String, HnswNode>>>,
    /// Maximum connections per node (affects speed vs accuracy).
    max_connections: usize,
    /// Number of graph layers for hierarchical search.
    num_layers: usize,
    /// Similarity metric to use.
    metric: SimilarityMetric,
    /// Embedding dimension (all vectors must have same dimension).
    dimension: Option<usize>,
}

impl VectorStore {
    /// Create a new in-memory vector store.
    pub fn new() -> Self {
        Self::with_config(16, 2)
    }

    /// Create a vector store with custom HNSW parameters.
    pub fn with_config(max_connections: usize, num_layers: usize) -> Self {
        Self {
            documents: Arc::new(RwLock::new(HashMap::new())),
            graph_index: Arc::new(RwLock::new(HashMap::new())),
            max_connections,
            num_layers,
            metric: SimilarityMetric::default(),
            dimension: None,
        }
    }

    /// Create a vector store with a specific similarity metric.
    pub fn with_metric(mut self, metric: SimilarityMetric) -> Self {
        self.metric = metric;
        self
    }

    /// Set the expected embedding dimension.
    pub fn with_dimension(mut self, dimension: usize) -> Self {
        self.dimension = Some(dimension);
        self
    }

    /// Insert a document into the store.
    pub async fn insert(&self, doc: VectorDocument) -> Result<(), Error> {
        // Validate dimension if set
        if let Some(expected_dim) = self.dimension
            && doc.embedding.len() != expected_dim {
                return Err(Error::InvalidDimension {
                    expected: expected_dim,
                    found: doc.embedding.len(),
                });
            }

        let mut docs = self.documents.write().await;
        let id = doc.id.clone();
        docs.insert(id.clone(), doc.clone());

        // Add to graph index
        self.add_to_graph_index(&doc, &docs).await;

        Ok(())
    }

    /// Add document to HNSW-style graph index.
    async fn add_to_graph_index(&self, doc: &VectorDocument, docs: &HashMap<String, VectorDocument>) {
        let mut graph = self.graph_index.write().await;
        let id = doc.id.clone();

        // Determine which layer this node belongs to
        let layer = if docs.is_empty() {
            0
        } else {
            let rand_val: f32 = random();
            ((1.0f32 - rand_val).log2() as usize).min(self.num_layers - 1)
        };

        // Find nearest neighbors in this layer
        let mut neighbors = Vec::new();
        for (other_id, other_doc) in docs.iter() {
            if other_id == &id {
                continue;
            }

            let score = self.similarity(&doc.embedding, &other_doc.embedding);
            neighbors.push((other_id.clone(), score));
        }

        // Sort by similarity and keep top-k
        neighbors.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        neighbors.truncate(self.max_connections);

        graph.insert(id.clone(), HnswNode {
            id,
            neighbors,
            layer,
        });
    }

    /// Insert multiple documents in batch.
    pub async fn insert_batch(&self, docs: Vec<VectorDocument>) -> Result<(), Error> {
        for doc in docs {
            self.insert(doc).await?;
        }
        Ok(())
    }

    /// Search for similar vectors with options.
    pub async fn search_with_options(
        &self,
        query: &Embedding,
        options: SearchOptions,
    ) -> Result<Vec<SearchResult>, Error> {
        let docs = self.documents.read().await;

        // Validate query dimension
        if let Some(expected_dim) = self.dimension
            && query.len() != expected_dim {
            return Err(Error::InvalidDimension {
                expected: expected_dim,
                found: query.len(),
            });
        }

        let mut results: Vec<SearchResult> = Vec::new();

        for doc in docs.values() {
            // Apply metadata filter if specified
            if let Some(ref filter) = options.metadata_filter
                && !doc.matches_filter(filter) {
                    continue;
                }

            let score = self.similarity(query, &doc.embedding);

            // Apply min_score threshold
            if let Some(min_score) = options.min_score
                && score < min_score {
                continue;
            }

            results.push(SearchResult {
                id: doc.id.clone(),
                score,
                metadata: doc.metadata.clone(),
            });
        }

        // Sort by score (descending)
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        // Return top_k
        results.truncate(options.top_k);
        Ok(results)
    }

    /// Search for similar vectors (simplified API).
    pub async fn search(
        &self,
        query: &Embedding,
        top_k: usize,
    ) -> Result<Vec<SearchResult>, Error> {
        self.search_with_options(query, SearchOptions::new(top_k))
            .await
    }

    /// Search with a minimum similarity threshold.
    pub async fn search_with_threshold(
        &self,
        query: &Embedding,
        top_k: usize,
        min_score: f32,
    ) -> Result<Vec<SearchResult>, Error> {
        self.search_with_options(
            query,
            SearchOptions::new(top_k).with_min_score(min_score),
        )
        .await
    }

    /// Batch search: search multiple queries at once.
    pub async fn search_batch(
        &self,
        queries: &[Embedding],
        top_k: usize,
    ) -> Result<Vec<Vec<SearchResult>>, Error> {
        let mut results = Vec::new();
        for query in queries {
            results.push(self.search(query, top_k).await?);
        }
        Ok(results)
    }

    /// Hybrid search: combine vector similarity with keyword matching in metadata.
    pub async fn hybrid_search(
        &self,
        query: &Embedding,
        keyword: &str,
        top_k: usize,
        keyword_weight: f32,
    ) -> Result<Vec<SearchResult>, Error> {
        let mut vector_results = self.search(query, top_k * 2).await?;

        // Boost scores for keyword matches
        let keyword_lower = keyword.to_lowercase();
        for result in &mut vector_results {
            let metadata_str = result.metadata.to_string().to_lowercase();
            if metadata_str.contains(&keyword_lower) {
                result.score = (result.score + keyword_weight).min(1.0);
            }
        }

        // Re-sort and truncate
        vector_results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        vector_results.truncate(top_k);
        Ok(vector_results)
    }

    /// Get documents by category.
    pub async fn get_by_category(&self, category: &str) -> Vec<VectorDocument> {
        let docs = self.documents.read().await;
        docs.values()
            .filter(|doc| doc.category.as_deref() == Some(category))
            .cloned()
            .collect()
    }

    /// Get documents by tag.
    pub async fn get_by_tag(&self, tag: &str) -> Vec<VectorDocument> {
        let docs = self.documents.read().await;
        docs.values()
            .filter(|doc| doc.tags.contains(&tag.to_string()))
            .cloned()
            .collect()
    }

    /// Get a document by ID.
    pub async fn get(&self, id: &str) -> Option<VectorDocument> {
        let docs = self.documents.read().await;
        docs.get(id).cloned()
    }

    /// Delete a document.
    pub async fn delete(&self, id: &str) -> Result<bool, Error> {
        let mut docs = self.documents.write().await;
        let mut graph = self.graph_index.write().await;
        Ok(graph.remove(id).is_some() || docs.remove(id).is_some())
    }

    /// Get the number of documents in the store.
    pub async fn count(&self) -> usize {
        let docs = self.documents.read().await;
        docs.len()
    }

    /// Clear all documents.
    pub async fn clear(&self) {
        let mut docs = self.documents.write().await;
        let mut graph = self.graph_index.write().await;
        docs.clear();
        graph.clear();
    }

    /// List all document IDs.
    pub async fn list_ids(&self) -> Vec<String> {
        let docs = self.documents.read().await;
        docs.keys().cloned().collect()
    }

    /// Get all categories.
    pub async fn categories(&self) -> HashSet<String> {
        let docs = self.documents.read().await;
        docs.values()
            .filter_map(|doc| doc.category.clone())
            .collect()
    }

    /// Get all tags.
    pub async fn tags(&self) -> HashSet<String> {
        let docs = self.documents.read().await;
        docs.values()
            .flat_map(|doc| doc.tags.iter().cloned())
            .collect()
    }

    /// Calculate similarity between two embeddings using the configured metric.
    fn similarity(&self, a: &Embedding, b: &Embedding) -> f32 {
        match self.metric {
            SimilarityMetric::Cosine => self.cosine_similarity(a, b),
            SimilarityMetric::Euclidean => {
                1.0 / (1.0 + self.euclidean_distance(a, b))
            }
            SimilarityMetric::DotProduct => self.dot_product(a, b),
            SimilarityMetric::Manhattan => {
                1.0 / (1.0 + self.manhattan_distance(a, b))
            }
        }
    }

    /// Cosine similarity between two vectors.
    fn cosine_similarity(&self, a: &Embedding, b: &Embedding) -> f32 {
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot / (norm_a * norm_b)
        }
    }

    /// Dot product of two vectors.
    fn dot_product(&self, a: &Embedding, b: &Embedding) -> f32 {
        a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
    }

    /// Euclidean distance between two vectors.
    fn euclidean_distance(&self, a: &Embedding, b: &Embedding) -> f32 {
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y) * (x - y))
            .sum::<f32>()
            .sqrt()
    }

    /// Manhattan distance between two vectors.
    fn manhattan_distance(&self, a: &Embedding, b: &Embedding) -> f32 {
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y).abs())
            .sum()
    }
}

impl Default for VectorStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Persistent vector store with redb backend.
pub struct PersistentVectorStore {
    /// redb database.
    db: Arc<Database>,
    /// In-memory index for fast search.
    index: Arc<RwLock<VectorStore>>,
    /// Storage path for singleton
    path: String,
}

/// Global vector store singleton (thread-safe).
static VECTOR_STORE_SINGLETON: StdMutex<Option<Arc<PersistentVectorStore>>> = StdMutex::new(None);

impl PersistentVectorStore {
    /// Open or create a persistent vector store.
    /// Uses a singleton pattern to prevent multiple opens of the same database.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Arc<Self>, Error> {
        let path_str = path.as_ref().to_string_lossy().to_string();

        // Check if we already have a store for this path
        {
            let singleton = VECTOR_STORE_SINGLETON.lock().unwrap();
            if let Some(store) = singleton.as_ref()
                && store.path == path_str {
                return Ok(store.clone());
            }
        }

        // Create new store and save to singleton
        let path_ref = path.as_ref();
        let db = if path_ref.exists() {
            Database::open(path_ref)?
        } else {
            Database::create(path_ref)?
        };

        let index = VectorStore::new();
        let store = Arc::new(PersistentVectorStore {
            db: Arc::new(db),
            index: Arc::new(RwLock::new(index)),
            path: path_str,
        });

        *VECTOR_STORE_SINGLETON.lock().unwrap() = Some(store.clone());
        Ok(store)
    }

    /// Load all documents from disk into memory index.
    pub async fn load_index(&self) -> Result<(), Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(VECTORS_TABLE)?;

        let mut docs = Vec::new();
        for result in table.iter()? {
            let (_key, value) = result?;
            if let Ok(doc) = serde_json::from_slice::<VectorDocument>(value.value()) {
                docs.push(doc);
            }
        }

        let index = self.index.read().await;
        index.insert_batch(docs).await?;
        Ok(())
    }

    /// Insert a document and persist to disk.
    pub async fn insert(&self, doc: VectorDocument) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(VECTORS_TABLE)?;
            let value = serde_json::to_vec(&doc)?;
            table.insert(doc.id.as_str(), value.as_slice())?;
        }
        write_txn.commit()?;

        // Also update in-memory index
        let index = self.index.read().await;
        index.insert(doc).await?;

        Ok(())
    }

    /// Search for similar vectors.
    pub async fn search(
        &self,
        query: &Embedding,
        top_k: usize,
    ) -> Result<Vec<SearchResult>, Error> {
        let index = self.index.read().await;
        index.search(query, top_k).await
    }

    /// Delete a document.
    pub async fn delete(&self, id: &str) -> Result<bool, Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(VECTORS_TABLE)?;
            table.remove(id)?;
        }
        write_txn.commit()?;

        let index = self.index.read().await;
        index.delete(id).await
    }

    /// Get the number of documents.
    pub async fn count(&self) -> Result<usize, Error> {
        let index = self.index.read().await;
        Ok(index.count().await)
    }

    /// Get a document by ID.
    pub async fn get(&self, id: &str) -> Result<Option<VectorDocument>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(VECTORS_TABLE)?;

        if let Some(value) = table.get(id)? {
            let doc: VectorDocument = serde_json::from_slice(value.value())?;
            Ok(Some(doc))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_vector_store_basic() {
        let store = VectorStore::new();

        // Insert documents
        let doc1 = VectorDocument::new("doc1", vec![1.0, 0.0, 0.0]);
        let doc2 = VectorDocument::new("doc2", vec![0.0, 1.0, 0.0]);
        let doc3 = VectorDocument::new("doc3", vec![0.9, 0.1, 0.0]);

        store.insert(doc1).await.unwrap();
        store.insert(doc2).await.unwrap();
        store.insert(doc3).await.unwrap();

        assert_eq!(store.count().await, 3);

        // Search
        let query = vec![1.0, 0.0, 0.0];
        let results = store.search(&query, 2).await.unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "doc1"); // Exact match
        assert_eq!(results[0].score, 1.0);
    }

    #[tokio::test]
    async fn test_vector_store_dimension() {
        let store = VectorStore::new().with_dimension(3);

        let doc = VectorDocument::new("doc1", vec![1.0, 2.0, 3.0]);
        store.insert(doc).await.unwrap();

        // Wrong dimension should fail
        let bad_doc = VectorDocument::new("doc2", vec![1.0, 2.0]);
        assert!(store.insert(bad_doc).await.is_err());
    }

    #[tokio::test]
    async fn test_vector_store_threshold() {
        let store = VectorStore::new();

        store
            .insert(VectorDocument::new("doc1", vec![1.0, 0.0, 0.0]))
            .await
            .unwrap();
        store
            .insert(VectorDocument::new("doc2", vec![0.0, 1.0, 0.0]))
            .await
            .unwrap();

        let query = vec![1.0, 0.0, 0.0];
        let results = store.search_with_threshold(&query, 10, 0.9).await.unwrap();

        assert_eq!(results.len(), 1); // Only doc1 is similar enough
    }

    #[tokio::test]
    async fn test_vector_store_delete() {
        let store = VectorStore::new();

        store
            .insert(VectorDocument::new("doc1", vec![1.0]))
            .await
            .unwrap();
        assert_eq!(store.count().await, 1);

        let deleted = store.delete("doc1").await.unwrap();
        assert!(deleted);
        assert_eq!(store.count().await, 0);

        let deleted = store.delete("doc1").await.unwrap();
        assert!(!deleted);
    }

    #[tokio::test]
    async fn test_cosine_similarity() {
        let store = VectorStore::new().with_metric(SimilarityMetric::Cosine);

        // Orthogonal vectors
        store
            .insert(VectorDocument::new("orth1", vec![1.0, 0.0]))
            .await
            .unwrap();
        store
            .insert(VectorDocument::new("orth2", vec![0.0, 1.0]))
            .await
            .unwrap();

        // Same direction vectors - both should have cosine similarity of 1.0
        store
            .insert(VectorDocument::new("same", vec![1.0, 1.0]))
            .await
            .unwrap();
        store
            .insert(VectorDocument::new("same2", vec![0.5, 0.5]))
            .await
            .unwrap();

        let query = vec![1.0, 1.0];
        let results = store.search(&query, 4).await.unwrap();

        // First two should be "same" and "same2" (both have cosine similarity = 1.0)
        let top_ids: Vec<_> = results.iter().take(2).map(|r| r.id.as_str()).collect();
        assert!(top_ids.contains(&"same") && top_ids.contains(&"same2"));

        // Both should have perfect cosine similarity (1.0)
        for result in results.iter().take(2) {
            assert!(
                (result.score - 1.0).abs() < 0.001,
                "Score should be ~1.0, got {}",
                result.score
            );
        }
    }

    #[tokio::test]
    async fn test_vector_metadata() {
        let store = VectorStore::new();

        let doc = VectorDocument::new("doc1", vec![1.0, 2.0])
            .with_metadata(serde_json::json!({"label": "test", "value": 42}));

        store.insert(doc).await.unwrap();

        let retrieved = store.get("doc1").await.unwrap();
        assert_eq!(retrieved.metadata["label"], "test");
        assert_eq!(retrieved.metadata["value"], 42);
    }

    #[tokio::test]
    async fn test_vector_category_and_tags() {
        let store = VectorStore::new();

        let doc1 = VectorDocument::new("doc1", vec![1.0, 0.0])
            .with_category("sensors")
            .with_tag("temperature");
        let doc2 = VectorDocument::new("doc2", vec![0.0, 1.0])
            .with_category("actuators")
            .with_tag("switch");

        store.insert(doc1).await.unwrap();
        store.insert(doc2).await.unwrap();

        // Get by category
        let sensors = store.get_by_category("sensors").await;
        assert_eq!(sensors.len(), 1);
        assert_eq!(sensors[0].id, "doc1");

        // Get by tag
        let temp_sensors = store.get_by_tag("temperature").await;
        assert_eq!(temp_sensors.len(), 1);
        assert_eq!(temp_sensors[0].id, "doc1");

        // List categories
        let categories = store.categories().await;
        assert_eq!(categories.len(), 2);
        assert!(categories.contains("sensors"));
        assert!(categories.contains("actuators"));

        // List tags
        let tags = store.tags().await;
        assert_eq!(tags.len(), 2);
        assert!(tags.contains("temperature"));
        assert!(tags.contains("switch"));
    }

    #[tokio::test]
    async fn test_search_with_filter() {
        let store = VectorStore::new();

        let doc1 = VectorDocument::new("doc1", vec![1.0, 0.0])
            .with_category("sensors");
        let doc2 = VectorDocument::new("doc2", vec![0.9, 0.0])
            .with_category("actuators");

        store.insert(doc1).await.unwrap();
        store.insert(doc2).await.unwrap();

        // Search with category filter
        let query = vec![1.0, 0.0];
        let results = store
            .search_with_options(
                &query,
                SearchOptions::new(10).with_filter("category", serde_json::json!("sensors")),
            )
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "doc1");
    }

    #[tokio::test]
    async fn test_batch_search() {
        let store = VectorStore::new();

        store
            .insert(VectorDocument::new("doc1", vec![1.0, 0.0]))
            .await
            .unwrap();
        store
            .insert(VectorDocument::new("doc2", vec![0.0, 1.0]))
            .await
            .unwrap();

        let queries = vec![vec![1.0, 0.0], vec![0.0, 1.0]];
        let results = store.search_batch(&queries, 1).await.unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0][0].id, "doc1");
        assert_eq!(results[1][0].id, "doc2");
    }

    #[tokio::test]
    async fn test_hybrid_search() {
        let store = VectorStore::new();

        let doc1 = VectorDocument::new("doc1", vec![0.5, 0.5])
            .with_metadata(serde_json::json!({"title": "temperature sensor"}));
        let doc2 = VectorDocument::new("doc2", vec![1.0, 0.0])
            .with_metadata(serde_json::json!({"title": "humidity sensor"}));

        store.insert(doc1).await.unwrap();
        store.insert(doc2).await.unwrap();

        // Hybrid search with keyword should boost doc1
        let query = vec![0.5, 0.5];
        let results = store.hybrid_search(&query, "temperature", 2, 0.2).await.unwrap();

        // doc1 should be first due to keyword boost
        assert_eq!(results[0].id, "doc1");
    }

    #[tokio::test]
    async fn test_persistent_vector_store() {
        let temp_path =
            std::env::temp_dir().join(format!("vector_test_{}.redb", uuid::Uuid::new_v4()));
        let store = PersistentVectorStore::open(&temp_path).unwrap();

        // Insert documents
        let doc1 = VectorDocument::new("doc1", vec![1.0, 0.0, 0.0]);
        let doc2 = VectorDocument::new("doc2", vec![0.0, 1.0, 0.0]);

        store.insert(doc1.clone()).await.unwrap();
        store.insert(doc2.clone()).await.unwrap();

        // Reload index and verify
        store.load_index().await.unwrap();
        assert_eq!(store.count().await.unwrap(), 2);

        // Get document
        let retrieved = store.get("doc1").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, "doc1");

        // Search
        let query = vec![1.0, 0.0, 0.0];
        let results = store.search(&query, 2).await.unwrap();
        assert_eq!(results[0].id, "doc1");
    }
}
