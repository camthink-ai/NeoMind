//! Process management for isolated extensions
//!
//! This module provides the `IsolatedExtension` wrapper that manages
//! extension processes with automatic restart and health monitoring.
//!
//! # Concurrency Model
//!
//! This implementation uses an "in-flight requests" pattern for high-performance
//! concurrent IPC communication:
//!
//! 1. A background thread continuously reads responses from the extension process
//! 2. Each request is assigned a unique ID and tracked in a HashMap
//! 3. Responses are routed to the correct caller via oneshot channels
//! 4. Multiple concurrent requests can be in-flight simultaneously
//!
//! This design is based on patterns from mature RPC frameworks like tarpc.

use std::io::{BufReader, BufWriter, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use super::in_flight::InFlightRequests;
use super::ipc::{IpcFrame, IpcMessage, IpcResponse};
use super::{IsolatedExtensionError, IsolatedResult};
use crate::extension::system::{ExtensionMetadata, ExtensionMetricValue};

/// Configuration for isolated extension
#[derive(Debug, Clone)]
pub struct IsolatedExtensionConfig {
    /// Maximum startup time in seconds
    pub startup_timeout_secs: u64,
    /// Command execution timeout in seconds
    pub command_timeout_secs: u64,
    /// Maximum memory usage in MB (0 = unlimited)
    pub max_memory_mb: usize,
    /// Restart on crash
    pub restart_on_crash: bool,
    /// Maximum restart attempts
    pub max_restart_attempts: u32,
    /// Restart cooldown in seconds
    pub restart_cooldown_secs: u64,
}

impl Default for IsolatedExtensionConfig {
    fn default() -> Self {
        Self {
            startup_timeout_secs: 30,
            command_timeout_secs: 30,
            max_memory_mb: 4096,  // Increased for YOLO - will optimize architecture later
            restart_on_crash: true,
            max_restart_attempts: 3,
            restart_cooldown_secs: 5,
        }
    }
}

/// Process-isolated extension wrapper
pub struct IsolatedExtension {
    /// Extension ID
    extension_id: String,
    /// Path to extension binary
    extension_path: std::path::PathBuf,
    /// Child process handle
    process: Mutex<Option<Child>>,
    /// Stdin writer (for sending messages)
    stdin: Mutex<Option<BufWriter<std::process::ChildStdin>>>,
    /// In-flight request tracker
    in_flight: InFlightRequests,
    /// Shutdown signal for the receiver thread
    shutdown_tx: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    /// Extension descriptor (set after initialization)
    descriptor: Mutex<Option<super::super::system::ExtensionDescriptor>>,
    /// Configuration
    config: IsolatedExtensionConfig,
    /// Restart counter
    restart_count: AtomicU64,
    /// Last restart time
    last_restart: Mutex<Option<Instant>>,
    /// Running state (shared with background receiver thread)
    running: Arc<AtomicBool>,
    /// Process ID for resource monitoring
    process_id: Mutex<Option<u32>>,
    /// Last resource check time
    last_resource_check: Mutex<Option<Instant>>,
}

impl IsolatedExtension {
    /// Create a new isolated extension wrapper
    pub fn new(
        extension_id: impl Into<String>,
        extension_path: impl Into<std::path::PathBuf>,
        config: IsolatedExtensionConfig,
    ) -> Self {
        Self {
            extension_id: extension_id.into(),
            extension_path: extension_path.into(),
            process: Mutex::new(None),
            stdin: Mutex::new(None),
            in_flight: InFlightRequests::new(Duration::from_secs(config.command_timeout_secs)),
            shutdown_tx: Mutex::new(None),
            descriptor: Mutex::new(None),
            config,
            restart_count: AtomicU64::new(0),
            last_restart: Mutex::new(None),
            running: Arc::new(AtomicBool::new(false)),
            process_id: Mutex::new(None),
            last_resource_check: Mutex::new(None),
        }
    }

    /// Find the extension runner binary
    ///
    /// Looks for `neomind-extension-runner` in:
    /// 1. Same directory as current executable
    /// 2. PATH environment variable
    fn find_extension_runner() -> Result<std::path::PathBuf, String> {
        let runner_name = if cfg!(windows) {
            "neomind-extension-runner.exe"
        } else {
            "neomind-extension-runner"
        };

        // First, try same directory as current executable
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let runner_in_exe_dir = exe_dir.join(runner_name);
                if runner_in_exe_dir.exists() {
                    return Ok(runner_in_exe_dir);
                }
            }
        }

        // Second, try to find in PATH
        if let Ok(path_var) = std::env::var("PATH") {
            let separator = if cfg!(windows) { ";" } else { ":" };
            for path in path_var.split(separator) {
                let runner_in_path = std::path::Path::new(path).join(runner_name);
                if runner_in_path.exists() {
                    return Ok(runner_in_path);
                }
            }
        }

        Err(format!(
            "{} not found in executable directory or PATH",
            runner_name
        ))
    }

    /// Start the extension process
    pub async fn start(&self) -> IsolatedResult<()> {
        let mut process_guard = self.process.lock().await;

        if process_guard.is_some() {
            return Err(IsolatedExtensionError::AlreadyRunning);
        }

        // Find the extension runner binary
        let runner_path = Self::find_extension_runner().map_err(|e| {
            IsolatedExtensionError::SpawnFailed(format!(
                "Could not find neomind-extension-runner: {}. Please ensure it is built and in PATH or same directory as the main executable.",
                e
            ))
        })?;

        info!(
            runner_path = %runner_path.display(),
            extension_path = %self.extension_path.display(),
            "Spawning extension runner process"
        );

        // Spawn the extension runner process
        let extension_dir = self.extension_path.parent()
            .ok_or_else(|| IsolatedExtensionError::SpawnFailed("Invalid extension path".to_string()))?;

        let mut child = Command::new(&runner_path)
            .arg("--extension-path")
            .arg(&self.extension_path)
            .env("NEOMIND_EXTENSION_DIR", extension_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| IsolatedExtensionError::SpawnFailed(e.to_string()))?;

        let stdin = BufWriter::new(child.stdin.take().unwrap());
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let stderr = child.stderr.take().unwrap();

        // Save process ID for resource monitoring
        let pid = child.id();

        *process_guard = Some(child);
        *self.stdin.lock().await = Some(stdin);
        *self.process_id.lock().await = Some(pid);
        self.running.store(true, Ordering::SeqCst);

        // Start the background receiver thread
        let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();
        *self.shutdown_tx.lock().await = Some(shutdown_tx);

        // Get the current tokio runtime handle to pass to the receiver thread
        let rt_handle = tokio::runtime::Handle::current();
        self.spawn_receiver_thread(stdout, shutdown_rx, rt_handle);

        // Spawn stderr reader to prevent pipe buffer from filling up
        let extension_id = self.extension_id.clone();
        std::thread::spawn(move || {
            use std::io::{BufRead, BufReader};
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if let Ok(line) = line {
                    // Use eprintln to output directly, or tracing::info for structured logging
                    eprintln!("[Extension:{}] {}", extension_id, line);
                }
            }
        });

        // Send initialization message
        self.send_message(&IpcMessage::Init {
            config: serde_json::json!({}),
        })
        .await?;

        // Wait for ready response with timeout
        // Use request_id = 0 for initialization
        let rx = self.in_flight.register_with_id(0).await;
        let response = self
            .in_flight
            .wait_with_timeout(
                0,
                rx,
                Duration::from_secs(self.config.startup_timeout_secs),
            )
            .await
            .map_err(|e| match e {
                super::in_flight::InFlightError::Timeout(ms) => {
                    IsolatedExtensionError::Timeout(ms)
                }
                super::in_flight::InFlightError::ChannelClosed => {
                    IsolatedExtensionError::IpcError("Response channel closed".to_string())
                }
            })?;

        match response {
            IpcResponse::Ready { descriptor } => {
                *self.descriptor.lock().await = Some(descriptor);
                info!(extension_id = %self.extension_id, "Extension started successfully");
                Ok(())
            }
            IpcResponse::Error { error, .. } => {
                self.kill_internal(&mut process_guard).await;
                Err(IsolatedExtensionError::SpawnFailed(error))
            }
            _ => {
                self.kill_internal(&mut process_guard).await;
                Err(IsolatedExtensionError::InvalidResponse(
                    "Expected Ready response".to_string(),
                ))
            }
        }
    }

    /// Spawn the background receiver thread
    ///
    /// This thread continuously reads responses from the extension process
    /// and routes them to the correct waiting caller via the in-flight tracker.
    fn spawn_receiver_thread(
        &self,
        mut stdout: BufReader<std::process::ChildStdout>,
        shutdown_rx: std::sync::mpsc::Receiver<()>,
        rt_handle: tokio::runtime::Handle,
    ) {
        let extension_id = self.extension_id.clone();
        let in_flight = self.in_flight.clone();
        let running = self.running.clone();

        std::thread::spawn(move || {
            debug!(extension_id = %extension_id, "Receiver thread started");

            loop {
                // Check for shutdown signal (non-blocking)
                match shutdown_rx.try_recv() {
                    Ok(()) => {
                        debug!(extension_id = %extension_id, "Receiver thread received shutdown");
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        debug!(extension_id = %extension_id, "Shutdown channel disconnected");
                        break;
                    }
                }

                // Read length prefix (4 bytes) - this is blocking but that's OK in a dedicated thread
                let mut len_bytes = [0u8; 4];
                match stdout.read_exact(&mut len_bytes) {
                    Ok(()) => {}
                    Err(e) => {
                        // Process likely terminated or stdout closed
                        if running.load(Ordering::SeqCst) {
                            warn!(extension_id = %extension_id, error = %e, "Failed to read from extension stdout");
                        }
                        running.store(false, Ordering::SeqCst);
                        break;
                    }
                }

                let len = u32::from_le_bytes(len_bytes) as usize;

                // Sanity check
                if len > 10 * 1024 * 1024 {
                    error!(extension_id = %extension_id, len, "Response too large");
                    running.store(false, Ordering::SeqCst);
                    break;
                }

                // Read payload
                let mut payload = vec![0u8; len];
                if let Err(e) = stdout.read_exact(&mut payload) {
                    error!(extension_id = %extension_id, error = %e, "Failed to read response payload");
                    running.store(false, Ordering::SeqCst);
                    break;
                }

                // Parse response
                let response = match IpcResponse::from_bytes(&payload) {
                    Ok(r) => r,
                    Err(e) => {
                        warn!(extension_id = %extension_id, error = %e, "Failed to parse response");
                        continue;
                    }
                };

                // Route response to the correct waiting caller
                if let Some(request_id) = response.request_id() {
                    debug!(
                        extension_id = %extension_id,
                        request_id,
                        "Routing response"
                    );

                    // Use the passed runtime handle to complete async operations
                    rt_handle.block_on(async {
                        in_flight.complete(request_id, response).await;
                    });
                } else {
                    // Response without request_id (e.g., Ready during init)
                    // Route to request_id 0
                    debug!(
                        extension_id = %extension_id,
                        "Routing response without request_id to request_id=0"
                    );
                    rt_handle.block_on(async {
                        in_flight.complete(0, response).await;
                    });
                }
            }

            // Cancel any pending requests on exit
            rt_handle.block_on(async {
                let count = in_flight.cancel_all().await;
                if count > 0 {
                    debug!(extension_id = %extension_id, count, "Cancelled pending requests");
                }
            });

            debug!(extension_id = %extension_id, "Receiver thread exiting");
        });
    }

    /// Stop the extension process
    pub async fn stop(&self) -> IsolatedResult<()> {
        let mut process_guard = self.process.lock().await;

        if process_guard.is_none() {
            return Err(IsolatedExtensionError::NotRunning);
        }

        // Send shutdown signal to receiver thread
        if let Some(tx) = self.shutdown_tx.lock().await.take() {
            let _ = tx.send(());
        }

        // Send shutdown message to extension
        let _ = self.send_message(&IpcMessage::Shutdown).await;

        // Wait for process to exit
        if let Some(mut child) = process_guard.take() {
            let _ = child.wait();
        }

        *self.stdin.lock().await = None;
        self.running.store(false, Ordering::SeqCst);

        // Cancel any pending requests
        self.in_flight.cancel_all().await;

        info!(extension_id = %self.extension_id, "Extension stopped");
        Ok(())
    }

    /// Execute a command
    pub async fn execute_command(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> IsolatedResult<serde_json::Value> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(IsolatedExtensionError::NotRunning);
        }

        // Register the request and get a receiver
        let (request_id, rx) = self.in_flight.register().await;

        debug!(
            extension_id = %self.extension_id,
            request_id,
            command,
            "Sending execute command"
        );

        // Send the request
        self.send_message(&IpcMessage::ExecuteCommand {
            command: command.to_string(),
            args: args.clone(),
            request_id,
        })
        .await?;

        // Wait for the response with timeout
        let response = self
            .in_flight
            .wait_with_timeout(
                request_id,
                rx,
                Duration::from_secs(self.config.command_timeout_secs),
            )
            .await
            .map_err(|e| match e {
                super::in_flight::InFlightError::Timeout(ms) => {
                    IsolatedExtensionError::Timeout(ms)
                }
                super::in_flight::InFlightError::ChannelClosed => {
                    IsolatedExtensionError::IpcError("Response channel closed".to_string())
                }
            })?;

        match response {
            IpcResponse::Success { data, .. } => Ok(data),
            IpcResponse::Error { error, kind, .. } => {
                use super::ipc::ErrorKind;
                match kind {
                    ErrorKind::CommandNotFound => Err(IsolatedExtensionError::IpcError(error)),
                    ErrorKind::Timeout => Err(IsolatedExtensionError::Timeout(
                        self.config.command_timeout_secs * 1000,
                    )),
                    _ => Err(IsolatedExtensionError::IpcError(error)),
                }
            }
            _ => Err(IsolatedExtensionError::InvalidResponse(format!(
                "Expected Success or Error response, got {:?}",
                std::mem::discriminant(&response)
            ))),
        }
    }

    /// Get metrics from extension
    pub async fn produce_metrics(&self) -> IsolatedResult<Vec<ExtensionMetricValue>> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(IsolatedExtensionError::NotRunning);
        }

        let (request_id, rx) = self.in_flight.register().await;

        self.send_message(&IpcMessage::ProduceMetrics { request_id })
            .await?;

        let response = self
            .in_flight
            .wait_with_timeout(request_id, rx, Duration::from_secs(5))
            .await
            .map_err(|e| match e {
                super::in_flight::InFlightError::Timeout(ms) => {
                    IsolatedExtensionError::Timeout(ms)
                }
                super::in_flight::InFlightError::ChannelClosed => {
                    IsolatedExtensionError::IpcError("Response channel closed".to_string())
                }
            })?;

        match response {
            IpcResponse::Metrics { metrics, .. } => Ok(metrics),
            IpcResponse::Error { error, .. } => Err(IsolatedExtensionError::IpcError(error)),
            _ => Err(IsolatedExtensionError::InvalidResponse(
                "Expected Metrics response".to_string(),
            )),
        }
    }

    /// Check if process is alive
    pub fn is_alive(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Get the extension ID
    pub fn extension_id(&self) -> String {
        self.extension_id.clone()
    }

    /// Check extension health via IPC
    pub async fn health_check(&self) -> IsolatedResult<bool> {
        if !self.running.load(Ordering::SeqCst) {
            return Ok(false);
        }

        let (request_id, rx) = self.in_flight.register().await;

        if let Err(_) = self.send_message(&IpcMessage::HealthCheck { request_id }).await {
            return Ok(false);
        }

        match self
            .in_flight
            .wait_with_timeout(request_id, rx, Duration::from_secs(5))
            .await
        {
            Ok(IpcResponse::Health { healthy, .. }) => Ok(healthy),
            _ => Ok(false),
        }
    }

    // =========================================================================
    // Streaming Support
    // =========================================================================

    /// Get stream capability via IPC
    pub async fn stream_capability(&self) -> IsolatedResult<Option<super::super::stream::StreamCapability>> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(IsolatedExtensionError::NotRunning);
        }

        let (request_id, rx) = self.in_flight.register().await;

        self.send_message(&IpcMessage::GetStreamCapability { request_id }).await?;

        let response = self
            .in_flight
            .wait_with_timeout(request_id, rx, Duration::from_secs(5))
            .await
            .map_err(|e| match e {
                super::in_flight::InFlightError::Timeout(ms) => {
                    IsolatedExtensionError::Timeout(ms)
                }
                super::in_flight::InFlightError::ChannelClosed => {
                    IsolatedExtensionError::IpcError("Response channel closed".to_string())
                }
            })?;

        match response {
            IpcResponse::StreamCapability { capability, .. } => {
                match capability {
                    Some(cap_json) => {
                        let cap: super::super::stream::StreamCapability = 
                            serde_json::from_value(cap_json)
                                .map_err(|e| IsolatedExtensionError::IpcError(e.to_string()))?;
                        Ok(Some(cap))
                    }
                    None => Ok(None),
                }
            }
            IpcResponse::Error { error, .. } => Err(IsolatedExtensionError::IpcError(error)),
            _ => Err(IsolatedExtensionError::InvalidResponse("Expected StreamCapability response".to_string())),
        }
    }

    /// Initialize a stream session via IPC
    pub async fn init_session(&self, session_id: &str, config: serde_json::Value) -> IsolatedResult<()> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(IsolatedExtensionError::NotRunning);
        }

        let client_info = super::ipc::StreamClientInfo {
            client_id: "host".to_string(),
            ip_addr: None,
            user_agent: None,
        };

        self.send_message(&IpcMessage::InitStreamSession {
            session_id: session_id.to_string(),
            extension_id: self.extension_id.clone(),
            config,
            client_info,
        }).await?;

        // Wait for StreamSessionInit response
        // Note: InitStreamSession doesn't have a request_id, so we need to handle it specially
        // For now, just return Ok - the runner will send the response asynchronously
        Ok(())
    }

    /// Process a stream chunk via IPC
    pub async fn process_session_chunk(
        &self,
        session_id: &str,
        chunk: super::super::stream::DataChunk,
    ) -> IsolatedResult<super::super::stream::StreamResult> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(IsolatedExtensionError::NotRunning);
        }

        // Check resources before processing
        self.check_resources().await?;

        let stream_chunk = super::ipc::StreamDataChunk {
            sequence: chunk.sequence,
            data_type: chunk.data_type.mime_type(),
            data: chunk.data,
            timestamp: chunk.timestamp,
            is_last: chunk.is_last,
        };

        // Register request and get receiver for response
        let (request_id, rx) = self.in_flight.register().await;

        // Send message with request_id
        self.send_message(&IpcMessage::ProcessStreamChunk {
            request_id,
            session_id: session_id.to_string(),
            chunk: stream_chunk,
        }).await?;

        // Wait for response with timeout (10s for streaming operations)
        let response = tokio::time::timeout(
            Duration::from_secs(10),
            rx
        ).await
        .map_err(|_| IsolatedExtensionError::Timeout(10000))?
        .map_err(|_| IsolatedExtensionError::ChannelClosed)?;

        // Parse response
        match response {
            IpcResponse::StreamChunkResult {
                request_id: _,
                session_id: _,
                input_sequence,
                output_sequence,
                data,
                data_type,
                processing_ms,
            } => {
                // Parse data_type string back to StreamDataType
                let stream_data_type = if data_type.starts_with("image/") {
                    let format = data_type.strip_prefix("image/").unwrap_or("jpeg").to_string();
                    super::super::stream::StreamDataType::Image { format }
                } else if data_type == "application/json" {
                    super::super::stream::StreamDataType::Json
                } else if data_type == "text/plain" {
                    super::super::stream::StreamDataType::Text
                } else {
                    // For video, audio, and other complex types, use Binary
                    // since we don't have all the required metadata from IPC
                    super::super::stream::StreamDataType::Binary
                };

                Ok(super::super::stream::StreamResult::success(
                    Some(input_sequence),
                    output_sequence,
                    data,
                    stream_data_type,
                    processing_ms,
                ))
            }
            IpcResponse::Error { error, .. } => {
                Err(IsolatedExtensionError::ExtensionError(error))
            }
            _ => {
                Err(IsolatedExtensionError::UnexpectedResponse)
            }
        }
    }

    /// Close a stream session via IPC
    pub async fn close_session(&self, session_id: &str) -> IsolatedResult<super::super::stream::SessionStats> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(IsolatedExtensionError::NotRunning);
        }

        self.send_message(&IpcMessage::CloseStreamSession {
            session_id: session_id.to_string(),
        }).await?;

        Ok(super::super::stream::SessionStats::default())
    }

    /// Get extension descriptor
    pub async fn descriptor(&self) -> Option<super::super::system::ExtensionDescriptor> {
        self.descriptor.lock().await.clone()
    }

    /// Get extension metadata
    pub async fn metadata(&self) -> Option<ExtensionMetadata> {
        self.descriptor
            .lock()
            .await
            .as_ref()
            .map(|d| d.metadata.clone())
    }

    /// Get extension commands
    pub async fn commands(&self) -> Vec<super::super::system::ExtensionCommand> {
        self.descriptor
            .lock()
            .await
            .as_ref()
            .map(|d| d.commands.clone())
            .unwrap_or_default()
    }

    /// Get extension metrics descriptors
    pub async fn metrics(&self) -> Vec<super::super::system::MetricDescriptor> {
        self.descriptor
            .lock()
            .await
            .as_ref()
            .map(|d| d.metrics.clone())
            .unwrap_or_default()
    }

    // Internal helper methods

    async fn send_message(&self, msg: &IpcMessage) -> IsolatedResult<()> {
        let mut stdin_guard = self.stdin.lock().await;

        let stdin = stdin_guard
            .as_mut()
            .ok_or(IsolatedExtensionError::NotInitialized)?;

        let payload = msg.to_bytes().map_err(|e| {
            IsolatedExtensionError::IpcError(format!("Serialization error: {}", e))
        })?;

        let frame = IpcFrame::new(payload);
        let bytes = frame.encode();

        stdin
            .write_all(&bytes)
            .map_err(|e| IsolatedExtensionError::IpcError(e.to_string()))?;
        stdin
            .flush()
            .map_err(|e| IsolatedExtensionError::IpcError(e.to_string()))?;

        Ok(())
    }

    async fn kill_internal(&self, process_guard: &mut Option<Child>) {
        // Send shutdown signal to receiver thread
        if let Some(tx) = self.shutdown_tx.lock().await.take() {
            let _ = tx.send(());
        }

        if let Some(mut child) = process_guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.running.store(false, Ordering::SeqCst);
        *self.process_id.lock().await = None;
    }

    /// Check resource usage and restart if necessary
    pub async fn check_resources(&self) -> IsolatedResult<()> {
        // Only check every 5 seconds to avoid overhead
        {
            let mut last_check = self.last_resource_check.lock().await;
            if let Some(last) = *last_check {
                if last.elapsed() < Duration::from_secs(5) {
                    return Ok(());
                }
            }
            *last_check = Some(Instant::now());
        }

        let pid = {
            let pid_guard = self.process_id.lock().await;
            match *pid_guard {
                Some(p) => p,
                None => return Ok(()), // Process not running
            }
        };

        // Check if process is still alive
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            let mut process_guard = self.process.lock().await;
            if let Some(child) = process_guard.as_mut() {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        // Process has exited
                        warn!(
                            extension_id = %self.extension_id,
                            exit_code = status.code(),
                            signal = status.signal(),
                            "Extension process exited unexpectedly"
                        );
                        self.kill_internal(&mut *process_guard).await;
                        return Err(IsolatedExtensionError::Crashed(
                            format!("Process exited with status: {:?}", status)
                        ));
                    }
                    Ok(None) => {
                        // Process still running, check resources
                    }
                    Err(e) => {
                        warn!(extension_id = %self.extension_id, error = %e, "Failed to check process status");
                    }
                }
            }
        }

        #[cfg(not(unix))]
        {
            let mut process_guard = self.process.lock().await;
            if let Some(child) = process_guard.as_mut() {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        warn!(
                            extension_id = %self.extension_id,
                            exit_code = status.code(),
                            "Extension process exited unexpectedly"
                        );
                        self.kill_internal(&mut *process_guard).await;
                        return Err(IsolatedExtensionError::Crashed(
                            format!("Process exited with code: {:?}", status.code())
                        ));
                    }
                    Ok(None) => {}
                    Err(e) => {
                        warn!(extension_id = %self.extension_id, error = %e, "Failed to check process status");
                    }
                }
            }
        }

        // Check memory usage (platform-specific)
        #[cfg(target_os = "linux")]
        {
            if let Ok(memory_kb) = self.get_process_memory_linux(pid) {
                let memory_mb = memory_kb / 1024;
                if memory_mb > self.config.max_memory_mb as u64 {
                    error!(
                        extension_id = %self.extension_id,
                        memory_mb,
                        max_memory_mb = self.config.max_memory_mb,
                        "Extension exceeded memory limit, restarting"
                    );
                    let mut process_guard = self.process.lock().await;
                    self.kill_internal(&mut *process_guard).await;
                    drop(process_guard);

                    // Attempt restart
                    if self.config.restart_on_crash {
                        return self.start().await;
                    }
                    return Err(IsolatedExtensionError::Crashed(
                        format!("Memory limit exceeded: {}MB > {}MB", memory_mb, self.config.max_memory_mb)
                    ));
                }
            }
        }

        #[cfg(target_os = "macos")]
        {
            if let Ok(memory_bytes) = self.get_process_memory_macos(pid) {
                let memory_mb = memory_bytes / (1024 * 1024);
                if memory_mb > self.config.max_memory_mb as u64 {
                    error!(
                        extension_id = %self.extension_id,
                        memory_mb,
                        max_memory_mb = self.config.max_memory_mb,
                        "Extension exceeded memory limit, restarting"
                    );
                    let mut process_guard = self.process.lock().await;
                    self.kill_internal(&mut *process_guard).await;
                    drop(process_guard);

                    // Attempt restart
                    if self.config.restart_on_crash {
                        return self.start().await;
                    }
                    return Err(IsolatedExtensionError::Crashed(
                        format!("Memory limit exceeded: {}MB > {}MB", memory_mb, self.config.max_memory_mb)
                    ));
                }
            }
        }

        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn get_process_memory_linux(&self, pid: u32) -> Result<u64, std::io::Error> {
        let status_path = format!("/proc/{}/status", pid);
        let content = std::fs::read_to_string(status_path)?;

        for line in content.lines() {
            if line.starts_with("VmRSS:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(kb) = parts[1].parse::<u64>() {
                        return Ok(kb);
                    }
                }
            }
        }

        Err(std::io::Error::new(std::io::ErrorKind::NotFound, "VmRSS not found"))
    }

    #[cfg(target_os = "macos")]
    fn get_process_memory_macos(&self, pid: u32) -> Result<u64, std::io::Error> {
        use std::process::Command;

        let output = Command::new("ps")
            .args(&["-o", "rss=", "-p", &pid.to_string()])
            .output()?;

        if output.status.success() {
            let rss_str = String::from_utf8_lossy(&output.stdout);
            if let Ok(rss_kb) = rss_str.trim().parse::<u64>() {
                return Ok(rss_kb * 1024); // Convert KB to bytes
            }
        }

        Err(std::io::Error::new(std::io::ErrorKind::NotFound, "Failed to get memory"))
    }
}

impl Drop for IsolatedExtension {
    fn drop(&mut self) {
        // Attempt graceful shutdown
        // Use block_in_place to allow blocking inside async runtime
        tokio::task::block_in_place(|| {
            if let Some(mut child) = self.process.blocking_lock().take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = IsolatedExtensionConfig::default();
        assert_eq!(config.startup_timeout_secs, 30);
        assert_eq!(config.command_timeout_secs, 30);
        assert_eq!(config.max_memory_mb, 512);
        assert!(config.restart_on_crash);
    }
}
