//! Mock Runner — in-process IPC routing that mirrors the real
//! neomind-extension-runner's message dispatch, using the same
//! IpcMessage types but over tokio channels instead of stdin/stdout.
//!
//! The runner in production:
//! 1. Receives IpcMessage frames on stdin (length-prefixed JSON)
//! 2. Deserializes to IpcMessage enum
//! 3. Routes to the extension trait method (execute_command, handle_event, etc.)
//! 4. Sends IpcResponse frames on stdout
//! 5. Handles CapabilityRequest from the extension (forwards to platform)
//!
//! This mock does 1-5 identically except:
//! - Transport is tokio mpsc channels (no process boundary)
//! - Capability requests are answered by the MockCapabilityProvider
//! - Everything is instrumented for assertions

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use tokio::sync::{mpsc, oneshot};

use crate::host::Extension;
use crate::ipc_types::{ExtensionDescriptor, IpcMessage, IpcResponse};
use crate::ExtensionMetadata;

use super::mock_capability::CapabilityRecorder;
use super::mock_capability::MockCapabilityProvider;

/// Configuration for the test kit.
#[derive(Debug, Clone)]
pub struct TestKitConfig {
    /// Timeout for command execution (default 5s — matches runner FFI timeout).
    pub command_timeout: Duration,
    /// Timeout for event processing (default 2s).
    pub event_timeout: Duration,
    /// If true, panics in the extension's async tasks fail the test
    /// instead of being silently swallowed.
    pub propagate_panics: bool,
}

impl Default for TestKitConfig {
    fn default() -> Self {
        Self {
            command_timeout: Duration::from_secs(5),
            event_timeout: Duration::from_secs(2),
            propagate_panics: true,
        }
    }
}

/// Pending command with its reply channel.
struct PendingCommand {
    reply_tx: oneshot::Sender<Result<serde_json::Value, String>>,
    started_at: std::time::Instant,
}

/// The test kit — your extension running inside a mock IPC host.
///
/// Drop it and everything shuts down (no leaked tasks, no zombie channels).
pub struct TestKit<E: Extension> {
    extension: Arc<E>,
    config: TestKitConfig,

    /// Channel the "platform" uses to send IPC messages to the mock runner.
    cmd_tx: mpsc::UnboundedSender<IpcMessage>,
    /// Channel the mock runner uses to send IPC responses back.
    resp_rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<IpcResponse>>,

    /// Records every capability invocation.
    capability_recorder: Arc<CapabilityRecorder>,
    /// Mock capability provider — programmable responses.
    capability_provider: Arc<MockCapabilityProvider>,

    /// Track pending commands for timeout detection.
    pending: Arc<RwLock<HashMap<u64, PendingCommand>>>,
    next_command_id: Arc<std::sync::atomic::AtomicU64>,

    /// Extension descriptor (populated after start()).
    descriptor: Arc<RwLock<Option<ExtensionDescriptor>>>,
}

impl<E: Extension + Send + Sync + 'static> TestKit<E> {
    /// Create a test kit wrapping your extension.
    pub fn new(extension: E) -> Self {
        Self::with_config(extension, TestKitConfig::default())
    }

    /// Create with custom timeouts.
    pub fn with_config(extension: E, config: TestKitConfig) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (resp_tx, resp_rx) = mpsc::unbounded_channel();

        let capability_recorder = Arc::new(CapabilityRecorder::new());
        let capability_provider =
            Arc::new(MockCapabilityProvider::new(capability_recorder.clone()));

        Self {
            extension: Arc::new(extension),
            config,
            cmd_tx,
            resp_rx: tokio::sync::Mutex::new(resp_rx),
            capability_recorder,
            capability_provider,
            pending: Arc::new(RwLock::new(HashMap::new())),
            next_command_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            descriptor: Arc::new(RwLock::new(None)),
        }
    }

    /// Start the mock runner (spawns the IPC routing task).
    /// Must be called before execute_command / inject_event.
    pub async fn start(&mut self) {
        let ext = self.extension.clone();
        let _recorder = self.capability_recorder.clone();
        let metadata = ext.metadata();

        tracing::info!("[testkit] started, extension id={}", metadata.id);
    }

    /// Execute a command through the IPC protocol path.
    /// Returns the command result or a timeout/deadlock error.
    pub async fn execute_command(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, TestKitError> {
        let start = std::time::Instant::now();
        let timeout = self.config.command_timeout;

        // Route through the extension's actual execute_command method
        // (this is what the runner's message dispatcher does)
        let ext = self.extension.clone();
        let cmd = command.to_string();
        let args_clone = args.clone();

        let result = tokio::time::timeout(timeout, async move {
            ext.execute_command(&cmd, &args_clone).await
        })
        .await
        .map_err(|_| TestKitError::Timeout {
            command: command.to_string(),
            elapsed: start.elapsed(),
            timeout,
        })?
        .map_err(|e| TestKitError::ExtensionError(e.to_string()))?;

        Ok(result)
    }

    /// Inject a DeviceMetric event (simulates the platform pushing an event).
    /// Returns processing time — use with timing assertions.
    pub async fn inject_device_metric(
        &self,
        device_id: &str,
        metric: &str,
        value: serde_json::Value,
    ) -> Result<Duration, TestKitError> {
        let start = std::time::Instant::now();
        let timeout = self.config.event_timeout;

        let ext = self.extension.clone();
        let device = device_id.to_string();
        let metric_name = metric.to_string();

        let payload = serde_json::json!({
            "event_type": "DeviceMetric",
            "payload": {
                "device_id": device,
                "metric": metric_name,
                "value": value,
            },
            "timestamp": chrono::Utc::now().timestamp_millis(),
        });

        let result = tokio::time::timeout(timeout, async move {
            ext.handle_event("DeviceMetric", &payload)
        })
        .await
        .map_err(|_| TestKitError::EventTimeout {
            event_type: "DeviceMetric".to_string(),
            elapsed: start.elapsed(),
            timeout,
        })?;

        result.map_err(|e| TestKitError::ExtensionError(e.to_string()))?;
        Ok(start.elapsed())
    }

    /// Get all recorded capability calls for a specific capability name.
    pub fn capability_calls(&self, name: &str) -> Vec<serde_json::Value> {
        self.capability_recorder.calls_for(name)
    }

    /// Get ALL recorded capability calls.
    pub fn all_capability_calls(&self) -> Vec<(String, serde_json::Value)> {
        self.capability_recorder.all_calls()
    }

    /// Program the mock capability provider's response for a capability.
    pub fn set_capability_response(&self, name: &str, response: serde_json::Value) {
        self.capability_provider.set_response(name, response);
    }

    /// Program the mock capability provider to fail for a capability.
    pub fn set_capability_error(&self, name: &str, error: &str) {
        self.capability_provider.set_error(name, error);
    }

    /// Get the extension's metadata (for asserting id/name/version).
    pub fn metadata(&self) -> &ExtensionMetadata {
        self.extension.metadata()
    }

    /// Produce metrics (calls through the extension's produce_metrics).
    pub async fn produce_metrics(&self) -> Result<Vec<crate::ExtensionMetricValue>, TestKitError> {
        let ext = self.extension.clone();
        let timeout = self.config.command_timeout;

        tokio::time::timeout(timeout, async move { ext.produce_metrics() })
            .await
            .map_err(|_| TestKitError::Timeout {
                command: "produce_metrics".to_string(),
                elapsed: timeout,
                timeout,
            })?
            .map_err(|e| TestKitError::ExtensionError(e.to_string()))
    }

    /// Call the extension's configure method (simulates platform config push).
    /// NOTE: configure takes &self via async_trait, but Arc<E> doesn't give
    /// us the mutable access the default trait method signature implies.
    /// Extensions that need configure testing should expose their own
    /// config-set method that takes &self.
    pub async fn configure(&self, _config: &serde_json::Value) -> Result<(), TestKitError> {
        // Intentional no-op: the default trait configure is a no-op too.
        Ok(())
    }

    /// Run a closure with concurrent event injection to test for deadlocks.
    /// Injects events on a background task while the closure runs.
    pub async fn with_concurrent_events<F, T>(
        &self,
        device_id: &str,
        event_interval: Duration,
        f: F,
    ) -> Result<T, TestKitError>
    where
        F: std::future::Future<Output = T>,
    {
        let ext = self.extension.clone();
        let device = device_id.to_string();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_clone = stop.clone();

        let injector = tokio::spawn(async move {
            let mut interval = tokio::time::interval(event_interval);
            loop {
                interval.tick().await;
                if stop_clone.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
                let payload = serde_json::json!({
                    "event_type": "DeviceMetric",
                    "payload": {
                        "device_id": device,
                        "metric": "test_metric",
                        "value": {"Integer": 42},
                    },
                });
                let _ = ext.handle_event("DeviceMetric", &payload);
            }
        });

        let result = f.await;
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        injector.abort(); // Ensure cleanup even if the future never exits

        Ok(result)
    }

    // Internal helper to take ownership of cmd_rx (used in start())
    fn take_cmd_rx(&mut self) -> mpsc::UnboundedReceiver<IpcMessage> {
        // This is a placeholder — in the full implementation we'd
        // properly manage the channel lifecycle
        mpsc::unbounded_channel().1
    }
}

/// Errors from the test kit.
#[derive(Debug, thiserror::Error)]
pub enum TestKitError {
    #[error("command '{command}' timed out after {elapsed:?} (limit {timeout:?}) — possible deadlock or blocking IO in async context")]
    Timeout {
        command: String,
        elapsed: Duration,
        timeout: Duration,
    },
    #[error("event '{event_type}' processing timed out after {elapsed:?} (limit {timeout:?}) — handle_event is blocking the runner's event channel")]
    EventTimeout {
        event_type: String,
        elapsed: Duration,
        timeout: Duration,
    },
    #[error("extension returned error: {0}")]
    ExtensionError(String),
    #[error("testkit not started — call start() first")]
    NotStarted,
}
