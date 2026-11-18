//! Comprehensive Unit Tests for ExtensionContext
//!
//! Tests cover:
//! - Context creation and configuration
//! - Provider registration
//! - Capability invocation
//! - Capability checking
//! - Required capabilities validation
//! - Available capabilities registry

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

struct TestProvider {
    name: String,
    capabilities: Vec<ExtensionCapability>,
}

impl TestProvider {
    fn new(name: &str, capabilities: Vec<ExtensionCapability>) -> Self {
        Self {
            name: name.to_string(),
            capabilities,
        }
    }
}

#[async_trait]
impl ExtensionCapabilityProvider for TestProvider {
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
        match capability {
            ExtensionCapability::DeviceMetricsRead => {
                Ok(json!({"result": "device_metrics", "params": params}))
            }
            ExtensionCapability::EventPublish => {
                Ok(json!({"result": "event_published", "params": params}))
            }
            _ => Err(CapabilityError::NotAvailable(capability)),
        }
    }
}

// ============================================================================
// Configuration Tests
// ============================================================================

#[test]
fn test_extension_context_config_default() {
    let config = ExtensionContextConfig::default();

    assert!(config.api_base_url.is_empty());
    assert_eq!(config.api_version, "v1");
    assert!(config.rate_limit.is_none());
}

#[test]
fn test_extension_context_config_custom() {
    let config = ExtensionContextConfig {
        api_base_url: "http://localhost:8080".to_string(),
        api_version: "v2".to_string(),
        rate_limit: Some(100),
        extension_id: "test-extension".to_string(),
    };

    assert_eq!(config.api_base_url, "http://localhost:8080");
    assert_eq!(config.api_version, "v2");
    assert_eq!(config.rate_limit, Some(100));
}

// ============================================================================
// Context Creation Tests
// ============================================================================

#[tokio::test]
#[serial_test::serial]
async fn test_context_creation() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig::default();
    let context = ExtensionContext::new(config, providers);

    let capabilities = context.list_capabilities().await;
    assert!(capabilities.is_empty());
}

#[tokio::test]
#[serial_test::serial]
async fn test_context_with_config() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig {
        extension_id: "test-ext".to_string(),
        ..Default::default()
    };
    let context = ExtensionContext::new(config, providers);

    // Context should be created successfully
    let _ = context;
}

// ============================================================================
// Provider Registration Tests
// ============================================================================

#[tokio::test]
#[serial_test::serial]
async fn test_register_provider() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig::default();
    let context = ExtensionContext::new(config, providers);

    let provider = Arc::new(TestProvider::new(
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

    let provider1 = Arc::new(TestProvider::new(
        "provider-1",
        vec![ExtensionCapability::DeviceMetricsRead],
    ));
    let provider2 = Arc::new(TestProvider::new(
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

    let provider = Arc::new(TestProvider::new(
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
async fn test_invoke_capability() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig {
        ..Default::default()
    };
    let context = ExtensionContext::new(config, providers);

    let provider = Arc::new(TestProvider::new(
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
    assert_eq!(value["result"], "device_metrics");
}

#[tokio::test]
#[serial_test::serial]
async fn test_invoke_capability_not_registered() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig::default();
    let context = ExtensionContext::new(config, providers);

    let result = context
        .invoke_capability(ExtensionCapability::DeviceMetricsRead, &json!({}))
        .await;

    assert!(result.is_err());
}

#[tokio::test]
#[serial_test::serial]
async fn test_invoke_capability_not_in_required() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig {
        ..Default::default()
    };
    let context = ExtensionContext::new(config, providers);

    let provider = Arc::new(TestProvider::new(
        "test-provider",
        vec![
            ExtensionCapability::DeviceMetricsRead,
            ExtensionCapability::EventPublish,
        ],
    ));

    context
        .register_provider("test-provider".to_string(), provider)
        .await;

    // EventPublish is registered, should succeed
    let result = context
        .invoke_capability(ExtensionCapability::EventPublish, &json!({}))
        .await;

    assert!(result.is_ok());
}

// ============================================================================
// Capability Checking Tests
// ============================================================================

#[tokio::test]
#[serial_test::serial]
async fn test_has_capability() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig::default();
    let context = ExtensionContext::new(config, providers);

    let provider = Arc::new(TestProvider::new(
        "test-provider",
        vec![ExtensionCapability::DeviceMetricsRead],
    ));

    context
        .register_provider("test-provider".to_string(), provider)
        .await;

    assert!(
        context
            .has_capability(&ExtensionCapability::DeviceMetricsRead)
            .await
    );
    assert!(
        !context
            .has_capability(&ExtensionCapability::EventPublish)
            .await
    );
}

#[tokio::test]
#[serial_test::serial]
async fn test_list_capabilities() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig::default();
    let context = ExtensionContext::new(config, providers);

    let provider = Arc::new(TestProvider::new(
        "test-provider",
        vec![
            ExtensionCapability::DeviceMetricsRead,
            ExtensionCapability::EventPublish,
        ],
    ));

    context
        .register_provider("test-provider".to_string(), provider)
        .await;

    let capabilities = context.list_capabilities().await;

    assert_eq!(capabilities.len(), 2);

    let cap_names: Vec<String> = capabilities.iter().map(|(c, _, _)| c.name()).collect();

    assert!(cap_names.contains(&"device_metrics_read".to_string()));
    assert!(cap_names.contains(&"event_publish".to_string()));
}

// ============================================================================
// Available Capabilities Registry Tests
// ============================================================================

#[test]
fn test_available_capabilities_new() {
    let registry = AvailableCapabilities::new();
    assert!(!registry.has_capability(&ExtensionCapability::DeviceMetricsRead));
}

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
fn test_available_capabilities_get_provider() {
    let mut registry = AvailableCapabilities::new();

    registry.register_capability(
        ExtensionCapability::DeviceMetricsRead,
        "device-provider".to_string(),
        "v1".to_string(),
    );

    let (provider, version) = registry
        .get_provider(&ExtensionCapability::DeviceMetricsRead)
        .unwrap();

    assert_eq!(provider, "device-provider");
    assert_eq!(version, "v1");
}

#[test]
fn test_available_capabilities_get_provider_not_found() {
    let registry = AvailableCapabilities::new();

    let result = registry.get_provider(&ExtensionCapability::DeviceMetricsRead);
    assert!(result.is_none());
}

#[test]
fn test_available_capabilities_list() {
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

// ============================================================================
// ExtensionCapability Tests
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
    assert_eq!(
        ExtensionCapability::EventPublish.display_name(),
        "Event Publish"
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

    assert_eq!(
        ExtensionCapability::from_name("unknown_capability"),
        Some(ExtensionCapability::Custom(
            "unknown_capability".to_string()
        ))
    );
}

#[test]
fn test_capability_all_capabilities() {
    let all = ExtensionCapability::all_capabilities();
    assert!(!all.is_empty());

    // Verify standard capabilities are present
    let names: Vec<String> = all.iter().map(|c| c.name()).collect();
    assert!(names.contains(&"device_metrics_read".to_string()));
    assert!(names.contains(&"event_publish".to_string()));
    assert!(names.contains(&"agent_trigger".to_string()));
}

// ============================================================================
// Capability Error Tests
// ============================================================================

#[test]
fn test_capability_error_display() {
    let err = CapabilityError::InvalidParameters("test error".to_string());
    assert!(err.to_string().contains("Invalid parameters"));

    let err = CapabilityError::NotAvailable(ExtensionCapability::DeviceMetricsRead);
    assert!(err.to_string().contains("not available"));

    let err = CapabilityError::ProviderError("test".to_string());
    assert!(err.to_string().contains("Provider error"));
}

// ============================================================================
// Concurrent Access Tests
// ============================================================================

#[tokio::test]
#[serial_test::serial]
async fn test_concurrent_capability_check() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig::default();
    let context = Arc::new(ExtensionContext::new(config, providers));

    let provider = Arc::new(TestProvider::new(
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

#[tokio::test]
#[serial_test::serial]
async fn test_concurrent_invoke() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig {
        ..Default::default()
    };
    let context = Arc::new(ExtensionContext::new(config, providers));

    let provider = Arc::new(TestProvider::new(
        "test-provider",
        vec![ExtensionCapability::DeviceMetricsRead],
    ));

    context
        .register_provider("test-provider".to_string(), provider)
        .await;

    let mut handles = vec![];

    for i in 0..5 {
        let ctx = context.clone();
        let handle = tokio::spawn(async move {
            ctx.invoke_capability(ExtensionCapability::DeviceMetricsRead, &json!({"index": i}))
                .await
        });
        handles.push(handle);
    }

    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }
}
