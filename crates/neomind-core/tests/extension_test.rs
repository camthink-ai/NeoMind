//! Unit tests for Extension types and traits (V2)
//!
//! Tests the unified command-based extension system:
//! - ExtensionCommand and ParameterDefinition types
//! - MetricDefinition and MetricDataType
//! - ExtensionMetadata (V2 - no extension_type)
//! - ExtensionError

use neomind_core::extension::*;
use std::collections::HashMap;

// ========================================================================
// MetricDataType Tests
// ========================================================================

#[test]
fn test_metric_data_type_serialize() {
    let dt = MetricDataType::Float;
    let json = serde_json::to_string(&dt).unwrap();
    assert_eq!(json, "\"float\"");

    let dt = MetricDataType::Integer;
    let json = serde_json::to_string(&dt).unwrap();
    assert_eq!(json, "\"integer\"");

    let dt = MetricDataType::Boolean;
    let json = serde_json::to_string(&dt).unwrap();
    assert_eq!(json, "\"boolean\"");

    let dt = MetricDataType::String;
    let json = serde_json::to_string(&dt).unwrap();
    assert_eq!(json, "\"string\"");

    let dt = MetricDataType::Binary;
    let json = serde_json::to_string(&dt).unwrap();
    assert_eq!(json, "\"binary\"");

    let dt = MetricDataType::Enum {
        options: vec!["opt1".to_string(), "opt2".to_string()],
    };
    let json = serde_json::to_string(&dt).unwrap();
    assert_eq!(json, "{\"enum\":[\"opt1\",\"opt2\"]}");
}

#[test]
fn test_metric_data_type_deserialize() {
    let json = "\"float\"";
    let dt: MetricDataType = serde_json::from_str(json).unwrap();
    assert_eq!(dt, MetricDataType::Float);

    let json = "\"integer\"";
    let dt: MetricDataType = serde_json::from_str(json).unwrap();
    assert_eq!(dt, MetricDataType::Integer);

    let json = "{\"enum\":[\"a\",\"b\"]}";
    let dt: MetricDataType = serde_json::from_str(json).unwrap();
    match dt {
        MetricDataType::Enum { options } => {
            assert_eq!(options, vec!["a", "b"]);
        }
        _ => panic!("Expected Enum type"),
    }
}

// ========================================================================
// MetricDefinition Tests
// ========================================================================

#[test]
fn test_metric_definition_builder() {
    let metric = MetricDefinition {
        name: "temperature".to_string(),
        display_name: "Temperature".to_string(),
        data_type: MetricDataType::Float,
        unit: "°C".to_string(),
        min: Some(-40.0),
        max: Some(120.0),
        required: true,
    };

    assert_eq!(metric.name, "temperature");
    assert_eq!(metric.display_name, "Temperature");
    assert_eq!(metric.unit, "°C");
    assert_eq!(metric.min, Some(-40.0));
    assert_eq!(metric.max, Some(120.0));
}

#[test]
fn test_metric_definition_serialization() {
    let metric = MetricDefinition {
        name: "humidity".to_string(),
        display_name: "Humidity".to_string(),
        data_type: MetricDataType::Integer,
        unit: "%".to_string(),
        min: Some(0.0),
        max: Some(100.0),
        required: true,
    };

    let json = serde_json::to_string(&metric).unwrap();
    let parsed: MetricDefinition = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.name, "humidity");
    assert_eq!(parsed.display_name, "Humidity");
    assert_eq!(parsed.unit, "%");
}

// ========================================================================
// ParameterDefinition Tests
// ========================================================================

#[test]
fn test_parameter_definition() {
    let param = ParameterDefinition {
        name: "threshold".to_string(),
        display_name: "Threshold".to_string(),
        description: "Detection threshold".to_string(),
        param_type: MetricDataType::Float,
        required: true,
        default_value: Some(ParamMetricValue::Float(0.5)),
        min: Some(0.0),
        max: Some(1.0),
        options: vec![],
    };

    assert_eq!(param.name, "threshold");
    assert_eq!(param.required, true);
}

// ========================================================================
// ExtensionCommand (CommandDefinition) Tests
// ========================================================================

#[test]
fn test_extension_command() {
    let mut fixed_values: HashMap<String, serde_json::Value> = HashMap::new();

    let cmd = ExtensionCommand {
        name: "detect_objects".to_string(),
        display_name: "Detect Objects".to_string(),
        payload_template: "{ \"image_path\": {{image_path}} }".to_string(),
        parameters: vec![ParameterDefinition {
            name: "image_path".to_string(),
            display_name: "Image Path".to_string(),
            description: "Path to image file".to_string(),
            param_type: MetricDataType::String,
            required: true,
            default_value: None,
            min: None,
            max: None,
            options: vec![],
        }],
        fixed_values,
        samples: vec![],
        llm_hints: "Detect objects in an image using YOLO".to_string(),
        parameter_groups: vec![],
    };

    assert_eq!(cmd.name, "detect_objects");
    assert_eq!(cmd.parameters.len(), 1);
    assert_eq!(cmd.parameters[0].name, "image_path");
}

#[test]
fn test_extension_command_serialization() {
    let cmd = ExtensionCommand {
        name: "get_weather".to_string(),
        display_name: "Get Weather".to_string(),
        payload_template: "{}".to_string(),
        parameters: vec![],
        fixed_values: HashMap::new(),
        samples: vec![],
        llm_hints: "Get current weather".to_string(),
        parameter_groups: vec![],
    };

    let json = serde_json::to_string(&cmd).unwrap();
    let parsed: ExtensionCommand = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.name, "get_weather");
}

// ========================================================================
// ExtensionMetadata Tests (V2)
// ========================================================================

#[test]
fn test_extension_metadata_builder() {
    let meta = ExtensionMetadata::new(
        "neomind.weather.live",
        "Live Weather Provider",
        semver::Version::new(1, 0, 0),
    )
    .with_description("Provides real-time weather data")
    .with_author("NeoMind Team");

    assert_eq!(meta.id, "neomind.weather.live");
    assert_eq!(meta.name, "Live Weather Provider");
    assert_eq!(meta.version.major, 1);
    // V2: extension_type removed
    assert_eq!(
        meta.description,
        Some("Provides real-time weather data".to_string())
    );
    assert_eq!(meta.author, Some("NeoMind Team".to_string()));
}

#[test]
fn test_extension_metadata_serialization() {
    let meta = ExtensionMetadata::new("test.ext", "Test Extension", semver::Version::new(0, 1, 0));

    let json = serde_json::to_string(&meta).unwrap();
    let parsed: ExtensionMetadata = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.id, "test.ext");
    assert_eq!(parsed.name, "Test Extension");
}

// ========================================================================
// ExtensionError Tests
// ========================================================================

#[test]
fn test_extension_error_display() {
    let err = ExtensionError::CommandNotFound("test_command".to_string());
    assert!(err.to_string().contains("Command not found"));
    assert!(err.to_string().contains("test_command"));
}

#[test]
fn test_extension_error_invalid_arguments() {
    let err = ExtensionError::InvalidArguments("missing required field 'city'".to_string());
    assert!(err.to_string().contains("Invalid arguments"));
}

#[test]
fn test_extension_error_execution_failed() {
    let err = ExtensionError::ExecutionFailed("Connection timeout".to_string());
    assert!(err.to_string().contains("Execution failed"));
}

#[test]
fn test_extension_error_timeout() {
    let err = ExtensionError::Timeout("Test timeout".to_string());
    assert!(err.to_string().contains("timeout"));
}

// ========================================================================
// ExtensionMetricValue Tests
// ========================================================================

#[test]
fn test_extension_metric_value_new() {
    let val = ExtensionMetricValue::new("temperature", ParamMetricValue::Float(42.0));
    assert_eq!(val.name, "temperature");
}

#[test]
fn test_extension_metric_value_serialization() {
    let val = ExtensionMetricValue::new("count", ParamMetricValue::Integer(42));
    let json = serde_json::to_string(&val).unwrap();

    // Check that it serializes correctly
    assert!(json.contains("count"));
    assert!(json.contains("42"));
}

// ========================================================================
// ToolDescriptor Tests
// ========================================================================

#[test]
fn test_tool_descriptor() {
    let tool = ToolDescriptor {
        name: "detect_objects".to_string(),
        description: "Detect objects in image".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "image_path": { "type": "string" }
            }
        }),
        returns: Some("Detected objects".to_string()),
    };

    assert_eq!(tool.name, "detect_objects");
    assert_eq!(tool.returns, Some("Detected objects".to_string()));
}

#[test]
fn test_tool_descriptor_serialization() {
    let tool = ToolDescriptor {
        name: "get_weather".to_string(),
        description: "Get weather data".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            }
        }),
        returns: Some("Weather data".to_string()),
    };

    let json = serde_json::to_string(&tool).unwrap();
    let parsed: ToolDescriptor = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.name, "get_weather");
}

// ========================================================================
// ExtensionState Tests
// ========================================================================

#[test]
fn test_extension_state_serialization() {
    let state = ExtensionState::Running;
    let json = serde_json::to_string(&state).unwrap();
    let parsed: ExtensionState = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, ExtensionState::Running);
}

#[test]
fn test_extension_state_display() {
    assert_eq!(ExtensionState::Running.to_string(), "Running");
    assert_eq!(ExtensionState::Stopped.to_string(), "Stopped");
    assert_eq!(ExtensionState::Error.to_string(), "Error");
}
