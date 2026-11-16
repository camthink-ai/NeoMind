//! `adapter` — split from the former mqtt.rs monolith.

use super::*;

use crate::adapter::{AdapterError, AdapterResult, ConnectionStatus, DeviceAdapter, DeviceEvent};

use async_trait::async_trait;

use futures::Stream;

use std::pin::Pin;

use std::sync::Arc;

use tracing::{info, warn};

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
            loop {
                // Lagged is RECOVERABLE: the receiver has skipped the missed
                // backlog and continues from the oldest live message. Ending
                // the stream here (the old `while let Ok`) permanently cut
                // this adapter's telemetry off from the event bus after one
                // burst — storage kept writing while rules, last_seen,
                // data-push and the frontend went blind, with no recovery
                // until restart. Closed is the only terminal error.
                match rx.recv().await {
                    Ok(event) => yield event,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(
                            skipped = n,
                            "device event subscriber lagged — continuing from oldest live event"
                        );
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
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

        // Record topic + payload for short-TTL echo suppression. Matching
        // topic alone would suppress legitimate uplinks on a shared command
        // topic; we require an exact payload match within OUTBOUND_ECHO_TTL.
        {
            let mut outbound = self.outbound_echoes.write().await;
            outbound.record(topic.clone(), payload.as_bytes().to_vec());
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

        // Re-registration after delete clears the teardown ban.
        {
            let mut banned = self.uncollectable.write().await;
            banned.remove(device_id);
        }

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
            // CT-Prov / some firmwares publish to the bare `device/{type}/{id}`
            // path (exact `mqtt.topic` without `/uplink`). Alias-map + subscribe
            // so telemetry associates to the registered device instead of
            // spawning a draft under device_id=<type>.
            let bare_prefix = topic
                .strip_suffix("/uplink")
                .map(|s| s.to_string())
                .filter(|s| s.starts_with("device/"));
            if let Some(ref bare) = bare_prefix {
                if bare != &topic {
                    let _ = self.subscribe_topic(bare).await;
                    info!(
                        "Also subscribed to CT-Prov bare topic alias: {} -> {}",
                        bare, device_id
                    );
                }
            }
            // Store topic→device association (shared-topic safe: one topic, many devices).
            {
                let mut mapping = self.topic_to_device.write().await;
                topic_map_insert(&mut mapping, &topic, device_id);
                if let Some(bare) = bare_prefix {
                    topic_map_insert(&mut mapping, &bare, device_id);
                }
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
        // Unified sync boundary with CT-Prov learn: hold `uncollectable` write
        // through topic-map clear so a concurrent learn cannot observe "not
        // banned" then re-insert after we emptied the map. Registry removal
        // happens later in DeviceService — mid-window uplinks must not restore
        // formal routing just because the device is still registered.
        let cleared = {
            let mut banned = self.uncollectable.write().await;
            banned.insert(device_id.to_string());
            // Drop ALL learned topic associations for this device (not only the
            // configured telemetry_topic — CT-Prov / aliases may have added more).
            let mut mapping = self.topic_to_device.write().await;
            topic_map_remove_device_all(&mut mapping, device_id)
        };

        // Type / metric caches are advisory for extraction; clear outside the
        // ban+map boundary (lock order stays uncollectable → topic_to_device).
        {
            let mut types = self.device_types.write().await;
            types.remove(device_id);
        }
        {
            let mut cache = self.metric_cache.write().await;
            cache.remove(device_id);
        }

        for (topic, still_used) in &cleared {
            if !still_used {
                if let Err(e) = self.unsubscribe_topic(topic).await {
                    warn!(
                        "Failed to unsubscribe emptied topic {} after removing {}: {}",
                        topic, device_id, e
                    );
                } else {
                    info!(
                        "Unsubscribed from topic {} (no remaining devices after removing {})",
                        topic, device_id
                    );
                }
            } else {
                info!(
                    "Removed device {} from shared topic {}; keeping MQTT subscribe",
                    device_id, topic
                );
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
