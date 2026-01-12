//! Vector storage for semantic search.
//!
//! Provides in-memory vector indexing with persistent storage using redb.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::Error;

// Vector table: key = document_id, value = VectorDocument (serialized)
const VECTORS_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("vectors");

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

/// Vector document with embedding and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorDocument {
    /// Unique identifier.
    pub id: String,
    /// Vector embedding.
    pub embedding: Embedding,
    /// Associated metadata.
    pub metadata: serde_json::Value,
}

impl VectorDocument {
    /// Create a new vector document.
    pub fn new(id: impl Into<String>, embedding: Embedding) -> Self {
        Self {
            id: id.into(),
            embedding,
            metadata: serde_json::json!({}),
        }
    }

    /// Set metadata.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    /// Get the embedding dimension.
    pub fn dimension(&self) -> usize {
        self.embedding.len()
    }
}

/// Similarity metric for vector comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimilarityMetric {
    /// Cosine similarity (default).
    Cosine,
    /// Euclidean distance (converted to similarity).
    Euclidean,
    /// Dot product.
    DotProduct,
}

impl Default for SimilarityMetric {
    fn default() -> Self {
        Self::Cosine
    }
}

/// In-memory vector store.
pub struct VectorStore {
    /// Stored documents indexed by ID.
    documents: Arc<RwLock<HashMap<String, VectorDocument>>>,
    /// Similarity metric to use.
    metric: SimilarityMetric,
    /// Embedding dimension (all vectors must have same dimension).
    dimension: Option<usize>,
}

impl VectorStore {
    /// Create a new in-memory vector store.
    pub fn new() -> Self {
        Self {
            documents: Arc::new(RwLock::new(HashMap::new())),
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
        if let Some(expected_dim) = self.dimension {
            if doc.embedding.len() != expected_dim {
                return Err(Error::InvalidDimension {
                    expected: expected_dim,
                    found: doc.embedding.len(),
                });
            }
        }

        let mut docs = self.documents.write().await;
        docs.insert(doc.id.clone(), doc);
        Ok(())
    }

    /// Insert multiple documents in batch.
    pub async fn insert_batch(&self, docs: Vec<VectorDocument>) -> Result<(), Error> {
        for doc in docs {
            self.insert(doc).await?;
        }
        Ok(())
    }

    /// Search for similar vectors.
    pub async fn search(
        &self,
        query: &Embedding,
        top_k: usize,
    ) -> Result<Vec<SearchResult>, Error> {
        let docs = self.documents.read().await;

        // Validate query dimension
        if let Some(expected_dim) = self.dimension {
            if query.len() != expected_dim {
                return Err(Error::InvalidDimension {
                    expected: expected_dim,
                    found: query.len(),
                });
            }
        }

        let mut results: Vec<(String, f32, serde_json::Value)> = docs
            .values()
            .map(|doc| {
                let score = match self.metric {
                    SimilarityMetric::Cosine => self.cosine_similarity(query, &doc.embedding),
                    SimilarityMetric::Euclidean => {
                        1.0 / (1.0 + self.euclidean_distance(query, &doc.embedding))
                    }
                    SimilarityMetric::DotProduct => self.dot_product(query, &doc.embedding),
                };
                (doc.id.clone(), score, doc.metadata.clone())
            })
            .collect();

        // Sort by score (descending)
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Return top_k
        results.truncate(top_k);
        Ok(results
            .into_iter()
            .map(|(id, score, metadata)| SearchResult { id, score, metadata })
            .collect())
    }

    /// Search with a minimum similarity threshold.
    pub async fn search_with_threshold(
        &self,
        query: &Embedding,
        top_k: usize,
        min_score: f32,
    ) -> Result<Vec<SearchResult>, Error> {
        let mut results = self.search(query, top_k).await?;
        results.retain(|r| r.score >= min_score);
        Ok(results)
    }

    /// Get a document by ID.
    pub async fn get(&self, id: &str) -> Option<VectorDocument> {
        let docs = self.documents.read().await;
        docs.get(id).cloned()
    }

    /// Delete a document.
    pub async fn delete(&self, id: &str) -> Result<bool, Error> {
        let mut docs = self.documents.write().await;
        Ok(docs.remove(id).is_some())
    }

    /// Get the number of documents in the store.
    pub async fn count(&self) -> usize {
        let docs = self.documents.read().await;
        docs.len()
    }

    /// Clear all documents.
    pub async fn clear(&self) {
        let mut docs = self.documents.write().await;
        docs.clear();
    }

    /// List all document IDs.
    pub async fn list_ids(&self) -> Vec<String> {
        let docs = self.documents.read().await;
        docs.keys().cloned().collect()
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
            if let Some(store) = singleton.as_ref() {
                if store.path == path_str {
                    return Ok(store.clone());
                }
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

        store.insert(VectorDocument::new("doc1", vec![1.0, 0.0, 0.0])).await.unwrap();
        store.insert(VectorDocument::new("doc2", vec![0.0, 1.0, 0.0])).await.unwrap();

        let query = vec![1.0, 0.0, 0.0];
        let results = store.search_with_threshold(&query, 10, 0.9).await.unwrap();

        assert_eq!(results.len(), 1); // Only doc1 is similar enough
    }

    #[tokio::test]
    async fn test_vector_store_delete() {
        let store = VectorStore::new();

        store.insert(VectorDocument::new("doc1", vec![1.0])).await.unwrap();
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
        store.insert(VectorDocument::new("orth1", vec![1.0, 0.0])).await.unwrap();
        store.insert(VectorDocument::new("orth2", vec![0.0, 1.0])).await.unwrap();

        // Same direction vectors - both should have cosine similarity of 1.0
        store.insert(VectorDocument::new("same", vec![1.0, 1.0])).await.unwrap();
        store.insert(VectorDocument::new("same2", vec![0.5, 0.5])).await.unwrap();

        let query = vec![1.0, 1.0];
        let results = store.search(&query, 4).await.unwrap();

        // First two should be "same" and "same2" (both have cosine similarity = 1.0)
        // Order between them is not deterministic since scores are equal
        let top_ids: Vec<_> = results.iter().take(2).map(|r| r.id.as_str()).collect();
        assert!(top_ids.contains(&"same") && top_ids.contains(&"same2"));

        // Both should have perfect cosine similarity (1.0)
        for result in results.iter().take(2) {
            assert!((result.score - 1.0).abs() < 0.001, "Score should be ~1.0, got {}", result.score);
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
    async fn test_persistent_vector_store() {
        let temp_path = std::env::temp_dir().join(format!("vector_test_{}.redb", uuid::Uuid::new_v4()));
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

