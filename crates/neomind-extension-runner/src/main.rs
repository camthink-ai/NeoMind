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

use std::collections::HashMap;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use serde_json::json;
use tokio::sync::RwLock;
use tracing::{debug, error, info};
use wasmtime::{AsContextMut, Config, Engine, Linker, Memory, Module, Store, Val};
use wasmtime_wasi::preview1::{self, WasiP1Ctx};
use wasmtime_wasi::WasiCtxBuilder;

use neomind_core::extension::isolated::{ErrorKind, IpcFrame, IpcMessage, IpcResponse};
use neomind_core::extension::loader::NativeExtensionLoader;
use neomind_core::extension::system::DynExtension;

/// Extension type detected from file
#[derive(Debug, Clone, Copy, PartialEq)]
enum ExtensionType {
    Native,
    Wasm,
}

impl ExtensionType {
    /// Detect extension type from file path
    fn from_path(path: &PathBuf) -> Self {
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
}

/// Extension runner state
struct Runner {
    /// Loaded extension (for native)
    extension: Option<DynExtension>,
    /// WASM runtime (for WASM)
    wasm_runtime: Option<WasmRuntime>,
    /// Extension descriptor (unified capabilities)
    descriptor: neomind_core::extension::system::ExtensionDescriptor,
    /// Extension type
    extension_type: ExtensionType,
    /// Stdin reader
    stdin: BufReader<std::io::Stdin>,
    /// Stdout writer
    stdout: BufWriter<std::io::Stdout>,
    /// Running flag
    running: bool,
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

        let engine = Engine::new(&config)
            .map_err(|e| format!("Failed to create WASM engine: {}", e))?;

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
    fn get_descriptor_blocking(&self) -> Result<neomind_core::extension::system::ExtensionDescriptor, String> {
        let engine = self.engine.clone();
        let module = self.module.clone();

        // Create a new store for this operation
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create runtime: {}", e))?;

        rt.block_on(async {
            self.get_descriptor_async(&engine, &module).await
        })
    }

    /// Get the extension descriptor from the WASM module (async version)
    async fn get_descriptor_async(
        &self,
        engine: &Engine,
        module: &Module,
    ) -> Result<neomind_core::extension::system::ExtensionDescriptor, String> {
        // Create linker with WASI support
        let mut linker = Linker::new(engine);
        preview1::add_to_linker_async(&mut linker, |t: &mut HostState| &mut t.wasi)
            .map_err(|e| format!("Failed to add WASI: {}", e))?;

        let wasi = WasiCtxBuilder::new()
            .inherit_stdio()
            .build_p1();

        let host_state = HostState {
            wasi,
            memory: None,
        };

        let mut store = Store::new(engine, host_state);
        store.set_fuel(1_000_000)
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
        let memory = store.data().memory.unwrap();
        let mut result_bytes = vec![0u8; result_len];
        memory.read(&store, WASM_RESULT_OFFSET, &mut result_bytes)
            .map_err(|e| format!("Failed to read result: {}", e))?;

        let json_str = String::from_utf8_lossy(&result_bytes);
        let descriptor_json: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| format!("Failed to parse descriptor JSON: {}", e))?;

        // Parse the descriptor JSON
        Self::parse_descriptor_json(&descriptor_json)
    }

    /// Parse descriptor JSON into ExtensionDescriptor
    fn parse_descriptor_json(json: &serde_json::Value) -> Result<neomind_core::extension::system::ExtensionDescriptor, String> {
        use neomind_core::extension::system::{
            ExtensionMetadata, ExtensionCommand, MetricDescriptor, 
            MetricDataType, ParameterDefinition, ParamMetricValue
        };

        let metadata_json = json.get("metadata")
            .ok_or("Missing 'metadata' in descriptor")?;

        // Parse metadata
        let id = metadata_json.get("id")
            .and_then(|v| v.as_str())
            .ok_or("Missing 'id' in metadata")?
            .to_string();
        let name = metadata_json.get("name")
            .and_then(|v| v.as_str())
            .ok_or("Missing 'name' in metadata")?
            .to_string();
        let version_str = metadata_json.get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("1.0.0");
        let version = semver::Version::parse(version_str)
            .unwrap_or(semver::Version::new(1, 0, 0));
        let description = metadata_json.get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let author = metadata_json.get("author")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut metadata = ExtensionMetadata::new(id, name, version);
        metadata.description = description;
        metadata.author = author;

        // Parse metrics
        let metrics: Vec<MetricDescriptor> = json.get("metrics")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter().filter_map(|m| {
                    let name = m.get("name")?.as_str()?.to_string();
                    let display_name = m.get("display_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&name)
                        .to_string();
                    let data_type_str = m.get("data_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("string");
                    let data_type = match data_type_str {
                        "float" => MetricDataType::Float,
                        "integer" => MetricDataType::Integer,
                        "boolean" => MetricDataType::Boolean,
                        "binary" => MetricDataType::Binary,
                        _ => MetricDataType::String,
                    };
                    let unit = m.get("unit")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let min = m.get("min").and_then(|v| v.as_f64());
                    let max = m.get("max").and_then(|v| v.as_f64());
                    let required = m.get("required")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    Some(MetricDescriptor {
                        name,
                        display_name,
                        data_type,
                        unit,
                        min,
                        max,
                        required,
                    })
                }).collect()
            })
            .unwrap_or_default();

        // Parse commands
        let commands: Vec<ExtensionCommand> = json.get("commands")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter().filter_map(|c| {
                    let name = c.get("name")?.as_str()?.to_string();
                    let display_name = c.get("display_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&name)
                        .to_string();
                    let llm_hints = c.get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    // Parse parameters
                    let parameters: Vec<ParameterDefinition> = c.get("parameters")
                        .and_then(|v| v.as_array())
                        .map(|params| {
                            params.iter().filter_map(|p| {
                                let param_name = p.get("name")?.as_str()?.to_string();
                                let param_display_name = p.get("display_name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or(&param_name)
                                    .to_string();
                                let param_desc = p.get("description")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let param_type_str = p.get("param_type")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("string");
                                let param_type = match param_type_str {
                                    "float" => MetricDataType::Float,
                                    "integer" => MetricDataType::Integer,
                                    "boolean" => MetricDataType::Boolean,
                                    "binary" => MetricDataType::Binary,
                                    _ => MetricDataType::String,
                                };
                                let required = p.get("required")
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
                            }).collect()
                        })
                        .unwrap_or_default();

                    // Parse samples
                    let samples: Vec<serde_json::Value> = c.get("samples")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();

                    Some(ExtensionCommand {
                        name,
                        display_name,
                        payload_template: String::new(),
                        parameters,
                        fixed_values: HashMap::new(),
                        samples,
                        llm_hints,
                        parameter_groups: Vec::new(),
                    })
                }).collect()
            })
            .unwrap_or_default();

        Ok(neomind_core::extension::system::ExtensionDescriptor::with_capabilities(
            metadata,
            commands,
            metrics,
        ))
    }

    /// Execute a command using the new execute_command_json function
    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value, String> {
        let input = serde_json::to_string(&json!({
            "command": command,
            "args": args
        })).map_err(|e| format!("Failed to serialize input: {}", e))?;

        let module = self.module.clone();
        let engine = self.engine.clone();
        let input_bytes = input.into_bytes();
        let input_len = input_bytes.len();
        let metric_values = self.metric_values.clone();

        // Execute with timeout
        let result = tokio::time::timeout(
            tokio::time::Duration::from_secs(30),
            async move {
                // Create linker with WASI support
                let mut linker = Linker::new(&engine);
                preview1::add_to_linker_async(&mut linker, |t: &mut HostState| &mut t.wasi)
                    .map_err(|e| format!("Failed to add WASI: {}", e))?;

                let wasi = WasiCtxBuilder::new()
                    .inherit_stdio()
                    .build_p1();

                let host_state = HostState {
                    wasi,
                    memory: None,
                };

                let mut store = Store::new(&engine, host_state);
                store.set_fuel(1_000_000)
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
                    memory.write(&mut store, 0, &input_bytes)
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
                        memory.read(&store, WASM_RESULT_OFFSET, &mut result_bytes)
                            .map_err(|e| format!("Failed to read result: {}", e))?;

                        let result_str = String::from_utf8_lossy(&result_bytes);
                        let result_json: serde_json::Value = serde_json::from_str(&result_str)
                            .map_err(|e| format!("Failed to parse result JSON: {}", e))?;

                        // Cache metric values if present
                        if let Some(metrics) = result_json.get("metrics").and_then(|v| v.as_array()) {
                            let mut values = metric_values.write().await;
                            for m in metrics {
                                if let (Some(name), Some(value)) = (m.get("name").and_then(|n| n.as_str()), m.get("value")) {
                                    values.insert(name.to_string(), value.clone());
                                }
                            }
                        }

                        return Ok(result_json);
                    }
                }

                // Fallback: try the old execute function
                Err("execute_command_json not found, extension may not support new API".to_string())
            }
        ).await;

        result.map_err(|_| "Execution timeout".to_string())?
    }

    /// Legacy execute function for backward compatibility
    async fn execute(&self, function_name: &str, args: &serde_json::Value) -> Result<serde_json::Value, String> {
        let args_str = serde_json::to_string(args)
            .map_err(|e| format!("Failed to serialize args: {}", e))?;

        let module = self.module.clone();
        let engine = self.engine.clone();
        let function_name_owned = function_name.to_string();
        let module_name = self.module_name.clone();
        let args_str_clone = args_str.clone();
        let metric_values = self.metric_values.clone();

        // Execute with timeout
        let result = tokio::time::timeout(
            tokio::time::Duration::from_secs(30),
            async move {
                // Create linker
                let mut linker = Linker::new(&engine);

                // Add WASI support
                preview1::add_to_linker_async(&mut linker, |t: &mut HostState| &mut t.wasi)
                    .map_err(|e| format!("Failed to add WASI: {}", e))?;

                // Build WASI context
                let wasi = WasiCtxBuilder::new()
                    .inherit_stdio()
                    .build_p1();

                let host_state = HostState {
                    wasi,
                    memory: None,
                };

                // Create store with fuel
                let mut store = Store::new(&engine, host_state);
                store.set_fuel(1_000_000)
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
                    let args_bytes = args_str_clone.as_bytes();
                    let args_len = args_bytes.len();

                    memory.write(&mut store, 0, args_bytes)
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
                        memory.read(&store, WASM_RESULT_OFFSET, &mut result_bytes)
                            .map_err(|e| format!("Failed to read result: {}", e))?;

                        let result_str = String::from_utf8_lossy(&result_bytes);
                        let result_json: serde_json::Value = serde_json::from_str(&result_str)
                            .unwrap_or_else(|_| json!({
                                "success": true,
                                "raw_result": result_str.to_string()
                            }));

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
            }
        ).await;

        result.map_err(|_| "Execution timeout".to_string())?
    }

    async fn health_check(&self) -> bool {
        match self.execute("health", &json!({})).await {
            Ok(result) => result.as_bool().unwrap_or(true),
            Err(_) => true, // Assume healthy if function not found
        }
    }

    fn produce_metrics(&self) -> Result<Vec<neomind_core::extension::system::ExtensionMetricValue>, String> {
        use neomind_core::extension::system::ExtensionMetricValue;

        let values = self.metric_values.try_read()
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
            } else if let Some(s) = value.as_str() {
                Some(ExtensionMetricValue {
                    name: name.clone(),
                    value: s.into(),
                    timestamp: chrono::Utc::now().timestamp_millis(),
                })
            } else {
                None
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
}

impl Runner {
    /// Load extension and create runner
    fn load(extension_path: &PathBuf) -> Result<Self, String> {
        let extension_type = ExtensionType::from_path(extension_path);
        info!(
            path = %extension_path.display(),
            extension_type = ?extension_type,
            "Loading extension"
        );

        // Load the extension based on type
        let (extension, wasm_runtime, descriptor) = match extension_type {
            ExtensionType::Native => {
                let (ext, desc) = Self::load_native(extension_path)?;
                (Some(ext), None, desc)
            }
            ExtensionType::Wasm => {
                let (runtime, descriptor) = Self::load_wasm(extension_path)?;
                (None, Some(runtime), descriptor)
            }
        };

        info!(
            extension_id = %descriptor.metadata.id,
            name = %descriptor.metadata.name,
            version = %descriptor.metadata.version,
            extension_type = ?extension_type,
            commands_count = descriptor.commands.len(),
            metrics_count = descriptor.metrics.len(),
            "Extension loaded successfully"
        );

        Ok(Self {
            extension,
            wasm_runtime,
            descriptor,
            extension_type,
            stdin: BufReader::new(std::io::stdin()),
            stdout: BufWriter::new(std::io::stdout()),
            running: true,
        })
    }

    /// Load a native extension and return its descriptor
    fn load_native(extension_path: &PathBuf) -> Result<(DynExtension, neomind_core::extension::system::ExtensionDescriptor), String> {
        let loader = NativeExtensionLoader::new();
        let loaded = loader.load(extension_path)
            .map_err(|e| format!("Failed to load native extension: {}", e))?;

        // Use the unified descriptor() method
        let ext_guard = loaded.extension.blocking_read();
        let descriptor = ext_guard.descriptor();
        drop(ext_guard);

        Ok((loaded.extension, descriptor))
    }

    /// Load a WASM extension with full descriptor support
    fn load_wasm(extension_path: &PathBuf) -> Result<(WasmRuntime, neomind_core::extension::system::ExtensionDescriptor), String> {
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
                info!(
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
                info!(error = %e, "Failed to get descriptor from WASM, trying sidecar files");
                
                // Fallback to sidecar JSON files
                let metadata = Self::load_wasm_metadata(extension_path)?;
                let descriptor = neomind_core::extension::system::ExtensionDescriptor::new(metadata);
                Ok((runtime, descriptor))
            }
        }
    }

    /// Load WASM metadata (fallback from sidecar files)
    fn load_wasm_metadata(extension_path: &PathBuf) -> Result<neomind_core::extension::system::ExtensionMetadata, String> {
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

        Ok(neomind_core::extension::system::ExtensionMetadata::new(
            file_name.to_string(),
            format!("{} WASM Extension", file_name),
            semver::Version::new(1, 0, 0),
        ))
    }

    fn parse_metadata_json(path: &PathBuf) -> Result<neomind_core::extension::system::ExtensionMetadata, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read JSON: {}", e))?;

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

        let json: MetaJson = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse JSON: {}", e))?;

        let version = semver::Version::parse(&json.version).unwrap_or(semver::Version::new(1, 0, 0));

        let mut meta = neomind_core::extension::system::ExtensionMetadata::new(
            json.id,
            json.name,
            version,
        );
        meta.description = json.description;
        meta.author = json.author;

        Ok(meta)
    }

    fn find_nep_manifest(wasm_path: &PathBuf) -> Option<PathBuf> {
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
    fn run(&mut self) {
        info!("Starting IPC message loop");

        self.send_response(IpcResponse::Ready {
            descriptor: self.descriptor.clone(),
        });

        while self.running {
            match self.receive_message() {
                Ok(Some(message)) => {
                    self.handle_message(message);
                }
                Ok(None) => {
                    info!("stdin closed, exiting");
                    break;
                }
                Err(e) => {
                    error!(error = %e, "Failed to receive message");
                    break;
                }
            }
        }

        info!("Extension runner shutting down");
    }

    fn receive_message(&mut self) -> Result<Option<IpcMessage>, String> {
        let mut len_bytes = [0u8; 4];
        match self.stdin.read_exact(&mut len_bytes) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(None);
            }
            Err(e) => {
                return Err(format!("Failed to read length: {}", e));
            }
        }

        let len = u32::from_le_bytes(len_bytes) as usize;
        if len > 10 * 1024 * 1024 {
            return Err(format!("Message too large: {} bytes", len));
        }

        let mut payload = vec![0u8; len];
        self.stdin.read_exact(&mut payload)
            .map_err(|e| format!("Failed to read payload: {}", e))?;

        let message = IpcMessage::from_bytes(&payload)
            .map_err(|e| format!("Failed to decode message: {}", e))?;

        debug!(message = ?message, "Received IPC message");
        Ok(Some(message))
    }

    fn send_response(&mut self, response: IpcResponse) {
        debug!(response = ?response, "Sending IPC response");

        let payload = match response.to_bytes() {
            Ok(p) => p,
            Err(e) => {
                error!(error = %e, "Failed to serialize response");
                return;
            }
        };

        let frame = IpcFrame::new(payload);
        let bytes = frame.encode();

        if let Err(e) = self.stdout.write_all(&bytes) {
            error!(error = %e, "Failed to write response");
            return;
        }

        let _ = self.stdout.flush();
    }

    fn handle_message(&mut self, message: IpcMessage) {
        match message {
            IpcMessage::Init { config: _ } => {
                self.send_response(IpcResponse::Ready {
                    descriptor: self.descriptor.clone(),
                });
            }

            IpcMessage::ExecuteCommand { command, args, request_id } => {
                self.handle_execute_command(command, args, request_id);
            }

            IpcMessage::ProduceMetrics { request_id } => {
                self.handle_produce_metrics(request_id);
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

            IpcMessage::Shutdown => {
                info!("Received shutdown command");
                self.running = false;
            }

            IpcMessage::Ping { timestamp } => {
                self.send_response(IpcResponse::Pong { timestamp });
            }

            // Streaming support
            IpcMessage::GetStreamCapability { request_id } => {
                self.handle_get_stream_capability(request_id);
            }

            IpcMessage::InitStreamSession { session_id, extension_id: _, config, client_info: _ } => {
                self.handle_init_stream_session(session_id, config);
            }

            IpcMessage::ProcessStreamChunk { request_id, session_id, chunk } => {
                self.handle_process_stream_chunk(request_id, session_id, chunk);
            }

            IpcMessage::CloseStreamSession { session_id } => {
                self.handle_close_stream_session(session_id);
            }
        }
    }

    fn handle_execute_command(&mut self, command: String, args: serde_json::Value, request_id: u64) {
        debug!(command = %command, request_id, "Executing command");

        let result = match self.extension_type {
            ExtensionType::Native => {
                self.execute_native_command(&command, &args)
            }
            ExtensionType::Wasm => {
                self.execute_wasm_command(&command, &args)
            }
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

    fn execute_native_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value, String> {
        let ext = self.extension.as_ref().ok_or("No native extension loaded")?;

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create runtime: {}", e))?;

        let ext_clone = Arc::clone(ext);

        rt.block_on(async {
            let ext_guard = ext_clone.read().await;
            ext_guard.execute_command(command, args).await
                .map_err(|e| e.to_string())
        })
    }

    fn execute_wasm_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value, String> {
        let runtime = self.wasm_runtime.as_ref().ok_or("No WASM runtime loaded")?;

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create runtime: {}", e))?;

        rt.block_on(async {
            // Try new execute_command API first
            match runtime.execute_command(command, args).await {
                Ok(result) => {
                    // Extract the actual result from the response
                    if result.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                        Ok(result.get("result").cloned().unwrap_or(result))
                    } else {
                        Err(result.get("error")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown error")
                            .to_string())
                    }
                }
                Err(_) => {
                    // Fallback to legacy execute function
                    runtime.execute(command, args).await
                }
            }
        })
    }

    fn handle_produce_metrics(&mut self, request_id: u64) {
        debug!(request_id, "Producing metrics");

        let result = match self.extension_type {
            ExtensionType::Native => {
                self.produce_native_metrics()
            }
            ExtensionType::Wasm => {
                self.produce_wasm_metrics()
            }
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

    fn produce_native_metrics(&self) -> Result<Vec<neomind_core::extension::system::ExtensionMetricValue>, String> {
        let ext = self.extension.as_ref().ok_or("No native extension loaded")?;

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create runtime: {}", e))?;

        let ext_clone = Arc::clone(ext);

        rt.block_on(async {
            let ext_guard = ext_clone.read().await;
            ext_guard.produce_metrics()
                .map_err(|e| e.to_string())
        })
    }

    fn produce_wasm_metrics(&self) -> Result<Vec<neomind_core::extension::system::ExtensionMetricValue>, String> {
        let runtime = self.wasm_runtime.as_ref().ok_or("No WASM runtime loaded")?;
        runtime.produce_metrics()
    }

    fn handle_health_check(&mut self, request_id: u64) {
        debug!(request_id, "Health check");

        let healthy = match self.extension_type {
            ExtensionType::Native => {
                self.native_health_check()
            }
            ExtensionType::Wasm => {
                self.wasm_health_check()
            }
        };

        self.send_response(IpcResponse::Health {
            request_id,
            healthy,
        });
    }

    fn native_health_check(&self) -> bool {
        let ext = match &self.extension {
            Some(e) => e,
            None => return false,
        };

        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(_) => return false,
        };

        let ext_clone = Arc::clone(ext);

        rt.block_on(async {
            let ext_guard = ext_clone.read().await;
            ext_guard.health_check().await.unwrap_or(false)
        })
    }

    fn wasm_health_check(&self) -> bool {
        let runtime = match &self.wasm_runtime {
            Some(r) => r,
            None => return false,
        };

        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(_) => return false,
        };

        rt.block_on(async {
            runtime.health_check().await
        })
    }

    // =========================================================================
    // Streaming Support
    // =========================================================================

    fn handle_get_stream_capability(&mut self, request_id: u64) {
        debug!(request_id, "Getting stream capability");

        let capability = match self.extension_type {
            ExtensionType::Native => {
                self.get_native_stream_capability()
            }
            ExtensionType::Wasm => {
                Ok(None)  // WASM doesn't support streaming yet
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

    fn get_native_stream_capability(&self) -> Result<Option<neomind_core::extension::StreamCapability>, String> {
        let ext = self.extension.as_ref().ok_or("No native extension loaded")?;

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create runtime: {}", e))?;

        let ext_clone = Arc::clone(ext);

        rt.block_on(async {
            let ext_guard = ext_clone.read().await;
            Ok(ext_guard.stream_capability())
        })
    }

    fn handle_init_stream_session(&mut self, session_id: String, config: serde_json::Value) {
        debug!(session_id = %session_id, "Initializing stream session");

        let result = match self.extension_type {
            ExtensionType::Native => {
                self.init_native_stream_session(&session_id, config)
            }
            ExtensionType::Wasm => {
                Err("WASM streaming not supported".to_string())
            }
        };

        match result {
            Ok(_) => {
                self.send_response(IpcResponse::StreamSessionInit {
                    session_id,
                    success: true,
                    error: None,
                });
            }
            Err(e) => {
                self.send_response(IpcResponse::StreamSessionInit {
                    session_id,
                    success: false,
                    error: Some(e),
                });
            }
        }
    }

    fn init_native_stream_session(&self, session_id: &str, config: serde_json::Value) -> Result<(), String> {
        let ext = self.extension.as_ref().ok_or("No native extension loaded")?;

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create runtime: {}", e))?;

        let ext_clone = Arc::clone(ext);
        let session_id_owned = session_id.to_string();

        rt.block_on(async {
            let ext_guard = ext_clone.read().await;
            
            // Create StreamSession
            let session = neomind_core::extension::StreamSession::new(
                session_id_owned,
                "extension".to_string(),  // Extension ID from metadata
                config,
                neomind_core::extension::ClientInfo {
                    client_id: "runner".to_string(),
                    ip_addr: None,
                    user_agent: None,
                },
            );

            ext_guard.init_session(&session).await
                .map_err(|e| e.to_string())
        })
    }

    fn handle_process_stream_chunk(&mut self, request_id: u64, session_id: String, chunk: neomind_core::extension::isolated::StreamDataChunk) {
        debug!(session_id = %session_id, sequence = chunk.sequence, request_id, "Processing stream chunk");

        let result = match self.extension_type {
            ExtensionType::Native => {
                self.process_native_stream_chunk(&session_id, chunk)
            }
            ExtensionType::Wasm => {
                Err("WASM streaming not supported".to_string())
            }
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
        chunk: neomind_core::extension::isolated::StreamDataChunk,
    ) -> Result<neomind_core::extension::StreamResult, String> {
        let ext = self.extension.as_ref().ok_or("No native extension loaded")?;

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create runtime: {}", e))?;

        let ext_clone = Arc::clone(ext);
        let session_id_owned = session_id.to_string();

        rt.block_on(async {
            let ext_guard = ext_clone.read().await;

            // Convert StreamDataChunk to DataChunk
            let data_chunk = neomind_core::extension::DataChunk {
                sequence: chunk.sequence,
                data_type: neomind_core::extension::StreamDataType::Binary,  // Will be overridden by actual data
                data: chunk.data,
                timestamp: chunk.timestamp,
                metadata: None,
                is_last: chunk.is_last,
            };

            ext_guard.process_session_chunk(&session_id_owned, data_chunk).await
                .map_err(|e| e.to_string())
        })
    }

    fn handle_close_stream_session(&mut self, session_id: String) {
        debug!(session_id = %session_id, "Closing stream session");

        let result = match self.extension_type {
            ExtensionType::Native => {
                self.close_native_stream_session(&session_id)
            }
            ExtensionType::Wasm => {
                Ok(neomind_core::extension::SessionStats::default())
            }
        };

        match result {
            Ok(stats) => {
                self.send_response(IpcResponse::StreamSessionClosed {
                    session_id,
                    total_frames: stats.input_chunks,
                    duration_ms: 0,  // We don't track this in runner
                });
            }
            Err(e) => {
                self.send_response(IpcResponse::StreamError {
                    session_id,
                    code: "CLOSE_ERROR".to_string(),
                    message: e,
                });
            }
        }
    }

    fn close_native_stream_session(&self, session_id: &str) -> Result<neomind_core::extension::SessionStats, String> {
        let ext = self.extension.as_ref().ok_or("No native extension loaded")?;

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create runtime: {}", e))?;

        let ext_clone = Arc::clone(ext);
        let session_id_owned = session_id.to_string();

        rt.block_on(async {
            let ext_guard = ext_clone.read().await;
            ext_guard.close_session(&session_id_owned).await
                .map_err(|e| e.to_string())
        })
    }
}

fn main() {
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

    info!("NeoMind Extension Runner starting");
    debug!(extension_path = %args.extension_path.display(), "Extension path");

    if !args.extension_path.exists() {
        error!(path = %args.extension_path.display(), "Extension file not found");
        std::process::exit(1);
    }

    let mut runner = match Runner::load(&args.extension_path) {
        Ok(r) => r,
        Err(e) => {
            error!(error = %e, "Failed to load extension");
            std::process::exit(1);
        }
    };

    runner.run();

    info!("Extension runner exiting normally");
}
