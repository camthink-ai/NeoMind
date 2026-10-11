//! `capabilities` — split from the former main.rs monolith.

use super::*;

use std::os::raw::c_char;

use serde_json::json;

use tracing::error;

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
