//! `connection` — split from the former mqtt.rs monolith.

use super::config::MqttAdapterConfig;
use super::*;

use crate::adapter::{AdapterError, AdapterResult, ConnectionStatus};

use crate::mdl::MetricValue;

use crate::protocol::ProtocolMapping;

use crate::registry::DeviceRegistry;

use crate::telemetry::TimeSeriesStorage;

use crate::unified_extractor::UnifiedExtractor;

use futures::FutureExt;

use neomind_core::EventBus;

use serde_json::Value;

use std::collections::HashMap;

use std::path::PathBuf;

use std::sync::atomic::AtomicBool;

use std::sync::Arc;

use std::time::Duration;

use tokio::sync::{broadcast, Mutex, RwLock};

use tracing::{debug, info, warn};

use uuid::Uuid;

use rumqttc::{TlsConfiguration, Transport};

use rustls_pki_types::{CertificateDer, PrivateKeyDer};

use std::io::Cursor;

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
            outbound_echoes: Arc::new(RwLock::new(OutboundEchoBuffer::new())),
            uncollectable: Arc::new(RwLock::new(std::collections::HashSet::new())),
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
            outbound_echoes: Arc::new(RwLock::new(OutboundEchoBuffer::new())),
            uncollectable: Arc::new(RwLock::new(std::collections::HashSet::new())),
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
    /// Server-restart path: uses the same request-register + SubAck path as
    /// dynamic subscribe. Cover reuse reads confirmed only — a rejected startup
    /// `#` must not skip device topics.
    pub(crate) async fn subscribe_device_telemetry_topics(&self, broker_id: &str) {
        let registry = self.device_registry.read().await;
        let devices = registry.list_devices();
        drop(registry);

        let handles = {
            let clients = self.mqtt_clients.read().await;
            let Some(inner) = clients.get(broker_id) else {
                return;
            };
            BrokerSubscribeHandles::from_inner(inner)
        };

        let mut subscribed_count = 0u32;
        for device in &devices {
            if let Some(ref telemetry_topic) = device.connection_config.telemetry_topic {
                debug!(
                    "Re-subscribing to device telemetry topic '{}' for broker '{}' (device '{}')",
                    telemetry_topic, broker_id, device.device_id
                );
                // Enqueue-only during broker add (don't block on SubAck before ConnAck).
                match Self::request_subscribe_on_broker(&handles, broker_id, telemetry_topic, false)
                    .await
                {
                    Ok(()) => subscribed_count += 1,
                    Err(e) => {
                        warn!(
                            "Failed to re-subscribe to telemetry topic '{}' on broker {}: {}",
                            telemetry_topic, broker_id, e
                        );
                    }
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

    /// Out-of-poll reconnect worker: restore `desired_topics` via the unified
    /// subscribe path after ConnAck. Never called from the poll task itself.
    pub(crate) async fn restore_desired_topics_after_reconnect(
        mqtt_clients: &Arc<RwLock<HashMap<String, MqttClientInner>>>,
        broker_id: &str,
    ) {
        let (handles, topics): (BrokerSubscribeHandles, Vec<String>) = {
            let clients = mqtt_clients.read().await;
            let Some(inner) = clients.get(broker_id) else {
                return;
            };
            let topics = inner.desired_topics.read().await.iter().cloned().collect();
            (BrokerSubscribeHandles::from_inner(inner), topics)
        };

        if topics.is_empty() {
            debug!(
                "No desired topics to restore after reconnect on broker '{}'",
                broker_id
            );
            return;
        }

        let total = topics.len() as u32;
        let mut ok = 0u32;
        for topic in &topics {
            // Out-of-poll worker may wait for SubAck.
            match Self::request_subscribe_on_broker(&handles, broker_id, topic, true).await {
                Ok(()) => ok += 1,
                Err(e) => {
                    warn!(
                        "Reconnect restore failed for '{}' on broker '{}': {}",
                        topic, broker_id, e
                    );
                }
            }
        }

        info!(
            "Restored {}/{} desired topics after reconnect on broker '{}'",
            ok, total, broker_id
        );
    }

    /// Build broker client state + spawn the out-of-poll reconnect worker.
    pub(crate) fn new_broker_inner(
        broker_id: String,
        broker_addr: String,
        client: rumqttc::AsyncClient,
        running: Arc<RwLock<bool>>,
        mqtt_clients: Arc<RwLock<HashMap<String, MqttClientInner>>>,
    ) -> MqttClientInner {
        let (reconnect_tx, mut reconnect_rx) = tokio::sync::mpsc::channel::<()>(8);
        let broker_id_worker = broker_id.clone();
        let running_worker = running.clone();
        tokio::spawn(async move {
            while *running_worker.read().await {
                match reconnect_rx.recv().await {
                    Some(()) => {
                        Self::restore_desired_topics_after_reconnect(
                            &mqtt_clients,
                            &broker_id_worker,
                        )
                        .await;
                    }
                    None => break,
                }
            }
        });
        MqttClientInner {
            _broker_id: broker_id,
            _broker_addr: broker_addr,
            client,
            running,
            subscribed_topics: Arc::new(RwLock::new(std::collections::HashSet::new())),
            desired_topics: Arc::new(RwLock::new(std::collections::HashSet::new())),
            pending_subscribes: Arc::new(Mutex::new(PendingSubscribeState::default())),
            subscribe_send: Arc::new(Mutex::new(())),
            reconnect_tx,
            await_suback: true,
            suback_timeout: DEFAULT_SUBACK_TIMEOUT,
            session_ready: AtomicBool::new(false),
        }
    }

    /// On disconnect: fail waiters, clear session confirmed, keep restore intent.
    pub(crate) async fn on_broker_disconnect(inner: &MqttClientInner, reason: &str) {
        Self::fail_all_pending_subscribes(inner, reason).await;
        inner.subscribed_topics.write().await.clear();
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
        // Store the client BEFORE spawning the event loop. Client insert ≠ ConnAck:
        // session_ready stays false until ConnAck Success (subscribe may only queue).
        // Reconnect restore runs on a dedicated worker (never inside poll).
        let inner = Self::new_broker_inner(
            broker_id.clone(),
            broker_addr.clone(),
            client,
            running.clone(),
            self.mqtt_clients.clone(),
        );
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
            topic_map_insert(&mut topic_mapping, &topic, &device.device_id);
            // CT-Prov bare alias for NeoMind uplink topics.
            if let Some(bare) = topic
                .strip_suffix("/uplink")
                .filter(|s| s.starts_with("device/"))
            {
                topic_map_insert(&mut topic_mapping, bare, &device.device_id);
            }
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
        let outbound_echoes = self.outbound_echoes.clone();
        let uncollectable = self.uncollectable.clone();
        let data_dir_clone = self.data_dir.clone();

        info!(
            "Starting event loop task for broker '{}', connecting to {}...",
            broker_id, broker_addr
        );

        // Spawn a periodic metric_cache sweep to prevent unbounded growth from
        // phantom auto-onboarded devices. Drops entries older than 30 min.
        let sweep_cache = Arc::clone(&self.metric_cache);
        let sweep_running = running.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
            loop {
                interval.tick().await;
                if !*sweep_running.read().await {
                    break;
                }
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
                        Self::apply_session_ready_from_notification(
                            &mqtt_clients,
                            &connection_status,
                            &broker_id_clone,
                            &notification,
                        )
                        .await;
                        // SubAck correlation lives in the poll loop (complete
                        // waiters only). Confirm/subscribe waits OUTSIDE.
                        Self::apply_subscription_events(
                            &mqtt_clients,
                            &broker_id_clone,
                            &notification,
                        )
                        .await;
                        if was_disconnected {
                            was_disconnected = false;
                            // Notify out-of-poll worker — never subscribe here.
                            if let Some(inner) = mqtt_clients.read().await.get(&broker_id_clone) {
                                let _ = inner.reconnect_tx.try_send(());
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
                                &outbound_echoes,
                                &uncollectable,
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
                        // Client remains in map while reconnecting — clear session
                        // so subscribe_topic cannot claim ready from a queued sub.
                        Self::set_broker_session_ready(
                            &mqtt_clients,
                            &connection_status,
                            &broker_id_clone,
                            false,
                        )
                        .await;
                        if let Some(inner) = mqtt_clients.read().await.get(&broker_id_clone) {
                            Self::on_broker_disconnect(
                                inner,
                                &format!("MQTT broker disconnected: {e}"),
                            )
                            .await;
                        }
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
            if let Some(inner) = mqtt_clients.write().await.remove(&broker_id_clone) {
                Self::on_broker_disconnect(&inner, "MQTT broker connection closed").await;
            }
            info!("MQTT broker {} connection closed", broker_id_clone);
        });

        // Bug 4: subscribe AFTER the event loop task is spawned and polling.
        // Unified path: register pending + wait SubAck; only success → confirmed.
        // Rejected startup wildcards must NOT enter confirmed (no false cover).
        //
        // NOTE: add_broker() is used by the INTERNAL embedded broker via start(). The
        // internal broker configures subscribe_topics = ["#"] (subscribe to all topics
        // for auto-discovery), so we MUST include self.config.subscribe_topics here —
        // otherwise the internal broker only sees device/+/+/uplink|downlink messages
        // and devices publishing to custom topics (e.g. "ne101/abc") are never seen.
        // add_broker_with_tls (external brokers) takes subscribe_topics as an explicit
        // parameter and is unaffected.
        let initial_topics = normalized_initial_subscription_topics(&self.config.subscribe_topics);
        let handles = {
            let clients = self.mqtt_clients.read().await;
            let inner = clients.get(&broker_id).expect("broker just inserted");
            BrokerSubscribeHandles::from_inner(inner)
        };

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
            // Enqueue-only: SubAck alone confirms; rejected wildcards never cover.
            match Self::request_subscribe_on_broker(&handles, &broker_id, topic, false).await {
                Ok(()) => {
                    success_count += 1;
                    info!(
                        "Subscribe enqueued for topic '{}' on broker '{}' (awaiting SubAck)",
                        topic, broker_id
                    );
                }
                Err(e) => {
                    warn!(
                        "Failed to subscribe to {} on broker {}: {}",
                        topic, broker_id, e
                    );
                }
            }
        }

        // Bug 3: re-subscribe telemetry topics of registered devices (server-restart)
        self.subscribe_device_telemetry_topics(&broker_id).await;

        info!(
            "Broker '{}' desired={:?} confirmed={:?}",
            broker_id,
            handles.desired_topics.read().await,
            handles.subscribed_topics.read().await
        );

        // Bug 5: if every enqueue failed, fail the broker add entirely so the
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
        // Store the client BEFORE spawning the event loop. Client insert ≠ ConnAck.
        // Reconnect restore runs on a dedicated worker (never inside poll).
        let inner = Self::new_broker_inner(
            broker_id.clone(),
            broker_addr.clone(),
            client,
            running.clone(),
            self.mqtt_clients.clone(),
        );
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
            topic_map_insert(&mut topic_mapping, &topic, &device.device_id);
            if let Some(bare) = topic
                .strip_suffix("/uplink")
                .filter(|s| s.starts_with("device/"))
            {
                topic_map_insert(&mut topic_mapping, bare, &device.device_id);
            }
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
        let connection_status_poll = connection_status.clone();
        let mqtt_clients = self.mqtt_clients.clone();
        let broker_id_clone = broker_id.clone();
        let broker_id_clone2 = broker_id.clone();
        let extractor = self.extractor.clone();
        let topic_to_device = self.topic_to_device.clone();
        let outbound_echoes = self.outbound_echoes.clone();
        let uncollectable = self.uncollectable.clone();
        let data_dir_clone = self.data_dir.clone();

        let (eventloop_tx, eventloop_rx) = async_channel::bounded(2048);
        let event_tx_clone = event_tx.clone();

        tokio::spawn(async move {
            while *running_flag.read().await {
                match eventloop_rx.recv().await {
                    Ok(notification) => {
                        if let Err(panic) = std::panic::AssertUnwindSafe(
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
                                &outbound_echoes,
                                &uncollectable,
                                data_dir_clone.read().await.as_ref(),
                            ),
                        )
                        .catch_unwind()
                        .await
                        {
                            tracing::error!(
                                broker = %broker_id_clone,
                                panic = ?panic,
                                "MQTT notification handler panicked — consumer continues"
                            );
                        }
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
                        Self::apply_session_ready_from_notification(
                            &mqtt_clients,
                            &connection_status_poll,
                            &broker_id_clone2,
                            &notification,
                        )
                        .await;
                        Self::apply_subscription_events(
                            &mqtt_clients,
                            &broker_id_clone2,
                            &notification,
                        )
                        .await;
                        if was_disconnected {
                            was_disconnected = false;
                            // Notify out-of-poll worker — never subscribe here.
                            if let Some(inner) = mqtt_clients.read().await.get(&broker_id_clone2) {
                                let _ = inner.reconnect_tx.try_send(());
                            }
                        }
                        if let Err(e) = eventloop_tx.send(notification).await {
                            warn!("Failed to send MQTT notification to channel: {}", e);
                        }
                    }
                    Err(e) => {
                        error_count += 1;
                        was_disconnected = true;
                        Self::set_broker_session_ready(
                            &mqtt_clients,
                            &connection_status_poll,
                            &broker_id_clone2,
                            false,
                        )
                        .await;
                        if let Some(inner) = mqtt_clients.read().await.get(&broker_id_clone2) {
                            Self::on_broker_disconnect(
                                inner,
                                &format!("MQTT broker disconnected: {e}"),
                            )
                            .await;
                        }
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

            if let Some(inner) = mqtt_clients.write().await.remove(&broker_id_clone2) {
                Self::on_broker_disconnect(&inner, "MQTT broker connection closed").await;
            }
            info!("MQTT broker {} connection closed", broker_id_clone2);
        });

        // Bug 4: subscribe AFTER the event loop poll task is spawned. Bug 1: only the
        // explicit `subscribe_topics` function parameter is used — the duplicate
        // self.config.subscribe_topics loop was removed (the API handler already sets
        // config.subscribe_topics from the same broker data, so adding it twice
        // caused rumqttc to receive duplicate SUBSCRIBE requests).
        // Unified path: only successful SubAck → confirmed.
        let initial_topics = normalized_initial_subscription_topics(&subscribe_topics);
        let handles = {
            let clients = self.mqtt_clients.read().await;
            let inner = clients.get(&broker_id).expect("broker just inserted");
            BrokerSubscribeHandles::from_inner(inner)
        };

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
            match Self::request_subscribe_on_broker(&handles, &broker_id, topic, false).await {
                Ok(()) => {
                    success_count += 1;
                    info!(
                        "Subscribe enqueued for topic '{}' on broker '{}' (awaiting SubAck)",
                        topic, broker_id
                    );
                }
                Err(e) => {
                    warn!(
                        "Failed to subscribe to {} on broker {}: {}",
                        topic, broker_id, e
                    );
                }
            }
        }

        // Bug 3: re-subscribe telemetry topics of registered devices (server-restart)
        self.subscribe_device_telemetry_topics(&broker_id).await;

        info!(
            "Broker '{}' desired={:?} confirmed={:?}",
            broker_id,
            handles.desired_topics.read().await,
            handles.subscribed_topics.read().await
        );

        // Bug 5: if every enqueue failed, fail the broker add entirely.
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
    pub(crate) fn resolve_pem(input: &str) -> Result<String, Box<dyn std::error::Error>> {
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
    pub(crate) fn load_certs(
        pem: &str,
    ) -> Result<Vec<CertificateDer<'static>>, Box<dyn std::error::Error>> {
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
    pub(crate) fn load_private_key(
        pem: &str,
    ) -> Result<PrivateKeyDer<'static>, Box<dyn std::error::Error>> {
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

    /// Mark a broker's MQTT session ready (ConnAck Success) or not (disconnect /
    /// reconnect window). Drives overall `connection_status`.
    pub(crate) async fn set_broker_session_ready(
        mqtt_clients: &Arc<RwLock<HashMap<String, MqttClientInner>>>,
        connection_status: &Arc<RwLock<ConnectionStatus>>,
        broker_id: &str,
        ready: bool,
    ) {
        {
            let clients = mqtt_clients.read().await;
            if let Some(inner) = clients.get(broker_id) {
                inner.set_session_ready(ready);
            }
        }
        let any_ready = mqtt_clients
            .read()
            .await
            .values()
            .any(MqttClientInner::is_session_ready);
        *connection_status.write().await = if any_ready {
            ConnectionStatus::Connected
        } else {
            ConnectionStatus::Disconnected
        };
    }

    /// Apply session_ready from a polled event (ConnAck only).
    pub(crate) async fn apply_session_ready_from_notification(
        mqtt_clients: &Arc<RwLock<HashMap<String, MqttClientInner>>>,
        connection_status: &Arc<RwLock<ConnectionStatus>>,
        broker_id: &str,
        notification: &rumqttc::Event,
    ) {
        if let rumqttc::Event::Incoming(rumqttc::Packet::ConnAck(ack)) = notification {
            let ready = ack.code == rumqttc::ConnectReturnCode::Success;
            Self::set_broker_session_ready(mqtt_clients, connection_status, broker_id, ready).await;
        }
    }

    /// Correlate Outgoing Subscribe pkids and Incoming SubAck **in the poll
    /// loop**. Completes oneshot waiters only — never blocks waiting for SubAck
    /// inside the MQTT notification/message handler.
    pub(crate) async fn apply_subscription_events(
        mqtt_clients: &Arc<RwLock<HashMap<String, MqttClientInner>>>,
        broker_id: &str,
        notification: &rumqttc::Event,
    ) {
        match notification {
            rumqttc::Event::Outgoing(rumqttc::Outgoing::Subscribe(pkid)) => {
                let clients = mqtt_clients.read().await;
                let Some(inner) = clients.get(broker_id) else {
                    return;
                };
                let mut pending = inner.pending_subscribes.lock().await;
                // Prefer Absorb(pkid=None) so a timed-out send cannot assign its
                // Outgoing pkid onto a later Active request.
                if let Some(entry) = pending.queue.iter_mut().find(|e| match e {
                    PendingQueueEntry::Absorb { pkid: None, .. } => true,
                    PendingQueueEntry::Active(s) => s.pkid.is_none(),
                    _ => false,
                }) {
                    match entry {
                        PendingQueueEntry::Absorb {
                            topic,
                            pkid: slot_pkid,
                        } => {
                            *slot_pkid = Some(*pkid);
                            debug!(
                                broker = %broker_id,
                                pkid,
                                topic = %topic,
                                "MQTT Subscribe outgoing absorbed by cancelled request"
                            );
                        }
                        PendingQueueEntry::Active(slot) => {
                            slot.pkid = Some(*pkid);
                            debug!(
                                broker = %broker_id,
                                pkid,
                                topic = %slot.topic,
                                "MQTT Subscribe outgoing — awaiting SubAck"
                            );
                        }
                    }
                } else {
                    warn!(
                        broker = %broker_id,
                        pkid,
                        "Outgoing Subscribe with no unmatched pending topic"
                    );
                }
            }
            rumqttc::Event::Incoming(rumqttc::Packet::SubAck(suback)) => {
                let clients = mqtt_clients.read().await;
                let Some(inner) = clients.get(broker_id) else {
                    return;
                };
                let entry = {
                    let mut pending = inner.pending_subscribes.lock().await;
                    let idx = pending.queue.iter().position(|e| match e {
                        PendingQueueEntry::Active(s) => s.pkid == Some(suback.pkid),
                        PendingQueueEntry::Absorb { pkid: Some(p), .. } => *p == suback.pkid,
                        PendingQueueEntry::Absorb { pkid: None, .. } => false,
                    });
                    idx.and_then(|i| pending.queue.remove(i))
                };
                let Some(entry) = entry else {
                    debug!(
                        broker = %broker_id,
                        pkid = suback.pkid,
                        "SubAck for unknown/expired pending Subscribe — ignored"
                    );
                    return;
                };
                match entry {
                    PendingQueueEntry::Absorb { topic, .. } => {
                        debug!(
                            broker = %broker_id,
                            pkid = suback.pkid,
                            topic = %topic,
                            "Late SubAck for cancelled Subscribe — absorbed"
                        );
                    }
                    PendingQueueEntry::Active(slot) => {
                        let rejected = suback
                            .return_codes
                            .iter()
                            .any(|c| matches!(c, rumqttc::SubscribeReasonCode::Failure));
                        if rejected {
                            let msg = format!(
                                "Broker rejected Subscribe for '{}' (pkid={}, codes={:?})",
                                slot.topic, suback.pkid, suback.return_codes
                            );
                            warn!(broker = %broker_id, %msg);
                            Self::complete_pending_subscribe_failure(slot, msg).await;
                        } else {
                            info!(
                                broker = %broker_id,
                                topic = %slot.topic,
                                pkid = suback.pkid,
                                "MQTT SubAck success — subscription confirmed"
                            );
                            Self::complete_pending_subscribe_success(inner, slot).await;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Fail every pending Subscribe waiter (disconnect / poll error).
    pub(crate) async fn fail_all_pending_subscribes(inner: &MqttClientInner, reason: &str) {
        let mut pending = inner.pending_subscribes.lock().await;
        while let Some(entry) = pending.queue.pop_front() {
            if let PendingQueueEntry::Active(slot) = entry {
                for waiter in slot.waiters {
                    let _ = waiter.tx.send(Err(format!(
                        "Subscribe to '{}' incomplete: {reason}",
                        slot.topic
                    )));
                }
            }
        }
    }

    /// Confirm a pending Subscribe slot (SubAck success or test settle Ok).
    /// Caller must already have removed the slot from `pending_subscribes`.
    pub(crate) async fn complete_pending_subscribe_success(
        inner: &MqttClientInner,
        slot: PendingSubscribeSlot,
    ) {
        inner
            .subscribed_topics
            .write()
            .await
            .insert(slot.topic.clone());
        for waiter in slot.waiters {
            let _ = waiter.tx.send(Ok(()));
        }
    }

    /// Fail a pending Subscribe slot (SubAck reject or test settle Err).
    /// Caller must already have removed the slot from `pending_subscribes`.
    pub(crate) async fn complete_pending_subscribe_failure(
        slot: PendingSubscribeSlot,
        message: String,
    ) {
        for waiter in slot.waiters {
            let _ = waiter.tx.send(Err(message.clone()));
        }
    }

    /// Test/helper: install a synthetic broker client slot with `session_ready=true`
    /// so `subscribe_topic` can succeed without a live TCP session. The request
    /// channel stays open; the event loop is held (not polled) so no connection
    /// is attempted.
    ///
    /// Production `add_broker` inserts the client **before** ConnAck — the map
    /// entry alone is "created/queued", not session-ready. `session_ready` flips
    /// only on ConnAck Success and clears on poll errors during reconnect.
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn install_test_broker_slot(&self, broker_id: &str) {
        self.install_test_broker_slot_with_capacity(broker_id, 256)
            .await;
    }

    /// Like [`install_test_broker_slot`] but with an explicit client request
    /// channel capacity (used to force subscribe backpressure in tests).
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn install_test_broker_slot_with_capacity(&self, broker_id: &str, capacity: usize) {
        use rumqttc::{AsyncClient, MqttOptions};
        use std::time::Duration;

        if self.mqtt_clients.read().await.contains_key(broker_id) {
            return;
        }
        let mut opts = MqttOptions::new(
            format!("neomind-test-slot-{broker_id}"),
            "127.0.0.1",
            9, // unused — event loop is never polled
        );
        opts.set_keep_alive(Duration::from_secs(5));
        // Default capacity (256) must cover large reconnect restores in tests
        // (>100 topics) because the eventloop is not polled and never drains.
        // Tests may pass a small capacity to force subscribe backpressure.
        let (client, eventloop) = AsyncClient::new(opts, capacity);
        // Hold the eventloop so the client request channel stays alive; do not
        // poll (avoids connection attempts / disconnect cleanup races).
        std::mem::forget(eventloop);
        let (reconnect_tx, mut reconnect_rx) = tokio::sync::mpsc::channel::<()>(8);
        // Drain reconnect signals so try_send never blocks test slots.
        tokio::spawn(async move { while reconnect_rx.recv().await.is_some() {} });
        self.mqtt_clients.write().await.insert(
            broker_id.to_string(),
            MqttClientInner {
                _broker_id: broker_id.to_string(),
                _broker_addr: "test-slot".to_string(),
                client,
                running: Arc::new(RwLock::new(true)),
                subscribed_topics: Arc::new(RwLock::new(std::collections::HashSet::new())),
                desired_topics: Arc::new(RwLock::new(std::collections::HashSet::new())),
                pending_subscribes: Arc::new(Mutex::new(PendingSubscribeState::default())),
                subscribe_send: Arc::new(Mutex::new(())),
                reconnect_tx,
                // No polled event loop → no SubAck. Default bypass keeps existing
                // unit tests working; SubAck-honesty tests call enable_test_suback_wait.
                await_suback: false,
                suback_timeout: DEFAULT_SUBACK_TIMEOUT,
                // Tests need an honest "session ready" stand-in without ConnAck.
                session_ready: AtomicBool::new(true),
            },
        );
        *self.connection_status.write().await = ConnectionStatus::Connected;
    }

    /// Test/helper: flip session_ready on an installed slot (simulate disconnect
    /// while the client entry is retained during reconnect).
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn set_test_session_ready(&self, broker_id: &str, ready: bool) {
        Self::set_broker_session_ready(
            &self.mqtt_clients,
            &self.connection_status,
            broker_id,
            ready,
        )
        .await;
    }

    /// Test/helper: require real SubAck wait (or [`settle_test_pending_suback`])
    /// instead of the test-slot queue-Ok bypass.
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn enable_test_suback_wait(&self, broker_id: &str, timeout: Duration) {
        let mut clients = self.mqtt_clients.write().await;
        if let Some(inner) = clients.get_mut(broker_id) {
            inner.await_suback = true;
            inner.suback_timeout = timeout;
        }
    }

    /// Test/helper: complete a pending Subscribe as if SubAck arrived.
    /// `Ok(())` moves the topic into the confirmed set; `Err` does not.
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn settle_test_pending_suback(
        &self,
        broker_id: &str,
        topic: &str,
        result: Result<(), String>,
    ) {
        let clients = self.mqtt_clients.read().await;
        let Some(inner) = clients.get(broker_id) else {
            return;
        };
        let slot = {
            let mut pending = inner.pending_subscribes.lock().await;
            let idx = pending.queue.iter().position(|e| match e {
                PendingQueueEntry::Active(s) => s.topic == topic,
                PendingQueueEntry::Absorb { .. } => false,
            });
            idx.and_then(|i| match pending.queue.remove(i) {
                Some(PendingQueueEntry::Active(slot)) => Some(slot),
                _ => None,
            })
        };
        let Some(slot) = slot else {
            return;
        };
        match result {
            Ok(()) => Self::complete_pending_subscribe_success(inner, slot).await,
            Err(msg) => Self::complete_pending_subscribe_failure(slot, msg).await,
        }
    }

    /// Test/helper: settle every currently pending Subscribe (main + bare alias).
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn settle_all_test_pending_subacks(
        &self,
        broker_id: &str,
        result: Result<(), String>,
    ) {
        let topics: Vec<String> = {
            let clients = self.mqtt_clients.read().await;
            let Some(inner) = clients.get(broker_id) else {
                return;
            };
            let pending = inner.pending_subscribes.lock().await;
            pending.active_topics()
        };
        for topic in topics {
            self.settle_test_pending_suback(broker_id, &topic, result.clone())
                .await;
        }
    }

    /// Test/helper: simulate disconnect — clear confirmed, keep desired, fail pending.
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn test_simulate_disconnect_clear_confirmed(&self, broker_id: &str) {
        let clients = self.mqtt_clients.read().await;
        if let Some(inner) = clients.get(broker_id) {
            Self::on_broker_disconnect(inner, "test simulated disconnect").await;
            inner.set_session_ready(false);
        }
        self.update_connection_status().await;
    }

    /// Test/helper: restore desired topics via the out-of-poll reconnect path.
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn test_restore_desired_after_reconnect(&self, broker_id: &str) {
        Self::restore_desired_topics_after_reconnect(&self.mqtt_clients, broker_id).await;
    }

    /// Test/helper: inject Outgoing::Subscribe / SubAck into the correlator.
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn test_apply_subscribe_outgoing(&self, broker_id: &str, pkid: u16) {
        Self::apply_subscription_events(
            &self.mqtt_clients,
            broker_id,
            &rumqttc::Event::Outgoing(rumqttc::Outgoing::Subscribe(pkid)),
        )
        .await;
    }

    /// Test/helper: inject a SubAck (success or failure) for a pkid.
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn test_apply_suback(&self, broker_id: &str, pkid: u16, success: bool) {
        let code = if success {
            rumqttc::SubscribeReasonCode::Success(rumqttc::QoS::AtLeastOnce)
        } else {
            rumqttc::SubscribeReasonCode::Failure
        };
        Self::apply_subscription_events(
            &self.mqtt_clients,
            broker_id,
            &rumqttc::Event::Incoming(rumqttc::Packet::SubAck(rumqttc::SubAck {
                pkid,
                return_codes: vec![code],
            })),
        )
        .await;
    }

    /// Test/helper: snapshot desired (restore intent) topics.
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn test_desired_topics(&self, broker_id: &str) -> Vec<String> {
        let clients = self.mqtt_clients.read().await;
        let Some(inner) = clients.get(broker_id) else {
            return Vec::new();
        };
        let mut v: Vec<_> = inner.desired_topics.read().await.iter().cloned().collect();
        v.sort();
        v
    }

    /// Test/helper: seed a desired topic without confirming it.
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn seed_test_desired_topic(&self, broker_id: &str, topic: &str) {
        let clients = self.mqtt_clients.read().await;
        if let Some(inner) = clients.get(broker_id) {
            inner.desired_topics.write().await.insert(topic.to_string());
        }
    }

    /// Test/helper: fail every pending Subscribe (disconnect / broker drop).
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn fail_test_pending_subscribes(&self, broker_id: &str, reason: &str) {
        let clients = self.mqtt_clients.read().await;
        if let Some(inner) = clients.get(broker_id) {
            Self::fail_all_pending_subscribes(inner, reason).await;
        }
    }

    /// Test/helper: snapshot whether a topic is in the **confirmed** set.
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn test_topic_confirmed(&self, broker_id: &str, topic: &str) -> bool {
        let clients = self.mqtt_clients.read().await;
        let Some(inner) = clients.get(broker_id) else {
            return false;
        };
        let hit = inner.subscribed_topics.read().await.contains(topic);
        hit
    }

    /// Test/helper: insert a topic into the confirmed set (e.g. seed `"#"`).
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn seed_test_confirmed_topic(&self, broker_id: &str, topic: &str) {
        let clients = self.mqtt_clients.read().await;
        if let Some(inner) = clients.get(broker_id) {
            inner
                .subscribed_topics
                .write()
                .await
                .insert(topic.to_string());
        }
    }

    /// Test/helper: how many Active Subscribe requests are awaiting SubAck.
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn test_pending_subscribe_count(&self, broker_id: &str) -> usize {
        let clients = self.mqtt_clients.read().await;
        let Some(inner) = clients.get(broker_id) else {
            return 0;
        };
        let pending = inner.pending_subscribes.lock().await;
        pending
            .queue
            .iter()
            .filter(|e| matches!(e, PendingQueueEntry::Active(_)))
            .count()
    }

    /// Test/helper: Absorb entries left for late Outgoing/SubAck.
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn test_absorb_subscribe_count(&self, broker_id: &str) -> usize {
        let clients = self.mqtt_clients.read().await;
        let Some(inner) = clients.get(broker_id) else {
            return 0;
        };
        let pending = inner.pending_subscribes.lock().await;
        pending.absorb_count()
    }

    /// Test/helper: whether an Active pending entry exists for `topic`.
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn test_pending_has_topic(&self, broker_id: &str, topic: &str) -> bool {
        let clients = self.mqtt_clients.read().await;
        let Some(inner) = clients.get(broker_id) else {
            return false;
        };
        let pending = inner.pending_subscribes.lock().await;
        pending.active_topics().iter().any(|t| t == topic)
    }

    /// Update overall connection status from session-ready brokers (not mere
    /// client-map presence — insert-before-ConnAck must not look Connected).
    pub(crate) async fn update_connection_status(&self) {
        let any_ready = self
            .mqtt_clients
            .read()
            .await
            .values()
            .any(MqttClientInner::is_session_ready);
        *self.connection_status.write().await = if any_ready {
            ConnectionStatus::Connected
        } else {
            ConnectionStatus::Disconnected
        };
    }

    /// Default value parsing (when no protocol mapping is available).
    pub(crate) fn default_parse_value(payload: &[u8]) -> Result<MetricValue, String> {
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
