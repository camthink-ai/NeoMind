//! MQTT device adapter for NeoMind event-driven architecture.
//!
//! This adapter connects to an MQTT broker, subscribes to device topics,
//! and publishes device events to the event bus.
//!
//! ## Topic Format
//!
//! Uplink telemetry: `device/{device_type}/{device_id}/uplink`
//! Downlink commands: `device/{device_type}/{device_id}/downlink`
//! Discovery: `{discovery_prefix}/announce`
//!
//! ## Protocol Mapping Integration
//!
//! The adapter can use a `ProtocolMapping` for flexible topic and payload handling:
//! ```text
//! Device Type Definition       MQTT Mapping
//! ├─ temperature capability  ──→ sensor/${id}/temperature
//! ├─ humidity capability     ──→ sensor/${id}/humidity
//! └─ set_interval command    ──→ sensor/${id}/command
//! ```

use crate::adapter::{
    AdapterError, AdapterResult, ConnectionStatus, DeviceAdapter, DeviceEvent, DiscoveredDeviceInfo,
};
use crate::mdl::MetricValue;
use crate::mqtt::MqttConfig;
use crate::protocol::ProtocolMapping;
use crate::registry::{DeviceConfig, DeviceRegistry};
use crate::telemetry::TimeSeriesStorage;
use crate::unified_extractor::UnifiedExtractor;

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use futures::{Stream, StreamExt};
use neomind_core::EventBus;
use neomind_core::NeoMindEvent;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};
use tracing::trace;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// MQTT device adapter configuration.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct MqttAdapterConfig {
    /// Adapter name
    pub name: String,
    /// MQTT broker configuration
    pub mqtt: MqttConfig,
    /// Topic patterns to subscribe to (e.g., ["sensors/+/temperature", "sensors/+/humidity"])
    pub subscribe_topics: Vec<String>,
    /// Topic pattern for device discovery (e.g., "devices/+/discovery")
    pub discovery_topic: Option<String>,
    /// Discovery prefix for auto-discovery
    pub discovery_prefix: String,
    /// Enable auto-discovery
    pub auto_discovery: bool,
    /// Storage directory for persistence
    pub storage_dir: Option<String>,
}

impl MqttAdapterConfig {
    /// Create a new MQTT adapter configuration.
    pub fn new(name: impl Into<String>, broker: impl Into<String>) -> Self {
        let mqtt_config = MqttConfig::new(broker, "neomind");
        Self {
            name: name.into(),
            mqtt: mqtt_config,
            subscribe_topics: Vec::new(),
            discovery_topic: None,
            discovery_prefix: "neomind".to_string(),
            auto_discovery: true,
            storage_dir: None,
        }
    }

    /// Add a subscription topic pattern.
    pub fn with_topic(mut self, topic: impl Into<String>) -> Self {
        self.subscribe_topics.push(topic.into());
        self
    }

    /// Add multiple subscription topics.
    pub fn with_topics(mut self, topics: Vec<String>) -> Self {
        self.subscribe_topics = topics;
        self
    }

    /// Set the discovery topic.
    pub fn with_discovery(mut self, topic: impl Into<String>) -> Self {
        self.discovery_topic = Some(topic.into());
        self
    }

    /// Set the discovery prefix.
    pub fn with_discovery_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.discovery_prefix = prefix.into();
        self
    }

    /// Set MQTT authentication.
    pub fn with_auth(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        self.mqtt = self.mqtt.with_auth(username, password);
        self
    }

    /// Set MQTT port.
    pub fn with_port(mut self, port: u16) -> Self {
        self.mqtt = self.mqtt.with_port(port);
        self
    }

    /// Set storage directory.
    pub fn with_storage_dir(mut self, dir: impl Into<String>) -> Self {
        self.storage_dir = Some(dir.into());
        self
    }

    /// Enable or disable auto-discovery.
    pub fn with_auto_discovery(mut self, enabled: bool) -> Self {
        self.auto_discovery = enabled;
        self
    }
}

impl Default for MqttAdapterConfig {
    fn default() -> Self {
        Self::new("mqtt", "localhost")
    }
}

/// Single MQTT broker connection
struct MqttClientInner {
    /// Unique broker identifier
    _broker_id: String,
    /// Broker address (host:port)
    _broker_addr: String,
    /// MQTT client
    client: rumqttc::AsyncClient,
    /// Running flag for the event loop task
    running: Arc<RwLock<bool>>,
    /// Subscribed topics for this broker
    subscribed_topics: Arc<RwLock<std::collections::HashSet<String>>>,
}

/// MQTT device adapter.
///
/// Manages multiple MQTT broker connections and automatically subscribes
/// to device topics on all connected brokers.
pub struct MqttAdapter {
    /// Adapter configuration
    config: MqttAdapterConfig,
    /// Event channel sender
    event_tx: broadcast::Sender<DeviceEvent>,
    /// Running state
    running: Arc<std::sync::atomic::AtomicBool>,
    /// Connected devices
    devices: Arc<RwLock<Vec<String>>>,
    /// Optional protocol mapping for flexible topic/payload handling
    protocol_mapping: Option<Arc<dyn ProtocolMapping>>,
    /// Device ID to device type mapping (used with protocol mapping)
    device_types: Arc<RwLock<HashMap<String, String>>>,
    /// Multiple MQTT broker connections (broker_id -> client)
    mqtt_clients: Arc<RwLock<HashMap<String, MqttClientInner>>>,
    /// Connection status (overall - true if at least one broker is connected)
    connection_status: Arc<RwLock<ConnectionStatus>>,
    /// Event bus for publishing system events
    event_bus: Option<Arc<EventBus>>,
    /// Device registry for template management (wrapped for interior mutability)
    device_registry: Arc<RwLock<Arc<DeviceRegistry>>>,
    /// Time series storage for telemetry
    telemetry_storage: Arc<RwLock<Option<Arc<TimeSeriesStorage>>>>,
    /// Metric cache (device_id -> metric_name -> (value, timestamp))
    metric_cache:
        Arc<RwLock<HashMap<String, HashMap<String, (MetricValue, chrono::DateTime<chrono::Utc>)>>>>,
    /// Topic to device ID mapping (for routing messages to registered devices)
    topic_to_device: Arc<RwLock<HashMap<String, String>>>,
    /// Unified data extractor
    extractor: Arc<UnifiedExtractor>,
}

#[allow(dead_code)]
impl MqttAdapter {
    /// Create a new MQTT adapter.
    pub fn new(config: MqttAdapterConfig) -> Self {
        let (event_tx, _) = broadcast::channel(1000);
        let device_registry = Arc::new(DeviceRegistry::new());
        let extractor = Arc::new(UnifiedExtractor::new(device_registry.clone()));

        Self {
            config,
            event_tx,
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            devices: Arc::new(RwLock::new(Vec::new())),
            protocol_mapping: None,
            device_types: Arc::new(RwLock::new(HashMap::new())),
            mqtt_clients: Arc::new(RwLock::new(HashMap::new())),
            connection_status: Arc::new(RwLock::new(ConnectionStatus::Disconnected)),
            event_bus: None,
            device_registry: Arc::new(RwLock::new(device_registry)),
            telemetry_storage: Arc::new(RwLock::new(None)),
            metric_cache: Arc::new(RwLock::new(HashMap::new())),
            topic_to_device: Arc::new(RwLock::new(HashMap::new())),
            extractor,
        }
    }

    /// Create a new MQTT adapter with an event bus.
    pub fn with_event_bus(mut self, event_bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    /// Set event bus (for Arc<EventBus>).
    pub fn set_event_bus(&mut self, event_bus: Arc<EventBus>) {
        self.event_bus = Some(event_bus);
    }

    /// Set telemetry storage.
    pub fn set_telemetry_storage(&self, storage: Arc<TimeSeriesStorage>) {
        // Spawn a task to set the storage asynchronously
        let telemetry = self.telemetry_storage.clone();
        tokio::spawn(async move {
            *telemetry.write().await = Some(storage);
        });
    }

    /// Set the device registry.
    pub fn with_device_registry(mut self, registry: Arc<DeviceRegistry>) -> Self {
        self.device_registry = Arc::new(RwLock::new(registry));
        self
    }

    /// Set a shared device registry (for looking up devices by custom telemetry topics)
    /// This allows the adapter to find devices registered via auto-onboarding
    pub async fn set_shared_device_registry(&self, registry: Arc<DeviceRegistry>) {
        *self.device_registry.write().await = registry;
    }

    /// Create a new MQTT adapter with a protocol mapping.
    pub fn with_mapping(
        config: MqttAdapterConfig,
        mapping: Arc<dyn ProtocolMapping>,
        event_bus: Option<Arc<EventBus>>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(1000);
        let device_registry = Arc::new(DeviceRegistry::new());
        let extractor = Arc::new(UnifiedExtractor::new(device_registry.clone()));

        Self {
            config,
            event_tx,
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            devices: Arc::new(RwLock::new(Vec::new())),
            protocol_mapping: Some(mapping),
            device_types: Arc::new(RwLock::new(HashMap::new())),
            mqtt_clients: Arc::new(RwLock::new(HashMap::new())),
            connection_status: Arc::new(RwLock::new(ConnectionStatus::Disconnected)),
            event_bus,
            device_registry: Arc::new(RwLock::new(device_registry)),
            telemetry_storage: Arc::new(RwLock::new(None)),
            metric_cache: Arc::new(RwLock::new(HashMap::new())),
            topic_to_device: Arc::new(RwLock::new(HashMap::new())),
            extractor,
        }
    }

    /// Set the protocol mapping.
    pub async fn set_mapping(&mut self, mapping: Arc<dyn ProtocolMapping>) {
        self.protocol_mapping = Some(mapping);
    }

    /// Get the current protocol mapping.
    pub fn mapping(&self) -> Option<&Arc<dyn ProtocolMapping>> {
        self.protocol_mapping.as_ref()
    }

    /// Register a device type with its ID (for protocol mapping).
    pub async fn register_device_type(&self, device_id: String, device_type: String) {
        let mut types = self.device_types.write().await;
        types.insert(device_id, device_type);
    }

    /// Add a broker connection to this adapter.
    ///
    /// This allows the MQTT adapter to connect to multiple brokers simultaneously.
    /// Device topics will be subscribed on all connected brokers.
    pub async fn add_broker(
        &self,
        broker_id: impl Into<String>,
        broker_host: impl Into<String>,
        broker_port: u16,
        username: Option<String>,
        password: Option<String>,
    ) -> AdapterResult<()> {
        let broker_id = broker_id.into();
        let broker_host = broker_host.into();
        let broker_addr = format!("{}:{}", broker_host, broker_port);

        // Check if broker already exists
        if self.mqtt_clients.read().await.contains_key(&broker_id) {
            return Err(AdapterError::Configuration(format!(
                "Broker already exists: {}",
                broker_id
            )));
        }

        // Build MQTT options
        let client_id = format!("neomind-{}-{}", broker_id, Uuid::new_v4());
        let mut mqttoptions = rumqttc::MqttOptions::new(&client_id, &broker_host, broker_port);
        mqttoptions.set_max_packet_size(10 * 1024 * 1024, 10 * 1024 * 1024);
        mqttoptions.set_keep_alive(Duration::from_secs(60));

        // Set credentials if provided
        if let (Some(user), Some(pass)) = (username, password) {
            mqttoptions.set_credentials(&user, &pass);
        }

        // Create client
        let (client, eventloop) = rumqttc::AsyncClient::new(mqttoptions, 10);

        let running = Arc::new(RwLock::new(true));
        let subscribed_topics = Arc::new(RwLock::new(std::collections::HashSet::new()));

        // Subscribe to initial topics on this broker
        let mut initial_topics = vec![
            "device/+/+/uplink".to_string(),
            "device/+/+/downlink".to_string(),
        ];
        for topic in &self.config.subscribe_topics {
            initial_topics.push(topic.clone());
        }

        for topic in &initial_topics {
            debug!(
                "Attempting to subscribe to topic '{}' on broker '{}'...",
                topic, broker_id
            );
            if let Err(e) = client.subscribe(topic, rumqttc::QoS::AtLeastOnce).await {
                warn!(
                    "Failed to subscribe to {} on broker {}: {}",
                    topic, broker_id, e
                );
            } else {
                subscribed_topics.write().await.insert(topic.clone());
                info!(
                    "Successfully subscribed to topic '{}' on broker '{}'",
                    topic, broker_id
                );
            }
        }

        info!(
            "Subscribed to {} topics for broker '{}': {:?}",
            subscribed_topics.read().await.len(),
            broker_id,
            subscribed_topics.read().await
        );

        // Store the client
        let inner = MqttClientInner {
            _broker_id: broker_id.clone(),
            _broker_addr: broker_addr.clone(),
            client,
            running: running.clone(),
            subscribed_topics,
        };
        self.mqtt_clients
            .write()
            .await
            .insert(broker_id.clone(), inner);

        // Restore topic_to_device and device_types mappings from device registry
        // This is critical for server restart - devices must be able to receive data and metrics must be stored
        let registry = self.device_registry.read().await;
        let devices = registry.list_devices().await;
        let mut topic_mapping = self.topic_to_device.write().await;
        let mut type_mapping = self.device_types.write().await;
        let mut restored_topic_count = 0;
        let mut restored_type_count = 0;

        for device in devices {
            // Restore topic_to_device mapping
            if let Some(ref telemetry_topic) = device.connection_config.telemetry_topic {
                topic_mapping.insert(telemetry_topic.clone(), device.device_id.clone());
                restored_topic_count += 1;
                debug!(
                    "Restored topic mapping: '{}' -> '{}'",
                    telemetry_topic, device.device_id
                );
            }

            // Restore device_types mapping (required for metric processing)
            type_mapping.insert(device.device_id.clone(), device.device_type.clone());
            restored_type_count += 1;
            debug!(
                "Restored device type mapping: '{}' -> '{}'",
                device.device_id, device.device_type
            );
        }

        drop(topic_mapping);
        drop(type_mapping);
        info!(
            "Restored {} topic-to-device and {} device_type mappings from device registry for broker {}",
            restored_topic_count, restored_type_count, broker_id
        );

        // Update connection status
        self.update_connection_status().await;

        // Spawn message processing task for this broker
        let running_flag = running.clone();
        let config = self.config.clone();
        let event_tx = self.event_tx.clone();
        let event_bus = self.event_bus.clone();
        let device_types = self.device_types.clone();
        let metric_cache = self.metric_cache.clone();
        let telemetry_storage = self.telemetry_storage.clone();
        let device_registry = self.device_registry.clone();
        let connection_status = self.connection_status.clone();
        let mqtt_clients = self.mqtt_clients.clone();
        let broker_id_clone = broker_id.clone();
        let extractor = self.extractor.clone();
        let topic_to_device = self.topic_to_device.clone();

        info!(
            "Starting event loop task for broker '{}', connecting to {}...",
            broker_id, broker_addr
        );

        tokio::spawn(async move {
            let mut eventloop = eventloop;
            let mut error_count = 0;
            let max_errors = 5;

            while *running_flag.read().await {
                match eventloop.poll().await {
                    Ok(notification) => {
                        error_count = 0; // Reset error count on success
                        Self::handle_mqtt_notification(
                            notification,
                            &config,
                            &event_tx,
                            &event_bus,
                            &device_types,
                            &metric_cache,
                            &telemetry_storage,
                            &device_registry,
                            &connection_status,
                            &broker_id_clone,
                            &extractor,
                            &topic_to_device,
                        )
                        .await;
                    }
                    Err(e) => {
                        error_count += 1;
                        if error_count >= max_errors {
                            error!(
                                "MQTT broker {} error count reached {}, stopping: {}",
                                broker_id_clone, max_errors, e
                            );
                            break;
                        }
                        warn!(
                            "MQTT broker {} error ({}/{}): {}",
                            broker_id_clone, error_count, max_errors, e
                        );
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }

            // Remove this broker from clients map when task ends
            mqtt_clients.write().await.remove(&broker_id_clone);
            info!("MQTT broker {} connection closed", broker_id_clone);
        });

        info!("Added MQTT broker: {} ({})", broker_id, broker_addr);
        Ok(())
    }

    /// Remove a broker connection from this adapter.
    pub async fn remove_broker(&self, broker_id: &str) -> AdapterResult<()> {
        let mut clients = self.mqtt_clients.write().await;
        if let Some(inner) = clients.remove(broker_id) {
            // Stop the running flag
            *inner.running.write().await = false;
            info!("Removed MQTT broker: {}", broker_id);
            Ok(())
        } else {
            Err(AdapterError::Configuration(format!(
                "Broker not found: {}",
                broker_id
            )))
        }
    }

    /// Get list of connected broker IDs.
    pub async fn list_brokers(&self) -> Vec<String> {
        self.mqtt_clients.read().await.keys().cloned().collect()
    }

    /// Update overall connection status based on connected brokers.
    async fn update_connection_status(&self) {
        let has_connected = !self.mqtt_clients.read().await.is_empty();
        *self.connection_status.write().await = if has_connected {
            ConnectionStatus::Connected
        } else {
            ConnectionStatus::Disconnected
        };
    }

    /// Extract device ID from topic using pattern matching.
    fn extract_device_id(&self, topic: &str) -> Option<String> {
        // Try device/{device_type}/{device_id}/{direction} format first
        let parts: Vec<&str> = topic.split('/').collect();
        if parts.len() >= 4 && parts[0] == "device" {
            return Some(parts[2].to_string());
        }

        // Try to match against subscription patterns
        for pattern in &self.config.subscribe_topics {
            if let Some(id) = Self::match_topic_pattern(topic, pattern) {
                return Some(id);
            }
        }

        // Fallback: extract from common patterns
        if parts.len() >= 2 {
            // Common pattern: prefix/{device_id}/...
            Some(parts[1].to_string())
        } else {
            None
        }
    }

    /// Match a topic against a pattern and extract device ID.
    fn match_topic_pattern(topic: &str, pattern: &str) -> Option<String> {
        let pattern_parts: Vec<&str> = pattern.split('/').collect();
        let topic_parts: Vec<&str> = topic.split('/').collect();

        if pattern_parts.len() != topic_parts.len() {
            return None;
        }

        let mut device_id = None;
        let mut matches = true;

        for (i, (p, t)) in pattern_parts.iter().zip(topic_parts.iter()).enumerate() {
            match *p {
                "+" => {
                    if i == 1 {
                        device_id = Some(t.to_string());
                    }
                }
                "#" => {}
                _ => {
                    if p != t {
                        matches = false;
                        break;
                    }
                }
            }
        }

        if matches {
            device_id
        } else {
            None
        }
    }

    /// Extract metric name from topic.
    fn extract_metric_name(&self, topic: &str) -> Option<String> {
        let parts: Vec<&str> = topic.split('/').collect();
        if parts.len() >= 4 && parts[0] == "device" {
            // device/{device_type}/{device_id}/uplink - metrics are in payload
            return None;
        }
        if parts.len() >= 3 {
            Some(parts[2].to_string())
        } else {
            topic.split('/').next_back().map(|s| s.to_string())
        }
    }

    /// Parse MQTT payload to MetricValue.
    fn parse_payload(&self, payload: &[u8]) -> Result<MetricValue, String> {
        // Try protocol mapping first
        // Default payload parsing
        Self::default_parse_value(payload)
    }

    /// Default value parsing (when no protocol mapping is available).
    fn default_parse_value(payload: &[u8]) -> Result<MetricValue, String> {
        // Try JSON first
        if let Ok(json) = serde_json::from_slice::<Value>(payload) {
            if let Some(num) = json.as_f64() {
                return Ok(MetricValue::Float(num));
            } else if let Some(s) = json.as_str() {
                return Ok(MetricValue::String(s.to_string()));
            } else if let Some(b) = json.as_bool() {
                return Ok(MetricValue::Boolean(b));
            } else if let Some(obj) = json.as_object() {
                // Handle JSON object - return as-is for further processing
                let json_str = serde_json::to_string(obj).unwrap_or_default();
                return Ok(MetricValue::String(json_str));
            }
        }

        // Try as UTF-8 string
        if let Ok(text) = std::str::from_utf8(payload) {
            let text = text.trim();
            if let Ok(num) = text.parse::<f64>() {
                return Ok(MetricValue::Float(num));
            } else if let Ok(b) = text.parse::<bool>() {
                return Ok(MetricValue::Boolean(b));
            }
            return Ok(MetricValue::String(text.to_string()));
        }

        Err("Failed to parse payload".to_string())
    }

    /// Process an incoming MQTT message and emit device events.
    async fn process_message(&self, topic: &str, payload: &[u8]) {
        let Some(device_id) = self.extract_device_id(topic) else {
            debug!("Could not extract device ID from topic: {}", topic);
            return;
        };

        let now = chrono::Utc::now();

        // Handle discovery announcement
        if topic.ends_with("/announce")
            || topic == format!("{}/announce", self.config.discovery_prefix)
        {
            self.handle_discovery_announcement(device_id, topic, payload)
                .await;
            return;
        }

        // Handle device uplink/downlink messages
        // Topic format: device/{device_type}/{device_id}/uplink
        let parts: Vec<&str> = topic.split('/').collect();
        if parts.len() >= 4 && parts[0] == "device" {
            let device_type = parts[1].to_string();
            let direction = parts.get(3);

            // Handle uplink data
            if direction == Some(&"uplink") {
                self.handle_uplink_message(device_id, device_type, payload)
                    .await;
                return;
            }
        }

        // Handle simple topic format: prefix/{device_id}/metric
        if let Some(metric_name) = self.extract_metric_name(topic) {
            match self.parse_payload(payload) {
                Ok(value) => {
                    self.emit_metric_event(device_id, metric_name, value, now)
                        .await;
                }
                Err(e) => {
                    warn!("Failed to parse payload from {}: {}", topic, e);
                }
            }
        }
    }

    /// Handle a discovery announcement message.
    async fn handle_discovery_announcement(&self, device_id: String, _topic: &str, payload: &[u8]) {
        #[derive(Debug, Deserialize)]
        struct Announcement {
            device_type: String,
            #[serde(default)]
            name: Option<String>,
            #[serde(default)]
            config: HashMap<String, String>,
        }

        let announcement: Announcement = match serde_json::from_slice(payload) {
            Ok(a) => a,
            Err(e) => {
                error!("Failed to parse discovery announcement: {}", e);
                return;
            }
        };

        info!(
            "Discovered device: {} (type: {})",
            device_id, announcement.device_type
        );

        let now = chrono::Utc::now();

        // Add to devices list
        let mut devices = self.devices.write().await;
        if !devices.contains(&device_id) {
            devices.push(device_id.clone());
        }
        drop(devices);

        // Store device type mapping
        let mut types = self.device_types.write().await;
        types.insert(device_id.clone(), announcement.device_type.clone());
        drop(types);

        // Register device with registry
        let config = DeviceConfig {
            device_id: device_id.clone(),
            name: announcement
                .name
                .clone()
                .unwrap_or_else(|| device_id.clone()),
            device_type: announcement.device_type.clone(),
            adapter_type: "mqtt".to_string(),
            connection_config: crate::registry::ConnectionConfig::default(),
            adapter_id: Some(self.config.name.clone()),
        };

        if let Err(e) = self
            .device_registry
            .read()
            .await
            .register_device(config)
            .await
        {
            warn!("Failed to register discovered device: {}", e);
        }

        // Publish discovery event
        if let Err(e) = self.event_tx.send(DeviceEvent::Discovery {
            device: DiscoveredDeviceInfo {
                device_id: device_id.clone(),
                device_type: announcement.device_type.clone(),
                name: announcement.name,
                endpoint: None,
                capabilities: Vec::new(),
                timestamp: now.timestamp(),
                metadata: serde_json::json!({}),
            },
        }) {
            error!(
                "Failed to send discovery event to channel: {} - no receivers?",
                e
            );
        }

        // Publish to EventBus if available
        if let Some(bus) = &self.event_bus {
            bus.publish(NeoMindEvent::DeviceOnline {
                device_id: device_id.clone(),
                device_type: announcement.device_type,
                timestamp: now.timestamp(),
            })
            .await;
        }
    }

    /// Handle uplink message from device.
    async fn handle_uplink_message(&self, device_id: String, device_type: String, payload: &[u8]) {
        let now = chrono::Utc::now();

        // Add to devices list if not already present
        {
            let mut devices = self.devices.write().await;
            if !devices.contains(&device_id) {
                devices.push(device_id.clone());

                // Publish online event for new device
                if let Some(bus) = &self.event_bus {
                    bus.publish(NeoMindEvent::DeviceOnline {
                        device_id: device_id.clone(),
                        device_type: device_type.clone(),
                        timestamp: now.timestamp(),
                    })
                    .await;
                }
            }
        }

        // Try to parse as JSON
        let json_value = match serde_json::from_slice::<serde_json::Value>(payload) {
            Ok(v) => v,
            Err(_) => {
                // Not JSON, try single value parsing and store
                match self.parse_payload(payload) {
                    Ok(value) => {
                        // Store raw and single value
                        self.emit_metric_event(
                            device_id.clone(),
                            "_raw".to_string(),
                            self.payload_to_raw_metric(payload),
                            now,
                        )
                        .await;
                        self.emit_metric_event(device_id, "value".to_string(), value, now)
                            .await;
                    }
                    Err(e) => {
                        // Store raw even if parsing fails
                        self.emit_metric_event(
                            device_id.clone(),
                            "_raw".to_string(),
                            self.payload_to_raw_metric(payload),
                            now,
                        )
                        .await;
                        warn!("Failed to parse uplink payload from {}: {}", device_id, e);
                    }
                }
                return;
            }
        };

        // Use UnifiedExtractor for consistent data processing
        let result = self
            .extractor
            .extract(&device_id, &device_type, &json_value)
            .await;

        // Log extracted metrics for debugging
        for metric in &result.metrics {
            let value_preview = match &metric.value {
                MetricValue::String(s) => format!("String(len={})", s.len()),
                MetricValue::Float(f) => format!("Float({})", f),
                MetricValue::Integer(i) => format!("Integer({})", i),
                MetricValue::Boolean(b) => format!("Boolean({})", b),
                MetricValue::Null => "Null".to_string(),
                MetricValue::Binary(b) => format!("Binary(len={})", b.len()),
                MetricValue::Array(a) => format!("Array(len={})", a.len()),
            };
            debug!(
                "Extracted metric for device '{}': {} = {}",
                device_id, metric.name, value_preview
            );
        }

        debug!(
            "Extraction result for device '{}': mode={:?}, metrics={}",
            device_id,
            result.mode,
            result.metrics.len()
        );

        // Emit all extracted metrics
        for metric in result.metrics {
            self.emit_metric_event(device_id.clone(), metric.name, metric.value, now)
                .await;
        }

        // Log warnings if any
        for warning in &result.warnings {
            warn!("Extraction warning for device '{}': {}", device_id, warning);
        }
    }

    /// Convert payload to a raw metric value (for _raw storage).
    fn payload_to_raw_metric(&self, payload: &[u8]) -> MetricValue {
        if let Ok(json_str) = std::str::from_utf8(payload) {
            if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(json_str) {
                MetricValue::String(
                    serde_json::to_string(&json_value).unwrap_or_else(|_| json_str.to_string()),
                )
            } else {
                MetricValue::String(json_str.to_string())
            }
        } else {
            // Binary data - store as base64
            MetricValue::String(BASE64.encode(payload))
        }
    }

    /// Emit a metric event to both channels and telemetry storage.
    /// EventBus publishing is handled by the event forwarding task to avoid duplicates.
    async fn emit_metric_event(
        &self,
        device_id: String,
        metric_name: String,
        value: MetricValue,
        timestamp: chrono::DateTime<chrono::Utc>,
    ) {
        // Log event emission for debugging
        let value_type = match &value {
            MetricValue::String(s) => format!("String(len={})", s.len()),
            MetricValue::Float(f) => format!("Float({})", f),
            MetricValue::Integer(i) => format!("Integer({})", i),
            MetricValue::Boolean(b) => format!("Boolean({})", b),
            MetricValue::Null => "Null".to_string(),
            MetricValue::Binary(b) => format!("Binary(len={})", b.len()),
            MetricValue::Array(a) => format!("Array(len={})", a.len()),
        };
        debug!(
            "Emitting metric event: device_id={}, metric={}, value={}",
            device_id, metric_name, value_type
        );

        // Update metric cache
        {
            let mut cache = self.metric_cache.write().await;
            cache
                .entry(device_id.clone())
                .or_default()
                .insert(metric_name.clone(), (value.clone(), timestamp));
        }

        // Emit to device event channel - event forwarding task will publish to EventBus
        match self.event_tx.send(DeviceEvent::Metric {
            device_id: device_id.clone(),
            metric: metric_name.clone(),
            value: value.clone(),
            timestamp: timestamp.timestamp(),
        }) {
            Ok(_) => {
                trace!(
                    "Sent metric event to channel: {}/{}",
                    device_id,
                    metric_name
                );
            }
            Err(_e) => {
                error!(
                    "Failed to send metric event to channel: {}/{} - no receivers?",
                    device_id, metric_name
                );
            }
        }

        // Store to time series storage
        {
            let storage_guard = self.telemetry_storage.read().await;
            if let Some(storage) = storage_guard.as_ref() {
                let data_point = crate::telemetry::DataPoint {
                    timestamp: timestamp.timestamp(),
                    value: value.clone(),
                    quality: None,
                };
                if let Err(e) = storage.write(&device_id, &metric_name, data_point).await {
                    error!(
                        "Failed to write telemetry for {}/{}: {}",
                        device_id, metric_name, e
                    );
                } else {
                    debug!(
                        "Successfully wrote telemetry for {}/{} at {}",
                        device_id,
                        metric_name,
                        timestamp.timestamp()
                    );
                }
            } else {
                warn!(
                    "Telemetry storage not set for MQTT adapter, cannot write metric {} for device {}",
                    metric_name, device_id
                );
            }
        }

        // Note: Do NOT publish DeviceMetric to EventBus here - the event forwarding task
        // in create_mqtt_adapter handles all EventBus publishing to avoid duplicates
    }

    /// Send a command via MQTT.
    /// Sends to ALL connected brokers - the device will receive from whichever broker it's connected to.
    async fn send_command_mqtt(
        &self,
        device_id: &str,
        command: &str,
        params: &HashMap<String, Value>,
    ) -> Result<(), AdapterError> {
        let clients = self.mqtt_clients.read().await;

        if clients.is_empty() {
            return Err(AdapterError::Connection(
                "No MQTT brokers connected".to_string(),
            ));
        }

        // Build topic: device/{device_type}/{device_id}/downlink
        // Or use default: {device_id}/command/{command}
        let device_type = self.device_types.read().await.get(device_id).cloned();

        let topic = if let Some(dt) = device_type {
            format!("device/{}/{}/downlink", dt, device_id)
        } else {
            format!("{}/command/{}", device_id, command)
        };

        // Build payload
        let payload = serde_json::to_string(params).map_err(|e| {
            AdapterError::Communication(format!("Failed to serialize params: {}", e))
        })?;

        // Send to all connected brokers
        let mut last_error = None;
        let mut success_count = 0;

        for (broker_id, inner) in clients.iter() {
            match inner
                .client
                .publish(
                    topic.clone(),
                    rumqttc::QoS::AtLeastOnce,
                    false,
                    payload.clone(),
                )
                .await
            {
                Ok(_) => {
                    success_count += 1;
                    info!(
                        "Sent command '{}' to device {} via broker {}",
                        command, device_id, broker_id
                    );
                }
                Err(e) => {
                    last_error = Some(AdapterError::Communication(format!(
                        "Failed to publish on {}: {}",
                        broker_id, e
                    )));
                }
            }
        }

        if success_count == 0 {
            Err(last_error.unwrap_or_else(|| {
                AdapterError::Communication("Failed to publish on any broker".to_string())
            }))
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl DeviceAdapter for MqttAdapter {
    fn name(&self) -> &str {
        &self.config.name
    }

    fn adapter_type(&self) -> &'static str {
        "mqtt"
    }

    fn is_running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn set_telemetry_storage(&self, storage: Arc<crate::TimeSeriesStorage>) {
        // Use a oneshot channel to ensure storage is set synchronously
        let telemetry_storage = self.telemetry_storage.clone();
        let (tx, rx) = std::sync::mpsc::channel::<()>();

        tokio::spawn(async move {
            *telemetry_storage.write().await = Some(storage);
            let _ = tx.send(());
        });

        // Wait for the storage to be set (with timeout)
        let _ = rx.recv_timeout(std::time::Duration::from_secs(5));
    }

    async fn start(&self) -> AdapterResult<()> {
        if self.is_running() {
            return Ok(());
        }

        info!("Starting MQTT adapter: {}", self.config.name);

        // Add the default broker from config
        self.add_broker(
            "default",
            &self.config.mqtt.broker,
            self.config.mqtt.port,
            self.config.mqtt.username.clone(),
            self.config.mqtt.password.clone(),
        )
        .await?;

        self.running
            .store(true, std::sync::atomic::Ordering::Relaxed);
        info!("MQTT adapter '{}' started", self.config.name);
        Ok(())
    }

    async fn stop(&self) -> AdapterResult<()> {
        info!("Stopping MQTT adapter: {}", self.config.name);

        self.running
            .store(false, std::sync::atomic::Ordering::Relaxed);

        // Stop all broker connections
        let mut clients = self.mqtt_clients.write().await;
        for inner in clients.values() {
            *inner.running.write().await = false;
        }
        clients.clear();

        *self.connection_status.write().await = ConnectionStatus::Disconnected;

        info!("MQTT adapter '{}' stopped", self.config.name);
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

    fn device_count(&self) -> usize {
        // Use try_read to avoid blocking in async runtime
        self.devices.try_read().map(|d| d.len()).unwrap_or(0)
    }

    fn list_devices(&self) -> Vec<String> {
        // Use try_read to avoid blocking in async runtime
        self.devices
            .try_read()
            .map(|d| d.clone())
            .unwrap_or_default()
    }

    async fn send_command(
        &self,
        device_id: &str,
        command_name: &str,
        payload: String,
        _topic: Option<String>,
    ) -> AdapterResult<()> {
        // Parse payload as JSON params
        let params: HashMap<String, Value> = serde_json::from_str(&payload).unwrap_or_default();

        self.send_command_mqtt(device_id, command_name, &params)
            .await
    }

    fn connection_status(&self) -> ConnectionStatus {
        // Use try_read to avoid blocking in async runtime
        // Return Disconnected if lock is contended (safe default)
        self.connection_status
            .try_read()
            .map(|s| *s)
            .unwrap_or(ConnectionStatus::Disconnected)
    }

    async fn subscribe_device(&self, device_id: &str) -> AdapterResult<()> {
        info!("subscribe_device called for device_id: {}", device_id);

        // Get the device configuration to find its telemetry topic
        let device_opt = self
            .device_registry
            .read()
            .await
            .get_device(device_id)
            .await;
        info!(
            "Device lookup result for {}: {:?}",
            device_id,
            device_opt.is_some()
        );

        if let Some(device) = device_opt {
            info!(
                "Found device: id={}, type={}",
                device.device_id, device.device_type
            );
            info!(
                "Connection config telemetry_topic: {:?}",
                device.connection_config.telemetry_topic
            );

            // Subscribe to the device's telemetry topic if configured
            if let Some(ref telemetry_topic) = device.connection_config.telemetry_topic {
                // Subscribe to the exact topic specified by the user
                self.subscribe_topic(telemetry_topic).await?;
                info!(
                    "Subscribed to device {} telemetry topic: {}",
                    device_id, telemetry_topic
                );
                // Store topic-to-device mapping for message routing
                let mut mapping = self.topic_to_device.write().await;
                mapping.insert(telemetry_topic.clone(), device_id.to_string());
                info!("Stored topic mapping: {} -> {}", telemetry_topic, device_id);
            } else {
                // Use default topic pattern if not specified: device/{device_type}/{device_id}/uplink
                let default_topic = format!("device/{}/{}+/uplink", device.device_type, device_id);
                self.subscribe_topic(&default_topic).await?;
                info!(
                    "No telemetry_topic specified for device {}, subscribed to default: {}",
                    device_id, default_topic
                );
            }

            // Store device type mapping for metric extraction
            {
                let mut types = self.device_types.write().await;
                types.insert(device_id.to_string(), device.device_type.clone());
                info!(
                    "Stored device type mapping: {} -> {}",
                    device_id, device.device_type
                );
            }

            // Also track the device
            let mut devices = self.devices.write().await;
            if !devices.contains(&device_id.to_string()) {
                devices.push(device_id.to_string());
            }
        } else {
            warn!(
                "Device {} not found in registry during subscribe_device",
                device_id
            );
            // If device not found in registry, use a wildcard pattern
            let topic = format!("device/+/{}+/uplink", device_id);
            self.subscribe_topic(&topic).await?;
            info!(
                "Device {} not found in registry, subscribed to wildcard topic: {}",
                device_id, topic
            );

            let mut devices = self.devices.write().await;
            if !devices.contains(&device_id.to_string()) {
                devices.push(device_id.to_string());
            }
        }
        Ok(())
    }

    async fn unsubscribe_device(&self, device_id: &str) -> AdapterResult<()> {
        // Get the device configuration to find its telemetry topic
        let device_opt = self
            .device_registry
            .read()
            .await
            .get_device(device_id)
            .await;
        if let Some(device) = device_opt {
            // Unsubscribe from the device's telemetry topic if configured
            if let Some(ref telemetry_topic) = device.connection_config.telemetry_topic {
                self.unsubscribe_topic(telemetry_topic).await?;
                info!(
                    "Unsubscribed from device {} telemetry topic: {}",
                    device_id, telemetry_topic
                );
                // Remove topic-to-device mapping
                let mut mapping = self.topic_to_device.write().await;
                mapping.remove(telemetry_topic);
            }
        }

        // Remove device from tracking
        let mut devices = self.devices.write().await;
        devices.retain(|d| d != device_id);
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl MqttAdapter {
    /// Dynamically subscribe to a topic on ALL connected brokers.
    /// This is used when a device is registered with a custom telemetry topic.
    pub async fn subscribe_topic(&self, topic: &str) -> AdapterResult<()> {
        let clients = self.mqtt_clients.read().await;
        if clients.is_empty() {
            warn!(
                "No MQTT brokers connected, cannot subscribe to topic: {}",
                topic
            );
            return Ok(());
        }

        let mut subscribed_count = 0;
        let mut last_error = None;

        for (broker_id, inner) in clients.iter() {
            // Check if already subscribed on this broker
            if inner.subscribed_topics.read().await.contains(topic) {
                debug!(
                    "Already subscribed to topic {} on broker {}",
                    topic, broker_id
                );
                subscribed_count += 1;
                continue;
            }

            match inner
                .client
                .subscribe(topic, rumqttc::QoS::AtLeastOnce)
                .await
            {
                Ok(_) => {
                    inner
                        .subscribed_topics
                        .write()
                        .await
                        .insert(topic.to_string());
                    subscribed_count += 1;
                    info!("Subscribed to topic {} on broker {}", topic, broker_id);
                }
                Err(e) => {
                    warn!(
                        "Failed to subscribe to {} on broker {}: {}",
                        topic, broker_id, e
                    );
                    last_error = Some(e);
                }
            }
        }

        if subscribed_count == 0 {
            if let Some(e) = last_error {
                return Err(AdapterError::Communication(format!(
                    "Failed to subscribe to {} on any broker: {}",
                    topic, e
                )));
            }
        } else {
            info!(
                "Subscribed to topic {} on {} broker(s)",
                topic, subscribed_count
            );
        }

        Ok(())
    }

    /// Unsubscribe from a topic on ALL connected brokers.
    pub async fn unsubscribe_topic(&self, topic: &str) -> AdapterResult<()> {
        let clients = self.mqtt_clients.read().await;

        for (broker_id, inner) in clients.iter() {
            if !inner.subscribed_topics.read().await.contains(topic) {
                continue;
            }

            match inner.client.unsubscribe(topic).await {
                Ok(_) => {
                    inner.subscribed_topics.write().await.remove(topic);
                    info!("Unsubscribed from topic {} on broker {}", topic, broker_id);
                }
                Err(e) => {
                    warn!(
                        "Failed to unsubscribe from {} on broker {}: {}",
                        topic, broker_id, e
                    );
                }
            }
        }

        Ok(())
    }

    /// Handle MQTT notification from a specific broker.
    /// This is a static method that processes incoming messages.
    async fn handle_mqtt_notification(
        notification: rumqttc::Event,
        config: &MqttAdapterConfig,
        event_tx: &broadcast::Sender<DeviceEvent>,
        event_bus: &Option<Arc<EventBus>>,
        device_types: &Arc<RwLock<HashMap<String, String>>>,
        metric_cache: &Arc<
            RwLock<HashMap<String, HashMap<String, (MetricValue, chrono::DateTime<chrono::Utc>)>>>,
        >,
        telemetry_storage: &Arc<RwLock<Option<Arc<TimeSeriesStorage>>>>,
        _device_registry: &Arc<RwLock<Arc<DeviceRegistry>>>,
        _connection_status: &Arc<RwLock<ConnectionStatus>>,
        broker_id: &str,
        extractor: &Arc<UnifiedExtractor>,
        topic_to_device: &Arc<RwLock<HashMap<String, String>>>,
    ) {
        match notification {
            rumqttc::Event::Incoming(rumqttc::Packet::Publish(publish)) => {
                let topic = publish.topic.to_string();
                let payload = publish.payload.to_vec();

                info!(
                    "Received MQTT message on topic: {}, payload length: {}",
                    topic,
                    payload.len()
                );

                let now = chrono::Utc::now();

                // Check if this is a standard uplink format first
                let parts: Vec<&str> = topic.split('/').collect();
                let is_standard_uplink =
                    parts.len() >= 4 && parts[0] == "device" && parts.get(3) == Some(&"uplink");

                // For standard uplink format, handle normally
                if is_standard_uplink {
                    // Extract device ID
                    let device_id = extract_device_id_from_topic(&topic, config);
                    if let Some(device_id) = device_id {
                        info!("Extracted device_id: {} from topic: {}", device_id, topic);

                        // Extract device type from topic
                        let device_type = extract_device_type_from_topic(&topic);

                        // Check if this is an uplink message with device_type
                        // Topic format: device/{device_type}/{device_id}/uplink
                        if let Some(dt) = &device_type {
                            // Try to parse as JSON
                            if let Ok(json_value) =
                                serde_json::from_slice::<serde_json::Value>(&payload)
                            {
                                info!(
                                    "Processing uplink message for device {} (type: {})",
                                    device_id, dt
                                );

                                // Use UnifiedExtractor to extract metrics
                                let result = extractor.extract(&device_id, dt, &json_value).await;

                                info!(
                                    "Extraction result for device '{}': mode={:?}, metrics={}",
                                    device_id,
                                    result.mode,
                                    result.metrics.len()
                                );

                                // Store device type mapping
                                {
                                    let mut types = device_types.write().await;
                                    types.insert(device_id.clone(), dt.to_string());
                                }

                                // Emit all extracted metrics
                                for metric in result.metrics {
                                    // Update metric cache
                                    {
                                        let mut cache = metric_cache.write().await;
                                        cache.entry(device_id.clone()).or_default().insert(
                                            metric.name.clone(),
                                            (metric.value.clone(), now),
                                        );
                                    }

                                    // Store in telemetry storage
                                    if let Some(storage) = telemetry_storage.read().await.as_ref() {
                                        let data_point = crate::telemetry::DataPoint {
                                            timestamp: now.timestamp(),
                                            value: metric.value.clone(),
                                            quality: None,
                                        };
                                        if let Err(e) = storage
                                            .write(&device_id, &metric.name, data_point)
                                            .await
                                        {
                                            error!(
                                                "Failed to write telemetry for {}/{}: {}",
                                                device_id, metric.name, e
                                            );
                                        } else {
                                            debug!(
                                                "Stored metric {} = {:?} for device {}",
                                                metric.name, metric.value, device_id
                                            );
                                        }
                                    }

                                    // Emit to device event channel - event forwarding task will publish to EventBus
                                    if let Err(e) = event_tx.send(DeviceEvent::Metric {
                                        device_id: device_id.clone(),
                                        metric: metric.name.clone(),
                                        value: metric.value.clone(),
                                        timestamp: now.timestamp(),
                                    }) {
                                        error!(
                                            "Failed to send metric event to channel: {}/{} - {}",
                                            device_id, metric.name, e
                                        );
                                    }

                                    // Note: Do NOT publish to EventBus here - the event forwarding task
                                    // in create_mqtt_adapter handles all EventBus publishing to avoid duplicates
                                }

                                // Publish DeviceOnline event for new devices
                                if let Some(bus) = event_bus {
                                    bus.publish(NeoMindEvent::DeviceOnline {
                                        device_id: device_id.clone(),
                                        device_type: dt.to_string(),
                                        timestamp: now.timestamp(),
                                    })
                                    .await;
                                    info!(
                                        "Publishing DeviceOnline to EventBus: device_id={}, device_type={}",
                                        device_id, dt
                                    );
                                }

                                return;
                            } else {
                                warn!(
                                    "Failed to parse uplink payload as JSON for device {}",
                                    device_id
                                );
                            }
                        }
                    }

                    // Fall back to simple metric extraction for non-uplink messages
                    // This requires a device_id to be extractable from the topic
                    let device_id_for_fallback = extract_device_id_from_topic(&topic, config);
                    if let Some(device_id) = device_id_for_fallback {
                        if let Ok(value) = MqttAdapter::default_parse_value(&payload) {
                            let metric_name = extract_metric_name_from_topic(&topic)
                                .unwrap_or_else(|| "value".to_string());

                            // Update metric cache
                            {
                                let mut cache = metric_cache.write().await;
                                cache
                                    .entry(device_id.clone())
                                    .or_default()
                                    .insert(metric_name.clone(), (value.clone(), now));
                            }

                            // Store in telemetry storage
                            if let Some(storage) = telemetry_storage.read().await.as_ref() {
                                let data_point = crate::telemetry::DataPoint {
                                    timestamp: now.timestamp(),
                                    value: value.clone(),
                                    quality: None,
                                };
                                let _ = storage.write(&device_id, &metric_name, data_point).await;
                            }

                            // Emit event to device event channel - event forwarding task will publish to EventBus
                            if let Err(e) = event_tx.send(DeviceEvent::Metric {
                                device_id: device_id.clone(),
                                metric: metric_name.clone(),
                                value: value.clone(),
                                timestamp: now.timestamp(),
                            }) {
                                error!(
                                    "Failed to send metric event to channel: {}/{} - {}",
                                    device_id, metric_name, e
                                );
                            }

                            // Note: Do NOT publish DeviceMetric to EventBus here - the event forwarding task
                            // in create_mqtt_adapter handles all EventBus publishing to avoid duplicates

                            // Publish device online event - extract device_type from topic if available
                            // This is NOT duplicated by the forwarding task
                            if let Some(bus) = event_bus {
                                let device_type = extract_device_type_from_topic(&topic);
                                info!(
                                "Publishing DeviceOnline to EventBus: device_id={}, device_type={:?}",
                                device_id, device_type
                            );
                                bus.publish(NeoMindEvent::DeviceOnline {
                                    device_id: device_id.clone(),
                                    device_type: device_type
                                        .unwrap_or_else(|| "unknown".to_string()),
                                    timestamp: now.timestamp(),
                                })
                                .await;
                            } else {
                                warn!(
                                "EventBus is None in handle_mqtt_notification - cannot publish DeviceOnline"
                            );
                            }
                        }
                    } // Close: if let Some(device_id)
                } // Close: if is_standard_uplink

                // Auto-onboarding: For non-standard topics, trigger auto-discovery
                // This handles arbitrary MQTT topics like "device12asdas"
                // Supports both JSON and binary/hex data
                // Only trigger if NOT a standard uplink format (those are handled above)
                if !is_standard_uplink {
                    // First check if this topic belongs to a registered device
                    let device_id_opt = {
                        let mapping = topic_to_device.read().await;
                        debug!(
                            "Checking topic_to_device mapping for topic '{}': {} entries",
                            topic,
                            mapping.len()
                        );
                        if !mapping.contains_key(&topic) {
                            debug!(
                                "Topic '{}' not found in mapping, triggering auto-onboarding",
                                topic
                            );
                        }
                        mapping.get(&topic).cloned()
                    };

                    if let Some(ref device_id) = device_id_opt {
                        info!(
                            "Routing message for registered device {} from topic {}",
                            device_id, topic
                        );

                        // Try to get device type from device_types cache
                        let device_type_opt = {
                            let types = device_types.read().await;
                            types.get(device_id).cloned()
                        };

                        info!("Device type for {}: {:?}", device_id, device_type_opt);

                        // Parse payload and process for the registered device
                        if let Ok(json_data) = serde_json::from_slice::<serde_json::Value>(&payload)
                        {
                            info!("Successfully parsed JSON payload for device {}", device_id);

                            // Use UnifiedExtractor with the full JSON data
                            // The extractor handles dot-notation paths including "data.field" prefixes
                            // DO NOT pre-extract the "data" field - it causes double-extraction issues
                            if let Some(dt) = device_type_opt {
                                let result = extractor.extract(device_id, &dt, &json_data).await;
                                info!(
                                    "Extraction result for device {}: mode={:?}, metrics={}",
                                    device_id,
                                    result.mode,
                                    result.metrics.len()
                                );

                                if result.metrics.is_empty() {
                                    warn!(
                                        "No metrics extracted for device {} (type: {}). raw_stored={}",
                                        device_id, dt, result.raw_stored
                                    );
                                }

                                for metric in result.metrics {
                                    // Update metric cache
                                    {
                                        let mut cache = metric_cache.write().await;
                                        cache.entry(device_id.clone()).or_default().insert(
                                            metric.name.clone(),
                                            (metric.value.clone(), now),
                                        );
                                    }

                                    // Store in telemetry storage
                                    if let Some(storage) = telemetry_storage.read().await.as_ref() {
                                        let data_point = crate::telemetry::DataPoint {
                                            timestamp: now.timestamp(),
                                            value: metric.value.clone(),
                                            quality: None,
                                        };
                                        if let Err(e) =
                                            storage.write(device_id, &metric.name, data_point).await
                                        {
                                            error!(
                                                "Failed to write telemetry for {}/{}: {}",
                                                device_id, metric.name, e
                                            );
                                        }
                                    }

                                    // Emit to device event channel - event forwarding task will publish to EventBus
                                    // This ensures single publish path to avoid duplicate events
                                    if let Err(e) = event_tx.send(DeviceEvent::Metric {
                                        device_id: device_id.clone(),
                                        metric: metric.name.clone(),
                                        value: metric.value.clone(),
                                        timestamp: now.timestamp(),
                                    }) {
                                        error!(
                                            "Failed to send metric event to channel: {}/{} - {}",
                                            device_id, metric.name, e
                                        );
                                    }
                                }
                            } else {
                                // No device type - try simple value extraction
                                if let Ok(value) = MqttAdapter::default_parse_value(&payload) {
                                    let metric_name = "value";

                                    // Update metric cache
                                    {
                                        let mut cache = metric_cache.write().await;
                                        cache
                                            .entry(device_id.clone())
                                            .or_default()
                                            .insert(metric_name.to_string(), (value.clone(), now));
                                    }

                                    // Store in telemetry storage
                                    if let Some(storage) = telemetry_storage.read().await.as_ref() {
                                        let data_point = crate::telemetry::DataPoint {
                                            timestamp: now.timestamp(),
                                            value: value.clone(),
                                            quality: None,
                                        };
                                        let _ =
                                            storage.write(device_id, metric_name, data_point).await;
                                    }

                                    // Emit to device event channel - event forwarding task will publish to EventBus
                                    if let Err(e) = event_tx.send(DeviceEvent::Metric {
                                        device_id: device_id.clone(),
                                        metric: metric_name.to_string(),
                                        value: value.clone(),
                                        timestamp: now.timestamp(),
                                    }) {
                                        error!(
                                            "Failed to send metric event to channel: {}/{} - {}",
                                            device_id, metric_name, e
                                        );
                                    }
                                    // Note: Do NOT publish DeviceMetric to EventBus here - the event forwarding task handles it
                                }
                            }
                        }
                        // Skip auto-onboarding for registered devices - message already handled
                    } else {
                        // Check if this message came from a configured subscription topic
                        // If yes, skip auto-onboarding since the user explicitly subscribed to it
                        let is_from_subscription = config
                            .subscribe_topics
                            .iter()
                            .any(|pattern| match_topic_pattern_helper(&topic, pattern).is_some());

                        if !is_from_subscription {
                            // Not from subscription topics - trigger auto-onboarding
                            info!(
                                "Triggering auto-onboarding for non-standard topic: {}",
                                topic
                            );

                            // Generate a device_id for auto-discovery
                            // Try to extract from topic, or use a hash-based ID
                            let auto_device_id = extract_device_id_from_topic(&topic, config)
                                .unwrap_or_else(|| {
                                    // Use topic hash as device_id
                                    format!("mqtt_{}", {
                                        use std::collections::hash_map::DefaultHasher;
                                        use std::hash::{Hash, Hasher};
                                        let mut hasher = DefaultHasher::new();
                                        topic.hash(&mut hasher);
                                        format!("{:x}", hasher.finish())
                                    })
                                });

                            // Determine data format and prepare sample
                            // Extract the actual device data from payload.data if it exists
                            let (sample_data, is_binary, data_format) = if let Ok(json_data) =
                                serde_json::from_slice::<serde_json::Value>(&payload)
                            {
                                // Check if payload has a 'data' field containing the actual device data
                                let actual_data = json_data.get("data").unwrap_or(&json_data);
                                (actual_data.clone(), false, "json")
                            } else {
                                // Not JSON - store as base64 encoded binary data
                                (serde_json::json!(BASE64.encode(&payload)), true, "base64")
                            };

                            // Publish unknown_device_data event for auto-onboarding
                            if let Some(bus) = event_bus {
                                let sample = serde_json::json!({
                                    "device_id": auto_device_id,
                                    "timestamp": chrono::Utc::now().timestamp(),
                                    "topic": topic,
                                    "data": sample_data,
                                    "format": data_format,
                                    "is_binary": is_binary
                                });

                                // Use the adapter's name as adapter_id for auto-onboarding
                                // This matches the adapter_id used during registration
                                let adapter_id = config.name.clone();

                                bus.publish(NeoMindEvent::Custom {
                                    event_type: "unknown_device_data".to_string(),
                                    data: serde_json::json!({
                                        "device_id": auto_device_id,
                                        "source": "mqtt",
                                        "broker_id": broker_id,
                                        "adapter_id": adapter_id,
                                        "original_topic": topic,
                                        "sample": sample,
                                        "is_binary": is_binary
                                    }),
                                })
                                .await;
                            }
                        }
                    }
                }
            }
            rumqttc::Event::Incoming(rumqttc::Packet::ConnAck(connack)) => {
                info!(
                    "MQTT broker {} connection acknowledged - session present: {}",
                    broker_id, connack.session_present
                );
            }
            rumqttc::Event::Incoming(rumqttc::Packet::SubAck(suback)) => {
                info!(
                    "MQTT broker {} subscription acknowledged - packet id: {}, granted QoS: {:?}",
                    broker_id, suback.pkid, suback.return_codes
                );
            }
            _ => {}
        }
    }
}

/// Helper function to extract device ID from topic.
fn extract_device_id_from_topic(topic: &str, config: &MqttAdapterConfig) -> Option<String> {
    let parts: Vec<&str> = topic.split('/').collect();

    // Try device/{device_type}/{device_id}/{direction} format first
    if parts.len() >= 4 && parts[0] == "device" {
        return Some(parts[2].to_string());
    }

    // Try to match against subscription patterns
    for pattern in &config.subscribe_topics {
        if let Some(id) = match_topic_pattern_helper(topic, pattern) {
            return Some(id);
        }
    }

    // Fallback: extract from common patterns
    if parts.len() >= 2 {
        Some(parts[1].to_string())
    } else {
        None
    }
}

/// Helper function to match topic pattern.
fn match_topic_pattern_helper(topic: &str, pattern: &str) -> Option<String> {
    let pattern_parts: Vec<&str> = pattern.split('/').collect();
    let topic_parts: Vec<&str> = topic.split('/').collect();

    if pattern_parts.len() != topic_parts.len() {
        return None;
    }

    let mut device_id = None;
    let mut matches = true;

    for (i, (p, t)) in pattern_parts.iter().zip(topic_parts.iter()).enumerate() {
        match *p {
            "+" => {
                if i == 1 {
                    device_id = Some(t.to_string());
                }
            }
            "#" => {}
            _ => {
                if p != t {
                    matches = false;
                    break;
                }
            }
        }
    }

    if matches {
        device_id
    } else {
        None
    }
}

/// Helper function to extract metric name from topic.
fn extract_metric_name_from_topic(topic: &str) -> Option<String> {
    let parts: Vec<&str> = topic.split('/').collect();
    if parts.len() >= 4 && parts[0] == "device" {
        return None; // metrics are in payload
    }
    if parts.len() >= 3 {
        Some(parts[2].to_string())
    } else {
        topic.split('/').next_back().map(|s| s.to_string())
    }
}

/// Helper function to extract device type from topic.
/// For topic format device/{device_type}/{device_id}/{direction}
fn extract_device_type_from_topic(topic: &str) -> Option<String> {
    let parts: Vec<&str> = topic.split('/').collect();
    if parts.len() >= 4 && parts[0] == "device" {
        Some(parts[1].to_string())
    } else {
        None
    }
}

/// Create an MQTT adapter connected to an event bus.
pub fn create_mqtt_adapter(
    config: MqttAdapterConfig,
    event_bus: &EventBus,
    device_registry: Arc<DeviceRegistry>,
) -> Arc<MqttAdapter> {
    // Convert &EventBus to Arc<EventBus> by creating a new Arc
    // Note: This assumes EventBus can be cloned safely
    let event_bus_arc = Arc::new(event_bus.clone());

    let adapter = Arc::new(
        MqttAdapter::new(config)
            .with_event_bus(event_bus_arc)
            .with_device_registry(device_registry),
    );

    let adapter_clone = adapter.clone();
    let event_bus = event_bus.clone();

    // Spawn event forwarding task
    tokio::spawn(async move {
        let mut rx = adapter_clone.subscribe();
        while let Some(event) = rx.next().await {
            if let Some(device_id) = event.device_id() {
                let source = format!("adapter:mqtt:{}", device_id);
                let neomind_event = event.clone().to_neomind_event();
                event_bus.publish_with_source(neomind_event, source).await;
            }
        }
    });

    adapter
}

/// Create an MQTT adapter with protocol mapping.
pub fn create_mqtt_adapter_with_mapping(
    config: MqttAdapterConfig,
    mapping: Arc<dyn ProtocolMapping>,
    event_bus: &EventBus,
    device_registry: Arc<DeviceRegistry>,
) -> Arc<MqttAdapter> {
    let event_bus_arc = Arc::new(event_bus.clone());

    let adapter = Arc::new(
        MqttAdapter::with_mapping(config, mapping, Some(event_bus_arc))
            .with_device_registry(device_registry),
    );

    let adapter_clone = adapter.clone();
    let event_bus = event_bus.clone();

    // Spawn event forwarding task
    tokio::spawn(async move {
        let mut rx = adapter_clone.subscribe();
        while let Some(event) = rx.next().await {
            if let Some(device_id) = event.device_id() {
                let source = format!("adapter:mqtt:{}", device_id);
                let neomind_event = event.clone().to_neomind_event();
                event_bus.publish_with_source(neomind_event, source).await;
            }
        }
    });

    adapter
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_topic_pattern() {
        assert_eq!(
            MqttAdapter::match_topic_pattern("sensor/temp1/temperature", "sensor/+/temperature"),
            Some("temp1".to_string())
        );
        assert_eq!(
            MqttAdapter::match_topic_pattern("sensor/temp1/humidity", "sensor/+/humidity"),
            Some("temp1".to_string())
        );
    }

    #[test]
    fn test_default_parse_value() {
        assert!(matches!(
            MqttAdapter::default_parse_value(b"25.5"),
            Ok(MetricValue::Float(25.5))
        ));
        assert!(matches!(
            MqttAdapter::default_parse_value(b"true"),
            Ok(MetricValue::Boolean(true))
        ));
        // Test string value parsing
        match MqttAdapter::default_parse_value(b"\"hello\"") {
            Ok(MetricValue::String(s)) => assert_eq!(s, "hello"),
            _ => panic!("Expected String value"),
        }
    }
}
