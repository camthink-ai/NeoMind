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
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;
use tracing::{debug, error, warn};

use super::in_flight::InFlightRequests;
use super::ipc::{IpcFrame, IpcMessage, IpcResponse};
use super::{IsolatedExtensionError, IsolatedResult};
use crate::extension::system::{ExtensionMetadata, ExtensionMetricValue};
use serde_json::Value;

/// Helper function to send a message to the extension process
/// Used for sending IpcMessage types (like CapabilityResult)
fn send_message_to_extension(
    stdin: &Arc<Mutex<Option<BufWriter<std::process::ChildStdin>>>>,
    extension_id: &str,
    message: IpcMessage,
) {
    match message.to_bytes() {
        Ok(bytes) => {
            let frame = IpcFrame::new(bytes);
            let encoded = frame.encode();

            // Try to send - use try_lock to avoid blocking the receiver thread
            if let Ok(mut stdin_guard) = stdin.try_lock() {
                if let Some(stdin_writer) = stdin_guard.as_mut() {
                    if let Err(e) = stdin_writer.write_all(&encoded) {
                        warn!(
                            extension_id = %extension_id,
                            error = %e,
                            "Failed to send message to extension"
                        );
                    } else if let Err(e) = stdin_writer.flush() {
                        warn!(
                            extension_id = %extension_id,
                            error = %e,
                            "Failed to flush message to extension"
                        );
                    } else {
                        debug!(
                            extension_id = %extension_id,
                            "Message sent to extension"
                        );
                    }
                }
            } else {
                warn!(
                    extension_id = %extension_id,
                    "Could not acquire stdin lock to send message"
                );
            }
        }
        Err(e) => {
            warn!(
                extension_id = %extension_id,
                error = %e,
                "Failed to serialize message"
            );
        }
    }
}

/// Helper function to send a response to the extension process
/// Used in the receiver thread for bidirectional communication
fn send_response_to_extension(
    stdin: &Arc<Mutex<Option<BufWriter<std::process::ChildStdin>>>>,
    extension_id: &str,
    response: IpcResponse,
) {
    match response.to_bytes() {
        Ok(bytes) => {
            let frame = IpcFrame::new(bytes);
            let encoded = frame.encode();

            // Try to send - use try_lock to avoid blocking the receiver thread
            if let Ok(mut stdin_guard) = stdin.try_lock() {
                if let Some(stdin_writer) = stdin_guard.as_mut() {
                    if let Err(e) = stdin_writer.write_all(&encoded) {
                        warn!(
                            extension_id = %extension_id,
                            error = %e,
                            "Failed to send response to extension"
                        );
                    } else if let Err(e) = stdin_writer.flush() {
                        warn!(
                            extension_id = %extension_id,
                            error = %e,
                            "Failed to flush response to extension"
                        );
                    } else {
                        debug!(
                            extension_id = %extension_id,
                            "Response sent to extension"
                        );
                    }
                }
            } else {
                warn!(
                    extension_id = %extension_id,
                    "Could not acquire stdin lock to send response"
                );
            }
        }
        Err(e) => {
            warn!(
                extension_id = %extension_id,
                error = %e,
                "Failed to serialize response"
            );
        }
    }
}

// ✨ FIX: IPC 缓冲区池配置
const IPC_BUFFER_POOL_SIZE: usize = 4;  // Reduced from 8 to minimize memory footprint
const IPC_MAX_BUFFER_SIZE: usize = 10 * 1024 * 1024;  // 10MB 最大缓冲区

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
    /// Maximum concurrent requests (0 = unlimited)
    pub max_concurrent_requests: usize,
}

impl Default for IsolatedExtensionConfig {
    fn default() -> Self {
        Self {
            startup_timeout_secs: 30,
            command_timeout_secs: 30,
            // ✨ FIX: Memory limit increased to 2048MB for YOLO extensions
            // Breakdown:
            // - ONNX model: ~100MB
            // - ONNX Runtime memory pool: ~800MB (per-frame inference + caching)
            // - Frame buffers: ~100MB (multiple frames in pipeline)
            // - Detection history: ~20MB
            // - IPC buffers: ~10MB
            // - System overhead: ~100MB
            // - Headroom: ~918MB
            max_memory_mb: 2048,  // Increased from 1024MB for YOLO stability
            restart_on_crash: true,
            max_restart_attempts: 3,
            restart_cooldown_secs: 5,
            max_concurrent_requests: 100,
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
    stdin: Arc<Mutex<Option<BufWriter<std::process::ChildStdin>>>>,
    /// In-flight request tracker
    in_flight: InFlightRequests,
    /// Shutdown signal for the receiver thread
    shutdown_tx: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    /// Extension descriptor (set after initialization)
    descriptor: Mutex<Option<super::super::system::ExtensionDescriptor>>,
    /// Configuration
    config: IsolatedExtensionConfig,
    #[allow(dead_code)]
    /// Restart counter
    restart_count: AtomicU64,
    #[allow(dead_code)]
    /// Last restart time
    last_restart: Mutex<Option<Instant>>,
    /// Running state (shared with background receiver thread)
    running: Arc<AtomicBool>,
    /// Process ID for resource monitoring
    process_id: Mutex<Option<u32>>,
    /// Active request counter for concurrency limiting
    active_requests: Arc<AtomicUsize>,
    /// Last resource check time
    last_resource_check: Mutex<Option<Instant>>,
    /// Event push channel for sending events to extension process
    event_push_tx: Mutex<Option<tokio::sync::mpsc::UnboundedSender<(String, Value)>>>,
    /// Push output channel for receiving PushOutput messages from extension
    /// Uses std::sync::Mutex for thread safety in receiver thread
    push_output_tx: Arc<std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedSender<super::ipc::PushOutputData>>>>,
    /// ✨ FIX: IPC receive buffer pool (reusable buffers) - use std::sync::Mutex for thread safety
    ipc_buffer_pool: Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    /// ✨ FIX: Active stream sessions - used to notify clients when extension restarts
    active_sessions: Arc<tokio::sync::RwLock<std::collections::HashSet<String>>>,
    /// ✨ FIX: Session invalidation callback - called when extension restarts
    session_invalidation_tx: Mutex<Option<tokio::sync::mpsc::UnboundedSender<String>>>,
    /// Capability provider for handling InvokeCapability requests from extension
    capability_provider: Arc<std::sync::RwLock<Option<Arc<dyn super::super::context::ExtensionCapabilityProvider>>>>,
}

impl IsolatedExtension {
    /// Create a new isolated extension wrapper
    pub fn new(
        extension_id: impl Into<String>,
        extension_path: impl Into<std::path::PathBuf>,
        config: IsolatedExtensionConfig,
    ) -> Self {
        // ✨ FIX: Pre-allocate IPC buffer pool
        let mut buffer_pool = Vec::with_capacity(IPC_BUFFER_POOL_SIZE);
        for _ in 0..IPC_BUFFER_POOL_SIZE {
            buffer_pool.push(Vec::with_capacity(4096));  // Start with 4KB buffers
        }

        Self {
            extension_id: extension_id.into(),
            extension_path: extension_path.into(),
            process: Mutex::new(None),
            stdin: Arc::new(Mutex::new(None)),
            in_flight: InFlightRequests::new(Duration::from_secs(config.command_timeout_secs)),
            shutdown_tx: Mutex::new(None),
            descriptor: Mutex::new(None),
            config,
            restart_count: AtomicU64::new(0),
            last_restart: Mutex::new(None),
            running: Arc::new(AtomicBool::new(false)),
            process_id: Mutex::new(None),
            active_requests: Arc::new(AtomicUsize::new(0)),
            last_resource_check: Mutex::new(None),
            event_push_tx: Mutex::new(None),
            push_output_tx: Arc::new(std::sync::Mutex::new(None)),
            ipc_buffer_pool: Arc::new(std::sync::Mutex::new(buffer_pool)),
            active_sessions: Arc::new(tokio::sync::RwLock::new(std::collections::HashSet::new())),
            session_invalidation_tx: Mutex::new(None),
            capability_provider: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    /// Set the capability provider for handling InvokeCapability requests
    pub fn set_capability_provider(&self, provider: Arc<dyn super::super::context::ExtensionCapabilityProvider>) {
        *self.capability_provider.write().unwrap() = Some(provider);
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

        debug!(
            runner_path = %runner_path.display(),
            extension_path = %self.extension_path.display(),
            "Spawning extension runner process"
        );

        // Spawn the extension runner process
        // NEOMIND_EXTENSION_DIR should point to the extension root directory
        // (e.g., data/extensions/yolo-device-inference), not the binary directory
        //
        // Support two formats:
        // 1. .nep package format: <extension_root>/binaries/<platform>/extension.<ext>
        //    - Need to go up 3 levels to get extension root
        // 2. Legacy format: <extension_root>/extension.<ext>
        //    - Parent directory IS the extension root (go up 0 levels from parent)
        let extension_dir = if self.extension_path.ends_with("extension.dylib")
            || self.extension_path.ends_with("extension.so")
            || self.extension_path.ends_with("extension.dll")
        {
            // Check if this is .nep format (in binaries/<platform>/ subdirectory)
            // by checking if parent directory name matches platform patterns
            let parent_name = self.extension_path.parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("");

            let is_nep_format = parent_name.starts_with("darwin_")
                || parent_name.starts_with("darwin-")
                || parent_name.starts_with("linux_")
                || parent_name.starts_with("linux-")
                || parent_name.starts_with("windows_")
                || parent_name.starts_with("windows-");

            if is_nep_format {
                // .nep format: go up 3 levels
                let dir = self.extension_path.parent()
                    .and_then(|p| p.parent())
                    .and_then(|p| p.parent())
                    .ok_or_else(|| IsolatedExtensionError::SpawnFailed(
                        "Invalid extension path - expected binaries/<platform>/extension.<ext>".to_string()
                    ))?;
                tracing::info!("Detected .nep format, extension_dir: {}", dir.display());
                dir
            } else {
                // Legacy format: parent directory IS the extension root
                // e.g., /path/to/extensions/yolo-video-v2/extension.dylib -> /path/to/extensions/yolo-video-v2/
                let dir = self.extension_path.parent()
                    .ok_or_else(|| IsolatedExtensionError::SpawnFailed(
                        "Invalid extension path - expected extension root directory".to_string()
                    ))?;
                tracing::info!("Detected legacy format, extension_dir: {}", dir.display());
                dir
            }
        } else {
            // Fallback: try to go up 3 levels (old behavior)
            let dir = self.extension_path.parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
                .ok_or_else(|| IsolatedExtensionError::SpawnFailed(
                    "Invalid extension path - expected binaries/<platform>/extension.<ext>".to_string()
                ))?;
            tracing::info!("Using fallback path, extension_dir: {}", dir.display());
            dir
        };

        // Convert extension path to absolute path for the runner
        // The runner's working directory will be set to extension_dir,
        // but we pass the absolute path to avoid confusion
        let extension_path_absolute = if self.extension_path.is_absolute() {
            self.extension_path.clone()
        } else {
            // Convert relative path to absolute using current working directory
            std::env::current_dir()
                .map(|cwd| cwd.join(&self.extension_path))
                .unwrap_or_else(|_| self.extension_path.clone())
        };

        let mut child = Command::new(&runner_path)
            .arg("--extension-path")
            .arg(&extension_path_absolute)
            .env("NEOMIND_EXTENSION_DIR", extension_dir)
            .current_dir(extension_dir)  // Set working directory to extension root
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

        // Spawn event push task to send events to extension process
        let mut event_push_rx = {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            *self.event_push_tx.lock().await = Some(tx);
            rx
        };

        let extension_id_for_push = self.extension_id.clone();
        let stdin_ref = self.stdin.clone();
        let running_for_push = self.running.clone();

        tokio::spawn(async move {
            while running_for_push.load(Ordering::SeqCst) {
                match event_push_rx.recv().await {
                    Some((event_type, payload)) => {
                        tracing::trace!(
                            extension_id = %extension_id_for_push,
                            event_type = %event_type,
                            "Pushing event to extension process"
                        );

                        let msg = IpcMessage::EventPush {
                            event_type,
                            payload,
                            timestamp: chrono::Utc::now().timestamp_millis(),
                        };

                        // Send message to extension
                        if let Ok(frame) = msg.to_bytes() {
                            let ipc_frame = IpcFrame::new(frame);
                            let bytes = ipc_frame.encode();

                            let mut stdin_guard: tokio::sync::MutexGuard<Option<BufWriter<std::process::ChildStdin>>> = stdin_ref.lock().await;
                            if let Some(stdin) = stdin_guard.as_mut() {
                                let _ = stdin.write_all(&bytes);
                                let _ = stdin.flush();
                            }
                        }
                    }
                    None => {
                        tracing::info!(
                            extension_id = %extension_id_for_push,
                            "Event push channel closed"
                        );
                        break;
                    }
                }
            }
        });

        // Send initialization message
        // Note: We need to register the request BEFORE sending the message to avoid race condition
        // The extension will respond with Ready message using request_id=0

        // Register the request first
        let rx = self.in_flight.register_with_id(0).await;

        // Then send the Init message
        self.send_message(&IpcMessage::Init {
            config: serde_json::json!({}),
        })
        .await?;

        // Wait for ready response with timeout
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
                debug!(extension_id = %self.extension_id, "Extension started successfully");
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
        // ✨ FIX: Pass buffer pool to receiver thread
        let ipc_buffer_pool = self.ipc_buffer_pool.clone();
        // Push output channel for forwarding PushOutput messages
        let push_output_tx = self.push_output_tx.clone();
        // Capability provider for handling InvokeCapability requests
        let capability_provider = self.capability_provider.clone();
        // Stdin for sending responses
        let stdin = self.stdin.clone();

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
                if len > IPC_MAX_BUFFER_SIZE {
                    error!(extension_id = %extension_id, len, "Response too large");
                    running.store(false, Ordering::SeqCst);
                    break;
                }

                // ✨ FIX: Get buffer from pool instead of allocating new one
                let mut payload = {
                    let mut pool = ipc_buffer_pool.lock().unwrap();
                    // Find a buffer that's large enough
                    if let Some(idx) = pool.iter().position(|b| b.capacity() >= len) {
                        let mut buf = pool.remove(idx);
                        buf.clear();
                        buf.resize(len, 0);
                        buf
                    } else {
                        // No suitable buffer, allocate new one (but still use pool later)
                        vec![0u8; len]
                    }
                };

                if let Err(e) = stdout.read_exact(&mut payload) {
                    error!(extension_id = %extension_id, error = %e, "Failed to read response payload");
                    // Return buffer to pool before breaking
                    let mut pool = ipc_buffer_pool.lock().unwrap();
                    if pool.len() < IPC_BUFFER_POOL_SIZE {
                        payload.clear();
                        pool.push(payload);
                    }
                    running.store(false, Ordering::SeqCst);
                    break;
                }

                // Parse response
                let response = match IpcResponse::from_bytes(&payload) {
                    Ok(r) => r,
                    Err(e) => {
                        warn!(extension_id = %extension_id, error = %e, "Failed to parse response");
                        // ✨ FIX: Return buffer to pool even on error
                        let mut pool = ipc_buffer_pool.lock().unwrap();
                        if pool.len() < IPC_BUFFER_POOL_SIZE {
                            let mut buf = payload;
                            buf.clear();
                            pool.push(buf);
                        }
                        continue;
                    }
                };

                // Route response to the correct waiting caller
                // IMPORTANT: Check is_capability_request() FIRST, because CapabilityRequest
                // has a request_id but should NOT be routed to waiting callers
                if response.is_capability_request() {
                    // Handle CapabilityRequest from extension (bidirectional)
                    if let IpcResponse::CapabilityRequest { request_id, capability, params } = response {
                        debug!(
                            extension_id = %extension_id,
                            request_id,
                            capability = %capability,
                            "Received CapabilityRequest from extension"
                        );

                        // Get the capability provider
                        let provider_opt = capability_provider.read().unwrap().clone();

                        if let Some(provider) = provider_opt {
                            // Parse capability
                            let cap = match super::super::context::ExtensionCapability::from_name(&capability) {
                                Some(c) => c,
                                None => {
                                    // Send error response as IpcMessage
                                    let error_message = IpcMessage::CapabilityResult {
                                        request_id,
                                        result: serde_json::json!({}),
                                        error: Some(format!("Unknown capability: {}", capability)),
                                    };
                                    send_message_to_extension(&stdin, &extension_id, error_message);
                                    continue;
                                }
                            };

                            // Invoke capability using block_in_place for async
                            let result = tokio::task::block_in_place(|| {
                                rt_handle.block_on(async {
                                    provider.invoke_capability(cap, &params).await
                                })
                            });

                            // Send response back to extension as IpcMessage
                            let message = match result {
                                Ok(value) => IpcMessage::CapabilityResult {
                                    request_id,
                                    result: value,
                                    error: None,
                                },
                                Err(e) => IpcMessage::CapabilityResult {
                                    request_id,
                                    result: serde_json::json!({}),
                                    error: Some(e.to_string()),
                                },
                            };

                            send_message_to_extension(&stdin, &extension_id, message);
                        } else {
                            // No capability provider configured
                            warn!(
                                extension_id = %extension_id,
                                "No capability provider configured, sending error response"
                            );
                            let error_message = IpcMessage::CapabilityResult {
                                request_id,
                                result: serde_json::json!({}),
                                error: Some("No capability provider configured".to_string()),
                            };
                            send_message_to_extension(&stdin, &extension_id, error_message);
                        }
                    }
                } else if let Some(request_id) = response.request_id() {
                    debug!(
                        extension_id = %extension_id,
                        request_id,
                        "Routing response"
                    );

                    // Use the passed runtime handle to complete async operations
                    rt_handle.block_on(async {
                        in_flight.complete(request_id, response).await;
                    });
                } else if response.is_push_output() {
                    // Handle PushOutput messages (extension-initiated)
                    debug!(
                        extension_id = %extension_id,
                        "Received PushOutput from extension"
                    );

                    // Extract push output data and forward to channel
                    if let Some(push_data) = Option::<super::ipc::PushOutputData>::from(response) {
                        if let Some(tx) = push_output_tx.lock().unwrap().as_ref() {
                            if let Err(e) = tx.send(push_data) {
                                warn!(
                                    extension_id = %extension_id,
                                    error = %e,
                                    "Failed to forward PushOutput"
                                );
                            }
                        }
                    }
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

                // ✨ FIX: Return buffer to pool after processing
                let mut pool = ipc_buffer_pool.lock().unwrap();
                if pool.len() < IPC_BUFFER_POOL_SIZE {
                    let mut buf = payload;
                    buf.clear();
                    pool.push(buf);
                }
                // If pool is full, let the buffer be dropped (GC will reclaim)
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

        debug!(extension_id = %self.extension_id, "Extension stopped");
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


        // Check concurrent request limit (if configured)
        if self.config.max_concurrent_requests > 0 {
            let active = self.active_requests.load(Ordering::SeqCst);
            if active >= self.config.max_concurrent_requests {
                warn!(
                    extension_id = %self.extension_id,
                    active,
                    limit = self.config.max_concurrent_requests,
                    "Concurrent request limit reached"
                );
                return Err(IsolatedExtensionError::TooManyRequests(
                    self.config.max_concurrent_requests
                ));
            }
        }

        // Increment active requests counter
        self.active_requests.fetch_add(1, Ordering::SeqCst);

        // Use scopeguard to ensure counter is decremented on exit
        let _guard = scopeguard::guard(&self.active_requests, |counter| {
            counter.fetch_sub(1, Ordering::SeqCst);
        });

        // Register the request and get a receiver
        let (request_id, rx) = self.in_flight.register().await;

        debug!(
            extension_id = %self.extension_id,
            request_id,
            command,
            active_requests = self.active_requests.load(Ordering::SeqCst) - 1,  // -1 because we just incremented
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

    /// Get extension statistics via IPC
    pub async fn get_stats(&self) -> IsolatedResult<super::super::system::ExtensionStats> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(IsolatedExtensionError::NotRunning);
        }

        let (request_id, rx) = self.in_flight.register().await;

        self.send_message(&IpcMessage::GetStats { request_id })
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
            IpcResponse::Stats { start_count, stop_count, error_count, last_error, .. } => {
                Ok(super::super::system::ExtensionStats {
                    start_count,
                    stop_count,
                    error_count,
                    last_error,
                    ..Default::default()
                })
            }
            IpcResponse::Error { error, .. } => Err(IsolatedExtensionError::IpcError(error)),
            _ => Err(IsolatedExtensionError::InvalidResponse(
                "Expected Stats response".to_string(),
            )),
        }
    }

    /// Execute multiple commands in a batch
    pub async fn execute_batch(&self, commands: Vec<super::ipc_batch_types::BatchCommand>)
        -> IsolatedResult<super::ipc_batch_types::BatchResultsVec>
    {
        if !self.running.load(Ordering::SeqCst) {
            return Err(IsolatedExtensionError::NotRunning);
        }

        let start = Instant::now();
        let mut results = Vec::with_capacity(commands.len());

        // Execute each command and collect results
        for cmd in commands {
            let cmd_start = Instant::now();
            let result = match self.execute_command(&cmd.command, &cmd.args).await {
                Ok(data) => super::ipc_batch_types::BatchResult {
                    command: cmd.command.clone(),
                    success: true,
                    data: Some(data),
                    error: None,
                    elapsed_ms: cmd_start.elapsed().as_secs_f64() * 1000.0,
                },
                Err(e) => super::ipc_batch_types::BatchResult {
                    command: cmd.command.clone(),
                    success: false,
                    data: None,
                    error: Some(e.to_string()),
                    elapsed_ms: cmd_start.elapsed().as_secs_f64() * 1000.0,
                },
            };
            results.push(result);
        }

        let total_elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

        Ok(super::ipc_batch_types::BatchResultsVec {
            results,
            total_elapsed_ms,
        })
    }

    /// Send batch request to extension process
    pub async fn execute_batch_ipc(&self, commands: Vec<super::ipc_batch_types::BatchCommand>)
        -> IsolatedResult<super::ipc_batch_types::BatchResultsVec>
    {
        if !self.running.load(Ordering::SeqCst) {
            return Err(IsolatedExtensionError::NotRunning);
        }

        // Start timing for performance metrics
        let _start = Instant::now();
        if self.config.max_concurrent_requests > 0 {
            let active = self.active_requests.load(Ordering::SeqCst);
            if active >= self.config.max_concurrent_requests {
                warn!(
                    extension_id = %self.extension_id,
                    active,
                    limit = self.config.max_concurrent_requests,
                    "Concurrent request limit reached"
                );
                return Err(IsolatedExtensionError::TooManyRequests(
                    self.config.max_concurrent_requests
                ));
            }
        }

        // Increment active requests counter
        self.active_requests.fetch_add(1, Ordering::SeqCst);

        // Use scopeguard to ensure counter is decremented on exit
        let _guard = scopeguard::guard(&self.active_requests, |counter| {
            counter.fetch_sub(1, Ordering::SeqCst);
        });

        // Register the request and get a receiver
        let (request_id, rx) = self.in_flight.register().await;

        let _start = Instant::now();

        debug!(
            extension_id = %self.extension_id,
            request_id,
            command_count = commands.len(),
            "Sending execute batch request"
        );

        // Send batch request
        self.send_message(&IpcMessage::ExecuteBatch {
            commands: commands.clone(),
            request_id,
        })
        .await?;

        // Wait for response with timeout
        let response = self
            .in_flight
            .wait_with_timeout(
                request_id,
                rx,
                Duration::from_secs(self.config.command_timeout_secs * commands.len() as u64), // Scale timeout
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
            IpcResponse::BatchResults { results, total_elapsed_ms, .. } => {
                Ok(super::ipc_batch_types::BatchResultsVec {
                    results,
                    total_elapsed_ms,
                })
            }
            IpcResponse::Error { error, .. } => {
                Err(IsolatedExtensionError::IpcError(error))
            }
            _ => Err(IsolatedExtensionError::InvalidResponse(
                format!("Expected BatchResults response, got {:?}", std::mem::discriminant(&response))
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

        // Register session as active
        self.register_session(session_id).await;

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
        // Unregister session
        self.unregister_session(session_id).await;

        if !self.running.load(Ordering::SeqCst) {
            return Err(IsolatedExtensionError::NotRunning);
        }

        self.send_message(&IpcMessage::CloseStreamSession {
            session_id: session_id.to_string(),
        }).await?;

        Ok(super::super::stream::SessionStats::default())
    }

    /// ✨ FIX: Close all active stream sessions before restart
    /// This ensures the extension cleans up frame queues and session state
    /// Also notifies clients that their sessions are invalid
    pub async fn close_all_stream_sessions(&self) -> IsolatedResult<()> {
        // Get all active sessions
        let sessions: Vec<String> = {
            let sessions = self.active_sessions.read().await;
            sessions.iter().cloned().collect()
        };

        if sessions.is_empty() {
            tracing::debug!(
                extension_id = %self.extension_id,
                "No active sessions to close"
            );
            return Ok(());
        }

        tracing::warn!(
            extension_id = %self.extension_id,
            session_count = sessions.len(),
            "Invalidating active sessions due to extension restart"
        );

        // Notify clients that their sessions are invalid
        if let Some(tx) = self.session_invalidation_tx.lock().await.as_ref() {
            for session_id in &sessions {
                if let Err(e) = tx.send(session_id.clone()) {
                    tracing::error!(
                        extension_id = %self.extension_id,
                        session_id = %session_id,
                        error = %e,
                        "Failed to notify session invalidation"
                    );
                }
            }
        }

        // Clear active sessions
        {
            let mut active = self.active_sessions.write().await;
            active.clear();
        }

        // Try to send close messages to extension process
        if self.running.load(Ordering::SeqCst) {
            for session_id in &sessions {
                let _ = self.send_message(&IpcMessage::CloseStreamSession {
                    session_id: session_id.clone(),
                }).await;
            }
        }

        Ok(())
    }

    /// ✨ FIX: Register a session as active
    pub async fn register_session(&self, session_id: &str) {
        let mut sessions = self.active_sessions.write().await;
        sessions.insert(session_id.to_string());
        tracing::debug!(
            extension_id = %self.extension_id,
            session_id = %session_id,
            active_sessions = sessions.len(),
            "Session registered"
        );
    }

    /// ✨ FIX: Unregister a session
    pub async fn unregister_session(&self, session_id: &str) {
        let mut sessions = self.active_sessions.write().await;
        let removed = sessions.remove(session_id);
        if removed {
            tracing::debug!(
                extension_id = %self.extension_id,
                session_id = %session_id,
                active_sessions = sessions.len(),
                "Session unregistered"
            );
        }
    }

    /// ✨ FIX: Set session invalidation callback
    pub async fn set_session_invalidation_callback(
        &self,
        tx: tokio::sync::mpsc::UnboundedSender<String>,
    ) {
        *self.session_invalidation_tx.lock().await = Some(tx);
    }

    // =========================================================================
    // Stateless Mode Support
    // =========================================================================

    /// Process a single data chunk (stateless mode) via IPC
    ///
    /// Used for one-shot processing where each request is independent.
    /// Examples: image analysis, single inference, data transformation.
    pub async fn process_chunk(
        &self,
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

        debug!(
            extension_id = %self.extension_id,
            request_id,
            sequence = chunk.sequence,
            "Sending ProcessChunk request"
        );

        // Send the request
        self.send_message(&IpcMessage::ProcessChunk {
            request_id,
            chunk: stream_chunk,
        }).await?;

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
            IpcResponse::ChunkResult {
                request_id: _,
                input_sequence,
                output_sequence,
                data,
                data_type,
                processing_ms,
                metadata,
            } => {
                let mut result = super::super::stream::StreamResult::success(
                    Some(input_sequence),
                    output_sequence,
                    data,
                    super::super::stream::StreamDataType::from_mime_type(&data_type)
                        .unwrap_or(super::super::stream::StreamDataType::Binary),
                    processing_ms,
                );
                if let Some(meta) = metadata {
                    result = result.with_metadata(meta);
                }
                Ok(result)
            }
            IpcResponse::Error { error, .. } => {
                Err(IsolatedExtensionError::ExecutionFailed(error))
            }
            _ => {
                Err(IsolatedExtensionError::InvalidResponse(
                    "Expected ChunkResult response".to_string(),
                ))
            }
        }
    }

    // =========================================================================
    // Push Mode Support
    // =========================================================================

    /// Start pushing data for a session (Push mode) via IPC
    ///
    /// Called after init_session for Push mode extensions.
    /// The extension should start its data production loop and push
    /// outputs via PushOutput messages.
    pub async fn start_push(&self, session_id: &str) -> IsolatedResult<()> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(IsolatedExtensionError::NotRunning);
        }

        // Register request and get receiver for response
        let (request_id, rx) = self.in_flight.register().await;

        debug!(
            extension_id = %self.extension_id,
            request_id,
            session_id = %session_id,
            "Sending StartPush request"
        );

        // Send the request
        self.send_message(&IpcMessage::StartPush {
            request_id,
            session_id: session_id.to_string(),
        }).await?;

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
            IpcResponse::PushStarted { success, error, .. } => {
                if success {
                    debug!(
                        extension_id = %self.extension_id,
                        session_id = %session_id,
                        "Push mode started successfully"
                    );
                    Ok(())
                } else {
                    Err(IsolatedExtensionError::ExecutionFailed(
                        error.unwrap_or_else(|| "Unknown error".to_string())
                    ))
                }
            }
            IpcResponse::Error { error, .. } => {
                Err(IsolatedExtensionError::ExecutionFailed(error))
            }
            _ => {
                Err(IsolatedExtensionError::InvalidResponse(
                    "Expected PushStarted response".to_string(),
                ))
            }
        }
    }

    /// Stop pushing data for a session (Push mode) via IPC
    ///
    /// Called when the client disconnects or session is closed.
    /// The extension should stop its data production loop.
    pub async fn stop_push(&self, session_id: &str) -> IsolatedResult<()> {
        if !self.running.load(Ordering::SeqCst) {
            // Extension not running, nothing to stop
            return Ok(());
        }

        // Register request and get receiver for response
        let (request_id, rx) = self.in_flight.register().await;

        debug!(
            extension_id = %self.extension_id,
            request_id,
            session_id = %session_id,
            "Sending StopPush request"
        );

        // Send the request
        self.send_message(&IpcMessage::StopPush {
            request_id,
            session_id: session_id.to_string(),
        }).await?;

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
            IpcResponse::PushStopped { success, .. } => {
                debug!(
                    extension_id = %self.extension_id,
                    session_id = %session_id,
                    success,
                    "Push mode stopped"
                );
                Ok(())
            }
            IpcResponse::Error { error, .. } => {
                // Log but don't fail - extension might have already stopped
                warn!(
                    extension_id = %self.extension_id,
                    session_id = %session_id,
                    error = %error,
                    "Error stopping push mode (may already be stopped)"
                );
                Ok(())
            }
            _ => {
                Err(IsolatedExtensionError::InvalidResponse(
                    "Expected PushStopped response".to_string(),
                ))
            }
        }
    }

    /// Set the push output channel for receiving PushOutput messages from extension
    ///
    /// This channel is used to forward push data from the extension to WebSocket clients.
    /// The host should set this before starting a Push mode session.
    pub async fn set_push_output_channel(
        &self,
        tx: tokio::sync::mpsc::UnboundedSender<super::ipc::PushOutputData>,
    ) {
        *self.push_output_tx.lock().unwrap() = Some(tx);
    }

    /// Get a clone of the push output channel sender
    pub async fn get_push_output_channel(
        &self,
    ) -> Option<tokio::sync::mpsc::UnboundedSender<super::ipc::PushOutputData>> {
        self.push_output_tx.lock().unwrap().clone()
    }

    /// ✨ FIX: Trigger memory cleanup in extension
    /// Sends a command to extension to clean up cached frames and detection history
    pub async fn trigger_memory_cleanup(&self) -> IsolatedResult<()> {
        if !self.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        tracing::info!(
            extension_id = %self.extension_id,
            "Triggering memory cleanup in extension"
        );

        // Send custom command to extension to trigger GC
        // Extension should implement "gc_memory" command
        let _ = self.execute_command("gc_memory", &serde_json::json!({})).await;

        Ok(())
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

    /// Get performance metrics

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

    /// Push an event to the extension
    pub async fn push_event(&self, event_type: String, payload: serde_json::Value) -> IsolatedResult<()> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(IsolatedExtensionError::NotRunning);
        }

        self.send_message(&IpcMessage::EventPush {
            event_type,
            payload,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }).await
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

        // ✨ FIX: Cancel all in-flight requests immediately on kill
        // This prevents requests from waiting until timeout (30s)
        let cancelled_count = self.in_flight.cancel_all().await;
        if cancelled_count > 0 {
            tracing::warn!(
                extension_id = %self.extension_id,
                cancelled_count,
                "Cancelled in-flight requests on process kill"
            );
        }
    }

    /// Check resource usage and restart if necessary
    pub async fn check_resources(&self) -> IsolatedResult<()> {
        // ✨ FIX: Check every 2 seconds (reduced from 5) for faster memory limit detection
        {
            let mut last_check = self.last_resource_check.lock().await;
            if let Some(last) = *last_check {
                if last.elapsed() < Duration::from_secs(2) {
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
                        self.kill_internal(&mut process_guard).await;
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

                // ✨ FIX: Dynamic memory management with multiple thresholds
                if memory_mb > self.config.max_memory_mb as u64 {
                    // CRITICAL: Memory critically high, force restart
                    error!(
                        extension_id = %self.extension_id,
                        memory_mb,
                        max_memory_mb = self.config.max_memory_mb,
                        "Extension exceeded CRITICAL memory limit, forcing restart"
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
                } else if memory_mb > (self.config.max_memory_mb as u64 * 80 / 100) {
                    // WARNING: Memory at 80%, trigger GC
                    warn!(
                        extension_id = %self.extension_id,
                        memory_mb,
                        threshold_mb = self.config.max_memory_mb * 80 / 100,
                        "Extension memory at 80%, triggering GC"
                    );

                    // Send GC trigger to extension
                    let _ = self.trigger_memory_cleanup().await;
                }
            }
        }

        #[cfg(target_os = "macos")]
        {
            if let Ok(memory_bytes) = self.get_process_memory_macos(pid) {
                let memory_mb = memory_bytes / (1024 * 1024);

                // ✨ FIX: Dynamic memory management with multiple thresholds
                if memory_mb > self.config.max_memory_mb as u64 {
                    // CRITICAL: Memory critically high, force restart
                    error!(
                        extension_id = %self.extension_id,
                        memory_mb,
                        max_memory_mb = self.config.max_memory_mb,
                        "Extension exceeded CRITICAL memory limit, forcing restart"
                    );

                    let mut process_guard = self.process.lock().await;
                    self.kill_internal(&mut process_guard).await;
                    drop(process_guard);

                    // Attempt restart
                    if self.config.restart_on_crash {
                        return self.start().await;
                    }
                    return Err(IsolatedExtensionError::Crashed(
                        format!("Memory limit exceeded: {}MB > {}MB", memory_mb, self.config.max_memory_mb)
                    ));
                } else if memory_mb > (self.config.max_memory_mb as u64 * 80 / 100) {
                    // WARNING: Memory at 80%, trigger GC
                    warn!(
                        extension_id = %self.extension_id,
                        memory_mb,
                        threshold_mb = self.config.max_memory_mb * 80 / 100,
                        "Extension memory at 80%, triggering GC"
                    );

                    // Send GC trigger to extension
                    let _ = self.trigger_memory_cleanup().await;
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
            .args(["-o", "rss=", "-p", &pid.to_string()])
            .output()?;

        if output.status.success() {
            let rss_str = String::from_utf8_lossy(&output.stdout);
            if let Ok(rss_kb) = rss_str.trim().parse::<u64>() {
                return Ok(rss_kb * 1024); // Convert KB to bytes
            }
        }

        Err(std::io::Error::new(std::io::ErrorKind::NotFound, "Failed to get memory"))
    }

    /// Get the event push channel for this extension
    ///
    /// This method is called by the EventDispatcher to get the channel
    /// for pushing events to the isolated extension process.
    pub async fn get_event_push_channel(&self) -> Option<tokio::sync::mpsc::UnboundedSender<(String, Value)>> {
        self.event_push_tx.lock().await.clone()
    }

    /// Get event subscriptions from the extension
    ///
    /// This method queries the extension process for its event subscriptions
    /// via the GetEventSubscriptions IPC message.
    pub async fn get_event_subscriptions(&self) -> IsolatedResult<Vec<String>> {
        // Register a new request
        let (request_id, rx) = self.in_flight.register().await;

        // Send GetEventSubscriptions message
        self.send_message(&IpcMessage::GetEventSubscriptions { request_id })
            .await?;

        // Wait for response with timeout
        let timeout_duration = Duration::from_secs(5);
        match tokio::time::timeout(timeout_duration, rx).await {
            Ok(Ok(response)) => {
                match response {
                    IpcResponse::EventSubscriptions { event_types, .. } => Ok(event_types),
                    IpcResponse::Error { error, .. } => Err(IsolatedExtensionError::SpawnFailed(error)),
                    _ => Err(IsolatedExtensionError::SpawnFailed(
                        "Unexpected response type".to_string()
                    )),
                }
            }
            Ok(Err(_)) => Err(IsolatedExtensionError::SpawnFailed(
                "Response channel closed".to_string()
            )),
            Err(_) => Err(IsolatedExtensionError::SpawnFailed(
                "Request timeout".to_string()
            )),
        }
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
        assert_eq!(config.max_memory_mb, 2048);  // Updated for YOLO extensions
        assert!(config.restart_on_crash);
    }
}
