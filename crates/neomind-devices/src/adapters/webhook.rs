//! Webhook device adapter for NeoMind event-driven architecture.
//!
//! This adapter receives device data via HTTP POST (webhook) from devices
//! that actively push data instead of being polled.
//!
//! ## Features
//!
//! - Passive data reception via webhook endpoint
//! - Device authentication support (API keys)
//! - IP whitelist/blacklist
//! - Request rate limiting
//! - Automatic device discovery
//! - Command support (via response body)
//!
//! ## Webhook URL Format
//!
//! ```text
//! POST /api/devices/webhook/{device_id}
//! ```
//!
//! ## Payload Format
//!
//! ```json
//! {
//!   "timestamp": 1234567890,
//!   "quality": 1.0,
//!   "data": {
//!     "temperature": 23.5,
//!     "humidity": 65
//!   }
//! }
//! ```

use crate::adapter::{AdapterError, AdapterResult, ConnectionStatus, DeviceAdapter, DeviceEvent};
use crate::mdl::MetricValue;
use crate::registry::DeviceRegistry;
use crate::telemetry::TimeSeriesStorage;
use crate::unified_extractor::UnifiedExtractor;
use async_trait::async_trait;
use futures::Stream;
use neomind_core::EventBus;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{info, warn};

/// Webhook device adapter configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookAdapterConfig {
    /// Adapter name
    pub name: String,
    /// API key for authentication (optional)
    pub api_key: Option<String>,
    /// Allowed IP addresses (whitelist)
    pub allowed_ips: Vec<String>,
    /// Blocked IP addresses (blacklist)
    pub blocked_ips: Vec<String>,
    /// Maximum requests per minute (rate limiting)
    pub rate_limit_per_minute: Option<u32>,
    /// Storage directory for persistence
    pub storage_dir: Option<String>,
}

impl WebhookAdapterConfig {
    /// Create a new webhook adapter configuration.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            api_key: None,
            allowed_ips: Vec::new(),
            blocked_ips: Vec::new(),
            rate_limit_per_minute: None,
            storage_dir: None,
        }
    }

    /// Set API key for authentication.
    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// Add an IP to the whitelist.
    pub fn with_allowed_ip(mut self, ip: impl Into<String>) -> Self {
        self.allowed_ips.push(ip.into());
        self
    }

    /// Add an IP to the blacklist.
    pub fn with_blocked_ip(mut self, ip: impl Into<String>) -> Self {
        self.blocked_ips.push(ip.into());
        self
    }

    /// Set rate limit.
    pub fn with_rate_limit(mut self, limit: u32) -> Self {
        self.rate_limit_per_minute = Some(limit);
        self
    }
}

impl Default for WebhookAdapterConfig {
    fn default() -> Self {
        Self::new("webhook")
    }
}

/// Webhook payload from device.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebhookPayload {
    /// Device ID (optional, can be from URL path)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    /// Timestamp (optional, will use server time if not provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
    /// Quality indicator (0-1, optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<f32>,
    /// Metrics data - can be any JSON structure
    pub data: Value,
}

/// Webhook device adapter.
///
/// This adapter is passive - it doesn't actively connect to devices.
/// Instead, it processes webhook requests and emits events.
pub struct WebhookAdapter {
    /// Adapter name
    name: String,
    /// Configuration
    config: WebhookAdapterConfig,
    /// Event bus
    event_bus: Option<Arc<EventBus>>,
    /// Device registry
    device_registry: Arc<DeviceRegistry>,
    /// Event channel
    event_tx: broadcast::Sender<DeviceEvent>,
    /// Running state
    running: Arc<RwLock<bool>>,
    /// Connected devices (devices that have sent data)
    devices: Arc<RwLock<Vec<String>>>,
    /// Telemetry storage
    telemetry_storage: Arc<RwLock<Option<Arc<TimeSeriesStorage>>>>,
    /// Request counter for rate limiting
    request_count: Arc<RwLock<HashMap<String, (u32, std::time::Instant)>>>,
    /// Unified data extractor
    extractor: Arc<UnifiedExtractor>,
}

impl WebhookAdapter {
    /// Create a new webhook adapter.
    pub fn new(
        config: WebhookAdapterConfig,
        event_bus: Option<Arc<EventBus>>,
        device_registry: Arc<DeviceRegistry>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(1000);

        let extractor = Arc::new(UnifiedExtractor::new(device_registry.clone()));

        Self {
            name: config.name.clone(),
            config,
            event_bus,
            device_registry,
            event_tx,
            running: Arc::new(RwLock::new(false)),
            devices: Arc::new(RwLock::new(Vec::new())),
            telemetry_storage: Arc::new(RwLock::new(None)),
            request_count: Arc::new(RwLock::new(HashMap::new())),
            extractor,
        }
    }

    /// Create a new webhook adapter with an event bus.
    pub fn with_event_bus(mut self, event_bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    /// Set event bus.
    pub fn set_event_bus(&mut self, event_bus: Arc<EventBus>) {
        self.event_bus = Some(event_bus);
    }

    /// Set telemetry storage.
    pub async fn set_telemetry_storage(&self, storage: Arc<TimeSeriesStorage>) {
        *self.telemetry_storage.write().await = Some(storage);
    }

    /// Set the device registry.
    pub fn with_device_registry(mut self, registry: Arc<DeviceRegistry>) -> Self {
        self.device_registry = registry;
        self
    }

    /// Validate an incoming webhook request.
    pub fn validate_request(
        &self,
        _device_id: &str,
        provided_api_key: Option<&str>,
        remote_ip: Option<&IpAddr>,
    ) -> AdapterResult<()> {
        // Check API key if configured
        if let Some(ref expected_key) = self.config.api_key {
            match provided_api_key {
                Some(key) if key == expected_key => {}
                Some(_) => return Err(AdapterError::Connection("Invalid API key".to_string())),
                None => return Err(AdapterError::Connection("Missing API key".to_string())),
            }
        }

        // Check IP blacklist
        if let Some(ip) = remote_ip {
            let ip_str = ip.to_string();
            if self.config.blocked_ips.contains(&ip_str) {
                return Err(AdapterError::Connection(format!(
                    "IP {} is blocked",
                    ip_str
                )));
            }
        }

        // Check IP whitelist (if configured)
        if !self.config.allowed_ips.is_empty() {
            if let Some(ip) = remote_ip {
                let ip_str = ip.to_string();
                if !self.config.allowed_ips.contains(&ip_str) {
                    return Err(AdapterError::Connection(format!(
                        "IP {} not in whitelist",
                        ip_str
                    )));
                }
            }
        }

        Ok(())
    }

    /// Check rate limit for a device.
    pub async fn check_rate_limit(&self, device_id: &str) -> AdapterResult<()> {
        if let Some(limit) = self.config.rate_limit_per_minute {
            let mut counts = self.request_count.write().await;
            let now = std::time::Instant::now();

            // Clean up old entries (older than 1 minute)
            counts.retain(|_, (_, timestamp)| now.duration_since(*timestamp).as_secs() < 60);

            let (count, _) = counts.entry(device_id.to_string()).or_insert((0, now));

            if *count >= limit {
                return Err(AdapterError::Connection("Rate limit exceeded".to_string()));
            }

            *count += 1;
        }

        Ok(())
    }

    /// Process a webhook payload and emit events.
    pub async fn process_webhook(
        &self,
        device_id: String,
        payload: WebhookPayload,
    ) -> AdapterResult<usize> {
        // Validate request
        self.validate_request(&device_id, None, None)?;

        // Check rate limit
        self.check_rate_limit(&device_id).await?;

        let timestamp = payload
            .timestamp
            .unwrap_or_else(|| chrono::Utc::now().timestamp());

        // Get device type from registry for template-driven extraction
        let device_type = self
            .device_registry
            .get_device(&device_id)
            .await
            .map(|d| d.device_type)
            .unwrap_or_else(|| "webhook".to_string());

        // Add device to tracking if not already present
        {
            let mut devices = self.devices.write().await;
            if !devices.contains(&device_id) {
                devices.push(device_id.clone());

                // Publish device online event
                if let Some(bus) = &self.event_bus {
                    use neomind_core::NeoMindEvent;

                    bus.publish(NeoMindEvent::DeviceOnline {
                        device_id: device_id.clone(),
                        device_type: device_type.clone(),
                        timestamp,
                    })
                    .await;
                }
            }
        }

        let mut metrics_count = 0;

        // Use UnifiedExtractor for consistent data processing
        let result = self
            .extractor
            .extract(&device_id, &device_type, &payload.data)
            .await;

        // Emit all extracted metrics
        for metric in result.metrics {
            self.emit_metric_event(device_id.clone(), metric.name, metric.value, timestamp)
                .await;
            metrics_count += 1;
        }

        // Log warnings if any
        for warning in &result.warnings {
            warn!(
                "Extraction warning for webhook device '{}': {}",
                device_id, warning
            );
        }

        info!(
            "Webhook adapter '{}' processed {} metrics from device '{}'",
            self.name, metrics_count, device_id
        );

        Ok(metrics_count)
    }

    /// Emit a metric event to both channels and EventBus.
    async fn emit_metric_event(
        &self,
        device_id: String,
        metric_name: String,
        value: MetricValue,
        timestamp: i64,
    ) {
        use neomind_core::NeoMindEvent;

        // Emit to device event channel
        let _ = self.event_tx.send(DeviceEvent::Metric {
            device_id: device_id.clone(),
            metric: metric_name.clone(),
            value: value.clone(),
            timestamp,
        });

        // Store to time series storage
        {
            let storage_guard = self.telemetry_storage.read().await;
            if let Some(storage) = storage_guard.as_ref() {
                let data_point = crate::telemetry::DataPoint {
                    timestamp,
                    value: value.clone(),
                    quality: None,
                };
                if let Err(e) = storage.write(&device_id, &metric_name, data_point).await {
                    warn!(
                        "Failed to write telemetry for {}/{}: {}",
                        device_id, metric_name, e
                    );
                }
            }
        }

        // Publish to EventBus if available
        if let Some(bus) = &self.event_bus {
            let core_value = match &value {
                MetricValue::Integer(i) => neomind_core::MetricValue::Integer(*i),
                MetricValue::Float(f) => neomind_core::MetricValue::Float(*f),
                MetricValue::String(s) => neomind_core::MetricValue::String(s.clone()),
                MetricValue::Boolean(b) => neomind_core::MetricValue::Boolean(*b),
                MetricValue::Array(arr) => {
                    // Convert array to JSON
                    let json_arr: Vec<serde_json::Value> = arr
                        .iter()
                        .map(|v| match v {
                            MetricValue::Integer(i) => serde_json::json!(*i),
                            MetricValue::Float(f) => serde_json::json!(*f),
                            MetricValue::String(s) => serde_json::json!(s),
                            MetricValue::Boolean(b) => serde_json::json!(*b),
                            _ => serde_json::json!(null),
                        })
                        .collect();
                    neomind_core::MetricValue::Json(serde_json::json!(json_arr))
                }
                MetricValue::Binary(_) => neomind_core::MetricValue::Json(serde_json::json!(null)),
                MetricValue::Null => neomind_core::MetricValue::Json(serde_json::json!(null)),
            };

            bus.publish(NeoMindEvent::DeviceMetric {
                device_id,
                metric: metric_name,
                value: core_value,
                timestamp,
                quality: None,
            })
            .await;
        }
    }

    /// Get the webhook URL for a device.
    pub fn get_webhook_url(&self, base_url: &str, device_id: &str) -> String {
        format!("{}/api/devices/webhook/{}", base_url, device_id)
    }
}

#[async_trait]
impl DeviceAdapter for WebhookAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn adapter_type(&self) -> &'static str {
        "webhook"
    }

    fn is_running(&self) -> bool {
        self.running.try_read().map(|r| *r).unwrap_or(false)
    }

    async fn start(&self) -> AdapterResult<()> {
        let mut running = self.running.write().await;
        if *running {
            return Ok(());
        }
        *running = true;

        info!("Webhook adapter '{}' started", self.name);
        Ok(())
    }

    async fn stop(&self) -> AdapterResult<()> {
        let mut running = self.running.write().await;
        *running = false;

        info!("Webhook adapter '{}' stopped", self.name);
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
        let telemetry_storage = self.telemetry_storage.clone();
        tokio::spawn(async move {
            *telemetry_storage.write().await = Some(storage);
        });
    }

    fn device_count(&self) -> usize {
        self.devices.try_read().map(|d| d.len()).unwrap_or(0)
    }

    fn list_devices(&self) -> Vec<String> {
        self.devices
            .try_read()
            .map(|d| d.clone())
            .unwrap_or_default()
    }

    async fn send_command(
        &self,
        _device_id: &str,
        _command_name: &str,
        _payload: String,
        _topic: Option<String>,
    ) -> AdapterResult<()> {
        // Webhook adapter doesn't support sending commands
        // The adapter is receive-only (devices push data, we don't push back)
        Err(AdapterError::Configuration(
            "Webhook adapter is receive-only and cannot send commands".to_string(),
        ))
    }

    fn connection_status(&self) -> ConnectionStatus {
        // Webhook adapter is always "connected" when running
        // It doesn't have an active connection to devices
        if self.is_running() {
            ConnectionStatus::Connected
        } else {
            ConnectionStatus::Disconnected
        }
    }

    async fn subscribe_device(&self, device_id: &str) -> AdapterResult<()> {
        // Track the device (may have been pre-registered)
        let mut devices = self.devices.write().await;
        if !devices.contains(&device_id.to_string()) {
            devices.push(device_id.to_string());
        }
        Ok(())
    }

    async fn unsubscribe_device(&self, device_id: &str) -> AdapterResult<()> {
        let mut devices = self.devices.write().await;
        devices.retain(|d| d != device_id);
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// Implement Clone for WebhookAdapter
impl Clone for WebhookAdapter {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            config: self.config.clone(),
            event_bus: self.event_bus.clone(),
            device_registry: Arc::clone(&self.device_registry),
            event_tx: self.event_tx.clone(),
            running: Arc::clone(&self.running),
            devices: Arc::clone(&self.devices),
            telemetry_storage: Arc::clone(&self.telemetry_storage),
            request_count: Arc::clone(&self.request_count),
            extractor: Arc::clone(&self.extractor),
        }
    }
}

/// Create a webhook adapter from configuration.
pub fn create_webhook_adapter(
    config: WebhookAdapterConfig,
    event_bus: &EventBus,
    device_registry: Arc<DeviceRegistry>,
) -> Arc<WebhookAdapter> {
    let event_bus_arc = Arc::new(event_bus.clone());

    Arc::new(WebhookAdapter::new(
        config,
        Some(event_bus_arc),
        device_registry,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webhook_config_defaults() {
        let config = WebhookAdapterConfig::new("test");
        assert_eq!(config.name, "test");
        assert!(config.api_key.is_none());
        assert!(config.allowed_ips.is_empty());
        assert!(config.blocked_ips.is_empty());
        assert!(config.rate_limit_per_minute.is_none());
    }

    #[test]
    fn test_webhook_config_builder() {
        let config = WebhookAdapterConfig::new("test")
            .with_api_key("test-key")
            .with_allowed_ip("192.168.1.100")
            .with_blocked_ip("10.0.0.1")
            .with_rate_limit(100);

        assert_eq!(config.api_key, Some("test-key".to_string()));
        assert!(config.allowed_ips.contains(&"192.168.1.100".to_string()));
        assert!(config.blocked_ips.contains(&"10.0.0.1".to_string()));
        assert_eq!(config.rate_limit_per_minute, Some(100));
    }

    #[test]
    fn test_unified_extractor_conversion() {
        let config = WebhookAdapterConfig::new("test");
        let adapter = WebhookAdapter::new(config, None, Arc::new(DeviceRegistry::new()));

        use serde_json::json;

        let int_val = json!(42);
        assert!(matches!(
            adapter.extractor.value_to_metric_value(&int_val),
            MetricValue::Integer(42)
        ));

        let float_val = json!(23.5);
        assert!(matches!(
            adapter.extractor.value_to_metric_value(&float_val),
            MetricValue::Float(23.5)
        ));

        let str_val = json!("hello");
        assert!(matches!(
            adapter.extractor.value_to_metric_value(&str_val),
            MetricValue::String(_)
        ));

        let bool_val = json!(true);
        assert!(matches!(
            adapter.extractor.value_to_metric_value(&bool_val),
            MetricValue::Boolean(true)
        ));
    }

    #[test]
    fn test_get_webhook_url() {
        let config = WebhookAdapterConfig::new("test");
        let adapter = WebhookAdapter::new(config, None, Arc::new(DeviceRegistry::new()));

        let url = adapter.get_webhook_url("http://localhost:3000", "sensor01");
        assert_eq!(url, "http://localhost:3000/api/devices/webhook/sensor01");
    }
}
