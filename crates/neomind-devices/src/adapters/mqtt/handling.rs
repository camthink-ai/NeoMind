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

                                // Client-supplied timestamp (DEF-001): honor
                                // backfill payloads like the webhook path does;
                                // fall back to server receive time.
                                let client_ts = extract_client_timestamp(&json_value);

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
                                    let point_ts = client_ts.unwrap_or_else(|| now.timestamp());
                                    // Convert Binary to URL before storage + event bus (fork point)
                                    let value = Self::convert_binary_to_url(
                                        &device_id,
                                        &metric.name,
                                        point_ts,
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
                                            timestamp: point_ts,
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
                                        timestamp: point_ts,
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
                            // Client timestamp (DEF-001) — same policy as the
                            // registered-type branch above.
                            let client_ts_fallback = extract_client_timestamp(&json_data);
                            debug!("Successfully parsed JSON payload for device {}", device_id);

                            // Use UnifiedExtractor with the full JSON data
                            // The extractor handles dot-notation paths including "data.field" prefixes
                            // DO NOT pre-extract the "data" field - it causes double-extraction issues
                            if let Some(dt) = device_type_opt {
                                let result = extractor.extract(device_id, &dt, &json_data).await;
                                let point_ts_fb =
                                    client_ts_fallback.unwrap_or_else(|| now.timestamp());
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
                                            timestamp: point_ts_fb,
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
                                        timestamp: point_ts_fb,
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
