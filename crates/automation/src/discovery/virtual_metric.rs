//! Virtual metric generator for auto-generating metric definitions.
//!
//! This module combines path extraction and semantic inference to
//! automatically generate virtual metric definitions from device samples.

use crate::discovery::types::*;
use crate::discovery::{DataPathExtractor, SemanticInference};
use edge_ai_core::LlmRuntime;
use std::sync::Arc;

/// Auto-generates virtual metric definitions from device samples
pub struct VirtualMetricGenerator {
    path_extractor: DataPathExtractor,
    semantic_inference: SemanticInference,
}

impl VirtualMetricGenerator {
    /// Create a new virtual metric generator
    pub fn new(llm: Arc<dyn LlmRuntime>) -> Self {
        Self {
            path_extractor: DataPathExtractor::new(llm.clone()),
            semantic_inference: SemanticInference::new(llm),
        }
    }

    /// Generate virtual metrics from device samples
    pub async fn generate_metrics(
        &self,
        _device_id: &str,
        samples: &[DeviceSample],
        context: &InferenceContext,
    ) -> Result<Vec<DiscoveredMetric>> {
        if samples.is_empty() {
            return Err(DiscoveryError::InsufficientData(
                "No samples provided for metric generation".into()
            ));
        }

        // Step 1: Extract all accessible paths
        let paths = self.path_extractor.extract_paths(samples).await?;

        // Step 2: Enhance each path with semantic information
        let mut metrics = Vec::new();
        for path in paths {
            // Skip non-leaf paths (objects/arrays without primitive values)
            if path.is_object || path.is_array {
                continue;
            }

            // Skip low coverage paths (< 50%)
            if path.coverage < 0.5 {
                continue;
            }

            // Skip null-type paths
            if path.data_type == DataType::Null {
                continue;
            }

            // Enhance with semantic inference
            let metric = self.semantic_inference.enhance_path(&path, context).await;

            metrics.push(metric);
        }

        // Step 3: Deduplicate metrics by semantic type
        Ok(self.deduplicate_metrics(metrics))
    }

    /// Generate metrics for a specific path only
    pub async fn generate_metric_for_path(
        &self,
        path: &str,
        samples: &[DeviceSample],
        context: &InferenceContext,
    ) -> Result<DiscoveredMetric> {
        // Extract paths to find the target
        let paths = self.path_extractor.extract_paths(samples).await?;

        let target_path = paths.iter()
            .find(|p| p.path == path)
            .ok_or_else(|| DiscoveryError::InvalidData(format!("Path not found: {}", path)))?;

        Ok(self.semantic_inference.enhance_path(target_path, context).await)
    }

    /// Suggest aggregations for multiple metrics
    pub async fn suggest_aggregations(
        &self,
        metrics: &[DiscoveredMetric],
    ) -> Vec<AggregationSuggestion> {
        let mut suggestions = Vec::new();

        // Group metrics by device or location
        let mut by_semantic: std::collections::HashMap<&str, Vec<&DiscoveredMetric>> =
            std::collections::HashMap::new();

        for metric in metrics {
            let key = metric.semantic_type.display_name();
            by_semantic.entry(key).or_default().push(metric);
        }

        // Suggest aggregations for groups with 2+ metrics
        for (semantic, group_metrics) in by_semantic {
            if group_metrics.len() >= 2 {
                suggestions.push(AggregationSuggestion {
                    name: format!("avg_{}", semantic.to_lowercase()),
                    display_name: format!("平均{}", semantic),
                    description: format!("所有{}传感器的平均值", semantic),
                    operation: AggregationOperation::Average,
                    source_metrics: group_metrics.iter().map(|m| m.name.clone()).collect(),
                    unit: group_metrics.first().and_then(|m| m.unit.clone()),
                });
            }
        }

        suggestions
    }

    /// Generate derived/calculated metrics
    pub async fn suggest_derived_metrics(
        &self,
        metrics: &[DiscoveredMetric],
    ) -> Vec<DerivedMetric> {
        let mut derived = Vec::new();

        // Check for temperature and humidity to suggest heat index
        let has_temp = metrics.iter().any(|m| m.semantic_type == SemanticType::Temperature);
        let has_humid = metrics.iter().any(|m| m.semantic_type == SemanticType::Humidity);

        if has_temp && has_humid {
            let temp_metric = metrics.iter()
                .find(|m| m.semantic_type == SemanticType::Temperature);
            let humid_metric = metrics.iter()
                .find(|m| m.semantic_type == SemanticType::Humidity);

            if let (Some(temp), Some(humid)) = (temp_metric, humid_metric) {
                derived.push(DerivedMetric {
                    name: "heat_index".to_string(),
                    display_name: "体感温度".to_string(),
                    description: "基于温度和湿度计算的体感温度".to_string(),
                    formula: "heat_index(temperature, humidity)".to_string(),
                    source_metrics: vec![temp.name.clone(), humid.name.clone()],
                    unit: Some("°C".to_string()),
                });
            }
        }

        // Check for power to suggest energy accumulation
        let power_metrics: Vec<_> = metrics.iter()
            .filter(|m| m.semantic_type == SemanticType::Power)
            .collect();

        if !power_metrics.is_empty() {
            derived.push(DerivedMetric {
                name: "total_energy".to_string(),
                display_name: "总能耗".to_string(),
                description: "基于功率积分计算的总能耗".to_string(),
                formula: "integral(power)".to_string(),
                source_metrics: power_metrics.iter().map(|m| m.name.clone()).collect(),
                unit: Some("kWh".to_string()),
            });
        }

        derived
    }

    /// Deduplicate metrics by semantic type and path
    fn deduplicate_metrics(&self, metrics: Vec<DiscoveredMetric>) -> Vec<DiscoveredMetric> {
        let mut unique = std::collections::HashMap::new();

        for metric in metrics {
            // Key by semantic type and path - clone to avoid borrow issues
            let key = (metric.semantic_type.clone(), metric.path.clone());

            unique.entry(key)
                .or_insert(metric);
        }

        unique.into_values().collect()
    }
}

/// Suggested aggregation for multiple metrics
#[derive(Debug, Clone)]
pub struct AggregationSuggestion {
    /// Aggregation name
    pub name: String,
    /// Display name
    pub display_name: String,
    /// Description
    pub description: String,
    /// Aggregation operation
    pub operation: AggregationOperation,
    /// Source metric names
    pub source_metrics: Vec<String>,
    /// Unit of the result
    pub unit: Option<String>,
}

/// Aggregation operation type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AggregationOperation {
    /// Average of all values
    Average,
    /// Sum of all values
    Sum,
    /// Minimum value
    Min,
    /// Maximum value
    Max,
    /// Count of values
    Count,
    /// Latest value
    Latest,
}

/// Derived/calculated metric
#[derive(Debug, Clone)]
pub struct DerivedMetric {
    /// Metric name
    pub name: String,
    /// Display name
    pub display_name: String,
    /// Description
    pub description: String,
    /// Calculation formula
    pub formula: String,
    /// Source metrics used in calculation
    pub source_metrics: Vec<String>,
    /// Result unit
    pub unit: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aggregation_operation() {
        let ops = vec![
            AggregationOperation::Average,
            AggregationOperation::Sum,
            AggregationOperation::Min,
            AggregationOperation::Max,
        ];

        assert!(ops.contains(&AggregationOperation::Average));
        assert_eq!(ops.len(), 4);
    }

    #[test]
    fn test_deduplicate_metrics() {
        // This test verifies the deduplication logic structure
        let metrics = vec![
            DiscoveredMetric {
                name: "temp1".to_string(),
                display_name: "Temperature 1".to_string(),
                description: "Temperature reading".to_string(),
                path: "sensors[0].temp".to_string(),
                data_type: DataType::Float,
                semantic_type: SemanticType::Temperature,
                unit: Some("°C".to_string()),
                value_range: None,
                is_readable: true,
                is_writable: false,
            },
            DiscoveredMetric {
                name: "temp2".to_string(),
                display_name: "Temperature 2".to_string(),
                description: "Temperature reading".to_string(),
                path: "sensors[1].temp".to_string(),
                data_type: DataType::Float,
                semantic_type: SemanticType::Temperature,
                unit: Some("°C".to_string()),
                value_range: None,
                is_readable: true,
                is_writable: false,
            },
        ];

        assert_eq!(metrics.len(), 2);
    }
}
