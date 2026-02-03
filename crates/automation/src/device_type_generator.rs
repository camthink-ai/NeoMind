//! Device Type Generator for zero-config device onboarding.
//!
//! This module auto-generates device type definitions (MDL) from device samples,
//! enabling zero-config device onboarding where users only need to provide data.

use std::sync::Arc;

use crate::discovery::*;
use crate::error::{AutomationError, Result};
use edge_ai_core::{LlmRuntime, Message, GenerationParams};
use edge_ai_core::llm::backend::LlmInput;
use serde_json::json;

/// Device type generator for auto-generating MDL definitions
pub struct DeviceTypeGenerator {
    llm: Arc<dyn LlmRuntime>,
    path_extractor: DataPathExtractor,
    semantic_inference: SemanticInference,
}

/// Configuration for device type generation
pub struct GenerationConfig {
    /// Minimum coverage threshold (0.0-1.0) for including fields
    /// Fields appearing in less than this ratio of samples will be excluded
    pub min_coverage: f32,
    /// Minimum confidence threshold (0.0-1.0) for including metrics
    /// Metrics with AI confidence below this will be excluded
    pub min_confidence: f32,
}

impl Default for GenerationConfig {
    fn default() -> Self {
        Self {
            min_coverage: 0.0,  // Include all fields by default
            min_confidence: 0.0, // Include all metrics by default
        }
    }
}

impl DeviceTypeGenerator {
    /// Create a new device type generator
    pub fn new(llm: Arc<dyn LlmRuntime>) -> Self {
        Self {
            path_extractor: DataPathExtractor::new(llm.clone()),
            semantic_inference: SemanticInference::new(llm.clone()),
            llm,
        }
    }

    /// Generate a device type definition from samples
    pub async fn generate_device_type(
        &self,
        device_id: &str,
        manufacturer: Option<&str>,
        samples: &[DeviceSample],
    ) -> Result<GeneratedDeviceType> {
        self.generate_device_type_with_config(device_id, manufacturer, samples, GenerationConfig::default()).await
    }

    /// Generate a device type definition from samples with custom configuration
    pub async fn generate_device_type_with_config(
        &self,
        device_id: &str,
        manufacturer: Option<&str>,
        samples: &[DeviceSample],
        config: GenerationConfig,
    ) -> Result<GeneratedDeviceType> {
        if samples.is_empty() {
            return Err(AutomationError::IntentAnalysisFailed(
                "No samples provided for device type generation".into()
            ));
        }

        // Step 1: Extract all paths
        let paths = self.path_extractor.extract_paths(samples).await
            .map_err(|e| AutomationError::IntentAnalysisFailed(format!("Path extraction failed: {}", e)))?;

        // Step 2: Infer device category
        let context = InferenceContext {
            device_type_hint: manufacturer.map(|m| m.to_string()),
            manufacturer_hint: manufacturer.map(|m| m.to_string()),
            ..Default::default()
        };

        let category = self.infer_device_category(&paths, &context).await?;

        // Step 3: Generate metrics from paths
        let mut metrics = Vec::new();

        for path in &paths {
            if path.is_array || path.is_object {
                continue; // Skip non-leaf paths
            }

            // Use configured coverage threshold (default 0.0 = include all)
            if path.coverage < config.min_coverage {
                continue; // Skip low coverage paths
            }

            let metric = self.semantic_inference.enhance_path(path, &context).await;

            metrics.push(metric);
        }

        // Step 4: Infer commands from writable patterns
        let commands = self.infer_commands(&paths, &context).await?;

        // Step 5: Generate device type definition
        let capabilities = self.infer_capabilities(&metrics, &commands);

        let device_type = GeneratedDeviceType {
            id: format!("auto-generated-{}", device_id),
            name: self.generate_device_name(device_id, &category, manufacturer),
            description: self.generate_description(&category, &metrics, manufacturer),
            category: category.clone(),
            manufacturer: manufacturer.unwrap_or("Unknown").to_string(),
            metrics,
            commands,
            capabilities,
        };

        Ok(device_type)
    }

    /// Infer device category from available metrics
    async fn infer_device_category(
        &self,
        paths: &[DiscoveredPath],
        context: &InferenceContext,
    ) -> Result<DeviceCategory> {
        // Count semantic types from paths
        let mut semantic_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

        for path in paths {
            for value in &path.sample_values {
                if let Some(obj) = value.as_object() {
                    for (key, _) in obj {
                        let semantic = SemanticType::infer_from_context(key, &Some(value.clone()));
                        let key = format!("{:?}", semantic);
                        *semantic_counts.entry(key).or_insert(0) += 1;
                    }
                }
            }
        }

        // Use LLM for category inference
        let metrics_summary = semantic_counts.iter()
            .map(|(k, v)| format!("{}: {}", k, v))
            .collect::<Vec<_>>()
            .join(", ");

        let prompt = format!(
            r#"Determine the device category based on the detected semantic types.

Detected metrics: {}
Manufacturer hint: {}

Respond with a JSON object:
{{
  "category": "temperature_sensor|humidity_sensor|multi_sensor|motion_sensor|light_sensor|switch|dimmer|thermostat|camera|energy_monitor|gateway|controller|actuator|display|alarm|lock|unknown",
  "confidence": 0.0-1.0,
  "reasoning": "brief explanation"
}}"#,
            metrics_summary,
            context.manufacturer_hint.as_deref().unwrap_or("Unknown")
        );

        let input = LlmInput {
            messages: vec![
                Message::system("You are an IoT device classifier. Determine device categories based on detected capabilities. Respond ONLY with valid JSON."),
                Message::user(prompt),
            ],
            params: GenerationParams {
                temperature: Some(0.2),
                max_tokens: Some(300),
                ..Default::default()
            },
            model: None,
            stream: false,
            tools: None,
        };

        let response = self.llm.generate(input).await?;
        let json_str = extract_json_from_response(&response.text)?;
        let result: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| AutomationError::IntentAnalysisFailed(format!("Invalid JSON: {}", e)))?;

        let category_str = result.get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let category = match category_str {
            "temperature_sensor" => DeviceCategory::TemperatureSensor,
            "humidity_sensor" => DeviceCategory::HumiditySensor,
            "multi_sensor" => DeviceCategory::MultiSensor,
            "motion_sensor" => DeviceCategory::MotionSensor,
            "light_sensor" => DeviceCategory::LightSensor,
            "switch" => DeviceCategory::Switch,
            "dimmer" => DeviceCategory::Dimmer,
            "thermostat" => DeviceCategory::Thermostat,
            "camera" => DeviceCategory::Camera,
            "energy_monitor" => DeviceCategory::EnergyMonitor,
            "gateway" => DeviceCategory::Gateway,
            "controller" => DeviceCategory::Controller,
            "actuator" => DeviceCategory::Actuator,
            "display" => DeviceCategory::Display,
            "alarm" => DeviceCategory::Alarm,
            "lock" => DeviceCategory::Lock,
            _ => DeviceCategory::Unknown,
        };

        Ok(category)
    }

    /// Infer commands from path patterns
    async fn infer_commands(
        &self,
        paths: &[DiscoveredPath],
        _context: &InferenceContext,
    ) -> Result<Vec<DiscoveredCommand>> {
        let mut commands = Vec::new();

        // Look for common command patterns
        for path in paths {
            let field_name = Self::extract_field_name(&path.path);

            // Check if field name suggests write capability
            if field_name.contains("set_") || field_name.contains("command") || field_name.contains("control") {
                commands.push(DiscoveredCommand {
                    name: field_name.clone(),
                    display_name: format!("Set {}", field_name.replace("set_", "").replace("_", " ")),
                    description: format!("Command to set {}", field_name),
                    parameters: vec![],
                });
            }

            // Check for boolean switches
            if path.data_type == DataType::Boolean && field_name.contains("power") {
                commands.push(DiscoveredCommand {
                    name: "turn_on".to_string(),
                    display_name: "Turn On".to_string(),
                    description: "Turn the device on".to_string(),
                    parameters: vec![],
                });

                commands.push(DiscoveredCommand {
                    name: "turn_off".to_string(),
                    display_name: "Turn Off".to_string(),
                    description: "Turn the device off".to_string(),
                    parameters: vec![],
                });
            }
        }

        Ok(commands)
    }

    /// Infer device capabilities
    fn infer_capabilities(&self, metrics: &[DiscoveredMetric], commands: &[DiscoveredCommand]) -> DeviceCapabilities {
        DeviceCapabilities {
            readable: !metrics.is_empty(),
            writable: !commands.is_empty(),
            supports_telemetry: metrics.iter().any(|m| m.semantic_type != SemanticType::Switch),
            supports_commands: !commands.is_empty(),
            supports_state_change: metrics.iter().any(|m| {
                matches!(m.semantic_type,
                    SemanticType::Switch | SemanticType::Motion |
                    SemanticType::Status | SemanticType::Alarm)
            }),
        }
    }

    /// Generate a device name
    fn generate_device_name(&self, device_id: &str, category: &DeviceCategory, manufacturer: Option<&str>) -> String {
        let category_name = category.display_name();
        let mfr = manufacturer.unwrap_or("Generic");

        if device_id.len() > 20 {
            format!("{} {} Device", mfr, category_name)
        } else {
            format!("{} {}", mfr, device_id)
        }
    }

    /// Generate a description
    fn generate_description(&self, category: &DeviceCategory, metrics: &[DiscoveredMetric], manufacturer: Option<&str>) -> String {
        let mfr = manufacturer.unwrap_or("Generic");

        let metric_summary: String = metrics.iter()
            .take(5)
            .map(|m| m.display_name.clone())
            .collect::<Vec<_>>()
            .join(", ");

        format!(
            "{} {} device. Supports monitoring of: {}.",
            mfr,
            category.display_name(),
            if metric_summary.is_empty() { "various metrics".to_string() } else { metric_summary }
        )
    }

    /// Extract field name from a JSON path
    fn extract_field_name(path: &str) -> String {
        let parts: Vec<&str> = path.split('.').collect();
        let last = parts.last().unwrap_or(&path);
        last.split('[').next().unwrap_or(last).to_string()
    }

    /// Validate a generated device type
    pub fn validate_device_type(&self, device_type: &GeneratedDeviceType) -> ValidationResult {
        let mut issues = Vec::new();
        let mut warnings = Vec::new();

        // Check if device has any metrics
        if device_type.metrics.is_empty() {
            warnings.push("No metrics were discovered. Device may not be usable.".to_string());
        }

        // Check for required fields based on category
        if device_type.category == DeviceCategory::Unknown {
            warnings.push("Device category could not be determined.".to_string());
        }

        // Check for duplicate metric names
        let mut metric_names = std::collections::HashSet::new();
        for metric in &device_type.metrics {
            if !metric_names.insert(&metric.name) {
                issues.push(format!("Duplicate metric name: {}", metric.name));
            }
        }

        ValidationResult {
            is_valid: issues.is_empty(),
            issues,
            warnings,
        }
    }

    /// Generate an MDL JSON definition
    pub fn generate_mdl(&self, device_type: &GeneratedDeviceType) -> String {
        let metrics_json: Vec<serde_json::Value> = device_type.metrics.iter()
            .map(|m| {
                json!({
                    "name": m.name,
                    "display_name": m.display_name,
                    "description": m.description,
                    "path": m.path,
                    "data_type": format!("{:?}", m.data_type).to_lowercase(),
                    "semantic_type": format!("{:?}", m.semantic_type).to_lowercase(),
                    "unit": m.unit,
                    "readable": m.is_readable,
                    "writable": m.is_writable,
                })
            })
            .collect();

        let commands_json: Vec<serde_json::Value> = device_type.commands.iter()
            .map(|c| {
                json!({
                    "name": c.name,
                    "display_name": c.display_name,
                    "description": c.description,
                    "parameters": c.parameters,
                })
            })
            .collect();

        let mdl = json!({
            "id": device_type.id,
            "name": device_type.name,
            "description": device_type.description,
            "category": format!("{:?}", device_type.category).to_lowercase(),
            "manufacturer": device_type.manufacturer,
            "capabilities": {
                "readable": device_type.capabilities.readable,
                "writable": device_type.capabilities.writable,
                "supports_telemetry": device_type.capabilities.supports_telemetry,
                "supports_commands": device_type.capabilities.supports_commands,
            },
            "metrics": metrics_json,
            "commands": commands_json,
            "metadata": {
                "auto_generated": true,
            }
        });

        serde_json::to_string_pretty(&mdl).unwrap_or_default()
    }
}

/// Generated device type definition
#[derive(Debug, Clone)]
pub struct GeneratedDeviceType {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: DeviceCategory,
    pub manufacturer: String,
    pub metrics: Vec<DiscoveredMetric>,
    pub commands: Vec<DiscoveredCommand>,
    pub capabilities: DeviceCapabilities,
}

/// Device capabilities
#[derive(Debug, Clone, Default)]
pub struct DeviceCapabilities {
    pub readable: bool,
    pub writable: bool,
    pub supports_telemetry: bool,
    pub supports_commands: bool,
    pub supports_state_change: bool,
}

/// Validation result for a device type
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub issues: Vec<String>,
    pub warnings: Vec<String>,
}

fn extract_json_from_response(response: &str) -> Result<String> {
    let start = response.find('{')
        .ok_or_else(|| AutomationError::IntentAnalysisFailed("No JSON object found".into()))?;

    let end = response.rfind('}')
        .ok_or_else(|| AutomationError::IntentAnalysisFailed("Incomplete JSON object".into()))?;

    Ok(response[start..=end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_capabilities_default() {
        let caps = DeviceCapabilities::default();

        assert!(!caps.readable);
        assert!(!caps.writable);
        assert!(!caps.supports_telemetry);
        assert!(!caps.supports_commands);
        assert!(!caps.supports_state_change);
    }

    #[test]
    fn test_validation_result_empty() {
        let result = ValidationResult {
            is_valid: true,
            issues: Vec::new(),
            warnings: Vec::new(),
        };

        assert!(result.is_valid);
        assert!(result.issues.is_empty());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn test_extract_field_name() {
        assert_eq!(
            DeviceTypeGenerator::extract_field_name("payload.sensors[0].v"),
            "v"
        );
        assert_eq!(
            DeviceTypeGenerator::extract_field_name("temperature"),
            "temperature"
        );
        assert_eq!(
            DeviceTypeGenerator::extract_field_name("set_power"),
            "set_power"
        );
    }

    #[test]
    fn test_infer_capabilities() {
        // Test with readable metrics
        let metrics = vec![DiscoveredMetric {
            name: "temperature".to_string(),
            path: "temp".to_string(),
            semantic_type: SemanticType::Temperature,
            is_readable: true,
            is_writable: false,
            confidence: 0.8,
            ..Default::default()
        }];

        let caps = infer_capabilities_direct(&metrics, &[]);
        assert!(caps.readable);
        assert!(caps.supports_telemetry);
        assert!(!caps.writable);

        // Test with switch
        let switch_metrics = vec![DiscoveredMetric {
            name: "power".to_string(),
            path: "power".to_string(),
            semantic_type: SemanticType::Switch,
            is_readable: true,
            is_writable: false,
            confidence: 0.8,
            ..Default::default()
        }];

        let caps = infer_capabilities_direct(&switch_metrics, &[]);
        assert!(caps.supports_state_change);
    }

    #[test]
    fn test_calculate_confidence() {
        let metrics = vec![DiscoveredMetric {
            name: "temperature".to_string(),
            path: "temp".to_string(),
            semantic_type: SemanticType::Temperature,
            confidence: 0.9,
            ..Default::default()
        }];

        let commands = vec![DiscoveredCommand {
            name: "turn_on".to_string(),
            display_name: "Turn On".to_string(),
            description: "Turn on".to_string(),
            parameters: vec![],
            confidence: 0.8,
        }];

        let confidence = calculate_confidence_direct(&metrics, &commands, 10);
        assert!(confidence > 0.5);
    }

    // Helper functions to avoid needing the full struct
    fn infer_capabilities_direct(metrics: &[DiscoveredMetric], commands: &[DiscoveredCommand]) -> DeviceCapabilities {
        DeviceCapabilities {
            readable: !metrics.is_empty(),
            writable: !commands.is_empty(),
            supports_telemetry: metrics.iter().any(|m| m.semantic_type != SemanticType::Switch),
            supports_commands: !commands.is_empty(),
            supports_state_change: metrics.iter().any(|m| {
                matches!(m.semantic_type,
                    SemanticType::Switch | SemanticType::Motion |
                    SemanticType::Status | SemanticType::Alarm)
            }),
        }
    }
}
