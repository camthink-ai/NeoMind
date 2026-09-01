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

use std::collections::VecDeque;
use std::io::{BufReader, BufWriter, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use tokio::sync::broadcast;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use super::in_flight::InFlightRequests;
use super::{ErrorKind, IpcFrame, IpcMessage, IpcResponse, IsolatedExtensionError, IsolatedResult};
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

// ✨ FIX: IPC 缓冲区池配置
const IPC_MAX_BUFFER_SIZE: usize = 10 * 1024 * 1024; // 10MB 最大缓冲区

// Tiered buffer pool configuration
const TIERED_POOL_SMALL_SIZE: usize = 4 * 1024; // 4KB - 控制消息
const TIERED_POOL_MEDIUM_SIZE: usize = 64 * 1024; // 64KB - 小数据
const TIERED_POOL_LARGE_SIZE: usize = 1024 * 1024; // 1MB - 大数据/视频帧
const TIERED_POOL_SMALL_COUNT: usize = 16;
const TIERED_POOL_MEDIUM_COUNT: usize = 8;
const TIERED_POOL_LARGE_COUNT: usize = 4;
const TIERED_POOL_MAX_SIZE: usize = 16; // Maximum buffers per tier

/// Tiered IPC buffer pool for optimal memory management
/// Uses different buffer sizes based on payload requirements
#[derive(Debug)]
struct TieredBufferPool {
    small: Arc<std::sync::Mutex<Vec<Vec<u8>>>>, // 4KB - 控制消息
    medium: Arc<std::sync::Mutex<Vec<Vec<u8>>>>, // 64KB - 小数据
    large: Arc<std::sync::Mutex<Vec<Vec<u8>>>>, // 1MB - 大数据/视频帧
}

impl TieredBufferPool {
    /// Create a new tiered buffer pool
    fn new() -> Self {
        Self {
            small: Arc::new(std::sync::Mutex::new(Self::init_pool(
                TIERED_POOL_SMALL_SIZE,
                TIERED_POOL_SMALL_COUNT,
            ))),
            medium: Arc::new(std::sync::Mutex::new(Self::init_pool(
                TIERED_POOL_MEDIUM_SIZE,
                TIERED_POOL_MEDIUM_COUNT,
            ))),
            large: Arc::new(std::sync::Mutex::new(Self::init_pool(
                TIERED_POOL_LARGE_SIZE,
                TIERED_POOL_LARGE_COUNT,
            ))),
        }
    }

    /// Initialize a buffer pool with pre-allocated buffers
    fn init_pool(capacity: usize, count: usize) -> Vec<Vec<u8>> {
        (0..count).map(|_| Vec::with_capacity(capacity)).collect()
    }

    /// Acquire a buffer suitable for the given size
    /// Returns a ReusableBuffer that will automatically return to the pool when dropped
    fn acquire(&self, size: usize) -> ReusableBuffer {
        let (pool, capacity) = if size < TIERED_POOL_SMALL_SIZE {
            (&self.small, TIERED_POOL_SMALL_SIZE)
        } else if size < TIERED_POOL_MEDIUM_SIZE {
            (&self.medium, TIERED_POOL_MEDIUM_SIZE)
        } else {
            (&self.large, TIERED_POOL_LARGE_SIZE)
        };

        let mut buffers = pool.lock().unwrap_or_else(|e| {
            tracing::error!(error = %e, "Buffer pool mutex poisoned, recovering");
            e.into_inner()
        });
        let data = buffers
            .pop()
            .unwrap_or_else(|| Vec::with_capacity(capacity));

        ReusableBuffer {
            data,
            pool: pool.clone(),
            max_size: TIERED_POOL_MAX_SIZE,
        }
    }
}

/// RAII wrapper for reusable buffers
/// Automatically returns the buffer to the pool when dropped
#[derive(Debug)]
struct ReusableBuffer {
    data: Vec<u8>,
    pool: Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    max_size: usize,
}

impl ReusableBuffer {
    /// Get mutable reference to the underlying data
    fn as_mut(&mut self) -> &mut Vec<u8> {
        &mut self.data
    }

    /// Get reference to the underlying data
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}

impl Drop for ReusableBuffer {
    fn drop(&mut self) {
        if !self.data.is_empty() {
            // Clear and return to pool
            self.data.clear();
            // In Drop, we can't do much if the lock is poisoned - just skip returning to pool
            if let Ok(mut buffers) = self.pool.lock() {
                if buffers.len() < self.max_size {
                    buffers.push(std::mem::take(&mut self.data));
                }
            }
            // If lock is poisoned, we lose this buffer but don't panic
        }
    }
}

/// Configuration for isolated extension
#[derive(Debug, Clone)]
pub struct IsolatedExtensionConfig {
    /// Maximum startup time in seconds
    pub startup_timeout_secs: u64,
    /// Command execution timeout in seconds
    pub command_timeout_secs: u64,
    /// Maximum memory usage in MB (0 = unlimited) — enforced by the RSS
    /// polling check (accurate for resident memory, incl. GPU-adjacent libs).
    pub max_memory_mb: usize,
    /// HARD address-space rlimit for the runner process, in MB (None =
    /// unlimited). OFF by default on purpose: RLIMIT_AS caps *virtual*
    /// address space, and CUDA/ONNX runtimes reserve multi-GB VA regions at
    /// init, so a naive cap kills exactly the heavy extensions it looks like
    /// it should protect. Set this only for extensions with known-bounded
    /// allocators (pure-Rust CLI-style extensions).
    pub rlimit_memory_mb: Option<u64>,
    /// Periodic liveness probe interval in seconds (0 = disabled). A hung
    /// extension (deadlock without exit) never closes stdout, so the death
    /// monitor can't see it; the probe Pings the runner and kills it after
    /// repeated failures, converting hangs into crashes the existing
    /// restart machinery already handles.
    pub health_check_interval_secs: u64,
    /// Per-probe timeout in seconds.
    pub health_check_timeout_secs: u64,
    /// Consecutive probe failures before the process is killed.
    pub health_check_max_failures: u32,
    /// Restart on crash
    pub restart_on_crash: bool,
    /// Maximum restart attempts
    pub max_restart_attempts: u32,
    /// Restart cooldown in seconds
    pub restart_cooldown_secs: u64,
    /// Maximum concurrent requests (0 = unlimited)
    pub max_concurrent_requests: usize,
    /// 🔧 Phase 2: IPC read timeout in seconds
    pub ipc_read_timeout_secs: u64,
    /// 🔧 Phase 2: Maximum IPC retry attempts
    pub ipc_max_retries: usize,
    /// 🔧 Phase 2: IPC retry base delay in milliseconds
    pub ipc_retry_delay_ms: u64,
}

impl Default for IsolatedExtensionConfig {
    fn default() -> Self {
        Self {
            startup_timeout_secs: 120,
            command_timeout_secs: 300,
            // ✨ FIX: Memory limit increased to 2048MB for YOLO extensions
            // Breakdown:
            // - ONNX model: ~100MB
            // - ONNX Runtime memory pool: ~800MB (per-frame inference + caching)
            // - Frame buffers: ~100MB (multiple frames in pipeline)
            // - Detection history: ~20MB
            // - IPC buffers: ~10MB
            // - System overhead: ~100MB
            // - Headroom: ~918MB
            max_memory_mb: 2048, // Increased from 1024MB for YOLO stability
            rlimit_memory_mb: None,
            health_check_interval_secs: 30,
            health_check_timeout_secs: 5,
            health_check_max_failures: 2,
            restart_on_crash: true,
            max_restart_attempts: 3,
            restart_cooldown_secs: 5,
            max_concurrent_requests: 100,
            // 🔧 Phase 2: IPC robustness settings
            ipc_read_timeout_secs: 10, // 10 second timeout for IPC reads
            ipc_max_retries: 2,        // Retry failed IPC calls twice
            ipc_retry_delay_ms: 100,   // Start with 100ms delay, exponential backoff
        }
    }
}

/// 🔧 Phase 1: Detailed crash event information
#[derive(Debug, Clone)]
pub enum CrashEvent {
    UnexpectedExit {
        exit_code: Option<i32>,
        signal: Option<i32>,
    },
    IpcFailure {
        reason: String,
        stage: IpcFailureStage,
    },
}

/// 🔧 Phase 1: IPC failure stage categorization
#[derive(Debug, Clone, Copy)]
pub enum IpcFailureStage {
    ReadLength,
}

impl CrashEvent {
    pub fn description(&self) -> String {
        match self {
            CrashEvent::UnexpectedExit { exit_code, signal } => match (exit_code, signal) {
                (Some(code), None) => format!("Process exited with code {}", code),
                (None, Some(sig)) => format!("Process terminated by signal {}", sig),
                _ => "Process exited unexpectedly".to_string(),
            },
            CrashEvent::IpcFailure { reason, stage } => {
                format!("IPC failure during {:?}: {}", stage, reason)
            }
        }
    }
}

/// Maximum number of log lines retained per extension.
const MAX_LOG_LINES: usize = 2000;

/// A single log line captured from extension stderr.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExtensionLogEntry {
    /// Timestamp when the log was captured (milliseconds since epoch)
    pub timestamp: i64,
    /// Log level inferred from the line content (trace/debug/info/warn/error)
    pub level: String,
    /// The raw log message
    pub message: String,
}

/// Thread-safe ring buffer for extension logs.
struct LogBuffer {
    lines: std::sync::Mutex<VecDeque<ExtensionLogEntry>>,
}

impl LogBuffer {
    fn new() -> Self {
        Self {
            lines: std::sync::Mutex::new(VecDeque::with_capacity(MAX_LOG_LINES)),
        }
    }

    fn push(&self, entry: ExtensionLogEntry) {
        let mut buf = self.lines.lock().unwrap_or_else(|e| e.into_inner());
        if buf.len() >= MAX_LOG_LINES {
            buf.pop_front();
        }
        buf.push_back(entry);
    }

    fn snapshot(&self) -> Vec<ExtensionLogEntry> {
        let buf = self.lines.lock().unwrap_or_else(|e| e.into_inner());
        buf.iter().cloned().collect()
    }

    fn clear(&self) {
        let mut buf = self.lines.lock().unwrap_or_else(|e| e.into_inner());
        buf.clear();
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
    /// 🔧 Phase 2: Process start time for health monitoring
    start_time: Mutex<Option<SystemTime>>,
    /// Running state (shared with background receiver thread)
    running: Arc<AtomicBool>,
    /// Process ID for resource monitoring
    process_id: Mutex<Option<u32>>,
    /// Active request counter for concurrency limiting
    active_requests: Arc<AtomicUsize>,
    /// Last resource check time
    last_resource_check: Mutex<Option<Instant>>,
    /// Event push channel for sending events to extension process
    event_push_tx: Mutex<Option<tokio::sync::mpsc::Sender<(String, Value)>>>,
    /// Push output channel for receiving PushOutput messages from extension
    /// Uses std::sync::Mutex for thread safety in receiver thread
    /// Bounded (256) to provide backpressure when WebSocket clients are slow
    push_output_tx: Arc<std::sync::Mutex<Option<tokio::sync::mpsc::Sender<super::PushOutputData>>>>,
    /// ✨ FIX: Tiered IPC buffer pool for optimal memory management
    ipc_buffer_pool: Arc<TieredBufferPool>,
    /// ✨ FIX: Active stream sessions - used to notify clients when extension restarts
    active_sessions: Arc<tokio::sync::RwLock<std::collections::HashSet<String>>>,
    /// ✨ FIX: Session invalidation callback - called when extension restarts
    session_invalidation_tx: Mutex<Option<tokio::sync::mpsc::UnboundedSender<String>>>,
    /// Extension death notification channel
    death_tx: Arc<Mutex<Option<broadcast::Sender<()>>>>,
    /// Capability provider for handling InvokeCapability requests from extension
    capability_provider:
        Arc<std::sync::RwLock<Option<Arc<dyn super::super::context::ExtensionCapabilityProvider>>>>,
    /// Crash loop detection: consecutive crash count
    consecutive_crashes: AtomicU32,
    /// Human-readable reason for the most recent crash (exited/signal/IPC
    /// failure) — surfaced through the API so "Stopped" can become "Crashed".
    last_crash_reason: tokio::sync::Mutex<Option<String>>,
    /// Crash loop detection: timestamp of last crash
    last_crash_time: Mutex<Option<Instant>>,
    /// Ring buffer capturing stderr output as structured log entries
    log_buffer: Arc<LogBuffer>,
}

impl IsolatedExtension {
    /// Create a new isolated extension wrapper
    pub fn new(
        extension_id: impl Into<String>,
        extension_path: impl Into<std::path::PathBuf>,
        config: IsolatedExtensionConfig,
    ) -> Self {
        // ✨ FIX: Use tiered IPC buffer pool for optimal performance
        let ipc_buffer_pool = Arc::new(TieredBufferPool::new());

        Self {
            extension_id: extension_id.into(),
            extension_path: extension_path.into(),
            process: Mutex::new(None),
            stdin: Arc::new(Mutex::new(None)),
            in_flight: InFlightRequests::new(Duration::from_secs(config.command_timeout_secs)),
            shutdown_tx: Mutex::new(None),
            descriptor: Mutex::new(None),
            config,
            running: Arc::new(AtomicBool::new(false)),
            process_id: Mutex::new(None),
            active_requests: Arc::new(AtomicUsize::new(0)),
            last_resource_check: Mutex::new(None),
            event_push_tx: Mutex::new(None),
            push_output_tx: Arc::new(std::sync::Mutex::new(None)),
            ipc_buffer_pool,
            active_sessions: Arc::new(tokio::sync::RwLock::new(std::collections::HashSet::new())),
            session_invalidation_tx: Mutex::new(None),
            death_tx: Arc::new(Mutex::new(None)),
            capability_provider: Arc::new(std::sync::RwLock::new(None)),
            start_time: Mutex::new(None),
            // Crash loop detection
            consecutive_crashes: AtomicU32::new(0),
            last_crash_reason: tokio::sync::Mutex::new(None),
            last_crash_time: Mutex::new(None),
            log_buffer: Arc::new(LogBuffer::new()),
        }
    }

    /// Set the capability provider for handling InvokeCapability requests
    pub fn set_capability_provider(
        &self,
        provider: Arc<dyn super::super::context::ExtensionCapabilityProvider>,
    ) {
        let mut cap_provider = self.capability_provider.write().unwrap_or_else(|e| {
            tracing::error!(error = %e, "CapabilityProvider mutex poisoned, recovering");
            e.into_inner()
        });
        *cap_provider = Some(provider);
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
            let parent_name = self
                .extension_path
                .parent()
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
                let dir = self
                    .extension_path
                    .parent()
                    .and_then(|p| p.parent())
                    .and_then(|p| p.parent())
                    .ok_or_else(|| {
                        IsolatedExtensionError::SpawnFailed(
                            "Invalid extension path - expected binaries/<platform>/extension.<ext>"
                                .to_string(),
                        )
                    })?;
                tracing::info!("Detected .nep format, extension_dir: {}", dir.display());
                dir
            } else {
                // Legacy format: parent directory IS the extension root
                // e.g., /path/to/extensions/yolo-video-v2/extension.dylib -> /path/to/extensions/yolo-video-v2/
                let dir = self.extension_path.parent().ok_or_else(|| {
                    IsolatedExtensionError::SpawnFailed(
                        "Invalid extension path - expected extension root directory".to_string(),
                    )
                })?;
                tracing::info!("Detected legacy format, extension_dir: {}", dir.display());
                dir
            }
        } else {
            // Fallback: try to go up 3 levels (old behavior)
            let dir = self
                .extension_path
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
                .ok_or_else(|| {
                    IsolatedExtensionError::SpawnFailed(
                        "Invalid extension path - expected binaries/<platform>/extension.<ext>"
                            .to_string(),
                    )
                })?;
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

        // ✅ FIX: Also convert extension_dir to absolute path
        // This ensures NEOMIND_EXTENSION_DIR environment variable is always absolute
        // Convert relative path to absolute using current working directory
        let extension_dir_absolute = if extension_dir.is_absolute() {
            extension_dir.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(extension_dir))
                .unwrap_or_else(|_| extension_dir.to_path_buf())
        };
        let mut cmd = Command::new(&runner_path);
        // Hard rlimit only when explicitly configured — see the field docs on
        // IsolatedExtensionConfig::rlimit_memory_mb for why this is not fed
        // from max_memory_mb (the runner's flags existed but were never wired,
        // which made "resource limits" a no-op on the spawn path).
        if let Some(mb) = self.config.rlimit_memory_mb {
            cmd.arg("--memory-limit").arg(mb.to_string());
        }
        // Extension-private data dir: platform-guaranteed to survive
        // upgrades AND uninstall. Created here so extensions can rely on
        // it existing without any bootstrap of their own.
        let extension_data_dir = extension_dir_absolute.join("data");
        if let Err(e) = std::fs::create_dir_all(&extension_data_dir) {
            tracing::warn!(
                extension_id = %self.extension_id,
                error = %e,
                "Could not create extension data dir"
            );
        }
        cmd.arg("--extension-path")
            .arg(&extension_path_absolute)
            .env("NEOMIND_EXTENSION_DIR", &extension_dir_absolute)
            .env("NEOMIND_EXTENSION_DATA_DIR", &extension_data_dir)
            .current_dir(&extension_dir_absolute) // Set working directory to extension root
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Add the extension's binary directory to the platform-specific library search path
        // so bundled shared libraries (onnxruntime, ffmpeg, etc.) are found at runtime.
        if let Some(binary_dir) = extension_path_absolute.parent() {
            let bin_dir = binary_dir.to_string_lossy().to_string();

            #[cfg(target_os = "windows")]
            {
                let current_path = std::env::var("PATH").unwrap_or_default();
                cmd.env("PATH", format!("{};{}", bin_dir, current_path));
            }

            #[cfg(target_os = "linux")]
            {
                let current = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
                cmd.env(
                    "LD_LIBRARY_PATH",
                    if current.is_empty() {
                        bin_dir.clone()
                    } else {
                        format!("{}:{}", bin_dir, current)
                    },
                );
            }

            #[cfg(target_os = "macos")]
            {
                let current = std::env::var("DYLD_LIBRARY_PATH").unwrap_or_default();
                cmd.env(
                    "DYLD_LIBRARY_PATH",
                    if current.is_empty() {
                        bin_dir.clone()
                    } else {
                        format!("{}:{}", bin_dir, current)
                    },
                );
            }

            // Manifest-declared env hints (generic, opt-in). Extensions that
            // need runtime-specific env vars (ORT_DYLIB_PATH for
            // load-dynamic ONNX Runtime, RKNN/TensorRT lib paths, …)
            // declare them in their .nep manifest instead of the platform
            // special-casing each runtime:
            //
            //   "env_hints": { "ORT_DYLIB_PATH": "{binaries}/libonnxruntime.dylib" }
            //
            // Mutating DYLD_* at runtime inside the child is unreliable on
            // macOS (SIP), which is why exact-file env vars belong here at
            // spawn. A hint only applies when the variable is not already
            // set and the resolved file exists — no-ops otherwise, so
            // extensions with other (or no) acceleration runtimes are
            // untouched.
            apply_manifest_env_hints(&mut cmd, &extension_dir_absolute, binary_dir);

            tracing::debug!(
                extension_id = %self.extension_id,
                binary_dir = %bin_dir,
                "Added extension binary directory to platform library search path"
            );
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| IsolatedExtensionError::SpawnFailed(e.to_string()))?;

        let stdin = BufWriter::new(child.stdin.take().ok_or_else(|| {
            IsolatedExtensionError::SpawnFailed("Child stdin not available".to_string())
        })?);
        let stdout = BufReader::new(child.stdout.take().ok_or_else(|| {
            IsolatedExtensionError::SpawnFailed("Child stdout not available".to_string())
        })?);
        let stderr = child.stderr.take().ok_or_else(|| {
            IsolatedExtensionError::SpawnFailed("Child stderr not available".to_string())
        })?;

        // Save process ID for resource monitoring
        let pid = child.id();

        *process_guard = Some(child);
        *self.stdin.lock().await = Some(stdin);
        *self.process_id.lock().await = Some(pid);
        self.running.store(true, Ordering::SeqCst);
        *self.start_time.lock().await = Some(SystemTime::now());

        // Start the background receiver thread
        let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();
        *self.shutdown_tx.lock().await = Some(shutdown_tx);

        // Get the current tokio runtime handle to pass to the receiver thread
        let rt_handle = tokio::runtime::Handle::current();
        self.spawn_receiver_thread(stdout, shutdown_rx, rt_handle, pid);

        // Spawn stderr reader to prevent pipe buffer from filling up and capture logs.
        // Body lives in `capture_stderr_loop` so it can be unit-tested with a
        // synthetic stream (see `stderr_capture_survives_non_utf8_byte`).
        let extension_id = self.extension_id.clone();
        let log_buffer = self.log_buffer.clone();
        std::thread::spawn(move || {
            capture_stderr_loop(BufReader::new(stderr), extension_id, log_buffer);
        });

        // Spawn event push task to send events to extension process
        let mut event_push_rx = {
            let (tx, rx) = tokio::sync::mpsc::channel(100);
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

                            let mut stdin_guard: tokio::sync::MutexGuard<
                                Option<BufWriter<std::process::ChildStdin>>,
                            > = stdin_ref.lock().await;
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
        let rx = self.in_flight.register_with_id(0);

        // Then send the Init message
        self.send_message(&IpcMessage::Init {
            config: serde_json::json!({}),
        })
        .await?;

        // Wait for ready response with timeout
        let extension_id_for_error = self.extension_id.clone();
        let log_buffer_ref = self.log_buffer.clone();
        let response = self
            .in_flight
            .wait_with_timeout(0, rx, Duration::from_secs(self.config.startup_timeout_secs))
            .await
            .map_err(|e| match e {
                super::in_flight::InFlightError::Timeout(ms) => IsolatedExtensionError::Timeout(ms),
                super::in_flight::InFlightError::ChannelClosed => {
                    // Collect recent stderr logs to help diagnose why the runner crashed
                    let all_logs = log_buffer_ref.snapshot();
                    let recent_logs: Vec<String> = all_logs
                        .iter()
                        .rev()
                        .take(10)
                        .cloned()
                        .map(|entry| entry.message)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect();
                    let log_suffix = if recent_logs.is_empty() {
                        String::new()
                    } else {
                        format!("\n\nRecent runner logs:\n{}", recent_logs.join("\n"))
                    };
                    tracing::error!(
                        extension_id = %extension_id_for_error,
                        recent_logs = ?recent_logs,
                        "Extension runner process exited before sending Ready response"
                    );
                    IsolatedExtensionError::IpcError(format!(
                        "Response channel closed (runner process exited unexpectedly){}",
                        log_suffix
                    ))
                }
            })?;

        match response {
            IpcResponse::Ready { descriptor } => {
                *self.descriptor.lock().await = Some(descriptor);
                // Record successful start for crash loop detection
                self.record_successful_start().await;
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
        child_pid: u32,
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
        // Death notification channel
        let death_tx = self.death_tx.clone();

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
                        // 🔧 Phase 1: Structured crash detection
                        let crash_event = if e.kind() == std::io::ErrorKind::UnexpectedEof {
                            CrashEvent::UnexpectedExit {
                                exit_code: None,
                                signal: None,
                            }
                        } else if e.kind() == std::io::ErrorKind::BrokenPipe {
                            CrashEvent::IpcFailure {
                                reason: "Broken pipe".to_string(),
                                stage: IpcFailureStage::ReadLength,
                            }
                        } else {
                            CrashEvent::IpcFailure {
                                reason: format!("{}: {}", e.kind(), e),
                                stage: IpcFailureStage::ReadLength,
                            }
                        };

                        warn!(
                            extension_id = %extension_id,
                            crash_event = %crash_event.description(),
                            error_kind = ?e.kind(),
                            error = %e,
                            "Extension process crashed"
                        );

                        running.store(false, Ordering::SeqCst);

                        // 🔧 FIX: Immediately reap the child process to prevent zombies.
                        // On Unix, a dead child stays as a zombie until waitpid() is called.
                        // The `Child` handle in `self.process` is behind a tokio async mutex
                        // which this std thread cannot lock, so we use the raw PID instead.
                        #[cfg(unix)]
                        {
                            let pid = child_pid as libc::pid_t;
                            let mut status: libc::c_int = 0;
                            let ret = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
                            if ret > 0 {
                                debug!(
                                    extension_id = %extension_id,
                                    pid = child_pid,
                                    "Reaped crashed extension process (waitpid)"
                                );
                            }
                        }

                        // Send death notification to manager
                        if let Ok(tx_guard) = death_tx.try_lock() {
                            if let Some(sender) = tx_guard.as_ref() {
                                let _ = sender.send(());
                            }
                        }
                        break;
                    }
                }

                let len = u32::from_le_bytes(len_bytes) as usize;

                // Sanity check — drain oversized payload to keep stream aligned
                if len > IPC_MAX_BUFFER_SIZE {
                    error!(extension_id = %extension_id, len, max = IPC_MAX_BUFFER_SIZE, "Response too large, draining payload");
                    let mut remaining = len;
                    let mut drain_buf = [0u8; 4096];
                    while remaining > 0 {
                        let to_read = remaining.min(drain_buf.len());
                        if let Err(e) = stdout.read_exact(&mut drain_buf[..to_read]) {
                            error!(extension_id = %extension_id, error = %e, "Failed to drain oversized response");
                            running.store(false, Ordering::SeqCst);
                            break;
                        }
                        remaining -= to_read;
                    }
                    continue;
                }

                // ✨ FIX: Get buffer from tiered pool instead of allocating
                let mut payload = ipc_buffer_pool.acquire(len);
                payload.as_mut().resize(len, 0);

                if let Err(e) = stdout.read_exact(payload.as_mut()) {
                    error!(extension_id = %extension_id, error = %e, "Failed to read response payload");
                    // Buffer automatically returned to pool when dropped
                    running.store(false, Ordering::SeqCst);
                    break;
                }

                // Parse response
                let response = match IpcResponse::from_bytes(payload.as_ref()) {
                    Ok(r) => {
                        // Debug: log StreamSessionInit responses specifically
                        if let IpcResponse::StreamSessionInit {
                            request_id,
                            session_id,
                            success,
                            ..
                        } = &r
                        {
                            debug!(
                                request_id,
                                session_id, success, "Received StreamSessionInit response"
                            );
                        }
                        r
                    }
                    Err(e) => {
                        warn!(extension_id = %extension_id, error = %e, "Failed to parse response");
                        // Buffer automatically returned to pool when dropped
                        continue;
                    }
                };

                // Route response to the correct waiting caller
                // IMPORTANT: Check is_capability_request() FIRST, because CapabilityRequest
                // has a request_id but should NOT be routed to waiting callers
                if response.is_capability_request() {
                    // Handle CapabilityRequest from extension (bidirectional)
                    if let IpcResponse::CapabilityRequest {
                        request_id,
                        capability,
                        params,
                    } = response
                    {
                        debug!(
                            extension_id = %extension_id,
                            request_id,
                            capability = %capability,
                            "Received CapabilityRequest from extension"
                        );

                        // Get the capability provider
                        let provider_opt = capability_provider.read()
                            .map_err(|e| {
                                tracing::error!(error = %e, "CapabilityProvider mutex poisoned in receiver thread");
                                e
                            })
                            .ok()
                            .and_then(|guard| guard.clone());

                        if let Some(provider) = provider_opt {
                            // Parse capability
                            let cap = match super::super::context::ExtensionCapability::from_name(
                                &capability,
                            ) {
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

                            // Spawn capability invocation onto the runtime so the receiver
                            // thread is NOT blocked while the capability runs.
                            let stdin_clone = stdin.clone();
                            let ext_id = extension_id.clone();
                            rt_handle.spawn(async move {
                                // Inject extension_id into params for device_register so the
                                // capability provider can set adapter_id on the device config
                                let params = match cap {
                                    super::super::context::ExtensionCapability::DeviceRegister => {
                                        let mut p = params.as_object().cloned().unwrap_or_default();
                                        p.insert(
                                            "_extension_id".to_string(),
                                            serde_json::json!(ext_id),
                                        );
                                        serde_json::Value::Object(p)
                                    }
                                    _ => params,
                                };
                                let result = provider.invoke_capability(cap, &params).await;

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

                                send_message_to_extension(&stdin_clone, &ext_id, message);
                            });
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

                    // complete() is sync (uses std::sync::Mutex), no block_on needed
                    in_flight.complete(request_id, response);
                } else if response.is_push_output() {
                    // Handle PushOutput messages (extension-initiated)
                    debug!(
                        extension_id = %extension_id,
                        "Received PushOutput from extension"
                    );

                    // Extract push output data and forward to channel
                    if let Some(push_data) = Option::<super::PushOutputData>::from(response) {
                        // Handle push_output_tx gracefully - channel may not be set
                        let tx_guard = push_output_tx.lock().unwrap_or_else(|e| {
                            tracing::warn!(error = %e, "PushOutputTx mutex poisoned in receiver thread, recovering");
                            e.into_inner()
                        });
                        if let Some(tx) = tx_guard.as_ref() {
                            if let Err(e) = tx.try_send(push_data) {
                                if matches!(e, tokio::sync::mpsc::error::TrySendError::Full(_)) {
                                    warn!(
                                        extension_id = %extension_id,
                                        "PushOutput channel full (256), dropping message — backpressure"
                                    );
                                } else {
                                    warn!(
                                        extension_id = %extension_id,
                                        error = %e,
                                        "Failed to forward PushOutput"
                                    );
                                }
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
                    in_flight.complete(0, response);
                }
                // Note: payload is automatically returned to pool when dropped (RAII)
            }

            // Cancel any pending requests on exit (sync — no block_on needed)
            let count = in_flight.cancel_all();
            if count > 0 {
                debug!(extension_id = %extension_id, count, "Cancelled pending requests");
            }

            debug!(extension_id = %extension_id, "Receiver thread exiting");
        });
    }

    /// Stop the extension process
    pub async fn stop(&self) -> IsolatedResult<()> {
        let mut process_guard = self.process.lock().await;

        if process_guard.is_none() {
            return Err(IsolatedExtensionError::NotRunning);
        }

        // Register for ShutdownAck before sending Shutdown message.
        // The runner should respond with IpcResponse::ShutdownAck.
        let rx = self.in_flight.register_with_id(0);

        // Send shutdown message to extension FIRST (before killing receiver thread)
        // The runner will respond with ShutdownAck and then exit
        let _ = self.send_message(&IpcMessage::Shutdown).await;

        // Wait for ShutdownAck with a 2-second timeout (instead of blind sleep).
        // This gives the extension time to flush state via its stop() lifecycle.
        match self
            .in_flight
            .wait_with_timeout(0, rx, std::time::Duration::from_secs(2))
            .await
        {
            Ok(_) => {
                debug!(extension_id = %self.extension_id, "Received ShutdownAck from extension");
            }
            Err(_) => {
                warn!(extension_id = %self.extension_id, "Timeout waiting for ShutdownAck, proceeding with shutdown");
            }
        }

        // Now send shutdown signal to receiver thread
        if let Some(tx) = self.shutdown_tx.lock().await.take() {
            let _ = tx.send(());
        }

        // ✅ FIX: Wait for process to exit with timeout
        // If graceful shutdown fails, force kill after 5 seconds
        if let Some(mut child) = process_guard.take() {
            // ✅ FIX: Wait with polling to prevent hanging on stuck processes
            let start = std::time::Instant::now();
            let timeout = std::time::Duration::from_secs(5);

            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        debug!(extension_id = %self.extension_id, ?status, "Extension process exited");
                        break;
                    }
                    Ok(None) => {
                        if start.elapsed() >= timeout {
                            warn!(extension_id = %self.extension_id, "Process did not exit, force killing");
                            let _ = child.kill();
                            let _ = child.wait();
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    Err(e) => {
                        warn!(extension_id = %self.extension_id, error = %e, "Error waiting for process");
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                }
            }
        }

        *self.stdin.lock().await = None;
        self.running.store(false, Ordering::SeqCst);

        // Cancel any pending requests
        self.in_flight.cancel_all();

        // ✅ FIX: Close all active stream sessions to prevent state inconsistency
        let _ = self.close_all_stream_sessions().await;

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

        // Check concurrent request limit using atomic CAS to avoid TOCTOU race
        if self.config.max_concurrent_requests > 0 {
            let limit = self.config.max_concurrent_requests;
            loop {
                let current = self.active_requests.load(Ordering::SeqCst);
                if current >= limit {
                    warn!(
                        extension_id = %self.extension_id,
                        active = current,
                        limit,
                        "Concurrent request limit reached"
                    );
                    return Err(IsolatedExtensionError::TooManyRequests(limit));
                }
                if self
                    .active_requests
                    .compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    break;
                }
                // CAS failed, another thread modified the counter — retry
            }
        } else {
            // No limit configured, just increment
            self.active_requests.fetch_add(1, Ordering::SeqCst);
        }

        // Use scopeguard to ensure counter is decremented on exit
        let _guard = scopeguard::guard(&self.active_requests, |counter| {
            counter.fetch_sub(1, Ordering::SeqCst);
        });

        // Register the request and get a receiver
        let (request_id, rx) = self.in_flight.register();

        debug!(
            extension_id = %self.extension_id,
            request_id,
            command,
            active_requests = self.active_requests.load(Ordering::SeqCst) - 1,  // -1 because we just incremented
            "Sending execute command"
        );

        // Send the request with retry logic
        self.send_message_with_retry(&IpcMessage::ExecuteCommand {
            command: command.to_string(),
            args: args.clone(),
            request_id,
        })
        .await?;

        // Wait for the response with timeout
        let response = match self
            .in_flight
            .wait_with_timeout(
                request_id,
                rx,
                Duration::from_secs(self.config.command_timeout_secs),
            )
            .await
        {
            Ok(r) => r,
            Err(super::in_flight::InFlightError::Timeout(ms)) => {
                // The extension process is stuck (infinite loop, deadlock,
                // unreachable network call). Previously we only cancelled the
                // in-flight tracker and returned Timeout, leaving the process
                // alive as a zombie that would fail every subsequent command
                // with the same timeout while the death monitor sat idle
                // (stdout pipe never closed). Kill the process now so the
                // receiver thread observes EOF, fires the death notification,
                // and the manager restarts the extension cleanly.
                tracing::warn!(
                    extension_id = %self.extension_id,
                    command,
                    timeout_ms = ms,
                    "Command timed out, killing stuck extension process for restart"
                );
                let mut process_guard = self.process.lock().await;
                if process_guard.is_some() {
                    self.kill_internal(&mut process_guard).await;
                }
                return Err(IsolatedExtensionError::Timeout(ms));
            }
            Err(super::in_flight::InFlightError::ChannelClosed) => {
                return Err(IsolatedExtensionError::IpcError(
                    "Response channel closed".to_string(),
                ));
            }
        };

        match response {
            IpcResponse::Success { data, .. } => Ok(data),
            IpcResponse::Error { error, kind, .. } => match kind {
                ErrorKind::CommandNotFound => Err(IsolatedExtensionError::IpcError(error)),
                ErrorKind::Timeout => Err(IsolatedExtensionError::Timeout(
                    self.config.command_timeout_secs * 1000,
                )),
                _ => Err(IsolatedExtensionError::IpcError(error)),
            },
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

        let (request_id, rx) = self.in_flight.register();

        self.send_message_with_retry(&IpcMessage::ProduceMetrics { request_id })
            .await?;

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
                    tracing::warn!(
                        extension_id = %self.extension_id,
                        timeout_ms = ms,
                        "produce_metrics() timed out - extension may be doing slow network calls"
                    );
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

    /// Refresh the cached descriptor by asking the runner to rebuild it.
    ///
    /// Used by the metrics collector so that runtime-discovered dynamic
    /// metrics (e.g. per-stream `fps.cam1`, `fps.cam2`) become visible
    /// through `/api/extensions`. Failures are non-fatal: callers fall
    /// back to the previously cached descriptor.
    pub async fn refresh_descriptor(&self) -> IsolatedResult<()> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(IsolatedExtensionError::NotRunning);
        }

        let (request_id, rx) = self.in_flight.register();

        self.send_message_with_retry(&IpcMessage::GetDescriptor { request_id })
            .await?;

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
                    tracing::warn!(
                        extension_id = %self.extension_id,
                        timeout_ms = ms,
                        "refresh_descriptor() timed out"
                    );
                    IsolatedExtensionError::Timeout(ms)
                }
                super::in_flight::InFlightError::ChannelClosed => {
                    IsolatedExtensionError::IpcError("Response channel closed".to_string())
                }
            })?;

        match response {
            IpcResponse::Descriptor { descriptor, .. } => {
                *self.descriptor.lock().await = Some(descriptor);
                Ok(())
            }
            IpcResponse::Error { error, .. } => Err(IsolatedExtensionError::IpcError(error)),
            _ => Err(IsolatedExtensionError::InvalidResponse(
                "Expected Descriptor response".to_string(),
            )),
        }
    }

    /// Get captured log entries from the ring buffer.
    pub fn get_logs(&self) -> Vec<ExtensionLogEntry> {
        self.log_buffer.snapshot()
    }

    /// Clear all captured log entries.
    pub fn clear_logs(&self) {
        self.log_buffer.clear();
    }

    /// Get extension statistics via IPC
    pub async fn get_stats(&self) -> IsolatedResult<super::super::system::ExtensionStats> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(IsolatedExtensionError::NotRunning);
        }

        let (request_id, rx) = self.in_flight.register();

        self.send_message_with_retry(&IpcMessage::GetStats { request_id })
            .await?;

        let response = self
            .in_flight
            .wait_with_timeout(request_id, rx, Duration::from_secs(5))
            .await
            .map_err(|e| match e {
                super::in_flight::InFlightError::Timeout(ms) => IsolatedExtensionError::Timeout(ms),
                super::in_flight::InFlightError::ChannelClosed => {
                    IsolatedExtensionError::IpcError("Response channel closed".to_string())
                }
            })?;

        match response {
            IpcResponse::Stats {
                start_count,
                stop_count,
                error_count,
                last_error,
                ..
            } => Ok(super::super::system::ExtensionStats {
                start_count,
                stop_count,
                error_count,
                last_error,
                ..Default::default()
            }),
            IpcResponse::Error { error, .. } => Err(IsolatedExtensionError::IpcError(error)),
            _ => Err(IsolatedExtensionError::InvalidResponse(
                "Expected Stats response".to_string(),
            )),
        }
    }

    /// Execute multiple commands in a batch
    pub async fn execute_batch(
        &self,
        commands: Vec<super::BatchCommand>,
    ) -> IsolatedResult<super::BatchResultsVec> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(IsolatedExtensionError::NotRunning);
        }

        let start = Instant::now();
        let mut results = Vec::with_capacity(commands.len());

        // Execute each command and collect results
        for cmd in commands {
            let cmd_start = Instant::now();
            let result = match self.execute_command(&cmd.command, &cmd.args).await {
                Ok(data) => super::BatchResult {
                    command: cmd.command.clone(),
                    success: true,
                    data: Some(data),
                    error: None,
                    elapsed_ms: cmd_start.elapsed().as_secs_f64() * 1000.0,
                },
                Err(e) => super::BatchResult {
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

        Ok(super::BatchResultsVec {
            results,
            total_elapsed_ms,
        })
    }

    /// Send batch request to extension process
    pub async fn execute_batch_ipc(
        &self,
        commands: Vec<super::BatchCommand>,
    ) -> IsolatedResult<super::BatchResultsVec> {
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
                    self.config.max_concurrent_requests,
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
        let (request_id, rx) = self.in_flight.register();

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
                super::in_flight::InFlightError::Timeout(ms) => IsolatedExtensionError::Timeout(ms),
                super::in_flight::InFlightError::ChannelClosed => {
                    IsolatedExtensionError::IpcError("Response channel closed".to_string())
                }
            })?;

        match response {
            IpcResponse::BatchResults {
                results,
                total_elapsed_ms,
                ..
            } => Ok(super::BatchResultsVec {
                results,
                total_elapsed_ms,
            }),
            IpcResponse::Error { error, .. } => Err(IsolatedExtensionError::IpcError(error)),
            _ => Err(IsolatedExtensionError::InvalidResponse(format!(
                "Expected BatchResults response, got {:?}",
                std::mem::discriminant(&response)
            ))),
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
    /// Mark the extension as being stopped (prevents death monitor from mistakenly restarting it)
    ///
    /// This should be called before stop() during unload to prevent the death monitoring
    /// from treating this as a crash and triggering an automatic restart.
    pub fn mark_stopping(&self) {
        tracing::debug!(
            extension_id = %self.extension_id,
            "Marking extension as stopping (to prevent death monitor restart)"
        );
        self.running.store(false, Ordering::SeqCst);
    }

    /// Check extension health via IPC
    pub async fn health_check(&self) -> IsolatedResult<bool> {
        if !self.running.load(Ordering::SeqCst) {
            return Ok(false);
        }

        let (request_id, rx) = self.in_flight.register();

        if self
            .send_message(&IpcMessage::HealthCheck { request_id })
            .await
            .is_err()
        {
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
    pub async fn stream_capability(
        &self,
    ) -> IsolatedResult<Option<super::super::stream::StreamCapability>> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(IsolatedExtensionError::NotRunning);
        }

        let (request_id, rx) = self.in_flight.register();

        self.send_message_with_retry(&IpcMessage::GetStreamCapability { request_id })
            .await?;

        let response = self
            .in_flight
            .wait_with_timeout(request_id, rx, Duration::from_secs(5))
            .await
            .map_err(|e| match e {
                super::in_flight::InFlightError::Timeout(ms) => IsolatedExtensionError::Timeout(ms),
                super::in_flight::InFlightError::ChannelClosed => {
                    IsolatedExtensionError::IpcError("Response channel closed".to_string())
                }
            })?;

        match response {
            IpcResponse::StreamCapability { capability, .. } => match capability {
                Some(cap_json) => {
                    let cap: super::super::stream::StreamCapability =
                        serde_json::from_value(cap_json)
                            .map_err(|e| IsolatedExtensionError::IpcError(e.to_string()))?;
                    Ok(Some(cap))
                }
                None => Ok(None),
            },
            IpcResponse::Error { error, .. } => Err(IsolatedExtensionError::IpcError(error)),
            _ => Err(IsolatedExtensionError::InvalidResponse(
                "Expected StreamCapability response".to_string(),
            )),
        }
    }

    /// Initialize a stream session via IPC
    pub async fn init_session(
        &self,
        session_id: &str,
        config: serde_json::Value,
    ) -> IsolatedResult<()> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(IsolatedExtensionError::NotRunning);
        }

        // Register session as active
        self.register_session(session_id).await;

        let client_info = super::StreamClientInfo {
            client_id: "host".to_string(),
            ip_addr: None,
            user_agent: None,
        };

        // Register request and get receiver for response
        let (request_id, rx) = self.in_flight.register();

        self.send_message(&IpcMessage::InitStreamSession {
            request_id,
            session_id: session_id.to_string(),
            extension_id: self.extension_id.clone(),
            config,
            client_info,
        })
        .await?;

        // Wait for StreamSessionInit response with timeout
        let response = tokio::time::timeout(Duration::from_secs(10), rx)
            .await
            .map_err(|_| IsolatedExtensionError::Timeout(10000))?
            .map_err(|_| IsolatedExtensionError::ChannelClosed)?;

        match response {
            IpcResponse::StreamSessionInit { success: true, .. } => Ok(()),
            IpcResponse::StreamSessionInit {
                success: false,
                error: Some(e),
                ..
            } => Err(IsolatedExtensionError::ExecutionFailed(e)),
            IpcResponse::StreamSessionInit {
                success: false,
                error: None,
                ..
            } => Err(IsolatedExtensionError::ExecutionFailed(
                "Stream session init failed".to_string(),
            )),
            IpcResponse::Error { error, .. } => Err(IsolatedExtensionError::ExecutionFailed(error)),
            _ => Err(IsolatedExtensionError::IpcError(
                "Unexpected response to InitStreamSession".to_string(),
            )),
        }
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

        let stream_chunk = super::StreamDataChunk {
            sequence: chunk.sequence,
            data_type: chunk.data_type.mime_type(),
            data: chunk.data,
            timestamp: chunk.timestamp,
            is_last: chunk.is_last,
        };

        // Register request and get receiver for response
        let (request_id, rx) = self.in_flight.register();

        // Send message with request_id
        self.send_message(&IpcMessage::ProcessStreamChunk {
            request_id,
            session_id: session_id.to_string(),
            chunk: stream_chunk,
        })
        .await?;

        // Wait for response with timeout (10s for streaming operations)
        let response = tokio::time::timeout(Duration::from_secs(10), rx)
            .await
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
                    let format = data_type
                        .strip_prefix("image/")
                        .unwrap_or("jpeg")
                        .to_string();
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
            IpcResponse::Error { error, .. } => Err(IsolatedExtensionError::ExtensionError(error)),
            _ => Err(IsolatedExtensionError::UnexpectedResponse),
        }
    }

    /// Close a stream session via IPC
    pub async fn close_session(
        &self,
        session_id: &str,
    ) -> IsolatedResult<super::super::stream::SessionStats> {
        // Unregister session
        self.unregister_session(session_id).await;

        if !self.running.load(Ordering::SeqCst) {
            return Err(IsolatedExtensionError::NotRunning);
        }

        // Register request and get receiver for response
        let (request_id, rx) = self.in_flight.register();

        self.send_message(&IpcMessage::CloseStreamSession {
            request_id,
            session_id: session_id.to_string(),
        })
        .await?;

        // Wait for StreamSessionClosed response with timeout
        let response = tokio::time::timeout(Duration::from_secs(10), rx)
            .await
            .map_err(|_| IsolatedExtensionError::Timeout(10000))?
            .map_err(|_| IsolatedExtensionError::ChannelClosed)?;

        match response {
            IpcResponse::StreamSessionClosed {
                total_frames,
                duration_ms: _,
                ..
            } => Ok(super::super::stream::SessionStats {
                output_chunks: total_frames,
                // [unit fix] this wrote a DURATION into a timestamp field —
                // the API computes now_millis - last_activity for duration_ms,
                // so stats.last_activity must be a MILLIS timestamp.
                last_activity: chrono::Utc::now().timestamp_millis(),
                ..Default::default()
            }),
            IpcResponse::Error { error, .. } => Err(IsolatedExtensionError::ExecutionFailed(error)),
            _ => Err(IsolatedExtensionError::IpcError(
                "Unexpected response to CloseStreamSession".to_string(),
            )),
        }
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
                let _ = self
                    .send_message(&IpcMessage::CloseStreamSession {
                        request_id: 0,
                        session_id: session_id.clone(),
                    })
                    .await;
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

    /// Get all active stream session IDs
    pub async fn get_active_sessions(&self) -> Vec<String> {
        let sessions = self.active_sessions.read().await;
        sessions.iter().cloned().collect()
    }

    /// ✨ FIX: Set session invalidation callback
    pub async fn set_session_invalidation_callback(
        &self,
        tx: tokio::sync::mpsc::UnboundedSender<String>,
    ) {
        *self.session_invalidation_tx.lock().await = Some(tx);
    }

    /// Set the death notification callback
    ///
    /// This is called when the extension process dies unexpectedly.
    /// The provided sender is used to notify the manager to restart the extension.
    pub async fn set_death_notification(&self, tx: broadcast::Sender<()>) {
        *self.death_tx.lock().await = Some(tx);
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

        let stream_chunk = super::StreamDataChunk {
            sequence: chunk.sequence,
            data_type: chunk.data_type.mime_type(),
            data: chunk.data,
            timestamp: chunk.timestamp,
            is_last: chunk.is_last,
        };

        // Register request and get receiver for response
        let (request_id, rx) = self.in_flight.register();

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
                super::in_flight::InFlightError::Timeout(ms) => IsolatedExtensionError::Timeout(ms),
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
            IpcResponse::Error { error, .. } => Err(IsolatedExtensionError::ExecutionFailed(error)),
            _ => Err(IsolatedExtensionError::InvalidResponse(
                "Expected ChunkResult response".to_string(),
            )),
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
        let (request_id, rx) = self.in_flight.register();

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
                super::in_flight::InFlightError::Timeout(ms) => IsolatedExtensionError::Timeout(ms),
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
                        error.unwrap_or_else(|| "Unknown error".to_string()),
                    ))
                }
            }
            IpcResponse::Error { error, .. } => Err(IsolatedExtensionError::ExecutionFailed(error)),
            _ => Err(IsolatedExtensionError::InvalidResponse(
                "Expected PushStarted response".to_string(),
            )),
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
        let (request_id, rx) = self.in_flight.register();

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
                super::in_flight::InFlightError::Timeout(ms) => IsolatedExtensionError::Timeout(ms),
                super::in_flight::InFlightError::ChannelClosed => {
                    IsolatedExtensionError::IpcError("Response channel closed".to_string())
                }
            })?;

        match response {
            IpcResponse::PushStopped { success, .. } => {
                if success {
                    debug!(
                        extension_id = %self.extension_id,
                        session_id = %session_id,
                        "Push mode stopped"
                    );
                    Ok(())
                } else {
                    warn!(
                        extension_id = %self.extension_id,
                        session_id = %session_id,
                        "Extension reported push stop failure"
                    );
                    Ok(()) // Still return Ok — caller should clean up regardless
                }
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
            _ => Err(IsolatedExtensionError::InvalidResponse(
                "Expected PushStopped response".to_string(),
            )),
        }
    }

    /// Set the push output channel for receiving PushOutput messages from extension
    ///
    /// This channel is used to forward push data from the extension to WebSocket clients.
    /// The host should set this before starting a Push mode session.
    pub async fn set_push_output_channel(
        &self,
        tx: tokio::sync::mpsc::Sender<super::PushOutputData>,
    ) {
        let mut push_tx = self.push_output_tx.lock().unwrap_or_else(|e| {
            tracing::warn!(error = %e, "PushOutputTx mutex poisoned, recovering");
            e.into_inner()
        });
        *push_tx = Some(tx);
    }

    /// Get a clone of the push output channel sender
    pub async fn get_push_output_channel(
        &self,
    ) -> Option<tokio::sync::mpsc::Sender<super::PushOutputData>> {
        let push_tx = self.push_output_tx.lock().unwrap_or_else(|e| {
            tracing::warn!(error = %e, "PushOutputTx mutex poisoned, recovering");
            e.into_inner()
        });
        push_tx.clone()
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
        let _ = self
            .execute_command("gc_memory", &serde_json::json!({}))
            .await;

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

    // Internal helper methods

    async fn send_message(&self, msg: &IpcMessage) -> IsolatedResult<()> {
        let mut stdin_guard = self.stdin.lock().await;

        let stdin = stdin_guard
            .as_mut()
            .ok_or(IsolatedExtensionError::NotInitialized)?;

        let payload = msg
            .to_bytes()
            .map_err(|e| IsolatedExtensionError::IpcError(format!("Serialization error: {}", e)))?;

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

    /// 🔧 Phase 2: Send message with retry logic for transient failures
    async fn send_message_with_retry(&self, msg: &IpcMessage) -> IsolatedResult<()> {
        let mut retries = 0;
        let max_retries = self.config.ipc_max_retries;
        let base_delay = self.config.ipc_retry_delay_ms;

        loop {
            match self.send_message(msg).await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    if retries >= max_retries {
                        return Err(e);
                    }

                    // Exponential backoff: 100ms, 200ms, 400ms...
                    let delay_ms = base_delay * (2_u64.pow(retries as u32));
                    warn!(
                        extension_id = %self.extension_id,
                        error = %e,
                        retries,
                        next_retry_in_ms = delay_ms,
                        "IPC send failed, retrying with exponential backoff"
                    );

                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    retries += 1;
                }
            }
        }
    }

    /// Push an event to the extension
    pub async fn push_event(
        &self,
        event_type: String,
        payload: serde_json::Value,
    ) -> IsolatedResult<()> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(IsolatedExtensionError::NotRunning);
        }

        self.send_message(&IpcMessage::EventPush {
            event_type,
            payload,
            timestamp: chrono::Utc::now().timestamp_millis(),
        })
        .await
    }

    /// Send a config hot-reload update to the running extension.
    ///
    /// This sends a ConfigUpdate IPC message (fire-and-forget).
    /// The extension's `configure()` method will be called in the runner process.
    pub async fn send_config_update(&self, config: &serde_json::Value) -> IsolatedResult<()> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(IsolatedExtensionError::NotRunning);
        }

        self.send_message_with_retry(&IpcMessage::ConfigUpdate {
            config: config.clone(),
        })
        .await
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
        let cancelled_count = self.in_flight.cancel_all();
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
                        drop(process_guard);
                        // Record crash for crash loop detection
                        self.record_crash(format!("process exited with status: {:?}", status))
                            .await;
                        return Err(IsolatedExtensionError::Crashed(format!(
                            "Process exited with status: {:?}",
                            status
                        )));
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
                        self.kill_internal(&mut process_guard).await;
                        drop(process_guard);
                        // Record crash for crash loop detection
                        self.record_crash(format!("process exited with code: {:?}", status.code()))
                            .await;
                        return Err(IsolatedExtensionError::Crashed(format!(
                            "Process exited with code: {:?}",
                            status.code()
                        )));
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
                    self.kill_internal(&mut process_guard).await;
                    drop(process_guard);

                    // Record crash for crash loop detection
                    self.record_crash("IPC failure while starting extension")
                        .await;

                    // Attempt restart with crash loop detection
                    if self.config.restart_on_crash {
                        if let Err(e) = self.should_allow_restart().await {
                            return Err(e);
                        }
                        return self.start().await;
                    }
                    return Err(IsolatedExtensionError::Crashed(format!(
                        "Memory limit exceeded: {}MB > {}MB",
                        memory_mb, self.config.max_memory_mb
                    )));
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

                    // Record crash for crash loop detection
                    self.record_crash("IPC failure during health check").await;

                    // Attempt restart with crash loop detection
                    if self.config.restart_on_crash {
                        if let Err(e) = self.should_allow_restart().await {
                            return Err(e);
                        }
                        return self.start().await;
                    }
                    return Err(IsolatedExtensionError::Crashed(format!(
                        "Memory limit exceeded: {}MB > {}MB",
                        memory_mb, self.config.max_memory_mb
                    )));
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

        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "VmRSS not found",
        ))
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

        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Failed to get memory",
        ))
    }

    /// Get the event push channel for this extension
    ///
    /// This method is called by the EventDispatcher to get the channel
    /// for pushing events to the isolated extension process.
    pub async fn get_event_push_channel(
        &self,
    ) -> Option<tokio::sync::mpsc::Sender<(String, Value)>> {
        self.event_push_tx.lock().await.clone()
    }

    /// Get event subscriptions from the extension
    ///
    /// This method queries the extension process for its event subscriptions
    /// via the GetEventSubscriptions IPC message.
    pub async fn get_event_subscriptions(&self) -> IsolatedResult<Vec<String>> {
        // Register a new request
        let (request_id, rx) = self.in_flight.register();

        // Send GetEventSubscriptions message
        self.send_message_with_retry(&IpcMessage::GetEventSubscriptions { request_id })
            .await?;

        // Wait for response with timeout
        let timeout_duration = Duration::from_secs(5);
        match tokio::time::timeout(timeout_duration, rx).await {
            Ok(Ok(response)) => match response {
                IpcResponse::EventSubscriptions { event_types, .. } => Ok(event_types),
                IpcResponse::Error { error, .. } => Err(IsolatedExtensionError::SpawnFailed(error)),
                _ => Err(IsolatedExtensionError::SpawnFailed(
                    "Unexpected response type".to_string(),
                )),
            },
            Ok(Err(_)) => Err(IsolatedExtensionError::SpawnFailed(
                "Response channel closed".to_string(),
            )),
            Err(_) => Err(IsolatedExtensionError::SpawnFailed(
                "Request timeout".to_string(),
            )),
        }
    }

    // ========================================================================
    // Crash Loop Detection Methods
    // ========================================================================

    /// Check if restart should be allowed based on crash history
    ///
    /// Returns Ok(()) if restart is allowed, or Err if crash loop is detected.
    /// A crash loop is detected when:
    /// - Consecutive crashes >= max_restart_attempts
    /// - AND last crash was within the cooldown period
    pub async fn should_allow_restart(&self) -> IsolatedResult<()> {
        let consecutive = self.consecutive_crashes.load(Ordering::SeqCst);
        let last_crash = self.last_crash_time.lock().await;

        if consecutive >= self.config.max_restart_attempts {
            // Check if cooldown period has passed
            if let Some(last_time) = *last_crash {
                let cooldown = Duration::from_secs(self.config.restart_cooldown_secs * 10); // 10x cooldown for crash loop
                if last_time.elapsed() < cooldown {
                    warn!(
                        extension_id = %self.extension_id,
                        consecutive_crashes = consecutive,
                        max_restart_attempts = self.config.max_restart_attempts,
                        "Crash loop detected - too many crashes in cooldown period. Will not auto restart."
                    );
                    return Err(IsolatedExtensionError::Crashed(format!(
                        "Crash loop detected: {} consecutive crashes within cooldown period. \
                             Extension has stability issues. Will not restart.",
                        consecutive
                    )));
                } else {
                    // Cooldown passed, reset counter and allow restart
                    drop(last_crash);
                    self.consecutive_crashes.store(0, Ordering::SeqCst);
                    info!(
                        extension_id = %self.extension_id,
                        "Crash loop cooldown expired, resetting crash counter"
                    );
                }
            }
        }

        Ok(())
    }

    /// Record a crash for crash loop detection
    pub async fn record_crash(&self, reason: impl Into<String>) {
        let consecutive = self.consecutive_crashes.fetch_add(1, Ordering::SeqCst);
        let reason = reason.into();
        *self.last_crash_reason.lock().await = Some(reason.clone());
        let mut last_crash = self.last_crash_time.lock().await;
        *last_crash = Some(Instant::now());
        warn!(
            extension_id = %self.extension_id,
            consecutive_crashes = consecutive + 1,
            reason = %reason,
            "Extension crash recorded for crash loop detection"
        );
    }

    /// Current crash-loop status: (consecutive crashes, last crash reason).
    pub async fn crash_info(&self) -> (u32, Option<String>) {
        (
            self.consecutive_crashes.load(Ordering::SeqCst),
            self.last_crash_reason.lock().await.clone(),
        )
    }

    /// Round-trip liveness probe. The runner answers `Ping` with `Pong`
    /// without touching the extension; a timeout means the runner itself is
    /// wedged (deadlock, unbounded loop in the message pump) — see
    /// `spawn_health_monitor`.
    pub async fn ping(&self) -> IsolatedResult<()> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(IsolatedExtensionError::NotRunning);
        }
        let (request_id, rx) = self.in_flight.register();
        if let Err(e) = self
            .send_message_with_retry(&IpcMessage::Ping {
                request_id,
                timestamp: chrono::Utc::now().timestamp_millis(),
            })
            .await
        {
            // Cancel our own entry on send failure: a leaked pending entry
            // pins pending_count() ≥ 1 forever, which makes the health
            // monitor skip every future probe (hang detection silently dead).
            self.in_flight.cancel(request_id);
            return Err(e);
        }
        match self
            .in_flight
            .wait_with_timeout(
                request_id,
                rx,
                Duration::from_secs(self.config.health_check_timeout_secs.max(1)),
            )
            .await
        {
            Ok(IpcResponse::Pong { .. }) => Ok(()),
            Ok(other) => Err(IsolatedExtensionError::InvalidResponse(format!(
                "expected Pong, got {other:?}"
            ))),
            Err(super::in_flight::InFlightError::Timeout(ms)) => {
                Err(IsolatedExtensionError::Timeout(ms))
            }
            Err(super::in_flight::InFlightError::ChannelClosed) => Err(
                IsolatedExtensionError::IpcError("response channel closed".to_string()),
            ),
        }
    }

    /// Spawn the periodic liveness probe (see `health_check_interval_secs`).
    /// Exits when the process stops running — the death monitor owns restarts;
    /// this task's only job is to convert "hung but alive" into "dead" by
    /// killing the process after repeated Ping failures.
    pub fn spawn_health_monitor(self: &std::sync::Arc<Self>) {
        let interval = self.config.health_check_interval_secs;
        if interval == 0 {
            return;
        }
        let max_failures = self.config.health_check_max_failures.max(1);
        let this = std::sync::Arc::clone(self);
        tokio::spawn(async move {
            let mut consecutive_failures: u32 = 0;
            loop {
                tokio::time::sleep(Duration::from_secs(interval)).await;
                if !this.running.load(Ordering::SeqCst) {
                    debug!(
                        extension_id = %this.extension_id,
                        "Health monitor exiting: process no longer running"
                    );
                    return;
                }
                // Busy runner ≠ hung runner. The runner's message pump handles
                // one message at a time and native commands run as a single
                // sync FFI call, so a legitimate long command (model load,
                // batch inference — command_timeout_secs is 300s by default)
                // blocks Ping for its whole duration. Probing during a command
                // would kill healthy extensions mid-command; the command's own
                // timeout already owns the hang path while one is executing.
                if this.in_flight.pending_count() > 0 {
                    debug!(
                        extension_id = %this.extension_id,
                        "Health probe skipped: command(s) in flight"
                    );
                    consecutive_failures = 0;
                    continue;
                }
                match this.ping().await {
                    Ok(()) => consecutive_failures = 0,
                    Err(e) => {
                        consecutive_failures += 1;
                        warn!(
                            extension_id = %this.extension_id,
                            error = %e,
                            consecutive_failures,
                            max_failures,
                            "Extension liveness probe failed"
                        );
                        if consecutive_failures >= max_failures {
                            // Same kill path a command timeout takes: EOF on
                            // stdout fires the death notification and the
                            // manager's crash machinery restarts or
                            // circuit-breaks the extension.
                            warn!(
                                extension_id = %this.extension_id,
                                consecutive_failures,
                                "Extension unresponsive to liveness probes — killing hung process"
                            );
                            this.record_crash("unresponsive to liveness probes (hang)")
                                .await;
                            let mut process_guard = this.process.lock().await;
                            if process_guard.is_some() {
                                this.kill_internal(&mut process_guard).await;
                            }
                            return;
                        }
                    }
                }
            }
        });
    }

    /// Record successful start - reset crash counter after stable period
    pub async fn record_successful_start(&self) {
        self.consecutive_crashes.store(0, Ordering::SeqCst);
        let mut last_crash = self.last_crash_time.lock().await;
        *last_crash = None;

        info!(
            extension_id = %self.extension_id,
            "Extension started successfully, resetting crash loop counter"
        );
    }
}

/// Infer log level from a log line by looking at common prefixes.
fn infer_log_level(line: &str) -> &'static str {
    let lower = line.to_ascii_lowercase();

    // First, try to match tracing compact format: "LEVEL " after timestamp
    // e.g. "2026-06-05T05:16:34.378080Z  WARN Failed to..."
    // Look for the log level keyword that appears right after the timestamp (after 'Z ' or at start)
    if let Some(pos) = lower.find("z  ") {
        let after_ts = &lower[pos + 3..];
        if after_ts.starts_with("error") {
            return "error";
        }
        if after_ts.starts_with("warn") {
            return "warn";
        }
        if after_ts.starts_with("info") {
            return "info";
        }
        if after_ts.starts_with("debug") {
            return "debug";
        }
        if after_ts.starts_with("trace") {
            return "trace";
        }
    }

    // Fallback: match prefix patterns
    if lower.starts_with("error") || lower.contains("[error]") {
        "error"
    } else if lower.starts_with("warn") || lower.contains("[warn]") {
        "warn"
    } else if lower.starts_with("info") || lower.contains("[info]") {
        "info"
    } else if lower.starts_with("debug") || lower.contains("[debug]") {
        "debug"
    } else if lower.starts_with("trace") {
        "trace"
    } else {
        "info"
    }
}

/// Continuously read stderr lines from `reader` into `log_buffer`.
///
/// Extracted from the stderr-capture spawn site so the logic can be exercised
/// with a synthetic `BufRead` in unit tests.
fn capture_stderr_loop<R: std::io::BufRead>(
    reader: R,
    extension_id: String,
    log_buffer: Arc<LogBuffer>,
) {
    // Byte-level splitting instead of `BufRead::lines()`. `lines()` yields
    // Err(InvalidData) on any non-UTF-8 byte; combined with a `map_while(Result::ok)`
    // exit that permanently killed the capture thread — freezing the log buffer
    // at startup output. Native deps (OpenCV/ONNX/ffmpeg/...) write directly to
    // fd 2 and routinely emit non-UTF-8 bytes during model load, so we read raw
    // bytes and lossy-convert, and never abort on a single bad line.
    let mut reader = reader;
    let mut buf: Vec<u8> = Vec::with_capacity(512);
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) => break, // EOF — child closed stderr
            Ok(_) => {
                // Strip trailing CR/LF produced by the splitter.
                while matches!(buf.last(), Some(b'\n') | Some(b'\r')) {
                    buf.pop();
                }
                if buf.is_empty() {
                    continue;
                }
                let line = String::from_utf8_lossy(&buf).into_owned();
                let level = infer_log_level(&line);
                match level {
                    "ERROR" | "error" => {
                        tracing::error!(extension_id = %extension_id, "{}", line);
                    }
                    "WARN" | "warning" => {
                        tracing::warn!(extension_id = %extension_id, "{}", line);
                    }
                    _ => {
                        tracing::info!(extension_id = %extension_id, "{}", line);
                    }
                }
                let timestamp = chrono::Utc::now().timestamp_millis();
                log_buffer.push(ExtensionLogEntry {
                    timestamp,
                    level: level.to_string(),
                    message: line,
                });
            }
            Err(e) => {
                // Transient read error — keep draining. A single blip must not
                // freeze the whole log buffer (the old failure mode).
                tracing::warn!(
                    extension_id = %extension_id,
                    error = %e,
                    "stderr read error, continuing capture"
                );
                continue;
            }
        }
    }
}

impl Drop for IsolatedExtension {
    fn drop(&mut self) {
        if let Ok(mut child) = self.process.try_lock() {
            if let Some(mut proc) = child.take() {
                let extension_id = self.extension_id.clone();

                // Spawn background thread for graceful-then-forceful shutdown
                std::thread::spawn(move || {
                    // Phase 1: Try graceful shutdown via stdin close
                    // Closing stdin signals the runner to exit its event loop
                    drop(proc.stdin.take());

                    let graceful_deadline =
                        std::time::Instant::now() + std::time::Duration::from_secs(3);

                    // Wait for process to exit gracefully
                    loop {
                        match proc.try_wait() {
                            Ok(Some(_)) => {
                                tracing::debug!(
                                    extension_id = %extension_id,
                                    "Extension process exited gracefully"
                                );
                                return;
                            }
                            Ok(None) => {
                                if std::time::Instant::now() >= graceful_deadline {
                                    break; // Proceed to SIGKILL
                                }
                                std::thread::sleep(std::time::Duration::from_millis(50));
                            }
                            Err(_) => return, // Already reaped
                        }
                    }

                    // Phase 2: Force kill after graceful timeout
                    tracing::warn!(
                        extension_id = %extension_id,
                        "Extension did not exit gracefully, sending SIGKILL"
                    );
                    let _ = proc.kill();

                    let kill_deadline =
                        std::time::Instant::now() + std::time::Duration::from_secs(5);
                    loop {
                        match proc.try_wait() {
                            Ok(Some(_)) => {
                                tracing::debug!(
                                    extension_id = %extension_id,
                                    "Extension process reaped after SIGKILL"
                                );
                                return;
                            }
                            Ok(None) => {
                                if std::time::Instant::now() >= kill_deadline {
                                    tracing::error!(
                                        extension_id = %extension_id,
                                        "Process did not exit after SIGKILL, may become zombie"
                                    );
                                    return;
                                }
                                std::thread::sleep(std::time::Duration::from_millis(100));
                            }
                            Err(_) => return,
                        }
                    }
                });
            }
        }
    }
}

/// Apply manifest-declared env hints to a runner command.
///
/// Reads `<extension_dir>/manifest.json` → optional `"env_hints"` map of
/// `{VAR: path_template}`. Templates may contain `{binaries}` (the
/// platform binaries dir of this spawn) and `{extension_dir}`. A hint is
/// applied only when the env var is not already set AND the resolved file
/// exists; anything else is silently skipped (declared-but-absent files are
/// a normal state, e.g. runtime libs only bundled in jetson/cuda variants).
fn apply_manifest_env_hints(
    cmd: &mut Command,
    extension_dir: &std::path::Path,
    binaries_dir: &std::path::Path,
) {
    let manifest_path = extension_dir.join("manifest.json");
    let Ok(raw) = std::fs::read_to_string(&manifest_path) else {
        return;
    };
    let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&raw) else {
        tracing::debug!(
            manifest = %manifest_path.display(),
            "env_hints: manifest.json unreadable, skipping"
        );
        return;
    };
    let Some(hints) = manifest.get("env_hints").and_then(|h| h.as_object()) else {
        return;
    };
    for (var, template) in hints {
        let Some(template) = template.as_str() else {
            continue;
        };
        if std::env::var_os(var).is_some() {
            continue;
        }
        let resolved = template
            .replace("{binaries}", &binaries_dir.to_string_lossy())
            .replace("{extension_dir}", &extension_dir.to_string_lossy());
        let path = std::path::Path::new(&resolved);
        if path.is_file() {
            tracing::debug!(
                var,
                value = %resolved,
                "Applying manifest env hint"
            );
            cmd.env(var, &resolved);
        } else {
            tracing::debug!(
                var,
                value = %resolved,
                "Manifest env hint target missing, skipping"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "neomind-env-hints-test-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn env_hints_resolve_placeholders_and_apply() {
        let tmp = scratch_dir("apply");
        let bin_dir = tmp.join("binaries/darwin_aarch64");
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::write(bin_dir.join("libonnxruntime.dylib"), b"fake").unwrap();
        std::fs::write(
            tmp.join("manifest.json"),
            r#"{"id":"t","env_hints":{"ORT_DYLIB_PATH":"{binaries}/libonnxruntime.dylib"}}"#,
        )
        .unwrap();

        let mut cmd = Command::new("true");
        apply_manifest_env_hints(&mut cmd, &tmp, &bin_dir);
        assert_eq!(
            cmd.get_envs()
                .find(|(k, _)| *k == std::ffi::OsStr::new("ORT_DYLIB_PATH"))
                .and_then(|(_, v)| v)
                .map(|v| v.to_string_lossy().to_string()),
            Some(
                bin_dir
                    .join("libonnxruntime.dylib")
                    .to_string_lossy()
                    .to_string()
            )
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn env_hints_skipped_when_target_missing() {
        let tmp = scratch_dir("missing");
        std::fs::create_dir_all(tmp.join("binaries/darwin_aarch64")).unwrap();
        std::fs::write(
            tmp.join("manifest.json"),
            r#"{"id":"t","env_hints":{"ORT_DYLIB_PATH":"{binaries}/libonnxruntime.dylib"}}"#,
        )
        .unwrap();

        let mut cmd = Command::new("true");
        apply_manifest_env_hints(&mut cmd, &tmp, &tmp.join("binaries/darwin_aarch64"));
        assert!(!cmd.get_envs().any(|(k, _)| k == "ORT_DYLIB_PATH"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn env_hints_noop_without_manifest_or_field() {
        let tmp = scratch_dir("noop");
        std::fs::create_dir_all(&tmp).unwrap();
        let mut cmd = Command::new("true");
        // no manifest.json at all
        apply_manifest_env_hints(&mut cmd, &tmp, &tmp);
        // manifest without env_hints
        std::fs::write(tmp.join("manifest.json"), r#"{"id":"t"}"#).unwrap();
        apply_manifest_env_hints(&mut cmd, &tmp, &tmp);
        assert!(cmd.get_envs().next().is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn env_hints_extension_dir_placeholder() {
        let tmp = scratch_dir("extdir");
        std::fs::create_dir_all(tmp.join("rknn")).unwrap();
        std::fs::write(tmp.join("rknn/librknnrt.so"), b"fake").unwrap();
        std::fs::write(
            tmp.join("manifest.json"),
            r#"{"id":"t","env_hints":{"RKNN_LIB_PATH":"{extension_dir}/rknn/librknnrt.so"}}"#,
        )
        .unwrap();

        let mut cmd = Command::new("true");
        apply_manifest_env_hints(&mut cmd, &tmp, &tmp);
        assert!(cmd
            .get_envs()
            .any(|(k, _)| k == std::ffi::OsStr::new("RKNN_LIB_PATH")));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn stderr_capture_survives_non_utf8_byte() {
        // Regression: the stderr capture loop used `reader.lines().map_while(Result::ok)`.
        // `BufRead::lines()` yields Err(InvalidData) on any non-UTF-8 byte, and
        // `map_while(Result::ok)` terminates the whole iteration at the first Err —
        // silently killing the capture thread. The log buffer then froze at whatever
        // was collected before the bad byte (i.e. startup output), and no later logs
        // ever appeared. Native deps (OpenCV/ONNX/ffmpeg/...) write directly to fd 2
        // and routinely emit non-UTF-8 bytes during model load — right after startup.
        use std::io::Cursor;

        let mut stream: Vec<u8> = Vec::new();
        stream.extend_from_slice(b"2026-07-31T00:00:00.000000Z  INFO extension starting\n");
        stream.extend_from_slice(b"model load warning: [");
        stream.push(0xFF); // invalid UTF-8 byte — killed the old loop
        stream.extend_from_slice(b"] check locale\n");
        stream.extend_from_slice(b"2026-07-31T00:00:01.000000Z  INFO inference ready\n");

        let buffer = Arc::new(LogBuffer::new());
        capture_stderr_loop(Cursor::new(stream), "test-ext".to_string(), buffer.clone());

        let snapshot = buffer.snapshot();
        let messages: Vec<&str> = snapshot.iter().map(|e| e.message.as_str()).collect();

        // Pre-bad-byte line must be present.
        assert!(
            messages.iter().any(|m| m.contains("extension starting")),
            "startup line missing: {messages:?}"
        );
        // The bug: the line AFTER the non-UTF-8 byte was never captured because the
        // loop had already died. A passing fix must reach this line.
        assert!(
            messages.iter().any(|m| m.contains("inference ready")),
            "post-non-utf8 line missing — capture thread died at 0xFF: {messages:?}"
        );
    }

    #[test]
    fn test_config_default() {
        let config = IsolatedExtensionConfig::default();
        assert_eq!(config.startup_timeout_secs, 120);
        assert_eq!(config.command_timeout_secs, 300);
        assert_eq!(config.max_memory_mb, 2048); // Updated for YOLO extensions
        assert!(config.restart_on_crash);
    }

    #[test]
    fn test_crash_event_description() {
        // Test UnexpectedExit with exit code
        let event = CrashEvent::UnexpectedExit {
            exit_code: Some(1),
            signal: None,
        };
        assert_eq!(event.description(), "Process exited with code 1");

        // Test UnexpectedExit with signal
        let event = CrashEvent::UnexpectedExit {
            exit_code: None,
            signal: Some(9),
        };
        assert_eq!(event.description(), "Process terminated by signal 9");

        // Test UnexpectedExit with no info
        let event = CrashEvent::UnexpectedExit {
            exit_code: None,
            signal: None,
        };
        assert_eq!(event.description(), "Process exited unexpectedly");

        // Test IpcFailure
        let event = CrashEvent::IpcFailure {
            reason: "Broken pipe".to_string(),
            stage: IpcFailureStage::ReadLength,
        };
        assert!(event
            .description()
            .contains("IPC failure during ReadLength"));
        assert!(event.description().contains("Broken pipe"));
    }
}
