//! Unified data extraction for all device adapters.
//!
//! This module provides a single, consistent way for all device adapters
//! (MQTT, HTTP, Webhook, etc.) to process incoming device data.
//!
//! ## Features
//!
//! - Dot notation path extraction (e.g., "values.battery", "data.sensors[0].temp")
//! - Raw data preservation as `_raw` metric
//! - Template-driven extraction based on device type definitions
//! - Auto-extraction fallback for undefined devices
//! - Consistent MetricValue conversion
//!
//! ## Extraction Modes
//!
//! 1. **Template-driven**: When device type has defined metrics, extract only those
//! 2. **Auto-extraction**: When no template exists, extract all top-level fields
//! 3. **Raw-only**: Store only `_raw` for debugging/replay

use crate::mdl::MetricValue;
use crate::registry::DeviceRegistry;
use serde_json::Value;
use std::sync::Arc;
use tracing::{debug, trace, warn};

/// Configuration for the extraction process.
#[derive(Debug, Clone)]
pub struct ExtractionConfig {
    /// Always store raw data as `_raw` metric
    pub store_raw: bool,
    /// Auto-extract fields when no template is defined
    pub auto_extract: bool,
    /// Maximum depth for nested field extraction
    pub max_depth: usize,
    /// Include arrays in auto-extraction
    pub include_arrays: bool,
}

impl Default for ExtractionConfig {
    fn default() -> Self {
        Self {
            store_raw: true,
            auto_extract: true,
            max_depth: 10,
            include_arrays: false,
        }
    }
}

/// A single extracted metric with its metadata.
#[derive(Debug, Clone)]
pub struct ExtractedMetric {
    /// Metric name (as defined in template or auto-generated)
    pub name: String,
    /// Metric value
    pub value: MetricValue,
    /// Source path in the original data (for debugging)
    pub source_path: String,
}

/// Result of a data extraction operation.
#[derive(Debug, Clone)]
pub struct ExtractionResult {
    /// Raw data stored (if enabled)
    pub raw_stored: bool,
    /// Extracted metrics
    pub metrics: Vec<ExtractedMetric>,
    /// Extraction mode used
    pub mode: ExtractionMode,
    /// Warnings (e.g., missing fields)
    pub warnings: Vec<String>,
}

/// Mode used for extraction.
#[derive(Debug, Clone, PartialEq)]
pub enum ExtractionMode {
    /// Template-driven extraction (metrics defined in device type)
    TemplateDriven,
    /// Auto-extraction from top-level fields
    AutoExtract,
    /// Raw data only stored
    RawOnly,
    /// No data extracted
    NoData,
}

/// Unified data extractor for all device adapters.
///
/// This extractor provides consistent behavior across MQTT, HTTP, Webhook,
/// and any future adapters.
pub struct UnifiedExtractor {
    /// Device registry for template lookup
    device_registry: Arc<DeviceRegistry>,
    /// Extraction configuration
    config: ExtractionConfig,
}

impl UnifiedExtractor {
    /// Create a new unified extractor.
    pub fn new(device_registry: Arc<DeviceRegistry>) -> Self {
        Self {
            device_registry,
            config: ExtractionConfig::default(),
        }
    }

    /// Create with custom configuration.
    pub fn with_config(device_registry: Arc<DeviceRegistry>, config: ExtractionConfig) -> Self {
        Self {
            device_registry,
            config,
        }
    }

    /// Extract metrics from raw device data.
    ///
    /// This is the main entry point for data extraction. It handles:
    /// 1. Raw data storage (as `_raw` metric)
    /// 2. Template-driven extraction (if device type exists)
    /// 3. Auto-extraction fallback (for unknown devices)
    ///
    /// # Arguments
    /// * `device_id` - Device identifier
    /// * `device_type` - Device type identifier (for template lookup)
    /// * `raw_data` - Raw JSON data from device
    ///
    /// # Returns
    /// Extraction result with metrics and metadata
    pub async fn extract(
        &self,
        device_id: &str,
        device_type: &str,
        raw_data: &Value,
    ) -> ExtractionResult {
        let mut metrics = Vec::new();
        let mut warnings = Vec::new();
        let mut raw_stored = false;

        // Step 1: Always store raw data if configured
        if self.config.store_raw {
            let raw_value = self.value_to_metric_value(raw_data);
            metrics.push(ExtractedMetric {
                name: "_raw".to_string(),
                value: raw_value,
                source_path: "$".to_string(),
            });
            raw_stored = true;
        }

        // Step 2: Try template-driven extraction
        let template = self.device_registry.get_template(device_type).await;

        let mode = if let Some(template) = template {
            // Check if device type is in Simple mode (raw data only)
            if matches!(template.mode, crate::registry::DeviceTypeMode::Simple) {
                debug!(
                    "Device '{}' of type '{}' is in Simple (Raw Data) mode - storing raw data only",
                    device_id, device_type
                );
                ExtractionMode::RawOnly
            } else if !template.metrics.is_empty() {
                // Template has defined metrics - extract using dot notation
                debug!(
                    "Using template-driven extraction for device '{}' of type '{}': {} metrics defined",
                    device_id,
                    device_type,
                    template.metrics.len()
                );

                for metric_def in &template.metrics {
                    match self.extract_by_path(raw_data, &metric_def.name, 0) {
                        Ok(Some(value)) => {
                            let metric_value = self.value_to_metric_value(&value);
                            trace!("Extracted metric '{}' = {:?}", metric_def.name, metric_value);
                            metrics.push(ExtractedMetric {
                                name: metric_def.name.clone(),
                                value: metric_value,
                                source_path: metric_def.name.clone(),
                            });
                        }
                        Ok(None) => {
                            // Path not found in data - not an error, metric might be optional
                            trace!(
                                "Metric '{}' not found in payload for device '{}'",
                                metric_def.name,
                                device_id
                            );
                        }
                        Err(e) => {
                            // Extraction error (e.g., circular reference)
                            warn!(
                                "Failed to extract metric '{}' for device '{}': {}",
                                metric_def.name, device_id, e
                            );
                            warnings.push(format!("{}: {}", metric_def.name, e));
                        }
                    }
                }
                ExtractionMode::TemplateDriven
            } else {
                // Template exists but no metrics defined - use auto-extract
                self.auto_extract(raw_data, device_id, &mut metrics);
                ExtractionMode::AutoExtract
            }
        } else if self.config.auto_extract {
            // No template - use auto-extraction
            debug!(
                "No template found for device type '{}', using auto-extraction for device '{}'",
                device_type, device_id
            );
            self.auto_extract(raw_data, device_id, &mut metrics);
            ExtractionMode::AutoExtract
        } else {
            // No template and auto-extract disabled - raw only
            debug!(
                "No template found for device type '{}' and auto-extract disabled, storing raw only for device '{}'",
                device_type, device_id
            );
            ExtractionMode::RawOnly
        };

        ExtractionResult {
            raw_stored,
            metrics,
            mode,
            warnings,
        }
    }

    /// Extract a value using dot notation path.
    ///
    /// Supports:
    /// - Nested objects: "values.battery"
    /// - Array indices: "data.sensors[0]"
    /// - Combined: "values.data[0].temp"
    ///
    /// # Arguments
    /// * `data` - JSON data to extract from
    /// * `path` - Dot notation path
    /// * `depth` - Current depth (for circular reference protection)
    ///
    /// # Returns
    /// - `Ok(Some(value))` - Value found
    /// - `Ok(None)` - Path not found (not an error)
    /// - `Err(e)` - Extraction error (e.g., max depth exceeded)
    pub fn extract_by_path(
        &self,
        data: &Value,
        path: &str,
        depth: usize,
    ) -> Result<Option<Value>, String> {
        // Handle empty path - return None
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }

        // Handle root notation
        if trimmed == "$" {
            return Ok(Some(data.clone()));
        }

        // Handle trailing dot - malformed path
        if trimmed.ends_with('.') {
            return Ok(None);
        }

        if depth > self.config.max_depth {
            return Err(format!("Max depth exceeded: {}", self.config.max_depth));
        }

        let parts: Vec<&str> = path.split('.').collect();
        if parts.is_empty() || parts.iter().all(|p| p.is_empty()) {
            return Ok(None);
        }

        // Count non-empty parts to determine actual path depth
        let actual_depth = parts.iter().filter(|p| !p.trim().is_empty() && **p != "$").count();
        if actual_depth > self.config.max_depth {
            return Err(format!(
                "Max depth {} exceeded: path has {} levels",
                self.config.max_depth, actual_depth
            ));
        }

        let mut current = data;
        for (i, part) in parts.iter().enumerate() {
            let part = part.trim();
            if part.is_empty() {
                // Skip empty parts (but we already checked for trailing dots)
                continue;
            }
            if part == "$" {
                // Root notation in middle of path - skip
                continue;
            }

            // Handle array notation [index]
            if let Some(bracket_start) = part.find('[')
                && let Some(bracket_end) = part.find(']') {
                    let key = &part[0..bracket_start];
                    let index_str = &part[bracket_start + 1..bracket_end];

                    // First navigate to the key
                    if !key.is_empty() {
                        match current {
                            Value::Object(map) => {
                                current = map.get(key).ok_or_else(|| {
                                    format!("Key '{}' not found at part {}", key, i)
                                })?;
                            }
                            _ => return Ok(None),
                        }
                    }

                    // Then access the array index
                    let index: usize = index_str
                        .parse()
                        .map_err(|_| format!("Invalid array index: {}", index_str))?;

                    match current {
                        Value::Array(arr) => {
                            // Return None for out of bounds instead of error
                            current = match arr.get(index) {
                                Some(v) => v,
                                None => return Ok(None),
                            };
                        }
                        _ => return Ok(None),
                    }
                    continue;
                }

            // Regular object key access
            match current {
                Value::Object(map) => {
                    current = map.get(part).unwrap_or(&Value::Null);
                }
                Value::Array(arr) => {
                    // Try to parse as array index
                    if let Ok(index) = part.parse::<usize>() {
                        current = arr.get(index).unwrap_or(&Value::Null);
                    } else {
                        current = &Value::Null;
                    }
                }
                // If current is not an object/array (e.g., we're already at null or a primitive value),
                // we can't navigate further, so set to null
                _ => {
                    current = &Value::Null;
                }
            }

            // Note: We continue even if current is null, as null values are valid metrics
        }

        Ok(Some(current.clone()))
    }

    /// Auto-extract metrics from top-level JSON fields.
    ///
    /// Used when no template is defined for the device type.
    /// Now supports recursive extraction of nested objects (e.g., values.battery).
    fn auto_extract(&self, data: &Value, device_id: &str, metrics: &mut Vec<ExtractedMetric>) {
        self.auto_extract_recursive(data, device_id, metrics, "", 0);
    }

    /// Recursive helper for auto-extract.
    ///
    /// Expands nested objects to create dot-notation metric names.
    fn auto_extract_recursive(
        &self,
        data: &Value,
        device_id: &str,
        metrics: &mut Vec<ExtractedMetric>,
        parent_path: &str,
        depth: usize,
    ) {
        // Check depth limit
        if depth > self.config.max_depth {
            trace!(
                "Max depth {} reached for device '{}' in auto-extract",
                self.config.max_depth,
                device_id
            );
            return;
        }

        if let Some(obj) = data.as_object() {
            for (key, value) in obj {
                // Skip _raw as it's already stored
                if key == "_raw" {
                    continue;
                }

                let current_path = if parent_path.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", parent_path, key)
                };

                // Check if value is a nested object
                if let Value::Object(nested_obj) = value {
                    // First check: if this object only contains other objects (no primitives/arrays),
                    // skip it and recurse directly - don't create a metric for this intermediate layer
                    let has_only_objects = nested_obj.iter().all(|(_, v)| {
                        matches!(v, Value::Object(_))
                    });

                    if has_only_objects && !nested_obj.is_empty() {
                        // This is a pure intermediate object layer - skip and dive deeper
                        trace!(
                            "Skipping intermediate object layer '{}' for device '{}' (auto-extract) - recursing into children",
                            current_path,
                            device_id
                        );
                        self.auto_extract_recursive(value, device_id, metrics, &current_path, depth + 1);
                    } else if !nested_obj.is_empty() {
                        // Check if the nested object only contains primitive values (numbers, strings, booleans, null)
                        let has_only_primitives = nested_obj.iter().all(|(_, v)| {
                            matches!(v, Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_))
                        });

                        if has_only_primitives {
                            // This looks like a metrics object - expand it without storing the parent
                            trace!(
                                "Expanding nested object '{}' for device '{}' (auto-extract) - only has primitives",
                                current_path,
                                device_id
                            );
                            self.auto_extract_recursive(value, device_id, metrics, &current_path, depth + 1);
                        } else {
                            // Mixed object (contains both objects and primitives) - expand to get primitives,
                            // but don't store this intermediate layer either
                            trace!(
                                "Expanding mixed object '{}' for device '{}' (auto-extract) - has both objects and primitives",
                                current_path,
                                device_id
                            );
                            self.auto_extract_recursive(value, device_id, metrics, &current_path, depth + 1);
                        }
                    } else {
                        // Empty object - skip
                        trace!(
                            "Skipping empty object '{}' for device '{}' (auto-extract)",
                            current_path,
                            device_id
                        );
                    }
                } else if let Value::Array(arr) = value {
                    // Handle arrays based on config
                    if self.config.include_arrays && arr.len() <= 10 {
                        // Small array - expand as JSON string
                        let metric_value = self.value_to_metric_value(value);
                        metrics.push(ExtractedMetric {
                            name: current_path.clone(),
                            value: metric_value,
                            source_path: format!("$.{}", current_path),
                        });
                    } else {
                        // Large array or arrays not included - store as JSON string
                        let metric_value = self.value_to_metric_value(value);
                        metrics.push(ExtractedMetric {
                            name: current_path.clone(),
                            value: metric_value,
                            source_path: format!("$.{}", current_path),
                        });
                    }
                } else {
                    // Primitive value - add as metric
                    let metric_value = self.value_to_metric_value(value);
                    metrics.push(ExtractedMetric {
                        name: current_path.clone(),
                        value: metric_value,
                        source_path: format!("$.{}", current_path),
                    });
                }
            }
        }
    }

    /// Convert a JSON value to MetricValue.
    pub fn value_to_metric_value(&self, value: &Value) -> MetricValue {
        match value {
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    MetricValue::Integer(i)
                } else if let Some(f) = n.as_f64() {
                    MetricValue::Float(f)
                } else {
                    MetricValue::Null
                }
            }
            Value::String(s) => MetricValue::String(s.clone()),
            Value::Bool(b) => MetricValue::Boolean(*b),
            Value::Null => MetricValue::Null,
            Value::Array(arr) => {
                // Convert array to JSON string
                MetricValue::String(serde_json::to_string(arr).unwrap_or_default())
            }
            Value::Object(obj) => {
                // Convert object to JSON string
                MetricValue::String(serde_json::to_string(obj).unwrap_or_default())
            }
        }
    }

    /// Get the extraction configuration.
    pub fn config(&self) -> &ExtractionConfig {
        &self.config
    }

    /// Update the extraction configuration.
    pub fn set_config(&mut self, config: ExtractionConfig) {
        self.config = config;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_test_registry() -> Arc<DeviceRegistry> {
        Arc::new(DeviceRegistry::new())
    }

    #[test]
    fn test_extract_by_path_simple() {
        let extractor = UnifiedExtractor::new(create_test_registry());
        let data = json!({
            "battery": 85,
            "temp": 23.5
        });

        assert_eq!(
            extractor.extract_by_path(&data, "battery", 0).unwrap(),
            Some(json!(85))
        );
        assert_eq!(
            extractor.extract_by_path(&data, "temp", 0).unwrap(),
            Some(json!(23.5))
        );
        assert_eq!(
            extractor.extract_by_path(&data, "missing", 0).unwrap(),
            None
        );
    }

    #[test]
    fn test_extract_by_path_nested() {
        let extractor = UnifiedExtractor::new(create_test_registry());
        let data = json!({
            "values": {
                "battery": 85,
                "devMac": "AA:BB:CC:DD"
            },
            "ts": 1234567890
        });

        assert_eq!(
            extractor.extract_by_path(&data, "values.battery", 0).unwrap(),
            Some(json!(85))
        );
        assert_eq!(
            extractor.extract_by_path(&data, "values.devMac", 0).unwrap(),
            Some(json!("AA:BB:CC:DD"))
        );
        assert_eq!(
            extractor.extract_by_path(&data, "ts", 0).unwrap(),
            Some(json!(1234567890))
        );
    }

    #[test]
    fn test_extract_by_path_array() {
        let extractor = UnifiedExtractor::new(create_test_registry());
        let data = json!({
            "sensors": [
                {"name": "temp1", "value": 23.5},
                {"name": "temp2", "value": 24.1}
            ]
        });

        assert_eq!(
            extractor.extract_by_path(&data, "sensors[0]", 0).unwrap(),
            Some(json!({"name": "temp1", "value": 23.5}))
        );
        assert_eq!(
            extractor.extract_by_path(&data, "sensors[1].value", 0).unwrap(),
            Some(json!(24.1))
        );
    }

    #[test]
    fn test_extract_by_path_combined() {
        let extractor = UnifiedExtractor::new(create_test_registry());
        let data = json!({
            "data": {
                "values": {
                    "battery": 85
                }
            }
        });

        assert_eq!(
            extractor.extract_by_path(&data, "data.values.battery", 0).unwrap(),
            Some(json!(85))
        );
    }

    #[test]
    fn test_value_to_metric_value() {
        let extractor = UnifiedExtractor::new(create_test_registry());

        assert!(matches!(
            extractor.value_to_metric_value(&json!(42)),
            MetricValue::Integer(42)
        ));
        assert!(matches!(
            extractor.value_to_metric_value(&json!(23.5)),
            MetricValue::Float(23.5)
        ));
        assert!(matches!(
            extractor.value_to_metric_value(&json!("hello")),
            MetricValue::String(_)
        ));
        assert!(matches!(
            extractor.value_to_metric_value(&json!(true)),
            MetricValue::Boolean(true)
        ));
        assert!(matches!(
            extractor.value_to_metric_value(&json!(null)),
            MetricValue::Null
        ));
    }

    #[test]
    fn test_extract_max_depth() {
        let config = ExtractionConfig {
            store_raw: false,
            auto_extract: false,
            max_depth: 2,
            include_arrays: false,
        };
        let extractor = UnifiedExtractor::with_config(create_test_registry(), config);
        let data = json!({
            "a": {
                "b": {
                    "c": "too deep"
                }
            }
        });

        assert!(extractor
            .extract_by_path(&data, "a.b.c", 0)
            .is_err());
    }

    #[tokio::test]
    async fn test_extract_no_template_auto_extract() {
        let registry = create_test_registry();
        let extractor = UnifiedExtractor::new(registry);

        let data = json!({
            "battery": 85,
            "temp": 23.5,
            "ts": 1234567890
        });

        let result = extractor.extract("device1", "unknown_type", &data).await;

        assert_eq!(result.mode, ExtractionMode::AutoExtract);
        assert!(result.raw_stored);
        assert_eq!(result.metrics.len(), 4); // _raw + 3 fields
    }

    #[tokio::test]
    async fn test_extract_no_template_no_auto_extract() {
        let config = ExtractionConfig {
            store_raw: true,
            auto_extract: false,
            max_depth: 10,
            include_arrays: false,
        };
        let registry = create_test_registry();
        let extractor = UnifiedExtractor::with_config(registry, config);

        let data = json!({
            "battery": 85,
            "temp": 23.5
        });

        let result = extractor.extract("device1", "unknown_type", &data).await;

        assert_eq!(result.mode, ExtractionMode::RawOnly);
        assert!(result.raw_stored);
        assert_eq!(result.metrics.len(), 1); // Only _raw
    }
}
