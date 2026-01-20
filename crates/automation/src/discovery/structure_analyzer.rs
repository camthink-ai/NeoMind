//! Structure analyzer for deterministic data structure extraction.
//!
//! This module provides fast, reliable structure inference without LLM dependency.
//! It analyzes JSON samples to extract paths, data types, and basic statistics.

use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Result of structure analysis
#[derive(Debug, Clone)]
pub struct StructureResult {
    /// Discovered paths with their data types
    pub paths: Vec<PathInfo>,
    /// Sample count used for analysis
    pub sample_count: usize,
    /// Whether the structure appears consistent across samples
    pub is_consistent: bool,
}

/// Information about a single path
#[derive(Debug, Clone)]
pub struct PathInfo {
    /// JSONPath to the value (e.g., "$.temperature", "$.sensors[0].value")
    pub path: String,
    /// Inferred data type
    pub data_type: InferredType,
    /// Whether the value is always present
    pub always_present: bool,
    /// Whether this is an array element
    pub is_array_element: bool,
    /// Array path if this is inside an array (e.g., "$.sensors[*]")
    pub array_path: Option<String>,
    /// Presence count (how many samples had this field)
    pub presence_count: usize,
}

/// Inferred data type from samples
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InferredType {
    /// Integer number
    Integer,
    /// Floating point number
    Float,
    /// Boolean
    Boolean,
    /// String
    String,
    /// Array
    Array(Box<InferredType>),
    /// Object/Struct
    Object,
    /// Null (only null values seen)
    Null,
    /// Mixed (multiple types seen)
    Mixed(Vec<InferredType>),
    /// Unknown / Not enough data
    Unknown,
}

impl InferredType {
    /// Get the display name for this type
    pub fn display_name(&self) -> &'static str {
        match self {
            InferredType::Integer => "integer",
            InferredType::Float => "float",
            InferredType::Boolean => "boolean",
            InferredType::String => "string",
            InferredType::Array(_) => "array",
            InferredType::Object => "object",
            InferredType::Null => "null",
            InferredType::Mixed(_) => "mixed",
            InferredType::Unknown => "unknown",
        }
    }

    /// Check if this is a numeric type
    pub fn is_numeric(&self) -> bool {
        matches!(self, InferredType::Integer | InferredType::Float)
    }
}

/// Value statistics for a path
#[derive(Debug, Clone)]
pub struct ValueStats {
    /// Minimum value (for numeric types)
    pub min: Option<f64>,
    /// Maximum value (for numeric types)
    pub max: Option<f64>,
    /// Average value (for numeric types)
    pub avg: Option<f64>,
    /// Most common string value (for strings)
    pub most_common_string: Option<String>,
    /// String length range
    pub string_length: Option<(usize, usize)>,
    /// Whether values appear to be hex-encoded
    pub hex_like: bool,
    /// Whether values appear to be boolean-like (0/1, true/false)
    pub boolean_like: bool,
    /// Unit/hint from value patterns
    pub unit_hint: Option<String>,
}

/// Structure analyzer - extracts schema from JSON samples
pub struct StructureAnalyzer {
    /// Minimum confidence threshold for consistency check
    consistency_threshold: f64,
}

impl StructureAnalyzer {
    /// Create a new structure analyzer
    pub fn new() -> Self {
        Self {
            consistency_threshold: 0.8,
        }
    }

    /// Set consistency threshold (0.0 - 1.0)
    pub fn with_consistency_threshold(mut self, threshold: f64) -> Self {
        self.consistency_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Analyze JSON samples to extract structure
    pub fn analyze(&self, samples: &[Value]) -> StructureResult {
        if samples.is_empty() {
            return StructureResult {
                paths: vec![],
                sample_count: 0,
                is_consistent: true,
            };
        }

        let sample_count = samples.len();
        let mut path_data: HashMap<String, PathTypeInfo> = HashMap::new();

        // Extract paths from each sample
        for (idx, sample) in samples.iter().enumerate() {
            self.extract_paths(&mut path_data, sample, idx);
        }

        // Convert to result
        let paths = self.finalize_paths(&path_data, sample_count);
        let is_consistent = self.check_consistency(&path_data, sample_count);

        StructureResult {
            paths,
            sample_count,
            is_consistent,
        }
    }

    /// Extract paths from a single sample
    fn extract_paths(&self, path_data: &mut PathTypeInfoMap, value: &Value, sample_idx: usize) {
        match value {
            Value::Object(map) => {
                for (key, val) in map {
                    let path = format!("$.{}", key);
                    self.update_path_info(path_data, &path, val, sample_idx, false, None);
                    // Recurse into nested objects
                    self.extract_paths_recursive(path_data, &path, val, sample_idx, false);
                }
            }
            Value::Array(arr) => {
                for (idx, val) in arr.iter().enumerate() {
                    let path = format!("$[{}]", idx);
                    self.update_path_info(path_data, &path, val, sample_idx, true, Some("$[*]".to_string()));
                    self.extract_paths_recursive(path_data, &path, val, sample_idx, true);
                }
            }
            // Primitive types at root - store as-is
            Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null => {
                let path = "$".to_string();
                self.update_path_info(path_data, &path, value, sample_idx, false, None);
            }
        }
    }

    /// Recursively extract nested paths
    fn extract_paths_recursive(
        &self,
        path_data: &mut PathTypeInfoMap,
        parent_path: &str,
        value: &Value,
        sample_idx: usize,
        in_array: bool,
    ) {
        match value {
            Value::Object(map) => {
                for (key, val) in map {
                    let path = format!("{}.{}", parent_path, key);
                    self.update_path_info(path_data, &path, val, sample_idx, in_array, None);
                    self.extract_paths_recursive(path_data, &path, val, sample_idx, in_array);
                }
            }
            Value::Array(arr) => {
                for (idx, val) in arr.iter().enumerate() {
                    let path = format!("{}[{}]", parent_path, idx);
                    self.update_path_info(path_data, &path, val, sample_idx, in_array, Some(format!("{}[*]", parent_path)));
                    self.extract_paths_recursive(path_data, &path, val, sample_idx, true);
                }
            }
            _ => {
                // Primitive type - already handled by update_path_info
            }
        }
    }

    /// Update path information from a single value
    fn update_path_info(
        &self,
        path_data: &mut PathTypeInfoMap,
        path: &str,
        value: &Value,
        sample_idx: usize,
        is_array_element: bool,
        array_path: Option<String>,
    ) {
        let entry = path_data.entry(path.to_string()).or_insert_with(|| PathTypeInfo {
            data_types: HashSet::new(),
            presence: HashSet::new(),
            is_array_element,
            array_path: array_path.clone(),
            values: vec![],
        });

        entry.presence.insert(sample_idx);

        let inferred_type = match value {
            Value::Number(n) => {
                if n.is_i64() || n.is_u64() {
                    InferredType::Integer
                } else {
                    InferredType::Float
                }
            }
            Value::String(_) => InferredType::String,
            Value::Bool(_) => InferredType::Boolean,
            Value::Array(_) => InferredType::Array(Box::new(InferredType::Unknown)),
            Value::Object(_) => InferredType::Object,
            Value::Null => InferredType::Null,
        };

        entry.data_types.insert(inferred_type);

        // Store a sample value (limit stored values)
        if entry.values.len() < 100 {
            entry.values.push(value.clone());
        }
    }

    /// Finalize path information into result format
    fn finalize_paths(&self, path_data: &PathTypeInfoMap, sample_count: usize) -> Vec<PathInfo> {
        path_data
            .iter()
            .map(|(path, info)| {
                let data_type = if info.data_types.len() == 1 {
                    info.data_types.iter().next().unwrap().clone()
                } else if info.data_types.is_empty() {
                    InferredType::Unknown
                } else {
                    InferredType::Mixed(info.data_types.iter().cloned().collect())
                };

                PathInfo {
                    path: path.clone(),
                    data_type,
                    always_present: info.presence.len() == sample_count,
                    is_array_element: info.is_array_element,
                    array_path: info.array_path.clone(),
                    presence_count: info.presence.len(),
                }
            })
            .collect()
    }

    /// Check if structure is consistent across samples
    fn check_consistency(&self, path_data: &PathTypeInfoMap, sample_count: usize) -> bool {
        if sample_count <= 1 {
            return true;
        }

        // Check if most paths appear in most samples
        let consistent_paths = path_data
            .values()
            .filter(|info| {
                let presence_ratio = info.presence.len() as f64 / sample_count as f64;
                presence_ratio >= self.consistency_threshold
            })
            .count();

        // If more than 80% of paths are consistent, consider structure consistent
        if path_data.is_empty() {
            return true;
        }

        let consistency_ratio = consistent_paths as f64 / path_data.len() as f64;
        consistency_ratio >= 0.8
    }
}

impl Default for StructureAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

/// Internal type for tracking path information during analysis
type PathTypeInfoMap = HashMap<String, PathTypeInfo>;

#[derive(Debug, Clone)]
struct PathTypeInfo {
    data_types: HashSet<InferredType>,
    presence: HashSet<usize>,
    is_array_element: bool,
    array_path: Option<String>,
    values: Vec<Value>,
}

/// Get a normalized path for use as a metric name
pub fn normalize_path(path: &str) -> String {
    path.replace("$.", "")
        .replace("$", "")
        .replace("[*]", "")
        .replace("[", "_")
        .replace("]", "")
        .replace(".", "_")
}

/// Extract a simple field name from a JSON path
pub fn extract_field_name(path: &str) -> String {
    if let Some(last_dot) = path.rfind('.') {
        path[last_dot + 1..].to_string()
    } else if let Some(bracket) = path.find('[') {
        path[..bracket].to_string()
    } else {
        path.replace("$", "")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path() {
        assert_eq!(normalize_path("$.temperature"), "temperature");
        assert_eq!(normalize_path("$.sensors[0].temp"), "sensors_0_temp");
        assert_eq!(normalize_path("$"), "");
        assert_eq!(normalize_path("$.data.humidity"), "data_humidity");
    }

    #[test]
    fn test_extract_field_name() {
        assert_eq!(extract_field_name("$.temperature"), "temperature");
        assert_eq!(extract_field_name("$.sensors[0]"), "sensors");
        assert_eq!(extract_field_name("$.data.humidity"), "humidity");
    }

    #[test]
    fn test_analyze_simple_object() {
        let analyzer = StructureAnalyzer::new();
        let samples = vec![
            json!({"temperature": 23.5, "humidity": 65}),
            json!({"temperature": 24.1, "humidity": 63}),
        ];

        let result = analyzer.analyze(&samples);

        assert_eq!(result.sample_count, 2);
        assert!(result.is_consistent);
        assert_eq!(result.paths.len(), 2);

        // Check temperature path
        let temp_path = result.paths.iter().find(|p| p.path == "$.temperature").unwrap();
        assert!(matches!(temp_path.data_type, InferredType::Float));
        assert!(temp_path.always_present);
    }

    #[test]
    fn test_analyze_nested_structure() {
        let analyzer = StructureAnalyzer::new();
        let samples = vec![
            json!({"sensors": [{"type": "temp", "value": 23.5}]}),
            json!({"sensors": [{"type": "hum", "value": 65}]}),
        ];

        let result = analyzer.analyze(&samples);

        // Should find paths at different levels
        assert!(result.paths.iter().any(|p| p.path == "$.sensors"));
        assert!(result.paths.iter().any(|p| p.path.contains("type")));
        assert!(result.paths.iter().any(|p| p.path.contains("value")));
    }

    #[test]
    fn test_analyze_array_at_root() {
        let analyzer = StructureAnalyzer::new();
        let samples = vec![
            json!([23.5, 24.1, 22.8]),
        ];

        let result = analyzer.analyze(&samples);

        // Should detect array elements
        assert!(result.paths.iter().any(|p| p.is_array_element));
    }

    #[test]
    fn test_consistency_check() {
        let analyzer = StructureAnalyzer::new();

        // Consistent structure
        let consistent = vec![
            json!({"temp": 20, "hum": 50}),
            json!({"temp": 21, "hum": 51}),
            json!({"temp": 22, "hum": 52}),
        ];
        let result = analyzer.analyze(&consistent);
        assert!(result.is_consistent);

        // Inconsistent structure
        let inconsistent = vec![
            json!({"temp": 20, "hum": 50}),
            json!({"temp": 21, "pressure": 101}),  // different field
            json!({"co2": 400}),  // completely different
        ];
        let result2 = analyzer.analyze(&inconsistent);
        // Should still mark as somewhat consistent since 'temp' is common
        // but lower consistency
    }
}
