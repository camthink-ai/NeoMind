//! Transform Output Registry
//!
//! This module provides automatic registration of Transform outputs as data sources.
//! When a Transform executes successfully, its output metrics are automatically
//! registered and can be queried like device metrics.
//!
//! # Design
//!
//! - Transform outputs are registered in the format: `transform:{transform_id}:{metric_name}`
//! - Outputs are tracked with metadata (name, data type, unit, last update time)
//! - Registry is thread-safe and can be shared across the application
//!
//! # Use Cases
//!
//! - Dashboard components can select Transform outputs as data sources
//! - Rules can use Transform outputs as conditions
//! - AI Agents can query Transform outputs in their reasoning

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::transform::TransformedMetric;
use neomind_core::event::MetricValue;

/// Information about a Transform output metric (data source)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformOutputInfo {
    /// Transform ID that produces this output
    pub transform_id: String,
    /// Transform name (human-readable)
    pub transform_name: String,
    /// Metric name (the output field from the Transform)
    pub metric_name: String,
    /// Full data source ID for querying
    pub data_source_id: String,
    /// Human-readable display name
    pub display_name: String,
    /// Data type of the output
    pub data_type: TransformOutputType,
    /// Unit of measurement (if applicable)
    pub unit: Option<String>,
    /// Description of what this output represents
    pub description: String,
    /// When this output was last updated
    pub last_update: Option<i64>,
    /// When this output was first registered
    pub registered_at: i64,
    /// Whether the Transform is currently enabled
    pub enabled: bool,
}

/// Data type for Transform outputs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransformOutputType {
    Float,
    Integer,
    Boolean,
    String,
    Unknown,
}

impl From<f64> for TransformOutputType {
    fn from(_: f64) -> Self {
        TransformOutputType::Float
    }
}

impl From<i64> for TransformOutputType {
    fn from(_: i64) -> Self {
        TransformOutputType::Integer
    }
}

impl TransformOutputType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TransformOutputType::Float => "float",
            TransformOutputType::Integer => "integer",
            TransformOutputType::Boolean => "boolean",
            TransformOutputType::String => "string",
            TransformOutputType::Unknown => "unknown",
        }
    }
}

/// Detect the output type from a MetricValue.
fn detect_output_type(value: &MetricValue) -> TransformOutputType {
    match value {
        MetricValue::Float(_) => TransformOutputType::Float,
        MetricValue::Integer(_) => TransformOutputType::Integer,
        MetricValue::Boolean(_) => TransformOutputType::Boolean,
        MetricValue::String(_) => TransformOutputType::String,
        MetricValue::Json(_) => TransformOutputType::Unknown,
    }
}

/// Registry for Transform output metrics
///
/// Tracks all Transform outputs and makes them available as data sources.
pub struct TransformOutputRegistry {
    /// Map of data_source_id -> output info
    outputs: Arc<RwLock<HashMap<String, TransformOutputInfo>>>,
    /// Map of transform_id -> set of metric names
    transform_metrics: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl Default for TransformOutputRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TransformOutputRegistry {
    /// Create a new registry
    pub fn new() -> Self {
        Self {
            outputs: Arc::new(RwLock::new(HashMap::new())),
            transform_metrics: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register Transform outputs after execution
    ///
    /// This should be called when a Transform executes successfully.
    /// The outputs will be registered as available data sources.
    ///
    /// # Arguments
    /// * `transform_id` - The Transform's ID
    /// * `transform_name` - The Transform's name
    /// * `metrics` - The metrics produced by the Transform
    /// * `enabled` - Whether the Transform is enabled
    pub async fn register_outputs(
        &self,
        transform_id: &str,
        transform_name: &str,
        metrics: &[TransformedMetric],
        enabled: bool,
    ) {
        let now = Utc::now().timestamp();
        let mut outputs = self.outputs.write().await;
        let mut transform_metrics = self.transform_metrics.write().await;

        // Track metric names for this transform
        let metric_names: Vec<String> = metrics.iter().map(|m| m.metric.clone()).collect();
        transform_metrics.insert(transform_id.to_string(), metric_names.clone());

        // [stale-name eviction] Metrics whose names VARY with the data
        // (GroupBy's output_{group} keys) used to leave phantom data-source
        // entries forever: insert() replaced the name list but never removed
        // outputs registered by earlier runs. Drop this transform's outputs
        // whose metric is not in the fresh set.
        let fresh: std::collections::HashSet<&str> =
            metric_names.iter().map(|s| s.as_str()).collect();
        let stale_ids: Vec<String> = outputs
            .iter()
            .filter(|(id, info)| {
                info.transform_id == transform_id
                    && !fresh.contains(info.metric_name.as_str())
                    && !id.is_empty()
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale_ids {
            outputs.remove(&id);
        }

        // Register each metric as a data source
        for metric in metrics {
            let data_source_id = format!("transform:{}:{}", transform_id, metric.metric);
            let output_info = TransformOutputInfo {
                transform_id: transform_id.to_string(),
                transform_name: transform_name.to_string(),
                metric_name: metric.metric.clone(),
                data_source_id: data_source_id.clone(),
                display_name: format!("{}: {}", transform_name, metric.metric),
                data_type: detect_output_type(&metric.value),
                unit: None,
                description: format!("Output from Transform: {}", transform_name),
                last_update: Some(metric.timestamp),
                registered_at: now,
                enabled,
            };

            outputs.insert(data_source_id, output_info);
        }

        tracing::debug!(
            transform_id = %transform_id,
            count = metrics.len(),
            "Registered Transform outputs"
        );
    }

    /// Update the last update time for a specific metric
    pub async fn update_metric(&self, transform_id: &str, metric_name: &str, timestamp: i64) {
        let data_source_id = format!("transform:{}:{}", transform_id, metric_name);
        let mut outputs = self.outputs.write().await;

        if let Some(output) = outputs.get_mut(&data_source_id) {
            output.last_update = Some(timestamp);
        }
    }

    /// Remove all outputs for a Transform
    ///
    /// Call this when a Transform is deleted.
    pub async fn unregister_transform(&self, transform_id: &str) {
        let mut outputs = self.outputs.write().await;
        let mut transform_metrics = self.transform_metrics.write().await;

        // Remove all metrics for this transform
        if let Some(metric_names) = transform_metrics.remove(transform_id) {
            for metric_name in metric_names {
                let data_source_id = format!("transform:{}:{}", transform_id, metric_name);
                outputs.remove(&data_source_id);
            }
        }

        tracing::debug!(
            transform_id = %transform_id,
            "Unregistered Transform outputs"
        );
    }

    /// Get all registered outputs
    pub async fn list_outputs(&self) -> Vec<TransformOutputInfo> {
        let outputs = self.outputs.read().await;
        outputs.values().cloned().collect()
    }

    /// Get outputs for a specific Transform
    pub async fn get_transform_outputs(&self, transform_id: &str) -> Vec<TransformOutputInfo> {
        let outputs = self.outputs.read().await;
        outputs
            .values()
            .filter(|o| o.transform_id == transform_id)
            .cloned()
            .collect()
    }

    /// Get a specific output by data source ID
    pub async fn get_output(&self, data_source_id: &str) -> Option<TransformOutputInfo> {
        let outputs = self.outputs.read().await;
        outputs.get(data_source_id).cloned()
    }

    /// Get outputs in a format compatible with ExtensionDataSourceInfo
    ///
    /// This allows the frontend to use Transform outputs the same way
    /// it uses Extension data sources.
    pub async fn list_as_data_sources(&self) -> Vec<TransformDataSourceInfo> {
        let outputs = self.outputs.read().await;
        outputs
            .values()
            .filter(|o| o.enabled) // Only enabled transforms
            .map(|output| TransformDataSourceInfo {
                id: output.data_source_id.clone(),
                transform_id: output.transform_id.clone(),
                transform_name: output.transform_name.clone(),
                metric_name: output.metric_name.clone(),
                display_name: output.display_name.clone(),
                data_type: output.data_type.as_str().to_string(),
                unit: output.unit.clone(),
                description: output.description.clone(),
                last_update: output.last_update,
            })
            .collect()
    }

    /// Clear all outputs (for testing)
    pub async fn clear(&self) {
        let mut outputs = self.outputs.write().await;
        let mut transform_metrics = self.transform_metrics.write().await;
        outputs.clear();
        transform_metrics.clear();
    }

    /// Get the number of registered outputs
    pub async fn count(&self) -> usize {
        let outputs = self.outputs.read().await;
        outputs.len()
    }
}

/// Transform data source info (compatible with ExtensionDataSourceInfo format)
///
/// This format allows the frontend to treat Transform outputs the same
/// way it treats Extension data sources.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformDataSourceInfo {
    /// Unique identifier for this data source
    pub id: String,
    /// Transform ID that produces this data
    pub transform_id: String,
    /// Transform name (human-readable)
    pub transform_name: String,
    /// Metric name
    pub metric_name: String,
    /// Display name
    pub display_name: String,
    /// Data type
    pub data_type: String,
    /// Unit (optional)
    pub unit: Option<String>,
    /// Description
    pub description: String,
    /// Last update timestamp
    pub last_update: Option<i64>,
}

/// Response for listing Transform data sources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformDataSourcesResponse {
    pub data_sources: Vec<TransformDataSourceInfo>,
    pub count: usize,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::super::transform::TransformedMetric;
    use super::*;

    fn create_test_metrics() -> Vec<TransformedMetric> {
        vec![
            TransformedMetric {
                device_id: "sensor1".to_string(),
                transform_id: None,
                metric: "temperature_f".to_string(),
                value: 72.5.into(),
                timestamp: 12345,
                quality: Some(1.0),
            },
            TransformedMetric {
                device_id: "sensor1".to_string(),
                transform_id: None,
                metric: "humidity_pct".to_string(),
                value: 65.0.into(),
                timestamp: 12345,
                quality: Some(1.0),
            },
        ]
    }

    #[tokio::test]
    async fn test_register_outputs() {
        let registry = TransformOutputRegistry::new();
        let metrics = create_test_metrics();

        registry
            .register_outputs("transform1", "Temp Converter", &metrics, true)
            .await;

        // Check count
        assert_eq!(registry.count().await, 2);

        // Check outputs
        let outputs = registry.list_outputs().await;
        assert_eq!(outputs.len(), 2);

        // Check first output
        let temp_output = outputs
            .iter()
            .find(|o| o.metric_name == "temperature_f")
            .unwrap();
        assert_eq!(temp_output.transform_id, "transform1");
        assert_eq!(temp_output.transform_name, "Temp Converter");
        assert_eq!(
            temp_output.data_source_id,
            "transform:transform1:temperature_f"
        );
    }

    #[tokio::test]
    async fn test_get_transform_outputs() {
        let registry = TransformOutputRegistry::new();
        let metrics = create_test_metrics();

        registry
            .register_outputs("transform1", "Test Transform", &metrics, true)
            .await;

        let outputs = registry.get_transform_outputs("transform1").await;
        assert_eq!(outputs.len(), 2);
        assert!(outputs.iter().all(|o| o.transform_id == "transform1"));
    }

    #[tokio::test]
    async fn test_unregister_transform() {
        let registry = TransformOutputRegistry::new();
        let metrics = create_test_metrics();

        registry
            .register_outputs("transform1", "Test", &metrics, true)
            .await;
        assert_eq!(registry.count().await, 2);

        registry.unregister_transform("transform1").await;
        assert_eq!(registry.count().await, 0);
    }

    #[tokio::test]
    async fn test_list_as_data_sources() {
        let registry = TransformOutputRegistry::new();
        let metrics = create_test_metrics();

        registry
            .register_outputs("transform1", "Temp Converter", &metrics, true)
            .await;

        let data_sources = registry.list_as_data_sources().await;
        assert_eq!(data_sources.len(), 2);

        let temp_ds = data_sources
            .iter()
            .find(|ds| ds.metric_name == "temperature_f")
            .unwrap();
        assert_eq!(temp_ds.transform_id, "transform1");
        assert_eq!(temp_ds.transform_name, "Temp Converter");
        assert_eq!(temp_ds.data_type, "float");
    }

    #[tokio::test]
    async fn test_disabled_transform_not_listed() {
        let registry = TransformOutputRegistry::new();
        let metrics = create_test_metrics();

        // Register as disabled
        registry
            .register_outputs("transform1", "Test", &metrics, false)
            .await;

        let data_sources = registry.list_as_data_sources().await;
        // Disabled transforms should not be in the list
        assert_eq!(data_sources.len(), 0);
    }

    #[tokio::test]
    async fn test_update_metric() {
        let registry = TransformOutputRegistry::new();
        let metrics = create_test_metrics();

        registry
            .register_outputs("transform1", "Test", &metrics, true)
            .await;

        // Update timestamp
        registry
            .update_metric("transform1", "temperature_f", 99999)
            .await;

        let output = registry
            .get_output("transform:transform1:temperature_f")
            .await
            .unwrap();
        assert_eq!(output.last_update, Some(99999));
    }
}
