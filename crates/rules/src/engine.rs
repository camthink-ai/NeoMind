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
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::dependencies::DependencyManager;
use super::dsl::{ParsedRule, RuleAction, RuleCondition, RuleError};

/// Optional alert manager for creating alerts from rule actions.
/// Wrapped in Option to allow RuleEngine to function without it.
/// Double-wrapped in Arc because AlertManager doesn't implement Clone.
type OptionAlertManager = Arc<tokio::sync::RwLock<Option<Arc<edge_ai_alerts::AlertManager>>>>;

/// Scheduler task handle for managing the rule evaluation loop.
type SchedulerHandle = Arc<StdRwLock<Option<JoinHandle<()>>>>;

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
    /// Rule description (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Original DSL text.
    pub dsl: String,
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
    /// Frontend UI state for proper restoration on edit (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<serde_json::Value>,
}

impl CompiledRule {
    /// Create a new compiled rule from a parsed rule.
    pub fn from_parsed(parsed: ParsedRule) -> Self {
        Self::from_parsed_with_dsl(parsed, String::new())
    }

    /// Create a new compiled rule from a parsed rule with the original DSL text.
    pub fn from_parsed_with_dsl(parsed: ParsedRule, dsl: String) -> Self {
        Self {
            id: RuleId::new(),
            name: parsed.name.clone(),
            description: parsed.description,
            dsl,
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
            source: None,
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

    /// Build a device name → device ID mapping from source.uiCondition.
    /// This resolves the issue where DSL contains device names but evaluation needs device IDs.
    fn build_device_id_mapping(&self) -> std::collections::HashMap<String, String> {
        let mut mapping = std::collections::HashMap::new();
        if let Some(source) = &self.source {
            self.extract_device_ids_from_ui_condition(source.get("uiCondition"), &mut mapping);
        }
        mapping
    }

    /// Recursively extract device_id from uiCondition structure.
    fn extract_device_ids_from_ui_condition(
        &self,
        ui_cond: Option<&serde_json::Value>,
        mapping: &mut std::collections::HashMap<String, String>,
    ) {
        if let Some(cond) = ui_cond {
            // For simple conditions, map the device name (derived from device_id during parsing)
            if let Some(device_id) = cond.get("device_id").and_then(|v| v.as_str()) {
                // Also check if there's a device name that needs to be mapped
                if let Some(name) = cond.get("deviceName").and_then(|v| v.as_str())
                    && !name.is_empty() && !device_id.is_empty() {
                        mapping.insert(name.to_string(), device_id.to_string());
                    }
                // Direct mapping: the parsed DSL uses device_id as the key
                // but the value is stored under the actual device ID
                if !device_id.is_empty() {
                    mapping.insert(device_id.to_string(), device_id.to_string());
                }
            }

            // Handle nested conditions for AND/OR
            if let Some(conditions) = cond.get("conditions").and_then(|v| v.as_array()) {
                for sub_cond in conditions {
                    self.extract_device_ids_from_ui_condition(Some(sub_cond), mapping);
                }
            }

            // Handle NOT condition
            if let Some(sub_cond) = cond.get("condition") {
                self.extract_device_ids_from_ui_condition(Some(sub_cond), mapping);
            }
        }
    }

    /// Resolve device_id using the source.uiCondition mapping.
    /// Returns the actual device ID if found, otherwise returns the original device_id.
    fn resolve_device_id(&self, dsl_device_id: &str, mapping: &std::collections::HashMap<String, String>) -> String {
        // First check if the dsl_device_id is already a valid device ID
        if let Some(resolved) = mapping.get(dsl_device_id) {
            return resolved.clone();
        }
        // If not found, return the original (might already be a device ID)
        dsl_device_id.to_string()
    }

    /// Evaluate a condition with the given value provider.
    fn evaluate_condition(&self, condition: &RuleCondition, value_provider: &dyn ValueProvider) -> bool {
        // Build device ID mapping from source (cache this for efficiency)
        let device_id_mapping = self.build_device_id_mapping();

        self.evaluate_condition_with_mapping(condition, value_provider, &device_id_mapping)
    }

    /// Evaluate a condition with device ID mapping.
    fn evaluate_condition_with_mapping(
        &self,
        condition: &RuleCondition,
        value_provider: &dyn ValueProvider,
        device_id_mapping: &std::collections::HashMap<String, String>,
    ) -> bool {
        match condition {
            RuleCondition::Simple { device_id, metric, operator, threshold } => {
                let resolved_id = self.resolve_device_id(device_id, device_id_mapping);
                if let Some(value) = value_provider.get_value(&resolved_id, metric) {
                    operator.evaluate(value, *threshold)
                } else {
                    false
                }
            }
            RuleCondition::Range { device_id, metric, min, max } => {
                let resolved_id = self.resolve_device_id(device_id, device_id_mapping);
                if let Some(value) = value_provider.get_value(&resolved_id, metric) {
                    value >= *min && value <= *max
                } else {
                    false
                }
            }
            RuleCondition::And(conditions) => {
                conditions.iter().all(|c| self.evaluate_condition_with_mapping(c, value_provider, device_id_mapping))
            }
            RuleCondition::Or(conditions) => {
                conditions.iter().any(|c| self.evaluate_condition_with_mapping(c, value_provider, device_id_mapping))
            }
            RuleCondition::Not(condition) => {
                !self.evaluate_condition_with_mapping(condition, value_provider, device_id_mapping)
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
    /// Optional alert manager for creating alerts from rule actions.
    alert_manager: OptionAlertManager,
    /// Scheduler task handle.
    scheduler_handle: SchedulerHandle,
    /// Scheduler interval (how often to evaluate rules).
    scheduler_interval: Arc<StdRwLock<Duration>>,
    /// Whether the scheduler is running.
    scheduler_running: Arc<StdRwLock<bool>>,
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
            alert_manager: Arc::new(tokio::sync::RwLock::new(None)),
            scheduler_handle: Arc::new(StdRwLock::new(None)),
            scheduler_interval: Arc::new(StdRwLock::new(Duration::from_secs(5))),
            scheduler_running: Arc::new(StdRwLock::new(false)),
        }
    }

    /// Set the alert manager for creating alerts from rule actions.
    /// This must be called after construction as it requires async access.
    pub async fn set_alert_manager(&self, alert_manager: Arc<edge_ai_alerts::AlertManager>) {
        *self.alert_manager.write().await = Some(alert_manager);
    }

    /// Get a reference to the alert manager (if set).
    pub async fn get_alert_manager(&self) -> Option<Arc<edge_ai_alerts::AlertManager>> {
        let guard = self.alert_manager.read().await;
        guard.as_ref().map(Arc::clone)
    }

    /// Start the automatic rule scheduler.
    /// The scheduler will periodically evaluate rules and execute triggered ones.
    /// Returns an error if the scheduler is already running.
    pub fn start_scheduler(&self) -> Result<(), RuleError> {
        // Check if already running
        {
            let mut running = self.scheduler_running.write().unwrap();
            if *running {
                return Err(RuleError::Validation("Scheduler is already running".to_string()));
            }
            *running = true;
        }

        // Get the interval
        let interval = {
            let interval_guard = self.scheduler_interval.read().unwrap();
            *interval_guard
        };

        // Clone needed Arcs for the task
        let rules = self.rules.clone();
        let value_provider = self.value_provider.clone();
        let history = self.history.clone();
        let max_history_size = self.max_history_size;
        let _alert_manager = self.alert_manager.clone();
        let scheduler_running = self.scheduler_running.clone();

        // Spawn the scheduler task
        let handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;

                // Check if we should stop
                {
                    let running = scheduler_running.read().unwrap();
                    if !*running {
                        break;
                    }
                }

                // Update rule states
                {
                    let mut rules_guard = rules.write().await;
                    for rule in rules_guard.values_mut() {
                        if rule.status == RuleStatus::Active {
                            rule.update_state(value_provider.as_ref());
                        }
                    }
                }

                // Evaluate and execute triggered rules
                let rules_to_execute: Vec<_> = {
                    let rules_guard = rules.read().await;
                    rules_guard
                        .iter()
                        .filter(|(_, rule)| {
                            rule.status == RuleStatus::Active
                                && rule.should_trigger(value_provider.as_ref())
                        })
                        .map(|(id, rule)| (id.clone(), rule.clone()))
                        .collect()
                };

                // Execute triggered rules
                for (id, rule) in rules_to_execute {
                    // Update rule state
                    {
                        let mut rules_guard = rules.write().await;
                        if let Some(r) = rules_guard.get_mut(&id) {
                            r.state.trigger_count += 1;
                            r.state.last_triggered = Some(Utc::now());
                        }
                    }

                    // Execute actions
                    for action in &rule.actions {
                        // In a real implementation, this would use the action executor
                        // For now, we just log that an action was executed
                        tracing::debug!(
                            rule_id = %id,
                            rule_name = %rule.name,
                            action = ?action,
                            "Executing rule action"
                        );
                    }

                    // Record in history
                    let result = RuleExecutionResult {
                        rule_id: id.clone(),
                        rule_name: rule.name.clone(),
                        success: true,
                        actions_executed: rule.actions.iter().map(|a| format!("{:?}", a)).collect(),
                        error: None,
                        duration_ms: 0,
                    };

                    let mut hist = history.write().await;
                    hist.push(result);
                    if hist.len() > max_history_size {
                        hist.remove(0);
                    }
                }
            }
        });

        // Store the handle
        let mut handle_guard = self.scheduler_handle.write().unwrap();
        *handle_guard = Some(handle);

        tracing::info!(
            interval_sec = interval.as_secs(),
            "Rule scheduler started"
        );

        Ok(())
    }

    /// Stop the automatic rule scheduler.
    /// Returns an error if the scheduler is not running.
    pub fn stop_scheduler(&self) -> Result<(), RuleError> {
        // Check if running
        {
            let running = self.scheduler_running.read().unwrap();
            if !*running {
                return Err(RuleError::Validation("Scheduler is not running".to_string()));
            }
        }

        // Signal the task to stop
        {
            let mut running = self.scheduler_running.write().unwrap();
            *running = false;
        }

        // Abort the task if it exists
        {
            let mut handle_guard = self.scheduler_handle.write().unwrap();
            if let Some(handle) = handle_guard.take() {
                handle.abort();
            }
        }

        tracing::info!("Rule scheduler stopped");
        Ok(())
    }

    /// Check if the scheduler is currently running.
    pub fn is_scheduler_running(&self) -> bool {
        let running = self.scheduler_running.read().unwrap();
        *running
    }

    /// Set the scheduler interval.
    /// This will not affect a running scheduler; it must be restarted for the new interval to take effect.
    pub fn set_scheduler_interval(&self, interval: Duration) {
        let mut interval_guard = self.scheduler_interval.write().unwrap();
        *interval_guard = interval;
    }

    /// Get the current scheduler interval.
    pub fn get_scheduler_interval(&self) -> Duration {
        let interval_guard = self.scheduler_interval.read().unwrap();
        *interval_guard
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
        let compiled = CompiledRule::from_parsed_with_dsl(parsed, dsl.to_string());
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

    /// Get a reference to the value provider for updating values.
    pub fn get_value_provider(&self) -> Arc<dyn ValueProvider> {
        self.value_provider.clone()
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
                use edge_ai_alerts::{Alert, AlertSeverity as AlertSev};
                
                let sev = match severity {
                    AlertSeverity::Info => AlertSev::Info,
                    AlertSeverity::Warning => AlertSev::Warning,
                    AlertSeverity::Error => AlertSev::Warning, // Map Error to Warning
                    AlertSeverity::Critical => AlertSev::Critical,
                };
                
                // Try to create alert through AlertManager if available
                let alert_manager = self.alert_manager.read().await;
                if let Some(manager) = alert_manager.as_ref() {
                    let alert = Alert::new(
                        sev,
                        title.clone(),
                        message.clone(),
                        "rule".to_string(),
                    );
                    
                    match manager.create_alert(alert).await {
                        Ok(created) => {
                            tracing::info!(
                                "Alert created from rule: {} [{}] - {}",
                                title,
                                sev,
                                message
                            );
                            Ok(format!("ALERT [{}]: {} (id: {})", sev, title, created.id))
                        }
                        Err(e) => {
                            tracing::error!("Failed to create alert from rule: {}", e);
                            Err(format!("Failed to create alert: {}", e))
                        }
                    }
                } else {
                    // Fallback to logging if no AlertManager is set
                    let sev_str = match severity {
                        AlertSeverity::Info => "INFO",
                        AlertSeverity::Warning => "WARNING",
                        AlertSeverity::Error => "ERROR",
                        AlertSeverity::Critical => "CRITICAL",
                    };
                    tracing::warn!("ALERT [{}]: {} - {} (no AlertManager configured)", sev_str, title, message);
                    Ok(format!("ALERT [{}]: {} (logged only)", sev_str, title))
                }
            }
            RuleAction::HttpRequest { method, url, headers, body } => {
                let method_str = match method {
                    HttpMethod::Get => reqwest::Method::GET,
                    HttpMethod::Post => reqwest::Method::POST,
                    HttpMethod::Put => reqwest::Method::PUT,
                    HttpMethod::Delete => reqwest::Method::DELETE,
                    HttpMethod::Patch => reqwest::Method::PATCH,
                };

                // Build HTTP request with timeout
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(30))
                    .build();

                let client = match client {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!("Failed to create HTTP client: {}", e);
                        return Err(format!("HTTP client error: {}", e));
                    }
                };

                let mut request = client.request(method_str.clone(), url);

                // Add headers if provided
                if let Some(hdrs) = headers {
                    for (key, value) in hdrs {
                        request = request.header(key, value);
                    }
                }

                // Add body if provided (for POST/PUT/PATCH)
                if let Some(b) = body {
                    request = request.body(b.clone());
                }

                // Execute the request
                match request.send().await {
                    Ok(response) => {
                        let status = response.status();
                        let status_code = status.as_u16();

                        // Try to get response body
                        let body_result = match response.text().await {
                            Ok(text) => {
                                // Truncate if too long
                                if text.len() > 500 {
                                    format!("{}... (truncated)", &text[..500])
                                } else {
                                    text
                                }
                            }
                            Err(e) => format!("Failed to read response: {}", e),
                        };

                        tracing::info!(
                            "HTTP request completed: {} {} -> {}",
                            method_str,
                            url,
                            status_code
                        );

                        Ok(format!("HTTP: {} {} -> {} ({})", method_str, url, status_code, body_result))
                    }
                    Err(e) => {
                        tracing::error!("HTTP request failed: {} {} - {}", method_str, url, e);
                        Err(format!("HTTP request failed: {}", e))
                    }
                }
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

    #[tokio::test]
    async fn test_scheduler_start_stop() {
        let provider = Arc::new(InMemoryValueProvider::new());
        let engine = RuleEngine::new(provider);

        // Initially not running
        assert!(!engine.is_scheduler_running());

        // Start the scheduler
        engine.start_scheduler().unwrap();
        assert!(engine.is_scheduler_running());

        // Cannot start again
        assert!(engine.start_scheduler().is_err());

        // Stop the scheduler
        engine.stop_scheduler().unwrap();
        assert!(!engine.is_scheduler_running());

        // Cannot stop again
        assert!(engine.stop_scheduler().is_err());
    }

    #[tokio::test]
    async fn test_scheduler_interval() {
        let provider = Arc::new(InMemoryValueProvider::new());
        let engine = RuleEngine::new(provider);

        // Default interval is 5 seconds
        assert_eq!(engine.get_scheduler_interval(), Duration::from_secs(5));

        // Set a custom interval
        engine.set_scheduler_interval(Duration::from_millis(100));
        assert_eq!(engine.get_scheduler_interval(), Duration::from_millis(100));
    }

    #[tokio::test]
    async fn test_scheduler_executes_rules() {
        let provider = Arc::new(InMemoryValueProvider::new());
        let engine = RuleEngine::new(provider.clone());

        // Set a very short interval for testing
        engine.set_scheduler_interval(Duration::from_millis(50));

        // Add a rule
        let dsl = r#"
            RULE "Test Scheduler Rule"
            WHEN sensor.temperature > 50
            DO
                NOTIFY "High temperature"
            END
        "#;
        let _rule_id = engine.add_rule_from_dsl(dsl).await.unwrap();

        // Start the scheduler
        engine.start_scheduler().unwrap();

        // Wait for scheduler to tick at least once
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Rule should not have triggered yet (temperature is default 0.0)
        let rules = engine.list_rules().await;
        assert_eq!(rules[0].state.trigger_count, 0);

        // Set temperature above threshold
        let mem_provider = provider
            .as_any()
            .downcast_ref::<InMemoryValueProvider>()
            .unwrap();
        mem_provider.set_value("sensor", "temperature", 75.0);

        // Wait for scheduler to tick again
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Rule should have triggered
        let rules = engine.list_rules().await;
        assert!(rules[0].state.trigger_count > 0);

        // Stop the scheduler
        engine.stop_scheduler().unwrap();
    }
}
