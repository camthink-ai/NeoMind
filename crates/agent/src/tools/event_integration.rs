//! Tool execution with event integration.
//!
//! This module provides a wrapper around the tool registry that:
//! - Publishes events when tools are executed
//! - Records tool execution history
//! - Handles tool errors with proper event publishing

use std::sync::Arc;
use std::collections::HashMap;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;

use edge_ai_core::eventbus::EventBus;
use edge_ai_core::event::NeoTalkEvent;
use edge_ai_tools::{ToolRegistry, ToolOutput, ToolError};

/// Maximum number of tool execution records to keep in history.
const MAX_HISTORY_SIZE: usize = 1000;

/// A record of a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionRecord {
    /// Unique execution ID
    pub id: String,
    /// Session ID (if applicable)
    pub session_id: Option<String>,
    /// Tool name
    pub tool_name: String,
    /// Tool arguments
    pub arguments: Value,
    /// Execution result (success/failure)
    pub success: bool,
    /// Result data (if successful)
    pub result_data: Option<Value>,
    /// Error message (if failed)
    pub error: Option<String>,
    /// Execution duration in milliseconds
    pub duration_ms: u64,
    /// Timestamp when execution started
    pub started_at: i64,
    /// Timestamp when execution completed
    pub completed_at: i64,
}

impl ToolExecutionRecord {
    /// Create a new successful execution record.
    pub fn success(
        tool_name: String,
        arguments: Value,
        result_data: Value,
        duration_ms: u64,
        started_at: i64,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: None,
            tool_name,
            arguments,
            success: true,
            result_data: Some(result_data),
            error: None,
            duration_ms,
            started_at,
            completed_at: started_at + (duration_ms as i64 / 1000),
        }
    }

    /// Create a new failed execution record.
    pub fn failure(
        tool_name: String,
        arguments: Value,
        error: String,
        duration_ms: u64,
        started_at: i64,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: None,
            tool_name,
            arguments,
            success: false,
            result_data: None,
            error: Some(error),
            duration_ms,
            started_at,
            completed_at: started_at + (duration_ms as i64 / 1000),
        }
    }

    /// Set the session ID for this record.
    pub fn with_session(mut self, session_id: String) -> Self {
        self.session_id = Some(session_id);
        self
    }
}

/// Tool execution history tracker.
#[derive(Debug, Clone)]
pub struct ToolExecutionHistory {
    /// History storage
    records: Arc<RwLock<Vec<ToolExecutionRecord>>>,
    /// Maximum size of history
    max_size: usize,
}

impl ToolExecutionHistory {
    /// Create a new history tracker with default max size.
    pub fn new() -> Self {
        Self::with_max_size(MAX_HISTORY_SIZE)
    }

    /// Create a new history tracker with custom max size.
    pub fn with_max_size(max_size: usize) -> Self {
        Self {
            records: Arc::new(RwLock::new(Vec::with_capacity(max_size))),
            max_size,
        }
    }

    /// Add a record to history.
    pub async fn add(&self, record: ToolExecutionRecord) {
        let mut records = self.records.write().await;
        records.push(record);

        // Trim if over max size
        if records.len() > self.max_size {
            let remove_count = records.len() - self.max_size;
            records.drain(0..remove_count);
        }
    }

    /// Get all records.
    pub async fn get_all(&self) -> Vec<ToolExecutionRecord> {
        self.records.read().await.clone()
    }

    /// Get records for a specific session.
    pub async fn get_for_session(&self, session_id: &str) -> Vec<ToolExecutionRecord> {
        let records = self.records.read().await;
        records
            .iter()
            .filter(|r| r.session_id.as_deref() == Some(session_id))
            .cloned()
            .collect()
    }

    /// Get records for a specific tool.
    pub async fn get_for_tool(&self, tool_name: &str) -> Vec<ToolExecutionRecord> {
        let records = self.records.read().await;
        records
            .iter()
            .filter(|r| r.tool_name == tool_name)
            .cloned()
            .collect()
    }

    /// Get records for a specific session and tool.
    pub async fn get_for_session_and_tool(
        &self,
        session_id: &str,
        tool_name: &str,
    ) -> Vec<ToolExecutionRecord> {
        let records = self.records.read().await;
        records
            .iter()
            .filter(|r| {
                r.session_id.as_deref() == Some(session_id) && r.tool_name == tool_name
            })
            .cloned()
            .collect()
    }

    /// Get only failed records.
    pub async fn get_failures(&self) -> Vec<ToolExecutionRecord> {
        let records = self.records.read().await;
        records
            .iter()
            .filter(|r| !r.success)
            .cloned()
            .collect()
    }

    /// Get statistics about tool executions.
    pub async fn get_stats(&self) -> ToolExecutionStats {
        let records = self.records.read().await;

        if records.is_empty() {
            return ToolExecutionStats::default();
        }

        let total_count = records.len();
        let success_count = records.iter().filter(|r| r.success).count();
        let failure_count = total_count - success_count;

        let mut tool_counts: HashMap<String, usize> = HashMap::new();
        for record in records.iter() {
            *tool_counts.entry(record.tool_name.clone()).or_insert(0) += 1;
        }

        let total_duration_ms: u64 = records.iter().map(|r| r.duration_ms).sum();
        let avg_duration_ms = if total_count > 0 {
            total_duration_ms / total_count as u64
        } else {
            0
        };

        ToolExecutionStats {
            total_count,
            success_count,
            failure_count,
            tool_counts,
            avg_duration_ms,
        }
    }

    /// Clear all history.
    pub async fn clear(&self) {
        self.records.write().await.clear();
    }
}

impl Default for ToolExecutionHistory {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about tool executions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionStats {
    /// Total number of executions
    pub total_count: usize,
    /// Number of successful executions
    pub success_count: usize,
    /// Number of failed executions
    pub failure_count: usize,
    /// Count per tool
    pub tool_counts: HashMap<String, usize>,
    /// Average execution duration in milliseconds
    pub avg_duration_ms: u64,
}

impl Default for ToolExecutionStats {
    fn default() -> Self {
        Self {
            total_count: 0,
            success_count: 0,
            failure_count: 0,
            tool_counts: HashMap::new(),
            avg_duration_ms: 0,
        }
    }
}

/// Tool registry wrapper with event integration.
///
/// This wraps a standard ToolRegistry and adds:
/// - Event publishing for tool executions
/// - Execution history tracking
/// - Error handling with event publishing
#[derive(Clone)]
pub struct EventIntegratedToolRegistry {
    /// Underlying tool registry
    inner: Arc<ToolRegistry>,
    /// Event bus for publishing tool events
    event_bus: Arc<EventBus>,
    /// Execution history
    history: ToolExecutionHistory,
}

impl EventIntegratedToolRegistry {
    /// Create a new event-integrated tool registry.
    pub fn new(registry: ToolRegistry, event_bus: EventBus) -> Self {
        Self {
            inner: Arc::new(registry),
            event_bus: Arc::new(event_bus),
            history: ToolExecutionHistory::new(),
        }
    }

    /// Create with custom history max size.
    pub fn with_history_size(
        registry: ToolRegistry,
        event_bus: EventBus,
        max_history_size: usize,
    ) -> Self {
        Self {
            inner: Arc::new(registry),
            event_bus: Arc::new(event_bus),
            history: ToolExecutionHistory::with_max_size(max_history_size),
        }
    }

    /// Get a reference to the inner registry.
    pub fn inner(&self) -> &ToolRegistry {
        &self.inner
    }

    /// Get the event bus.
    pub fn event_bus(&self) -> &EventBus {
        &self.event_bus
    }

    /// Get the execution history.
    pub fn history(&self) -> &ToolExecutionHistory {
        &self.history
    }

    /// Get the execution history (cloned).
    pub fn history_clone(&self) -> ToolExecutionHistory {
        self.history.clone()
    }

    /// List all tool names.
    pub fn list(&self) -> Vec<String> {
        self.inner.list()
    }

    /// Check if a tool exists.
    pub fn has(&self, name: &str) -> bool {
        self.inner.has(name)
    }

    /// Get all tool definitions.
    pub fn definitions(&self) -> Vec<edge_ai_tools::ToolDefinition> {
        self.inner.definitions()
    }

    /// Get tool definitions as JSON (for LLM).
    pub fn definitions_json(&self) -> Value {
        self.inner.definitions_json()
    }

    /// Execute a tool with event publishing.
    ///
    /// This method:
    /// 1. Publishes a tool execution start event
    /// 2. Executes the tool
    /// 3. Publishes a tool execution result event
    /// 4. Records the execution in history
    pub async fn execute(&self, name: &str, args: Value) -> Result<ToolOutput, ToolError> {
        let start_time = Instant::now();
        let started_at = chrono::Utc::now().timestamp();

        // Publish tool execution start event
        self.publish_tool_start(name, &args, started_at).await;

        // Execute the tool
        let result = self.inner.execute(name, args.clone()).await;
        let duration_ms = start_time.elapsed().as_millis() as u64;

        // Handle result and publish event
        match &result {
            Ok(output) => {
                self.publish_tool_success(name, &args, &output.data, duration_ms, started_at).await;

                // Record successful execution
                let record = ToolExecutionRecord::success(
                    name.to_string(),
                    args,
                    output.data.clone(),
                    duration_ms,
                    started_at,
                );
                self.history.add(record).await;
            }
            Err(error) => {
                self.publish_tool_failure(name, &args, error, duration_ms, started_at).await;

                // Record failed execution
                let record = ToolExecutionRecord::failure(
                    name.to_string(),
                    args,
                    error.to_string(),
                    duration_ms,
                    started_at,
                );
                self.history.add(record).await;
            }
        }

        result
    }

    /// Execute a tool with session tracking.
    pub async fn execute_with_session(
        &self,
        name: &str,
        args: Value,
        session_id: &str,
    ) -> Result<ToolOutput, ToolError> {
        let start_time = Instant::now();
        let started_at = chrono::Utc::now().timestamp();

        // Publish tool execution start event
        self.publish_tool_start_with_session(name, &args, session_id, started_at).await;

        // Execute the tool
        let result = self.inner.execute(name, args.clone()).await;
        let duration_ms = start_time.elapsed().as_millis() as u64;

        // Handle result and publish event
        match &result {
            Ok(output) => {
                self.publish_tool_success_with_session(
                    name,
                    &args,
                    &output.data,
                    session_id,
                    duration_ms,
                    started_at,
                ).await;

                // Record successful execution
                let record = ToolExecutionRecord::success(
                    name.to_string(),
                    args,
                    output.data.clone(),
                    duration_ms,
                    started_at,
                ).with_session(session_id.to_string());
                self.history.add(record).await;
            }
            Err(error) => {
                self.publish_tool_failure_with_session(
                    name,
                    &args,
                    error,
                    session_id,
                    duration_ms,
                    started_at,
                ).await;

                // Record failed execution
                let record = ToolExecutionRecord::failure(
                    name.to_string(),
                    args,
                    error.to_string(),
                    duration_ms,
                    started_at,
                ).with_session(session_id.to_string());
                self.history.add(record).await;
            }
        }

        result
    }

    /// Publish tool execution start event.
    async fn publish_tool_start(&self, tool_name: &str, args: &Value, timestamp: i64) {
        let event = NeoTalkEvent::ToolExecutionStart {
            tool_name: tool_name.to_string(),
            arguments: args.clone(),
            session_id: None,
            timestamp,
        };
        let _ = self.event_bus.publish_with_source(event, "tool_registry").await;
    }

    /// Publish tool execution start event with session.
    async fn publish_tool_start_with_session(
        &self,
        tool_name: &str,
        args: &Value,
        session_id: &str,
        timestamp: i64,
    ) {
        let event = NeoTalkEvent::ToolExecutionStart {
            tool_name: tool_name.to_string(),
            arguments: args.clone(),
            session_id: Some(session_id.to_string()),
            timestamp,
        };
        let _ = self.event_bus.publish_with_source(event, "tool_registry").await;
    }

    /// Publish tool execution success event.
    async fn publish_tool_success(
        &self,
        tool_name: &str,
        args: &Value,
        result: &Value,
        duration_ms: u64,
        timestamp: i64,
    ) {
        let event = NeoTalkEvent::ToolExecutionSuccess {
            tool_name: tool_name.to_string(),
            arguments: args.clone(),
            result: result.clone(),
            duration_ms,
            session_id: None,
            timestamp,
        };
        let _ = self.event_bus.publish_with_source(event, "tool_registry").await;
    }

    /// Publish tool execution success event with session.
    async fn publish_tool_success_with_session(
        &self,
        tool_name: &str,
        args: &Value,
        result: &Value,
        session_id: &str,
        duration_ms: u64,
        timestamp: i64,
    ) {
        let event = NeoTalkEvent::ToolExecutionSuccess {
            tool_name: tool_name.to_string(),
            arguments: args.clone(),
            result: result.clone(),
            duration_ms,
            session_id: Some(session_id.to_string()),
            timestamp,
        };
        let _ = self.event_bus.publish_with_source(event, "tool_registry").await;
    }

    /// Publish tool execution failure event.
    async fn publish_tool_failure(
        &self,
        tool_name: &str,
        args: &Value,
        error: &ToolError,
        duration_ms: u64,
        timestamp: i64,
    ) {
        let event = NeoTalkEvent::ToolExecutionFailure {
            tool_name: tool_name.to_string(),
            arguments: args.clone(),
            error: error.to_string(),
            error_type: error.error_type().to_string(),
            duration_ms,
            session_id: None,
            timestamp,
        };
        let _ = self.event_bus.publish_with_source(event, "tool_registry").await;
    }

    /// Publish tool execution failure event with session.
    async fn publish_tool_failure_with_session(
        &self,
        tool_name: &str,
        args: &Value,
        error: &ToolError,
        session_id: &str,
        duration_ms: u64,
        timestamp: i64,
    ) {
        let event = NeoTalkEvent::ToolExecutionFailure {
            tool_name: tool_name.to_string(),
            arguments: args.clone(),
            error: error.to_string(),
            error_type: error.error_type().to_string(),
            duration_ms,
            session_id: Some(session_id.to_string()),
            timestamp,
        };
        let _ = self.event_bus.publish_with_source(event, "tool_registry").await;
    }
}

/// Extension trait for ToolError to get error type.
trait ToolErrorExt {
    fn error_type(&self) -> ToolErrorType;
}

/// Types of tool errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolErrorType {
    /// Tool not found
    NotFound,
    /// Invalid arguments
    InvalidArguments,
    /// Execution error
    Execution,
    /// Other error
    Other,
}

impl std::fmt::Display for ToolErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "NotFound"),
            Self::InvalidArguments => write!(f, "InvalidArguments"),
            Self::Execution => write!(f, "Execution"),
            Self::Other => write!(f, "Other"),
        }
    }
}

impl ToolErrorExt for ToolError {
    fn error_type(&self) -> ToolErrorType {
        match self {
            ToolError::NotFound(_) => ToolErrorType::NotFound,
            ToolError::InvalidArguments(_) => ToolErrorType::InvalidArguments,
            ToolError::Execution(_) => ToolErrorType::Execution,
            _ => ToolErrorType::Other,
        }
    }
}

// We need to add the tool execution events to NeoTalkEvent in core/src/event.rs
// For now, we'll define them here and they can be moved to core later

/// Extension: Add tool execution events to NeoTalkEvent
///
/// NOTE: These events should be added to the core NeoTalkEvent enum
/// For now, we use a wrapper that can publish to the event bus
/// The actual event types would be:
///
/// ```text
/// // In NeoTalkEvent enum:
/// ToolExecutionStart {
///     tool_name: String,
///     arguments: Value,
///     session_id: Option<String>,
///     timestamp: i64,
/// },
/// ToolExecutionSuccess {
///     tool_name: String,
///     arguments: Value,
///     result: Value,
///     duration_ms: u64,
///     session_id: Option<String>,
///     timestamp: i64,
/// },
/// ToolExecutionFailure {
///     tool_name: String,
///     arguments: Value,
///     error: String,
///     error_type: String,
///     duration_ms: u64,
///     session_id: Option<String>,
///     timestamp: i64,
/// },
/// ```

// Temporary wrapper events for publishing to event bus
// These will be replaced once the core events are added
#[allow(dead_code)]
enum ToolEvent {
    ExecutionStart {
        tool_name: String,
        arguments: Value,
        session_id: Option<String>,
        timestamp: i64,
    },
    ExecutionSuccess {
        tool_name: String,
        arguments: Value,
        result: Value,
        duration_ms: u64,
        session_id: Option<String>,
        timestamp: i64,
    },
    ExecutionFailure {
        tool_name: String,
        arguments: Value,
        error: String,
        error_type: String,
        duration_ms: u64,
        session_id: Option<String>,
        timestamp: i64,
    },
}

// We need to add these to the NeoTalkEvent enum in core/src/event.rs
// For now, let's update that file to include these events

#[cfg(test)]
mod tests {
    use super::*;
    use edge_ai_tools::ToolRegistryBuilder;

    #[tokio::test]
    async fn test_execution_history_add() {
        let history = ToolExecutionHistory::new();

        let record = ToolExecutionRecord::success(
            "test_tool".to_string(),
            serde_json::json!({}),
            serde_json::json!({"result": "ok"}),
            100,
            chrono::Utc::now().timestamp(),
        );

        history.add(record.clone()).await;

        let records = history.get_all().await;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tool_name, "test_tool");
        assert!(records[0].success);
    }

    #[tokio::test]
    async fn test_execution_history_for_session() {
        let history = ToolExecutionHistory::new();

        let record1 = ToolExecutionRecord::success(
            "test_tool".to_string(),
            serde_json::json!({}),
            serde_json::json!({}),
            100,
            chrono::Utc::now().timestamp(),
        ).with_session("session_1".to_string());

        let record2 = ToolExecutionRecord::success(
            "test_tool".to_string(),
            serde_json::json!({}),
            serde_json::json!({}),
            100,
            chrono::Utc::now().timestamp(),
        ).with_session("session_2".to_string());

        history.add(record1).await;
        history.add(record2).await;

        let session1_records = history.get_for_session("session_1").await;
        assert_eq!(session1_records.len(), 1);
    }

    #[tokio::test]
    async fn test_execution_history_for_tool() {
        let history = ToolExecutionHistory::new();

        let record1 = ToolExecutionRecord::success(
            "tool_a".to_string(),
            serde_json::json!({}),
            serde_json::json!({}),
            100,
            chrono::Utc::now().timestamp(),
        );

        let record2 = ToolExecutionRecord::success(
            "tool_b".to_string(),
            serde_json::json!({}),
            serde_json::json!({}),
            100,
            chrono::Utc::now().timestamp(),
        );

        history.add(record1).await;
        history.add(record2).await;

        let tool_a_records = history.get_for_tool("tool_a").await;
        assert_eq!(tool_a_records.len(), 1);
        assert_eq!(tool_a_records[0].tool_name, "tool_a");
    }

    #[tokio::test]
    async fn test_execution_history_stats() {
        let history = ToolExecutionHistory::new();

        // Add some successful records
        for _ in 0..3 {
            let record = ToolExecutionRecord::success(
                "tool_a".to_string(),
                serde_json::json!({}),
                serde_json::json!({}),
                100,
                chrono::Utc::now().timestamp(),
            );
            history.add(record).await;
        }

        // Add a failed record
        let failed = ToolExecutionRecord::failure(
            "tool_b".to_string(),
            serde_json::json!({}),
            "test error".to_string(),
            50,
            chrono::Utc::now().timestamp(),
        );
        history.add(failed).await;

        let stats = history.get_stats().await;
        assert_eq!(stats.total_count, 4);
        assert_eq!(stats.success_count, 3);
        assert_eq!(stats.failure_count, 1);
        assert_eq!(*stats.tool_counts.get("tool_a").unwrap(), 3);
        assert_eq!(*stats.tool_counts.get("tool_b").unwrap(), 1);
    }

    #[tokio::test]
    async fn test_event_integrated_registry() {
        let registry = ToolRegistryBuilder::new()
            .with_standard_tools()
            .build();

        let event_bus = EventBus::new();
        let integrated = EventIntegratedToolRegistry::new(registry, event_bus);

        assert!(integrated.has("list_devices"));
        // At least 6 standard tools (may have more with additions)
        assert!(integrated.list().len() >= 6);
    }

    #[tokio::test]
    async fn test_event_integrated_registry_execute() {
        let registry = ToolRegistryBuilder::new()
            .with_standard_tools()
            .build();

        let event_bus = EventBus::new();
        let integrated = EventIntegratedToolRegistry::new(registry, event_bus);

        // Execute a tool
        let result = integrated.execute(
            "list_devices",
            serde_json::json!({})
        ).await;

        assert!(result.is_ok());
        assert!(result.unwrap().success);

        // Check history
        let history_records = integrated.history().get_all().await;
        assert_eq!(history_records.len(), 1);
        assert_eq!(history_records[0].tool_name, "list_devices");
        assert!(history_records[0].success);
    }

    #[tokio::test]
    async fn test_execution_record_failure() {
        let record = ToolExecutionRecord::failure(
            "test_tool".to_string(),
            serde_json::json!({"arg": "value"}),
            "Something went wrong".to_string(),
            200,
            1000,
        );

        assert!(!record.success);
        assert_eq!(record.error, Some("Something went wrong".to_string()));
        assert_eq!(record.duration_ms, 200);
        assert_eq!(record.arguments["arg"], "value");
    }
}
