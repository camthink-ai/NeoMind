//! Rule v2 data models.
//!
//! Pure JSON rule definitions — no DSL parsing required.
//!
//! ## Conditions (3 types)
//! - **Comparison**: metric OP threshold (>, <, >=, <=, ==, !=)
//! - **Range**: metric BETWEEN min AND max
//! - **Logical**: AND / OR / NOT combining sub-conditions
//!
//! ## Actions (3 types)
//! - **Notify**: send a notification message
//! - **Execute**: run a command on a device or extension
//! - **TriggerAgent**: hand off to an AI Agent

use chrono::{DateTime, Utc};
use neomind_core::datasource::DataSourceId;
use serde::{Deserialize, Deserializer, Serialize};
use std::time::Duration;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// IDs
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Rule value — numeric or text
// ---------------------------------------------------------------------------

/// A rule value that can be either numeric or textual.
///
/// Used by [`ValueProvider`] so that string metrics (e.g. device status `"online"`)
/// participate in rule evaluation alongside numeric metrics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum RuleValue {
    Number(f64),
    Text(String),
}

impl RuleValue {
    /// Extract the numeric value if this is a `Number`.
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(v) => Some(*v),
            Self::Text(_) => None,
        }
    }

    /// Extract the text value if this is `Text`.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s),
            Self::Number(_) => None,
        }
    }
}

impl From<f64> for RuleValue {
    fn from(v: f64) -> Self {
        Self::Number(v)
    }
}

impl From<String> for RuleValue {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}

// ---------------------------------------------------------------------------
// Comparison operator
// ---------------------------------------------------------------------------

/// Comparison operators for conditions.
/// Serializes as symbol (">"). Accepts both snake_case ("greater_than") and symbol (">") on deserialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComparisonOperator {
    GreaterThan,
    LessThan,
    GreaterEqual,
    LessEqual,
    Equal,
    NotEqual,
    // String-only operators
    Contains,
    StartsWith,
    EndsWith,
    Regex,
}

impl Serialize for ComparisonOperator {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.symbol())
    }
}

impl<'de> Deserialize<'de> for ComparisonOperator {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            ">" | "greater_than" | "gt" => Ok(Self::GreaterThan),
            "<" | "less_than" | "lt" => Ok(Self::LessThan),
            ">=" | "greater_equal" | "gte" | "greater_than_or_equal" => Ok(Self::GreaterEqual),
            "<=" | "less_equal" | "lte" | "less_than_or_equal" => Ok(Self::LessEqual),
            "==" | "equal" | "eq" => Ok(Self::Equal),
            "!=" | "not_equal" | "ne" => Ok(Self::NotEqual),
            "contains" => Ok(Self::Contains),
            "starts_with" | "startswith" => Ok(Self::StartsWith),
            "ends_with" | "endswith" => Ok(Self::EndsWith),
            "regex" | "matches" => Ok(Self::Regex),
            _ => Err(serde::de::Error::unknown_variant(
                &s,
                &[
                    ">",
                    "<",
                    ">=",
                    "<=",
                    "==",
                    "!=",
                    "greater_than",
                    "less_than",
                    "greater_equal",
                    "less_equal",
                    "equal",
                    "not_equal",
                    "contains",
                    "starts_with",
                    "ends_with",
                    "regex",
                ],
            )),
        }
    }
}

impl ComparisonOperator {
    /// Evaluate a numeric comparison.
    pub fn evaluate(&self, left: f64, right: f64) -> bool {
        match self {
            Self::GreaterThan => left > right,
            Self::LessThan => left < right,
            Self::GreaterEqual => left >= right,
            Self::LessEqual => left <= right,
            Self::Equal => (left - right).abs() < 0.0001,
            Self::NotEqual => (left - right).abs() >= 0.0001,
            // String-only operators always return false for numeric comparison
            Self::Contains | Self::StartsWith | Self::EndsWith | Self::Regex => false,
        }
    }

    /// Evaluate a string comparison.
    pub fn evaluate_str(&self, left: &str, right: &str) -> bool {
        match self {
            Self::Equal => left == right,
            Self::NotEqual => left != right,
            Self::Contains => left.contains(right),
            Self::StartsWith => left.starts_with(right),
            Self::EndsWith => left.ends_with(right),
            Self::Regex => regex::Regex::new(right)
                .map(|r| r.is_match(left))
                .unwrap_or(false),
            // Numeric-only operators always return false for string comparison
            Self::GreaterThan | Self::LessThan | Self::GreaterEqual | Self::LessEqual => false,
        }
    }

    /// Returns true if this operator only makes sense for string values.
    pub fn is_string_op(&self) -> bool {
        matches!(
            self,
            Self::Contains | Self::StartsWith | Self::EndsWith | Self::Regex
        )
    }

    /// Human-readable symbol.
    pub fn symbol(&self) -> &str {
        match self {
            Self::GreaterThan => ">",
            Self::LessThan => "<",
            Self::GreaterEqual => ">=",
            Self::LessEqual => "<=",
            Self::Equal => "==",
            Self::NotEqual => "!=",
            Self::Contains => "contains",
            Self::StartsWith => "starts_with",
            Self::EndsWith => "ends_with",
            Self::Regex => "regex",
        }
    }
}

// ---------------------------------------------------------------------------
// Logical operator
// ---------------------------------------------------------------------------

/// Logical operators for combining conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogicalOperator {
    And,
    Or,
    Not,
}

// ---------------------------------------------------------------------------
// Condition — 3 types only
// ---------------------------------------------------------------------------

/// A rule condition.
///
/// Use [`RuleCondition::extract_sources`] to discover which DataSourceIds
/// Accepts a number (f64) OR a boolean as a comparison threshold. Boolean
/// metrics are ingested as Number(0/1), so a boolean threshold maps naturally
/// (true→1.0, false→0.0) — lets users/agents write `compressor_running ==
/// false` instead of hacking `!= 1`. (Surfaced by the solution-level
/// cold-chain scenario: the rule API rejected a boolean threshold.)
#[derive(Deserialize)]
#[serde(untagged)]
enum ThresholdNumOrBool {
    Num(f64),
    Bool(bool),
}

fn deserialize_threshold<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    match ThresholdNumOrBool::deserialize(d)? {
        ThresholdNumOrBool::Num(n) => Ok(n),
        ThresholdNumOrBool::Bool(b) => Ok(if b { 1.0 } else { 0.0 }),
    }
}

/// the condition references (needed for the subscription index).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "condition_type", rename_all = "snake_case")]
pub enum RuleCondition {
    /// A single metric compared against a threshold.
    Comparison {
        /// Data source providing the metric value.
        #[serde(with = "datasource_id_serde")]
        source: DataSourceId,
        operator: ComparisonOperator,
        #[serde(default, deserialize_with = "deserialize_threshold")]
        threshold: f64,
        /// String threshold for string comparison operators (contains, starts_with, etc.).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        threshold_value: Option<String>,
    },
    /// A single metric checked against a [min, max] range (inclusive).
    Range {
        #[serde(with = "datasource_id_serde")]
        source: DataSourceId,
        min: f64,
        max: f64,
    },
    /// Logical combination of sub-conditions.
    Logical {
        operator: LogicalOperator,
        conditions: Vec<RuleCondition>,
    },
}

impl RuleCondition {
    /// Collect all `DataSourceId`s referenced by this condition tree.
    pub fn extract_sources(&self) -> Vec<DataSourceId> {
        match self {
            RuleCondition::Comparison { source, .. } => vec![source.clone()],
            RuleCondition::Range { source, .. } => vec![source.clone()],
            RuleCondition::Logical { conditions, .. } => conditions
                .iter()
                .flat_map(|c| c.extract_sources())
                .collect(),
        }
    }

    /// Evaluate the condition against a value provider.
    pub fn evaluate(&self, provider: &dyn ValueProvider) -> bool {
        match self {
            RuleCondition::Comparison {
                source,
                operator,
                threshold,
                threshold_value,
            } => match provider.get_by_source(source) {
                Some(rv) => match rv {
                    RuleValue::Number(v) => operator.evaluate(v, *threshold),
                    RuleValue::Text(s) => {
                        let fallback = threshold.to_string();
                        let t = threshold_value.as_deref().unwrap_or(&fallback);
                        operator.evaluate_str(&s, t)
                    }
                },
                None => false,
            },
            RuleCondition::Range { source, min, max } => match provider.get_by_source(source) {
                Some(rv) => rv
                    .as_number()
                    .map(|v| v >= *min && v <= *max)
                    .unwrap_or(false),
                None => false,
            },
            RuleCondition::Logical {
                operator,
                conditions,
            } => match operator {
                LogicalOperator::And => conditions.iter().all(|c| c.evaluate(provider)),
                LogicalOperator::Or => conditions.iter().any(|c| c.evaluate(provider)),
                LogicalOperator::Not => {
                    // NOT: true only when NONE of the sub-conditions are met.
                    // For a single condition this is standard logical NOT.
                    !conditions.iter().any(|c| c.evaluate(provider))
                }
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Value provider trait (v2)
// ---------------------------------------------------------------------------

/// Provides metric values for rule evaluation.
pub trait ValueProvider: Send + Sync {
    /// Get a metric value by its DataSourceId.
    fn get_by_source(&self, source: &DataSourceId) -> Option<RuleValue>;

    /// Downcast support for concrete provider access.
    fn as_any(&self) -> &dyn std::any::Any;
}

// ---------------------------------------------------------------------------
// Action — 3 types only
// ---------------------------------------------------------------------------

/// What kind of target an `Execute` action points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteTarget {
    Device,
    Extension,
}

/// Notification severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum NotifySeverity {
    #[default]
    Info,
    Warning,
    Critical,
    Emergency,
}

/// A rule action.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuleAction {
    /// Send a notification message.
    Notify {
        /// Message template — supports `{value}`, `{source_id}` placeholders.
        message: String,
        #[serde(default)]
        severity: NotifySeverity,
    },
    /// Execute a command on a device or extension.
    Execute {
        /// Target device ID or extension ID.
        target: String,
        target_type: ExecuteTarget,
        command: String,
        #[serde(default)]
        params: serde_json::Value,
    },
    /// Trigger an AI Agent.
    TriggerAgent {
        agent_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        input: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<serde_json::Value>,
    },
}

// ---------------------------------------------------------------------------
// Trigger
// ---------------------------------------------------------------------------

/// How a rule is triggered.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "trigger_type", rename_all = "snake_case")]
pub enum RuleTrigger {
    /// Triggered when a subscribed data source changes.
    DataChange {
        /// Auto-extracted from condition. Populated by the engine.
        #[serde(default, with = "datasource_id_vec_serde")]
        sources: Vec<DataSourceId>,
    },
    /// Triggered on a cron schedule.
    Schedule { cron: String },
    /// Triggered manually via API / CLI.
    Manual,
}

impl RuleTrigger {
    /// Build a DataChange trigger, extracting sources from the condition.
    pub fn from_condition(condition: &Option<RuleCondition>) -> Self {
        let sources = condition
            .as_ref()
            .map(|c| c.extract_sources())
            .unwrap_or_default();
        Self::DataChange { sources }
    }
}

// ---------------------------------------------------------------------------
// Rule state (runtime, persisted)
// ---------------------------------------------------------------------------

/// Runtime state of a rule.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RuleState {
    pub trigger_count: u64,
    pub last_triggered: Option<DateTime<Utc>>,
    /// When the condition first became true (for `for_duration`).
    /// Stored as DateTime so it survives serialization.
    pub condition_since: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// Compiled rule (complete)
// ---------------------------------------------------------------------------

/// A compiled rule ready for evaluation and execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledRule {
    #[serde(default)]
    pub id: RuleId,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub tags: Vec<String>,

    pub trigger: RuleTrigger,
    /// None = unconditional (Schedule / Manual).
    pub condition: Option<RuleCondition>,
    #[serde(default)]
    pub actions: Vec<RuleAction>,

    /// Minimum time between triggers. Default 60 s.
    #[serde(
        serialize_with = "serialize_duration",
        deserialize_with = "deserialize_duration",
        default = "default_cooldown"
    )]
    pub cooldown: Duration,

    /// Condition must hold for this long before triggering.
    #[serde(
        serialize_with = "serialize_duration_opt",
        deserialize_with = "deserialize_duration_opt",
        default
    )]
    pub for_duration: Option<Duration>,

    #[serde(default)]
    pub state: RuleState,

    /// Auto-generated human-readable preview (read-only).
    #[serde(default)]
    pub dsl_preview: String,

    /// Frontend UI state for edit restoration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<serde_json::Value>,

    #[serde(default)]
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub updated_at: DateTime<Utc>,
}

impl CompiledRule {
    /// Create a new rule with defaults.
    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: RuleId::new(),
            name: name.into(),
            description: None,
            enabled: true,
            tags: Vec::new(),
            trigger: RuleTrigger::Manual,
            condition: None,
            actions: Vec::new(),
            cooldown: Duration::from_secs(60),
            for_duration: None,
            state: RuleState::default(),
            dsl_preview: String::new(),
            source: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Finalize a rule: auto-generate dsl_preview and extract trigger sources.
    pub fn finalize(&mut self) {
        // Auto-extract sources for DataChange trigger
        if let RuleTrigger::DataChange { sources } = &mut self.trigger {
            *sources = self
                .condition
                .as_ref()
                .map(|c| c.extract_sources())
                .unwrap_or_default();
        }
        self.dsl_preview = crate::preview::to_dsl_preview(self);
        self.updated_at = Utc::now();
    }
}

// ---------------------------------------------------------------------------
// Execution result
// ---------------------------------------------------------------------------

/// Result of executing a rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleExecutionResult {
    pub rule_id: RuleId,
    pub rule_name: String,
    pub success: bool,
    pub actions_executed: Vec<String>,
    pub error: Option<String>,
    pub duration_ms: u64,
    pub triggered_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Serde helpers
// ---------------------------------------------------------------------------

fn default_true() -> bool {
    true
}

fn default_cooldown() -> Duration {
    Duration::from_secs(60)
}

fn serialize_duration<S: serde::Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_u64(d.as_millis() as u64)
}

fn deserialize_duration<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
    let ms = u64::deserialize(d)?;
    Ok(Duration::from_millis(ms))
}

fn serialize_duration_opt<S: serde::Serializer>(
    d: &Option<Duration>,
    s: S,
) -> Result<S::Ok, S::Error> {
    match d {
        Some(dur) => s.serialize_some(&(dur.as_millis() as u64)),
        None => s.serialize_none(),
    }
}

fn deserialize_duration_opt<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<Duration>, D::Error> {
    let opt = Option::<u64>::deserialize(d)?;
    Ok(opt.map(Duration::from_millis))
}

/// Custom serde for DataSourceId: serializes as the storage_key string.
mod datasource_id_serde {
    use neomind_core::datasource::DataSourceId;
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(ds: &DataSourceId, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&ds.storage_key())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<DataSourceId, D::Error> {
        let key = String::deserialize(d)?;
        DataSourceId::parse(&key)
            .ok_or_else(|| serde::de::Error::custom(format!("invalid DataSourceId: {}", key)))
    }
}

/// Serde module for `Vec<DataSourceId>` — serializes as string array.
mod datasource_id_vec_serde {
    use neomind_core::datasource::DataSourceId;
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &[DataSourceId], s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(v.iter().map(|ds| ds.storage_key()))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<DataSourceId>, D::Error> {
        let keys: Vec<String> = Vec::deserialize(d)?;
        keys.into_iter()
            .map(|key| {
                DataSourceId::parse(&key).ok_or_else(|| {
                    serde::de::Error::custom(format!("invalid DataSourceId: {}", key))
                })
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comparison_operator_evaluate() {
        assert!(ComparisonOperator::GreaterThan.evaluate(10.0, 5.0));
        assert!(!ComparisonOperator::GreaterThan.evaluate(5.0, 10.0));
        assert!(ComparisonOperator::LessThan.evaluate(5.0, 10.0));
        assert!(ComparisonOperator::GreaterEqual.evaluate(10.0, 10.0));
        assert!(ComparisonOperator::Equal.evaluate(5.0, 5.0));
        assert!(ComparisonOperator::NotEqual.evaluate(5.1, 5.0));
    }

    #[test]
    fn test_string_comparison_operators() {
        assert!(ComparisonOperator::Contains.evaluate_str("device online", "online"));
        assert!(!ComparisonOperator::Contains.evaluate_str("offline", "online"));
        assert!(ComparisonOperator::StartsWith.evaluate_str("error_timeout", "error"));
        assert!(ComparisonOperator::EndsWith.evaluate_str("device_error", "error"));
        assert!(ComparisonOperator::Regex.evaluate_str("temp_42c", r"temp_\d+c"));
        assert!(!ComparisonOperator::Regex.evaluate_str("hello", r"\d+"));
        assert!(ComparisonOperator::Equal.evaluate_str("online", "online"));
        assert!(ComparisonOperator::NotEqual.evaluate_str("online", "offline"));
        // Numeric-only operators return false for strings
        assert!(!ComparisonOperator::GreaterThan.evaluate_str("b", "a"));
    }

    #[test]
    fn test_rule_value_serde() {
        let num = RuleValue::Number(42.0);
        let json = serde_json::to_string(&num).unwrap();
        assert_eq!(json, "42.0");
        let back: RuleValue = serde_json::from_str(&json).unwrap();
        assert_eq!(back, RuleValue::Number(42.0));

        let text = RuleValue::Text("online".into());
        let json = serde_json::to_string(&text).unwrap();
        assert_eq!(json, "\"online\"");
        let back: RuleValue = serde_json::from_str(&json).unwrap();
        assert_eq!(back, RuleValue::Text("online".into()));
    }

    #[test]
    fn comparison_threshold_accepts_boolean() {
        // Boolean threshold → 0.0/1.0 (boolean metrics are ingested as
        // Number(0/1)), so `compressor_running == false` parses. Surfaced by
        // the solution-level cold-chain scenario (rule API rejected bool).
        let c: RuleCondition = serde_json::from_str(
            r#"{"condition_type":"comparison","source":"device:d:compressor_running","operator":"equal","threshold":false}"#,
        ).unwrap();
        match c {
            RuleCondition::Comparison { threshold, .. } => assert_eq!(threshold, 0.0),
            _ => panic!("expected Comparison"),
        }
        let c: RuleCondition = serde_json::from_str(
            r#"{"condition_type":"comparison","source":"device:d:on","operator":"equal","threshold":true}"#,
        ).unwrap();
        match c {
            RuleCondition::Comparison { threshold, .. } => assert_eq!(threshold, 1.0),
            _ => panic!("expected Comparison"),
        }
        // Numeric thresholds still parse (backward compat).
        let c: RuleCondition = serde_json::from_str(
            r#"{"condition_type":"comparison","source":"device:d:t","operator":"greater_than","threshold":4.5}"#,
        ).unwrap();
        match c {
            RuleCondition::Comparison { threshold, .. } => assert_eq!(threshold, 4.5),
            _ => panic!("expected Comparison"),
        }
    }

    #[test]
    fn test_condition_extract_sources() {
        let cond = RuleCondition::Comparison {
            source: DataSourceId::device("sensor1", "temperature"),
            operator: ComparisonOperator::GreaterThan,
            threshold: 50.0,
            threshold_value: None,
        };
        let sources = cond.extract_sources();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].source_id, "sensor1");
        assert_eq!(sources[0].field_path, "temperature");
    }

    #[test]
    fn test_logical_condition_extract_sources() {
        let cond = RuleCondition::Logical {
            operator: LogicalOperator::And,
            conditions: vec![
                RuleCondition::Comparison {
                    source: DataSourceId::device("s1", "temp"),
                    operator: ComparisonOperator::GreaterThan,
                    threshold: 30.0,
                    threshold_value: None,
                },
                RuleCondition::Range {
                    source: DataSourceId::extension("weather", "humidity"),
                    min: 20.0,
                    max: 80.0,
                },
            ],
        };
        let sources = cond.extract_sources();
        assert_eq!(sources.len(), 2);
    }

    #[test]
    fn test_serialize_deserialize_rule() {
        let mut rule = CompiledRule::new("Test Rule");
        rule.description = Some("A test".into());
        rule.condition = Some(RuleCondition::Comparison {
            source: DataSourceId::device("sensor1", "temperature"),
            operator: ComparisonOperator::GreaterThan,
            threshold: 50.0,
            threshold_value: None,
        });
        rule.trigger = RuleTrigger::from_condition(&rule.condition);
        rule.actions = vec![
            RuleAction::Notify {
                message: "Too hot!".into(),
                severity: NotifySeverity::Critical,
            },
            RuleAction::Execute {
                target: "fan-001".into(),
                target_type: ExecuteTarget::Device,
                command: "turn_on".into(),
                params: serde_json::json!({"speed": 100}),
            },
        ];

        let json = serde_json::to_string_pretty(&rule).unwrap();
        let back: CompiledRule = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "Test Rule");
        assert!(back.condition.is_some());
        assert_eq!(back.actions.len(), 2);
    }

    #[test]
    fn test_action_tagged_serde() {
        let action = RuleAction::TriggerAgent {
            agent_id: "agent-1".into(),
            input: Some("Check temperature".into()),
            data: None,
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"type\":\"trigger_agent\""));

        let back: RuleAction = serde_json::from_str(&json).unwrap();
        match back {
            RuleAction::TriggerAgent { agent_id, .. } => {
                assert_eq!(agent_id, "agent-1");
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_trigger_serde() {
        let trigger = RuleTrigger::Schedule {
            cron: "0 */5 * * *".into(),
        };
        let json = serde_json::to_string(&trigger).unwrap();
        assert!(json.contains("\"trigger_type\":\"schedule\""));
    }

    #[test]
    fn test_data_change_trigger_roundtrip() {
        // Serialize: Vec<DataSourceId> → string array
        let trigger = RuleTrigger::DataChange {
            sources: vec![
                DataSourceId::device("sensor1", "temperature"),
                DataSourceId::extension("weather", "humidity"),
            ],
        };
        let json = serde_json::to_string(&trigger).unwrap();
        assert!(json.contains("\"trigger_type\":\"data_change\""));
        assert!(json.contains("\"device:sensor1:temperature\""));
        assert!(json.contains("\"extension:weather:humidity\""));

        // Deserialize: string array → Vec<DataSourceId>
        let input =
            r#"{"trigger_type":"data_change","sources":["device:s1:temp","extension:ext1:field"]}"#;
        let parsed: RuleTrigger = serde_json::from_str(input).unwrap();
        match parsed {
            RuleTrigger::DataChange { sources } => {
                assert_eq!(sources.len(), 2);
                assert_eq!(sources[0].storage_key(), "device:s1:temp");
                assert_eq!(sources[1].storage_key(), "extension:ext1:field");
            }
            _ => panic!("Expected DataChange"),
        }
    }
}
