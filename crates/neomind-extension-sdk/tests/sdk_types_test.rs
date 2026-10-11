//! Comprehensive Unit Tests for Extension SDK
//!
//! Tests cover:
//! - SDK metadata types
//! - Metric types and builders
//! - Command types and builders
//! - Parameter types and builders
//! - Error types
//! - Frontend manifest types
//! - Argument parser helpers

use neomind_extension_sdk::*;
use serde_json::json;

// ============================================================================
// SDK Metadata Tests
// ============================================================================

#[test]
fn test_sdk_metadata_creation() {
    let meta = SdkExtensionMetadata::new("test.extension", "Test Extension", "1.0.0");

    assert_eq!(meta.id, "test.extension");
    assert_eq!(meta.name, "Test Extension");
    assert_eq!(meta.version, "1.0.0");
    assert!(meta.description.is_none());
    assert!(meta.author.is_none());
}

#[test]
fn test_sdk_metadata_with_description() {
    let meta = SdkExtensionMetadata::new("test.extension", "Test", "1.0.0")
        .with_description("A test extension for unit testing");

    assert_eq!(
        meta.description,
        Some("A test extension for unit testing".to_string())
    );
}

#[test]
fn test_sdk_metadata_with_author() {
    let meta =
        SdkExtensionMetadata::new("test.extension", "Test", "1.0.0").with_author("Test Author");

    assert_eq!(meta.author, Some("Test Author".to_string()));
}

#[test]
fn test_sdk_metadata_with_type() {
    let meta = SdkExtensionMetadata::new("test.extension", "Test", "1.0.0").with_type("native");

    assert_eq!(meta.extension_type, "native");
}

#[test]
fn test_sdk_metadata_serialization() {
    let meta = SdkExtensionMetadata::new("test.extension", "Test Extension", "1.0.0")
        .with_description("Test description")
        .with_author("Test Author");

    let json = serde_json::to_string(&meta).unwrap();
    let parsed: SdkExtensionMetadata = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.id, "test.extension");
    assert_eq!(parsed.name, "Test Extension");
    assert_eq!(parsed.version, "1.0.0");
    assert_eq!(parsed.description, Some("Test description".to_string()));
    assert_eq!(parsed.author, Some("Test Author".to_string()));
}

// ============================================================================
// Metric Data Type Tests
// ============================================================================

#[test]
fn test_metric_data_type_default() {
    let dt = SdkMetricDataType::default();
    assert_eq!(dt, SdkMetricDataType::String);
}

#[test]
fn test_metric_data_type_serialization() {
    let types = vec![
        (SdkMetricDataType::Float, "float"),
        (SdkMetricDataType::Integer, "integer"),
        (SdkMetricDataType::Boolean, "boolean"),
        (SdkMetricDataType::String, "string"),
        (SdkMetricDataType::Binary, "binary"),
    ];

    for (dt, expected) in types {
        let json = serde_json::to_string(&dt).unwrap();
        assert!(json.contains(expected));
    }
}

// ============================================================================
// Metric Definition Tests
// ============================================================================

#[test]
fn test_metric_definition_creation() {
    let metric = SdkMetricDefinition::new("temperature", "Temperature", SdkMetricDataType::Float);

    assert_eq!(metric.name, "temperature");
    assert_eq!(metric.display_name, "Temperature");
    assert_eq!(metric.data_type, SdkMetricDataType::Float);
    assert!(metric.unit.is_empty());
    assert!(metric.min.is_none());
    assert!(metric.max.is_none());
    assert!(!metric.required);
}

#[test]
fn test_metric_definition_with_unit() {
    let metric =
        SdkMetricDefinition::new("temp", "Temperature", SdkMetricDataType::Float).with_unit("°C");

    assert_eq!(metric.unit, "°C");
}

#[test]
fn test_metric_definition_with_range() {
    let metric = SdkMetricDefinition::new("temp", "Temperature", SdkMetricDataType::Float)
        .with_min(-40.0)
        .with_max(100.0);

    assert_eq!(metric.min, Some(-40.0));
    assert_eq!(metric.max, Some(100.0));
}

#[test]
fn test_metric_definition_required() {
    let metric = SdkMetricDefinition::new("temp", "Temperature", SdkMetricDataType::Float)
        .with_required(true);

    assert!(metric.required);
}

// ============================================================================
// Metric Value Tests
// ============================================================================

#[test]
fn test_metric_value_from_float() {
    let value: SdkMetricValue = 42.5.into();
    assert!(matches!(value, SdkMetricValue::Float(v) if v == 42.5));
}

#[test]
fn test_metric_value_from_integer() {
    let value: SdkMetricValue = 42i64.into();
    assert!(matches!(value, SdkMetricValue::Integer(v) if v == 42));
}

#[test]
fn test_metric_value_from_bool() {
    let value: SdkMetricValue = true.into();
    assert!(matches!(value, SdkMetricValue::Boolean(v) if v));
}

#[test]
fn test_metric_value_from_string() {
    let value: SdkMetricValue = "test".to_string().into();
    assert!(matches!(value, SdkMetricValue::String(v) if v == "test"));
}

#[test]
fn test_metric_value_from_str() {
    let value: SdkMetricValue = "test".into();
    assert!(matches!(value, SdkMetricValue::String(v) if v == "test"));
}

#[test]
fn test_metric_value_from_binary() {
    let value: SdkMetricValue = vec![1u8, 2, 3].into();
    assert!(matches!(value, SdkMetricValue::Binary(v) if v == vec![1, 2, 3]));
}

#[test]
fn test_metric_value_default() {
    let value = SdkMetricValue::default();
    assert!(matches!(value, SdkMetricValue::Null));
}

#[test]
fn test_metric_value_serialization() {
    let value = SdkMetricValue::Float(42.5);
    let json = serde_json::to_string(&value).unwrap();
    assert_eq!(json, "42.5");

    let value = SdkMetricValue::Integer(42);
    let json = serde_json::to_string(&value).unwrap();
    assert_eq!(json, "42");

    let value = SdkMetricValue::Boolean(true);
    let json = serde_json::to_string(&value).unwrap();
    assert_eq!(json, "true");

    let value = SdkMetricValue::String("test".to_string());
    let json = serde_json::to_string(&value).unwrap();
    assert_eq!(json, "\"test\"");
}

// ============================================================================
// Extension Metric Value Tests
// ============================================================================

#[test]
fn test_extension_metric_value_creation() {
    let metric = SdkExtensionMetricValue::new("temperature", SdkMetricValue::Float(25.5));

    assert_eq!(metric.name, "temperature");
    assert!(matches!(metric.value, SdkMetricValue::Float(v) if v == 25.5));
    assert!(metric.timestamp > 0);
}

#[test]
fn test_extension_metric_value_with_timestamp() {
    let metric = SdkExtensionMetricValue::with_timestamp(
        "counter",
        SdkMetricValue::Integer(100),
        1234567890,
    );

    assert_eq!(metric.name, "counter");
    assert_eq!(metric.timestamp, 1234567890);
}

// ============================================================================
// Parameter Definition Tests
// ============================================================================

#[test]
fn test_parameter_definition_creation() {
    let param = SdkParameterDefinition::new("amount", SdkMetricDataType::Integer);

    assert_eq!(param.name, "amount");
    assert_eq!(param.param_type, SdkMetricDataType::Integer);
    assert!(param.required);
    assert!(param.default_value.is_none());
}

#[test]
fn test_parameter_definition_with_display_name() {
    let param = SdkParameterDefinition::new("amount", SdkMetricDataType::Integer)
        .with_display_name("Amount");

    assert_eq!(param.display_name, "Amount");
}

#[test]
fn test_parameter_definition_with_description() {
    let param = SdkParameterDefinition::new("amount", SdkMetricDataType::Integer)
        .with_description("The amount to process");

    assert_eq!(param.description, "The amount to process");
}

#[test]
fn test_parameter_definition_optional() {
    let param = SdkParameterDefinition::new("amount", SdkMetricDataType::Integer).optional();

    assert!(!param.required);
}

#[test]
fn test_parameter_definition_with_default() {
    let param = SdkParameterDefinition::new("amount", SdkMetricDataType::Integer)
        .with_default(SdkMetricValue::Integer(10));

    match &param.default_value {
        Some(SdkMetricValue::Integer(v)) => assert_eq!(*v, 10),
        _ => panic!("Expected Integer(10)"),
    }
    assert!(!param.required); // Default makes it optional
}

// ============================================================================
// Command Definition Tests
// ============================================================================

#[test]
fn test_command_definition_creation() {
    let cmd = SdkCommandDefinition::new("execute");

    assert_eq!(cmd.name, "execute");
    assert!(cmd.parameters.is_empty());
    assert!(cmd.samples.is_empty());
}

#[test]
fn test_command_definition_with_description() {
    let cmd = SdkCommandDefinition::new("execute").with_description("Execute a command");

    assert_eq!(cmd.description, "Execute a command");
}

#[test]
fn test_command_definition_with_parameters() {
    let cmd = SdkCommandDefinition::new("execute")
        .param(SdkParameterDefinition::new(
            "target",
            SdkMetricDataType::String,
        ))
        .param(SdkParameterDefinition::new(
            "timeout",
            SdkMetricDataType::Integer,
        ));

    assert_eq!(cmd.parameters.len(), 2);
}

#[test]
fn test_command_definition_default() {
    let cmd = SdkCommandDefinition::default();

    assert!(cmd.name.is_empty());
    assert!(cmd.parameters.is_empty());
}

// ============================================================================
// Error Type Tests
// ============================================================================

#[test]
fn test_extension_error_command_not_found() {
    let err = SdkExtensionError::CommandNotFound("test".to_string());
    let msg = err.to_string();
    assert!(msg.contains("Command not found"));
    assert!(msg.contains("test"));
}

#[test]
fn test_extension_error_invalid_arguments() {
    let err = SdkExtensionError::InvalidArguments("missing param".to_string());
    let msg = err.to_string();
    assert!(msg.contains("Invalid arguments"));
}

#[test]
fn test_extension_error_execution_failed() {
    let err = SdkExtensionError::ExecutionFailed("crashed".to_string());
    let msg = err.to_string();
    assert!(msg.contains("Execution failed"));
}

#[test]
fn test_extension_error_timeout() {
    let err = SdkExtensionError::Timeout("30s".to_string());
    let msg = err.to_string();
    assert!(msg.contains("Timeout"));
}

#[test]
fn test_extension_error_not_found() {
    let err = SdkExtensionError::NotFound("device".to_string());
    let msg = err.to_string();
    assert!(msg.contains("Not found"));
}

#[test]
fn test_extension_error_invalid_format() {
    let err = SdkExtensionError::InvalidFormat("JSON".to_string());
    let msg = err.to_string();
    assert!(msg.contains("Invalid format"));
}

#[test]
fn test_extension_error_load_failed() {
    let err = SdkExtensionError::LoadFailed("missing lib".to_string());
    let msg = err.to_string();
    assert!(msg.contains("Load failed"));
}

#[test]
fn test_extension_error_security_error() {
    let err = SdkExtensionError::SecurityError("unauthorized".to_string());
    let msg = err.to_string();
    assert!(msg.contains("Security error"));
}

#[test]
fn test_extension_error_not_supported() {
    let err = SdkExtensionError::NotSupported("WASM".to_string());
    let msg = err.to_string();
    assert!(msg.contains("Not supported"));
}

#[test]
fn test_extension_error_configuration_error() {
    let err = SdkExtensionError::ConfigurationError("invalid config".to_string());
    let msg = err.to_string();
    assert!(msg.contains("Configuration error"));
}

#[test]
fn test_extension_error_internal_error() {
    let err = SdkExtensionError::InternalError("panic".to_string());
    let msg = err.to_string();
    assert!(msg.contains("Internal error"));
}

#[test]
fn test_extension_error_other() {
    let err = SdkExtensionError::Other("custom error".to_string());
    let msg = err.to_string();
    assert!(msg.contains("Error"));
}

#[test]
fn test_extension_error_from_serde_json() {
    let json_err = serde_json::from_str::<serde_json::Value>("invalid json");
    let sdk_err: SdkExtensionError = json_err.unwrap_err().into();

    assert!(matches!(sdk_err, SdkExtensionError::InvalidFormat(_)));
}

// ============================================================================
// Frontend Manifest Tests
// ============================================================================

#[test]
fn test_frontend_manifest_creation() {
    let manifest = FrontendManifestBuilder::new("test.extension", "1.0.0").build();

    assert_eq!(manifest.id, "test.extension");
    assert_eq!(manifest.version, "1.0.0");
    assert_eq!(manifest.entrypoint, "index.js");
    assert!(manifest.components.is_empty());
}

#[test]
fn test_frontend_manifest_with_entrypoint() {
    let manifest = FrontendManifestBuilder::new("test.extension", "1.0.0")
        .entrypoint("main.js")
        .build();

    assert_eq!(manifest.entrypoint, "main.js");
}

#[test]
fn test_frontend_manifest_with_style_entrypoint() {
    let manifest = FrontendManifestBuilder::new("test.extension", "1.0.0")
        .style_entrypoint("styles.css")
        .build();

    assert_eq!(manifest.style_entrypoint, Some("styles.css".to_string()));
}

#[test]
fn test_frontend_manifest_with_card() {
    let manifest = FrontendManifestBuilder::new("test.extension", "1.0.0")
        .card("status-card", "Status Card")
        .build();

    assert_eq!(manifest.components.len(), 1);
    assert_eq!(manifest.components[0].name, "status-card");
    assert_eq!(
        manifest.components[0].component_type,
        FrontendComponentType::Card
    );
}

#[test]
fn test_frontend_manifest_with_widget() {
    let manifest = FrontendManifestBuilder::new("test.extension", "1.0.0")
        .widget("chart-widget", "Chart Widget")
        .build();

    assert_eq!(manifest.components.len(), 1);
    assert_eq!(
        manifest.components[0].component_type,
        FrontendComponentType::Widget
    );
}

#[test]
fn test_frontend_manifest_with_panel() {
    let manifest = FrontendManifestBuilder::new("test.extension", "1.0.0")
        .panel("settings-panel", "Settings Panel")
        .build();

    assert_eq!(manifest.components.len(), 1);
    assert_eq!(
        manifest.components[0].component_type,
        FrontendComponentType::Panel
    );
}

#[test]
fn test_frontend_manifest_with_i18n() {
    let i18n = I18nConfig {
        default_language: "en".to_string(),
        supported_languages: vec!["en".to_string(), "zh".to_string()],
        resources_path: Some("i18n".to_string()),
    };

    let manifest = FrontendManifestBuilder::new("test.extension", "1.0.0")
        .i18n(i18n)
        .build();

    assert!(manifest.i18n.is_some());
    let i18n = manifest.i18n.unwrap();
    assert_eq!(i18n.default_language, "en");
    assert_eq!(i18n.supported_languages.len(), 2);
}

#[test]
fn test_frontend_manifest_with_dependency() {
    let manifest = FrontendManifestBuilder::new("test.extension", "1.0.0")
        .dependency("react", "^18.0.0")
        .dependency("lodash", "^4.0.0")
        .build();

    assert_eq!(manifest.dependencies.len(), 2);
    assert_eq!(
        manifest.dependencies.get("react"),
        Some(&"^18.0.0".to_string())
    );
}

#[test]
fn test_frontend_component_serialization() {
    let component = FrontendComponent {
        name: "test-card".to_string(),
        component_type: FrontendComponentType::Card,
        display_name: "Test Card".to_string(),
        description: Some("A test card component".to_string()),
        icon: Some("test-icon".to_string()),
        default_size: Some(ComponentSize::new(300, 200)),
        min_size: None,
        max_size: None,
        config_schema: None,
        refreshable: true,
        refresh_interval: 5000,
    };

    let json = serde_json::to_string(&component).unwrap();
    let parsed: FrontendComponent = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.name, "test-card");
    assert_eq!(parsed.component_type, FrontendComponentType::Card);
    assert_eq!(parsed.refresh_interval, 5000);
}

#[test]
fn test_component_size() {
    let size = ComponentSize::new(400, 300);

    assert_eq!(size.width, 400);
    assert_eq!(size.height, 300);
}

// ============================================================================
// Argument Parser Tests
// ============================================================================

#[test]
fn test_arg_parser_get_string() {
    let args = json!({"name": "test", "value": 42});
    let parser = ArgParser::new(&args);

    let name = parser.get_string("name").unwrap();
    assert_eq!(name, "test");
}

#[test]
fn test_arg_parser_get_string_missing() {
    let args = json!({});
    let parser = ArgParser::new(&args);

    let result = parser.get_string("missing");
    assert!(result.is_err());
}

#[test]
fn test_arg_parser_get_optional_string() {
    let args = json!({"name": "test"});
    let parser = ArgParser::new(&args);

    let name = parser.get_optional_string("name");
    assert_eq!(name, Some("test".to_string()));

    let missing = parser.get_optional_string("missing");
    assert!(missing.is_none());
}

#[test]
fn test_arg_parser_get_i64() {
    let args = json!({"count": 42});
    let parser = ArgParser::new(&args);

    let count = parser.get_i64("count").unwrap();
    assert_eq!(count, 42);
}

#[test]
fn test_arg_parser_get_optional_i64() {
    let args = json!({"count": 42});
    let parser = ArgParser::new(&args);

    let count = parser.get_optional_i64("count");
    assert_eq!(count, Some(42));

    let missing = parser.get_optional_i64("missing");
    assert!(missing.is_none());
}

#[test]
fn test_arg_parser_get_f64() {
    let args = json!({"temperature": 25.5});
    let parser = ArgParser::new(&args);

    let temp = parser.get_f64("temperature").unwrap();
    assert_eq!(temp, 25.5);
}

#[test]
fn test_arg_parser_get_optional_f64() {
    let args = json!({"temperature": 25.5});
    let parser = ArgParser::new(&args);

    let temp = parser.get_optional_f64("temperature");
    assert_eq!(temp, Some(25.5));
}

#[test]
fn test_arg_parser_get_bool() {
    let args = json!({"enabled": true});
    let parser = ArgParser::new(&args);

    let enabled = parser.get_bool("enabled").unwrap();
    assert!(enabled);
}

#[test]
fn test_arg_parser_get_optional_bool() {
    let args = json!({"enabled": true});
    let parser = ArgParser::new(&args);

    let enabled = parser.get_optional_bool("enabled");
    assert_eq!(enabled, Some(true));
}

#[test]
fn test_arg_parser_get_object() {
    let args = json!({"config": {"key": "value"}});
    let parser = ArgParser::new(&args);

    let obj = parser.get_object("config").unwrap();
    assert_eq!(obj.get("key").unwrap().as_str().unwrap(), "value");
}

#[test]
fn test_arg_parser_get_array() {
    let args = json!({"items": [1, 2, 3]});
    let parser = ArgParser::new(&args);

    let arr = parser.get_array("items").unwrap();
    assert_eq!(arr.len(), 3);
}

#[test]
fn test_arg_parser_parse() {
    #[derive(serde::Deserialize)]
    struct TestArgs {
        name: String,
        count: i64,
    }

    let args = json!({"name": "test", "count": 42});
    let parser = ArgParser::new(&args);

    let parsed: TestArgs = parser.parse().unwrap();
    assert_eq!(parsed.name, "test");
    assert_eq!(parsed.count, 42);
}

// ============================================================================
// SDK Constants Tests
// ============================================================================

#[test]
fn test_sdk_version() {
    assert!(!SDK_VERSION.is_empty());
}

#[test]
fn test_sdk_abi_version() {
    assert_eq!(SDK_ABI_VERSION, 3);
}

#[test]
fn test_min_neomind_version() {
    assert!(!MIN_NEOMIND_VERSION.is_empty());
}

// ============================================================================
// Builder Pattern Tests
// ============================================================================

#[test]
fn test_metric_builder() {
    let metric = MetricBuilder::new("temperature", "Temperature")
        .float()
        .unit("°C")
        .min(-40.0)
        .max(100.0)
        .required()
        .build();

    assert_eq!(metric.name, "temperature");
    assert_eq!(metric.data_type, MetricDataType::Float);
    assert_eq!(metric.unit, "°C");
    assert_eq!(metric.min, Some(-40.0));
    assert_eq!(metric.max, Some(100.0));
    assert!(metric.required);
}

#[test]
fn test_metric_builder_integer() {
    let metric = MetricBuilder::new("count", "Count").integer().build();

    assert_eq!(metric.data_type, MetricDataType::Integer);
}

#[test]
fn test_metric_builder_boolean() {
    let metric = MetricBuilder::new("enabled", "Enabled").boolean().build();

    assert_eq!(metric.data_type, MetricDataType::Boolean);
}

#[test]
fn test_metric_builder_string() {
    let metric = MetricBuilder::new("name", "Name").string().build();

    assert_eq!(metric.data_type, MetricDataType::String);
}

#[test]
fn test_command_builder() {
    let cmd = CommandBuilder::new("execute")
        .display_name("Execute Command")
        .param_simple("target", "Target", MetricDataType::String)
        .param_optional("timeout", "Timeout", MetricDataType::Integer)
        .build();

    assert_eq!(cmd.name, "execute");
    assert_eq!(cmd.display_name, "Execute Command");
    assert_eq!(cmd.parameters.len(), 2);
}

#[test]
fn test_param_builder() {
    let param = ParamBuilder::new("timeout", MetricDataType::Integer)
        .display_name("Timeout")
        .description("Timeout in seconds")
        .optional()
        .min(1.0)
        .max(300.0)
        .build();

    assert_eq!(param.name, "timeout");
    assert_eq!(param.display_name, "Timeout");
    assert!(!param.required);
    assert_eq!(param.min, Some(1.0));
    assert_eq!(param.max, Some(300.0));
}
