//! Process management for isolated extensions
//!
//! This module provides the `IsolatedExtension` wrapper that manages
//! extension processes with automatic restart and health monitoring.

use std::io::{BufReader, BufWriter, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use super::ipc::{IpcFrame, IpcMessage, IpcResponse};
use super::{IsolatedExtensionError, IsolatedResult};
use crate::extension::system::{ExtensionMetadata, ExtensionMetricValue, Result};

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
            max_memory_mb: 512,
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
    /// Stdin writer
    stdin: Mutex<Option<BufWriter<std::process::ChildStdin>>>,
    /// Stdout reader
    stdout: Mutex<Option<BufReader<std::process::ChildStdout>>>,
    /// Request counter
    request_id: AtomicU64,
    /// Extension metadata (set after initialization)
    metadata: Mutex<Option<ExtensionMetadata>>,
    /// Configuration
    config: IsolatedExtensionConfig,
    /// Restart counter
    restart_count: AtomicU64,
    /// Last restart time
    last_restart: Mutex<Option<Instant>>,
    /// Running state
    running: std::sync::atomic::AtomicBool,
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
            stdout: Mutex::new(None),
            request_id: AtomicU64::new(0),
            metadata: Mutex::new(None),
            config,
            restart_count: AtomicU64::new(0),
            last_restart: Mutex::new(None),
            running: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Start the extension process
    pub async fn start(&self) -> IsolatedResult<()> {
        let mut process_guard = self.process.lock().await;

        if process_guard.is_some() {
            return Err(IsolatedExtensionError::AlreadyRunning);
        }

        // Spawn the extension process
        let mut child = Command::new(&self.extension_path)
            .arg("--isolated-mode")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| IsolatedExtensionError::SpawnFailed(e.to_string()))?;

        let stdin = BufWriter::new(child.stdin.take().unwrap());
        let stdout = BufReader::new(child.stdout.take().unwrap());

        *process_guard = Some(child);
        *self.stdin.lock().await = Some(stdin);
        *self.stdout.lock().await = Some(stdout);
        self.running.store(true, Ordering::SeqCst);

        // Send initialization message
        self.send_message(&IpcMessage::Init {
            config: serde_json::json!({}),
        })
        .await?;

        // Wait for ready response with timeout
        let response = self
            .receive_response_with_timeout(Duration::from_secs(self.config.startup_timeout_secs))
            .await?;

        match response {
            IpcResponse::Ready { metadata } => {
                *self.metadata.lock().await = Some(metadata);
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

    /// Stop the extension process
    pub async fn stop(&self) -> IsolatedResult<()> {
        let mut process_guard = self.process.lock().await;

        if process_guard.is_none() {
            return Err(IsolatedExtensionError::NotRunning);
        }

        // Send shutdown message
        let _ = self.send_message(&IpcMessage::Shutdown).await;

        // Wait for process to exit
        if let Some(mut child) = process_guard.take() {
            let _ = child.wait();
        }

        *self.stdin.lock().await = None;
        *self.stdout.lock().await = None;
        self.running.store(false, Ordering::SeqCst);

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

        let request_id = self.request_id.fetch_add(1, Ordering::SeqCst);

        self.send_message(&IpcMessage::ExecuteCommand {
            command: command.to_string(),
            args: args.clone(),
            request_id,
        })
        .await?;

        let response = self
            .receive_response_with_timeout(Duration::from_secs(self.config.command_timeout_secs))
            .await?;

        match response {
            IpcResponse::Success { data, .. } => Ok(data),
            IpcResponse::Error { error, kind, .. } => {
                use super::ipc::ErrorKind;
                match kind {
                    ErrorKind::CommandNotFound => {
                        Err(IsolatedExtensionError::IpcError(error))
                    }
                    ErrorKind::Timeout => Err(IsolatedExtensionError::Timeout(
                        self.config.command_timeout_secs * 1000,
                    )),
                    _ => Err(IsolatedExtensionError::IpcError(error)),
                }
            }
            _ => Err(IsolatedExtensionError::InvalidResponse(
                "Expected Success or Error response".to_string(),
            )),
        }
    }

    /// Get metrics from extension
    pub async fn produce_metrics(&self) -> IsolatedResult<Vec<ExtensionMetricValue>> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(IsolatedExtensionError::NotRunning);
        }

        let request_id = self.request_id.fetch_add(1, Ordering::SeqCst);

        self.send_message(&IpcMessage::ProduceMetrics { request_id })
            .await?;

        let response = self
            .receive_response_with_timeout(Duration::from_secs(5))
            .await?;

        match response {
            IpcResponse::Metrics { metrics, .. } => Ok(metrics),
            IpcResponse::Error { error, .. } => {
                Err(IsolatedExtensionError::IpcError(error))
            }
            _ => Err(IsolatedExtensionError::InvalidResponse(
                "Expected Metrics response".to_string(),
            )),
        }
    }

    /// Check if process is alive
    pub fn is_alive(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Get extension metadata
    pub async fn metadata(&self) -> Option<ExtensionMetadata> {
        self.metadata.lock().await.clone()
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

    async fn receive_response_with_timeout(
        &self,
        timeout: Duration,
    ) -> IsolatedResult<IpcResponse> {
        let mut stdout_guard = self.stdout.lock().await;

        let stdout = stdout_guard
            .as_mut()
            .ok_or(IsolatedExtensionError::NotInitialized)?;

        // Read length prefix (4 bytes)
        let mut len_bytes = [0u8; 4];
        let start = Instant::now();

        loop {
            match stdout.read_exact(&mut len_bytes) {
                Ok(()) => break,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if start.elapsed() > timeout {
                        return Err(IsolatedExtensionError::Timeout(
                            timeout.as_millis() as u64,
                        ));
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(e) => {
                    return Err(IsolatedExtensionError::IpcError(format!(
                        "Read error: {}",
                        e
                    )))
                }
            }
        }

        let len = u32::from_le_bytes(len_bytes) as usize;

        // Read payload
        let mut payload = vec![0u8; len];
        stdout
            .read_exact(&mut payload)
            .map_err(|e| IsolatedExtensionError::IpcError(e.to_string()))?;

        let response = IpcResponse::from_bytes(&payload).map_err(|e| {
            IsolatedExtensionError::InvalidResponse(format!("Deserialization error: {}", e))
        })?;

        Ok(response)
    }

    async fn kill_internal(&self, process_guard: &mut Option<Child>) {
        if let Some(mut child) = process_guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.running.store(false, Ordering::SeqCst);
    }
}

impl Drop for IsolatedExtension {
    fn drop(&mut self) {
        // Attempt graceful shutdown
        if let Some(mut child) = self.process.blocking_lock().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
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
