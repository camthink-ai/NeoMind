//! Host API exposed to WASM modules.
//!
//! This module defines the functions that are exposed to WASM modules
//! for safe interaction with the host system.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;

/// Host API response type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostApiResponse {
    /// Whether the call was successful.
    pub success: bool,

    /// Response data.
    pub data: serde_json::Value,

    /// Error message if the call failed.
    pub error: Option<String>,
}

/// The host API available to WASM modules.
pub struct HostApi {
    /// Shared state for the host API.
    state: Arc<RwLock<HostApiState>>,
}

/// Internal state for the host API.
#[derive(Debug, Default)]
struct HostApiState {
    /// Device data storage (simulated).
    devices: HashMap<String, DeviceData>,

    /// Rule execution history.
    rule_history: Vec<RuleExecution>,

    /// Logs from sandboxed modules.
    logs: Vec<String>,
}

/// Simulated device data.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeviceData {
    id: String,
    metrics: HashMap<String, f64>,
    last_updated: u64,
}

/// Record of a rule execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuleExecution {
    rule_name: String,
    timestamp: u64,
    result: bool,
    output: String,
}

impl HostApi {
    /// Create a new host API.
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(HostApiState::default())),
        }
    }

    /// Read a device metric.
    pub async fn device_read(&self, device_id: &str, metric: &str) -> HostApiResponse {
        let state = self.state.read().await;
        let log = format!("device_read: device={}, metric={}", device_id, metric);
        tracing::info!("{}", log);

        // Check if device exists
        if let Some(device) = state.devices.get(device_id) {
            if let Some(value) = device.metrics.get(metric) {
                HostApiResponse {
                    success: true,
                    data: serde_json::json!({ "value": value }),
                    error: None,
                }
            } else {
                HostApiResponse {
                    success: false,
                    data: serde_json::json!(null),
                    error: Some(format!("Metric '{}' not found on device '{}'", metric, device_id)),
                }
            }
        } else {
            // Return mock data for devices that don't exist yet
            HostApiResponse {
                success: true,
                data: serde_json::json!({ "value": 20.0 }),
                error: None,
            }
        }
    }

    /// Write a command to a device.
    pub async fn device_write(
        &self,
        device_id: &str,
        command: &str,
        params: &Value,
    ) -> HostApiResponse {
        let log = format!(
            "device_write: device={}, command={}, params={}",
            device_id, command, params
        );
        tracing::info!("{}", log);

        HostApiResponse {
            success: true,
            data: serde_json::json!({ "executed": true }),
            error: None,
        }
    }

    /// Log a message from the sandboxed module.
    pub async fn log(&self, level: &str, message: &str) -> HostApiResponse {
        let log = format!("{}: {}", level, message);
        tracing::info!("{}", log);

        let mut state = self.state.write().await;
        state.logs.push(log);

        HostApiResponse {
            success: true,
            data: serde_json::json!(null),
            error: None,
        }
    }

    /// Send a notification.
    pub async fn notify(&self, message: &str) -> HostApiResponse {
        let log = format!("NOTIFY: {}", message);
        tracing::info!("{}", log);

        let mut state = self.state.write().await;
        state.logs.push(log);

        HostApiResponse {
            success: true,
            data: serde_json::json!({ "sent": true }),
            error: None,
        }
    }

    /// Query data (SQL-like interface).
    pub async fn query_data(&self, query: &str) -> HostApiResponse {
        let log = format!("query_data: {}", query);
        tracing::info!("{}", log);

        // Mock response for now
        HostApiResponse {
            success: true,
            data: serde_json::json!({
                "results": []
            }),
            error: None,
        }
    }

    /// Register a device.
    pub async fn register_device(&self, device_id: &str, metrics: HashMap<String, f64>) -> HostApiResponse {
        let mut state = self.state.write().await;
        let device = DeviceData {
            id: device_id.to_string(),
            metrics,
            last_updated: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };
        state.devices.insert(device_id.to_string(), device);

        tracing::info!("Registered device: {}", device_id);

        HostApiResponse {
            success: true,
            data: serde_json::json!({ "registered": true }),
            error: None,
        }
    }

    /// Get all logs from the host API.
    pub async fn get_logs(&self) -> Vec<String> {
        let state = self.state.read().await;
        state.logs.clone()
    }

    /// Clear all logs.
    pub async fn clear_logs(&self) {
        let mut state = self.state.write().await;
        state.logs.clear();
    }
}

impl Default for HostApi {
    fn default() -> Self {
        Self::new()
    }
}
