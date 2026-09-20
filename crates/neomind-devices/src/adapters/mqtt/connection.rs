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

use std::collections::{HashMap, HashSet};

use std::path::PathBuf;

use std::sync::Arc;

use std::time::Duration;

use tokio::sync::{broadcast, RwLock};

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
    pub(crate) async fn subscribe_device_telemetry_topics(
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
    pub(crate) async fn resubscribe_after_reconnect(
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

    /// Update overall connection status based on connected brokers.
    pub(crate) async fn update_connection_status(&self) {
        let has_connected = !self.mqtt_clients.read().await.is_empty();
        *self.connection_status.write().await = if has_connected {
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
