//! Rule engine v2 — event-driven, no polling.
//!
//! The engine evaluates rules **only** when data changes arrive via
//! [`RuleEngine::on_data_update`]. A subscription index maps each
//! `DataSourceId` → relevant `RuleId`s so only affected rules are evaluated.

use std::collections::{HashMap, VecDeque};
use std::panic::{self, AssertUnwindSafe};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use neomind_core::datasource::DataSourceId;
use parking_lot::RwLock as StdRwLock;
use tokio::sync::RwLock;

use crate::device_integration::DeviceActionExecutor;
use crate::error::RuleError;
use crate::extension_integration::ExtensionActionExecutor;
use crate::models::{
    CompiledRule, ExecuteTarget, NotifySeverity, RuleAction, RuleCondition, RuleExecutionResult,
    RuleId, RuleTrigger, RuleValue, ValueProvider,
};
use crate::store::RuleStore;

// ---------------------------------------------------------------------------
// Type aliases for optional dependencies
// ---------------------------------------------------------------------------

type OptionMessageManager = Arc<tokio::sync::RwLock<Option<Arc<neomind_messages::MessageManager>>>>;
type OptionDeviceActionExecutor = Arc<tokio::sync::RwLock<Option<Arc<DeviceActionExecutor>>>>;
type OptionExtensionActionExecutor = Arc<tokio::sync::RwLock<Option<Arc<ExtensionActionExecutor>>>>;

pub type AgentTriggerCallback = Arc<
    dyn Fn(
            String,
            Option<String>,
            Option<serde_json::Value>,
        ) -> Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>
        + Send
        + Sync,
>;
type OptionAgentTriggerCallback = Arc<tokio::sync::RwLock<Option<AgentTriggerCallback>>>;

// ---------------------------------------------------------------------------
// In-memory value provider (testing)
// ---------------------------------------------------------------------------

/// Simple in-memory value provider for testing.
pub struct InMemoryValueProvider {
    values: Arc<StdRwLock<HashMap<String, RuleValue>>>,
}

impl Default for InMemoryValueProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryValueProvider {
    pub fn new() -> Self {
        Self {
            values: Arc::new(StdRwLock::new(HashMap::new())),
        }
    }

    /// Set a numeric value. Key format: `source_type:source_id:field_path`.
    pub fn set_value(&self, source_key: &str, value: f64) {
        let mut values = self.values.write();
        values.insert(source_key.to_string(), RuleValue::Number(value));
    }

    /// Set a string value. Key format: `source_type:source_id:field_path`.
    pub fn set_string_value(&self, source_key: &str, value: &str) {
        let mut values = self.values.write();
        values.insert(source_key.to_string(), RuleValue::Text(value.to_string()));
    }
}

impl ValueProvider for InMemoryValueProvider {
    fn get_by_source(&self, source: &DataSourceId) -> Option<RuleValue> {
        let key = source.storage_key();
        let values = self.values.read();
        values.get(&key).cloned()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// Rule engine
// ---------------------------------------------------------------------------

/// Maximum number of in-memory history entries per engine instance.
const MAX_HISTORY_SIZE: usize = 1000;

/// Event-driven rule engine.
///
/// Rules are evaluated only when [`on_data_update`] is called — no polling.
pub struct RuleEngine {
    /// All registered rules.
    // parking_lot (sync): consumed from both sync and async contexts; the
    // tokio lock forced try_read in the sync rebuild path, which silently
    // SKIPPED the rebuild under trigger-path contention — a rule added in
    // that window never entered subscription_index until the next CRUD.
    rules: Arc<StdRwLock<HashMap<RuleId, CompiledRule>>>,
    /// Subscription index: DataSourceId → Vec<RuleId>
    subscription_index: Arc<StdRwLock<HashMap<String, Arc<Vec<RuleId>>>>>,
    /// Cooldown tracking: RuleId → last trigger Instant
    cooldowns: Arc<StdRwLock<HashMap<RuleId, Instant>>>,
    /// Consecutive all-actions-failed count per rule. Gates the cooldown
    /// refund so a persistently-failing rule paces itself instead of re-firing
    /// on every matching data point.
    consecutive_action_failures: Arc<StdRwLock<HashMap<RuleId, u32>>>,
    /// Value provider for condition evaluation.
    value_provider: Arc<dyn ValueProvider>,
    /// In-memory execution history.
    history: Arc<RwLock<VecDeque<RuleExecutionResult>>>,
    // Optional executors
    message_manager: OptionMessageManager,
    device_action_executor: OptionDeviceActionExecutor,
    extension_action_executor: OptionExtensionActionExecutor,
    agent_trigger: OptionAgentTriggerCallback,
    /// Persistent rule store.
    rule_store: Arc<StdRwLock<Option<Arc<RuleStore>>>>,
}

impl RuleEngine {
    /// Create a new engine.
    pub fn new(value_provider: Arc<dyn ValueProvider>) -> Self {
        Self {
            rules: Arc::new(StdRwLock::new(HashMap::new())),
            subscription_index: Arc::new(StdRwLock::new(HashMap::new())),
            cooldowns: Arc::new(StdRwLock::new(HashMap::new())),
            consecutive_action_failures: Arc::new(StdRwLock::new(HashMap::new())),
            value_provider,
            history: Arc::new(RwLock::new(VecDeque::new())),
            message_manager: Arc::new(tokio::sync::RwLock::new(None)),
            device_action_executor: Arc::new(tokio::sync::RwLock::new(None)),
            extension_action_executor: Arc::new(tokio::sync::RwLock::new(None)),
            agent_trigger: Arc::new(tokio::sync::RwLock::new(None)),
            rule_store: Arc::new(StdRwLock::new(None)),
        }
    }

    // -- Setters for optional dependencies --

    pub fn set_rule_store(&self, store: Arc<RuleStore>) {
        *self.rule_store.write() = Some(store);
    }

    pub async fn set_message_manager(&self, mm: Arc<neomind_messages::MessageManager>) {
        *self.message_manager.write().await = Some(mm);
    }

    pub async fn set_device_action_executor(&self, ex: Arc<DeviceActionExecutor>) {
        *self.device_action_executor.write().await = Some(ex);
    }

    pub async fn set_extension_action_executor(&self, ex: Arc<ExtensionActionExecutor>) {
        *self.extension_action_executor.write().await = Some(ex);
    }

    pub async fn set_agent_trigger_callback(&self, cb: AgentTriggerCallback) {
        *self.agent_trigger.write().await = Some(cb);
    }

    // -- Rule CRUD --

    /// Add a compiled rule. Rebuilds subscription index for the rule.
    pub async fn add_rule(&self, rule: CompiledRule) -> Result<(), RuleError> {
        let id = rule.id.clone();
        let mut rules = self.rules.write();
        rules.insert(id, rule);
        drop(rules);
        // Rebuild after insert so the index reflects the new state
        self.rebuild_all_subscriptions();
        Ok(())
    }

    /// Remove a rule and its subscription entries.
    pub async fn remove_rule(&self, id: &RuleId) -> Result<bool, RuleError> {
        let mut rules = self.rules.write();
        let removed = rules.remove(id).is_some();
        drop(rules);
        if removed {
            self.rebuild_all_subscriptions();
            // Clean up cooldown for removed rule
            self.cooldowns.write().remove(id);
        }
        Ok(removed)
    }

    /// Update an existing rule (or insert if new).
    pub async fn update_rule(&self, rule: CompiledRule) -> Result<(), RuleError> {
        let id = rule.id.clone();
        let mut rules = self.rules.write();
        rules.insert(id, rule);
        drop(rules);
        // Rebuild after insert so the index reflects the new state
        self.rebuild_all_subscriptions();
        Ok(())
    }

    /// Get a rule by ID.
    pub async fn get_rule(&self, id: &RuleId) -> Option<CompiledRule> {
        self.rules.read().get(id).cloned()
    }

    /// List all rules.
    pub async fn list_rules(&self) -> Vec<CompiledRule> {
        self.rules.read().values().cloned().collect()
    }

    /// Enable / disable a rule.
    pub async fn set_enabled(&self, id: &RuleId, enabled: bool) -> Result<(), RuleError> {
        let mut rules = self.rules.write();
        match rules.get_mut(id) {
            Some(rule) => {
                rule.enabled = enabled;
                rule.updated_at = Utc::now();
                Ok(())
            }
            None => Err(RuleError::Validation(format!("Rule not found: {}", id))),
        }
    }

    // -- Subscription index --

    fn rebuild_all_subscriptions(&self) {
        // [contention-safe] parking_lot read — the old tokio try_read silently
        // kept the STALE index whenever the trigger path held the write lock
        // (update_condition_since / update_rule_state_after_trigger take it on
        // every firing), so a rule added during active triggering could exist
        // in `rules` but never in subscription_index — silently unevaluated
        // until some later rule CRUD happened to succeed. A blocking read of
        // a short critical section cannot skip.
        let rules = self.rules.read();
        let mut idx = HashMap::new();
        for rule in rules.values() {
            if let RuleTrigger::DataChange { sources } = &rule.trigger {
                for source in sources {
                    let key = source.storage_key();
                    idx.entry(key)
                        .or_insert_with(Vec::new)
                        .push(rule.id.clone());
                }
            }
        }
        let idx: HashMap<String, Arc<Vec<RuleId>>> =
            idx.into_iter().map(|(k, v)| (k, Arc::new(v))).collect();
        *self.subscription_index.write() = idx;
    }

    /// Return device_ids referenced by any rule whose subscription index entry
    /// matches `device:<id>:<metric_name>`.
    ///
    /// Used by `DeviceStatusEmitter` to know which devices need their virtual metric
    /// refreshed. O(N) over the subscription index (bounded by rule count).
    pub fn subscribed_virtual_metric_devices(&self, metric_name: &str) -> Vec<String> {
        let idx = self.subscription_index.read();
        let suffix = format!(":{}", metric_name);
        let mut devices: Vec<String> = Vec::new();
        for key in idx.keys() {
            // storage_key format for devices is "device:<id>:<metric>"
            if let Some(rest) = key.strip_prefix("device:") {
                if let Some(device_id) = rest.strip_suffix(&suffix) {
                    devices.push(device_id.to_string());
                }
            }
        }
        devices
    }

    // -- Core: data-driven evaluation --

    /// Called when a data source value changes.
    ///
    /// 1. Look up affected rules via subscription index
    /// 2. Check cooldown
    /// 3. Evaluate condition
    /// 4. Check for_duration
    /// 5. Execute actions
    /// 6. Persist history + state
    /// 7. Set cooldown
    pub async fn on_data_update(&self, source: &DataSourceId, _value: RuleValue) {
        let source_key = source.storage_key();

        // 1. Find affected rules
        let affected: Arc<Vec<RuleId>> = {
            let idx = self.subscription_index.read();
            idx.get(&source_key).cloned().unwrap_or_default()
        };

        if affected.is_empty() {
            return;
        }

        for rule_id in &*affected {
            if let Err(e) = self.evaluate_and_fire(rule_id).await {
                tracing::warn!(rule_id = %rule_id, error = %e, "Rule evaluation failed");
            }
        }
    }

    /// Manually trigger a rule by ID (for Manual / Schedule triggers).
    pub async fn execute_rule(&self, id: &RuleId) -> RuleExecutionResult {
        let start = Instant::now();
        let now = Utc::now();

        let rule = {
            let rules = self.rules.read();
            rules.get(id).cloned()
        };

        let Some(rule) = rule else {
            return RuleExecutionResult {
                rule_id: id.clone(),
                rule_name: "Unknown".to_string(),
                success: false,
                actions_executed: Vec::new(),
                error: Some("Rule not found".to_string()),
                duration_ms: 0,
                triggered_at: now,
            };
        };

        // Check cooldown (prevents cron double-firing and rapid manual re-triggers)
        if self.is_in_cooldown(id, &rule) {
            tracing::debug!(rule_id = %id, "Rule execution skipped due to cooldown");
            return RuleExecutionResult {
                rule_id: id.clone(),
                rule_name: rule.name.clone(),
                success: false,
                actions_executed: Vec::new(),
                error: Some("Rule is in cooldown".to_string()),
                duration_ms: 0,
                triggered_at: now,
            };
        }

        // Evaluate condition (if any)
        if let Some(ref cond) = rule.condition {
            let eval_result = panic::catch_unwind(AssertUnwindSafe(|| {
                cond.evaluate(self.value_provider.as_ref())
            }));
            match eval_result {
                Ok(true) => {}
                Ok(false) | Err(_) => {
                    // Reset condition_since on failure
                    self.update_condition_since(id, false).await;
                    return RuleExecutionResult {
                        rule_id: id.clone(),
                        rule_name: rule.name.clone(),
                        success: false,
                        actions_executed: Vec::new(),
                        error: Some("Condition not met".to_string()),
                        duration_ms: start.elapsed().as_millis() as u64,
                        triggered_at: now,
                    };
                }
            }
        }

        // Check for_duration for Schedule triggers (consistent with evaluate_and_fire).
        // Manual triggers skip this — the user explicitly requested immediate execution.
        if let (Some(dur), RuleTrigger::Schedule { .. }) = (rule.for_duration, &rule.trigger) {
            let since = self.update_condition_since(id, true).await;
            match since {
                Some(since_time) => {
                    let elapsed = Utc::now()
                        .signed_duration_since(since_time)
                        .to_std()
                        .unwrap_or(Duration::ZERO);
                    if elapsed < dur {
                        tracing::debug!(
                            rule_id = %id,
                            elapsed_s = elapsed.as_secs(),
                            required_s = dur.as_secs(),
                            "Rule condition not yet sustained for required duration"
                        );
                        return RuleExecutionResult {
                            rule_id: id.clone(),
                            rule_name: rule.name.clone(),
                            success: false,
                            actions_executed: Vec::new(),
                            error: Some(format!(
                                "Condition sustained for {:?}, requires {:?}",
                                elapsed, dur
                            )),
                            duration_ms: start.elapsed().as_millis() as u64,
                            triggered_at: now,
                        };
                    }
                }
                None => {
                    // Just set condition_since, wait for next scheduled evaluation
                    return RuleExecutionResult {
                        rule_id: id.clone(),
                        rule_name: rule.name.clone(),
                        success: false,
                        actions_executed: Vec::new(),
                        error: Some(
                            "Condition timing started, awaiting sustained duration".to_string(),
                        ),
                        duration_ms: start.elapsed().as_millis() as u64,
                        triggered_at: now,
                    };
                }
            }
        }

        // Atomically claim cooldown — prevents concurrent double-fire
        if !self.try_claim_cooldown(id, rule.cooldown) {
            tracing::debug!(rule_id = %id, "Rule execution skipped due to cooldown (atomic claim)");
            return RuleExecutionResult {
                rule_id: id.clone(),
                rule_name: rule.name.clone(),
                success: false,
                actions_executed: Vec::new(),
                error: Some("Rule is in cooldown".to_string()),
                duration_ms: start.elapsed().as_millis() as u64,
                triggered_at: now,
            };
        }

        // Extract trigger value for message placeholder substitution
        let (trigger_value, trigger_source) = rule
            .condition
            .as_ref()
            .map(|c| Self::extract_trigger_value(c, self.value_provider.as_ref()))
            .unwrap_or((None, None));

        // Execute actions
        let mut actions_executed = Vec::new();
        let mut error = None;

        for action in &rule.actions {
            match self
                .execute_action(action, trigger_value, trigger_source.as_deref())
                .await
            {
                Ok(name) => actions_executed.push(name),
                Err(e) => {
                    tracing::warn!(
                        rule_id = %id,
                        action = ?action,
                        error = %e,
                        "Action execution failed"
                    );
                    if error.is_none() {
                        error = Some(e);
                    }
                }
            }
        }

        // If ALL actions failed, retry quickly so critical-sensor rules don't
        // silently miss alerts due to a transient failure (device offline,
        // extension down). But pace the retries: refund the cooldown ONLY on the
        // first consecutive failure (immediate retry on next match); on repeat
        // failures leave the cooldown claimed so the rule fires at most once
        // per cooldown window. Without this cap, a rule bound to a high-rate
        // stream whose only actions persistently fail (e.g. a lone Execute
        // against an offline device — Notify essentially always succeeds, so it
        // only applies to action sets with no Notify) would re-fire on every
        // data point and flood rule_history (0.9.11 implicitly rate-limited via
        // the held cooldown; 8d3d0349's full refund removed that cap).
        if actions_executed.is_empty() && error.is_some() {
            let first_consecutive = {
                let mut failures = self.consecutive_action_failures.write();
                let count = failures.entry(id.clone()).or_insert(0);
                let was_zero = *count == 0;
                *count += 1;
                was_zero
            };
            if first_consecutive {
                self.cooldowns.write().remove(id);
                tracing::warn!(
                    rule_id = %id,
                    "All actions failed — refunded cooldown for one immediate retry"
                );
            } else {
                tracing::warn!(
                    rule_id = %id,
                    "All actions failed again — cooldown held to pace retries (avoids rule_history flood)"
                );
            }
        } else if !actions_executed.is_empty() {
            // At least one action succeeded → clear the consecutive-failure counter.
            self.consecutive_action_failures.write().remove(id);
        }

        // Update state (cooldown already claimed before action execution)
        self.update_rule_state_after_trigger(id).await;

        let result = RuleExecutionResult {
            rule_id: id.clone(),
            rule_name: rule.name.clone(),
            success: error.is_none(),
            actions_executed,
            error,
            duration_ms: start.elapsed().as_millis() as u64,
            triggered_at: now,
        };

        self.record_history(result.clone()).await;
        result
    }

    /// Evaluate a single rule and fire actions if conditions are met.
    async fn evaluate_and_fire(&self, rule_id: &RuleId) -> Result<(), RuleError> {
        let rule = {
            let rules = self.rules.read();
            rules.get(rule_id).cloned()
        };
        let Some(rule) = rule else {
            return Err(RuleError::Validation(format!(
                "Rule not found: {}",
                rule_id
            )));
        };

        if !rule.enabled {
            return Ok(());
        }

        // Check cooldown
        if self.is_in_cooldown(rule_id, &rule) {
            return Ok(());
        }

        // Evaluate condition
        let cond = match &rule.condition {
            Some(c) => c,
            None => return Ok(()), // DataChange with no condition → skip
        };

        let condition_met = panic::catch_unwind(AssertUnwindSafe(|| {
            cond.evaluate(self.value_provider.as_ref())
        }))
        .unwrap_or(false);

        if !condition_met {
            // Reset condition_since
            self.update_condition_since(rule_id, false).await;
            return Ok(());
        }

        // Check for_duration
        if let Some(dur) = rule.for_duration {
            let since = self.update_condition_since(rule_id, true).await;
            match since {
                Some(since_time) => {
                    let elapsed = Utc::now()
                        .signed_duration_since(since_time)
                        .to_std()
                        .unwrap_or(Duration::ZERO);
                    if elapsed < dur {
                        return Ok(()); // Not yet sustained long enough
                    }
                }
                None => return Ok(()), // Just set, wait for next evaluation
            }
        }

        // Atomically claim cooldown slot — prevents concurrent double-fire
        // (closes the TOCTOU window between the initial check and action exec).
        if !self.try_claim_cooldown(rule_id, rule.cooldown) {
            return Ok(());
        }

        // Extract trigger value for message placeholder substitution
        let (trigger_value, trigger_source) =
            Self::extract_trigger_value(cond, self.value_provider.as_ref());

        // Fire actions
        let start = Instant::now();
        let mut actions_executed = Vec::new();
        let mut first_error = None;
        for action in &rule.actions {
            match self
                .execute_action(action, trigger_value, trigger_source.as_deref())
                .await
            {
                Ok(name) => actions_executed.push(name),
                Err(e) => {
                    tracing::warn!(
                        rule_id = %rule_id,
                        action = ?action,
                        error = %e,
                        "Action execution failed"
                    );
                    if first_error.is_none() {
                        first_error = Some(e);
                    }
                }
            }
        }

        // Update state (cooldown already claimed before action execution)
        self.update_rule_state_after_trigger(rule_id).await;

        // Record history
        self.record_history(RuleExecutionResult {
            rule_id: rule_id.clone(),
            rule_name: rule.name.clone(),
            success: first_error.is_none(),
            actions_executed,
            error: first_error,
            duration_ms: start.elapsed().as_millis() as u64,
            triggered_at: Utc::now(),
        })
        .await;

        Ok(())
    }

    // -- Action execution --

    /// Substitute `{value}` and `{source_id}` placeholders in a message template.
    fn substitute_placeholders(message: &str, value: Option<f64>, source: Option<&str>) -> String {
        let mut result = message.to_string();
        if let Some(v) = value {
            result = result.replace("{value}", &format!("{}", v));
        }
        if let Some(s) = source {
            result = result.replace("{source_id}", s);
        }
        result
    }

    /// Extract the primary trigger value and source from a condition tree.
    /// Returns the first leaf condition's current value and source key.
    fn extract_trigger_value(
        condition: &RuleCondition,
        provider: &dyn ValueProvider,
    ) -> (Option<f64>, Option<String>) {
        fn find_first(
            cond: &RuleCondition,
            provider: &dyn ValueProvider,
        ) -> (Option<f64>, Option<String>) {
            match cond {
                RuleCondition::Comparison { source, .. } | RuleCondition::Range { source, .. } => {
                    let value = provider.get_by_source(source);
                    (
                        value.as_ref().and_then(|rv| rv.as_number()),
                        Some(source.storage_key()),
                    )
                }
                RuleCondition::Logical { conditions, .. } => {
                    for c in conditions {
                        let (v, s) = find_first(c, provider);
                        if v.is_some() || s.is_some() {
                            return (v, s);
                        }
                    }
                    (None, None)
                }
            }
        }
        find_first(condition, provider)
    }

    async fn execute_action(
        &self,
        action: &RuleAction,
        trigger_value: Option<f64>,
        trigger_source: Option<&str>,
    ) -> Result<String, String> {
        match action {
            RuleAction::Notify { message, severity } => {
                let formatted =
                    Self::substitute_placeholders(message, trigger_value, trigger_source);

                let msg_sev = match severity {
                    NotifySeverity::Info => neomind_messages::MessageSeverity::Info,
                    NotifySeverity::Warning => neomind_messages::MessageSeverity::Warning,
                    NotifySeverity::Critical => neomind_messages::MessageSeverity::Critical,
                    NotifySeverity::Emergency => {
                        tracing::warn!(
                            "Emergency severity mapped to Critical (MessageManager has no Emergency level)"
                        );
                        neomind_messages::MessageSeverity::Critical
                    }
                };

                let mgr = self.message_manager.read().await;
                if let Some(manager) = mgr.as_ref() {
                    let msg = neomind_messages::Message::alert(
                        msg_sev,
                        "Rule Triggered".to_string(),
                        formatted.clone(),
                        "rule_engine".to_string(),
                    );
                    match manager.create_message(msg).await {
                        Ok(_) => Ok(format!("NOTIFY: {}", formatted)),
                        Err(e) => Err(format!("Failed to create message: {}", e)),
                    }
                } else {
                    tracing::info!("NOTIFY: {} (no MessageManager)", formatted);
                    Ok(format!("NOTIFY: {} (logged only)", formatted))
                }
            }

            RuleAction::Execute {
                target,
                target_type,
                command,
                params,
            } => match target_type {
                ExecuteTarget::Device => {
                    let executor = self.device_action_executor.read().await;
                    if let Some(ex) = executor.as_ref() {
                        let params_map: HashMap<String, serde_json::Value> = params
                            .as_object()
                            .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                            .unwrap_or_default();
                        match ex
                            .execute_command_with_retry(target, command, &params_map)
                            .await
                        {
                            Ok(_) => Ok(format!("EXECUTE: {}.{}", target, command)),
                            Err(e) => Err(format!("EXECUTE failed: {}", e)),
                        }
                    } else {
                        tracing::info!("EXECUTE device {}.{} (no executor)", target, command);
                        Ok(format!("EXECUTE: {}.{} (logged only)", target, command))
                    }
                }
                ExecuteTarget::Extension => {
                    let executor = self.extension_action_executor.read().await;
                    if let Some(ex) = executor.as_ref() {
                        let ext_action = crate::extension_integration::ExtensionCommandAction::new(
                            target, command,
                        )
                        .with_args(params.clone());
                        match ex.execute(&ext_action).await {
                            Ok(result) if result.success => Ok(format!(
                                "EXTENSION: {}.{}",
                                result.extension_id, result.command
                            )),
                            Ok(result) => Err(result.error.unwrap_or_else(|| "Failed".to_string())),
                            Err(e) => Err(e),
                        }
                    } else {
                        tracing::info!("EXECUTE extension {}.{} (no executor)", target, command);
                        Ok(format!("EXTENSION: {}.{} (logged only)", target, command))
                    }
                }
            },

            RuleAction::TriggerAgent {
                agent_id,
                input,
                data,
            } => {
                let trigger = self.agent_trigger.read().await;
                if let Some(cb) = trigger.as_ref() {
                    // [decoupled] The full agent run used to be awaited INLINE
                    // here — one rule with a TriggerAgent action stalled every
                    // other rule's evaluation platform-wide for the entire
                    // agent execution (bounded only by the global 5-min cap),
                    // because on_data_update awaits evaluate_and_fire
                    // sequentially per affected rule. Spawn it: rule
                    // processing continues, and the agent's own executor
                    // (semaphores, timeout, journaling) governs the run.
                    let cb = cb.clone();
                    let (spawn_id, spawn_input, spawn_data, log_id) = (
                        agent_id.clone(),
                        input.clone(),
                        data.clone(),
                        agent_id.clone(),
                    );
                    tokio::spawn(async move {
                        if let Err(e) = cb(spawn_id, spawn_input, spawn_data).await {
                            tracing::warn!(
                                agent_id = %log_id,
                                error = %e,
                                "TRIGGER_AGENT execution failed"
                            );
                        }
                    });
                    Ok(format!("TRIGGER_AGENT: {} (spawned)", agent_id))
                } else {
                    tracing::warn!("TRIGGER_AGENT: {} (no callback wired)", agent_id);
                    Err("TRIGGER_AGENT failed: agent trigger callback not initialized".to_string())
                }
            }
        }
    }

    // -- State helpers --

    fn is_in_cooldown(&self, rule_id: &RuleId, rule: &CompiledRule) -> bool {
        let cooldowns = self.cooldowns.read();
        if let Some(last) = cooldowns.get(rule_id) {
            last.elapsed() < rule.cooldown
        } else {
            false
        }
    }

    /// Atomically check cooldown and claim the slot.
    ///
    /// If the rule is NOT in cooldown, sets the cooldown timestamp immediately
    /// and returns `true`. If it IS in cooldown, returns `false` without
    /// modifying state. This closes the TOCTOU window between `is_in_cooldown`
    /// and `set_cooldown` that could allow concurrent double-firing.
    fn try_claim_cooldown(&self, rule_id: &RuleId, cooldown: Duration) -> bool {
        let mut cooldowns = self.cooldowns.write();
        if let Some(last) = cooldowns.get(rule_id) {
            if last.elapsed() < cooldown {
                return false;
            }
        }
        cooldowns.insert(rule_id.clone(), Instant::now());
        true
    }

    async fn update_condition_since(
        &self,
        rule_id: &RuleId,
        condition_met: bool,
    ) -> Option<chrono::DateTime<Utc>> {
        let (result, rule_snapshot) = {
            let mut rules = self.rules.write();
            let Some(rule) = rules.get_mut(rule_id) else {
                return None;
            };
            if condition_met {
                if rule.state.condition_since.is_none() {
                    rule.state.condition_since = Some(Utc::now());
                    (rule.state.condition_since, Some(rule.clone()))
                } else {
                    (rule.state.condition_since, None) // no state change
                }
            } else if rule.state.condition_since.is_some() {
                rule.state.condition_since = None;
                (None, Some(rule.clone()))
            } else {
                (None, None) // no state change
            }
        };
        // Persist only when condition_since changed (was missing — for_duration
        // elapsed accumulation was lost on restart, causing premature triggering).
        if let Some(rule) = rule_snapshot {
            if let Some(store) = self.rule_store.read().as_ref() {
                if let Err(e) = store.save(&rule) {
                    tracing::warn!(rule_id = %rule_id, error = %e, "Failed to persist condition_since");
                }
            }
        }
        result
    }

    async fn update_rule_state_after_trigger(&self, rule_id: &RuleId) {
        let rule_snapshot = {
            let mut rules = self.rules.write();
            if let Some(rule) = rules.get_mut(rule_id) {
                rule.state.trigger_count += 1;
                rule.state.last_triggered = Some(Utc::now());
                rule.state.condition_since = None;
                Some(rule.clone())
            } else {
                None
            }
        };

        // Persist to store
        if let Some(rule) = rule_snapshot {
            if let Some(store) = self.rule_store.read().as_ref() {
                if let Err(e) = store.save(&rule) {
                    tracing::warn!(rule_id = %rule_id, error = %e, "Failed to persist rule state");
                }
            }
        }
    }

    async fn record_history(&self, result: RuleExecutionResult) {
        // Persist to store if available
        if let Some(store) = self.rule_store.read().as_ref() {
            if let Err(e) = store.save_history(&result) {
                tracing::warn!("Failed to persist rule history: {}", e);
            }
        }

        let mut history = self.history.write().await;
        history.push_back(result);
        while history.len() > MAX_HISTORY_SIZE {
            history.pop_front();
        }
    }

    // -- History --

    pub async fn get_rule_history(&self, rule_id: &RuleId) -> Vec<RuleExecutionResult> {
        self.history
            .read()
            .await
            .iter()
            .filter(|r| &r.rule_id == rule_id)
            .cloned()
            .collect()
    }

    /// List only Schedule-type rules with their cron expressions.
    pub async fn list_schedule_rules(&self) -> Vec<(RuleId, String)> {
        let rules = self.rules.read();
        rules
            .iter()
            .filter_map(|(id, rule)| {
                if rule.enabled {
                    if let RuleTrigger::Schedule { ref cron } = rule.trigger {
                        Some((id.clone(), cron.clone()))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect()
    }

    // -- Value provider access --

    pub fn get_value_provider(&self) -> Arc<dyn ValueProvider> {
        self.value_provider.clone()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;

    #[tokio::test]
    async fn test_add_and_list_rules() {
        let provider = Arc::new(InMemoryValueProvider::new());
        let engine = RuleEngine::new(provider);

        let mut rule = CompiledRule::new("Test Rule");
        rule.condition = Some(RuleCondition::Comparison {
            source: DataSourceId::device("sensor1", "temperature"),
            operator: ComparisonOperator::GreaterThan,
            threshold: 50.0,
            threshold_value: None,
        });
        rule.trigger = RuleTrigger::from_condition(&rule.condition);
        rule.finalize();

        engine.add_rule(rule).await.unwrap();
        assert_eq!(engine.list_rules().await.len(), 1);
    }

    #[tokio::test]
    async fn test_on_data_update_triggers_rule() {
        let provider = Arc::new(InMemoryValueProvider::new());
        let engine = RuleEngine::new(provider.clone());

        let mut rule = CompiledRule::new("High Temp");
        rule.condition = Some(RuleCondition::Comparison {
            source: DataSourceId::device("sensor1", "temperature"),
            operator: ComparisonOperator::GreaterThan,
            threshold: 50.0,
            threshold_value: None,
        });
        rule.trigger = RuleTrigger::from_condition(&rule.condition);
        rule.actions = vec![RuleAction::Notify {
            message: "Too hot".into(),
            severity: NotifySeverity::Warning,
        }];
        rule.finalize();

        let rule_id = rule.id.clone();
        engine.add_rule(rule).await.unwrap();

        // Set value above threshold and notify
        provider.set_value("device:sensor1:temperature", 75.0);
        engine
            .on_data_update(
                &DataSourceId::device("sensor1", "temperature"),
                RuleValue::Number(75.0),
            )
            .await;

        // Check trigger count
        let r = engine.get_rule(&rule_id).await.unwrap();
        assert_eq!(r.state.trigger_count, 1);
    }

    #[tokio::test]
    async fn test_on_data_update_below_threshold() {
        let provider = Arc::new(InMemoryValueProvider::new());
        let engine = RuleEngine::new(provider.clone());

        let mut rule = CompiledRule::new("High Temp");
        rule.condition = Some(RuleCondition::Comparison {
            source: DataSourceId::device("sensor1", "temperature"),
            operator: ComparisonOperator::GreaterThan,
            threshold: 50.0,
            threshold_value: None,
        });
        rule.trigger = RuleTrigger::from_condition(&rule.condition);
        rule.finalize();

        let rule_id = rule.id.clone();
        engine.add_rule(rule).await.unwrap();

        provider.set_value("device:sensor1:temperature", 25.0);
        engine
            .on_data_update(
                &DataSourceId::device("sensor1", "temperature"),
                RuleValue::Number(25.0),
            )
            .await;

        let r = engine.get_rule(&rule_id).await.unwrap();
        assert_eq!(r.state.trigger_count, 0);
    }

    #[tokio::test]
    async fn test_cooldown_prevents_retrigger() {
        let provider = Arc::new(InMemoryValueProvider::new());
        let engine = RuleEngine::new(provider.clone());

        let mut rule = CompiledRule::new("High Temp");
        rule.condition = Some(RuleCondition::Comparison {
            source: DataSourceId::device("sensor1", "temperature"),
            operator: ComparisonOperator::GreaterThan,
            threshold: 50.0,
            threshold_value: None,
        });
        rule.trigger = RuleTrigger::from_condition(&rule.condition);
        rule.cooldown = Duration::from_secs(60);
        rule.actions = vec![RuleAction::Notify {
            message: "Too hot".into(),
            severity: NotifySeverity::Warning,
        }];
        rule.finalize();

        let rule_id = rule.id.clone();
        engine.add_rule(rule).await.unwrap();

        // First trigger
        provider.set_value("device:sensor1:temperature", 75.0);
        engine
            .on_data_update(
                &DataSourceId::device("sensor1", "temperature"),
                RuleValue::Number(75.0),
            )
            .await;

        // Second trigger (within cooldown)
        engine
            .on_data_update(
                &DataSourceId::device("sensor1", "temperature"),
                RuleValue::Number(80.0),
            )
            .await;

        let r = engine.get_rule(&rule_id).await.unwrap();
        assert_eq!(r.state.trigger_count, 1); // Still 1 due to cooldown
    }

    #[tokio::test]
    async fn test_manual_trigger() {
        let provider = Arc::new(InMemoryValueProvider::new());
        let engine = RuleEngine::new(provider.clone());

        let mut rule = CompiledRule::new("Manual Rule");
        rule.trigger = RuleTrigger::Manual;
        // No condition — always fires on manual trigger
        rule.actions = vec![RuleAction::Notify {
            message: "Manual fired".into(),
            severity: NotifySeverity::Info,
        }];
        rule.finalize();

        let rule_id = rule.id.clone();
        engine.add_rule(rule).await.unwrap();

        let result = engine.execute_rule(&rule_id).await;
        assert!(result.success);

        let r = engine.get_rule(&rule_id).await.unwrap();
        assert_eq!(r.state.trigger_count, 1);
    }

    #[tokio::test]
    async fn test_subscription_index_selective() {
        let provider = Arc::new(InMemoryValueProvider::new());
        let engine = RuleEngine::new(provider.clone());

        // Rule 1: watches sensor1
        let mut rule1 = CompiledRule::new("Rule 1");
        rule1.condition = Some(RuleCondition::Comparison {
            source: DataSourceId::device("sensor1", "temp"),
            operator: ComparisonOperator::GreaterThan,
            threshold: 50.0,
            threshold_value: None,
        });
        rule1.trigger = RuleTrigger::from_condition(&rule1.condition);
        rule1.actions = vec![RuleAction::Notify {
            message: "s1 hot".into(),
            severity: NotifySeverity::Warning,
        }];
        rule1.finalize();
        let rule1_id = rule1.id.clone();

        // Rule 2: watches sensor2
        let mut rule2 = CompiledRule::new("Rule 2");
        rule2.condition = Some(RuleCondition::Comparison {
            source: DataSourceId::device("sensor2", "temp"),
            operator: ComparisonOperator::GreaterThan,
            threshold: 50.0,
            threshold_value: None,
        });
        rule2.trigger = RuleTrigger::from_condition(&rule2.condition);
        rule2.actions = vec![RuleAction::Notify {
            message: "s2 hot".into(),
            severity: NotifySeverity::Warning,
        }];
        rule2.finalize();
        let rule2_id = rule2.id.clone();

        engine.add_rule(rule1).await.unwrap();
        engine.add_rule(rule2).await.unwrap();

        // Update sensor1 — only rule1 should trigger
        provider.set_value("device:sensor1:temp", 75.0);
        engine
            .on_data_update(
                &DataSourceId::device("sensor1", "temp"),
                RuleValue::Number(75.0),
            )
            .await;

        let r1 = engine.get_rule(&rule1_id).await.unwrap();
        let r2 = engine.get_rule(&rule2_id).await.unwrap();
        assert_eq!(r1.state.trigger_count, 1);
        assert_eq!(r2.state.trigger_count, 0);
    }

    #[tokio::test]
    async fn test_string_comparison_rule() {
        let provider = Arc::new(InMemoryValueProvider::new());
        let engine = RuleEngine::new(provider.clone());

        let mut rule = CompiledRule::new("Status Online");
        rule.condition = Some(RuleCondition::Comparison {
            source: DataSourceId::device("dev1", "status"),
            operator: ComparisonOperator::Equal,
            threshold: 0.0,
            threshold_value: Some("online".into()),
        });
        rule.trigger = RuleTrigger::from_condition(&rule.condition);
        rule.actions = vec![RuleAction::Notify {
            message: "Device is online".into(),
            severity: NotifySeverity::Info,
        }];
        rule.finalize();

        let rule_id = rule.id.clone();
        engine.add_rule(rule).await.unwrap();

        // Set string value and trigger
        provider.set_string_value("device:dev1:status", "online");
        engine
            .on_data_update(
                &DataSourceId::device("dev1", "status"),
                RuleValue::Text("online".into()),
            )
            .await;

        let r = engine.get_rule(&rule_id).await.unwrap();
        assert_eq!(r.state.trigger_count, 1);
    }

    #[tokio::test]
    async fn test_string_contains_rule() {
        let provider = Arc::new(InMemoryValueProvider::new());
        let engine = RuleEngine::new(provider.clone());

        let mut rule = CompiledRule::new("Error Detected");
        rule.condition = Some(RuleCondition::Comparison {
            source: DataSourceId::device("dev1", "log"),
            operator: ComparisonOperator::Contains,
            threshold: 0.0,
            threshold_value: Some("error".into()),
        });
        rule.trigger = RuleTrigger::from_condition(&rule.condition);
        rule.actions = vec![RuleAction::Notify {
            message: "Error in log".into(),
            severity: NotifySeverity::Warning,
        }];
        rule.finalize();

        let rule_id = rule.id.clone();
        engine.add_rule(rule).await.unwrap();

        provider.set_string_value("device:dev1:log", "device_error_timeout");
        engine
            .on_data_update(
                &DataSourceId::device("dev1", "log"),
                RuleValue::Text("device_error_timeout".into()),
            )
            .await;

        let r = engine.get_rule(&rule_id).await.unwrap();
        assert_eq!(r.state.trigger_count, 1);
    }

    #[test]
    fn test_subscribed_virtual_metric_devices_returns_only_matching() {
        use neomind_core::datasource::DataSourceId;
        let provider = Arc::new(InMemoryValueProvider::new());
        let engine = RuleEngine::new(provider);

        // Rule 1 watches dev-A's last_seen_age_secs
        let mut r1 = CompiledRule::new("A offline");
        r1.condition = Some(RuleCondition::Comparison {
            source: DataSourceId::device("dev-A", "__last_seen_age_secs"),
            operator: ComparisonOperator::GreaterThan,
            threshold: 3600.0,
            threshold_value: None,
        });
        r1.trigger = RuleTrigger::from_condition(&r1.condition);
        r1.finalize();

        // Rule 2 watches dev-B's temperature (regular metric — should NOT be picked up)
        let mut r2 = CompiledRule::new("B hot");
        r2.condition = Some(RuleCondition::Comparison {
            source: DataSourceId::device("dev-B", "temperature"),
            operator: ComparisonOperator::GreaterThan,
            threshold: 50.0,
            threshold_value: None,
        });
        r2.trigger = RuleTrigger::from_condition(&r2.condition);
        r2.finalize();

        // Rule 3 watches dev-C's last_seen_age_secs
        let mut r3 = CompiledRule::new("C offline");
        r3.condition = Some(RuleCondition::Comparison {
            source: DataSourceId::device("dev-C", "__last_seen_age_secs"),
            operator: ComparisonOperator::GreaterThan,
            threshold: 3600.0,
            threshold_value: None,
        });
        r3.trigger = RuleTrigger::from_condition(&r3.condition);
        r3.finalize();

        // Use block_on to add all three
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            engine.add_rule(r1).await.unwrap();
            engine.add_rule(r2).await.unwrap();
            engine.add_rule(r3).await.unwrap();
        });

        let mut devices = engine.subscribed_virtual_metric_devices("__last_seen_age_secs");
        devices.sort();
        assert_eq!(devices, vec!["dev-A".to_string(), "dev-C".to_string()]);
    }
}
