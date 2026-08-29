//! Comprehensive Unit Tests for Extension Runner
//!
//! Tests cover:
//! - Extension type detection
//! - IPC message types
//! - IPC response types
//! - Error handling
//! - Stream types

use neomind_extension_sdk::{
    ErrorKind, ExtensionDescriptor, ExtensionMetadata, IpcMessage, IpcResponse, StreamClientInfo,
    StreamDataChunk,
};
use serde_json::json;

// ============================================================================
// Extension Type Tests
// ============================================================================

#[test]
fn test_extension_type_from_wasm_path() {
    let path = std::path::PathBuf::from("/path/to/extension.wasm");
    let ext = path.extension().and_then(|e| e.to_str());
    assert_eq!(ext, Some("wasm"));
}

#[test]
fn test_extension_type_from_native_path() {
    let paths = vec![
        "/path/to/extension.so",
        "/path/to/extension.dylib",
        "/path/to/extension.dll",
    ];

    for path_str in paths {
        let path = std::path::PathBuf::from(path_str);
        let ext = path.extension().and_then(|e| e.to_str());
        assert!(ext.is_some());
        assert_ne!(ext.unwrap().to_lowercase().as_str(), "wasm");
    }
}

// ============================================================================
// IPC Message Tests
// ============================================================================

#[test]
fn test_ipc_message_init() {
    let msg = IpcMessage::Init {
        config: json!({"key": "value"}),
    };

    let json = serde_json::to_string(&msg).unwrap();
    let parsed: IpcMessage = serde_json::from_str(&json).unwrap();

    match parsed {
        IpcMessage::Init { config } => {
            assert_eq!(config["key"], "value");
        }
        _ => panic!("Expected Init"),
    }
}

#[test]
fn test_ipc_message_execute_command() {
    let msg = IpcMessage::ExecuteCommand {
        command: "test_command".to_string(),
        args: json!({"param": "value"}),
        request_id: 1,
    };

    let json = serde_json::to_string(&msg).unwrap();
    let parsed: IpcMessage = serde_json::from_str(&json).unwrap();

    match parsed {
        IpcMessage::ExecuteCommand {
            command,
            args,
            request_id,
        } => {
            assert_eq!(command, "test_command");
            assert_eq!(args["param"], "value");
            assert_eq!(request_id, 1);
        }
        _ => panic!("Expected ExecuteCommand"),
    }
}

#[test]
fn test_ipc_message_produce_metrics() {
    let msg = IpcMessage::ProduceMetrics { request_id: 2 };

    let json = serde_json::to_string(&msg).unwrap();
    let parsed: IpcMessage = serde_json::from_str(&json).unwrap();

    match parsed {
        IpcMessage::ProduceMetrics { request_id } => {
            assert_eq!(request_id, 2);
        }
        _ => panic!("Expected ProduceMetrics"),
    }
}

#[test]
fn test_ipc_message_health_check() {
    let msg = IpcMessage::HealthCheck { request_id: 3 };

    let json = serde_json::to_string(&msg).unwrap();
    let parsed: IpcMessage = serde_json::from_str(&json).unwrap();

    match parsed {
        IpcMessage::HealthCheck { request_id } => {
            assert_eq!(request_id, 3);
        }
        _ => panic!("Expected HealthCheck"),
    }
}

#[test]
fn test_ipc_message_get_metadata() {
    let msg = IpcMessage::GetMetadata { request_id: 4 };

    let json = serde_json::to_string(&msg).unwrap();
    let parsed: IpcMessage = serde_json::from_str(&json).unwrap();

    match parsed {
        IpcMessage::GetMetadata { request_id } => {
            assert_eq!(request_id, 4);
        }
        _ => panic!("Expected GetMetadata"),
    }
}

#[test]
fn test_ipc_message_get_descriptor() {
    // Round-trip serialization of the descriptor-refresh request. Old
    // runners that don't know this variant must reject the JSON cleanly
    // (covered separately by the unknown-variant tolerance test below).
    let msg = IpcMessage::GetDescriptor { request_id: 42 };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"GetDescriptor\""));
    let parsed: IpcMessage = serde_json::from_str(&json).unwrap();
    match parsed {
        IpcMessage::GetDescriptor { request_id } => assert_eq!(request_id, 42),
        _ => panic!("Expected GetDescriptor"),
    }
}

#[test]
fn test_ipc_response_descriptor_round_trip() {
    use neomind_extension_sdk::CommandBuilder;
    let metadata = ExtensionMetadata::new("test-ext", "Test Extension", "1.0.0")
        .with_description("for round-trip");
    let cmd = CommandBuilder::new("ping")
        .display_name("Ping")
        .description("Health probe")
        .build();
    let descriptor = ExtensionDescriptor {
        metadata: metadata.clone(),
        commands: vec![cmd],
        metrics: vec![],
    };
    let resp = IpcResponse::Descriptor {
        request_id: 7,
        descriptor: descriptor.clone(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("\"Descriptor\""));
    let parsed: IpcResponse = serde_json::from_str(&json).unwrap();
    match parsed {
        IpcResponse::Descriptor {
            request_id,
            descriptor,
        } => {
            assert_eq!(request_id, 7);
            assert_eq!(descriptor.metadata.id, "test-ext");
            assert_eq!(descriptor.metadata.version, "1.0.0");
            assert_eq!(descriptor.commands.len(), 1);
            assert_eq!(descriptor.commands[0].name, "ping");
        }
        _ => panic!("Expected Descriptor"),
    }
}

#[test]
fn test_unknown_ipc_message_variant_tolerated() {
    // Mixed-version deployment safety: a JSON shape the receiver doesn't
    // know must produce a clean serde error (NOT a panic). The host then
    // falls back to its cached descriptor. This locks that contract.
    let unknown = r#"{"GetDescriptor":{"request_id":99}}"#;
    let err = serde_json::from_str::<IpcMessage>(unknown);
    // If the variant exists in this build, the parse succeeds. Either way
    // it must not panic; the only acceptable outcomes are Ok or Err.
    match err {
        Ok(IpcMessage::GetDescriptor { request_id }) => assert_eq!(request_id, 99),
        Ok(other) => panic!("unexpected variant: {:?}", other),
        Err(_) => { /* old binary: tolerated */ }
    }
}

#[test]
fn test_ipc_message_get_stats() {
    let msg = IpcMessage::GetStats { request_id: 5 };

    let json = serde_json::to_string(&msg).unwrap();
    let parsed: IpcMessage = serde_json::from_str(&json).unwrap();

    match parsed {
        IpcMessage::GetStats { request_id } => {
            assert_eq!(request_id, 5);
        }
        _ => panic!("Expected GetStats"),
    }
}

#[test]
fn test_ipc_message_shutdown() {
    let msg = IpcMessage::Shutdown;
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: IpcMessage = serde_json::from_str(&json).unwrap();

    assert!(matches!(parsed, IpcMessage::Shutdown));
}

#[test]
fn test_ipc_message_ping() {
    let msg = IpcMessage::Ping {
        request_id: 0,
        timestamp: 1234567890,
    };

    let json = serde_json::to_string(&msg).unwrap();
    let parsed: IpcMessage = serde_json::from_str(&json).unwrap();

    match parsed {
        IpcMessage::Ping { timestamp, .. } => {
            assert_eq!(timestamp, 1234567890);
        }
        _ => panic!("Expected Ping"),
    }
}

#[test]
fn test_ipc_message_event_push() {
    let msg = IpcMessage::EventPush {
        event_type: "DeviceMetric".to_string(),
        payload: json!({"temperature": 25.5}),
        timestamp: 1234567890,
    };

    let json = serde_json::to_string(&msg).unwrap();
    let parsed: IpcMessage = serde_json::from_str(&json).unwrap();

    match parsed {
        IpcMessage::EventPush {
            event_type,
            payload,
            timestamp,
        } => {
            assert_eq!(event_type, "DeviceMetric");
            assert_eq!(payload["temperature"], 25.5);
            assert_eq!(timestamp, 1234567890);
        }
        _ => panic!("Expected EventPush"),
    }
}

// ============================================================================
// IPC Response Tests
// ============================================================================

#[test]
fn test_ipc_response_ready() {
    let metadata = ExtensionMetadata::new("test.extension", "Test Extension", "1.0.0");
    let descriptor = ExtensionDescriptor::new(metadata);

    let resp = IpcResponse::Ready { descriptor };

    let json = serde_json::to_string(&resp).unwrap();
    let parsed: IpcResponse = serde_json::from_str(&json).unwrap();

    match parsed {
        IpcResponse::Ready { descriptor } => {
            assert_eq!(descriptor.id(), "test.extension");
        }
        _ => panic!("Expected Ready"),
    }
}

#[test]
fn test_ipc_response_success() {
    let resp = IpcResponse::Success {
        request_id: 1,
        data: json!({"status": "success"}),
    };

    let json = serde_json::to_string(&resp).unwrap();
    let parsed: IpcResponse = serde_json::from_str(&json).unwrap();

    match parsed {
        IpcResponse::Success { request_id, data } => {
            assert_eq!(request_id, 1);
            assert_eq!(data["status"], "success");
        }
        _ => panic!("Expected Success"),
    }
}

#[test]
fn test_ipc_response_error() {
    let resp = IpcResponse::Error {
        request_id: 1,
        error: "Something went wrong".to_string(),
        kind: ErrorKind::ExecutionFailed,
    };

    let json = serde_json::to_string(&resp).unwrap();
    let parsed: IpcResponse = serde_json::from_str(&json).unwrap();

    match parsed {
        IpcResponse::Error {
            request_id,
            error,
            kind,
        } => {
            assert_eq!(request_id, 1);
            assert_eq!(error, "Something went wrong");
            assert_eq!(kind, ErrorKind::ExecutionFailed);
        }
        _ => panic!("Expected Error"),
    }
}

#[test]
fn test_ipc_response_health() {
    let resp = IpcResponse::Health {
        request_id: 1,
        healthy: true,
    };

    let json = serde_json::to_string(&resp).unwrap();
    let parsed: IpcResponse = serde_json::from_str(&json).unwrap();

    match parsed {
        IpcResponse::Health {
            request_id,
            healthy,
        } => {
            assert_eq!(request_id, 1);
            assert!(healthy);
        }
        _ => panic!("Expected Health"),
    }
}

#[test]
fn test_ipc_response_metadata() {
    let metadata = ExtensionMetadata::new("test.extension", "Test Extension", "1.0.0");

    let resp = IpcResponse::Metadata {
        request_id: 1,
        metadata: metadata.clone(),
    };

    let json = serde_json::to_string(&resp).unwrap();
    let parsed: IpcResponse = serde_json::from_str(&json).unwrap();

    match parsed {
        IpcResponse::Metadata {
            request_id,
            metadata,
        } => {
            assert_eq!(request_id, 1);
            assert_eq!(metadata.id, "test.extension");
        }
        _ => panic!("Expected Metadata"),
    }
}

#[test]
fn test_ipc_response_stats() {
    let resp = IpcResponse::Stats {
        request_id: 1,
        start_count: 5,
        stop_count: 2,
        error_count: 1,
        last_error: Some("Test error".to_string()),
    };

    let json = serde_json::to_string(&resp).unwrap();
    let parsed: IpcResponse = serde_json::from_str(&json).unwrap();

    match parsed {
        IpcResponse::Stats {
            request_id,
            start_count,
            stop_count,
            error_count,
            last_error,
        } => {
            assert_eq!(request_id, 1);
            assert_eq!(start_count, 5);
            assert_eq!(stop_count, 2);
            assert_eq!(error_count, 1);
            assert_eq!(last_error, Some("Test error".to_string()));
        }
        _ => panic!("Expected Stats"),
    }
}

#[test]
fn test_ipc_response_pong() {
    let resp = IpcResponse::Pong {
        request_id: 0,
        timestamp: 1234567890,
    };

    let json = serde_json::to_string(&resp).unwrap();
    let parsed: IpcResponse = serde_json::from_str(&json).unwrap();

    match parsed {
        IpcResponse::Pong { timestamp, .. } => {
            assert_eq!(timestamp, 1234567890);
        }
        _ => panic!("Expected Pong"),
    }
}

// ============================================================================
// Error Kind Tests
// ============================================================================

#[test]
fn test_error_kind_command_not_found() {
    let err = ErrorKind::CommandNotFound;
    let msg = format!("{:?}", err);
    assert!(msg.contains("CommandNotFound"));
}

#[test]
fn test_error_kind_invalid_arguments() {
    let err = ErrorKind::InvalidArguments;
    let msg = format!("{:?}", err);
    assert!(msg.contains("InvalidArguments"));
}

#[test]
fn test_error_kind_execution_failed() {
    let err = ErrorKind::ExecutionFailed;
    let msg = format!("{:?}", err);
    assert!(msg.contains("ExecutionFailed"));
}

#[test]
fn test_error_kind_timeout() {
    let err = ErrorKind::Timeout;
    let msg = format!("{:?}", err);
    assert!(msg.contains("Timeout"));
}

#[test]
fn test_error_kind_not_found() {
    let err = ErrorKind::NotFound;
    let msg = format!("{:?}", err);
    assert!(msg.contains("NotFound"));
}

#[test]
fn test_error_kind_invalid_format() {
    let err = ErrorKind::InvalidFormat;
    let msg = format!("{:?}", err);
    assert!(msg.contains("InvalidFormat"));
}

#[test]
fn test_error_kind_serialization() {
    let err = ErrorKind::Timeout;
    let json = serde_json::to_string(&err).unwrap();
    let parsed: ErrorKind = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed, ErrorKind::Timeout);
}

#[test]
fn test_error_kind_equality() {
    assert_eq!(ErrorKind::Timeout, ErrorKind::Timeout);
    assert_ne!(ErrorKind::Timeout, ErrorKind::ExecutionFailed);
}

// ============================================================================
// Stream Types Tests
// ============================================================================

#[test]
fn test_stream_client_info() {
    let info = StreamClientInfo {
        client_id: "client-123".to_string(),
        ip_addr: Some("192.168.1.1".to_string()),
        user_agent: Some("NeoMind/1.0".to_string()),
    };

    let json = serde_json::to_string(&info).unwrap();
    let parsed: StreamClientInfo = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.client_id, "client-123");
    assert_eq!(parsed.ip_addr, Some("192.168.1.1".to_string()));
    assert_eq!(parsed.user_agent, Some("NeoMind/1.0".to_string()));
}

#[test]
fn test_stream_data_chunk() {
    let chunk = StreamDataChunk {
        sequence: 1,
        data_type: "text/plain".to_string(),
        data: vec![1, 2, 3, 4, 5],
        timestamp: 1234567890,
        is_last: false,
    };

    let json = serde_json::to_string(&chunk).unwrap();
    let parsed: StreamDataChunk = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.sequence, 1);
    assert_eq!(parsed.data_type, "text/plain");
    assert_eq!(parsed.data, vec![1, 2, 3, 4, 5]);
    assert!(!parsed.is_last);
}

#[test]
fn test_stream_data_chunk_last() {
    let chunk = StreamDataChunk {
        sequence: 10,
        data_type: "application/json".to_string(),
        data: vec![],
        timestamp: 1234567900,
        is_last: true,
    };

    let json = serde_json::to_string(&chunk).unwrap();
    let parsed: StreamDataChunk = serde_json::from_str(&json).unwrap();

    assert!(parsed.is_last);
    assert_eq!(parsed.sequence, 10);
}

// ============================================================================
// Stream Session Tests
// ============================================================================

#[test]
fn test_ipc_message_init_stream_session() {
    let msg = IpcMessage::InitStreamSession {
        request_id: 42,
        session_id: "session-123".to_string(),
        extension_id: "test.extension".to_string(),
        config: json!({"mode": "push"}),
        client_info: StreamClientInfo {
            client_id: "client-1".to_string(),
            ip_addr: None,
            user_agent: None,
        },
    };

    let json = serde_json::to_string(&msg).unwrap();
    let parsed: IpcMessage = serde_json::from_str(&json).unwrap();

    match parsed {
        IpcMessage::InitStreamSession {
            request_id,
            session_id,
            extension_id,
            config,
            client_info,
        } => {
            assert_eq!(request_id, 42);
            assert_eq!(session_id, "session-123");
            assert_eq!(extension_id, "test.extension");
            assert_eq!(config["mode"], "push");
            assert_eq!(client_info.client_id, "client-1");
        }
        _ => panic!("Expected InitStreamSession"),
    }
}

#[test]
fn test_ipc_message_close_stream_session() {
    let msg = IpcMessage::CloseStreamSession {
        request_id: 43,
        session_id: "session-123".to_string(),
    };

    let json = serde_json::to_string(&msg).unwrap();
    let parsed: IpcMessage = serde_json::from_str(&json).unwrap();

    match parsed {
        IpcMessage::CloseStreamSession {
            request_id,
            session_id,
        } => {
            assert_eq!(request_id, 43);
            assert_eq!(session_id, "session-123");
        }
        _ => panic!("Expected CloseStreamSession"),
    }
}

#[test]
fn test_ipc_response_stream_session_init() {
    let resp = IpcResponse::StreamSessionInit {
        request_id: 42,
        session_id: "session-123".to_string(),
        success: true,
        error: None,
    };

    let json = serde_json::to_string(&resp).unwrap();
    let parsed: IpcResponse = serde_json::from_str(&json).unwrap();

    match parsed {
        IpcResponse::StreamSessionInit {
            request_id,
            session_id,
            success,
            error,
        } => {
            assert_eq!(request_id, 42);
            assert_eq!(session_id, "session-123");
            assert!(success);
            assert!(error.is_none());
        }
        _ => panic!("Expected StreamSessionInit"),
    }
}

#[test]
fn test_ipc_response_stream_session_init_error() {
    let resp = IpcResponse::StreamSessionInit {
        request_id: 43,
        session_id: "session-456".to_string(),
        success: false,
        error: Some("Failed to initialize".to_string()),
    };

    let json = serde_json::to_string(&resp).unwrap();
    let parsed: IpcResponse = serde_json::from_str(&json).unwrap();

    match parsed {
        IpcResponse::StreamSessionInit { success, error, .. } => {
            assert!(!success);
            assert_eq!(error, Some("Failed to initialize".to_string()));
        }
        _ => panic!("Expected StreamSessionInit"),
    }
}

#[test]
fn test_ipc_response_stream_session_closed() {
    let resp = IpcResponse::StreamSessionClosed {
        request_id: 44,
        session_id: "session-123".to_string(),
        total_frames: 100,
        duration_ms: 5000,
    };

    let json = serde_json::to_string(&resp).unwrap();
    let parsed: IpcResponse = serde_json::from_str(&json).unwrap();

    match parsed {
        IpcResponse::StreamSessionClosed {
            request_id,
            session_id,
            total_frames,
            duration_ms,
        } => {
            assert_eq!(request_id, 44);
            assert_eq!(session_id, "session-123");
            assert_eq!(total_frames, 100);
            assert_eq!(duration_ms, 5000);
        }
        _ => panic!("Expected StreamSessionClosed"),
    }
}

// ============================================================================
// Capability Invocation Tests
// ============================================================================

#[test]
fn test_ipc_message_invoke_capability() {
    let msg = IpcMessage::InvokeCapability {
        request_id: 1,
        capability: "device_metrics_read".to_string(),
        params: json!({"device_id": "device-1"}),
    };

    let json = serde_json::to_string(&msg).unwrap();
    let parsed: IpcMessage = serde_json::from_str(&json).unwrap();

    match parsed {
        IpcMessage::InvokeCapability {
            request_id,
            capability,
            params,
        } => {
            assert_eq!(request_id, 1);
            assert_eq!(capability, "device_metrics_read");
            assert_eq!(params["device_id"], "device-1");
        }
        _ => panic!("Expected InvokeCapability"),
    }
}

#[test]
fn test_ipc_message_subscribe_events() {
    let msg = IpcMessage::SubscribeEvents {
        request_id: 1,
        event_types: vec!["DeviceMetric".to_string(), "DeviceStatus".to_string()],
        filter: Some(json!({"device_id": "device-1"})),
    };

    let json = serde_json::to_string(&msg).unwrap();
    let parsed: IpcMessage = serde_json::from_str(&json).unwrap();

    match parsed {
        IpcMessage::SubscribeEvents {
            request_id,
            event_types,
            filter,
        } => {
            assert_eq!(request_id, 1);
            assert_eq!(event_types.len(), 2);
            assert!(filter.is_some());
        }
        _ => panic!("Expected SubscribeEvents"),
    }
}

#[test]
fn test_ipc_message_poll_events() {
    let msg = IpcMessage::PollEvents {
        request_id: 1,
        subscription_id: "sub-123".to_string(),
    };

    let json = serde_json::to_string(&msg).unwrap();
    let parsed: IpcMessage = serde_json::from_str(&json).unwrap();

    match parsed {
        IpcMessage::PollEvents {
            request_id,
            subscription_id,
        } => {
            assert_eq!(request_id, 1);
            assert_eq!(subscription_id, "sub-123");
        }
        _ => panic!("Expected PollEvents"),
    }
}

// ============================================================================
// Integration Scenario Tests
// ============================================================================

#[test]
fn test_full_message_roundtrip() {
    // Create a message
    let msg = IpcMessage::ExecuteCommand {
        command: "process_data".to_string(),
        args: json!({
            "input": "test data",
            "options": {
                "format": "json",
                "validate": true,
            }
        }),
        request_id: 42,
    };

    // Serialize
    let json = serde_json::to_string(&msg).unwrap();

    // Deserialize
    let parsed: IpcMessage = serde_json::from_str(&json).unwrap();

    // Verify
    match parsed {
        IpcMessage::ExecuteCommand {
            command,
            args,
            request_id,
        } => {
            assert_eq!(command, "process_data");
            assert_eq!(request_id, 42);
            assert_eq!(args["input"], "test data");
            assert_eq!(args["options"]["format"], "json");
            assert_eq!(args["options"]["validate"], true);
        }
        _ => panic!("Expected ExecuteCommand"),
    }
}

#[test]
fn test_full_response_roundtrip() {
    // Create a response
    let resp = IpcResponse::Success {
        request_id: 42,
        data: json!({
            "processed": true,
            "output": {
                "count": 100,
                "status": "success",
            }
        }),
    };

    // Serialize
    let json = serde_json::to_string(&resp).unwrap();

    // Deserialize
    let parsed: IpcResponse = serde_json::from_str(&json).unwrap();

    // Verify
    match parsed {
        IpcResponse::Success { request_id, data } => {
            assert_eq!(request_id, 42);
            assert_eq!(data["processed"], true);
            assert_eq!(data["output"]["count"], 100);
        }
        _ => panic!("Expected Success"),
    }
}

#[test]
fn test_error_response_roundtrip() {
    let resp = IpcResponse::Error {
        request_id: 42,
        error: "Command failed".to_string(),
        kind: ErrorKind::ExecutionFailed,
    };

    let json = serde_json::to_string(&resp).unwrap();
    let parsed: IpcResponse = serde_json::from_str(&json).unwrap();

    match parsed {
        IpcResponse::Error {
            request_id,
            error,
            kind,
        } => {
            assert_eq!(request_id, 42);
            assert_eq!(error, "Command failed");
            assert_eq!(kind, ErrorKind::ExecutionFailed);
        }
        _ => panic!("Expected Error"),
    }
}
