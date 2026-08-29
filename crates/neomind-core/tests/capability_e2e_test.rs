//! End-to-End Capability Invocation Tests
//!
//! These tests verify the complete capability invocation flow:
//! 1. Extension requests a capability
//! 2. Context routes to appropriate provider
//! 3. Provider executes and returns result
//! 4. Result is returned to extension
//!
//! Tests cover both in-process and IPC-isolated modes.

use neomind_core::eventbus::EventBus;
use neomind_core::extension::context::{
    CapabilityError, CapabilityManifest, ExtensionCapability, ExtensionCapabilityProvider,
    ExtensionContext, ExtensionContextConfig,
};
use neomind_core::extension::isolated::{IsolatedExtensionManager, IsolatedManagerConfig};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use tokio::sync::RwLock;

// ============================================================================
// Mock Capability Provider
// ============================================================================

struct TestCapabilityProvider {
    name: String,
}

impl TestCapabilityProvider {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl ExtensionCapabilityProvider for TestCapabilityProvider {
    fn capability_manifest(&self) -> CapabilityManifest {
        CapabilityManifest {
            capabilities: vec![
                ExtensionCapability::DeviceMetricsRead,
                ExtensionCapability::DeviceMetricsWrite,
                ExtensionCapability::EventPublish,
                ExtensionCapability::EventSubscribe,
            ],
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
                let device_id = params
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default");

                Ok(json!({
                    "device_id": device_id,
                    "metrics": {
                        "temperature": 25.5,
                        "humidity": 65.0,
                    },
                    "timestamp": chrono::Utc::now().timestamp_millis(),
                }))
            }
            ExtensionCapability::DeviceMetricsWrite => {
                let device_id = params
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default");
                let metric = params
                    .get("metric")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let value = params.get("value").cloned().unwrap_or(json!(0));

                Ok(json!({
                    "success": true,
                    "device_id": device_id,
                    "metric": metric,
                    "value": value,
                    "written_at": chrono::Utc::now().timestamp_millis(),
                }))
            }
            ExtensionCapability::EventPublish => {
                let event_type = params
                    .get("event_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("custom");

                Ok(json!({
                    "success": true,
                    "event_type": event_type,
                    "published_at": chrono::Utc::now().timestamp_millis(),
                }))
            }
            ExtensionCapability::EventSubscribe => {
                let event_types = params
                    .get("event_types")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                    .unwrap_or_default();

                Ok(json!({
                    "success": true,
                    "subscription_id": uuid::Uuid::new_v4().to_string(),
                    "event_types": event_types,
                }))
            }
            _ => Err(CapabilityError::NotAvailable(capability)),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[tokio::test]
async fn test_capability_invocation_through_context() {
    // Create provider
    let provider = Arc::new(TestCapabilityProvider::new("test-provider"));

    // Create shared providers map
    let providers = Arc::new(RwLock::new(HashMap::new()));

    // Create context with provider
    let config = ExtensionContextConfig {
        extension_id: "test-extension".to_string(),
        ..Default::default()
    };

    let context = ExtensionContext::new(config, providers.clone());

    // Register provider
    context
        .register_provider("test-provider".to_string(), provider)
        .await;

    // Test DeviceMetricsRead
    let result = context
        .invoke_capability(
            ExtensionCapability::DeviceMetricsRead,
            &json!({ "device_id": "sensor-001" }),
        )
        .await
        .unwrap();

    assert!(result.get("device_id").unwrap().as_str().unwrap() == "sensor-001");
    assert!(result.get("metrics").is_some());

    // Test DeviceMetricsWrite
    let result = context
        .invoke_capability(
            ExtensionCapability::DeviceMetricsWrite,
            &json!({
                "device_id": "sensor-001",
                "metric": "temperature",
                "value": 28.5,
            }),
        )
        .await
        .unwrap();

    assert!(result.get("success").unwrap().as_bool().unwrap());

    // Test EventPublish
    let result = context
        .invoke_capability(
            ExtensionCapability::EventPublish,
            &json!({ "event_type": "temperature_alert" }),
        )
        .await
        .unwrap();

    assert!(result.get("success").unwrap().as_bool().unwrap());
}

#[tokio::test]
async fn test_capability_permission_check() {
    // Create provider
    let provider = Arc::new(TestCapabilityProvider::new("test-provider"));

    // Create shared providers map
    let providers = Arc::new(RwLock::new(HashMap::new()));

    // Create context with limited permissions
    let config = ExtensionContextConfig {
        extension_id: "limited-extension".to_string(),
        ..Default::default()
    };

    let context = ExtensionContext::new(config, providers.clone());

    // Register provider
    context
        .register_provider("test-provider".to_string(), provider)
        .await;

    // Should succeed - provider is registered
    let result = context
        .invoke_capability(
            ExtensionCapability::DeviceMetricsRead,
            &json!({ "device_id": "sensor-001" }),
        )
        .await;
    assert!(result.is_ok());

    // Should also succeed - same provider handles it
    let result = context
        .invoke_capability(
            ExtensionCapability::DeviceMetricsWrite,
            &json!({
                "device_id": "sensor-001",
                "metric": "temperature",
                "value": 28.5,
            }),
        )
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_multiple_providers_routing() {
    // Create multiple providers with different capabilities
    let device_provider = Arc::new(TestCapabilityProvider::new("device-provider"));
    let event_provider = Arc::new(TestCapabilityProvider::new("event-provider"));

    // Create shared providers map
    let providers = Arc::new(RwLock::new(HashMap::new()));

    let config = ExtensionContextConfig {
        extension_id: "multi-provider-test".to_string(),
        ..Default::default()
    };

    let context = ExtensionContext::new(config, providers.clone());

    // Register both providers
    context
        .register_provider("device-provider".to_string(), device_provider)
        .await;
    context
        .register_provider("event-provider".to_string(), event_provider)
        .await;

    // Both capabilities should work
    let result1 = context
        .invoke_capability(
            ExtensionCapability::DeviceMetricsRead,
            &json!({ "device_id": "test" }),
        )
        .await;
    assert!(result1.is_ok());

    let result2 = context
        .invoke_capability(
            ExtensionCapability::EventPublish,
            &json!({ "event_type": "test" }),
        )
        .await;
    assert!(result2.is_ok());
}

#[tokio::test]
async fn test_capability_not_available() {
    // Create provider with limited capabilities
    let provider = Arc::new(TestCapabilityProvider::new("limited-provider"));

    // Create shared providers map
    let providers = Arc::new(RwLock::new(HashMap::new()));

    let config = ExtensionContextConfig {
        extension_id: "test-extension".to_string(),
        ..Default::default()
    };

    let context = ExtensionContext::new(config, providers.clone());

    // Register provider
    context
        .register_provider("limited-provider".to_string(), provider)
        .await;

    // Should fail - capability not available from any provider
    let result = context
        .invoke_capability(
            ExtensionCapability::RuleTrigger,
            &json!({ "rule_id": "rule-001" }),
        )
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        CapabilityError::ProviderNotFound(cap) => {
            assert_eq!(cap, ExtensionCapability::RuleTrigger);
        }
        _ => panic!("Expected ProviderNotFound error"),
    }
}

#[tokio::test]
async fn test_capability_error_handling() {
    let provider = Arc::new(TestCapabilityProvider::new("test-provider"));

    // Create shared providers map
    let providers = Arc::new(RwLock::new(HashMap::new()));

    let config = ExtensionContextConfig {
        extension_id: "error-test".to_string(),
        ..Default::default()
    };

    let context = ExtensionContext::new(config, providers.clone());

    // Register provider
    context
        .register_provider("test-provider".to_string(), provider)
        .await;

    // Test with missing parameters - provider should handle gracefully
    let result = context
        .invoke_capability(
            ExtensionCapability::DeviceMetricsRead,
            &json!({}), // Missing device_id
        )
        .await;

    // Provider should still return a result (using default)
    assert!(result.is_ok());
}

// ============================================================================
// IPC Isolated Mode Tests
// ============================================================================

#[tokio::test]
#[ignore = "Requires extension-runner binary to be built"]
async fn test_ipc_capability_invocation() {
    // This test verifies capability invocation through IPC
    // It requires the extension-runner binary to be available

    let _event_bus = Arc::new(EventBus::new());

    let config = IsolatedManagerConfig::default();

    let manager = Arc::new(IsolatedExtensionManager::new(config));

    // Set up capability provider
    let provider = Arc::new(TestCapabilityProvider::new("ipc-provider"));
    manager.set_capability_provider(provider).await;

    // Note: This test would require a real extension binary that uses capabilities
    // For now, we just verify the manager can be created with a provider
    println!("IPC capability test placeholder - requires compiled binaries");
}

struct NativeWriteCapabilityProvider;

#[async_trait::async_trait]
impl ExtensionCapabilityProvider for NativeWriteCapabilityProvider {
    fn capability_manifest(&self) -> CapabilityManifest {
        CapabilityManifest {
            capabilities: vec![ExtensionCapability::DeviceMetricsWrite],
            api_version: "v1".to_string(),
            min_core_version: "0.6.0".to_string(),
            package_name: "native-write-provider".to_string(),
        }
    }

    async fn invoke_capability(
        &self,
        capability: ExtensionCapability,
        params: &Value,
    ) -> Result<Value, CapabilityError> {
        match capability {
            ExtensionCapability::DeviceMetricsWrite => Ok(json!({
                "success": true,
                "capability": capability.name(),
                "device_id": params.get("device_id").cloned().unwrap_or(json!(null)),
                "metric": params.get("metric").cloned().unwrap_or(json!(null)),
                "value": params.get("value").cloned().unwrap_or(json!(null)),
                "is_virtual": params.get("is_virtual").cloned().unwrap_or(json!(false)),
            })),
            _ => Err(CapabilityError::NotAvailable(capability)),
        }
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn build_test_binaries() {
    let status = Command::new("cargo")
        .current_dir(workspace_root())
        .args([
            "build",
            "-p",
            "neomind-extension-runner",
            "-p",
            "neomind-smoke-extension",
        ])
        .status()
        .expect("failed to run cargo build for native capability IPC test");
    assert!(
        status.success(),
        "cargo build failed for native capability IPC test"
    );
}

fn runner_dir() -> PathBuf {
    workspace_root().join("target").join("debug")
}

fn smoke_extension_path() -> PathBuf {
    let lib_name = if cfg!(target_os = "macos") {
        "libneomind_smoke_extension.dylib"
    } else if cfg!(target_os = "windows") {
        "neomind_smoke_extension.dll"
    } else {
        "libneomind_smoke_extension.so"
    };
    runner_dir().join(lib_name)
}

#[tokio::test]
#[ignore = "Requires compiled runner binary and native smoke extension"]
async fn test_native_isolated_capability_ipc() {
    build_test_binaries();

    let original_path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![runner_dir()];
    paths.extend(std::env::split_paths(&original_path));
    let joined = std::env::join_paths(paths).expect("failed to join PATH for test");
    unsafe {
        std::env::set_var("PATH", &joined);
    }

    let manager = Arc::new(IsolatedExtensionManager::new(
        IsolatedManagerConfig::default(),
    ));
    manager
        .set_capability_provider(Arc::new(NativeWriteCapabilityProvider))
        .await;

    let extension_path = smoke_extension_path();
    assert!(
        extension_path.exists(),
        "smoke extension binary not found at {}",
        extension_path.display()
    );

    let metadata = manager
        .load(&extension_path)
        .await
        .expect("failed to load smoke extension");
    assert_eq!(metadata.id, "smoke-test");

    // Liveness probe must round-trip on a healthy process, and a fresh
    // process has no crash history (the health monitor's Ping path).
    let ext = manager.get("smoke-test").await.expect("extension handle");
    ext.ping().await.expect("liveness probe should round-trip");
    assert_eq!(ext.crash_info().await, (0, None));

    let response = manager
        .execute_command(
            "smoke-test",
            "write_virtual_metric",
            &json!({
                "device_id": "device-42",
                "metric": "virtual.test.status",
                "value": "ok",
            }),
        )
        .await
        .expect("failed to execute write_virtual_metric");

    let capability_response = response
        .get("capability_response")
        .expect("missing capability_response");
    assert_eq!(
        capability_response.get("success").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        capability_response
            .get("capability")
            .and_then(|v| v.as_str()),
        Some("device_metrics_write")
    );
    assert_eq!(
        capability_response.get("device_id"),
        Some(&json!("device-42"))
    );
    assert_eq!(
        capability_response.get("metric"),
        Some(&json!("virtual.test.status"))
    );
    assert_eq!(capability_response.get("value"), Some(&json!("ok")));
    assert_eq!(capability_response.get("is_virtual"), Some(&json!(true)));

    manager
        .unload("smoke-test")
        .await
        .expect("failed to unload smoke extension");
}

#[tokio::test]
#[ignore = "Requires compiled runner binary and native smoke extension"]
async fn test_native_isolated_event_capability_ipc() {
    build_test_binaries();

    let original_path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![runner_dir()];
    paths.extend(std::env::split_paths(&original_path));
    let joined = std::env::join_paths(paths).expect("failed to join PATH for test");
    unsafe {
        std::env::set_var("PATH", &joined);
    }

    let mut config = IsolatedManagerConfig::default();
    config.extension_config.startup_timeout_secs = 60;
    let manager = Arc::new(IsolatedExtensionManager::new(config));
    manager
        .set_capability_provider(Arc::new(NativeWriteCapabilityProvider))
        .await;

    let extension_path = smoke_extension_path();
    manager
        .load(&extension_path)
        .await
        .expect("failed to load native capability smoke extension");

    manager
        .event_dispatcher()
        .dispatch_event(
            "CapabilitySmokeEvent",
            json!({
                "device_id": "device-event",
                "metric": "virtual.test.event",
                "value": "from-event",
            }),
        )
        .await;

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let response = manager
        .execute_command("smoke-test", "get_last_event_result", &json!({}))
        .await
        .expect("failed to fetch last event result");

    let capability_response = response
        .get("last_event_result")
        .expect("missing last_event_result");
    assert_eq!(
        capability_response.get("success").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        capability_response
            .get("capability")
            .and_then(|v| v.as_str()),
        Some("device_metrics_write")
    );
    assert_eq!(
        capability_response.get("device_id"),
        Some(&json!("device-event"))
    );
    assert_eq!(
        capability_response.get("metric"),
        Some(&json!("virtual.test.event"))
    );
    assert_eq!(capability_response.get("value"), Some(&json!("from-event")));
    assert_eq!(capability_response.get("is_virtual"), Some(&json!(true)));

    manager
        .unload("smoke-test")
        .await
        .expect("failed to unload smoke extension");
}
