//! MQTT Device Manager with MDL Support
//!
//! This module provides a complete MQTT device management system with:
//! - Real MQTT client using rumqttc
//! - MDL-based device type definitions
//! - Device discovery via MQTT announcements
//! - Device lifecycle management
//! - Data streaming and storage

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

use super::mdl::{
    ConnectionStatus, DeviceError, DeviceId, MetricValue,
};
use super::mdl_format::{DeviceInstance, DeviceTypeDefinition, MdlRegistry, MdlStorage};
use super::registry::DeviceRegistry;
use super::telemetry::{DataPoint, TimeSeriesStorage};
use edge_ai_core::EventBus;

// HASS discovery functionality has been removed - providing stubs
// TODO: Re-implement HASS discovery if needed
fn discovery_subscription_patterns(_components: Option<Vec<String>>) -> Vec<String> {
    vec![]
}
fn is_discovery_topic(topic: &str) -> bool {
    topic.starts_with("homeassistant/") && topic.contains("/+/config/")
}
fn parse_discovery_message(_topic: &str, _payload: &[u8]) -> Result<serde_json::Value, String> {
    Err("HASS discovery deprecated".to_string())
}

/// Simple discovery announcement for internal NeoTalk discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiscoveryAnnouncement {
    pub device_type: String,
    pub device_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub config: std::collections::HashMap<String, String>,
}

/// Discovered HASS device (single entity)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredHassDevice {
    pub entity_id: String,
    pub name: Option<String>,
    pub component: String,
    pub discovery_topic: String,
    pub device_info: HashMap<String, String>,
    pub metric_count: usize,
    pub command_count: usize,
    pub discovered_at: chrono::DateTime<chrono::Utc>,
    /// Raw discovery message for re-processing
    pub raw_message: String,
}

/// Aggregated HASS device (multiple entities from the same physical device)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedHassDevice {
    /// Unique device identifier (from device.identifiers or derived)
    pub device_id: String,
    /// Device name (from device info or first entity)
    pub name: Option<String>,
    /// All entities belonging to this device
    pub entities: Vec<DiscoveredHassDevice>,
    /// Total metrics across all entities
    pub total_metrics: usize,
    /// Total commands across all entities
    pub total_commands: usize,
    /// Discovery topics for all entities
    pub discovery_topics: Vec<String>,
    /// Device info from first entity
    pub device_info: HashMap<String, String>,
}

impl DiscoveredHassDevice {
    /// Extract the device identifier from this entity's discovery data.
    /// Uses device.identifiers if available, otherwise derives from topic.
    pub fn get_device_identifier(&self) -> String {
        // Try to get device identifier from device_info
        if let Some(identifiers) = self.device_info.get("identifiers") {
            // Parse identifiers (stored as JSON array string)
            if let Ok(parsed) = serde_json::from_str::<Vec<String>>(identifiers)
                && let Some(first_id) = parsed.first() {
                    tracing::debug!(
                        "Entity {} using device.identifiers: {}",
                        self.entity_id,
                        first_id
                    );
                    return format!("hass_{}", first_id);
                }
        }

        // Derive from topic: homeassistant/sensor/hass-simulator-001/temperature/config
        // Extract the device ID (middle part for 5-part format)
        let parts: Vec<&str> = self.discovery_topic.split('/').collect();
        if parts.len() >= 5 && parts[0] == "homeassistant" {
            // 5-part format: homeassistant/<component>/<device_id>/<entity_id>/config
            let device_id = parts[2];
            tracing::debug!(
                "Entity {} using topic-derived device_id: {} from topic: {}",
                self.entity_id,
                device_id,
                self.discovery_topic
            );
            return format!("hass_{}", device_id);
        }

        // Fallback: use entity_id as device_id (no grouping possible)
        tracing::warn!(
            "Entity {} has no device.identifiers and cannot derive from topic, using entity_id as fallback",
            self.entity_id
        );
        format!("hass_{}", self.entity_id.replace('.', "_"))
    }

    /// Extract device name from device info
    pub fn get_device_name(&self) -> Option<String> {
        self.device_info.get("device_name").cloned()
    }
}

/// Aggregate HASS entities by device identifier (deprecated - returns empty)
pub fn aggregate_hass_devices(_entities: Vec<DiscoveredHassDevice>) -> Vec<AggregatedHassDevice> {
    // HASS discovery deprecated - stub implementation
    Vec::new()
}

/// Configuration for the MQTT Device Manager
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttManagerConfig {
    /// MQTT broker address
    pub broker: String,

    /// MQTT broker port
    #[serde(default = "default_broker_port")]
    pub port: u16,

    /// Client ID
    #[serde(default)]
    pub client_id: Option<String>,

    /// Username for authentication
    #[serde(default)]
    pub username: Option<String>,

    /// Password for authentication
    #[serde(default)]
    pub password: Option<String>,

    /// Keep-alive interval in seconds
    #[serde(default = "default_keep_alive")]
    pub keep_alive: u64,

    /// Discovery topic prefix
    #[serde(default = "default_discovery_prefix")]
    pub discovery_prefix: String,

    /// Auto-discovery enabled
    #[serde(default = "default_auto_discovery")]
    pub auto_discovery: bool,
}

fn default_broker_port() -> u16 {
    1883
}
fn default_keep_alive() -> u64 {
    60
}
fn default_discovery_prefix() -> String {
    "neotalk/discovery".to_string()
}
fn default_auto_discovery() -> bool {
    true
}

impl MqttManagerConfig {
    pub fn new(broker: impl Into<String>) -> Self {
        Self {
            broker: broker.into(),
            port: 1883,
            client_id: None,
            username: None,
            password: None,
            keep_alive: 60,
            discovery_prefix: "neotalk/discovery".to_string(),
            auto_discovery: true,
        }
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn with_auth(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        self.username = Some(username.into());
        self.password = Some(password.into());
        self
    }

    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = Some(client_id.into());
        self
    }

    pub fn full_broker_addr(&self) -> String {
        format!("{}:{}", self.broker, self.port)
    }
}

impl Default for MqttManagerConfig {
    fn default() -> Self {
        Self::new("localhost")
    }
}

/// MQTT Device Manager
///
/// Manages MQTT-based devices with MDL support for device type definitions.
/// Can be used for both embedded and external MQTT brokers.
///
/// ⚠️ **DEPRECATED**: This manager is now primarily used internally by `MqttManagerAdapter`.
/// For new code, use `DeviceService` and `DeviceAdapter` instead.
/// Direct access to `MqttDeviceManager` is only needed for HASS discovery and other MQTT-specific features.
/// See `MqttManagerAdapter` in `adapters/mqtt_manager_adapter.rs` for the adapter interface.
#[deprecated(
    note = "Use DeviceService and MqttManagerAdapter instead. MqttDeviceManager is now primarily an internal implementation detail. Only access directly for HASS discovery and other MQTT-specific features."
)]
pub struct MqttDeviceManager {
    /// Broker identifier (e.g., "internal-mqtt", "broker-1", etc.)
    broker_id: String,

    /// Configuration
    config: MqttManagerConfig,

    /// MDL Registry for device type definitions
    mdl_registry: Arc<MdlRegistry>,

    /// Device registry for looking up device configurations (including custom telemetry topics)
    device_registry: Option<Arc<DeviceRegistry>>,

    /// Device instances indexed by device_id
    devices: Arc<RwLock<HashMap<String, DeviceInstance>>>,

    /// Mapping from telemetry topic to device_id
    /// This handles custom topics like "ashuau" that don't match the standard pattern
    topic_to_device: Arc<RwLock<HashMap<String, String>>>,

    /// MQTT client (when connected)
    mqtt_client: Arc<RwLock<Option<MqttClientInner>>>,

    /// Connection status
    connection_status: Arc<RwLock<ConnectionStatus>>,

    /// Metric values cache (device_id -> metric_name -> (value, timestamp))
    metric_cache:
        Arc<RwLock<HashMap<String, HashMap<String, (MetricValue, chrono::DateTime<chrono::Utc>)>>>>,

    /// Time series storage for telemetry data
    time_series_storage: Arc<RwLock<Option<Arc<TimeSeriesStorage>>>>,

    /// Storage directory
    storage_dir: Option<String>,

    /// Initialization flag to prevent double initialization
    initialized: Arc<RwLock<bool>>,

    /// HASS discovery enabled
    hass_discovery_enabled: Arc<RwLock<bool>>,

    /// Discovered HASS devices (entity_id -> device)
    hass_discovered_devices: Arc<RwLock<HashMap<String, DiscoveredHassDevice>>>,

    /// Mapping from HASS state_topic to (device_id, metric_name) for handling state updates
    /// For aggregated devices with multiple metrics, each state_topic maps to its specific metric
    hass_state_topic_map: Arc<RwLock<HashMap<String, (String, String)>>>,

    /// Device timeout monitor task handle
    timeout_monitor_handle: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,

    /// Event bus for publishing device events (optional)
    event_bus: Option<Arc<EventBus>>,
}

/// Inner MQTT client holder
struct MqttClientInner {
    client: rumqttc::AsyncClient,
    /// Running flag for the event loop task
    running: Arc<RwLock<bool>>,
}

impl MqttDeviceManager {
    /// Create a new MQTT device manager
    pub fn new(broker_id: impl Into<String>, config: MqttManagerConfig) -> Self {
        let broker_id = broker_id.into();
        Self {
            broker_id,
            config,
            mdl_registry: Arc::new(MdlRegistry::new()),
            device_registry: None,
            devices: Arc::new(RwLock::new(HashMap::new())),
            topic_to_device: Arc::new(RwLock::new(HashMap::new())),
            mqtt_client: Arc::new(RwLock::new(None)),
            connection_status: Arc::new(RwLock::new(ConnectionStatus::Disconnected)),
            metric_cache: Arc::new(RwLock::new(HashMap::new())),
            time_series_storage: Arc::new(RwLock::new(None)),
            storage_dir: None,
            initialized: Arc::new(RwLock::new(false)),
            hass_discovery_enabled: Arc::new(RwLock::new(false)),
            hass_discovered_devices: Arc::new(RwLock::new(HashMap::new())),
            hass_state_topic_map: Arc::new(RwLock::new(HashMap::new())),
            timeout_monitor_handle: Arc::new(RwLock::new(None)),
            event_bus: None,
        }
    }

    /// Set the device registry for looking up device configurations
    pub fn with_device_registry(mut self, registry: Arc<DeviceRegistry>) -> Self {
        self.device_registry = Some(registry);
        self
    }

    /// Set the event bus for publishing device events
    pub fn with_event_bus(mut self, event_bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    /// Get the broker ID
    pub fn broker_id(&self) -> &str {
        &self.broker_id
    }

    /// Set storage directory for persistence
    pub fn with_storage_dir(mut self, dir: impl Into<String>) -> Self {
        self.storage_dir = Some(dir.into());
        self
    }

    /// Get the MDL registry
    pub fn mdl_registry(&self) -> Arc<MdlRegistry> {
        self.mdl_registry.clone()
    }

    /// Get the time series storage (if initialized)
    pub async fn time_series_storage(&self) -> Option<Arc<TimeSeriesStorage>> {
        self.time_series_storage.read().await.clone()
    }

    /// Initialize the manager with storage
    /// This method is idempotent - calling it multiple times is safe.
    pub async fn initialize(&self) -> Result<(), DeviceError> {
        // Check if already initialized
        {
            let initialized = self.initialized.read().await;
            if *initialized {
                return Ok(());
            }
        }

        // Use configured storage dir or default to "data"
        let storage_dir = self
            .storage_dir.as_deref()
            .unwrap_or("data");
        let db_path = std::path::Path::new(storage_dir).join("devices.redb");

        // Try to open storage, fall back to memory if it fails
        let storage = match MdlStorage::open(&db_path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    "Failed to open MDL storage at {:?}: {}, using in-memory",
                    db_path,
                    e
                );
                // Create in-memory storage
                MdlStorage::memory()?
            }
        };

        self.mdl_registry.set_storage(storage).await;

        // Initialize time series storage for telemetry data
        let telemetry_path = std::path::Path::new(storage_dir).join("telemetry.redb");
        let ts_storage = match TimeSeriesStorage::open(&telemetry_path) {
            Ok(ts) => ts,
            Err(e) => {
                tracing::warn!(
                    "Failed to open telemetry storage at {:?}: {}, using in-memory",
                    telemetry_path,
                    e
                );
                TimeSeriesStorage::memory()?
            }
        };
        *self.time_series_storage.write().await = Some(Arc::new(ts_storage));
        tracing::info!("Telemetry storage initialized at {:?}", telemetry_path);

        // Load existing device types
        self.mdl_registry.load_from_storage().await?;

        // Load existing device instances
        self.load_device_instances().await?;

        tracing::info!("MDL storage initialized at {:?}", db_path);

        // Mark as initialized
        *self.initialized.write().await = true;

        Ok(())
    }

    /// Load device instances from storage
    async fn load_device_instances(&self) -> Result<(), DeviceError> {
        let storage_guard = self.mdl_registry.storage.read().await;
        if let Some(storage) = storage_guard.as_ref() {
            let instances = storage.load_all_device_instances().await?;
            let ts_storage = self.time_series_storage.read().await.clone();

            for instance in instances {
                let device_id = instance.device_id.clone();
                let device_type = instance.device_type.clone();

                // Keep the persisted status instead of resetting to Offline
                let mut updated_instance = instance.clone();

                // Restore current_values from time-series storage
                // Load the latest value for each metric defined in the device type
                if let Some(ts) = &ts_storage
                    && let Some(dt) = self.mdl_registry.get(&device_type).await {
                        let mut restored_values = std::collections::HashMap::new();
                        for metric in &dt.uplink.metrics {
                            if let Ok(Some(latest)) = ts.latest(&device_id, &metric.name).await {
                                let timestamp = chrono::DateTime::<chrono::Utc>::from_timestamp(
                                    latest.timestamp,
                                    0,
                                )
                                .unwrap_or_else(chrono::Utc::now);
                                restored_values
                                    .insert(metric.name.clone(), (latest.value, timestamp));
                            }
                        }
                        let restored_count = restored_values.len();
                        updated_instance.current_values = restored_values;
                        if restored_count > 0 {
                            tracing::info!(
                                "Restored {} current values for device {}",
                                restored_count,
                                device_id
                            );
                        }
                    }

                // Also update metric cache
                let mut cache = self.metric_cache.write().await;
                let cache_entry = cache.entry(device_id.clone()).or_default();
                for (metric_name, (value, timestamp)) in &updated_instance.current_values {
                    cache_entry.insert(metric_name.clone(), (value.clone(), *timestamp));
                }
                drop(cache);

                let mut devices = self.devices.write().await;
                devices.insert(device_id.clone(), updated_instance);
                drop(devices);
            }

            tracing::info!(
                "Loaded {} device instances from storage",
                self.devices.read().await.len()
            );

            // Restore HASS state topic mappings from device configs
            let _ = self.restore_hass_state_topic_mappings().await;
        }

        Ok(())
    }

    /// Save a device instance to storage
    async fn save_device_instance(&self, instance: &DeviceInstance) -> Result<(), DeviceError> {
        let storage_guard = self.mdl_registry.storage.read().await;
        if let Some(storage) = storage_guard.as_ref() {
            storage.save_device_instance(instance).await?;
        }
        Ok(())
    }

    /// Delete a device instance from storage
    async fn delete_device_instance(&self, device_id: &str) -> Result<(), DeviceError> {
        let storage_guard = self.mdl_registry.storage.read().await;
        if let Some(storage) = storage_guard.as_ref() {
            storage.delete_device_instance(device_id).await?;
        }
        Ok(())
    }

    /// Connect to MQTT broker and start processing messages
    pub async fn connect(&self) -> Result<(), DeviceError> {
        *self.connection_status.write().await = ConnectionStatus::Connecting;

        // Load saved HASS discovery state
        let saved_enabled = self.load_hass_discovery_state().await;
        *self.hass_discovery_enabled.write().await = saved_enabled;
        if saved_enabled {
            tracing::info!("HASS discovery state loaded: enabled");
        }

        let mut mqttoptions = rumqttc::MqttOptions::new(
            self.config
                .client_id
                .clone()
                .unwrap_or_else(|| format!("neotalk_mgr_{}", Uuid::new_v4())),
            &self.config.broker,
            self.config.port,
        );

        // Set maximum packet size to 10MB to handle large payloads
        // Default is much smaller and causes connection resets
        mqttoptions.set_max_packet_size(10 * 1024 * 1024, 10 * 1024 * 1024);

        mqttoptions.set_keep_alive(Duration::from_secs(self.config.keep_alive));

        if let (Some(u), Some(p)) = (&self.config.username, &self.config.password) {
            mqttoptions.set_credentials(u, p);
        }

        let (client, eventloop) = rumqttc::AsyncClient::new(mqttoptions, 10);

        // Subscribe to discovery topics if auto-discovery is enabled
        if self.config.auto_discovery {
            let discovery_topic = format!("{}/#", self.config.discovery_prefix);
            client
                .subscribe(discovery_topic, rumqttc::QoS::AtLeastOnce)
                .await
                .map_err(|e| DeviceError::Communication(e.to_string()))?;
        }

        // Subscribe to all device uplink/downlink topics
        // Topic format: device/{device_type}/{device_id}/uplink or /downlink
        client
            .subscribe("device/+/+/uplink", rumqttc::QoS::AtLeastOnce)
            .await
            .map_err(|e| DeviceError::Communication(e.to_string()))?;
        client
            .subscribe("device/+/+/downlink", rumqttc::QoS::AtLeastOnce)
            .await
            .map_err(|e| DeviceError::Communication(e.to_string()))?;

        // Subscribe to ALL topics for auto-discovery
        // This captures any device data regardless of topic format
        // Messages that don't match registered devices will trigger auto-onboarding
        client
            .subscribe("#", rumqttc::QoS::AtLeastOnce)
            .await
            .map_err(|e| DeviceError::Communication(e.to_string()))?;
        tracing::info!("Subscribed to '#' - all MQTT topics for auto-discovery");

        // Subscribe to HASS discovery topics if enabled
        if saved_enabled {
            // Subscribe to both 4-part and 5-part topic formats
            let hass_topics = discovery_subscription_patterns(None);
            for hass_topic in &hass_topics {
                client
                    .subscribe(hass_topic, rumqttc::QoS::AtLeastOnce)
                    .await
                    .map_err(|e| DeviceError::Communication(e.to_string()))?;
                tracing::info!("HASS discovery auto-started: subscribed to {}", hass_topic);
            }
        }

        let running = Arc::new(RwLock::new(true));
        *self.mqtt_client.write().await = Some(MqttClientInner {
            client,
            running: running.clone(),
        });

        *self.connection_status.write().await = ConnectionStatus::Connected;

        // Restore topic_to_device mappings from device registry
        // This is critical for server restart - devices must be able to receive data
        if let Some(registry) = &self.device_registry {
            let devices = registry.list_devices().await;
            let mut mapping = self.topic_to_device.write().await;
            let mut restored_count = 0;

            for device in devices {
                if let Some(telemetry_topic) = &device.connection_config.telemetry_topic {
                    mapping.insert(telemetry_topic.clone(), device.device_id.clone());
                    restored_count += 1;
                    tracing::debug!(
                        "Restored topic mapping: '{}' -> '{}'",
                        telemetry_topic,
                        device.device_id
                    );
                }
            }

            drop(mapping);
            tracing::info!(
                "Restored {} topic-to-device mappings from device registry",
                restored_count
            );
        }

        // Restore HASS state topic mappings and re-subscribe (must be after client is set)
        let _ = self.restore_hass_state_topic_mappings().await;

        // Spawn message processing task
        self.start_message_processor(eventloop, running).await;

        // Spawn device timeout monitor task
        self.start_timeout_monitor().await;

        Ok(())
    }

    /// Start the background message processor
    async fn start_message_processor(
        &self,
        eventloop: rumqttc::EventLoop,
        running: Arc<RwLock<bool>>,
    ) {
        let devices = self.devices.clone();
        let topic_to_device = self.topic_to_device.clone();
        let device_registry = self.device_registry.clone();
        let mdl_registry = self.mdl_registry.clone();
        let metric_cache = self.metric_cache.clone();
        let discovery_prefix = self.config.discovery_prefix.clone();
        let connection_status = self.connection_status.clone();
        let hass_discovered_devices = self.hass_discovered_devices.clone();
        let hass_state_topic_map = self.hass_state_topic_map.clone();
        let time_series_storage = self.time_series_storage.clone();
        let storage = self.mdl_registry.storage.clone();
        let event_bus = self.event_bus.clone();
        let broker_id = self.broker_id.clone();

        tokio::spawn(async move {
            let mut eventloop = eventloop;
            let mut error_count = 0;
            let max_errors = 3;

            while *running.read().await {
                match eventloop.poll().await {
                    Ok(notification) => {
                        error_count = 0; // Reset error count on success
                        match notification {
                            rumqttc::Event::Incoming(packet) => {
                                Self::handle_packet(
                                    packet,
                                    &devices,
                                    &topic_to_device,
                                    &device_registry,
                                    &mdl_registry,
                                    &metric_cache,
                                    &discovery_prefix,
                                    &hass_discovered_devices,
                                    &hass_state_topic_map,
                                    &time_series_storage,
                                    &storage,
                                    &event_bus,
                                    &broker_id,
                                )
                                .await;
                            }
                            rumqttc::Event::Outgoing(_) => {}
                        }
                    }
                    Err(e) => {
                        error_count += 1;
                        if error_count <= max_errors {
                            tracing::error!("MQTT error: {}", e);
                            *connection_status.write().await = ConnectionStatus::Error;
                        }

                        // Stop polling after consecutive errors to avoid spam
                        if error_count >= max_errors {
                            tracing::warn!(
                                "MQTT connection failed, stopping polling. Device management will be unavailable."
                            );
                            *running.write().await = false;
                            break;
                        }
                    }
                }
            }
        });
    }

    /// Start the device timeout monitor
    /// Periodically checks device last_seen timestamps and sets devices to Offline if they haven't been seen recently
    async fn start_timeout_monitor(&self) {
        let devices = self.devices.clone();
        let timeout_duration = chrono::Duration::seconds(300); // 5 minutes timeout
        let check_interval = Duration::from_secs(30); // Check every 30 seconds

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(check_interval);
            loop {
                interval.tick().await;

                let now = chrono::Utc::now();
                let mut devices_guard = devices.write().await;

                let mut offline_count = 0;
                for (device_id, device) in devices_guard.iter_mut() {
                    // Only check devices that are currently Online
                    if device.status == super::mdl_format::ConnectionStatus::Online {
                        let time_since_last_seen = now.signed_duration_since(device.last_seen);

                        // Convert chrono Duration to seconds for comparison
                        if time_since_last_seen.num_seconds() > timeout_duration.num_seconds() {
                            tracing::warn!(
                                "Device {} timed out (last seen: {} seconds ago), setting to Offline",
                                device_id,
                                time_since_last_seen.num_seconds()
                            );
                            device.status = super::mdl_format::ConnectionStatus::Offline;
                            offline_count += 1;
                        }
                    }
                }

                if offline_count > 0 {
                    tracing::info!(
                        "Device timeout check: {} devices set to Offline",
                        offline_count
                    );
                }
            }
        });

        *self.timeout_monitor_handle.write().await = Some(handle);
        tracing::info!(
            "Device timeout monitor started (timeout: {}s, check interval: {}s)",
            timeout_duration.num_seconds(),
            check_interval.as_secs()
        );
    }

    /// Handle incoming MQTT packet
    async fn handle_packet(
        packet: rumqttc::Packet,
        devices: &Arc<RwLock<HashMap<String, DeviceInstance>>>,
        topic_to_device: &Arc<RwLock<HashMap<String, String>>>,
        device_registry: &Option<Arc<DeviceRegistry>>,
        mdl_registry: &MdlRegistry,
        metric_cache: &Arc<
            RwLock<HashMap<String, HashMap<String, (MetricValue, chrono::DateTime<chrono::Utc>)>>>,
        >,
        discovery_prefix: &str,
        hass_discovered_devices: &Arc<RwLock<HashMap<String, DiscoveredHassDevice>>>,
        hass_state_topic_map: &Arc<RwLock<HashMap<String, (String, String)>>>,
        time_series_storage: &Arc<RwLock<Option<Arc<TimeSeriesStorage>>>>,
        storage: &Arc<RwLock<Option<Arc<MdlStorage>>>>,
        event_bus: &Option<Arc<EventBus>>,
        broker_id: &str,
    ) {
        match packet {
            rumqttc::Packet::Publish(publish) => {
                let topic = publish.topic.to_string();
                let payload = publish.payload;

                // Special case: HASS discovery messages (deprecated)
                if Self::is_hass_discovery_topic(&topic) {
                    tracing::info!("Received HASS discovery message on topic: {}", topic);
                    Self::handle_hass_discovery_message(&topic, &payload, hass_discovered_devices)
                        .await;
                    return;
                }

                // Special case: Discovery announcements
                if topic.starts_with(discovery_prefix) && topic.ends_with("/announce") {
                    Self::handle_discovery_announcement(&payload, devices, mdl_registry, event_bus)
                        .await;
                    return;
                }

                // Special case: HASS state updates for registered devices
                if let Some((device_id, metric_name)) =
                    hass_state_topic_map.read().await.get(&topic).cloned()
                {
                    Self::handle_hass_state_update(
                        &device_id,
                        &metric_name,
                        &topic,
                        &payload,
                        devices,
                        mdl_registry,
                        metric_cache,
                        time_series_storage,
                        event_bus,
                    )
                    .await;
                    return;
                }

                // Try to match to a registered device
                // This handles our standard topic format: device/{device_type}/{device_id}/uplink
                // And custom telemetry topics registered via auto-onboarding
                let device_matched = Self::try_handle_registered_device(
                    &topic,
                    &payload,
                    devices,
                    topic_to_device,
                    device_registry,
                    mdl_registry,
                    metric_cache,
                    time_series_storage,
                    storage,
                    event_bus,
                ).await;

                // If device was matched, we're done
                if device_matched {
                    return;
                }

                // Device not matched - trigger auto-onboarding for unknown devices
                // This captures any MQTT message from unregistered devices
                Self::handle_unknown_device_mqtt(
                    &topic,
                    &payload,
                    event_bus,
                    broker_id,
                ).await;
            }
            rumqttc::Packet::ConnAck(_) => {
                tracing::info!("MQTT connected successfully");
            }
            _ => {}
        }
    }

    /// Try to handle message for a registered device
    ///
    /// Returns true if the device was matched and handled, false otherwise
    ///
    /// This handles three cases:
    /// 1. Custom telemetry topic mappings in topic_to_device (manually registered)
    /// 2. Custom telemetry topics in device_registry (from auto-onboarding)
    /// 3. Standard topic format: device/{device_type}/{device_id}/uplink
    async fn try_handle_registered_device(
        topic: &str,
        payload: &[u8],
        devices: &Arc<RwLock<HashMap<String, DeviceInstance>>>,
        topic_to_device: &Arc<RwLock<HashMap<String, String>>>,
        device_registry: &Option<Arc<DeviceRegistry>>,
        mdl_registry: &MdlRegistry,
        metric_cache: &Arc<
            RwLock<HashMap<String, HashMap<String, (MetricValue, chrono::DateTime<chrono::Utc>)>>>,
        >,
        time_series_storage: &Arc<RwLock<Option<Arc<TimeSeriesStorage>>>>,
        storage: &Arc<RwLock<Option<Arc<MdlStorage>>>>,
        event_bus: &Option<Arc<EventBus>>,
    ) -> bool {
        // Case 1: Check manually registered topic mappings
        if let Some(device_id) = topic_to_device.read().await.get(topic).cloned() {
            // Get device type from device instances
            let device_type_name = {
                let devices_guard = devices.read().await;
                devices_guard.get(&device_id).map(|d| d.device_type.clone())
            };

            if let Some(device_type_name) = device_type_name {
                if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(payload) {
                    // Process device data inline
                    let now = chrono::Utc::now();

                    // Update metric cache
                    if let Some(obj) = json_value.as_object() {
                        let mut cache = metric_cache.write().await;
                        for (key, value) in obj {
                            let metric_value = match value {
                                serde_json::Value::Number(n) => {
                                    if let Some(i) = n.as_i64() {
                                        MetricValue::Integer(i)
                                    } else if let Some(f) = n.as_f64() {
                                        MetricValue::Float(f)
                                    } else {
                                        MetricValue::Null
                                    }
                                }
                                serde_json::Value::String(s) => MetricValue::String(s.clone()),
                                serde_json::Value::Bool(b) => MetricValue::Boolean(*b),
                                _ => MetricValue::Null,
                            };
                            cache.entry(device_id.clone())
                                .or_insert_with(HashMap::new)
                                .insert(key.clone(), (metric_value, now));
                        }
                    }

                    // Publish event
                    if let Some(bus) = event_bus {
                        use edge_ai_core::MetricValue as CoreMetricValue;

                        if let Some(obj) = json_value.as_object() {
                            for (key, value) in obj {
                                let core_value = match value {
                                    serde_json::Value::Number(n) => {
                                        if let Some(i) = n.as_i64() {
                                            CoreMetricValue::Integer(i)
                                        } else if let Some(f) = n.as_f64() {
                                            CoreMetricValue::Float(f)
                                        } else {
                                            CoreMetricValue::String("null".to_string())
                                        }
                                    }
                                    serde_json::Value::String(s) => CoreMetricValue::String(s.clone()),
                                    serde_json::Value::Bool(b) => CoreMetricValue::Boolean(*b),
                                    _ => CoreMetricValue::String("null".to_string()),
                                };
                                let _ = bus.publish(edge_ai_core::NeoTalkEvent::DeviceMetric {
                                    device_id: device_id.clone(),
                                    metric: key.clone(),
                                    value: core_value,
                                    timestamp: now.timestamp(),
                                    quality: None,
                                }).await;
                            }
                        }

                        let _ = bus.publish(edge_ai_core::NeoTalkEvent::DeviceOnline {
                            device_id: device_id.clone(),
                            device_type: device_type_name.clone(),
                            timestamp: now.timestamp(),
                        }).await;
                    }

                    return true;
                }
            }
        }

        // Case 2: Check device registry for custom telemetry topics (auto-onboarding)
        if let Some(registry) = device_registry {
            if let Some((device_id, config)) = registry.find_device_by_telemetry_topic(topic).await {
                if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(payload) {
                    // Process device data inline
                    let now = chrono::Utc::now();

                    // Update metric cache
                    if let Some(obj) = json_value.as_object() {
                        let mut cache = metric_cache.write().await;
                        for (key, value) in obj {
                            let metric_value = match value {
                                serde_json::Value::Number(n) => {
                                    if let Some(i) = n.as_i64() {
                                        MetricValue::Integer(i)
                                    } else if let Some(f) = n.as_f64() {
                                        MetricValue::Float(f)
                                    } else {
                                        MetricValue::Null
                                    }
                                }
                                serde_json::Value::String(s) => MetricValue::String(s.clone()),
                                serde_json::Value::Bool(b) => MetricValue::Boolean(*b),
                                _ => MetricValue::Null,
                            };
                            cache.entry(device_id.clone())
                                .or_insert_with(HashMap::new)
                                .insert(key.clone(), (metric_value, now));
                        }
                    }

                    // Publish event
                    if let Some(bus) = event_bus {
                        use edge_ai_core::MetricValue as CoreMetricValue;

                        if let Some(obj) = json_value.as_object() {
                            for (key, value) in obj {
                                let core_value = match value {
                                    serde_json::Value::Number(n) => {
                                        if let Some(i) = n.as_i64() {
                                            CoreMetricValue::Integer(i)
                                        } else if let Some(f) = n.as_f64() {
                                            CoreMetricValue::Float(f)
                                        } else {
                                            CoreMetricValue::String("null".to_string())
                                        }
                                    }
                                    serde_json::Value::String(s) => CoreMetricValue::String(s.clone()),
                                    serde_json::Value::Bool(b) => CoreMetricValue::Boolean(*b),
                                    _ => CoreMetricValue::String("null".to_string()),
                                };
                                let _ = bus.publish(edge_ai_core::NeoTalkEvent::DeviceMetric {
                                    device_id: device_id.clone(),
                                    metric: key.clone(),
                                    value: core_value,
                                    timestamp: now.timestamp(),
                                    quality: None,
                                }).await;
                            }
                        }

                        let _ = bus.publish(edge_ai_core::NeoTalkEvent::DeviceOnline {
                            device_id: device_id.clone(),
                            device_type: config.device_type.clone(),
                            timestamp: now.timestamp(),
                        }).await;
                    }

                    return true;
                }
            }
        }

        // Case 3: Fall back to standard topic format: device/{device_type}/{device_id}/uplink
        // Our standard topic format: device/{device_type}/{device_id}/uplink
        let parts: Vec<&str> = topic.split('/').collect();
        if parts.len() < 4 || parts[0] != "device" {
            return false;
        }

        let device_type_name = parts[1].to_string();
        let device_id = parts[2].to_string();
        let direction = parts.get(3).copied();

        // Check if this device type exists
        let device_type = match mdl_registry.get(&device_type_name).await {
            Some(dt) => dt,
            None => {
                // Unknown device type, not a registered device
                return false;
            }
        };

        // Process uplink data
        if direction == Some("uplink") {
            // Parse and store the metrics
            if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(payload) {
                let mut cache = metric_cache.write().await;
                let now = chrono::Utc::now();

                let is_new_device = {
                    let mut devices_mut = devices.write().await;
                    !devices_mut.contains_key(&device_id)
                };

                if is_new_device {
                    tracing::info!(
                        "Auto-registered device: {} (type: {})",
                        device_id,
                        device_type_name
                    );
                }

                // Update metric cache (simplified)
                if let Some(obj) = json_value.as_object() {
                    for (key, value) in obj {
                        let metric_value = match value {
                            serde_json::Value::Number(n) => {
                                if let Some(i) = n.as_i64() {
                                    MetricValue::Integer(i)
                                } else if let Some(f) = n.as_f64() {
                                    MetricValue::Float(f)
                                } else {
                                    MetricValue::Null
                                }
                            }
                            serde_json::Value::String(s) => MetricValue::String(s.clone()),
                            serde_json::Value::Bool(b) => MetricValue::Boolean(*b),
                            _ => MetricValue::Null,
                        };
                        cache.entry(device_id.clone())
                            .or_insert_with(HashMap::new)
                            .insert(key.clone(), (metric_value, now));
                    }
                }

                // Publish event
                if let Some(bus) = event_bus {
                    let _ = bus.publish(edge_ai_core::NeoTalkEvent::DeviceOnline {
                        device_id: device_id.clone(),
                        device_type: device_type_name.clone(),
                        timestamp: now.timestamp(),
                    }).await;
                }

                return true;
            }
        }

        false
    }

    /// Handle unknown device MQTT message for auto-onboarding
    ///
    /// This is called when a message doesn't match any registered device
    async fn handle_unknown_device_mqtt(
        topic: &str,
        payload: &[u8],
        event_bus: &Option<Arc<EventBus>>,
        broker_id: &str,
    ) {
        // Skip non-JSON payloads
        let json_data: Result<serde_json::Value, _> = serde_json::from_slice(payload);
        let data = match json_data {
            Ok(v) => v,
            Err(_) => {
                tracing::debug!(
                    "Skipping non-JSON payload for auto-onboarding: topic={}, size={}",
                    topic,
                    payload.len()
                );
                return;
            }
        };

        // Extract device_id from topic or payload
        let device_id = Self::extract_device_id_from_mqtt(topic, &data);

        let device_id = match device_id {
            Some(id) => id,
            None => {
                // Use topic as device_id (hashed to avoid conflicts)
                format!("mqtt_{}", Self::hash_topic(topic))
            }
        };

        tracing::info!(
            "Unknown device data: topic='{}', device_id='{}', triggering auto-onboarding",
            topic,
            device_id
        );

        // Publish event for auto-onboarding
        if let Some(bus) = event_bus {
            let sample = serde_json::json!({
                "device_id": device_id,
                "timestamp": chrono::Utc::now().timestamp(),
                "topic": topic,
                "data": data
            });

            // Include broker_id and adapter_id so auto-onboarding knows which adapter to use
            let adapter_id = format!("external-{}", broker_id);

            let event = edge_ai_core::NeoTalkEvent::Custom {
                event_type: "unknown_device_data".to_string(),
                data: serde_json::json!({
                    "device_id": device_id,
                    "source": topic,  // Use actual topic as source (e.g., "device999")
                    "original_topic": topic,
                    "broker_id": broker_id,
                    "adapter_id": adapter_id,
                    "sample": sample
                }),
            };

            bus.publish(event).await;
        }
    }

    /// Extract device_id from MQTT topic or payload
    fn extract_device_id_from_mqtt(topic: &str, payload: &serde_json::Value) -> Option<String> {
        // Try to extract from topic (last non-empty segment)
        let parts: Vec<&str> = topic.split('/').collect();
        for part in parts.iter().rev() {
            if !part.is_empty() && *part != "#" && *part != "uplink" && *part != "downlink" {
                // Skip common topic words
                if !["tele", "stat", "cmnd", "result", "sensor", "set"].contains(&part.to_lowercase().as_str()) {
                    return Some(part.to_string());
                }
            }
        }

        // Try to find device_id in payload
        if let Some(dev_id) = payload.get("device_id").and_then(|v| v.as_str()) {
            return Some(dev_id.to_string());
        }
        if let Some(dev_id) = payload.get("id").and_then(|v| v.as_str()) {
            return Some(dev_id.to_string());
        }
        if let Some(dev_id) = payload.get("device").and_then(|v| v.as_str()) {
            return Some(dev_id.to_string());
        }
        if let Some(dev_id) = payload.get("Topic").and_then(|v| v.as_str()) {
            // Tasmota devices
            return Some(dev_id.to_string());
        }

        // Try to use the second-to-last part of topic
        if parts.len() >= 2 {
            let candidate = parts[parts.len() - 2];
            if !candidate.is_empty() {
                return Some(candidate.to_string());
            }
        }

        None
    }

    /// Handle HASS discovery message
    async fn handle_hass_discovery_message(
        topic: &str,
        payload: &[u8],
        hass_discovered_devices: &Arc<RwLock<HashMap<String, DiscoveredHassDevice>>>,
    ) {
        tracing::info!(
            "HASS discovery is deprecated, ignoring: topic={}, payload_len={}",
            topic,
            payload.len()
        );
        // HASS discovery functionality removed - stub implementation
    }

    /// Check if topic is a HASS discovery topic (stub)
    fn is_hass_discovery_topic(topic: &str) -> bool {
        topic.starts_with("homeassistant/")
    }

    /// Simple hash function for fallback device_id generation
    fn hash_topic(s: &str) -> u64 {
        let mut hash: u64 = 5381;
        for c in s.chars() {
            hash = hash.wrapping_mul(33).wrapping_add(c as u64);
        }
        hash
    }

    /// Handle HASS device state update (stub - HASS functionality deprecated)
    async fn handle_hass_state_update(
        device_id: &str,
        metric_name: &str,
        topic: &str,
        payload: &[u8],
        devices: &Arc<RwLock<HashMap<String, DeviceInstance>>>,
        mdl_registry: &MdlRegistry,
        metric_cache: &Arc<
            RwLock<HashMap<String, HashMap<String, (MetricValue, chrono::DateTime<chrono::Utc>)>>>,
        >,
        time_series_storage: &Arc<RwLock<Option<Arc<TimeSeriesStorage>>>>,
        event_bus: &Option<Arc<EventBus>>,
    ) {
        let now = chrono::Utc::now();

        // Parse the payload as a string value
        let payload_str = match std::str::from_utf8(payload) {
            Ok(s) => s.trim(),
            Err(_) => {
                tracing::warn!(
                    "Invalid UTF-8 payload from HASS device {} on topic {}",
                    device_id,
                    topic
                );
                return;
            }
        };

        tracing::info!(
            "HASS state update: device_id={}, metric_name={}, topic='{}', payload='{}'",
            device_id,
            metric_name,
            topic,
            payload_str
        );

        // Get device, auto-register if it doesn't exist
        let device = {
            let devices_guard = devices.read().await;
            devices_guard.get(device_id).cloned()
        };

        let (device_type_name, device_type) = match device {
            Some(d) => {
                // Device exists, get its type
                match mdl_registry.get(&d.device_type).await {
                    Some(dt) => (d.device_type.clone(), dt),
                    None => {
                        tracing::warn!(
                            "Device type '{}' not found for HASS device {}",
                            d.device_type,
                            device_id
                        );
                        return;
                    }
                }
            }
            None => {
                // Device doesn't exist - need to find its type from metric name
                // Search all device types to find one with this metric
                let mut found_type = None;
                let mut found_type_name = None;
                let all_types = mdl_registry.list().await;
                for dt in all_types {
                    if dt.uplink.metrics.iter().any(|m| m.name == metric_name) {
                        found_type_name = Some(dt.device_type.clone());
                        found_type = Some(dt);
                        break;
                    }
                }

                let (dt_name, dt) = match (found_type_name, found_type) {
                    (Some(name), Some(dt)) => (name, dt),
                    _ => {
                        tracing::warn!(
                            "Cannot auto-register HASS device {}: metric '{}' not found in any device type",
                            device_id,
                            metric_name
                        );
                        return;
                    }
                };

                // Auto-register the device
                tracing::info!(
                    "Auto-registering HASS device {} with type {}",
                    device_id,
                    dt_name
                );
                let new_device = super::mdl_format::DeviceInstance {
                    device_type: dt_name.clone(),
                    device_id: device_id.to_string(),
                    name: None,
                    status: super::mdl_format::ConnectionStatus::Online,
                    last_seen: now,
                    config: std::collections::HashMap::new(),
                    current_values: std::collections::HashMap::new(),
                    adapter_id: Some("hass".to_string()),
                };

                {
                    let mut devices_mut = devices.write().await;
                    devices_mut.insert(device_id.to_string(), new_device.clone());
                }

                // Publish DeviceOnline event for auto-registered HASS device
                if let Some(bus) = event_bus {
                    use edge_ai_core::NeoTalkEvent;
                    bus.publish(NeoTalkEvent::DeviceOnline {
                        device_id: device_id.to_string(),
                        device_type: dt_name.clone(),
                        timestamp: now.timestamp(),
                    })
                    .await;
                    tracing::debug!("Published DeviceOnline event for HASS device {}", device_id);
                }

                (dt_name, dt)
            }
        };

        tracing::info!(
            "  Device type '{}' has {} metrics: {:?}",
            device_type_name,
            device_type.uplink.metrics.len(),
            device_type
                .uplink
                .metrics
                .iter()
                .map(|m| &m.name)
                .collect::<Vec<_>>()
        );

        // Find the metric by name (for aggregated devices with multiple metrics)
        let metric_def = match device_type
            .uplink
            .metrics
            .iter()
            .find(|m| m.name == metric_name)
        {
            Some(m) => m,
            None => {
                tracing::warn!(
                    "Metric '{}' not found in device type {}",
                    metric_name,
                    device_type.device_type
                );
                return;
            }
        };

        // Parse the value based on metric data type
        let value = match metric_def.data_type {
            super::mdl::MetricDataType::Float => payload_str
                .parse::<f64>()
                .map(super::mdl::MetricValue::Float)
                .unwrap_or_else(|_| super::mdl::MetricValue::String(payload_str.to_string())),
            super::mdl::MetricDataType::Integer => payload_str
                .parse::<i64>()
                .map(super::mdl::MetricValue::Integer)
                .unwrap_or_else(|_| super::mdl::MetricValue::String(payload_str.to_string())),
            super::mdl::MetricDataType::Boolean => {
                let bool_val = payload_str.eq_ignore_ascii_case("true")
                    || payload_str.eq_ignore_ascii_case("on")
                    || payload_str.eq_ignore_ascii_case("1")
                    || payload_str.eq_ignore_ascii_case("yes");
                super::mdl::MetricValue::Boolean(bool_val)
            }
            _ => super::mdl::MetricValue::String(payload_str.to_string()),
        };

        // Update metric cache
        {
            let mut cache = metric_cache.write().await;
            cache
                .entry(device_id.to_string())
                .or_default()
                .insert(metric_name.to_string(), (value.clone(), now));
        }

        // Update device status and current_values
        {
            let mut devices_guard = devices.write().await;
            if let Some(device) = devices_guard.get_mut(device_id) {
                device.last_seen = now;
                device.status = super::mdl_format::ConnectionStatus::Online;
                device
                    .current_values
                    .insert(metric_name.to_string(), (value.clone(), now));
            }
        }

        // Persist to time series storage (all value types)
        let ts_storage = time_series_storage.read().await;
        if let Some(storage) = ts_storage.as_ref() {
            let data_point = DataPoint {
                timestamp: now.timestamp(),
                value: value.clone(),
                quality: None,
            };
            if let Err(e) = storage.write(device_id, &metric_def.name, data_point).await {
                tracing::error!("Failed to write HASS device telemetry: {}", e);
            } else {
                tracing::info!(
                    "Stored HASS device {} metric {} = {:?} (topic: {})",
                    device_id,
                    metric_def.name,
                    value,
                    topic
                );
            }
        }

        // Publish DeviceMetric event to EventBus
        if let Some(bus) = event_bus {
            use edge_ai_core::{MetricValue as CoreMetricValue, NeoTalkEvent};
            use serde_json::json;
            // Convert our MetricValue to core's MetricValue
            let core_value = match &value {
                super::mdl::MetricValue::Integer(i) => CoreMetricValue::Integer(*i),
                super::mdl::MetricValue::Float(f) => CoreMetricValue::Float(*f),
                super::mdl::MetricValue::String(s) => CoreMetricValue::String(s.clone()),
                super::mdl::MetricValue::Boolean(b) => CoreMetricValue::Boolean(*b),
                super::mdl::MetricValue::Array(arr) => {
                    // Convert array to JSON
                    let json_arr: Vec<serde_json::Value> = arr.iter().map(|v| match v {
                        super::mdl::MetricValue::Integer(i) => json!(*i),
                        super::mdl::MetricValue::Float(f) => json!(*f),
                        super::mdl::MetricValue::String(s) => json!(s),
                        super::mdl::MetricValue::Boolean(b) => json!(*b),
                        _ => json!(null),
                    }).collect();
                    CoreMetricValue::Json(json!(json_arr))
                }
                super::mdl::MetricValue::Binary(_) => CoreMetricValue::Json(json!(null)),
                super::mdl::MetricValue::Null => CoreMetricValue::Json(json!(null)),
            };
            bus.publish(NeoTalkEvent::DeviceMetric {
                device_id: device_id.to_string(),
                metric: metric_name.to_string(),
                value: core_value,
                timestamp: now.timestamp(),
                quality: None,
            })
            .await;
        }

        tracing::info!(
            "HASS device {} state updated: {} = {:?}",
            device_id,
            metric_def.name,
            value
        );
    }

    /// Handle discovery announcement
    async fn handle_discovery_announcement(
        payload: &[u8],
        devices: &Arc<RwLock<HashMap<String, DeviceInstance>>>,
        mdl_registry: &MdlRegistry,
        event_bus: &Option<Arc<EventBus>>,
    ) {
        let announcement: DiscoveryAnnouncement = match serde_json::from_slice(payload) {
            Ok(a) => a,
            Err(e) => {
                tracing::error!("Failed to parse discovery announcement: {}", e);
                return;
            }
        };

        tracing::info!(
            "Discovered device: {} (type: {})",
            announcement.device_id,
            announcement.device_type
        );

        // Check if device type exists
        let device_type = match mdl_registry.get(&announcement.device_type).await {
            Some(dt) => dt,
            None => {
                tracing::warn!("Unknown device type: {}", announcement.device_type);
                return;
            }
        };

        let now = chrono::Utc::now();

        // Create or update device instance
        let instance = DeviceInstance {
            device_type: announcement.device_type.clone(),
            device_id: announcement.device_id.clone(),
            name: announcement.name.clone(),
            status: super::mdl_format::ConnectionStatus::Online,
            last_seen: now,
            config: announcement.config.clone(),
            current_values: HashMap::new(),
            adapter_id: None,
        };

        let mut devices_guard = devices.write().await;
        devices_guard.insert(announcement.device_id.clone(), instance);
        drop(devices_guard);

        tracing::info!(
            "Device {} registered with {} metrics",
            announcement.device_id,
            device_type.uplink.metrics.len()
        );

        // Publish DeviceOnline event to EventBus
        if let Some(bus) = event_bus {
            use edge_ai_core::NeoTalkEvent;
            bus.publish(NeoTalkEvent::DeviceOnline {
                device_id: announcement.device_id.clone(),
                device_type: announcement.device_type.clone(),
                timestamp: now.timestamp(),
            })
            .await;
            tracing::debug!(
                "Published DeviceOnline event for {}",
                announcement.device_id
            );
        }
    }

    /// Handle metric message from device
    /// Topic format: device/{device_type}/{device_id}/uplink or /downlink
    async fn handle_metric_message(
        topic: &str,
        payload: &[u8],
        devices: &Arc<RwLock<HashMap<String, DeviceInstance>>>,
        mdl_registry: &MdlRegistry,
        metric_cache: &Arc<
            RwLock<HashMap<String, HashMap<String, (MetricValue, chrono::DateTime<chrono::Utc>)>>>,
        >,
        time_series_storage: &Arc<RwLock<Option<Arc<TimeSeriesStorage>>>>,
        storage: &Arc<RwLock<Option<Arc<MdlStorage>>>>,
        event_bus: &Option<Arc<EventBus>>,
    ) {
        // Parse topic: device/{device_type}/{device_id}/uplink or /downlink
        // Extract device_type and device_id directly from topic - no registration needed!
        let parts: Vec<&str> = topic.split('/').collect();
        if parts.len() < 4 || parts[0] != "device" {
            tracing::warn!("Invalid topic format: {}", topic);
            return;
        }

        let device_type_name = parts[1].to_string();
        let device_id = parts[2].to_string();
        let direction = parts.get(3).copied();

        // Get MDL definition directly from device_type in topic
        let device_type = match mdl_registry.get(&device_type_name).await {
            Some(dt) => dt,
            None => {
                // Unknown device type - trigger auto-onboarding
                tracing::info!(
                    "Unknown device type '{}' from topic: {}, triggering auto-onboarding for device '{}'",
                    device_type_name,
                    topic,
                    device_id
                );

                // Try to parse payload as JSON for auto-onboarding
                if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(payload) {
                    // Publish event for auto-onboarding to pick up
                    if let Some(bus) = event_bus {
                        let sample = serde_json::json!({
                            "device_id": device_id,
                            "timestamp": chrono::Utc::now().timestamp(),
                            "data": json_value
                        });

                        let event = edge_ai_core::NeoTalkEvent::Custom {
                            event_type: "unknown_device_data".to_string(),
                            data: serde_json::json!({
                                "device_id": device_id,
                                "source": topic,  // Use actual topic as source
                                "original_topic": topic,
                                "inferred_type": device_type_name,
                                "sample": sample
                            }),
                        };

                        bus.publish(event).await;
                        tracing::info!(
                            "Published auto-onboarding event for unknown device: {} (inferred type: {})",
                            device_id,
                            device_type_name
                        );
                    }
                } else {
                    tracing::warn!(
                        "Failed to parse JSON payload for unknown device {}: {} bytes",
                        device_id,
                        payload.len()
                    );
                }
                return;
            }
        };

        // Handle uplink data
        if direction == Some("uplink") {
            // Log payload size for debugging
            tracing::info!(
                "Received uplink from device {} (type: {}), payload size: {} bytes",
                device_id,
                device_type_name,
                payload.len()
            );

            // Warn about large payloads
            if payload.len() > 100_000 {
                tracing::warn!(
                    "Large payload detected: {} bytes from device {}",
                    payload.len(),
                    device_id
                );
            }

            // Try to parse payload as JSON with multiple metrics
            let json_result: Result<serde_json::Value, _> = serde_json::from_slice(payload);
            if let Ok(json_value) = json_result {
                tracing::info!(
                    "Parsed JSON payload from device {}: {}",
                    device_id,
                    serde_json::to_string_pretty(&json_value)
                        .unwrap_or_else(|_| "<complex>".to_string())
                );

                let mut cache = metric_cache.write().await;

                // Update or create device instance
                let now = chrono::Utc::now();
                let is_new_device;
                {
                    let mut devices_mut = devices.write().await;
                    is_new_device = !devices_mut.contains_key(&device_id);
                    let device = devices_mut.entry(device_id.clone()).or_insert_with(|| {
                        tracing::info!(
                            "Auto-registered device: {} (type: {})",
                            device_id,
                            device_type_name
                        );
                        DeviceInstance {
                            device_type: device_type_name.clone(),
                            device_id: device_id.clone(),
                            name: None,
                            status: super::mdl_format::ConnectionStatus::Online,
                            last_seen: now,
                            config: std::collections::HashMap::new(),
                            current_values: std::collections::HashMap::new(),
                            adapter_id: None,
                        }
                    });
                    device.last_seen = now;
                    device.status = super::mdl_format::ConnectionStatus::Online;
                }

                // Persist newly registered device to storage
                if is_new_device {
                    if let Some(store) = storage.read().await.as_ref()
                        && let Some(device) = devices.read().await.get(&device_id).cloned() {
                            if let Err(e) = store.save_device_instance(&device).await {
                                tracing::warn!(
                                    "Failed to persist auto-registered device {}: {}",
                                    device_id,
                                    e
                                );
                            } else {
                                tracing::info!(
                                    "Persisted auto-registered device {} to storage",
                                    device_id
                                );
                            }
                        }

                    // Publish DeviceOnline event for new device
                    if let Some(bus) = event_bus {
                        use edge_ai_core::NeoTalkEvent;
                        bus.publish(NeoTalkEvent::DeviceOnline {
                            device_id: device_id.clone(),
                            device_type: device_type_name.clone(),
                            timestamp: now.timestamp(),
                        })
                        .await;
                        tracing::debug!(
                            "Published DeviceOnline event for auto-registered device {}",
                            device_id
                        );
                    }
                }

                // Get time series storage for persistence
                let ts_storage = time_series_storage.read().await;

                // Log device type info
                tracing::info!(
                    "Device type {} has {} metrics defined",
                    device_type_name,
                    device_type.uplink.metrics.len()
                );

                // Match each metric definition against the JSON payload
                let mut metrics_found = 0;
                for metric_def in &device_type.uplink.metrics {
                    match mdl_registry.parse_metric_value(metric_def, payload) {
                        Ok(value) => {
                            metrics_found += 1;
                            // Update metric cache
                            cache
                                .entry(device_id.clone())
                                .or_default()
                                .insert(metric_def.name.clone(), (value.clone(), now));

                            // Update device.current_values
                            {
                                let mut devices_mut = devices.write().await;
                                if let Some(device) = devices_mut.get_mut(&device_id) {
                                    device
                                        .current_values
                                        .insert(metric_def.name.clone(), (value.clone(), now));
                                }
                            }

                            // Persist to time series storage (all value types)
                            if let Some(storage) = ts_storage.as_ref() {
                                let data_point = DataPoint {
                                    timestamp: now.timestamp(),
                                    value: value.clone(),
                                    quality: None,
                                };
                                if let Err(e) = storage
                                    .write(&device_id, &metric_def.name, data_point)
                                    .await
                                {
                                    tracing::error!("Failed to write telemetry data: {}", e);
                                } else {
                                    tracing::info!(
                                        "Stored metric {} = {:?} for device {}",
                                        metric_def.name,
                                        value,
                                        device_id
                                    );
                                }
                            }

                            // Publish DeviceMetric event to EventBus
                            if let Some(bus) = event_bus {
                                use edge_ai_core::{MetricValue as CoreMetricValue, NeoTalkEvent};
                                use serde_json::json;
                                // Convert our MetricValue to core's MetricValue
                                let core_value = match &value {
                                    super::mdl::MetricValue::Integer(i) => {
                                        CoreMetricValue::Integer(*i)
                                    }
                                    super::mdl::MetricValue::Float(f) => CoreMetricValue::Float(*f),
                                    super::mdl::MetricValue::String(s) => {
                                        CoreMetricValue::String(s.clone())
                                    }
                                    super::mdl::MetricValue::Boolean(b) => {
                                        CoreMetricValue::Boolean(*b)
                                    }
                                    super::mdl::MetricValue::Array(arr) => {
                                        // Convert array to JSON
                                        let json_arr: Vec<serde_json::Value> = arr.iter().map(|v| match v {
                                            super::mdl::MetricValue::Integer(i) => json!(*i),
                                            super::mdl::MetricValue::Float(f) => json!(*f),
                                            super::mdl::MetricValue::String(s) => json!(s),
                                            super::mdl::MetricValue::Boolean(b) => json!(*b),
                                            _ => json!(null),
                                        }).collect();
                                        CoreMetricValue::Json(json!(json_arr))
                                    }
                                    super::mdl::MetricValue::Binary(_) => {
                                        CoreMetricValue::Json(json!(null))
                                    }
                                    super::mdl::MetricValue::Null => {
                                        CoreMetricValue::Json(json!(null))
                                    }
                                };
                                bus.publish(NeoTalkEvent::DeviceMetric {
                                    device_id: device_id.clone(),
                                    metric: metric_def.name.clone(),
                                    value: core_value,
                                    timestamp: now.timestamp(),
                                    quality: None,
                                })
                                .await;
                            }

                            tracing::info!(
                                "Received metric {} for device {}: {:?}",
                                metric_def.name,
                                device_id,
                                value
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to parse metric '{}' from device {}: {}",
                                metric_def.name,
                                device_id,
                                e
                            );
                        }
                    }
                }

                if metrics_found == 0 {
                    tracing::warn!(
                        "No metrics matched from device type {} for device {}. Payload: {}",
                        device_type_name,
                        device_id,
                        String::from_utf8_lossy(&payload[..payload.len().min(200)])
                    );
                } else {
                    tracing::info!(
                        "Successfully processed {} metrics for device {}",
                        metrics_found,
                        device_id
                    );
                }
            } else {
                tracing::error!(
                    "Failed to parse JSON payload from device {}: {}, payload: {}",
                    device_id,
                    json_result
                        .err()
                        .map(|e| e.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    String::from_utf8_lossy(&payload[..payload.len().min(200)])
                );
            }
        }
        // Downlink messages are handled separately via command sending
    }

    /// Register a device type definition
    pub async fn register_device_type(&self, def: DeviceTypeDefinition) -> Result<(), DeviceError> {
        self.mdl_registry.register(def).await
    }

    /// Get a device type definition
    pub async fn get_device_type(&self, device_type: &str) -> Option<DeviceTypeDefinition> {
        self.mdl_registry.get(device_type).await
    }

    /// List all device types, filtering out old HASS individual entity types
    pub async fn list_device_types(&self) -> Vec<DeviceTypeDefinition> {
        let all_types = self.mdl_registry.list().await;
        // Filter out old HASS individual entity device types (those with "hass_discovery" category)
        // These are replaced by aggregated HASS devices
        all_types
            .into_iter()
            .filter(|dt| !dt.categories.contains(&"hass_discovery".to_string()))
            .collect()
    }

    /// Add a device manually (without discovery)
    pub async fn add_device(
        &self,
        device_id: String,
        device_type: String,
        name: Option<String>,
        adapter_id: Option<String>,
        config: HashMap<String, String>,
    ) -> Result<(), DeviceError> {
        // Check if device type exists
        let _type_def = self.mdl_registry.get(&device_type).await.ok_or_else(|| {
            DeviceError::InvalidParameter(format!("Unknown device type: {}", device_type))
        })?;

        // Create device instance
        let instance = DeviceInstance {
            device_type,
            device_id: device_id.clone(),
            name,
            status: super::mdl_format::ConnectionStatus::Disconnected,
            last_seen: chrono::Utc::now(),
            config,
            current_values: HashMap::new(),
            adapter_id,
        };

        // Save to storage first
        self.save_device_instance(&instance).await?;

        // Then add to in-memory cache
        let mut devices = self.devices.write().await;
        devices.insert(device_id, instance);

        Ok(())
    }

    /// Remove a device
    pub async fn remove_device(&self, device_id: &str) -> Result<(), DeviceError> {
        // Get device's HASS state topics before removing
        let hass_state_topics: Vec<String> = {
            let devices = self.devices.read().await;
            if let Some(device) = devices.get(device_id) {
                device
                    .config
                    .iter()
                    .filter(|(k, _)| k.starts_with("hass_state:"))
                    .map(|(_, v)| v.clone())
                    .collect()
            } else {
                Vec::new()
            }
        };

        let mut devices = self.devices.write().await;
        devices.remove(device_id).ok_or_else(|| {
            DeviceError::InvalidParameter(format!("Device not found: {}", device_id))
        })?;

        // Remove from metric cache
        let mut cache = self.metric_cache.write().await;
        cache.remove(device_id);

        // Remove HASS state topic mappings
        let mut state_map = self.hass_state_topic_map.write().await;
        for state_topic in hass_state_topics {
            if let Some((mapped_device_id, _)) = state_map.remove(&state_topic) {
                tracing::info!(
                    "Removed HASS state topic mapping: {} -> {}",
                    state_topic,
                    mapped_device_id
                );
            }
        }

        // Remove from storage
        self.delete_device_instance(device_id).await?;

        tracing::info!("Device {} removed successfully", device_id);
        Ok(())
    }

    /// List all devices
    pub async fn list_devices(&self) -> Vec<DeviceInstance> {
        let devices = self.devices.read().await;
        devices.values().cloned().collect()
    }

    /// Get a device
    pub async fn get_device(&self, device_id: &str) -> Option<DeviceInstance> {
        let devices = self.devices.read().await;
        devices.get(device_id).cloned()
    }

    /// Read a metric value from a device
    pub async fn read_metric(
        &self,
        device_id: &str,
        metric_name: &str,
    ) -> Result<MetricValue, DeviceError> {
        let cache = self.metric_cache.read().await;

        cache
            .get(device_id)
            .and_then(|m| m.get(metric_name))
            .map(|(v, _)| v.clone())
            .ok_or_else(|| {
                DeviceError::InvalidMetric(format!(
                    "Metric {} not found for device {}",
                    metric_name, device_id
                ))
            })
    }

    /// Send a command to a device
    pub async fn send_command(
        &self,
        device_id: &str,
        command_name: &str,
        params: HashMap<String, MetricValue>,
    ) -> Result<(), DeviceError> {
        let device = self.get_device(device_id).await.ok_or_else(|| {
            DeviceError::InvalidParameter(format!("Device not found: {}", device_id))
        })?;

        let device_type = self
            .mdl_registry
            .get(&device.device_type)
            .await
            .ok_or_else(|| {
                DeviceError::InvalidParameter(format!(
                    "Device type not found: {}",
                    device.device_type
                ))
            })?;

        let command = device_type
            .downlink
            .commands
            .iter()
            .find(|c| c.name == command_name)
            .ok_or_else(|| {
                DeviceError::InvalidCommand(format!("Command not found: {}", command_name))
            })?;

        let payload = self.mdl_registry.build_command_payload(command, &params)?;

        // Check if this is a HASS device with a custom command topic
        let topic = if let Some(command_topics) = device.config.get("hass_command_topic") {
            // HASS devices may have multiple command topics (comma-separated)
            // Use the first one for now (all entities typically share the same device command topic)
            command_topics
                .split(',')
                .next()
                .unwrap_or(command_topics)
                .to_string()
        } else {
            // Use the standard downlink topic format
            self.mdl_registry
                .downlink_topic(&device.device_type, device_id)
        };

        let client_guard = self.mqtt_client.read().await;
        let client_wrapper = client_guard
            .as_ref()
            .ok_or_else(|| DeviceError::NotConnected(DeviceId::new()))?;

        client_wrapper
            .client
            .publish(&topic, rumqttc::QoS::AtLeastOnce, false, payload)
            .await
            .map_err(|e| DeviceError::Communication(e.to_string()))?;

        Ok(())
    }

    /// Get connection status
    pub async fn connection_status(&self) -> ConnectionStatus {
        *self.connection_status.read().await
    }

    /// Disconnect from MQTT broker
    pub async fn disconnect(&self) -> Result<(), DeviceError> {
        // Stop the event loop
        if let Some(wrapper) = self.mqtt_client.read().await.as_ref() {
            *wrapper.running.write().await = false;
        }

        *self.connection_status.write().await = ConnectionStatus::Disconnected;
        *self.mqtt_client.write().await = None;
        Ok(())
    }

    /// Subscribe to a device's metrics
    /// Note: With simplified topic scheme, all devices are already subscribed via wildcard
    pub async fn subscribe_device(&self, device_id: &str) -> Result<(), DeviceError> {
        // All devices are already subscribed via device/+/uplink and device/+/downlink
        // This is kept for API compatibility but does nothing
        let _ = device_id;
        Ok(())
    }

    /// Register a custom telemetry topic for a device
    /// This is used for devices registered via auto-onboarding that use custom topics
    /// (e.g., "ashuau") instead of the standard "device/{type}/{id}/uplink" format
    pub async fn register_custom_topic(&self, device_id: &str, telemetry_topic: &str) -> Result<(), DeviceError> {
        let mut mapping = self.topic_to_device.write().await;
        mapping.insert(telemetry_topic.to_string(), device_id.to_string());
        tracing::info!(
            "Registered custom topic mapping: '{}' -> device '{}'",
            telemetry_topic, device_id
        );
        Ok(())
    }

    /// Unregister a custom telemetry topic for a device
    pub async fn unregister_custom_topic(&self, telemetry_topic: &str) -> Result<(), DeviceError> {
        let mut mapping = self.topic_to_device.write().await;
        mapping.remove(telemetry_topic);
        tracing::info!("Unregistered custom topic mapping: '{}'", telemetry_topic);
        Ok(())
    }

    /// Get all custom topic mappings (for debugging/monitoring)
    pub async fn get_topic_mappings(&self) -> HashMap<String, String> {
        self.topic_to_device.read().await.clone()
    }

    /// Start HASS (Home Assistant) MQTT discovery
    /// HASS discovery functionality is deprecated
    pub async fn start_hass_discovery(&self) -> Result<(), DeviceError> {
        tracing::warn!("HASS discovery is deprecated and no longer functional");
        Ok(())
    }

    /// Stop HASS discovery (deprecated)
    pub async fn stop_hass_discovery(&self) -> Result<(), DeviceError> {
        tracing::warn!("HASS discovery is deprecated and no longer functional");
        let _ = std::mem::replace(&mut *self.hass_discovery_enabled.write().await, false);
        self.hass_discovered_devices.write().await.clear();
        Ok(())
    }

    /// Register a HASS device state topic mapping (deprecated)
    pub async fn register_hass_state_topic(
        &self,
        _device_id: &str,
        _metric_name: &str,
        _state_topic: &str,
    ) -> Result<(), DeviceError> {
        tracing::warn!("HASS discovery is deprecated and no longer functional");
        Ok(())
    }

    /// Save HASS discovery enabled state to storage (stub)
    async fn save_hass_discovery_state(&self, _enabled: bool) {
        // HASS discovery deprecated - stub
    }

    /// Load HASS discovery enabled state from storage (stub)
    async fn load_hass_discovery_state(&self) -> bool {
        false
    }

    /// Check if HASS discovery is enabled
    pub async fn is_hass_discovery_enabled(&self) -> bool {
        *self.hass_discovery_enabled.read().await
    }

    /// Restore HASS state topic mappings (deprecated)
    pub async fn restore_hass_state_topic_mappings(&self) -> Result<(), DeviceError> {
        tracing::warn!("HASS discovery is deprecated and no longer functional");
        Ok(())
    }

    /// Get HASS discovered devices (deprecated)
    pub async fn get_hass_discovered_devices(&self) -> Vec<DiscoveredHassDevice> {
        Vec::new()
    }

    /// Get discovered HASS devices aggregated by physical device (deprecated)
    pub async fn get_hass_discovered_devices_aggregated(&self) -> Vec<AggregatedHassDevice> {
        Vec::new()
    }

    /// Clear discovered HASS devices (deprecated)
    pub async fn clear_hass_discovered_devices(&self) {
        self.hass_discovered_devices.write().await.clear();
    }

    /// Get a specific discovered HASS device by entity_id (deprecated)
    pub async fn get_hass_discovered_device(
        &self,
        _entity_id: &str,
    ) -> Option<DiscoveredHassDevice> {
        None
    }
}

/// Device implementation for MQTT devices
pub struct MqttDevice {
    id: DeviceId,
    name: String,
    device_type: String,
    device_id: String,
    manager: Arc<MqttDeviceManager>,
}

impl MqttDevice {
    pub fn new(
        name: String,
        device_type: String,
        device_id: String,
        manager: Arc<MqttDeviceManager>,
    ) -> Self {
        Self {
            id: DeviceId::new(),
            name,
            device_type,
            device_id,
            manager,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = MqttManagerConfig::default();
        assert_eq!(config.broker, "localhost");
        assert_eq!(config.port, 1883);
        assert_eq!(config.keep_alive, 60);
    }

    #[test]
    fn test_config_builder() {
        let config = MqttManagerConfig::new("192.168.1.1")
            .with_port(8883)
            .with_auth("user", "pass");

        assert_eq!(config.broker, "192.168.1.1");
        assert_eq!(config.port, 8883);
        assert_eq!(config.username, Some("user".to_string()));
        assert_eq!(config.password, Some("pass".to_string()));
    }

    #[test]
    fn test_full_broker_addr() {
        let config = MqttManagerConfig::new("broker.example.com");
        assert_eq!(config.full_broker_addr(), "broker.example.com:1883");

        let config = config.with_port(8883);
        assert_eq!(config.full_broker_addr(), "broker.example.com:8883");
    }
}
