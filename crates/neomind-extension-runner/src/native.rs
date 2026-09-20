//! `native` — split from the former main.rs monolith.

use super::*;

use std::ffi::CStr;

use std::os::raw::c_char;

use std::panic::AssertUnwindSafe;

use std::path::Path;

use std::sync::Arc;

use serde_json::json;

use tracing::{debug, error, warn};

use neomind_extension_sdk::{
    DataChunk, ExtensionDescriptor, ExtensionMetricValue, ExtensionStats, SessionStats,
    StreamCapability, StreamDataChunk, StreamDataType, StreamResult, ABI_VERSION,
};

use ipc_routing::{safe_ffi_call, safe_ffi_call_with_timeout, SendPtr};

pub(crate) struct NativeExtensionBridge {
    _library: Arc<libloading::Library>,
    free_string: FreeStringFn,
    descriptor_json: JsonFn0,
    set_capability_bridge: Option<SetCapabilityBridgeFn>,
    register_push_writer: Option<RegisterPushWriterFn>,
    register_push_writer_raw: Option<RegisterPushWriterRawFn>,
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
    pub(crate) fn load(path: &Path) -> Result<(Self, ExtensionDescriptor), String> {
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
            register_push_writer_raw: Self::load_optional_symbol(
                &library,
                b"neomind_extension_register_push_writer_raw\0",
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

    pub(crate) fn install_capability_bridge(&self, ipc_client: Arc<SyncIpcClient>) {
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
        // Zero-serialization push path: only present when the extension
        // was built against an SDK that exports the v2 registration
        // (optional symbol — legacy extensions simply skip this).
        if let Some(register) = self.register_push_writer_raw {
            if let Err(e) = safe_ffi_call(
                "register_push_writer_raw",
                AssertUnwindSafe(|| unsafe { register(push_output_raw_writer) }),
            ) {
                warn!(error = %e, "Failed to register RAW push writer; JSON path stays active");
            } else {
                debug!("RAW push output writer registered (zero-serialization path)");
            }
        }
    }

    pub(crate) fn execute_command(
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

    pub(crate) fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>, String> {
        let response = self.call_json0(self.produce_metrics_json)?;
        let metrics = Self::extract_success_value(&response, "metrics")?;
        serde_json::from_value(metrics).map_err(|e| format!("Failed to parse metrics JSON: {}", e))
    }

    /// Re-fetch a fresh descriptor from the extension by re-invoking the
    /// `descriptor_json` FFI symbol. The macro-generated implementation
    /// rebuilds the descriptor on every call, so any runtime changes to
    /// `metrics()` / `commands()` are reflected. Used to support dynamic
    /// metrics discovery.
    pub(crate) fn get_descriptor_fresh(&self) -> Result<ExtensionDescriptor, String> {
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

    pub(crate) fn configure(&self, config: &serde_json::Value) -> Result<(), String> {
        self.call_unit_json(self.configure_json, config)
    }

    pub(crate) fn health_check(&self) -> bool {
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

    pub(crate) fn get_stats(&self) -> ExtensionStats {
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

    pub(crate) fn event_subscriptions(&self) -> Vec<String> {
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

    pub(crate) fn get_stream_capability(&self) -> Result<Option<StreamCapability>, String> {
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

    pub(crate) fn init_session(
        &self,
        session_id: &str,
        config: serde_json::Value,
    ) -> Result<(), String> {
        self.call_unit_json(
            self.init_session_json,
            &json!({ "session_id": session_id, "config": config }),
        )
    }

    pub(crate) fn process_session_chunk(
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

    pub(crate) fn close_session(&self, session_id: &str) -> Result<SessionStats, String> {
        let response = self.call_required_json1(
            self.close_session_json,
            "native close_session",
            &json!({ "session_id": session_id }),
        )?;
        let stats = Self::extract_success_value(&response, "stats")?;
        serde_json::from_value(stats)
            .map_err(|e| format!("Failed to parse native session stats JSON: {}", e))
    }

    pub(crate) fn process_chunk(&self, chunk: StreamDataChunk) -> Result<StreamResult, String> {
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

    pub(crate) fn start_push(&self, session_id: &str) -> Result<(), String> {
        self.call_unit_json(self.start_push_json, &json!({ "session_id": session_id }))
    }

    pub(crate) fn stop_push(&self, session_id: &str) -> Result<(), String> {
        self.call_unit_json(self.stop_push_json, &json!({ "session_id": session_id }))
    }

    pub(crate) fn handle_event(
        &self,
        event_type: &str,
        payload: &serde_json::Value,
    ) -> Result<(), String> {
        self.call_unit_json(
            self.handle_event_json,
            &json!({ "event_type": event_type, "payload": payload }),
        )
    }

    pub(crate) fn call_unit_json(
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

    pub(crate) fn call_required_json1(
        &self,
        func: Option<JsonFn1>,
        name: &str,
        input: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let func = func.ok_or_else(|| format!("{} is not supported by this extension", name))?;
        self.call_json1(func, input)
    }

    pub(crate) fn call_json0(&self, func: JsonFn0) -> Result<serde_json::Value, String> {
        let SendPtr(ptr) = safe_ffi_call_with_timeout(
            "call_json0",
            AssertUnwindSafe(move || unsafe { SendPtr(func()) }),
        )?;
        self.read_json_ptr(ptr)
    }

    pub(crate) fn call_json1(
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

    pub(crate) fn read_json_ptr(&self, ptr: *mut c_char) -> Result<serde_json::Value, String> {
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

    pub(crate) fn extract_success_value(
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

    pub(crate) fn extract_error(response: &serde_json::Value) -> String {
        response
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Native extension returned an unknown error")
            .to_string()
    }

    pub(crate) fn load_symbol<T: Copy>(
        library: &libloading::Library,
        name: &[u8],
    ) -> Result<T, String> {
        let symbol: libloading::Symbol<T> = unsafe { library.get(name) }.map_err(|e| {
            format!(
                "Failed to load symbol {:?}: {}",
                String::from_utf8_lossy(name),
                e
            )
        })?;
        Ok(*symbol)
    }

    pub(crate) fn load_optional_symbol<T: Copy>(
        library: &libloading::Library,
        name: &[u8],
    ) -> Option<T> {
        let symbol: libloading::Symbol<T> = unsafe { library.get(name).ok()? };
        Some(*symbol)
    }
}
