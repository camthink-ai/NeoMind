//! Integration tests for the unified data pipeline.
//!
//! Tests verify that all adapters (MQTT, HTTP, Webhook) process data
//! consistently using the UnifiedExtractor.

use neomind_devices::mdl::{MetricDataType, MetricValue};
use neomind_devices::mdl_format::MetricDefinition;
use neomind_devices::{
    registry::DeviceRegistry,
    unified_extractor::{ExtractionConfig, ExtractionMode, UnifiedExtractor},
};
use serde_json::json;

async fn create_test_registry_with_template() -> DeviceRegistry {
    let registry = DeviceRegistry::new();

    // Register a device type with nested metric definitions
    let template = neomind_devices::registry::DeviceTypeTemplate {
        device_type: "test_sensor".to_string(),
        name: "Test Sensor".to_string(),
        description: "A test sensor with nested data".to_string(),
        categories: vec!["sensor".to_string()],
        mode: neomind_devices::registry::DeviceTypeMode::Simple,
        metrics: vec![
            MetricDefinition {
                name: "values.battery".to_string(),
                display_name: "Battery".to_string(),
                data_type: MetricDataType::Integer,
                unit: "%".to_string(),
                min: Some(0.0),
                max: Some(100.0),
                required: false,
            },
            MetricDefinition {
                name: "values.temp".to_string(),
                display_name: "Temperature".to_string(),
                data_type: MetricDataType::Float,
                unit: "°C".to_string(),
                min: Some(-40.0),
                max: Some(100.0),
                required: false,
            },
            MetricDefinition {
                name: "ts".to_string(),
                display_name: "Timestamp".to_string(),
                data_type: MetricDataType::Integer,
                unit: String::new(),
                min: None,
                max: None,
                required: false,
            },
        ],
        commands: vec![],
        uplink_samples: vec![],
    };

    registry.register_template(template).await.unwrap();
    registry
}

#[test]
fn test_unified_extractor_dot_notation() {
    let registry = std::sync::Arc::new(
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(create_test_registry_with_template()),
    );
    let extractor = UnifiedExtractor::new(registry);

    let data = json!({
        "values": {
            "battery": 85,
            "temp": 23.5,
            "humidity": 60
        },
        "ts": 1234567890_i64
    });

    // Test dot notation extraction
    assert_eq!(
        extractor
            .extract_by_path(&data, "values.battery", 0)
            .unwrap(),
        Some(json!(85))
    );
    assert_eq!(
        extractor.extract_by_path(&data, "values.temp", 0).unwrap(),
        Some(json!(23.5))
    );
    assert_eq!(
        extractor.extract_by_path(&data, "ts", 0).unwrap(),
        Some(json!(1234567890))
    );

    // Non-existent path returns None (not an error)
    assert_eq!(
        extractor
            .extract_by_path(&data, "missing.field", 0)
            .unwrap(),
        None
    );
}

#[test]
fn test_unified_extractor_array_notation() {
    let registry = std::sync::Arc::new(
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(create_test_registry_with_template()),
    );
    let extractor = UnifiedExtractor::new(registry);

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
        extractor
            .extract_by_path(&data, "sensors[1].value", 0)
            .unwrap(),
        Some(json!(24.1))
    );
}

#[test]
fn test_unified_extractor_max_depth() {
    let config = ExtractionConfig {
        store_raw: false,
        auto_extract: false,
        max_depth: 2,
        include_arrays: false,
    };
    let registry = std::sync::Arc::new(
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(create_test_registry_with_template()),
    );
    let extractor = UnifiedExtractor::with_config(registry, config);

    let data = json!({
        "a": {
            "b": {
                "c": "too deep"
            }
        }
    });

    assert!(extractor.extract_by_path(&data, "a.b.c", 0).is_err());
}

#[tokio::test]
async fn test_unified_extractor_template_driven() {
    let registry = std::sync::Arc::new(create_test_registry_with_template().await);
    let extractor = UnifiedExtractor::new(registry);

    let data = json!({
        "values": {
            "battery": 85,
            "temp": 23.5,
            "humidity": 60
        },
        "ts": 1234567890_i64
    });

    let result = extractor.extract("device1", "test_sensor", &data).await;

    assert_eq!(result.mode, ExtractionMode::TemplateDriven);
    assert!(result.raw_stored);

    // Should extract: _raw + values.battery + values.temp + ts
    // humidity is NOT in template, so it should NOT be extracted
    let metric_names: Vec<&str> = result.metrics.iter().map(|m| m.name.as_str()).collect();
    assert!(metric_names.contains(&"_raw"));
    assert!(metric_names.contains(&"values.battery"));
    assert!(metric_names.contains(&"values.temp"));
    assert!(metric_names.contains(&"ts"));
    assert!(!metric_names.contains(&"values.humidity")); // Not in template
    assert!(!metric_names.contains(&"values")); // Not a valid metric name
}

#[tokio::test]
async fn test_unified_extractor_auto_extract() {
    let registry = std::sync::Arc::new(DeviceRegistry::new());
    let extractor = UnifiedExtractor::new(registry);

    let data = json!({
        "battery": 85,
        "temp": 23.5,
        "ts": 1234567890_i64
    });

    let result = extractor.extract("device1", "unknown_type", &data).await;

    assert_eq!(result.mode, ExtractionMode::AutoExtract);
    assert!(result.raw_stored);

    // Should extract: _raw + all top-level fields
    assert_eq!(result.metrics.len(), 4); // _raw + battery + temp + ts
}

#[tokio::test]
async fn test_unified_extractor_raw_only() {
    let config = ExtractionConfig {
        store_raw: true,
        auto_extract: false,
        max_depth: 10,
        include_arrays: false,
    };
    let registry = std::sync::Arc::new(DeviceRegistry::new());
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

#[test]
fn test_unified_extractor_value_conversion() {
    let registry = std::sync::Arc::new(
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(create_test_registry_with_template()),
    );
    let extractor = UnifiedExtractor::new(registry);

    // Integer
    assert!(matches!(
        extractor.value_to_metric_value(&json!(42)),
        MetricValue::Integer(42)
    ));

    // Float
    assert!(matches!(
        extractor.value_to_metric_value(&json!(23.5)),
        MetricValue::Float(23.5)
    ));

    // String
    assert!(matches!(
        extractor.value_to_metric_value(&json!("hello")),
        MetricValue::String(_)
    ));

    // Boolean
    assert!(matches!(
        extractor.value_to_metric_value(&json!(true)),
        MetricValue::Boolean(true)
    ));

    // Null
    assert!(matches!(
        extractor.value_to_metric_value(&json!(null)),
        MetricValue::Null
    ));

    // Array
    assert!(matches!(
        extractor.value_to_metric_value(&json!([1, 2, 3])),
        MetricValue::String(_)
    ));

    // Object
    assert!(matches!(
        extractor.value_to_metric_value(&json!({"key": "value"})),
        MetricValue::String(_)
    ));
}

#[test]
fn test_unified_path_extraction_edge_cases() {
    let registry = std::sync::Arc::new(
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(create_test_registry_with_template()),
    );
    let extractor = UnifiedExtractor::new(registry);

    // Empty path
    assert_eq!(
        extractor.extract_by_path(&json!({"a": 1}), "", 0).unwrap(),
        None
    );

    // Root notation
    assert_eq!(
        extractor.extract_by_path(&json!({"a": 1}), "$", 0).unwrap(),
        Some(json!({"a": 1}))
    );

    // Trailing dot
    assert_eq!(
        extractor
            .extract_by_path(&json!({"a": 1}), "a.", 0)
            .unwrap(),
        None
    );

    // Array out of bounds
    assert!(extractor
        .extract_by_path(&json!({"arr": [1, 2]}), "arr[5]", 0)
        .is_ok()); // Returns Ok(None), not an error
}

#[tokio::test]
async fn test_unified_extractor_with_real_device_payload() {
    // Simulate a real device payload like the one from the logs
    let registry = std::sync::Arc::new(create_test_registry_with_template().await);
    let extractor = UnifiedExtractor::new(registry);

    let real_payload = json!({
        "ts": 1703145600000_i64,
        "values": {
            "battery": 85,
            "devMac": "AA:BB:CC:DD:EE:FF",
            "signal": -65
        }
    });

    let result = extractor
        .extract("sensor001", "test_sensor", &real_payload)
        .await;

    assert_eq!(result.mode, ExtractionMode::TemplateDriven);
    assert!(result.raw_stored);

    // Verify extracted metrics
    let metrics: std::collections::HashMap<String, MetricValue> = result
        .metrics
        .into_iter()
        .map(|m| (m.name, m.value))
        .collect();

    // _raw should always be present
    assert!(metrics.contains_key("_raw"));

    // values.battery should be extracted (in template)
    assert!(metrics.contains_key("values.battery"));

    // ts should be extracted (in template)
    assert!(metrics.contains_key("ts"));

    // devMac and signal are NOT in template, so they should NOT be extracted
    assert!(!metrics.contains_key("values.devMac"));
    assert!(!metrics.contains_key("values.signal"));
}
