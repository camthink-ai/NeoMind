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

        // WASM exports - Full support for metrics, commands, and execution
        #[cfg(target_arch = "wasm32")]
        mod __neomind_wasm_exports {
            use super::*;
            use std::sync::atomic::{AtomicPtr, Ordering};

            // Thread-local extension instance storage
            static EXTENSION_PTR: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());

            // Helper to get or create extension instance
            fn get_extension() -> &'static mut $extension_type {
                let ptr = EXTENSION_PTR.load(Ordering::SeqCst);
                if ptr.is_null() {
                    let ext = Box::new(<$extension_type>::$constructor());
                    let raw = Box::into_raw(ext) as *mut u8;
                    EXTENSION_PTR.store(raw, Ordering::SeqCst);
                    unsafe { &mut *(raw as *mut $extension_type) }
                } else {
                    unsafe { &mut *(ptr as *mut $extension_type) }
                }
            }

            #[no_mangle]
            pub extern "C" fn neomind_extension_abi_version() -> u32 {
                $crate::SDK_ABI_VERSION
            }

            /// Initialize the extension (optional, for explicit lifecycle control)
            #[no_mangle]
            pub extern "C" fn extension_init() -> i32 {
                // Extension is lazily initialized on first use
                0
            }

            /// Clean up extension resources
            #[no_mangle]
            pub extern "C" fn extension_cleanup() {
                let ptr = EXTENSION_PTR.swap(std::ptr::null_mut(), Ordering::SeqCst);
                if !ptr.is_null() {
                    unsafe {
                        let _ = Box::from_raw(ptr as *mut $extension_type);
                    }
                }
            }

            /// Get basic metadata (legacy, kept for compatibility)
            #[no_mangle]
            pub extern "C" fn get_metadata() -> i32 {
                let ext = get_extension();
                let meta = <$extension_type as $crate::Extension>::metadata(ext);
                let metadata = serde_json::json!({
                    "id": meta.id,
                    "name": meta.name,
                    "version": meta.version.to_string(),
                    "description": meta.description,
                    "author": meta.author,
                });
                let json = serde_json::to_string(&metadata).unwrap_or_else(|_| "{}".to_string());
                $crate::wasm::write_result(&json)
            }

            /// Get full extension descriptor including metrics and commands
            /// Returns JSON string length, data written to RESULT_OFFSET
            #[no_mangle]
            pub extern "C" fn get_descriptor_json() -> i32 {
                let ext = get_extension();
                let meta = <$extension_type as $crate::Extension>::metadata(ext);
                let metrics = <$extension_type as $crate::Extension>::metrics(ext);
                let commands = <$extension_type as $crate::Extension>::commands(ext);

                // Build metrics array
                let metrics_json: Vec<serde_json::Value> = metrics.iter().map(|m| {
                    serde_json::json!({
                        "name": m.name,
                        "display_name": m.display_name,
                        "data_type": format!("{:?}", m.data_type).to_lowercase(),
                        "unit": m.unit,
                        "min": m.min,
                        "max": m.max,
                        "required": m.required
                    })
                }).collect();

                // Build commands array
                let commands_json: Vec<serde_json::Value> = commands.iter().map(|c| {
                    let params_json: Vec<serde_json::Value> = c.parameters.iter().map(|p| {
                        serde_json::json!({
                            "name": p.name,
                            "display_name": p.display_name,
                            "description": p.description,
                            "param_type": format!("{:?}", p.param_type).to_lowercase(),
                            "required": p.required,
                            "default_value": p.default_value.as_ref().map(|v| match v {
                                $crate::ParamMetricValue::Float(f) => serde_json::json!(f),
                                $crate::ParamMetricValue::Integer(i) => serde_json::json!(i),
                                $crate::ParamMetricValue::Boolean(b) => serde_json::json!(b),
                                $crate::ParamMetricValue::String(s) => serde_json::json!(s),
                                $crate::ParamMetricValue::Binary(_) => serde_json::json!(null),
                                $crate::ParamMetricValue::Null => serde_json::json!(null),
                            }),
                            "min": p.min,
                            "max": p.max,
                            "options": p.options
                        })
                    }).collect();

                    serde_json::json!({
                        "name": c.name,
                        "display_name": c.display_name,
                        "description": c.llm_hints,
                        "parameters": params_json,
                        "samples": c.samples
                    })
                }).collect();

                let descriptor = serde_json::json!({
                    "metadata": {
                        "id": meta.id,
                        "name": meta.name,
                        "version": meta.version.clone(),
                        "description": meta.description.as_ref().unwrap_or(&String::new()),
                        "author": meta.author.as_ref().unwrap_or(&String::new()),
                    },
                    "metrics": metrics_json,
                    "commands": commands_json
                });

                let json = serde_json::to_string(&descriptor).unwrap_or_else(|_| "{}".to_string());
                $crate::wasm::write_result(&json)
            }

            /// Execute a command
            /// Input: JSON at input_ptr with { "command": "cmd_name", "args": {...} }
            /// Output: Result JSON at RESULT_OFFSET
            #[no_mangle]
            pub extern "C" fn execute_command_json(input_ptr: *const u8, input_len: i32) -> i32 {
                if input_ptr.is_null() || input_len <= 0 {
                    let error = serde_json::json!({
                        "success": false,
                        "error": "Invalid input"
                    });
                    let json = serde_json::to_string(&error).unwrap_or_else(|_| "{}".to_string());
                    return $crate::wasm::write_result(&json);
                }

                // Read input JSON
                let input_slice = unsafe {
                    std::slice::from_raw_parts(input_ptr, input_len as usize)
                };
                let input_str = match std::str::from_utf8(input_slice) {
                    Ok(s) => s,
                    Err(_) => {
                        let error = serde_json::json!({
                            "success": false,
                            "error": "Invalid UTF-8 input"
                        });
                        let json = serde_json::to_string(&error).unwrap_or_else(|_| "{}".to_string());
                        return $crate::wasm::write_result(&json);
                    }
                };

                // Parse input
                let input: serde_json::Value = match serde_json::from_str(input_str) {
                    Ok(v) => v,
                    Err(_) => {
                        let error = serde_json::json!({
                            "success": false,
                            "error": "Invalid JSON input"
                        });
                        let json = serde_json::to_string(&error).unwrap_or_else(|_| "{}".to_string());
                        return $crate::wasm::write_result(&json);
                    }
                };

                let command = input.get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let args = input.get("args").cloned().unwrap_or(serde_json::json!({}));

                // Execute command - for WASM we use sync execution via block_on
                // WASM extensions should keep execute_command simple (no heavy async)
                let ext = get_extension();
                let future = <$extension_type as $crate::Extension>::execute_command(ext, command, &args);
                
                // Use futures::executor::block_on for WASM
                let result = futures::executor::block_on(future);

                let response = match result {
                    Ok(value) => serde_json::json!({
                        "success": true,
                        "result": value
                    }),
                    Err(e) => serde_json::json!({
                        "success": false,
                        "error": e.to_string()
                    }),
                };

                let json = serde_json::to_string(&response).unwrap_or_else(|_| "{}".to_string());
                $crate::wasm::write_result(&json)
            }

            /// Produce current metric values
            /// Output: JSON array of metric values at RESULT_OFFSET
            #[no_mangle]
            pub extern "C" fn produce_metrics_json() -> i32 {
                let ext = get_extension();
                let result = <$extension_type as $crate::Extension>::produce_metrics(ext);

                let metrics = match result {
                    Ok(values) => {
                        let items: Vec<serde_json::Value> = values.iter().map(|m| {
                            let value_json = match &m.value {
                                $crate::ParamMetricValue::Float(f) => serde_json::json!(f),
                                $crate::ParamMetricValue::Integer(i) => serde_json::json!(i),
                                $crate::ParamMetricValue::Boolean(b) => serde_json::json!(b),
                                $crate::ParamMetricValue::String(s) => serde_json::json!(s),
                                $crate::ParamMetricValue::Binary(data) => {
                                    // Encode binary as base64 using helper function
                                    serde_json::json!($crate::wasm::encode_base64(data))
                                },
                                $crate::ParamMetricValue::Null => serde_json::json!(null),
                            };
                            serde_json::json!({
                                "name": m.name,
                                "value": value_json,
                                "timestamp": m.timestamp
                            })
                        }).collect();
                        serde_json::json!({
                            "success": true,
                            "metrics": items
                        })
                    },
                    Err(e) => serde_json::json!({
                        "success": false,
                        "error": e.to_string(),
                        "metrics": []
                    }),
                };

                let json = serde_json::to_string(&metrics).unwrap_or_else(|_| "{}".to_string());
                $crate::wasm::write_result(&json)
            }

            /// Health check
            /// Output: JSON at RESULT_OFFSET
            #[no_mangle]
            pub extern "C" fn health_check_json() -> i32 {
                let ext = get_extension();
                // For WASM, we do a simple health check
                // Full async health_check would require runtime support
                let meta = <$extension_type as $crate::Extension>::metadata(ext);
                let response = serde_json::json!({
                    "success": true,
                    "healthy": true,
                    "extension_id": meta.id,
                    "message": "WASM extension is healthy"
                });
                let json = serde_json::to_string(&response).unwrap_or_else(|_| "{}".to_string());
                $crate::wasm::write_result(&json)
            }

            /// Get stream capability (for streaming extensions)
            /// Output: JSON at RESULT_OFFSET
            #[no_mangle]
            pub extern "C" fn get_stream_capability_json() -> i32 {
                let ext = get_extension();
                let capability = <$extension_type as $crate::Extension>::stream_capability(ext);

                let response = if let Some(cap) = capability {
                    serde_json::json!({
                        "success": true,
                        "has_streaming": true,
                        "direction": format!("{:?}", cap.direction).to_lowercase(),
                        "mode": format!("{:?}", cap.mode).to_lowercase(),
                        "max_chunk_size": cap.max_chunk_size,
                        "preferred_chunk_size": cap.preferred_chunk_size,
                        "max_concurrent_sessions": cap.max_concurrent_sessions
                    })
                } else {
                    serde_json::json!({
                        "success": true,
                        "has_streaming": false
                    })
                };

                let json = serde_json::to_string(&response).unwrap_or_else(|_| "{}".to_string());
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
        $crate::metric_value!($name, $value, $crate::utils::current_timestamp_ms())
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
