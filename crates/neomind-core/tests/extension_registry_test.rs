//! Comprehensive Unit Tests for ExtensionRegistry
//!
//! Tests cover:
//! - Registry creation and initialization
//! - Extension registration and unregistration
//! - Extension lookup and listing
//! - Command execution with safety checks
//! - Health checking
//! - Metrics collection
//! - Event bus integration
//! - Safety manager integration

use neomind_core::extension::*;
use neomind_core::extension::registry::ExtensionRegistry;
use neomind_core::extension::system::{
    Extension, ExtensionMetadata, ExtensionError, ExtensionState,
    ExtensionMetricValue, MetricDescriptor, ExtensionCommand,
    MetricDataType, ParameterDefinition, ParamMetricValue, ExtensionStats,
};
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use serde_json::json;

// ============================================================================
// Mock Extension for Testing
// ============================================================================

/// A simple mock extension for testing registry operations
struct MockExtension {
    id: String,
    name: String,
    version: String,
    counter: AtomicI64,
    should_fail_health: std::sync::Mutex<bool>,
    should_fail_execute: std::sync::Mutex<bool>,
}

impl MockExtension {
    fn new(id: &str, name: &str, version: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            counter: AtomicI64::new(0),
            should_fail_health: std::sync::Mutex::new(false),
            should_fail_execute: std::sync::Mutex::new(false),
        }
    }

    fn set_fail_health(&self, fail: bool) {
        *self.should_fail_health.lock().unwrap() = fail;
    }

    fn set_fail_execute(&self, fail: bool) {
        *self.should_fail_execute.lock().unwrap() = fail;
    }
}

#[async_trait]
impl Extension for MockExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "mock.extension",
                "Mock Extension",
                semver::Version::new(1, 0, 0),
            )
        })
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![]
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        static COMMANDS: std::sync::OnceLock<Vec<ExtensionCommand>> = std::sync::OnceLock::new();
        COMMANDS.get_or_init(|| {
            vec![
                ExtensionCommand {
                    name: "increment".to_string(),
                    display_name: "Increment".to_string(),
                    description: "Increment the counter".to_string(),
                    payload_template: "{}".to_string(),
                    parameters: vec![
                        ParameterDefinition {
                            name: "amount".to_string(),
                            display_name: "Amount".to_string(),
                            description: "Amount to add".to_string(),
                            param_type: MetricDataType::Integer,
                            required: false,
                            default_value: Some(ParamMetricValue::Integer(1)),
                            min: None,
                            max: None,
                            options: vec![],
                        }
                    ],
                    fixed_values: Default::default(),
                    samples: vec![],
                    parameter_groups: vec![],
                },
                ExtensionCommand {
                    name: "get_value".to_string(),
                    display_name: "Get Value".to_string(),
                    description: "Get current counter value".to_string(),
                    payload_template: "{}".to_string(),
                    parameters: vec![],
                    fixed_values: Default::default(),
                    samples: vec![],
                    parameter_groups: vec![],
                },
            ]
        }).clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn execute_command(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        if *self.should_fail_execute.lock().unwrap() {
            return Err(ExtensionError::ExecutionFailed("Mock execution failure".to_string()));
        }

        match command {
            "increment" => {
                let amount = args.get("amount")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(1);
                let new_value = self.counter.fetch_add(amount, Ordering::SeqCst) + amount;
                Ok(json!({ "value": new_value }))
            }
            "get_value" => {
                let value = self.counter.load(Ordering::SeqCst);
                Ok(json!({ "value": value }))
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        Ok(vec![
            ExtensionMetricValue {
                name: "counter".to_string(),
                value: ParamMetricValue::Integer(self.counter.load(Ordering::SeqCst)),
                timestamp: chrono::Utc::now().timestamp_millis(),
            }
        ])
    }

    async fn health_check(&self) -> Result<bool> {
        if *self.should_fail_health.lock().unwrap() {
            Err(ExtensionError::ExecutionFailed("Health check failed".to_string()))
        } else {
            Ok(true)
        }
    }

    fn get_stats(&self) -> ExtensionStats {
        ExtensionStats {
            commands_executed: self.counter.load(Ordering::SeqCst) as u64,
            ..Default::default()
        }
    }
}

// ============================================================================
// Registry Creation Tests
// ============================================================================

#[tokio::test]
async fn test_registry_creation() {
    let registry = ExtensionRegistry::new();
    assert_eq!(registry.count().await, 0);
}

#[tokio::test]
async fn test_registry_default() {
    let registry = ExtensionRegistry::default();
    assert_eq!(registry.count().await, 0);
}

// ============================================================================
// Extension Registration Tests
// ============================================================================

#[tokio::test]
async fn test_register_extension() {
    let registry = ExtensionRegistry::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockExtension::new("test.ext", "Test Extension", "1.0.0"))
            as Box<dyn Extension>
    ));

    let result = registry.register("test.ext".to_string(), ext).await;
    assert!(result.is_ok());
    assert_eq!(registry.count().await, 1);
}

#[tokio::test]
async fn test_register_duplicate_extension() {
    let registry = ExtensionRegistry::new();
    let ext1 = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockExtension::new("test.ext", "Test Extension", "1.0.0"))
            as Box<dyn Extension>
    ));
    let ext2 = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockExtension::new("test.ext", "Test Extension", "1.0.0"))
            as Box<dyn Extension>
    ));

    // First registration should succeed
    let result1 = registry.register("test.ext".to_string(), ext1).await;
    assert!(result1.is_ok());

    // Second registration should fail
    let result2 = registry.register("test.ext".to_string(), ext2).await;
    assert!(matches!(result2, Err(ExtensionError::AlreadyRegistered(_))));
}

#[tokio::test]
async fn test_unregister_extension() {
    let registry = ExtensionRegistry::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockExtension::new("test.ext", "Test Extension", "1.0.0"))
            as Box<dyn Extension>
    ));

    registry.register("test.ext".to_string(), ext).await.unwrap();
    assert_eq!(registry.count().await, 1);

    let result = registry.unregister("test.ext").await;
    assert!(result.is_ok());
    assert_eq!(registry.count().await, 0);
}

#[tokio::test]
async fn test_unregister_nonexistent_extension() {
    let registry = ExtensionRegistry::new();

    // Unregistering non-existent extension should succeed (idempotent)
    let result = registry.unregister("nonexistent").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_register_multiple_extensions() {
    let registry = ExtensionRegistry::new();

    for i in 0..5 {
        let ext = Arc::new(tokio::sync::RwLock::new(
            Box::new(MockExtension::new(&format!("ext.{}", i), &format!("Extension {}", i), "1.0.0"))
                as Box<dyn Extension>
        ));
        registry.register(format!("ext.{}", i), ext).await.unwrap();
    }

    assert_eq!(registry.count().await, 5);
}

// ============================================================================
// Extension Lookup Tests
// ============================================================================

#[tokio::test]
async fn test_get_extension() {
    let registry = ExtensionRegistry::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockExtension::new("test.ext", "Test Extension", "1.0.0"))
            as Box<dyn Extension>
    ));

    registry.register("test.ext".to_string(), ext).await.unwrap();

    let retrieved = registry.get("test.ext").await;
    assert!(retrieved.is_some());
}

#[tokio::test]
async fn test_get_nonexistent_extension() {
    let registry = ExtensionRegistry::new();

    let retrieved = registry.get("nonexistent").await;
    assert!(retrieved.is_none());
}

#[tokio::test]
async fn test_get_extension_info() {
    let registry = ExtensionRegistry::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockExtension::new("test.ext", "Test Extension", "1.0.0"))
            as Box<dyn Extension>
    ));

    registry.register("test.ext".to_string(), ext).await.unwrap();

    let info = registry.get_info("test.ext").await;
    assert!(info.is_some());

    let info = info.unwrap();
    assert_eq!(info.metadata.id, "mock.extension"); // MockExtension uses static metadata
    assert_eq!(info.state, ExtensionState::Running);
    assert!(info.loaded_at.is_some());
}

#[tokio::test]
async fn test_list_extensions() {
    let registry = ExtensionRegistry::new();

    for i in 0..3 {
        let ext = Arc::new(tokio::sync::RwLock::new(
            Box::new(MockExtension::new(&format!("ext.{}", i), &format!("Extension {}", i), "1.0.0"))
                as Box<dyn Extension>
        ));
        registry.register(format!("ext.{}", i), ext).await.unwrap();
    }

    let list = registry.list().await;
    assert_eq!(list.len(), 3);
}

#[tokio::test]
async fn test_contains_extension() {
    let registry = ExtensionRegistry::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockExtension::new("test.ext", "Test Extension", "1.0.0"))
            as Box<dyn Extension>
    ));

    assert!(!registry.contains("test.ext").await);

    registry.register("test.ext".to_string(), ext).await.unwrap();

    assert!(registry.contains("test.ext").await);
}

// ============================================================================
// Command Execution Tests
// ============================================================================

#[tokio::test]
async fn test_execute_command_success() {
    let registry = ExtensionRegistry::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockExtension::new("test.ext", "Test Extension", "1.0.0"))
            as Box<dyn Extension>
    ));

    registry.register("test.ext".to_string(), ext).await.unwrap();

    let result = registry.execute_command("test.ext", "increment", &json!({"amount": 5})).await;
    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value["value"], 5);
}

#[tokio::test]
async fn test_execute_command_not_found() {
    let registry = ExtensionRegistry::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockExtension::new("test.ext", "Test Extension", "1.0.0"))
            as Box<dyn Extension>
    ));

    registry.register("test.ext".to_string(), ext).await.unwrap();

    let result = registry.execute_command("test.ext", "unknown_command", &json!({})).await;
    assert!(matches!(result, Err(ExtensionError::CommandNotFound(_))));
}

#[tokio::test]
async fn test_execute_command_extension_not_found() {
    let registry = ExtensionRegistry::new();

    let result = registry.execute_command("nonexistent", "increment", &json!({})).await;
    assert!(matches!(result, Err(ExtensionError::NotFound(_))));
}

#[tokio::test]
async fn test_execute_command_execution_failure() {
    let registry = ExtensionRegistry::new();
    let mock = MockExtension::new("test.ext", "Test Extension", "1.0.0");
    mock.set_fail_execute(true);
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(mock) as Box<dyn Extension>
    ));

    registry.register("test.ext".to_string(), ext).await.unwrap();

    let result = registry.execute_command("test.ext", "increment", &json!({})).await;
    assert!(matches!(result, Err(ExtensionError::ExecutionFailed(_))));
}

// ============================================================================
// Health Check Tests
// ============================================================================

#[tokio::test]
async fn test_health_check_success() {
    let registry = ExtensionRegistry::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockExtension::new("test.ext", "Test Extension", "1.0.0"))
            as Box<dyn Extension>
    ));

    registry.register("test.ext".to_string(), ext).await.unwrap();

    let result = registry.health_check("test.ext").await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_health_check_failure() {
    let registry = ExtensionRegistry::new();
    let mock = MockExtension::new("test.ext", "Test Extension", "1.0.0");
    mock.set_fail_health(true);
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(mock) as Box<dyn Extension>
    ));

    registry.register("test.ext".to_string(), ext).await.unwrap();

    let result = registry.health_check("test.ext").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_health_check_extension_not_found() {
    let registry = ExtensionRegistry::new();

    let result = registry.health_check("nonexistent").await;
    assert!(matches!(result, Err(ExtensionError::NotFound(_))));
}

// ============================================================================
// Metrics Tests
// ============================================================================

#[tokio::test]
async fn test_get_current_metrics() {
    let registry = ExtensionRegistry::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockExtension::new("test.ext", "Test Extension", "1.0.0"))
            as Box<dyn Extension>
    ));

    registry.register("test.ext".to_string(), ext).await.unwrap();

    // Execute a command to change the counter
    registry.execute_command("test.ext", "increment", &json!({"amount": 10})).await.unwrap();

    let metrics = registry.get_current_metrics("test.ext").await;
    assert_eq!(metrics.len(), 1);
    assert_eq!(metrics[0].name, "counter");
}

#[tokio::test]
async fn test_get_current_metrics_extension_not_found() {
    let registry = ExtensionRegistry::new();

    let metrics = registry.get_current_metrics("nonexistent").await;
    assert!(metrics.is_empty());
}

#[tokio::test]
async fn test_get_stats() {
    let registry = ExtensionRegistry::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockExtension::new("test.ext", "Test Extension", "1.0.0"))
            as Box<dyn Extension>
    ));

    registry.register("test.ext".to_string(), ext).await.unwrap();

    // Execute some commands
    registry.execute_command("test.ext", "increment", &json!({"amount": 5})).await.unwrap();
    registry.execute_command("test.ext", "increment", &json!({"amount": 3})).await.unwrap();

    let stats = registry.get_stats("test.ext").await;
    assert!(stats.is_ok());
    let stats = stats.unwrap();
    assert!(stats.commands_executed > 0);
}

// ============================================================================
// Safety Manager Integration Tests
// ============================================================================

#[tokio::test]
async fn test_safety_manager_exists() {
    let registry = ExtensionRegistry::new();
    let safety_manager = registry.safety_manager();
    assert!(Arc::strong_count(&safety_manager) >= 1);
}

// ============================================================================
// Extension Registry Trait Tests
// ============================================================================

#[tokio::test]
async fn test_trait_get_extensions() {
    use neomind_core::extension::registry::ExtensionRegistryTrait;

    let registry = ExtensionRegistry::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockExtension::new("test.ext", "Test Extension", "1.0.0"))
            as Box<dyn Extension>
    ));

    registry.register("test.ext".to_string(), ext).await.unwrap();

    let extensions = registry.get_extensions().await;
    assert_eq!(extensions.len(), 1);
}

#[tokio::test]
async fn test_trait_get_extension() {
    use neomind_core::extension::registry::ExtensionRegistryTrait;

    let registry = ExtensionRegistry::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockExtension::new("test.ext", "Test Extension", "1.0.0"))
            as Box<dyn Extension>
    ));

    registry.register("test.ext".to_string(), ext).await.unwrap();

    let retrieved = registry.get_extension("test.ext").await;
    assert!(retrieved.is_some());
}

#[tokio::test]
async fn test_trait_execute_command() {
    use neomind_core::extension::registry::ExtensionRegistryTrait;

    let registry = ExtensionRegistry::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockExtension::new("test.ext", "Test Extension", "1.0.0"))
            as Box<dyn Extension>
    ));

    registry.register("test.ext".to_string(), ext).await.unwrap();

    let result = registry.execute_command("test.ext", "increment", &json!({"amount": 7})).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_trait_get_metrics() {
    use neomind_core::extension::registry::ExtensionRegistryTrait;

    let registry = ExtensionRegistry::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockExtension::new("test.ext", "Test Extension", "1.0.0"))
            as Box<dyn Extension>
    ));

    registry.register("test.ext".to_string(), ext).await.unwrap();

    let metrics = registry.get_metrics("test.ext").await;
    assert_eq!(metrics.len(), 0); // MockExtension has empty metrics slice
}

// ============================================================================
// Concurrent Access Tests
// ============================================================================

#[tokio::test]
async fn test_concurrent_registration() {
    let registry = Arc::new(ExtensionRegistry::new());
    let mut handles = vec![];

    for i in 0..10 {
        let reg = registry.clone();
        let handle = tokio::spawn(async move {
            let ext = Arc::new(tokio::sync::RwLock::new(
                Box::new(MockExtension::new(&format!("ext.{}", i), &format!("Extension {}", i), "1.0.0"))
                    as Box<dyn Extension>
            ));
            reg.register(format!("ext.{}", i), ext).await
        });
        handles.push(handle);
    }

    for handle in handles {
        let _ = handle.await;
    }

    // All registrations should succeed
    assert_eq!(registry.count().await, 10);
}

#[tokio::test]
async fn test_concurrent_command_execution() {
    let registry = Arc::new(ExtensionRegistry::new());
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockExtension::new("test.ext", "Test Extension", "1.0.0"))
            as Box<dyn Extension>
    ));

    registry.register("test.ext".to_string(), ext).await.unwrap();

    let mut handles = vec![];

    for _ in 0..10 {
        let reg = registry.clone();
        let handle = tokio::spawn(async move {
            reg.execute_command("test.ext", "increment", &json!({"amount": 1})).await
        });
        handles.push(handle);
    }

    for handle in handles {
        let _ = handle.await;
    }

    // Verify counter was incremented 10 times
    let result = registry.execute_command("test.ext", "get_value", &json!({})).await.unwrap();
    assert_eq!(result["value"], 10);
}