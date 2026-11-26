//! AgentSpec and related contracts.

use serde::{Deserialize, Serialize};

/// Official task-family preset id (single source of truth in `presets`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetId {
    DailyBrief,
    PartialMeter,
    EventVision,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputModality {
    Metric,
    TimeSeries,
    StructuredJson,
    Text,
    Image,
    CallerInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InputSourceRef {
    DeviceMetric { device_id: String, field: String },
    WholeDevice { device_id: String },
    ExtensionInstance { id: String },
    UpstreamAi { agent_id: String, field: String },
    CallerPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FreshnessPolicy {
    SameReport,
    LatestWithin { max_age_secs: u64 },
    LookbackWindow { lookback_secs: u64, max_points: u32 },
    Any,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissingPolicy {
    Undetermined,
    ReportStale,
    Skip,
    Ignore,
}

/// One authorized input requirement. Host InputPipeline reads these even when
/// devices are empty and ToolPolicy::Disabled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputRequirement {
    pub key: String,
    pub source: InputSourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    #[serde(default)]
    pub params: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_scope: Option<String>,
    pub timeout_secs: u32,
    pub result_type: InputModality,
    pub freshness: FreshnessPolicy,
    pub on_missing: MissingPolicy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct InputContract {
    pub requirements: Vec<InputRequirement>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolPolicy {
    Disabled,
    Scoped {
        allowed_tools: Vec<String>,
        #[serde(default)]
        allowed_commands: Vec<String>,
    },
    Unrestricted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputField {
    pub name: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OutputContract {
    #[serde(default)]
    pub fields: Vec<OutputField>,
    /// Publish `ai:{agent_id}:{field}` when true.
    #[serde(default)]
    pub publish: bool,
}

/// Three-way notify policy (complete / alert / failure).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct NotifyPolicy {
    #[serde(default)]
    pub channels: Vec<String>,
    #[serde(default)]
    pub on_complete: bool,
    #[serde(default)]
    pub on_alert: bool,
    #[serde(default)]
    pub alert_depends_on: Vec<String>,
    #[serde(default)]
    pub on_failure: bool,
    #[serde(default)]
    pub pending_confirmation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TriggerSpec {
    Cron {
        expr: String,
        timezone: String,
    },
    Interval {
        every_secs: u64,
    },
    Event {
        #[serde(default)]
        filter: serde_json::Value,
    },
    Manual,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PacingPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub burst_merge_secs: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_interval_secs: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_runs_per_day: Option<u32>,
    /// IANA timezone for the daily quota window (falls back to cron trigger TZ, then UTC).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: u32,
}

fn default_failure_threshold() -> u32 {
    3
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlledAction {
    pub name: String,
    pub destination: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub idempotent: bool,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// Frozen static configuration for a managed agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSpec {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub user_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_backend_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<PresetId>,
    pub trigger: TriggerSpec,
    #[serde(default)]
    pub pacing: PacingPolicy,
    pub input: InputContract,
    pub tool_policy: ToolPolicy,
    #[serde(default)]
    pub output: OutputContract,
    #[serde(default)]
    pub notify: NotifyPolicy,
    #[serde(default)]
    pub controlled_actions: Vec<ControlledAction>,
    /// Monotonic config revision; bump on every successful update.
    pub config_version: u64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Create/update request DTO (M1 test service + future API).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSpecDraft {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub user_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_backend_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<PresetId>,
    pub trigger: TriggerSpec,
    #[serde(default)]
    pub pacing: PacingPolicy,
    pub input: InputContract,
    pub tool_policy: ToolPolicy,
    #[serde(default)]
    pub output: OutputContract,
    #[serde(default)]
    pub notify: NotifyPolicy,
    #[serde(default)]
    pub controlled_actions: Vec<ControlledAction>,
}

/// Partial update: omitted fields keep previous values; explicit empty channels = mute.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AgentSpecPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_backend_id: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<Option<PresetId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<TriggerSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pacing: Option<PacingPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<InputContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_policy: Option<ToolPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<OutputContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notify: Option<NotifyPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controlled_actions: Option<Vec<ControlledAction>>,
}

/// Static EffectivePlan — zero external reads, zero side effects.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectivePlan {
    pub agent_id: String,
    pub config_version: u64,
    pub trigger_summary: String,
    pub timezone: Option<String>,
    pub authorized_reads: Vec<PlanRead>,
    pub tool_policy: ToolPolicy,
    pub notify: NotifyPolicy,
    pub controlled_actions: Vec<ControlledAction>,
    #[serde(default)]
    pub warnings: Vec<String>,
    /// Always 0 for static preview (asserted by T12).
    pub external_read_counts: ExternalReadCounts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExternalReadCounts {
    pub weather: u64,
    pub tariff: u64,
    pub other: u64,
}

impl ExternalReadCounts {
    pub fn total(&self) -> u64 {
        self.weather + self.tariff + self.other
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanRead {
    pub key: String,
    pub source_id: String,
    pub operation: Option<String>,
    pub auth_scope: Option<String>,
    pub capability_class: String,
}
