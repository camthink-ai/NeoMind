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

use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use tracing::{debug, error, info};

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
    /// Loaded extension
    extension: DynExtension,
    /// Extension metadata
    metadata: neomind_core::extension::system::ExtensionMetadata,
    /// Extension type
    extension_type: ExtensionType,
    /// Stdin reader
    stdin: BufReader<std::io::Stdin>,
    /// Stdout writer
    stdout: BufWriter<std::io::Stdout>,
    /// Running flag
    running: bool,
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
        let (extension, metadata) = match extension_type {
            ExtensionType::Native => {
                Self::load_native(extension_path)?
            }
            ExtensionType::Wasm => {
                Self::load_wasm(extension_path)?
            }
        };

        info!(
            extension_id = %metadata.id,
            name = %metadata.name,
            version = %metadata.version,
            extension_type = ?extension_type,
            "Extension loaded successfully"
        );

        Ok(Self {
            extension,
            metadata,
            extension_type,
            stdin: BufReader::new(std::io::stdin()),
            stdout: BufWriter::new(std::io::stdout()),
            running: true,
        })
    }

    /// Load a native extension (.so, .dylib, .dll)
    fn load_native(extension_path: &PathBuf) -> Result<(DynExtension, neomind_core::extension::system::ExtensionMetadata), String> {
        let loader = NativeExtensionLoader::new();
        let loaded = loader.load(extension_path).map_err(|e| format!("Failed to load native extension: {}", e))?;

        let ext_guard = loaded.extension.blocking_read();
        let metadata = ext_guard.metadata().clone();
        drop(ext_guard);

        Ok((loaded.extension, metadata))
    }

    /// Load a WASM extension (.wasm)
    fn load_wasm(extension_path: &PathBuf) -> Result<(DynExtension, neomind_core::extension::system::ExtensionMetadata), String> {
        // Use neomind-sandbox to load WASM
        use neomind_sandbox::{Sandbox, SandboxConfig};

        info!("Loading WASM extension using sandbox");

        // Create sandbox with config
        let config = SandboxConfig {
            max_memory_mb: 256,
            max_execution_time_secs: 30,
            allow_wasi: true,
        };

        let sandbox = Arc::new(Sandbox::new(config).map_err(|e| format!("Failed to create sandbox: {}", e))?);

        // Load metadata first
        let metadata = Self::load_wasm_metadata(extension_path)?;

        // Load the WASM module
        let module_name = metadata.id.clone();
        let sandbox_clone = Arc::clone(&sandbox);

        // Use tokio runtime for async loading
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create runtime: {}", e))?;

        rt.block_on(async {
            sandbox_clone.load_module_from_file(&module_name, extension_path).await
        }).map_err(|e| format!("Failed to load WASM module: {}", e))?;

        // Create WasmExtension wrapper
        let wasm_ext = WasmExtensionWrapper::new(metadata.clone(), sandbox, module_name);
        let extension: DynExtension = Arc::new(tokio::sync::RwLock::new(Box::new(wasm_ext)));

        Ok((extension, metadata))
    }

    /// Load WASM metadata from sidecar JSON or filename
    fn load_wasm_metadata(extension_path: &PathBuf) -> Result<neomind_core::extension::system::ExtensionMetadata, String> {
        // Try to load from sidecar JSON file
        let json_path = extension_path.with_extension("json");
        if json_path.exists() {
            if let Ok(meta) = Self::load_metadata_from_json(&json_path) {
                return Ok(meta);
            }
        }

        // Try to find manifest.json in parent directories (.nep package structure)
        if let Some(manifest_path) = Self::find_nep_manifest(extension_path) {
            if manifest_path.exists() {
                if let Ok(meta) = Self::load_metadata_from_json(&manifest_path) {
                    return Ok(meta);
                }
            }
        }

        // Fall back to filename
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

    /// Load metadata from JSON file
    fn load_metadata_from_json(json_path: &PathBuf) -> Result<neomind_core::extension::system::ExtensionMetadata, String> {
        let content = std::fs::read_to_string(json_path)
            .map_err(|e| format!("Failed to read JSON: {}", e))?;

        #[derive(serde::Deserialize)]
        struct MetadataJson {
            id: String,
            name: String,
            version: String,
            #[serde(default)]
            description: Option<String>,
            #[serde(default)]
            author: Option<String>,
            #[serde(default)]
            homepage: Option<String>,
            #[serde(default)]
            license: Option<String>,
        }

        let json: MetadataJson = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse JSON: {}", e))?;

        let version = semver::Version::parse(&json.version).unwrap_or(semver::Version::new(1, 0, 0));

        Ok(neomind_core::extension::system::ExtensionMetadata {
            id: json.id,
            name: json.name,
            version,
            description: json.description,
            author: json.author,
            homepage: json.homepage,
            license: json.license,
            file_path: None,
            config_parameters: None,
        })
    }

    /// Find manifest.json in .nep package folder structure
    fn find_nep_manifest(wasm_path: &PathBuf) -> Option<PathBuf> {
        // Go up from binaries/wasm/extension.wasm to find manifest.json
        let binaries_dir = wasm_path.parent()?; // binaries/wasm
        let wasm_dir = binaries_dir.parent()?;  // binaries
        let extension_folder = wasm_dir.parent()?;  // extension_folder

        let manifest = extension_folder.join("manifest.json");
        if manifest.exists() {
            return Some(manifest);
        }

        None
    }

    /// Run the main loop
    fn run(&mut self) {
        info!("Starting IPC message loop");

        // Send Ready message
        self.send_response(IpcResponse::Ready {
            metadata: self.metadata.clone(),
        });

        // Message loop
        while self.running {
            match self.receive_message() {
                Ok(Some(message)) => {
                    self.handle_message(message);
                }
                Ok(None) => {
                    // EOF, exit
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

    /// Receive an IPC message from stdin
    fn receive_message(&mut self) -> Result<Option<IpcMessage>, String> {
        // Read length prefix (4 bytes)
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

        // Sanity check
        if len > 10 * 1024 * 1024 {
            return Err(format!("Message too large: {} bytes", len));
        }

        // Read payload
        let mut payload = vec![0u8; len];
        self.stdin
            .read_exact(&mut payload)
            .map_err(|e| format!("Failed to read payload: {}", e))?;

        // Decode message
        let message = IpcMessage::from_bytes(&payload)
            .map_err(|e| format!("Failed to decode message: {}", e))?;

        debug!(message = ?message, "Received IPC message");
        Ok(Some(message))
    }

    /// Send an IPC response to stdout
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

        if let Err(e) = self.stdout.flush() {
            error!(error = %e, "Failed to flush stdout");
        }
    }

    /// Handle an incoming IPC message
    fn handle_message(&mut self, message: IpcMessage) {
        match message {
            IpcMessage::Init { config: _ } => {
                // Already initialized, just acknowledge
                self.send_response(IpcResponse::Ready {
                    metadata: self.metadata.clone(),
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
                    metadata: self.metadata.clone(),
                });
            }

            IpcMessage::Shutdown => {
                info!("Received shutdown command");
                self.running = false;
            }

            IpcMessage::Ping { timestamp } => {
                self.send_response(IpcResponse::Pong { timestamp });
            }
        }
    }

    /// Handle execute command
    fn handle_execute_command(&mut self, command: String, args: serde_json::Value, request_id: u64) {
        debug!(command = %command, request_id, "Executing command");

        // Use tokio runtime for async execution
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                self.send_response(IpcResponse::Error {
                    request_id,
                    error: format!("Failed to create runtime: {}", e),
                    kind: ErrorKind::Internal,
                });
                return;
            }
        };

        let ext_clone = Arc::clone(&self.extension);
        let command_clone = command.clone();

        let result = rt.block_on(async {
            let ext_guard = ext_clone.read().await;
            ext_guard.execute_command(&command_clone, &args).await
        });

        match result {
            Ok(value) => {
                self.send_response(IpcResponse::Success {
                    request_id,
                    data: value,
                });
            }
            Err(e) => {
                let kind = match &e {
                    neomind_core::extension::system::ExtensionError::CommandNotFound(_) => {
                        ErrorKind::CommandNotFound
                    }
                    neomind_core::extension::system::ExtensionError::InvalidArguments(_) => {
                        ErrorKind::InvalidArguments
                    }
                    neomind_core::extension::system::ExtensionError::Timeout(_) => ErrorKind::Timeout,
                    _ => ErrorKind::ExecutionFailed,
                };

                self.send_response(IpcResponse::Error {
                    request_id,
                    error: e.to_string(),
                    kind,
                });
            }
        }
    }

    /// Handle produce metrics
    fn handle_produce_metrics(&mut self, request_id: u64) {
        debug!(request_id, "Producing metrics");

        // Use tokio runtime
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                self.send_response(IpcResponse::Error {
                    request_id,
                    error: format!("Failed to create runtime: {}", e),
                    kind: ErrorKind::Internal,
                });
                return;
            }
        };

        let ext_clone = Arc::clone(&self.extension);

        let result = rt.block_on(async {
            let ext_guard = ext_clone.read().await;
            ext_guard.produce_metrics()
        });

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
                    error: e.to_string(),
                    kind: ErrorKind::Internal,
                });
            }
        }
    }

    /// Handle health check
    fn handle_health_check(&mut self, request_id: u64) {
        debug!(request_id, "Health check");

        // Use tokio runtime
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(_) => {
                self.send_response(IpcResponse::Health {
                    request_id,
                    healthy: false,
                });
                return;
            }
        };

        let ext_clone = Arc::clone(&self.extension);

        let healthy = rt.block_on(async {
            let ext_guard = ext_clone.read().await;
            ext_guard.health_check().await.unwrap_or(false)
        });

        self.send_response(IpcResponse::Health {
            request_id,
            healthy,
        });
    }
}

/// WASM Extension wrapper that implements the Extension trait
/// This wraps the neomind-sandbox Sandbox to provide a unified interface
struct WasmExtensionWrapper {
    metadata: neomind_core::extension::system::ExtensionMetadata,
    sandbox: Arc<neomind_sandbox::Sandbox>,
    module_name: String,
    metrics: Vec<neomind_core::extension::system::MetricDescriptor>,
    commands: Vec<neomind_core::extension::system::CommandDefinition>,
    metric_values: std::sync::RwLock<std::collections::HashMap<String, serde_json::Value>>,
}

impl WasmExtensionWrapper {
    fn new(
        metadata: neomind_core::extension::system::ExtensionMetadata,
        sandbox: Arc<neomind_sandbox::Sandbox>,
        module_name: String,
    ) -> Self {
        Self {
            metadata,
            sandbox,
            module_name,
            metrics: Vec::new(),
            commands: Vec::new(),
            metric_values: std::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl neomind_core::extension::system::Extension for WasmExtensionWrapper {
    fn metadata(&self) -> &neomind_core::extension::system::ExtensionMetadata {
        &self.metadata
    }

    fn metrics(&self) -> &[neomind_core::extension::system::MetricDescriptor] {
        &self.metrics
    }

    fn commands(&self) -> &[neomind_core::extension::system::CommandDefinition] {
        &self.commands
    }

    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> neomind_core::extension::types::Result<serde_json::Value> {
        use neomind_core::extension::system::ExtensionError;

        let result = self.sandbox.execute(&self.module_name, command, args.clone()).await
            .map_err(|e| ExtensionError::ExecutionFailed(format!("{}", e)))?;

        // Cache metric values from result
        if let Some(obj) = result.as_object() {
            let mut values = self.metric_values.write().unwrap();
            for metric in &self.metrics {
                if let Some(value) = obj.get(&metric.name) {
                    values.insert(metric.name.clone(), value.clone());
                }
            }
        }

        Ok(result)
    }

    fn produce_metrics(&self) -> neomind_core::extension::types::Result<Vec<neomind_core::extension::system::ExtensionMetricValue>> {
        use neomind_core::extension::system::{ExtensionMetricValue, MetricDataType};

        let values = self.metric_values.try_read().map_err(|_| {
            neomind_core::extension::system::ExtensionError::ExecutionFailed("Lock error".to_string())
        })?;

        let mut result = Vec::new();
        for metric in &self.metrics {
            if let Some(value) = values.get(&metric.name) {
                let metric_value = match metric.data_type {
                    MetricDataType::Float => value.as_f64().map(|v| ExtensionMetricValue {
                        name: metric.name.clone(),
                        value: v.into(),
                        timestamp: chrono::Utc::now().timestamp_millis(),
                    }),
                    MetricDataType::Integer => value.as_i64().map(|v| ExtensionMetricValue {
                        name: metric.name.clone(),
                        value: v.into(),
                        timestamp: chrono::Utc::now().timestamp_millis(),
                    }),
                    MetricDataType::Boolean => value.as_bool().map(|v| ExtensionMetricValue {
                        name: metric.name.clone(),
                        value: v.into(),
                        timestamp: chrono::Utc::now().timestamp_millis(),
                    }),
                    MetricDataType::String => value.as_str().map(|v| ExtensionMetricValue {
                        name: metric.name.clone(),
                        value: v.into(),
                        timestamp: chrono::Utc::now().timestamp_millis(),
                    }),
                    _ => None,
                };
                if let Some(v) = metric_value {
                    result.push(v);
                }
            }
        }

        Ok(result)
    }

    async fn health_check(&self) -> neomind_core::extension::types::Result<bool> {
        // Try to execute a simple health check
        match self.sandbox.execute(&self.module_name, "health", serde_json::json!({})).await {
            Ok(result) => Ok(result.as_bool().unwrap_or(true)),
            Err(_) => Ok(true), // Assume healthy if health function not implemented
        }
    }

    async fn configure(&mut self, _config: &serde_json::Value) -> neomind_core::extension::types::Result<()> {
        Ok(())
    }
}

fn main() {
    let args = Args::parse();

    // Initialize logging to stderr (stdout is used for IPC)
    let log_level = if args.verbose {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };

    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .with_target(false)
        .with_writer(std::io::stderr) // Important: log to stderr, not stdout
        .with_ansi(false)
        .compact()
        .init();

    info!("NeoMind Extension Runner starting");
    debug!(extension_path = %args.extension_path.display(), "Extension path");

    // Check if file exists
    if !args.extension_path.exists() {
        error!(path = %args.extension_path.display(), "Extension file not found");
        std::process::exit(1);
    }

    // Load the extension
    let mut runner = match Runner::load(&args.extension_path) {
        Ok(r) => r,
        Err(e) => {
            error!(error = %e, "Failed to load extension");
            std::process::exit(1);
        }
    };

    // Run the main loop
    runner.run();

    info!("Extension runner exiting normally");
}
