//! Host-side state for WASM execution: `HostState` (per-store WASI ctx +
//! memory + IPC client + resource limits), the synchronous IPC client, and
//! the per-store linear-memory cap. Split from main.rs 2026-09 — the runner
//! binary was a 3900-line monolith.

use neomind_extension_sdk::{IpcFrame, IpcResponse};
use serde_json::json;
use std::io::Write;
use std::sync::Arc;
use tracing::{debug, error, warn};
use wasmtime::{Memory, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::preview1::WasiP1Ctx;

use crate::ipc_routing::{get_pending_requests, register_pending_request};
use crate::STDOUT_WRITE_MUTEX;

/// Host state for WASM execution
pub(crate) struct HostState {
    pub(crate) wasi: WasiP1Ctx,
    pub(crate) memory: Option<Memory>,
    /// IPC client for capability invocation (communicates with main process)
    /// Uses sync channels for synchronous WASM host function calls
    pub(crate) ipc_client: Option<Arc<SyncIpcClient>>,
    /// Per-store resource limits — true cap on WASM linear-memory growth,
    /// enforced via `store.limiter()` at every `Store::new` site
    pub(crate) limits: StoreLimits,
}

/// Synchronous IPC client for capability invocation
///
/// This client sends CapabilityRequest messages via stdout and waits for
/// CapabilityResult responses via the pending requests queue (routed by main loop).
pub struct SyncIpcClient {}

impl Default for SyncIpcClient {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncIpcClient {
    /// Create a new sync IPC client
    pub fn new() -> Self {
        Self {}
    }

    /// Invoke a capability synchronously
    ///
    /// Sends CapabilityRequest via stdout and waits for CapabilityResult
    /// via the pending requests queue (routed by main loop).
    pub fn invoke(&self, capability: &str, params: &serde_json::Value) -> serde_json::Value {
        use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

        static REQUEST_ID_COUNTER: AtomicU64 = AtomicU64::new(1);
        let request_id = REQUEST_ID_COUNTER.fetch_add(1, AtomicOrdering::SeqCst);

        debug!(capability, request_id, "SyncIpcClient invoke");

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
                error!("SyncIpcClient: failed to serialize request: {e}");
                // Remove pending request on error
                get_pending_requests().remove(&request_id);
                return json!({"success": false, "error": format!("Failed to serialize request: {}", e)});
            }
        };

        let frame = IpcFrame::new(payload);
        let bytes = frame.encode();

        // Write to stdout (protected by global mutex)
        {
            let _guard = STDOUT_WRITE_MUTEX.lock().unwrap_or_else(|e| {
                error!("STDOUT_WRITE_MUTEX poisoned: {}", e);
                e.into_inner()
            });
            let mut stdout = std::io::stdout();
            if let Err(e) = stdout.write_all(&bytes) {
                drop(_guard);
                error!("SyncIpcClient: failed to write request: {e}");
                get_pending_requests().remove(&request_id);
                return json!({"success": false, "error": format!("Failed to write request: {}", e)});
            }
            if let Err(e) = stdout.flush() {
                drop(_guard);
                error!("SyncIpcClient: failed to flush request: {e}");
                get_pending_requests().remove(&request_id);
                return json!({"success": false, "error": format!("Failed to flush request: {}", e)});
            }
        }

        debug!("SyncIpcClient: request sent, waiting for response");

        // Wait for response from the pending requests queue (with timeout)
        match response_rx.recv_timeout(std::time::Duration::from_secs(30)) {
            Ok(response) => {
                debug!("SyncIpcClient: received response");
                match response {
                    IpcResponse::CapabilityResult {
                        request_id: resp_id,
                        result,
                        error,
                    } => {
                        if resp_id != request_id {
                            warn!(expected = request_id, got = resp_id, "Request ID mismatch");
                            return json!({"success": false, "error": "Request ID mismatch"});
                        }
                        if let Some(err) = error {
                            json!({"success": false, "error": err})
                        } else {
                            result
                        }
                    }
                    _ => {
                        warn!("SyncIpcClient: unexpected response type");
                        json!({"success": false, "error": "Unexpected response type".to_string()})
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                error!("SyncIpcClient: timeout waiting for response");
                get_pending_requests().remove(&request_id);
                json!({"success": false, "error": "Timeout waiting for response"})
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                error!("SyncIpcClient: response channel disconnected");
                get_pending_requests().remove(&request_id);
                json!({"success": false, "error": "Response channel disconnected"})
            }
        }
    }
}

/// Per-store WASM resource limits: a hard cap on linear-memory growth
/// (`memory.grow` beyond it fails), default 256 MB per store, override via
/// `NEOMIND_WASM_MEMORY_MB` (same name and default as the pre-wasmtime-36 code).
pub(crate) fn wasm_store_limits() -> StoreLimits {
    let max_mb = std::env::var("NEOMIND_WASM_MEMORY_MB")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(256);
    StoreLimitsBuilder::new()
        .memory_size(max_mb as usize * 1024 * 1024)
        .build()
}

impl HostState {
    /// Create a new host state with WASI context
    pub(crate) fn new(wasi: WasiP1Ctx) -> Self {
        Self {
            wasi,
            memory: None,
            ipc_client: None,
            limits: wasm_store_limits(),
        }
    }

    /// Create host state with IPC capability client
    pub(crate) fn with_ipc(wasi: WasiP1Ctx, ipc_client: Option<Arc<SyncIpcClient>>) -> Self {
        Self {
            wasi,
            memory: None,
            ipc_client,
            limits: wasm_store_limits(),
        }
    }
}
