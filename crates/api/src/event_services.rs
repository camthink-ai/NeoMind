//! Event processing services for rule engine and transform engine.
//!
//! This module provides background services that subscribe to events from the EventBus
//! and trigger actions in the rule engine and transform engine.

use std::sync::Arc;

use edge_ai_core::eventbus::EventBus;
use edge_ai_core::NeoTalkEvent;
use edge_ai_rules::RuleEngine;
use edge_ai_automation::store::SharedAutomationStore;

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
    _transform_engine: Arc<edge_ai_automation::TransformEngine>,
    _automation_store: Arc<SharedAutomationStore>,
    _time_series_storage: Arc<edge_ai_devices::TimeSeriesStorage>,
    _device_registry: Arc<edge_ai_devices::DeviceRegistry>,
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl TransformEventService {
    /// Create a new transform event service.
    pub fn new(
        event_bus: Arc<EventBus>,
        transform_engine: Arc<edge_ai_automation::TransformEngine>,
        automation_store: Arc<SharedAutomationStore>,
        time_series_storage: Arc<edge_ai_devices::TimeSeriesStorage>,
        device_registry: Arc<edge_ai_devices::DeviceRegistry>,
    ) -> Self {
        Self {
            event_bus,
            _transform_engine: transform_engine,
            _automation_store: automation_store,
            _time_series_storage: time_series_storage,
            _device_registry: device_registry,
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
                tracing::info!("Transform event service started - subscribing to device events");

                while let Some((event, _metadata)) = rx.recv().await {
                    if let NeoTalkEvent::DeviceMetric { device_id, metric, value, .. } = event {
                        tracing::trace!(device_id = %device_id, metric = %metric, "Received device metric for transform processing");
                        // TODO: Process transforms to generate virtual metrics
                        let _ = (device_id, metric, value);
                    }
                }
            });
        }
        self.running.clone()
    }
}
