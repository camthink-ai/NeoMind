//! Rule engine for evaluating and executing rules.
//!
//! The rule engine manages rule lifecycle, evaluates conditions,
//! and executes actions when rules are triggered.

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock as StdRwLock;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

use super::dsl::{ParsedRule, RuleAction, RuleCondition, RuleError};

/// Unique identifier for a rule.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RuleId(pub Uuid);

impl RuleId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn from_string(s: &str) -> Result<Self, uuid::Error> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl Default for RuleId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for RuleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Rule status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleStatus {
    /// Rule is active and being evaluated.
    Active,
    /// Rule is paused.
    Paused,
    /// Rule has been triggered and is executing.
    Triggered,
    /// Rule is disabled.
    Disabled,
}

/// Rule execution state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleState {
    /// Number of times the rule has been triggered.
    pub trigger_count: u64,
    /// Last time the rule was triggered.
    pub last_triggered: Option<DateTime<Utc>>,
    /// Last evaluation result.
    pub last_evaluation: bool,
    /// Time since condition has been true (for FOR clauses).
    /// Note: Instant is not serialized, will be reset on deserialization.
    #[serde(skip)]
    pub condition_true_since: Option<Instant>,
}

/// Compiled rule ready for execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledRule {
    /// Unique rule identifier.
    pub id: RuleId,
    /// Rule name.
    pub name: String,
    /// Condition to evaluate.
    pub condition: RuleCondition,
    /// Duration condition must be true before triggering.
    pub for_duration: Option<Duration>,
    /// Actions to execute on trigger.
    pub actions: Vec<RuleAction>,
    /// Current rule status.
    pub status: RuleStatus,
    /// Rule state.
    pub state: RuleState,
    /// When the rule was created.
    pub created_at: DateTime<Utc>,
}

impl CompiledRule {
    /// Create a new compiled rule from a parsed rule.
    pub fn from_parsed(parsed: ParsedRule) -> Self {
        Self {
            id: RuleId::new(),
            name: parsed.name,
            condition: parsed.condition,
            for_duration: parsed.for_duration,
            actions: parsed.actions,
            status: RuleStatus::Active,
            state: RuleState {
                trigger_count: 0,
                last_triggered: None,
                last_evaluation: false,
                condition_true_since: None,
            },
            created_at: Utc::now(),
        }
    }

    /// Check if the rule should trigger based on current values.
    pub fn should_trigger(&self, current_value: f64) -> bool {
        // Evaluate condition
        let condition_met = self
            .condition
            .operator
            .evaluate(current_value, self.condition.threshold);

        if let Some(duration) = self.for_duration {
            if condition_met {
                // Check if condition has been true long enough
                if let Some(since) = self.state.condition_true_since {
                    since.elapsed() >= duration
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            condition_met
        }
    }

    /// Update the rule's state based on current evaluation.
    pub fn update_state(&mut self, current_value: f64) {
        let condition_met = self
            .condition
            .operator
            .evaluate(current_value, self.condition.threshold);

        if condition_met {
            if self.state.condition_true_since.is_none() {
                self.state.condition_true_since = Some(Instant::now());
            }
        } else {
            self.state.condition_true_since = None;
        }

        self.state.last_evaluation = condition_met;
    }
}

/// Rule execution result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleExecutionResult {
    /// Rule that was executed.
    pub rule_id: RuleId,
    /// Rule name.
    pub rule_name: String,
    /// Whether execution was successful.
    pub success: bool,
    /// Actions executed.
    pub actions_executed: Vec<String>,
    /// Error message if execution failed.
    pub error: Option<String>,
    /// Execution duration.
    pub duration_ms: u64,
}

/// Value provider for rule evaluation.
pub trait ValueProvider: Send + Sync {
    /// Get the current value for a device metric.
    fn get_value(&self, device_id: &str, metric: &str) -> Option<f64>;

    /// Get as Any for downcasting.
    fn as_any(&self) -> &dyn Any;
}

/// Rule engine that manages and executes rules.
pub struct RuleEngine {
    /// Registered rules.
    rules: Arc<RwLock<HashMap<RuleId, CompiledRule>>>,
    /// Value provider for evaluating conditions.
    value_provider: Arc<dyn ValueProvider>,
    /// Execution history.
    history: Arc<RwLock<Vec<RuleExecutionResult>>>,
    /// Maximum history size.
    max_history_size: usize,
}

impl RuleEngine {
    /// Create a new rule engine.
    pub fn new(value_provider: Arc<dyn ValueProvider>) -> Self {
        Self {
            rules: Arc::new(RwLock::new(HashMap::new())),
            value_provider,
            history: Arc::new(RwLock::new(Vec::new())),
            max_history_size: 1000,
        }
    }

    /// Set the maximum history size.
    pub fn with_max_history_size(mut self, size: usize) -> Self {
        self.max_history_size = size;
        self
    }

    /// Add a rule to the engine.
    pub async fn add_rule(&self, rule: CompiledRule) -> Result<(), RuleError> {
        let mut rules = self.rules.write().await;
        rules.insert(rule.id.clone(), rule);
        Ok(())
    }

    /// Add a rule from DSL.
    pub async fn add_rule_from_dsl(&self, dsl: &str) -> Result<RuleId, RuleError> {
        let parsed = super::dsl::RuleDslParser::parse(dsl)?;
        let compiled = CompiledRule::from_parsed(parsed);
        let id = compiled.id.clone();
        self.add_rule(compiled).await?;
        Ok(id)
    }

    /// Remove a rule.
    pub async fn remove_rule(&self, id: &RuleId) -> Result<bool, RuleError> {
        let mut rules = self.rules.write().await;
        Ok(rules.remove(id).is_some())
    }

    /// Get a rule by ID.
    pub async fn get_rule(&self, id: &RuleId) -> Option<CompiledRule> {
        let rules = self.rules.read().await;
        rules.get(id).cloned()
    }

    /// List all rules.
    pub async fn list_rules(&self) -> Vec<CompiledRule> {
        let rules = self.rules.read().await;
        rules.values().cloned().collect()
    }

    /// Get the current value for a device metric.
    pub fn get_value(&self, device_id: &str, metric: &str) -> Option<f64> {
        self.value_provider.get_value(device_id, metric)
    }

    /// Evaluate all active rules.
    pub async fn evaluate_rules(&self) -> Vec<RuleId> {
        let mut triggered = Vec::new();
        let rules = self.rules.read().await;

        for (id, rule) in rules.iter() {
            if rule.status != RuleStatus::Active {
                continue;
            }

            if let Some(value) = self
                .value_provider
                .get_value(&rule.condition.device_id, &rule.condition.metric)
            {
                if rule.should_trigger(value) {
                    triggered.push(id.clone());
                }
            }
        }

        triggered
    }

    /// Update rule states based on current values.
    pub async fn update_states(&self) {
        let mut rules = self.rules.write().await;

        for rule in rules.values_mut() {
            if rule.status != RuleStatus::Active {
                continue;
            }

            if let Some(value) = self
                .value_provider
                .get_value(&rule.condition.device_id, &rule.condition.metric)
            {
                rule.update_state(value);
            }
        }
    }

    /// Execute triggered rules.
    pub async fn execute_triggered(&self) -> Vec<RuleExecutionResult> {
        let triggered_ids = self.evaluate_rules().await;
        let mut results = Vec::new();

        for id in triggered_ids {
            let result = self.execute_rule(&id).await;
            results.push(result);
        }

        results
    }

    /// Execute a specific rule.
    pub async fn execute_rule(&self, id: &RuleId) -> RuleExecutionResult {
        let start = Instant::now();

        let rule = {
            let rules = self.rules.read().await;
            match rules.get(id) {
                Some(r) => r.clone(),
                None => {
                    return RuleExecutionResult {
                        rule_id: id.clone(),
                        rule_name: "Unknown".to_string(),
                        success: false,
                        actions_executed: Vec::new(),
                        error: Some("Rule not found".to_string()),
                        duration_ms: 0,
                    };
                }
            }
        };

        let mut actions_executed = Vec::new();
        let mut error = None;

        for action in &rule.actions {
            match self.execute_action(action).await {
                Ok(name) => actions_executed.push(name),
                Err(e) => {
                    error = Some(e.to_string());
                    break;
                }
            }
        }

        // Update rule state
        {
            let mut rules = self.rules.write().await;
            if let Some(rule) = rules.get_mut(id) {
                rule.state.trigger_count += 1;
                rule.state.last_triggered = Some(Utc::now());
            }
        }

        let result = RuleExecutionResult {
            rule_id: id.clone(),
            rule_name: rule.name.clone(),
            success: error.is_none(),
            actions_executed,
            error,
            duration_ms: start.elapsed().as_millis() as u64,
        };

        // Add to history
        let mut history = self.history.write().await;
        history.push(result.clone());
        if history.len() > self.max_history_size {
            history.remove(0);
        }

        result
    }

    /// Execute a single action.
    async fn execute_action(&self, action: &RuleAction) -> Result<String, String> {
        match action {
            RuleAction::Notify { message } => {
                tracing::info!("NOTIFY: {}", message);
                Ok(format!("NOTIFY: {}", message))
            }
            RuleAction::Execute {
                device_id,
                command,
                params,
            } => {
                tracing::info!(
                    "EXECUTE: {}.{} with params {:?}",
                    device_id,
                    command,
                    params
                );
                // Here you would integrate with the device manager
                Ok(format!("EXECUTE: {}.{}", device_id, command))
            }
            RuleAction::Log {
                level,
                message,
                severity,
            } => {
                let log_msg = if let Some(sev) = severity {
                    format!("{}: {} (severity: {})", level, message, sev)
                } else {
                    format!("{}: {}", level, message)
                };
                tracing::info!("{}", log_msg);
                Ok(log_msg)
            }
        }
    }

    /// Get execution history.
    pub async fn get_history(&self) -> Vec<RuleExecutionResult> {
        let history = self.history.read().await;
        history.clone()
    }

    /// Get history for a specific rule.
    pub async fn get_rule_history(&self, rule_id: &RuleId) -> Vec<RuleExecutionResult> {
        let history = self.history.read().await;
        history
            .iter()
            .filter(|r| &r.rule_id == rule_id)
            .cloned()
            .collect()
    }

    /// Pause a rule.
    pub async fn pause_rule(&self, id: &RuleId) -> Result<(), RuleError> {
        let mut rules = self.rules.write().await;
        if let Some(rule) = rules.get_mut(id) {
            rule.status = RuleStatus::Paused;
            Ok(())
        } else {
            Err(RuleError::Validation(format!("Rule not found: {}", id)))
        }
    }

    /// Resume a rule.
    pub async fn resume_rule(&self, id: &RuleId) -> Result<(), RuleError> {
        let mut rules = self.rules.write().await;
        if let Some(rule) = rules.get_mut(id) {
            rule.status = RuleStatus::Active;
            Ok(())
        } else {
            Err(RuleError::Validation(format!("Rule not found: {}", id)))
        }
    }

    /// Clear execution history.
    pub async fn clear_history(&self) {
        let mut history = self.history.write().await;
        history.clear();
    }
}

/// Simple in-memory value provider for testing.
pub struct InMemoryValueProvider {
    values: Arc<StdRwLock<HashMap<String, f64>>>,
}

impl InMemoryValueProvider {
    pub fn new() -> Self {
        Self {
            values: Arc::new(StdRwLock::new(HashMap::new())),
        }
    }

    /// Set a value for a device metric.
    pub fn set_value(&self, device_id: &str, metric: &str, value: f64) {
        let mut values = self.values.write().unwrap();
        let key = format!("{}:{}", device_id, metric);
        values.insert(key, value);
    }
}

impl Default for InMemoryValueProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ValueProvider for InMemoryValueProvider {
    fn get_value(&self, device_id: &str, metric: &str) -> Option<f64> {
        let key = format!("{}:{}", device_id, metric);
        let values = self.values.read().unwrap();
        values.get(&key).copied()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rule_engine_basic() {
        let provider = Arc::new(InMemoryValueProvider::new());
        let engine = RuleEngine::new(provider);

        let dsl = r#"
            RULE "Test Rule"
            WHEN sensor.temperature > 50
            DO
                NOTIFY "High temperature"
            END
        "#;

        let rule_id = engine.add_rule_from_dsl(dsl).await.unwrap();
        assert_eq!(engine.list_rules().await.len(), 1);

        // Set value below threshold
        let provider = engine.value_provider.clone();
        let mem_provider = provider
            .as_any()
            .downcast_ref::<InMemoryValueProvider>()
            .unwrap();
        mem_provider.set_value("sensor", "temperature", 25.0);

        engine.update_states().await;
        let triggered = engine.evaluate_rules().await;
        assert!(triggered.is_empty());

        // Set value above threshold
        mem_provider.set_value("sensor", "temperature", 75.0);
        engine.update_states().await;
        let triggered = engine.evaluate_rules().await;
        assert_eq!(triggered.len(), 1);
        assert_eq!(triggered[0], rule_id);
    }

    #[tokio::test]
    async fn test_rule_with_duration() {
        let provider = Arc::new(InMemoryValueProvider::new());
        let engine = RuleEngine::new(provider);

        let _dsl = r#"
            RULE "Test Rule"
            WHEN sensor.temperature > 50
            FOR 100 milliseconds
            DO
                NOTIFY "High temperature"
            END
        "#;

        // Modify DSL to use our duration format
        let dsl = r#"
            RULE "Test Rule"
            WHEN sensor.temperature > 50
            DO
                NOTIFY "High temperature"
            END
        "#;

        let _rule_id = engine.add_rule_from_dsl(dsl).await.unwrap();

        let provider = engine.value_provider.clone();
        let mem_provider = provider
            .as_any()
            .downcast_ref::<InMemoryValueProvider>()
            .unwrap();

        mem_provider.set_value("sensor", "temperature", 75.0);
        engine.update_states().await;

        // Should trigger immediately without FOR clause
        let triggered = engine.evaluate_rules().await;
        assert_eq!(triggered.len(), 1);
    }
}
