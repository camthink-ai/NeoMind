//! Unified MQTT association bind: config + subscribe switch as one operation.
//!
//! Business sequence (confirm path):
//! confirm target + identity + broker + topic
//! → validate sample/config (caller)
//! → apply config + subscribe switch (this module)
//! → return actual result
//! → consume draft only on full success (caller)

use std::sync::Arc;

use super::adapter::DeviceAdapter;
use super::mdl::DeviceError;
use super::registry::DeviceConfig;
use super::service::DeviceService;
use crate::adapters::mqtt::{MQTT_IDENTITY_FIELD_KEY, MQTT_IDENTITY_SOURCE_KEY};

/// Input for the unified association bind / subscribe-switch operation.
#[derive(Debug, Clone)]
pub struct AssociationBindInput {
    pub device_id: String,
    pub name: String,
    pub device_type: String,
    pub adapter_type: String,
    /// Confirmed draft source adapter (cross-broker must use this, not stale).
    pub adapter_id: Option<String>,
    pub telemetry_topic: String,
    pub identity_source: String,
    pub identity_field: Option<String>,
}

/// What the bind actually achieved (honest result for UI / retry).
#[derive(Debug, Clone)]
pub struct AssociationBindOutcome {
    pub device_id: String,
    pub was_existing: bool,
    pub adapter_id: Option<String>,
    pub telemetry_topic: String,
    pub config_applied: bool,
    pub subscribe_ok: bool,
    /// Adapters that were unsubscribed (old source on cross-broker / topic move).
    pub cleared_adapter_ids: Vec<String>,
    /// Adapter that received the new subscribe.
    pub subscribed_adapter_id: Option<String>,
}

impl DeviceService {
    /// Apply association config and switch runtime subscriptions.
    ///
    /// - Preserves unrelated connection fields (command topic, non-identity extra).
    /// - Uses the confirmed `adapter_id` (clears old adapter on cross-broker).
    /// - Shared-topic unsubscribe only drops this device (adapter `unsubscribe_device`).
    /// - Does **not** mark the device online / Connected.
    /// - On adapter missing or subscribe failure: config may already be persisted;
    ///   returns [`DeviceError::BindIncomplete`] so the caller keeps the draft and
    ///   surfaces a retry path (idempotent — no double-register).
    pub async fn apply_association_bind(
        &self,
        input: AssociationBindInput,
    ) -> Result<AssociationBindOutcome, DeviceError> {
        if input.adapter_type == "extension" {
            return Err(DeviceError::InvalidParameter(
                "association bind is not supported for extension-managed devices".into(),
            ));
        }

        let existing = self.get_device(&input.device_id);
        let was_existing = existing.is_some();
        let previous_adapter_id = existing.as_ref().and_then(|d| d.adapter_id.clone());

        let mut connection_config = existing
            .as_ref()
            .map(|d| d.connection_config.clone())
            .unwrap_or_default();

        // Only change topic + identity rule; keep command_topic / json_path /
        // entity_id and non-identity extra keys.
        connection_config.telemetry_topic = Some(input.telemetry_topic.clone());
        connection_config.extra.insert(
            MQTT_IDENTITY_SOURCE_KEY.to_string(),
            serde_json::Value::String(input.identity_source.clone()),
        );
        match &input.identity_field {
            Some(field) if !field.is_empty() => {
                connection_config.extra.insert(
                    MQTT_IDENTITY_FIELD_KEY.to_string(),
                    serde_json::Value::String(field.clone()),
                );
            }
            _ => {
                connection_config.extra.remove(MQTT_IDENTITY_FIELD_KEY);
            }
        }

        let device_config = DeviceConfig {
            device_id: input.device_id.clone(),
            name: input.name.clone(),
            device_type: input.device_type.clone(),
            adapter_type: input.adapter_type.clone(),
            connection_config,
            // Confirmed draft source wins (cross-broker must not keep stale id).
            adapter_id: input.adapter_id.clone(),
            last_seen: existing
                .as_ref()
                .map(|d| d.last_seen)
                .unwrap_or_else(|| chrono::Utc::now().timestamp()),
            offline_timeout_secs: existing.as_ref().and_then(|d| d.offline_timeout_secs),
        };

        // Persist first so retry after a subscribe miss finds the device and
        // updates in place (no double-register via register's name-only branch).
        if was_existing {
            self.registry
                .update_device(&input.device_id, device_config)
                .await?;
        } else {
            self.registry.register_device(device_config).await?;
            // Track status as Disconnected — config ≠ online.
            {
                let mut status_map = self.device_status.write().await;
                status_map
                    .entry(input.device_id.clone())
                    .or_insert_with(|| super::service::DeviceStatus {
                        status: super::adapter::ConnectionStatus::Disconnected,
                        last_seen: chrono::Utc::now().timestamp(),
                        adapter_id: input.adapter_id.clone(),
                        transport_connected: false,
                        transport_changed_at: 0,
                    });
            }
        }

        let config_applied = true;

        // Clear old adapter runtime maps when source broker changes (or always
        // clear previous matching adapters before re-subscribe on the target).
        let mut cleared_adapter_ids = Vec::new();
        let old_to_clear = collect_adapters_to_clear(
            self,
            &input.adapter_type,
            previous_adapter_id.as_deref(),
            input.adapter_id.as_deref(),
        )
        .await;

        for (adapter_id, adapter) in &old_to_clear {
            if let Err(e) = adapter.unsubscribe_device(&input.device_id).await {
                tracing::warn!(
                    device_id = %input.device_id,
                    adapter = %adapter_id,
                    error = %e,
                    "Failed to unsubscribe device from previous adapter during association bind"
                );
                // Continue — subscribe on the new target is the success criterion;
                // a soft unsubscribe miss must not fake overall success later.
            } else {
                cleared_adapter_ids.push(adapter_id.clone());
            }
        }

        // Target adapter(s) for the confirmed source.
        let target = match_adapters(self, &input.adapter_type, input.adapter_id.as_deref()).await;

        if target.is_empty() {
            let hint = format!(
                "Config for device '{}' is saved (topic='{}'). Ensure adapter {:?} is registered/connected, then retry confirm-association — retry will not double-register.",
                input.device_id, input.telemetry_topic, input.adapter_id
            );
            return Err(DeviceError::BindIncomplete {
                message: format!(
                    "No adapter found for device '{}' (type={}, adapter_id={:?}); subscription not switched",
                    input.device_id, input.adapter_type, input.adapter_id
                ),
                config_applied,
                retry_hint: hint,
            });
        }

        let mut subscribed_adapter_id = None;
        let mut last_err: Option<String> = None;
        for (adapter_id, adapter) in &target {
            // If we did not already unsubscribe this same adapter above (same
            // broker topic move), drop old topic maps before rebuild.
            let already_cleared = cleared_adapter_ids.iter().any(|id| id == adapter_id);
            if !already_cleared {
                if let Err(e) = adapter.unsubscribe_device(&input.device_id).await {
                    tracing::warn!(
                        device_id = %input.device_id,
                        adapter = %adapter_id,
                        error = %e,
                        "Failed to clear prior subscription before association re-subscribe"
                    );
                } else {
                    cleared_adapter_ids.push(adapter_id.clone());
                }
            }
            match adapter.subscribe_device(&input.device_id).await {
                Ok(()) => {
                    subscribed_adapter_id = Some(adapter_id.clone());
                }
                Err(e) => {
                    last_err = Some(format!("{adapter_id}: {e}"));
                }
            }
        }

        if subscribed_adapter_id.is_none() {
            let detail = last_err.unwrap_or_else(|| "unknown subscribe error".into());
            let hint = format!(
                "Config for device '{}' is saved (topic='{}'). Fix adapter/broker connectivity and retry confirm-association — retry updates in place, does not double-register.",
                input.device_id, input.telemetry_topic
            );
            return Err(DeviceError::BindIncomplete {
                message: format!(
                    "Subscribe switch failed for device '{}': {detail}",
                    input.device_id
                ),
                config_applied,
                retry_hint: hint,
            });
        }

        Ok(AssociationBindOutcome {
            device_id: input.device_id,
            was_existing,
            adapter_id: input.adapter_id,
            telemetry_topic: input.telemetry_topic,
            config_applied: true,
            subscribe_ok: true,
            cleared_adapter_ids,
            subscribed_adapter_id,
        })
    }
}

async fn match_adapters(
    service: &DeviceService,
    adapter_type: &str,
    target_adapter_id: Option<&str>,
) -> Vec<(String, Arc<dyn DeviceAdapter>)> {
    let adapters = service.adapters.read().await;
    let mut result = Vec::new();
    for (adapter_id, adapter) in adapters.iter() {
        let is_match = if let Some(target_id) = target_adapter_id {
            adapter.adapter_type() == adapter_type && adapter_id == target_id
        } else {
            adapter.adapter_type() == adapter_type
        };
        if is_match {
            result.push((adapter_id.clone(), adapter.clone()));
        }
    }
    result
}

/// Adapters that held the previous binding and must be cleared when moving.
async fn collect_adapters_to_clear(
    service: &DeviceService,
    adapter_type: &str,
    previous_adapter_id: Option<&str>,
    new_adapter_id: Option<&str>,
) -> Vec<(String, Arc<dyn DeviceAdapter>)> {
    // Same explicit adapter → cleared in the subscribe loop instead.
    if previous_adapter_id.is_some() && previous_adapter_id == new_adapter_id {
        return Vec::new();
    }
    // No previous binding.
    if previous_adapter_id.is_none() && new_adapter_id.is_some() {
        // Still clear any same-type adapters that might hold stale maps when
        // moving from broadcast/None → explicit id.
        let adapters = match_adapters(service, adapter_type, None).await;
        return adapters
            .into_iter()
            .filter(|(id, _)| Some(id.as_str()) != new_adapter_id)
            .collect();
    }
    match_adapters(service, adapter_type, previous_adapter_id).await
}

#[cfg(test)]
mod merge_tests {
    use super::*;
    use crate::registry::ConnectionConfig;
    use serde_json::json;

    #[test]
    fn merge_preserves_command_topic_and_non_identity_extra() {
        let mut base = ConnectionConfig::default();
        base.command_topic = Some("device/gw/B/downlink".into());
        base.extra.insert("ssid".into(), json!("plant-wifi"));
        base.extra
            .insert(MQTT_IDENTITY_SOURCE_KEY.into(), json!("topic_strong"));

        // Mirror apply_association_bind's preserve rules.
        let mut cfg = base.clone();
        cfg.telemetry_topic = Some("gateway/data".into());
        cfg.extra
            .insert(MQTT_IDENTITY_SOURCE_KEY.to_string(), json!("auto_payload"));
        cfg.extra
            .insert(MQTT_IDENTITY_FIELD_KEY.to_string(), json!("data.sn"));

        assert_eq!(cfg.command_topic.as_deref(), Some("device/gw/B/downlink"));
        assert_eq!(cfg.extra.get("ssid"), Some(&json!("plant-wifi")));
        assert_eq!(
            cfg.extra.get(MQTT_IDENTITY_SOURCE_KEY),
            Some(&json!("auto_payload"))
        );
        assert_eq!(
            cfg.extra.get(MQTT_IDENTITY_FIELD_KEY),
            Some(&json!("data.sn"))
        );
        assert_eq!(cfg.telemetry_topic.as_deref(), Some("gateway/data"));
    }
}
