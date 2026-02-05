//! AI-powered data path extraction from device samples.
//!
//! This module analyzes device data samples and automatically extracts
//! accessible data paths, their types, and value ranges.

use std::sync::Arc;

use crate::discovery::types::*;
use edge_ai_core::{LlmRuntime, Message, GenerationParams, llm::backend::LlmInput};

/// Extracts data paths from device samples
pub struct DataPathExtractor {
    llm: Arc<dyn LlmRuntime>,
}

impl DataPathExtractor {
    /// Create a new path extractor
    pub fn new(llm: Arc<dyn LlmRuntime>) -> Self {
        Self { llm }
    }

    /// Extract all accessible paths from samples
    pub async fn extract_paths(
        &self,
        samples: &[DeviceSample],
    ) -> Result<Vec<DiscoveredPath>> {
        if samples.is_empty() {
            return Err(DiscoveryError::InsufficientData(
                "No samples provided for path extraction".into()
            ));
        }

        // First, try to extract from JSON samples
        let json_samples: Vec<_> = samples.iter()
            .filter_map(|s| s.parsed.as_ref())
            .collect();

        if !json_samples.is_empty() {
            self.extract_from_json_samples(&json_samples).await
        } else {
            // For non-JSON samples, use LLM to analyze structure
            self.extract_from_raw_samples(samples).await
        }
    }

    /// Extract paths from JSON samples (primary method)
    async fn extract_from_json_samples(
        &self,
        samples: &[&serde_json::Value],
    ) -> Result<Vec<DiscoveredPath>> {
        let mut paths = std::collections::HashMap::new();
        let total_samples = samples.len();

        // Recursively extract all paths from each sample
        for (idx, sample) in samples.iter().enumerate() {
            self.extract_paths_recursive(
                sample,
                String::new(),
                &mut paths,
                idx,
                total_samples,
            );
        }

        // Convert to DiscoveredPath with analysis
        let mut result = Vec::new();
        for (path, info) in paths {
            let discovered = self.analyze_path(&path, &info, samples)?;
            result.push(discovered);
        }

        // Sort by coverage and path length
        result.sort_by(|a, b| {
            b.coverage.partial_cmp(&a.coverage)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.path.len().cmp(&b.path.len()))
        });

        Ok(result)
    }

    /// Recursively extract paths from a JSON value
    fn extract_paths_recursive(
        &self,
        value: &serde_json::Value,
        current_path: String,
        paths: &mut std::collections::HashMap<String, PathInfo>,
        sample_index: usize,
        total_samples: usize,
    ) {
        match value {
            serde_json::Value::Null => {
                // Record null value at this path
                self.record_path_value(paths, &current_path, DataType::Null, sample_index, total_samples);
            }
            serde_json::Value::Bool(_b) => {
                self.record_path_value(paths, &current_path, DataType::Boolean, sample_index, total_samples);
            }
            serde_json::Value::Number(n) => {
                let data_type = if n.is_i64() {
                    DataType::Integer
                } else {
                    DataType::Float
                };
                self.record_path_value(paths, &current_path, data_type, sample_index, total_samples);
            }
            serde_json::Value::String(s) => {
                self.record_path_value(paths, &current_path, DataType::String, sample_index, total_samples);

                // Check for encoded data (base64, hex)
                if self.looks_like_base64(s) {
                    paths.entry(format!("{}_decoded", current_path))
                        .or_insert_with(|| PathInfo::new(DataType::Binary))
                        .add_sample(sample_index, serde_json::Value::String(s.clone()));
                }
            }
            serde_json::Value::Array(arr) => {
                // Record array path
                self.record_path_value(paths, &current_path, DataType::Array, sample_index, total_samples);

                // Process array elements
                for (idx, element) in arr.iter().enumerate() {
                    let element_path = if current_path.is_empty() {
                        format!("{}", idx)
                    } else {
                        format!("{}.{}", current_path, idx)
                    };
                    self.extract_paths_recursive(element, element_path, paths, sample_index, total_samples);
                }
            }
            serde_json::Value::Object(obj) => {
                // Record object path
                self.record_path_value(paths, &current_path, DataType::Object, sample_index, total_samples);

                // Process object fields
                for (key, value) in obj {
                    let field_path = if current_path.is_empty() {
                        key.clone()
                    } else {
                        format!("{}.{}", current_path, key)
                    };
                    self.extract_paths_recursive(value, field_path, paths, sample_index, total_samples);
                }
            }
        }
    }

    /// Record a path value in the info map
    fn record_path_value(
        &self,
        paths: &mut std::collections::HashMap<String, PathInfo>,
        path: &str,
        data_type: DataType,
        sample_index: usize,
        _total_samples: usize,
    ) {
        let entry = paths.entry(path.to_string())
            .or_insert_with(|| PathInfo::new(data_type.clone()));

        entry.add_sample(sample_index, serde_json::Value::Null);
        entry.sample_count += 1;
        if entry.data_type == DataType::Unknown {
            entry.data_type = data_type;
        }
    }

    /// Analyze a path to create DiscoveredPath
    fn analyze_path(
        &self,
        path: &str,
        info: &PathInfo,
        samples: &[&serde_json::Value],
    ) -> Result<DiscoveredPath> {
        let mut sample_values = Vec::new();
        let mut numeric_values = Vec::new();
        let mut is_array = false;
        let mut is_object = false;

        // Extract values from all samples at this path
        for sample in samples {
            if let Some(value) = self.extract_by_path(sample, path) {
                let data_type = DataType::from_json(&value);

                match data_type {
                    DataType::Array => is_array = true,
                    DataType::Object => is_object = true,
                    _ => {}
                }

                if data_type.is_numeric()
                    && let Some(n) = value.as_f64() {
                        numeric_values.push(n);
                    }

                sample_values.push(value);
            }
        }

        let coverage = if samples.is_empty() {
            0.0
        } else {
            sample_values.len() as f32 / samples.len() as f32
        };

        let value_range = if !numeric_values.is_empty() {
            ValueRange::from_values(&numeric_values)
        } else {
            None
        };

        let data_type = if info.data_type != DataType::Unknown {
            info.data_type.clone()
        } else if !sample_values.is_empty() {
            DataType::from_json(&sample_values[0])
        } else {
            DataType::Unknown
        };

        Ok(DiscoveredPath {
            path: path.to_string(),
            data_type,
            is_consistent: coverage >= 1.0,
            coverage,
            sample_values,
            value_range,
            is_array,
            is_object,
            is_array_pattern: false,
            array_name: None,
            inferred_array_length: None,
        })
    }

    /// Extract value from JSON using dot/bracket notation path
    ///
    /// Supports:
    /// - "field" - direct field access
    /// - "field.nested" - nested field access
    /// - "field[0]" - array index (bracket notation)
    /// - "field[]" - array wildcard (returns first element or all elements as array)
    /// - "field.0" - array index (dot notation)
    fn extract_by_path(
        &self,
        value: &serde_json::Value,
        path: &str,
    ) -> Option<serde_json::Value> {
        if path.is_empty() {
            return Some(value.clone());
        }

        let parts: Vec<&str> = path.split('.').collect();
        let mut current = value;

        for part in parts {
            // Handle array indexing: sensors[0] or sensors[] for wildcard
            if let Some(bracket_start) = part.find('[') {
                let key = &part[..bracket_start];
                let bracket_part = &part[bracket_start..];

                // First navigate to the key
                if let Some(obj) = current.as_object() {
                    current = obj.get(key)?;
                } else {
                    return None;
                }

                // Then handle array indices or wildcards
                let mut bracket_iter = bracket_part.chars().peekable();
                while bracket_iter.peek() == Some(&'[') {
                    bracket_iter.next(); // consume '['

                    let index_str: String = bracket_iter
                        .by_ref()
                        .take_while(|c| *c != ']')
                        .collect();
                    bracket_iter.next(); // consume ']'

                    if index_str.is_empty() {
                        // Wildcard "[]" - return array or first element
                        if let Some(arr) = current.as_array() {
                            if arr.len() == 1 {
                                // Single element: unwrap it
                                current = &arr[0];
                            } else {
                                // Multiple elements: keep as array
                                // Keep current as the array itself
                                break;
                            }
                        }
                        // If not an array, keep current as-is
                    } else if let Ok(index) = index_str.parse::<usize>() {
                        if let Some(arr) = current.as_array() {
                            if index < arr.len() {
                                current = &arr[index];
                            } else {
                                return None;
                            }
                        } else {
                            return None;
                        }
                    }
                }
            } else if part.chars().all(|c: char| c.is_ascii_digit() || c == '-') {
                // Handle numeric array index in dot notation: sensors.0.type
                if let Ok(index) = part.parse::<usize>() {
                    if let Some(arr) = current.as_array() {
                        if index < arr.len() {
                            current = &arr[index];
                        } else {
                            return None;
                        }
                    } else {
                        return None;
                    }
                } else {
                    return None;
                }
            } else {
                // Simple field access
                current = current.get(part)?;
            }
        }

        Some(current.clone())
    }

    /// Extract paths from raw (non-JSON) samples using LLM
    async fn extract_from_raw_samples(
        &self,
        samples: &[DeviceSample],
    ) -> Result<Vec<DiscoveredPath>> {
        // Limit samples for LLM analysis
        let max_samples = 5;
        let limited_samples = &samples[..samples.len().min(max_samples)];

        // Build prompt for LLM
        let prompt = self.build_raw_analysis_prompt(limited_samples);

        let input = LlmInput {
            messages: vec![Message::user(prompt)],
            params: GenerationParams {
                temperature: Some(0.1),
                max_tokens: Some(1000),
                ..Default::default()
            },
            model: None,
            stream: false,
            tools: None,
        };

        let response = self.llm.generate(input).await
            .map_err(|e| DiscoveryError::Llm(format!("LLM call failed: {}", e)))?;

        // Parse LLM response
        self.parse_llm_path_response(&response.text)
    }

    /// Build prompt for raw sample analysis
    fn build_raw_analysis_prompt(&self, samples: &[DeviceSample]) -> String {
        let mut prompt = String::from(
            r#"Analyze the following raw device data samples and extract the data structure.

Samples:
"#
        );

        for (idx, sample) in samples.iter().enumerate() {
            let preview = if sample.raw_data.len() > 100 {
                format!("{}... ({} bytes total)",
                    String::from_utf8_lossy(&sample.raw_data[..100]),
                    sample.raw_data.len()
                )
            } else {
                format!("{} ({} bytes)",
                    String::from_utf8_lossy(&sample.raw_data),
                    sample.raw_data.len()
                )
            };

            prompt.push_str(&format!("\nSample {}:\n{}\n", idx + 1, preview));

            // Try to show as hex for binary data
            if sample.parsed.is_none() {
                let hex_preview: String = sample.raw_data.iter()
                    .take(32)
                    .map(|b| format!("{:02x}", b))
                    .collect::<Vec<_>>()
                    .join(" ");
                prompt.push_str(&format!("Hex preview: {}\n", hex_preview));
            }
        }

        prompt.push_str(
            r#"

Respond in JSON format:
{
  "encoding": "raw|hex|base64|...",
  "structure": "describe the data structure",
  "fields": [
    {
      "name": "field_name",
      "path": "extraction_path",
      "type": "string|int|float|bool",
      "description": "what this field represents",
      "sample_values": ["value1", "value2"]
    }
  ]
}

If the data appears to be:
- Hex encoded binary: describe the byte structure
- Base64 encoded: note that and describe the inner structure
- Raw binary: describe any visible patterns
"#
        );

        prompt
    }

    /// Parse LLM response for paths
    fn parse_llm_path_response(&self, response: &str) -> Result<Vec<DiscoveredPath>> {
        // Extract JSON from response
        let json_str = self.extract_json_from_response(response)?;

        let parsed: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| DiscoveryError::Parse(format!("Invalid LLM JSON: {}", e)))?;

        let mut paths = Vec::new();

        if let Some(fields) = parsed["fields"].as_array() {
            for field in fields {
                if let Some(name) = field["name"].as_str() {
                    paths.push(DiscoveredPath {
                        path: name.to_string(),
                        data_type: match field["type"].as_str() {
                            Some("string") => DataType::String,
                            Some("int") => DataType::Integer,
                            Some("float") => DataType::Float,
                            Some("bool") => DataType::Boolean,
                            _ => DataType::Unknown,
                        },
                        is_consistent: true,
                        coverage: 1.0, // LLM-provided paths assumed consistent
                        sample_values: field["sample_values"]
                            .as_array().cloned()
                            .unwrap_or_default(),
                        value_range: None,
                        is_array: false,
                        is_object: false,
                        is_array_pattern: false,
                        array_name: None,
                        inferred_array_length: None,
                    });
                }
            }
        }

        Ok(paths)
    }

    /// Extract JSON from LLM response (handles markdown code blocks)
    fn extract_json_from_response(&self, response: &str) -> Result<String> {
        let response = response.trim();

        // Check for JSON in markdown code block
        if response.starts_with("```") {
            let lines: Vec<&str> = response.lines().collect();
            let mut in_json_block = false;
            let mut json_lines = Vec::new();

            for line in &lines[1..] {
                if line.starts_with("```") {
                    if in_json_block {
                        break;
                    }
                } else if in_json_block || line.trim().starts_with('{') || line.trim().starts_with('[') {
                    in_json_block = true;
                    json_lines.push(line);
                }
            }

            if !json_lines.is_empty() {
                let joined: String = json_lines.iter().map(|s| s.to_string()).collect::<Vec<_>>().join("\n");
                return Ok(joined);
            }
        }

        // Direct JSON
        if response.starts_with('{') || response.starts_with('[') {
            return Ok(response.to_string());
        }

        // Find first JSON object
        if let Some(start) = response.find('{')
            && let Some(end) = response.rfind('}') {
                return Ok(response[start..=end].to_string());
            }

        Err(DiscoveryError::Parse("No JSON found in LLM response".into()))
    }

    /// Validate a path against all samples
    pub fn validate_path(
        &self,
        path: &str,
        samples: &[DeviceSample],
    ) -> PathValidity {
        if samples.is_empty() {
            return PathValidity::Invalid;
        }

        let mut null_count = 0;
        let mut found_count = 0;
        let mut inconsistent = false;

        for sample in samples {
            if let Some(parsed) = &sample.parsed {
                match self.extract_by_path(parsed, path) {
                    Some(value) => {
                        found_count += 1;
                        if value.is_null() {
                            null_count += 1;
                        }
                    }
                    None => {
                        inconsistent = true;
                    }
                }
            } else {
                inconsistent = true;
            }
        }

        if found_count == 0 {
            PathValidity::NotFound
        } else {
            let coverage = found_count as f32 / samples.len() as f32;
            // Consider valid if present in at least 50% of samples
            if inconsistent {
                PathValidity::Inconsistent
            } else if coverage >= 0.5 && null_count < found_count {
                PathValidity::Valid
            } else if null_count == found_count {
                PathValidity::NullValue
            } else {
                PathValidity::Inconsistent
            }
        }
    }

    /// Check if a string looks like base64 encoded data
    fn looks_like_base64(&self, s: &str) -> bool {
        if s.len() < 4 {
            return false;
        }

        // Base64 typically has length multiple of 4 and uses specific characters
        if !s.len().is_multiple_of(4) {
            return false;
        }

        s.chars().all(|c| c.is_alphanumeric() || c == '+' || c == '/' || c == '=')
    }

    /// Get suggested access paths for a semantic type
    pub fn suggest_paths_for_semantic(
        &self,
        discovered_paths: &[DiscoveredPath],
        semantic_type: SemanticType,
    ) -> Vec<String> {
        let mut suggestions = Vec::new();

        // Common patterns for each semantic type
        let patterns = match semantic_type {
            SemanticType::Temperature => {
                vec!["temp", "temperature", "t", "value.temp", "data.temp"]
            }
            SemanticType::Humidity => {
                vec!["hum", "humidity", "h", "value.hum", "data.hum"]
            }
            SemanticType::Battery => {
                vec!["battery", "batt", "power.battery"]
            }
            SemanticType::Switch => {
                vec!["state", "status", "power", "on"]
            }
            _ => {
                vec!["value", "data"]
            }
        };

        for pattern in &patterns {
            for discovered in discovered_paths {
                let path_lower = discovered.path.to_lowercase();
                if path_lower.contains(pattern) {
                    suggestions.push(discovered.path.clone());
                }
            }
        }

        // Remove duplicates while preserving order
        let mut unique = Vec::new();
        for s in suggestions {
            if !unique.contains(&s) {
                unique.push(s);
            }
        }

        unique
    }
}

/// Internal info for path extraction
#[derive(Debug, Clone)]
struct PathInfo {
    /// Data type at this path
    data_type: DataType,
    /// Number of samples containing this path
    sample_count: usize,
    /// Sample values at this path
    values: Vec<(usize, serde_json::Value)>,
}

impl PathInfo {
    fn new(data_type: DataType) -> Self {
        Self {
            data_type,
            sample_count: 0,
            values: Vec::new(),
        }
    }

    fn add_sample(&mut self, sample_index: usize, value: serde_json::Value) {
        self.values.push((sample_index, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    // Mock LLM for testing
    struct MockLlm;

    #[async_trait::async_trait]
    impl edge_ai_core::llm::backend::LlmRuntime for MockLlm {
        fn backend_id(&self) -> edge_ai_core::llm::backend::BackendId {
            edge_ai_core::llm::backend::BackendId::new("mock")
        }

        fn model_name(&self) -> &str {
            "mock"
        }

        fn max_context_length(&self) -> usize {
            4096
        }

        fn capabilities(&self) -> edge_ai_core::llm::backend::BackendCapabilities {
            edge_ai_core::llm::backend::BackendCapabilities::default()
        }

        async fn generate(
            &self,
            _input: edge_ai_core::llm::backend::LlmInput,
        ) -> std::result::Result<edge_ai_core::llm::backend::LlmOutput, edge_ai_core::llm::backend::LlmError> {
            Ok(edge_ai_core::llm::backend::LlmOutput {
                text: r#"{"fields": [{"name": "temperature", "type": "float", "sample_values": [25.5, 26.0, 24.8]}]}"#.to_string(),
                finish_reason: edge_ai_core::llm::backend::FinishReason::Stop,
                usage: None,
                thinking: None,
            })
        }

        async fn generate_stream(
            &self,
            _input: edge_ai_core::llm::backend::LlmInput,
        ) -> std::result::Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>, edge_ai_core::llm::backend::LlmError> {
            use futures::stream;
            Ok(Box::pin(stream::empty()))
        }
    }

    use std::pin::Pin;
    use futures::Stream;
    use edge_ai_core::llm::backend::StreamChunk;

    #[tokio::test]
    async fn test_extract_from_json_samples() {
        let llm = Arc::new(MockLlm);
        let extractor = DataPathExtractor::new(llm);

        let samples = vec![
            DeviceSample::from_json(
                serde_json::json!({"temp": 25.5, "hum": 60}),
                "test"
            ),
            DeviceSample::from_json(
                serde_json::json!({"temp": 26.0, "hum": 61}),
                "test"
            ),
            DeviceSample::from_json(
                serde_json::json!({"temp": 24.8, "hum": 59}),
                "test"
            ),
        ];

        let paths = extractor.extract_paths(&samples).await.unwrap();

        // Should find paths: temp, hum
        assert!(paths.iter().any(|p| p.path == "temp"));
        assert!(paths.iter().any(|p| p.path == "hum"));

        // Check temp path
        let temp = paths.iter().find(|p| p.path == "temp").unwrap();
        assert_eq!(temp.data_type, DataType::Float);
        assert_eq!(temp.coverage, 1.0);
        assert_eq!(temp.sample_values.len(), 3);
    }

    #[tokio::test]
    async fn test_extract_nested_json() {
        let llm = Arc::new(MockLlm);
        let extractor = DataPathExtractor::new(llm);

        let samples = vec![DeviceSample::from_json(
            serde_json::json!({
                "payload": {
                    "sensors": [
                        {"t": "temp", "v": 25.5},
                        {"t": "hum", "v": 60}
                    ]
                }
            }),
            "test"
        )];

        let paths = extractor.extract_paths(&samples).await.unwrap();

        // Should extract nested paths
        assert!(paths.iter().any(|p| p.path.contains("payload")));
        assert!(paths.iter().any(|p| p.path.contains("sensors")));
    }

    #[test]
    fn test_extract_by_path() {
        let extractor = DataPathExtractor {
            llm: Arc::new(MockLlm),
        };

        let value = serde_json::json!({
            "data": {
                "sensors": {
                    "temp": 25.5
                }
            }
        });

        // Test dot notation
        assert_eq!(
            extractor.extract_by_path(&value, "data.sensors.temp"),
            Some(serde_json::json!(25.5))
        );

        // Test array indexing
        let array_value = serde_json::json!({
            "data": {
                "sensors": [
                    {"type": "temp", "value": 25.5},
                    {"type": "hum", "value": 60}
                ]
            }
        });

        assert_eq!(
            extractor.extract_by_path(&array_value, "data.sensors.0.type"),
            Some(serde_json::json!("temp"))
        );
    }

    #[test]
    fn test_validate_path() {
        let extractor = DataPathExtractor {
            llm: Arc::new(MockLlm),
        };

        let samples = vec![
            DeviceSample::from_json(
                serde_json::json!({"temp": 25.5, "hum": 60}),
                "test"
            ),
            DeviceSample::from_json(
                serde_json::json!({"temp": 26.0}),
                "test"
            ),
            DeviceSample::from_json(
                serde_json::json!({"other": 123}),
                "test"
            ),
        ];

        // Consistent path
        assert_eq!(
            extractor.validate_path("temp", &samples),
            PathValidity::Inconsistent  // 2 out of 3 samples have "temp"
        );

        // Partially present
        assert_eq!(
            extractor.validate_path("hum", &samples),
            PathValidity::Inconsistent
        );

        // Not present
        assert_eq!(
            extractor.validate_path("other", &samples),
            PathValidity::Inconsistent
        );

        // Missing
        assert_eq!(
            extractor.validate_path("missing", &samples),
            PathValidity::NotFound
        );
    }

    #[test]
    fn test_suggest_paths_for_semantic() {
        let extractor = DataPathExtractor {
            llm: Arc::new(MockLlm),
        };

        let discovered = vec![
            DiscoveredPath {
                path: "payload.sensors[0].v".to_string(),
                data_type: DataType::Float,
                is_consistent: true,
                coverage: 1.0,
                sample_values: vec![],
                value_range: None,
                is_array: false,
                is_object: false,
                is_array_pattern: false,
                array_name: None,
                inferred_array_length: None,
            },
            DiscoveredPath {
                path: "data.temperature".to_string(),
                data_type: DataType::Float,
                is_consistent: true,
                coverage: 1.0,
                sample_values: vec![],
                value_range: None,
                is_array: false,
                is_object: false,
                is_array_pattern: false,
                array_name: None,
                inferred_array_length: None,
            },
        ];

        let suggestions = extractor.suggest_paths_for_semantic(&discovered, SemanticType::Temperature);

        assert!(!suggestions.is_empty());
    }
}
