//! Edge AI Storage Crate
//!
//! This crate provides storage capabilities for the NeoTalk platform.
//!
//! ## Features
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `redb` | ✅ | Persistent storage using redb |
//! | `memory` | ❌ | In-memory storage for testing |
//! | `hnsw` | ❌ | Vector search with HNSW |
//! | `all` | ❌ | All features |
//!
//! ## Storage Backends
//!
//! This crate provides pluggable storage backends through the `StorageBackend` trait:
//!
//! - **RedbBackend**: Persistent embedded database (default)
//! - **MemoryBackend**: In-memory storage for testing
//!
//! ## Example
//!
//! ```rust,no_run
//! use edge_ai_storage::{
//!     TimeSeriesStore, DataPoint,
//!     VectorStore, VectorDocument,
//!     SessionStore, SessionMessage,
//!     backends::create_backend,
//! };
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create a storage backend
//!     let backend = create_backend("redb", &serde_json::json!({"path": "./data"}))?;
//!
//!     // Time series storage
//!     let ts_store = TimeSeriesStore::memory()?;
//!     let point = DataPoint::new(1234567890, 23.5);
//!     ts_store.write("sensor1", "temperature", point).await?;
//!
//!     // Vector storage
//!     let vec_store = VectorStore::new();
//!     let doc = VectorDocument::new("doc1", vec![0.1, 0.2, 0.3, 0.4]);
//!     vec_store.insert(doc).await?;
//!
//!     // Session storage
//!     let session_store = SessionStore::open(":memory:")?;
//!     session_store.save_session_id("chat-1")?;
//!     let messages = vec![
//!         SessionMessage::user("Hello"),
//!         SessionMessage::assistant("Hi there!"),
//!     ];
//!     session_store.save_history("chat-1", &messages)?;
//!
//!     Ok(())
//! }
//! ```

// Storage backends module
pub mod backends;

pub mod singleton;
pub mod error;
pub mod timeseries;
pub mod vector;
pub mod session;
pub mod multimodal;
pub mod knowledge;
pub mod settings;
pub mod llm_backends;
pub mod decisions;
pub mod backend;
pub mod device_state;
pub mod business;
pub mod llm_data;
pub mod backup;
pub mod maintenance;
pub mod monitoring;

// Re-exports
pub use error::{Error, Result};

pub use timeseries::{
    DataPoint, TimeSeriesBucket, TimeSeriesResult, TimeSeriesStore,
    RetentionPolicy, PerformanceStats, BatchWriteRequest,
    RetentionPolicyCleanupResult, TimeSeriesConfig,
};

pub use vector::{
    Embedding, PersistentVectorStore, SearchResult, SimilarityMetric,
    VectorDocument, VectorStore,
};

pub use session::{
    SessionStore, SessionMessage, SessionMetadata,
};

pub use multimodal::{
    MultimodalStore, ImageMetadata, DocumentMetadata,
};

pub use settings::{
    SettingsStore, LlmSettings, LlmBackendType,
    MqttSettings,
    HassSettings, ExternalBroker, ConfigChangeEntry,
    SecurityLevel, SecurityWarning,
};

pub use llm_backends::{
    LlmBackendStore, LlmBackendInstance, BackendCapabilities,
    ConnectionTestResult, LlmBackendStats,
};

pub use decisions::{
    DecisionStore, StoredDecision, DecisionFilter, DecisionStats,
    DecisionType, DecisionPriority, DecisionStatus,
    StoredAction, ExecutionResult,
};

pub use device_state::{
    DeviceState, DeviceStateStore, DeviceFilter,
    DeviceCapabilities, MetricSpec, CommandSpec, ParameterSpec, ConfigSpec,
    MetricValue, MetricQuality, CacheStats,
};

pub use business::{
    EventLog, EventLogStore, EventSeverity, EventFilter,
    RuleExecution, RuleExecutionResult, RuleHistoryStore, RuleExecutionStats,
    WorkflowExecution, WorkflowStatus, StepExecution, WorkflowHistoryStore,
    Alert, AlertStatus, AlertStore, AlertFilter,
};

pub use llm_data::{
    MemoryEntry, MemoryFilter, MemoryStats, LongTermMemoryStore,
};

pub use backup::{
    BackupManager, BackupHandler, BackupMetadata, BackupConfig, BackupType,
};

pub use maintenance::{
    MaintenanceScheduler, MaintenanceConfig, MaintenanceResult, CleanupUtils,
};

pub use monitoring::{
    StorageMonitor, StorageMetrics, HealthStatus, HealthCheckResult,
    CheckResult, MonitoringConfig, AlertThresholds, OperationStats,
};

// Re-exports from core (backward compatibility)
pub use edge_ai_core::storage::{
    StorageBackend, StorageFactory, StorageError,
};

// Backends module exports
pub use backends::{
    create_backend, available_backends,
    RedbBackend, RedbBackendConfig,
};

// Singleton module exports
pub use singleton::{
    get_or_open_db, close_db, clear_cache, cache_size, is_cached,
};

#[cfg(feature = "memory")]
pub use backends::{MemoryBackend, MemoryBackendConfig};

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// # Changelog
///
/// ## v0.2.0 (2026-01) - Storage Migration
///
/// ### Breaking Changes
/// - **Migrated from sled to redb 2.1** - All database files have a new format
/// - `VectorStore` serialization changed from bincode to JSON (for `serde_json::Value` compatibility)
///
/// ### New Features
/// - **Session storage** - New `SessionStore` for chat history persistence
/// - **Multimodal storage** - New `MultimodalStore` for image and document storage
/// - **Composite key queries** - TimeSeriesStore now supports efficient range queries
///
/// ### Migration Notes
/// - Old sled databases are **not compatible** with redb
/// - Use `SessionStore::open(":memory:")` for in-memory storage
/// - Vector document metadata is now serialized as JSON

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }
}
