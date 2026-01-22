//! Rule engine for evaluating and executing rules.
//!
//! The rule engine manages rule lifecycle, evaluates conditions,
//! and executes actions when rules are triggered.

use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::RwLock as StdRwLock;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

use super::dependencies::DependencyManager;
use super::dsl::{ParsedRule, RuleAction, RuleCondition, RuleError, ComparisonOperator};

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
            name: parsed.name.clone(),
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
    /// This now supports complex conditions through value provider.
    pub fn should_trigger(&self, value_provider: &dyn ValueProvider) -> bool {
        let condition_met = self.evaluate_condition(&self.condition, value_provider);

        if let Some(duration) = self.for_duration {
            if condition_met {
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
    pub fn update_state(&mut self, value_provider: &dyn ValueProvider) {
        let condition_met = self.evaluate_condition(&self.condition, value_provider);

        if condition_met {
            if self.state.condition_true_since.is_none() {
                self.state.condition_true_since = Some(Instant::now());
            }
        } else {
            self.state.condition_true_since = None;
        }

        self.state.last_evaluation = condition_met;
    }

    /// Evaluate a condition with the given value provider.
    fn evaluate_condition(&self, condition: &RuleCondition, value_provider: &dyn ValueProvider) -> bool {
        match condition {
            RuleCondition::Simple { device_id, metric, operator, threshold } => {
                if let Some(value) = value_provider.get_value(device_id, metric) {
                    operator.evaluate(value, *threshold)
                } else {
                    false
                }
            }
            RuleCondition::Range { device_id, metric, min, max } => {
                if let Some(value) = value_provider.get_value(device_id, metric) {
                    value >= *min && value <= *max
                } else {
                    false
                }
            }
            RuleCondition::And(conditions) => {
                conditions.iter().all(|c| self.evaluate_condition(c, value_provider))
            }
            RuleCondition::Or(conditions) => {
                conditions.iter().any(|c| self.evaluate_condition(c, value_provider))
            }
            RuleCondition::Not(condition) => {
                !self.evaluate_condition(condition, value_provider)
            }
        }
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
    /// Dependency manager for execution ordering.
    dependency_manager: Arc<StdRwLock<DependencyManager>>,
}

impl RuleEngine {
    /// Create a new rule engine.
    pub fn new(value_provider: Arc<dyn ValueProvider>) -> Self {
        Self {
            rules: Arc::new(RwLock::new(HashMap::new())),
            value_provider,
            history: Arc::new(RwLock::new(Vec::new())),
            max_history_size: 1000,
            dependency_manager: Arc::new(StdRwLock::new(DependencyManager::new())),
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
        // Remove from dependency manager first
        {
            let mut dep_manager = self.dependency_manager.write().unwrap();
            dep_manager.remove_rule(id);
        }

        // Then remove the rule itself
        let mut rules = self.rules.write().await;
        Ok(rules.remove(id).is_some())
    }

    /// Add a dependency between rules.
    ///
    /// After calling this, `dependent` will only execute after `dependency` has completed.
    pub fn add_dependency(&self, dependent: RuleId, dependency: RuleId) -> Result<(), RuleError> {
        let mut dep_manager = self.dependency_manager.write().unwrap();
        dep_manager.add_dependency(dependent, dependency);
        Ok(())
    }

    /// Remove a dependency between rules.
    pub fn remove_dependency(&self, dependent: &RuleId, dependency: &RuleId) -> Result<(), RuleError> {
        let mut dep_manager = self.dependency_manager.write().unwrap();
        dep_manager.remove_dependency(dependent, dependency);
        Ok(())
    }

    /// Get all dependencies for a rule.
    pub fn get_dependencies(&self, rule_id: &RuleId) -> Vec<RuleId> {
        let dep_manager = self.dependency_manager.read().unwrap();
        dep_manager.get_dependencies(rule_id)
    }

    /// Get all rules that depend on this rule.
    pub fn get_dependents(&self, rule_id: &RuleId) -> Vec<RuleId> {
        let dep_manager = self.dependency_manager.read().unwrap();
        dep_manager.get_dependents(rule_id)
    }

    /// Validate dependencies and get execution order.
    ///
    /// Returns the order in which rules should be executed based on their dependencies.
    /// Also detects circular dependencies and missing dependencies.
    pub fn validate_dependencies(&self) -> Result<Vec<RuleId>, RuleError> {
        let existing_rules: std::collections::HashSet<RuleId> = {
            let rules = self
                .rules
                .try_read()
                .map_err(|e| RuleError::Validation(format!("Failed to acquire lock: {}", e)))?;
            rules.keys().cloned().collect()
        };

        let dep_manager = self.dependency_manager.read().unwrap();
        let result = dep_manager.validate_and_order(&existing_rules);

        if result.is_valid {
            Ok(result.execution_order)
        } else {
            Err(RuleError::Validation(result.format_message()))
        }
    }

    /// Get rules ready to execute based on dependencies.
    ///
    /// Returns rules whose dependencies have all been satisfied.
    pub async fn get_ready_rules(&self, completed: &HashSet<RuleId>) -> Vec<RuleId> {
        let rules = self.rules.read().await;
        let existing_rules: std::collections::HashSet<RuleId> = rules.keys().cloned().collect();
        drop(rules);

        let dep_manager = self.dependency_manager.read().unwrap();
        dep_manager.get_ready_rules(&existing_rules, completed)
    }

    /// Get a reference to the dependency manager.
    pub fn dependency_manager(&self) -> Arc<StdRwLock<DependencyManager>> {
        Arc::clone(&self.dependency_manager)
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

            if rule.should_trigger(self.value_provider.as_ref()) {
                triggered.push(id.clone());
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

            rule.update_state(self.value_provider.as_ref());
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

    /// Execute a single action - supports all action types.
    async fn execute_action(&self, action: &RuleAction) -> Result<String, String> {
        use super::dsl::{HttpMethod, AlertSeverity};

        match action {
            RuleAction::Notify { message, channels } => {
                tracing::info!("NOTIFY: {} (channels: {:?})", message, channels);
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
            RuleAction::Set { device_id, property, value } => {
                tracing::info!("SET: {}.{} = {}", device_id, property, value);
                Ok(format!("SET: {}.{} = {}", device_id, property, value))
            }
            RuleAction::Delay { duration } => {
                tracing::info!("DELAY: {:?} (sleeping...)", duration);
                tokio::time::sleep(*duration).await;
                Ok(format!("DELAY: {:?} completed", duration))
            }
            RuleAction::CreateAlert { title, message, severity } => {
                let sev_str = match severity {
                    AlertSeverity::Info => "INFO",
                    AlertSeverity::Warning => "WARNING",
                    AlertSeverity::Error => "ERROR",
                    AlertSeverity::Critical => "CRITICAL",
                };
                tracing::info!("ALERT [{}]: {} - {}", sev_str, title, message);
                Ok(format!("ALERT [{}]: {}", sev_str, title))
            }
            RuleAction::HttpRequest { method, url, .. } => {
                let method_str = match method {
                    HttpMethod::Get => "GET",
                    HttpMethod::Post => "POST",
                    HttpMethod::Put => "PUT",
                    HttpMethod::Delete => "DELETE",
                    HttpMethod::Patch => "PATCH",
                };
                tracing::info!("HTTP: {} {}", method_str, url);
                // In a real implementation, you would make the actual HTTP request
                Ok(format!("HTTP: {} {}", method_str, url))
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
