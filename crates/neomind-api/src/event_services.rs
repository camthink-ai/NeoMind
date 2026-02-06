//! Event processing services for rule engine and transform engine.
//!
//! This module provides background services that subscribe to events from the EventBus
//! and trigger actions in the rule engine and transform engine.

use std::sync::Arc;
use std::collections::HashMap;

use neomind_core::eventbus::EventBus;
use neomind_core::{NeoMindEvent, MetricValue};
use neomind_rules::RuleEngine;
use neomind_automation::{store::SharedAutomationStore, Automation, TransformEngine};
use neomind_devices::DeviceRegistry;

/// Rule engine event service.
///
/// Subscribes to device metric events and auto-evaluates rules.
pub struct RuleEngineEventService {
    event_bus: Arc<EventBus>,
    _rule_engine: Arc<RuleEngine>,
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl RuleEngineEventService {
    /// Create a new rule engine event service.
    pub fn new(event_bus: Arc<EventBus>, rule_engine: Arc<RuleEngine>) -> Self {
        Self {
            event_bus,
            _rule_engine: rule_engine,
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Start the service.
    pub fn start(&self) -> Arc<std::sync::atomic::AtomicBool> {
        if self.running.compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst
        ).is_ok() {
            let _running = self.running.clone();
            let event_bus = self.event_bus.clone();
            tokio::spawn(async move {
                let mut rx = event_bus.filter().device_events();
                tracing::info!("Rule engine event service started - subscribing to device events");

                while let Some((event, _metadata)) = rx.recv().await {
                    if let NeoMindEvent::DeviceMetric { device_id, metric, value, .. } = event {
                        tracing::trace!(device_id = %device_id, metric = %metric, "Device metric received for rule evaluation");
                        // Rule states are updated by the separate value provider update task in init_rule_engine_events
                        let _ = (device_id, metric, value);
                    }
                }
            });
        }
        self.running.clone()
    }
}

/// Transform event service.
///
/// Subscribes to device metric events and processes transforms to generate virtual metrics.
pub struct TransformEventService {
    event_bus: Arc<EventBus>,
    transform_engine: Arc<TransformEngine>,
    automation_store: Arc<SharedAutomationStore>,
    device_registry: Arc<DeviceRegistry>,
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl TransformEventService {
    /// Create a new transform event service.
    pub fn new(
        event_bus: Arc<EventBus>,
        transform_engine: Arc<TransformEngine>,
        automation_store: Arc<SharedAutomationStore>,
        _time_series_storage: Arc<neomind_devices::TimeSeriesStorage>,
        device_registry: Arc<neomind_devices::DeviceRegistry>,
    ) -> Self {
        Self {
            event_bus,
            transform_engine,
            automation_store,
            device_registry,
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Start the service.
    pub fn start(&self) -> Arc<std::sync::atomic::AtomicBool> {
        if self.running.compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst
        ).is_ok() {
            let running = self.running.clone();
            let event_bus = self.event_bus.clone();
            let transform_engine = self.transform_engine.clone();
            let automation_store = self.automation_store.clone();
            let device_registry = self.device_registry.clone();

            tokio::spawn(async move {
                let mut rx = event_bus.filter().device_events();
                tracing::info!("Transform event service started - subscribing to device events");

                // Track pending device data and debounce timers
                // (device_id -> (raw_data, latest_timestamp, timer_handle))
                let mut device_raw_data: HashMap<String, serde_json::Value> = HashMap::new();
                let mut device_latest_ts: HashMap<String, i64> = HashMap::new();
                let mut device_timers: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();

                // Debounce delay: wait this long after the last metric before processing
                // This allows multiple metrics from the same device to be collected and processed together
                const DEBOUNCE_MS: u64 = 100; // 100ms debounce window

                while let Some((event, _metadata)) = rx.recv().await {
                    if let NeoMindEvent::DeviceMetric { device_id, metric, value, timestamp, quality: _ } = event {
                        // Skip transform output metrics to prevent infinite loop
                        // Transforms publish metrics with "transform." prefix, which should not be re-processed
                        if metric.starts_with("transform.") {
                            continue;
                        }

                        // Log incoming device metric for debugging
                        tracing::info!(
                            device_id = %device_id,
                            metric = %metric,
                            timestamp = timestamp,
                            "Received device metric event for transform processing"
                        );

                        // Update the latest timestamp for this device
                        device_latest_ts.insert(device_id.clone(), timestamp);

                        // Build or update the device's raw data structure
                        let device_entry = device_raw_data.entry(device_id.clone()).or_insert_with(|| {
                            serde_json::json!({
                                "device_id": device_id,
                                "timestamp": timestamp,
                                "values": {}
                            })
                        });

                        // Update the device data with the new metric
                        if let Some(obj) = device_entry.as_object_mut() {
                            // Update top-level timestamp to latest
                            obj.insert("timestamp".to_string(), serde_json::Value::Number(timestamp.into()));

                            // Update values object
                            let values = obj.entry("values").or_insert_with(|| serde_json::json!({}));
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
                        let device_type: Option<String> = device_registry.get_device(&device_id).await
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

                        // Schedule a new debounce timer
                        let timer_handle = tokio::spawn(async move {
                            // Wait for the debounce delay
                            tokio::time::sleep(tokio::time::Duration::from_millis(DEBOUNCE_MS)).await;

                            tracing::info!(
                                device_id = %device_id_clone,
                                "Debounce timer expired, processing device data"
                            );

                            // Load all enabled transforms
                            let transforms = match automation_store_clone.list_automations().await {
                                Ok(all) => all.into_iter()
                                    .filter_map(|a| match a {
                                        Automation::Transform(t) if t.metadata.enabled => Some(t),
                                        _ => None,
                                    })
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
                            let applicable_transforms: Vec<_> = transforms.into_iter()
                                .filter(|t| t.applies_to_device(&device_id_clone, device_type_clone.as_deref()))
                                .collect();

                            if applicable_transforms.is_empty() {
                                return;
                            }

                            // Process the device data through transforms
                            match transform_engine_clone.process_device_data(
                                &applicable_transforms,
                                &device_id_clone,
                                device_type_clone.as_deref(),
                                &device_entry_clone,
                            ).await {
                                Ok(result) => {
                                    if !result.metrics.is_empty() {
                                        tracing::debug!(
                                            device_id = %device_id_clone,
                                            device_type = ?device_type_clone,
                                            metric_count = result.metrics.len(),
                                            "Transform processed device data (debounced)"
                                        );

                                        // Publish transformed metrics back to event bus
                                        for transformed_metric in result.metrics {
                                            // Publish as DeviceMetric event so rules can also use them
                                            let _ = event_bus_clone.publish(NeoMindEvent::DeviceMetric {
                                                device_id: transformed_metric.device_id.clone(),
                                                metric: transformed_metric.metric.clone(),
                                                value: MetricValue::Float(transformed_metric.value),
                                                timestamp: transformed_metric.timestamp,
                                                quality: transformed_metric.quality,
                                            }).await;

                                            tracing::trace!(
                                                device_id = %transformed_metric.device_id,
                                                metric = %transformed_metric.metric,
                                                value = transformed_metric.value,
                                                "Published transformed metric"
                                            );
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
