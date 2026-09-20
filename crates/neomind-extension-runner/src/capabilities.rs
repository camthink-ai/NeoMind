//! `capabilities` — split from the former main.rs monolith.

use super::*;

use std::os::raw::c_char;

use serde_json::json;

use tracing::{debug, error};

use neomind_extension_sdk::capability_constants as cap;

/// Allowed capability names that can be invoked through the native bridge.
/// Unknown names are rejected before reaching the host process.
pub(crate) const ALLOWED_CAPABILITIES: &[&str] = &[
    cap::DEVICE_METRICS_READ,
    cap::DEVICE_METRICS_WRITE,
    cap::DEVICE_CONTROL,
    cap::DEVICE_TEMPLATE_REGISTER,
    cap::DEVICE_REGISTER,
    cap::DEVICE_UNREGISTER,
    cap::STORAGE_QUERY,
    cap::EVENT_PUBLISH,
    cap::EVENT_SUBSCRIBE,
    cap::TELEMETRY_HISTORY,
    cap::METRICS_AGGREGATE,
    cap::EXTENSION_CALL,
    cap::AGENT_TRIGGER,
    cap::CHAT_STREAM,
    cap::CHAT_STREAM_CANCEL,
    cap::CHAT_SESSION_OPEN,
    cap::CHAT_SESSION_SEND,
    cap::CHAT_SESSION_CLOSE,
    cap::CHAT_STREAM_CANCEL_TURN,
    cap::RULE_TRIGGER,
];

pub(crate) unsafe extern "C" fn runner_native_capability_invoke(
    input_ptr: *const u8,
    input_len: usize,
) -> *mut c_char {
    let error_json = |message: String| {
        std::ffi::CString::new(
            json!({
                "success": false,
                "error": message,
            })
            .to_string(),
        )
        .unwrap_or_else(|_| {
            std::ffi::CString::new("{\"success\":false,\"error\":\"runner bridge failed\"}")
                .expect("fallback error JSON is valid ASCII")
        })
        .into_raw()
    };

    if input_ptr.is_null() || input_len == 0 {
        return error_json("empty native capability bridge input".to_string());
    }

    // Cap input size to prevent OOM from malicious extensions (1 MB)
    const MAX_CAPABILITY_INPUT: usize = 1024 * 1024;
    if input_len > MAX_CAPABILITY_INPUT {
        return error_json(format!(
            "capability input too large: {} bytes (max {})",
            input_len, MAX_CAPABILITY_INPUT
        ));
    }

    let input_bytes = std::slice::from_raw_parts(input_ptr, input_len);
    let input = match serde_json::from_slice::<serde_json::Value>(input_bytes) {
        Ok(value) => value,
        Err(error) => {
            return error_json(format!("invalid native capability bridge json: {}", error))
        }
    };

    let capability = input
        .get("capability")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let params = input.get("params").cloned().unwrap_or_else(|| json!({}));

    // Validate capability name against allowlist
    if !ALLOWED_CAPABILITIES.contains(&capability) {
        error!(
            capability = %capability,
            "Rejected capability invocation: name not in allowlist"
        );
        return error_json(format!("capability not allowed: {}", capability));
    }

    let Some(ipc_client) = GLOBAL_NATIVE_IPC_CLIENT.get() else {
        return error_json("native capability IPC client is not initialized".to_string());
    };

    let response = ipc_client.invoke(capability, &params);
    std::ffi::CString::new(response.to_string())
        .unwrap_or_else(|_| {
            std::ffi::CString::new(
                "{\"success\":false,\"error\":\"failed to serialize native capability response\"}",
            )
            .expect("fallback error JSON is valid ASCII")
        })
        .into_raw()
}

pub(crate) unsafe extern "C" fn runner_native_capability_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        let _ = std::ffi::CString::from_raw(ptr);
    }
}

// ============================================================================
// Push output buffering — decouples FFI callback from blocking stdout I/O
// ============================================================================

/// Handle capability invocation from WASM extension
///
/// NOTE: This is a simplified implementation that returns mock data.
/// WASM extensions have limited capability access due to sandbox restrictions.
/// For full capability access, use Native extensions in non-isolated mode.
///
/// SYNC: Capability names are imported from context::capabilities
pub(crate) fn handle_capability_invocation(
    capability: &str,
    params: &serde_json::Value,
) -> serde_json::Value {
    debug!(capability = %capability, "Handling capability invocation (WASM mock)");
    match capability {
        // Device capabilities
        cap::DEVICE_METRICS_READ => {
            let device_id = params
                .get("device_id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            json!({
                "success": true,
                "device_id": device_id,
                "metrics": {
                    "temperature": 25.5,
                    "humidity": 65.0,
                    "status": "online",
                },
                "timestamp": chrono::Utc::now().timestamp_millis(),
            })
        }
        cap::DEVICE_METRICS_WRITE => {
            let device_id = params
                .get("device_id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let metric = params
                .get("metric")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            json!({
                "success": true,
                "device_id": device_id,
                "metric": metric,
                "message": "Metric written successfully",
            })
        }
        cap::DEVICE_CONTROL => {
            let device_id = params
                .get("device_id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let command = params
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            json!({
                "success": true,
                "device_id": device_id,
                "command": command,
                "result": "Command executed",
            })
        }

        // Telemetry capabilities
        cap::TELEMETRY_HISTORY => {
            let device_id = params
                .get("device_id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            json!({
                "success": true,
                "device_id": device_id,
                "data": [
                    {"timestamp": chrono::Utc::now().timestamp_millis() - 3600000, "value": 25.0},
                    {"timestamp": chrono::Utc::now().timestamp_millis() - 1800000, "value": 25.5},
                    {"timestamp": chrono::Utc::now().timestamp_millis(), "value": 26.0},
                ],
            })
        }
        cap::METRICS_AGGREGATE => {
            let aggregation = params
                .get("aggregation")
                .and_then(|v| v.as_str())
                .unwrap_or("avg");
            json!({
                "success": true,
                "aggregation": aggregation,
                "value": match aggregation {
                    "avg" => 25.5_f64,
                    "min" => 24.0_f64,
                    "max" => 27.0_f64,
                    "sum" => 153.0_f64,
                    "count" => 6.0_f64,
                    _ => 0.0_f64,
                },
            })
        }

        // Event capabilities
        cap::EVENT_PUBLISH => {
            let event_type = params
                .get("event_type")
                .and_then(|v| v.as_str())
                .unwrap_or("custom");
            json!({
                "success": true,
                "event_type": event_type,
                "message": "Event published",
            })
        }
        cap::EVENT_SUBSCRIBE => {
            let event_type = params
                .get("event_type")
                .and_then(|v| v.as_str())
                .unwrap_or("all");
            let subscription_id = uuid::Uuid::new_v4().to_string();
            json!({
                "success": true,
                "subscription_id": subscription_id,
                "event_type": event_type,
            })
        }

        // Extension capabilities
        cap::EXTENSION_CALL => {
            let extension_id = params
                .get("extension_id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let command = params
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            json!({
                "success": true,
                "extension_id": extension_id,
                "command": command,
                "result": "Extension call completed",
            })
        }

        // Agent capabilities
        cap::AGENT_TRIGGER => {
            let agent_id = params
                .get("agent_id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            // Check for action commands
            if let Some(action) = params.get("action").and_then(|v| v.as_str()) {
                match action {
                    "status" => json!({
                        "success": true,
                        "agent_id": agent_id,
                        "status": "idle",
                    }),
                    "list" => json!({
                        "success": true,
                        "agents": [
                            {"id": "analyzer-agent", "name": "Data Analyzer", "status": "idle"},
                        ],
                    }),
                    _ => json!({
                        "success": false,
                        "error": format!("Unknown action: {}", action),
                    }),
                }
            } else {
                json!({
                    "success": true,
                    "agent_id": agent_id,
                    "result": "Agent triggered successfully",
                })
            }
        }

        // Rule capabilities
        cap::RULE_TRIGGER => {
            let rule_id = params
                .get("rule_id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            // Check for action commands
            if let Some(action) = params.get("action").and_then(|v| v.as_str()) {
                match action {
                    "list" => json!({
                        "success": true,
                        "rules": [
                            {"id": "alert-threshold", "name": "Temperature Alert", "enabled": true},
                        ],
                    }),
                    _ => json!({
                        "success": false,
                        "error": format!("Unknown action: {}", action),
                    }),
                }
            } else {
                json!({
                    "success": true,
                    "rule_id": rule_id,
                    "result": "Rule triggered successfully",
                })
            }
        }

        // Storage capability
        cap::STORAGE_QUERY => {
            json!({
                "success": true,
                "results": [],
                "message": "Storage query executed",
            })
        }

        // System capabilities
        "system_timestamp" => {
            json!({
                "success": true,
                "timestamp_ms": chrono::Utc::now().timestamp_millis(),
            })
        }

        // Unknown capability
        _ => json!({
            "success": false,
            "error": format!("Unknown capability: {}", capability),
        }),
    }
}
