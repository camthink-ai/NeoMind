//! Event processing services for rule engine and transform engine.
//!
//! This module provides background services that subscribe to events from the EventBus
//! and trigger actions in the rule engine and transform engine.

use std::sync::Arc;
use std::collections::HashMap;

use edge_ai_core::eventbus::EventBus;
use edge_ai_core::{NeoTalkEvent, MetricValue};
use edge_ai_rules::RuleEngine;
use edge_ai_automation::{store::SharedAutomationStore, Automation, TransformEngine};
use edge_ai_devices::DeviceRegistry;

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
                    if let NeoTalkEvent::DeviceMetric { device_id, metric, value, .. } = event {
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
        _time_series_storage: Arc<edge_ai_devices::TimeSeriesStorage>,
        device_registry: Arc<edge_ai_devices::DeviceRegistry>,
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

                // Track recent device data per device (using a simple sliding window approach)
                let mut device_raw_data: HashMap<String, serde_json::Value> = HashMap::new();

                while let Some((event, _metadata)) = rx.recv().await {
                    if let NeoTalkEvent::DeviceMetric { device_id, metric, value, timestamp, quality: _ } = event {
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
                            // Update top-level timestamp
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

                        // Get device type from registry
                        let device_type: Option<String> = device_registry.get_device(&device_id).await
                            .map(|d| d.device_type.clone());

                        // Load all enabled transforms
                        let transforms = match automation_store.list_automations().await {
                            Ok(all) => all.into_iter()
                                .filter_map(|a| match a {
                                    Automation::Transform(t) if t.metadata.enabled => Some(t),
                                    _ => None,
                                })
                                .collect::<Vec<_>>(),
                            Err(e) => {
                                tracing::debug!("Failed to load transforms: {}", e);
                                continue;
                            }
                        };

                        // Skip if no transforms
                        if transforms.is_empty() {
                            continue;
                        }

                        // Process the device data through transforms
                        match transform_engine.process_device_data(
                            &transforms,
                            &device_id,
                            device_type.as_deref(),
                            device_entry,
                        ).await {
                            Ok(result) => {
                                if !result.metrics.is_empty() {
                                    tracing::debug!(
                                        device_id = %device_id,
                                        device_type = ?device_type,
                                        metric_count = result.metrics.len(),
                                        "Transform processed device data"
                                    );

                                    // Publish transformed metrics back to event bus
                                    for transformed_metric in result.metrics {
                                        // Publish as DeviceMetric event so rules can also use them
                                        let _ = event_bus.publish(NeoTalkEvent::DeviceMetric {
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
                                        device_id = %device_id,
                                        warning = %warning,
                                        "Transform processing warning"
                                    );
                                }
                            }
                            Err(e) => {
                                tracing::debug!(
                                    device_id = %device_id,
                                    error = %e,
                                    "Transform processing failed (non-critical)"
                                );
                            }
                        }
                    }
                }

                tracing::info!("Transform event service stopped");
            });
        }
        self.running.clone()
    }
}
