//! WASM-specific type definitions
//!
//! This module provides types for WASM extensions that don't have
//! access to the full neomind-core crate.

use serde::{Deserialize, Serialize};
use std::cell::RefCell;

// Re-export extension types
pub use crate::extension::*;

/// Host API for WASM extensions
pub struct Host;

impl Host {
    /// Make an HTTP request
    pub fn http_request(method: &str, url: &str) -> Result<serde_json::Value, String> {
        super::bindings::http_request(method, url)
    }

    /// Log a message
    pub fn log(level: &str, message: &str) {
        super::bindings::log(level, message)
    }

    /// Store a metric value
    pub fn store_metric(name: &str, value: &serde_json::Value) {
        super::bindings::store_metric(name, value)
    }

    /// Read a device metric
    pub fn device_read(device_id: &str, metric: &str) -> Result<serde_json::Value, String> {
        super::bindings::device_read(device_id, metric)
    }

    /// Write to a device
    pub fn device_write(
        device_id: &str,
        command: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        super::bindings::device_write(device_id, command, params)
    }
}

/// Thread-local metric storage for WASM extensions.
///
/// Using thread-local storage instead of `static mut` is safe because:
/// 1. Each WASM instance runs in its own thread
/// 2. No data races between threads
/// 3. Memory is automatically cleaned up when the thread exits
thread_local! {
    static WASM_METRIC_CACHE: RefCell<std::collections::HashMap<String, SdkMetricValue>> =
        RefCell::new(std::collections::HashMap::new());
}

/// Store a metric value in WASM cache
pub fn store_metric_value<T: Into<SdkMetricValue>>(name: &str, value: T) {
    let metric_value = value.into();
    WASM_METRIC_CACHE.with(|cache| {
        cache.borrow_mut().insert(name.to_string(), metric_value);
    });
}

/// Initialize the metric cache (now a no-op, kept for API compatibility)
pub fn init_metric_cache() {
    // Thread-local storage is automatically initialized
    // This function is kept for backward compatibility
}

/// Clear the metric cache
pub fn clear_metric_cache() {
    WASM_METRIC_CACHE.with(|cache| {
        cache.borrow_mut().clear();
    });
}

/// Get all cached metrics as JSON
pub fn get_cached_metrics_json() -> String {
    WASM_METRIC_CACHE.with(|cache| {
        let cache = cache.borrow();
        let metrics: Vec<_> = cache
            .iter()
            .map(|(k, v)| {
                serde_json::json!({
                    "name": k,
                    "value": v,
                    "timestamp": 0
                })
            })
            .collect();
        serde_json::to_string(&metrics).unwrap_or_else(|_| "[]".to_string())
    })
}

/// Get the number of cached metrics (for testing/debugging)
pub fn cached_metrics_count() -> usize {
    WASM_METRIC_CACHE.with(|cache| cache.borrow().len())
}
