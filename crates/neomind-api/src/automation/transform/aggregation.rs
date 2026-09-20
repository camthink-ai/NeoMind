#![allow(clippy::too_many_arguments)]

//! `aggregation` — split from the former transform.rs monolith.

use super::*;
use crate::automation::error::{AutomationError, Result};

use crate::automation::types::{AggregationFunc, TimeWindow};

use chrono::Utc;

use serde_json::Value;

impl TransformEngine {
    /// Execute an ArrayAggregation operation
    pub(crate) async fn execute_array_aggregation(
        &self,
        json_path: &str,
        aggregation: AggregationFunc,
        value_path: Option<&str>,
        output_metric: &str,
        device_id: &str,
        timestamp: i64,
        raw_data: &Value,
    ) -> Result<TransformedMetric> {
        // Extract array
        let array_value = self.extract_value_by_path(raw_data, json_path)?;

        let array = match &array_value {
            Value::Array(arr) => arr,
            _ => {
                return Err(AutomationError::TransformError {
                    operation: "ArrayAggregation".to_string(),
                    message: format!("Value at '{}' is not an array", json_path),
                });
            }
        };

        if array.is_empty() {
            return Err(AutomationError::TransformError {
                operation: "ArrayAggregation".to_string(),
                message: format!("Array at '{}' is empty", json_path),
            });
        }

        // Extract values from array elements
        let mut values = Vec::new();
        for element in array {
            let value = if let Some(path) = value_path {
                self.extract_value_by_path(element, path)?
            } else {
                element.clone()
            };

            if let Some(f) = value_as_f64(&value) {
                values.push(f);
            }
        }

        if values.is_empty() {
            return Err(AutomationError::TransformError {
                operation: "ArrayAggregation".to_string(),
                message: "No numeric values found in array".to_string(),
            });
        }

        // Compute aggregation
        let result = self.compute_aggregation(&values, aggregation)?;

        Ok(TransformedMetric {
            device_id: device_id.to_string(),
            transform_id: None,
            metric: output_metric.to_string(),
            value: result.into(),
            timestamp,
            quality: Some(1.0),
        })
    }
}
impl TransformEngine {
    /// Execute a TimeSeriesAggregation operation
    pub(crate) async fn execute_time_series_aggregation(
        &self,
        source_metric: &str,
        window: &TimeWindow,
        aggregation: AggregationFunc,
        output_metric: &str,
        device_id: &str,
    ) -> Result<TransformedMetric> {
        // Query the persistent telemetry store over the window. Timestamps
        // are SECONDS (device metrics storage writes Utc::now().timestamp();
        // the old code's millis comment was backwards).
        let storage =
            self.time_series_storage
                .as_ref()
                .ok_or_else(|| AutomationError::TransformError {
                    operation: "TimeSeriesAggregation".to_string(),
                    message: "Time-series storage is not available for aggregation".to_string(),
                })?;
        let now = Utc::now().timestamp();
        let start = now - window.duration_secs as i64;
        let source_id = format!("device:{}", device_id);
        let points: Vec<neomind_devices::telemetry::DataPoint> = storage
            .query(&source_id, source_metric, start, now)
            .await
            .map_err(|e| AutomationError::TransformError {
                operation: "TimeSeriesAggregation".to_string(),
                message: format!(
                    "Failed to query history for '{}.{}': {}",
                    device_id, source_metric, e
                ),
            })?;

        let values: Vec<f64> = points
            .iter()
            .filter_map(|p| match &p.value {
                neomind_devices::MetricValue::Float(f) => Some(*f),
                neomind_devices::MetricValue::Integer(i) => Some(*i as f64),
                _ => None,
            })
            .collect();

        if values.is_empty() {
            return Err(AutomationError::TransformError {
                operation: "TimeSeriesAggregation".to_string(),
                message: format!("No data points found for '{}.{}'", device_id, source_metric),
            });
        }

        let result = self.compute_aggregation(&values, aggregation)?;

        // Seconds, aligned with device metrics storage and sibling outputs.
        let timestamp = Utc::now().timestamp();

        Ok(TransformedMetric {
            device_id: device_id.to_string(),
            transform_id: None,
            metric: output_metric.to_string(),
            value: result.into(),
            timestamp,
            quality: Some(1.0),
        })
    }
}
impl TransformEngine {
    /// Compute aggregation over a set of values
    pub(crate) fn compute_aggregation(
        &self,
        values: &[f64],
        aggregation: AggregationFunc,
    ) -> Result<f64> {
        if values.is_empty() {
            return Err(AutomationError::TransformError {
                operation: format!("{:?}", aggregation),
                message: "Cannot aggregate empty value set".to_string(),
            });
        }

        let result = match aggregation {
            AggregationFunc::Mean => values.iter().sum::<f64>() / values.len() as f64,
            AggregationFunc::Max => values.iter().fold(f64::NAN, |a, &b| a.max(b)),
            AggregationFunc::Min => values.iter().fold(f64::NAN, |a, &b| a.min(b)),
            AggregationFunc::Sum => values.iter().sum(),
            AggregationFunc::Count => values.len() as f64,
            AggregationFunc::First => values[0],
            AggregationFunc::Last => values[values.len() - 1],
            AggregationFunc::Median => {
                let mut sorted = values.to_vec();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                sorted[sorted.len() / 2]
            }
            AggregationFunc::StdDev => {
                let mean = values.iter().sum::<f64>() / values.len() as f64;
                let variance =
                    values.iter().map(|&x| (x - mean).powi(2)).sum::<f64>() / values.len() as f64;
                variance.sqrt()
            }
            AggregationFunc::Trend => {
                // Simple trend: positive if last > first, negative if last < first, zero if equal
                if values.len() < 2 {
                    0.0
                } else {
                    let last = values[values.len() - 1];
                    let first = values[0];
                    if last > first {
                        1.0
                    } else if last < first {
                        -1.0
                    } else {
                        0.0
                    }
                }
            }
            AggregationFunc::Delta => {
                if values.len() < 2 {
                    0.0
                } else {
                    values[values.len() - 1] - values[0]
                }
            }
            AggregationFunc::Rate => {
                // Rate of change per second (assuming 1-second intervals)
                if values.len() < 2 {
                    0.0
                } else {
                    (values[values.len() - 1] - values[0]) / (values.len() - 1) as f64
                }
            }
        };

        Ok(result)
    }
}
impl TransformEngine {
    /// Extract a value from JSON using a simple path notation
    ///
    /// Supports:
    /// - "$" - root
    /// - "$.field" - direct field access
    /// - "$.field.nested" - nested field access
    /// - "$.field[0]" - array access with index
    /// - "$.field[]" - array wildcard (returns first element or all elements as array)
    /// - "$.field[0]" or "$.field.0" - dot notation for array index
    pub(crate) fn extract_value_by_path(&self, data: &Value, path: &str) -> Result<Value> {
        let path = path.trim();
        let mut current = data;

        // Handle root
        if path == "$" || path.is_empty() {
            return Ok(current.clone());
        }

        // Remove leading "$."
        let path_parts = if let Some(rest) = path.strip_prefix("$.") {
            rest
        } else if let Some(rest) = path.strip_prefix('$') {
            rest
        } else {
            path
        };

        // Split by "." and traverse
        for part in path_parts.split('.') {
            if part.is_empty() {
                continue;
            }

            // Handle array indexing: "field[0]" or "field[]" for wildcard
            if let Some(bracket_pos) = part.find('[') {
                let field = &part[..bracket_pos];
                let index_part = &part[bracket_pos..];

                // Get the field first
                current = current
                    .get(field)
                    .ok_or_else(|| AutomationError::TransformError {
                        operation: "PathExtract".to_string(),
                        message: format!("Field '{}' not found", field),
                    })?;

                // Parse array index or wildcard
                if let Some(end_bracket) = index_part.find(']') {
                    let index_str = &index_part[1..end_bracket];

                    if index_str.is_empty() {
                        // Wildcard "[]" - return the array as-is, or first element
                        if let Value::Array(arr) = current {
                            current = if arr.len() == 1 { &arr[0] } else { current };
                        }
                        // If not an array, keep current as-is
                    } else {
                        // Numeric index
                        let index: usize =
                            index_str
                                .parse()
                                .map_err(|_| AutomationError::TransformError {
                                    operation: "PathExtract".to_string(),
                                    message: format!("Invalid array index: {}", index_str),
                                })?;

                        if let Value::Array(arr) = current {
                            current =
                                arr.get(index)
                                    .ok_or_else(|| AutomationError::TransformError {
                                        operation: "PathExtract".to_string(),
                                        message: format!(
                                            "Array index {} out of bounds (len: {})",
                                            index,
                                            arr.len()
                                        ),
                                    })?;
                        } else {
                            return Err(AutomationError::TransformError {
                                operation: "PathExtract".to_string(),
                                message: format!("Field '{}' is not an array", field),
                            });
                        }
                    }
                } else {
                    return Err(AutomationError::TransformError {
                        operation: "PathExtract".to_string(),
                        message: format!("Unclosed bracket in path part: {}", part),
                    });
                }
            } else if let Ok(index) = part.parse::<usize>() {
                // Handle numeric array index in dot notation: "field.0"
                if let Value::Array(arr) = current {
                    current = arr
                        .get(index)
                        .ok_or_else(|| AutomationError::TransformError {
                            operation: "PathExtract".to_string(),
                            message: format!(
                                "Array index {} out of bounds (len: {})",
                                index,
                                arr.len()
                            ),
                        })?;
                } else {
                    return Err(AutomationError::TransformError {
                        operation: "PathExtract".to_string(),
                        message: format!("Cannot index non-array with {}", index),
                    });
                }
            } else {
                // Regular field access
                current = current
                    .get(part)
                    .ok_or_else(|| AutomationError::TransformError {
                        operation: "PathExtract".to_string(),
                        message: format!("Field '{}' not found", part),
                    })?;
            }
        }

        Ok(current.clone())
    }
}
impl TransformEngine {
    /// Phase 4.2: Extract a metric value from an extension result
    /// Handles various result formats: object with named fields, array, or direct value
    pub(crate) fn extract_metric_value(result: &Value, metric_name: &str) -> f64 {
        match result {
            // Object: look for the metric_name as a key
            Value::Object(map) => {
                // Try exact match first
                if let Some(v) = map.get(metric_name) {
                    return value_as_f64(v).unwrap_or(0.0);
                }
                // Try lowercase
                let lower_name = metric_name.to_lowercase();
                for (key, val) in map.iter() {
                    if key.to_lowercase() == lower_name {
                        return value_as_f64(val).unwrap_or(0.0);
                    }
                }
                // Fallback to first numeric value
                for val in map.values() {
                    if let Some(f) = value_as_f64(val) {
                        return f;
                    }
                }
                0.0
            }
            // Array: if metric_name is a number, use as index, else use first
            Value::Array(arr) => {
                if let Ok(index) = metric_name.parse::<usize>() {
                    if index < arr.len() {
                        value_as_f64(&arr[index]).unwrap_or(0.0)
                    } else {
                        0.0
                    }
                } else if !arr.is_empty() {
                    value_as_f64(&arr[0]).unwrap_or(0.0)
                } else {
                    0.0
                }
            }
            // Direct number
            Value::Number(n) => n.as_f64().unwrap_or(0.0),
            // Boolean as 0/1
            Value::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            // String: try to parse as number
            Value::String(s) => s.parse::<f64>().unwrap_or(0.0),
            // Null
            Value::Null => 0.0,
        }
    }
}
