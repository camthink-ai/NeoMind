//! Integration tests for the extension capability system.
//!
//! These tests verify the complete integration of:
//! - ExtensionContext
//! - ExtensionCapabilityProvider
//! - Capability registration and invocation
//! - Capability checking and listing

use neomind_core::extension::context::{
    AvailableCapabilities, CapabilityError, CapabilityManifest, ExtensionCapability,
    ExtensionCapabilityProvider, ExtensionContext, ExtensionContextConfig,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

// ============================================================================
// Mock Capability Provider for Testing
// ============================================================================

struct MockDeviceProvider {
    device_data: Arc<RwLock<HashMap<String, HashMap<String, Value>>>>,
}

impl MockDeviceProvider {
    fn new() -> Self {
        Self {
            device_data: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn init_with_devices(&self) {
        let mut data = self.device_data.write().await;

        let mut metrics1 = HashMap::new();
        metrics1.insert("temperature".to_string(), json!(22.5));
        metrics1.insert("humidity".to_string(), json!(45.0));
        data.insert("device-1".to_string(), metrics1);

        let mut metrics2 = HashMap::new();
        metrics2.insert("temperature".to_string(), json!(18.3));
        metrics2.insert("humidity".to_string(), json!(60.0));
        data.insert("device-2".to_string(), metrics2);
    }
}

#[async_trait::async_trait]
impl ExtensionCapabilityProvider for MockDeviceProvider {
    fn capability_manifest(&self) -> CapabilityManifest {
        CapabilityManifest {
            capabilities: vec![
                ExtensionCapability::DeviceMetricsRead,
                ExtensionCapability::DeviceMetricsWrite,
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
        params: &Value,
    ) -> Result<Value, CapabilityError> {
        match capability {
            ExtensionCapability::DeviceMetricsRead => {
                let device_id = params
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CapabilityError::InvalidParameters("device_id required".to_string())
                    })?;

                let data = self.device_data.read().await;
                data.get(device_id)
                    .map(|metrics| json!(metrics))
                    .ok_or_else(|| {
                        CapabilityError::ProviderError(format!("Device {} not found", device_id))
                    })
            }

            ExtensionCapability::DeviceMetricsWrite => {
                let device_id = params
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CapabilityError::InvalidParameters("device_id required".to_string())
                    })?;

                let metric = params
                    .get("metric")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CapabilityError::InvalidParameters("metric required".to_string())
                    })?;

                let value = params.get("value").cloned().ok_or_else(|| {
                    CapabilityError::InvalidParameters("value required".to_string())
                })?;

                let value_clone = value.clone();

                // Store metric (in real implementation, would write to storage)
                {
                    let mut data = self.device_data.write().await;
                    let device_metrics = data
                        .entry(device_id.to_string())
                        .or_insert_with(HashMap::new);
                    device_metrics.insert(metric.to_string(), value);
                }

                Ok(json!({
                    "device_id": device_id,
                    "metric": metric,
                    "value": value_clone,
                    "written": true,
                }))
            }

            ExtensionCapability::DeviceControl => {
                let device_id = params
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CapabilityError::InvalidParameters("device_id required".to_string())
                    })?;

                let command = params
                    .get("command")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CapabilityError::InvalidParameters("command required".to_string())
                    })?;

                Ok(json!({
                    "device_id": device_id,
                    "command": command,
                    "status": "executed",
                }))
            }

            _ => Err(CapabilityError::ProviderError(format!(
                "Capability {:?} not implemented",
                capability
            ))),
        }
    }
}

// ============================================================================
// Mock Event Provider for Testing
// ============================================================================

struct MockEventProvider {
    published_events: std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
}

impl MockEventProvider {
    fn new() -> Self {
        Self {
            published_events: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    fn get_published_events(&self) -> Vec<Value> {
        self.published_events.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl ExtensionCapabilityProvider for MockEventProvider {
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
        params: &Value,
    ) -> Result<Value, CapabilityError> {
        match capability {
            ExtensionCapability::EventPublish => {
                let event_type = params
                    .get("event_type")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CapabilityError::InvalidParameters("event_type required".to_string())
                    })?;

                let data = params.get("data").cloned().unwrap_or(json!({}));

                // Store event (in real implementation, would publish to event bus)
                let event = json!({
                    "event_type": event_type,
                    "data": data,
                    "timestamp": chrono::Utc::now().timestamp_millis(),
                });

                self.published_events.lock().unwrap().push(event);

                Ok(json!({
                    "event_type": event_type,
                    "published": true,
                }))
            }

            ExtensionCapability::EventSubscribe => {
                let event_type = params
                    .get("event_type")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CapabilityError::InvalidParameters("event_type required".to_string())
                    })?;

                Ok(json!({
                    "event_type": event_type,
                    "subscribed": true,
                }))
            }

            _ => Err(CapabilityError::ProviderError(format!(
                "Capability {:?} not implemented",
                capability
            ))),
        }
    }
}

// ============================================================================
// Integration Tests
// ============================================================================

#[tokio::test]
async fn test_provider_registration() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig {
        extension_id: "test-extension".to_string(),
        ..Default::default()
    };
    let context = ExtensionContext::new(config, providers);
    let provider = Arc::new(MockDeviceProvider::new());
    provider.init_with_devices().await;

    // Register provider
    context
        .register_provider("mock-device-provider".to_string(), provider)
        .await;

    // Verify provider is registered
    let capabilities = context.list_capabilities().await;

    assert_eq!(capabilities.len(), 3);
    assert!(capabilities
        .iter()
        .any(|(c, _, _)| c.name() == "device_metrics_read"));
    assert!(capabilities
        .iter()
        .any(|(c, _, _)| c.name() == "device_metrics_write"));
    assert!(capabilities
        .iter()
        .any(|(c, _, _)| c.name() == "device_control"));
}

#[tokio::test]
async fn test_capability_invocation() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig {
        extension_id: "test-extension".to_string(),
        ..Default::default()
    };
    let context = ExtensionContext::new(config, providers);
    let provider = Arc::new(MockDeviceProvider::new());
    provider.init_with_devices().await;

    context
        .register_provider("mock-device-provider".to_string(), provider)
        .await;

    // Invoke DeviceMetricsRead capability
    let result = context
        .invoke_capability(
            ExtensionCapability::DeviceMetricsRead,
            &json!({"device_id": "device-1"}),
        )
        .await
        .expect("Failed to invoke capability");

    let metrics: HashMap<String, Value> = serde_json::from_value(result).unwrap();
    assert_eq!(metrics.get("temperature").unwrap(), &json!(22.5));
    assert_eq!(metrics.get("humidity").unwrap(), &json!(45.0));
}

#[tokio::test]
async fn test_capability_check() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig::default();
    let context = ExtensionContext::new(config, providers);
    let provider = Arc::new(MockDeviceProvider::new());
    provider.init_with_devices().await;

    context
        .register_provider("mock-device-provider".to_string(), provider)
        .await;

    // Check if a specific capability is available
    let is_available = context
        .has_capability(&ExtensionCapability::DeviceMetricsRead)
        .await;

    assert!(is_available);

    // Check for unavailable capability
    let is_available = context
        .has_capability(&ExtensionCapability::TelemetryHistory)
        .await;

    assert!(!is_available);
}

#[tokio::test]
async fn test_multiple_providers() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig::default();
    let context = ExtensionContext::new(config, providers);

    // Register device provider
    let device_provider = Arc::new(MockDeviceProvider::new());
    device_provider.init_with_devices().await;
    context
        .register_provider("mock-device-provider".to_string(), device_provider)
        .await;

    // Register event provider
    let event_provider = Arc::new(MockEventProvider::new());
    context
        .register_provider("mock-event-provider".to_string(), event_provider)
        .await;

    // Verify all capabilities are available
    let capabilities = context.list_capabilities().await;

    assert_eq!(capabilities.len(), 5); // 3 device + 2 event
}

#[tokio::test]
async fn test_capability_write() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig {
        extension_id: "test-extension".to_string(),
        ..Default::default()
    };
    let context = ExtensionContext::new(config, providers);
    let provider = Arc::new(MockDeviceProvider::new());
    provider.init_with_devices().await;

    context
        .register_provider("mock-device-provider".to_string(), provider)
        .await;

    // Write a new metric
    let result = context
        .invoke_capability(
            ExtensionCapability::DeviceMetricsWrite,
            &json!({
                "device_id": "device-1",
                "metric": "pressure",
                "value": 1013.25,
                "is_virtual": true,
            }),
        )
        .await
        .expect("Failed to write metric");

    assert_eq!(result.get("written").unwrap(), &json!(true));

    // Read back the metric
    let result = context
        .invoke_capability(
            ExtensionCapability::DeviceMetricsRead,
            &json!({"device_id": "device-1"}),
        )
        .await
        .expect("Failed to read metric");

    let metrics: HashMap<String, Value> = serde_json::from_value(result).unwrap();
    assert_eq!(metrics.get("pressure").unwrap(), &json!(1013.25));
}

#[tokio::test]
async fn test_capability_control() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig {
        extension_id: "test-extension".to_string(),
        ..Default::default()
    };
    let context = ExtensionContext::new(config, providers);
    let provider = Arc::new(MockDeviceProvider::new());
    provider.init_with_devices().await;

    context
        .register_provider("mock-device-provider".to_string(), provider)
        .await;

    // Send device command
    let result: Result<Value, CapabilityError> = context
        .invoke_capability(
            ExtensionCapability::DeviceControl,
            &json!({
                "device_id": "device-1",
                "command": "set_temperature",
                "params": {"target": 24.0},
            }),
        )
        .await;

    assert_eq!(result.unwrap().get("status").unwrap(), &json!("executed"));
}

#[tokio::test]
async fn test_event_publish() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig {
        extension_id: "test-extension".to_string(),
        ..Default::default()
    };
    let context = ExtensionContext::new(config, providers);
    let provider = Arc::new(MockEventProvider::new());

    context
        .register_provider("mock-event-provider".to_string(), provider.clone())
        .await;

    // Publish an event
    let result: Result<Value, CapabilityError> = context
        .invoke_capability(
            ExtensionCapability::EventPublish,
            &json!({
                "event_type": "device_metric_updated",
                "data": {
                    "device_id": "device-1",
                    "metric": "temperature",
                    "value": 23.5,
                },
            }),
        )
        .await;

    assert_eq!(result.unwrap().get("published").unwrap(), &json!(true));

    // Verify event was stored
    let events = provider.get_published_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["event_type"], "device_metric_updated");
}

#[tokio::test]
async fn test_capability_error_handling() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig {
        extension_id: "test-extension".to_string(),
        ..Default::default()
    };
    let context = ExtensionContext::new(config, providers);
    let provider = Arc::new(MockDeviceProvider::new());
    provider.init_with_devices().await;

    context
        .register_provider("mock-device-provider".to_string(), provider)
        .await;

    // Test missing required parameter
    let result = context
        .invoke_capability(
            ExtensionCapability::DeviceMetricsRead,
            &json!({}), // Missing device_id
        )
        .await;

    assert!(result.is_err());
    match result {
        Err(CapabilityError::InvalidParameters(msg)) => {
            assert!(msg.contains("device_id"));
        }
        _ => panic!("Expected InvalidParameters error"),
    }

    // Test device not found
    let result = context
        .invoke_capability(
            ExtensionCapability::DeviceMetricsRead,
            &json!({"device_id": "nonexistent-device"}),
        )
        .await;

    assert!(result.is_err());
    match result {
        Err(CapabilityError::ProviderError(msg)) => {
            assert!(msg.contains("not found"));
        }
        _ => panic!("Expected ProviderError"),
    }

    // Test unimplemented capability
    let result = context
        .invoke_capability(
            ExtensionCapability::TelemetryHistory,
            &json!({"device_id": "device-1"}),
        )
        .await;

    assert!(result.is_err());
    match result {
        Err(CapabilityError::ProviderNotFound(_)) => {
            // Expected - no provider registered for this capability
        }
        other => panic!("Expected ProviderNotFound error, got {:?}", other),
    }
}

#[tokio::test]
async fn test_capability_metadata() {
    let cap = ExtensionCapability::DeviceMetricsRead;

    assert_eq!(cap.name(), "device_metrics_read");
    assert_eq!(cap.display_name(), "Device Metrics Read");
    assert!(cap.description().contains("Read current device metrics"));
    assert_eq!(cap.category(), "device");
    assert!(!cap.is_custom());

    let custom_cap = ExtensionCapability::Custom("my_custom".to_string());
    assert_eq!(custom_cap.name(), "my_custom");
    assert_eq!(custom_cap.category(), "custom");
    assert!(custom_cap.is_custom());
}

#[tokio::test]
async fn test_all_capabilities() {
    let all_caps = ExtensionCapability::all_capabilities();

    // 15 standard + ChatStreamCancel + 4 ChatSession capabilities
    // (Open/Send/Close/CancelTurn) + LlmComplete (1.2.10 headless VLM).
    assert_eq!(all_caps.len(), 21);

    let cap_names: Vec<String> = all_caps.iter().map(|c| c.name()).collect();

    assert!(cap_names.contains(&"device_metrics_read".to_string()));
    assert!(cap_names.contains(&"device_metrics_write".to_string()));
    assert!(cap_names.contains(&"device_control".to_string()));
    assert!(cap_names.contains(&"storage_query".to_string()));
    assert!(cap_names.contains(&"telemetry_history".to_string()));
    assert!(cap_names.contains(&"metrics_aggregate".to_string()));
    assert!(cap_names.contains(&"event_publish".to_string()));
    assert!(cap_names.contains(&"event_subscribe".to_string()));
    assert!(cap_names.contains(&"extension_call".to_string()));
    assert!(cap_names.contains(&"agent_trigger".to_string()));
    assert!(cap_names.contains(&"rule_trigger".to_string()));
    assert!(cap_names.contains(&"device_template_register".to_string()));
    assert!(cap_names.contains(&"device_register".to_string()));
    assert!(cap_names.contains(&"device_unregister".to_string()));
    assert!(cap_names.contains(&"llm_complete".to_string()));
}

#[tokio::test]
async fn test_capability_from_name() {
    // Test parsing standard capabilities
    let cap = ExtensionCapability::from_name("device_metrics_read");
    assert_eq!(cap, Some(ExtensionCapability::DeviceMetricsRead));

    // Test unknown capability returns Custom
    let cap = ExtensionCapability::from_name("unknown_capability");
    match cap {
        Some(ExtensionCapability::Custom(name)) => {
            assert_eq!(name, "unknown_capability");
        }
        _ => panic!("Expected Custom capability"),
    }
}

#[tokio::test]
async fn test_capability_categories() {
    // Test device capabilities
    assert_eq!(ExtensionCapability::DeviceMetricsRead.category(), "device");
    assert_eq!(ExtensionCapability::DeviceControl.category(), "device");

    // Test event capabilities
    assert_eq!(ExtensionCapability::EventPublish.category(), "event");
    assert_eq!(ExtensionCapability::EventSubscribe.category(), "event");

    // Test agent capabilities
    assert_eq!(ExtensionCapability::AgentTrigger.category(), "agent");

    // Test rule capabilities
    assert_eq!(ExtensionCapability::RuleTrigger.category(), "rule");

    // Test storage capabilities
    assert_eq!(ExtensionCapability::StorageQuery.category(), "storage");

    // Test custom capabilities
    let custom = ExtensionCapability::Custom("test".to_string());
    assert_eq!(custom.category(), "custom");
}

#[tokio::test]
async fn test_available_capabilities_registry() {
    let mut registry = AvailableCapabilities::new();

    // Register a capability
    registry.register_capability(
        ExtensionCapability::DeviceMetricsRead,
        "device-provider".to_string(),
        "v1".to_string(),
    );

    // Check capability exists
    assert!(registry.has_capability(&ExtensionCapability::DeviceMetricsRead));
    assert!(!registry.has_capability(&ExtensionCapability::DeviceControl));

    // Get provider info
    let (provider, version) = registry
        .get_provider(&ExtensionCapability::DeviceMetricsRead)
        .expect("Failed to get provider");

    assert_eq!(provider, "device-provider");
    assert_eq!(version, "v1");
}

#[tokio::test]
async fn test_capability_manifest() {
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

#[tokio::test]
async fn test_extension_context_config_default() {
    let config = ExtensionContextConfig::default();

    assert_eq!(config.api_base_url, String::new());
    assert_eq!(config.api_version, "v1");
    assert_eq!(config.rate_limit, None);
}
