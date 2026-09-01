//! Mock Capability Provider — records every capability invocation and
//! allows programming responses or failures for testing.
//!
//! In production, capability calls from extensions travel: extension →
//! FFI callback → runner → platform → handler → response → runner → FFI
//! return → extension. This mock collapses that to an in-process call
//! that records parameters and returns programmed responses.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;
use serde_json::Value;

/// A single recorded capability call.
#[derive(Debug, Clone)]
pub struct RecordedCall {
    /// The capability name (e.g., "device_metrics_write").
    pub capability: String,
    /// The parameters passed by the extension.
    pub params: Value,
    /// Wall-clock timestamp of the call.
    pub timestamp_ms: i64,
    /// Monotonic sequence number (for ordering assertions).
    pub sequence: u64,
}

/// Thread-safe recorder — share via Arc.
pub struct CapabilityRecorder {
    calls: RwLock<Vec<RecordedCall>>,
    next_sequence: AtomicU64,
}

impl CapabilityRecorder {
    pub fn new() -> Self {
        Self {
            calls: RwLock::new(Vec::new()),
            next_sequence: AtomicU64::new(1),
        }
    }

    pub fn record(&self, capability: &str, params: &Value) {
        let call = RecordedCall {
            capability: capability.to_string(),
            params: params.clone(),
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            sequence: self.next_sequence.fetch_add(1, Ordering::SeqCst),
        };
        self.calls.write().push(call);
    }

    /// All calls for a specific capability, in sequence order.
    pub fn calls_for(&self, capability: &str) -> Vec<Value> {
        self.calls
            .read()
            .iter()
            .filter(|c| c.capability == capability)
            .map(|c| c.params.clone())
            .collect()
    }

    /// All calls with their capability names.
    pub fn all_calls(&self) -> Vec<(String, Value)> {
        self.calls
            .read()
            .iter()
            .map(|c| (c.capability.clone(), c.params.clone()))
            .collect()
    }

    /// Total call count.
    pub fn call_count(&self) -> usize {
        self.calls.read().len()
    }

    /// Call count for a specific capability.
    pub fn count_for(&self, capability: &str) -> usize {
        self.calls.read().iter().filter(|c| c.capability == capability).count()
    }

    /// Clear recorded calls (useful between test phases).
    pub fn clear(&self) {
        self.calls.write().clear();
    }
}

impl Default for CapabilityRecorder {
    fn default() -> Self {
        Self::new()
    }
}

/// Mock capability provider — programmable responses.
pub struct MockCapabilityProvider {
    recorder: std::sync::Arc<CapabilityRecorder>,
    responses: RwLock<HashMap<String, Result<Value, String>>>,
}

impl MockCapabilityProvider {
    pub fn new(recorder: std::sync::Arc<CapabilityRecorder>) -> Self {
        Self {
            recorder,
            responses: RwLock::new(HashMap::new()),
        }
    }

    /// Program a successful response for a capability.
    pub fn set_response(&self, capability: &str, response: Value) {
        self.responses
            .write()
            .insert(capability.to_string(), Ok(response));
    }

    /// Program an error response for a capability.
    pub fn set_error(&self, capability: &str, error: &str) {
        self.responses
            .write()
            .insert(capability.to_string(), Err(error.to_string()));
    }

    /// Handle a capability invocation (record + return programmed response).
    pub fn invoke(&self, capability: &str, params: &Value) -> Result<Value, String> {
        self.recorder.record(capability, params);

        let responses = self.responses.read();
        match responses.get(capability) {
            Some(Ok(v)) => Ok(v.clone()),
            Some(Err(e)) => Err(e.clone()),
            None => {
                // Default: success with echo of params (common pattern)
                Ok(serde_json::json!({"success": true, "result": params}))
            }
        }
    }
}
