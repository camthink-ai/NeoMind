//! LLM Knowledge base with vector storage.
//!
//! Provides semantic indexing and search for MDL definitions and DSL rules.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::{vector::{VectorStore, VectorDocument, Embedding}, Error};

/// Knowledge entry type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum KnowledgeType {
    /// MDL device type definition.
    MdlDevice,
    /// DSL rule definition.
    DslRule,
    /// General knowledge.
    General,
}

/// Knowledge entry in the vector store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEntry {
    /// Unique identifier.
    pub id: String,
    /// Entry type.
    pub entry_type: KnowledgeType,
    /// Title/name.
    pub title: String,
    /// Text content for embedding.
    pub content: String,
    /// Additional metadata.
    pub metadata: HashMap<String, String>,
}

impl KnowledgeEntry {
    /// Create a new knowledge entry.
    pub fn new(
        id: impl Into<String>,
        entry_type: KnowledgeType,
        title: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            entry_type,
            title: title.into(),
            content: content.into(),
            metadata: HashMap::new(),
        }
    }

    /// Add metadata.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Generate text representation for embedding.
    pub fn to_embedding_text(&self) -> String {
        format!("{}: {}", self.title, self.content)
    }
}

/// LLM Knowledge base with vector storage.
pub struct LlmKnowledgeBase {
    /// Vector store for semantic search.
    vector_store: Arc<RwLock<VectorStore>>,
    /// In-memory index of entries by ID.
    entries: Arc<RwLock<HashMap<String, KnowledgeEntry>>>,
    /// Embedding dimension.
    embedding_dim: usize,
}

impl LlmKnowledgeBase {
    /// Create a new knowledge base.
    pub fn new() -> Self {
        Self {
            vector_store: Arc::new(RwLock::new(VectorStore::new())),
            entries: Arc::new(RwLock::new(HashMap::new())),
            embedding_dim: 384, // Default embedding dimension
        }
    }

    /// Set the embedding dimension.
    pub fn with_embedding_dim(mut self, dim: usize) -> Self {
        self.embedding_dim = dim;
        self
    }

    /// Initialize with a vector store.
    pub fn with_vector_store(mut self, store: VectorStore) -> Self {
        self.vector_store = Arc::new(RwLock::new(store));
        self
    }

    /// Index an MDL device type definition.
    pub async fn index_mdl(
        &self,
        device_type: &str,
        name: &str,
        description: &str,
        metrics: &[String],
        commands: &[String],
    ) -> Result<(), Error> {
        let id = format!("mdl:{}", device_type);

        let content = format!(
            "{}. Metrics: {}. Commands: {}.",
            description,
            metrics.join(", "),
            commands.join(", ")
        );

        let entry = KnowledgeEntry::new(id.clone(), KnowledgeType::MdlDevice, name, content)
            .with_metadata("device_type", device_type)
            .with_metadata("metrics", &metrics.join(","))
            .with_metadata("commands", &commands.join(","));

        self.insert_entry(entry).await?;

        Ok(())
    }

    /// Index a DSL rule definition.
    pub async fn index_dsl_rule(
        &self,
        rule_id: &str,
        name: &str,
        condition: &str,
        actions: &[String],
    ) -> Result<(), Error> {
        let id = format!("dsl:{}", rule_id);

        let content = format!(
            "When {}, then: {}.",
            condition,
            actions.join(", ")
        );

        let entry = KnowledgeEntry::new(id.clone(), KnowledgeType::DslRule, name, content)
            .with_metadata("rule_id", rule_id)
            .with_metadata("condition", condition)
            .with_metadata("actions", &actions.join(","));

        self.insert_entry(entry).await?;

        Ok(())
    }

    /// Insert a knowledge entry with embedding.
    pub async fn insert_entry(&self, entry: KnowledgeEntry) -> Result<(), Error> {
        // Generate a simple hash-based embedding (in production, use a real embedding model)
        let embedding = self.generate_embedding(&entry.to_embedding_text()).await;

        let doc = VectorDocument::new(entry.id.clone(), embedding)
            .with_metadata(serde_json::json!({
                "entry_type": format!("{:?}", entry.entry_type),
                "title": entry.title,
                "metadata": entry.metadata,
            }));

        let mut entries = self.entries.write().await;
        entries.insert(entry.id.clone(), entry);
        drop(entries);

        let store = self.vector_store.read().await;
        store.insert(doc).await?;

        Ok(())
    }

    /// Generate a simple embedding from text (hash-based for demo).
    /// In production, this would call an embedding model API.
    async fn generate_embedding(&self, text: &str) -> Embedding {
        // Simple hash-based embedding for demonstration
        // In production, use a real embedding model like sentence-transformers
        let mut embedding = vec![0.0f32; self.embedding_dim];
        let bytes = text.as_bytes();

        for (i, &byte) in bytes.iter().enumerate() {
            let idx = (i * 7) % self.embedding_dim;
            embedding[idx] += ((byte as f32) / 255.0) * 0.1;
        }

        // Add some variation based on character patterns
        for (i, c) in text.chars().enumerate() {
            let idx = (i * 13) % self.embedding_dim;
            embedding[idx] += ((c as u32 % 100) as f32) / 1000.0;
        }

        // Normalize
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in embedding.iter_mut() {
                *v /= norm;
            }
        }

        embedding
    }

    /// Semantic search with optional type filter.
    pub async fn search(
        &self,
        query: &str,
        top_k: usize,
        filter_type: Option<KnowledgeType>,
    ) -> Result<Vec<KnowledgeSearchResult>, Error> {
        let query_embedding = self.generate_embedding(query).await;

        let store = self.vector_store.read().await;
        let raw_results = store.search(&query_embedding, top_k * 2).await?;

        let entries = self.entries.read().await;

        let mut results = Vec::new();
        for result in raw_results {
            if let Some(entry) = entries.get(&result.id) {
                // Filter by type if specified
                if let Some(ref ft) = filter_type {
                    if &entry.entry_type != ft {
                        continue;
                    }
                }

                results.push(KnowledgeSearchResult {
                    entry: entry.clone(),
                    score: result.score,
                });
            }
        }

        // Sort by score and limit to top_k
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(top_k);

        Ok(results)
    }

    /// Search only MDL device types.
    pub async fn search_mdl(&self, query: &str, top_k: usize) -> Result<Vec<KnowledgeSearchResult>, Error> {
        self.search(query, top_k, Some(KnowledgeType::MdlDevice)).await
    }

    /// Search only DSL rules.
    pub async fn search_dsl(&self, query: &str, top_k: usize) -> Result<Vec<KnowledgeSearchResult>, Error> {
        self.search(query, top_k, Some(KnowledgeType::DslRule)).await
    }

    /// Get an entry by ID.
    pub async fn get(&self, id: &str) -> Option<KnowledgeEntry> {
        let entries = self.entries.read().await;
        entries.get(id).cloned()
    }

    /// Get all entries of a specific type.
    pub async fn get_by_type(&self, entry_type: KnowledgeType) -> Vec<KnowledgeEntry> {
        let entries = self.entries.read().await;
        entries
            .values()
            .filter(|e| e.entry_type == entry_type)
            .cloned()
            .collect()
    }

    /// Get the number of entries.
    pub async fn count(&self) -> usize {
        let entries = self.entries.read().await;
        entries.len()
    }

    /// Get the count by type.
    pub async fn count_by_type(&self, entry_type: KnowledgeType) -> usize {
        let entries = self.entries.read().await;
        entries.values().filter(|e| e.entry_type == entry_type).count()
    }

    /// Clear all entries.
    pub async fn clear(&self) {
        let mut entries = self.entries.write().await;
        entries.clear();

        let store = self.vector_store.read().await;
        store.clear().await;
    }

    /// Delete an entry by ID.
    pub async fn delete(&self, id: &str) -> Result<bool, Error> {
        let mut entries = self.entries.write().await;
        if entries.remove(id).is_some() {
            let store = self.vector_store.read().await;
            store.delete(id).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Get the vector store reference.
    pub fn vector_store(&self) -> Arc<RwLock<VectorStore>> {
        self.vector_store.clone()
    }
}

impl Default for LlmKnowledgeBase {
    fn default() -> Self {
        Self::new()
    }
}

/// Knowledge search result with entry and score.
#[derive(Debug, Clone)]
pub struct KnowledgeSearchResult {
    /// The matching knowledge entry.
    pub entry: KnowledgeEntry,
    /// Similarity score (0-1).
    pub score: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_knowledge_entry_creation() {
        let entry = KnowledgeEntry::new(
            "test-1",
            KnowledgeType::MdlDevice,
            "Temperature Sensor",
            "Measures temperature in degrees Celsius",
        );

        assert_eq!(entry.id, "test-1");
        assert_eq!(entry.title, "Temperature Sensor");
    }

    #[tokio::test]
    async fn test_knowledge_base_index_mdl() {
        let kb = LlmKnowledgeBase::new();

        kb.index_mdl(
            "dht22",
            "DHT22 Sensor",
            "Temperature and humidity sensor",
            &["temperature".to_string(), "humidity".to_string()],
            &["reset".to_string()],
        ).await.unwrap();

        assert_eq!(kb.count().await, 1);
        assert_eq!(kb.count_by_type(KnowledgeType::MdlDevice).await, 1);
    }

    #[tokio::test]
    async fn test_knowledge_base_index_dsl() {
        let kb = LlmKnowledgeBase::new();

        kb.index_dsl_rule(
            "rule-1",
            "High Temperature Alert",
            "temperature > 50",
            &["notify".to_string(), "log".to_string()],
        ).await.unwrap();

        assert_eq!(kb.count().await, 1);
        assert_eq!(kb.count_by_type(KnowledgeType::DslRule).await, 1);
    }

    #[tokio::test]
    async fn test_knowledge_base_search() {
        let kb = LlmKnowledgeBase::new();

        kb.index_mdl(
            "dht22",
            "DHT22 Sensor",
            "Temperature and humidity sensor",
            &["temperature".to_string(), "humidity".to_string()],
            &["reset".to_string()],
        ).await.unwrap();

        kb.index_dsl_rule(
            "rule-1",
            "High Temperature Alert",
            "temperature > 50",
            &["notify".to_string()],
        ).await.unwrap();

        // Search for temperature related
        let results = kb.search("temperature sensor", 10, None).await.unwrap();
        assert!(!results.is_empty());
    }

    #[tokio::test]
    async fn test_knowledge_base_search_by_type() {
        let kb = LlmKnowledgeBase::new();

        kb.index_mdl(
            "dht22",
            "DHT22 Sensor",
            "Temperature sensor",
            &["temperature".to_string()],
            &[],
        ).await.unwrap();

        kb.index_dsl_rule(
            "rule-1",
            "High Temp Alert",
            "temperature > 50",
            &["notify".to_string()],
        ).await.unwrap();

        // Search only MDL
        let mdl_results = kb.search_mdl("temperature", 10).await.unwrap();
        assert_eq!(mdl_results.len(), 1);
        assert_eq!(mdl_results[0].entry.entry_type, KnowledgeType::MdlDevice);

        // Search only DSL
        let dsl_results = kb.search_dsl("temperature", 10).await.unwrap();
        assert_eq!(dsl_results.len(), 1);
        assert_eq!(dsl_results[0].entry.entry_type, KnowledgeType::DslRule);
    }

    #[tokio::test]
    async fn test_knowledge_entry_metadata() {
        let entry = KnowledgeEntry::new("test", KnowledgeType::General, "Test", "Content")
            .with_metadata("key1", "value1")
            .with_metadata("key2", "value2");

        assert_eq!(entry.metadata.len(), 2);
        assert_eq!(entry.metadata.get("key1"), Some(&"value1".to_string()));
    }

    #[tokio::test]
    async fn test_knowledge_base_delete() {
        let kb = LlmKnowledgeBase::new();

        kb.index_mdl("dht22", "DHT22", "Sensor", &[], &[]).await.unwrap();
        assert_eq!(kb.count().await, 1);

        let deleted = kb.delete("mdl:dht22").await.unwrap();
        assert!(deleted);
        assert_eq!(kb.count().await, 0);

        let deleted = kb.delete("mdl:dht22").await.unwrap();
        assert!(!deleted);
    }

    #[tokio::test]
    async fn test_knowledge_base_get() {
        let kb = LlmKnowledgeBase::new();

        kb.index_mdl("dht22", "DHT22", "Sensor", &[], &[]).await.unwrap();

        let entry = kb.get("mdl:dht22").await;
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().title, "DHT22");

        let entry = kb.get("nonexistent").await;
        assert!(entry.is_none());
    }

    #[tokio::test]
    async fn test_embedding_generation() {
        let kb = LlmKnowledgeBase::new().with_embedding_dim(128);

        let emb1 = kb.generate_embedding("hello world").await;
        let emb2 = kb.generate_embedding("hello world").await;
        let emb3 = kb.generate_embedding("goodbye").await;

        assert_eq!(emb1.len(), 128);
        assert_eq!(emb2.len(), 128);

        // Same text should generate same embedding
        for i in 0..128 {
            assert!((emb1[i] - emb2[i]).abs() < 0.001);
        }

        // Different text should generate different embedding
        let mut diff_count = 0;
        for i in 0..128 {
            if (emb1[i] - emb3[i]).abs() > 0.01 {
                diff_count += 1;
            }
        }
        assert!(diff_count > 10, "Embeddings should be different");
    }
}
