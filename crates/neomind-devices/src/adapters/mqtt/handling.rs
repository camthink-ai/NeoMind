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

use tokio::sync::{broadcast, oneshot, RwLock};

use tracing::{debug, error, info, warn};

use super::identity::{
    approved_identity_from_extra, lookup_topic_device_ids, match_by_approved_identity,
    match_registered_device, match_registered_device_except, pending_association_candidate_id,
    resolve_mqtt_device_identity, topic_map_insert, topic_map_remove_device_all,
    ApprovedIdentityOutcome, AssociationExceptionKind, OutboundEchoBuffer, TopicDeviceMap,
    MQTT_ASSOCIATION_EXCEPTION_KEY, MQTT_ASSOCIATION_KIND_KEY, MQTT_BLOCKED_DEVICE_IDS_KEY,
    MQTT_CLAIMED_IDENTITY_KEY, MQTT_IDENTITY_FIELD_KEY, MQTT_IDENTITY_SOURCE_KEY,
    MQTT_PENDING_CANDIDATE_ID_KEY, OUTBOUND_ECHO_TTL,
};

/// Latest value per metric per device — updated on write, before anything is
/// announced.
type MetricCache =
    Arc<RwLock<HashMap<String, HashMap<String, (MetricValue, chrono::DateTime<chrono::Utc>)>>>>;

impl MqttAdapter {
    /// Dynamically subscribe to a topic on ALL session-ready brokers.
    /// This is used when a device is registered with a custom telemetry topic.
    ///
    /// Client map presence alone is not enough: production inserts the client
    /// before ConnAck, and retains it while reconnecting. Only `session_ready`
    /// (ConnAck Success) may claim a live subscribe path.
    ///
    /// Request queued ≠ broker accepted. On production brokers (`await_suback`)
    /// this waits for SubAck **outside** the MQTT message callback; only a
    /// successful SubAck enters the confirmed set. Reject / timeout / disconnect
    /// leave the topic out of confirmed and return Err (BindIncomplete upstream).
    pub async fn subscribe_topic(&self, topic: &str) -> AdapterResult<()> {
        let clients = self.mqtt_clients.read().await;
        if clients.is_empty() {
            // Honest not-ready: adapter may be registered while no broker client
            // exists yet. Returning Ok here previously made association confirm
            // claim subscription_ready and consume the draft with no live sub.
            warn!(
                "No MQTT brokers connected, cannot subscribe to topic: {}",
                topic
            );
            return Err(AdapterError::Communication(format!(
                "No MQTT brokers connected; cannot subscribe to '{topic}' — retry when the adapter is connected"
            )));
        }

        let ready_ids: Vec<String> = clients
            .iter()
            .filter(|(_, inner)| inner.is_session_ready())
            .map(|(id, _)| id.clone())
            .collect();
        drop(clients);
        if ready_ids.is_empty() {
            // Clients exist (created/queued or reconnecting) but no ConnAck session.
            warn!(
                "MQTT broker client(s) present but session not ready; cannot subscribe to topic: {}",
                topic
            );
            return Err(AdapterError::Communication(format!(
                "MQTT broker session not ready; cannot subscribe to '{topic}' — client may be connecting or reconnecting; retry after ConnAck"
            )));
        }

        let mut subscribed_count = 0;
        let mut last_error: Option<String> = None;

        for broker_id in &ready_ids {
            // Re-lock per broker and clone Arc handles so SubAck wait does not
            // hold mqtt_clients (poll loop / settler must progress).
            let handles = {
                let clients = self.mqtt_clients.read().await;
                let Some(inner) = clients.get(broker_id) else {
                    continue;
                };
                if !inner.is_session_ready() {
                    continue;
                }
                BrokerSubscribeHandles::from_inner(inner)
            };
            match Self::request_subscribe_on_broker(&handles, broker_id, topic, true).await {
                Ok(()) => subscribed_count += 1,
                Err(e) => {
                    warn!(
                        "Failed to subscribe to {} on broker {}: {}",
                        topic, broker_id, e
                    );
                    last_error = Some(e.to_string());
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
            return Err(AdapterError::Communication(format!(
                "Failed to subscribe to {topic} on any broker"
            )));
        }

        info!(
            "Subscribed to topic {} on {} broker(s)",
            topic, subscribed_count
        );
        Ok(())
    }

    /// Unified subscribe request (startup / restore / dynamic / reconnect).
    ///
    /// - Records restore intent in `desired_topics` immediately.
    /// - Cover reuse reads **confirmed** only (rejected wildcards never cover).
    /// - Registers pending **before** send under `subscribe_send` (pkid order).
    /// - When `wait_for_suback`: timeout covers enqueue + wait. Cancel is by
    ///   **request id**. Pre-enqueue cancel removes the pending entry (no Absorb);
    ///   post-enqueue cancel leaves Absorb so late Outgoing/SubAck cannot steal.
    /// - When not waiting (startup/bootstrap): enqueue-Ok keeps the broker; SubAck
    ///   still alone promotes into confirmed (rejected `#` never covers).
    /// - Only successful SubAck enters confirmed.
    pub(crate) async fn request_subscribe_on_broker(
        handles: &BrokerSubscribeHandles,
        broker_id: &str,
        topic: &str,
        wait_for_suback: bool,
    ) -> AdapterResult<()> {
        // Intent survives disconnect even if this attempt fails.
        handles
            .desired_topics
            .write()
            .await
            .insert(topic.to_string());

        // Confirmed set only — pending / desired / rejected must not look subscribed.
        {
            let confirmed = handles.subscribed_topics.read().await;
            if confirmed.contains(topic) {
                debug!(
                    "Already confirmed subscribed to topic {} on broker {}",
                    topic, broker_id
                );
                return Ok(());
            }
            if confirmed
                .iter()
                .any(|existing| topic_filter_covers(existing, topic))
            {
                debug!(
                    "Topic '{}' already covered by a confirmed subscription on broker {}, skipping to avoid duplicate delivery",
                    topic, broker_id
                );
                return Ok(());
            }
        }

        // Test slots without a polled event loop: queue-Ok simulates SubAck success.
        if !handles.await_suback {
            match handles
                .client
                .subscribe(topic, rumqttc::QoS::AtLeastOnce)
                .await
            {
                Ok(()) => {
                    handles
                        .subscribed_topics
                        .write()
                        .await
                        .insert(topic.to_string());
                    info!(
                        "Subscribed to topic {} on broker {} (test slot: queue-Ok as confirmed)",
                        topic, broker_id
                    );
                    return Ok(());
                }
                Err(e) => {
                    return Err(AdapterError::Communication(format!(
                        "Failed to queue Subscribe for '{topic}' on broker {broker_id}: {e}"
                    )));
                }
            }
        }

        if wait_for_suback {
            let timeout = handles.suback_timeout;
            let request_id = next_subscribe_request_id();
            match tokio::time::timeout(
                timeout,
                Self::enqueue_and_await_suback(handles, broker_id, topic, request_id),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => {
                    Self::cancel_pending_request(
                        &handles.pending_subscribes,
                        request_id,
                        format!(
                            "SUBACK timeout waiting for '{topic}' on broker {broker_id} after {timeout:?}"
                        ),
                    )
                    .await;
                    Err(AdapterError::Communication(format!(
                        "SUBACK timeout waiting for '{topic}' on broker {broker_id} after {timeout:?}"
                    )))
                }
            }
        } else {
            // Startup/bootstrap: enqueue only. SubAck (or reject) arrives via poll.
            let request_id = next_subscribe_request_id();
            Self::enqueue_subscribe_pending(handles, broker_id, topic, request_id, None).await
        }
    }

    /// Register pending + send Subscribe. Optional oneshot waiter for SubAck.
    ///
    /// `enqueued` flips to true only after `client.subscribe` returns Ok. If the
    /// waiter was cancelled while subscribe was in flight and the send still
    /// succeeded, an Absorb entry is inserted so the late Outgoing/SubAck cannot
    /// attach to a later Active request.
    async fn enqueue_subscribe_pending(
        handles: &BrokerSubscribeHandles,
        broker_id: &str,
        topic: &str,
        request_id: u64,
        waiter: Option<oneshot::Sender<Result<(), String>>>,
    ) -> AdapterResult<()> {
        let _send_guard = handles.subscribe_send.lock().await;
        let need_send = {
            let mut pending = handles.pending_subscribes.lock().await;
            if let Some(slot) = pending.find_active_mut(topic) {
                if let Some(tx) = waiter {
                    slot.waiters.push(PendingSubscribeWaiter { request_id, tx });
                }
                false
            } else {
                let mut waiters = Vec::new();
                if let Some(tx) = waiter {
                    waiters.push(PendingSubscribeWaiter { request_id, tx });
                }
                pending
                    .queue
                    .push_back(PendingQueueEntry::Active(PendingSubscribeSlot {
                        topic: topic.to_string(),
                        owner_request_id: request_id,
                        pkid: None,
                        enqueued: false,
                        waiters,
                    }));
                true
            }
        };
        if need_send {
            match handles
                .client
                .subscribe(topic, rumqttc::QoS::AtLeastOnce)
                .await
            {
                Ok(()) => {
                    let mut pending = handles.pending_subscribes.lock().await;
                    if let Some(slot) = pending.find_active_by_request_mut(request_id) {
                        slot.enqueued = true;
                    } else {
                        // Timed out / cancelled while subscribe was in flight, but
                        // the client accepted the packet — absorb late protocol events.
                        pending.queue.push_back(PendingQueueEntry::Absorb {
                            topic: topic.to_string(),
                            pkid: None,
                        });
                    }
                    info!(
                        "Subscribe queued for topic {} on broker {} — awaiting SubAck",
                        topic, broker_id
                    );
                }
                Err(e) => {
                    // Send never left the client — remove, never Absorb.
                    Self::cancel_pending_request(
                        &handles.pending_subscribes,
                        request_id,
                        format!("Failed to queue Subscribe for '{topic}': {e}"),
                    )
                    .await;
                    return Err(AdapterError::Communication(format!(
                        "Failed to queue Subscribe for '{topic}' on broker {broker_id}: {e}"
                    )));
                }
            }
        }
        Ok(())
    }

    async fn enqueue_and_await_suback(
        handles: &BrokerSubscribeHandles,
        broker_id: &str,
        topic: &str,
        request_id: u64,
    ) -> AdapterResult<()> {
        let (tx, rx) = oneshot::channel();
        Self::enqueue_subscribe_pending(handles, broker_id, topic, request_id, Some(tx)).await?;

        match rx.await {
            Ok(Ok(())) => {
                info!("SubAck confirmed topic {} on broker {}", topic, broker_id);
                Ok(())
            }
            Ok(Err(msg)) => Err(AdapterError::Communication(msg)),
            Err(_) => Err(AdapterError::Communication(format!(
                "Subscribe waiter dropped for '{topic}' on broker {broker_id}"
            ))),
        }
    }

    /// Cancel one subscribe request by id.
    ///
    /// - Pre-enqueue (`enqueued=false`): remove the waiter / slot — **no Absorb**.
    /// - Post-enqueue: when the last waiter leaves, replace Active with Absorb so
    ///   late Outgoing/SubAck cannot complete a later request.
    pub(crate) async fn cancel_pending_request(
        pending_subscribes: &Arc<tokio::sync::Mutex<PendingSubscribeState>>,
        request_id: u64,
        message: String,
    ) {
        let to_fail = {
            let mut pending = pending_subscribes.lock().await;
            let idx = pending.queue.iter().position(|e| match e {
                PendingQueueEntry::Active(s) => {
                    s.owner_request_id == request_id
                        || s.waiters.iter().any(|w| w.request_id == request_id)
                }
                PendingQueueEntry::Absorb { .. } => false,
            });
            let Some(i) = idx else {
                return;
            };
            let Some(PendingQueueEntry::Active(mut slot)) = pending.queue.remove(i) else {
                return;
            };
            let owner_cancelled = slot.owner_request_id == request_id;
            let (cancelled, remaining): (Vec<_>, Vec<_>) = slot
                .waiters
                .into_iter()
                .partition(|w| w.request_id == request_id);

            // Owner timed out before enqueue: the send future is gone — fail
            // every waiter and drop without Absorb (no protocol event).
            if owner_cancelled && !slot.enqueued {
                cancelled.into_iter().chain(remaining).collect::<Vec<_>>()
            } else if remaining.is_empty() {
                if slot.enqueued {
                    let absorb = PendingQueueEntry::Absorb {
                        topic: slot.topic.clone(),
                        pkid: slot.pkid,
                    };
                    let insert_at = i.min(pending.queue.len());
                    pending.queue.insert(insert_at, absorb);
                }
                cancelled
            } else {
                slot.waiters = remaining;
                let insert_at = i.min(pending.queue.len());
                pending
                    .queue
                    .insert(insert_at, PendingQueueEntry::Active(slot));
                cancelled
            }
        };
        for waiter in to_fail {
            let _ = waiter.tx.send(Err(message.clone()));
        }
    }

    /// Unsubscribe from a topic on ALL connected brokers.
    pub async fn unsubscribe_topic(&self, topic: &str) -> AdapterResult<()> {
        let clients = self.mqtt_clients.read().await;

        for (broker_id, inner) in clients.iter() {
            // Drop restore intent even if not currently confirmed.
            inner.desired_topics.write().await.remove(topic);
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
    /// Phase 1 of announcing an uplink: convert, cache and store every
    /// metric of the report. Nothing is announced until the whole report is
    /// stored — see [`Self::announce_report_metrics`].
    ///
    /// An event starts an agent run the moment it is published, and a run
    /// reads the device's report from storage: "store one, announce one" let
    /// the run arrive before the frame (a later metric) was written. Returns
    /// the stored `(metric, value)` pairs in report order.
    pub(crate) async fn store_report_metrics(
        device_id: &str,
        metrics: Vec<crate::unified_extractor::ExtractedMetric>,
        point_ts: i64,
        data_dir: Option<&PathBuf>,
        metric_cache: &MetricCache,
        telemetry_storage: &Arc<RwLock<Option<Arc<TimeSeriesStorage>>>>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Vec<(String, MetricValue)> {
        let mut stored_metrics = Vec::with_capacity(metrics.len());
        for metric in metrics {
            let value = Self::convert_binary_to_url(
                device_id,
                &metric.name,
                point_ts,
                metric.value,
                data_dir,
            );

            {
                let mut cache = metric_cache.write().await;
                cache
                    .entry(device_id.to_string())
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
                    .write(&format!("device:{}", device_id), &metric.name, data_point)
                    .await
                {
                    error!(
                        "Failed to write telemetry for {}/{}: {}",
                        device_id, metric.name, e
                    );
                }
            }

            stored_metrics.push((metric.name, value));
        }
        stored_metrics
    }

    /// Phase 2: announce a stored report on the device event channel, in
    /// report order. The service task turns these into bus events, which
    /// start agent runs.
    pub(crate) fn announce_report_metrics(
        device_id: &str,
        stored_metrics: Vec<(String, MetricValue)>,
        point_ts: i64,
        event_tx: &broadcast::Sender<DeviceEvent>,
    ) {
        for (metric_name, value) in stored_metrics {
            if let Err(e) = event_tx.send(DeviceEvent::Metric {
                device_id: device_id.to_string(),
                metric: metric_name.clone(),
                value,
                timestamp: point_ts,
            }) {
                error!(
                    "Failed to send metric event to channel: {}/{} - {}",
                    device_id, metric_name, e
                );
            }
        }
    }

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
        chunked_images: &Arc<tokio::sync::Mutex<chunked_image::ChunkedImageReassembler>>,
    ) {
        match notification {
            rumqttc::Event::Incoming(rumqttc::Packet::Publish(publish)) => {
                let topic = publish.topic.to_string();
                let mut payload = publish.payload.to_vec();

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

                // Chunked large-image reassembly (`type = "image_chunked_header" | "image_chunk"`).
                // Intercept BEFORE identity/telemetry/discovery routing so incomplete
                // chunks never emit partial metrics or pollute auto-onboarding drafts.
                // Once complete, synthesize a lightweight JSON payload for identity /
                // discovery and save the assembled raw image directly via
                // `save_image_binary` on the registered-device path (avoiding a multi-MB
                // base64 re-encode + decode round trip).
                let mut reassembled_image: Option<chunked_image::ReassembledImage> = None;
                if chunked_image::payload_may_be_chunked_image(&payload) {
                    if let Ok(raw_json) = serde_json::from_slice::<serde_json::Value>(&payload) {
                        let outcome = {
                            let mut guard = chunked_images.lock().await;
                            guard.ingest_json(broker_id, &topic, &raw_json)
                        };
                        match outcome {
                            chunked_image::ChunkIngestOutcome::NotChunked => {}
                            chunked_image::ChunkIngestOutcome::Pending {
                                image_id,
                                received_chunks,
                                total_chunks,
                            } => {
                                debug!(
                                    topic = %topic,
                                    image_id = %image_id,
                                    received_chunks,
                                    total_chunks,
                                    "Buffered MQTT image chunk"
                                );
                                return;
                            }
                            chunked_image::ChunkIngestOutcome::Duplicate { image_id } => {
                                debug!(
                                    topic = %topic,
                                    image_id = %image_id,
                                    "Ignoring duplicate MQTT image chunk or retransmit"
                                );
                                return;
                            }
                            chunked_image::ChunkIngestOutcome::Rejected { reason } => {
                                warn!(
                                    topic = %topic,
                                    reason = %reason,
                                    "Rejected invalid MQTT chunked image message"
                                );
                                return;
                            }
                            chunked_image::ChunkIngestOutcome::Completed(img) => {
                                info!(
                                    topic = %topic,
                                    image_id = %img.image_id,
                                    bytes = img.image_bytes.len(),
                                    "Reassembled MQTT chunked image"
                                );
                                if let Ok(light_bytes) =
                                    serde_json::to_vec(&img.to_discovery_sample_json())
                                {
                                    payload = light_bytes;
                                }
                                reassembled_image = Some(img);
                            }
                        }
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
                debug!(
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
                let mut association_exception_kind: Option<AssociationExceptionKind> = None;
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
                        ApprovedIdentityOutcome::Conflict { blocked } => {
                            approved_blocked = blocked;
                            association_exception_kind = Some(AssociationExceptionKind::Conflict);
                            let mapping = topic_to_device.read().await;
                            match_registered_device_except(
                                &mapping,
                                &topic,
                                &identity,
                                &approved_blocked,
                            )
                        }
                        ApprovedIdentityOutcome::Unverifiable { blocked } => {
                            approved_blocked = blocked;
                            association_exception_kind =
                                Some(AssociationExceptionKind::Unverifiable);
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
                // Before learning, verify the **target** device's approved identity
                // rule against this message (bound-topic check above only covers
                // devices already mapped to the current topic). Rule present →
                // must Match; Conflict / Unverifiable → no formal association
                // (discovery may still proceed). NoRule keeps prior compat.
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
                        let learn_allowed = {
                            let registry = device_registry.read().await;
                            match registry.get_device(&identity.device_id) {
                                None => false,
                                Some(dev) => {
                                    let candidates: HashSet<String> =
                                        std::iter::once(identity.device_id.clone()).collect();
                                    let extra = &dev.connection_config.extra;
                                    match match_by_approved_identity(
                                        &candidates,
                                        &topic,
                                        &payload,
                                        config,
                                        |_| approved_identity_from_extra(extra),
                                    ) {
                                        ApprovedIdentityOutcome::Match(_)
                                        | ApprovedIdentityOutcome::NoRule => true,
                                        ApprovedIdentityOutcome::Conflict { blocked } => {
                                            approved_blocked.extend(blocked);
                                            association_exception_kind =
                                                Some(AssociationExceptionKind::Conflict);
                                            false
                                        }
                                        ApprovedIdentityOutcome::Unverifiable { blocked } => {
                                            approved_blocked.extend(blocked);
                                            if association_exception_kind.is_none() {
                                                association_exception_kind =
                                                    Some(AssociationExceptionKind::Unverifiable);
                                            }
                                            false
                                        }
                                    }
                                }
                            }
                        };
                        if learn_allowed {
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

                    let json_opt = if let Some(img) = reassembled_image {
                        let preliminary = img.to_discovery_sample_json();
                        let client_ts = extract_client_timestamp(&preliminary);
                        let point_ts = client_ts.unwrap_or_else(|| now.timestamp());
                        let image_data_val = if let Some(dir) = data_dir {
                            match save_image_binary(
                                &device_id,
                                "image_data",
                                point_ts,
                                &img.image_bytes,
                                dir,
                            ) {
                                Ok(url) => {
                                    debug!(
                                        "Saved reassembled chunked image for {}/image_data ({} bytes) -> {}",
                                        device_id,
                                        img.image_bytes.len(),
                                        url
                                    );
                                    url
                                }
                                Err(e) => {
                                    error!(
                                        "Failed to save reassembled chunked image for {}/image_data: {}",
                                        device_id, e
                                    );
                                    BASE64.encode(&img.image_bytes)
                                }
                            }
                        } else {
                            BASE64.encode(&img.image_bytes)
                        };
                        Some(img.to_report_json(image_data_val))
                    } else {
                        serde_json::from_slice::<serde_json::Value>(&payload).ok()
                    };

                    if let Some(json_data) = json_opt {
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

                            // Store the whole report, then announce it.
                            let stored_metrics = Self::store_report_metrics(
                                &device_id,
                                result.metrics,
                                point_ts,
                                data_dir,
                                metric_cache,
                                telemetry_storage,
                                now,
                            )
                            .await;
                            Self::announce_report_metrics(
                                &device_id,
                                stored_metrics,
                                point_ts,
                                event_tx,
                            );

                            if let Some(bus) = event_bus {
                                bus.publish(NeoMindEvent::DeviceOnline {
                                    device_id: device_id.clone(),
                                    device_type: dt,
                                    timestamp: now.timestamp(),
                                })
                                .await;
                            }
                        } else if let Ok(value) = serde_json::to_vec(&json_data)
                            .map_err(|e| e.to_string())
                            .and_then(|bytes| MqttAdapter::default_parse_value(&bytes))
                        {
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

                // No formal route → discovery / association-exception candidate.
                // If the resolved identity is already registered (Conflict /
                // Unverifiable learn gate, etc.), do NOT emit that registered id
                // — the server listener would skip auto-onboarding and the
                // customer would see a silent drop. Emit a pending candidate
                // that keeps topic/sample/conflict info for human confirm.
                // Never invent a formal id from another payload field.
                // Registered claimed identity + no formal route ⇒ association
                // exception (server would otherwise skip DeviceDiscovered(A)).
                let identity_registered = {
                    let registry = device_registry.read().await;
                    registry.get_device(&identity.device_id).is_some()
                };
                let is_association_exception = identity_registered;
                let auto_device_id = if is_association_exception {
                    pending_association_candidate_id(&topic, &identity.device_id)
                } else {
                    identity.device_id.clone()
                };

                info!(
                    "Triggering auto-onboarding for {} '{}' (claimed='{}') on topic {}",
                    if is_association_exception {
                        "association-exception candidate"
                    } else {
                        "unregistered identity"
                    },
                    auto_device_id,
                    identity.device_id,
                    topic
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

                // Keep the FULL wire JSON (including nested `data.sn`). Do not
                // peel a top-level `data` key — that broke confirm/ingest parity
                // when identity lived under `data.*`. The DeviceDiscovered
                // envelope still wraps this value as its own `data` field;
                // process_device_discovered unwraps that envelope once.
                let (sample_data, is_binary, data_format) =
                    if let Ok(json_data) = serde_json::from_slice::<serde_json::Value>(&payload) {
                        (json_data, false, "json")
                    } else {
                        (serde_json::json!(BASE64.encode(&payload)), true, "base64")
                    };

                tracing::info!(
                    topic = %topic,
                    claimed = %identity.device_id,
                    candidate = %auto_device_id,
                    association_exception = is_association_exception,
                    "MQTT discovery event only (no formal telemetry) — pending human confirm"
                );

                {
                    let adapter_id = config.name.clone();
                    let mut sample = serde_json::json!({
                        "device_id": auto_device_id,
                        "claimed_identity": identity.device_id,
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
                        if is_association_exception {
                            obj.insert(
                                MQTT_ASSOCIATION_EXCEPTION_KEY.to_string(),
                                serde_json::Value::Bool(true),
                            );
                            let kind = association_exception_kind
                                .unwrap_or(AssociationExceptionKind::Conflict);
                            obj.insert(
                                MQTT_ASSOCIATION_KIND_KEY.to_string(),
                                serde_json::Value::String(kind.as_str().to_string()),
                            );
                            obj.insert(
                                MQTT_CLAIMED_IDENTITY_KEY.to_string(),
                                serde_json::Value::String(identity.device_id.clone()),
                            );
                            obj.insert(
                                MQTT_PENDING_CANDIDATE_ID_KEY.to_string(),
                                serde_json::Value::String(auto_device_id.clone()),
                            );
                            let mut blocked: Vec<String> =
                                approved_blocked.iter().cloned().collect();
                            if blocked.is_empty() {
                                blocked.push(identity.device_id.clone());
                            }
                            blocked.sort();
                            obj.insert(
                                MQTT_BLOCKED_DEVICE_IDS_KEY.to_string(),
                                serde_json::Value::Array(
                                    blocked.into_iter().map(serde_json::Value::String).collect(),
                                ),
                            );
                        }
                    }

                    let discovered = crate::adapter::DiscoveredDeviceInfo {
                        device_id: auto_device_id.clone(),
                        device_type: "unknown".to_string(),
                        name: None,
                        endpoint: Some(topic.to_string()),
                        capabilities: vec![],
                        timestamp: chrono::Utc::now().timestamp(),
                        metadata: meta.clone(),
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
                            if let Some(meta_obj) = meta.as_object() {
                                for key in [
                                    MQTT_ASSOCIATION_EXCEPTION_KEY,
                                    MQTT_ASSOCIATION_KIND_KEY,
                                    MQTT_BLOCKED_DEVICE_IDS_KEY,
                                    MQTT_CLAIMED_IDENTITY_KEY,
                                    MQTT_PENDING_CANDIDATE_ID_KEY,
                                ] {
                                    if let Some(v) = meta_obj.get(key) {
                                        obj.insert(key.to_string(), v.clone());
                                    }
                                }
                            }
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
                // session_ready is applied in the poll loop (before this handler)
                // via apply_session_ready_from_notification — ConnAck here is log-only.
                info!(
                    "MQTT broker {} connection acknowledged - session present: {}, return={:?}",
                    broker_id, connack.session_present, connack.code
                );
            }
            rumqttc::Event::Incoming(rumqttc::Packet::SubAck(suback)) => {
                // Correlation / confirmed-set update happens in the poll loop via
                // apply_subscription_events (waiter complete only — no wait here).
                info!(
                    "MQTT broker {} subscription acknowledged - packet id: {}, granted QoS: {:?}",
                    broker_id, suback.pkid, suback.return_codes
                );
            }
            _ => {}
        }
    }
}
