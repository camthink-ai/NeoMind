//! `wasm` — split from the former main.rs monolith.

use super::*;

use std::collections::HashMap;

use std::path::PathBuf;

use std::sync::Arc;

use serde_json::json;

use tokio::sync::RwLock;

use tracing::{debug, error, info};

use wasmtime::{AsContext, AsContextMut, Config, Engine, Linker, Module, Store, Val};

use wasmtime_wasi::preview1;

use wasmtime_wasi::WasiCtxBuilder;

use neomind_extension_sdk::{
    ExtensionCommand, ExtensionDescriptor, ExtensionMetadata, ExtensionMetricValue, MetricDataType,
    MetricDescriptor, ParameterDefinition,
};

use event_handler::get_global_event_state;

/// WASM runtime state
pub(crate) struct WasmRuntime {
    engine: Engine,
    module: Module,
    module_name: String,
    metric_values: Arc<RwLock<HashMap<String, serde_json::Value>>>,
}

/// Result buffer offset for WASM (matches SDK)
pub(crate) const WASM_RESULT_OFFSET: usize = 65536;

impl WasmRuntime {
    pub(crate) fn new(path: &PathBuf, module_name: String) -> Result<Self, String> {
        // Configure wasmtime engine
        let mut config = Config::new();
        config.async_support(true);
        config.consume_fuel(true);
        config.async_stack_size(8 * 1024 * 1024);

        // WASM is bounded three ways: fuel (above), the module size check
        // (below), and the per-store ResourceLimiter attached to every store
        // (`HostState::limits`) — a TRUE cap on linear-memory growth, default
        // 256 MB, override via NEOMIND_WASM_MEMORY_MB. The old
        // `Config::static_memory_maximum_size` (removed in wasmtime 36+) only
        // ever bounded the static-allocation *virtual* memory upper bound.

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
    pub(crate) fn get_descriptor_blocking(&self) -> Result<ExtensionDescriptor, String> {
        let engine = self.engine.clone();
        let module = self.module.clone();

        let runtime_handle = tokio::runtime::Handle::try_current()
            .map_err(|e| format!("Failed to get runtime handle: {}", e))?;
        tokio::task::block_in_place(|| {
            runtime_handle.block_on(async { self.get_descriptor_async(&engine, &module).await })
        })
    }

    /// Get the extension descriptor from the WASM module (async version)
    pub(crate) async fn get_descriptor_async(
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
        store.limiter(|s| &mut s.limits);
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
    pub(crate) fn parse_descriptor_json(
        json: &serde_json::Value,
    ) -> Result<ExtensionDescriptor, String> {
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
    pub(crate) async fn execute_command(
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
            store.limiter(|s| &mut s.limits);
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
    pub(crate) async fn execute(
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
            store.limiter(|s| &mut s.limits);
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

    pub(crate) async fn health_check(&self) -> bool {
        match self.execute("health", &json!({}), None).await {
            Ok(result) => result.as_bool().unwrap_or(true),
            Err(_) => true, // Assume healthy if function not found
        }
    }

    pub(crate) fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>, String> {
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
pub(crate) fn add_neomind_to_linker(linker: &mut Linker<HostState>) -> Result<(), String> {
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
pub(crate) const WASM_MAX_MEMORY_READ: usize = 1024 * 1024;

/// Helper function to read a string from WASM memory
pub(crate) fn read_string_from_memory(
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
pub(crate) fn write_bytes_to_memory(
    caller: &mut wasmtime::Caller<'_, HostState>,
    offset: usize,
    data: &[u8],
) -> Result<(), String> {
    let memory = caller.data().memory.ok_or("Memory not set")?;
    memory
        .write(caller.as_context_mut(), offset, data)
        .map_err(|e| format!("Failed to write memory: {}", e))
}
