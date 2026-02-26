//! Command Executor - V2 (Device-Standard Compatible)
//!
//! This module provides the central execution service for all extension commands.
//! It handles:
//! - Command execution with timeout
//! - Event publishing for command lifecycle
//! - Metric collection (separate from command execution)

use super::system::{
    DynExtension, ExtensionCommand, ExtensionError, ExtensionMetricValue, ParamMetricValue, Result,
};
use crate::datasource::{DataPoint, DataSourceId};
use crate::event::{EventMetadata, MetricValue, NeoMindEvent};
use crate::eventbus::EventBus;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::warn;

/// Convert ParamMetricValue to MetricValue for storage
fn param_to_metric_value(value: &ParamMetricValue) -> MetricValue {
    match value {
        ParamMetricValue::Float(v) => MetricValue::Float(*v),
        ParamMetricValue::Integer(v) => MetricValue::Integer(*v),
        ParamMetricValue::Boolean(v) => MetricValue::Boolean(*v),
        ParamMetricValue::String(v) => MetricValue::String(v.clone()),
        ParamMetricValue::Binary(v) => MetricValue::String(format!("<binary {} bytes>", v.len())),
        ParamMetricValue::Null => MetricValue::String("null".to_string()),
    }
}

// ============================================================================
// Command Executor
// ============================================================================

/// Command execution service
///
/// Unified executor for all extension commands with:
/// - Timeout handling
/// - Event publishing
/// - NO automatic metric storage (metrics collected separately)
pub struct CommandExecutor {
    event_bus: Arc<EventBus>,
    storage: Arc<dyn UnifiedStorage + Send + Sync>,
}

impl CommandExecutor {
    pub fn new(event_bus: Arc<EventBus>, storage: Arc<dyn UnifiedStorage + Send + Sync>) -> Self {
        Self { event_bus, storage }
    }

    /// Execute a command on an extension
    ///
    /// In V2, commands do NOT auto-store data.
    /// Metric storage is handled separately via `produce_metrics()`.
    pub async fn execute_command(
        &self,
        extension_id: &str,
        extension_name: &str,
        command: &ExtensionCommand,
        args: &serde_json::Value,
        extension: &DynExtension,
    ) -> Result<CommandResult> {
        let start = Instant::now();
        let execution_id = uuid::Uuid::new_v4().to_string();

        // Publish start event
        self.publish_start_event(
            extension_id,
            extension_name,
            &command.name,
            &execution_id,
            args,
        )
        .await;

        // Execute with timeout
        let result: std::result::Result<Result<serde_json::Value>, tokio::time::error::Elapsed> =
            tokio::time::timeout(
                Duration::from_secs(30), // Default timeout
                async {
                    let ext = extension.read().await;
                    ext.execute_command(&command.name, args).await
                },
            )
            .await;

        let duration = start.elapsed();

        match result {
            Ok(inner_result) => match inner_result {
                Ok(output) => {
                    // Publish success event (no auto-storage)
                    self.publish_success_event(
                        extension_id,
                        extension_name,
                        &command.name,
                        &execution_id,
                        args,
                        &output,
                        duration.as_millis() as u64,
                    )
                    .await;

                    Ok(CommandResult {
                        success: true,
                        output,
                        duration_ms: duration.as_millis() as u64,
                    })
                }
                Err(e) => {
                    self.publish_error_event(
                        extension_id,
                        extension_name,
                        &command.name,
                        &execution_id,
                        &e.to_string(),
                        duration.as_millis() as u64,
                    )
                    .await;
                    Err(e)
                }
            },
            Err(_) => {
                let timeout_err = ExtensionError::Timeout(format!(
                    "Extension '{}' command '{}' timed out",
                    extension_id, command.name
                ));
                self.publish_error_event(
                    extension_id,
                    extension_name,
                    &command.name,
                    &execution_id,
                    &timeout_err.to_string(),
                    duration.as_millis() as u64,
                )
                .await;
                Err(timeout_err)
            }
        }
    }

    /// Collect and store metrics from an extension
    ///
    /// This is separate from command execution in V2.
    /// Extensions produce metrics on their own schedule (timers, events, polling).
    pub async fn collect_metrics(
        &self,
        extension_id: &str,
        extension: &DynExtension,
    ) -> Result<Vec<ExtensionMetricValue>> {
        let ext = extension.read().await;
        let metrics = ext.produce_metrics()?;
        drop(ext);

        // Store each metric
        for metric in &metrics {
            let data_source_id = DataSourceId::extension(extension_id, &metric.name);

            let datapoint = DataPoint {
                timestamp: metric.timestamp,
                value: param_to_metric_value(&metric.value),
                quality: None,
            };

            // Store the metric (fire and forget, log error only)
            if let Err(e) = self
                .storage
                .write_datapoint(&data_source_id, datapoint)
                .await
            {
                warn!(data_source_id = %data_source_id, error = %e, "Failed to store metric");
            }
        }

        Ok(metrics)
    }

    /// Publish command start event
    async fn publish_start_event(
        &self,
        extension_id: &str,
        extension_name: &str,
        command_id: &str,
        execution_id: &str,
        args: &serde_json::Value,
    ) {
        let event = NeoMindEvent::ExtensionCommandStarted {
            extension_id: extension_id.to_string(),
            extension_name: extension_name.to_string(),
            command_id: command_id.to_string(),
            execution_id: execution_id.to_string(),
            args: args.clone(),
            timestamp: chrono::Utc::now().timestamp_millis(),
        };

        let metadata = EventMetadata::new(format!("extension:{}", extension_id))
            .with_correlation_id(execution_id);

        let _ = self.event_bus.publish_with_metadata(event, metadata).await;
    }

    /// Publish command success event
    async fn publish_success_event(
        &self,
        extension_id: &str,
        extension_name: &str,
        command_id: &str,
        execution_id: &str,
        args: &serde_json::Value,
        output: &serde_json::Value,
        duration_ms: u64,
    ) {
        let event = NeoMindEvent::ExtensionCommandCompleted {
            extension_id: extension_id.to_string(),
            extension_name: extension_name.to_string(),
            command_id: command_id.to_string(),
            execution_id: execution_id.to_string(),
            args: args.clone(),
            outputs: vec![output.clone()],
            duration_ms,
            timestamp: chrono::Utc::now().timestamp_millis(),
        };

        let metadata = EventMetadata::new(format!("extension:{}", extension_id))
            .with_correlation_id(execution_id);

        let _ = self.event_bus.publish_with_metadata(event, metadata).await;
    }

    /// Publish command error event
    async fn publish_error_event(
        &self,
        extension_id: &str,
        extension_name: &str,
        command_id: &str,
        execution_id: &str,
        error: &str,
        duration_ms: u64,
    ) {
        let event = NeoMindEvent::ExtensionCommandFailed {
            extension_id: extension_id.to_string(),
            extension_name: extension_name.to_string(),
            command_id: command_id.to_string(),
            execution_id: execution_id.to_string(),
            error: error.to_string(),
            duration_ms,
            timestamp: chrono::Utc::now().timestamp_millis(),
        };

        let metadata = EventMetadata::new(format!("extension:{}", extension_id))
            .with_correlation_id(execution_id);

        let _ = self.event_bus.publish_with_metadata(event, metadata).await;
    }
}

/// Result of executing a command (V2 - simplified)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommandResult {
    pub success: bool,
    pub output: serde_json::Value,
    pub duration_ms: u64,
}

// ============================================================================
// Unified Storage Trait
// ============================================================================

/// Unified storage for all data source metrics
#[async_trait::async_trait]
pub trait UnifiedStorage: Send + Sync {
    /// Write a data point
    async fn write_datapoint(
        &self,
        source_id: &DataSourceId,
        datapoint: DataPoint,
    ) -> std::result::Result<(), StorageError>;

    /// Query data points
    async fn query_datapoints(
        &self,
        source_id: &DataSourceId,
        start: i64,
        end: i64,
    ) -> std::result::Result<Vec<DataPoint>, StorageError>;

    /// Get latest value
    async fn query_latest(
        &self,
        source_id: &DataSourceId,
        since: i64,
    ) -> std::result::Result<Option<DataPoint>, StorageError>;
}

/// Storage errors
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Database error: {0}")]
    Database(String),

    #[error("Invalid data source: {0}")]
    InvalidSource(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

// ============================================================================
// In-Memory Storage (for testing)
// ============================================================================

/// In-memory storage implementation for testing
pub struct MemoryStorage {
    data: tokio::sync::RwLock<std::collections::HashMap<String, Vec<DataPoint>>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self {
            data: tokio::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl UnifiedStorage for MemoryStorage {
    async fn write_datapoint(
        &self,
        source_id: &DataSourceId,
        datapoint: DataPoint,
    ) -> std::result::Result<(), StorageError> {
        let key = source_id.storage_key();
        let mut data = self.data.write().await;
        data.entry(key).or_insert_with(Vec::new).push(datapoint);
        Ok(())
    }

    async fn query_datapoints(
        &self,
        source_id: &DataSourceId,
        start: i64,
        end: i64,
    ) -> std::result::Result<Vec<DataPoint>, StorageError> {
        let key = source_id.storage_key();
        let data = self.data.read().await;

        Ok(data
            .get(&key)
            .map(|points| {
                points
                    .iter()
                    .filter(|p| p.timestamp >= start && p.timestamp <= end)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn query_latest(
        &self,
        source_id: &DataSourceId,
        since: i64,
    ) -> std::result::Result<Option<DataPoint>, StorageError> {
        let key = source_id.storage_key();
        let data = self.data.read().await;

        Ok(data
            .get(&key)
            .and_then(|points| points.iter().filter(|p| p.timestamp >= since).last())
            .cloned())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::super::system::{Extension, ExtensionMetadata, MetricDataType, MetricDefinition};
    use super::*;
    use crate::eventbus::EventBus;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_memory_storage_write_and_query() {
        let storage = MemoryStorage::new();
        let source_id = DataSourceId::extension("weather", "temperature"); // V2 format

        let datapoint = DataPoint::new(12345, MetricValue::Float(42.0));
        storage
            .write_datapoint(&source_id, datapoint.clone())
            .await
            .unwrap();

        let results = storage
            .query_datapoints(&source_id, 0, 99999)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].timestamp, 12345);
    }

    #[tokio::test]
    async fn test_command_executor() {
        let event_bus = EventBus::new();
        let storage: Arc<dyn UnifiedStorage + Send + Sync> = Arc::new(MemoryStorage::new());
        let executor = CommandExecutor::new(Arc::new(event_bus), storage);

        // Create a mock extension
        let extension = create_test_extension();

        let command = ExtensionCommand {
            name: "test".to_string(),
            display_name: "Test".to_string(),
            payload_template: String::new(),
            parameters: vec![],
            fixed_values: std::collections::HashMap::new(),
            samples: vec![],
            llm_hints: String::new(),
            parameter_groups: vec![],
        };

        let result = executor
            .execute_command(
                "test-ext",
                "Test Extension",
                &command,
                &serde_json::json!({}),
                &extension,
            )
            .await
            .unwrap();

        assert!(result.success);
    }

    fn create_test_extension() -> DynExtension {
        #[derive(Debug)]
        struct TestExtension;

        #[async_trait::async_trait]
        impl Extension for TestExtension {
            fn metadata(&self) -> &ExtensionMetadata {
                use std::sync::OnceLock;
                static META: OnceLock<ExtensionMetadata> = OnceLock::new();
                META.get_or_init(|| {
                    ExtensionMetadata::new("test-ext", "Test", semver::Version::new(1, 0, 0))
                })
            }

            fn metrics(&self) -> &[MetricDefinition] {
                use std::sync::OnceLock;
                static METRICS: OnceLock<Vec<MetricDefinition>> = OnceLock::new();
                METRICS.get_or_init(|| vec![])
            }

            fn commands(&self) -> &[ExtensionCommand] {
                use std::sync::OnceLock;
                static COMMANDS: OnceLock<Vec<ExtensionCommand>> = OnceLock::new();
                COMMANDS.get_or_init(|| vec![])
            }

            async fn execute_command(
                &self,
                _command: &str,
                _args: &serde_json::Value,
            ) -> Result<serde_json::Value> {
                Ok(serde_json::json!({"result": "ok"}))
            }
        }

        Arc::new(tokio::sync::RwLock::new(Box::new(TestExtension)))
    }
}
