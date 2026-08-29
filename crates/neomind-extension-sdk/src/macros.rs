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

            /// Static storage for metadata strings to avoid dangling pointers.
            /// These are leaked intentionally to ensure they remain valid for the
            /// lifetime of the extension library.
            static METADATA_STORAGE: std::sync::OnceLock<(
                std::ffi::CString,
                std::ffi::CString,
                std::ffi::CString,
                std::ffi::CString,
                std::ffi::CString,
                usize,  // metric_count
                usize,  // command_count
            )> = std::sync::OnceLock::new();

            #[no_mangle]
            pub extern "C" fn neomind_extension_metadata() -> $crate::CExtensionMetadata {
                use std::ffi::CStr;

                // Initialize static storage once - this creates only ONE extension instance
                // for the entire lifetime of the library to get metadata
                let (id, name, version, description, author, metric_count, command_count) = METADATA_STORAGE.get_or_init(|| {
                    // Create a temporary instance to get metadata
                    // This is the ONLY place where we create an instance for metadata
                    let ext = <$extension_type>::$constructor();
                    let meta = <$extension_type as $crate::Extension>::metadata(&ext);
                    let metrics = <$extension_type as $crate::Extension>::metrics(&ext);
                    let commands = <$extension_type as $crate::Extension>::commands(&ext);

                    // Convert to C-compatible format and leak the strings
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

                    // ext is dropped here, releasing any resources
                    (id, name, version, description, author, metrics.len(), commands.len())
                });

                $crate::CExtensionMetadata {
                    abi_version: $crate::SDK_ABI_VERSION,
                    id: id.as_ptr(),
                    name: name.as_ptr(),
                    version: version.as_ptr(),
                    description: description.as_ptr(),
                    author: author.as_ptr(),
                    metric_count: *metric_count,
                    command_count: *command_count,
                }
            }

            static EXTENSION_INSTANCE: std::sync::RwLock<
                Option<std::sync::Arc<tokio::sync::RwLock<std::boxed::Box<dyn $crate::Extension>>>>
            > = std::sync::RwLock::new(None);

            fn extension_instance(
            ) -> std::sync::Arc<tokio::sync::RwLock<std::boxed::Box<dyn $crate::Extension>>> {
                // Fast path: read lock
                {
                    let guard = EXTENSION_INSTANCE.read().unwrap();
                    if let Some(ref instance) = *guard {
                        return instance.clone();
                    }
                }
                // Slow path: write lock with double-check
                let mut guard = EXTENSION_INSTANCE.write().unwrap();
                if let Some(ref instance) = *guard {
                    return instance.clone();
                }
                let extension: $extension_type = <$extension_type>::$constructor();
                let boxed: Box<dyn $crate::Extension> = Box::new(extension);
                let instance = std::sync::Arc::new(tokio::sync::RwLock::new(boxed));
                *guard = Some(instance.clone());
                instance
            }

            /// Reset the extension instance, allowing re-initialization.
            /// Called by the runner when re-initializing an extension.
            #[no_mangle]
            pub extern "C" fn neomind_extension_reset_instance() {
                let mut guard = EXTENSION_INSTANCE.write().unwrap();
                *guard = None;
            }

            /// Thread-local buffer tracking recent CString allocations as a safety net.
            /// If `free_string` is never called by the host, the last 4 allocations
            /// per thread are automatically freed on rotation.
            thread_local! {
                static LAST_STRINGS: std::cell::RefCell<Vec<*mut std::os::raw::c_char>> = std::cell::RefCell::new(Vec::with_capacity(4));
            }

            fn json_ptr(value: serde_json::Value) -> *mut std::os::raw::c_char {
                let json = serde_json::to_string(&value).unwrap_or_else(|_| {
                    "{\"success\":false,\"error\":\"failed to serialize native response\"}".to_string()
                });
                let ptr = std::ffi::CString::new(json)
                    .unwrap_or_else(|_| std::ffi::CString::new("{}").unwrap())
                    .into_raw();

                // Track allocation; free oldest if buffer is full
                LAST_STRINGS.with(|buf| {
                    let mut buf = buf.borrow_mut();
                    if buf.len() >= 4 {
                        // Free the oldest tracked allocation
                        let old = buf.remove(0);
                        unsafe { drop(std::ffi::CString::from_raw(old)); }
                    }
                    buf.push(ptr);
                });

                ptr
            }

            fn error_ptr(message: impl Into<String>) -> *mut std::os::raw::c_char {
                json_ptr(serde_json::json!({
                    "success": false,
                    "error": message.into(),
                }))
            }

            fn parse_input_json(
                input_ptr: *const u8,
                input_len: usize,
            ) -> std::result::Result<serde_json::Value, String> {
                if input_ptr.is_null() || input_len == 0 {
                    return Ok(serde_json::json!({}));
                }

                let input = unsafe { std::slice::from_raw_parts(input_ptr, input_len) };
                let input_str = std::str::from_utf8(input)
                    .map_err(|e| format!("Invalid UTF-8 input: {}", e))?;
                serde_json::from_str(input_str)
                    .map_err(|e| format!("Invalid JSON input: {}", e))
            }

            fn block_on_result<F, T, E>(future: F) -> std::result::Result<T, String>
            where
                F: std::future::Future<Output = std::result::Result<T, E>>,
                E: std::fmt::Display,
            {
                match tokio::runtime::Handle::try_current() {
                    Ok(handle) => tokio::task::block_in_place(|| {
                        handle.block_on(async { future.await.map_err(|e| e.to_string()) })
                    }),
                    Err(_) => {
                        // FFI threads have no tokio context. Building a fresh
                        // multi-thread Runtime PER CALL (the old behavior)
                        // cost milliseconds of setup plus CPU-count worker
                        // threads created and torn down for every
                        // execute_command/produce_metrics — the single biggest
                        // fixed tax on extension calls. Cache one shared
                        // runtime per process instead (2 workers: enough to
                        // drive IO futures and spawned tasks without paying
                        // CPU-count threads).
                        static CACHED_RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> =
                            std::sync::OnceLock::new();
                        let runtime = CACHED_RUNTIME.get_or_init(|| {
                            tokio::runtime::Builder::new_multi_thread()
                                .worker_threads(2)
                                .enable_all()
                                .build()
                                .unwrap_or_else(|e| {
                                    panic!("failed to build cached tokio runtime: {e}")
                                })
                        });
                        runtime.block_on(async { future.await.map_err(|e| e.to_string()) })
                    }
                }
            }

            fn descriptor_json_value() -> std::result::Result<serde_json::Value, String> {
                let instance = extension_instance().clone();
                block_on_result(async move {
                    let ext = instance.read().await;
                    let descriptor = $crate::ExtensionDescriptor::with_capabilities(
                        ext.metadata().clone(),
                        ext.commands(),
                        ext.metrics(),
                    );
                    Ok::<serde_json::Value, String>(serde_json::json!({
                        "success": true,
                        "descriptor": descriptor,
                    }))
                })
            }

            #[no_mangle]
            pub extern "C" fn neomind_extension_free_string(ptr: *mut std::os::raw::c_char) {
                if !ptr.is_null() {
                    // Remove from tracking buffer to avoid double-free
                    LAST_STRINGS.with(|buf| {
                        let mut buf = buf.borrow_mut();
                        if let Some(pos) = buf.iter().position(|&p| p == ptr) {
                            buf.remove(pos);
                        }
                    });
                    unsafe {
                        let _ = std::ffi::CString::from_raw(ptr);
                    }
                }
            }

            #[no_mangle]
            pub extern "C" fn neomind_extension_set_capability_bridge(
                invoke: $crate::NativeCapabilityInvokeFn,
                free: $crate::NativeCapabilityFreeFn,
            ) {
                $crate::set_native_capability_bridge(invoke, free);
            }

            #[no_mangle]
            pub extern "C" fn neomind_extension_descriptor_json() -> *mut std::os::raw::c_char {
                match descriptor_json_value() {
                    Ok(value) => json_ptr(value),
                    Err(e) => error_ptr(e),
                }
            }

            #[no_mangle]
            pub extern "C" fn neomind_extension_execute_command_json(
                input_ptr: *const u8,
                input_len: usize,
            ) -> *mut std::os::raw::c_char {
                let input = match parse_input_json(input_ptr, input_len) {
                    Ok(value) => value,
                    Err(e) => return error_ptr(e),
                };
                let command = input
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let args = input.get("args").cloned().unwrap_or_else(|| serde_json::json!({}));
                let instance = extension_instance().clone();
                match block_on_result(async move {
                    let ext = instance.read().await;
                    let result = ext.execute_command(&command, &args).await?;
                    Ok::<serde_json::Value, $crate::ExtensionError>(serde_json::json!({
                        "success": true,
                        "result": result,
                    }))
                }) {
                    Ok(value) => json_ptr(value),
                    Err(e) => error_ptr(e),
                }
            }

            #[no_mangle]
            pub extern "C" fn neomind_extension_configure_json(
                input_ptr: *const u8,
                input_len: usize,
            ) -> *mut std::os::raw::c_char {
                let input = match parse_input_json(input_ptr, input_len) {
                    Ok(value) => value,
                    Err(e) => return error_ptr(e),
                };
                let instance = extension_instance().clone();
                match block_on_result(async move {
                    let mut ext = instance.write().await;
                    ext.configure(&input).await?;
                    Ok::<serde_json::Value, $crate::ExtensionError>(serde_json::json!({
                        "success": true,
                    }))
                }) {
                    Ok(value) => json_ptr(value),
                    Err(e) => error_ptr(e),
                }
            }

            #[no_mangle]
            pub extern "C" fn neomind_extension_produce_metrics_json() -> *mut std::os::raw::c_char {
                let instance = extension_instance().clone();
                match block_on_result(async move {
                    let ext = instance.read().await;
                    let metrics = ext.produce_metrics()?;
                    Ok::<serde_json::Value, $crate::ExtensionError>(serde_json::json!({
                        "success": true,
                        "metrics": metrics,
                    }))
                }) {
                    Ok(value) => json_ptr(value),
                    Err(e) => error_ptr(e),
                }
            }

            #[no_mangle]
            pub extern "C" fn neomind_extension_health_check_json() -> *mut std::os::raw::c_char {
                let instance = extension_instance().clone();
                match block_on_result(async move {
                    let ext = instance.read().await;
                    let healthy = ext.health_check().await?;
                    Ok::<serde_json::Value, $crate::ExtensionError>(serde_json::json!({
                        "success": true,
                        "healthy": healthy,
                    }))
                }) {
                    Ok(value) => json_ptr(value),
                    Err(e) => error_ptr(e),
                }
            }

            #[no_mangle]
            pub extern "C" fn neomind_extension_stats_json() -> *mut std::os::raw::c_char {
                let instance = extension_instance().clone();
                match block_on_result(async move {
                    let ext = instance.read().await;
                    Ok::<serde_json::Value, String>(serde_json::json!({
                        "success": true,
                        "stats": ext.get_stats(),
                    }))
                }) {
                    Ok(value) => json_ptr(value),
                    Err(e) => error_ptr(e),
                }
            }

            #[no_mangle]
            pub extern "C" fn neomind_extension_event_subscriptions_json() -> *mut std::os::raw::c_char {
                let instance = extension_instance().clone();
                match block_on_result(async move {
                    let ext = instance.read().await;
                    let event_types: Vec<String> = ext.event_subscriptions().iter().map(|s| s.to_string()).collect();
                    Ok::<serde_json::Value, String>(serde_json::json!({
                        "success": true,
                        "event_types": event_types,
                    }))
                }) {
                    Ok(value) => json_ptr(value),
                    Err(e) => error_ptr(e),
                }
            }

            #[no_mangle]
            pub extern "C" fn neomind_extension_stream_capability_json() -> *mut std::os::raw::c_char {
                let instance = extension_instance().clone();
                match block_on_result(async move {
                    let ext = instance.read().await;
                    Ok::<serde_json::Value, String>(serde_json::json!({
                        "success": true,
                        "capability": ext.stream_capability(),
                    }))
                }) {
                    Ok(value) => json_ptr(value),
                    Err(e) => error_ptr(e),
                }
            }

            #[no_mangle]
            pub extern "C" fn neomind_extension_init_session_json(
                input_ptr: *const u8,
                input_len: usize,
            ) -> *mut std::os::raw::c_char {
                let input = match parse_input_json(input_ptr, input_len) {
                    Ok(value) => value,
                    Err(e) => return error_ptr(e),
                };
                let session_id = input.get("session_id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                let config = input.get("config").cloned().unwrap_or_else(|| serde_json::json!({}));
                let instance = extension_instance().clone();
                match block_on_result(async move {
                    let ext = instance.read().await;
                    let session = $crate::StreamSession::new(
                        session_id,
                        ext.metadata().id.clone(),
                        config,
                        $crate::ClientInfo {
                            client_id: "runner".to_string(),
                            ip_addr: None,
                            user_agent: None,
                        },
                    );
                    ext.init_session(&session).await?;
                    Ok::<serde_json::Value, $crate::ExtensionError>(serde_json::json!({ "success": true }))
                }) {
                    Ok(value) => json_ptr(value),
                    Err(e) => error_ptr(e),
                }
            }

            #[no_mangle]
            pub extern "C" fn neomind_extension_process_session_chunk_json(
                input_ptr: *const u8,
                input_len: usize,
            ) -> *mut std::os::raw::c_char {
                let input = match parse_input_json(input_ptr, input_len) {
                    Ok(value) => value,
                    Err(e) => return error_ptr(e),
                };
                let session_id = input.get("session_id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                let chunk: $crate::DataChunk = match input.get("chunk").cloned() {
                    Some(value) => match serde_json::from_value(value) {
                        Ok(chunk) => chunk,
                        Err(e) => return error_ptr(format!("Invalid stream chunk JSON: {}", e)),
                    },
                    None => return error_ptr("Missing stream chunk"),
                };
                let instance = extension_instance().clone();
                match block_on_result(async move {
                    let ext = instance.read().await;
                    let result = ext.process_session_chunk(&session_id, chunk).await?;
                    Ok::<serde_json::Value, $crate::ExtensionError>(serde_json::json!({
                        "success": true,
                        "result": result,
                    }))
                }) {
                    Ok(value) => json_ptr(value),
                    Err(e) => error_ptr(e),
                }
            }

            #[no_mangle]
            pub extern "C" fn neomind_extension_close_session_json(
                input_ptr: *const u8,
                input_len: usize,
            ) -> *mut std::os::raw::c_char {
                let input = match parse_input_json(input_ptr, input_len) {
                    Ok(value) => value,
                    Err(e) => return error_ptr(e),
                };
                let session_id = input.get("session_id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                let instance = extension_instance().clone();
                match block_on_result(async move {
                    let ext = instance.read().await;
                    let stats = ext.close_session(&session_id).await?;
                    Ok::<serde_json::Value, $crate::ExtensionError>(serde_json::json!({
                        "success": true,
                        "stats": stats,
                    }))
                }) {
                    Ok(value) => json_ptr(value),
                    Err(e) => error_ptr(e),
                }
            }

            #[no_mangle]
            pub extern "C" fn neomind_extension_process_chunk_json(
                input_ptr: *const u8,
                input_len: usize,
            ) -> *mut std::os::raw::c_char {
                let input = match parse_input_json(input_ptr, input_len) {
                    Ok(value) => value,
                    Err(e) => return error_ptr(e),
                };
                let chunk: $crate::DataChunk = match input.get("chunk").cloned() {
                    Some(value) => match serde_json::from_value(value) {
                        Ok(chunk) => chunk,
                        Err(e) => return error_ptr(format!("Invalid chunk JSON: {}", e)),
                    },
                    None => return error_ptr("Missing chunk"),
                };
                let instance = extension_instance().clone();
                match block_on_result(async move {
                    let ext = instance.read().await;
                    let result = ext.process_chunk(chunk).await?;
                    Ok::<serde_json::Value, $crate::ExtensionError>(serde_json::json!({
                        "success": true,
                        "result": result,
                    }))
                }) {
                    Ok(value) => json_ptr(value),
                    Err(e) => error_ptr(e),
                }
            }

            #[no_mangle]
            pub extern "C" fn neomind_extension_start_push_json(
                input_ptr: *const u8,
                input_len: usize,
            ) -> *mut std::os::raw::c_char {
                let input = match parse_input_json(input_ptr, input_len) {
                    Ok(value) => value,
                    Err(e) => return error_ptr(e),
                };
                let session_id = input.get("session_id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                let instance = extension_instance().clone();
                match block_on_result(async move {
                    let ext = instance.read().await;
                    ext.start_push(&session_id).await?;
                    Ok::<serde_json::Value, $crate::ExtensionError>(serde_json::json!({ "success": true }))
                }) {
                    Ok(value) => json_ptr(value),
                    Err(e) => error_ptr(e),
                }
            }

            #[no_mangle]
            pub extern "C" fn neomind_extension_stop_push_json(
                input_ptr: *const u8,
                input_len: usize,
            ) -> *mut std::os::raw::c_char {
                let input = match parse_input_json(input_ptr, input_len) {
                    Ok(value) => value,
                    Err(e) => return error_ptr(e),
                };
                let session_id = input.get("session_id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                let instance = extension_instance().clone();
                match block_on_result(async move {
                    let ext = instance.read().await;
                    ext.stop_push(&session_id).await?;
                    Ok::<serde_json::Value, $crate::ExtensionError>(serde_json::json!({ "success": true }))
                }) {
                    Ok(value) => json_ptr(value),
                    Err(e) => error_ptr(e),
                }
            }

            #[no_mangle]
            pub extern "C" fn neomind_extension_handle_event_json(
                input_ptr: *const u8,
                input_len: usize,
            ) -> *mut std::os::raw::c_char {
                let input = match parse_input_json(input_ptr, input_len) {
                    Ok(value) => value,
                    Err(e) => return error_ptr(e),
                };
                let event_type = input.get("event_type").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                let payload = input.get("payload").cloned().unwrap_or_else(|| serde_json::json!({}));
                let instance = extension_instance().clone();
                match block_on_result(async move {
                    let ext = instance.read().await;
                    ext.handle_event(&event_type, &payload)?;
                    Ok::<serde_json::Value, $crate::ExtensionError>(serde_json::json!({ "success": true }))
                }) {
                    Ok(value) => json_ptr(value),
                    Err(e) => error_ptr(e),
                }
            }

            /// Called by the runner at init time to register a push-output callback
            /// so the extension can send data without JSON FFI overhead.
            #[no_mangle]
            pub extern "C" fn neomind_extension_register_push_writer(
                writer: $crate::PushOutputWriterFn,
            ) -> i32 {
                $crate::set_push_output_writer(writer);
                0
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
                        "description": c.description,
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

                // Use $crate::pollster::block_on for WASM
                let result = $crate::pollster::block_on(future);

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

// ============================================================================
// Configuration-Style Extension Definition Macros
// ============================================================================

/// Define an extension with configuration-style syntax
///
/// This macro simplifies extension development by generating boilerplate code
/// for metadata, metrics, and command definitions.
///
/// # Basic Example
///
/// ```rust,ignore
/// use neomind_extension_sdk::prelude::*;
///
/// pub struct MyExtension {
///     counter: std::sync::atomic::AtomicI64,
/// }
///
/// impl MyExtension {
///     pub fn new() -> Self {
///         Self {
///             counter: std::sync::atomic::AtomicI64::new(0),
///         }
///     }
///
///     // Business methods
///     pub async fn increment(&self, amount: i64) -> Result<i64> {
///         Ok(self.counter.fetch_add(amount, std::sync::atomic::Ordering::SeqCst) + amount)
///     }
/// }
///
/// // Define extension with configuration-style macro
/// neomind_extension!(MyExtension {
///     id: "my-extension",
///     name: "My Extension",
///     version: "1.0.0",
///     description: "A simple counter extension",
///
///     commands: [
///         ("increment", "Increment Counter", [
///             ("amount", "Amount", Integer, optional)
///         ])
///     ],
///
///     dispatch: {
///         "increment" => |ext, args| {
///             let amount = args.get("amount").and_then(|v| v.as_i64()).unwrap_or(1);
///             let result = ext.increment(amount).await?;
///             Ok(serde_json::json!({ "counter": result }))
///         }
///     }
/// });
/// ```
#[macro_export]
macro_rules! neomind_extension {
    (
        $extension_type:ty {
            id: $id:literal,
            name: $name:literal,
            version: $version:literal,

            // Optional fields
            $(description: $description:literal,)?
            $(author: $author:literal,)?

            // Metrics definition (optional)
            $(metrics: [
                $($metric_name:literal : $metric_display:literal => $metric_type:ident $(($metric_unit:literal, $metric_min:literal, $metric_max:literal))?),*
            ],)?

            // Commands definition (optional)
            $(commands: [
                $($cmd_name:literal : $cmd_display:literal => [$($param_name:literal : $param_display:literal : $param_type:ident),*]),*
            ],)?

            // Dispatch handlers
            dispatch: {
                $($dispatch_cmd:literal => |$dispatch_ext:ident, $dispatch_args:ident| $dispatch_body:block)*
            }
        }
    ) => {
        // Implement the Extension trait
        #[async_trait::async_trait]
        impl $crate::Extension for $extension_type {
            fn metadata(&self) -> &$crate::ExtensionMetadata {
                use std::sync::OnceLock;
                static META: OnceLock<$crate::ExtensionMetadata> = OnceLock::new();
                META.get_or_init(|| {
                    let version = $crate::semver::Version::parse($version)
                        .unwrap_or_else(|_| $crate::semver::Version::new(0, 0, 0));
                    let mut meta = $crate::ExtensionMetadata::new($id, $name, version);
                    $(meta.description = Some($description.to_string());)?
                    $(meta.author = Some($author.to_string());)?
                    meta
                })
            }

            fn metrics(&self) -> Vec<$crate::MetricDescriptor> {
                vec![
                    $(
                        $(
                            $crate::MetricDescriptor {
                                name: $metric_name.to_string(),
                                display_name: $metric_display.to_string(),
                                data_type: $crate::neomind_extension_metric_type!($metric_type),
                                unit: String::new(),
                                min: None,
                                max: None,
                                required: false,
                            }
                        ),*
                    )?
                ]
            }

            fn commands(&self) -> Vec<$crate::ExtensionCommand> {
                vec![
                    $(
                        $(
                            $crate::ExtensionCommand {
                                name: $cmd_name.to_string(),
                                display_name: $cmd_display.to_string(),
                                description: String::new(),
                                payload_template: String::new(),
                                parameters: vec![
                                    $(
                                        $crate::ParameterDefinition {
                                            name: $param_name.to_string(),
                                            display_name: $param_display.to_string(),
                                            description: String::new(),
                                            param_type: $crate::neomind_extension_param_type!($param_type),
                                            required: true,
                                            default_value: None,
                                            min: None,
                                            max: None,
                                            options: Vec::new(),
                                        }
                                    ),*
                                ],
                                fixed_values: std::collections::HashMap::new(),
                                samples: Vec::new(),
                                parameter_groups: Vec::new(),
                            }
                        ),*
                    )?
                ]
            }

            async fn execute_command(
                &self,
                command: &str,
                args: &serde_json::Value,
            ) -> $crate::Result<serde_json::Value> {
                match command {
                    $(
                        $dispatch_cmd => {
                            let $dispatch_ext = self;
                            let $dispatch_args = args;
                            $dispatch_body
                        }
                    )*
                    _ => Err($crate::ExtensionError::CommandNotFound(command.to_string())),
                }
            }
        }

        // Export FFI functions
        $crate::neomind_export!($extension_type);
    };
}

/// Helper macro to convert type name to MetricDataType
#[macro_export]
macro_rules! neomind_extension_metric_type {
    (Float) => {
        $crate::MetricDataType::Float
    };
    (Integer) => {
        $crate::MetricDataType::Integer
    };
    (Boolean) => {
        $crate::MetricDataType::Boolean
    };
    (String) => {
        $crate::MetricDataType::String
    };
    (Binary) => {
        $crate::MetricDataType::Binary
    };
}

/// Helper macro to convert type name to MetricDataType for parameters
#[macro_export]
macro_rules! neomind_extension_param_type {
    (Float) => {
        $crate::MetricDataType::Float
    };
    (Integer) => {
        $crate::MetricDataType::Integer
    };
    (Boolean) => {
        $crate::MetricDataType::Boolean
    };
    (String) => {
        $crate::MetricDataType::String
    };
    (Binary) => {
        $crate::MetricDataType::Binary
    };
}

/// Simplified extension definition with minimal boilerplate
///
/// This macro is a simplified version of `neomind_extension!` that only requires
/// the essential fields. Use this when you don't need metrics or complex commands.
///
/// # Example
///
/// ```rust,ignore
/// neomind_simple_extension!(MyExtension {
///     id: "my-extension",
///     name: "My Extension",
///     version: "1.0.0",
///
///     dispatch: {
///         "hello" => |_ext, _args| {
///             Ok(serde_json::json!({ "message": "Hello, World!" }))
///         }
///     }
/// });
/// ```
#[macro_export]
macro_rules! neomind_simple_extension {
    (
        $extension_type:ty {
            id: $id:literal,
            name: $name:literal,
            version: $version:literal,

            $(description: $description:literal,)?

            dispatch: {
                $($dispatch_cmd:literal => |$dispatch_ext:ident, $dispatch_args:ident| $dispatch_body:block)*
            }
        }
    ) => {
        #[async_trait::async_trait]
        impl $crate::Extension for $extension_type {
            fn metadata(&self) -> &$crate::ExtensionMetadata {
                use std::sync::OnceLock;
                static META: OnceLock<$crate::ExtensionMetadata> = OnceLock::new();
                META.get_or_init(|| {
                    let version = $crate::semver::Version::parse($version)
                        .unwrap_or_else(|_| $crate::semver::Version::new(0, 0, 0));
                    let mut meta = $crate::ExtensionMetadata::new($id, $name, version);
                    $(meta.description = Some($description.to_string());)?
                    meta
                })
            }

            async fn execute_command(
                &self,
                command: &str,
                args: &serde_json::Value,
            ) -> $crate::Result<serde_json::Value> {
                match command {
                    $(
                        $dispatch_cmd => {
                            let $dispatch_ext = self;
                            let $dispatch_args = args;
                            $dispatch_body
                        }
                    )*
                    _ => Err($crate::ExtensionError::CommandNotFound(command.to_string())),
                }
            }
        }

        $crate::neomind_export!($extension_type);
    };
}

/// Define metrics for an extension (helper macro)
///
/// Use this with the `metrics!` macro inside your extension impl block.
#[macro_export]
macro_rules! define_metrics {
    ($($name:literal => $display:literal : $type:ident $([$unit:literal, $min:expr, $max:expr])?),* $(,)?) => {
        vec![
            $(
                $crate::MetricDescriptor {
                    name: $name.to_string(),
                    display_name: $display.to_string(),
                    data_type: $crate::neomind_extension_metric_type!($type),
                    unit: String::new() $(.to_string() = $unit.to_string())?,
                    min: None $(.or(Some($min as f64)))?,
                    max: None $(.or(Some($max as f64)))?,
                    required: false,
                }
            ),*
        ]
    };
}

/// Define commands for an extension (helper macro)
#[macro_export]
macro_rules! define_commands {
    ($($name:literal => $display:literal : [$($param:tt),*] $(samples: [$($sample:expr),*])?),* $(,)?) => {
        vec![
            $(
                $crate::ExtensionCommand {
                    name: $name.to_string(),
                    display_name: $display.to_string(),
                    description: String::new(),
                    payload_template: String::new(),
                    parameters: $crate::define_params!($($param),*),
                    fixed_values: std::collections::HashMap::new(),
                    samples: vec![$($($sample.clone()),*)?],
                    parameter_groups: Vec::new(),
                }
            ),*
        ]
    };
}

/// Define parameters for a command (helper macro)
#[macro_export]
macro_rules! define_params {
    () => { Vec::new() };
    ($name:literal : $display:literal : $type:ident $(, $rest:tt)*) => {
        {
            let mut params = vec![
                $crate::ParameterDefinition {
                    name: $name.to_string(),
                    display_name: $display.to_string(),
                    description: String::new(),
                    param_type: $crate::neomind_extension_param_type!($type),
                    required: true,
                    default_value: None,
                    min: None,
                    max: None,
                    options: Vec::new(),
                }
            ];
            params.extend($crate::define_params!($($rest)*));
            params
        }
    };
    ($name:literal : $display:literal : $type:ident ? $(, $rest:tt)*) => {
        {
            let mut params = vec![
                $crate::ParameterDefinition {
                    name: $name.to_string(),
                    display_name: $display.to_string(),
                    description: String::new(),
                    param_type: $crate::neomind_extension_param_type!($type),
                    required: false,
                    default_value: None,
                    min: None,
                    max: None,
                    options: Vec::new(),
                }
            ];
            params.extend($crate::define_params!($($rest)*));
            params
        }
    };
}
