//! IPC routing for capability invocation and message dispatch
//!
//! A background thread reads all stdin messages and routes them to:
//! 1. Pending capability requests (via PENDING_REQUESTS)
//! 2. Main event queue (via EVENT_TX)

use std::io::Read;
use std::panic::UnwindSafe;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;

use dashmap::DashMap;
use serde_json::json;
use tracing::{debug, error, trace, warn};

use neomind_extension_sdk::{IpcMessage, IpcResponse};

type ResponseSender = Sender<IpcResponse>;

/// Pending capability requests: request_id -> response sender
static PENDING_REQUESTS: std::sync::OnceLock<DashMap<u64, ResponseSender>> =
    std::sync::OnceLock::new();

/// Channel-based event queue for main loop (replaces polling)
static EVENT_TX: std::sync::OnceLock<tokio::sync::mpsc::Sender<IpcMessage>> =
    std::sync::OnceLock::new();

/// Global mutex for stdout writes — prevents interleaved frames from
/// `send_response()` and the push-output callback running concurrently.
pub(crate) static STDOUT_WRITE_MUTEX: Mutex<()> = Mutex::new(());

/// Wrap an FFI call in `catch_unwind` so an extension panic does not
/// abort the runner process. Returns the value or an error string.
pub(crate) fn safe_ffi_call<F, T>(label: &str, f: F) -> Result<T, String>
where
    F: FnOnce() -> T + UnwindSafe,
{
    std::panic::catch_unwind(f).map_err(|payload| {
        let msg = if let Some(s) = payload.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic".to_string()
        };
        error!(label = label, panic = %msg, "Extension FFI call panicked");
        format!("Extension panicked in {}: {}", label, msg)
    })
}

/// Maximum time an FFI call is allowed to run before we consider it hung.
/// Default 120s. Override via `NEOMIND_FFI_TIMEOUT_SECS` env var.
fn ffi_call_timeout() -> std::time::Duration {
    static TIMEOUT: std::sync::OnceLock<std::time::Duration> = std::sync::OnceLock::new();
    *TIMEOUT.get_or_init(|| {
        let secs = std::env::var("NEOMIND_FFI_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(120);
        std::time::Duration::from_secs(secs.max(10))
    })
}

/// Wrapper that makes a raw pointer `Send` for crossing thread boundaries.
/// SAFETY: The pointer is only moved between threads, not accessed concurrently.
pub(crate) struct SendPtr<T>(pub *mut T);
unsafe impl<T> Send for SendPtr<T> {}

/// Like [`safe_ffi_call`] but also enforces a wall-clock timeout.
///
/// The FFI closure is executed on a dedicated thread. If it does not return
/// within the configured timeout, the thread is **detached** (it cannot be
/// safely cancelled) and an error is returned. The runner remains alive but
/// the hung thread will eventually finish or be cleaned up when the process
/// exits.
pub(crate) fn safe_ffi_call_with_timeout<F, T>(label: &str, f: F) -> Result<T, String>
where
    F: FnOnce() -> T + UnwindSafe + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    let builder = std::thread::Builder::new().name(format!("ffi-{label}"));
    builder
        .spawn(move || {
            let result = safe_ffi_call("ffi-worker", f);
            let _ = tx.send(result);
        })
        .map_err(|e| format!("Failed to spawn FFI thread for {}: {}", label, e))?;

    let timeout = ffi_call_timeout();
    match rx.recv_timeout(timeout) {
        Ok(inner_result) => inner_result,
        Err(_) => {
            error!(
                label = label,
                timeout_secs = timeout.as_secs(),
                "FFI call timed out — extension may be stuck, thread detached"
            );
            Err(format!(
                "FFI call timed out after {}s in {}",
                timeout.as_secs(),
                label
            ))
        }
    }
}

pub(crate) fn get_pending_requests() -> &'static DashMap<u64, ResponseSender> {
    PENDING_REQUESTS.get_or_init(DashMap::new)
}

/// Create the event channel and return the receiver (call once at startup)
pub(crate) fn create_event_channel() -> tokio::sync::mpsc::Receiver<IpcMessage> {
    let (tx, rx) = tokio::sync::mpsc::channel(128);
    EVENT_TX.set(tx).expect("event channel already initialized");
    rx
}

/// Register a pending request and return the response receiver
pub(crate) fn register_pending_request(request_id: u64) -> Receiver<IpcResponse> {
    let (tx, rx) = channel();
    get_pending_requests().insert(request_id, tx);
    rx
}

/// Complete a pending request with the response
pub(crate) fn complete_pending_request(request_id: u64, response: IpcResponse) {
    if let Some((_, tx)) = get_pending_requests().remove(&request_id) {
        let _ = tx.send(response);
    }
}

/// Push an event to the channel for main loop processing
pub(crate) fn push_event(message: IpcMessage) {
    if let Some(tx) = EVENT_TX.get() {
        // Use try_send with backpressure handling - if channel is full, log a warning
        if let Err(e) = tx.try_send(message) {
            // Channel full or closed - drop the event with a warning
            // This is safer than blocking the stdin reader thread
            tracing::warn!("IPC event channel full or closed, dropping event: {}", e);
        }
    } else {
        tracing::error!("IPC event channel not initialized, dropping message - event channel must be created before stdin reader starts");
    }
}

/// Start the stdin reader thread
/// This thread reads all messages from stdin and routes them appropriately
pub(crate) fn start_stdin_reader() -> std::thread::JoinHandle<()> {
    std::thread::spawn(|| {
        debug!("StdinReader started");

        let mut consecutive_errors = 0u32;
        const MAX_CONSECUTIVE_ERRORS: u32 = 5;

        loop {
            // Read length prefix
            let mut len_bytes = [0u8; 4];
            match std::io::stdin().read_exact(&mut len_bytes) {
                Ok(_) => {
                    consecutive_errors = 0;
                }
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    debug!("Stdin closed");
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                    // Retry on interrupt (signal received during read)
                    continue;
                }
                Err(e) => {
                    consecutive_errors += 1;
                    warn!(
                        consecutive_errors,
                        max = MAX_CONSECUTIVE_ERRORS,
                        "Error reading length: {e}"
                    );
                    if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                        error!("Too many consecutive stdin errors, giving up");
                        break;
                    }
                    // Brief backoff before retry
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    continue;
                }
            }

            let len = u32::from_le_bytes(len_bytes) as usize;
            let max_size = std::env::var("NEOMIND_IPC_MAX_SIZE")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(10 * 1024 * 1024);
            if len > max_size {
                warn!(len, max_size, "Message too large, draining");
                // Drain the oversized payload in chunks to keep stdin aligned
                let mut remaining = len;
                let mut drain_buf = [0u8; 4096];
                while remaining > 0 {
                    let to_read = remaining.min(drain_buf.len());
                    if std::io::stdin()
                        .read_exact(&mut drain_buf[..to_read])
                        .is_err()
                    {
                        debug!("Stdin closed while draining oversized message");
                        break;
                    }
                    remaining -= to_read;
                }
                continue;
            }

            // Read payload
            let mut payload = vec![0u8; len];
            if let Err(e) = std::io::stdin().read_exact(&mut payload) {
                warn!("Error reading payload: {e}");
                continue;
            }

            // Parse message
            let message: IpcMessage = match IpcMessage::from_bytes(&payload) {
                Ok(m) => m,
                Err(e) => {
                    warn!("Failed to parse IPC message: {e}");
                    continue;
                }
            };

            // Route message
            match &message {
                IpcMessage::CapabilityResult { request_id, .. } => {
                    // Convert to IpcResponse and route to waiting invoke()
                    let response = IpcResponse::CapabilityResult {
                        request_id: *request_id,
                        result: match message {
                            IpcMessage::CapabilityResult { ref result, .. } => result.clone(),
                            _ => json!({}),
                        },
                        error: match message {
                            IpcMessage::CapabilityResult { ref error, .. } => error.clone(),
                            _ => None,
                        },
                    };
                    complete_pending_request(*request_id, response);
                    trace!(request_id, "Routed CapabilityResult");
                }
                _ => {
                    // Push to event queue for main loop
                    push_event(message);
                }
            }
        }

        debug!("StdinReader exiting");
    })
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use neomind_extension_sdk::{
        ErrorKind, IpcMessage, IpcResponse, StreamClientInfo, StreamDataChunk,
    };
    use serde_json::json;

    // ------------------------------------------------------------------------
    // Safe FFI Call Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_safe_ffi_call_success() {
        let result = safe_ffi_call("test_label", || 42);
        assert_eq!(result, Ok(42));
    }

    #[test]
    fn test_safe_ffi_call_panic_with_string() {
        let result = safe_ffi_call("test_panic", || {
            panic!("test panic message");
        });
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("test panic message"));
        assert!(err.contains("test_panic"));
    }

    #[test]
    fn test_safe_ffi_call_panic_with_str() {
        let result = safe_ffi_call("test_panic_str", || {
            panic!("static panic");
        });
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("static panic"));
    }

    #[test]
    fn test_safe_ffi_call_panic_unknown() {
        let result: Result<i32, String> = safe_ffi_call("test_unknown", || {
            std::panic::panic_any(42i32);
        });
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("unknown panic"));
    }

    // ------------------------------------------------------------------------
    // Pending Requests Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_register_and_complete_pending_request() {
        let request_id = 12345u64;
        let rx = register_pending_request(request_id);

        // Complete the request
        let response = IpcResponse::Success {
            request_id,
            data: json!({"result": "ok"}),
        };
        complete_pending_request(request_id, response);

        // Verify response was received
        let received = rx.recv().unwrap();
        match received {
            IpcResponse::Success {
                request_id: rid,
                data,
            } => {
                assert_eq!(rid, request_id);
                assert_eq!(data["result"], "ok");
            }
            _ => panic!("Expected Success response"),
        }
    }

    #[test]
    fn test_complete_nonexistent_request() {
        // Should not panic when completing a non-existent request
        let response = IpcResponse::Success {
            request_id: 99999,
            data: json!(null),
        };
        complete_pending_request(99999, response); // No-op, should not panic
    }

    #[test]
    fn test_multiple_pending_requests() {
        let rx1 = register_pending_request(1);
        let rx2 = register_pending_request(2);
        let rx3 = register_pending_request(3);

        // Complete in reverse order
        complete_pending_request(
            3,
            IpcResponse::Success {
                request_id: 3,
                data: json!("three"),
            },
        );
        complete_pending_request(
            1,
            IpcResponse::Success {
                request_id: 1,
                data: json!("one"),
            },
        );
        complete_pending_request(
            2,
            IpcResponse::Success {
                request_id: 2,
                data: json!("two"),
            },
        );

        // Verify each receiver got the correct response
        let resp1 = rx1.recv().unwrap();
        let resp2 = rx2.recv().unwrap();
        let resp3 = rx3.recv().unwrap();

        match resp1 {
            IpcResponse::Success { data, .. } => assert_eq!(data, json!("one")),
            _ => panic!("Expected Success"),
        }
        match resp2 {
            IpcResponse::Success { data, .. } => assert_eq!(data, json!("two")),
            _ => panic!("Expected Success"),
        }
        match resp3 {
            IpcResponse::Success { data, .. } => assert_eq!(data, json!("three")),
            _ => panic!("Expected Success"),
        }
    }

    #[test]
    fn test_register_same_request_id_twice() {
        let request_id = 42u64;
        let _rx1 = register_pending_request(request_id);

        // Registering the same request_id again should replace the previous one
        let rx2 = register_pending_request(request_id);

        let response = IpcResponse::Success {
            request_id,
            data: json!("second"),
        };
        complete_pending_request(request_id, response);

        // Only the second receiver should get the response
        let received = rx2.recv().unwrap();
        match received {
            IpcResponse::Success { data, .. } => assert_eq!(data, json!("second")),
            _ => panic!("Expected Success"),
        }
    }

    // ------------------------------------------------------------------------
    // Event Channel Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_create_and_push_event() {
        let mut rx = create_event_channel();

        let msg = IpcMessage::Ping {
            request_id: 0,
            timestamp: 1234567890,
        };
        push_event(msg.clone());

        let received = rx.blocking_recv().unwrap();
        match received {
            IpcMessage::Ping { timestamp, .. } => {
                assert_eq!(timestamp, 1234567890);
            }
            _ => panic!("Expected Ping message"),
        }
    }

    #[test]
    fn test_push_event_without_channel() {
        // Push events when channel is initialized should not panic
        // Note: We can't test this fully without create_event_channel() which
        // can only be called once per process. The basic create_and_push_event
        // test covers the happy path.
        push_event(IpcMessage::Shutdown);
        // If we get here without panicking, the test passes
    }

    // ------------------------------------------------------------------------
    // IPC Message Serialization Roundtrip Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_ipc_message_init_roundtrip() {
        let msg = IpcMessage::Init {
            config: json!({"key": "value", "number": 42}),
        };

        let bytes = msg.to_bytes().unwrap();
        let parsed = IpcMessage::from_bytes(&bytes).unwrap();

        match parsed {
            IpcMessage::Init { config } => {
                assert_eq!(config["key"], "value");
                assert_eq!(config["number"], 42);
            }
            _ => panic!("Expected Init"),
        }
    }

    #[test]
    fn test_ipc_message_execute_command_roundtrip() {
        let msg = IpcMessage::ExecuteCommand {
            command: "test_cmd".to_string(),
            args: json!({"arg1": "val1"}),
            request_id: 100,
        };

        let bytes = msg.to_bytes().unwrap();
        let parsed = IpcMessage::from_bytes(&bytes).unwrap();

        match parsed {
            IpcMessage::ExecuteCommand {
                command,
                args,
                request_id,
            } => {
                assert_eq!(command, "test_cmd");
                assert_eq!(args["arg1"], "val1");
                assert_eq!(request_id, 100);
            }
            _ => panic!("Expected ExecuteCommand"),
        }
    }

    #[test]
    fn test_ipc_message_produce_metrics_roundtrip() {
        let msg = IpcMessage::ProduceMetrics { request_id: 200 };

        let bytes = msg.to_bytes().unwrap();
        let parsed = IpcMessage::from_bytes(&bytes).unwrap();

        match parsed {
            IpcMessage::ProduceMetrics { request_id } => {
                assert_eq!(request_id, 200);
            }
            _ => panic!("Expected ProduceMetrics"),
        }
    }

    #[test]
    fn test_ipc_message_health_check_roundtrip() {
        let msg = IpcMessage::HealthCheck { request_id: 300 };

        let bytes = msg.to_bytes().unwrap();
        let parsed = IpcMessage::from_bytes(&bytes).unwrap();

        match parsed {
            IpcMessage::HealthCheck { request_id } => {
                assert_eq!(request_id, 300);
            }
            _ => panic!("Expected HealthCheck"),
        }
    }

    #[test]
    fn test_ipc_message_get_metadata_roundtrip() {
        let msg = IpcMessage::GetMetadata { request_id: 400 };

        let bytes = msg.to_bytes().unwrap();
        let parsed = IpcMessage::from_bytes(&bytes).unwrap();

        match parsed {
            IpcMessage::GetMetadata { request_id } => {
                assert_eq!(request_id, 400);
            }
            _ => panic!("Expected GetMetadata"),
        }
    }

    #[test]
    fn test_ipc_message_shutdown_roundtrip() {
        let msg = IpcMessage::Shutdown;

        let bytes = msg.to_bytes().unwrap();
        let parsed = IpcMessage::from_bytes(&bytes).unwrap();

        assert!(matches!(parsed, IpcMessage::Shutdown));
    }

    #[test]
    fn test_ipc_message_ping_roundtrip() {
        let msg = IpcMessage::Ping {
            request_id: 0,
            timestamp: 9876543210,
        };

        let bytes = msg.to_bytes().unwrap();
        let parsed = IpcMessage::from_bytes(&bytes).unwrap();

        match parsed {
            IpcMessage::Ping { timestamp, .. } => {
                assert_eq!(timestamp, 9876543210);
            }
            _ => panic!("Expected Ping"),
        }
    }

    #[test]
    fn test_ipc_message_config_update_roundtrip() {
        let msg = IpcMessage::ConfigUpdate {
            config: json!({"new_setting": true}),
        };

        let bytes = msg.to_bytes().unwrap();
        let parsed = IpcMessage::from_bytes(&bytes).unwrap();

        match parsed {
            IpcMessage::ConfigUpdate { config } => {
                assert_eq!(config["new_setting"], true);
            }
            _ => panic!("Expected ConfigUpdate"),
        }
    }

    #[test]
    fn test_ipc_message_capability_result_roundtrip() {
        let msg = IpcMessage::CapabilityResult {
            request_id: 500,
            result: json!({"status": "success"}),
            error: None,
        };

        let bytes = msg.to_bytes().unwrap();
        let parsed = IpcMessage::from_bytes(&bytes).unwrap();

        match parsed {
            IpcMessage::CapabilityResult {
                request_id,
                result,
                error,
            } => {
                assert_eq!(request_id, 500);
                assert_eq!(result["status"], "success");
                assert!(error.is_none());
            }
            _ => panic!("Expected CapabilityResult"),
        }
    }

    #[test]
    fn test_ipc_message_capability_result_with_error() {
        let msg = IpcMessage::CapabilityResult {
            request_id: 501,
            result: json!(null),
            error: Some("Operation failed".to_string()),
        };

        let bytes = msg.to_bytes().unwrap();
        let parsed = IpcMessage::from_bytes(&bytes).unwrap();

        match parsed {
            IpcMessage::CapabilityResult {
                request_id, error, ..
            } => {
                assert_eq!(request_id, 501);
                assert_eq!(error.as_deref(), Some("Operation failed"));
            }
            _ => panic!("Expected CapabilityResult"),
        }
    }

    #[test]
    fn test_ipc_message_invoke_capability_roundtrip() {
        let msg = IpcMessage::InvokeCapability {
            request_id: 600,
            capability: "device_read".to_string(),
            params: json!({"device": "sensor1"}),
        };

        let bytes = msg.to_bytes().unwrap();
        let parsed = IpcMessage::from_bytes(&bytes).unwrap();

        match parsed {
            IpcMessage::InvokeCapability {
                request_id,
                capability,
                params,
            } => {
                assert_eq!(request_id, 600);
                assert_eq!(capability, "device_read");
                assert_eq!(params["device"], "sensor1");
            }
            _ => panic!("Expected InvokeCapability"),
        }
    }

    #[test]
    fn test_ipc_message_stream_session_init_roundtrip() {
        let msg = IpcMessage::InitStreamSession {
            request_id: 700,
            session_id: "session-123".to_string(),
            extension_id: "ext-456".to_string(),
            config: json!({"setting": "value"}),
            client_info: StreamClientInfo {
                client_id: "client-1".to_string(),
                ip_addr: Some("127.0.0.1".to_string()),
                user_agent: Some("test-agent".to_string()),
            },
        };

        let bytes = msg.to_bytes().unwrap();
        let parsed = IpcMessage::from_bytes(&bytes).unwrap();

        match parsed {
            IpcMessage::InitStreamSession {
                request_id,
                session_id,
                extension_id,
                config,
                client_info,
            } => {
                assert_eq!(request_id, 700);
                assert_eq!(session_id, "session-123");
                assert_eq!(extension_id, "ext-456");
                assert_eq!(config["setting"], "value");
                assert_eq!(client_info.client_id, "client-1");
            }
            _ => panic!("Expected InitStreamSession"),
        }
    }

    #[test]
    fn test_ipc_message_process_stream_chunk_roundtrip() {
        let msg = IpcMessage::ProcessStreamChunk {
            request_id: 800,
            session_id: "session-789".to_string(),
            chunk: StreamDataChunk {
                sequence: 1,
                data: vec![1, 2, 3, 4],
                data_type: "application/octet-stream".to_string(),
                timestamp: 1234567890,
                is_last: false,
            },
        };

        let bytes = msg.to_bytes().unwrap();
        let parsed = IpcMessage::from_bytes(&bytes).unwrap();

        match parsed {
            IpcMessage::ProcessStreamChunk {
                request_id,
                session_id,
                chunk,
            } => {
                assert_eq!(request_id, 800);
                assert_eq!(session_id, "session-789");
                assert_eq!(chunk.sequence, 1);
                assert_eq!(chunk.data, vec![1, 2, 3, 4]);
            }
            _ => panic!("Expected ProcessStreamChunk"),
        }
    }

    // ------------------------------------------------------------------------
    // IPC Response Serialization Roundtrip Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_ipc_response_success_roundtrip() {
        let resp = IpcResponse::Success {
            request_id: 10,
            data: json!({"output": "result"}),
        };

        let bytes = resp.to_bytes().unwrap();
        let parsed = IpcResponse::from_bytes(&bytes).unwrap();

        match parsed {
            IpcResponse::Success { request_id, data } => {
                assert_eq!(request_id, 10);
                assert_eq!(data["output"], "result");
            }
            _ => panic!("Expected Success"),
        }
    }

    #[test]
    fn test_ipc_response_error_roundtrip() {
        let resp = IpcResponse::Error {
            request_id: 20,
            error: "Test error".to_string(),
            kind: ErrorKind::InvalidArguments,
        };

        let bytes = resp.to_bytes().unwrap();
        let parsed = IpcResponse::from_bytes(&bytes).unwrap();

        match parsed {
            IpcResponse::Error {
                request_id,
                error,
                kind,
            } => {
                assert_eq!(request_id, 20);
                assert_eq!(error, "Test error");
                assert_eq!(kind, ErrorKind::InvalidArguments);
            }
            _ => panic!("Expected Error"),
        }
    }

    #[test]
    fn test_ipc_response_metrics_roundtrip() {
        use neomind_extension_sdk::{ExtensionMetricValue, MetricValue};

        let resp = IpcResponse::Metrics {
            request_id: 30,
            metrics: vec![ExtensionMetricValue {
                name: "temp".to_string(),
                value: MetricValue::Float(25.5),
                timestamp: 1234567890,
            }],
        };

        let bytes = resp.to_bytes().unwrap();
        let parsed = IpcResponse::from_bytes(&bytes).unwrap();

        match parsed {
            IpcResponse::Metrics {
                request_id,
                metrics,
            } => {
                assert_eq!(request_id, 30);
                assert_eq!(metrics.len(), 1);
                assert_eq!(metrics[0].name, "temp");
            }
            _ => panic!("Expected Metrics"),
        }
    }

    #[test]
    fn test_ipc_response_health_roundtrip() {
        let resp = IpcResponse::Health {
            request_id: 40,
            healthy: true,
        };

        let bytes = resp.to_bytes().unwrap();
        let parsed = IpcResponse::from_bytes(&bytes).unwrap();

        match parsed {
            IpcResponse::Health {
                request_id,
                healthy,
            } => {
                assert_eq!(request_id, 40);
                assert!(healthy);
            }
            _ => panic!("Expected Health"),
        }
    }

    #[test]
    fn test_ipc_response_pong_roundtrip() {
        let resp = IpcResponse::Pong {
            request_id: 0,
            timestamp: 1111111111,
        };

        let bytes = resp.to_bytes().unwrap();
        let parsed = IpcResponse::from_bytes(&bytes).unwrap();

        match parsed {
            IpcResponse::Pong { timestamp, .. } => {
                assert_eq!(timestamp, 1111111111);
            }
            _ => panic!("Expected Pong"),
        }
    }

    #[test]
    fn test_ipc_response_capability_result_roundtrip() {
        let resp = IpcResponse::CapabilityResult {
            request_id: 50,
            result: json!({"value": 42}),
            error: None,
        };

        let bytes = resp.to_bytes().unwrap();
        let parsed = IpcResponse::from_bytes(&bytes).unwrap();

        match parsed {
            IpcResponse::CapabilityResult {
                request_id,
                result,
                error,
            } => {
                assert_eq!(request_id, 50);
                assert_eq!(result["value"], 42);
                assert!(error.is_none());
            }
            _ => panic!("Expected CapabilityResult"),
        }
    }

    // ------------------------------------------------------------------------
    // Message Routing Logic Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_capability_result_routing() {
        let request_id = 999u64;
        let rx = register_pending_request(request_id);

        // Simulate the routing logic from stdin reader
        let msg = IpcMessage::CapabilityResult {
            request_id,
            result: json!({"data": "test"}),
            error: None,
        };

        match &msg {
            IpcMessage::CapabilityResult { request_id, .. } => {
                let response = IpcResponse::CapabilityResult {
                    request_id: *request_id,
                    result: match msg {
                        IpcMessage::CapabilityResult { ref result, .. } => result.clone(),
                        _ => json!({}),
                    },
                    error: match msg {
                        IpcMessage::CapabilityResult { ref error, .. } => error.clone(),
                        _ => None,
                    },
                };
                complete_pending_request(*request_id, response);
            }
            _ => panic!("Expected CapabilityResult"),
        }

        let received = rx.recv().unwrap();
        match received {
            IpcResponse::CapabilityResult { result, .. } => {
                assert_eq!(result["data"], "test");
            }
            _ => panic!("Expected CapabilityResult response"),
        }
    }

    // ------------------------------------------------------------------------
    // Error Handling Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_ipc_message_invalid_json() {
        let invalid_json = b"{invalid json}";
        let result = IpcMessage::from_bytes(invalid_json);
        assert!(result.is_err());
    }

    #[test]
    fn test_ipc_message_empty_payload() {
        let empty = b"";
        let result = IpcMessage::from_bytes(empty);
        assert!(result.is_err());
    }

    #[test]
    fn test_ipc_message_missing_required_field() {
        // Missing required fields in ExecuteCommand
        let incomplete = r#"{"ExecuteCommand":{"command":"test"}}"#;
        let result = IpcMessage::from_bytes(incomplete.as_bytes());
        // This should fail because args and request_id are missing
        assert!(result.is_err());
    }

    #[test]
    fn test_ipc_response_invalid_json() {
        let invalid_json = b"{invalid response}";
        let result = IpcResponse::from_bytes(invalid_json);
        assert!(result.is_err());
    }

    #[test]
    fn test_ipc_message_unknown_variant() {
        // JSON with a non-existent variant
        let unknown = r#"{"UnknownVariant":{}}"#;
        let result = IpcMessage::from_bytes(unknown.as_bytes());
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------------
    // Edge Cases Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_ipc_message_empty_config() {
        let msg = IpcMessage::Init {
            config: json!(null),
        };

        let bytes = msg.to_bytes().unwrap();
        let parsed = IpcMessage::from_bytes(&bytes).unwrap();

        match parsed {
            IpcMessage::Init { config } => {
                assert!(config.is_null());
            }
            _ => panic!("Expected Init"),
        }
    }

    #[test]
    fn test_ipc_message_empty_args() {
        let msg = IpcMessage::ExecuteCommand {
            command: "test".to_string(),
            args: json!(null),
            request_id: 1,
        };

        let bytes = msg.to_bytes().unwrap();
        let parsed = IpcMessage::from_bytes(&bytes).unwrap();

        match parsed {
            IpcMessage::ExecuteCommand { args, .. } => {
                assert!(args.is_null());
            }
            _ => panic!("Expected ExecuteCommand"),
        }
    }

    #[test]
    fn test_ipc_message_zero_request_id() {
        let msg = IpcMessage::ExecuteCommand {
            command: "test".to_string(),
            args: json!({}),
            request_id: 0,
        };

        let bytes = msg.to_bytes().unwrap();
        let parsed = IpcMessage::from_bytes(&bytes).unwrap();

        match parsed {
            IpcMessage::ExecuteCommand { request_id, .. } => {
                assert_eq!(request_id, 0);
            }
            _ => panic!("Expected ExecuteCommand"),
        }
    }

    #[test]
    fn test_ipc_message_large_request_id() {
        let large_id = u64::MAX;
        let msg = IpcMessage::ExecuteCommand {
            command: "test".to_string(),
            args: json!({}),
            request_id: large_id,
        };

        let bytes = msg.to_bytes().unwrap();
        let parsed = IpcMessage::from_bytes(&bytes).unwrap();

        match parsed {
            IpcMessage::ExecuteCommand { request_id, .. } => {
                assert_eq!(request_id, large_id);
            }
            _ => panic!("Expected ExecuteCommand"),
        }
    }

    #[test]
    fn test_ipc_message_unicode_strings() {
        let msg = IpcMessage::ExecuteCommand {
            command: "测试命令".to_string(),
            args: json!({"emoji": "🚀", "chinese": "你好"}),
            request_id: 1,
        };

        let bytes = msg.to_bytes().unwrap();
        let parsed = IpcMessage::from_bytes(&bytes).unwrap();

        match parsed {
            IpcMessage::ExecuteCommand { command, args, .. } => {
                assert_eq!(command, "测试命令");
                assert_eq!(args["emoji"], "🚀");
                assert_eq!(args["chinese"], "你好");
            }
            _ => panic!("Expected ExecuteCommand"),
        }
    }

    #[test]
    fn test_ipc_message_complex_nested_json() {
        let complex_json = json!({
            "level1": {
                "level2": {
                    "level3": {
                        "array": [1, 2, 3],
                        "nested": {
                            "key": "value"
                        }
                    }
                }
            },
            "top_array": [{"a": 1}, {"b": 2}]
        });

        let msg = IpcMessage::Init {
            config: complex_json,
        };

        let bytes = msg.to_bytes().unwrap();
        let parsed = IpcMessage::from_bytes(&bytes).unwrap();

        match parsed {
            IpcMessage::Init { config } => {
                assert_eq!(config["level1"]["level2"]["level3"]["array"][2], 3);
                assert_eq!(config["top_array"][1]["b"], 2);
            }
            _ => panic!("Expected Init"),
        }
    }

    #[test]
    fn test_ipc_message_special_characters_in_command() {
        let special = "cmd-with_special.chars:123";
        let msg = IpcMessage::ExecuteCommand {
            command: special.to_string(),
            args: json!({}),
            request_id: 1,
        };

        let bytes = msg.to_bytes().unwrap();
        let parsed = IpcMessage::from_bytes(&bytes).unwrap();

        match parsed {
            IpcMessage::ExecuteCommand { command, .. } => {
                assert_eq!(command, special);
            }
            _ => panic!("Expected ExecuteCommand"),
        }
    }

    #[test]
    fn test_concurrent_request_registration() {
        use std::thread;

        let handles: Vec<_> = (0..10)
            .map(|i| {
                thread::spawn(move || {
                    let rx = register_pending_request(i as u64);
                    complete_pending_request(
                        i as u64,
                        IpcResponse::Success {
                            request_id: i as u64,
                            data: json!(i),
                        },
                    );
                    rx.recv().unwrap()
                })
            })
            .collect();

        for (i, handle) in handles.into_iter().enumerate() {
            let resp = handle.join().unwrap();
            match resp {
                IpcResponse::Success { data, .. } => {
                    assert_eq!(data, json!(i));
                }
                _ => panic!("Expected Success"),
            }
        }
    }

    #[test]
    fn test_ipc_response_request_id_extraction() {
        let test_cases = vec![
            (
                IpcResponse::Success {
                    request_id: 100,
                    data: json!({}),
                },
                Some(100),
            ),
            (
                IpcResponse::Error {
                    request_id: 200,
                    error: "err".to_string(),
                    kind: ErrorKind::Internal,
                },
                Some(200),
            ),
            (
                IpcResponse::Metrics {
                    request_id: 300,
                    metrics: vec![],
                },
                Some(300),
            ),
            (
                IpcResponse::Pong {
                    request_id: 0,
                    timestamp: 123,
                },
                Some(0),
            ),
            (IpcResponse::ShutdownAck, None),
        ];

        for (resp, expected_id) in test_cases {
            assert_eq!(resp.request_id(), expected_id);
        }
    }

    #[test]
    fn test_ipc_response_is_stream_error() {
        let err_resp = IpcResponse::StreamError {
            request_id: 1,
            session_id: "sess".to_string(),
            code: "ERR".to_string(),
            message: "error".to_string(),
        };
        assert!(err_resp.is_stream_error());

        let ok_resp = IpcResponse::Success {
            request_id: 1,
            data: json!({}),
        };
        assert!(!ok_resp.is_stream_error());
    }

    #[test]
    fn test_ipc_response_is_capability_request() {
        let cap_req = IpcResponse::CapabilityRequest {
            request_id: 1,
            capability: "test".to_string(),
            params: json!({}),
        };
        assert!(cap_req.is_capability_request());

        let other = IpcResponse::Pong {
            request_id: 0,
            timestamp: 123,
        };
        assert!(!other.is_capability_request());
    }

    #[test]
    fn test_stdout_mutex_concurrent_access() {
        use std::thread;

        let handles: Vec<_> = (0..5)
            .map(|_| {
                thread::spawn(|| {
                    let _guard = STDOUT_WRITE_MUTEX.lock().unwrap();
                    // Simulate some work
                    std::thread::sleep(std::time::Duration::from_millis(10));
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }
    }
}
