//! Comprehensive Unit Tests for Built-in Capability Providers
//!
//! Tests cover:
//! - Mock capability providers for testing
//! - Capability invocation patterns
//! - Error handling for capabilities
//! - Capability manifest structure

#![allow(dead_code)]

use async_trait::async_trait;
use neomind_core::eventbus::EventBus;
use neomind_core::extension::context::{
    CapabilityError, CapabilityManifest, ExtensionCapability, ExtensionCapabilityProvider,
};
use serde_json::json;
use std::sync::Arc;

// ============================================================================
// Mock Capability Providers for Testing
// ============================================================================

/// Mock device capability provider for testing
struct MockDeviceCapabilityProvider;

#[async_trait]
impl ExtensionCapabilityProvider for MockDeviceCapabilityProvider {
    fn capability_manifest(&self) -> CapabilityManifest {
        CapabilityManifest {
            capabilities: vec![
                ExtensionCapability::DeviceMetricsRead,
                ExtensionCapability::DeviceControl,
            ],
            api_version: "v1".to_string(),
            min_core_version: "0.5.0".to_string(),
            package_name: "mock-device-provider".to_string(),
        }
    }

    async fn invoke_capability(
        &self,
        capability: ExtensionCapability,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, CapabilityError> {
        match capability {
            ExtensionCapability::DeviceMetricsRead => Ok(json!({
                "device_id": params["device_id"],
                "metrics": {
                    "cpu": 45.2,
                    "memory": 1024,
                    "temperature": 65.0
                }
            })),
            ExtensionCapability::DeviceControl => {
                if params["action"].is_null() {
                    return Err(CapabilityError::InvalidParameters(
                        "Missing action".to_string(),
                    ));
                }
                Ok(json!({"success": true, "action": params["action"]}))
            }
            _ => Err(CapabilityError::NotAvailable(capability)),
        }
    }
}

/// Mock event capability provider for testing
struct MockEventCapabilityProvider {
    event_bus: Arc<EventBus>,
}

impl MockEventCapabilityProvider {
    fn new(event_bus: Arc<EventBus>) -> Self {
        Self { event_bus }
    }
}

#[async_trait]
impl ExtensionCapabilityProvider for MockEventCapabilityProvider {
    fn capability_manifest(&self) -> CapabilityManifest {
        CapabilityManifest {
            capabilities: vec![
                ExtensionCapability::EventPublish,
                ExtensionCapability::EventSubscribe,
            ],
            api_version: "v1".to_string(),
            min_core_version: "0.5.0".to_string(),
            package_name: "mock-event-provider".to_string(),
        }
    }

    async fn invoke_capability(
        &self,
        capability: ExtensionCapability,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, CapabilityError> {
        match capability {
            ExtensionCapability::EventPublish => {
                let topic = params["topic"].as_str().unwrap_or("default");
                Ok(json!({"published": true, "topic": topic}))
            }
            ExtensionCapability::EventSubscribe => {
                let topic = params["topic"].as_str().unwrap_or("default");
                Ok(json!({"subscribed": true, "topic": topic}))
            }
            _ => Err(CapabilityError::NotAvailable(capability)),
        }
    }
}

/// Mock telemetry capability provider for testing
struct MockTelemetryCapabilityProvider;

#[async_trait]
impl ExtensionCapabilityProvider for MockTelemetryCapabilityProvider {
    fn capability_manifest(&self) -> CapabilityManifest {
        CapabilityManifest {
            capabilities: vec![
                ExtensionCapability::TelemetryHistory,
                ExtensionCapability::StorageQuery,
            ],
            api_version: "v1".to_string(),
            min_core_version: "0.5.0".to_string(),
            package_name: "mock-telemetry-provider".to_string(),
        }
    }

    async fn invoke_capability(
        &self,
        capability: ExtensionCapability,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, CapabilityError> {
        match capability {
            ExtensionCapability::TelemetryHistory => {
                let device_id = params["device_id"].as_str().unwrap_or("unknown");
                Ok(json!({
                    "device_id": device_id,
                    "history": [
                        {"timestamp": 1000, "value": 25.5},
                        {"timestamp": 2000, "value": 26.0},
                    ]
                }))
            }
            ExtensionCapability::StorageQuery => Ok(json!({"results": [], "count": 0})),
            _ => Err(CapabilityError::NotAvailable(capability)),
        }
    }
}

/// Mock agent capability provider for testing
struct MockAgentCapabilityProvider;

#[async_trait]
impl ExtensionCapabilityProvider for MockAgentCapabilityProvider {
    fn capability_manifest(&self) -> CapabilityManifest {
        CapabilityManifest {
            capabilities: vec![ExtensionCapability::AgentTrigger],
            api_version: "v1".to_string(),
            min_core_version: "0.5.0".to_string(),
            package_name: "mock-agent-provider".to_string(),
        }
    }

    async fn invoke_capability(
        &self,
        capability: ExtensionCapability,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, CapabilityError> {
        match capability {
            ExtensionCapability::AgentTrigger => {
                let agent_id = params["agent_id"].as_str().unwrap_or("default");
                let prompt = params["prompt"].as_str().unwrap_or("");
                Ok(json!({
                    "agent_id": agent_id,
                    "response": format!("Processed: {}", prompt),
                    "success": true
                }))
            }
            _ => Err(CapabilityError::NotAvailable(capability)),
        }
    }
}

/// Mock rule capability provider for testing
struct MockRuleCapabilityProvider;

#[async_trait]
impl ExtensionCapabilityProvider for MockRuleCapabilityProvider {
    fn capability_manifest(&self) -> CapabilityManifest {
        CapabilityManifest {
            capabilities: vec![ExtensionCapability::RuleTrigger],
            api_version: "v1".to_string(),
            min_core_version: "0.5.0".to_string(),
            package_name: "mock-rule-provider".to_string(),
        }
    }

    async fn invoke_capability(
        &self,
        capability: ExtensionCapability,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, CapabilityError> {
        match capability {
            ExtensionCapability::RuleTrigger => {
                if params["action"].as_str() == Some("list") {
                    return Ok(json!({"rules": ["rule-1", "rule-2"]}));
                }
                let rule_id = params["rule_id"].as_str().ok_or_else(|| {
                    CapabilityError::InvalidParameters("Missing rule_id".to_string())
                })?;
                Ok(json!({
                    "triggered": true,
                    "rule_id": rule_id,
                    "context": params["context"]
                }))
            }
            _ => Err(CapabilityError::NotAvailable(capability)),
        }
    }
}

/// Mock extension call capability provider for testing
struct MockExtensionCallCapabilityProvider;

#[async_trait]
impl ExtensionCapabilityProvider for MockExtensionCallCapabilityProvider {
    fn capability_manifest(&self) -> CapabilityManifest {
        CapabilityManifest {
            capabilities: vec![ExtensionCapability::ExtensionCall],
            api_version: "v1".to_string(),
            min_core_version: "0.5.0".to_string(),
            package_name: "mock-extension-call-provider".to_string(),
        }
    }

    async fn invoke_capability(
        &self,
        capability: ExtensionCapability,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, CapabilityError> {
        match capability {
            ExtensionCapability::ExtensionCall => {
                let extension_id = params["extension_id"].as_str().ok_or_else(|| {
                    CapabilityError::InvalidParameters("Missing extension_id".to_string())
                })?;
                let command = params["command"].as_str().ok_or_else(|| {
                    CapabilityError::InvalidParameters("Missing command".to_string())
                })?;
                Ok(json!({
                    "success": true,
                    "extension_id": extension_id,
                    "command": command,
                    "result": {}
                }))
            }
            _ => Err(CapabilityError::NotAvailable(capability)),
        }
    }
}

// ============================================================================
// DeviceCapabilityProvider Tests
// ============================================================================

#[tokio::test]
#[serial_test::serial]
async fn test_device_provider_manifest() {
    let provider = MockDeviceCapabilityProvider;
    let manifest = provider.capability_manifest();

    assert_eq!(manifest.capabilities.len(), 2);
    assert!(manifest
        .capabilities
        .contains(&ExtensionCapability::DeviceMetricsRead));
    assert!(manifest
        .capabilities
        .contains(&ExtensionCapability::DeviceControl));
}

#[tokio::test]
#[serial_test::serial]
async fn test_device_metrics_read() {
    let provider = MockDeviceCapabilityProvider;

    let result = provider
        .invoke_capability(
            ExtensionCapability::DeviceMetricsRead,
            &json!({"device_id": "device-1"}),
        )
        .await;

    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value["device_id"], "device-1");
    assert!(value["metrics"]["cpu"].is_number());
}

#[tokio::test]
#[serial_test::serial]
async fn test_device_control() {
    let provider = MockDeviceCapabilityProvider;

    let result = provider
        .invoke_capability(
            ExtensionCapability::DeviceControl,
            &json!({"action": "restart"}),
        )
        .await;

    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value["success"], true);
    assert_eq!(value["action"], "restart");
}

#[tokio::test]
#[serial_test::serial]
async fn test_device_control_missing_action() {
    let provider = MockDeviceCapabilityProvider;

    let result = provider
        .invoke_capability(ExtensionCapability::DeviceControl, &json!({}))
        .await;

    assert!(result.is_err());
}

// ============================================================================
// EventCapabilityProvider Tests
// ============================================================================

#[tokio::test]
#[serial_test::serial]
async fn test_event_provider_manifest() {
    let event_bus = Arc::new(EventBus::new());
    let provider = MockEventCapabilityProvider::new(event_bus);
    let manifest = provider.capability_manifest();

    assert_eq!(manifest.capabilities.len(), 2);
    assert!(manifest
        .capabilities
        .contains(&ExtensionCapability::EventPublish));
    assert!(manifest
        .capabilities
        .contains(&ExtensionCapability::EventSubscribe));
}

#[tokio::test]
#[serial_test::serial]
async fn test_event_publish() {
    let event_bus = Arc::new(EventBus::new());
    let provider = MockEventCapabilityProvider::new(event_bus);

    let result = provider
        .invoke_capability(
            ExtensionCapability::EventPublish,
            &json!({"topic": "alerts", "message": "test"}),
        )
        .await;

    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value["published"], true);
    assert_eq!(value["topic"], "alerts");
}

#[tokio::test]
#[serial_test::serial]
async fn test_event_subscribe() {
    let event_bus = Arc::new(EventBus::new());
    let provider = MockEventCapabilityProvider::new(event_bus);

    let result = provider
        .invoke_capability(
            ExtensionCapability::EventSubscribe,
            &json!({"topic": "alerts"}),
        )
        .await;

    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value["subscribed"], true);
}

// ============================================================================
// TelemetryCapabilityProvider Tests
// ============================================================================

#[tokio::test]
#[serial_test::serial]
async fn test_telemetry_provider_manifest() {
    let provider = MockTelemetryCapabilityProvider;
    let manifest = provider.capability_manifest();

    assert_eq!(manifest.capabilities.len(), 2);
    assert!(manifest
        .capabilities
        .contains(&ExtensionCapability::TelemetryHistory));
    assert!(manifest
        .capabilities
        .contains(&ExtensionCapability::StorageQuery));
}

#[tokio::test]
#[serial_test::serial]
async fn test_telemetry_history() {
    let provider = MockTelemetryCapabilityProvider;

    let result = provider
        .invoke_capability(
            ExtensionCapability::TelemetryHistory,
            &json!({"device_id": "sensor-1", "hours": 24}),
        )
        .await;

    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value["device_id"], "sensor-1");
    assert!(value["history"].is_array());
}

#[tokio::test]
#[serial_test::serial]
async fn test_storage_query() {
    let provider = MockTelemetryCapabilityProvider;

    let result = provider
        .invoke_capability(
            ExtensionCapability::StorageQuery,
            &json!({"query": "temperature > 30"}),
        )
        .await;

    assert!(result.is_ok());
    let value = result.unwrap();
    assert!(value["results"].is_array());
}

// ============================================================================
// AgentCapabilityProvider Tests
// ============================================================================

#[tokio::test]
#[serial_test::serial]
async fn test_agent_provider_manifest() {
    let provider = MockAgentCapabilityProvider;
    let manifest = provider.capability_manifest();

    assert_eq!(manifest.capabilities.len(), 1);
    assert!(manifest
        .capabilities
        .contains(&ExtensionCapability::AgentTrigger));
}

#[tokio::test]
#[serial_test::serial]
async fn test_agent_trigger() {
    let provider = MockAgentCapabilityProvider;

    let result = provider
        .invoke_capability(
            ExtensionCapability::AgentTrigger,
            &json!({
                "agent_id": "assistant",
                "prompt": "Hello, world!"
            }),
        )
        .await;

    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value["agent_id"], "assistant");
    assert!(value["success"].as_bool().unwrap());
}

// ============================================================================
// RuleCapabilityProvider Tests
// ============================================================================

#[tokio::test]
#[serial_test::serial]
async fn test_rule_provider_manifest() {
    let provider = MockRuleCapabilityProvider;
    let manifest = provider.capability_manifest();

    assert_eq!(manifest.capabilities.len(), 1);
    assert!(manifest
        .capabilities
        .contains(&ExtensionCapability::RuleTrigger));
}

#[tokio::test]
#[serial_test::serial]
async fn test_rule_trigger() {
    let provider = MockRuleCapabilityProvider;

    let result = provider
        .invoke_capability(
            ExtensionCapability::RuleTrigger,
            &json!({
                "rule_id": "alert-threshold",
                "context": {"value": 100}
            }),
        )
        .await;

    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value["triggered"], true);
    assert_eq!(value["rule_id"], "alert-threshold");
}

#[tokio::test]
#[serial_test::serial]
async fn test_rule_trigger_missing_rule_id() {
    let provider = MockRuleCapabilityProvider;

    let result = provider
        .invoke_capability(
            ExtensionCapability::RuleTrigger,
            &json!({"context": {"value": 100}}),
        )
        .await;

    assert!(result.is_err());
}

#[tokio::test]
#[serial_test::serial]
async fn test_rule_list() {
    let provider = MockRuleCapabilityProvider;

    let result = provider
        .invoke_capability(ExtensionCapability::RuleTrigger, &json!({"action": "list"}))
        .await;

    assert!(result.is_ok());
    let value = result.unwrap();
    assert!(value["rules"].is_array());
}

// ============================================================================
// ExtensionCallCapabilityProvider Tests
// ============================================================================

#[tokio::test]
#[serial_test::serial]
async fn test_extension_call_provider_manifest() {
    let provider = MockExtensionCallCapabilityProvider;
    let manifest = provider.capability_manifest();

    assert_eq!(manifest.capabilities.len(), 1);
    assert!(manifest
        .capabilities
        .contains(&ExtensionCapability::ExtensionCall));
}

#[tokio::test]
#[serial_test::serial]
async fn test_extension_call() {
    let provider = MockExtensionCallCapabilityProvider;

    let result = provider
        .invoke_capability(
            ExtensionCapability::ExtensionCall,
            &json!({
                "extension_id": "weather-extension",
                "command": "get_forecast",
                "args": {"city": "Beijing"},
            }),
        )
        .await;

    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value["success"], true);
    assert_eq!(value["extension_id"], "weather-extension");
    assert_eq!(value["command"], "get_forecast");
}

#[tokio::test]
#[serial_test::serial]
async fn test_extension_call_missing_params() {
    let provider = MockExtensionCallCapabilityProvider;

    // Missing extension_id
    let result = provider
        .invoke_capability(
            ExtensionCapability::ExtensionCall,
            &json!({"command": "test"}),
        )
        .await;
    assert!(result.is_err());

    // Missing command
    let result = provider
        .invoke_capability(
            ExtensionCapability::ExtensionCall,
            &json!({"extension_id": "test"}),
        )
        .await;
    assert!(result.is_err());
}

// ============================================================================
// Capability Error Tests
// ============================================================================

#[tokio::test]
#[serial_test::serial]
async fn test_capability_not_available() {
    let provider = MockDeviceCapabilityProvider;

    let result = provider
        .invoke_capability(
            ExtensionCapability::TelemetryHistory, // Not supported by DeviceCapabilityProvider
            &json!({}),
        )
        .await;

    assert!(result.is_err());
    match result {
        Err(CapabilityError::NotAvailable(cap)) => {
            assert_eq!(cap, ExtensionCapability::TelemetryHistory);
        }
        _ => panic!("Expected NotAvailable error"),
    }
}

// ============================================================================
// Capability Manifest Tests
// ============================================================================

#[test]
fn test_capability_manifest_structure() {
    let manifest = CapabilityManifest {
        capabilities: vec![
            ExtensionCapability::DeviceMetricsRead,
            ExtensionCapability::DeviceControl,
        ],
        api_version: "v1".to_string(),
        min_core_version: "0.5.0".to_string(),
        package_name: "test-provider".to_string(),
    };

    assert_eq!(manifest.capabilities.len(), 2);
    assert_eq!(manifest.api_version, "v1");
    assert_eq!(manifest.min_core_version, "0.5.0");
    assert_eq!(manifest.package_name, "test-provider");
}

// ============================================================================
// Capability Error Tests
// ============================================================================

#[test]
fn test_capability_error_display() {
    let err = CapabilityError::NotAvailable(ExtensionCapability::DeviceMetricsRead);
    let msg = err.to_string();
    assert!(msg.contains("not available"));

    let err = CapabilityError::InvalidParameters("test".to_string());
    let msg = err.to_string();
    assert!(msg.contains("Invalid parameters"));

    let err = CapabilityError::ProviderError("test".to_string());
    let msg = err.to_string();
    assert!(msg.contains("Provider error"));
}

// ============================================================================
// ExtensionCapability Tests
// ============================================================================

#[test]
fn test_extension_capability_equality() {
    assert_eq!(
        ExtensionCapability::DeviceMetricsRead,
        ExtensionCapability::DeviceMetricsRead
    );
    assert_ne!(
        ExtensionCapability::DeviceMetricsRead,
        ExtensionCapability::DeviceControl
    );
}

#[test]
fn test_extension_capability_clone() {
    let cap = ExtensionCapability::DeviceMetricsRead;
    let cloned = cap.clone();
    assert_eq!(cap, cloned);
}

#[test]
fn test_extension_capability_debug() {
    let cap = ExtensionCapability::DeviceMetricsRead;
    let debug = format!("{:?}", cap);
    assert!(!debug.is_empty());
}

#[test]
fn test_extension_capability_name() {
    assert_eq!(
        ExtensionCapability::DeviceMetricsRead.name(),
        "device_metrics_read"
    );
    assert_eq!(ExtensionCapability::EventPublish.name(), "event_publish");
    assert_eq!(ExtensionCapability::AgentTrigger.name(), "agent_trigger");
}

#[test]
fn test_extension_capability_custom() {
    let cap = ExtensionCapability::Custom("my_custom_capability".to_string());
    assert!(cap.is_custom());
    assert_eq!(cap.name(), "my_custom_capability");
}
