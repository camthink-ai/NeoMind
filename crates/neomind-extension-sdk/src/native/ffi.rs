//! FFI utilities for Native extensions
//!
//! This module provides FFI-related utilities and helpers.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

/// Convert a C string pointer to a Rust string
///
/// # Safety
///
/// The pointer must be valid and point to a null-terminated string.
pub unsafe fn c_str_to_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    CStr::from_ptr(ptr).to_str().ok().map(|s| s.to_string())
}

/// Convert a Rust string to a C string pointer
///
/// # Safety
///
/// The returned pointer must not be used after the original CString is dropped.
pub fn string_to_c_str(s: &str) -> *const c_char {
    match CString::new(s) {
        Ok(cstr) => cstr.as_ptr(),
        Err(_) => std::ptr::null(),
    }
}

/// Safe wrapper for FFI calls that may panic
///
/// This function catches panics in FFI calls and converts them to errors.
pub fn safe_ffi_call<T, E, F>(fn_name: &str, f: F) -> Result<T, E>
where
    F: FnOnce() -> Result<T, E> + std::panic::UnwindSafe,
    E: From<String>,
{
    match std::panic::catch_unwind(f) {
        Ok(result) => result,
        Err(panic_payload) => {
            let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "Unknown panic in extension FFI".to_string()
            };
            Err(E::from(format!("Extension panicked in {}: {}", fn_name, msg)))
        }
    }
}

/// Create a C-compatible metadata structure
#[macro_export]
macro_rules! create_c_metadata {
    ($id:literal, $name:literal, $version:literal, $metric_count:expr, $command_count:expr) => {{
        use std::ffi::CStr;

        let id = CStr::from_bytes_with_nul(concat!($id, "\0").as_bytes()).unwrap();
        let name = CStr::from_bytes_with_nul(concat!($name, "\0").as_bytes()).unwrap();
        let version = CStr::from_bytes_with_nul(concat!($version, "\0").as_bytes()).unwrap();

        $crate::CExtensionMetadata {
            abi_version: $crate::NEW_ABI_VERSION,
            id: id.as_ptr(),
            name: name.as_ptr(),
            version: version.as_ptr(),
            description: std::ptr::null(),
            author: std::ptr::null(),
            metric_count: $metric_count,
            command_count: $command_count,
        }
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_str_conversion() {
        let rust_str = "hello";
        let c_ptr = string_to_c_str(rust_str);
        unsafe {
            let converted = c_str_to_string(c_ptr);
            assert_eq!(converted, Some(rust_str.to_string()));
        }
    }
}
