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

pub mod backend;
pub mod agents;
pub mod backup;
pub mod business;
pub mod dashboards;
pub mod device_registry;
pub mod device_state;
pub mod error;
pub mod knowledge;
pub mod llm_backends;
pub mod llm_data;
pub mod maintenance;
pub mod messages;
pub mod monitoring;
pub mod multimodal;
pub mod session;
pub mod settings;
pub mod singleton;
pub mod timeseries;
pub mod vector;

// Re-exports
pub use error::{Error, Result};

pub use timeseries::{
    BatchWriteRequest, DataPoint, PerformanceStats, RetentionPolicy, RetentionPolicyCleanupResult,
    TimeSeriesBucket, TimeSeriesConfig, TimeSeriesResult, TimeSeriesStore,
};

pub use vector::{
    Embedding, PersistentVectorStore, SearchResult, SimilarityMetric, VectorDocument, VectorStore,
};

pub use session::{SessionMessage, SessionMessageImage, SessionMetadata, SessionStore, PendingStreamState, StreamStage};

pub use multimodal::{DocumentMetadata, ImageMetadata, MultimodalStore};

pub use messages::{MessageStore, MessageStats, StoredMessage};

pub use settings::{
    ConfigChangeEntry, ExternalBroker, LlmBackendType, LlmSettings, MqttSettings,
    SecurityLevel, SecurityWarning, SettingsStore,
    // Timezone settings
    DEFAULT_GLOBAL_TIMEZONE, KEY_GLOBAL_TIMEZONE, KEY_LLM_CONFIG, KEY_MQTT_CONFIG,
};

pub use llm_backends::{
    BackendCapabilities, ConnectionTestResult, LlmBackendInstance, LlmBackendStats, LlmBackendStore,
};

pub use agents::{
    ActionExecuted, AgentExecutionRecord, AgentFilter, AgentMemory, AgentResource, AgentSchedule,
    AgentStats, AgentStore, AgentStatus, AiAgent, DataCollected, DataSummary, Decision,
    DecisionProcess, ExecutionFilter, ExecutionResult, ExecutionStatus, GeneratedReport,
    LearnedPattern, NotificationSent, ParsedIntent, ReasoningStep, ResourceType, ScheduleType,
    TrendPoint, IntentType,
    // Conversation types
    ConversationTurn, TurnInput, TurnOutput,
    UserMessage,
    // Hierarchical memory types
    WorkingMemory, ShortTermMemory, LongTermMemory, MemorySummary, ImportantMemory,
};

pub use device_state::{
    CacheStats, CommandSpec, ConfigSpec, DeviceCapabilities, DeviceFilter, DeviceState,
    DeviceStateStore, MetricQuality, MetricSpec, MetricValue, ParameterSpec,
};

pub use business::{
    Alert, AlertFilter, AlertStatus, AlertStore,
    EventSeverity, RuleExecution, RuleExecutionResult, RuleExecutionStats, RuleHistoryStore,
};

pub use llm_data::{LongTermMemoryStore, MemoryEntry, MemoryFilter, MemoryStats};

pub use backup::{BackupConfig, BackupHandler, BackupManager, BackupMetadata, BackupType};

pub use maintenance::{CleanupUtils, MaintenanceConfig, MaintenanceResult, MaintenanceScheduler};

pub use monitoring::{
    AlertThresholds, CheckResult, HealthCheckResult, HealthStatus, MonitoringConfig,
    OperationStats, StorageMetrics, StorageMonitor,
};

pub use device_registry::{
    CommandDefinition, CommandHistoryRecord, CommandStatus, ConnectionConfig, DeviceConfig,
    DeviceRegistryStore, DeviceTypeTemplate, MetricDataType, MetricDefinition, ParamMetricValue,
    ParameterDefinition,
};

pub use dashboards::{
    ComponentPosition, Dashboard, DashboardLayout, DashboardStore, DashboardTemplate,
    LayoutBreakpoints, RequiredResources, RowsValue, default_templates,
};

// Re-exports from core (backward compatibility)
pub use edge_ai_core::storage::{StorageBackend, StorageError, StorageFactory};

// Backends module exports
pub use backends::{RedbBackend, RedbBackendConfig, available_backends, create_backend};

// Singleton module exports
pub use singleton::{cache_size, clear_cache, close_db, get_or_open_db, is_cached};

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
