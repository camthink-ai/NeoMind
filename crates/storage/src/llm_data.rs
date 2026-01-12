//! LLM data storage with long-term memory and vector search.
//!
//! Provides:
//! - LongTermMemoryStore for persistent memory entries
//! - Type and keyword indexing
//! - Importance scoring and expiration
//! - Semantic search via vector integration

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use chrono::Utc;

use crate::{Error, Result};
use crate::vector::{VectorStore, VectorDocument};

// Table definitions
const MEMORY_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("llm_memory");

/// Memory entry for long-term storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Unique memory ID
    pub id: String,
    /// Memory type (fact, event, user_preference, context, etc.)
    pub memory_type: String,
    /// Memory content
    pub content: String,
    /// Source (user, system, device, etc.)
    pub source: String,
    /// Associated session ID (if any)
    pub session_id: Option<String>,
    /// Keywords for indexing
    pub keywords: Vec<String>,
    /// Importance score (0-100)
    pub importance: u8,
    /// Creation timestamp
    pub created_at: i64,
    /// Last access timestamp
    pub last_accessed: i64,
    /// Access count
    pub access_count: u32,
    /// Embedding for semantic search
    pub embedding: Option<Vec<f32>>,
    /// TTL in seconds (None = never expires)
    pub ttl_seconds: Option<u64>,
    /// Additional metadata
    pub metadata: Option<serde_json::Value>,
}

impl MemoryEntry {
    /// Create a new memory entry.
    pub fn new(
        id: String,
        memory_type: String,
        content: String,
        source: String,
    ) -> Self {
        let now = Utc::now().timestamp();
        Self {
            id,
            memory_type,
            content,
            source,
            session_id: None,
            keywords: Vec::new(),
            importance: 50, // Default importance
            created_at: now,
            last_accessed: now,
            access_count: 0,
            embedding: None,
            ttl_seconds: None,
            metadata: None,
        }
    }

    /// Set session ID.
    pub fn with_session(mut self, session_id: String) -> Self {
        self.session_id = Some(session_id);
        self
    }

    /// Set keywords.
    pub fn with_keywords(mut self, keywords: Vec<String>) -> Self {
        self.keywords = keywords;
        self
    }

    /// Set importance.
    pub fn with_importance(mut self, importance: u8) -> Self {
        self.importance = importance.min(100);
        self
    }

    /// Set embedding.
    pub fn with_embedding(mut self, embedding: Vec<f32>) -> Self {
        self.embedding = Some(embedding);
        self
    }

    /// Set TTL.
    pub fn with_ttl(mut self, ttl_seconds: u64) -> Self {
        self.ttl_seconds = Some(ttl_seconds);
        self
    }

    /// Set metadata.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Check if memory has expired.
    pub fn is_expired(&self) -> bool {
        if let Some(ttl) = self.ttl_seconds {
            let now = Utc::now().timestamp();
            now - self.created_at > ttl as i64
        } else {
            false
        }
    }

    /// Calculate expiration timestamp.
    pub fn expires_at(&self) -> Option<i64> {
        self.ttl_seconds.map(|ttl| self.created_at + ttl as i64)
    }
}

/// Memory filter for queries.
#[derive(Debug, Clone, Default)]
pub struct MemoryFilter {
    /// Filter by memory types
    pub memory_types: Vec<String>,
    /// Filter by source
    pub source: Option<String>,
    /// Filter by session ID
    pub session_id: Option<String>,
    /// Minimum importance
    pub min_importance: Option<u8>,
    /// Include expired memories
    pub include_expired: bool,
    /// Keyword filter (any match)
    pub keywords: Vec<String>,
    /// Time range start
    pub start_time: Option<i64>,
    /// Time range end
    pub end_time: Option<i64>,
    /// Maximum results
    pub limit: Option<usize>,
}

impl MemoryFilter {
    /// Create a new empty filter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add memory type filter.
    pub fn with_memory_type(mut self, memory_type: impl Into<String>) -> Self {
        self.memory_types.push(memory_type.into());
        self
    }

    /// Set source filter.
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Set session ID filter.
    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Set minimum importance.
    pub fn with_min_importance(mut self, importance: u8) -> Self {
        self.min_importance = Some(importance);
        self
    }

    /// Set expired inclusion.
    pub fn include_expired(mut self, include: bool) -> Self {
        self.include_expired = include;
        self
    }

    /// Add keyword filter.
    pub fn with_keyword(mut self, keyword: impl Into<String>) -> Self {
        self.keywords.push(keyword.into());
        self
    }

    /// Set time range.
    pub fn with_time_range(mut self, start: i64, end: i64) -> Self {
        self.start_time = Some(start);
        self.end_time = Some(end);
        self
    }

    /// Set result limit.
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }
}

/// Long-term memory store for LLM.
pub struct LongTermMemoryStore {
    db: Arc<Database>,
    /// Optional vector store for semantic search
    vector_store: Option<Arc<RwLock<VectorStore>>>,
    /// Type index: memory_type -> count
    type_index: Arc<RwLock<HashMap<String, usize>>>,
    /// Keyword index: keyword -> set of memory IDs
    keyword_index: Arc<RwLock<HashMap<String, HashSet<String>>>>,
}

impl LongTermMemoryStore {
    /// Open or create a memory store without vector search.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();
        let db = if path_ref.exists() {
            Database::open(path_ref)?
        } else {
            if let Some(parent) = path_ref.parent() {
                std::fs::create_dir_all(parent)?;
            }
            Database::create(path_ref)?
        };

        Ok(Self {
            db: Arc::new(db),
            vector_store: None,
            type_index: Arc::new(RwLock::new(HashMap::new())),
            keyword_index: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Open with vector search support.
    pub fn with_vector<P: AsRef<Path>>(path: P, vector_store: VectorStore) -> Result<Self> {
        let path_ref = path.as_ref();
        let db = if path_ref.exists() {
            Database::open(path_ref)?
        } else {
            if let Some(parent) = path_ref.parent() {
                std::fs::create_dir_all(parent)?;
            }
            Database::create(path_ref)?
        };

        // Rebuild indexes from existing data
        let type_index = Arc::new(RwLock::new(HashMap::new()));
        let keyword_index = Arc::new(RwLock::new(HashMap::new()));

        Ok(Self {
            db: Arc::new(db),
            vector_store: Some(Arc::new(RwLock::new(vector_store))),
            type_index,
            keyword_index,
        })
    }

    /// Store a memory entry.
    pub async fn store(&self, memory: &MemoryEntry) -> Result<()> {
        let key = memory.id.as_str();
        let value = serde_json::to_vec(memory)?;

        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(MEMORY_TABLE)?;
            table.insert(key, &*value)?;
        }
        txn.commit()?;

        // Update type index
        {
            let mut index = self.type_index.write().await;
            *index.entry(memory.memory_type.clone()).or_insert(0) += 1;
        }

        // Update keyword index
        {
            let mut index = self.keyword_index.write().await;
            for keyword in &memory.keywords {
                index.entry(keyword.clone())
                    .or_insert_with(HashSet::new)
                    .insert(memory.id.clone());
            }
        }

        // Update vector store if available
        if let (Some(vector_store), Some(embedding)) =
            (&self.vector_store, &memory.embedding) {
            let vs = vector_store.write().await;
            let doc = VectorDocument {
                id: memory.id.clone(),
                embedding: embedding.clone(),
                metadata: serde_json::to_value(memory).unwrap_or(serde_json::Value::Null),
            };
            vs.insert(doc).await?;
        }

        Ok(())
    }

    /// Get a memory entry by ID.
    pub async fn get(&self, id: &str) -> Result<Option<MemoryEntry>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(MEMORY_TABLE)?;

        match table.get(id)? {
            Some(value) => {
                let memory: MemoryEntry = serde_json::from_slice(value.value())?;

                // Check if expired
                if memory.is_expired() {
                    return Ok(None);
                }

                // Update access stats
                self.update_access(id).await;

                Ok(Some(memory))
            }
            None => Ok(None),
        }
    }

    /// Update access statistics for a memory.
    /// This is a best-effort update that doesn't fail the main operation.
    async fn update_access(&self, id: &str) {
        // Spawn a background task to update access stats
        let db = self.db.clone();
        let id = id.to_string();
        tokio::spawn(async move {
            // First, read the current memory
            let memory_data = {
                let txn = match db.begin_write() {
                    Ok(t) => t,
                    Err(_) => return,
                };
                let table = match txn.open_table(MEMORY_TABLE) {
                    Ok(t) => t,
                    Err(_) => return,
                };
                match table.get(&*id) {
                    Ok(Some(value)) => value.value().to_vec(),
                    _ => return,
                }
            };

            // Then update with a new transaction
            if let Ok(mut memory) = serde_json::from_slice::<MemoryEntry>(&memory_data) {
                memory.last_accessed = Utc::now().timestamp();
                memory.access_count += 1;

                if let Ok(updated) = serde_json::to_vec(&memory) {
                    if let Ok(txn) = db.begin_write() {
                        {
                            let mut table = match txn.open_table(MEMORY_TABLE) {
                                Ok(t) => t,
                                Err(_) => return,
                            };
                            let _ = table.insert(&*id, &*updated);
                        } // table dropped here
                        let _ = txn.commit();
                    }
                }
            }
        });
    }

    /// Query memories with filter.
    pub async fn query(&self, filter: &MemoryFilter) -> Result<Vec<MemoryEntry>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(MEMORY_TABLE)?;

        let mut results = Vec::new();

        for result in table.iter()? {
            let (_key, value) = result?;
            if let Ok(memory) = serde_json::from_slice::<MemoryEntry>(value.value()) {
                if self.matches_filter(&memory, filter) {
                    results.push(memory);
                    if let Some(limit) = filter.limit {
                        if results.len() >= limit {
                            break;
                        }
                    }
                }
            }
        }

        Ok(results)
    }

    /// Semantic search using vector embeddings.
    pub async fn semantic_search(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let vector_store = self.vector_store.as_ref()
            .ok_or_else(|| Error::NotFound("Vector store not configured".to_string()))?;

        let vs = vector_store.read().await;
        let query_vec = query_embedding.to_vec();
        let search_results = vs.search_with_threshold(&query_vec, limit, 0.5).await?;

        // Fetch full memory entries
        let mut results = Vec::new();
        for result in search_results {
            if let Some(memory) = self.get(&result.id).await? {
                results.push(memory);
            }
        }

        Ok(results)
    }

    /// Get memories by type.
    pub async fn get_by_type(&self, memory_type: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        let filter = MemoryFilter::new()
            .with_memory_type(memory_type)
            .with_limit(limit);
        self.query(&filter).await
    }

    /// Get memories by keyword.
    pub async fn get_by_keyword(&self, keyword: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        let filter = MemoryFilter::new()
            .with_keyword(keyword)
            .with_limit(limit);
        self.query(&filter).await
    }

    /// Get memories for a session.
    pub async fn get_by_session(&self, session_id: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        let filter = MemoryFilter::new()
            .with_session(session_id)
            .with_limit(limit);
        self.query(&filter).await
    }

    /// Delete a memory entry.
    pub async fn delete(&self, id: &str) -> Result<bool> {
        let txn = self.db.begin_write()?;
        let mut table = txn.open_table(MEMORY_TABLE)?;

        // Get memory before deleting to update indexes
        let memory_data = if let Some(value) = table.get(id)? {
            Some(value.value().to_vec())
        } else {
            None
        };

        let removed = table.remove(id)?.is_some();
        drop(table); // Drop table before commit

        if removed {
            // Update indexes
            if let Some(data) = memory_data {
                if let Ok(memory) = serde_json::from_slice::<MemoryEntry>(&data) {
                    // Update type index
                    let mut type_index = self.type_index.write().await;
                    if let Some(count) = type_index.get_mut(&memory.memory_type) {
                        *count = count.saturating_sub(1);
                        if *count == 0 {
                            type_index.remove(&memory.memory_type);
                        }
                    }

                    // Update keyword index
                    let mut keyword_index = self.keyword_index.write().await;
                    for keyword in &memory.keywords {
                        if let Some(ids) = keyword_index.get_mut(keyword) {
                            ids.remove(&id.to_string());
                            if ids.is_empty() {
                                keyword_index.remove(keyword);
                            }
                        }
                    }
                }
            }
        }

        txn.commit()?;
        Ok(removed)
    }

    /// Clean up expired memories.
    pub async fn cleanup_expired(&self) -> Result<usize> {
        let txn = self.db.begin_write()?;
        let table = txn.open_table(MEMORY_TABLE)?;

        let mut ids_to_delete: Vec<String> = Vec::new();
        let now = Utc::now().timestamp();

        for result in table.iter()? {
            let (key, value) = result?;
            if let Ok(memory) = serde_json::from_slice::<MemoryEntry>(value.value()) {
                if let Some(expires_at) = memory.expires_at() {
                    if expires_at < now {
                        ids_to_delete.push(key.value().to_string());
                    }
                }
            }
        }
        drop(table);

        let mut count = 0;
        if !ids_to_delete.is_empty() {
            let mut table = txn.open_table(MEMORY_TABLE)?;
            for id in &ids_to_delete {
                if table.remove(&**id)?.is_some() {
                    count += 1;
                }
            }
            drop(table);
        }

        txn.commit()?;

        // Note: Type and keyword indexes will be updated on next rebuild

        Ok(count)
    }

    /// Get memory statistics.
    pub async fn get_stats(&self) -> MemoryStats {
        let total_count = match self.db.begin_read() {
            Ok(txn) => match txn.open_table(MEMORY_TABLE) {
                Ok(table) => {
                    let mut count = 0;
                    let _ = table.iter().map(|x| { count += 1; x });
                    count
                },
                Err(_) => 0,
            },
            Err(_) => 0,
        };

        let type_counts = self.type_index.read().await.clone();
        let keyword_count = self.keyword_index.read().await.len();

        MemoryStats {
            total_count,
            type_counts,
            keyword_count,
        }
    }

    /// Rebuild indexes from stored data.
    pub async fn rebuild_indexes(&self) -> Result<usize> {
        let mut type_index: HashMap<String, usize> = HashMap::new();
        let mut keyword_index: HashMap<String, HashSet<String>> = HashMap::new();

        let txn = self.db.begin_read()?;
        let table = txn.open_table(MEMORY_TABLE)?;

        for result in table.iter()? {
            let (_key, value) = result?;
            if let Ok(memory) = serde_json::from_slice::<MemoryEntry>(value.value()) {
                *type_index.entry(memory.memory_type.clone()).or_insert(0) += 1;
                for keyword in &memory.keywords {
                    keyword_index.entry(keyword.clone())
                        .or_insert_with(HashSet::new)
                        .insert(memory.id.clone());
                }
            }
        }

        // Compute total before moving
        let total = type_index.values().sum();

        *self.type_index.write().await = type_index;
        *self.keyword_index.write().await = keyword_index;

        Ok(total)
    }

    /// Get all memory types.
    pub async fn list_types(&self) -> Vec<String> {
        self.type_index.read().await.keys().cloned().collect()
    }

    /// Get all keywords.
    pub async fn list_keywords(&self) -> Vec<String> {
        self.keyword_index.read().await.keys().cloned().collect()
    }

    fn matches_filter(&self, memory: &MemoryEntry, filter: &MemoryFilter) -> bool {
        // Check expiration
        if !filter.include_expired && memory.is_expired() {
            return false;
        }

        // Check memory types
        if !filter.memory_types.is_empty() && !filter.memory_types.contains(&memory.memory_type) {
            return false;
        }

        // Check source
        if let Some(ref source) = filter.source {
            if &memory.source != source {
                return false;
            }
        }

        // Check session
        if let Some(ref session_id) = filter.session_id {
            if memory.session_id.as_ref() != Some(session_id) {
                return false;
            }
        }

        // Check importance
        if let Some(min_importance) = filter.min_importance {
            if memory.importance < min_importance {
                return false;
            }
        }

        // Check keywords
        if !filter.keywords.is_empty() {
            let has_keyword = filter.keywords.iter()
                .any(|k| memory.keywords.contains(k) || memory.content.contains(k));
            if !has_keyword {
                return false;
            }
        }

        // Check time range
        if let Some(start) = filter.start_time {
            if memory.created_at < start {
                return false;
            }
        }
        if let Some(end) = filter.end_time {
            if memory.created_at > end {
                return false;
            }
        }

        true
    }
}

/// Memory statistics.
#[derive(Debug, Clone)]
pub struct MemoryStats {
    /// Total memory count
    pub total_count: usize,
    /// Count by memory type
    pub type_counts: HashMap<String, usize>,
    /// Total unique keywords
    pub keyword_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_entry_basic() {
        let entry = MemoryEntry::new(
            "mem-1".to_string(),
            "fact".to_string(),
            "The temperature is 25 degrees".to_string(),
            "user".to_string(),
        )
        .with_keywords(vec!["temperature".to_string(), "sensor".to_string()])
        .with_importance(80);

        assert_eq!(entry.id, "mem-1");
        assert_eq!(entry.memory_type, "fact");
        assert_eq!(entry.keywords.len(), 2);
        assert_eq!(entry.importance, 80);
        assert!(!entry.is_expired());
    }

    #[tokio::test]
    async fn test_memory_store() {
        let store = LongTermMemoryStore::open("/tmp/test_llm_memory.redb").unwrap();

        let memory = MemoryEntry::new(
            "mem-1".to_string(),
            "fact".to_string(),
            "Test memory".to_string(),
            "test".to_string(),
        );

        store.store(&memory).await.unwrap();

        let retrieved = store.get("mem-1").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().content, "Test memory");
    }

    #[tokio::test]
    async fn test_memory_filter() {
        let store = LongTermMemoryStore::open("/tmp/test_llm_memory2.redb").unwrap();

        let memory = MemoryEntry::new(
            "mem-1".to_string(),
            "event".to_string(),
            "Test event".to_string(),
            "system".to_string(),
        )
        .with_importance(90);

        store.store(&memory).await.unwrap();

        let filter = MemoryFilter::new()
            .with_memory_type("event")
            .with_min_importance(80);

        let results = store.query(&filter).await.unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_memory_cleanup() {
        let store = LongTermMemoryStore::open("/tmp/test_llm_memory3.redb").unwrap();

        // Create a memory with short TTL
        let memory = MemoryEntry::new(
            "mem-expire".to_string(),
            "temp".to_string(),
            "Temporary memory".to_string(),
            "test".to_string(),
        )
        .with_ttl(1); // 1 second TTL

        store.store(&memory).await.unwrap();

        // Wait for expiration
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        let cleaned = store.cleanup_expired().await.unwrap();
        assert_eq!(cleaned, 1);

        let retrieved = store.get("mem-expire").await.unwrap();
        assert!(retrieved.is_none());
    }
}
