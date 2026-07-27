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
//! POST /api/devices/{device_id}/webhook
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

use crate::adapter::{
    AdapterError, AdapterResult, ConnectionStatus, DeviceAdapter, DeviceEvent, DiscoveredDeviceInfo,
};
use crate::image_storage::save_image_binary;
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
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error};
use tracing::{info, warn};

/// Webhook device adapter configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookAdapterConfig {
    /// Adapter name
    pub name: String,
    /// API key for authentication (optional).
    ///
    /// When set, inbound webhooks MUST carry it in the `X-API-Key` header.
    /// Distinct from per-device `webhook_token` (which uses `Authorization: Bearer`
    /// or `?token=`): the adapter key is a global pre-shared secret for the whole
    /// adapter, useful when the platform is exposed to the internet without per-device
    /// provisioning. On closed LAN deployments leave this `None`.
    pub api_key: Option<String>,
    /// Allowed IP addresses (whitelist)
    pub allowed_ips: Vec<String>,
    /// Blocked IP addresses (blacklist)
    pub blocked_ips: Vec<String>,
    /// Maximum requests per minute (rate limiting)
    pub rate_limit_per_minute: Option<u32>,
    /// Storage directory for persistence
    pub storage_dir: Option<String>,
    /// Maximum discovery events per minute per source IP (default: 30).
    ///
    /// Caps how many `DeviceDiscovered` events an unregistered-device webhook
    /// can emit per minute from a single source IP. Prevents amplification when
    /// attackers rotate `device_id`s to bypass per-device rate limiting. When the
    /// cap is hit, subsequent unknown-device posts still have their metrics
    /// processed (so data isn't lost) but no discovery event fires — the
    /// auto-onboard manager is spared from LLM-driven analysis floods.
    #[serde(default = "default_discovery_rate")]
    pub discovery_rate_per_minute: u32,
}

fn default_discovery_rate() -> u32 {
    30
}

/// Constant-time string comparison to prevent timing attacks on token / API
/// key verification. Returns false immediately on length mismatch (this leaks
/// length info, which is acceptable for random secrets where length is fixed
/// and known).
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a_bytes, b_bytes) = (a.as_bytes(), b.as_bytes());
    if a_bytes.len() != b_bytes.len() {
        return false;
    }
    let mut result: u8 = 0;
    for (x, y) in a_bytes.iter().zip(b_bytes.iter()) {
        result |= x ^ y;
    }
    result == 0
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
            discovery_rate_per_minute: default_discovery_rate(),
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
    /// Device registry (shared with DeviceService)
    device_registry: Arc<RwLock<Arc<DeviceRegistry>>>,
    /// Event channel
    event_tx: broadcast::Sender<DeviceEvent>,
    /// Running state
    running: Arc<RwLock<bool>>,
    /// Connected devices (devices that have sent data)
    devices: Arc<RwLock<Vec<String>>>,
    /// Telemetry storage
    telemetry_storage: Arc<RwLock<Option<Arc<TimeSeriesStorage>>>>,
    /// Request counter for rate limiting (per device_id)
    request_count: Arc<RwLock<HashMap<String, (u32, std::time::Instant)>>>,
    /// Discovery emission counter (per source IP) — caps DeviceDiscovered events
    /// to prevent event-bus / auto-onboard amplification when attackers rotate
    /// device_ids. See `discovery_rate_per_minute` in config.
    discovery_count: Arc<RwLock<HashMap<String, (u32, std::time::Instant)>>>,
    /// Unified data extractor
    extractor: Arc<UnifiedExtractor>,
    /// Data directory for image storage (runtime, not config)
    pub data_dir: Arc<RwLock<Option<PathBuf>>>,
}

impl WebhookAdapter {
    /// Create a new webhook adapter.
    pub fn new(
        config: WebhookAdapterConfig,
        event_bus: Option<Arc<EventBus>>,
        device_registry: Arc<DeviceRegistry>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(1000);

        let registry = Arc::new(RwLock::new(device_registry.clone()));
        let extractor = Arc::new(UnifiedExtractor::new(device_registry));

        Self {
            name: config.name.clone(),
            config,
            event_bus,
            device_registry: registry,
            event_tx,
            running: Arc::new(RwLock::new(false)),
            devices: Arc::new(RwLock::new(Vec::new())),
            telemetry_storage: Arc::new(RwLock::new(None)),
            request_count: Arc::new(RwLock::new(HashMap::new())),
            discovery_count: Arc::new(RwLock::new(HashMap::new())),
            extractor,
            data_dir: Arc::new(RwLock::new(None)),
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

    /// Set the device registry (shared with DeviceService).
    pub async fn set_shared_device_registry(&self, registry: Arc<DeviceRegistry>) {
        *self.device_registry.write().await = registry;
    }

    /// Set the data directory for image storage.
    pub fn set_data_dir(&self, data_dir: PathBuf) {
        let data_dir_arc = self.data_dir.clone();
        tokio::spawn(async move {
            *data_dir_arc.write().await = Some(data_dir);
        });
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
                Some(key) if constant_time_eq(key, expected_key) => {}
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

            // Only run the full cleanup scan when the map grows large (>1000
            // entries) — avoids a per-request O(N) retain on the hot path.
            if counts.len() > 1000 {
                counts.retain(|_, (_, ts)| now.duration_since(*ts).as_secs() < 60);
            }

            let (count, ts) = counts.entry(device_id.to_string()).or_insert((0, now));

            // Reset stale per-device counter (outside the 1-min window) —
            // handles expiry without a full map scan.
            if now.duration_since(*ts).as_secs() >= 60 {
                *count = 0;
                *ts = now;
            }

            if *count >= limit {
                return Err(AdapterError::Connection("Rate limit exceeded".to_string()));
            }
            *count += 1;
        }

        Ok(())
    }

    /// Process a webhook payload and emit events.
    ///
    /// `provided_token`: Optional per-device webhook token from `Authorization: Bearer`
    ///   header or `?token=` query param. If the device has `webhook_token` configured
    ///   in its `connection_config.extra`, this must match.
    ///
    /// `provided_api_key`: Optional adapter-level API key from `X-API-Key` header.
    ///   Only checked when `self.config.api_key` is configured. Use this for
    ///   internet-exposed deployments where per-device tokens aren't pre-provisioned.
    ///
    /// `remote_ip`: Client IP from `ConnectInfo<SocketAddr>`. Required for the IP
    ///   allowlist/blocklist to take effect, and used as the discovery-event
    ///   throttle key.
    pub async fn process_webhook(
        &self,
        device_id: String,
        payload: WebhookPayload,
        provided_token: Option<&str>,
        provided_api_key: Option<&str>,
        remote_ip: Option<&IpAddr>,
    ) -> AdapterResult<usize> {
        // Validate request (adapter API key, IP blacklist/whitelist).
        // All three params now flow through from the handler.
        self.validate_request(&device_id, provided_api_key, remote_ip)?;

        // Get device from shared registry (for token verification and type info)
        let registry = self.device_registry.read().await;
        let device_info = registry.get_device(&device_id);
        let is_registered = device_info.is_some();
        let device_type = device_info
            .as_ref()
            .map(|d| d.device_type.clone())
            .unwrap_or_else(|| "webhook".to_string());

        // Verify per-device webhook token (if configured)
        if let Some(device) = device_info {
            if let Some(expected) = device
                .connection_config
                .extra
                .get("webhook_token")
                .and_then(|v| v.as_str())
            {
                match provided_token {
                    Some(token) if constant_time_eq(token, expected) => {}
                    Some(_) => {
                        return Err(AdapterError::Connection(
                            "Invalid webhook token".to_string(),
                        ));
                    }
                    None => {
                        return Err(AdapterError::Connection(
                            "Webhook token required".to_string(),
                        ));
                    }
                }
            }
        }
        drop(registry);

        // Check rate limit
        self.check_rate_limit(&device_id).await?;

        let timestamp = payload
            .timestamp
            .unwrap_or_else(|| chrono::Utc::now().timestamp());

        // Track device first sighting (for DeviceOnline emission on registered devices)
        let is_new = {
            let mut devices = self.devices.write().await;
            if !devices.contains(&device_id) {
                devices.push(device_id.clone());
                true
            } else {
                false
            }
        };

        // For known/registered devices, emit DeviceOnline only on first sighting
        if is_new && is_registered {
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

        // For unknown/unregistered devices, emit DeviceDiscovered subject to a
        // per-IP rate cap. Without this cap, an attacker rotating `device_id`s
        // can bypass per-device rate limiting and flood the auto-onboard manager
        // (which may call an LLM per discovery). Metrics are still processed below
        // even when the discovery event is throttled — data isn't lost.
        if !is_registered {
            let ip_key = remote_ip
                .map(|ip| ip.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let limit = self.config.discovery_rate_per_minute.max(1);
            let should_emit = {
                let mut counts = self.discovery_count.write().await;
                let now = std::time::Instant::now();
                // Drop entries older than 1 minute (fixed-window cleanup).
                counts.retain(|_, (_, ts)| now.duration_since(*ts).as_secs() < 60);
                let (count, _) = counts.entry(ip_key).or_insert((0, now));
                if *count >= limit {
                    false
                } else {
                    *count += 1;
                    true
                }
            };

            if should_emit {
                let sample = payload.data.clone();
                let discovered = DiscoveredDeviceInfo {
                    device_id: device_id.clone(),
                    device_type: "unknown".to_string(),
                    name: None,
                    endpoint: Some(format!("webhook:{}", self.name)),
                    capabilities: vec![],
                    timestamp,
                    metadata: serde_json::json!({
                        "source": "webhook",
                        "adapter_id": self.name,
                    }),
                };

                let _ = self
                    .event_tx
                    .send(DeviceEvent::Discovery { device: discovered });

                if let Some(bus) = &self.event_bus {
                    use neomind_core::NeoMindEvent;

                    bus.publish(NeoMindEvent::DeviceDiscovered {
                        device_id: device_id.clone(),
                        source: "webhook".to_string(),
                        adapter_id: Some(self.name.clone()),
                        metadata: serde_json::json!({
                            "endpoint": format!("webhook:{}", self.name),
                        }),
                        sample,
                        is_binary: false,
                        timestamp,
                    })
                    .await;
                }
            } else {
                warn!(
                    adapter = %self.name,
                    device_id = %device_id,
                    "Discovery event throttled for unknown device; metrics still recorded"
                );
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

    /// Convert image data (Binary or base64 String) to URL if applicable.
    pub async fn convert_binary_to_url(
        device_id: &str,
        metric_name: &str,
        timestamp: i64,
        value: MetricValue,
        data_dir: Arc<RwLock<Option<PathBuf>>>,
    ) -> MetricValue {
        match value {
            MetricValue::Binary(bytes) => {
                let dir_guard = data_dir.read().await;
                if let Some(dir) = dir_guard.as_ref() {
                    match save_image_binary(device_id, metric_name, timestamp, &bytes, dir) {
                        Ok(url) => {
                            debug!(
                                "Saved binary image for {}/{} -> {}",
                                device_id, metric_name, url
                            );
                            MetricValue::String(url)
                        }
                        Err(e) => {
                            error!(
                                "Failed to save binary image for {}/{}: {}",
                                device_id, metric_name, e
                            );
                            MetricValue::Binary(bytes)
                        }
                    }
                } else {
                    MetricValue::Binary(bytes)
                }
            }
            MetricValue::String(s) => {
                // Webhook JSON payloads carry images as base64 strings — detect and convert
                if let Some(bytes) = crate::image_storage::try_decode_base64_image(&s) {
                    let dir_guard = data_dir.read().await;
                    if let Some(dir) = dir_guard.as_ref() {
                        match save_image_binary(device_id, metric_name, timestamp, &bytes, dir) {
                            Ok(url) => {
                                debug!(
                                    "Saved string image for {}/{} -> {}",
                                    device_id, metric_name, url
                                );
                                return MetricValue::String(url);
                            }
                            Err(e) => {
                                error!(
                                    "Failed to save string image for {}/{}: {}",
                                    device_id, metric_name, e
                                );
                            }
                        }
                    }
                }
                MetricValue::String(s)
            }
            // Not a Binary value, return unchanged
            other => other,
        }
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

        // Convert Binary to URL before storage + event bus (fork point)
        let value = Self::convert_binary_to_url(
            &device_id,
            &metric_name,
            timestamp,
            value,
            self.data_dir.clone(),
        )
        .await;

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
                if let Err(e) = storage
                    .write(&format!("device:{}", device_id), &metric_name, data_point)
                    .await
                {
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
                is_virtual: None,
            })
            .await;
        }
    }

    /// Get the webhook URL for a device.
    pub fn get_webhook_url(&self, base_url: &str, device_id: &str) -> String {
        format!("{}/api/devices/{}/webhook", base_url, device_id)
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
            discovery_count: Arc::clone(&self.discovery_count),
            extractor: Arc::clone(&self.extractor),
            data_dir: Arc::clone(&self.data_dir),
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
    fn test_constant_time_eq_basic() {
        // Equal strings
        assert!(constant_time_eq("secret", "secret"));
        // Different content, same length
        assert!(!constant_time_eq("secret", "secreX"));
        // Different length
        assert!(!constant_time_eq("secret", "secret-extra"));
        assert!(!constant_time_eq("short", "longer-string"));
        // Empty
        assert!(constant_time_eq("", ""));
        assert!(!constant_time_eq("", "x"));
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
        assert_eq!(url, "http://localhost:3000/api/devices/sensor01/webhook");
    }

    // ----- validate_request coverage -----

    fn make_adapter_with_config(config: WebhookAdapterConfig) -> WebhookAdapter {
        WebhookAdapter::new(config, None, Arc::new(DeviceRegistry::new()))
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn test_validate_request_rejects_blacklisted_ip() {
        let config = WebhookAdapterConfig::new("t").with_blocked_ip("10.0.0.5");
        let adapter = make_adapter_with_config(config);
        let res = adapter.validate_request("dev-1", None, Some(&ip("10.0.0.5")));
        assert!(res.is_err(), "blacklisted IP must be rejected");
        match res.unwrap_err() {
            AdapterError::Connection(msg) => assert!(msg.contains("blocked")),
            other => panic!("expected Connection error, got {other:?}"),
        }
    }

    #[test]
    fn test_validate_request_enforces_whitelist() {
        let config = WebhookAdapterConfig::new("t").with_allowed_ip("192.168.1.10");
        let adapter = make_adapter_with_config(config);
        // Not in whitelist → reject
        let denied = adapter.validate_request("dev-1", None, Some(&ip("192.168.1.99")));
        assert!(denied.is_err());
        // In whitelist → allow
        let allowed = adapter.validate_request("dev-1", None, Some(&ip("192.168.1.10")));
        assert!(allowed.is_ok());
    }

    #[test]
    fn test_validate_request_api_key_enforced_when_configured() {
        let config = WebhookAdapterConfig::new("t").with_api_key("secret");
        let adapter = make_adapter_with_config(config);
        // Missing key
        assert!(adapter.validate_request("dev-1", None, None).is_err());
        // Wrong key
        assert!(adapter
            .validate_request("dev-1", Some("wrong"), None)
            .is_err());
        // Correct key
        assert!(adapter
            .validate_request("dev-1", Some("secret"), None)
            .is_ok());
    }

    #[test]
    fn test_validate_request_no_api_key_configured_allows_any() {
        // When the adapter has no api_key configured (closed-LAN default),
        // requests without an X-API-Key header are accepted.
        let adapter = make_adapter_with_config(WebhookAdapterConfig::new("t"));
        assert!(adapter.validate_request("dev-1", None, None).is_ok());
        assert!(adapter
            .validate_request("dev-1", Some("anything"), None)
            .is_ok());
    }

    // ----- discovery throttle -----
    //
    // Strategy: subscribe to the adapter's event stream and count how many
    // `DeviceEvent::Discovery` variants fire. The stream is `Pin<Box<dyn
    // Stream>>`, so we drive it with `futures::StreamExt::next` under a short
    // timeout — events fired synchronously inside `process_webhook` land in
    // the broadcast buffer before we poll, so they arrive on the first poll.

    #[tokio::test]
    async fn test_discovery_throttle_kicks_in_after_limit() {
        use futures::StreamExt;
        use std::time::Duration;

        let mut config = WebhookAdapterConfig::new("t");
        config.discovery_rate_per_minute = 3;

        let adapter = make_adapter_with_config(config);
        let src_ip = ip("203.0.113.7");

        // Phase 1: fire 3 (== cap) unregistered-device posts. Each must emit
        // exactly one Discovery event. Metrics also emit Metric events, so
        // we filter by variant.
        let mut stream = adapter.subscribe();

        for i in 0..3 {
            let payload = WebhookPayload {
                device_id: Some(format!("unreg-{i}")),
                timestamp: None,
                quality: None,
                data: serde_json::json!({"v": i}),
            };
            adapter
                .process_webhook(format!("unreg-{i}"), payload, None, None, Some(&src_ip))
                .await
                .unwrap();
            // Don't assert metric count in phase 1 — extraction width varies
            // by device type/template state. The point is that the call
            // succeeds.
        }

        // Collect all events pending for phase 1. Each post emits at least a
        // Discovery and a Metric event, so drain until the channel is idle.
        let mut discoveries_phase1 = 0u32;
        let drain_deadline = tokio::time::Instant::now() + Duration::from_millis(150);
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(drain_deadline) => break,
                ev = stream.next() => match ev {
                    Some(DeviceEvent::Discovery { .. }) => discoveries_phase1 += 1,
                    Some(_) => continue,
                    None => break,
                },
            }
        }
        assert_eq!(
            discoveries_phase1, 3,
            "first 3 unregistered posts should each emit Discovery"
        );

        // Phase 2: fire 3 more. Discovery events MUST be throttled → zero
        // new Discovery. Metrics still process (return value == 1).
        for i in 3..6 {
            let payload = WebhookPayload {
                device_id: Some(format!("unreg-{i}")),
                timestamp: None,
                quality: None,
                data: serde_json::json!({"v": i}),
            };
            let n = adapter
                .process_webhook(format!("unreg-{i}"), payload, None, None, Some(&src_ip))
                .await
                .expect("metrics still process when discovery is throttled");
            assert!(n >= 1, "metrics must still process when throttled");
        }

        // Drain again; only Metric events should appear.
        let mut discoveries_phase2 = 0u32;
        let drain_deadline = tokio::time::Instant::now() + Duration::from_millis(150);
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(drain_deadline) => break,
                ev = stream.next() => match ev {
                    Some(DeviceEvent::Discovery { .. }) => discoveries_phase2 += 1,
                    Some(_) => continue,
                    None => break,
                },
            }
        }
        assert_eq!(
            discoveries_phase2, 0,
            "discovery events must be throttled once per-IP cap is hit"
        );
    }
}
