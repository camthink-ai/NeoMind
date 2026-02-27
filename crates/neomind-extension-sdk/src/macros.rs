//! Macros for NeoMind Extension SDK
//!
//! This module provides macros to simplify extension development.

/// Export FFI functions for an extension type
///
/// This macro generates all necessary FFI exports for a NeoMind extension.
/// The extension type must implement the `Extension` trait.
///
/// # Example
///
/// ```rust,ignore
/// use neomind_extension_sdk::*;
///
/// pub struct MyExtension {
///     // fields...
/// }
///
/// #[async_trait::async_trait]
/// impl Extension for MyExtension {
///     // implement trait methods...
/// }
///
/// // Export FFI functions
/// neomind_export!(MyExtension);
/// ```
#[macro_export]
macro_rules! neomind_export {
    // Simple case: just the type name
    ($extension_type:ty) => {
        $crate::neomind_export_with_constructor!($extension_type, new);
    };
}

/// Export FFI functions with a custom constructor
///
/// # Example
///
/// ```rust,ignore
/// neomind_export_with_constructor!(MyExtension, with_config);
/// ```
#[macro_export]
macro_rules! neomind_export_with_constructor {
    ($extension_type:ty, $constructor:ident) => {
        // Native FFI exports
        #[cfg(not(target_arch = "wasm32"))]
        mod __neomind_ffi_exports {
            use super::*;

            #[no_mangle]
            pub extern "C" fn neomind_extension_abi_version() -> u32 {
                $crate::SDK_ABI_VERSION
            }

            #[no_mangle]
            pub extern "C" fn neomind_extension_metadata() -> $crate::CExtensionMetadata {
                use std::ffi::CStr;

                // Create a temporary instance to get metadata
                let ext = <$extension_type>::$constructor();
                let meta = <$extension_type as $crate::Extension>::metadata(&ext);

                // Convert to C-compatible format
                let id = std::ffi::CString::new(&meta.id[..]).unwrap_or_else(|_| std::ffi::CString::new("unknown").unwrap());
                let name = std::ffi::CString::new(&meta.name[..]).unwrap_or_else(|_| std::ffi::CString::new("Unknown").unwrap());
                let version_str = meta.version.to_string();
                let version = std::ffi::CString::new(&version_str[..]).unwrap_or_else(|_| std::ffi::CString::new("0.0.0").unwrap());
                let description = meta.description.as_ref()
                    .map(|d| std::ffi::CString::new(&d[..]).unwrap_or_else(|_| std::ffi::CString::new("").unwrap()))
                    .unwrap_or_else(|| std::ffi::CString::new("").unwrap());
                let author = meta.author.as_ref()
                    .map(|a| std::ffi::CString::new(&a[..]).unwrap_or_else(|_| std::ffi::CString::new("").unwrap()))
                    .unwrap_or_else(|| std::ffi::CString::new("").unwrap());

                $crate::CExtensionMetadata {
                    abi_version: $crate::SDK_ABI_VERSION,
                    id: id.as_ptr(),
                    name: name.as_ptr(),
                    version: version.as_ptr(),
                    description: description.as_ptr(),
                    author: author.as_ptr(),
                    metric_count: 0,
                    command_count: 0,
                }
            }

            #[no_mangle]
            pub extern "C" fn neomind_extension_create(
                config_json: *const u8,
                config_len: usize,
            ) -> *mut tokio::sync::RwLock<std::boxed::Box<dyn $crate::Extension>> {
                let _config = if config_json.is_null() || config_len == 0 {
                    serde_json::json!({})
                } else {
                    unsafe {
                        let slice = std::slice::from_raw_parts(config_json, config_len);
                        match std::str::from_utf8(slice) {
                            Ok(s) => serde_json::from_str(s).unwrap_or(serde_json::json!({})),
                            Err(_) => serde_json::json!({}),
                        }
                    }
                };

                let extension: $extension_type = <$extension_type>::$constructor();
                let boxed: Box<dyn $crate::Extension> = Box::new(extension);
                Box::into_raw(Box::new(tokio::sync::RwLock::new(boxed)))
            }

            #[no_mangle]
            pub extern "C" fn neomind_extension_destroy(
                ptr: *mut tokio::sync::RwLock<std::boxed::Box<dyn $crate::Extension>>,
            ) {
                if !ptr.is_null() {
                    unsafe {
                        let _ = Box::from_raw(ptr);
                    }
                }
            }
        }

        // WASM exports
        #[cfg(target_arch = "wasm32")]
        mod __neomind_wasm_exports {
            use super::*;

            #[no_mangle]
            pub extern "C" fn neomind_extension_abi_version() -> u32 {
                $crate::SDK_ABI_VERSION
            }

            #[no_mangle]
            pub extern "C" fn extension_init() -> i32 {
                0
            }

            #[no_mangle]
            pub extern "C" fn extension_cleanup() {}

            #[no_mangle]
            pub extern "C" fn get_metadata() -> i32 {
                let ext = <$extension_type>::$constructor();
                let meta = <$extension_type as $crate::Extension>::metadata(&ext);
                let metadata = serde_json::json!({
                    "id": meta.id,
                    "name": meta.name,
                    "version": meta.version,
                    "description": meta.description,
                    "author": meta.author,
                });
                let json = serde_json::to_string(&metadata).unwrap_or_else(|_| "{}".to_string());
                $crate::wasm::write_result(&json)
            }
        }
    };
}

/// Helper macro to create a metric value
#[macro_export]
macro_rules! metric_value {
    ($name:expr, $value:expr, $ts:expr) => {
        $crate::ExtensionMetricValue {
            name: $name.to_string(),
            value: $value,
            timestamp: $ts,
        }
    };

    ($name:expr, $value:expr) => {
        $crate::metric_value!($name, $value, chrono::Utc::now().timestamp_millis())
    };
}

/// Helper macro to create a float metric value
#[macro_export]
macro_rules! metric_float {
    ($name:expr, $value:expr) => {
        $crate::metric_value!($name, $crate::ParamMetricValue::Float($value as f64))
    };
}

/// Helper macro to create an integer metric value
#[macro_export]
macro_rules! metric_int {
    ($name:expr, $value:expr) => {
        $crate::metric_value!($name, $crate::ParamMetricValue::Integer($value as i64))
    };
}

/// Helper macro to create a boolean metric value
#[macro_export]
macro_rules! metric_bool {
    ($name:expr, $value:expr) => {
        $crate::metric_value!($name, $crate::ParamMetricValue::Boolean($value))
    };
}

/// Helper macro to create a string metric value
#[macro_export]
macro_rules! metric_string {
    ($name:expr, $value:expr) => {
        $crate::metric_value!($name, $crate::ParamMetricValue::String($value.to_string()))
    };
}

/// Helper macro to log a message
#[macro_export]
macro_rules! ext_log {
    ($level:ident, $msg:expr) => {
        #[cfg(not(target_arch = "wasm32"))]
        {
            tracing::$level!("[Extension] {}", $msg);
        }
        #[cfg(target_arch = "wasm32")]
        {
            $crate::wasm::log(stringify!($level), &$msg.to_string());
        }
    };
    ($level:ident, $fmt:expr, $($arg:expr),+ $(,)?) => {
        #[cfg(not(target_arch = "wasm32"))]
        {
            tracing::$level!("[Extension] {}", format!($fmt, $($arg),+));
        }
        #[cfg(target_arch = "wasm32")]
        {
            let msg = format!($fmt, $($arg),+);
            $crate::wasm::log(stringify!($level), &msg);
        }
    };
}

/// Extension debug log
#[macro_export]
macro_rules! ext_debug {
    ($($arg:tt)*) => {
        $crate::ext_log!(debug, $($arg)*)
    };
}

/// Extension info log
#[macro_export]
macro_rules! ext_info {
    ($($arg:tt)*) => {
        $crate::ext_log!(info, $($arg)*)
    };
}

/// Extension warning log
#[macro_export]
macro_rules! ext_warn {
    ($($arg:tt)*) => {
        $crate::ext_log!(warn, $($arg)*)
    };
}

/// Extension error log
#[macro_export]
macro_rules! ext_error {
    ($($arg:tt)*) => {
        $crate::ext_log!(error, $($arg)*)
    };
}
