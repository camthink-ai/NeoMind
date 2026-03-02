//! WASM-specific extension utilities
//!
//! This module provides utilities for WASM extensions.
//!
//! # Memory Layout
//!
//! ```text
//! WASM Linear Memory:
//! ┌─────────────────────────────────────────────────────────┐
//! │ 0x0000 - 0xFFFF  │ Stack / Heap (managed by WASM runtime) │
//! ├─────────────────────────────────────────────────────────┤
//! │ 0x10000 (64KB)   │ Result Buffer Start                   │
//! │                  │ Used for returning JSON data to host   │
//! │ 0x1FFFF (128KB)  │ Result Buffer End (64KB max)          │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! # Host Functions Available
//!
//! - `host_http_request(method, url)` - Make HTTP requests
//! - `host_log(level, message)` - Log messages
//! - `host_store_metric(name, value)` - Store metric values
//! - `host_device_read(device_id, metric)` - Read device data
//! - `host_device_write(device_id, command, params)` - Write to device

pub mod bindings;
pub mod types;

pub use bindings::*;
pub use types::*;

/// Result buffer offset for WASM memory layout (64KB)
pub const RESULT_OFFSET: usize = 65536;

/// Maximum result size for WASM (64KB)
pub const RESULT_MAX_LEN: usize = 65536;

/// Input buffer offset for WASM memory layout (128KB)
pub const INPUT_OFFSET: usize = 131072;

/// Maximum input size for WASM (64KB)
pub const INPUT_MAX_LEN: usize = 65536;

/// Write a result string to WASM memory at the result offset
///
/// Returns the length of the written data.
/// Data is written to RESULT_OFFSET with a null terminator.
pub fn write_result(result: &str) -> i32 {
    let bytes = result.as_bytes();
    let write_len = bytes.len().min(RESULT_MAX_LEN - 1);

    unsafe {
        let dest = RESULT_OFFSET as *mut u8;
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), dest, write_len);
        *dest.add(write_len) = 0;  // Null terminator
    }

    write_len as i32
}

/// Write bytes to WASM memory at a specific offset
///
/// # Safety
///
/// The offset + len must not exceed WASM memory bounds.
pub fn write_bytes_at_offset(offset: usize, data: &[u8]) -> i32 {
    let write_len = data.len().min(RESULT_MAX_LEN - 1);
    
    unsafe {
        let dest = offset as *mut u8;
        core::ptr::copy_nonoverlapping(data.as_ptr(), dest, write_len);
        *dest.add(write_len) = 0;  // Null terminator
    }
    
    write_len as i32
}

/// Read bytes from WASM memory
///
/// # Safety
///
/// The pointer must be valid and the length must be correct.
pub unsafe fn read_bytes_from_memory(ptr: i32, len: i32) -> Vec<u8> {
    if ptr == 0 || len <= 0 {
        return Vec::new();
    }
    let slice = core::slice::from_raw_parts(ptr as *const u8, len as usize);
    slice.to_vec()
}

/// Read a string from WASM memory
///
/// # Safety
///
/// The pointer must be valid and the length must be correct.
pub unsafe fn read_string_from_memory(ptr: i32, len: i32) -> String {
    let bytes = read_bytes_from_memory(ptr, len);
    String::from_utf8_lossy(&bytes).to_string()
}

/// Read JSON from WASM memory at the result offset
///
/// This reads data that was written by the WASM extension.
pub fn read_result_json() -> Option<serde_json::Value> {
    unsafe {
        // Find null terminator
        let start = RESULT_OFFSET as *const u8;
        let mut end = 0usize;
        while end < RESULT_MAX_LEN {
            if *start.add(end) == 0 {
                break;
            }
            end += 1;
        }
        
        if end == 0 {
            return None;
        }
        
        let slice = core::slice::from_raw_parts(start, end);
        let json_str = core::str::from_utf8(slice).ok()?;
        serde_json::from_str(json_str).ok()
    }
}

/// Parse JSON from bytes
pub fn parse_json(bytes: &[u8]) -> Result<serde_json::Value, String> {
    let json_str = core::str::from_utf8(bytes)
        .map_err(|e| format!("Invalid UTF-8: {}", e))?;
    serde_json::from_str(json_str)
        .map_err(|e| format!("Invalid JSON: {}", e))
}

/// Create an error JSON response
pub fn error_response(error: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "success": false,
        "error": error
    })).unwrap_or_else(|_| r#"{"success":false,"error":"JSON error"}"#.to_string())
}

/// Encode binary data as base64
/// Uses the base64 crate when available
pub fn encode_base64(data: &[u8]) -> String {
    use base64::{Engine, engine::general_purpose::STANDARD};
    STANDARD.encode(data)
}

/// Create a success JSON response
pub fn success_response(result: serde_json::Value) -> String {
    serde_json::to_string(&serde_json::json!({
        "success": true,
        "result": result
    })).unwrap_or_else(|_| r#"{"success":false,"error":"JSON error"}"#.to_string())
}

/// Get current timestamp in milliseconds (Unix epoch)
pub fn current_timestamp_ms() -> i64 {
    // For WASM without wasm-bindgen, use a simple implementation
    // The host can provide this via a host function if needed
    0
}
