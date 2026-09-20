#![allow(clippy::too_many_arguments)]

//! `operations` — split from the former transform.rs monolith.

use super::*;
use crate::automation::error::{AutomationError, Result};

use crate::automation::types::{AggregationFunc, TransformOperation};

use serde_json::Value;

use neomind_core::event::MetricValue;

impl TransformEngine {
    /// Execute Extract operation - extract value using JSONPath
    pub(crate) async fn execute_extract(
        &self,
        from: &str,
        output: &str,
        _as_type: Option<crate::automation::types::TargetDataType>,
        device_id: &str,
        timestamp: i64,
        raw_data: &Value,
    ) -> Result<TransformedMetric> {
        let value = self.extract_value_by_path(raw_data, from)?;
        let float_value = value_as_f64(&value).ok_or_else(|| AutomationError::TransformError {
            operation: "Extract".to_string(),
            message: format!("Value at '{}' is not a number: {}", from, value),
        })?;

        Ok(TransformedMetric {
            device_id: device_id.to_string(),
            transform_id: None,
            metric: output.to_string(),
            value: float_value.into(),
            timestamp,
            quality: Some(1.0),
        })
    }
}
impl TransformEngine {
    /// Execute Map operation - transform array elements using template
    pub(crate) async fn execute_map(
        &self,
        over: &str,
        template: &str,
        output_pattern: &str,
        _where_clause: Option<&str>,
        device_id: &str,
        timestamp: i64,
        raw_data: &Value,
    ) -> Result<Vec<TransformedMetric>> {
        let array_value = self.extract_value_by_path(raw_data, over)?;

        let array = match &array_value {
            Value::Array(arr) => arr,
            _ => {
                return Err(AutomationError::TransformError {
                    operation: "Map".to_string(),
                    message: format!("Value at '{}' is not an array", over),
                });
            }
        };

        let mut metrics = Vec::new();
        for (index, item) in array.iter().enumerate() {
            // Render template with item context
            let rendered = self.render_template(template, raw_data, Some(item), index);
            let rendered_output = self.render_template(output_pattern, raw_data, Some(item), index);

            // Try to parse as number; if not numeric, store as a Text metric
            // (previously hashed to a bogus float — silent data corruption).
            let value: MetricValue = if let Ok(num) = rendered.trim().parse::<f64>() {
                num.into()
            } else {
                rendered.clone().into()
            };

            metrics.push(TransformedMetric {
                device_id: device_id.to_string(),
                transform_id: None,
                metric: rendered_output,
                value,
                timestamp,
                quality: Some(1.0),
            });
        }

        Ok(metrics)
    }
}
impl TransformEngine {
    /// Execute Reduce operation - aggregate array
    pub(crate) async fn execute_reduce(
        &self,
        over: &str,
        aggregation: AggregationFunc,
        value_path: Option<&str>,
        output: &str,
        device_id: &str,
        timestamp: i64,
        raw_data: &Value,
    ) -> Result<TransformedMetric> {
        self.execute_array_aggregation(
            over,
            aggregation,
            value_path,
            output,
            device_id,
            timestamp,
            raw_data,
        )
        .await
    }
}
impl TransformEngine {
    /// Execute Format operation - template string formatting
    pub(crate) async fn execute_format(
        &self,
        template: &str,
        output: &str,
        from: Option<&str>,
        device_id: &str,
        timestamp: i64,
        raw_data: &Value,
    ) -> Result<TransformedMetric> {
        let data = if let Some(path) = from {
            self.extract_value_by_path(raw_data, path)?
        } else {
            raw_data.clone()
        };

        let rendered = self.render_template(template, &data, None, 0);

        // Try to parse as number; if not numeric, store as Text (not a bogus hash).
        let value: MetricValue = if let Ok(num) = rendered.trim().parse::<f64>() {
            num.into()
        } else {
            rendered.clone().into()
        };

        Ok(TransformedMetric {
            device_id: device_id.to_string(),
            transform_id: None,
            metric: output.to_string(),
            value,
            timestamp,
            quality: Some(1.0),
        })
    }
}
impl TransformEngine {
    /// Execute Compute operation - mathematical expression
    pub(crate) async fn execute_compute(
        &self,
        expression: &str,
        output: &str,
        device_id: &str,
        timestamp: i64,
        raw_data: &Value,
    ) -> Result<TransformedMetric> {
        // Simple expression evaluator
        // Supports: {{path}}, +, -, *, /, ( )
        let rendered = self.render_template(expression, raw_data, None, 0);

        let value = self
            .evaluate_expression(&rendered, raw_data)
            .unwrap_or_else(|_| rendered.trim().parse().unwrap_or(0.0));

        Ok(TransformedMetric {
            device_id: device_id.to_string(),
            transform_id: None,
            metric: output.to_string(),
            value: value.into(),
            timestamp,
            quality: Some(1.0),
        })
    }
}
impl TransformEngine {
    /// Execute Pipeline operation - chain operations
    pub(crate) async fn execute_pipeline(
        &self,
        _steps: &[TransformOperation],
        _final_output: &str,
        _device_id: &str,
        _timestamp: i64,
        _raw_data: &Value,
    ) -> Result<Vec<TransformedMetric>> {
        Err(AutomationError::TransformError {
            operation: "Pipeline".to_string(),
            message: "Pipeline operation is not yet implemented".to_string(),
        })
    }
}
impl TransformEngine {
    /// Execute Fork operation - parallel branches
    pub(crate) async fn execute_fork(
        &self,
        _branches: &[TransformOperation],
        _device_id: &str,
        _timestamp: i64,
        _raw_data: &Value,
    ) -> Result<Vec<TransformedMetric>> {
        Err(AutomationError::TransformError {
            operation: "Fork".to_string(),
            message: "Fork operation is not yet implemented".to_string(),
        })
    }
}
impl TransformEngine {
    /// Execute If operation - conditional execution
    pub(crate) async fn execute_if(
        &self,
        _condition: &str,
        _then_op: &TransformOperation,
        _else_op: Option<&TransformOperation>,
        _output: &str,
        _device_id: &str,
        _timestamp: i64,
        _raw_data: &Value,
    ) -> Result<Vec<TransformedMetric>> {
        Err(AutomationError::TransformError {
            operation: "If".to_string(),
            message: "If operation is not yet implemented".to_string(),
        })
    }
}
impl TransformEngine {
    /// Render a template string with variable substitution
    pub(crate) fn render_template(
        &self,
        template: &str,
        root: &Value,
        item: Option<&Value>,
        index: usize,
    ) -> String {
        let mut result = template.to_string();

        // Replace {{variable}} patterns. [loop-guard] The scan used to
        // restart from 0 after every replacement — a replacement drawn
        // from device data that itself contains `{{...}}` (e.g.
        // `{"a": "{{a}}"}`) re-expanded forever, hanging the async
        // executor worker. The cursor now advances past each replacement
        // so substituted content is never re-expanded; a hard iteration
        // cap bounds pathological input as belt-and-braces.
        let mut scan_from = 0usize;
        let mut iterations = 0usize;
        while let Some(rel) = result[scan_from..].find("{{") {
            let start = scan_from + rel;
            iterations += 1;
            if iterations > 1000 {
                tracing::warn!("render_template: iteration cap hit, leaving rest as-is");
                break;
            }
            let end = match result[start..].find("}}") {
                Some(pos) => start + pos + 2,
                None => break,
            };

            let var_name = &result[start + 2..end - 2].trim();

            // Special variables
            let replacement = if var_name == &"item" {
                item.and_then(value_as_f64)
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| item.map(|i| i.to_string()).unwrap_or_default())
            } else if var_name == &"index" {
                index.to_string()
            } else {
                // JSONPath extraction
                let path = if var_name.starts_with("$") {
                    var_name.to_string()
                } else {
                    format!("$.{}", var_name)
                };

                self.extract_value_by_path(root, &path)
                    .map(|v| {
                        if v.is_string() {
                            v.as_str().unwrap_or_default().to_string()
                        } else {
                            v.to_string()
                        }
                    })
                    .unwrap_or_default()
            };

            result.replace_range(start..end, &replacement);
            // Advance past the replacement — never re-expand substituted
            // content (the infinite-loop guard).
            scan_from = (start + replacement.len()).min(result.len());
        }

        result
    }
}
impl TransformEngine {
    /// Evaluate a mathematical expression
    pub(crate) fn evaluate_expression(&self, expr: &str, data: &Value) -> Result<f64> {
        // Very simple expression evaluator
        // In production, use a proper expression parsing library
        let sanitized = expr.replace("{{", "").replace("}}", "").trim().to_string();

        // Try to extract value and evaluate
        if let Ok(num) = sanitized.parse::<f64>() {
            return Ok(num);
        }

        // Try to extract from data
        match self.extract_value_by_path(data, &sanitized) {
            Ok(value) => value_as_f64(&value).ok_or_else(|| AutomationError::TransformError {
                operation: "Compute".to_string(),
                message: format!("Cannot convert to number: {}", value),
            }),
            Err(_) => Ok(0.0),
        }
    }
}
impl TransformEngine {
    /// Execute a Single operation - extract a value using JSONPath-like syntax
    pub(crate) async fn execute_single(
        &self,
        json_path: &str,
        output_metric: &str,
        device_id: &str,
        timestamp: i64,
        raw_data: &Value,
    ) -> Result<TransformedMetric> {
        let value = self.extract_value_by_path(raw_data, json_path)?;

        // Convert to f64
        let float_value = value_as_f64(&value).ok_or_else(|| AutomationError::TransformError {
            operation: "Single".to_string(),
            message: format!("Value at '{}' is not a number: {}", json_path, value),
        })?;

        Ok(TransformedMetric {
            device_id: device_id.to_string(),
            transform_id: None,
            metric: output_metric.to_string(),
            value: float_value.into(),
            timestamp,
            quality: Some(1.0),
        })
    }
}
impl TransformEngine {
    /// Execute GroupBy operation - group array elements by key and aggregate
    pub(crate) async fn execute_group_by(
        &self,
        over: &str,
        key: &str,
        aggregation: AggregationFunc,
        value_path: Option<&str>,
        output_pattern: &str,
        device_id: &str,
        timestamp: i64,
        raw_data: &Value,
    ) -> Result<Vec<TransformedMetric>> {
        let array_value = self.extract_value_by_path(raw_data, over)?;

        let array = match &array_value {
            Value::Array(arr) => arr,
            _ => {
                return Err(AutomationError::TransformError {
                    operation: "GroupBy".to_string(),
                    message: format!("Value at '{}' is not an array", over),
                });
            }
        };

        // Group by key and collect values
        use std::collections::HashMap;
        let mut groups: HashMap<String, Vec<f64>> = HashMap::new();

        for item in array {
            // Get the group key
            let group_key = if let Some(key_field) = item.get(key) {
                match key_field {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    _ => "unknown".to_string(),
                }
            } else {
                continue;
            };

            // Get the value to aggregate (if specified)
            let item_value = if let Some(path) = value_path {
                match self.extract_value_by_path(item, path) {
                    Ok(v) => value_as_f64(&v),
                    Err(_) => None,
                }
            } else {
                // For count aggregation, use 1.0 for each item
                Some(1.0)
            };

            if let Some(v) = item_value {
                groups.entry(group_key).or_default().push(v);
            }
        }

        // Compute aggregation for each group
        let mut metrics = Vec::new();
        for (group_key, values) in groups {
            let aggregated = self.compute_aggregation(&values, aggregation)?;
            let metric_name = format!("{}_{}", output_pattern, group_key);

            metrics.push(TransformedMetric {
                device_id: device_id.to_string(),
                transform_id: None,
                metric: metric_name,
                value: aggregated.into(),
                timestamp,
                quality: Some(1.0),
            });
        }

        Ok(metrics)
    }
}
impl TransformEngine {
    /// Execute Decode operation - convert encoded data to JSON
    pub(crate) async fn execute_decode(
        &self,
        from: &str,
        format: crate::automation::types::DecodeFormat,
        output: &str,
        device_id: &str,
        timestamp: i64,
        raw_data: &Value,
    ) -> Result<Vec<TransformedMetric>> {
        let encoded_value = self.extract_value_by_path(raw_data, from)?;

        let encoded_str = match &encoded_value {
            Value::String(s) => s.as_str(),
            _ => {
                return Err(AutomationError::TransformError {
                    operation: "Decode".to_string(),
                    message: format!("Value at '{}' is not a string", from),
                });
            }
        };

        let decoded = match format {
            crate::automation::types::DecodeFormat::Hex => {
                // Hex string to bytes to string
                match hex::decode(encoded_str) {
                    Ok(bytes) => String::from_utf8(bytes).unwrap_or_default(),
                    Err(e) => {
                        return Err(AutomationError::TransformError {
                            operation: "Decode".to_string(),
                            message: format!("Invalid hex string: {}", e),
                        });
                    }
                }
            }
            crate::automation::types::DecodeFormat::Base64 => {
                // Base64 to string
                use base64::Engine;
                let engine = base64::engine::general_purpose::STANDARD;
                match engine.decode(encoded_str) {
                    Ok(bytes) => String::from_utf8(bytes).unwrap_or_default(),
                    Err(e) => {
                        return Err(AutomationError::TransformError {
                            operation: "Decode".to_string(),
                            message: format!("Invalid base64 string: {}", e),
                        });
                    }
                }
            }
            crate::automation::types::DecodeFormat::Bytes => {
                // Already a string, assume UTF-8
                encoded_str.to_string()
            }
            crate::automation::types::DecodeFormat::Url => {
                // URL decode
                match urlencoding::decode(encoded_str) {
                    Ok(s) => s.into_owned(),
                    Err(e) => {
                        return Err(AutomationError::TransformError {
                            operation: "Decode".to_string(),
                            message: format!("Invalid URL encoding: {}", e),
                        });
                    }
                }
            }
            crate::automation::types::DecodeFormat::Csv => {
                // CSV parsing - return as-is for now
                encoded_str.to_string()
            }
        };

        // Try to parse as JSON, otherwise use as string
        let json_value: Value = if let Ok(v) = serde_json::from_str(&decoded) {
            v
        } else {
            Value::String(decoded.clone())
        };

        // Try numeric first; if not numeric, store the decoded string as Text
        // (previously hashed to a bogus float — silent data corruption).
        let value: MetricValue = value_as_f64(&json_value)
            .map(MetricValue::from)
            .unwrap_or_else(|| decoded.clone().into());

        Ok(vec![TransformedMetric {
            device_id: device_id.to_string(),
            transform_id: None,
            metric: output.to_string(),
            value,
            timestamp,
            quality: Some(1.0),
        }])
    }
}
impl TransformEngine {
    /// Execute Encode operation - convert JSON to encoded format
    pub(crate) async fn execute_encode(
        &self,
        from: &str,
        format: crate::automation::types::DecodeFormat,
        output: &str,
        device_id: &str,
        timestamp: i64,
        raw_data: &Value,
    ) -> Result<Vec<TransformedMetric>> {
        let value = self.extract_value_by_path(raw_data, from)?;

        let to_encode = match &value {
            Value::String(s) => s.clone(),
            _ => value.to_string(),
        };

        let encoded = match format {
            crate::automation::types::DecodeFormat::Hex => hex::encode(to_encode),
            crate::automation::types::DecodeFormat::Base64 => {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD.encode(to_encode)
            }
            crate::automation::types::DecodeFormat::Bytes => to_encode,
            crate::automation::types::DecodeFormat::Url => {
                urlencoding::encode(&to_encode).into_owned()
            }
            crate::automation::types::DecodeFormat::Csv => to_encode,
        };

        // Store the encoded string as Text (previously hashed to a bogus float).
        let value: MetricValue = encoded
            .trim()
            .parse::<f64>()
            .map(MetricValue::from)
            .unwrap_or_else(|_| encoded.clone().into());

        Ok(vec![TransformedMetric {
            device_id: device_id.to_string(),
            transform_id: None,
            metric: output.to_string(),
            value,
            timestamp,
            quality: Some(1.0),
        }])
    }
}
