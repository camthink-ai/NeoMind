//! Event persistence module.
//!
//! Bridges EventBus events to EventLogStore for historical event storage.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use edge_ai_core::eventbus::EventBus;
use edge_ai_core::event::{EventMetadata, NeoTalkEvent};
use edge_ai_core::MetricValue;
use edge_ai_storage::business::{EventLog, EventLogStore, EventSeverity};
use edge_ai_rules::{CompiledRule, RuleCondition};

/// Event persistence configuration.
#[derive(Clone)]
pub struct EventPersistenceConfig {
    /// Batch size for writing events (0 = no batching)
    pub batch_size: usize,
    /// Batch timeout in milliseconds (0 = no timeout)
    pub batch_timeout_ms: u64,
    /// Whether to persist all event types or filter some
    pub persist_all: bool,
}

impl Default for EventPersistenceConfig {
    fn default() -> Self {
        Self {
            batch_size: 10,
            batch_timeout_ms: 100,
            persist_all: true,
        }
    }
}

/// Event persistence service.
///
/// Subscribes to EventBus and persists events to EventLogStore.
pub struct EventPersistenceService {
    event_bus: Arc<EventBus>,
    event_log: Arc<EventLogStore>,
    running: Arc<AtomicBool>,
    config: EventPersistenceConfig,
}

impl EventPersistenceService {
    /// Create a new event persistence service.
    pub fn new(
        event_bus: Arc<EventBus>,
        event_log: Arc<EventLogStore>,
        config: EventPersistenceConfig,
    ) -> Self {
        Self {
            event_bus,
            event_log,
            running: Arc::new(AtomicBool::new(false)),
            config,
        }
    }

    /// Create with default configuration.
    pub fn with_defaults(event_bus: Arc<EventBus>, event_log: Arc<EventLogStore>) -> Self {
        Self::new(event_bus, event_log, EventPersistenceConfig::default())
    }

    /// Start the persistence service.
    ///
    /// Spawns a background task that subscribes to all events
    /// and persists them to the event log store.
    pub fn start(&self) -> Arc<AtomicBool> {
        if self.running.load(Ordering::Relaxed) {
            return self.running.clone();
        }

        tracing::info!("Starting event persistence service");

        let mut rx = self.event_bus.subscribe();
        let running = self.running.clone();
        let running_copy = running.clone();  // Clone before async move
        let event_log = self.event_log.clone();
        let config = self.config.clone();

        running.store(true, Ordering::Relaxed);

        tokio::spawn(async move {
            let mut batch = Vec::with_capacity(config.batch_size.max(1));
            let mut last_flush = tokio::time::Instant::now();

            while running.load(Ordering::Relaxed) {
                match rx.recv().await {
                    Some((event, metadata)) => {
                        // Convert to EventLog and add to batch
                        if let Some(evt_log) = convert_to_event_log(event, metadata) {
                            batch.push(evt_log);

                            // Flush if batch is full
                            if batch.len() >= config.batch_size.max(1) {
                                flush_batch(&event_log, &mut batch);
                                last_flush = tokio::time::Instant::now();
                            }
                        }
                    }
                    None => {
                        tracing::warn!("Event bus closed, stopping event persistence");
                        break;
                    }
                }

                // Flush based on timeout
                if !batch.is_empty()
                    && last_flush.elapsed() >= Duration::from_millis(config.batch_timeout_ms)
                {
                    flush_batch(&event_log, &mut batch);
                    last_flush = tokio::time::Instant::now();
                }
            }

            // Flush remaining events
            if !batch.is_empty() {
                flush_batch(&event_log, &mut batch);
            }

            tracing::info!("Event persistence service stopped");
        });

        running_copy
    }

    /// Stop the persistence service.
    pub fn stop(&self) {
        tracing::info!("Stopping event persistence service");
        self.running.store(false, Ordering::Relaxed);
    }

    /// Check if the service is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

/// Flush a batch of events to the event log store.
fn flush_batch(event_log: &EventLogStore, batch: &mut Vec<EventLog>) {
    if batch.is_empty() {
        return;
    }

    let count = batch.len();
    for event in batch.drain(..) {
        if let Err(e) = event_log.write(&event) {
            tracing::error!(
                category = "event_persistence",
                event_id = %event.id,
                error = %e,
                "Failed to write event to log store"
            );
        }
    }

    if count > 0 {
        tracing::debug!(
            category = "event_persistence",
            count = %count,
            "Flushed event batch to log store"
        );
    }
}

/// Convert a NeoTalkEvent with metadata to an EventLog.
fn convert_to_event_log(event: NeoTalkEvent, metadata: EventMetadata) -> Option<EventLog> {
    let timestamp = event.timestamp();
    let event_type = event.type_name().to_string();
    let source = Some(metadata.source.clone());

    // Generate a unique event ID
    let id = format!("{}:{}", metadata.source, uuid::Uuid::new_v4());

    // Determine severity and message based on event type
    let (severity, message, data) = match &event {
        // Device events
        NeoTalkEvent::DeviceOnline { device_id, .. } => (
            EventSeverity::Info,
            format!("Device {} came online", device_id),
            Some(serde_json::to_value(event).ok()?),
        ),
        NeoTalkEvent::DeviceOffline { device_id, reason, .. } => (
            EventSeverity::Warning,
            format!("Device {} went online: {}", device_id, reason.as_deref().unwrap_or("unknown")),
            Some(serde_json::to_value(event).ok()?),
        ),
        NeoTalkEvent::DeviceMetric { device_id, metric, .. } => (
            EventSeverity::Info,
            format!("Metric update from {}: {}", device_id, metric),
            Some(serde_json::to_value(event).ok()?),
        ),
        NeoTalkEvent::DeviceCommandResult { device_id, command, success, .. } => (
            if *success { EventSeverity::Info } else { EventSeverity::Error },
            format!("Command {} executed on {}: {}", command, device_id, if *success { "success" } else { "failed" }),
            Some(serde_json::to_value(event).ok()?),
        ),

        // Rule events
        NeoTalkEvent::RuleEvaluated { rule_id, rule_name, condition_met, .. } => (
            EventSeverity::Info,
            format!("Rule '{}' ({}) evaluated: {}", rule_name, rule_id, if *condition_met { "met" } else { "not met" }),
            Some(serde_json::to_value(event).ok()?),
        ),
        NeoTalkEvent::RuleTriggered { rule_id, rule_name, .. } => (
            EventSeverity::Info,
            format!("Rule '{}' ({}) triggered", rule_name, rule_id),
            Some(serde_json::to_value(event).ok()?),
        ),
        NeoTalkEvent::RuleExecuted { rule_id, rule_name, success, .. } => (
            if *success { EventSeverity::Info } else { EventSeverity::Error },
            format!("Rule '{}' ({}) executed: {}", rule_name, rule_id, if *success { "success" } else { "failed" }),
            Some(serde_json::to_value(event).ok()?),
        ),

        // Workflow events
        NeoTalkEvent::WorkflowTriggered { workflow_id, execution_id, .. } => (
            EventSeverity::Info,
            format!("Workflow {} triggered (execution: {})", workflow_id, execution_id),
            Some(serde_json::to_value(event).ok()?),
        ),
        NeoTalkEvent::WorkflowStepCompleted { workflow_id, step_id, execution_id, .. } => (
            EventSeverity::Info,
            format!("Workflow {} step {} completed (execution: {})", workflow_id, step_id, execution_id),
            Some(serde_json::to_value(event).ok()?),
        ),
        NeoTalkEvent::WorkflowCompleted { workflow_id, execution_id, success, .. } => (
            if *success { EventSeverity::Info } else { EventSeverity::Error },
            format!("Workflow {} completed (execution: {}): {}", workflow_id, execution_id, if *success { "success" } else { "failed" }),
            Some(serde_json::to_value(event).ok()?),
        ),

        // Alert events
        NeoTalkEvent::AlertCreated { alert_id, severity, title, .. } => {
            let severity_level = match severity.as_str() {
                "critical" => EventSeverity::Critical,
                "error" => EventSeverity::Error,
                "warning" => EventSeverity::Warning,
                _ => EventSeverity::Info,
            };
            (
                severity_level,
                format!("Alert '{}' created: {}", alert_id, title),
                Some(serde_json::to_value(event).ok()?),
            )
        }
        NeoTalkEvent::AlertAcknowledged { alert_id, .. } => (
            EventSeverity::Info,
            format!("Alert {} acknowledged", alert_id),
            Some(serde_json::to_value(event).ok()?),
        ),

        // LLM events
        NeoTalkEvent::PeriodicReviewTriggered { review_id, .. } => (
            EventSeverity::Info,
            format!("Periodic review {} triggered", review_id),
            Some(serde_json::to_value(event).ok()?),
        ),
        NeoTalkEvent::LlmDecisionProposed { decision_id, title, .. } => (
            EventSeverity::Info,
            format!("LLM decision proposed: {} ({})", title, decision_id),
            Some(serde_json::to_value(event).ok()?),
        ),
        NeoTalkEvent::LlmDecisionExecuted { decision_id, success, .. } => (
            if *success { EventSeverity::Info } else { EventSeverity::Error },
            format!("LLM decision {} executed: {}", decision_id, if *success { "success" } else { "failed" }),
            Some(serde_json::to_value(event).ok()?),
        ),

        // User events
        NeoTalkEvent::UserMessage { session_id, .. } => (
            EventSeverity::Info,
            format!("User message in session {}", session_id),
            Some(serde_json::to_value(event).ok()?),
        ),
        NeoTalkEvent::LlmResponse { session_id, .. } => (
            EventSeverity::Info,
            format!("LLM response in session {}", session_id),
            Some(serde_json::to_value(event).ok()?),
        ),

        // Tool events
        NeoTalkEvent::ToolExecutionStart { tool_name, .. } => (
            EventSeverity::Info,
            format!("Tool {} execution started", tool_name),
            Some(serde_json::to_value(event).ok()?),
        ),
        NeoTalkEvent::ToolExecutionSuccess { tool_name, .. } => (
            EventSeverity::Info,
            format!("Tool {} execution succeeded", tool_name),
            Some(serde_json::to_value(event).ok()?),
        ),
        NeoTalkEvent::ToolExecutionFailure { tool_name, error, .. } => (
            EventSeverity::Error,
            format!("Tool {} execution failed: {}", tool_name, error),
            Some(serde_json::to_value(event).ok()?),
        ),

        // Custom events
        NeoTalkEvent::Custom { event_type, .. } => (
            EventSeverity::Info,
            format!("Custom event: {}", event_type),
            Some(serde_json::to_value(event).ok()?),
        ),
    };

    Some(EventLog {
        id,
        event_type,
        source,
        severity,
        timestamp,
        message,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = EventPersistenceConfig::default();
        assert_eq!(config.batch_size, 10);
        assert_eq!(config.batch_timeout_ms, 100);
        assert!(config.persist_all);
    }
}

/// Rule engine event service.
///
/// Subscribes to device metric events and triggers rule evaluation.
/// This enables automatic rule evaluation when device data is received.
pub struct RuleEngineEventService {
    event_bus: Arc<EventBus>,
    rule_engine: Arc<edge_ai_rules::RuleEngine>,
    running: Arc<AtomicBool>,
}

impl RuleEngineEventService {
    /// Create a new rule engine event service.
    pub fn new(
        event_bus: Arc<EventBus>,
        rule_engine: Arc<edge_ai_rules::RuleEngine>,
    ) -> Self {
        Self {
            event_bus,
            rule_engine,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Start the event service.
    ///
    /// Spawns a background task that subscribes to device metric events
    /// and evaluates relevant rules.
    pub fn start(&self) -> Arc<AtomicBool> {
        if self.running.load(Ordering::Relaxed) {
            return self.running.clone();
        }

        tracing::info!("Starting rule engine event service");

        let mut rx = self.event_bus.filter().device_events();
        let running = self.running.clone();
        let running_copy = running.clone();  // Clone before async move
        let rule_engine = self.rule_engine.clone();
        let event_bus = self.event_bus.clone();

        running.store(true, Ordering::Relaxed);

        tokio::spawn(async move {
            use edge_ai_core::{MetricValue, NeoTalkEvent};

            while running.load(Ordering::Relaxed) {
                match rx.recv().await {
                    Some((event, _metadata)) => {
                        if let NeoTalkEvent::DeviceMetric {
                            device_id,
                            metric,
                            value,
                            timestamp,
                            quality,
                        } = event
                        {
                            // Extract numeric value for rule evaluation
                            let numeric_value = match &value {
                                MetricValue::Float(v) => Some(*v),
                                MetricValue::Integer(v) => Some(*v as f64),
                                MetricValue::Boolean(v) => Some(if *v { 1.0 } else { 0.0 }),
                                _ => None,
                            };

                            if let Some(num_value) = numeric_value {
                                // Publish RuleEvaluated event for all matching rules
                                Self::evaluate_and_publish_rules(
                                    &rule_engine,
                                    &event_bus,
                                    &device_id,
                                    &metric,
                                    num_value,
                                    timestamp,
                                ).await;
                            }
                        }
                    }
                    None => {
                        tracing::warn!("Event bus closed, stopping rule engine event service");
                        break;
                    }
                }
            }

            tracing::info!("Rule engine event service stopped");
        });

        running_copy
    }

    /// Stop the event service.
    pub fn stop(&self) {
        tracing::info!("Stopping rule engine event service");
        self.running.store(false, Ordering::Relaxed);
    }

    /// Check if the service is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Evaluate rules relevant to a device metric and publish events.
    async fn evaluate_and_publish_rules(
        rule_engine: &edge_ai_rules::RuleEngine,
        event_bus: &EventBus,
        device_id: &str,
        metric: &str,
        value: f64,
        timestamp: i64,
    ) {
        use edge_ai_core::event::{EventMetadata, NeoTalkEvent};

        let rules = rule_engine.list_rules().await;
        let source = format!("rule_engine:{}", device_id);

        for rule in rules {
            let rule_id = rule.id.to_string();
            let rule_name = rule.name.clone();

            // Check if rule condition matches this metric
            let condition_met = match &rule.condition {
                RuleCondition::Simple { device_id: d_id, metric: m_name, operator, threshold } => {
                    d_id == device_id && m_name == metric && operator.evaluate(value, *threshold)
                }
                _ => false,  // For complex conditions, skip for now
            };

            // Publish RuleEvaluated event
            let _ = event_bus
                .publish_with_source(
                    NeoTalkEvent::RuleEvaluated {
                        rule_id: rule_id.clone(),
                        rule_name: rule_name.clone(),
                        condition_met,
                        timestamp,
                    },
                    source.clone(),
                )
                .await;

            // If condition met, trigger rule actions
            if condition_met {
                tracing::info!(
                    category = "rule_engine",
                    rule_id = %rule_id,
                    rule_name = %rule_name,
                    device_id = %device_id,
                    metric = %metric,
                    value = %value,
                    "Rule condition met, triggering actions"
                );

                // Publish RuleTriggered event
                let _ = event_bus
                    .publish_with_source(
                        NeoTalkEvent::RuleTriggered {
                            rule_id: rule_id.clone(),
                            rule_name: rule_name.clone(),
                            trigger_value: value,
                            actions: rule
                                .actions
                                .iter()
                                .map(|a| format!("{:?}", a))
                                .collect(),
                            timestamp,
                        },
                        source.clone(),
                    )
                    .await;

                // Execute rule actions (simplified - just log for now)
                // TODO: Implement actual action execution
            }
        }
    }

    /// Check if a rule condition matches a device metric value.
    fn condition_matches_metric(
        condition: &edge_ai_rules::dsl::RuleCondition,
        device_id: &str,
        metric: &str,
        value: f64,
    ) -> bool {
        // This is a simplified check - in a full implementation,
        // we'd parse the condition DSL and evaluate it properly
        let condition_str = format!("{:?}", condition);

        // Check if condition references this device and metric
        condition_str.contains(device_id)
            && condition_str.contains(metric)
            && Self::evaluate_condition_comparison(&condition_str, value)
    }

    /// Evaluate the comparison part of a condition.
    fn evaluate_condition_comparison(condition_str: &str, value: f64) -> bool {
        // Simplified evaluation - checks for common comparison patterns
        // In production, this would properly parse and evaluate the DSL

        if condition_str.contains(">") {
            if let Some(threshold) = Self::extract_threshold(condition_str) {
                return value > threshold;
            }
        } else if condition_str.contains("<") {
            if let Some(threshold) = Self::extract_threshold(condition_str) {
                return value < threshold;
            }
        } else if condition_str.contains(">=") {
            if let Some(threshold) = Self::extract_threshold(condition_str) {
                return value >= threshold;
            }
        } else if condition_str.contains("<=") {
            if let Some(threshold) = Self::extract_threshold(condition_str) {
                return value <= threshold;
            }
        } else if condition_str.contains("==") {
            if let Some(threshold) = Self::extract_threshold(condition_str) {
                return (value - threshold).abs() < 0.001;
            }
        }

        false
    }

    /// Extract threshold value from condition string.
    fn extract_threshold(condition_str: &str) -> Option<f64> {
        // Find numbers in the condition string
        use regex::Regex;

        let re = Regex::new(r"[-+]?\d*\.?\d+").ok()?;
        re.find(condition_str)?.as_str().parse().ok()
    }
}

/// Configuration for the Transform event service.
#[derive(Debug, Clone)]
pub struct TransformEventServiceConfig {
    /// Delay in milliseconds to wait for more metrics from the same device batch.
    /// This allows multiple metrics from the same device data batch to be collected
    /// before processing transforms.
    /// Default: 50ms
    pub batch_delay_ms: u64,

    /// Maximum time to keep device data in buffer before forcing processing.
    /// This prevents stale data from accumulating if no new metrics arrive.
    /// Default: 5000ms (5 seconds)
    pub max_buffer_age_ms: u64,
}

impl Default for TransformEventServiceConfig {
    fn default() -> Self {
        Self {
            batch_delay_ms: 50,     // 50ms default delay
            max_buffer_age_ms: 5000, // 5 seconds max age
        }
    }
}

/// Transform event service.
///
/// Subscribes to device metric events and processes transforms to generate virtual metrics.
/// This enables automatic transform processing when device data is received from any adapter.
///
/// # Processing Model
///
/// The service uses a batching approach to collect all metrics from a device before
/// processing transforms:
/// 1. Metrics are collected in a per-device buffer
/// 2. After `batch_delay_ms` of no new metrics, the buffer is processed
/// 3. Transforms are applied and virtual metrics are generated
/// 4. Virtual metrics are marked to prevent re-processing (loop prevention)
pub struct TransformEventService {
    event_bus: Arc<EventBus>,
    transform_engine: Arc<edge_ai_automation::transform::TransformEngine>,
    automation_store: Arc<edge_ai_automation::store::SharedAutomationStore>,
    time_series_storage: Arc<edge_ai_devices::TimeSeriesStorage>,
    device_registry: Arc<edge_ai_devices::registry::DeviceRegistry>,
    config: TransformEventServiceConfig,
    running: Arc<AtomicBool>,
}

impl TransformEventService {
    /// Create a new transform event service with default configuration.
    pub fn new(
        event_bus: Arc<EventBus>,
        transform_engine: Arc<edge_ai_automation::transform::TransformEngine>,
        automation_store: Arc<edge_ai_automation::store::SharedAutomationStore>,
        time_series_storage: Arc<edge_ai_devices::TimeSeriesStorage>,
        device_registry: Arc<edge_ai_devices::registry::DeviceRegistry>,
    ) -> Self {
        Self {
            event_bus,
            transform_engine,
            automation_store,
            time_series_storage,
            device_registry,
            config: TransformEventServiceConfig::default(),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create a new transform event service with custom configuration.
    pub fn with_config(
        event_bus: Arc<EventBus>,
        transform_engine: Arc<edge_ai_automation::transform::TransformEngine>,
        automation_store: Arc<edge_ai_automation::store::SharedAutomationStore>,
        time_series_storage: Arc<edge_ai_devices::TimeSeriesStorage>,
        device_registry: Arc<edge_ai_devices::registry::DeviceRegistry>,
        config: TransformEventServiceConfig,
    ) -> Self {
        Self {
            event_bus,
            transform_engine,
            automation_store,
            time_series_storage,
            device_registry,
            config,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Start the event service.
    ///
    /// Spawns a background task that:
    /// 1. Collects metrics for each device from DeviceMetric events
    /// 2. After a short delay, processes all transforms with the collected data
    /// 3. Publishes new virtual metric events
    pub fn start(&self) -> Arc<AtomicBool> {
        if self.running.load(Ordering::Relaxed) {
            return self.running.clone();
        }

        tracing::info!("Starting transform event service");

        let mut rx = self.event_bus.filter().device_events();
        let running = self.running.clone();
        let running_copy = running.clone();
        let transform_engine = self.transform_engine.clone();
        let automation_store = self.automation_store.clone();
        let time_series_storage = self.time_series_storage.clone();
        let event_bus = self.event_bus.clone();
        let device_registry = self.device_registry.clone();

        running.store(true, Ordering::Relaxed);

        let batch_delay = self.config.batch_delay_ms;

        tokio::spawn(async move {
            use edge_ai_core::{MetricValue, NeoTalkEvent};
            use std::collections::{HashMap, HashSet};
            use std::sync::Arc;
            use tokio::sync::Mutex;
            use tokio::time::{sleep, Duration};

            // Buffer to collect metrics per device before processing transforms
            // Each device's metrics are processed after a short delay to allow
            // all metrics from the same data batch to be collected
            let mut device_metrics: HashMap<String, serde_json::Value> = HashMap::new();
            let mut device_timers: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();

            // Track metrics that were generated by transforms to prevent re-processing
            // Shared between main loop and transform processing tasks
            let virtual_metrics: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));

            while running.load(Ordering::Relaxed) {
                match rx.recv().await {
                    Some((event, metadata)) => {
                        if let NeoTalkEvent::DeviceMetric {
                            device_id,
                            metric,
                            value,
                            timestamp: _,
                            quality: _,
                        } = event
                        {
                            // Create a unique key for this metric
                            let metric_key = format!("{}:{}", device_id, metric);

                            // Skip if this metric was generated by our transform processing
                            // We check the shared HashSet which is populated by transform tasks
                            {
                                let vm = virtual_metrics.lock().await;
                                if vm.contains(&metric_key) {
                                    tracing::debug!("Skipping virtual metric '{}' that was just generated", metric_key);
                                    // Don't remove here - let it be removed after being skipped once
                                    drop(vm);
                                    // Remove after check to allow skipping exactly once
                                    virtual_metrics.lock().await.remove(&metric_key);
                                    continue;
                                }
                            }

                            // Also check metric name patterns as a fallback for old-style virtual metrics
                            // NOTE: Transform-generated metrics use DOT notation (transform.xxx, virtual.xxx)
                            // not underscore notation (transform_xxx, virtual_xxx)
                            let is_virtual_pattern = metric.starts_with("transform.")
                                || metric.starts_with("virtual.")
                                || metric.starts_with("computed.")
                                || metric.starts_with("derived.")
                                || metric.starts_with("aggregated.")
                                || metric == "transform"
                                || metric == "virtual"
                                || metric == "computed"
                                || metric == "derived"
                                || metric == "aggregated";

                            if is_virtual_pattern {
                                tracing::debug!("Skipping metric '{}' with virtual pattern", metric);
                                continue;
                            }

                            tracing::trace!("Processing metric '{}' for device {}", metric, device_id);

                            // Add metric to device's data buffer
                            let device_data = device_metrics.entry(device_id.clone()).or_insert_with(|| {
                                serde_json::json!({})
                            });

                            // Convert value to JSON and add to the data object
                            if let Some(obj) = device_data.as_object_mut() {
                                let json_value = match &value {
                                    MetricValue::Float(v) => serde_json::json!(v),
                                    MetricValue::Integer(v) => serde_json::json!(v),
                                    MetricValue::Boolean(v) => serde_json::json!(v),
                                    // Special handling for _raw metric: try to parse as JSON
                                    // This allows Transform code to access nested properties like input.values.battery
                                    MetricValue::String(s) => {
                                        if metric == "_raw" {
                                            // Try to parse as JSON for easier access in Transform code
                                            serde_json::from_str::<serde_json::Value>(s)
                                                .unwrap_or_else(|_| serde_json::json!(s))
                                        } else {
                                            serde_json::json!(s)
                                        }
                                    },
                                    MetricValue::Json(j) => j.clone(),
                                };
                                // Use insert() instead of index assignment to avoid potential issues
                                obj.insert(metric.clone(), json_value);
                            }

                            // Get device type for transform processing
                            let device_type = Self::get_device_type(&device_id, &device_registry).await;

                            // Cancel any existing timer for this device
                            if let Some(timer) = device_timers.remove(&device_id) {
                                timer.abort();
                            }

                            // Take the current device data out of the buffer (this clears it)
                            // This prevents accumulation and re-processing of old metrics
                            let device_data_for_processing = device_metrics.remove(&device_id)
                                .unwrap_or_else(|| serde_json::json!({}));

                            // Clone data for the async task
                            let device_id_clone = device_id.clone();
                            let device_type_clone = device_type.clone();
                            let transform_engine_clone = transform_engine.clone();
                            let automation_store_clone = automation_store.clone();
                            let time_series_storage_clone = time_series_storage.clone();
                            let event_bus_clone = event_bus.clone();
                            let virtual_metrics_clone = virtual_metrics.clone();
                            let running_clone = running.clone();

                            // Spawn a delayed task to process transforms for this device
                            let timer = tokio::spawn(async move {
                                // Wait a bit to collect more metrics from the same batch
                                sleep(Duration::from_millis(batch_delay)).await;

                                if !running_clone.load(Ordering::Relaxed) {
                                    return;
                                }

                                // Process transforms for this device
                                // Returns metrics without publishing (to control timing)
                                let generated_metrics = Self::process_device_transforms(
                                    &device_id_clone,
                                    device_type_clone.as_deref(),
                                    &device_data_for_processing,
                                    &transform_engine_clone,
                                    &automation_store_clone,
                                ).await;

                                if !generated_metrics.is_empty() {
                                    // STEP 1: Mark all metrics as virtual FIRST (before publishing)
                                    // This prevents race conditions where events arrive before HashSet is updated
                                    {
                                        let mut vm = virtual_metrics_clone.lock().await;
                                        for (metric_key, _) in &generated_metrics {
                                            vm.insert(metric_key.clone());
                                        }
                                    }

                                    // STEP 2: NOW publish to EventBus (events will be skipped due to HashSet)
                                    // STEP 3: Store in time_series_storage
                                    for (metric_key, metric) in generated_metrics {
                                        // Publish to EventBus
                                        event_bus_clone
                                            .publish(NeoTalkEvent::DeviceMetric {
                                                device_id: metric.device_id.clone(),
                                                metric: metric.metric.clone(),
                                                value: MetricValue::Float(metric.value),
                                                timestamp: metric.timestamp,
                                                quality: metric.quality,
                                            })
                                            .await;

                                        // Store in telemetry
                                        let data_point = edge_ai_devices::telemetry::DataPoint {
                                            timestamp: metric.timestamp,
                                            value: edge_ai_devices::mdl::MetricValue::Float(metric.value),
                                            quality: metric.quality,
                                        };
                                        if let Err(e) = time_series_storage_clone.write(&metric.device_id, &metric.metric, data_point).await {
                                            tracing::warn!(
                                                "Failed to write virtual metric {}/{}: {}",
                                                metric.device_id,
                                                metric.metric,
                                                e
                                            );
                                        }

                                        tracing::trace!("Published and stored virtual metric: {}", metric_key);
                                    }
                                }
                            });

                            device_timers.insert(device_id.clone(), timer);
                        }
                    }
                    None => {
                        tracing::warn!("Event bus closed, stopping transform event service");
                        break;
                    }
                }
            }

            tracing::info!("Transform event service stopped");
        });

        running_copy
    }

    /// Stop the event service.
    pub fn stop(&self) {
        tracing::info!("Stopping transform event service");
        self.running.store(false, Ordering::Relaxed);
    }

    /// Check if the service is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Get device type from device registry
    async fn get_device_type(
        device_id: &str,
        device_registry: &Arc<edge_ai_devices::registry::DeviceRegistry>,
    ) -> Option<String> {
        // Get device type from device registry
        let device = device_registry.get_device(device_id).await?;
        Some(device.device_type)
    }

    /// Process transforms for a device and return virtual metrics without publishing
    /// Returns the list of (metric_key, metric_data) for controlled publishing
    async fn process_device_transforms(
        device_id: &str,
        device_type: Option<&str>,
        device_data: &serde_json::Value,
        transform_engine: &Arc<edge_ai_automation::transform::TransformEngine>,
        automation_store: &Arc<edge_ai_automation::store::SharedAutomationStore>,
    ) -> Vec<(String, edge_ai_automation::transform::TransformedMetric)> {
        use edge_ai_automation::Automation;

        // Load all transforms
        let transforms: Vec<_> = match automation_store.list_automations().await {
            Ok(automations) => automations
                .into_iter()
                .filter_map(|a| match a {
                    Automation::Transform(t) => Some(t),
                    _ => None,
                })
                .collect(),
            Err(e) => {
                tracing::warn!("Failed to load transforms: {}", e);
                return Vec::new();
            }
        };

        if transforms.is_empty() {
            return Vec::new();
        }

        // Process data through all applicable transforms
        match transform_engine
            .process_device_data(&transforms, device_id, device_type, device_data)
            .await
        {
            Ok(transform_result) => {
                if !transform_result.metrics.is_empty() {
                    let metric_names: Vec<&str> = transform_result.metrics.iter().map(|m| m.metric.as_str()).collect();
                    tracing::warn!(
                        "Transform generated metrics for device {}: {:?}",
                        device_id,
                        metric_names
                    );

                    tracing::info!(
                        "Transform processing produced {} virtual metrics for device {}",
                        transform_result.metrics.len(),
                        device_id
                    );

                    // Return metrics with their keys for controlled publishing
                    transform_result.metrics
                        .into_iter()
                        .map(|m| {
                            let key = format!("{}:{}", m.device_id, m.metric);
                            (key, m)
                        })
                        .collect()
                } else {
                    Vec::new()
                }
            }
            Err(e) => {
                tracing::warn!("Transform processing failed for device {}: {}", device_id, e);
                Vec::new()
            }
        }
    }
}
