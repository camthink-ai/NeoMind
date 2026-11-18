//! Integration Tests for Capability Invocation and Permissions
//!
//! Tests cover:
//! - Capability registration and discovery
//! - Capability invocation with providers
//! - Permission checking for capabilities
//! - Required capabilities validation
//! - Multi-provider capability routing
//! - Error handling for capability calls

use async_trait::async_trait;
use neomind_core::extension::context::{
    AvailableCapabilities, CapabilityError, CapabilityManifest, ExtensionCapability,
    ExtensionCapabilityProvider, ExtensionContext, ExtensionContextConfig,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

// ============================================================================
// Mock Provider for Testing
// ============================================================================

struct MockCapabilityProvider {
    name: String,
    capabilities: Vec<ExtensionCapability>,
    call_count: std::sync::atomic::AtomicU64,
}

impl MockCapabilityProvider {
    fn new(name: &str, capabilities: Vec<ExtensionCapability>) -> Self {
        Self {
            name: name.to_string(),
            capabilities,
            call_count: std::sync::atomic::AtomicU64::new(0),
        }
    }

    fn call_count(&self) -> u64 {
        self.call_count.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait]
impl ExtensionCapabilityProvider for MockCapabilityProvider {
    fn capability_manifest(&self) -> CapabilityManifest {
        CapabilityManifest {
            capabilities: self.capabilities.clone(),
            api_version: "v1".to_string(),
            min_core_version: "0.5.0".to_string(),
            package_name: self.name.clone(),
        }
    }

    async fn invoke_capability(
        &self,
        capability: ExtensionCapability,
        params: &Value,
    ) -> Result<Value, CapabilityError> {
        self.call_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        match capability {
            ExtensionCapability::DeviceMetricsRead => {
                let device_id = params
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CapabilityError::InvalidParameters("Missing device_id".to_string())
                    })?;

                Ok(json!({
                    "device_id": device_id,
                    "temperature": 25.5,
                    "humidity": 65.0,
                }))
            }
            ExtensionCapability::DeviceControl => {
                let device_id = params
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CapabilityError::InvalidParameters("Missing device_id".to_string())
                    })?;
                let command = params
                    .get("command")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CapabilityError::InvalidParameters("Missing command".to_string())
                    })?;

                Ok(json!({
                    "device_id": device_id,
                    "command": command,
                    "status": "executed",
                }))
            }
            ExtensionCapability::EventPublish => {
                let event_type = params
                    .get("event_type")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CapabilityError::InvalidParameters("Missing event_type".to_string())
                    })?;

                Ok(json!({
                    "event_type": event_type,
                    "published": true,
                }))
            }
            _ => Err(CapabilityError::NotAvailable(capability)),
        }
    }
}

// ============================================================================
// Capability Registration Tests
// ============================================================================

#[tokio::test]
#[serial_test::serial]
async fn test_register_single_provider() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig::default();
    let context = ExtensionContext::new(config, providers);

    let provider = Arc::new(MockCapabilityProvider::new(
        "test-provider",
        vec![ExtensionCapability::DeviceMetricsRead],
    ));

    context
        .register_provider("test-provider".to_string(), provider)
        .await;

    let capabilities = context.list_capabilities().await;
    assert_eq!(capabilities.len(), 1);
}

#[tokio::test]
#[serial_test::serial]
async fn test_register_multiple_providers() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig::default();
    let context = ExtensionContext::new(config, providers);

    let provider1 = Arc::new(MockCapabilityProvider::new(
        "provider-1",
        vec![ExtensionCapability::DeviceMetricsRead],
    ));
    let provider2 = Arc::new(MockCapabilityProvider::new(
        "provider-2",
        vec![ExtensionCapability::EventPublish],
    ));

    context
        .register_provider("provider-1".to_string(), provider1)
        .await;
    context
        .register_provider("provider-2".to_string(), provider2)
        .await;

    let capabilities = context.list_capabilities().await;
    assert_eq!(capabilities.len(), 2);
}

#[tokio::test]
#[serial_test::serial]
async fn test_register_provider_multiple_capabilities() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig::default();
    let context = ExtensionContext::new(config, providers);

    let provider = Arc::new(MockCapabilityProvider::new(
        "multi-provider",
        vec![
            ExtensionCapability::DeviceMetricsRead,
            ExtensionCapability::DeviceMetricsWrite,
            ExtensionCapability::DeviceControl,
        ],
    ));

    context
        .register_provider("multi-provider".to_string(), provider)
        .await;

    let capabilities = context.list_capabilities().await;
    assert_eq!(capabilities.len(), 3);
}

// ============================================================================
// Capability Invocation Tests
// ============================================================================

#[tokio::test]
#[serial_test::serial]
async fn test_invoke_capability_success() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig {
        ..Default::default()
    };
    let context = ExtensionContext::new(config, providers);

    let provider = Arc::new(MockCapabilityProvider::new(
        "test-provider",
        vec![ExtensionCapability::DeviceMetricsRead],
    ));

    context
        .register_provider("test-provider".to_string(), provider)
        .await;

    let result = context
        .invoke_capability(
            ExtensionCapability::DeviceMetricsRead,
            &json!({"device_id": "device-1"}),
        )
        .await;

    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value["device_id"], "device-1");
    assert!(value["temperature"].is_number());
}

#[tokio::test]
#[serial_test::serial]
async fn test_invoke_capability_multiple_times() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig {
        ..Default::default()
    };
    let context = ExtensionContext::new(config, providers);

    let provider = Arc::new(MockCapabilityProvider::new(
        "test-provider",
        vec![ExtensionCapability::DeviceMetricsRead],
    ));

    context
        .register_provider("test-provider".to_string(), provider.clone())
        .await;

    for i in 0..5 {
        let result = context
            .invoke_capability(
                ExtensionCapability::DeviceMetricsRead,
                &json!({"device_id": format!("device-{}", i)}),
            )
            .await;

        assert!(result.is_ok());
    }

    // Verify call count
    assert_eq!(provider.call_count(), 5);
}

// ============================================================================
// Permission Checking Tests
// ============================================================================

#[tokio::test]
#[serial_test::serial]
async fn test_capability_allowed_when_required() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig {
        ..Default::default()
    };
    let context = ExtensionContext::new(config, providers);

    let provider = Arc::new(MockCapabilityProvider::new(
        "test-provider",
        vec![ExtensionCapability::DeviceMetricsRead],
    ));

    context
        .register_provider("test-provider".to_string(), provider)
        .await;

    let result = context
        .invoke_capability(
            ExtensionCapability::DeviceMetricsRead,
            &json!({"device_id": "device-1"}),
        )
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
#[serial_test::serial]
async fn test_all_registered_capabilities_accessible() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig {
        ..Default::default()
    };
    let context = ExtensionContext::new(config, providers);

    let provider = Arc::new(MockCapabilityProvider::new(
        "test-provider",
        vec![
            ExtensionCapability::DeviceMetricsRead,
            ExtensionCapability::EventPublish,
        ],
    ));

    context
        .register_provider("test-provider".to_string(), provider)
        .await;

    // Both capabilities should work since provider is registered
    let result = context
        .invoke_capability(
            ExtensionCapability::DeviceMetricsRead,
            &json!({"device_id": "device-1"}),
        )
        .await;
    assert!(result.is_ok());

    let result = context
        .invoke_capability(
            ExtensionCapability::EventPublish,
            &json!({"event_type": "test"}),
        )
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
#[serial_test::serial]
async fn test_capability_denied_when_no_provider() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig {
        ..Default::default()
    };
    let context = ExtensionContext::new(config, providers);

    // No provider registered
    let result = context
        .invoke_capability(
            ExtensionCapability::DeviceMetricsRead,
            &json!({"device_id": "device-1"}),
        )
        .await;

    assert!(result.is_err());
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[tokio::test]
#[serial_test::serial]
async fn test_invalid_parameters_error() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig {
        ..Default::default()
    };
    let context = ExtensionContext::new(config, providers);

    let provider = Arc::new(MockCapabilityProvider::new(
        "test-provider",
        vec![ExtensionCapability::DeviceMetricsRead],
    ));

    context
        .register_provider("test-provider".to_string(), provider)
        .await;

    // Missing device_id
    let result = context
        .invoke_capability(ExtensionCapability::DeviceMetricsRead, &json!({}))
        .await;

    assert!(result.is_err());
    match result {
        Err(CapabilityError::InvalidParameters(msg)) => {
            assert!(msg.contains("device_id"));
        }
        _ => panic!("Expected InvalidParameters error"),
    }
}

#[tokio::test]
#[serial_test::serial]
async fn test_capability_not_available_error() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig {
        ..Default::default()
    };
    let context = ExtensionContext::new(config, providers);

    let provider = Arc::new(MockCapabilityProvider::new(
        "test-provider",
        vec![ExtensionCapability::DeviceMetricsRead],
    ));

    context
        .register_provider("test-provider".to_string(), provider)
        .await;

    // TelemetryHistory is not supported by this provider
    let result = context
        .invoke_capability(ExtensionCapability::TelemetryHistory, &json!({}))
        .await;

    assert!(result.is_err());
}

// ============================================================================
// Available Capabilities Registry Tests
// ============================================================================

#[test]
fn test_available_capabilities_register() {
    let mut registry = AvailableCapabilities::new();

    registry.register_capability(
        ExtensionCapability::DeviceMetricsRead,
        "device-provider".to_string(),
        "v1".to_string(),
    );

    assert!(registry.has_capability(&ExtensionCapability::DeviceMetricsRead));
}

#[test]
fn test_available_capabilities_multiple() {
    let mut registry = AvailableCapabilities::new();

    registry.register_capability(
        ExtensionCapability::DeviceMetricsRead,
        "provider-1".to_string(),
        "v1".to_string(),
    );
    registry.register_capability(
        ExtensionCapability::EventPublish,
        "provider-2".to_string(),
        "v1".to_string(),
    );

    assert!(registry.has_capability(&ExtensionCapability::DeviceMetricsRead));
    assert!(registry.has_capability(&ExtensionCapability::EventPublish));
}

#[test]
fn test_available_capabilities_get_provider() {
    let mut registry = AvailableCapabilities::new();

    registry.register_capability(
        ExtensionCapability::DeviceMetricsRead,
        "device-provider".to_string(),
        "v2".to_string(),
    );

    let (provider, version) = registry
        .get_provider(&ExtensionCapability::DeviceMetricsRead)
        .unwrap();

    assert_eq!(provider, "device-provider");
    assert_eq!(version, "v2");
}

// ============================================================================
// Built-in Providers Tests
// ============================================================================

#[tokio::test]
#[serial_test::serial]
async fn test_manual_provider_registration() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig::default();
    let context = ExtensionContext::new(config, providers);

    // Manually register a mock provider
    let mock_provider = Arc::new(MockProvider);
    context
        .register_provider("mock".to_string(), mock_provider)
        .await;

    // Should have capabilities registered
    let capabilities = context.list_capabilities().await;
    assert!(!capabilities.is_empty());
}

/// Mock provider for testing
struct MockProvider;

#[async_trait]
impl ExtensionCapabilityProvider for MockProvider {
    fn capability_manifest(&self) -> CapabilityManifest {
        CapabilityManifest {
            capabilities: vec![ExtensionCapability::DeviceMetricsRead],
            api_version: "v1".to_string(),
            min_core_version: "0.5.0".to_string(),
            package_name: "mock".to_string(),
        }
    }

    async fn invoke_capability(
        &self,
        _capability: ExtensionCapability,
        _params: &serde_json::Value,
    ) -> Result<serde_json::Value, CapabilityError> {
        Ok(serde_json::json!({"result": "ok"}))
    }
}

// ============================================================================
// Capability Metadata Tests
// ============================================================================

#[test]
fn test_capability_name() {
    assert_eq!(
        ExtensionCapability::DeviceMetricsRead.name(),
        "device_metrics_read"
    );
    assert_eq!(ExtensionCapability::EventPublish.name(), "event_publish");
    assert_eq!(ExtensionCapability::AgentTrigger.name(), "agent_trigger");
}

#[test]
fn test_capability_display_name() {
    assert_eq!(
        ExtensionCapability::DeviceMetricsRead.display_name(),
        "Device Metrics Read"
    );
}

#[test]
fn test_capability_description() {
    let desc = ExtensionCapability::DeviceMetricsRead.description();
    assert!(!desc.is_empty());
}

#[test]
fn test_capability_category() {
    assert_eq!(ExtensionCapability::DeviceMetricsRead.category(), "device");
    assert_eq!(ExtensionCapability::EventPublish.category(), "event");
    assert_eq!(ExtensionCapability::AgentTrigger.category(), "agent");
    assert_eq!(ExtensionCapability::RuleTrigger.category(), "rule");
}

#[test]
fn test_capability_is_custom() {
    assert!(!ExtensionCapability::DeviceMetricsRead.is_custom());

    let custom = ExtensionCapability::Custom("my_custom".to_string());
    assert!(custom.is_custom());
}

#[test]
fn test_capability_from_name() {
    assert_eq!(
        ExtensionCapability::from_name("device_metrics_read"),
        Some(ExtensionCapability::DeviceMetricsRead)
    );

    let custom = ExtensionCapability::from_name("unknown_capability");
    match custom {
        Some(ExtensionCapability::Custom(name)) => {
            assert_eq!(name, "unknown_capability");
        }
        _ => panic!("Expected Custom capability"),
    }
}

#[test]
fn test_all_capabilities() {
    let all = ExtensionCapability::all_capabilities();
    assert!(!all.is_empty());

    let names: Vec<String> = all.iter().map(|c| c.name()).collect();
    assert!(names.contains(&"device_metrics_read".to_string()));
    assert!(names.contains(&"event_publish".to_string()));
    assert!(names.contains(&"agent_trigger".to_string()));
    assert!(names.contains(&"rule_trigger".to_string()));
}

// ============================================================================
// Concurrent Capability Access Tests
// ============================================================================

#[tokio::test]
#[serial_test::serial]
async fn test_concurrent_capability_invocation() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig {
        ..Default::default()
    };
    let context = Arc::new(ExtensionContext::new(config, providers));

    let provider = Arc::new(MockCapabilityProvider::new(
        "test-provider",
        vec![ExtensionCapability::DeviceMetricsRead],
    ));

    context
        .register_provider("test-provider".to_string(), provider)
        .await;

    let mut handles = vec![];

    for i in 0..10 {
        let ctx = context.clone();
        let handle = tokio::spawn(async move {
            ctx.invoke_capability(
                ExtensionCapability::DeviceMetricsRead,
                &json!({"device_id": format!("device-{}", i)}),
            )
            .await
        });
        handles.push(handle);
    }

    let mut success_count = 0;
    for handle in handles {
        if handle.await.unwrap().is_ok() {
            success_count += 1;
        }
    }

    assert_eq!(success_count, 10);
}

#[tokio::test]
#[serial_test::serial]
async fn test_concurrent_capability_check() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig::default();
    let context = Arc::new(ExtensionContext::new(config, providers));

    let provider = Arc::new(MockCapabilityProvider::new(
        "test-provider",
        vec![ExtensionCapability::DeviceMetricsRead],
    ));

    context
        .register_provider("test-provider".to_string(), provider)
        .await;

    let mut handles = vec![];

    for _ in 0..10 {
        let ctx = context.clone();
        let handle = tokio::spawn(async move {
            ctx.has_capability(&ExtensionCapability::DeviceMetricsRead)
                .await
        });
        handles.push(handle);
    }

    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result);
    }
}
