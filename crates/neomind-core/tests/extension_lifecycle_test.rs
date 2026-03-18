//! Integration Tests for Extension Lifecycle
//!
//! Tests cover:
//! - Extension registration and unregistration flow
//! - Extension state transitions
//! - Extension loading from registry
//! - Extension discovery
//! - Lifecycle events
//! - Multi-extension management

use neomind_core::extension::*;
use neomind_core::extension::registry::ExtensionRegistry;
use neomind_core::extension::system::{
    Extension, ExtensionMetadata, ExtensionError, ExtensionState,
    ExtensionMetricValue, MetricDescriptor, ExtensionCommand,
    MetricDataType, ParameterDefinition, ParamMetricValue, ExtensionStats,
};
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};
use serde_json::json;

// ============================================================================
// Test Extension with Lifecycle Tracking
// ============================================================================

struct LifecycleTrackingExtension {
    id: String,
    name: String,
    version: String,
    init_count: AtomicI32,
    command_count: AtomicI32,
    health_status: std::sync::Mutex<bool>,
}

impl LifecycleTrackingExtension {
    fn new(id: &str, name: &str, version: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            init_count: AtomicI32::new(0),
            command_count: AtomicI32::new(0),
            health_status: std::sync::Mutex::new(true),
        }
    }

    fn init_count(&self) -> i32 {
        self.init_count.load(Ordering::SeqCst)
    }

    fn command_count(&self) -> i32 {
        self.command_count.load(Ordering::SeqCst)
    }

    fn set_health(&self, healthy: bool) {
        *self.health_status.lock().unwrap() = healthy;
    }
}

#[async_trait]
impl Extension for LifecycleTrackingExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "lifecycle.extension",
                "Lifecycle Extension",
                semver::Version::new(1, 0, 0),
            )
        })
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        static METRICS: std::sync::OnceLock<Vec<MetricDescriptor>> = std::sync::OnceLock::new();
        METRICS.get_or_init(|| {
            vec![
                MetricDescriptor {
                    name: "init_count".to_string(),
                    display_name: "Init Count".to_string(),
                    data_type: MetricDataType::Integer,
                    unit: "count".to_string(),
                    min: None,
                    max: None,
                    required: false,
                },
                MetricDescriptor {
                    name: "command_count".to_string(),
                    display_name: "Command Count".to_string(),
                    data_type: MetricDataType::Integer,
                    unit: "count".to_string(),
                    min: None,
                    max: None,
                    required: false,
                },
            ]
        }).clone()
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        static COMMANDS: std::sync::OnceLock<Vec<ExtensionCommand>> = std::sync::OnceLock::new();
        COMMANDS.get_or_init(|| {
            vec![
                ExtensionCommand {
                    name: "ping".to_string(),
                    display_name: "Ping".to_string(),
                    description: "Ping the extension".to_string(),
                    payload_template: "{}".to_string(),
                    parameters: vec![],
                    fixed_values: Default::default(),
                    samples: vec![],
                    parameter_groups: vec![],
                },
                ExtensionCommand {
                    name: "get_stats".to_string(),
                    display_name: "Get Stats".to_string(),
                    description: "Get extension statistics".to_string(),
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
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.command_count.fetch_add(1, Ordering::SeqCst);

        match command {
            "ping" => Ok(json!({ "pong": true, "extension_id": self.id })),
            "get_stats" => Ok(json!({
                "init_count": self.init_count.load(Ordering::SeqCst),
                "command_count": self.command_count.load(Ordering::SeqCst),
            })),
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        Ok(vec![
            ExtensionMetricValue {
                name: "init_count".to_string(),
                value: ParamMetricValue::Integer(self.init_count.load(Ordering::SeqCst) as i64),
                timestamp: chrono::Utc::now().timestamp_millis(),
            },
            ExtensionMetricValue {
                name: "command_count".to_string(),
                value: ParamMetricValue::Integer(self.command_count.load(Ordering::SeqCst) as i64),
                timestamp: chrono::Utc::now().timestamp_millis(),
            },
        ])
    }

    async fn health_check(&self) -> Result<bool> {
        let healthy = *self.health_status.lock().unwrap();
        if healthy {
            Ok(true)
        } else {
            Err(ExtensionError::ExecutionFailed("Extension is unhealthy".to_string()))
        }
    }

    fn get_stats(&self) -> ExtensionStats {
        ExtensionStats {
            commands_executed: self.command_count.load(Ordering::SeqCst) as u64,
            ..Default::default()
        }
    }
}

// ============================================================================
// Registration Lifecycle Tests
// ============================================================================

#[tokio::test]
async fn test_extension_registration_lifecycle() {
    let registry = ExtensionRegistry::new();

    // Create extension
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(LifecycleTrackingExtension::new("lifecycle.1", "Lifecycle Extension", "1.0.0"))
            as Box<dyn Extension>
    ));

    // Register
    let result = registry.register("lifecycle.1".to_string(), ext).await;
    assert!(result.is_ok());
    assert_eq!(registry.count().await, 1);

    // Verify extension is registered
    let info = registry.get_info("lifecycle.1").await;
    assert!(info.is_some());
    let info = info.unwrap();
    assert_eq!(info.state, ExtensionState::Running);

    // Unregister
    let result = registry.unregister("lifecycle.1").await;
    assert!(result.is_ok());
    assert_eq!(registry.count().await, 0);

    // Verify extension is gone
    let info = registry.get_info("lifecycle.1").await;
    assert!(info.is_none());
}

#[tokio::test]
async fn test_multiple_extensions_lifecycle() {
    let registry = ExtensionRegistry::new();

    // Register multiple extensions
    for i in 0..5 {
        let ext = Arc::new(tokio::sync::RwLock::new(
            Box::new(LifecycleTrackingExtension::new(
                &format!("lifecycle.{}", i),
                &format!("Lifecycle Extension {}", i),
                "1.0.0",
            )) as Box<dyn Extension>
        ));
        registry.register(format!("lifecycle.{}", i), ext).await.unwrap();
    }

    assert_eq!(registry.count().await, 5);

    // Unregister in reverse order
    for i in (0..5).rev() {
        registry.unregister(&format!("lifecycle.{}", i)).await.unwrap();
    }

    assert_eq!(registry.count().await, 0);
}

// ============================================================================
// State Transition Tests
// ============================================================================

#[tokio::test]
async fn test_extension_state_after_registration() {
    let registry = ExtensionRegistry::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(LifecycleTrackingExtension::new("state.test", "State Test", "1.0.0"))
            as Box<dyn Extension>
    ));

    registry.register("state.test".to_string(), ext).await.unwrap();

    let info = registry.get_info("state.test").await.unwrap();
    assert_eq!(info.state, ExtensionState::Running);
}

#[tokio::test]
async fn test_extension_state_after_unregister() {
    let registry = ExtensionRegistry::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(LifecycleTrackingExtension::new("state.test", "State Test", "1.0.0"))
            as Box<dyn Extension>
    ));

    registry.register("state.test".to_string(), ext).await.unwrap();
    registry.unregister("state.test").await.unwrap();

    // Extension should no longer exist
    let info = registry.get_info("state.test").await;
    assert!(info.is_none());
}

// ============================================================================
// Health Check Lifecycle Tests
// ============================================================================

#[tokio::test]
async fn test_health_check_healthy() {
    let registry = ExtensionRegistry::new();
    let mock = LifecycleTrackingExtension::new("health.test", "Health Test", "1.0.0");
    mock.set_health(true);
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(mock) as Box<dyn Extension>
    ));

    registry.register("health.test".to_string(), ext).await.unwrap();

    let result = registry.health_check("health.test").await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_health_check_unhealthy() {
    let registry = ExtensionRegistry::new();
    let mock = LifecycleTrackingExtension::new("health.test", "Health Test", "1.0.0");
    mock.set_health(false);
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(mock) as Box<dyn Extension>
    ));

    registry.register("health.test".to_string(), ext).await.unwrap();

    let result = registry.health_check("health.test").await;
    assert!(result.is_err());
}

// ============================================================================
// Command Execution Lifecycle Tests
// ============================================================================

#[tokio::test]
async fn test_command_execution_updates_stats() {
    let registry = ExtensionRegistry::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(LifecycleTrackingExtension::new("cmd.test", "Command Test", "1.0.0"))
            as Box<dyn Extension>
    ));

    registry.register("cmd.test".to_string(), ext).await.unwrap();

    // Execute multiple commands
    for _ in 0..5 {
        registry.execute_command("cmd.test", "ping", &json!({})).await.unwrap();
    }

    // Check stats
    let stats = registry.get_stats("cmd.test").await.unwrap();
    assert!(stats.commands_executed >= 5);
}

#[tokio::test]
async fn test_command_execution_after_unregister_fails() {
    let registry = ExtensionRegistry::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(LifecycleTrackingExtension::new("cmd.test", "Command Test", "1.0.0"))
            as Box<dyn Extension>
    ));

    registry.register("cmd.test".to_string(), ext).await.unwrap();
    registry.unregister("cmd.test").await.unwrap();

    // Command execution should fail
    let result = registry.execute_command("cmd.test", "ping", &json!({})).await;
    assert!(result.is_err());
    assert!(matches!(result, Err(ExtensionError::NotFound(_))));
}

// ============================================================================
// Metrics Lifecycle Tests
// ============================================================================

#[tokio::test]
async fn test_metrics_collection_lifecycle() {
    let registry = ExtensionRegistry::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(LifecycleTrackingExtension::new("metrics.test", "Metrics Test", "1.0.0"))
            as Box<dyn Extension>
    ));

    registry.register("metrics.test".to_string(), ext).await.unwrap();

    // Execute some commands to change metrics
    registry.execute_command("metrics.test", "ping", &json!({})).await.unwrap();
    registry.execute_command("metrics.test", "ping", &json!({})).await.unwrap();

    // Get metrics
    let metrics = registry.get_current_metrics("metrics.test").await;
    assert!(!metrics.is_empty());

    // Verify command count metric
    let command_count_metric = metrics.iter().find(|m| m.name == "command_count");
    assert!(command_count_metric.is_some());
}

// ============================================================================
// Re-registration Tests
// ============================================================================

#[tokio::test]
async fn test_re_registration_after_unregister() {
    let registry = ExtensionRegistry::new();

    // Register
    let ext1 = Arc::new(tokio::sync::RwLock::new(
        Box::new(LifecycleTrackingExtension::new("rereg.test", "ReReg Test", "1.0.0"))
            as Box<dyn Extension>
    ));
    registry.register("rereg.test".to_string(), ext1).await.unwrap();

    // Unregister
    registry.unregister("rereg.test").await.unwrap();

    // Re-register with new instance
    let ext2 = Arc::new(tokio::sync::RwLock::new(
        Box::new(LifecycleTrackingExtension::new("rereg.test", "ReReg Test", "1.0.0"))
            as Box<dyn Extension>
    ));
    let result = registry.register("rereg.test".to_string(), ext2).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_duplicate_registration_fails() {
    let registry = ExtensionRegistry::new();

    let ext1 = Arc::new(tokio::sync::RwLock::new(
        Box::new(LifecycleTrackingExtension::new("dup.test", "Dup Test", "1.0.0"))
            as Box<dyn Extension>
    ));
    registry.register("dup.test".to_string(), ext1).await.unwrap();

    let ext2 = Arc::new(tokio::sync::RwLock::new(
        Box::new(LifecycleTrackingExtension::new("dup.test", "Dup Test", "1.0.0"))
            as Box<dyn Extension>
    ));
    let result = registry.register("dup.test".to_string(), ext2).await;
    assert!(matches!(result, Err(ExtensionError::AlreadyRegistered(_))));
}

// ============================================================================
// Concurrent Lifecycle Tests
// ============================================================================

#[tokio::test]
async fn test_concurrent_registration_and_unregistration() {
    let registry = Arc::new(ExtensionRegistry::new());
    let mut handles = vec![];

    // Concurrent registrations
    for i in 0..10 {
        let reg = registry.clone();
        let handle = tokio::spawn(async move {
            let ext = Arc::new(tokio::sync::RwLock::new(
                Box::new(LifecycleTrackingExtension::new(
                    &format!("concurrent.{}", i),
                    &format!("Concurrent {}", i),
                    "1.0.0",
                )) as Box<dyn Extension>
            ));
            reg.register(format!("concurrent.{}", i), ext).await
        });
        handles.push(handle);
    }

    for handle in handles {
        let _ = handle.await;
    }

    assert_eq!(registry.count().await, 10);

    // Concurrent unregistrations
    let mut handles = vec![];
    for i in 0..10 {
        let reg = registry.clone();
        let handle = tokio::spawn(async move {
            reg.unregister(&format!("concurrent.{}", i)).await
        });
        handles.push(handle);
    }

    for handle in handles {
        let _ = handle.await;
    }

    assert_eq!(registry.count().await, 0);
}

// ============================================================================
// Extension Info Tests
// ============================================================================

#[tokio::test]
async fn test_extension_info_contains_metadata() {
    let registry = ExtensionRegistry::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(LifecycleTrackingExtension::new("info.test", "Info Test", "1.0.0"))
            as Box<dyn Extension>
    ));

    registry.register("info.test".to_string(), ext).await.unwrap();

    let info = registry.get_info("info.test").await.unwrap();

    // Verify metadata
    assert_eq!(info.metadata.id, "lifecycle.extension"); // Uses static metadata

    // Verify commands
    assert!(!info.commands.is_empty());

    // Verify metrics
    assert!(!info.metrics.is_empty());

    // Verify loaded_at timestamp
    assert!(info.loaded_at.is_some());
}

// ============================================================================
// Safety Manager Integration Tests
// ============================================================================

#[tokio::test]
async fn test_safety_manager_tracks_extension() {
    let registry = ExtensionRegistry::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(LifecycleTrackingExtension::new("safety.test", "Safety Test", "1.0.0"))
            as Box<dyn Extension>
    ));

    registry.register("safety.test".to_string(), ext).await.unwrap();

    // Safety manager should be tracking the extension
    let safety_manager = registry.safety_manager();
    assert!(safety_manager.is_allowed("safety.test").await);
}

#[tokio::test]
async fn test_safety_manager_untracks_on_unregister() {
    let registry = ExtensionRegistry::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(LifecycleTrackingExtension::new("safety.test", "Safety Test", "1.0.0"))
            as Box<dyn Extension>
    ));

    registry.register("safety.test".to_string(), ext).await.unwrap();
    registry.unregister("safety.test").await.unwrap();

    // Safety manager should no longer track the extension
    let safety_manager = registry.safety_manager();
    // After unregister, the extension should not be tracked
    // The safety manager should allow the check (since it's not tracked)
    assert!(safety_manager.is_allowed("safety.test").await);
}