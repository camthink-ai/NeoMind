//! Extension Loader Tests
//!
//! Tests extension loading error paths and validation:
//! - File not found errors
//! - Invalid format errors
//! - ABI version mismatches
//! - Symbol resolution failures
//! - Metadata parsing

use neomind_core::extension::{
    loader::NativeExtensionLoader,
    system::{ExtensionError, ExtensionMetadata, ExtensionState, MetricDataType, MetricDefinition},
};
use std::path::PathBuf;

#[test]
fn test_loader_create() {
    let loader = NativeExtensionLoader::new();
    // Should create without panic
    let _ = loader; // Use the variable to avoid unused warning
}

#[test]
fn test_load_nonexistent_file() {
    let loader = NativeExtensionLoader::new();
    let path = PathBuf::from("/nonexistent/path/to/extension.so");

    let result = loader.load(&path);

    match result {
        Err(ExtensionError::NotFound(_)) => {
            // Expected NotFound error
        }
        Ok(_) | Err(_) => {
            panic!("Expected NotFound error, got Ok or different error");
        }
    }
}

#[test]
fn test_load_invalid_extension_format() {
    let loader = NativeExtensionLoader::new();

    // Test with various invalid extensions
    let invalid_paths = vec![
        PathBuf::from("test.txt"),  // Wrong extension
        PathBuf::from("test.json"), // Wrong extension
        PathBuf::from("test.toml"), // Wrong extension
        PathBuf::from("test.exe"),  // Not a native library
        PathBuf::from("test"),      // No extension
    ];

    for path in invalid_paths {
        if path.exists() {
            continue; // Skip if file actually exists
        }

        let result = loader.load(&path);
        // Should error (either NotFound or InvalidFormat)
        assert!(result.is_err(), "Expected error for path: {:?}", path);
    }
}

#[test]
fn test_extension_metadata_builder() {
    let meta = ExtensionMetadata::new(
        "test.extension",
        "Test Extension",
        semver::Version::new(1, 0, 0),
    );

    assert_eq!(meta.id, "test.extension");
    assert_eq!(meta.name, "Test Extension");
    assert_eq!(meta.version.major, 1);
    assert_eq!(meta.version.minor, 0);
    assert_eq!(meta.version.patch, 0);
}

#[test]
fn test_extension_metadata_with_description() {
    let meta = ExtensionMetadata::new(
        "test.extension",
        "Test Extension",
        semver::Version::new(1, 0, 0),
    )
    .with_description("A test extension for unit testing");

    assert_eq!(
        meta.description,
        Some("A test extension for unit testing".to_string())
    );
}

#[test]
fn test_extension_metadata_with_author() {
    let meta = ExtensionMetadata::new(
        "test.extension",
        "Test Extension",
        semver::Version::new(1, 0, 0),
    )
    .with_author("Test Author");

    assert_eq!(meta.author, Some("Test Author".to_string()));
}

#[test]
fn test_extension_metadata_chaining() {
    let meta = ExtensionMetadata::new(
        "test.extension",
        "Test Extension",
        semver::Version::new(2, 1, 3),
    )
    .with_description("Test description")
    .with_author("Test Author");

    assert_eq!(meta.id, "test.extension");
    assert_eq!(meta.name, "Test Extension");
    assert_eq!(meta.version, semver::Version::new(2, 1, 3));
    assert_eq!(meta.description, Some("Test description".to_string()));
    assert_eq!(meta.author, Some("Test Author".to_string()));
}

#[test]
fn test_extension_state_serialization() {
    // Test all extension states
    let states = vec![
        ExtensionState::Running,
        ExtensionState::Stopped,
        ExtensionState::Error,
    ];

    for state in states {
        let json = serde_json::to_string(&state).unwrap();
        let parsed: ExtensionState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, state);
    }
}

#[test]
fn test_extension_state_display() {
    assert_eq!(ExtensionState::Running.to_string(), "Running");
    assert_eq!(ExtensionState::Stopped.to_string(), "Stopped");
    assert_eq!(ExtensionState::Error.to_string(), "Error");
}

#[test]
fn test_metric_definition_creation() {
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
    assert!(matches!(metric.data_type, MetricDataType::Float));
}

#[test]
fn test_metric_definition_serialization_roundtrip() {
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

    assert_eq!(parsed.name, metric.name);
    assert_eq!(parsed.display_name, metric.display_name);
    assert_eq!(parsed.unit, metric.unit);
    assert_eq!(parsed.min, metric.min);
    assert_eq!(parsed.max, metric.max);
}

#[test]
fn test_metric_data_type_variants() {
    let types = vec![
        MetricDataType::Float,
        MetricDataType::Integer,
        MetricDataType::Boolean,
        MetricDataType::String,
        MetricDataType::Binary,
        MetricDataType::Enum {
            options: vec!["a".to_string(), "b".to_string()],
        },
    ];

    for data_type in types {
        let json = serde_json::to_string(&data_type).unwrap();
        let parsed: MetricDataType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, data_type);
    }
}

#[test]
fn test_extension_error_not_found() {
    let err = ExtensionError::NotFound("/path/to/extension.so".to_string());
    assert!(err.to_string().contains("Not found"));
}

#[test]
fn test_extension_error_invalid_format() {
    let err = ExtensionError::InvalidFormat("Not a valid library".to_string());
    assert!(err.to_string().contains("Invalid format"));
}

#[test]
fn test_extension_error_load_failed() {
    let err = ExtensionError::LoadFailed("Symbol missing".to_string());
    assert!(err.to_string().contains("Load failed"));
    assert!(err.to_string().contains("Symbol missing"));
}

#[test]
fn test_extension_error_symbol_not_found() {
    let err = ExtensionError::SymbolNotFound("neomind_extension_abi_version".to_string());
    assert!(err.to_string().contains("Symbol not found"));
    assert!(err.to_string().contains("neomind_extension_abi_version"));
}

#[test]
fn test_extension_error_incompatible_version() {
    let err = ExtensionError::IncompatibleVersion {
        expected: 2,
        got: 1,
    };
    let msg = err.to_string();
    assert!(msg.contains("Incompatible"));
    assert!(msg.contains("2"));
    assert!(msg.contains("1"));
}

#[test]
fn test_extension_error_command_not_found() {
    let err = ExtensionError::CommandNotFound("test_command".to_string());
    assert!(err.to_string().contains("Command not found"));
    assert!(err.to_string().contains("test_command"));
}

#[test]
fn test_extension_error_invalid_arguments() {
    let err = ExtensionError::InvalidArguments("Missing required parameter".to_string());
    assert!(err.to_string().contains("Invalid arguments"));
    assert!(err.to_string().contains("Missing required parameter"));
}

#[test]
fn test_extension_error_execution_failed() {
    let err = ExtensionError::ExecutionFailed("Connection timeout".to_string());
    assert!(err.to_string().contains("Execution failed"));
    assert!(err.to_string().contains("Connection timeout"));
}

#[test]
fn test_extension_error_timeout() {
    let err = ExtensionError::Timeout("Test timeout".to_string());
    assert!(err.to_string().contains("Timeout"));
}

#[test]
fn test_extension_metadata_version_semver() {
    // Test various semver versions
    let versions = vec![
        semver::Version::new(0, 1, 0),
        semver::Version::new(1, 0, 0),
        semver::Version::new(2, 3, 4),
        semver::Version::new(10, 20, 30),
    ];

    for version in &versions {
        let meta = ExtensionMetadata::new("test.extension", "Test", version.clone());
        assert_eq!(meta.version, *version);
    }
}

#[test]
fn test_extension_metadata_id_validation() {
    // Test various valid extension IDs
    let valid_ids = vec![
        "com.example.extension",
        "neomind.weather.forecast",
        "a.b.c",
        "test.ext",
        "my_extension_v1",
    ];

    for id in valid_ids {
        let meta = ExtensionMetadata::new(id, "Test", semver::Version::new(1, 0, 0));
        assert_eq!(meta.id, id);
    }
}

#[test]
fn test_metric_definition_optional_bounds() {
    // Test metrics with no min/max
    let metric = MetricDefinition {
        name: "unbounded".to_string(),
        display_name: "Unbounded".to_string(),
        data_type: MetricDataType::Float,
        unit: "".to_string(),
        min: None,
        max: None,
        required: false,
    };

    assert_eq!(metric.name, "unbounded");
    assert!(metric.min.is_none());
    assert!(metric.max.is_none());
    assert!(!metric.required);
}

#[test]
fn test_metric_definition_all_data_types() {
    let types = vec![
        MetricDataType::Float,
        MetricDataType::Integer,
        MetricDataType::Boolean,
        MetricDataType::String,
        MetricDataType::Binary,
    ];

    for data_type in types {
        let metric = MetricDefinition {
            name: "test_metric".to_string(),
            display_name: "Test Metric".to_string(),
            data_type: data_type.clone(),
            unit: "".to_string(),
            min: None,
            max: None,
            required: false,
        };

        assert_eq!(metric.data_type, data_type);
    }
}

#[test]
fn test_enum_metric_data_type() {
    let enum_type = MetricDataType::Enum {
        options: vec!["opt1".to_string(), "opt2".to_string(), "opt3".to_string()],
    };

    match &enum_type {
        MetricDataType::Enum { options } => {
            assert_eq!(options.len(), 3);
            assert_eq!(options[0], "opt1");
            assert_eq!(options[2], "opt3");
        }
        _ => panic!("Expected Enum type"),
    }

    // Test serialization
    let json = serde_json::to_string(&enum_type).unwrap();
    assert!(json.contains("enum"));
    assert!(json.contains("opt1"));
}

#[test]
fn test_extension_metadata_empty_optional_fields() {
    let meta = ExtensionMetadata::new("test.ext", "Test", semver::Version::new(1, 0, 0));

    // Optional fields should be None by default
    assert!(meta.description.is_none());
    assert!(meta.author.is_none());
}

#[test]
fn test_loader_multiple_instances() {
    let loader1 = NativeExtensionLoader::new();
    let loader2 = NativeExtensionLoader::new();

    // Multiple loaders should be independent
    let _ = loader1;
    let _ = loader2;
}
