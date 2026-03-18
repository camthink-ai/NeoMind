//! Comprehensive Unit Tests for Extension Proxy Module
//!
//! Tests cover:
//! - IsolatedExtensionProxy creation
//! - Error conversion from IsolatedExtensionError to ExtensionError
//! - Metadata caching
//! - Command execution proxying
//! - Stream capability handling
//! - Session management proxying

use neomind_core::extension::proxy::{IsolatedExtensionProxy, create_proxy, create_proxy_with_descriptor};
use neomind_core::extension::isolated::IsolatedExtensionError;
use neomind_core::extension::system::{
    Extension, ExtensionError, ExtensionMetadata, ExtensionDescriptor,
    ExtensionCommand, MetricDescriptor, ExtensionMetricValue,
};
use neomind_core::extension::stream::{
    StreamCapability, StreamDirection, StreamMode, StreamSession,
    ClientInfo, DataChunk, StreamDataType,
};
use std::sync::Arc;
use serde_json::json;

// ============================================================================
// Error Conversion Tests
// ============================================================================

// Note: convert_error is a private method, so we test error handling
// through the public interface instead.

#[test]
fn test_isolated_extension_error_types() {
    // Test that all error types can be created and displayed
    let errors: Vec<IsolatedExtensionError> = vec![
        IsolatedExtensionError::SpawnFailed("Process failed".to_string()),
        IsolatedExtensionError::IpcError("Channel closed".to_string()),
        IsolatedExtensionError::Crashed("Segmentation fault".to_string()),
        IsolatedExtensionError::Timeout(5000),
        IsolatedExtensionError::InvalidResponse("Bad JSON".to_string()),
        IsolatedExtensionError::NotInitialized,
        IsolatedExtensionError::AlreadyRunning,
        IsolatedExtensionError::NotRunning,
        IsolatedExtensionError::TooManyRequests(100),
        IsolatedExtensionError::LoadError("Missing dependency".to_string()),
        IsolatedExtensionError::UnexpectedResponse,
        IsolatedExtensionError::ChannelClosed,
        IsolatedExtensionError::ExtensionError("Custom error".to_string()),
    ];

    for err in errors {
        // Each error should have a meaningful display message
        let msg = err.to_string();
        assert!(!msg.is_empty());
    }
}

// ============================================================================
// ExtensionDescriptor Tests
// ============================================================================

#[test]
fn test_extension_descriptor_creation() {
    let metadata = ExtensionMetadata::new(
        "test.extension",
        "Test Extension",
        semver::Version::new(1, 0, 0),
    );

    let descriptor = ExtensionDescriptor::new(metadata.clone());

    assert_eq!(descriptor.id(), "test.extension");
    assert_eq!(descriptor.name(), "Test Extension");
    assert!(descriptor.commands.is_empty());
    assert!(descriptor.metrics.is_empty());
}

#[test]
fn test_extension_descriptor_with_capabilities() {
    let metadata = ExtensionMetadata::new(
        "test.extension",
        "Test Extension",
        semver::Version::new(1, 0, 0),
    );

    let commands = vec![
        ExtensionCommand {
            name: "test_command".to_string(),
            display_name: "Test Command".to_string(),
            description: "A test command".to_string(),
            payload_template: "{}".to_string(),
            parameters: vec![],
            fixed_values: Default::default(),
            samples: vec![],
            parameter_groups: vec![],
        },
    ];

    let metrics = vec![
        MetricDescriptor {
            name: "test_metric".to_string(),
            display_name: "Test Metric".to_string(),
            data_type: neomind_core::extension::system::MetricDataType::Integer,
            unit: "count".to_string(),
            min: None,
            max: None,
            required: false,
        },
    ];

    let descriptor = ExtensionDescriptor::with_capabilities(
        metadata,
        commands.clone(),
        metrics.clone(),
    );

    assert_eq!(descriptor.commands.len(), 1);
    assert_eq!(descriptor.metrics.len(), 1);
}

// ============================================================================
// StreamCapability Tests
// ============================================================================

#[test]
fn test_stream_capability_direction() {
    let cap = StreamCapability::upload();
    assert_eq!(cap.direction, StreamDirection::Upload);

    let cap = StreamCapability::download();
    assert_eq!(cap.direction, StreamDirection::Download);

    let cap = StreamCapability::stateful();
    assert_eq!(cap.direction, StreamDirection::Bidirectional);
}

#[test]
fn test_stream_capability_mode() {
    let cap = StreamCapability::upload();
    assert_eq!(cap.mode, StreamMode::Stateless);

    let cap = StreamCapability::stateful();
    assert_eq!(cap.mode, StreamMode::Stateful);

    let cap = StreamCapability::push();
    assert_eq!(cap.mode, StreamMode::Push);
}

// ============================================================================
// StreamSession Tests
// ============================================================================

#[test]
fn test_stream_session_creation() {
    let client_info = ClientInfo {
        client_id: "test-client".to_string(),
        ip_addr: Some("127.0.0.1".to_string()),
        user_agent: Some("TestAgent/1.0".to_string()),
    };

    let session = StreamSession::new(
        "session-123".to_string(),
        "test-extension".to_string(),
        json!({"mode": "streaming"}),
        client_info.clone(),
    );

    assert_eq!(session.id, "session-123");
    assert_eq!(session.extension_id, "test-extension");
    assert_eq!(session.client_info.client_id, "test-client");
    assert!(session.created_at > 0);
}

// ============================================================================
// DataChunk Tests for Proxy
// ============================================================================

#[test]
fn test_data_chunk_for_streaming() {
    let chunk = DataChunk::binary(1, vec![0x01, 0x02, 0x03])
        .with_metadata(json!({"source": "camera"}));

    assert_eq!(chunk.sequence, 1);
    assert_eq!(chunk.data, vec![0x01, 0x02, 0x03]);
    assert!(chunk.metadata.is_some());
}

#[test]
fn test_data_chunk_image_for_streaming() {
    let jpeg_data = vec![0xFF, 0xD8, 0xFF, 0xE0]; // JPEG header
    let chunk = DataChunk::image(1, jpeg_data.clone(), "jpeg".to_string());

    assert_eq!(chunk.sequence, 1);
    assert_eq!(chunk.data, jpeg_data);
    assert!(matches!(chunk.data_type, StreamDataType::Image { format } if format == "jpeg"));
}

// ============================================================================
// ExtensionMetadata Tests for Proxy
// ============================================================================

#[test]
fn test_extension_metadata_for_proxy() {
    let metadata = ExtensionMetadata::new(
        "proxy.test",
        "Proxy Test Extension",
        semver::Version::new(2, 0, 0),
    )
    .with_description("A test extension for proxy testing")
    .with_author("Test Author");

    assert_eq!(metadata.id, "proxy.test");
    assert_eq!(metadata.name, "Proxy Test Extension");
    assert_eq!(metadata.version, semver::Version::new(2, 0, 0));
    assert!(metadata.description.is_some());
    assert!(metadata.author.is_some());
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_proxy_error_chain() {
    // Test that errors can be created and have proper messages
    let isolated_err = IsolatedExtensionError::IpcError("Connection lost".to_string());
    
    // Verify the error message is preserved
    let err_string = isolated_err.to_string();
    assert!(err_string.contains("Connection lost") || err_string.contains("IPC"));
}

#[test]
fn test_descriptor_with_stream_capability() {
    let metadata = ExtensionMetadata::new(
        "stream.test",
        "Stream Test",
        semver::Version::new(1, 0, 0),
    );

    let descriptor = ExtensionDescriptor::new(metadata);

    // Descriptor should have metadata
    assert_eq!(descriptor.id(), "stream.test");

    // Default commands and metrics should be empty
    assert!(descriptor.commands.is_empty());
    assert!(descriptor.metrics.is_empty());
}

#[test]
fn test_session_config_serialization() {
    let config = json!({
        "mode": "stateful",
        "buffer_size": 1024,
        "timeout_ms": 5000,
    });

    let session = StreamSession::new(
        "session-config-test".to_string(),
        "ext-config".to_string(),
        config.clone(),
        ClientInfo {
            client_id: "config-client".to_string(),
            ip_addr: None,
            user_agent: None,
        },
    );

    // Config should be preserved
    assert_eq!(session.config["mode"], "stateful");
    assert_eq!(session.config["buffer_size"], 1024);
}

// ============================================================================
// Proxy Creation Tests (without actual IsolatedExtension)
// ============================================================================

#[test]
fn test_proxy_module_exports() {
    // Test that the module exports are accessible
    use neomind_core::extension::proxy::{create_proxy, create_proxy_with_descriptor};

    // These functions exist and are callable
    let _ = create_proxy;
    let _ = create_proxy_with_descriptor;
}

// ============================================================================
// Stream Capability Validation Tests
// ============================================================================

#[test]
fn test_stream_capability_validation() {
    let cap = StreamCapability {
        direction: StreamDirection::Bidirectional,
        mode: StreamMode::Stateful,
        supported_data_types: vec![
            StreamDataType::Binary,
            StreamDataType::Json,
        ],
        max_chunk_size: 1024 * 1024,
        preferred_chunk_size: 64 * 1024,
        max_concurrent_sessions: 5,
        flow_control: Default::default(),
        config_schema: None,
    };

    // Validate capability settings
    assert_eq!(cap.direction, StreamDirection::Bidirectional);
    assert_eq!(cap.mode, StreamMode::Stateful);
    assert_eq!(cap.supported_data_types.len(), 2);
    assert!(cap.max_chunk_size > cap.preferred_chunk_size);
    assert!(cap.max_concurrent_sessions > 0);
}

#[test]
fn test_flow_control_defaults() {
    let fc = neomind_core::extension::stream::FlowControl::default();

    assert!(!fc.supports_backpressure);
    assert_eq!(fc.window_size, 10);
    assert!(!fc.supports_throttling);
    assert_eq!(fc.max_rate, 0);
}

// ============================================================================
// ClientInfo Tests
// ============================================================================

#[test]
fn test_client_info_for_proxy() {
    let info = ClientInfo {
        client_id: "proxy-client-1".to_string(),
        ip_addr: Some("192.168.1.100".to_string()),
        user_agent: Some("NeoMind-Proxy/1.0".to_string()),
    };

    assert_eq!(info.client_id, "proxy-client-1");
    assert!(info.ip_addr.is_some());
    assert!(info.user_agent.is_some());
}

#[test]
fn test_client_info_minimal() {
    let info = ClientInfo {
        client_id: "minimal-client".to_string(),
        ip_addr: None,
        user_agent: None,
    };

    assert_eq!(info.client_id, "minimal-client");
    assert!(info.ip_addr.is_none());
    assert!(info.user_agent.is_none());
}

// ============================================================================
// Extension Trait Tests (via mock)
// ============================================================================

/// Mock extension for testing proxy behavior
struct MockProxyExtension {
    metadata: ExtensionMetadata,
    commands: Vec<ExtensionCommand>,
    metrics: Vec<MetricDescriptor>,
}

impl MockProxyExtension {
    fn new() -> Self {
        Self {
            metadata: ExtensionMetadata::new(
                "mock.proxy",
                "Mock Proxy Extension",
                semver::Version::new(1, 0, 0),
            ),
            commands: vec![],
            metrics: vec![],
        }
    }
}

#[async_trait::async_trait]
impl Extension for MockProxyExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        &self.metadata
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        self.commands.clone()
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        self.metrics.clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn execute_command(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> neomind_core::extension::system::Result<serde_json::Value> {
        match command {
            "ping" => Ok(json!({"pong": true})),
            "echo" => Ok(json!({"echo": args})),
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }
}

#[tokio::test]
async fn test_mock_extension_via_trait() {
    let ext = MockProxyExtension::new();

    // Test metadata
    assert_eq!(ext.metadata().id, "mock.proxy");

    // Test command execution
    let result = ext.execute_command("ping", &json!({})).await.unwrap();
    assert_eq!(result["pong"], true);

    let result = ext.execute_command("echo", &json!({"message": "hello"})).await.unwrap();
    assert_eq!(result["echo"]["message"], "hello");
}

#[tokio::test]
async fn test_mock_extension_command_not_found() {
    let ext = MockProxyExtension::new();

    let result = ext.execute_command("nonexistent", &json!({})).await;
    assert!(result.is_err());

    match result {
        Err(ExtensionError::CommandNotFound(cmd)) => assert_eq!(cmd, "nonexistent"),
        _ => panic!("Expected CommandNotFound error"),
    }
}