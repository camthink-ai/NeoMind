//! HTTP device adapter for NeoMind event-driven architecture.
//!
//! This adapter polls HTTP endpoints for device telemetry and sends commands
//! via HTTP requests. It's useful for REST APIs, IoT services, and webhooks.
//!
//! ## Features
//!
//! - Polling device data from HTTP endpoints
//! - Sending commands via HTTP POST/PUT
//! - JSON and form-data payload support
//! - Bearer token and Basic authentication
//! - Configurable polling intervals
//! - Multiple devices per adapter
//! - Dynamic device add/remove (like MQTT)
//!
//! ## Configuration
//!
//! ```toml
//! [[devices.http_devices]]
//! id = "temperature-sensor-1"
//! name = "Living Room Temperature"
//! url = "http://192.168.1.100/api/telemetry"
//! poll_interval = 30  # seconds
//! method = "GET"
//! headers = { "Authorization": "Bearer token123" }
//! data_path = "$.temperature"  # JSONPath to extract value
//! ```

use crate::adapter::{
    AdapterError, AdapterResult, ConnectionStatus, DeviceAdapter, DeviceEvent,
};
use crate::mdl::MetricValue;
use crate::registry::DeviceRegistry;
use crate::telemetry::TimeSeriesStorage;
use crate::unified_extractor::UnifiedExtractor;
use async_trait::async_trait;
use neomind_core::EventBus;
use futures::Stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, broadcast};
use tokio::time::Instant;
use tracing::{info, warn};

/// HTTP device configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpDeviceConfig {
    /// Unique device identifier
    pub id: String,
    /// Human-readable device name
    pub name: String,
    /// HTTP endpoint URL for polling data
    pub url: String,
    /// HTTP method for polling (GET, POST)
    #[serde(default = "default_http_method")]
    pub method: String,
    /// Polling interval in seconds
    #[serde(default = "default_poll_interval")]
    pub poll_interval: u64,
    /// HTTP headers for authentication and content negotiation
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Request body (for POST requests)
    pub body: Option<String>,
    /// JSONPath to extract metric value (e.g., "$.data.temperature")
    pub data_path: Option<String>,
    /// Content type (json, form-data)
    #[serde(default = "default_content_type")]
    pub content_type: String,
    /// Timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    /// Device type (for template lookup)
    pub device_type: Option<String>,
    /// Command endpoint URL (if different from data URL)
    pub command_url: Option<String>,
    /// Command HTTP method (POST, PUT)
    #[serde(default = "default_command_method")]
    pub command_method: String,
}

fn default_http_method() -> String {
    "GET".to_string()
}

fn default_poll_interval() -> u64 {
    60
}

fn default_content_type() -> String {
    "json".to_string()
}

fn default_timeout() -> u64 {
    10
}

fn default_command_method() -> String {
    "POST".to_string()
}

/// HTTP adapter configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpAdapterConfig {
    /// Adapter name
    pub name: String,
    /// HTTP devices to poll
    pub devices: Vec<HttpDeviceConfig>,
    /// Global HTTP headers (applied to all requests)
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Global authentication token (Bearer)
    pub auth_token: Option<String>,
    /// Global timeout override
    pub global_timeout: Option<u64>,
    /// Storage directory for persistence
    pub storage_dir: Option<String>,
}

impl HttpAdapterConfig {
    /// Create a new HTTP adapter configuration.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            devices: Vec::new(),
            headers: HashMap::new(),
            auth_token: None,
            global_timeout: None,
            storage_dir: None,
        }
    }

    /// Add a device to poll.
    pub fn with_device(mut self, device: HttpDeviceConfig) -> Self {
        self.devices.push(device);
        self
    }

    /// Add a global header.
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    /// Set authentication token.
    pub fn with_auth(mut self, token: impl Into<String>) -> Self {
        self.auth_token = Some(token.into());
        self
    }
}

/// HTTP polling task state.
struct HttpPollingTask {
    device_id: String,
    config: HttpDeviceConfig,
    last_poll: Instant,
    next_poll: Instant,
    /// Error counter (reserved for future retry logic).
    #[allow(dead_code)]
    error_count: u32,
    is_running: bool,
}

/// HTTP device adapter.
///
/// This adapter polls HTTP endpoints for device telemetry and supports
/// sending commands via HTTP POST/PUT requests.
pub struct HttpAdapter {
    /// Adapter name
    name: String,
    /// Configuration
    config: HttpAdapterConfig,
    /// Event bus
    event_bus: Option<Arc<EventBus>>,
    /// Device registry
    device_registry: Arc<DeviceRegistry>,
    /// Event channel
    event_tx: broadcast::Sender<DeviceEvent>,
    /// Running state
    running: Arc<RwLock<bool>>,
    /// HTTP client
    client: Client,
    /// Polling tasks
    polling_tasks: Arc<RwLock<Vec<HttpPollingTask>>>,
    /// Telemetry storage
    telemetry_storage: Arc<RwLock<Option<Arc<TimeSeriesStorage>>>>,
    /// Unified data extractor
    extractor: Arc<UnifiedExtractor>,
}

impl HttpAdapter {
    /// Create a new HTTP adapter.
    pub fn new(
        config: HttpAdapterConfig,
        event_bus: Option<Arc<EventBus>>,
        device_registry: Arc<DeviceRegistry>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(1000);

        // Build HTTP client with default settings
        let client = Client::builder()
            .pool_max_idle_per_host(0)
            .build()
            .unwrap_or_default();

        let extractor = Arc::new(UnifiedExtractor::new(device_registry.clone()));

        Self {
            name: config.name.clone(),
            config,
            event_bus,
            device_registry,
            event_tx,
            running: Arc::new(RwLock::new(false)),
            client,
            polling_tasks: Arc::new(RwLock::new(Vec::new())),
            telemetry_storage: Arc::new(RwLock::new(None)),
            extractor,
        }
    }

    /// Initialize polling tasks from config.
    fn init_polling_tasks(&self) -> Vec<HttpPollingTask> {
        let now = Instant::now();
        self.config
            .devices
            .iter()
            .map(|device| HttpPollingTask {
                device_id: device.id.clone(),
                config: device.clone(),
                last_poll: now,
                next_poll: now + Duration::from_secs(device.poll_interval),
                error_count: 0,
                is_running: true,
            })
            .collect()
    }

    /// Poll a single device for data.
    async fn poll_device(&self, task: &HttpPollingTask) -> AdapterResult<Vec<DeviceEvent>> {
        let device = &task.config;

        // Build request
        let mut request = match device.method.as_str() {
            "GET" => self.client.get(&device.url),
            "POST" => {
                let mut req = self.client.post(&device.url);
                if let Some(body) = &device.body {
                    req = req.body(body.clone());
                }
                req
            }
            _ => {
                return Err(AdapterError::Configuration(format!(
                    "Unsupported HTTP method: {}",
                    device.method
                )))
            }
        };

        // Add timeout
        let timeout_secs = self.config.global_timeout.unwrap_or(device.timeout);
        request = request.timeout(Duration::from_secs(timeout_secs));

        // Add headers
        for (key, value) in &self.config.headers {
            request = request.header(key, value);
        }
        for (key, value) in &device.headers {
            request = request.header(key, value);
        }

        // Add auth token if configured
        if let Some(token) = &self.config.auth_token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        // Send request
        let response = request
            .send()
            .await
            .map_err(|e| AdapterError::Connection(format!("HTTP request failed: {}", e)))?;

        // Check status
        if !response.status().is_success() {
            return Err(AdapterError::Communication(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        // Parse response
        let events = self.parse_response(device, response).await?;

        Ok(events)
    }

    /// Parse HTTP response into device events.
    async fn parse_response(
        &self,
        device: &HttpDeviceConfig,
        response: reqwest::Response,
    ) -> AdapterResult<Vec<DeviceEvent>> {
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/json");

        let timestamp = chrono::Utc::now().timestamp();
        let mut events = Vec::new();

        if content_type.contains("json") {
            let json: Value = response
                .json()
                .await
                .map_err(|e| AdapterError::Communication(format!("JSON parse error: {}", e)))?;

            // Extract data using data_path if specified (for backward compatibility)
            let data = if let Some(path) = &device.data_path {
                // Convert JSONPath-style path to dot notation for UnifiedExtractor
                // $.data.temperature -> data.temperature
                let dot_path = path.replace("$.", "").replace("$.", "");
                match self.extractor.extract_by_path(&json, &dot_path, 0)
                    .map_err(|e| AdapterError::Communication(format!("Path extraction failed: {}", e)))? {
                    Some(v) => v,
                    None => {
                        return Err(AdapterError::Communication(format!(
                            "Data path '{}' not found in response",
                            path
                        )))
                    }
                }
            } else {
                json
            };

            // Use UnifiedExtractor for consistent metric extraction
            // Get device type from config, default to device ID if not set
            let device_type = device.device_type.as_deref().unwrap_or("http");

            // For HTTP polling, we extract from the data section
            let result = self.extractor.extract(&device.id, device_type, &data).await;

            // Convert extracted metrics to device events
            for metric in result.metrics {
                events.push(DeviceEvent::Metric {
                    device_id: device.id.clone(),
                    metric: metric.name,
                    value: metric.value,
                    timestamp,
                });
            }
        } else {
            // Non-JSON response - treat as single string value
            let text = response
                .text()
                .await
                .map_err(|e| AdapterError::Communication(format!("Text read error: {}", e)))?;

            events.push(DeviceEvent::Metric {
                device_id: device.id.clone(),
                metric: "value".to_string(),
                value: MetricValue::String(text),
                timestamp,
            });
        }

        Ok(events)
    }

    /// Run the polling loop.
    async fn polling_loop(self: Arc<Self>) {
        let running = self.running.clone();
        let tasks = self.polling_tasks.clone();

        while *running.read().await {
            let mut task_guard = tasks.write().await;
            let mut tasks_to_poll = Vec::new();
            let now = Instant::now();

            // Find tasks that need polling
            for task in task_guard.iter_mut() {
                if !task.is_running {
                    continue;
                }

                if now >= task.next_poll {
                    tasks_to_poll.push((task.device_id.clone(), task.config.clone()));
                    task.last_poll = now;
                    task.next_poll = now + Duration::from_secs(task.config.poll_interval);
                }
            }
            drop(task_guard);

            // Poll devices
            for (device_id, config) in tasks_to_poll {
                let adapter = Arc::clone(&self);
                let event_bus = self.event_bus.clone();
                let event_tx = self.event_tx.clone();
                let storage_guard = self.telemetry_storage.read().await;
                let telemetry_storage = storage_guard.as_ref().cloned();

                tokio::spawn(async move {
                    match adapter.poll_device(&HttpPollingTask {
                        device_id: device_id.clone(),
                        config: config.clone(),
                        last_poll: Instant::now(),
                        next_poll: Instant::now(),
                        error_count: 0,
                        is_running: true,
                    }).await {
                        Ok(events) => {
                            for event in events {
                                // Publish to event channel
                                let _ = event_tx.send(event.clone());

                                // Publish to event bus
                                if let Some(eb) = &event_bus {
                                    let neomind_event = event.clone().to_neomind_event();
                                    eb.publish(neomind_event).await;
                                }

                                // Write to telemetry storage if available
                                if let Some(storage) = &telemetry_storage
                                    && let DeviceEvent::Metric {
                                        device_id,
                                        metric,
                                        value,
                                        timestamp,
                                    } = event
                                    {
                                        use crate::telemetry::DataPoint;
                                        let data_point = DataPoint {
                                            timestamp,
                                            value: value.clone(),
                                            quality: None,
                                        };
                                        let _ = storage.write(&device_id, &metric, data_point).await;
                                    }
                            }
                        }
                        Err(e) => {
                            warn!(device_id = %device_id, "HTTP polling error: {}", e);
                        }
                    }
                });
            }

            // Sleep before next check
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

#[async_trait]
impl DeviceAdapter for HttpAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn adapter_type(&self) -> &'static str {
        "http"
    }

    fn is_running(&self) -> bool {
        // Use try_read to avoid blocking in async runtime
        self.running
            .try_read()
            .map(|r| *r)
            .unwrap_or(false)
    }

    async fn start(&self) -> AdapterResult<()> {
        let mut running = self.running.write().await;
        if *running {
            return Ok(());
        }
        *running = true;

        // Initialize polling tasks
        let mut tasks = self.polling_tasks.write().await;
        *tasks = self.init_polling_tasks();
        drop(tasks);

        info!("HTTP adapter '{}' started with {} devices", self.name, self.config.devices.len());

        // Start polling loop - spawn a background task for polling
        // We need to clone Arc references to use in the spawned task
        let adapter = Arc::new(self.clone());
        tokio::spawn(async move {
            adapter.polling_loop().await;
        });

        Ok(())
    }

    async fn stop(&self) -> AdapterResult<()> {
        let mut running = self.running.write().await;
        *running = false;

        // Stop all polling tasks
        let mut tasks = self.polling_tasks.write().await;
        for task in tasks.iter_mut() {
            task.is_running = false;
        }

        info!("HTTP adapter '{}' stopped", self.name);
        Ok(())
    }

    fn subscribe(&self) -> Pin<Box<dyn Stream<Item = DeviceEvent> + Send + '_>> {
        let rx = self.event_tx.subscribe();
        Box::pin(async_stream::stream! {
            let mut rx = rx;
            while let Ok(event) = rx.recv().await {
                yield event;
            }
        })
    }

    fn set_telemetry_storage(&self, storage: Arc<TimeSeriesStorage>) {
        // Spawn a task to set the telemetry storage asynchronously
        let telemetry_storage = self.telemetry_storage.clone();
        tokio::spawn(async move {
            *telemetry_storage.write().await = Some(storage);
        });
    }

    fn device_count(&self) -> usize {
        self.config.devices.len()
    }

    fn list_devices(&self) -> Vec<String> {
        self.config.devices.iter().map(|d| d.id.clone()).collect()
    }

    async fn send_command(
        &self,
        device_id: &str,
        _command_name: &str,
        payload: String,
        _topic: Option<String>,
    ) -> AdapterResult<()> {
        // Find device config
        let device = self
            .config
            .devices
            .iter()
            .find(|d| d.id == device_id)
            .ok_or_else(|| AdapterError::DeviceNotFound(device_id.to_string()))?;

        // Use command_url if specified, otherwise use device URL
        let url = device
            .command_url
            .as_ref()
            .unwrap_or(&device.url);

        // Build request
        let mut request = match device.command_method.as_str() {
            "POST" => self.client.post(url),
            "PUT" => self.client.put(url),
            _ => {
                return Err(AdapterError::Configuration(format!(
                    "Unsupported command method: {}",
                    device.command_method
                )))
            }
        };

        // Add timeout
        let timeout_secs = self.config.global_timeout.unwrap_or(device.timeout);
        request = request.timeout(Duration::from_secs(timeout_secs));

        // Add headers
        for (key, value) in &self.config.headers {
            request = request.header(key, value);
        }
        for (key, value) in &device.headers {
            request = request.header(key, value);
        }

        // Add auth token if configured
        if let Some(token) = &self.config.auth_token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        // Set content type
        if device.content_type == "json" {
            request = request.header("Content-Type", "application/json");
        }

        // Send command
        let response = request
            .body(payload.clone())
            .send()
            .await
            .map_err(|e| AdapterError::Communication(format!("HTTP command failed: {}", e)))?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(AdapterError::Communication(format!(
                "HTTP command error: {}",
                response.status()
            )))
        }
    }

    fn connection_status(&self) -> ConnectionStatus {
        if self.is_running() {
            ConnectionStatus::Connected
        } else {
            ConnectionStatus::Disconnected
        }
    }

    async fn subscribe_device(&self, device_id: &str) -> AdapterResult<()> {
        // Get device config from registry
        let device_opt = self.device_registry.get_device(device_id).await;
        if let Some(device) = device_opt {
            // Extract HTTP-specific config from connection_config.extra
            let cc = &device.connection_config;

            let url = cc.extra.get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AdapterError::Configuration("Missing url in connection_config".to_string()))?;

            let method = cc.extra.get("method")
                .and_then(|v| v.as_str())
                .unwrap_or("GET")
                .to_string();

            let poll_interval = cc.extra.get("poll_interval")
                .and_then(|v| v.as_u64())
                .unwrap_or(60);

            let data_path = cc.extra.get("data_path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let timeout = cc.extra.get("timeout")
                .and_then(|v| v.as_u64())
                .unwrap_or(10);

            let device_name = device.name.clone();

            // Build device config
            let device_config = HttpDeviceConfig {
                id: device_id.to_string(),
                name: device_name,
                url: url.to_string(),
                method,
                poll_interval,
                headers: HashMap::new(),
                body: None,
                data_path,
                content_type: "json".to_string(),
                timeout,
                device_type: Some(device.device_type.clone()),
                command_url: cc.extra.get("command_url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                command_method: cc.extra.get("command_method")
                    .and_then(|v| v.as_str())
                    .unwrap_or("POST")
                    .to_string(),
            };

            // Add to polling tasks
            let mut tasks = self.polling_tasks.write().await;
            // Check if already exists
            if tasks.iter().any(|t| t.device_id == device_id) {
                return Ok(());
            }

            let task = HttpPollingTask {
                device_id: device_id.to_string(),
                config: device_config,
                last_poll: Instant::now(),
                next_poll: Instant::now(),
                error_count: 0,
                is_running: true,
            };
            tasks.push(task);

            info!("HTTP adapter: subscribed to device '{}'", device_id);
        }
        Ok(())
    }

    async fn unsubscribe_device(&self, device_id: &str) -> AdapterResult<()> {
        let mut tasks = self.polling_tasks.write().await;
        tasks.retain(|t| t.device_id != device_id);
        info!("HTTP adapter: unsubscribed from device '{}'", device_id);
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// Implement Clone for HttpAdapter
impl Clone for HttpAdapter {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            config: self.config.clone(),
            event_bus: self.event_bus.clone(),
            device_registry: Arc::clone(&self.device_registry),
            event_tx: self.event_tx.clone(),
            running: Arc::clone(&self.running),
            client: self.client.clone(),
            polling_tasks: Arc::clone(&self.polling_tasks),
            telemetry_storage: Arc::clone(&self.telemetry_storage),
            extractor: Arc::clone(&self.extractor),
        }
    }
}

/// Create an HTTP adapter from configuration.
pub fn create_http_adapter(
    config: HttpAdapterConfig,
    event_bus: &EventBus,
    device_registry: Arc<DeviceRegistry>,
) -> Arc<dyn DeviceAdapter> {
    // Convert &EventBus to Arc<EventBus>
    let event_bus_arc = Arc::new(event_bus.clone());

    Arc::new(HttpAdapter::new(config, Some(event_bus_arc), device_registry))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_http_config_defaults() {
        let config = HttpAdapterConfig::new("test");
        assert_eq!(config.name, "test");
        assert!(config.devices.is_empty());
        assert!(config.headers.is_empty());
        assert!(config.auth_token.is_none());
    }

    #[test]
    fn test_http_config_builder() {
        let config = HttpAdapterConfig::new("test")
            .with_header("X-API-Key", "test123")
            .with_auth("token456");

        assert_eq!(config.headers.get("X-API-Key"), Some(&"test123".to_string()));
        assert_eq!(config.auth_token, Some("token456".to_string()));
    }

    #[test]
    fn test_unified_extractor_dot_notation() {
        let adapter = HttpAdapter::new(
            HttpAdapterConfig::new("test"),
            None,
            Arc::new(DeviceRegistry::new()),
        );

        let json = json!({
            "data": {
                "temperature": 25.5,
                "humidity": 60
            }
        });

        // Test dot notation extraction (UnifiedExtractor)
        let result = adapter.extractor.extract_by_path(&json, "data.temperature", 0);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(json!(25.5)));

        let result = adapter.extractor.extract_by_path(&json, "data.humidity", 0);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(json!(60)));
    }
}
