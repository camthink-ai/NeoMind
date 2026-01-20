//! Event persistence module.
//!
//! Bridges EventBus events to EventLogStore for historical event storage.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use edge_ai_core::eventbus::EventBus;
use edge_ai_core::event::{EventMetadata, NeoTalkEvent};
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
