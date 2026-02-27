//! WASM-specific extension utilities
//!
//! This module provides utilities for WASM extensions.

pub mod bindings;
pub mod types;

pub use bindings::*;
pub use types::*;

/// Result buffer offset for WASM memory layout
pub const RESULT_OFFSET: usize = 65536;  // 64KB

/// Maximum result size for WASM
pub const RESULT_MAX_LEN: usize = 65536;  // 64KB

/// Write a result string to WASM memory at the result offset
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
