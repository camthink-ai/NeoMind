//! Unified MQTT association bind: config + subscribe switch as one operation.
//!
//! Business sequence (confirm path):
//! confirm target + identity + broker + topic
//! → validate sample/config (caller)
//! → apply config + subscribe switch (this module)
//! → return actual result
//! → consume draft only on full success (caller)

use std::sync::Arc;

use tokio::sync::Mutex;

use super::adapter::DeviceAdapter;
use super::mdl::DeviceError;
use super::registry::DeviceConfig;
use super::service::DeviceService;
use crate::adapters::mqtt::{MQTT_IDENTITY_FIELD_KEY, MQTT_IDENTITY_SOURCE_KEY};

/// Persisted list of adapter ids whose unsubscribe still needs to finish after
/// a prior incomplete association switch (config may already point at the new
/// adapter_id). Cleared only when every listed unsubscribe succeeds.
const MQTT_PENDING_UNSUB_ADAPTERS_KEY: &str = "mqtt_pending_unsub_adapters";

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
    /// True when every required old-source unsubscribe completed.
    pub old_source_cleanup_ok: bool,
}

impl DeviceService {
    /// Per-device lock so concurrent association binds / confirms for the same
    /// real device id cannot interleave config + subscribe switches.
    pub(crate) fn association_bind_lock_for(&self, device_id: &str) -> Arc<Mutex<()>> {
        self.association_bind_locks
            .entry(device_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// Persist / clear `mqtt_pending_unsub_adapters` after overwrite + cleanup.
    /// Propagates save failure as [`DeviceError::BindIncomplete`].
    async fn persist_pending_unsub_adapters(
        &self,
        device_id: &str,
        pending_unsub: &[String],
        config_applied: bool,
        telemetry_topic: &str,
    ) -> Result<(), DeviceError> {
        #[cfg(any(test, feature = "test-utils"))]
        if self
            .fail_next_pending_unsub_persist
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Err(DeviceError::BindIncomplete {
                message: format!(
                    "Failed to persist pending old-source cleanup for device '{device_id}' (forced test save failure)"
                ),
                config_applied,
                retry_hint: format!(
                    "Config for device '{device_id}' may already point at the new source (topic='{telemetry_topic}'). Retry confirm-association after storage recovers — pending cleanup must be saved; retry will not double-register."
                ),
            });
        }

        let Some(mut dev) = self.get_device(device_id) else {
            return Err(DeviceError::BindIncomplete {
                message: format!(
                    "Device '{device_id}' missing while persisting pending old-source cleanup"
                ),
                config_applied,
                retry_hint: format!(
                    "Re-run confirm-association for '{device_id}' (topic='{telemetry_topic}') — retry will not double-register."
                ),
            });
        };
        if pending_unsub.is_empty() {
            dev.connection_config
                .extra
                .remove(MQTT_PENDING_UNSUB_ADAPTERS_KEY);
        } else {
            dev.connection_config.extra.insert(
                MQTT_PENDING_UNSUB_ADAPTERS_KEY.to_string(),
                serde_json::Value::Array(
                    pending_unsub
                        .iter()
                        .map(|id| serde_json::Value::String(id.clone()))
                        .collect(),
                ),
            );
        }
        self.registry
            .update_device(device_id, dev)
            .await
            .map_err(|e| DeviceError::BindIncomplete {
                message: format!(
                    "Failed to persist pending old-source cleanup for device '{device_id}': {e}"
                ),
                config_applied,
                retry_hint: format!(
                    "Config for device '{device_id}' may already point at the new source (topic='{telemetry_topic}'). Fix storage and retry confirm-association — pending cleanup must be saved; retry will not double-register."
                ),
            })?;
        Ok(())
    }

    /// Apply association config and switch runtime subscriptions.
    ///
    /// - Preserves unrelated connection fields (command topic, non-identity extra).
    /// - Uses the confirmed `adapter_id` (clears old adapter on cross-broker).
    /// - Shared-topic unsubscribe only drops this device (adapter `unsubscribe_device`).
    /// - Does **not** mark the device online / Connected.
    /// - On adapter missing, subscribe failure, **or incomplete old-source cleanup**:
    ///   config may already be persisted; returns [`DeviceError::BindIncomplete`]
    ///   so the caller keeps the draft and surfaces a retry path (idempotent —
    ///   no double-register). New subscribe alone is not full switch success.
    /// - When overwriting source, seeds `mqtt_pending_unsub_adapters` on the
    ///   first save and re-persists on every exit path (including early returns).
    pub async fn apply_association_bind(
        &self,
        input: AssociationBindInput,
    ) -> Result<AssociationBindOutcome, DeviceError> {
        if input.adapter_type == "extension" {
            return Err(DeviceError::InvalidParameter(
                "association bind is not supported for extension-managed devices".into(),
            ));
        }

        // Serialize same-device binds (cross-draft / retry races).
        let device_lock = self.association_bind_lock_for(&input.device_id);
        let _device_guard = device_lock.lock().await;

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

        // Outstanding + newly overwritten sources must survive early returns
        // after adapter_id already points at the new source. Only seed when the
        // previous adapter is actually registered — a stale adapter_id with no
        // runtime maps is not a cleanup failure (nothing to unsubscribe).
        let mut pending_unsub: Vec<String> = existing
            .as_ref()
            .and_then(|d| {
                d.connection_config
                    .extra
                    .get(MQTT_PENDING_UNSUB_ADAPTERS_KEY)
            })
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(prev) = previous_adapter_id.as_deref() {
            if Some(prev) != input.adapter_id.as_deref()
                && !pending_unsub.iter().any(|id| id == prev)
            {
                let adapters = self.adapters.read().await;
                if adapters.contains_key(prev) {
                    pending_unsub.push(prev.to_string());
                }
            }
        }
        if pending_unsub.is_empty() {
            connection_config
                .extra
                .remove(MQTT_PENDING_UNSUB_ADAPTERS_KEY);
        } else {
            connection_config.extra.insert(
                MQTT_PENDING_UNSUB_ADAPTERS_KEY.to_string(),
                serde_json::Value::Array(
                    pending_unsub
                        .iter()
                        .map(|id| serde_json::Value::String(id.clone()))
                        .collect(),
                ),
            );
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
        // When overwriting source, pending old adapters are already seeded above.
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
        let mut cleanup_errors: Vec<String> = Vec::new();

        let mut old_to_clear = collect_adapters_to_clear(
            self,
            &input.adapter_type,
            previous_adapter_id.as_deref(),
            input.adapter_id.as_deref(),
        )
        .await;
        // Retry path: config may already list the new adapter_id while prior
        // old-source unsubscribes are still outstanding — finish those too.
        if !pending_unsub.is_empty() {
            let adapters = self.adapters.read().await;
            let mut orphaned = Vec::new();
            for id in &pending_unsub {
                if old_to_clear
                    .iter()
                    .any(|(existing_id, _)| existing_id == id)
                {
                    continue;
                }
                if let Some(adapter) = adapters.get(id) {
                    old_to_clear.push((id.clone(), adapter.clone()));
                } else {
                    // No runtime adapter → nothing left to unsubscribe; drop.
                    tracing::warn!(
                        device_id = %input.device_id,
                        adapter = %id,
                        "Pending old-source cleanup adapter not registered; dropping orphan pending id"
                    );
                    orphaned.push(id.clone());
                }
            }
            for id in orphaned {
                pending_unsub.retain(|p| p != &id);
            }
        }

        for (adapter_id, adapter) in &old_to_clear {
            if let Err(e) = adapter.unsubscribe_device(&input.device_id).await {
                tracing::warn!(
                    device_id = %input.device_id,
                    adapter = %adapter_id,
                    error = %e,
                    "Failed to unsubscribe device from previous adapter during association bind"
                );
                cleanup_errors.push(format!("{adapter_id}: {e}"));
                if !pending_unsub.iter().any(|id| id == adapter_id) {
                    pending_unsub.push(adapter_id.clone());
                }
            } else {
                cleared_adapter_ids.push(adapter_id.clone());
                pending_unsub.retain(|id| id != adapter_id);
            }
        }

        // Target adapter(s) for the confirmed source.
        let target = match_adapters(self, &input.adapter_type, input.adapter_id.as_deref()).await;

        if target.is_empty() {
            self.persist_pending_unsub_adapters(
                &input.device_id,
                &pending_unsub,
                config_applied,
                &input.telemetry_topic,
            )
            .await?;
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
                    cleanup_errors.push(format!("{adapter_id}: {e}"));
                    if !pending_unsub.iter().any(|id| id == adapter_id) {
                        pending_unsub.push(adapter_id.clone());
                    }
                } else {
                    cleared_adapter_ids.push(adapter_id.clone());
                    pending_unsub.retain(|id| id != adapter_id);
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
            self.persist_pending_unsub_adapters(
                &input.device_id,
                &pending_unsub,
                config_applied,
                &input.telemetry_topic,
            )
            .await?;
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

        // Persist / clear the pending-unsub marker so retries finish cleanup
        // even after adapter_id already points at the new source.
        self.persist_pending_unsub_adapters(
            &input.device_id,
            &pending_unsub,
            config_applied,
            &input.telemetry_topic,
        )
        .await?;

        // New subscribe alone is not a full switch — old source must be cleared.
        if !cleanup_errors.is_empty() {
            let detail = cleanup_errors.join("; ");
            let hint = format!(
                "Config for device '{}' is saved and the new adapter subscribed (topic='{}'), but old-source cleanup is incomplete ({detail}). Retry confirm-association to finish clearing previous maps — retry will not double-register.",
                input.device_id, input.telemetry_topic
            );
            return Err(DeviceError::BindIncomplete {
                message: format!(
                    "Association switch incomplete for device '{}': old source cleanup failed ({detail})",
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
            old_source_cleanup_ok: true,
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
    // Outstanding cross-broker cleanup uses `mqtt_pending_unsub_adapters`.
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
        let base = ConnectionConfig {
            command_topic: Some("device/gw/B/downlink".into()),
            extra: [
                ("ssid".into(), json!("plant-wifi")),
                (MQTT_IDENTITY_SOURCE_KEY.into(), json!("topic_strong")),
            ]
            .into_iter()
            .collect(),
            ..ConnectionConfig::default()
        };

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
