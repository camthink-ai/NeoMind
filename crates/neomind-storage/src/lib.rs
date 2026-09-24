//! Edge AI Storage Crate
//!
//! This crate provides storage capabilities for the NeoMind platform.
//!
//! ## Features
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `redb` | ✅ | Persistent storage using redb |
//! | `hnsw` | ❌ | Vector search with HNSW |
//! | `all` | ❌ | All features |
//!
//! ## Example
//!
//! ```rust,no_run
//! use neomind_storage::{
//!     TimeSeriesStore, DataPoint,
//!     VectorStore, VectorDocument,
//!     SessionStore, SessionMessage,
//! };
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
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

pub mod agents;
pub mod atomic_write;
pub mod backup;
pub mod business;
pub mod dashboards;
pub mod device_registry;
pub mod error;
pub mod extensions;
pub mod frontend_components;
pub mod instances;
pub mod llm_backends;
pub mod memory_config;
pub mod messages;
pub mod schema;
pub mod secret;
pub mod session;
pub mod settings;
pub mod system_memory;
pub mod timeseries;
pub mod vector;

// Re-exports
pub use error::{Error, Result};

pub use timeseries::{compress_series_adaptive, DataPoint, TimeSeriesStore};

pub use vector::{VectorDocument, VectorStore};

pub use session::{
    PendingStreamState, SessionMessage, SessionMessageImage, SessionStore, StreamStage,
};

pub use messages::{MessageStore, StoredMessage};

pub use settings::{
    AgentDefaults, DeviceDefaults, ExternalBroker, LlmBackendType, LlmSettings, MqttSettings,
    SecurityLevel, SettingsStore, DEFAULT_GLOBAL_TIMEZONE,
};

pub use llm_backends::{
    BackendCapabilities, ConnectionTestResult, LlmBackendInstance, LlmBackendStore,
    ReasoningCapabilities,
};

pub use instances::InstanceRecord;

pub use extensions::{ExtensionRecord, ExtensionStore};

pub use agents::{
    ActionExecuted, AgentExecutionRecord, AgentFilter, AgentMemory, AgentNotify, AgentResource,
    AgentSchedule, AgentStats, AgentStatus, AgentStore, AgentToolConfig, AiAgent, DataCollected,
    DataSummary, Decision, DecisionProcess, ExecutionJournal, ExecutionMode, ExecutionRecord,
    ExecutionResult, ExecutionStatus, GeneratedReport, IntentType, KnowledgeFileRef, MemoryMode,
    NotificationSent, NotifyOn, OperatorConfig, OperatorField, OperatorFieldType, ParsedIntent,
    ReasoningStep, ResourceType, ScheduleType, UserMessage, DEFAULT_MAX_CHAIN_DEPTH,
};

pub use device_registry::DeviceRegistryStore;

// System memory exports (Markdown-based)
pub use system_memory::{CategoryStats, MarkdownMemoryStore, MemoryCategory, MemoryFileInfo};

// Memory configuration exports
pub use memory_config::MemoryConfig;

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// # Changelog
///
/// Remove leftovers from earlier runs of a test helper that parks a store in
/// the temp directory.
///
/// Those helpers open a database by path and nothing removes it afterwards.
/// A `TempDir` would not fix it either: its cleanup runs in `Drop`, which a
/// killed process never reaches — and this suite gets killed. Pruning by age
/// bounds the temp directory however a run ends, and the age gate keeps
/// concurrent runs from deleting each other's live directories.
#[cfg(test)]
pub(crate) fn prune_stale_temp_entries(prefix: &str, max_age: std::time::Duration) {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with(prefix) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(modified) = meta.modified() else {
            continue;
        };
        if !now
            .duration_since(modified)
            .map(|age| age > max_age)
            .unwrap_or(false)
        {
            continue;
        }
        let path = entry.path();
        let _ = if meta.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
    }
}

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
