//! Read-only rule explanation (edge-AI plan iteration 2, 2026-11-13).
//!
//! `RuleEngine::explain_rule` answers "why is this rule in the state it is
//! in RIGHT NOW" without executing anything and without mutating any state:
//! - every condition leaf reports source / current value / value age /
//!   result, and `unknown` (with a reason) when no value is cached — the
//!   evaluator itself would return `false` for a missing value, which is
//!   indistinguishable from a measured false and has misled diagnostics;
//! - logical nodes combine with three-valued (Kleene) logic;
//! - `for_duration` progress, cooldown status and the last persisted
//!   execution are attached as evidence;
//! - observable limits are stated, not hidden: value age is when the
//!   PLATFORM cached the value (the device's measurement time is not
//!   retained on this path), and cooldown lives in engine memory only —
//!   it resets on restart and is not persisted.
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use neomind_core::datasource::DataSourceId;

use crate::engine::RuleEngine;
use crate::models::{
    ComparisonOperator, LogicalOperator, RuleAction, RuleCondition, RuleId, RuleTrigger, RuleValue,
    ValueProvider,
};
use crate::preview::to_dsl_preview;
use crate::unified_provider::UnifiedValueProvider;

/// Three-valued evaluation outcome for explanation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvalOutcome {
    True,
    False,
    Unknown,
}

impl EvalOutcome {
    fn kleene_and(self, other: Self) -> Self {
        match (self, other) {
            (EvalOutcome::False, _) | (_, EvalOutcome::False) => EvalOutcome::False,
            (EvalOutcome::Unknown, _) | (_, EvalOutcome::Unknown) => EvalOutcome::Unknown,
            _ => EvalOutcome::True,
        }
    }
    fn kleene_or(self, other: Self) -> Self {
        match (self, other) {
            (EvalOutcome::True, _) | (_, EvalOutcome::True) => EvalOutcome::True,
            (EvalOutcome::Unknown, _) | (_, EvalOutcome::Unknown) => EvalOutcome::Unknown,
            _ => EvalOutcome::False,
        }
    }
}

/// One node of the condition tree, explained.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionExplanation {
    /// comparison | range | logical
    pub kind: String,
    pub result: EvalOutcome,
    /// DataSourceId storage key, e.g. `device:sensor1:temperature`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<RuleValue>,
    /// Age of the CACHED value (seconds, platform-side write time).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_age_secs: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// Set when result is unknown; machine-readable reason code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unknown_reason: Option<String>,
    /// Children of a logical node.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conditions: Option<Vec<ConditionExplanation>>,
}

/// `for_duration` progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForDurationExplanation {
    pub configured_secs: u64,
    /// When the condition first became true (persisted in rule state).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition_since: Option<chrono::DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_secs: Option<i64>,
    pub satisfied: bool,
    /// False when the trigger path does not enforce `for_duration` (Manual
    /// run-now skips the hold gate). UI must not treat unsatisfied progress
    /// as a blocker when `applicable` is false.
    #[serde(default = "for_duration_applicable_default")]
    pub applicable: bool,
}

fn for_duration_applicable_default() -> bool {
    true
}

/// Cooldown status. Lives in engine memory only — resets on restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CooldownExplanation {
    pub configured_secs: u64,
    pub in_cooldown: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining_secs: Option<u64>,
}

/// One action of the rule, as configured (not executed by explain).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionExplanation {
    pub action_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
}

/// The last persisted execution of this rule, as evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastExecutionEvidence {
    pub triggered_at: chrono::DateTime<Utc>,
    pub success: bool,
    pub duration_ms: u64,
    pub actions_executed: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The full read-only explanation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleExplanation {
    pub rule_id: String,
    pub rule_name: String,
    pub enabled: bool,
    /// When this snapshot was taken (diagnosing a moving system).
    pub snapshot_at: chrono::DateTime<Utc>,
    pub dsl_preview: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<ConditionExplanation>,
    /// None when the rule has no condition (data-change trigger without one).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition_met: Option<EvalOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub for_duration: Option<ForDurationExplanation>,
    pub cooldown: CooldownExplanation,
    pub trigger_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_triggered: Option<chrono::DateTime<Utc>>,
    pub actions: Vec<ActionExplanation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_execution: Option<LastExecutionEvidence>,
    /// Caveats the reader must know (disabled rule, restart-reset cooldown,
    /// value-age semantics, …).
    pub notes: Vec<String>,
}

impl RuleEngine {
    /// Explain a rule's current state. Read-only: evaluates the condition
    /// tree, reports cooldown/for_duration/state and the last persisted
    /// execution, and executes NOTHING.
    pub async fn explain_rule(&self, id: &RuleId) -> Option<RuleExplanation> {
        let rule = { self.rules.read().get(id).cloned()? };
        let snapshot_at = Utc::now();
        let now_ms = snapshot_at.timestamp_millis();

        // Concrete provider gives value AGE; the trait alone does not carry it.
        let unified = self
            .value_provider
            .as_any()
            .downcast_ref::<UnifiedValueProvider>();

        let mut notes = Vec::new();
        if !rule.enabled {
            notes.push("rule is disabled: the data path skips evaluation and firing".to_string());
        }

        let condition = rule
            .condition
            .as_ref()
            .map(|c| explain_condition(c, self.value_provider.as_ref(), unified, now_ms));
        let condition_met = condition.as_ref().map(|c| c.result);

        let for_duration = rule.for_duration.map(|dur| {
            // Manual execute_rule skips the duration gate (engine.rs) — surface
            // the configured hold but mark non-applicable so diagnostics do
            // not invent a blocker run-now will ignore.
            if matches!(rule.trigger, RuleTrigger::Manual) {
                notes.push(
                    "for_duration is configured but Manual execute_rule skips the duration gate"
                        .to_string(),
                );
                return ForDurationExplanation {
                    configured_secs: dur.as_secs(),
                    condition_since: None,
                    elapsed_secs: None,
                    satisfied: true,
                    applicable: false,
                };
            }
            let since = rule.state.condition_since;
            let elapsed_secs =
                since.map(|s| snapshot_at.signed_duration_since(s).num_seconds().max(0));
            ForDurationExplanation {
                configured_secs: dur.as_secs(),
                condition_since: since,
                elapsed_secs,
                satisfied: elapsed_secs
                    .map(|e| e >= dur.as_secs() as i64)
                    .unwrap_or(false),
                applicable: true,
            }
        });

        // Cooldown: engine memory only. Compare Durations (not secs) so a
        // configured Duration::ZERO never looks "in cooldown" via as_secs().
        let remaining = {
            let cooldowns = self.cooldowns.read();
            cooldowns
                .get(id)
                .map(|last| rule.cooldown.saturating_sub(last.elapsed()))
        };
        let in_cooldown = remaining.map(|r| r > Duration::ZERO).unwrap_or(false);
        notes.push("cooldown state is engine memory; it resets on server restart".to_string());
        if unified.is_some() {
            notes.push(
                "value_age_secs is when the platform cached the value, not the device's measurement time"
                    .to_string(),
            );
        }

        // Last persisted execution as evidence (store first, in-memory fallback).
        // Prefer load_latest_history (reverse key walk, early exit) over a
        // full collect+sort of every row for this rule.
        let last_execution = {
            let stored = self
                .rule_store
                .read()
                .as_ref()
                .and_then(|s| s.load_latest_history(id).ok())
                .flatten();
            let stored = match stored {
                Some(e) => Some(e),
                None => {
                    // The in-memory deque is GLOBAL (all rules, capped at
                    // MAX_HISTORY_SIZE) — filter by rule_id, or rule A's
                    // latest execution would be shown as rule B's evidence.
                    let history = self.history.read().await;
                    history
                        .iter()
                        .filter(|e| &e.rule_id == id)
                        .max_by_key(|e| e.triggered_at)
                        .cloned()
                }
            };
            stored.map(|e| LastExecutionEvidence {
                triggered_at: e.triggered_at,
                success: e.success,
                duration_ms: e.duration_ms,
                actions_executed: e.actions_executed,
                error: e.error,
            })
        };

        let actions = rule
            .actions
            .iter()
            .map(|a| match a {
                RuleAction::Notify { message, severity } => ActionExplanation {
                    action_type: "notify".to_string(),
                    target: None,
                    command: None,
                    message: Some(message.clone()),
                    severity: Some(format!("{:?}", severity).to_lowercase()),
                },
                RuleAction::Execute {
                    target,
                    target_type,
                    command,
                    ..
                } => ActionExplanation {
                    action_type: "execute".to_string(),
                    target: Some(format!("{:?}:{}", target_type, target).to_lowercase()),
                    command: Some(command.clone()),
                    message: None,
                    severity: None,
                },
                RuleAction::TriggerAgent {
                    agent_id, input, ..
                } => ActionExplanation {
                    action_type: "trigger_agent".to_string(),
                    target: Some(agent_id.clone()),
                    command: None,
                    message: input.clone(),
                    severity: None,
                },
                RuleAction::RunOperator { agent_id } => ActionExplanation {
                    action_type: "run_operator".to_string(),
                    target: Some(agent_id.clone()),
                    command: None,
                    message: None,
                    severity: None,
                },
            })
            .collect();

        Some(RuleExplanation {
            rule_id: id.to_string(),
            rule_name: rule.name.clone(),
            enabled: rule.enabled,
            snapshot_at,
            dsl_preview: to_dsl_preview(&rule),
            condition,
            condition_met,
            for_duration,
            cooldown: CooldownExplanation {
                configured_secs: rule.cooldown.as_secs(),
                in_cooldown,
                remaining_secs: if in_cooldown {
                    remaining.map(|d| d.as_secs())
                } else {
                    None
                },
            },
            trigger_count: rule.state.trigger_count,
            last_triggered: rule.state.last_triggered,
            actions,
            last_execution,
            notes,
        })
    }
}

fn explain_condition(
    cond: &RuleCondition,
    provider: &dyn ValueProvider,
    unified: Option<&UnifiedValueProvider>,
    now_ms: i64,
) -> ConditionExplanation {
    match cond {
        RuleCondition::Comparison {
            source,
            operator,
            threshold,
            threshold_value,
        } => {
            let (value, age) = lookup_with_age(source, provider, unified, now_ms);
            let (result, unknown_reason) = match &value {
                Some(RuleValue::Number(_)) if operator.is_string_op() => (
                    EvalOutcome::Unknown,
                    Some("operator_value_mismatch".to_string()),
                ),
                Some(RuleValue::Number(v)) => (outcome(operator.evaluate(*v, *threshold)), None),
                Some(RuleValue::Text(_))
                    if matches!(
                        operator,
                        ComparisonOperator::GreaterThan
                            | ComparisonOperator::LessThan
                            | ComparisonOperator::GreaterEqual
                            | ComparisonOperator::LessEqual
                    ) =>
                {
                    (EvalOutcome::Unknown, Some("value_not_numeric".to_string()))
                }
                Some(RuleValue::Text(s)) => {
                    let fallback = threshold.to_string();
                    let t = threshold_value.as_deref().unwrap_or(&fallback);
                    (outcome(operator.evaluate_str(s, t)), None)
                }
                None => (EvalOutcome::Unknown, Some("no_value_cached".to_string())),
            };
            ConditionExplanation {
                kind: "comparison".to_string(),
                result,
                source: Some(source.storage_key()),
                operator: Some(render_operator(operator)),
                value,
                value_age_secs: age,
                threshold: Some(*threshold),
                threshold_value: threshold_value.clone(),
                min: None,
                max: None,
                unknown_reason,
                conditions: None,
            }
        }
        RuleCondition::Range { source, min, max } => {
            let (value, age) = lookup_with_age(source, provider, unified, now_ms);
            let (result, unknown_reason) = match value.as_ref().and_then(|v| v.as_number()) {
                Some(v) => (outcome(v >= *min && v <= *max), None),
                None => (
                    EvalOutcome::Unknown,
                    Some(if value.is_some() {
                        "value_not_numeric".to_string()
                    } else {
                        "no_value_cached".to_string()
                    }),
                ),
            };
            ConditionExplanation {
                kind: "range".to_string(),
                result,
                source: Some(source.storage_key()),
                operator: None,
                value,
                value_age_secs: age,
                threshold: None,
                threshold_value: None,
                min: Some(*min),
                max: Some(*max),
                unknown_reason,
                conditions: None,
            }
        }
        RuleCondition::Logical {
            operator,
            conditions,
        } => {
            let children: Vec<ConditionExplanation> = conditions
                .iter()
                .map(|c| explain_condition(c, provider, unified, now_ms))
                .collect();
            // NOT is NOR over the children and must short-circuit: any True
            // child makes the result definitively False regardless of order
            // or of remaining children (mirrors RuleCondition::evaluate's
            // `!any`). Folding instead made [True, False] surface True and
            // [True, Unknown] surface Unknown.
            //
            // INTENTIONAL divergence from the evaluator: a missing sensor
            // value evaluates as NOT-met there (2-valued), so children like
            // [Unknown, False] make the evaluator FIRE while this view
            // reports Unknown. Explain is the honest surface — it tells the
            // user the rule fired on incomplete data rather than pretending
            // certainty. Do not "fix" either side to match the other without
            // deciding the product semantics for missing-value firing.
            let result = match operator {
                LogicalOperator::And => children
                    .iter()
                    .fold(EvalOutcome::True, |acc, c| acc.kleene_and(c.result)),
                LogicalOperator::Or => children
                    .iter()
                    .fold(EvalOutcome::False, |acc, c| acc.kleene_or(c.result)),
                LogicalOperator::Not => {
                    if children
                        .iter()
                        .any(|c| matches!(c.result, EvalOutcome::True))
                    {
                        EvalOutcome::False
                    } else if children
                        .iter()
                        .any(|c| matches!(c.result, EvalOutcome::Unknown))
                    {
                        EvalOutcome::Unknown
                    } else {
                        EvalOutcome::True
                    }
                }
            };
            ConditionExplanation {
                kind: "logical".to_string(),
                result,
                source: None,
                operator: Some(format!("{:?}", operator).to_lowercase()),
                value: None,
                value_age_secs: None,
                threshold: None,
                threshold_value: None,
                min: None,
                max: None,
                unknown_reason: None,
                conditions: Some(children),
            }
        }
    }
}

fn lookup_with_age(
    source: &DataSourceId,
    provider: &dyn ValueProvider,
    unified: Option<&UnifiedValueProvider>,
    now_ms: i64,
) -> (Option<RuleValue>, Option<i64>) {
    let value = provider.get_by_source(source);
    let age = if value.is_some() {
        unified
            .and_then(|u| u.value_cached_at_ms(source))
            .map(|ts| ((now_ms - ts) / 1000).max(0))
    } else {
        None
    };
    (value, age)
}

fn outcome(b: bool) -> EvalOutcome {
    if b {
        EvalOutcome::True
    } else {
        EvalOutcome::False
    }
}

fn render_operator(op: &ComparisonOperator) -> String {
    op.symbol().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::RuleEngine;
    use crate::models::*;
    use neomind_core::datasource::DataSourceId;
    use std::sync::Arc;

    fn provider() -> Arc<UnifiedValueProvider> {
        Arc::new(UnifiedValueProvider::new())
    }

    async fn engine_with_high_temp_rule(
        p: &Arc<UnifiedValueProvider>,
        enabled: bool,
    ) -> (RuleEngine, RuleId) {
        let engine = RuleEngine::new(p.clone());
        let mut rule = CompiledRule::new("High Temp");
        rule.condition = Some(RuleCondition::Comparison {
            source: DataSourceId::device("sensor1", "temperature"),
            operator: ComparisonOperator::GreaterThan,
            threshold: 30.0,
            threshold_value: None,
        });
        rule.trigger = RuleTrigger::Manual;
        rule.actions = vec![RuleAction::Notify {
            message: "too hot".into(),
            severity: NotifySeverity::Warning,
        }];
        rule.enabled = enabled;
        rule.finalize();
        let id = rule.id.clone();
        engine.add_rule(rule).await.unwrap();
        (engine, id)
    }

    #[tokio::test]
    async fn explain_leaf_true_false_and_value_age() {
        let p = provider();
        let (engine, id) = engine_with_high_temp_rule(&p, true).await;
        p.update_value("device", "sensor1", "temperature", 35.0)
            .await;

        let exp = engine.explain_rule(&id).await.expect("rule exists");
        let leaf = exp.condition.as_ref().unwrap();
        assert_eq!(leaf.result, EvalOutcome::True);
        assert_eq!(leaf.source.as_deref(), Some("device:sensor1:temperature"));
        assert!(leaf.value.is_some());
        assert!(leaf.value_age_secs.unwrap_or(99) <= 1, "fresh cache age");

        p.update_value("device", "sensor1", "temperature", 20.0)
            .await;
        let exp = engine.explain_rule(&id).await.unwrap();
        assert_eq!(exp.condition.as_ref().unwrap().result, EvalOutcome::False);
    }

    #[tokio::test]
    async fn explain_missing_value_is_unknown_not_false() {
        let p = provider();
        let (engine, id) = engine_with_high_temp_rule(&p, true).await;
        // No value ever cached for the source.
        let exp = engine.explain_rule(&id).await.unwrap();
        let leaf = exp.condition.as_ref().unwrap();
        assert_eq!(leaf.result, EvalOutcome::Unknown);
        assert_eq!(leaf.unknown_reason.as_deref(), Some("no_value_cached"));
        assert!(leaf.value.is_none());
    }

    #[tokio::test]
    async fn explain_and_with_unknown_is_unknown() {
        let p = provider();
        let engine = RuleEngine::new(p.clone());
        let mut rule = CompiledRule::new("AND rule");
        rule.condition = Some(RuleCondition::Logical {
            operator: LogicalOperator::And,
            conditions: vec![
                RuleCondition::Comparison {
                    source: DataSourceId::device("s1", "a"),
                    operator: ComparisonOperator::GreaterThan,
                    threshold: 1.0,
                    threshold_value: None,
                },
                RuleCondition::Comparison {
                    source: DataSourceId::device("s1", "b"),
                    operator: ComparisonOperator::GreaterThan,
                    threshold: 1.0,
                    threshold_value: None,
                },
            ],
        });
        rule.trigger = RuleTrigger::Manual;
        rule.actions = vec![];
        rule.finalize();
        let id = rule.id.clone();
        engine.add_rule(rule).await.unwrap();

        p.update_value("device", "s1", "a", 5.0).await;
        let exp = engine.explain_rule(&id).await.unwrap();
        assert_eq!(exp.condition_met, Some(EvalOutcome::Unknown));

        p.update_value("device", "s1", "b", 0.0).await; // measured false
        let exp = engine.explain_rule(&id).await.unwrap();
        assert_eq!(
            exp.condition_met,
            Some(EvalOutcome::False),
            "false dominates unknown in AND"
        );
    }

    /// Regression (order-dependent NOT fold): multi-child NOT is NOR — any
    /// True child makes the result definitively False regardless of position
    /// or remaining children. The old fold surfaced True for [True, False]
    /// and Unknown for [True, Unknown].
    #[tokio::test]
    async fn explain_multi_child_not_short_circuits_on_true() {
        async fn not_outcome(a: Option<f64>, b: Option<f64>) -> EvalOutcome {
            let p = provider();
            let engine = RuleEngine::new(p.clone());
            let mut rule = CompiledRule::new("NOT rule");
            rule.condition = Some(RuleCondition::Logical {
                operator: LogicalOperator::Not,
                conditions: vec![
                    RuleCondition::Comparison {
                        source: DataSourceId::device("s1", "a"),
                        operator: ComparisonOperator::GreaterThan,
                        threshold: 1.0,
                        threshold_value: None,
                    },
                    RuleCondition::Comparison {
                        source: DataSourceId::device("s1", "b"),
                        operator: ComparisonOperator::GreaterThan,
                        threshold: 1.0,
                        threshold_value: None,
                    },
                ],
            });
            rule.trigger = RuleTrigger::Manual;
            rule.actions = vec![];
            rule.finalize();
            let id = rule.id.clone();
            engine.add_rule(rule).await.unwrap();
            if let Some(v) = a {
                p.update_value("device", "s1", "a", v).await;
            }
            if let Some(v) = b {
                p.update_value("device", "s1", "b", v).await;
            }
            engine
                .explain_rule(&id)
                .await
                .unwrap()
                .condition_met
                .unwrap()
        }

        // Any True child anywhere → definitively False (order-independent).
        assert_eq!(
            not_outcome(Some(5.0), Some(0.0)).await,
            EvalOutcome::False,
            "[True, False] must be False (old fold returned True)"
        );
        assert_eq!(
            not_outcome(Some(0.0), Some(5.0)).await,
            EvalOutcome::False,
            "[False, True] must be False"
        );
        assert_eq!(
            not_outcome(Some(5.0), None).await,
            EvalOutcome::False,
            "[True, Unknown] must be False (old fold returned Unknown)"
        );
        assert_eq!(
            not_outcome(None, Some(5.0)).await,
            EvalOutcome::False,
            "[Unknown, True] must be False"
        );
        // No True child, at least one Unknown → Unknown.
        assert_eq!(
            not_outcome(None, Some(0.0)).await,
            EvalOutcome::Unknown,
            "[Unknown, False] must stay Unknown"
        );
        // All children False → True.
        assert_eq!(
            not_outcome(Some(0.0), Some(0.0)).await,
            EvalOutcome::True,
            "[False, False] must be True"
        );
    }

    #[tokio::test]
    async fn explain_disabled_rule_and_zero_writes() {
        let p = provider();
        let (engine, id) = engine_with_high_temp_rule(&p, false).await;
        p.update_value("device", "sensor1", "temperature", 35.0)
            .await;

        let before = engine.get_rule(&id).await.unwrap();
        let exp = engine.explain_rule(&id).await.unwrap();
        assert!(!exp.enabled);
        assert!(exp.notes.iter().any(|n| n.contains("disabled")));

        // Read-only: rule state must be untouched (no condition_since side effect).
        let after = engine.get_rule(&id).await.unwrap();
        assert_eq!(
            before.state.condition_since, after.state.condition_since,
            "explain must not mutate rule state"
        );
        assert_eq!(before.state.trigger_count, after.state.trigger_count);
    }

    #[tokio::test]
    async fn explain_cooldown_and_history_evidence() {
        let p = provider();
        let (engine, id) = engine_with_high_temp_rule(&p, true).await;
        engine.set_rule_store(crate::store::RuleStore::memory().unwrap());
        p.update_value("device", "sensor1", "temperature", 35.0)
            .await;

        // Fire once (manual trigger) → claims cooldown + writes history.
        let fired = engine.execute_rule(&id).await;
        assert!(fired.success);

        let exp = engine.explain_rule(&id).await.unwrap();
        assert!(
            exp.cooldown.in_cooldown,
            "default 60s cooldown should be claimed after firing"
        );
        assert!(exp.cooldown.remaining_secs.unwrap_or(0) > 0);
        assert_eq!(exp.trigger_count, 1);
        let last = exp
            .last_execution
            .as_ref()
            .expect("history evidence attached");
        assert!(last.success);
        assert_eq!(last.actions_executed.len(), 1);
    }

    #[tokio::test]
    async fn explain_history_is_isolated_per_rule() {
        let p = provider();
        let engine = RuleEngine::new(p.clone());
        engine.set_rule_store(crate::store::RuleStore::memory().unwrap());
        let messages = Arc::new(neomind_messages::MessageManager::new());
        engine.set_message_manager(messages).await;

        let mk = |name: &str| {
            let mut rule = CompiledRule::new(name);
            rule.trigger = RuleTrigger::Manual;
            rule.actions = vec![RuleAction::Notify {
                message: name.to_string(),
                severity: NotifySeverity::Info,
            }];
            rule.finalize();
            rule
        };
        let rule_a = mk("rule-A");
        let rule_b = mk("rule-B");
        let id_a = rule_a.id.clone();
        let id_b = rule_b.id.clone();
        engine.add_rule(rule_a).await.unwrap();
        engine.add_rule(rule_b).await.unwrap();

        // Fire ONLY rule A: rule B's explanation must not borrow A's record.
        let fired = engine.execute_rule(&id_a).await;
        assert!(fired.success);

        let exp_b = engine.explain_rule(&id_b).await.unwrap();
        assert!(
            exp_b.last_execution.is_none(),
            "rule B never fired — its explanation must show NO execution evidence, got {:?}",
            exp_b.last_execution
        );
        let exp_a = engine.explain_rule(&id_a).await.unwrap();
        assert!(exp_a.last_execution.is_some(), "rule A sees its own record");

        // Fire B too: each explanation still reflects only its own record.
        assert!(engine.execute_rule(&id_b).await.success);
        let exp_a2 = engine.explain_rule(&id_a).await.unwrap();
        let exp_b = engine.explain_rule(&id_b).await.unwrap();
        let evidence_a = exp_a2
            .last_execution
            .expect("rule A keeps its own record after B fires");
        let evidence_b = exp_b.last_execution.expect("B now has its own record");
        assert_ne!(
            evidence_a.triggered_at, evidence_b.triggered_at,
            "A and B executions must be independent timestamps"
        );
        assert_eq!(
            evidence_a.triggered_at,
            exp_a
                .last_execution
                .as_ref()
                .expect("A before B fire")
                .triggered_at,
            "firing B must not rewrite A's latest evidence"
        );
        assert!(
            evidence_a
                .actions_executed
                .iter()
                .any(|a| a.contains("rule-A")),
            "A evidence must name rule-A actions: {:?}",
            evidence_a.actions_executed
        );
        assert!(
            evidence_b
                .actions_executed
                .iter()
                .any(|a| a.contains("rule-B")),
            "B evidence must name rule-B actions: {:?}",
            evidence_b.actions_executed
        );
    }

    #[tokio::test]
    async fn explain_history_isolated_without_store() {
        let p = provider();
        // No set_rule_store — evidence must come only from the in-memory deque
        // filter by rule_id (the bug that used to leak sibling history).
        let engine = RuleEngine::new(p.clone());
        let messages = Arc::new(neomind_messages::MessageManager::new());
        engine.set_message_manager(messages).await;

        let mk = |name: &str| {
            let mut rule = CompiledRule::new(name);
            rule.trigger = RuleTrigger::Manual;
            rule.actions = vec![RuleAction::Notify {
                message: name.to_string(),
                severity: NotifySeverity::Info,
            }];
            rule.finalize();
            rule
        };
        let rule_a = mk("mem-A");
        let rule_b = mk("mem-B");
        let id_a = rule_a.id.clone();
        let id_b = rule_b.id.clone();
        engine.add_rule(rule_a).await.unwrap();
        engine.add_rule(rule_b).await.unwrap();

        assert!(engine.execute_rule(&id_a).await.success);
        let exp_b = engine.explain_rule(&id_b).await.unwrap();
        assert!(
            exp_b.last_execution.is_none(),
            "no-store path must not borrow A's evidence for B: {:?}",
            exp_b.last_execution
        );
        assert!(engine
            .explain_rule(&id_a)
            .await
            .unwrap()
            .last_execution
            .is_some());
    }

    #[tokio::test]
    async fn explain_manual_for_duration_is_not_a_blocker() {
        let p = provider();
        let engine = RuleEngine::new(p);
        let mut rule = CompiledRule::new("manual-hold");
        rule.trigger = RuleTrigger::Manual;
        rule.for_duration = Some(Duration::from_secs(120));
        rule.condition = Some(RuleCondition::Comparison {
            source: DataSourceId::device("sensor1", "temperature"),
            operator: ComparisonOperator::GreaterThan,
            threshold: 0.0,
            threshold_value: None,
        });
        rule.finalize();
        let id = rule.id.clone();
        engine.add_rule(rule).await.unwrap();

        let exp = engine.explain_rule(&id).await.unwrap();
        let fd = exp.for_duration.expect("configured for_duration surfaced");
        assert!(
            !fd.applicable,
            "Manual must mark for_duration non-applicable"
        );
        assert!(
            fd.satisfied,
            "non-applicable hold must not report unsatisfied progress"
        );
        assert!(
            exp.notes.iter().any(|n| n.contains("Manual")),
            "notes must name Manual skip: {:?}",
            exp.notes
        );
    }

    #[tokio::test]
    async fn explain_comparison_type_mismatch_is_unknown() {
        let p = provider();
        p.update_value("device", "sensor1", "status", 1.0).await;
        let engine = RuleEngine::new(p);
        let mut rule = CompiledRule::new("mismatch");
        rule.trigger = RuleTrigger::Manual;
        rule.condition = Some(RuleCondition::Comparison {
            source: DataSourceId::device("sensor1", "status"),
            operator: ComparisonOperator::Contains,
            threshold: 0.0,
            threshold_value: Some("online".into()),
        });
        rule.finalize();
        let id = rule.id.clone();
        engine.add_rule(rule).await.unwrap();

        let exp = engine.explain_rule(&id).await.unwrap();
        let cond = exp.condition.expect("condition");
        assert_eq!(cond.result, EvalOutcome::Unknown);
        assert_eq!(
            cond.unknown_reason.as_deref(),
            Some("operator_value_mismatch")
        );
    }

    #[tokio::test]
    async fn explain_comparison_numeric_op_on_text_is_unknown() {
        let p = provider();
        p.update_string_value("device", "sensor1", "label", "online")
            .await;
        let engine = RuleEngine::new(p);
        let mut rule = CompiledRule::new("text-gt");
        rule.trigger = RuleTrigger::Manual;
        rule.condition = Some(RuleCondition::Comparison {
            source: DataSourceId::device("sensor1", "label"),
            operator: ComparisonOperator::GreaterThan,
            threshold: 1.0,
            threshold_value: None,
        });
        rule.finalize();
        let id = rule.id.clone();
        engine.add_rule(rule).await.unwrap();

        let exp = engine.explain_rule(&id).await.unwrap();
        let cond = exp.condition.expect("condition");
        assert_eq!(cond.result, EvalOutcome::Unknown);
        assert_eq!(cond.unknown_reason.as_deref(), Some("value_not_numeric"));
    }

    #[tokio::test]
    async fn explain_unknown_rule_is_none() {
        let p = provider();
        let engine = RuleEngine::new(p);
        // Valid UUID format, but no such rule is registered.
        let ghost = RuleId::from_string("00000000-0000-4000-8000-000000000000").unwrap();
        assert!(engine.explain_rule(&ghost).await.is_none());
    }
}
