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

use crate::adapter::{AdapterError, AdapterResult, ConnectionStatus, DeviceAdapter, DeviceEvent};
use crate::image_storage::save_image_binary;
use crate::mdl::MetricValue;
use crate::mqtt::MqttConfig;
use crate::protocol::ProtocolMapping;
use crate::registry::DeviceRegistry;
use crate::telemetry::TimeSeriesStorage;
use crate::unified_extractor::UnifiedExtractor;

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use futures::{FutureExt, Stream, StreamExt};
use neomind_core::EventBus;
use neomind_core::NeoMindEvent;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

// TLS support
use rumqttc::{TlsConfiguration, Transport};
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use std::io::Cursor;

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
    /// Payload field to use as the device identity when the topic cannot
    /// uniquely identify a device (e.g. a gateway forwarding many devices on
    /// one topic). `None`/empty → auto-detect common fields (device_id, sn,
    /// mac, ...). Explicit value wins over auto-detection.
    #[serde(default)]
    pub device_id_field: Option<String>,
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
            device_id_field: None,
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
    /// Topics this adapter has published OUTBOUND command payloads to.
    /// Used to suppress the broker self-echo — when the embedded broker
    /// reflects our own publish back through our wildcard subscription,
    /// the inbound handler must NOT route it through auto-onboarding
    /// (otherwise every `capture` command creates a phantom "discovered
    /// device" entry for the command topic).
    outbound_command_topics: Arc<RwLock<HashSet<String>>>,
    /// Unified data extractor
    extractor: Arc<UnifiedExtractor>,
    /// Data directory for image storage (runtime, not config)
    data_dir: Arc<RwLock<Option<PathBuf>>>,
}

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
            outbound_command_topics: Arc::new(RwLock::new(HashSet::new())),
            extractor,
            data_dir: Arc::new(RwLock::new(None)),
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

    /// Set the data directory for image storage.
    pub fn set_data_dir(&self, data_dir: PathBuf) {
        let data_dir_arc = self.data_dir.clone();
        tokio::spawn(async move {
            *data_dir_arc.write().await = Some(data_dir);
        });
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
            outbound_command_topics: Arc::new(RwLock::new(HashSet::new())),
            extractor,
            data_dir: Arc::new(RwLock::new(None)),
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

    /// Re-subscribe to telemetry topics of all registered devices for a broker.
    ///
    /// Handles the server-restart scenario: when a broker is (re-)added, devices
    /// registered with custom `telemetry_topic`s need those topics re-subscribed so
    /// their data flows through. Without this, registered devices silently receive no
    /// data after restart until they are manually re-registered. (Bug 3)
    async fn subscribe_device_telemetry_topics(
        &self,
        client: &rumqttc::AsyncClient,
        broker_id: &str,
        subscribed_topics: &Arc<RwLock<std::collections::HashSet<String>>>,
    ) {
        let registry = self.device_registry.read().await;
        let devices = registry.list_devices();
        drop(registry);

        let mut subscribed_count = 0u32;
        for device in &devices {
            if let Some(ref telemetry_topic) = device.connection_config.telemetry_topic {
                // Skip if already covered by an existing subscription (e.g. "#"
                // on the internal broker) — re-subscribing would make rumqttc
                // deliver each message twice, duplicating every metric.
                let covered = subscribed_topics
                    .read()
                    .await
                    .iter()
                    .any(|existing| topic_filter_covers(existing, telemetry_topic));
                if covered {
                    debug!(
                        "Device telemetry topic '{}' already covered on broker {}, skipping re-subscribe",
                        telemetry_topic, broker_id
                    );
                    continue;
                }

                debug!(
                    "Re-subscribing to device telemetry topic '{}' for broker '{}' (device '{}')",
                    telemetry_topic, broker_id, device.device_id
                );
                if let Err(e) = client
                    .subscribe(telemetry_topic.as_str(), rumqttc::QoS::AtLeastOnce)
                    .await
                {
                    warn!(
                        "Failed to re-subscribe to telemetry topic '{}' on broker {}: {}",
                        telemetry_topic, broker_id, e
                    );
                } else {
                    subscribed_topics
                        .write()
                        .await
                        .insert(telemetry_topic.clone());
                    subscribed_count += 1;
                }
            }
        }
        if subscribed_count > 0 {
            info!(
                "Re-subscribed to {} device telemetry topics for broker '{}'",
                subscribed_count, broker_id
            );
        }
    }

    /// Re-subscribe all previously-subscribed topics after a broker reconnect.
    ///
    /// With `clean_session=true` (the rumqttc default), the broker discards all
    /// subscriptions on disconnect. rumqttc reconnects internally but does NOT
    /// auto-resubscribe, so after a network drop + reconnect data silently stops
    /// flowing. This is called from the event loop task when an `Ok` poll result
    /// follows one or more `Err` results (i.e. a reconnect just succeeded). (Bug 6)
    ///
    /// Takes the `mqtt_clients` map (the spawned task owns an Arc clone) and
    /// re-subscribes every topic currently tracked in the broker's
    /// `subscribed_topics` set. Topics that fail to re-subscribe are removed from
    /// the set so the dedup cache stays accurate.
    async fn resubscribe_after_reconnect(
        mqtt_clients: &Arc<RwLock<HashMap<String, MqttClientInner>>>,
        broker_id: &str,
    ) {
        // Snapshot current topics + clone the client out so we don't hold any map
        // or set locks across the subscribe awaits.
        let (client, topics): (rumqttc::AsyncClient, Vec<String>) = {
            let clients = mqtt_clients.read().await;
            let Some(inner) = clients.get(broker_id) else {
                return;
            };
            let client = inner.client.clone();
            let topics = inner
                .subscribed_topics
                .read()
                .await
                .iter()
                .cloned()
                .collect();
            (client, topics)
        };

        if topics.is_empty() {
            debug!(
                "No topics to re-subscribe after reconnect on broker '{}'",
                broker_id
            );
            return;
        }

        let total = topics.len() as u32;
        let mut failed: Vec<String> = Vec::new();
        let mut ok: u32 = 0;
        for topic in &topics {
            match client
                .subscribe(topic.as_str(), rumqttc::QoS::AtLeastOnce)
                .await
            {
                Ok(_) => ok += 1,
                Err(e) => {
                    warn!(
                        "Reconnect resubscribe failed for '{}' on broker '{}': {}",
                        topic, broker_id, e
                    );
                    failed.push(topic.clone());
                }
            }
        }

        // Remove failed topics from the dedup set so they aren't falsely cached.
        if !failed.is_empty() {
            let clients = mqtt_clients.read().await;
            if let Some(inner) = clients.get(broker_id) {
                let mut set = inner.subscribed_topics.write().await;
                for t in &failed {
                    set.remove(t);
                }
            }
        }

        info!(
            "Re-subscribed {}/{} topics after reconnect on broker '{}'",
            ok, total, broker_id
        );
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
        mqttoptions.set_clean_session(self.config.mqtt.clean_session);

        // Set credentials if provided
        if let (Some(user), Some(pass)) = (username, password) {
            mqttoptions.set_credentials(&user, &pass);
        }

        // Configure TLS from adapter config
        if self.config.mqtt.tls {
            let transport = Self::build_tls_transport(
                self.config.mqtt.ca_cert.as_deref(),
                self.config.mqtt.client_cert.as_deref(),
                self.config.mqtt.client_key.as_deref(),
            )?;
            mqttoptions.set_transport(transport);
            info!(
                "MQTT adapter configured with TLS for broker '{}'",
                broker_id
            );
        }

        // Create client with a larger request channel capacity (Bug 4: capacity 10
        // risks deadlock when subscribing many topics before the event loop polls).
        let (client, eventloop) = rumqttc::AsyncClient::new(mqttoptions, 100);

        let running = Arc::new(RwLock::new(true));
        // Bug 6: with clean_session=true the broker forgets subscriptions on
        // reconnect. The event loop task detects reconnects (Err then Ok) and calls
        // `resubscribe_after_reconnect`, which re-subscribes every topic in this set
        // and prunes any that fail. So this set is kept accurate across reconnects.
        let subscribed_topics = Arc::new(RwLock::new(std::collections::HashSet::new()));

        // Bug 4: clone client + subscribed set so we can subscribe AFTER spawning the
        // event loop task. Subscribing before the event loop is polled can deadlock
        // once the request channel fills up.
        let client_for_sub = client.clone();
        let subscribed_topics_for_sub = subscribed_topics.clone();

        // Store the client BEFORE spawning the event loop
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

        // Restore topic_to_device and device_types mappings from device registry.
        // This is critical for server restart - devices must be able to receive data
        // and metrics must be stored.
        let registry = self.device_registry.read().await;
        let devices = registry.list_devices();
        drop(registry);
        let mut topic_mapping = self.topic_to_device.write().await;
        let mut type_mapping = self.device_types.write().await;
        let mut restored_topic_count = 0;
        let mut restored_type_count = 0;

        for device in &devices {
            // Restore topic_to_device mapping
            // Use explicit telemetry_topic if set, otherwise default pattern
            // device/{type}/{id}/uplink. The default pattern MUST be in the map —
            // otherwise the auto-onboarding check at message intake (which queries
            // this map to decide if a standard-uplink topic is registered) will
            // misroute every message from default-topic devices to the discovery
            // path, even though they are registered.
            let topic = device
                .connection_config
                .telemetry_topic
                .clone()
                .unwrap_or_else(|| {
                    format!("device/{}/{}/uplink", device.device_type, device.device_id)
                });
            topic_mapping.insert(topic.clone(), device.device_id.clone());
            restored_topic_count += 1;
            debug!(
                "Restored topic mapping: '{}' -> '{}'",
                topic, device.device_id
            );

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
        let outbound_command_topics = self.outbound_command_topics.clone();
        let data_dir_clone = self.data_dir.clone();

        info!(
            "Starting event loop task for broker '{}', connecting to {}...",
            broker_id, broker_addr
        );

        // Spawn a periodic metric_cache sweep to prevent unbounded growth from
        // phantom auto-onboarded devices. Drops entries older than 30 min.
        let sweep_cache = Arc::clone(&self.metric_cache);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
            loop {
                interval.tick().await;
                let cutoff = chrono::Utc::now() - chrono::Duration::minutes(30);
                let mut cache = sweep_cache.write().await;
                let before = cache.len();
                for metrics in cache.values_mut() {
                    metrics.retain(|_, (_, ts)| *ts > cutoff);
                }
                cache.retain(|_, metrics| !metrics.is_empty());
                let removed = before.saturating_sub(cache.len());
                if removed > 0 {
                    tracing::debug!(removed, remaining = cache.len(), "metric_cache sweep");
                }
            }
        });

        tokio::spawn(async move {
            let mut eventloop = eventloop;
            let mut error_count: u32 = 0;
            // Bug 6: track whether we've seen at least one poll error since the last
            // successful poll. When the next Ok arrives we re-subscribe, because
            // clean_session=true brokers drop our subscriptions on every disconnect.
            let mut was_disconnected = false;

            while *running_flag.read().await {
                match eventloop.poll().await {
                    Ok(notification) => {
                        error_count = 0; // Reset error count on success
                        if was_disconnected {
                            was_disconnected = false;
                            if let Err(panic) = std::panic::AssertUnwindSafe(
                                Self::resubscribe_after_reconnect(&mqtt_clients, &broker_id_clone),
                            )
                            .catch_unwind()
                            .await
                            {
                                tracing::error!(
                                    broker = %broker_id_clone,
                                    panic = ?panic,
                                    "MQTT resubscribe panicked — eventloop continues"
                                );
                            }
                        }
                        // [panic guard] A panic inside the notification
                        // handler (arbitrary device payloads) used to unwind
                        // and KILL this poll task — the adapter stayed
                        // "running" but never polled again (MQTT silently
                        // dead until restart). Catch and keep polling.
                        if let Err(panic) =
                            std::panic::AssertUnwindSafe(Self::handle_mqtt_notification(
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
                                &outbound_command_topics,
                                data_dir_clone.read().await.as_ref(),
                            ))
                            .catch_unwind()
                            .await
                        {
                            tracing::error!(
                                broker = %broker_id_clone,
                                panic = ?panic,
                                "MQTT notification handler panicked — eventloop continues"
                            );
                        }
                    }
                    Err(e) => {
                        error_count += 1;
                        was_disconnected = true;
                        // Exponential backoff: 1s, 2s, 4s, 8s, 16s, 30s, 30s, ...
                        // rumqttc handles reconnection internally — just keep polling
                        let backoff = Duration::from_secs((1u64 << error_count.min(5)).min(30));
                        warn!(
                            "MQTT broker {} error ({}), reconnecting in {:?}: {}",
                            broker_id_clone, error_count, backoff, e
                        );
                        tokio::time::sleep(backoff).await;
                    }
                }
            }

            // Remove this broker from clients map when task ends
            mqtt_clients.write().await.remove(&broker_id_clone);
            info!("MQTT broker {} connection closed", broker_id_clone);
        });

        // Bug 4: subscribe AFTER the event loop task is spawned and polling. The event
        // loop drains the request channel, so subscribe() calls won't block on a full
        // channel.
        //
        // NOTE: add_broker() is used by the INTERNAL embedded broker via start(). The
        // internal broker configures subscribe_topics = ["#"] (subscribe to all topics
        // for auto-discovery), so we MUST include self.config.subscribe_topics here —
        // otherwise the internal broker only sees device/+/+/uplink|downlink messages
        // and devices publishing to custom topics (e.g. "ne101/abc") are never seen.
        // add_broker_with_tls (external brokers) takes subscribe_topics as an explicit
        // parameter and is unaffected.
        let initial_topics = normalized_initial_subscription_topics(&self.config.subscribe_topics);

        // Bug 5: track subscription success so a total failure surfaces as an error
        // instead of silently marking the broker as "connected".
        let mut success_count = 0u32;
        let mut total_count = 0u32;
        for topic in &initial_topics {
            total_count += 1;
            debug!(
                "Attempting to subscribe to topic '{}' on broker '{}'...",
                topic, broker_id
            );
            if let Err(e) = client_for_sub
                .subscribe(topic, rumqttc::QoS::AtLeastOnce)
                .await
            {
                warn!(
                    "Failed to subscribe to {} on broker {}: {}",
                    topic, broker_id, e
                );
            } else {
                success_count += 1;
                subscribed_topics_for_sub
                    .write()
                    .await
                    .insert(topic.clone());
                info!(
                    "Successfully subscribed to topic '{}' on broker '{}'",
                    topic, broker_id
                );
            }
        }

        // Bug 3: re-subscribe telemetry topics of registered devices (server-restart)
        self.subscribe_device_telemetry_topics(
            &client_for_sub,
            &broker_id,
            &subscribed_topics_for_sub,
        )
        .await;

        info!(
            "Subscribed to {} topics for broker '{}': {:?}",
            subscribed_topics_for_sub.read().await.len(),
            broker_id,
            subscribed_topics_for_sub.read().await
        );

        // Bug 5: if every subscription failed, fail the broker add entirely so the
        // caller can surface the error rather than report a false "connected".
        // Tear down the spawned event loop task + client so we don't leak a
        // half-connected broker that pretends to be alive.
        if success_count == 0 && total_count > 0 {
            // Signal the event loop task to exit on its next loop iteration.
            if let Some(running) = self
                .mqtt_clients
                .read()
                .await
                .get(&broker_id)
                .map(|inner| inner.running.clone())
            {
                *running.write().await = false;
            }
            self.mqtt_clients.write().await.remove(&broker_id);
            return Err(AdapterError::Configuration(format!(
                "All {} subscriptions failed on broker {}",
                total_count, broker_id
            )));
        }

        info!("Added MQTT broker: {} ({})", broker_id, broker_addr);
        Ok(())
    }

    /// Add a broker connection with full TLS support.
    ///
    /// This allows connecting to MQTT brokers with TLS/mTLS encryption.
    pub async fn add_broker_with_tls(
        &self,
        broker_id: impl Into<String>,
        broker_host: impl Into<String>,
        broker_port: u16,
        username: Option<String>,
        password: Option<String>,
        tls: bool,
        ca_cert: Option<String>,
        client_cert: Option<String>,
        client_key: Option<String>,
        client_id: Option<String>,
        subscribe_topics: Vec<String>,
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
        let mqtt_client_id =
            client_id.unwrap_or_else(|| format!("neomind-{}-{}", broker_id, Uuid::new_v4()));
        let mut mqttoptions = rumqttc::MqttOptions::new(&mqtt_client_id, &broker_host, broker_port);
        mqttoptions.set_max_packet_size(10 * 1024 * 1024, 10 * 1024 * 1024);
        mqttoptions.set_keep_alive(Duration::from_secs(60));
        mqttoptions.set_clean_session(self.config.mqtt.clean_session);

        // Set credentials if provided
        if let (Some(user), Some(pass)) = (username, password) {
            mqttoptions.set_credentials(&user, &pass);
        }

        // Configure TLS if enabled
        if tls {
            let transport = Self::build_tls_transport(
                ca_cert.as_deref(),
                client_cert.as_deref(),
                client_key.as_deref(),
            )?;
            mqttoptions.set_transport(transport);
            info!(
                "TLS enabled for broker {} with {} verification",
                broker_id,
                if ca_cert.is_some() {
                    "custom CA"
                } else {
                    "system CA"
                }
            );
        }

        // Create client with a larger request channel capacity (Bug 4: capacity 10
        // risks deadlock when subscribing many topics before the event loop polls).
        let (client, eventloop) = rumqttc::AsyncClient::new(mqttoptions, 100);

        let running = Arc::new(RwLock::new(true));
        // Bug 6: with clean_session=true the broker forgets subscriptions on
        // reconnect. The event loop task detects reconnects (Err then Ok) and calls
        // `resubscribe_after_reconnect`, which re-subscribes every topic in this set
        // and prunes any that fail. So this set is kept accurate across reconnects.
        let subscribed_topics = Arc::new(RwLock::new(std::collections::HashSet::new()));

        // Bug 4: clone client + subscribed set so we can subscribe AFTER spawning the
        // event loop task. Subscribing before the event loop is polled can deadlock
        // once the request channel fills up.
        let client_for_sub = client.clone();
        let subscribed_topics_for_sub = subscribed_topics.clone();

        // Store the client BEFORE spawning the event loop
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

        // Restore topic_to_device and device_types mappings from device registry.
        let registry = self.device_registry.read().await;
        let devices = registry.list_devices();
        drop(registry);
        let mut topic_mapping = self.topic_to_device.write().await;
        let mut type_mapping = self.device_types.write().await;
        let mut restored_topic_count = 0;
        let mut restored_type_count = 0;

        for device in &devices {
            // Use explicit telemetry_topic if set, otherwise default pattern
            // device/{type}/{id}/uplink (see add_broker restore comment for rationale).
            let topic = device
                .connection_config
                .telemetry_topic
                .clone()
                .unwrap_or_else(|| {
                    format!("device/{}/{}/uplink", device.device_type, device.device_id)
                });
            topic_mapping.insert(topic, device.device_id.clone());
            restored_topic_count += 1;
            type_mapping.insert(device.device_id.clone(), device.device_type.clone());
            restored_type_count += 1;
        }

        drop(topic_mapping);
        drop(type_mapping);
        info!(
            "Restored {} topic-to-device and {} device_type mappings for broker {}",
            restored_topic_count, restored_type_count, broker_id
        );

        // Update connection status
        self.update_connection_status().await;

        // Spawn message processing task (consumes notifications from the channel).
        let running_flag = running.clone();
        let running_flag2 = running.clone();
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
        let broker_id_clone2 = broker_id.clone();
        let extractor = self.extractor.clone();
        let topic_to_device = self.topic_to_device.clone();
        let outbound_command_topics = self.outbound_command_topics.clone();
        let data_dir_clone = self.data_dir.clone();

        let (eventloop_tx, eventloop_rx) = async_channel::unbounded();
        let event_tx_clone = event_tx.clone();

        tokio::spawn(async move {
            while *running_flag.read().await {
                match eventloop_rx.recv().await {
                    Ok(notification) => {
                        Self::handle_mqtt_notification(
                            notification,
                            &config,
                            &event_tx_clone,
                            &event_bus,
                            &device_types,
                            &metric_cache,
                            &telemetry_storage,
                            &device_registry,
                            &connection_status,
                            &broker_id_clone,
                            &extractor,
                            &topic_to_device,
                            &outbound_command_topics,
                            data_dir_clone.read().await.as_ref(),
                        )
                        .await;
                    }
                    Err(_) => break,
                }
            }
        });

        info!(
            "Starting event loop task for broker '{}' with TLS, connecting to {}...",
            broker_id, broker_addr
        );

        // Bug 4: spawn the event loop poll task BEFORE subscribing. The poll task
        // drains the request channel, so subscribe() calls won't deadlock.
        tokio::spawn(async move {
            let mut eventloop = eventloop;
            let mut error_count: u32 = 0;
            // Bug 6: clean_session=true brokers forget our subscriptions on every
            // disconnect. When the next Ok follows one or more Errs, re-subscribe.
            let mut was_disconnected = false;

            while *running_flag2.read().await {
                match eventloop.poll().await {
                    Ok(notification) => {
                        error_count = 0;
                        if was_disconnected {
                            was_disconnected = false;
                            if let Err(panic) = std::panic::AssertUnwindSafe(
                                Self::resubscribe_after_reconnect(&mqtt_clients, &broker_id_clone2),
                            )
                            .catch_unwind()
                            .await
                            {
                                tracing::error!(
                                    broker = %broker_id_clone2,
                                    panic = ?panic,
                                    "MQTT resubscribe panicked — eventloop continues"
                                );
                            }
                        }
                        if let Err(e) = eventloop_tx.send(notification).await {
                            warn!("Failed to send MQTT notification to channel: {}", e);
                        }
                    }
                    Err(e) => {
                        error_count += 1;
                        was_disconnected = true;
                        // Exponential backoff: 1s, 2s, 4s, 8s, 16s, 30s, 30s, ...
                        // rumqttc handles reconnection internally — just keep polling
                        let backoff = Duration::from_secs((1u64 << error_count.min(5)).min(30));
                        warn!(
                            "MQTT broker {} error ({}), reconnecting in {:?}: {}",
                            broker_id_clone2, error_count, backoff, e
                        );
                        tokio::time::sleep(backoff).await;
                    }
                }
            }

            mqtt_clients.write().await.remove(&broker_id_clone2);
            info!("MQTT broker {} connection closed", broker_id_clone2);
        });

        // Bug 4: subscribe AFTER the event loop poll task is spawned. Bug 1: only the
        // explicit `subscribe_topics` function parameter is used — the duplicate
        // self.config.subscribe_topics loop was removed (the API handler already sets
        // config.subscribe_topics from the same broker data, so adding it twice
        // caused rumqttc to receive duplicate SUBSCRIBE requests).
        let initial_topics = normalized_initial_subscription_topics(&subscribe_topics);

        // Bug 5: track subscription success so a total failure surfaces as an error
        // instead of silently marking the broker as "connected".
        let mut success_count = 0u32;
        let mut total_count = 0u32;
        for topic in &initial_topics {
            total_count += 1;
            debug!(
                "Attempting to subscribe to topic '{}' on broker '{}'...",
                topic, broker_id
            );
            if let Err(e) = client_for_sub
                .subscribe(topic, rumqttc::QoS::AtLeastOnce)
                .await
            {
                warn!(
                    "Failed to subscribe to {} on broker {}: {}",
                    topic, broker_id, e
                );
            } else {
                success_count += 1;
                subscribed_topics_for_sub
                    .write()
                    .await
                    .insert(topic.clone());
                info!(
                    "Successfully subscribed to topic '{}' on broker '{}'",
                    topic, broker_id
                );
            }
        }

        // Bug 3: re-subscribe telemetry topics of registered devices (server-restart)
        self.subscribe_device_telemetry_topics(
            &client_for_sub,
            &broker_id,
            &subscribed_topics_for_sub,
        )
        .await;

        info!(
            "Subscribed to {} topics for broker '{}': {:?}",
            subscribed_topics_for_sub.read().await.len(),
            broker_id,
            subscribed_topics_for_sub.read().await
        );

        // Bug 5: if every subscription failed, fail the broker add entirely.
        // Tear down the spawned event loop tasks + client so we don't leak a
        // half-connected broker that pretends to be alive.
        if success_count == 0 && total_count > 0 {
            if let Some(running) = self
                .mqtt_clients
                .read()
                .await
                .get(&broker_id)
                .map(|inner| inner.running.clone())
            {
                *running.write().await = false;
            }
            self.mqtt_clients.write().await.remove(&broker_id);
            return Err(AdapterError::Configuration(format!(
                "All {} subscriptions failed on broker {}",
                total_count, broker_id
            )));
        }

        info!(
            "Added MQTT broker with TLS: {} ({})",
            broker_id, broker_addr
        );
        Ok(())
    }

    /// Resolve a string that is either PEM content or a file path to PEM content.
    fn resolve_pem(input: &str) -> Result<String, Box<dyn std::error::Error>> {
        // Require both BEGIN and END markers to identify PEM content.
        // This avoids misidentifying file paths that might contain "-----BEGIN".
        let trimmed = input.trim();
        if trimmed.contains("-----BEGIN") && trimmed.contains("-----END") {
            Ok(input.to_string())
        } else {
            // Treat as file path
            let content = std::fs::read_to_string(input)?;
            Ok(content)
        }
    }

    /// Build TLS transport with optional certificates.
    pub fn build_tls_transport(
        ca_cert: Option<&str>,
        client_cert: Option<&str>,
        client_key: Option<&str>,
    ) -> AdapterResult<Transport> {
        // Use rumqttc's re-exported rustls types for compatibility
        use rumqttc::tokio_rustls::rustls::{ClientConfig, RootCertStore};

        let mut root_cert_store = RootCertStore::empty();

        // Load custom CA certificate if provided
        if let Some(ca_input) = ca_cert {
            let ca_pem = Self::resolve_pem(ca_input).map_err(|e| {
                AdapterError::Configuration(format!("Failed to read CA cert: {}", e))
            })?;
            let ca_certs = Self::load_certs(&ca_pem).map_err(|e| {
                AdapterError::Configuration(format!("Failed to load CA cert: {}", e))
            })?;
            let ca_cert_count = ca_certs.len();
            for cert in ca_certs {
                root_cert_store.add(cert).map_err(|e| {
                    AdapterError::Configuration(format!("Failed to add CA cert to store: {}", e))
                })?;
            }
            info!("Loaded {} CA certificates", ca_cert_count);
        } else {
            // Use system's native certificate store.
            // rustls-native-certs 0.8: load_native_certs() returns CertificateResult
            // (not Result); partial failures are collected in `.errors` and don't abort,
            // so we log them as a warning rather than erroring out.
            let native = rustls_native_certs::load_native_certs();
            let cert_count = native.certs.len();
            for cert in native.certs {
                root_cert_store.add(cert).map_err(|e| {
                    AdapterError::Configuration(format!("Failed to add native cert: {}", e))
                })?;
            }
            if native.errors.is_empty() {
                info!("Loaded {} system CA certificates", cert_count);
            } else {
                warn!(
                    "Loaded {} system CA certs; {} source(s) had errors: {:?}",
                    cert_count,
                    native.errors.len(),
                    native.errors
                );
            }
        }

        // Build client config
        let mut client_config = ClientConfig::builder()
            .with_root_certificates(root_cert_store.clone())
            .with_no_client_auth();

        // Configure mTLS if client certificates are provided
        if let (Some(cert_input), Some(key_input)) = (client_cert, client_key) {
            let cert_pem = Self::resolve_pem(cert_input).map_err(|e| {
                AdapterError::Configuration(format!("Failed to read client cert: {}", e))
            })?;
            let key_pem = Self::resolve_pem(key_input).map_err(|e| {
                AdapterError::Configuration(format!("Failed to read client key: {}", e))
            })?;
            let client_certs = Self::load_certs(&cert_pem).map_err(|e| {
                AdapterError::Configuration(format!("Failed to load client cert: {}", e))
            })?;
            let client_key = Self::load_private_key(&key_pem).map_err(|e| {
                AdapterError::Configuration(format!("Failed to load client key: {}", e))
            })?;

            client_config = ClientConfig::builder()
                .with_root_certificates(root_cert_store)
                .with_client_auth_cert(client_certs, client_key)
                .map_err(|e| {
                    AdapterError::Configuration(format!("Failed to configure mTLS: {}", e))
                })?;
            info!("Configured mTLS with client certificate");
        }

        Ok(Transport::tls_with_config(TlsConfiguration::from(
            client_config,
        )))
    }

    /// Load PEM-encoded certificates from a string.
    fn load_certs(pem: &str) -> Result<Vec<CertificateDer<'static>>, Box<dyn std::error::Error>> {
        let mut certs = Vec::new();
        let mut pem_cursor = Cursor::new(pem.as_bytes());
        let certs_iter = rustls_pemfile::certs(&mut pem_cursor);
        for cert in certs_iter {
            certs.push(cert?.to_owned());
        }
        Ok(certs)
    }

    /// Load a PEM-encoded private key from a string.
    ///
    /// Tries PKCS#8 first, then falls back to PKCS#1 RSA and SEC1 EC keys.
    fn load_private_key(pem: &str) -> Result<PrivateKeyDer<'static>, Box<dyn std::error::Error>> {
        // Try PKCS#8 (-----BEGIN PRIVATE KEY-----)
        let mut cursor = Cursor::new(pem.as_bytes());
        if let Some(key) = rustls_pemfile::pkcs8_private_keys(&mut cursor).next() {
            return Ok(PrivateKeyDer::Pkcs8(key?));
        }

        // Try PKCS#1 RSA (-----BEGIN RSA PRIVATE KEY-----)
        let mut cursor = Cursor::new(pem.as_bytes());
        if let Some(key) = rustls_pemfile::rsa_private_keys(&mut cursor).next() {
            return Ok(PrivateKeyDer::Pkcs1(key?));
        }

        // Try SEC1 EC (-----BEGIN EC PRIVATE KEY-----)
        let mut cursor = Cursor::new(pem.as_bytes());
        if let Some(key) = rustls_pemfile::ec_private_keys(&mut cursor).next() {
            return Ok(PrivateKeyDer::Sec1(key?));
        }

        Err("No supported private key found in PEM (tried PKCS#8, PKCS#1 RSA, SEC1 EC)".into())
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
        topic: Option<String>,
    ) -> AdapterResult<()> {
        // `payload` is the ALREADY-RENDERED payload produced by
        // `DeviceService::build_command_payload` (which delegates to
        // `payload_template::render`). The previous implementation
        // re-parsed this string into `HashMap<String, Value>` and
        // re-serialized it — which (a) destroyed non-object payloads
        // like HASS-style bare `"ON"` strings via `unwrap_or_default()`
        // collapsing them to `{}`, and (b) randomised key order via
        // HashMap iteration. We now publish the rendered bytes
        // verbatim.
        let clients = self.mqtt_clients.read().await;

        if clients.is_empty() {
            return Err(AdapterError::Connection(
                "No MQTT brokers connected".to_string(),
            ));
        }

        // Topic resolution priority:
        //   1. Device-configured `command_topic` (required for devices
        //      that don't follow the default downlink convention).
        //   2. Default when device_type is known.
        //   3. Bare fallback.
        let topic = if let Some(t) = topic.filter(|t| !t.is_empty()) {
            t
        } else {
            let device_type = self.device_types.read().await.get(device_id).cloned();
            if let Some(dt) = device_type {
                format!("device/{}/{}/downlink", dt, device_id)
            } else {
                format!("{}/command/{}", device_id, command_name)
            }
        };

        // Record this topic as an outbound command channel so the
        // inbound handler can recognise the broker self-echo (the
        // embedded broker reflects our own publish back through any
        // wildcard subscription) and skip auto-onboarding for it.
        // Without this, every successful `capture`/`sleep` publish
        // generates a phantom "Triggering auto-onboarding for
        // non-standard topic: <command-topic>" log entry and a
        // matching bogus discovered-device row.
        {
            let mut outbound = self.outbound_command_topics.write().await;
            outbound.insert(topic.clone());
        }

        let mut last_error = None;
        let mut success_count = 0u32;

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
                        command_name, device_id, broker_id
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
        let device_opt = self.device_registry.read().await.get_device(device_id);
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
            // Use explicit telemetry_topic if set, otherwise default pattern
            // device/{type}/{id}/uplink. Both branches MUST record the mapping —
            // the auto-onboarding check at message intake queries this map to
            // decide whether a standard-uplink topic belongs to a registered
            // device, and missing entries cause registered devices to be
            // misrouted to the discovery path.
            let topic = device
                .connection_config
                .telemetry_topic
                .clone()
                .unwrap_or_else(|| format!("device/{}/{}/uplink", device.device_type, device_id));
            self.subscribe_topic(&topic).await?;
            info!(
                "Subscribed to device {} telemetry topic: {}",
                device_id, topic
            );
            // Store topic-to-device mapping for message routing
            {
                let mut mapping = self.topic_to_device.write().await;
                mapping.insert(topic.clone(), device_id.to_string());
                info!("Stored topic mapping: {} -> {}", topic, device_id);
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
            // If device not found in registry, use a wildcard pattern to match all topics for this device
            let topic = format!("device/+/{}/#", device_id);
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
        let device_opt = self.device_registry.read().await.get_device(device_id);
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

            // Skip if an existing subscription already covers this topic. The
            // internal broker subscribes to "#", so a per-device
            // "device/.../uplink" is fully redundant — and worse, rumqttc
            // delivers each matching message once PER subscription (twice here),
            // duplicating every metric. Mirrors the overlap-dedup the initial
            // topic list gets via normalized_initial_subscription_topics.
            let covered = inner
                .subscribed_topics
                .read()
                .await
                .iter()
                .any(|existing| topic_filter_covers(existing, topic));
            if covered {
                debug!(
                    "Topic '{}' already covered by an existing subscription on broker {}, skipping to avoid duplicate delivery",
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

    /// Convert image data (Binary or base64 String) to URL if applicable.
    pub fn convert_binary_to_url(
        device_id: &str,
        metric_name: &str,
        timestamp: i64,
        value: MetricValue,
        data_dir: Option<&PathBuf>,
    ) -> MetricValue {
        match value {
            MetricValue::Binary(bytes) => {
                if let Some(dir) = data_dir {
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
                // MQTT JSON payloads carry images as base64 strings — detect and convert
                if let Some(bytes) = crate::image_storage::try_decode_base64_image(&s) {
                    if let Some(dir) = data_dir {
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
            other => other,
        }
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
        outbound_command_topics: &Arc<RwLock<HashSet<String>>>,
        data_dir: Option<&PathBuf>,
    ) {
        match notification {
            rumqttc::Event::Incoming(rumqttc::Packet::Publish(publish)) => {
                let topic = publish.topic.to_string();
                let payload = publish.payload.to_vec();

                debug!(
                    "Received MQTT message on topic: {}, payload length: {}",
                    topic,
                    payload.len()
                );

                let now = chrono::Utc::now();

                // External-broker `$SYS` presence synthesis.
                //
                // External MQTT brokers don't run our embedded rmqtt
                // `DevicePresenceHook`, so devices registered on them never
                // fire `DeviceTransportOnline/Offline` events via the hook
                // path. To keep the 4-state UI ("online/connectedIdle/offline/
                // disconnected") working, we subscribe to the broker's `$SYS`
                // client-presence broadcasts (see `create_and_connect_broker`,
                // which appends `$SYS/brokers/+/clients/+/{connected,disconnected}`
                // to the subscribe list) and synthesize transport events here.
                //
                // Skip internal NeoMind clients (e.g. `neomind-external-{id}` —
                // the adapter's own bridge connection) to avoid firing phantom
                // transport events for our own session.
                if topic.starts_with("$SYS/brokers/") {
                    if let Some((sys_client_id, is_online)) = parse_sys_presence_topic(&topic) {
                        if sys_client_id.starts_with("neomind-") {
                            debug!(
                                "Skipping $SYS presence for internal client '{}'",
                                sys_client_id
                            );
                        } else if let Some(bus) = event_bus {
                            let ts = now.timestamp();
                            debug!(
                                "Synthesizing transport event from $SYS: client_id='{}', online={}",
                                sys_client_id, is_online
                            );
                            if is_online {
                                bus.publish(NeoMindEvent::DeviceTransportOnline {
                                    device_id: sys_client_id.clone(),
                                    client_id: sys_client_id.clone(),
                                    timestamp: ts,
                                })
                                .await;
                            } else {
                                bus.publish(NeoMindEvent::DeviceTransportOffline {
                                    device_id: sys_client_id.clone(),
                                    client_id: sys_client_id.clone(),
                                    reason: None,
                                    timestamp: ts,
                                })
                                .await;
                            }
                        }
                    }
                    // $SYS topics must NEVER flow into telemetry or
                    // auto-onboarding paths — short-circuit here.
                    return;
                }

                // Check if this is a standard uplink format first
                let parts: Vec<&str> = topic.split('/').collect();
                let mut is_standard_uplink =
                    parts.len() >= 4 && parts[0] == "device" && parts.get(3) == Some(&"uplink");

                // If the topic is in standard uplink format but the device is NOT registered,
                // treat it as a discovery candidate so it falls through to the auto-onboarding
                // branch below. Without this, unregistered standard-uplink devices are silently
                // dropped (UnifiedExtractor returns 0 metrics for unknown device types) and never
                // appear in the Pending Devices list.
                if is_standard_uplink {
                    let is_registered = topic_to_device.read().await.contains_key(&topic);
                    if !is_registered {
                        info!(
                            "Standard uplink topic '{}' has no registered device, falling through to auto-onboarding",
                            topic
                        );
                        is_standard_uplink = false;
                    }
                }

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

                                debug!(
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
                                    // Convert Binary to URL before storage + event bus (fork point)
                                    let value = Self::convert_binary_to_url(
                                        &device_id,
                                        &metric.name,
                                        now.timestamp(),
                                        metric.value.clone(),
                                        data_dir,
                                    );

                                    // Update metric cache
                                    {
                                        let mut cache = metric_cache.write().await;
                                        cache
                                            .entry(device_id.clone())
                                            .or_default()
                                            .insert(metric.name.clone(), (value.clone(), now));
                                    }

                                    // Store in telemetry storage
                                    if let Some(storage) = telemetry_storage.read().await.as_ref() {
                                        let data_point = crate::telemetry::DataPoint {
                                            timestamp: now.timestamp(),
                                            value: value.clone(),
                                            quality: None,
                                        };
                                        if let Err(e) = storage
                                            .write(
                                                &format!("device:{}", device_id),
                                                &metric.name,
                                                data_point,
                                            )
                                            .await
                                        {
                                            error!(
                                                "Failed to write telemetry for {}/{}: {}",
                                                device_id, metric.name, e
                                            );
                                        } else {
                                            debug!(
                                                "Stored metric {} = {:?} for device {}",
                                                metric.name, value, device_id
                                            );
                                        }
                                    }

                                    // Emit to device event channel - event forwarding task will publish to EventBus
                                    if let Err(e) = event_tx.send(DeviceEvent::Metric {
                                        device_id: device_id.clone(),
                                        metric: metric.name.clone(),
                                        value: value.clone(),
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

                            // Convert Binary to URL before storage + event bus (fork point)
                            let value = Self::convert_binary_to_url(
                                &device_id,
                                &metric_name,
                                now.timestamp(),
                                value,
                                data_dir,
                            );

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
                                if let Err(e) = storage
                                    .write(
                                        &format!("device:{}", device_id),
                                        &metric_name,
                                        data_point,
                                    )
                                    .await
                                {
                                    tracing::warn!(
                                        device_id = %device_id,
                                        error = %e,
                                        "Failed to write telemetry to time-series storage"
                                    );
                                }
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
                // Only trigger if NOT a standard uplink format (those are handled above).
                //
                // Skip two classes of noise that previously polluted the
                // discovered-device stream:
                //   * **Self-echo** — the embedded broker reflects our own
                //     outbound command publishes back through wildcard
                //     subscriptions (e.g. `ne302/2819FD/down/control`).
                //     `outbound_command_topics` is populated by
                //     `send_command` immediately before each publish, so
                //     we can recognise and drop the echo here.
                //   * **LWT/status broadcasts** — devices publish
                //     `aicam/status/offline` and similar on
                //     connect/disconnect; these are not telemetry and
                //     would otherwise show up as phantom devices like
                //     `device_id=status, is_binary=true`.
                if !is_standard_uplink {
                    // First check if this topic belongs to a registered device.
                    // Registered devices MUST be processed before any noise filter —
                    // otherwise a device whose telemetry_topic happens to contain
                    // a `status` segment or end with `online`/`offline` (common in
                    // IoT firmware) would have its telemetry silently dropped.
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
                        debug!(
                            "Routing message for registered device {} from topic {}",
                            device_id, topic
                        );

                        // Try to get device type from device_types cache
                        let device_type_opt = {
                            let types = device_types.read().await;
                            types.get(device_id).cloned()
                        };

                        debug!("Device type for {}: {:?}", device_id, device_type_opt);

                        // Parse payload and process for the registered device
                        if let Ok(json_data) = serde_json::from_slice::<serde_json::Value>(&payload)
                        {
                            debug!("Successfully parsed JSON payload for device {}", device_id);

                            // Use UnifiedExtractor with the full JSON data
                            // The extractor handles dot-notation paths including "data.field" prefixes
                            // DO NOT pre-extract the "data" field - it causes double-extraction issues
                            if let Some(dt) = device_type_opt {
                                let result = extractor.extract(device_id, &dt, &json_data).await;
                                debug!(
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
                                    // Convert Binary to URL before storage + event bus (fork point)
                                    let value = Self::convert_binary_to_url(
                                        device_id,
                                        &metric.name,
                                        now.timestamp(),
                                        metric.value.clone(),
                                        data_dir,
                                    );

                                    // Update metric cache
                                    {
                                        let mut cache = metric_cache.write().await;
                                        cache
                                            .entry(device_id.clone())
                                            .or_default()
                                            .insert(metric.name.clone(), (value.clone(), now));
                                    }

                                    // Store in telemetry storage
                                    if let Some(storage) = telemetry_storage.read().await.as_ref() {
                                        let data_point = crate::telemetry::DataPoint {
                                            timestamp: now.timestamp(),
                                            value: value.clone(),
                                            quality: None,
                                        };
                                        if let Err(e) = storage
                                            .write(
                                                &format!("device:{}", device_id),
                                                &metric.name,
                                                data_point,
                                            )
                                            .await
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
                                        value: value.clone(),
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

                                    // Convert Binary to URL before storage + event bus (fork point)
                                    let value = Self::convert_binary_to_url(
                                        device_id,
                                        metric_name,
                                        now.timestamp(),
                                        value,
                                        data_dir,
                                    );

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
                                        if let Err(e) = storage
                                            .write(
                                                &format!("device:{}", device_id),
                                                metric_name,
                                                data_point,
                                            )
                                            .await
                                        {
                                            tracing::warn!(
                                                device_id = %device_id,
                                                error = %e,
                                                "Failed to write telemetry to time-series storage"
                                            );
                                        }
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
                        // Unknown topic — apply noise filters BEFORE auto-onboarding
                        // to prevent phantom discoveries.
                        //
                        //   * **Self-echo** — the embedded broker reflects our own
                        //     outbound command publishes back through wildcard
                        //     subscriptions (e.g. `ne302/2819FD/down/control`).
                        //   * **LWT/status broadcasts** — devices publish
                        //     `aicam/status/offline` and similar on connect/disconnect.
                        let is_self_echo = {
                            let outbound = outbound_command_topics.read().await;
                            outbound.contains(&topic)
                        };
                        if is_self_echo {
                            debug!(
                                "Skipping auto-onboarding for self-echo of outbound command topic: {}",
                                topic
                            );
                            return;
                        }
                        if looks_like_non_telemetry_topic(&topic) {
                            debug!(
                                "Skipping auto-onboarding for LWT/status-style topic: {}",
                                topic
                            );
                            return;
                        }

                        // Trigger auto-onboarding for unknown devices
                        // User-configured subscribe_topics define WHICH topics to listen on,
                        // and we should auto-onboard devices from those topics
                        info!(
                            "Triggering auto-onboarding for non-standard topic: {}",
                            topic
                        );

                        // Generate a device_id for auto-discovery.
                        // Precedence (gateway case: many devices forwarded on one
                        // topic need a payload-carried identity):
                        //   ① high-confidence topic extraction (device/{type}/{id},
                        //      subscription patterns)
                        //   ② payload identity (explicit config.device_id_field, or
                        //      auto-detect common fields device_id/sn/mac/...)
                        //   ③ weak topic fallback (parts[1])
                        //   ④ topic hash (last resort)
                        let auto_device_id = extract_device_id_from_topic_strong(&topic, config)
                            .and_then(sanitize_auto_device_id)
                            .or_else(|| extract_device_id_from_payload(&payload, config))
                            .and_then(sanitize_auto_device_id)
                            .or_else(|| extract_device_id_from_topic_weak(&topic))
                            .and_then(sanitize_auto_device_id)
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

                        // Bound the discovery sample — a huge payload would be stored
                        // verbatim as the onboarding sample (audit: no size limit).
                        if payload.len() > MAX_AUTOONBOARD_SAMPLE_BYTES {
                            tracing::warn!(
                                topic = %topic,
                                size = payload.len(),
                                limit = MAX_AUTOONBOARD_SAMPLE_BYTES,
                                "Auto-onboard payload exceeds sample size limit, skipping"
                            );
                            return;
                        }

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

                        // Publish DeviceDiscovered event via DeviceEvent::Discovery for auto-onboarding
                        {
                            let adapter_id = config.name.clone();
                            let sample = serde_json::json!({
                                "device_id": auto_device_id,
                                "timestamp": chrono::Utc::now().timestamp(),
                                "topic": topic,
                                "data": sample_data,
                                "format": data_format,
                                "is_binary": is_binary
                            });

                            let discovered = crate::adapter::DiscoveredDeviceInfo {
                                device_id: auto_device_id.clone(),
                                device_type: "unknown".to_string(),
                                name: None,
                                endpoint: Some(topic.to_string()),
                                capabilities: vec![],
                                timestamp: chrono::Utc::now().timestamp(),
                                metadata: serde_json::json!({
                                    "source": "mqtt",
                                    "broker_id": broker_id,
                                    "adapter_id": adapter_id,
                                    "original_topic": topic,
                                    "sample": sample,
                                    "is_binary": is_binary,
                                }),
                            };

                            let _ = event_tx.send(crate::adapter::DeviceEvent::Discovery {
                                device: discovered,
                            });

                            // Also publish directly to event bus if available
                            if let Some(bus) = event_bus {
                                bus.publish(NeoMindEvent::DeviceDiscovered {
                                    device_id: auto_device_id,
                                    source: "mqtt".to_string(),
                                    adapter_id: Some(adapter_id),
                                    metadata: serde_json::json!({
                                        "broker_id": broker_id,
                                        "original_topic": topic,
                                    }),
                                    sample,
                                    is_binary,
                                    timestamp: chrono::Utc::now().timestamp(),
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

/// Parse an MQTT `$SYS` client-presence topic into `(client_id, is_online)`.
///
/// Supports the EMQX / verneMQ / NanoMQ convention used by `create_and_connect_broker`:
///
/// ```text
/// $SYS/brokers/{node}/clients/{client_id}/connected      → Some((client_id, true))
/// $SYS/brokers/{node}/clients/{client_id}/disconnected   → Some((client_id, false))
/// ```
///
/// Returns `None` for any other shape — including aggregate `$SYS` topics
/// (`$SYS/broker/clients/connected`, `$SYS/brokers/+/metrics/...`, etc.) —
/// so they fall through to the early-return guard without firing a
/// synthesized transport event.
fn parse_sys_presence_topic(topic: &str) -> Option<(String, bool)> {
    let parts: Vec<&str> = topic.split('/').collect();
    if parts.len() != 6 {
        return None;
    }
    if parts[0] != "$SYS" || parts[1] != "brokers" || parts[3] != "clients" {
        return None;
    }
    let client_id = parts[4];
    if client_id.is_empty() {
        return None;
    }
    match parts[5] {
        "connected" => Some((client_id.to_string(), true)),
        "disconnected" => Some((client_id.to_string(), false)),
        _ => None,
    }
}

/// Helper function to extract device ID from topic.
fn normalized_initial_subscription_topics(configured_topics: &[String]) -> Vec<String> {
    let topics = [
        "device/+/+/uplink".to_string(),
        "device/+/+/downlink".to_string(),
    ]
    .into_iter()
    .chain(configured_topics.iter().cloned());

    normalize_subscription_topics(topics)
}

fn normalize_subscription_topics<I>(topics: I) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    let mut normalized: Vec<String> = Vec::new();

    for topic in topics {
        let topic = topic.trim();
        if topic.is_empty() {
            continue;
        }

        if normalized
            .iter()
            .any(|existing| topic_filter_covers(existing, topic))
        {
            continue;
        }

        normalized.retain(|existing| !topic_filter_covers(topic, existing));
        normalized.push(topic.to_string());
    }

    normalized
}

fn topic_filter_covers(covering: &str, covered: &str) -> bool {
    if covering == covered {
        return true;
    }

    // MQTT spec: a root-level wildcard filter ("#", "+/...") never matches a
    // topic whose first level starts with '$' (e.g. $SYS/...). Such topics
    // require an explicit "$..." filter. Without this guard, a user's "#"
    // device subscription would make normalize_subscription_topics treat the
    // broker's $SYS presence subscriptions as redundant and drop them —
    // silently breaking external-broker transport online/offline detection.
    let covering_first = covering.split('/').next().unwrap_or("");
    let covered_first = covered.split('/').next().unwrap_or("");
    if covered_first.starts_with('$') && matches!(covering_first, "#" | "+") {
        return false;
    }

    let covering_parts: Vec<&str> = covering.split('/').collect();
    let covered_parts: Vec<&str> = covered.split('/').collect();
    let mut i = 0usize;

    loop {
        match covering_parts.get(i).copied() {
            Some("#") => return i == covering_parts.len() - 1,
            Some("+") => match covered_parts.get(i).copied() {
                Some("#") | None => return false,
                Some(_) => i += 1,
            },
            Some(covering_part) => match covered_parts.get(i).copied() {
                Some("#") | Some("+") | None => return false,
                Some(covered_part) if covering_part == covered_part => i += 1,
                Some(_) => return false,
            },
            None => return i == covered_parts.len(),
        }
    }
}

/// Max auto-onboard device_id length (extracted from topic; hash fallback is shorter).
const MAX_AUTOONBOARD_DEVICE_ID_LEN: usize = 128;
/// Max auto-onboard sample payload size. Bounds memory against malicious/buggy
/// publishers (audit: sample was unbounded -> OOM). 2 MB lets camera image
/// payloads through (NeoMind has camera device types) while still capping the
/// worst case; MQTT brokers also cap packet size independently.
const MAX_AUTOONBOARD_SAMPLE_BYTES: usize = 2 * 1024 * 1024; // 2 MB

/// Sanitize an auto-onboard device_id extracted from an MQTT topic: keep only
/// device-id-safe chars (ascii alnum, `-`, `_`, `:`, `.`), cap length. Returns
/// None if empty/over-length so the caller falls back to a hash-derived id.
fn sanitize_auto_device_id(id: String) -> Option<String> {
    let cleaned: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'))
        .collect();
    if cleaned.is_empty() || cleaned.len() > MAX_AUTOONBOARD_DEVICE_ID_LEN {
        None
    } else {
        Some(cleaned)
    }
}

fn extract_device_id_from_topic(topic: &str, config: &MqttAdapterConfig) -> Option<String> {
    extract_device_id_from_topic_strong(topic, config)
        .or_else(|| extract_device_id_from_topic_weak(topic))
}

/// High-confidence topic extraction: `device/{type}/{id}/...` or a matching
/// subscription pattern (`+` at index 1 is the device id).
fn extract_device_id_from_topic_strong(topic: &str, config: &MqttAdapterConfig) -> Option<String> {
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

    None
}

/// Weak fallback: the second topic segment. Generic (a gateway forwarding
/// every device on `gateway/data` yields `"data"` for all), so only used
/// after the payload identity check.
fn extract_device_id_from_topic_weak(topic: &str) -> Option<String> {
    let parts: Vec<&str> = topic.split('/').collect();
    if parts.len() >= 2 {
        Some(parts[1].to_string())
    } else {
        None
    }
}

/// Payload-carried device identity. Used when the topic cannot uniquely
/// identify the device (gateway forwarding many devices on one topic).
/// ① explicit `config.device_id_field` wins (comma-separated list, tried in
/// order); ② otherwise auto-detect common fields (device_id / deviceId / sn /
/// mac / mac_address / ...).
/// Wrapper keys gateways commonly nest telemetry under. Identity lookup
/// descends ONE level into these when the field isn't at the top level
/// (e.g. `{"data": {"device_id": "x"}}`).
const ID_WRAPPER_KEYS: &[&str] = &["data", "payload", "state", "params", "body", "device"];

/// Resolve a field to a string: dotted paths walk (`data.sn`), plain names
/// check the top level first and then one level inside the wrapper keys.
fn lookup_id_field(
    root: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Option<String> {
    if field.contains('.') {
        // Dotted path: walk segments through nested objects; the leaf must
        // be a non-empty string. `Value::get` returns None on non-objects,
        // which is the walk's natural stop.
        let mut segs = field.split('.');
        let first = segs.next()?;
        let mut cur: &serde_json::Value = root.get(first)?;
        for seg in segs {
            cur = cur.get(seg)?;
        }
        return id_value_to_string(cur);
    }
    // Plain name: top level first…
    if let Some(v) = root.get(field).and_then(id_value_to_string) {
        return Some(v);
    }
    // …then one level inside common wrappers.
    for w in ID_WRAPPER_KEYS {
        if let Some(inner) = root.get(*w).and_then(serde_json::Value::as_object) {
            if let Some(v) = inner.get(field).and_then(id_value_to_string) {
                return Some(v);
            }
        }
    }
    None
}

/// Accept strings and integers as identity values — gateways do send
/// numeric SNs (`"sn": 42`); refusing them silently merged those devices.
fn id_value_to_string(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
        serde_json::Value::Number(n) if n.is_u64() || n.is_i64() => Some(n.to_string()),
        _ => None,
    }
}

fn extract_device_id_from_payload(payload: &[u8], config: &MqttAdapterConfig) -> Option<String> {
    let json: serde_json::Value = serde_json::from_slice(payload).ok()?;
    let obj = json.as_object()?;

    if let Some(fields) = config.device_id_field.as_deref() {
        // Candidate list, tried in order — gateway payloads vary per
        // firmware, so operators can chain fallbacks. Separators: newline
        // (the UI is one-field-per-line) and comma (API callers). Dotted
        // paths (`data.sn`) walk nested objects.
        for field in fields
            .split([',', '\n', '\r'])
            .map(str::trim)
            .filter(|f| !f.is_empty())
        {
            if let Some(v) = lookup_id_field(obj, field) {
                return Some(v);
            }
        }
    }

    // Auto-detect common device-identity fields, high confidence first.
    // Case-insensitive (device_id / deviceId / DeviceID all match).
    const CANDIDATES: &[&str] = &[
        "device_id",
        "deviceid",
        "dev_id",
        "devid",
        "device_sn",
        "devicesn",
        "sn",
        "serial",
        "serial_number",
        "serial_no",
        "mac",
        "mac_address",
        "macaddr",
        "eui",
        "deveui",
        "devaddr",
        "imei",
        "iccid",
        "node_id",
        "nodeid",
        "sensor_id",
        "sensorid",
        "device_uuid",
        "deviceuuid",
        "uuid",
        "device_name",
        "devicename",
        "dev_name",
    ];
    for key in CANDIDATES {
        if let Some(v) = obj.iter().find(|(k, _)| k.to_ascii_lowercase() == *key) {
            if let Some(s) = id_value_to_string(v.1) {
                return Some(s);
            }
        }
    }
    // Same candidates one level inside common wrappers — gateways often
    // nest identity with the telemetry (`{"data":{"device_id":"x"}}`).
    for w in ID_WRAPPER_KEYS {
        let Some(inner) = obj.get(*w).and_then(serde_json::Value::as_object) else {
            continue;
        };
        for key in CANDIDATES {
            if let Some(v) = inner.iter().find(|(k, _)| k.to_ascii_lowercase() == *key) {
                if let Some(s) = id_value_to_string(v.1) {
                    return Some(s);
                }
            }
        }
    }
    None
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

/// Detect topics that are NOT device telemetry and therefore should NOT
/// trigger auto-onboarding. Two classes:
///
/// 1. **LWT / status broadcasts**: many cameras and IoT devices publish
///    a Last-Will-and-Testament message on connect/disconnect to a
///    status topic such as `aicam/status/offline` or
///    `{prefix}/status/online`. These are not per-device telemetry —
///    treating them as devices creates phantom rows like
///    `device_id=status, is_binary=true`.
///
/// 2. **Downlink command echoes**: when the platform publishes to a
///    device's command topic (e.g. `ne302/2819FD/down/control`), the
///    embedded broker may reflect the publish back through a wildcard
///    subscription. The inbound handler must skip these so we don't
///    re-onboard a device we just sent a command to.
fn looks_like_non_telemetry_topic(topic: &str) -> bool {
    // Fast path: split once, reuse for all checks.
    let segments: Vec<&str> = topic.split('/').collect();

    // LWT-style: any segment is `status`, or topic ends with
    // `online`/`offline`/`connected`/`disconnected`. These are
    // near-universal LWT signatures across IoT firmware.
    if segments.contains(&"status") {
        return true;
    }
    if let Some(last) = segments.last() {
        matches!(
            *last,
            "online" | "offline" | "connected" | "disconnected" | "lwt" | "will"
        )
    } else {
        false
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

/// Result of an MQTT connection test.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MqttTestResult {
    pub success: bool,
    pub message: String,
}

/// Test MQTT connectivity by performing a real CONNECT/CONNACK handshake.
///
/// This creates a temporary MQTT client, connects to the broker, and waits
/// for CONNACK. Returns success if the broker accepts the connection.
pub async fn test_mqtt_connection(
    host: &str,
    port: u16,
    username: Option<&str>,
    password: Option<&str>,
    tls: bool,
    ca_cert: Option<&str>,
    client_cert: Option<&str>,
    client_key: Option<&str>,
) -> MqttTestResult {
    let client_id = format!("neomind-test-{}", uuid::Uuid::new_v4());
    let mut mqttoptions = rumqttc::MqttOptions::new(&client_id, host, port);
    mqttoptions.set_max_packet_size(1024, 1024);
    mqttoptions.set_keep_alive(Duration::from_secs(5));

    if let (Some(user), Some(pass)) = (username, password) {
        mqttoptions.set_credentials(user, pass);
    }

    if tls {
        match MqttAdapter::build_tls_transport(ca_cert, client_cert, client_key) {
            Ok(transport) => {
                mqttoptions.set_transport(transport);
            }
            Err(e) => {
                return MqttTestResult {
                    success: false,
                    message: format!("TLS configuration error: {}", e),
                };
            }
        }
    }

    let (_client, mut eventloop) = rumqttc::AsyncClient::new(mqttoptions, 5);

    // Poll for CONNACK with a 10-second timeout
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match eventloop.poll().await {
                Ok(rumqttc::Event::Incoming(rumqttc::Packet::ConnAck(ack))) => {
                    if ack.code == rumqttc::ConnectReturnCode::Success {
                        return Ok("MQTT CONNECT successful".to_string());
                    } else {
                        return Err(format!("CONNACK rejected: {:?}", ack.code));
                    }
                }
                Ok(_) => continue,
                Err(e) => return Err(format!("{}", e)),
            }
        }
    })
    .await;

    match result {
        Ok(Ok(msg)) => MqttTestResult {
            success: true,
            message: msg,
        },
        Ok(Err(msg)) => MqttTestResult {
            success: false,
            message: msg,
        },
        Err(_) => MqttTestResult {
            success: false,
            message: "Connection timeout after 10 seconds".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_auto_device_id() {
        // valid id (mixed safe chars) kept verbatim
        assert_eq!(
            sanitize_auto_device_id("sensor-01_room.a:b".to_string()).as_deref(),
            Some("sensor-01_room.a:b")
        );
        // unsafe chars (path separators) stripped; '.' kept
        assert_eq!(
            sanitize_auto_device_id("dev/../etc".to_string()).as_deref(),
            Some("dev..etc")
        );
        // only-unsafe chars (no alnum/_-:.) -> None (caller falls back to hash)
        assert_eq!(sanitize_auto_device_id("///".to_string()), None);
        // empty -> None
        assert_eq!(sanitize_auto_device_id(String::new()), None);
        // over-length -> None
        let long = "a".repeat(MAX_AUTOONBOARD_DEVICE_ID_LEN + 1);
        assert_eq!(sanitize_auto_device_id(long), None);
        // exactly at limit -> kept
        let at = "a".repeat(MAX_AUTOONBOARD_DEVICE_ID_LEN);
        assert!(sanitize_auto_device_id(at).is_some());
    }

    #[test]
    fn test_extract_device_id_from_payload_auto_detect() {
        let cfg = MqttAdapterConfig::new("test", "localhost:1883");
        assert_eq!(
            extract_device_id_from_payload(b"{\"device_id\":\"sensor-1\",\"data\":{}}", &cfg)
                .as_deref(),
            Some("sensor-1")
        );
        // case-insensitive deviceId
        assert_eq!(
            extract_device_id_from_payload(b"{\"deviceId\":\"s2\",\"data\":{}}", &cfg).as_deref(),
            Some("s2")
        );
        // mac
        assert_eq!(
            extract_device_id_from_payload(b"{\"mac\":\"aa:bb:cc\",\"data\":{}}", &cfg).as_deref(),
            Some("aa:bb:cc")
        );
        // no identity field -> None (caller falls back to topic/hash)
        assert_eq!(
            extract_device_id_from_payload(b"{\"temp\":21.5}", &cfg),
            None
        );
    }

    #[test]
    fn test_extract_device_id_from_payload_explicit_field() {
        let cfg = MqttAdapterConfig {
            device_id_field: Some("DEV".to_string()),
            ..MqttAdapterConfig::new("test", "localhost:1883")
        };
        assert_eq!(
            extract_device_id_from_payload(b"{\"DEV\":\"gw-42\",\"data\":{}}", &cfg).as_deref(),
            Some("gw-42")
        );
        // explicit field beats auto-detect
        assert_eq!(
            extract_device_id_from_payload(
                b"{\"DEV\":\"gw-42\",\"device_id\":\"other\",\"data\":{}}",
                &cfg
            )
            .as_deref(),
            Some("gw-42")
        );
    }

    #[test]
    fn test_extract_device_id_nested_wrapper() {
        let cfg = MqttAdapterConfig::new("test", "localhost:1883");
        // auto-detect descends into common wrappers
        assert_eq!(
            extract_device_id_from_payload(
                b"{\"data\":{\"device_id\":\"nested-1\",\"temperature\":21}}",
                &cfg
            )
            .as_deref(),
            Some("nested-1")
        );
        // explicit plain field also descends
        let cfg2 = MqttAdapterConfig {
            device_id_field: Some("sn".to_string()),
            ..MqttAdapterConfig::new("test", "localhost:1883")
        };
        assert_eq!(
            extract_device_id_from_payload(b"{\"payload\":{\"sn\":\"SN-9\"}}", &cfg2).as_deref(),
            Some("SN-9")
        );
        // dotted path walks explicitly
        let cfg3 = MqttAdapterConfig {
            device_id_field: Some("state.meta.mac".to_string()),
            ..MqttAdapterConfig::new("test", "localhost:1883")
        };
        assert_eq!(
            extract_device_id_from_payload(b"{\"state\":{\"meta\":{\"mac\":\"aa:bb\"}}}", &cfg3)
                .as_deref(),
            Some("aa:bb")
        );
        // top level wins over nested
        let cfg4 = MqttAdapterConfig {
            device_id_field: Some("sn".to_string()),
            ..MqttAdapterConfig::new("test", "localhost:1883")
        };
        assert_eq!(
            extract_device_id_from_payload(b"{\"sn\":\"TOP\",\"data\":{\"sn\":\"NESTED\"}}", &cfg4)
                .as_deref(),
            Some("TOP")
        );
        // numeric ids are accepted (gateways send numeric SNs)
        assert_eq!(
            extract_device_id_from_payload(b"{\"data\":{\"device_id\":42}}", &cfg).as_deref(),
            Some("42")
        );
        assert_eq!(
            extract_device_id_from_payload(b"{\"sn\":777}", &cfg).as_deref(),
            Some("777")
        );
        // non-id leaves (bool/object) still -> None
        assert_eq!(
            extract_device_id_from_payload(b"{\"data\":{\"device_id\":true}}", &cfg).as_deref(),
            None
        );
    }

    #[test]
    fn test_extract_device_id_comma_separated_fallback() {
        // Comma-separated candidate list: tried in order, first hit wins.
        let cfg = MqttAdapterConfig {
            device_id_field: Some("sn, dev_id, mac".to_string()),
            ..MqttAdapterConfig::new("test", "localhost:1883")
        };
        // first candidate present
        assert_eq!(
            extract_device_id_from_payload(b"{\"sn\":\"SN-1\",\"mac\":\"m1\"}", &cfg).as_deref(),
            Some("SN-1")
        );
        // first absent -> second candidate
        assert_eq!(
            extract_device_id_from_payload(b"{\"dev_id\":\"D-2\",\"mac\":\"m2\"}", &cfg).as_deref(),
            Some("D-2")
        );
        // all explicit candidates absent -> falls through to auto-detect
        // (device_id candidate)
        assert_eq!(
            extract_device_id_from_payload(b"{\"device_id\":\"auto-3\"}", &cfg).as_deref(),
            Some("auto-3")
        );
        // spaces around commas are trimmed
        let cfg_spaced = MqttAdapterConfig {
            device_id_field: Some("  sn ,  mac  ".to_string()),
            ..MqttAdapterConfig::new("test", "localhost:1883")
        };
        assert_eq!(
            extract_device_id_from_payload(b"{\"mac\":\"mm\"}", &cfg_spaced).as_deref(),
            Some("mm")
        );
        // newline-separated (the one-field-per-line UI form) — same behavior
        let cfg_lines = MqttAdapterConfig {
            device_id_field: Some("sn\n dev_id\nmac".to_string()),
            ..MqttAdapterConfig::new("test", "localhost:1883")
        };
        assert_eq!(
            extract_device_id_from_payload(b"{\"dev_id\":\"L-9\"}", &cfg_lines).as_deref(),
            Some("L-9")
        );
    }

    #[test]
    fn test_same_topic_different_payload_ids_distinct() {
        // Gateway case: two devices forwarded on one topic must get distinct
        // ids. The topic (`gateway/data`) yields a weak parts[1] = "data" for
        // both, so the payload `device_id` must win.
        let cfg = MqttAdapterConfig::new("test", "localhost:1883");
        let topic = "gateway/data";
        let derive = |payload: &[u8]| {
            extract_device_id_from_topic_strong(topic, &cfg)
                .and_then(sanitize_auto_device_id)
                .or_else(|| extract_device_id_from_payload(payload, &cfg))
                .and_then(sanitize_auto_device_id)
                .or_else(|| extract_device_id_from_topic_weak(topic))
        };
        let id1 = derive(b"{\"device_id\":\"sensor-1\",\"temp\":20}");
        let id2 = derive(b"{\"device_id\":\"sensor-2\",\"temp\":21}");
        assert_eq!(id1.as_deref(), Some("sensor-1"));
        assert_eq!(id2.as_deref(), Some("sensor-2"));
        assert_ne!(id1, id2);
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

    #[test]
    fn test_normalized_initial_topics_removes_topics_covered_by_hash() {
        let topics = normalized_initial_subscription_topics(&["#".to_string()]);
        assert_eq!(topics, vec!["#"]);
    }

    #[test]
    fn test_normalized_initial_topics_removes_topics_covered_by_device_hash() {
        let topics = normalized_initial_subscription_topics(&["device/#".to_string()]);
        assert_eq!(topics, vec!["device/#"]);
    }

    #[test]
    fn test_normalized_initial_topics_keeps_uncovered_custom_topics() {
        let topics = normalized_initial_subscription_topics(&["sensors/+/temperature".to_string()]);
        assert_eq!(
            topics,
            vec![
                "device/+/+/uplink".to_string(),
                "device/+/+/downlink".to_string(),
                "sensors/+/temperature".to_string(),
            ]
        );
    }

    #[test]
    fn test_topic_filter_covers_standard_cases() {
        assert!(topic_filter_covers("#", "device/+/+/uplink"));
        assert!(topic_filter_covers("device/#", "device/+/+/uplink"));
        assert!(topic_filter_covers(
            "device/+/+/uplink",
            "device/+/+/uplink"
        ));
        assert!(!topic_filter_covers("device/+/+/uplink", "device/#"));
        assert!(topic_filter_covers(
            "device/+/+/uplink",
            "device/abc/001/uplink"
        ));
        assert!(!topic_filter_covers(
            "device/abc/001/uplink",
            "device/+/+/uplink"
        ));
    }

    /// MQTT spec: root-level wildcards ("#", "+") must NOT cover `$`-prefixed
    /// topics (e.g. `$SYS/...`). Otherwise a user's "#" device subscription
    /// would dedup away the broker `$SYS` presence subscriptions and silently
    /// break external-broker transport online/offline detection.
    #[test]
    fn test_topic_filter_covers_wildcards_skip_dollar_topics() {
        // Root "#" must not cover a $SYS topic
        assert!(!topic_filter_covers(
            "#",
            "$SYS/brokers/emqx@host/clients/sensor-001/connected"
        ));
        // Root "+/..." must not cover a $SYS topic either
        assert!(!topic_filter_covers("+/brokers", "$SYS/brokers"));
        // But an explicit "$SYS/..." filter still covers $SYS topics normally
        assert!(topic_filter_covers(
            "$SYS/brokers/#",
            "$SYS/brokers/emqx@host/clients/sensor-001/connected"
        ));
        assert!(topic_filter_covers(
            "$SYS/brokers/+/clients/+/connected",
            "$SYS/brokers/emqx@host/clients/sensor-001/connected"
        ));
        // Non-$ topics are unaffected: "#" still covers device topics
        assert!(topic_filter_covers("#", "device/abc/001/uplink"));
    }

    /// `$SYS` presence topics are the ONLY `$SYS` shape we synthesize
    /// transport events from. The parser must:
    /// - accept EMQX-style `$SYS/brokers/{node}/clients/{cid}/connected|disconnected`
    /// - reject aggregate / metrics / malformed `$SYS` topics
    /// - reject empty client_ids (defensive — never observed in the wild)
    #[test]
    fn test_parse_sys_presence_topic_emqx_style() {
        // connected → online=true
        let (cid, online) =
            parse_sys_presence_topic("$SYS/brokers/emqx@10.0.0.1/clients/sensor-001/connected")
                .expect("EMQX connected topic must parse");
        assert_eq!(cid, "sensor-001");
        assert!(online);

        // disconnected → online=false
        let (cid, online) =
            parse_sys_presence_topic("$SYS/brokers/emqx@10.0.0.1/clients/sensor-001/disconnected")
                .expect("EMQX disconnected topic must parse");
        assert_eq!(cid, "sensor-001");
        assert!(!online);
    }

    #[test]
    fn test_parse_sys_presence_topic_rejects_non_presence_sys() {
        // Aggregate stats topic (Mosquitto-style) — not per-client
        assert!(parse_sys_presence_topic("$SYS/broker/clients/connected").is_none());
        // Metrics topic
        assert!(parse_sys_presence_topic("$SYS/brokers/emqx@node/metrics/bytes.sent").is_none());
        // Unknown suffix
        assert!(parse_sys_presence_topic("$SYS/brokers/emqx@node/clients/cid/kicked").is_none());
    }

    #[test]
    fn test_parse_sys_presence_topic_rejects_malformed() {
        // Too few segments
        assert!(parse_sys_presence_topic("$SYS/brokers").is_none());
        // Wrong root prefix
        assert!(parse_sys_presence_topic("devices/brokers/n/clients/c/connected").is_none());
        // Missing `clients` segment
        assert!(parse_sys_presence_topic("$SYS/brokers/n/sessions/c/connected").is_none());
    }

    #[test]
    fn test_parse_sys_presence_topic_handles_internal_client_id() {
        // The parser returns the id verbatim; the caller is responsible for
        // filtering `neomind-` prefixed ids (this mirrors the embedded-broker
        // `is_internal_client` convention).
        let (cid, _) =
            parse_sys_presence_topic("$SYS/brokers/n/clients/neomind-external-b1/connected")
                .expect("Internal client id still parses; caller filters");
        assert_eq!(cid, "neomind-external-b1");
    }

    /// Regression: LWT/status broadcast topics must NOT trigger
    /// auto-onboarding. Real-world example from NE301 field deployment:
    /// the device publishes `aicam/status/offline` as its MQTT
    /// Last-Will-Testament; without filtering this produced a phantom
    /// "discovered device" row with `device_id=status, is_binary=true`
    /// on every disconnect. Other patterns include `{prefix}/status/online`
    /// (connect) and bare `/lwt` topics.
    #[test]
    fn test_lwt_and_status_topics_skip_auto_onboarding() {
        // Status-broadcast topics
        assert!(looks_like_non_telemetry_topic("aicam/status/offline"));
        assert!(looks_like_non_telemetry_topic("aicam/status/online"));
        assert!(looks_like_non_telemetry_topic(
            "homeassistant/status/online"
        ));
        assert!(looks_like_non_telemetry_topic("devices/status/connected"));

        // Bare LWT signatures
        assert!(looks_like_non_telemetry_topic("aicam/offline"));
        assert!(looks_like_non_telemetry_topic("dev/abc/lwt"));
        assert!(looks_like_non_telemetry_topic("dev/abc/will"));

        // Real telemetry MUST pass through
        assert!(!looks_like_non_telemetry_topic(
            "ne301/2A0015/upload/report"
        ));
        assert!(!looks_like_non_telemetry_topic(
            "device/ne301_camera/2819FD/uplink"
        ));
        assert!(!looks_like_non_telemetry_topic(
            "sensors/temp-001/temperature"
        ));
        assert!(!looks_like_non_telemetry_topic("stat/deviceid/power"));
    }

    /// Regression: the `looks_like_non_telemetry_topic` filter must NOT
    /// cause telemetry loss for a registered device whose topic happens to
    /// contain a `status` segment or end with `online`/`offline` (common in
    /// real IoT firmware). The filter is only applied to UNKNOWN topics in
    /// the auto-onboarding path — registered-device telemetry is processed
    /// before the filter runs.
    ///
    /// This test documents the contract: the helper itself is aggressive
    /// (returns true for `device/abc/status`), so the CALLING CODE must
    /// ensure registered devices are routed before the filter. If this
    /// test ever breaks because someone moved the filter before the
    /// topic_to_device lookup, the bug is back.
    #[test]
    fn test_status_topic_filter_is_aggressive_by_design() {
        // These DO match the filter — that's intentional for the
        // auto-onboarding path. The fix is structural (filter runs AFTER
        // registered-device lookup), not in this helper.
        assert!(looks_like_non_telemetry_topic("device/abc/status"));
        assert!(looks_like_non_telemetry_topic("device/abc/online"));
    }
}
