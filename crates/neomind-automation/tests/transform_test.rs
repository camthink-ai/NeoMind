//! Tests for Transform engine
//!
//! Tests data transformation functionality.

use neomind_automation::{
    TransformEngine, TransformAutomation, TransformScope,
    TransformOperation, AggregationFunc,
};
// use serde_json::json; // Not used in tests

#[test]
fn test_transform_engine_new() {
    let engine = TransformEngine::new();
    // Test that the engine can be created with an extension registry
    let _ = engine;
}

#[test]
fn test_transform_automation_builder() {
    let transform = TransformAutomation::new(
        "test-transform",
        "Test Transform",
        TransformScope::Global,
    );

    assert_eq!(transform.metadata.id, "test-transform");
    assert_eq!(transform.metadata.name, "Test Transform");
    assert!(matches!(transform.scope, TransformScope::Global));
    assert!(transform.operations.is_none());
}

#[test]
fn test_transform_automation_with_js_code() {
    let transform = TransformAutomation::with_js_code(
        "test-transform",
        "Count Items",
        TransformScope::Global,
        "Count the items in the detections array",
        "return input.detections ? input.detections.length : 0;",
    );

    assert_eq!(transform.metadata.id, "test-transform");
    assert_eq!(transform.intent, Some("Count the items in the detections array".to_string()));
    assert_eq!(transform.js_code, Some("return input.detections ? input.detections.length : 0;".to_string()));
}

#[test]
fn test_transform_automation_builder_methods() {
    let transform = TransformAutomation::new(
        "test",
        "Test",
        TransformScope::DeviceType("sensor".to_string()),
    )
    .with_description("A test transform")
    .with_device_type("actuator")
    .with_output_prefix("custom_prefix")
    .with_complexity(4);

    assert_eq!(transform.metadata.description, "A test transform");
    assert!(matches!(transform.scope, TransformScope::DeviceType(t) if t == "actuator"));
    assert_eq!(transform.output_prefix, "custom_prefix");
    assert_eq!(transform.complexity, 4);
}

#[test]
fn test_transform_automation_with_operations() {
    let transform = TransformAutomation::new(
        "multi",
        "Multi Operation",
        TransformScope::Global,
    )
    .with_operation(TransformOperation::Single {
        json_path: "$.status".to_string(),
        output_metric: "status".to_string(),
    })
    .with_operation(TransformOperation::ArrayAggregation {
        json_path: "$.sensors".to_string(),
        aggregation: AggregationFunc::Mean,
        value_path: Some("temp".to_string()),
        output_metric: "avg_temp".to_string(),
    });

    assert!(transform.operations.is_some());
    assert_eq!(transform.operations.as_ref().unwrap().len(), 2);
}

#[test]
fn test_transform_scope() {
    let global = TransformAutomation::new(
        "global",
        "Global Transform",
        TransformScope::Global,
    );

    let by_type = TransformAutomation::new(
        "by-type",
        "By Device Type",
        TransformScope::DeviceType("sensor".to_string()),
    );

    let by_device = TransformAutomation::new(
        "by-device",
        "By Device",
        TransformScope::Device("sensor-1".to_string()),
    );

    // Verify scope is stored correctly
    assert!(matches!(global.scope, TransformScope::Global));
    assert!(matches!(by_type.scope, TransformScope::DeviceType(t) if t == "sensor"));
    assert!(matches!(by_device.scope, TransformScope::Device(d) if d == "sensor-1"));
}

#[test]
fn test_transform_serialization() {
    let transform = TransformAutomation::with_js_code(
        "t1",
        "JS Transform",
        TransformScope::DeviceType("sensor".to_string()),
        "Extract temperature",
        "return input.temp;",
    );

    let serialized = serde_json::to_string(&transform).unwrap();
    let deserialized: TransformAutomation = serde_json::from_str(&serialized).unwrap();

    assert_eq!(deserialized.metadata.id, "t1");
    assert_eq!(deserialized.scope, transform.scope);
    assert_eq!(deserialized.js_code, transform.js_code);
}

#[test]
fn test_all_aggregation_funcs() {
    let funcs = vec![
        AggregationFunc::Sum,
        AggregationFunc::Mean,
        AggregationFunc::Min,
        AggregationFunc::Max,
        AggregationFunc::Count,
        AggregationFunc::Median,
        AggregationFunc::StdDev,
        AggregationFunc::First,
        AggregationFunc::Last,
        AggregationFunc::Trend,
        AggregationFunc::Delta,
        AggregationFunc::Rate,
    ];

    for func in funcs {
        let serialized = serde_json::to_string(&func).unwrap();
        let deserialized: AggregationFunc = serde_json::from_str(&serialized).unwrap();
        assert_eq!(serialized, serde_json::to_string(&deserialized).unwrap());
    }
}
