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
