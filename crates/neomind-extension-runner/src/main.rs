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
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use clap::Parser;
use serde_json::json;
use tokio::sync::RwLock;
use tracing::{debug, error, info, trace, warn};
use wasmtime::{AsContext, AsContextMut, Config, Engine, Linker, Memory, Module, Store, Val};
use wasmtime_wasi::preview1::{self, WasiP1Ctx};
use wasmtime_wasi::WasiCtxBuilder;

use neomind_core::extension::isolated::{ErrorKind, IpcFrame, IpcMessage, IpcResponse, BatchCommand, BatchResult};
use neomind_core::extension::loader::NativeExtensionLoader;
use neomind_core::extension::system::DynExtension;
// Import capability name constants from Core for consistency
use neomind_core::extension::context::capabilities as cap;

// Resource limits module
mod resource_limits;
use resource_limits::{setup_resource_limits, ResourceLimitsConfig};

// ============================================================================
// Message routing for capability invocation
// ============================================================================

// Resource limits module
// A background thread reads all stdin messages and routes them to:
// 1. Pending capability requests (via PENDING_REQUESTS)
// 2. Main event queue (via EVENT_QUEUE)

use std::sync::mpsc::{Sender, Receiver, channel};

type ResponseSender = Sender<IpcResponse>;

/// Pending capability requests: request_id -> response sender
static PENDING_REQUESTS: std::sync::OnceLock<Mutex<HashMap<u64, ResponseSender>>> = std::sync::OnceLock::new();

/// Event queue for main loop
static EVENT_QUEUE: std::sync::OnceLock<Mutex<Vec<IpcMessage>>> = std::sync::OnceLock::new();

fn get_pending_requests() -> &'static Mutex<HashMap<u64, ResponseSender>> {
    PENDING_REQUESTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_event_queue() -> &'static Mutex<Vec<IpcMessage>> {
    EVENT_QUEUE.get_or_init(|| Mutex::new(Vec::new()))
}

/// Register a pending request and return the response receiver
fn register_pending_request(request_id: u64) -> Receiver<IpcResponse> {
    let (tx, rx) = channel();
    get_pending_requests().lock().unwrap().insert(request_id, tx);
    rx
}

/// Complete a pending request with the response
fn complete_pending_request(request_id: u64, response: IpcResponse) {
    if let Some(tx) = get_pending_requests().lock().unwrap().remove(&request_id) {
        let _ = tx.send(response);
    }
}

/// Push an event to the queue for main loop processing
fn push_event(message: IpcMessage) {
    get_event_queue().lock().unwrap().push(message);
}

/// Pop an event from the queue
fn pop_event() -> Option<IpcMessage> {
    get_event_queue().lock().unwrap().pop()
}

/// Start the stdin reader thread
/// This thread reads all messages from stdin and routes them appropriately
fn start_stdin_reader() -> std::thread::JoinHandle<()> {
    std::thread::spawn(|| {
        eprintln!("[StdinReader] Started");

        loop {
            // Read length prefix
            let mut len_bytes = [0u8; 4];
            match std::io::stdin().read_exact(&mut len_bytes) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    eprintln!("[StdinReader] stdin closed");
                    break;
                }
                Err(e) => {
                    eprintln!("[StdinReader] Error reading length: {}", e);
                    break;
                }
            }

            let len = u32::from_le_bytes(len_bytes) as usize;
            if len > 10 * 1024 * 1024 {
                eprintln!("[StdinReader] Message too large: {}", len);
                continue;
            }

            // Read payload
            let mut payload = vec![0u8; len];
            if let Err(e) = std::io::stdin().read_exact(&mut payload) {
                eprintln!("[StdinReader] Error reading payload: {}", e);
                continue;
            }

            // Parse message
            let message: IpcMessage = match IpcMessage::from_bytes(&payload) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("[StdinReader] Failed to parse message: {}", e);
                    continue;
                }
            };

            // Route message
            match &message {
                IpcMessage::CapabilityResult { request_id, .. } => {
                    // Convert to IpcResponse and route to waiting invoke()
                    let response = IpcResponse::CapabilityResult {
                        request_id: *request_id,
                        result: match message {
                            IpcMessage::CapabilityResult { ref result, .. } => result.clone(),
                            _ => json!({}),
                        },
                        error: match message {
                            IpcMessage::CapabilityResult { ref error, .. } => error.clone(),
                            _ => None,
                        },
                    };
                    complete_pending_request(*request_id, response);
                    eprintln!("[StdinReader] Routed CapabilityResult to request_id={}", request_id);
                }
                _ => {
                    // Push to event queue for main loop
                    push_event(message);
                }
            }
        }

        eprintln!("[StdinReader] Exiting");
    })
}

/// Global event state for WASM extensions
struct GlobalEventState {
    subscriptions: parking_lot::RwLock<HashMap<i64, String>>,
    queues: parking_lot::RwLock<HashMap<i64, Vec<serde_json::Value>>>,
    next_id: std::sync::atomic::AtomicI64,
}

impl GlobalEventState {
    fn new() -> Self {
        Self {
            subscriptions: parking_lot::RwLock::new(HashMap::new()),
            queues: parking_lot::RwLock::new(HashMap::new()),
            next_id: std::sync::atomic::AtomicI64::new(1),
        }
    }

    fn subscribe(&self, event_type: String) -> i64 {
        let id = self.next_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.subscriptions.write().insert(id, event_type);
        id
    }

    fn unsubscribe(&self, id: i64) -> bool {
        self.subscriptions.write().remove(&id).is_some()
    }

    fn push_event(&self, event_type: &str, payload: serde_json::Value) {
        let subscriptions = self.subscriptions.read();
        let mut queues = self.queues.write();

        for (id, sub_type) in subscriptions.iter() {
            if sub_type == "all" || sub_type == event_type || event_type.starts_with(&format!("{}::", sub_type)) {
                let event = json!({
                    "event_type": event_type,
                    "payload": payload,
                });
                queues.entry(*id).or_default().push(event);
            }
        }
    }

    fn take_events(&self, id: i64) -> Vec<serde_json::Value> {
        self.queues.write().remove(&id).unwrap_or_default()
    }
}

static GLOBAL_EVENT_STATE: std::sync::OnceLock<GlobalEventState> = std::sync::OnceLock::new();

fn get_global_event_state() -> &'static GlobalEventState {
    GLOBAL_EVENT_STATE.get_or_init(GlobalEventState::new)
}

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

    /// Memory limit in MB (0 = no limit)
    #[arg(long = "memory-limit", default_value = "512")]
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
    extension: Option<DynExtension>,
    /// WASM runtime (for WASM)
    wasm_runtime: Option<WasmRuntime>,
    /// Extension descriptor (unified capabilities)
    descriptor: neomind_core::extension::system::ExtensionDescriptor,
    /// Extension type
    extension_type: ExtensionType,
    /// Shared runtime for native async extension calls
    runtime: tokio::runtime::Runtime,
    /// Running flag
    running: bool,
    /// IPC client for WASM capability forwarding
    ipc_client: Option<Arc<SyncIpcClient>>,
    /// IPC request receiver (for forwarding to main process) - no longer used
    ipc_request_rx: Option<std::sync::mpsc::Receiver<SyncIpcRequest>>,
    /// IPC response sender (for returning results to WASM) - no longer used
    ipc_response_tx: Option<std::sync::mpsc::SyncSender<SyncIpcResponse>>,
    /// Capability context for invoking host capabilities
    capability_context: Option<neomind_core::extension::system::CapabilityContext>,
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

        // Add neomind host functions for capability invocation
        add_neomind_to_linker(&mut linker)?;

        let wasi = WasiCtxBuilder::new()
            .inherit_stdio()
            .build_p1();

        let host_state = HostState::new(wasi);

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
            MetricDataType, ParameterDefinition
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
                    let description = c.get("description")
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
                        description,
                        payload_template: String::new(),
                        parameters,
                        fixed_values: HashMap::new(),
                        samples,
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
    async fn execute_command(&self, command: &str, args: &serde_json::Value, ipc_client: Option<Arc<SyncIpcClient>>) -> Result<serde_json::Value, String> {
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

                // Add neomind host functions
                add_neomind_to_linker(&mut linker)?;

                let wasi = WasiCtxBuilder::new()
                    .inherit_stdio()
                    .build_p1();

                // Pass IPC client to host state for capability forwarding
                let host_state = HostState::with_ipc(wasi, ipc_client);

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

                // Add neomind host functions
                add_neomind_to_linker(&mut linker)?;

                // Build WASI context
                let wasi = WasiCtxBuilder::new()
                    .inherit_stdio()
                    .build_p1();

                let host_state = HostState::new(wasi);

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
            } else { value.as_str().map(|s| ExtensionMetricValue {
                    name: name.clone(),
                    value: s.into(),
                    timestamp: chrono::Utc::now().timestamp_millis(),
                }) };

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

/// Synchronous IPC request (for compatibility with existing code)
#[derive(Debug, Clone)]
pub struct SyncIpcRequest {
    pub request_id: u64,
    pub capability: String,
    pub params: serde_json::Value,
}

/// Synchronous IPC response (for compatibility with existing code)
#[derive(Debug, Clone)]
pub struct SyncIpcResponse {
    pub request_id: u64,
    pub result: serde_json::Value,
    pub error: Option<String>,
}

/// Synchronous IPC client for capability invocation
///
/// This client sends CapabilityRequest messages via stdout and waits for
/// CapabilityResult responses via the pending requests queue (routed by main loop).
pub struct SyncIpcClient {}

impl SyncIpcClient {
    /// Create a new sync IPC client
    pub fn new() -> (Self, std::sync::mpsc::Receiver<SyncIpcRequest>, std::sync::mpsc::SyncSender<SyncIpcResponse>) {
        // Return dummy channels for compatibility with existing code
        let (request_tx, request_rx) = std::sync::mpsc::sync_channel(16);
        let (response_tx, response_rx) = std::sync::mpsc::sync_channel(16);

        // Drop the channels we don't need
        drop(request_tx);
        drop(response_rx);

        (
            Self {},
            request_rx,
            response_tx,
        )
    }

    /// Invoke a capability synchronously
    /// 
    /// Sends CapabilityRequest via stdout and waits for CapabilityResult
    /// via the pending requests queue (routed by main loop).
    pub fn invoke(&self, capability: &str, params: &serde_json::Value) -> serde_json::Value {
        use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

        static REQUEST_ID_COUNTER: AtomicU64 = AtomicU64::new(1);
        let request_id = REQUEST_ID_COUNTER.fetch_add(1, AtomicOrdering::SeqCst);

        eprintln!("[SyncIpcClient] invoke called: capability={}, request_id={}", capability, request_id);

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
                eprintln!("[SyncIpcClient] Failed to serialize request: {}", e);
                // Remove pending request on error
                get_pending_requests().lock().unwrap().remove(&request_id);
                return json!({"success": false, "error": format!("Failed to serialize request: {}", e)});
            }
        };

        let frame = IpcFrame::new(payload);
        let bytes = frame.encode();

        // Write to stdout
        {
            let mut stdout = std::io::stdout();
            if let Err(e) = stdout.write_all(&bytes) {
                eprintln!("[SyncIpcClient] Failed to write request: {}", e);
                get_pending_requests().lock().unwrap().remove(&request_id);
                return json!({"success": false, "error": format!("Failed to write request: {}", e)});
            }
            if let Err(e) = stdout.flush() {
                eprintln!("[SyncIpcClient] Failed to flush request: {}", e);
                get_pending_requests().lock().unwrap().remove(&request_id);
                return json!({"success": false, "error": format!("Failed to flush request: {}", e)});
            }
        }

        eprintln!("[SyncIpcClient] Request sent, waiting for response via queue...");

        // Wait for response from the pending requests queue (with timeout)
        match response_rx.recv_timeout(std::time::Duration::from_secs(30)) {
            Ok(response) => {
                eprintln!("[SyncIpcClient] Received response: {:?}", response);
                match response {
                    IpcResponse::CapabilityResult { request_id: resp_id, result, error } => {
                        if resp_id != request_id {
                            eprintln!("[SyncIpcClient] Request ID mismatch: expected {}, got {}", request_id, resp_id);
                            return json!({"success": false, "error": "Request ID mismatch"});
                        }
                        if let Some(err) = error {
                            json!({"success": false, "error": err})
                        } else {
                            result
                        }
                    }
                    _ => {
                        eprintln!("[SyncIpcClient] Unexpected response type: {:?}", response);
                        json!({"success": false, "error": format!("Unexpected response type")})
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                eprintln!("[SyncIpcClient] Timeout waiting for response");
                get_pending_requests().lock().unwrap().remove(&request_id);
                json!({"success": false, "error": "Timeout waiting for response"})
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                eprintln!("[SyncIpcClient] Response channel disconnected");
                get_pending_requests().lock().unwrap().remove(&request_id);
                json!({"success": false, "error": "Response channel disconnected"})
            }
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
struct IpcCapabilityRequest {
    pub request_id: u64,
    pub capability: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
struct IpcCapabilityResponse {
    pub request_id: u64,
    pub result: Result<serde_json::Value, String>,
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
    linker.func_wrap(
        "neomind",
        "host_invoke_capability",
        |mut caller: wasmtime::Caller<'_, HostState>,
         capability_ptr: i32, capability_len: i32,
         params_ptr: i32, params_len: i32,
         result_ptr: i32, result_max_len: i32| -> i32 {
            // Read capability name from memory
            let capability = match read_string_from_memory(&mut caller, capability_ptr as usize, capability_len as usize) {
                Ok(s) => s,
                Err(_) => return -1,
            };

            // Read params from memory
            let params_str = match read_string_from_memory(&mut caller, params_ptr as usize, params_len as usize) {
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

            match write_bytes_to_memory(&mut caller, result_ptr as usize, &result_bytes[..write_len]) {
                Ok(_) => write_len as i32,
                Err(_) => -1,
            }
        }
    ).map_err(|e| format!("Failed to add host_invoke_capability: {}", e))?;

    // host_event_subscribe(
    //     event_type_ptr: *const u8, event_type_len: i32,
    //     filter_ptr: *const u8, filter_len: i32
    // ) -> i64
    linker.func_wrap(
        "neomind",
        "host_event_subscribe",
        |mut caller: wasmtime::Caller<'_, HostState>,
         event_type_ptr: i32, event_type_len: i32,
         _filter_ptr: i32, _filter_len: i32| -> i64 {
            // Read event type
            let event_type = match read_string_from_memory(&mut caller, event_type_ptr as usize, event_type_len as usize) {
                Ok(s) => s,
                Err(_) => return -1,
            };

            // Subscribe using global state
            let sub_id = get_global_event_state().subscribe(event_type);

            debug!(subscription_id = sub_id, "Event subscription created");
            sub_id
        }
    ).map_err(|e| format!("Failed to add host_event_subscribe: {}", e))?;

    // host_event_poll(subscription_id: i64, result_ptr: *mut u8, result_max_len: i32) -> i32
    linker.func_wrap(
        "neomind",
        "host_event_poll",
        |mut caller: wasmtime::Caller<'_, HostState>,
         subscription_id: i64,
         result_ptr: i32, result_max_len: i32| -> i32 {
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

            match write_bytes_to_memory(&mut caller, result_ptr as usize, &result_bytes[..write_len]) {
                Ok(_) => write_len as i32,
                Err(_) => -1,
            }
        }
    ).map_err(|e| format!("Failed to add host_event_poll: {}", e))?;

    // host_event_unsubscribe(subscription_id: i64) -> i32
    linker.func_wrap(
        "neomind",
        "host_event_unsubscribe",
        |_caller: wasmtime::Caller<'_, HostState>, subscription_id: i64| -> i32 {
            if get_global_event_state().unsubscribe(subscription_id) {
                debug!(subscription_id, "Event subscription removed");
                0
            } else {
                -1
            }
        }
    ).map_err(|e| format!("Failed to add host_event_unsubscribe: {}", e))?;

    // host_free(ptr: *const u8)
    linker.func_wrap(
        "neomind",
        "host_free",
        |_caller: wasmtime::Caller<'_, HostState>, _ptr: i32| {
            // No-op for now (memory management is handled by WASM linear memory)
        }
    ).map_err(|e| format!("Failed to add host_free: {}", e))?;

    // host_log(level_ptr: *const u8, level_len: i32, msg_ptr: *const u8, msg_len: i32)
    linker.func_wrap(
        "neomind",
        "host_log",
        |mut caller: wasmtime::Caller<'_, HostState>,
         level_ptr: i32, level_len: i32,
         msg_ptr: i32, msg_len: i32| {
            let level = read_string_from_memory(&mut caller, level_ptr as usize, level_len as usize)
                .unwrap_or_else(|_| "info".to_string());
            let msg = read_string_from_memory(&mut caller, msg_ptr as usize, msg_len as usize)
                .unwrap_or_else(|_| "".to_string());

            match level.as_str() {
                "error" => error!("{}", msg),
                "warn" => tracing::warn!("{}", msg),
                "debug" => debug!("{}", msg),
                _ => info!("{}", msg),
            }
        }
    ).map_err(|e| format!("Failed to add host_log: {}", e))?;

    // host_timestamp_ms() -> i64
    linker.func_wrap(
        "neomind",
        "host_timestamp_ms",
        |_caller: wasmtime::Caller<'_, HostState>| -> i64 {
            use std::time::{SystemTime, UNIX_EPOCH};
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64
        }
    ).map_err(|e| format!("Failed to add host_timestamp_ms: {}", e))?;

    Ok(())
}

/// Helper function to read a string from WASM memory
fn read_string_from_memory(
    caller: &mut wasmtime::Caller<'_, HostState>,
    offset: usize,
    len: usize,
) -> Result<String, String> {
    let memory = caller.data().memory.ok_or("Memory not set")?;
    let mut buffer = vec![0u8; len];
    memory.read(caller.as_context(), offset, &mut buffer)
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
    memory.write(caller.as_context_mut(), offset, data)
        .map_err(|e| format!("Failed to write memory: {}", e))
}

/// Handle capability invocation from WASM extension
///
/// NOTE: This is a simplified implementation that returns mock data.
/// WASM extensions have limited capability access due to sandbox restrictions.
/// For full capability access, use Native extensions in non-isolated mode.
///
/// SYNC: Capability names are imported from neomind_core::extension::context::capabilities
fn handle_capability_invocation(capability: &str, params: &serde_json::Value) -> serde_json::Value {
    debug!(capability = %capability, "Handling capability invocation (WASM mock)");
    match capability {
        // Device capabilities
        cap::DEVICE_METRICS_READ => {
            let device_id = params.get("device_id").and_then(|v| v.as_str()).unwrap_or("unknown");
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
            let device_id = params.get("device_id").and_then(|v| v.as_str()).unwrap_or("unknown");
            let metric = params.get("metric").and_then(|v| v.as_str()).unwrap_or("unknown");
            json!({
                "success": true,
                "device_id": device_id,
                "metric": metric,
                "message": "Metric written successfully",
            })
        }
        cap::DEVICE_CONTROL => {
            let device_id = params.get("device_id").and_then(|v| v.as_str()).unwrap_or("unknown");
            let command = params.get("command").and_then(|v| v.as_str()).unwrap_or("unknown");
            json!({
                "success": true,
                "device_id": device_id,
                "command": command,
                "result": "Command executed",
            })
        }

        // Telemetry capabilities
        cap::TELEMETRY_HISTORY => {
            let device_id = params.get("device_id").and_then(|v| v.as_str()).unwrap_or("unknown");
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
            let aggregation = params.get("aggregation").and_then(|v| v.as_str()).unwrap_or("avg");
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
            let event_type = params.get("event_type").and_then(|v| v.as_str()).unwrap_or("custom");
            json!({
                "success": true,
                "event_type": event_type,
                "message": "Event published",
            })
        }
        cap::EVENT_SUBSCRIBE => {
            let event_type = params.get("event_type").and_then(|v| v.as_str()).unwrap_or("all");
            let subscription_id = uuid::Uuid::new_v4().to_string();
            json!({
                "success": true,
                "subscription_id": subscription_id,
                "event_type": event_type,
            })
        }

        // Extension capabilities
        cap::EXTENSION_CALL => {
            let extension_id = params.get("extension_id").and_then(|v| v.as_str()).unwrap_or("unknown");
            let command = params.get("command").and_then(|v| v.as_str()).unwrap_or("unknown");
            json!({
                "success": true,
                "extension_id": extension_id,
                "command": command,
                "result": "Extension call completed",
            })
        }

        // Agent capabilities
        cap::AGENT_TRIGGER => {
            let agent_id = params.get("agent_id").and_then(|v| v.as_str()).unwrap_or("unknown");
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
            let rule_id = params.get("rule_id").and_then(|v| v.as_str()).unwrap_or("unknown");
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

        // Unknown capability
        _ => json!({
            "success": false,
            "error": format!("Unknown capability: {}", capability),
        })
    }
}

impl Runner {
    /// Load extension and create runner
    async fn load(extension_path: &PathBuf) -> Result<Self, String> {
        eprintln!("[Extension Runner] Runner::load called");
        let extension_type = ExtensionType::from_path(extension_path);
        debug!(
            path = %extension_path.display(),
            extension_type = ?extension_type,
            "Loading extension"
        );

        // Load the extension based on type
        let (extension, wasm_runtime, descriptor) = match extension_type {
            ExtensionType::Native => {
                eprintln!("[Extension Runner] Calling load_native");
                let (ext, desc) = Self::load_native(extension_path).await?;
                eprintln!("[Extension Runner] load_native returned successfully");
                (Some(ext), None, desc)
            }
            ExtensionType::Wasm => {
                let (runtime, descriptor) = Self::load_wasm(extension_path).await?;
                (None, Some(runtime), descriptor)
            }
        };

        eprintln!("[Extension Runner] Extension loaded, commands_count={}", descriptor.commands.len());

        debug!(
            extension_id = %descriptor.metadata.id,
            name = %descriptor.metadata.name,
            version = %descriptor.metadata.version,
            extension_type = ?extension_type,
            commands_count = descriptor.commands.len(),
            metrics_count = descriptor.metrics.len(),
            "Extension loaded successfully"
        );

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create runner runtime: {}", e))?;

        // Create IPC client for capability forwarding (both Native and WASM)
        let (ipc_client, ipc_request_rx, ipc_response_tx) = {
            let (client, request_rx, response_tx) = SyncIpcClient::new();
            let client_arc = Arc::new(client);
            (Some(client_arc), Some(request_rx), Some(response_tx))
        };

        // Create Async CapabilityContext for Native extensions
        let capability_context = if extension_type == ExtensionType::Native {
            if let Some(ref client_arc) = ipc_client {
                let client_for_invoker = client_arc.clone();
                let invoker = Box::new(move |capability: &str, params: &serde_json::Value| -> serde_json::Value {
                    client_for_invoker.invoke(capability, params)
                });
                Some(neomind_core::extension::system::CapabilityContext::new(invoker))
            } else {
                None
            }
        } else {
            None
        };

        Ok(Self {
            extension,
            wasm_runtime,
            descriptor,
            extension_type,
            runtime,
            running: true,
            ipc_client,
            ipc_request_rx,
            ipc_response_tx,
            capability_context,
        })
    }

    /// Load a native extension and return its descriptor
    async fn load_native(extension_path: &PathBuf) -> Result<(DynExtension, neomind_core::extension::system::ExtensionDescriptor), String> {
        eprintln!("[Extension Runner] load_native called");
        let loader = NativeExtensionLoader::new();
        let loaded = loader.load(extension_path)
            .map_err(|e| format!("Failed to load native extension: {}", e))?;

        eprintln!("[Extension Runner] Extension loaded, getting read lock...");

        // Use the unified descriptor() method
        let ext_guard = loaded.extension.read().await;
        
        eprintln!("[Extension Runner] Getting metadata...");
        
        // Get metadata, commands, and metrics separately
        // We need to serialize/deserialize each to ensure memory safety across FFI boundary
        let metadata = ext_guard.metadata();
        
        eprintln!("[Extension Runner] Metadata reference obtained, serializing...");
        
        let metadata_json = serde_json::to_string(&metadata)
            .map_err(|e| format!("Failed to serialize metadata: {}", e))?;
        
        info!("Metadata serialized, deserializing...");
        
        let metadata: neomind_core::extension::system::ExtensionMetadata = serde_json::from_str(&metadata_json)
            .map_err(|e| format!("Failed to deserialize metadata: {}", e))?;
        
        info!("Metadata obtained: id='{}', name='{}'", metadata.id, metadata.name);
        
        // Get commands and serialize/deserialize
        info!("Getting commands...");
        let commands = ext_guard.commands();
        
        info!("Commands reference obtained, serializing...");
        let commands_json = serde_json::to_string(&commands)
            .map_err(|e| format!("Failed to serialize commands: {}", e))?;
        
        info!("Commands serialized, deserializing...");
        let commands: Vec<neomind_core::extension::system::ExtensionCommand> = serde_json::from_str(&commands_json)
            .map_err(|e| format!("Failed to deserialize commands: {}", e))?;
        
        info!("Commands obtained: {} items", commands.len());
        
        // Get metrics and serialize/deserialize
        info!("Getting metrics...");
        let metrics = ext_guard.metrics();
        
        info!("Metrics reference obtained, serializing...");
        let metrics_json = serde_json::to_string(&metrics)
            .map_err(|e| format!("Failed to serialize metrics: {}", e))?;
        
        info!("Metrics serialized, deserializing...");
        let metrics: Vec<neomind_core::extension::system::MetricDescriptor> = serde_json::from_str(&metrics_json)
            .map_err(|e| format!("Failed to deserialize metrics: {}", e))?;
        
        info!("Metrics obtained: {} items", metrics.len());
        
        // Create descriptor from the deserialized data
        let descriptor = neomind_core::extension::system::ExtensionDescriptor::with_capabilities(
            metadata,
            commands,
            metrics,
        );
        
        info!("Descriptor created: id='{}', commands={}, metrics={}", 
            descriptor.metadata.id, descriptor.commands.len(), descriptor.metrics.len());
        
        drop(ext_guard);

        Ok((loaded.extension, descriptor))
    }

    /// Load a WASM extension with full descriptor support
    async fn load_wasm(extension_path: &PathBuf) -> Result<(WasmRuntime, neomind_core::extension::system::ExtensionDescriptor), String> {
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
    async fn run(&mut self) {
        debug!("Starting IPC message loop");

        // Note: We no longer send Ready here - we wait for Init message first
        // This ensures proper handshake sequence: host sends Init, we respond with Ready

        debug!("Waiting for Init message from host");

        // Start stdin reader thread (reads all stdin messages and routes them)
        let _stdin_reader_handle = start_stdin_reader();
        eprintln!("[Runner] Stdin reader thread started");

        // Start IPC capability forwarder thread for WASM extensions
        // Native extensions don't need this since they don't make capability requests during initialization
        let ipc_forwarder_handle = if self.extension_type == ExtensionType::Wasm {
            self.start_ipc_forwarder()
        } else {
            None
        };

        while self.running {
            debug!("Waiting for IPC message...");

            // Poll event queue with a small timeout
            match pop_event() {
                Some(message) => {
                    debug!("Received IPC message from queue: {:?}", std::mem::discriminant(&message));
                    self.handle_message(message).await;
                }
                None => {
                    // No message, sleep asynchronously to avoid blocking thread
                    tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                }
            }
        }

        // Wait for IPC forwarder thread to finish
        if let Some(handle) = ipc_forwarder_handle {
            debug!("Waiting for IPC forwarder thread to finish");
            // The thread will exit when the channels are dropped
            drop(handle);
        }

        debug!("Extension runner shutting down");
    }

    /// Start the IPC capability forwarder thread
    ///
    /// This thread forwards capability requests from WASM extensions to the main process
    /// via IPC, and returns the results back to the WASM extension.
    fn start_ipc_forwarder(&mut self) -> Option<std::thread::JoinHandle<()>> {
        let request_rx = self.ipc_request_rx.take()?;
        let response_tx = self.ipc_response_tx.take()?;

        debug!("Starting IPC capability forwarder thread");

        Some(std::thread::spawn(move || {
            debug!("IPC forwarder thread started");

            while let Ok(request) = request_rx.recv() {
                debug!(
                    request_id = request.request_id,
                    capability = %request.capability,
                    "Forwarding capability request to main process"
                );

                // Send CapabilityRequest to main process via stdout
                let ipc_request = IpcResponse::CapabilityRequest {
                    request_id: request.request_id,
                    capability: request.capability.clone(),
                    params: request.params.clone(),
                };

                // Serialize and send via stdout
                let result = match ipc_request.to_bytes() {
                    Ok(bytes) => {
                        let frame = IpcFrame::new(bytes);
                        let encoded = frame.encode();

                        // Write to stdout (synchronized with main message loop)
                        // Note: We use a simple approach here - in production,
                        // we might need a dedicated stdout mutex
                        let mut stdout = std::io::stdout().lock();
                        if let Err(e) = stdout.write_all(&encoded) {
                            error!(error = %e, "Failed to write CapabilityRequest to stdout");
                            SyncIpcResponse {
                                request_id: request.request_id,
                                result: json!({}),
                                error: Some(format!("Failed to write to stdout: {}", e)),
                            }
                        } else if let Err(e) = stdout.flush() {
                            error!(error = %e, "Failed to flush stdout");
                            SyncIpcResponse {
                                request_id: request.request_id,
                                result: json!({}),
                                error: Some(format!("Failed to flush stdout: {}", e)),
                            }
                        } else {
                            // Wait for response from main process via stdin
                            // Read length prefix
                            let mut len_bytes = [0u8; 4];
                            let mut stdin = std::io::stdin().lock();
                            match stdin.read_exact(&mut len_bytes) {
                                Ok(_) => {
                                    let len = u32::from_le_bytes(len_bytes) as usize;
                                    if len > 10 * 1024 * 1024 {
                                        SyncIpcResponse {
                                            request_id: request.request_id,
                                            result: json!({}),
                                            error: Some("Response too large".to_string()),
                                        }
                                    } else {
                                        let mut payload = vec![0u8; len];
                                        match stdin.read_exact(&mut payload) {
                                            Ok(_) => {
                                                match IpcResponse::from_bytes(&payload) {
                                                    Ok(IpcResponse::CapabilityResult { request_id: resp_id, result, error }) => {
                                                        if resp_id != request.request_id {
                                                            warn!(
                                                                expected = request.request_id,
                                                                got = resp_id,
                                                                "Request ID mismatch"
                                                            );
                                                        }
                                                        SyncIpcResponse {
                                                            request_id: request.request_id,
                                                            result,
                                                            error,
                                                        }
                                                    }
                                                    Ok(other) => {
                                                        error!("Unexpected response type: {:?}", std::mem::discriminant(&other));
                                                        SyncIpcResponse {
                                                            request_id: request.request_id,
                                                            result: json!({}),
                                                            error: Some("Unexpected response type".to_string()),
                                                        }
                                                    }
                                                    Err(e) => {
                                                        error!(error = %e, "Failed to parse response");
                                                        SyncIpcResponse {
                                                            request_id: request.request_id,
                                                            result: json!({}),
                                                            error: Some(format!("Failed to parse response: {}", e)),
                                                        }
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                error!(error = %e, "Failed to read response payload");
                                                SyncIpcResponse {
                                                    request_id: request.request_id,
                                                    result: json!({}),
                                                    error: Some(format!("Failed to read response: {}", e)),
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!(error = %e, "Failed to read response length");
                                    SyncIpcResponse {
                                        request_id: request.request_id,
                                        result: json!({}),
                                        error: Some(format!("Failed to read response length: {}", e)),
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to serialize CapabilityRequest");
                        SyncIpcResponse {
                            request_id: request.request_id,
                            result: json!({}),
                            error: Some(format!("Failed to serialize request: {}", e)),
                        }
                    }
                };

                if response_tx.send(result).is_err() {
                    debug!("Response channel closed, exiting forwarder thread");
                    break;
                }
            }

            debug!("IPC forwarder thread exiting");
        }))
    }

    fn receive_message(&mut self) -> Result<Option<IpcMessage>, String> {
        debug!("Reading length prefix from stdin...");
        let mut len_bytes = [0u8; 4];
        match std::io::stdin().read_exact(&mut len_bytes) {
            Ok(()) => {
                debug!("Read length prefix successfully");
            }
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                debug!("stdin closed (UnexpectedEof)");
                return Ok(None);
            }
            Err(e) => {
                error!(error = %e, "Failed to read length prefix");
                return Err(format!("Failed to read length: {}", e));
            }
        }

        let len = u32::from_le_bytes(len_bytes) as usize;
        if len > 10 * 1024 * 1024 {
            return Err(format!("Message too large: {} bytes", len));
        }

        debug!("Reading payload of {} bytes", len);
        let mut payload = vec![0u8; len];
        std::io::stdin().read_exact(&mut payload)
            .map_err(|e| {
                error!(error = %e, "Failed to read payload");
                format!("Failed to read payload: {}", e)
            })?;

        let message = IpcMessage::from_bytes(&payload)
            .map_err(|e| {
                error!(error = %e, "Failed to decode message");
                format!("Failed to decode message: {}", e)
            })?;

        debug!(message_type = ?std::mem::discriminant(&message), "Received IPC message");
        Ok(Some(message))
    }

    fn send_response(&mut self, response: IpcResponse) {
        debug!(response_type = ?std::mem::discriminant(&response), "Sending IPC response");

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

        match std::io::stdout().write_all(&bytes) {
            Ok(_) => {
                debug!("Response written to stdout");
            }
            Err(e) => {
                error!(error = %e, "Failed to write response");
                return;
            }
        }

        match std::io::stdout().flush() {
            Ok(_) => {
                debug!("Stdout flushed successfully");
            }
            Err(e) => {
                error!(error = %e, "Failed to flush stdout");
            }
        }
    }

    /// Send a capability request to the host and wait for response
    /// This is used for bidirectional IPC communication
    fn invoke_host_capability(&mut self, capability: &str, params: &serde_json::Value) -> serde_json::Value {
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

        if let Err(e) = std::io::stdout().write_all(&bytes) {
            error!(error = %e, "Failed to write CapabilityRequest");
            return json!({"success": false, "error": "Failed to write request"});
        }

        if let Err(e) = std::io::stdout().flush() {
            error!(error = %e, "Failed to flush CapabilityRequest");
            return json!({"success": false, "error": "Failed to flush request"});
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
            IpcResponse::CapabilityResult { request_id: resp_id, result, error } => {
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

                // Call configure on the extension if it's a native extension
                if self.extension_type == ExtensionType::Native {
                    if let Some(ref ext) = self.extension {
                        let ext_clone = ext.clone();
                        let config_clone = config.clone();
                        
                        // Call configure asynchronously (no block_on needed)
                        let mut ext_guard = ext_clone.write().await;
                        let configure_result = ext_guard.configure(&config_clone).await;
                        
                        match configure_result {
                            Ok(_) => {
                                debug!("Extension configure called successfully");
                            }
                            Err(e) => {
                                warn!(error = %e, "Extension configure failed, continuing anyway");
                            }
                        }
                    }
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
                        debug!(json_len = json_str.len(), "Descriptor serialized successfully");
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

            IpcMessage::ExecuteCommand { command, args, request_id } => {
                self.handle_execute_command(command, args, request_id).await;
            }

            IpcMessage::ProduceMetrics { request_id } => {
                self.handle_produce_metrics(request_id).await;
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
                    let ext_guard = extension.read().await;
                    ext_guard.event_subscriptions().iter().map(|s| s.to_string()).collect()
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
                self.running = false;
            }

            IpcMessage::Ping { timestamp } => {
                self.send_response(IpcResponse::Pong { timestamp });
            }

            // Streaming support
            IpcMessage::GetStreamCapability { request_id } => {
                self.handle_get_stream_capability(request_id).await;
            }

            IpcMessage::InitStreamSession { session_id, extension_id: _, config, client_info: _ } => {
                self.handle_init_stream_session(session_id, config).await;
            }

            IpcMessage::ProcessStreamChunk { request_id, session_id, chunk } => {
                self.handle_process_stream_chunk(request_id, session_id, chunk);
            }

            IpcMessage::CloseStreamSession { session_id } => {
                self.handle_close_stream_session(session_id);
            }

            // Stateless mode support
            IpcMessage::ProcessChunk { request_id, chunk } => {
                self.handle_process_chunk(request_id, chunk);
            }

            // Push mode support
            IpcMessage::StartPush { request_id, session_id } => {
                self.handle_start_push(request_id, session_id);
            }

            IpcMessage::StopPush { request_id, session_id } => {
                self.handle_stop_push(request_id, session_id);
            }

            // Batch command support
            IpcMessage::ExecuteBatch { commands, request_id } => {
                self.handle_execute_batch(commands, request_id).await;
            }

            // Capability invocation support (for WASM extensions)
            IpcMessage::InvokeCapability { request_id, capability, params } => {
                self.handle_invoke_capability(request_id, capability, params);
            }

            IpcMessage::SubscribeEvents { request_id, event_types: _, filter: _ } => {
                // Generate subscription ID
                let subscription_id = uuid::Uuid::new_v4().to_string();
                self.send_response(IpcResponse::EventSubscriptionResult {
                    request_id,
                    subscription_id: Some(subscription_id),
                    error: None,
                });
            }

            IpcMessage::UnsubscribeEvents { request_id, subscription_id: _ } => {
                self.send_response(IpcResponse::EventSubscriptionResult {
                    request_id,
                    subscription_id: None,
                    error: None,
                });
            }

            IpcMessage::PollEvents { request_id, subscription_id: _ } => {
                // Return empty events for now
                self.send_response(IpcResponse::EventPollResult {
                    request_id,
                    events: vec![],
                });
            }

            IpcMessage::EventPush { event_type, payload, timestamp: _ } => {
                // Handle event push from host
                info!(
                    event_type = %event_type,
                    payload_preview = %serde_json::to_string(&payload).unwrap_or_else(|_| "invalid".to_string()).chars().take(200).collect::<String>(),
                    "Received event push from host"
                );

                // For native extensions, call async handle_event_with_context method
                if let Some(extension) = &self.extension {
                    info!(
                        event_type = %event_type,
                        "Calling handle_event on extension"
                    );
                    let ext_guard = extension.read().await;

                    // Use handle_event_with_context if capability context is available
                    if let Some(ref ctx) = self.capability_context {
                        match ext_guard.handle_event_with_context(&event_type, &payload, ctx).await {
                            Ok(_) => {
                                info!(
                                    event_type = %event_type,
                                    "Event handled successfully by extension"
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
                        // Fallback to handle_event for backward compatibility
                        match ext_guard.handle_event(&event_type, &payload) {
                            Ok(_) => {
                                info!(
                                    event_type = %event_type,
                                    "Event handled successfully by extension"
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
                    }
                } else {
                    warn!("No extension loaded, cannot handle event");
                    // Call the SDK's event handler for native extensions (fallback)
                    neomind_extension_sdk::capabilities::event::call_event_handler(&event_type, &payload);
                }

                // For WASM extensions, push events to global subscription queues
                if self.extension_type == ExtensionType::Wasm {
                    get_global_event_state().push_event(&event_type, payload);
                    trace!(
                        event_type = %event_type,
                        "Pushed event to WASM global event state"
                    );
                }
            }

            IpcMessage::CapabilityResult { request_id, result, error } => {
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

    async fn handle_execute_command(&mut self, command: String, args: serde_json::Value, request_id: u64) {
        debug!(command = %command, request_id, "Executing command");

        let result = match self.extension_type {
            ExtensionType::Native => {
                self.execute_native_command(&command, &args).await
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

    async fn handle_execute_batch(&mut self, commands: Vec<BatchCommand>, request_id: u64) {
        debug!(request_id, command_count = commands.len(), "Executing batch command");

        let start = std::time::Instant::now();
        let mut results = Vec::new();

        for cmd in &commands {
            let cmd_start = std::time::Instant::now();
            
            let result = match self.extension_type {
                ExtensionType::Native => {
                    self.execute_native_command(&cmd.command, &cmd.args).await
                }
                ExtensionType::Wasm => {
                    self.execute_wasm_command(&cmd.command, &cmd.args)
                }
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

    async fn execute_native_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value, String> {
        let ext = self.extension.as_ref().ok_or("No native extension loaded")?;

        let ext_clone = Arc::clone(ext);

        let ext_guard = ext_clone.read().await;
        ext_guard.execute_command(command, args).await
            .map_err(|e| e.to_string())
    }

    fn execute_wasm_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value, String> {
        let runtime = self.wasm_runtime.as_ref().ok_or("No WASM runtime loaded")?;

        let ipc_client = self.ipc_client.clone();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create runtime: {}", e))?;

        rt.block_on(async {
            // Try new execute_command API first
            match runtime.execute_command(command, args, ipc_client).await {
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

    async fn handle_produce_metrics(&mut self, request_id: u64) {
        debug!(request_id, "Producing metrics");

        let result = match self.extension_type {
            ExtensionType::Native => {
                self.produce_native_metrics().await
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

    async fn produce_native_metrics(&self) -> Result<Vec<neomind_core::extension::system::ExtensionMetricValue>, String> {
        let ext = self.extension.as_ref().ok_or("No native extension loaded")?;

        let ext_clone = Arc::clone(ext);
        let ext_guard = ext_clone.read().await;
        ext_guard.produce_metrics()
            .map_err(|e| e.to_string())
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

    fn handle_invoke_capability(&mut self, request_id: u64, capability: String, params: serde_json::Value) {
        debug!(request_id, capability = %capability, "Invoking capability");

        // Forward the capability request to the host via bidirectional IPC
        // The host will invoke the real capability provider and return the result
        let result = self.invoke_host_capability(&capability, &params);

        // Extract error from result if present
        let (result_value, error) = if let Some(err) = result.get("error").and_then(|e| e.as_str()) {
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

        let ext_clone = Arc::clone(ext);

        self.runtime.block_on(async {
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
    // Statistics Support
    // =========================================================================

    fn handle_get_stats(&mut self, request_id: u64) {
        debug!(request_id, "Getting extension statistics");

        let stats = match self.extension_type {
            ExtensionType::Native => {
                self.get_native_stats()
            }
            ExtensionType::Wasm => {
                // WASM extensions don't support stats yet
                // Return default stats
                neomind_core::extension::system::ExtensionStats::default()
            }
        };

        debug!(request_id, start_count = stats.start_count, stop_count = stats.stop_count, error_count = stats.error_count, "Sending Stats response");
        
        self.send_response(IpcResponse::Stats {
            request_id,
            start_count: stats.start_count,
            stop_count: stats.stop_count,
            error_count: stats.error_count,
            last_error: stats.last_error,
        });
        
        debug!(request_id, "Stats response sent");
    }

    fn get_native_stats(&self) -> neomind_core::extension::system::ExtensionStats {
        debug!("Getting native extension stats");
        
        let ext = match &self.extension {
            Some(e) => e,
            None => {
                debug!("No extension loaded, returning default stats");
                return neomind_core::extension::system::ExtensionStats::default();
            }
        };

        // Get stats synchronously using blocking_read
        // get_stats() is a sync method so this is safe
        let ext_guard = ext.blocking_read();
        debug!("Got extension lock, calling get_stats()");
        let stats = ext_guard.get_stats();
        debug!(start_count = stats.start_count, stop_count = stats.stop_count, error_count = stats.error_count, "Got extension stats");
        stats
    }

    // =========================================================================
    // Streaming Support
    // =========================================================================

    async fn handle_get_stream_capability(&mut self, request_id: u64) {
        debug!(request_id, "Getting stream capability");

        let capability = match self.extension_type {
            ExtensionType::Native => {
                self.get_native_stream_capability().await
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

    async fn get_native_stream_capability(&self) -> Result<Option<neomind_core::extension::StreamCapability>, String> {
        let ext = self.extension.as_ref().ok_or("No native extension loaded")?;

        // Use read().await to avoid blocking in async context
        let ext_guard = ext.read().await;
        Ok(ext_guard.stream_capability())
    }

    async fn handle_init_stream_session(&mut self, session_id: String, config: serde_json::Value) {
        debug!(session_id = %session_id, "Initializing stream session");

        let result = match self.extension_type {
            ExtensionType::Native => {
                self.init_native_stream_session(&session_id, config).await
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
    async fn init_native_stream_session(&self, session_id: &str, config: serde_json::Value) -> Result<(), String> {
        let ext = self.extension.as_ref().ok_or("No native extension loaded")?;

        let ext_guard = ext.read().await;
        
        // Create StreamSession
        let session = neomind_core::extension::StreamSession::new(
            session_id.to_string(),
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

        let ext_clone = Arc::clone(ext);
        let session_id_owned = session_id.to_string();
let handle = self.runtime.handle().clone();

        // Use spawn_blocking to avoid runtime conflicts
        // This moves the async operation to a separate thread pool
        let result = std::thread::spawn(move || {
            handle.block_on(async {
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
        }).join();

        result.map_err(|e| format!("Thread join failed: {:?}", e))?
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

        let ext_clone = Arc::clone(ext);
        let session_id_owned = session_id.to_string();

        // ✨ CRITICAL FIX: Use tokio::task::spawn_blocking instead of runtime.block_on
        //
        // Extension Runner's main() is async, so we're already in a Tokio runtime context.
        // Using runtime.block_on() would try to block a thread that's already driving
        // async tasks, causing "Cannot start a runtime from within a runtime" panic.
        //
        // spawn_blocking moves the closure to a dedicated blocking thread pool,
        // which is safe and won't conflict with the async runtime.
        let handle = self.runtime.handle();
        
        tokio::task::block_in_place(|| {
            handle.block_on(async move {
                let ext_guard = ext_clone.read().await;
                ext_guard.close_session(&session_id_owned).await
                    .map_err(|e| e.to_string())
            })
        })
    }

    // =========================================================================
    // Stateless Mode Support
    // =========================================================================

    fn handle_process_chunk(
        &mut self,
        request_id: u64,
        chunk: neomind_core::extension::isolated::StreamDataChunk,
    ) {
        debug!(
            request_id,
            sequence = chunk.sequence,
            "Processing stateless chunk"
        );

        let result = match self.extension_type {
            ExtensionType::Native => {
                self.process_native_chunk(chunk)
            }
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
                    kind: neomind_core::extension::isolated::ErrorKind::ExecutionFailed,
                });
            }
        }
    }

    fn process_native_chunk(
        &self,
        chunk: neomind_core::extension::isolated::StreamDataChunk,
    ) -> Result<neomind_core::extension::StreamResult, String> {
        let ext = self.extension.as_ref().ok_or("No native extension loaded")?;

        let ext_clone = Arc::clone(ext);

        self.runtime.block_on(async {
            let ext_guard = ext_clone.read().await;

            // Convert StreamDataChunk to DataChunk
            let data_chunk = neomind_core::extension::DataChunk {
                sequence: chunk.sequence,
                data_type: neomind_core::extension::StreamDataType::from_mime_type(&chunk.data_type)
                    .unwrap_or(neomind_core::extension::StreamDataType::Binary),
                data: chunk.data,
                timestamp: chunk.timestamp,
                metadata: None,
                is_last: chunk.is_last,
            };

            ext_guard.process_chunk(data_chunk).await
                .map_err(|e| e.to_string())
        })
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
            ExtensionType::Native => {
                self.start_native_push(&session_id)
            }
            ExtensionType::Wasm => {
                Err("Push mode not supported for WASM".to_string())
            }
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
        let ext = self.extension.as_ref().ok_or("No native extension loaded")?;

        let ext_clone = Arc::clone(ext);
        let session_id_owned = session_id.to_string();

        self.runtime.block_on(async {
            let ext_guard = ext_clone.read().await;
            ext_guard.start_push(&session_id_owned).await
                .map_err(|e| e.to_string())
        })
    }

    fn handle_stop_push(&mut self, request_id: u64, session_id: String) {
        debug!(
            request_id,
            session_id = %session_id,
            "Stopping push mode"
        );

        let result = match self.extension_type {
            ExtensionType::Native => {
                self.stop_native_push(&session_id)
            }
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
                // Log but still return success - extension might have already stopped
                warn!(
                    session_id = %session_id,
                    error = %e,
                    "Error stopping push mode (may already be stopped)"
                );
                self.send_response(IpcResponse::PushStopped {
                    request_id,
                    session_id,
                    success: true,
                });
            }
        }
    }

    fn stop_native_push(&self, session_id: &str) -> Result<(), String> {
        let ext = self.extension.as_ref().ok_or("No native extension loaded")?;

        let ext_clone = Arc::clone(ext);
        let session_id_owned = session_id.to_string();

        self.runtime.block_on(async {
            let ext_guard = ext_clone.read().await;
            ext_guard.stop_push(&session_id_owned).await
                .map_err(|e| e.to_string())
        })
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
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
    eprintln!("[Extension Runner] main function called");
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

    eprintln!("[Extension Runner] calling Runner::load");
    let mut runner = match Runner::load(&args.extension_path).await {
        Ok(r) => {
            eprintln!("[Extension Runner] Runner::load returned successfully");
            r
        }
        Err(e) => {
            error!(error = %e, "Failed to load extension");
            std::process::exit(1);
        }
    };

    eprintln!("[Extension Runner] calling runner.run()");
    runner.run().await;

    debug!("Extension runner exiting normally");
}
