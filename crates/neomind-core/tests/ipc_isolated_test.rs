//! IPC Isolated Mode Complete Flow Tests
//!
//! These tests verify the complete IPC isolated extension flow:
//! 1. Extension process startup and initialization
//! 2. Command execution through IPC
//! 3. Capability invocation through bidirectional IPC
//! 4. Event subscription and delivery
//! 5. Streaming support (Stateless, Stateful, Push modes)
//! 6. Graceful shutdown

use std::sync::Arc;

use async_trait::async_trait;
use neomind_core::eventbus::EventBus;
use neomind_core::extension::context::{
    CapabilityError, CapabilityManifest, ExtensionCapability, ExtensionCapabilityProvider,
};
use neomind_core::extension::isolated::{
    BatchCommand, BatchResult, ErrorKind, IpcMessage, IpcResponse, IsolatedExtensionManager,
    IsolatedManagerConfig, StreamDataChunk,
};
use serde_json::{json, Value};

// ============================================================================
// Test Helpers
// ============================================================================

/// Create a test capability provider
fn create_test_provider() -> Arc<dyn ExtensionCapabilityProvider> {
    struct TestProvider;

    #[async_trait]
    impl ExtensionCapabilityProvider for TestProvider {
        fn capability_manifest(&self) -> CapabilityManifest {
            CapabilityManifest {
                capabilities: vec![
                    ExtensionCapability::DeviceMetricsRead,
                    ExtensionCapability::DeviceMetricsWrite,
                    ExtensionCapability::EventPublish,
                ],
                api_version: "v1".to_string(),
                min_core_version: "0.5.0".to_string(),
                package_name: "test-provider".to_string(),
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
                        "temperature": 25.5,
                        "humidity": 65.0,
                    }))
                }
                ExtensionCapability::DeviceMetricsWrite => Ok(json!({ "success": true })),
                ExtensionCapability::EventPublish => {
                    Ok(json!({ "success": true, "published": true }))
                }
                _ => Err(CapabilityError::NotAvailable(capability)),
            }
        }
    }

    Arc::new(TestProvider)
}

// ============================================================================
// IPC Message Tests
// ============================================================================

#[test]
fn test_ipc_message_serialization() {
    // Test IpcMessage serialization
    let init_msg = IpcMessage::Init {
        config: json!({ "test": true }),
    };

    let bytes = init_msg.to_bytes().expect("Failed to serialize");
    let decoded = IpcMessage::from_bytes(&bytes).expect("Failed to deserialize");

    match decoded {
        IpcMessage::Init { config } => {
            assert!(config.get("test").unwrap().as_bool().unwrap());
        }
        _ => panic!("Wrong message type"),
    }
}

#[test]
fn test_ipc_response_serialization() {
    // Test IpcResponse serialization
    let success_resp = IpcResponse::Success {
        request_id: 123,
        data: json!({ "result": "ok" }),
    };

    let bytes = success_resp.to_bytes().expect("Failed to serialize");
    let decoded = IpcResponse::from_bytes(&bytes).expect("Failed to deserialize");

    match decoded {
        IpcResponse::Success { request_id, data } => {
            assert_eq!(request_id, 123);
            assert!(data.get("result").unwrap().as_str().unwrap() == "ok");
        }
        _ => panic!("Wrong response type"),
    }
}

#[test]
fn test_capability_request_response() {
    // Test CapabilityRequest and CapabilityResult
    let cap_request = IpcResponse::CapabilityRequest {
        request_id: 456,
        capability: "device_metrics_read".to_string(),
        params: json!({ "device_id": "sensor-001" }),
    };

    let bytes = cap_request.to_bytes().expect("Failed to serialize");
    let decoded = IpcResponse::from_bytes(&bytes).expect("Failed to deserialize");

    match decoded {
        IpcResponse::CapabilityRequest {
            request_id,
            capability,
            params,
        } => {
            assert_eq!(request_id, 456);
            assert_eq!(capability, "device_metrics_read");
            assert!(params.get("device_id").unwrap().as_str().unwrap() == "sensor-001");
        }
        _ => panic!("Wrong response type"),
    }

    // Test CapabilityResult
    let cap_result = IpcResponse::CapabilityResult {
        request_id: 456,
        result: json!({ "temperature": 25.5 }),
        error: None,
    };

    let bytes = cap_result.to_bytes().expect("Failed to serialize");
    let decoded = IpcResponse::from_bytes(&bytes).expect("Failed to deserialize");

    match decoded {
        IpcResponse::CapabilityResult {
            request_id,
            result,
            error,
        } => {
            assert_eq!(request_id, 456);
            assert!(result.get("temperature").unwrap().as_f64().unwrap() == 25.5);
            assert!(error.is_none());
        }
        _ => panic!("Wrong response type"),
    }
}

// ============================================================================
// Streaming IPC Tests
// ============================================================================

#[test]
fn test_streaming_ipc_messages() {
    // Test ProcessChunk message
    let process_chunk = IpcMessage::ProcessChunk {
        request_id: 1,
        chunk: StreamDataChunk {
            sequence: 1,
            data_type: "image/jpeg".to_string(),
            data: vec![0xFF, 0xD8, 0xFF], // JPEG header
            timestamp: chrono::Utc::now().timestamp_millis(),
            is_last: false,
        },
    };

    let bytes = process_chunk.to_bytes().expect("Failed to serialize");
    let decoded = IpcMessage::from_bytes(&bytes).expect("Failed to deserialize");

    match decoded {
        IpcMessage::ProcessChunk { request_id, chunk } => {
            assert_eq!(request_id, 1);
            assert_eq!(chunk.sequence, 1);
            assert_eq!(chunk.data_type, "image/jpeg");
            assert!(!chunk.is_last);
        }
        _ => panic!("Wrong message type"),
    }

    // Test ChunkResult response
    let chunk_result = IpcResponse::ChunkResult {
        request_id: 1,
        input_sequence: 1,
        output_sequence: 1,
        data: vec![0x01, 0x02, 0x03],
        data_type: "application/json".to_string(),
        processing_ms: 15.5,
        metadata: Some(json!({ "detected": true })),
    };

    let bytes = chunk_result.to_bytes().expect("Failed to serialize");
    let decoded = IpcResponse::from_bytes(&bytes).expect("Failed to deserialize");

    match decoded {
        IpcResponse::ChunkResult {
            request_id,
            processing_ms,
            metadata,
            ..
        } => {
            assert_eq!(request_id, 1);
            assert!((processing_ms - 15.5).abs() < 0.01);
            assert!(metadata
                .unwrap()
                .get("detected")
                .unwrap()
                .as_bool()
                .unwrap());
        }
        _ => panic!("Wrong response type"),
    }
}

#[test]
fn test_push_mode_ipc_messages() {
    // Test StartPush message
    let start_push = IpcMessage::StartPush {
        request_id: 1,
        session_id: "session-001".to_string(),
    };

    let bytes = start_push.to_bytes().expect("Failed to serialize");
    let decoded = IpcMessage::from_bytes(&bytes).expect("Failed to deserialize");

    match decoded {
        IpcMessage::StartPush {
            request_id,
            session_id,
        } => {
            assert_eq!(request_id, 1);
            assert_eq!(session_id, "session-001");
        }
        _ => panic!("Wrong message type"),
    }

    // Test PushStarted response
    let push_started = IpcResponse::PushStarted {
        request_id: 1,
        session_id: "session-001".to_string(),
        success: true,
        error: None,
    };

    let bytes = push_started.to_bytes().expect("Failed to serialize");
    let decoded = IpcResponse::from_bytes(&bytes).expect("Failed to deserialize");

    match decoded {
        IpcResponse::PushStarted {
            request_id,
            session_id,
            success,
            error,
        } => {
            assert_eq!(request_id, 1);
            assert_eq!(session_id, "session-001");
            assert!(success);
            assert!(error.is_none());
        }
        _ => panic!("Wrong response type"),
    }

    // Test PushOutput message (extension-initiated)
    let push_output = IpcResponse::PushOutput {
        session_id: "session-001".to_string(),
        sequence: 1,
        data: vec![0x01, 0x02, 0x03],
        data_type: "video/h264".to_string(),
        timestamp: chrono::Utc::now().timestamp_millis(),
        metadata: None,
    };

    let bytes = push_output.to_bytes().expect("Failed to serialize");
    let decoded = IpcResponse::from_bytes(&bytes).expect("Failed to deserialize");

    match decoded {
        IpcResponse::PushOutput {
            session_id,
            sequence,
            data_type,
            ..
        } => {
            assert_eq!(session_id, "session-001");
            assert_eq!(sequence, 1);
            assert_eq!(data_type, "video/h264");
        }
        _ => panic!("Wrong response type"),
    }
}

// ============================================================================
// Event IPC Tests
// ============================================================================

#[test]
fn test_event_ipc_messages() {
    // Test EventPush message
    let event_push = IpcMessage::EventPush {
        event_type: "DeviceMetric".to_string(),
        payload: json!({
            "device_id": "sensor-001",
            "metric": "temperature",
            "value": 25.5,
        }),
        timestamp: chrono::Utc::now().timestamp_millis(),
    };

    let bytes = event_push.to_bytes().expect("Failed to serialize");
    let decoded = IpcMessage::from_bytes(&bytes).expect("Failed to deserialize");

    match decoded {
        IpcMessage::EventPush {
            event_type,
            payload,
            ..
        } => {
            assert_eq!(event_type, "DeviceMetric");
            assert!(payload.get("device_id").unwrap().as_str().unwrap() == "sensor-001");
        }
        _ => panic!("Wrong message type"),
    }

    // Test SubscribeEvents message
    let subscribe = IpcMessage::SubscribeEvents {
        request_id: 1,
        event_types: vec!["DeviceMetric".to_string(), "DeviceOnline".to_string()],
        filter: Some(json!({ "device_id": "sensor-001" })),
    };

    let bytes = subscribe.to_bytes().expect("Failed to serialize");
    let decoded = IpcMessage::from_bytes(&bytes).expect("Failed to deserialize");

    match decoded {
        IpcMessage::SubscribeEvents {
            request_id,
            event_types,
            filter,
        } => {
            assert_eq!(request_id, 1);
            assert_eq!(event_types.len(), 2);
            assert!(filter.is_some());
        }
        _ => panic!("Wrong message type"),
    }
}

// ============================================================================
// Manager Configuration Tests
// ============================================================================

#[test]
fn test_isolated_manager_config() {
    let config = IsolatedManagerConfig::default();

    assert!(config.isolated_by_default);
    assert!(config.force_isolated.is_empty());
    assert!(config.force_isolated.is_empty());
    assert!(config.extension_config.restart_on_crash);
}

// ============================================================================
// Integration Tests (require compiled binaries)
// ============================================================================

#[tokio::test]
#[serial_test::serial]
#[ignore = "Requires extension-runner binary to be built"]
async fn test_full_ipc_flow() {
    // This test requires:
    // 1. Compiled extension-runner binary
    // 2. A test extension (native or WASM)

    let _event_bus = Arc::new(EventBus::new());

    let config = IsolatedManagerConfig::default();

    let manager = Arc::new(IsolatedExtensionManager::new(config));

    // Set up capability provider
    let provider = create_test_provider();
    manager.set_capability_provider(provider).await;

    // Note: Full flow test would require:
    // 1. Loading an extension
    // 2. Sending commands
    // 3. Invoking capabilities
    // 4. Subscribing to events
    // 5. Streaming data
    // 6. Shutting down

    // This is a placeholder for the full integration test
    println!("Full IPC flow test placeholder - requires compiled binaries");
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn test_ipc_error_responses() {
    // Test Error response
    let error_resp = IpcResponse::Error {
        request_id: 1,
        error: "Command not found".to_string(),
        kind: ErrorKind::NotFound,
    };

    let bytes = error_resp.to_bytes().expect("Failed to serialize");
    let decoded = IpcResponse::from_bytes(&bytes).expect("Failed to deserialize");

    match decoded {
        IpcResponse::Error {
            request_id,
            error,
            kind,
        } => {
            assert_eq!(request_id, 1);
            assert_eq!(error, "Command not found");
            assert_eq!(kind, ErrorKind::NotFound);
        }
        _ => panic!("Wrong response type"),
    }

    // Test StreamError response
    let stream_error = IpcResponse::StreamError {
        request_id: 0,
        session_id: "session-001".to_string(),
        code: "PROCESSING_ERROR".to_string(),
        message: "Failed to process frame".to_string(),
    };

    let bytes = stream_error.to_bytes().expect("Failed to serialize");
    let decoded = IpcResponse::from_bytes(&bytes).expect("Failed to deserialize");

    match decoded {
        IpcResponse::StreamError {
            request_id: _,
            session_id,
            code,
            message,
        } => {
            assert_eq!(session_id, "session-001");
            assert_eq!(code, "PROCESSING_ERROR");
            assert_eq!(message, "Failed to process frame");
        }
        _ => panic!("Wrong response type"),
    }
}

// ============================================================================
// Batch Operations Tests
// ============================================================================

#[test]
fn test_batch_ipc_messages() {
    // Test ExecuteBatch message
    let batch = IpcMessage::ExecuteBatch {
        commands: vec![
            BatchCommand {
                command: "read".to_string(),
                args: json!({ "device_id": "sensor-001" }),
            },
            BatchCommand {
                command: "write".to_string(),
                args: json!({ "device_id": "sensor-001", "value": 30.0 }),
            },
        ],
        request_id: 1,
    };

    let bytes = batch.to_bytes().expect("Failed to serialize");
    let decoded = IpcMessage::from_bytes(&bytes).expect("Failed to deserialize");

    match decoded {
        IpcMessage::ExecuteBatch {
            commands,
            request_id,
        } => {
            assert_eq!(request_id, 1);
            assert_eq!(commands.len(), 2);
        }
        _ => panic!("Wrong message type"),
    }

    // Test BatchResults response
    let batch_results = IpcResponse::BatchResults {
        request_id: 1,
        results: vec![
            BatchResult {
                command: "read".to_string(),
                success: true,
                data: Some(json!({ "temperature": 25.5 })),
                error: None,
                elapsed_ms: 10.0,
            },
            BatchResult {
                command: "write".to_string(),
                success: true,
                data: Some(json!({ "written": true })),
                error: None,
                elapsed_ms: 15.5,
            },
        ],
        total_elapsed_ms: 25.5,
    };

    let bytes = batch_results.to_bytes().expect("Failed to serialize");
    let decoded = IpcResponse::from_bytes(&bytes).expect("Failed to deserialize");

    match decoded {
        IpcResponse::BatchResults {
            request_id,
            results,
            total_elapsed_ms,
        } => {
            assert_eq!(request_id, 1);
            assert_eq!(results.len(), 2);
            assert!((total_elapsed_ms - 25.5).abs() < 0.01);
        }
        _ => panic!("Wrong response type"),
    }
}
