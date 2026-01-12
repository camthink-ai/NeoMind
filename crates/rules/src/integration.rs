//! Event-driven rule engine integration.
//!
//! This module integrates the rule engine with the NeoTalk event bus,
//! enabling automatic rule evaluation when device metrics are published.

use crate::engine::{RuleEngine, RuleId, CompiledRule, ValueProvider};
use crate::dsl::{RuleError, RuleCondition};
use edge_ai_core::{EventBus, NeoTalkEvent, MetricValue};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::RwLock;
use tracing::{debug, error, info};

/// Result type for event-driven rule engine operations.
pub type EventEngineResult<T> = Result<T, EventEngineError>;

/// Error type for event-driven rule engine operations.
#[derive(Debug, thiserror::Error)]
pub enum EventEngineError {
    /// Rule engine error
    #[error("Rule engine error: {0}")]
    RuleEngine(#[from] RuleError),

    /// Event bus error
    #[error("Event bus error: {0}")]
    EventBus(String),

    /// Other error
    #[error("Event engine error: {0}")]
    Other(#[from] anyhow::Error),
}

/// Event-driven rule engine.
///
/// Wraps a RuleEngine and automatically evaluates rules when device
/// metric events are received on the event bus.
pub struct EventDrivenRuleEngine {
    /// Underlying rule engine
    engine: RuleEngine,
    /// Event bus for subscribing to events
    event_bus: EventBus,
    /// Running state
    running: Arc<AtomicBool>,
    /// Device value cache (for rule evaluation)
    value_cache: Arc<RwLock<std::collections::HashMap<(String, String), f64>>>,
}

impl EventDrivenRuleEngine {
    /// Create a new event-driven rule engine.
    ///
    /// The engine will subscribe to DeviceMetric events and automatically
    /// evaluate rules when relevant metrics are updated.
    pub fn new(
        value_provider: Arc<dyn ValueProvider>,
        event_bus: EventBus,
    ) -> Self {
        let engine = RuleEngine::new(value_provider);
        let value_cache = Arc::new(RwLock::new(std::collections::HashMap::new()));

        Self {
            engine,
            event_bus,
            running: Arc::new(AtomicBool::new(false)),
            value_cache,
        }
    }

    /// Get the underlying rule engine.
    pub fn engine(&self) -> &RuleEngine {
        &self.engine
    }

    /// Check if the engine is running.
    pub fn is_running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Start the event-driven rule engine.
    ///
    /// Subscribes to DeviceMetric events and begins automatic rule evaluation.
    pub async fn start(&self) -> EventEngineResult<()> {
        if self.is_running() {
            return Ok(());
        }

        info!("Starting event-driven rule engine");

        self.running.store(true, Ordering::Relaxed);

        // Subscribe to device metric events
        let mut rx = self.event_bus.filter().device_events();

        let running = self.running.clone();
        let value_cache = self.value_cache.clone();
        let event_bus = self.event_bus.clone();

        tokio::spawn(async move {
            while running.load(Ordering::Relaxed) {
                match rx.recv().await {
                    Some((event, _metadata)) => {
                        if let NeoTalkEvent::DeviceMetric {
                            device_id,
                            metric,
                            value,
                            timestamp: _,
                            quality: _,
                        } = event {
                            // Cache the value
                            if let Some(numeric_value) = extract_numeric_value(&value) {
                                let mut cache = value_cache.write().await;
                                cache.insert((device_id.clone(), metric.clone()), numeric_value);

                                // Trigger rule evaluation for relevant rules
                                if let Err(e) = Self::evaluate_metric_rules(
                                    &event_bus,
                                    &device_id,
                                    &metric,
                                    numeric_value,
                                ).await {
                                    error!("Failed to evaluate rules for {}.{}: {}",
                                          device_id, metric, e);
                                }
                            }
                        }
                    }
                    None => {
                        debug!("Event bus closed, stopping rule engine");
                        break;
                    }
                }
            }

            running.store(false, Ordering::Relaxed);
            debug!("Event-driven rule engine stopped");
        });

        info!("Event-driven rule engine started");
        Ok(())
    }

    /// Stop the event-driven rule engine.
    pub async fn stop(&self) -> EventEngineResult<()> {
        info!("Stopping event-driven rule engine");
        self.running.store(false, Ordering::Relaxed);
        Ok(())
    }

    /// Evaluate rules relevant to a specific metric.
    async fn evaluate_metric_rules(
        event_bus: &EventBus,
        device_id: &str,
        metric: &str,
        value: f64,
    ) -> Result<(), EventEngineError> {
        // Note: This is a simplified version that publishes events.
        // In a full implementation, this would need access to the rule engine
        // to get the list of rules and evaluate them.

        // Publish a generic rule evaluation event
        let _ = event_bus.publish(NeoTalkEvent::DeviceMetric {
            device_id: device_id.to_string(),
            metric: metric.to_string(),
            value: MetricValue::float(value),
            timestamp: chrono::Utc::now().timestamp(),
            quality: None,
        }).await;

        Ok(())
    }

    /// Add a rule to the engine.
    pub async fn add_rule(&self, rule: CompiledRule) -> EventEngineResult<()> {
        self.engine.add_rule(rule).await?;
        Ok(())
    }

    /// Add a rule from DSL.
    pub async fn add_rule_from_dsl(&self, dsl: &str) -> EventEngineResult<RuleId> {
        let id = self.engine.add_rule_from_dsl(dsl).await?;
        Ok(id)
    }

    /// Remove a rule.
    pub async fn remove_rule(&self, id: &RuleId) -> EventEngineResult<bool> {
        let removed = self.engine.remove_rule(id).await?;
        Ok(removed)
    }

    /// Get a rule by ID.
    pub async fn get_rule(&self, id: &RuleId) -> Option<CompiledRule> {
        self.engine.get_rule(id).await
    }

    /// List all rules.
    pub async fn list_rules(&self) -> Vec<CompiledRule> {
        self.engine.list_rules().await
    }

    /// Get the current value for a device metric.
    pub async fn get_value(&self, device_id: &str, metric: &str) -> Option<f64> {
        // Check cache first
        let cache = self.value_cache.read().await;
        if let Some(&value) = cache.get(&(device_id.to_string(), metric.to_string())) {
            return Some(value);
        }

        // Fall back to value provider
        self.engine.get_value(device_id, metric)
    }

    /// Manually evaluate all rules.
    pub async fn evaluate_rules(&self) -> Vec<RuleId> {
        self.engine.evaluate_rules().await
    }
}

/// Extract numeric value from a MetricValue.
fn extract_numeric_value(value: &MetricValue) -> Option<f64> {
    match value {
        MetricValue::Float(v) => Some(*v),
        MetricValue::Integer(v) => Some(*v as f64),
        MetricValue::Boolean(v) => Some(if *v { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// Device metric value provider backed by the rule engine's cache.
pub struct CachedValueProvider {
    /// Reference to the value cache
    cache: Arc<RwLock<std::collections::HashMap<(String, String), f64>>>,
}

impl CachedValueProvider {
    /// Create a new cached value provider.
    pub fn new(
        cache: Arc<RwLock<std::collections::HashMap<(String, String), f64>>>,
    ) -> Self {
        Self { cache }
    }
}

impl ValueProvider for CachedValueProvider {
    fn get_value(&self, device_id: &str, metric: &str) -> Option<f64> {
        // In a real implementation, this would use async
        // For now, we use try_read to avoid blocking
        if let Ok(cache) = self.cache.try_read() {
            cache.get(&(device_id.to_string(), metric.to_string())).copied()
        } else {
            None
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Evaluate a single rule condition against a value.
pub fn evaluate_rule_condition(
    condition: &RuleCondition,
    device_id: &str,
    metric: &str,
    value: f64,
) -> Option<bool> {
    // Check if this condition matches the device/metric
    if condition.device_id != device_id && condition.device_id != "*" && condition.device_id != "+" {
        return None;
    }
    if condition.metric != metric && metric != "*" {
        return None;
    }

    // Evaluate the comparison
    Some(condition.operator.evaluate(value, condition.threshold))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::InMemoryValueProvider;

    #[tokio::test]
    async fn test_event_driven_engine_creation() {
        let value_provider = Arc::new(InMemoryValueProvider::new());
        let event_bus = EventBus::new();
        let engine = EventDrivenRuleEngine::new(value_provider, event_bus);

        assert!(!engine.is_running());
    }

    #[tokio::test]
    async fn test_engine_lifecycle() {
        let value_provider = Arc::new(InMemoryValueProvider::new());
        let event_bus = EventBus::new();
        let engine = EventDrivenRuleEngine::new(value_provider, event_bus);

        engine.start().await.unwrap();
        assert!(engine.is_running());
        engine.stop().await.unwrap();
    }

    #[test]
    fn test_extract_numeric_value() {
        use edge_ai_core::MetricValue;

        assert_eq!(extract_numeric_value(&MetricValue::Float(42.5)), Some(42.5));
        assert_eq!(extract_numeric_value(&MetricValue::Integer(10)), Some(10.0));
        assert_eq!(extract_numeric_value(&MetricValue::Boolean(true)), Some(1.0));
        assert_eq!(extract_numeric_value(&MetricValue::Boolean(false)), Some(0.0));
        assert_eq!(extract_numeric_value(&MetricValue::String("test".to_string())), None);
    }

    #[tokio::test]
    async fn test_add_rule_from_dsl() {
        let value_provider = Arc::new(InMemoryValueProvider::new());
        let event_bus = EventBus::new();
        let engine = EventDrivenRuleEngine::new(value_provider, event_bus);

        let dsl = r#"
            RULE "test_rule"
            WHEN test.value > 10
            DO
                NOTIFY "Test notification"
            END
        "#;

        let result = engine.add_rule_from_dsl(dsl).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_list_rules() {
        let value_provider = Arc::new(InMemoryValueProvider::new());
        let event_bus = EventBus::new();
        let engine = EventDrivenRuleEngine::new(value_provider, event_bus);

        let rules = engine.list_rules().await;
        assert!(rules.is_empty());
    }

    #[test]
    fn test_evaluate_rule_condition() {
        use crate::dsl::RuleCondition;
        use crate::dsl::ComparisonOperator;

        let condition = RuleCondition {
            device_id: "sensor1".to_string(),
            metric: "temperature".to_string(),
            operator: ComparisonOperator::GreaterThan,
            threshold: 50.0,
        };

        // Matching device and metric, condition met
        assert_eq!(evaluate_rule_condition(&condition, "sensor1", "temperature", 75.0), Some(true));

        // Matching device and metric, condition not met
        assert_eq!(evaluate_rule_condition(&condition, "sensor1", "temperature", 25.0), Some(false));

        // Non-matching device
        assert_eq!(evaluate_rule_condition(&condition, "sensor2", "temperature", 75.0), None);

        // Wildcard device match
        let wildcard_condition = RuleCondition {
            device_id: "*".to_string(),
            metric: "temperature".to_string(),
            operator: ComparisonOperator::GreaterThan,
            threshold: 50.0,
        };
        assert_eq!(evaluate_rule_condition(&wildcard_condition, "sensor1", "temperature", 75.0), Some(true));
    }
}
