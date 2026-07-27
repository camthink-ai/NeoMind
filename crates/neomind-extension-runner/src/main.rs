// Prevents additional console window on Windows in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! NeoMind Extension Runner
//!
//! This is a standalone process that loads and runs a single extension.
//! It communicates with the main NeoMind process via stdin/stdout using
//! the IPC protocol.
//!
//! # Supported Extension Types
//!
//! - Native libraries (.so, .dylib, .dll)
//! - WebAssembly modules (.wasm)
//!
//! # Usage
//!
//! ```bash
//! neomind-extension-runner --extension-path /path/to/extension.dylib
//! neomind-extension-runner --extension-path /path/to/extension.wasm
//! ```
//!
//! # Protocol
//!
//! The runner reads IPC messages from stdin and writes responses to stdout.
//! All messages are framed with a 4-byte length prefix (little-endian).

use std::collections::{HashMap, VecDeque};
use std::ffi::CStr;
use std::io::{Read, Write};
use std::os::raw::c_char;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;

use clap::Parser;
use serde_json::json;
use tokio::sync::RwLock;
use tracing::{debug, error, info, trace, warn};
use wasmtime::{AsContext, AsContextMut, Config, Engine, Linker, Memory, Module, Store, Val};
use wasmtime_wasi::preview1::{self, WasiP1Ctx};
use wasmtime_wasi::WasiCtxBuilder;

use neomind_extension_sdk::{
    capability_constants as cap, BatchCommand, BatchResult, DataChunk, ErrorKind, ExtensionCommand,
    ExtensionDescriptor, ExtensionMetadata, ExtensionMetricValue, ExtensionStats, IpcFrame,
    IpcMessage, IpcResponse, MetricDataType, MetricDescriptor, ParameterDefinition, SessionStats,
    StreamCapability, StreamDataChunk, StreamDataType, StreamResult, ABI_VERSION,
};

// Resource limits module
mod resource_limits;
use resource_limits::{setup_resource_limits, ResourceLimitsConfig};

// Dylib validation module
mod dylib_validation;
use dylib_validation::validate_library;

// Event handler module
mod event_handler;
use event_handler::get_global_event_state;

// IPC routing module
mod ipc_routing;
use ipc_routing::{
    complete_pending_request, create_event_channel, get_pending_requests, register_pending_request,
    safe_ffi_call, safe_ffi_call_with_timeout, start_stdin_reader, SendPtr, STDOUT_WRITE_MUTEX,
};

// ============================================================================
// Message routing for capability invocation
// ============================================================================

// IPC routing and event handling now live in their own modules

/// Global event state for WASM extensions
/// Extension type detected from file
#[derive(Debug, Clone, Copy, PartialEq)]
enum ExtensionType {
    Native,
    Wasm,
}

impl ExtensionType {
    /// Detect extension type from file path
    fn from_path(path: &std::path::Path) -> Self {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|ext| match ext.to_lowercase().as_str() {
                "wasm" => ExtensionType::Wasm,
                _ => ExtensionType::Native,
            })
            .unwrap_or(ExtensionType::Native)
    }
}

/// Extension runner arguments
#[derive(Parser, Debug)]
#[command(name = "neomind-extension-runner")]
#[command(about = "Run a NeoMind extension in isolated mode")]
struct Args {
    /// Path to the extension library (.so, .dylib, .dll, or .wasm)
    #[arg(long, short = 'e')]
    extension_path: PathBuf,

    /// Enable verbose logging
    #[arg(long, short = 'v')]
    verbose: bool,

    /// Memory limit in MB (0 = no limit).
    ///
    /// Default is 0 (unlimited) so GPU extensions don't OOM on startup. If a
    /// deployment needs to bound extension memory, override via CLI flag or
    /// systemd `MemoryMax=`.
    #[arg(long = "memory-limit", default_value = "0")]
    memory_limit_mb: u64,

    /// Hard memory limit in MB (0 = 2x soft limit)
    #[arg(long = "memory-limit-hard", default_value = "0")]
    memory_limit_hard_mb: u64,

    /// Process nice level (priority, -20 to 19, use 10 for background)
    #[arg(long = "nice", default_value = "10")]
    nice_level: i32,
}

/// Extension runner state
struct Runner {
    /// Loaded extension (for native)
    extension: Option<NativeExtensionBridge>,
    /// WASM runtime (for WASM)
    wasm_runtime: Option<WasmRuntime>,
    /// Extension descriptor (unified capabilities)
    descriptor: ExtensionDescriptor,
    /// Extension type
    extension_type: ExtensionType,
    /// Shared runtime for native async extension calls
    runtime: tokio::runtime::Handle,
    /// Running flag
    running: bool,
    /// IPC client for WASM capability forwarding
    ipc_client: Option<Arc<SyncIpcClient>>,
}

type JsonFn0 = unsafe extern "C" fn() -> *mut c_char;
type JsonFn1 = unsafe extern "C" fn(*const u8, usize) -> *mut c_char;
type FreeStringFn = unsafe extern "C" fn(*mut c_char);
type SetCapabilityBridgeFn = unsafe extern "C" fn(HostCapabilityInvokeFn, HostCapabilityFreeFn);
type HostCapabilityInvokeFn = unsafe extern "C" fn(*const u8, usize) -> *mut c_char;
type HostCapabilityFreeFn = unsafe extern "C" fn(*mut c_char);

static GLOBAL_NATIVE_IPC_CLIENT: std::sync::OnceLock<Arc<SyncIpcClient>> =
    std::sync::OnceLock::new();

/// Allowed capability names that can be invoked through the native bridge.
/// Unknown names are rejected before reaching the host process.
const ALLOWED_CAPABILITIES: &[&str] = &[
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

unsafe extern "C" fn runner_native_capability_invoke(
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

unsafe extern "C" fn runner_native_capability_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        let _ = std::ffi::CString::from_raw(ptr);
    }
}

// ============================================================================
// Push output buffering — decouples FFI callback from blocking stdout I/O
// ============================================================================

/// Maximum queued frames before dropping the oldest.
const PUSH_BUFFER_CAPACITY: usize = 16;

/// Shared push-output buffer: newest frames survive, oldest get dropped.
static PUSH_BUFFER: std::sync::OnceLock<(std::sync::Mutex<VecDeque<Vec<u8>>>, std::sync::Condvar)> =
    std::sync::OnceLock::new();

/// Counter for dropped push frames (diagnostics).
static PUSH_DROPPED_COUNT: AtomicU64 = AtomicU64::new(0);

/// Background thread that drains the push buffer and writes to stdout.
fn push_stdout_writer_thread() {
    let (lock, cvar) = PUSH_BUFFER.get().expect("PUSH_BUFFER not initialized");
    let mut seq: u64 = 0;
    loop {
        let mut guard = lock.lock().unwrap();
        // Wait until there's at least one frame
        while guard.is_empty() {
            guard = cvar.wait(guard).unwrap();
        }
        // Drain all available frames, keep only the latest
        let batch: Vec<Vec<u8>> = guard.drain(..).collect();
        drop(guard);

        // Write each frame to stdout (under STDOUT_WRITE_MUTEX for inter-task safety)
        for frame in batch {
            let _write_guard = STDOUT_WRITE_MUTEX.lock().unwrap_or_else(|e| {
                error!("STDOUT_WRITE_MUTEX poisoned: {}", e);
                e.into_inner()
            });
            let mut stdout = std::io::stdout();
            if let Err(e) = stdout.write_all(&frame) {
                error!("Push stdout writer: write error: {e}");
                return;
            }
            if let Err(e) = stdout.flush() {
                error!("Push stdout writer: flush error: {e}");
                return;
            }
        }
        seq += 1;

        if seq.is_multiple_of(100) {
            let dropped = PUSH_DROPPED_COUNT.swap(0, AtomicOrdering::Relaxed);
            if dropped > 0 {
                debug!(
                    "Push buffer: dropped {} frames in last 100 batches",
                    dropped
                );
            }
        }
    }
}

/// Push-output writer callback — called by the extension via FFI to push data.
///
/// Enqueues the encoded frame into a bounded buffer. If the buffer is full,
/// the **oldest** frame is dropped so the newest data always gets through.
/// A background thread drains the buffer to stdout, keeping this callback
/// non-blocking regardless of downstream backpressure.
unsafe extern "C" fn push_output_writer(data: *const u8, len: usize) -> i32 {
    if data.is_null() || len == 0 {
        warn!("PushOutputWriter: null/empty data");
        return -1;
    }

    let bytes = std::slice::from_raw_parts(data, len);
    let msg: serde_json::Value = match serde_json::from_slice(bytes) {
        Ok(v) => v,
        Err(e) => {
            warn!("PushOutputWriter: invalid JSON: {e}");
            return -2;
        }
    };

    // Build IpcResponse::PushOutput from the JSON fields
    let response = IpcResponse::PushOutput {
        session_id: msg
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        sequence: msg.get("sequence").and_then(|v| v.as_u64()).unwrap_or(0),
        data: msg
            .get("data")
            .and_then(|v| v.as_str())
            .map(|s| {
                use base64::Engine as _;
                base64::engine::general_purpose::STANDARD
                    .decode(s)
                    .unwrap_or_else(|_| s.as_bytes().to_vec())
            })
            .unwrap_or_default(),
        data_type: msg
            .get("data_type")
            .and_then(|v| v.as_str())
            .unwrap_or("application/octet-stream")
            .to_string(),
        timestamp: msg
            .get("timestamp")
            .and_then(|v| v.as_i64())
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis()),
        metadata: msg.get("metadata").cloned(),
    };

    let payload = match response.to_bytes() {
        Ok(p) => p,
        Err(e) => {
            error!("PushOutputWriter: serialise error: {e}");
            return -3;
        }
    };

    let frame = IpcFrame::new(payload);
    let encoded = frame.encode();

    // Enqueue into bounded buffer (non-blocking).
    // If full, drop the oldest frame so the newest always gets through.
    let (lock, cvar) = PUSH_BUFFER.get().expect("PUSH_BUFFER not initialized");
    let mut guard = lock.lock().unwrap();
    if guard.len() >= PUSH_BUFFER_CAPACITY {
        guard.pop_front();
        PUSH_DROPPED_COUNT.fetch_add(1, AtomicOrdering::Relaxed);
    }
    guard.push_back(encoded);
    drop(guard);
    cvar.notify_one();

    0
}

type RegisterPushWriterFn = unsafe extern "C" fn(neomind_extension_sdk::PushOutputWriterFn) -> i32;

struct NativeExtensionBridge {
    _library: Arc<libloading::Library>,
    free_string: FreeStringFn,
    descriptor_json: JsonFn0,
    set_capability_bridge: Option<SetCapabilityBridgeFn>,
    register_push_writer: Option<RegisterPushWriterFn>,
    execute_command_json: JsonFn1,
    configure_json: Option<JsonFn1>,
    produce_metrics_json: JsonFn0,
    health_check_json: Option<JsonFn0>,
    stats_json: Option<JsonFn0>,
    event_subscriptions_json: Option<JsonFn0>,
    stream_capability_json: Option<JsonFn0>,
    init_session_json: Option<JsonFn1>,
    process_session_chunk_json: Option<JsonFn1>,
    close_session_json: Option<JsonFn1>,
    process_chunk_json: Option<JsonFn1>,
    start_push_json: Option<JsonFn1>,
    stop_push_json: Option<JsonFn1>,
    handle_event_json: Option<JsonFn1>,
}

impl NativeExtensionBridge {
    fn load(path: &Path) -> Result<(Self, ExtensionDescriptor), String> {
        let library = Arc::new(
            unsafe { libloading::Library::new(path) }
                .map_err(|e| format!("Failed to load native extension library: {}", e))?,
        );

        let abi_version = Self::load_symbol::<unsafe extern "C" fn() -> u32>(
            &library,
            b"neomind_extension_abi_version\0",
        )?;
        let version = unsafe { abi_version() };
        if version != ABI_VERSION {
            return Err(format!(
                "Incompatible ABI version: expected {}, got {}",
                ABI_VERSION, version
            ));
        }

        let bridge = Self {
            free_string: Self::load_symbol(&library, b"neomind_extension_free_string\0")?,
            descriptor_json: Self::load_symbol(&library, b"neomind_extension_descriptor_json\0")?,
            set_capability_bridge: Self::load_optional_symbol(
                &library,
                b"neomind_extension_set_capability_bridge\0",
            ),
            register_push_writer: Self::load_optional_symbol(
                &library,
                b"neomind_extension_register_push_writer\0",
            ),
            execute_command_json: Self::load_symbol(
                &library,
                b"neomind_extension_execute_command_json\0",
            )?,
            configure_json: Self::load_optional_symbol(
                &library,
                b"neomind_extension_configure_json\0",
            ),
            produce_metrics_json: Self::load_symbol(
                &library,
                b"neomind_extension_produce_metrics_json\0",
            )?,
            health_check_json: Self::load_optional_symbol(
                &library,
                b"neomind_extension_health_check_json\0",
            ),
            stats_json: Self::load_optional_symbol(&library, b"neomind_extension_stats_json\0"),
            event_subscriptions_json: Self::load_optional_symbol(
                &library,
                b"neomind_extension_event_subscriptions_json\0",
            ),
            stream_capability_json: Self::load_optional_symbol(
                &library,
                b"neomind_extension_stream_capability_json\0",
            ),
            init_session_json: Self::load_optional_symbol(
                &library,
                b"neomind_extension_init_session_json\0",
            ),
            process_session_chunk_json: Self::load_optional_symbol(
                &library,
                b"neomind_extension_process_session_chunk_json\0",
            ),
            close_session_json: Self::load_optional_symbol(
                &library,
                b"neomind_extension_close_session_json\0",
            ),
            process_chunk_json: Self::load_optional_symbol(
                &library,
                b"neomind_extension_process_chunk_json\0",
            ),
            start_push_json: Self::load_optional_symbol(
                &library,
                b"neomind_extension_start_push_json\0",
            ),
            stop_push_json: Self::load_optional_symbol(
                &library,
                b"neomind_extension_stop_push_json\0",
            ),
            handle_event_json: Self::load_optional_symbol(
                &library,
                b"neomind_extension_handle_event_json\0",
            ),
            _library: library,
        };

        let descriptor_response = bridge.call_json0(bridge.descriptor_json)?;
        let descriptor_json = if descriptor_response
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            descriptor_response
                .get("descriptor")
                .cloned()
                .ok_or("Missing descriptor in native response".to_string())?
        } else {
            return Err(Self::extract_error(&descriptor_response));
        };

        let descriptor = WasmRuntime::parse_descriptor_json(&descriptor_json)?;
        Ok((bridge, descriptor))
    }

    fn install_capability_bridge(&self, ipc_client: Arc<SyncIpcClient>) {
        let _ = GLOBAL_NATIVE_IPC_CLIENT.set(ipc_client);
        if let Some(set_bridge) = self.set_capability_bridge {
            if let Err(e) = safe_ffi_call(
                "install_capability_bridge",
                AssertUnwindSafe(|| unsafe {
                    set_bridge(
                        runner_native_capability_invoke,
                        runner_native_capability_free,
                    );
                }),
            ) {
                error!(error = %e, "Failed to install capability bridge");
            }
        }

        // Register push-output writer so extension can push data to host
        if let Some(register) = self.register_push_writer {
            if let Err(e) = safe_ffi_call(
                "register_push_writer",
                AssertUnwindSafe(|| unsafe { register(push_output_writer) }),
            ) {
                error!(error = %e, "Failed to register push output writer");
            } else {
                debug!("Push output writer registered successfully");
            }
        }
    }

    fn execute_command(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let response = self.call_json1(
            self.execute_command_json,
            &json!({ "command": command, "args": args }),
        )?;
        Self::extract_success_value(&response, "result")
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>, String> {
        let response = self.call_json0(self.produce_metrics_json)?;
        let metrics = Self::extract_success_value(&response, "metrics")?;
        serde_json::from_value(metrics).map_err(|e| format!("Failed to parse metrics JSON: {}", e))
    }

    /// Re-fetch a fresh descriptor from the extension by re-invoking the
    /// `descriptor_json` FFI symbol. The macro-generated implementation
    /// rebuilds the descriptor on every call, so any runtime changes to
    /// `metrics()` / `commands()` are reflected. Used to support dynamic
    /// metrics discovery.
    fn get_descriptor_fresh(&self) -> Result<ExtensionDescriptor, String> {
        let descriptor_response = self.call_json0(self.descriptor_json)?;
        let descriptor_json = if descriptor_response
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            descriptor_response
                .get("descriptor")
                .cloned()
                .ok_or("Missing descriptor in native response".to_string())?
        } else {
            return Err(Self::extract_error(&descriptor_response));
        };
        WasmRuntime::parse_descriptor_json(&descriptor_json)
    }

    fn configure(&self, config: &serde_json::Value) -> Result<(), String> {
        self.call_unit_json(self.configure_json, config)
    }

    fn health_check(&self) -> bool {
        let Some(func) = self.health_check_json else {
            return true;
        };
        match self.call_json0(func) {
            Ok(response) => {
                response
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                    && response
                        .get("healthy")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
            }
            Err(_) => false,
        }
    }

    fn get_stats(&self) -> ExtensionStats {
        let Some(func) = self.stats_json else {
            return ExtensionStats::default();
        };
        let Ok(response) = self.call_json0(func) else {
            return ExtensionStats::default();
        };
        if !response
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return ExtensionStats::default();
        }
        response
            .get("stats")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    }

    fn event_subscriptions(&self) -> Vec<String> {
        let Some(func) = self.event_subscriptions_json else {
            return Vec::new();
        };
        let Ok(response) = self.call_json0(func) else {
            return Vec::new();
        };
        if !response
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return Vec::new();
        }
        response
            .get("event_types")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    }

    fn get_stream_capability(&self) -> Result<Option<StreamCapability>, String> {
        let Some(func) = self.stream_capability_json else {
            return Ok(None);
        };
        let response = self.call_json0(func)?;
        if !response
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return Err(Self::extract_error(&response));
        }
        match response.get("capability") {
            Some(value) if !value.is_null() => serde_json::from_value(value.clone())
                .map(Some)
                .map_err(|e| format!("Failed to parse native stream capability JSON: {}", e)),
            _ => Ok(None),
        }
    }

    fn init_session(&self, session_id: &str, config: serde_json::Value) -> Result<(), String> {
        self.call_unit_json(
            self.init_session_json,
            &json!({ "session_id": session_id, "config": config }),
        )
    }

    fn process_session_chunk(
        &self,
        session_id: &str,
        chunk: StreamDataChunk,
    ) -> Result<StreamResult, String> {
        let chunk = DataChunk {
            sequence: chunk.sequence,
            data_type: StreamDataType::from_mime_type(&chunk.data_type)
                .unwrap_or(StreamDataType::Binary),
            data: chunk.data,
            timestamp: chunk.timestamp,
            metadata: None,
            is_last: chunk.is_last,
        };
        let response = self.call_required_json1(
            self.process_session_chunk_json,
            "native process_session_chunk",
            &json!({ "session_id": session_id, "chunk": chunk }),
        )?;
        let result = Self::extract_success_value(&response, "result")?;
        serde_json::from_value(result)
            .map_err(|e| format!("Failed to parse native stream result JSON: {}", e))
    }

    fn close_session(&self, session_id: &str) -> Result<SessionStats, String> {
        let response = self.call_required_json1(
            self.close_session_json,
            "native close_session",
            &json!({ "session_id": session_id }),
        )?;
        let stats = Self::extract_success_value(&response, "stats")?;
        serde_json::from_value(stats)
            .map_err(|e| format!("Failed to parse native session stats JSON: {}", e))
    }

    fn process_chunk(&self, chunk: StreamDataChunk) -> Result<StreamResult, String> {
        let chunk = DataChunk {
            sequence: chunk.sequence,
            data_type: StreamDataType::from_mime_type(&chunk.data_type)
                .unwrap_or(StreamDataType::Binary),
            data: chunk.data,
            timestamp: chunk.timestamp,
            metadata: None,
            is_last: chunk.is_last,
        };
        let response = self.call_required_json1(
            self.process_chunk_json,
            "native process_chunk",
            &json!({ "chunk": chunk }),
        )?;
        let result = Self::extract_success_value(&response, "result")?;
        serde_json::from_value(result)
            .map_err(|e| format!("Failed to parse native chunk result JSON: {}", e))
    }

    fn start_push(&self, session_id: &str) -> Result<(), String> {
        self.call_unit_json(self.start_push_json, &json!({ "session_id": session_id }))
    }

    fn stop_push(&self, session_id: &str) -> Result<(), String> {
        self.call_unit_json(self.stop_push_json, &json!({ "session_id": session_id }))
    }

    fn handle_event(&self, event_type: &str, payload: &serde_json::Value) -> Result<(), String> {
        self.call_unit_json(
            self.handle_event_json,
            &json!({ "event_type": event_type, "payload": payload }),
        )
    }

    fn call_unit_json(
        &self,
        func: Option<JsonFn1>,
        input: &serde_json::Value,
    ) -> Result<(), String> {
        let response = self.call_required_json1(func, "native bridge function", input)?;
        if response
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            Ok(())
        } else {
            Err(Self::extract_error(&response))
        }
    }

    fn call_required_json1(
        &self,
        func: Option<JsonFn1>,
        name: &str,
        input: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let func = func.ok_or_else(|| format!("{} is not supported by this extension", name))?;
        self.call_json1(func, input)
    }

    fn call_json0(&self, func: JsonFn0) -> Result<serde_json::Value, String> {
        let SendPtr(ptr) = safe_ffi_call_with_timeout(
            "call_json0",
            AssertUnwindSafe(move || unsafe { SendPtr(func()) }),
        )?;
        self.read_json_ptr(ptr)
    }

    fn call_json1(
        &self,
        func: JsonFn1,
        input: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let bytes = serde_json::to_vec(input)
            .map_err(|e| format!("Failed to serialize native bridge input: {}", e))?;
        let SendPtr(ptr) = safe_ffi_call_with_timeout(
            "call_json1",
            AssertUnwindSafe(move || unsafe { SendPtr(func(bytes.as_ptr(), bytes.len())) }),
        )?;
        self.read_json_ptr(ptr)
    }

    fn read_json_ptr(&self, ptr: *mut c_char) -> Result<serde_json::Value, String> {
        if ptr.is_null() {
            return Err("Native extension returned a null JSON pointer".to_string());
        }
        let json_string = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().to_string();
        safe_ffi_call(
            "free_string",
            AssertUnwindSafe(|| unsafe { (self.free_string)(ptr) }),
        )?;
        serde_json::from_str(&json_string)
            .map_err(|e| format!("Failed to parse native extension JSON response: {}", e))
    }

    fn extract_success_value(
        response: &serde_json::Value,
        key: &str,
    ) -> Result<serde_json::Value, String> {
        if response
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            response
                .get(key)
                .cloned()
                .ok_or_else(|| format!("Missing '{}' in native extension response", key))
        } else {
            Err(Self::extract_error(response))
        }
    }

    fn extract_error(response: &serde_json::Value) -> String {
        response
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Native extension returned an unknown error")
            .to_string()
    }

    fn load_symbol<T: Copy>(library: &libloading::Library, name: &[u8]) -> Result<T, String> {
        let symbol: libloading::Symbol<T> = unsafe { library.get(name) }.map_err(|e| {
            format!(
                "Failed to load symbol {:?}: {}",
                String::from_utf8_lossy(name),
                e
            )
        })?;
        Ok(*symbol)
    }

    fn load_optional_symbol<T: Copy>(library: &libloading::Library, name: &[u8]) -> Option<T> {
        let symbol: libloading::Symbol<T> = unsafe { library.get(name).ok()? };
        Some(*symbol)
    }
}

/// WASM runtime state
struct WasmRuntime {
    engine: Engine,
    module: Module,
    module_name: String,
    metric_values: Arc<RwLock<HashMap<String, serde_json::Value>>>,
}

/// Result buffer offset for WASM (matches SDK)
const WASM_RESULT_OFFSET: usize = 65536;

impl WasmRuntime {
    fn new(path: &PathBuf, module_name: String) -> Result<Self, String> {
        // Configure wasmtime engine
        let mut config = Config::new();
        config.async_support(true);
        config.consume_fuel(true);
        config.async_stack_size(8 * 1024 * 1024);

        // `Config::static_memory_maximum_size` was removed in wasmtime 36+. It
        // set the static-allocation *virtual* memory upper bound (a memory
        // protection optimization), NOT a real cap on `memory.growth`. WASM is
        // bounded here by fuel (above) + the module size check (below). To truly
        // cap linear-memory growth, set a per-store ResourceLimiter — TODO
        // (NEOMIND_WASM_MEMORY_MB env dropped; re-add via ResourceLimiter).

        let engine =
            Engine::new(&config).map_err(|e| format!("Failed to create WASM engine: {}", e))?;

        // Check WASM file size before loading (default 50 MB max, configurable)
        let max_wasm_size = std::env::var("NEOMIND_WASM_MAX_SIZE_MB")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(50)
            * 1024
            * 1024;
        match std::fs::metadata(path) {
            Ok(meta) => {
                let size = meta.len();
                if size > max_wasm_size {
                    return Err(format!(
                        "WASM module too large: {} bytes (max {} bytes)",
                        size, max_wasm_size
                    ));
                }
            }
            Err(e) => return Err(format!("Cannot read WASM file metadata: {}", e)),
        }

        // Load module
        let module = Module::from_file(&engine, path)
            .map_err(|e| format!("Failed to load WASM module: {}", e))?;

        Ok(Self {
            engine,
            module,
            module_name,
            metric_values: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Get the extension descriptor from the WASM module (blocking version)
    fn get_descriptor_blocking(&self) -> Result<ExtensionDescriptor, String> {
        let engine = self.engine.clone();
        let module = self.module.clone();

        let runtime_handle = tokio::runtime::Handle::try_current()
            .map_err(|e| format!("Failed to get runtime handle: {}", e))?;
        tokio::task::block_in_place(|| {
            runtime_handle.block_on(async { self.get_descriptor_async(&engine, &module).await })
        })
    }

    /// Get the extension descriptor from the WASM module (async version)
    async fn get_descriptor_async(
        &self,
        engine: &Engine,
        module: &Module,
    ) -> Result<ExtensionDescriptor, String> {
        // Create linker with WASI support
        let mut linker = Linker::new(engine);
        preview1::add_to_linker_async(&mut linker, |t: &mut HostState| &mut t.wasi)
            .map_err(|e| format!("Failed to add WASI: {}", e))?;

        // Add neomind host functions for capability invocation
        add_neomind_to_linker(&mut linker)?;

        let wasi = WasiCtxBuilder::new().inherit_stdio().build_p1();

        let host_state = HostState::new(wasi);

        let mut store = Store::new(engine, host_state);
        store
            .set_fuel(
                std::env::var("NEOMIND_WASM_FUEL")
                    .ok()
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(1_000_000),
            )
            .map_err(|e| format!("Failed to set fuel: {}", e))?;

        // Instantiate module
        let instance = linker
            .instantiate_async(&mut store, module)
            .await
            .map_err(|e| format!("Failed to instantiate module: {}", e))?;

        // Get memory
        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| "Module does not export 'memory'".to_string())?;
        store.data_mut().memory = Some(memory);

        // Try to call get_descriptor_json function
        let func = instance
            .get_func(&mut store, "get_descriptor_json")
            .ok_or_else(|| "Function 'get_descriptor_json' not found".to_string())?;

        let mut results = [Val::I32(0)];
        func.call_async(&mut store, &[], &mut results)
            .await
            .map_err(|e| format!("Failed to call get_descriptor_json: {}", e))?;

        let result_len = match results[0] {
            Val::I32(len) => len as usize,
            _ => return Err("Invalid return type from get_descriptor_json".to_string()),
        };

        if result_len == 0 || result_len >= 65536 {
            return Err(format!("Invalid result length: {}", result_len));
        }

        // Read result from memory
        let memory = store
            .data()
            .memory
            .ok_or_else(|| "WASM memory not available".to_string())?;
        let mut result_bytes = vec![0u8; result_len];
        memory
            .read(&store, WASM_RESULT_OFFSET, &mut result_bytes)
            .map_err(|e| format!("Failed to read result: {}", e))?;

        let json_str = String::from_utf8_lossy(&result_bytes);
        let descriptor_json: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| format!("Failed to parse descriptor JSON: {}", e))?;

        // Parse the descriptor JSON
        Self::parse_descriptor_json(&descriptor_json)
    }

    /// Parse descriptor JSON into ExtensionDescriptor
    fn parse_descriptor_json(json: &serde_json::Value) -> Result<ExtensionDescriptor, String> {
        use {
            ExtensionCommand, ExtensionMetadata, MetricDataType, MetricDescriptor,
            ParameterDefinition,
        };

        let metadata_json = json
            .get("metadata")
            .ok_or("Missing 'metadata' in descriptor")?;

        // Parse metadata
        let id = metadata_json
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or("Missing 'id' in metadata")?
            .to_string();
        let name = metadata_json
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("Missing 'name' in metadata")?
            .to_string();
        let version_str = metadata_json
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("1.0.0");
        let version = semver::Version::parse(version_str).unwrap_or(semver::Version::new(1, 0, 0));
        let description = metadata_json
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let author = metadata_json
            .get("author")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut metadata = ExtensionMetadata::new(id, name, version.to_string());
        metadata.description = description;
        metadata.author = author;

        // Parse config_parameters from metadata JSON
        if let Some(config_params_json) = metadata_json.get("config_parameters") {
            if let Ok(config_params) =
                serde_json::from_value::<Vec<ParameterDefinition>>(config_params_json.clone())
            {
                metadata.config_parameters = Some(config_params);
            }
        }

        // Parse metrics
        let metrics: Vec<MetricDescriptor> = json
            .get("metrics")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| {
                        let name = m.get("name")?.as_str()?.to_string();
                        let display_name = m
                            .get("display_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&name)
                            .to_string();
                        let data_type_str = m
                            .get("data_type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("string");
                        let data_type = match data_type_str {
                            "float" => MetricDataType::Float,
                            "integer" => MetricDataType::Integer,
                            "boolean" => MetricDataType::Boolean,
                            "binary" => MetricDataType::Binary,
                            _ => MetricDataType::String,
                        };
                        let unit = m
                            .get("unit")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let min = m.get("min").and_then(|v| v.as_f64());
                        let max = m.get("max").and_then(|v| v.as_f64());
                        let required = m.get("required").and_then(|v| v.as_bool()).unwrap_or(false);

                        Some(MetricDescriptor {
                            name,
                            display_name,
                            data_type,
                            unit,
                            min,
                            max,
                            required,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Parse commands
        let commands: Vec<ExtensionCommand> = json
            .get("commands")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| {
                        let name = c.get("name")?.as_str()?.to_string();
                        let display_name = c
                            .get("display_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&name)
                            .to_string();
                        let description = c
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        // Parse parameters
                        let parameters: Vec<ParameterDefinition> = c
                            .get("parameters")
                            .and_then(|v| v.as_array())
                            .map(|params| {
                                params
                                    .iter()
                                    .filter_map(|p| {
                                        let param_name = p.get("name")?.as_str()?.to_string();
                                        let param_display_name = p
                                            .get("display_name")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or(&param_name)
                                            .to_string();
                                        let param_desc = p
                                            .get("description")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("")
                                            .to_string();
                                        let param_type_str = p
                                            .get("param_type")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("string");
                                        let param_type = match param_type_str {
                                            "float" => MetricDataType::Float,
                                            "integer" => MetricDataType::Integer,
                                            "boolean" => MetricDataType::Boolean,
                                            "binary" => MetricDataType::Binary,
                                            _ => MetricDataType::String,
                                        };
                                        let required = p
                                            .get("required")
                                            .and_then(|v| v.as_bool())
                                            .unwrap_or(true);

                                        Some(ParameterDefinition {
                                            name: param_name,
                                            display_name: param_display_name,
                                            description: param_desc,
                                            param_type,
                                            required,
                                            default_value: None,
                                            min: None,
                                            max: None,
                                            options: Vec::new(),
                                        })
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();

                        // Parse samples
                        let samples: Vec<serde_json::Value> = c
                            .get("samples")
                            .and_then(|v| v.as_array())
                            .cloned()
                            .unwrap_or_default();

                        Some(ExtensionCommand {
                            name,
                            display_name,
                            description,
                            payload_template: String::new(),
                            parameters,
                            fixed_values: HashMap::new(),
                            samples,
                            parameter_groups: Vec::new(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(ExtensionDescriptor::with_capabilities(
            metadata, commands, metrics,
        ))
    }

    /// Execute a command using the new execute_command_json function
    async fn execute_command(
        &self,
        command: &str,
        args: &serde_json::Value,
        ipc_client: Option<Arc<SyncIpcClient>>,
    ) -> Result<serde_json::Value, String> {
        let input = serde_json::to_string(&json!({
            "command": command,
            "args": args
        }))
        .map_err(|e| format!("Failed to serialize input: {}", e))?;

        let module = self.module.clone();
        let engine = self.engine.clone();
        let input_bytes = input.into_bytes();
        let input_len = input_bytes.len();
        let metric_values = self.metric_values.clone();

        // Execute with timeout
        let result = tokio::time::timeout(tokio::time::Duration::from_secs(30), async move {
            // Create linker with WASI support
            let mut linker = Linker::new(&engine);
            preview1::add_to_linker_async(&mut linker, |t: &mut HostState| &mut t.wasi)
                .map_err(|e| format!("Failed to add WASI: {}", e))?;

            // Add neomind host functions
            add_neomind_to_linker(&mut linker)?;

            let wasi = WasiCtxBuilder::new().inherit_stdio().build_p1();

            // Pass IPC client to host state for capability forwarding
            let host_state = HostState::with_ipc(wasi, ipc_client);

            let mut store = Store::new(&engine, host_state);
            store
                .set_fuel(
                    std::env::var("NEOMIND_WASM_FUEL")
                        .ok()
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(1_000_000),
                )
                .map_err(|e| format!("Failed to set fuel: {}", e))?;

            let instance = linker
                .instantiate_async(&mut store, &module)
                .await
                .map_err(|e| format!("Failed to instantiate module: {}", e))?;

            let memory = instance
                .get_memory(&mut store, "memory")
                .ok_or_else(|| "Module does not export 'memory'".to_string())?;
            store.data_mut().memory = Some(memory);

            // Try execute_command_json first
            if let Some(func) = instance.get_func(&mut store, "execute_command_json") {
                // Write input to memory at offset 0
                memory
                    .write(&mut store, 0, &input_bytes)
                    .map_err(|e| format!("Failed to write input: {}", e))?;

                let mut results = [Val::I32(0)];
                let params = [Val::I32(0), Val::I32(input_len as i32)];

                func.call_async(&mut store, &params, &mut results)
                    .await
                    .map_err(|e| format!("execute_command_json call failed: {}", e))?;

                let result_len = match results[0] {
                    Val::I32(len) => len as usize,
                    _ => 0,
                };

                if result_len > 0 && result_len < 65536 {
                    let mut result_bytes = vec![0u8; result_len];
                    memory
                        .read(&store, WASM_RESULT_OFFSET, &mut result_bytes)
                        .map_err(|e| format!("Failed to read result: {}", e))?;

                    let result_str = String::from_utf8_lossy(&result_bytes);
                    let result_json: serde_json::Value = serde_json::from_str(&result_str)
                        .map_err(|e| format!("Failed to parse result JSON: {}", e))?;

                    // Cache metric values if present
                    if let Some(metrics) = result_json.get("metrics").and_then(|v| v.as_array()) {
                        let mut values = metric_values.write().await;
                        for m in metrics {
                            if let (Some(name), Some(value)) =
                                (m.get("name").and_then(|n| n.as_str()), m.get("value"))
                            {
                                values.insert(name.to_string(), value.clone());
                            }
                        }
                    }

                    return Ok(result_json);
                }
            }

            // Fallback: try the old execute function
            Err("execute_command_json not found, extension may not support new API".to_string())
        })
        .await;

        result.map_err(|_| "Execution timeout".to_string())?
    }

    /// Legacy execute function for backward compatibility
    async fn execute(
        &self,
        function_name: &str,
        args: &serde_json::Value,
        ipc_client: Option<Arc<SyncIpcClient>>,
    ) -> Result<serde_json::Value, String> {
        let args_str =
            serde_json::to_string(args).map_err(|e| format!("Failed to serialize args: {}", e))?;

        let module = self.module.clone();
        let engine = self.engine.clone();
        let function_name_owned = function_name.to_string();
        let module_name = self.module_name.clone();
        let metric_values = self.metric_values.clone();

        // Execute with timeout
        let result = tokio::time::timeout(tokio::time::Duration::from_secs(30), async move {
            // Create linker
            let mut linker = Linker::new(&engine);

            // Add WASI support
            preview1::add_to_linker_async(&mut linker, |t: &mut HostState| &mut t.wasi)
                .map_err(|e| format!("Failed to add WASI: {}", e))?;

            // Add neomind host functions
            add_neomind_to_linker(&mut linker)?;

            // Build WASI context
            let wasi = WasiCtxBuilder::new().inherit_stdio().build_p1();

            let host_state = HostState::with_ipc(wasi, ipc_client);

            // Create store with fuel
            let mut store = Store::new(&engine, host_state);
            store
                .set_fuel(
                    std::env::var("NEOMIND_WASM_FUEL")
                        .ok()
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(1_000_000),
                )
                .map_err(|e| format!("Failed to set fuel: {}", e))?;

            // Instantiate module
            let instance = linker
                .instantiate_async(&mut store, &module)
                .await
                .map_err(|e| format!("Failed to instantiate module: {}", e))?;

            // Get memory
            let memory = instance
                .get_memory(&mut store, "memory")
                .ok_or_else(|| "Module does not export 'memory'".to_string())?;
            store.data_mut().memory = Some(memory);

            // Get function
            let func = instance
                .get_func(&mut store, &function_name_owned)
                .ok_or_else(|| format!("Function '{}' not found", function_name_owned))?;

            let func_ty = func.ty(store.as_context_mut());
            let params_count = func_ty.params().len();
            let results_count = func_ty.results().len();

            // Call function based on signature
            if params_count == 0 && results_count == 0 {
                let mut results = [];
                func.call_async(&mut store, &[], &mut results)
                    .await
                    .map_err(|e| format!("Function call failed: {}", e))?;

                Ok(json!({
                    "success": true,
                    "message": format!("Function {} executed", function_name_owned),
                    "module": module_name
                }))
            } else if params_count == 2 && results_count == 1 {
                // Standard signature: (args_ptr: i32, args_len: i32) -> result_len: i32
                let args_bytes = args_str.as_bytes();
                let args_len = args_bytes.len();

                memory
                    .write(&mut store, 0, args_bytes)
                    .map_err(|e| format!("Failed to write args: {}", e))?;

                let params = [Val::I32(0), Val::I32(args_len as i32)];
                let mut results = [Val::I32(0)];

                func.call_async(&mut store, &params, &mut results)
                    .await
                    .map_err(|e| format!("Function call failed: {}", e))?;

                let result_len = match results[0] {
                    Val::I32(len) => len as usize,
                    _ => 0,
                };

                if result_len > 0 && result_len < 65536 {
                    let mut result_bytes = vec![0u8; result_len];
                    memory
                        .read(&store, WASM_RESULT_OFFSET, &mut result_bytes)
                        .map_err(|e| format!("Failed to read result: {}", e))?;

                    let result_str = String::from_utf8_lossy(&result_bytes);
                    let result_json: serde_json::Value = serde_json::from_str(&result_str)
                        .unwrap_or_else(|_| {
                            json!({
                                "success": true,
                                "raw_result": result_str.to_string()
                            })
                        });

                    // Cache metric values
                    if let Some(obj) = result_json.as_object() {
                        let mut values = metric_values.write().await;
                        for (key, value) in obj {
                            values.insert(key.clone(), value.clone());
                        }
                    }

                    Ok(result_json)
                } else {
                    Ok(json!({
                        "success": true,
                        "message": format!("Function {} executed", function_name_owned),
                        "result_length": result_len
                    }))
                }
            } else {
                Ok(json!({
                    "success": true,
                    "message": format!("Function {} found", function_name_owned),
                    "params_count": params_count,
                    "results_count": results_count,
                    "note": "Custom function signature"
                }))
            }
        })
        .await;

        result.map_err(|_| "Execution timeout".to_string())?
    }

    async fn health_check(&self) -> bool {
        match self.execute("health", &json!({}), None).await {
            Ok(result) => result.as_bool().unwrap_or(true),
            Err(_) => true, // Assume healthy if function not found
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>, String> {
        use ExtensionMetricValue;

        let values = self
            .metric_values
            .try_read()
            .map_err(|_| "Lock error".to_string())?;

        // For WASM, we don't have metric descriptors, so return raw values
        let mut result = Vec::new();
        for (name, value) in values.iter() {
            let metric_value = if let Some(f) = value.as_f64() {
                Some(ExtensionMetricValue {
                    name: name.clone(),
                    value: f.into(),
                    timestamp: chrono::Utc::now().timestamp_millis(),
                })
            } else if let Some(i) = value.as_i64() {
                Some(ExtensionMetricValue {
                    name: name.clone(),
                    value: i.into(),
                    timestamp: chrono::Utc::now().timestamp_millis(),
                })
            } else if let Some(b) = value.as_bool() {
                Some(ExtensionMetricValue {
                    name: name.clone(),
                    value: b.into(),
                    timestamp: chrono::Utc::now().timestamp_millis(),
                })
            } else {
                value.as_str().map(|s| ExtensionMetricValue {
                    name: name.clone(),
                    value: s.into(),
                    timestamp: chrono::Utc::now().timestamp_millis(),
                })
            };

            if let Some(v) = metric_value {
                result.push(v);
            }
        }

        Ok(result)
    }
}

/// Host state for WASM execution
struct HostState {
    wasi: WasiP1Ctx,
    memory: Option<Memory>,
    /// IPC client for capability invocation (communicates with main process)
    /// Uses sync channels for synchronous WASM host function calls
    ipc_client: Option<Arc<SyncIpcClient>>,
}

/// Synchronous IPC client for capability invocation
///
/// This client sends CapabilityRequest messages via stdout and waits for
/// CapabilityResult responses via the pending requests queue (routed by main loop).
pub struct SyncIpcClient {}

impl Default for SyncIpcClient {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncIpcClient {
    /// Create a new sync IPC client
    pub fn new() -> Self {
        Self {}
    }

    /// Invoke a capability synchronously
    ///
    /// Sends CapabilityRequest via stdout and waits for CapabilityResult
    /// via the pending requests queue (routed by main loop).
    pub fn invoke(&self, capability: &str, params: &serde_json::Value) -> serde_json::Value {
        use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

        static REQUEST_ID_COUNTER: AtomicU64 = AtomicU64::new(1);
        let request_id = REQUEST_ID_COUNTER.fetch_add(1, AtomicOrdering::SeqCst);

        debug!(capability, request_id, "SyncIpcClient invoke");

        // Register pending request BEFORE sending
        let response_rx = register_pending_request(request_id);

        // Send CapabilityRequest via stdout
        let request = IpcResponse::CapabilityRequest {
            request_id,
            capability: capability.to_string(),
            params: params.clone(),
        };

        let payload = match request.to_bytes() {
            Ok(p) => p,
            Err(e) => {
                error!("SyncIpcClient: failed to serialize request: {e}");
                // Remove pending request on error
                get_pending_requests().remove(&request_id);
                return json!({"success": false, "error": format!("Failed to serialize request: {}", e)});
            }
        };

        let frame = IpcFrame::new(payload);
        let bytes = frame.encode();

        // Write to stdout (protected by global mutex)
        {
            let _guard = STDOUT_WRITE_MUTEX.lock().unwrap_or_else(|e| {
                error!("STDOUT_WRITE_MUTEX poisoned: {}", e);
                e.into_inner()
            });
            let mut stdout = std::io::stdout();
            if let Err(e) = stdout.write_all(&bytes) {
                drop(_guard);
                error!("SyncIpcClient: failed to write request: {e}");
                get_pending_requests().remove(&request_id);
                return json!({"success": false, "error": format!("Failed to write request: {}", e)});
            }
            if let Err(e) = stdout.flush() {
                drop(_guard);
                error!("SyncIpcClient: failed to flush request: {e}");
                get_pending_requests().remove(&request_id);
                return json!({"success": false, "error": format!("Failed to flush request: {}", e)});
            }
        }

        debug!("SyncIpcClient: request sent, waiting for response");

        // Wait for response from the pending requests queue (with timeout)
        match response_rx.recv_timeout(std::time::Duration::from_secs(30)) {
            Ok(response) => {
                debug!("SyncIpcClient: received response");
                match response {
                    IpcResponse::CapabilityResult {
                        request_id: resp_id,
                        result,
                        error,
                    } => {
                        if resp_id != request_id {
                            warn!(expected = request_id, got = resp_id, "Request ID mismatch");
                            return json!({"success": false, "error": "Request ID mismatch"});
                        }
                        if let Some(err) = error {
                            json!({"success": false, "error": err})
                        } else {
                            result
                        }
                    }
                    _ => {
                        warn!("SyncIpcClient: unexpected response type");
                        json!({"success": false, "error": format!("Unexpected response type")})
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                error!("SyncIpcClient: timeout waiting for response");
                get_pending_requests().remove(&request_id);
                json!({"success": false, "error": "Timeout waiting for response"})
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                error!("SyncIpcClient: response channel disconnected");
                get_pending_requests().remove(&request_id);
                json!({"success": false, "error": "Response channel disconnected"})
            }
        }
    }
}

impl HostState {
    /// Create a new host state with WASI context
    fn new(wasi: WasiP1Ctx) -> Self {
        Self {
            wasi,
            memory: None,
            ipc_client: None,
        }
    }

    /// Create host state with IPC capability client
    fn with_ipc(wasi: WasiP1Ctx, ipc_client: Option<Arc<SyncIpcClient>>) -> Self {
        Self {
            wasi,
            memory: None,
            ipc_client,
        }
    }
}

/// Add neomind host functions to the linker
///
/// This function registers all host functions that WASM extensions can call:
/// - `host_invoke_capability`: Universal capability invocation
/// - `host_event_subscribe`: Subscribe to events
/// - `host_event_poll`: Poll for events
/// - `host_event_unsubscribe`: Unsubscribe from events
/// - `host_free`: Free host-allocated memory
/// - `host_log`: Log a message
/// - `host_timestamp_ms`: Get current timestamp
fn add_neomind_to_linker(linker: &mut Linker<HostState>) -> Result<(), String> {
    // host_invoke_capability(
    //     capability_ptr: *const u8, capability_len: i32,
    //     params_ptr: *const u8, params_len: i32,
    //     result_ptr: *mut u8, result_max_len: i32
    // ) -> i32
    linker
        .func_wrap(
            "neomind",
            "host_invoke_capability",
            |mut caller: wasmtime::Caller<'_, HostState>,
             capability_ptr: i32,
             capability_len: i32,
             params_ptr: i32,
             params_len: i32,
             result_ptr: i32,
             result_max_len: i32|
             -> i32 {
                // Read capability name from memory
                let capability = match read_string_from_memory(
                    &mut caller,
                    capability_ptr as usize,
                    capability_len as usize,
                ) {
                    Ok(s) => s,
                    Err(_) => return -1,
                };

                // Read params from memory
                let params_str = match read_string_from_memory(
                    &mut caller,
                    params_ptr as usize,
                    params_len as usize,
                ) {
                    Ok(s) => s,
                    Err(_) => return -1,
                };

                let params: serde_json::Value = match serde_json::from_str(&params_str) {
                    Ok(v) => v,
                    Err(_) => return -1,
                };

                // Try to use IPC client if available (for real capability invocation)
                // Otherwise fall back to mock implementation
                let result = if let Some(ipc_client) = &caller.data().ipc_client {
                    debug!(
                        capability = %capability,
                        "Forwarding capability request via IPC"
                    );
                    ipc_client.invoke(&capability, &params)
                } else {
                    // Fallback to mock implementation for testing or when IPC is not available
                    debug!(
                        capability = %capability,
                        "Using mock capability handler (no IPC client)"
                    );
                    handle_capability_invocation(&capability, &params)
                };

                // Write result to memory
                let result_str = match serde_json::to_string(&result) {
                    Ok(s) => s,
                    Err(_) => return -1,
                };

                let result_bytes = result_str.as_bytes();
                let write_len = result_bytes.len().min(result_max_len as usize);

                match write_bytes_to_memory(
                    &mut caller,
                    result_ptr as usize,
                    &result_bytes[..write_len],
                ) {
                    Ok(_) => write_len as i32,
                    Err(_) => -1,
                }
            },
        )
        .map_err(|e| format!("Failed to add host_invoke_capability: {}", e))?;

    // host_event_subscribe(
    //     event_type_ptr: *const u8, event_type_len: i32,
    //     filter_ptr: *const u8, filter_len: i32
    // ) -> i64
    linker
        .func_wrap(
            "neomind",
            "host_event_subscribe",
            |mut caller: wasmtime::Caller<'_, HostState>,
             event_type_ptr: i32,
             event_type_len: i32,
             _filter_ptr: i32,
             _filter_len: i32|
             -> i64 {
                // Read event type
                let event_type = match read_string_from_memory(
                    &mut caller,
                    event_type_ptr as usize,
                    event_type_len as usize,
                ) {
                    Ok(s) => s,
                    Err(_) => return -1,
                };

                // Subscribe using global state
                let sub_id = get_global_event_state().subscribe(event_type);

                debug!(subscription_id = sub_id, "Event subscription created");
                sub_id
            },
        )
        .map_err(|e| format!("Failed to add host_event_subscribe: {}", e))?;

    // host_event_poll(subscription_id: i64, result_ptr: *mut u8, result_max_len: i32) -> i32
    linker
        .func_wrap(
            "neomind",
            "host_event_poll",
            |mut caller: wasmtime::Caller<'_, HostState>,
             subscription_id: i64,
             result_ptr: i32,
             result_max_len: i32|
             -> i32 {
                // Get events from global state
                let events = get_global_event_state().take_events(subscription_id);

                // Return events as JSON array
                let result = json!(events);
                let result_str = match serde_json::to_string(&result) {
                    Ok(s) => s,
                    Err(_) => return -1,
                };

                let result_bytes = result_str.as_bytes();
                let write_len = result_bytes.len().min(result_max_len as usize);

                match write_bytes_to_memory(
                    &mut caller,
                    result_ptr as usize,
                    &result_bytes[..write_len],
                ) {
                    Ok(_) => write_len as i32,
                    Err(_) => -1,
                }
            },
        )
        .map_err(|e| format!("Failed to add host_event_poll: {}", e))?;

    // host_event_unsubscribe(subscription_id: i64) -> i32
    linker
        .func_wrap(
            "neomind",
            "host_event_unsubscribe",
            |_caller: wasmtime::Caller<'_, HostState>, subscription_id: i64| -> i32 {
                if get_global_event_state().unsubscribe(subscription_id) {
                    debug!(subscription_id, "Event subscription removed");
                    0
                } else {
                    -1
                }
            },
        )
        .map_err(|e| format!("Failed to add host_event_unsubscribe: {}", e))?;

    // host_free(ptr: *const u8)
    linker
        .func_wrap(
            "neomind",
            "host_free",
            |_caller: wasmtime::Caller<'_, HostState>, _ptr: i32| {
                // No-op for now (memory management is handled by WASM linear memory)
            },
        )
        .map_err(|e| format!("Failed to add host_free: {}", e))?;

    // host_log(level_ptr: *const u8, level_len: i32, msg_ptr: *const u8, msg_len: i32)
    linker
        .func_wrap(
            "neomind",
            "host_log",
            |mut caller: wasmtime::Caller<'_, HostState>,
             level_ptr: i32,
             level_len: i32,
             msg_ptr: i32,
             msg_len: i32| {
                let level =
                    read_string_from_memory(&mut caller, level_ptr as usize, level_len as usize)
                        .unwrap_or_else(|_| "info".to_string());
                let msg = read_string_from_memory(&mut caller, msg_ptr as usize, msg_len as usize)
                    .unwrap_or_else(|_| "".to_string());

                match level.as_str() {
                    "error" => error!("{}", msg),
                    "warn" => tracing::warn!("{}", msg),
                    "debug" => debug!("{}", msg),
                    _ => info!("{}", msg),
                }
            },
        )
        .map_err(|e| format!("Failed to add host_log: {}", e))?;

    // host_timestamp_ms() -> i64
    linker
        .func_wrap(
            "neomind",
            "host_timestamp_ms",
            |_caller: wasmtime::Caller<'_, HostState>| -> i64 {
                use std::time::{SystemTime, UNIX_EPOCH};
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64
            },
        )
        .map_err(|e| format!("Failed to add host_timestamp_ms: {}", e))?;

    Ok(())
}

/// Maximum size for a single WASM memory read (1 MB)
const WASM_MAX_MEMORY_READ: usize = 1024 * 1024;

/// Helper function to read a string from WASM memory
fn read_string_from_memory(
    caller: &mut wasmtime::Caller<'_, HostState>,
    offset: usize,
    len: usize,
) -> Result<String, String> {
    if len == 0 {
        return Ok(String::new());
    }
    if len > WASM_MAX_MEMORY_READ {
        return Err(format!(
            "Memory read too large: {} bytes (max {})",
            len, WASM_MAX_MEMORY_READ
        ));
    }
    let memory = caller.data().memory.ok_or("Memory not set")?;
    let mut buffer = vec![0u8; len];
    memory
        .read(caller.as_context(), offset, &mut buffer)
        .map_err(|e| format!("Failed to read memory: {}", e))?;
    String::from_utf8(buffer).map_err(|e| format!("Invalid UTF-8: {}", e))
}

/// Helper function to write bytes to WASM memory
fn write_bytes_to_memory(
    caller: &mut wasmtime::Caller<'_, HostState>,
    offset: usize,
    data: &[u8],
) -> Result<(), String> {
    let memory = caller.data().memory.ok_or("Memory not set")?;
    memory
        .write(caller.as_context_mut(), offset, data)
        .map_err(|e| format!("Failed to write memory: {}", e))
}

/// Handle capability invocation from WASM extension
///
/// NOTE: This is a simplified implementation that returns mock data.
/// WASM extensions have limited capability access due to sandbox restrictions.
/// For full capability access, use Native extensions in non-isolated mode.
///
/// SYNC: Capability names are imported from context::capabilities
fn handle_capability_invocation(capability: &str, params: &serde_json::Value) -> serde_json::Value {
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

impl Runner {
    /// Load extension and create runner
    async fn load(extension_path: &PathBuf) -> Result<Self, String> {
        debug!("Runner::load called");
        let extension_type = ExtensionType::from_path(extension_path);
        debug!(
            path = %extension_path.display(),
            extension_type = ?extension_type,
            "Loading extension"
        );

        // Load the extension based on type
        let (extension, wasm_runtime, descriptor) = match extension_type {
            ExtensionType::Native => {
                let (ext, desc) = Self::load_native(extension_path).await?;
                (Some(ext), None, desc)
            }
            ExtensionType::Wasm => {
                let (runtime, descriptor) = Self::load_wasm(extension_path).await?;
                (None, Some(runtime), descriptor)
            }
        };

        debug!(
            commands_count = descriptor.commands.len(),
            "Extension loaded"
        );

        debug!(
            extension_id = %descriptor.metadata.id,
            name = %descriptor.metadata.name,
            version = %descriptor.metadata.version,
            extension_type = ?extension_type,
            commands_count = descriptor.commands.len(),
            metrics_count = descriptor.metrics.len(),
            "Extension loaded successfully"
        );

        let runtime_handle = tokio::runtime::Handle::try_current()
            .map_err(|e| format!("Failed to get current runtime handle: {}", e))?;

        // Create IPC client for capability forwarding (both Native and WASM)
        let ipc_client = {
            let client = SyncIpcClient::new();
            Some(Arc::new(client))
        };

        if let (Some(extension), Some(ipc_client)) = (extension.as_ref(), ipc_client.as_ref()) {
            extension.install_capability_bridge(ipc_client.clone());
        }

        Ok(Self {
            extension,
            wasm_runtime,
            descriptor,
            extension_type,
            runtime: runtime_handle,
            running: true,
            ipc_client,
        })
    }

    /// Load a native extension and return its descriptor
    async fn load_native(
        extension_path: &Path,
    ) -> Result<(NativeExtensionBridge, ExtensionDescriptor), String> {
        debug!("load_native called");

        // Pre-load validation: check dylib headers before loading
        // This catches issues like incorrect LC_ID_DYLIB on macOS
        if let Err(e) = validate_library(extension_path) {
            error!(
                path = %extension_path.display(),
                error = %e,
                "Library validation failed - this extension may crash on other machines"
            );
            return Err(format!(
                "Library validation failed: {}. The extension may have been built incorrectly. \
                 On macOS, ensure LC_ID_DYLIB is set to '@rpath/extension.dylib' using: \
                 install_name_tool -id '@rpath/extension.dylib' extension.dylib",
                e
            ));
        }

        // Use catch_unwind to catch panics during library loading
        // This prevents the runner process from crashing abruptly
        let path = extension_path.to_path_buf();
        let load_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            NativeExtensionBridge::load(&path)
        }));

        match load_result {
            Ok(Ok((bridge, descriptor))) => {
                info!(
                    "Descriptor created: id='{}', commands={}, metrics={}",
                    descriptor.metadata.id,
                    descriptor.commands.len(),
                    descriptor.metrics.len()
                );
                Ok((bridge, descriptor))
            }
            Ok(Err(e)) => {
                // Extension returned an error (not a panic)
                Err(e)
            }
            Err(panic_payload) => {
                // Library loading panicked - extract panic message
                let panic_msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown panic during library loading".to_string()
                };
                error!(
                    path = %extension_path.display(),
                    panic = %panic_msg,
                    "Native library loading panicked - extension is likely corrupted or incompatible"
                );
                Err(format!(
                    "Library loading panicked: {}. The extension binary may be corrupted, \
                     incompatible with this platform, or built with incorrect settings.",
                    panic_msg
                ))
            }
        }
    }

    /// Load a WASM extension with full descriptor support
    async fn load_wasm(
        extension_path: &PathBuf,
    ) -> Result<(WasmRuntime, ExtensionDescriptor), String> {
        // First, create the runtime
        let module_name = extension_path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let runtime = WasmRuntime::new(extension_path, module_name)?;

        // Try to get descriptor from WASM module itself
        match runtime.get_descriptor_blocking() {
            Ok(descriptor) => {
                debug!(
                    extension_id = %descriptor.metadata.id,
                    name = %descriptor.metadata.name,
                    version = %descriptor.metadata.version,
                    commands_count = descriptor.commands.len(),
                    metrics_count = descriptor.metrics.len(),
                    "Got descriptor from WASM module"
                );
                Ok((runtime, descriptor))
            }
            Err(e) => {
                debug!(error = %e, "Failed to get descriptor from WASM, trying sidecar files");

                // Fallback to sidecar JSON files
                let metadata = Self::load_wasm_metadata(extension_path)?;
                let descriptor = ExtensionDescriptor::new(metadata);
                Ok((runtime, descriptor))
            }
        }
    }

    /// Load WASM metadata (fallback from sidecar files)
    fn load_wasm_metadata(extension_path: &Path) -> Result<ExtensionMetadata, String> {
        // Try sidecar JSON
        let json_path = extension_path.with_extension("json");
        if json_path.exists() {
            if let Ok(meta) = Self::parse_metadata_json(&json_path) {
                return Ok(meta);
            }
        }

        // Try .nep manifest
        if let Some(manifest_path) = Self::find_nep_manifest(extension_path) {
            if manifest_path.exists() {
                if let Ok(meta) = Self::parse_metadata_json(&manifest_path) {
                    return Ok(meta);
                }
            }
        }

        // Fallback to filename
        let file_name = extension_path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        Ok(ExtensionMetadata::new(
            file_name.to_string(),
            format!("{} WASM Extension", file_name),
            "1.0.0",
        ))
    }

    fn parse_metadata_json(path: &PathBuf) -> Result<ExtensionMetadata, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("Failed to read JSON: {}", e))?;

        #[derive(serde::Deserialize)]
        struct MetaJson {
            id: String,
            name: String,
            version: String,
            #[serde(default)]
            description: Option<String>,
            #[serde(default)]
            author: Option<String>,
        }

        let json: MetaJson =
            serde_json::from_str(&content).map_err(|e| format!("Failed to parse JSON: {}", e))?;

        let version =
            semver::Version::parse(&json.version).unwrap_or(semver::Version::new(1, 0, 0));

        let mut meta = ExtensionMetadata::new(json.id, json.name, version.to_string());
        meta.description = json.description;
        meta.author = json.author;

        Ok(meta)
    }

    fn find_nep_manifest(wasm_path: &Path) -> Option<PathBuf> {
        let binaries_dir = wasm_path.parent()?;
        let wasm_dir = binaries_dir.parent()?;
        let extension_folder = wasm_dir.parent()?;

        let manifest = extension_folder.join("manifest.json");
        if manifest.exists() {
            return Some(manifest);
        }
        None
    }

    /// Run the main loop
    async fn run(&mut self) {
        debug!("Starting IPC message loop");

        // Note: We no longer send Ready here - we wait for Init message first
        // This ensures proper handshake sequence: host sends Init, we respond with Ready

        debug!("Waiting for Init message from host");

        // Create event channel BEFORE starting stdin reader to prevent race condition.
        // If stdin reader starts first, it may read the Init message before EVENT_TX
        // is initialized, causing the message to be silently dropped and a 120s timeout.
        let mut event_rx = create_event_channel();

        // Start stdin reader thread (reads all stdin messages and routes them)
        let _stdin_reader_handle = start_stdin_reader();

        // Start push-output stdout writer thread (drains buffer → stdout)
        PUSH_BUFFER
            .set((
                std::sync::Mutex::new(VecDeque::with_capacity(PUSH_BUFFER_CAPACITY)),
                std::sync::Condvar::new(),
            ))
            .expect("PUSH_BUFFER already initialized");
        std::thread::Builder::new()
            .name("push-stdout-writer".into())
            .spawn(push_stdout_writer_thread)
            .expect("Failed to spawn push stdout writer thread");
        debug!("Push stdout writer thread started");
        debug!("Stdin reader thread started");

        while self.running {
            // Channel recv blocks efficiently - zero CPU when idle
            tokio::select! {
                Some(message) = event_rx.recv() => {
                    debug!(
                        "Received IPC message from channel: {:?}",
                        std::mem::discriminant(&message)
                    );
                    self.handle_message(message).await;
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
                    // Periodic keepalive check
                    trace!("Extension runner keepalive");
                }
            }
        }

        debug!("Extension runner shutting down");
    }

    /// Start the IPC capability forwarder thread
    ///
    fn send_response(&mut self, response: IpcResponse) {
        debug!(response_type = ?std::mem::discriminant(&response), "Sending IPC response");

        // Debug: log StreamSessionInit responses specifically
        if let IpcResponse::StreamSessionInit {
            request_id,
            session_id,
            success,
            ..
        } = &response
        {
            debug!(
                request_id,
                session_id, success, "Sending StreamSessionInit response"
            );
        }

        let payload = match response.to_bytes() {
            Ok(p) => {
                debug!(payload_len = p.len(), "Response serialized successfully");
                p
            }
            Err(e) => {
                error!(error = %e, "Failed to serialize response");
                return;
            }
        };

        let frame = IpcFrame::new(payload);
        let bytes = frame.encode();

        debug!(frame_len = bytes.len(), "Frame encoded");

        let _guard = STDOUT_WRITE_MUTEX.lock().unwrap_or_else(|e| {
            error!("STDOUT_WRITE_MUTEX poisoned: {}", e);
            e.into_inner()
        });
        if let Err(e) = std::io::stdout().write_all(&bytes) {
            error!(error = %e, "Failed to write response");
            return;
        }
        if let Err(e) = std::io::stdout().flush() {
            error!(error = %e, "Failed to flush stdout");
        }

        // Debug: confirm StreamSessionInit was sent
        if let IpcResponse::StreamSessionInit {
            request_id,
            session_id,
            ..
        } = &response
        {
            debug!(request_id, session_id, "StreamSessionInit response sent");
        }
    }

    /// Send a capability request to the host and wait for response
    /// This is used for bidirectional IPC communication
    fn invoke_host_capability(
        &mut self,
        capability: &str,
        params: &serde_json::Value,
    ) -> serde_json::Value {
        use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

        static REQUEST_ID_COUNTER: AtomicU64 = AtomicU64::new(1);
        let request_id = REQUEST_ID_COUNTER.fetch_add(1, AtomicOrdering::SeqCst);

        debug!(
            request_id,
            capability = %capability,
            "Sending CapabilityRequest to host"
        );

        // Send CapabilityRequest
        let request = IpcResponse::CapabilityRequest {
            request_id,
            capability: capability.to_string(),
            params: params.clone(),
        };

        let payload = match request.to_bytes() {
            Ok(p) => p,
            Err(e) => {
                error!(error = %e, "Failed to serialize CapabilityRequest");
                return json!({"success": false, "error": "Failed to serialize request"});
            }
        };

        let frame = IpcFrame::new(payload);
        let bytes = frame.encode();

        {
            let _guard = STDOUT_WRITE_MUTEX.lock().unwrap_or_else(|e| {
                error!("STDOUT_WRITE_MUTEX poisoned: {}", e);
                e.into_inner()
            });
            if let Err(e) = std::io::stdout().write_all(&bytes) {
                drop(_guard);
                error!(error = %e, "Failed to write CapabilityRequest");
                return json!({"success": false, "error": "Failed to write request"});
            }
            if let Err(e) = std::io::stdout().flush() {
                drop(_guard);
                error!(error = %e, "Failed to flush CapabilityRequest");
                return json!({"success": false, "error": "Failed to flush request"});
            }
        }

        debug!(request_id, "CapabilityRequest sent, waiting for response");

        // Read response from host
        // Read length prefix (4 bytes)
        let mut len_bytes = [0u8; 4];
        match std::io::stdin().read_exact(&mut len_bytes) {
            Ok(_) => {}
            Err(e) => {
                error!(error = %e, "Failed to read response length");
                return json!({"success": false, "error": "Failed to read response length"});
            }
        }

        let len = u32::from_le_bytes(len_bytes) as usize;
        if len > 10 * 1024 * 1024 {
            error!(len, "Response too large");
            return json!({"success": false, "error": "Response too large"});
        }

        // Read payload
        let mut payload_buf = vec![0u8; len];
        match std::io::stdin().read_exact(&mut payload_buf) {
            Ok(_) => {}
            Err(e) => {
                error!(error = %e, "Failed to read response payload");
                return json!({"success": false, "error": "Failed to read response payload"});
            }
        }

        // Parse response
        let response: IpcResponse = match IpcResponse::from_bytes(&payload_buf) {
            Ok(r) => r,
            Err(e) => {
                error!(error = %e, "Failed to parse response");
                return json!({"success": false, "error": "Failed to parse response"});
            }
        };

        match response {
            IpcResponse::CapabilityResult {
                request_id: resp_id,
                result,
                error,
            } => {
                if resp_id != request_id {
                    warn!(
                        expected = request_id,
                        got = resp_id,
                        "Request ID mismatch in CapabilityResult"
                    );
                }
                if let Some(err) = error {
                    json!({"success": false, "error": err})
                } else {
                    result
                }
            }
            _ => {
                error!("Unexpected response type to CapabilityRequest");
                json!({"success": false, "error": "Unexpected response type"})
            }
        }
    }

    async fn handle_message(&mut self, message: IpcMessage) {
        match message {
            IpcMessage::Init { config } => {
                debug!("Received Init message from host with config");

                if let Err(error) = self.handle_init(config).await {
                    self.send_response(IpcResponse::Error {
                        request_id: 0,
                        error,
                        kind: ErrorKind::Internal,
                    });
                    return;
                }

                // Debug: Log descriptor details before sending
                debug!(
                    descriptor_id = %self.descriptor.id(),
                    commands_count = self.descriptor.commands.len(),
                    metrics_count = self.descriptor.metrics.len(),
                    "Sending Ready response with descriptor"
                );

                // Debug: Log each command and metric
                for (i, cmd) in self.descriptor.commands.iter().enumerate() {
                    debug!(command_index = i, command_name = %cmd.name, "Command {}", i);
                }
                for (i, metric) in self.descriptor.metrics.iter().enumerate() {
                    debug!(metric_index = i, metric_name = %metric.name, "Metric {}", i);
                }

                // Try to serialize descriptor to verify it's valid
                match serde_json::to_string(&self.descriptor) {
                    Ok(json_str) => {
                        debug!(
                            json_len = json_str.len(),
                            "Descriptor serialized successfully"
                        );
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to serialize descriptor");
                    }
                }

                self.send_response(IpcResponse::Ready {
                    descriptor: self.descriptor.clone(),
                });

                debug!("Ready response sent to host");
            }

            IpcMessage::ExecuteCommand {
                command,
                args,
                request_id,
            } => {
                self.handle_execute_command(command, args, request_id).await;
            }

            IpcMessage::ProduceMetrics { request_id } => {
                self.handle_produce_metrics(request_id).await;
            }

            IpcMessage::GetDescriptor { request_id } => {
                self.handle_get_descriptor(request_id).await;
            }

            IpcMessage::HealthCheck { request_id } => {
                self.handle_health_check(request_id);
            }

            IpcMessage::GetMetadata { request_id } => {
                self.send_response(IpcResponse::Metadata {
                    request_id,
                    metadata: self.descriptor.metadata.clone(),
                });
            }

            IpcMessage::GetEventSubscriptions { request_id } => {
                // Get event subscriptions from the extension
                let event_types = if let Some(extension) = &self.extension {
                    extension.event_subscriptions()
                } else {
                    vec![]
                };

                self.send_response(IpcResponse::EventSubscriptions {
                    request_id,
                    event_types,
                });
            }

            IpcMessage::GetStats { request_id } => {
                self.handle_get_stats(request_id);
            }

            IpcMessage::Shutdown => {
                debug!("Received shutdown command");
                self.send_response(IpcResponse::ShutdownAck);
                self.running = false;
            }

            IpcMessage::ConfigUpdate { config } => {
                debug!("Received config hot-reload update");
                let result = match self.extension.as_ref() {
                    Some(ext) => match ext.configure(&config) {
                        Ok(()) => {
                            debug!("Config hot-reload applied successfully");
                            IpcResponse::ConfigUpdated {
                                success: true,
                                error: None,
                            }
                        }
                        Err(e) => {
                            warn!(error = %e, "Config hot-reload failed");
                            IpcResponse::ConfigUpdated {
                                success: false,
                                error: Some(e.to_string()),
                            }
                        }
                    },
                    None => IpcResponse::ConfigUpdated {
                        success: false,
                        error: Some("Extension not loaded".to_string()),
                    },
                };
                self.send_response(result);
            }

            IpcMessage::Ping { timestamp } => {
                self.send_response(IpcResponse::Pong { timestamp });
            }

            // Streaming support
            IpcMessage::GetStreamCapability { request_id } => {
                self.handle_get_stream_capability(request_id).await;
            }

            IpcMessage::InitStreamSession {
                request_id,
                session_id,
                extension_id: _,
                config,
                client_info: _,
            } => {
                self.handle_init_stream_session(request_id, session_id, config)
                    .await;
            }

            IpcMessage::ProcessStreamChunk {
                request_id,
                session_id,
                chunk,
            } => {
                self.handle_process_stream_chunk(request_id, session_id, chunk);
            }

            IpcMessage::CloseStreamSession {
                request_id,
                session_id,
            } => {
                self.handle_close_stream_session(request_id, session_id);
            }

            // Stateless mode support
            IpcMessage::ProcessChunk { request_id, chunk } => {
                self.handle_process_chunk(request_id, chunk);
            }

            // Push mode support
            IpcMessage::StartPush {
                request_id,
                session_id,
            } => {
                self.handle_start_push(request_id, session_id);
            }

            IpcMessage::StopPush {
                request_id,
                session_id,
            } => {
                self.handle_stop_push(request_id, session_id);
            }

            // Batch command support
            IpcMessage::ExecuteBatch {
                commands,
                request_id,
            } => {
                self.handle_execute_batch(commands, request_id).await;
            }

            // Capability invocation support (for WASM extensions)
            IpcMessage::InvokeCapability {
                request_id,
                capability,
                params,
            } => {
                self.handle_invoke_capability(request_id, capability, params);
            }

            IpcMessage::SubscribeEvents {
                request_id,
                event_types: _,
                filter: _,
            } => {
                // Generate subscription ID
                let subscription_id = uuid::Uuid::new_v4().to_string();
                self.send_response(IpcResponse::EventSubscriptionResult {
                    request_id,
                    subscription_id: Some(subscription_id),
                    error: None,
                });
            }

            IpcMessage::UnsubscribeEvents {
                request_id,
                subscription_id: _,
            } => {
                self.send_response(IpcResponse::EventSubscriptionResult {
                    request_id,
                    subscription_id: None,
                    error: None,
                });
            }

            IpcMessage::PollEvents {
                request_id,
                subscription_id: _,
            } => {
                // Return empty events for now
                self.send_response(IpcResponse::EventPollResult {
                    request_id,
                    events: vec![],
                });
            }

            IpcMessage::EventPush {
                event_type,
                payload,
                timestamp: _,
            } => {
                // Handle event push from host
                debug!(
                    event_type = %event_type,
                    "Received event push from host"
                );

                // Forward event pushes to the native JSON bridge when available.
                if let Some(extension) = &self.extension {
                    match extension.handle_event(&event_type, &payload) {
                        Ok(_) => {
                            trace!(
                                event_type = %event_type,
                                "Event handled by extension"
                            );
                        }
                        Err(e) => {
                            error!(
                                event_type = %event_type,
                                error = %e,
                                "Failed to handle event in extension"
                            );
                        }
                    }
                } else {
                    // Call the SDK's event handler for native extensions (fallback)
                    neomind_extension_sdk::capabilities::event::call_event_handler(
                        &event_type,
                        &payload,
                    );
                }

                // For WASM extensions, push events to global subscription queues
                if self.extension_type == ExtensionType::Wasm {
                    get_global_event_state().push_event(&event_type, payload);
                }
            }

            IpcMessage::CapabilityResult {
                request_id,
                result,
                error,
            } => {
                // Handle capability result from host (response to our CapabilityRequest)
                debug!(
                    request_id,
                    has_error = error.is_some(),
                    "Received CapabilityResult from host"
                );

                // Route to waiting invoke() call via pending requests queue
                let response = IpcResponse::CapabilityResult {
                    request_id,
                    result,
                    error,
                };
                complete_pending_request(request_id, response);
            }
        }
    }

    async fn handle_execute_command(
        &mut self,
        command: String,
        args: serde_json::Value,
        request_id: u64,
    ) {
        debug!(command = %command, request_id, "Executing command");

        let result = match self.extension_type {
            ExtensionType::Native => self.execute_native_command(&command, &args).await,
            ExtensionType::Wasm => self.execute_wasm_command(&command, &args),
        };

        match result {
            Ok(value) => {
                self.send_response(IpcResponse::Success {
                    request_id,
                    data: value,
                });
            }
            Err(e) => {
                self.send_response(IpcResponse::Error {
                    request_id,
                    error: e.clone(),
                    kind: ErrorKind::ExecutionFailed,
                });
            }
        }
    }

    async fn handle_init(&mut self, config: serde_json::Value) -> Result<(), String> {
        match self.extension_type {
            ExtensionType::Native => {
                let ext = self
                    .extension
                    .as_ref()
                    .ok_or("No native extension loaded")?;
                ext.configure(&config)
            }
            ExtensionType::Wasm => Ok(()),
        }
    }

    async fn handle_execute_batch(&mut self, commands: Vec<BatchCommand>, request_id: u64) {
        debug!(
            request_id,
            command_count = commands.len(),
            "Executing batch command"
        );

        let start = std::time::Instant::now();
        let mut results = Vec::new();

        for cmd in &commands {
            let cmd_start = std::time::Instant::now();

            let result = match self.extension_type {
                ExtensionType::Native => self.execute_native_command(&cmd.command, &cmd.args).await,
                ExtensionType::Wasm => self.execute_wasm_command(&cmd.command, &cmd.args),
            };

            results.push(BatchResult {
                command: cmd.command.clone(),
                success: result.is_ok(),
                data: result.as_ref().ok().cloned(),
                error: result.as_ref().err().map(|e| e.to_string()),
                elapsed_ms: cmd_start.elapsed().as_millis() as f64,
            });
        }

        self.send_response(IpcResponse::BatchResults {
            request_id,
            results,
            total_elapsed_ms: start.elapsed().as_millis() as f64,
        });
    }

    async fn execute_native_command(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let ext = self
            .extension
            .as_ref()
            .ok_or("No native extension loaded")?;
        ext.execute_command(command, args)
    }

    fn execute_wasm_command(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let runtime = self.wasm_runtime.as_ref().ok_or("No WASM runtime loaded")?;

        let ipc_client = self.ipc_client.clone();

        let runtime_handle = self.runtime.clone();
        tokio::task::block_in_place(|| {
            runtime_handle.block_on(async {
                // Try new execute_command API first
                match runtime
                    .execute_command(command, args, ipc_client.clone())
                    .await
                {
                    Ok(result) => {
                        // Extract the actual result from the response
                        if result
                            .get("success")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false)
                        {
                            Ok(result.get("result").cloned().unwrap_or(result))
                        } else {
                            Err(result
                                .get("error")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown error")
                                .to_string())
                        }
                    }
                    Err(_) => {
                        // Fallback to legacy execute function
                        runtime.execute(command, args, ipc_client.clone()).await
                    }
                }
            })
        })
    }

    async fn handle_produce_metrics(&mut self, request_id: u64) {
        debug!(request_id, "Producing metrics");

        let result = match self.extension_type {
            ExtensionType::Native => self.produce_native_metrics().await,
            ExtensionType::Wasm => self.produce_wasm_metrics(),
        };

        match result {
            Ok(metrics) => {
                self.send_response(IpcResponse::Metrics {
                    request_id,
                    metrics,
                });
            }
            Err(e) => {
                self.send_response(IpcResponse::Error {
                    request_id,
                    error: e,
                    kind: ErrorKind::Internal,
                });
            }
        }
    }

    async fn produce_native_metrics(&self) -> Result<Vec<ExtensionMetricValue>, String> {
        let ext = self
            .extension
            .as_ref()
            .ok_or("No native extension loaded")?;
        ext.produce_metrics()
    }

    fn produce_wasm_metrics(&self) -> Result<Vec<ExtensionMetricValue>, String> {
        let runtime = self.wasm_runtime.as_ref().ok_or("No WASM runtime loaded")?;
        runtime.produce_metrics()
    }

    /// Build a fresh descriptor by asking the extension (native FFI or WASM)
    /// to regenerate metadata/commands/metrics. The returned descriptor
    /// reflects the extension's current runtime state, enabling dynamic
    /// metrics discovery.
    fn build_descriptor_fresh(&self) -> Result<ExtensionDescriptor, String> {
        match self.extension_type {
            ExtensionType::Native => {
                let ext = self
                    .extension
                    .as_ref()
                    .ok_or("No native extension loaded")?;
                ext.get_descriptor_fresh()
            }
            ExtensionType::Wasm => {
                let runtime = self.wasm_runtime.as_ref().ok_or("No WASM runtime loaded")?;
                runtime.get_descriptor_blocking()
            }
        }
    }

    /// Handler for `IpcMessage::GetDescriptor`. Rebuilds the descriptor and
    /// updates the runner's cached copy so subsequent `GetMetadata` and
    /// `Ready` responses also reflect the latest state.
    async fn handle_get_descriptor(&mut self, request_id: u64) {
        debug!(request_id, "Refreshing descriptor");
        match self.build_descriptor_fresh() {
            Ok(descriptor) => {
                let metrics_count = descriptor.metrics.len();
                let commands_count = descriptor.commands.len();
                self.descriptor = descriptor.clone();
                debug!(
                    request_id,
                    metrics_count, commands_count, "Descriptor refreshed"
                );
                self.send_response(IpcResponse::Descriptor {
                    request_id,
                    descriptor,
                });
            }
            Err(e) => {
                warn!(request_id, error = %e, "Failed to refresh descriptor");
                self.send_response(IpcResponse::Error {
                    request_id,
                    error: e,
                    kind: ErrorKind::Internal,
                });
            }
        }
    }

    fn handle_health_check(&mut self, request_id: u64) {
        debug!(request_id, "Health check");

        let healthy = match self.extension_type {
            ExtensionType::Native => self.native_health_check(),
            ExtensionType::Wasm => self.wasm_health_check(),
        };

        self.send_response(IpcResponse::Health {
            request_id,
            healthy,
        });
    }

    fn handle_invoke_capability(
        &mut self,
        request_id: u64,
        capability: String,
        params: serde_json::Value,
    ) {
        debug!(request_id, capability = %capability, "Invoking capability");

        // Forward the capability request to the host via bidirectional IPC
        // The host will invoke the real capability provider and return the result
        let result = self.invoke_host_capability(&capability, &params);

        // Extract error from result if present
        let (result_value, error) = if let Some(err) = result.get("error").and_then(|e| e.as_str())
        {
            (serde_json::json!({}), Some(err.to_string()))
        } else {
            (result, None)
        };

        self.send_response(IpcResponse::CapabilityResult {
            request_id,
            result: result_value,
            error,
        });
    }

    fn native_health_check(&self) -> bool {
        let ext = match &self.extension {
            Some(e) => e,
            None => return false,
        };
        ext.health_check()
    }

    fn wasm_health_check(&self) -> bool {
        let runtime = match &self.wasm_runtime {
            Some(r) => r,
            None => return false,
        };

        let runtime_handle = match tokio::runtime::Handle::try_current() {
            Ok(h) => h,
            Err(_) => return false,
        };
        tokio::task::block_in_place(|| {
            runtime_handle.block_on(async { runtime.health_check().await })
        })
    }

    // =========================================================================
    // Statistics Support
    // =========================================================================

    fn handle_get_stats(&mut self, request_id: u64) {
        debug!(request_id, "Getting extension statistics");

        let stats = match self.extension_type {
            ExtensionType::Native => self.get_native_stats(),
            ExtensionType::Wasm => {
                // WASM extensions don't support stats yet
                // Return default stats
                ExtensionStats::default()
            }
        };

        debug!(
            request_id,
            start_count = stats.start_count,
            stop_count = stats.stop_count,
            error_count = stats.error_count,
            "Sending Stats response"
        );

        self.send_response(IpcResponse::Stats {
            request_id,
            start_count: stats.start_count,
            stop_count: stats.stop_count,
            error_count: stats.error_count,
            last_error: stats.last_error,
        });

        debug!(request_id, "Stats response sent");
    }

    fn get_native_stats(&self) -> ExtensionStats {
        debug!("Getting native extension stats");
        let ext = match &self.extension {
            Some(e) => e,
            None => {
                debug!("No extension loaded, returning default stats");
                return ExtensionStats::default();
            }
        };
        let stats = ext.get_stats();
        debug!(
            start_count = stats.start_count,
            stop_count = stats.stop_count,
            error_count = stats.error_count,
            "Got extension stats"
        );
        stats
    }

    // =========================================================================
    // Streaming Support
    // =========================================================================

    async fn handle_get_stream_capability(&mut self, request_id: u64) {
        debug!(request_id, "Getting stream capability");

        let capability = match self.extension_type {
            ExtensionType::Native => self.get_native_stream_capability().await,
            ExtensionType::Wasm => {
                Ok(None) // WASM doesn't support streaming yet
            }
        };

        match capability {
            Ok(cap) => {
                self.send_response(IpcResponse::StreamCapability {
                    request_id,
                    capability: cap.map(|c| serde_json::to_value(c).unwrap_or_default()),
                });
            }
            Err(e) => {
                self.send_response(IpcResponse::Error {
                    request_id,
                    error: e,
                    kind: ErrorKind::Internal,
                });
            }
        }
    }

    async fn get_native_stream_capability(&self) -> Result<Option<StreamCapability>, String> {
        let ext = self
            .extension
            .as_ref()
            .ok_or("No native extension loaded")?;
        ext.get_stream_capability()
    }

    async fn handle_init_stream_session(
        &mut self,
        request_id: u64,
        session_id: String,
        config: serde_json::Value,
    ) {
        debug!(session_id = %session_id, request_id, "Initializing stream session");

        let result = match self.extension_type {
            ExtensionType::Native => self.init_native_stream_session(&session_id, config).await,
            ExtensionType::Wasm => Err("WASM streaming not supported".to_string()),
        };

        match result {
            Ok(_) => {
                self.send_response(IpcResponse::StreamSessionInit {
                    request_id,
                    session_id,
                    success: true,
                    error: None,
                });
            }
            Err(e) => {
                self.send_response(IpcResponse::StreamSessionInit {
                    request_id,
                    session_id,
                    success: false,
                    error: Some(e),
                });
            }
        }
    }
    async fn init_native_stream_session(
        &self,
        session_id: &str,
        config: serde_json::Value,
    ) -> Result<(), String> {
        let ext = self
            .extension
            .as_ref()
            .ok_or("No native extension loaded")?;
        ext.init_session(session_id, config)
    }
    fn handle_process_stream_chunk(
        &mut self,
        request_id: u64,
        session_id: String,
        chunk: StreamDataChunk,
    ) {
        debug!(session_id = %session_id, sequence = chunk.sequence, request_id, "Processing stream chunk");

        let result = match self.extension_type {
            ExtensionType::Native => self.process_native_stream_chunk(&session_id, chunk),
            ExtensionType::Wasm => Err("WASM streaming not supported".to_string()),
        };

        match result {
            Ok(stream_result) => {
                self.send_response(IpcResponse::StreamChunkResult {
                    request_id,
                    session_id,
                    input_sequence: stream_result.input_sequence.unwrap_or(0),
                    output_sequence: stream_result.output_sequence,
                    data: stream_result.data,
                    data_type: stream_result.data_type.mime_type(),
                    processing_ms: stream_result.processing_ms,
                });
            }
            Err(e) => {
                self.send_response(IpcResponse::Error {
                    request_id,
                    error: e,
                    kind: ErrorKind::ExecutionFailed,
                });
            }
        }
    }

    fn process_native_stream_chunk(
        &self,
        session_id: &str,
        chunk: StreamDataChunk,
    ) -> Result<StreamResult, String> {
        let ext = self
            .extension
            .as_ref()
            .ok_or("No native extension loaded")?;
        ext.process_session_chunk(session_id, chunk)
    }

    fn handle_close_stream_session(&mut self, request_id: u64, session_id: String) {
        debug!(session_id = %session_id, request_id, "Closing stream session");

        let result = match self.extension_type {
            ExtensionType::Native => self.close_native_stream_session(&session_id),
            ExtensionType::Wasm => Ok(SessionStats::default()),
        };

        match result {
            Ok(stats) => {
                self.send_response(IpcResponse::StreamSessionClosed {
                    request_id,
                    session_id,
                    total_frames: stats.input_chunks,
                    duration_ms: 0, // We don't track this in runner
                });
            }
            Err(e) => {
                self.send_response(IpcResponse::Error {
                    request_id,
                    error: e,
                    kind: ErrorKind::ExecutionFailed,
                });
            }
        }
    }

    fn close_native_stream_session(&self, session_id: &str) -> Result<SessionStats, String> {
        let ext = self
            .extension
            .as_ref()
            .ok_or("No native extension loaded")?;
        ext.close_session(session_id)
    }

    // =========================================================================
    // Stateless Mode Support
    // =========================================================================

    fn handle_process_chunk(&mut self, request_id: u64, chunk: StreamDataChunk) {
        debug!(
            request_id,
            sequence = chunk.sequence,
            "Processing stateless chunk"
        );

        let result = match self.extension_type {
            ExtensionType::Native => self.process_native_chunk(chunk),
            ExtensionType::Wasm => {
                Err("Stateless chunk processing not supported for WASM".to_string())
            }
        };

        match result {
            Ok(stream_result) => {
                self.send_response(IpcResponse::ChunkResult {
                    request_id,
                    input_sequence: stream_result.input_sequence.unwrap_or(0),
                    output_sequence: stream_result.output_sequence,
                    data: stream_result.data,
                    data_type: stream_result.data_type.mime_type(),
                    processing_ms: stream_result.processing_ms,
                    metadata: stream_result.metadata,
                });
            }
            Err(e) => {
                self.send_response(IpcResponse::Error {
                    request_id,
                    error: e,
                    kind: ErrorKind::ExecutionFailed,
                });
            }
        }
    }

    fn process_native_chunk(&self, chunk: StreamDataChunk) -> Result<StreamResult, String> {
        let ext = self
            .extension
            .as_ref()
            .ok_or("No native extension loaded")?;
        ext.process_chunk(chunk)
    }

    // =========================================================================
    // Push Mode Support
    // =========================================================================

    fn handle_start_push(&mut self, request_id: u64, session_id: String) {
        debug!(
            request_id,
            session_id = %session_id,
            "Starting push mode"
        );

        let result = match self.extension_type {
            ExtensionType::Native => self.start_native_push(&session_id),
            ExtensionType::Wasm => Err("Push mode not supported for WASM".to_string()),
        };

        match result {
            Ok(_) => {
                self.send_response(IpcResponse::PushStarted {
                    request_id,
                    session_id,
                    success: true,
                    error: None,
                });
            }
            Err(e) => {
                self.send_response(IpcResponse::PushStarted {
                    request_id,
                    session_id,
                    success: false,
                    error: Some(e),
                });
            }
        }
    }

    fn start_native_push(&self, session_id: &str) -> Result<(), String> {
        let ext = self
            .extension
            .as_ref()
            .ok_or("No native extension loaded")?;
        ext.start_push(session_id)
    }

    fn handle_stop_push(&mut self, request_id: u64, session_id: String) {
        debug!(
            request_id,
            session_id = %session_id,
            "Stopping push mode"
        );

        let result = match self.extension_type {
            ExtensionType::Native => self.stop_native_push(&session_id),
            ExtensionType::Wasm => {
                Ok(()) // WASM doesn't support push, just return success
            }
        };

        match result {
            Ok(_) => {
                self.send_response(IpcResponse::PushStopped {
                    request_id,
                    session_id,
                    success: true,
                });
            }
            Err(e) => {
                warn!(
                    session_id = %session_id,
                    error = %e,
                    "Error stopping push mode"
                );
                self.send_response(IpcResponse::PushStopped {
                    request_id,
                    session_id,
                    success: false,
                });
            }
        }
    }

    fn stop_native_push(&self, session_id: &str) -> Result<(), String> {
        let ext = self
            .extension
            .as_ref()
            .ok_or("No native extension loaded")?;
        ext.stop_push(session_id)
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    // Set up panic hook FIRST to capture any panics during loading
    // This ensures we can report errors to the parent process before dying
    std::panic::set_hook(Box::new(|panic_info| {
        let location = panic_info
            .location()
            .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
            .unwrap_or_else(|| "unknown location".to_string());

        let message = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic".to_string()
        };

        // Write to stderr in a format the parent process can parse
        eprintln!("[EXTENSION_RUNNER_PANIC] {} at {}", message, location);
        eprintln!("[EXTENSION_RUNNER_FATAL] Extension runner crashed - see panic details above");
        let _ = std::io::stderr().flush();
    }));

    let args = Args::parse();

    let log_level = if args.verbose {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };

    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .with_target(false)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .compact()
        .init();

    debug!("NeoMind Extension Runner starting");
    let _ = std::io::stderr().flush();
    debug!(extension_path = %args.extension_path.display(), "Extension path");

    // Set up resource limits BEFORE loading extension
    let memory_limit = if args.memory_limit_mb > 0 {
        Some(args.memory_limit_mb)
    } else {
        None
    };
    let hard_memory_limit = if args.memory_limit_hard_mb > 0 {
        Some(args.memory_limit_hard_mb)
    } else {
        None
    };

    let limits_config = ResourceLimitsConfig {
        memory_limit_mb: memory_limit,
        memory_limit_hard_mb: hard_memory_limit,
        cpu_affinity: None,
        nice_level: Some(args.nice_level),
    };

    if let Err(e) = setup_resource_limits(&limits_config) {
        error!("Failed to set resource limits: {}. Continuing anyway.", e);
    }

    if !args.extension_path.exists() {
        error!(path = %args.extension_path.display(), "Extension file not found");
        std::process::exit(1);
    }

    let mut runner = match Runner::load(&args.extension_path).await {
        Ok(r) => r,
        Err(e) => {
            error!(error = %e, "Failed to load extension");
            std::process::exit(1);
        }
    };

    debug!("Starting runner main loop");
    runner.run().await;

    debug!("Extension runner exiting normally");
    std::process::exit(0);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // Test ExtensionType::from_path()
    #[test]
    fn test_extension_type_detection_wasm() {
        let path = PathBuf::from("/tmp/test.wasm");
        let ext_type = ExtensionType::from_path(&path);
        assert_eq!(ext_type, ExtensionType::Wasm);
    }

    #[test]
    fn test_extension_type_detection_wasm_uppercase() {
        let path = PathBuf::from("/tmp/test.WASM");
        let ext_type = ExtensionType::from_path(&path);
        assert_eq!(ext_type, ExtensionType::Wasm);
    }

    #[test]
    fn test_extension_type_detection_dylib() {
        let path = PathBuf::from("/tmp/test.dylib");
        let ext_type = ExtensionType::from_path(&path);
        assert_eq!(ext_type, ExtensionType::Native);
    }

    #[test]
    fn test_extension_type_detection_so() {
        let path = PathBuf::from("/tmp/test.so");
        let ext_type = ExtensionType::from_path(&path);
        assert_eq!(ext_type, ExtensionType::Native);
    }

    #[test]
    fn test_extension_type_detection_dll() {
        let path = PathBuf::from("/tmp/test.dll");
        let ext_type = ExtensionType::from_path(&path);
        assert_eq!(ext_type, ExtensionType::Native);
    }

    #[test]
    fn test_extension_type_detection_no_extension() {
        let path = PathBuf::from("/tmp/test");
        let ext_type = ExtensionType::from_path(&path);
        assert_eq!(ext_type, ExtensionType::Native);
    }

    #[test]
    fn test_extension_type_detection_empty_path() {
        let path = PathBuf::from("");
        let ext_type = ExtensionType::from_path(&path);
        assert_eq!(ext_type, ExtensionType::Native);
    }

    #[test]
    fn test_extension_type_detection_weird_extensions() {
        // Unknown extensions should default to Native
        let path = PathBuf::from("/tmp/test.xyz");
        let ext_type = ExtensionType::from_path(&path);
        assert_eq!(ext_type, ExtensionType::Native);

        let path = PathBuf::from("/tmp/test.txt");
        let ext_type = ExtensionType::from_path(&path);
        assert_eq!(ext_type, ExtensionType::Native);
    }

    #[test]
    fn test_extension_type_detection_multiple_extensions() {
        // Test files with multiple dots like "my.ext.wasm"
        let path = PathBuf::from("/tmp/my.ext.wasm");
        let ext_type = ExtensionType::from_path(&path);
        assert_eq!(ext_type, ExtensionType::Wasm);

        let path = PathBuf::from("/tmp/my.ext.dylib");
        let ext_type = ExtensionType::from_path(&path);
        assert_eq!(ext_type, ExtensionType::Native);
    }

    #[test]
    fn test_extension_type_equality() {
        assert_eq!(ExtensionType::Wasm, ExtensionType::Wasm);
        assert_eq!(ExtensionType::Native, ExtensionType::Native);
        assert_ne!(ExtensionType::Wasm, ExtensionType::Native);
    }

    #[test]
    fn test_extension_type_copy() {
        let ext_type = ExtensionType::Wasm;
        let copied = ext_type;
        assert_eq!(ext_type, copied);
    }
}
