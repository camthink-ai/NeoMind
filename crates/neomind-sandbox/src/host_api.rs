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

/// HTTP request options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequestOptions {
    /// HTTP method (GET, POST, PUT, DELETE, etc.)
    pub method: String,
    /// Request URL
    pub url: String,
    /// Request headers
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Request body (for POST/PUT)
    #[serde(default)]
    pub body: Option<String>,
    /// Request timeout in milliseconds
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

fn default_timeout() -> u64 {
    30000 // 30 seconds default
}

/// HTTP response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    /// HTTP status code
    pub status: u16,
    /// Response headers
    pub headers: HashMap<String, String>,
    /// Response body
    pub body: String,
    /// Whether the request was successful (2xx status)
    pub ok: bool,
}

/// The host API available to WASM modules.
pub struct HostApi {
    /// Shared state for the host API.
    state: Arc<RwLock<HostApiState>>,
    /// HTTP client for making requests.
    http_client: reqwest::Client,
}

/// Internal state for the host API.
#[derive(Debug, Default)]
struct HostApiState {
    /// Device data storage (simulated).
    devices: HashMap<String, DeviceData>,

    /// Rule execution history (reserved for future use).
    #[allow(dead_code)]
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
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("NeoMind-WASM-Extension/0.5.8")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            state: Arc::new(RwLock::new(HostApiState::default())),
            http_client,
        }
    }

    /// Make an HTTP request.
    ///
    /// This allows WASM extensions to make HTTP requests through the host.
    /// The request is executed asynchronously.
    pub async fn http_request(&self, options: &HttpRequestOptions) -> HostApiResponse {
        let log = format!(
            "http_request: {} {}",
            options.method.to_uppercase(),
            options.url
        );
        tracing::info!("{}", log);

        // Build the request
        let mut request = match options.method.to_uppercase().as_str() {
            "GET" => self.http_client.get(&options.url),
            "POST" => self.http_client.post(&options.url),
            "PUT" => self.http_client.put(&options.url),
            "DELETE" => self.http_client.delete(&options.url),
            "PATCH" => self.http_client.patch(&options.url),
            "HEAD" => self.http_client.head(&options.url),
            method => {
                return HostApiResponse {
                    success: false,
                    data: serde_json::json!(null),
                    error: Some(format!("Unsupported HTTP method: {}", method)),
                };
            }
        };

        // Add headers
        for (key, value) in &options.headers {
            request = request.header(key, value);
        }

        // Add body if provided
        if let Some(body) = &options.body {
            request = request.body(body.clone());
        }

        // Set timeout
        request = request.timeout(std::time::Duration::from_millis(options.timeout_ms));

        // Execute the request
        match request.send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                let ok = response.status().is_success();

                // Extract headers
                let mut headers = HashMap::new();
                for (key, value) in response.headers() {
                    if let Ok(v) = value.to_str() {
                        headers.insert(key.to_string(), v.to_string());
                    }
                }

                // Get body
                let body = match response.text().await {
                    Ok(text) => text,
                    Err(e) => {
                        return HostApiResponse {
                            success: false,
                            data: serde_json::json!(null),
                            error: Some(format!("Failed to read response body: {}", e)),
                        };
                    }
                };

                let http_response = HttpResponse {
                    status,
                    headers,
                    body,
                    ok,
                };

                HostApiResponse {
                    success: true,
                    data: serde_json::to_value(http_response).unwrap_or(serde_json::json!(null)),
                    error: None,
                }
            }
            Err(e) => HostApiResponse {
                success: false,
                data: serde_json::json!(null),
                error: Some(format!("HTTP request failed: {}", e)),
            },
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
                    error: Some(format!(
                        "Metric '{}' not found on device '{}'",
                        metric, device_id
                    )),
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
    pub async fn register_device(
        &self,
        device_id: &str,
        metrics: HashMap<String, f64>,
    ) -> HostApiResponse {
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
