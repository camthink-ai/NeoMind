//! Error Path Coverage Tests
//!
//! Comprehensive tests for error handling across various code paths:
//! - Error propagation
//! - Error conversion
//! - Context preservation
//! - Recovery scenarios

use neomind_core::extension::{
    system::{ExtensionError, MetricDataType, ParameterDefinition, ParamMetricValue},
};
use neomind_core::config::{agent, agent_env_vars};
use std::sync::Arc;
use tokio::sync::RwLock;

// ============================================================================
// Extension Error Path Tests
// ============================================================================

#[test]
fn test_error_metric_not_found() {
    let err = ExtensionError::MetricNotFound("nonexistent_metric".to_string());
    assert!(err.to_string().contains("Metric not found"));
    assert!(err.to_string().contains("nonexistent_metric"));
}

#[test]
fn test_error_chain_propagation() {
    // Test error wrapping (if any)
    let inner_err = ExtensionError::CommandNotFound("test_cmd".to_string());
    // In some error handling patterns, this might be wrapped
    // For now, just verify the error message is descriptive
    assert!(inner_err.to_string().contains("Command not found"));
}

#[test]
fn test_parameter_validation_error() {
    // Test parameter validation scenarios
    let param = ParameterDefinition {
        name: "threshold".to_string(),
        display_name: "Threshold".to_string(),
        description: "Detection threshold".to_string(),
        param_type: MetricDataType::Float,
        required: true,
        default_value: None,
        min: Some(0.0),
        max: Some(1.0),
        options: vec![],
    };

    // Verify parameter bounds are reasonable
    if let Some(min) = param.min {
        if let Some(max) = param.max {
            assert!(min <= max, "Parameter min {} should be <= max {}", min, max);
        }
    }
}

#[test]
fn test_required_parameter_without_default() {
    let param = ParameterDefinition {
        name: "required_param".to_string(),
        display_name: "Required".to_string(),
        description: "A required parameter".to_string(),
        param_type: MetricDataType::String,
        required: true,
        default_value: None,  // No default for required parameter
        min: None,
        max: None,
        options: vec![],
    };

    // This is valid - required parameters don't need defaults
    assert!(param.required);
    assert!(param.default_value.is_none());
}

#[test]
fn test_optional_parameter_with_default() {
    let param = ParameterDefinition {
        name: "optional_param".to_string(),
        display_name: "Optional".to_string(),
        description: "An optional parameter".to_string(),
        param_type: MetricDataType::Integer,
        required: false,
        default_value: Some(ParamMetricValue::Integer(42)),
        min: None,
        max: None,
        options: vec![],
    };

    // Optional parameters can have defaults
    assert!(!param.required);
    assert!(param.default_value.is_some());
}

#[test]
fn test_enum_parameter_options() {
    let param = ParameterDefinition {
        name: "mode".to_string(),
        display_name: "Mode".to_string(),
        description: "Operation mode".to_string(),
        param_type: MetricDataType::Enum {
            options: vec!["auto".to_string(), "manual".to_string(), "off".to_string()],
        },
        required: true,
        default_value: Some(ParamMetricValue::String("auto".to_string())),
        min: None,
        max: None,
        options: vec!["auto".to_string(), "manual".to_string(), "off".to_string()],
    };

    // Verify enum has options
    match param.param_type {
        MetricDataType::Enum { ref options } => {
            assert_eq!(options.len(), 3);
            assert!(options.contains(&"auto".to_string()));
        }
        _ => panic!("Expected Enum type"),
    }
}

// ============================================================================
// Config Error Path Tests
// ============================================================================

#[test]
fn test_config_default_values_valid() {
    // Test that default config values are within valid ranges
    assert!(agent::DEFAULT_MAX_CONTEXT_TOKENS >= 1);
    assert!(agent::DEFAULT_MAX_CONTEXT_TOKENS <= 200000);

    assert!(agent::DEFAULT_TEMPERATURE >= 0.0);
    assert!(agent::DEFAULT_TEMPERATURE <= 2.0);

    assert!(agent::DEFAULT_TOP_P >= 0.0);
    assert!(agent::DEFAULT_TOP_P <= 1.0);

    assert!(agent::DEFAULT_MAX_TOKENS >= 1);

    assert!(agent::DEFAULT_CONCURRENT_LIMIT >= 1);
    assert!(agent::DEFAULT_CONCURRENT_LIMIT <= 100);
}

#[test]
fn test_config_env_var_parsing_invalid() {
    // Test that invalid env var values fall back to defaults
    let orig = std::env::var(agent_env_vars::MAX_CONTEXT_TOKENS);

    unsafe { std::env::set_var(agent_env_vars::MAX_CONTEXT_TOKENS, "not_a_number"); }
    let result = agent_env_vars::max_context_tokens();
    assert_eq!(result, agent::DEFAULT_MAX_CONTEXT_TOKENS);

    match orig {
        Ok(v) => unsafe { std::env::set_var(agent_env_vars::MAX_CONTEXT_TOKENS, v); },
        Err(_) => unsafe { std::env::remove_var(agent_env_vars::MAX_CONTEXT_TOKENS); },
    }
}

#[test]
fn test_config_env_var_parsing_zero() {
    // Test edge case of zero value
    let orig = std::env::var(agent_env_vars::MAX_CONTEXT_TOKENS);

    unsafe { std::env::set_var(agent_env_vars::MAX_CONTEXT_TOKENS, "0"); }
    let result = agent_env_vars::max_context_tokens();
    // Zero is parsed successfully (though may be invalid for config validation)
    assert_eq!(result, 0);

    match orig {
        Ok(v) => unsafe { std::env::set_var(agent_env_vars::MAX_CONTEXT_TOKENS, v); },
        Err(_) => unsafe { std::env::remove_var(agent_env_vars::MAX_CONTEXT_TOKENS); },
    }
}

#[test]
fn test_config_env_var_parsing_overflow() {
    // Test very large values that might overflow
    let orig = std::env::var(agent_env_vars::MAX_TOKENS);

    // Use usize::MAX which is valid but may cause issues
    unsafe { std::env::set_var(agent_env_vars::MAX_TOKENS, "18446744073709551615"); }
    let result = agent_env_vars::max_tokens();
    // Should parse but the value may be truncated on 32-bit platforms
    assert!(result > 0);

    match orig {
        Ok(v) => unsafe { std::env::set_var(agent_env_vars::MAX_TOKENS, v); },
        Err(_) => unsafe { std::env::remove_var(agent_env_vars::MAX_TOKENS); },
    }
}

// ============================================================================
// Concurrency Error Tests
// ============================================================================

#[tokio::test]
async fn test_concurrent_state_access() {
    // Test that concurrent reads don't cause issues
    let state = Arc::new(RwLock::new(vec![1, 2, 3]));

    // Spawn multiple concurrent reads
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let state = state.clone();
            tokio::spawn(async move {
                let guard = state.read().await;
                guard.len()
            })
        })
        .collect();

    // All reads should succeed
    let results: Vec<_> = futures::future::join_all(handles).await;
    for result in results {
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 3);
    }
}

#[tokio::test]
async fn test_concurrent_write_conflict() {
    // Test that concurrent writes are serialized correctly
    let state = Arc::new(RwLock::new(0));

    let mut handles = vec![];

    // Spawn multiple concurrent writes
    for i in 0..100 {
        let state = state.clone();
        handles.push(tokio::spawn(async move {
            let mut guard = state.write().await;
            *guard += 1;
            i
        }));
    }

    // All writes should complete
    let results: Vec<_> = futures::future::join_all(handles).await;
    assert!(results.len() == 100);

    // Final value should be 100 (all increments applied)
    let final_value = *state.read().await;
    assert_eq!(final_value, 100);
}

// ============================================================================
// Panic Recovery Tests
// ============================================================================

#[test]
fn test_panic_in_std_ops() {
    // Test that standard operations don't panic on edge cases

    // String operations
    let empty = "";
    assert_eq!(empty.len(), 0);
    assert!(empty.is_empty());

    // Vec operations
    let empty_vec: Vec<i32> = vec![];
    assert_eq!(empty_vec.len(), 0);
    assert!(empty_vec.is_empty());
    assert_eq!(empty_vec.first(), None);

    // Option operations
    let none_opt: Option<i32> = None;
    assert_eq!(none_opt.unwrap_or(42), 42);

    // Result operations
    let ok_result: Result<i32, &str> = Ok(10);
    assert_eq!(ok_result.unwrap_or(20), 10);

    let err_result: Result<i32, &str> = Err("error");
    assert_eq!(err_result.unwrap_or(20), 20);
}

// ============================================================================
// Serialization Error Tests
// ============================================================================

#[test]
fn test_serialization_invalid_json() {
    // Test handling of invalid JSON
    let invalid_json = "not valid json";

    let result: Result<MetricDataType, _> = serde_json::from_str(invalid_json);
    assert!(result.is_err());
}

#[test]
fn test_serialization_empty_object() {
    // Test handling of empty JSON object
    let empty_json = "{}";

    let result: Result<MetricDataType, _> = serde_json::from_str(empty_json);
    assert!(result.is_err()); // MetricDataType requires specific fields
}

#[test]
fn test_serialization_wrong_type() {
    // Test handling of wrong type in JSON
    let wrong_type_json = "\"float\""; // This is actually valid for MetricDataType

    let result: Result<MetricDataType, _> = serde_json::from_str(wrong_type_json);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), MetricDataType::Float);
}

#[test]
fn test_serialization_malformed_enum() {
    // Test empty enum options - this fails deserialization
    let malformed_enum = "{\"enum\":[]}"; // Empty enum options

    let result: Result<MetricDataType, _> = serde_json::from_str(malformed_enum);
    assert!(result.is_err()); // Empty enum is not valid
}

// ============================================================================
// Numeric Edge Cases
// ============================================================================

#[test]
fn test_numeric_edge_cases() {
    // Test boundary values
    assert_eq!(0u32.saturating_sub(1), 0);
    assert_eq!(1u32.saturating_sub(1), 0);
    assert_eq!(u32::MAX.saturating_add(1), u32::MAX);

    // Test floating point edge cases
    assert!((0.0_f32).is_finite());
    assert!((f32::INFINITY).is_infinite());
    assert!((f32::NAN).is_nan());

    // Test that comparisons work with edge cases
    assert!(0.0 < 1.0);
    assert!((-1.0_f32) < 0.0);
}

#[test]
fn test_numeric_conversion() {
    // Test numeric type conversions
    let int_val: i32 = 42;
    let float_val = int_val as f64;
    assert_eq!(float_val, 42.0);

    let back_to_int = float_val as i32;
    assert_eq!(back_to_int, 42);

    // Test truncation
    let large_float = 42.9_f64;
    let truncated = large_float as i32;
    assert_eq!(truncated, 42); // Truncates, not rounds
}

// ============================================================================
// String Edge Cases
// ============================================================================

#[test]
fn test_string_edge_cases() {
    // Empty strings
    assert!("".is_empty());
    assert_eq!("".len(), 0);
    assert_eq!("".chars().count(), 0);

    // Unicode handling
    let unicode = "Hello 世界";
    assert_eq!(unicode.len(), 12); // bytes, not chars
    assert_eq!(unicode.chars().count(), 8); // actual characters

    // Whitespace
    assert!("   ".trim().is_empty());
    assert!("\t\n".trim().is_empty());

    // String splitting edge cases
    assert_eq!("".split(',').count(), 1); // Returns one empty string
    assert_eq!("a".split(',').count(), 1);
    assert_eq!("a,b".split(',').count(), 2);
    assert_eq!("a,b,".split(',').count(), 3); // Trailing empty string
}

// ============================================================================
// Collection Edge Cases
// ============================================================================

#[test]
fn test_collection_edge_cases() {
    // Vec operations
    let vec = vec![1, 2, 3];
    assert_eq!(vec.get(0), Some(&1));
    assert_eq!(vec.get(10), None); // Out of bounds
    assert_eq!(vec.get(100), None);

    // HashMap operations
    use std::collections::HashMap;
    let mut map = HashMap::new();
    map.insert("key", "value");

    assert_eq!(map.get("key"), Some(&"value"));
    assert_eq!(map.get("nonexistent"), None);

    // Remove non-existent key
    assert_eq!(map.remove("nonexistent"), None);
}

// ============================================================================
// Clone and Copy Edge Cases
// ============================================================================

#[test]
fn test_clone_edge_cases() {
    // Arc clone behavior
    let arc = Arc::new(42);
    let arc_clone = arc.clone();
    assert_eq!(*arc, *arc_clone);
    assert!(Arc::strong_count(&arc) == 2);

    // String clone
    let s = "hello".to_string();
    let s_clone = s.clone();
    assert_eq!(s, s_clone);

    // Vec clone
    let v = vec![1, 2, 3];
    let v_clone = v.clone();
    assert_eq!(v, v_clone);
}
