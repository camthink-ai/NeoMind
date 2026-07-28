//! Event processing services for rule engine and transform engine.
//!
//! This module provides background services that subscribe to events from the EventBus
//! and trigger actions in the rule engine and transform engine.

use std::collections::HashMap;
use std::sync::Arc;

use crate::automation::{store::SharedAutomationStore, TransformEngine};
use neomind_core::eventbus::EventBus;
use neomind_core::{MetricValue, NeoMindEvent};
use neomind_devices::DeviceRegistry;
use neomind_rules::RuleEngine;

/// Transform event service.
///
/// Subscribes to device metric events and processes transforms to generate virtual metrics.
pub struct TransformEventService {
    event_bus: Arc<EventBus>,
    transform_engine: Arc<TransformEngine>,
    automation_store: Arc<SharedAutomationStore>,
    device_registry: Arc<DeviceRegistry>,
    time_series_storage: Arc<neomind_devices::TimeSeriesStorage>,
    value_provider: Arc<neomind_rules::UnifiedValueProvider>,
    rule_engine: Arc<RuleEngine>,
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl TransformEventService {
    /// Create a new transform event service.
    pub fn new(
        event_bus: Arc<EventBus>,
        transform_engine: Arc<TransformEngine>,
        automation_store: Arc<SharedAutomationStore>,
        time_series_storage: Arc<neomind_devices::TimeSeriesStorage>,
        device_registry: Arc<neomind_devices::DeviceRegistry>,
        value_provider: Arc<neomind_rules::UnifiedValueProvider>,
        rule_engine: Arc<RuleEngine>,
    ) -> Self {
        Self {
            event_bus,
            transform_engine,
            automation_store,
            device_registry,
            time_series_storage,
            value_provider,
            rule_engine,
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Start the service.
    pub fn start(&self) -> Arc<std::sync::atomic::AtomicBool> {
        if self
            .running
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            )
            .is_ok()
        {
            let event_bus = self.event_bus.clone();
            let transform_engine = self.transform_engine.clone();
            let automation_store = self.automation_store.clone();
            let device_registry = self.device_registry.clone();
            let time_series_storage = self.time_series_storage.clone();
            let value_provider = self.value_provider.clone();
            let rule_engine = self.rule_engine.clone();

            tokio::spawn(async move {
                let mut rx = event_bus.filter().device_events();
                tracing::info!("Transform event service started - subscribing to device events");

                // Track pending device data and debounce timers
                // (device_id -> (raw_data, latest_timestamp, timer_handle))
                let mut device_raw_data: HashMap<String, serde_json::Value> = HashMap::new();
                let mut device_latest_ts: HashMap<String, i64> = HashMap::new();
                let mut device_timers: HashMap<String, tokio::task::JoinHandle<()>> =
                    HashMap::new();

                // Throttle map for execution-stat persistence: transform_id -> last persist instant.
                // The first execution of a transform flushes immediately (so last_executed appears
                // in the UI without delay); subsequent ones are coalesced to at most one write per
                // FLUSH_INTERVAL per transform, bounding write amplification on high-frequency telemetry.
                let exec_flush: Arc<std::sync::Mutex<HashMap<String, std::time::Instant>>> =
                    Arc::new(std::sync::Mutex::new(HashMap::new()));
                const FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

                // Debounce delay: wait this long after the last metric before processing
                // This allows multiple metrics from the same device to be collected and processed together
                const DEBOUNCE_MS: u64 = 100; // 100ms debounce window

                while let Some((event, _metadata)) = rx.recv().await {
                    if let NeoMindEvent::DeviceMetric {
                        device_id,
                        metric,
                        value,
                        timestamp,
                        quality: _,
                        is_virtual,
                        ..
                    } = event
                    {
                        // Skip virtual metrics (transform outputs, extension writes)
                        // to prevent feedback loops.
                        if NeoMindEvent::is_virtual_device_metric(is_virtual, &metric) {
                            continue;
                        }

                        // Log incoming device metric for debugging (debug level to reduce noise)
                        tracing::debug!(
                            device_id = %device_id,
                            metric = %metric,
                            timestamp = timestamp,
                            "Received device metric event for transform processing"
                        );

                        // Update the latest timestamp for this device
                        device_latest_ts.insert(device_id.clone(), timestamp);

                        // Build or update the device's raw data structure
                        let device_entry =
                            device_raw_data.entry(device_id.clone()).or_insert_with(|| {
                                serde_json::json!({
                                    "device_id": device_id,
                                    "timestamp": timestamp,
                                    "values": {}
                                })
                            });

                        // Update the device data with the new metric
                        if let Some(obj) = device_entry.as_object_mut() {
                            // Update top-level timestamp to latest
                            obj.insert(
                                "timestamp".to_string(),
                                serde_json::Value::Number(timestamp.into()),
                            );

                            // Update values object
                            let values =
                                obj.entry("values").or_insert_with(|| serde_json::json!({}));
                            if let Some(values_obj) = values.as_object_mut() {
                                let json_value = match value {
                                    MetricValue::Float(f) => serde_json::json!(f),
                                    MetricValue::Integer(i) => serde_json::json!(i),
                                    MetricValue::Boolean(b) => serde_json::json!(b),
                                    MetricValue::String(s) => serde_json::json!(s),
                                    MetricValue::Json(j) => j,
                                };

                                // Store with full path (e.g., "values.temperature")
                                values_obj.insert(metric.clone(), json_value.clone());

                                // Also store at top level for simpler transforms
                                obj.insert(metric.clone(), json_value);
                            }
                        }

                        // Get device type from registry (cached for the debounce task)
                        let device_type: Option<String> = device_registry
                            .get_device(&device_id)
                            .map(|d| d.device_type.clone());

                        // Cancel existing timer for this device if any
                        if let Some(existing_timer) = device_timers.remove(&device_id) {
                            existing_timer.abort();
                        }

                        // Clone needed values for the async task
                        let device_id_clone = device_id.clone();
                        let event_bus_clone = event_bus.clone();
                        let transform_engine_clone = transform_engine.clone();
                        let automation_store_clone = automation_store.clone();
                        let device_entry_clone = device_entry.clone();
                        let device_type_clone = device_type.clone();
                        let time_series_storage_inner = time_series_storage.clone();
                        let exec_flush_clone = exec_flush.clone();
                        let value_provider_clone = value_provider.clone();
                        let rule_engine_clone = rule_engine.clone();

                        // Schedule a new debounce timer
                        let timer_handle = tokio::spawn(async move {
                            // Wait for the debounce delay
                            tokio::time::sleep(tokio::time::Duration::from_millis(DEBOUNCE_MS))
                                .await;

                            tracing::debug!(
                                device_id = %device_id_clone,
                                "Debounce timer expired, processing device data"
                            );

                            // Load all enabled transforms
                            let transforms = match automation_store_clone.list_automations().await {
                                Ok(all) => all
                                    .into_iter()
                                    .filter(|t| t.metadata.enabled)
                                    .collect::<Vec<_>>(),
                                Err(e) => {
                                    tracing::debug!("Failed to load transforms: {}", e);
                                    return;
                                }
                            };

                            // Skip if no transforms
                            if transforms.is_empty() {
                                return;
                            }

                            // Filter transforms to only those applicable to this device
                            let applicable_transforms: Vec<_> = transforms
                                .into_iter()
                                .filter(|t| {
                                    t.applies_to_device(
                                        &device_id_clone,
                                        device_type_clone.as_deref(),
                                    )
                                })
                                .collect();

                            if applicable_transforms.is_empty() {
                                return;
                            }

                            // Process the device data through transforms
                            match transform_engine_clone
                                .process_device_data(
                                    &applicable_transforms,
                                    &device_id_clone,
                                    device_type_clone.as_deref(),
                                    &device_entry_clone,
                                )
                                .await
                            {
                                Ok(result) => {
                                    if !result.metrics.is_empty() {
                                        tracing::debug!(
                                            device_id = %device_id_clone,
                                            device_type = ?device_type_clone,
                                            metric_count = result.metrics.len(),
                                            "Transform processed device data (debounced)"
                                        );

                                        // Mark transforms that produced output as executed
                                        // (updates execution_count + last_executed so the UI reflects activity).
                                        // Done before the metrics are moved/consumed below.
                                        // Persistence is throttled: the first execution flushes immediately
                                        // (so last_executed shows up at once); subsequent ones within
                                        // FLUSH_INTERVAL are skipped to bound write amplification.
                                        let now = std::time::Instant::now();
                                        let executed_ids: std::collections::HashSet<&str> = result
                                            .metrics
                                            .iter()
                                            .filter_map(|m| m.transform_id.as_deref())
                                            .collect();
                                        for tid in executed_ids {
                                            let should_flush = {
                                                let mut map = exec_flush_clone
                                                    .lock()
                                                    .expect("exec_flush poisoned");
                                                let last = map.get(tid).copied();
                                                let do_flush = last.is_none_or(|t| {
                                                    now.duration_since(t) >= FLUSH_INTERVAL
                                                });
                                                if do_flush {
                                                    map.insert(tid.to_string(), now);
                                                }
                                                do_flush
                                            };
                                            if !should_flush {
                                                continue;
                                            }
                                            if let Ok(Some(mut automation)) =
                                                automation_store_clone.get_automation(tid).await
                                            {
                                                automation.metadata.mark_executed();
                                                if let Err(e) = automation_store_clone
                                                    .save_automation(&automation)
                                                    .await
                                                {
                                                    tracing::warn!(
                                                        transform_id = %tid,
                                                        error = %e,
                                                        "Failed to persist last_executed"
                                                    );
                                                }
                                            }
                                        }

                                        // Publish transformed metrics back to event bus AND store to telemetry
                                        for transformed_metric in result.metrics {
                                            // Use storage_device_id() ("transform:{transform_id}") as device_id
                                            // so the event is consistent with time-series storage namespace,
                                            // frontend fetch path, and rule engine data source filters.
                                            // The transform trigger handler (above) skips is_virtual metrics,
                                            // so this won't cause feedback loops.
                                            let storage_device_id =
                                                transformed_metric.storage_device_id();
                                            let _ = event_bus_clone
                                                .publish(NeoMindEvent::DeviceMetric {
                                                    device_id: storage_device_id.clone(),
                                                    metric: transformed_metric.metric.clone(),
                                                    value: transformed_metric.value.clone(),
                                                    timestamp: transformed_metric.timestamp,
                                                    quality: transformed_metric.quality,
                                                    is_virtual: Some(true),
                                                })
                                                .await;

                                            // Also publish in the original device namespace
                                            // (device:{device_id}:{metric}) so event-driven consumers that
                                            // select the device's virtual metrics — e.g. data-push targets
                                            // filtering on `device:9999:virtual.*` — actually receive them.
                                            // The dual-write below stores these under device:{id} for REST
                                            // discovery, but the event above was only published under
                                            // transform:{id}, so device-namespace filters never matched and
                                            // the virtual metrics were never pushed. is_virtual keeps this
                                            // feedback-safe (transform trigger + frontend WS both skip
                                            // is_virtual events).
                                            let _ = event_bus_clone
                                                .publish(NeoMindEvent::DeviceMetric {
                                                    device_id: device_id_clone.clone(),
                                                    metric: transformed_metric.metric.clone(),
                                                    value: transformed_metric.value.clone(),
                                                    timestamp: transformed_metric.timestamp,
                                                    quality: transformed_metric.quality,
                                                    is_virtual: Some(true),
                                                })
                                                .await;

                                            // Store to time series storage.
                                            // Dual-write: transform namespace (for Data Explorer, rules, useDataSource)
                                            // AND original device namespace (so GET /api/devices/:id/current and
                                            // community components using fetchDeviceValues can discover virtual metrics).
                                            // The frontend Redux store skips is_virtual events, so this does NOT
                                            // pollute deviceTelemetry — it only makes metrics queryable via REST.
                                            let storage_value = match &transformed_metric.value {
                                                MetricValue::Float(f) => {
                                                    neomind_devices::MetricValue::Float(*f)
                                                }
                                                MetricValue::Integer(i) => {
                                                    neomind_devices::MetricValue::Integer(*i)
                                                }
                                                MetricValue::Boolean(b) => {
                                                    neomind_devices::MetricValue::Boolean(*b)
                                                }
                                                MetricValue::String(s) => {
                                                    neomind_devices::MetricValue::String(s.clone())
                                                }
                                                MetricValue::Json(v) => {
                                                    neomind_devices::MetricValue::String(
                                                        v.to_string(),
                                                    )
                                                }
                                            };
                                            let data_point = neomind_devices::DataPoint {
                                                timestamp: transformed_metric.timestamp,
                                                value: storage_value,
                                                quality: transformed_metric.quality,
                                            };
                                            // Primary: transform namespace
                                            if let Err(e) = time_series_storage_inner
                                                .write(
                                                    &storage_device_id,
                                                    &transformed_metric.metric,
                                                    data_point.clone(),
                                                )
                                                .await
                                            {
                                                tracing::warn!(
                                                    device_id = %storage_device_id,
                                                    metric = %transformed_metric.metric,
                                                    error = %e,
                                                    "Failed to store transformed metric to time series storage"
                                                );
                                            }

                                            // Secondary: original device namespace (for REST API discovery)
                                            let device_source_id =
                                                format!("device:{}", device_id_clone);
                                            if let Err(e) = time_series_storage_inner
                                                .write(
                                                    &device_source_id,
                                                    &transformed_metric.metric,
                                                    data_point,
                                                )
                                                .await
                                            {
                                                tracing::debug!(
                                                    device_id = %device_source_id,
                                                    metric = %transformed_metric.metric,
                                                    error = %e,
                                                    "Failed to store transformed metric to device namespace (non-critical)"
                                                );
                                            }

                                            tracing::trace!(
                                                device_id = %storage_device_id,
                                                metric = %transformed_metric.metric,
                                                value = ?transformed_metric.value,
                                                "Published and stored transformed metric"
                                            );

                                            // Update rule engine value provider + notify engine
                                            // so that rules referencing `transform:{transform_id}:{metric}` fire.
                                            // Both the DeviceMetric event and the time-series storage now use
                                            // the "transform:{id}" namespace, consistent with rule data source filters.
                                            if let Some(ref transform_id) =
                                                transformed_metric.transform_id
                                            {
                                                let rv = match &transformed_metric.value {
                                                    MetricValue::Float(v) => {
                                                        neomind_rules::RuleValue::Number(*v)
                                                    }
                                                    MetricValue::Integer(v) => {
                                                        neomind_rules::RuleValue::Number(*v as f64)
                                                    }
                                                    MetricValue::Boolean(v) => {
                                                        neomind_rules::RuleValue::Number(if *v {
                                                            1.0
                                                        } else {
                                                            0.0
                                                        })
                                                    }
                                                    MetricValue::String(s) => {
                                                        neomind_rules::RuleValue::Text(s.clone())
                                                    }
                                                    MetricValue::Json(v) => {
                                                        neomind_rules::RuleValue::Text(
                                                            v.to_string(),
                                                        )
                                                    }
                                                };
                                                value_provider_clone
                                                    .update_rule_value(
                                                        "transform",
                                                        transform_id,
                                                        &transformed_metric.metric,
                                                        rv.clone(),
                                                    )
                                                    .await;
                                                let ds = neomind_core::datasource::DataSourceId::transform(transform_id, &transformed_metric.metric);
                                                rule_engine_clone.on_data_update(&ds, rv).await;
                                            }
                                        }
                                    }

                                    // Log warnings
                                    for warning in &result.warnings {
                                        tracing::warn!(
                                            device_id = %device_id_clone,
                                            warning = %warning,
                                            "Transform processing warning"
                                        );
                                    }
                                }
                                Err(e) => {
                                    tracing::debug!(
                                        device_id = %device_id_clone,
                                        error = %e,
                                        "Transform processing failed (non-critical)"
                                    );
                                }
                            }
                        });

                        // Store the timer handle
                        device_timers.insert(device_id, timer_handle);
                    }
                }

                // Abort all pending timers when shutting down
                for (_, timer) in device_timers {
                    timer.abort();
                }

                tracing::info!("Transform event service stopped");
            });
        }
        self.running.clone()
    }
}
