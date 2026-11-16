//! `handling` — split from the former mqtt.rs monolith.

use super::config::MqttAdapterConfig;
use super::*;

use crate::adapter::{AdapterError, AdapterResult, ConnectionStatus, DeviceEvent};

use crate::image_storage::save_image_binary;

use crate::mdl::MetricValue;

use crate::registry::DeviceRegistry;

use crate::telemetry::TimeSeriesStorage;

use crate::unified_extractor::UnifiedExtractor;

use base64::engine::general_purpose::STANDARD as BASE64;

use neomind_core::EventBus;

use neomind_core::NeoMindEvent;

use std::collections::{HashMap, HashSet};

use std::path::PathBuf;

use std::sync::Arc;

use tokio::sync::{broadcast, RwLock};

use tracing::{debug, error, info, warn};

use super::identity::{
    approved_identity_from_extra, lookup_topic_device_ids, match_by_approved_identity,
    match_registered_device, match_registered_device_except, resolve_mqtt_device_identity,
    topic_map_insert, topic_map_remove_device_all, ApprovedIdentityOutcome, OutboundEchoBuffer,
    TopicDeviceMap, MQTT_IDENTITY_FIELD_KEY, MQTT_IDENTITY_SOURCE_KEY, OUTBOUND_ECHO_TTL,
};

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
    pub(crate) async fn handle_mqtt_notification(
        notification: rumqttc::Event,
        config: &MqttAdapterConfig,
        event_tx: &broadcast::Sender<DeviceEvent>,
        event_bus: &Option<Arc<EventBus>>,
        device_types: &Arc<RwLock<HashMap<String, String>>>,
        metric_cache: &Arc<
            RwLock<HashMap<String, HashMap<String, (MetricValue, chrono::DateTime<chrono::Utc>)>>>,
        >,
        telemetry_storage: &Arc<RwLock<Option<Arc<TimeSeriesStorage>>>>,
        device_registry: &Arc<RwLock<Arc<DeviceRegistry>>>,
        _connection_status: &Arc<RwLock<ConnectionStatus>>,
        broker_id: &str,
        extractor: &Arc<UnifiedExtractor>,
        topic_to_device: &Arc<RwLock<TopicDeviceMap>>,
        outbound_echoes: &Arc<RwLock<OutboundEchoBuffer>>,
        uncollectable: &Arc<RwLock<HashSet<String>>>,
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

                // Echo suppression: topic + payload + short TTL (not "topic ever outbound").
                {
                    let mut echoes = outbound_echoes.write().await;
                    if echoes.is_echo(&topic, &payload, OUTBOUND_ECHO_TTL) {
                        debug!(
                            "Skipping self-echo of outbound command (topic+payload match): {}",
                            topic
                        );
                        return;
                    }
                }

                // Bound devices on this topic (explicit telemetry / learned map).
                // Topic shape is not purpose: unbound NeoMind `…/downlink` may still
                // become a discovery candidate. `$SYS` (above) and outbound echo
                // suppression remain the only protocol drops.
                let bound_devices = {
                    let mapping = topic_to_device.read().await;
                    lookup_topic_device_ids(&mapping, &topic)
                };

                // Discovery-rule identity (adapter config / auto-detect). Used for
                // unknown identities and as fallback when no approved per-device rule.
                let identity = resolve_mqtt_device_identity(&topic, &payload, config);
                info!(
                    "Resolved MQTT identity topic='{}' id='{}' source={} field={:?}",
                    topic,
                    identity.device_id,
                    identity.source.as_str(),
                    identity.field
                );

                // Registered devices: prefer each device's approved identity rule
                // (sticky source/field from draft/approve). Conflict / Unverifiable
                // isolate **per device** — blocked ids skip auto/1:1 remapping,
                // other bound devices may still route. Missing approved field is
                // Unverifiable (not NoRule).
                let mut approved_blocked = HashSet::new();
                let mut device_id_opt = {
                    let registry = device_registry.read().await;
                    match match_by_approved_identity(
                        &bound_devices,
                        &topic,
                        &payload,
                        config,
                        |dev_id| {
                            registry.get_device(dev_id).and_then(|d| {
                                approved_identity_from_extra(&d.connection_config.extra)
                            })
                        },
                    ) {
                        ApprovedIdentityOutcome::Match(id) => Some(id),
                        ApprovedIdentityOutcome::NoRule => {
                            let mapping = topic_to_device.read().await;
                            match_registered_device(&mapping, &topic, &identity)
                        }
                        ApprovedIdentityOutcome::Conflict { blocked }
                        | ApprovedIdentityOutcome::Unverifiable { blocked } => {
                            approved_blocked = blocked;
                            let mapping = topic_to_device.read().await;
                            match_registered_device_except(
                                &mapping,
                                &topic,
                                &identity,
                                &approved_blocked,
                            )
                        }
                    }
                };

                // CT-Prov bare-topic / stale map: strong-extract id already in the
                // **live registry** → associate + learn the topic, do NOT draft.
                // Learn + ban share one sync boundary: hold `uncollectable` write
                // through the map insert so unsubscribe cannot clear-then-lose to
                // a re-insert race.
                if device_id_opt.is_none()
                    && identity.source == super::identity::MqttIdentitySource::TopicStrong
                    && !approved_blocked.contains(&identity.device_id)
                {
                    // Hold ban write through map insert so unsubscribe's
                    // ban+clear cannot interleave (same lock order).
                    let banned_guard = uncollectable.write().await;
                    if !banned_guard.contains(&identity.device_id) {
                        let known = {
                            let registry = device_registry.read().await;
                            registry.get_device(&identity.device_id).is_some()
                        };
                        if known {
                            let mut mapping = topic_to_device.write().await;
                            topic_map_insert(&mut mapping, &topic, &identity.device_id);
                            info!(
                                "Learned topic mapping for registered device {} from '{}'",
                                identity.device_id, topic
                            );
                            device_id_opt = Some(identity.device_id.clone());
                        }
                    }
                    drop(banned_guard);
                }

                // Formal routing requires a live, collectable registration.
                // Banned / stale map hits clear the association so a race residue
                // cannot keep routing to a teardown or ghost device.
                if let Some(id) = device_id_opt.clone() {
                    let banned = {
                        let u = uncollectable.read().await;
                        u.contains(&id)
                    };
                    if banned || approved_blocked.contains(&id) {
                        debug!(
                            "Skipping formal route for banned/blocked device {} on {}",
                            id, topic
                        );
                        if banned {
                            let mut mapping = topic_to_device.write().await;
                            let _ = topic_map_remove_device_all(&mut mapping, &id);
                        }
                        device_id_opt = None;
                        if banned {
                            let still_registered = {
                                let registry = device_registry.read().await;
                                registry.get_device(&id).is_some()
                            };
                            if still_registered {
                                // Mid-delete — do not rediscover while registry
                                // still lists the device.
                                return;
                            }
                            // Delete complete: fall through so this uplink can
                            // rediscover after the stale map was cleared.
                        }
                    } else {
                        let registered = {
                            let registry = device_registry.read().await;
                            registry.get_device(&id).is_some()
                        };
                        if !registered {
                            debug!(
                                "Dropping stale map route for unregistered device {} on {}",
                                id, topic
                            );
                            let mut mapping = topic_to_device.write().await;
                            let _ = topic_map_remove_device_all(&mut mapping, &id);
                            device_id_opt = None;
                        }
                    }
                }

                // Mid-delete (banned + still registered): no discovery draft.
                // After registry remove, banned-but-unregistered may rediscover
                // (ban clears on re-subscribe).
                if device_id_opt.is_none() {
                    let banned = {
                        let u = uncollectable.read().await;
                        u.contains(&identity.device_id)
                    };
                    if banned {
                        let still_registered = {
                            let registry = device_registry.read().await;
                            registry.get_device(&identity.device_id).is_some()
                        };
                        if still_registered {
                            debug!(
                                "Skipping discovery for uncollectable+registered identity {} on {}",
                                identity.device_id, topic
                            );
                            return;
                        }
                    }
                }

                if let Some(device_id) = device_id_opt {
                    debug!(
                        "Routing message for registered device {} from topic {}",
                        device_id, topic
                    );

                    let device_type_opt = {
                        let types = device_types.read().await;
                        types
                            .get(&device_id)
                            .cloned()
                            .or_else(|| extract_device_type_from_topic(&topic))
                    };

                    // Cache type from topic when present.
                    if let Some(ref dt) = device_type_opt {
                        let mut types = device_types.write().await;
                        types.insert(device_id.clone(), dt.clone());
                    }

                    if let Ok(json_data) = serde_json::from_slice::<serde_json::Value>(&payload) {
                        let client_ts = extract_client_timestamp(&json_data);
                        let point_ts = client_ts.unwrap_or_else(|| now.timestamp());

                        if let Some(dt) = device_type_opt {
                            let result = extractor.extract(&device_id, &dt, &json_data).await;
                            debug!(
                                "Extraction result for device '{}': mode={:?}, metrics={}",
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
                                let value = Self::convert_binary_to_url(
                                    &device_id,
                                    &metric.name,
                                    point_ts,
                                    metric.value.clone(),
                                    data_dir,
                                );

                                {
                                    let mut cache = metric_cache.write().await;
                                    cache
                                        .entry(device_id.clone())
                                        .or_default()
                                        .insert(metric.name.clone(), (value.clone(), now));
                                }

                                if let Some(storage) = telemetry_storage.read().await.as_ref() {
                                    let data_point = crate::telemetry::DataPoint {
                                        timestamp: point_ts,
                                        value: value.clone(),
                                        quality: None,
                                        metadata: None,
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

                                if let Err(e) = event_tx.send(DeviceEvent::Metric {
                                    device_id: device_id.clone(),
                                    metric: metric.name.clone(),
                                    value: value.clone(),
                                    timestamp: point_ts,
                                }) {
                                    error!(
                                        "Failed to send metric event to channel: {}/{} - {}",
                                        device_id, metric.name, e
                                    );
                                }
                            }

                            if let Some(bus) = event_bus {
                                bus.publish(NeoMindEvent::DeviceOnline {
                                    device_id: device_id.clone(),
                                    device_type: dt,
                                    timestamp: now.timestamp(),
                                })
                                .await;
                            }
                        } else if let Ok(value) = MqttAdapter::default_parse_value(&payload) {
                            let metric_name = "value";
                            let value = Self::convert_binary_to_url(
                                &device_id,
                                metric_name,
                                now.timestamp(),
                                value,
                                data_dir,
                            );
                            {
                                let mut cache = metric_cache.write().await;
                                cache
                                    .entry(device_id.clone())
                                    .or_default()
                                    .insert(metric_name.to_string(), (value.clone(), now));
                            }
                            if let Some(storage) = telemetry_storage.read().await.as_ref() {
                                let data_point = crate::telemetry::DataPoint {
                                    timestamp: now.timestamp(),
                                    value: value.clone(),
                                    quality: None,
                                    metadata: None,
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
                            if let Err(e) = event_tx.send(DeviceEvent::Metric {
                                device_id: device_id.clone(),
                                metric: metric_name.to_string(),
                                value,
                                timestamp: now.timestamp(),
                            }) {
                                error!(
                                    "Failed to send metric event to channel: {}/{} - {}",
                                    device_id, metric_name, e
                                );
                            }
                        }
                    } else if is_neomind_device_telemetry_topic(&topic) {
                        warn!(
                            "Failed to parse uplink payload as JSON for device {} on topic {} — dropping",
                            device_id, topic
                        );
                    }
                    return;
                }

                // Unregistered identity → discovery candidate (human approve gate).
                info!(
                    "Triggering auto-onboarding for unregistered identity '{}' on topic {}",
                    identity.device_id, topic
                );

                if payload.len() > MAX_AUTOONBOARD_SAMPLE_BYTES {
                    tracing::warn!(
                        topic = %topic,
                        size = payload.len(),
                        limit = MAX_AUTOONBOARD_SAMPLE_BYTES,
                        "Auto-onboard payload exceeds sample size limit, skipping"
                    );
                    return;
                }

                let auto_device_id = identity.device_id.clone();
                let (sample_data, is_binary, data_format) =
                    if let Ok(json_data) = serde_json::from_slice::<serde_json::Value>(&payload) {
                        let actual_data = json_data.get("data").unwrap_or(&json_data);
                        (actual_data.clone(), false, "json")
                    } else {
                        (serde_json::json!(BASE64.encode(&payload)), true, "base64")
                    };

                tracing::info!(
                    topic = %topic,
                    "Unregistered MQTT device: telemetry not recorded (discovery event only) — register the device to start recording"
                );

                {
                    let adapter_id = config.name.clone();
                    let mut sample = serde_json::json!({
                        "device_id": auto_device_id,
                        "timestamp": chrono::Utc::now().timestamp(),
                        "topic": topic,
                        "data": sample_data,
                        "format": data_format,
                        "is_binary": is_binary
                    });
                    if let Some(obj) = sample.as_object_mut() {
                        obj.insert(
                            MQTT_IDENTITY_SOURCE_KEY.to_string(),
                            serde_json::Value::String(identity.source.as_str().to_string()),
                        );
                        if let Some(ref field) = identity.field {
                            obj.insert(
                                MQTT_IDENTITY_FIELD_KEY.to_string(),
                                serde_json::Value::String(field.clone()),
                            );
                        }
                    }

                    let mut meta = serde_json::json!({
                        "source": "mqtt",
                        "broker_id": broker_id,
                        "adapter_id": adapter_id,
                        "original_topic": topic,
                        "sample": sample,
                        "is_binary": is_binary,
                    });
                    if let Some(obj) = meta.as_object_mut() {
                        obj.extend(identity.to_metadata_map());
                    }

                    let discovered = crate::adapter::DiscoveredDeviceInfo {
                        device_id: auto_device_id.clone(),
                        device_type: "unknown".to_string(),
                        name: None,
                        endpoint: Some(topic.to_string()),
                        capabilities: vec![],
                        timestamp: chrono::Utc::now().timestamp(),
                        metadata: meta,
                    };

                    let _ = event_tx
                        .send(crate::adapter::DeviceEvent::Discovery { device: discovered });

                    if let Some(bus) = event_bus {
                        let mut bus_meta = serde_json::json!({
                            "broker_id": broker_id,
                            "original_topic": topic,
                        });
                        if let Some(obj) = bus_meta.as_object_mut() {
                            obj.extend(identity.to_metadata_map());
                        }
                        bus.publish(NeoMindEvent::DeviceDiscovered {
                            device_id: auto_device_id,
                            source: "mqtt".to_string(),
                            adapter_id: Some(adapter_id),
                            metadata: bus_meta,
                            sample,
                            is_binary,
                            timestamp: chrono::Utc::now().timestamp(),
                        })
                        .await;
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
