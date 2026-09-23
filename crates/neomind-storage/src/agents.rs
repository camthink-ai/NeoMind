//! AI Agent storage for persistent autonomous agents.
//!
//! This module provides storage for AI Agents that:
//! - Execute periodically or based on events
//! - Maintain persistent memory across executions
//! - Record decision processes for verification
//! - Handle errors gracefully for long-running stability

use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

use std::sync::Arc;

use crate::Error;

// Tables for agent storage
const AGENTS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("agents");
const AGENT_EXECUTIONS_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("agent_executions");
const AGENT_MEMORY_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("agent_memory");

/// AI Agent store for persisting autonomous agents.
pub struct AgentStore {
    /// redb database
    db: Arc<Database>,
}

/// An AI Agent definition.
///
/// Represents a user-created autonomous agent that monitors devices,
/// analyzes data, and takes actions based on natural language requirements.
/// The agent maintains a persistent conversation history across executions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiAgent {
    /// Unique agent ID
    pub id: String,
    /// Agent name
    pub name: String,
    /// User-provided description (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// User's natural language description of requirements
    pub user_prompt: String,
    /// Optional LLM backend ID for this agent (uses default if not specified)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_backend_id: Option<String>,
    /// AI-generated understanding of the requirements
    pub parsed_intent: Option<ParsedIntent>,
    /// Selected resources (devices, metrics, commands)
    pub resources: Vec<AgentResource>,
    /// Schedule configuration
    pub schedule: AgentSchedule,
    /// Agent status
    pub status: AgentStatus,
    /// Priority for execution (0-255, higher = more priority)
    #[serde(default = "default_priority")]
    pub priority: u8,
    /// Creation timestamp
    pub created_at: i64,
    /// Last update timestamp
    pub updated_at: i64,
    /// Last execution timestamp
    pub last_execution_at: Option<i64>,
    /// Execution statistics
    pub stats: AgentStats,
    /// Persistent memory across executions
    pub memory: AgentMemory,
    /// Conversation history — kept for backward compat deserialization, no longer written to.
    #[serde(default)]
    pub conversation_history: Vec<serde_json::Value>,
    /// User messages sent between executions
    #[serde(default)]
    pub user_messages: Vec<UserMessage>,
    /// Compressed summary of old conversation turns — kept for backward compat, no longer written to.
    #[serde(default)]
    pub conversation_summary: Option<String>,
    /// How many recent turns to include in LLM context
    #[serde(default = "default_context_window")]
    pub context_window_size: usize,
    /// DEPRECATED (dead field, 2026-08-26): written nowhere meaningful — the
    /// executor unconditionally uses tool-calling when the LLM supports it
    /// (`should_use_tools` ignores this flag), and it is absent from the API
    /// DTOs and UI. Kept ONLY for bincode compatibility with rows already in
    /// `agents.redb` (removing a mid-struct field desyncs every stored agent);
    /// physically remove alongside the next storage migration. Always false.
    #[serde(default)]
    pub enable_tool_chaining: bool,
    /// Maximum chain depth (prevents infinite loops)
    #[serde(default = "default_max_chain_depth")]
    pub max_chain_depth: usize,
    /// Tool configuration for function calling mode
    #[serde(default)]
    pub tool_config: Option<AgentToolConfig>,
    /// Execution mode: focused (single-pass with bound resources) or free (multi-round tool calling)
    #[serde(default)]
    pub execution_mode: ExecutionMode,
    /// Error message (if status is error)
    pub error_message: Option<String>,
    /// Custom system prompt override (replaces default IoT role prompt)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// Maximum number of automatic retries for transient execution failures (default: 0 = no retry)
    #[serde(default)]
    pub max_retries: u32,
    /// Current consecutive failure count (reset to 0 on success)
    #[serde(default)]
    pub consecutive_failures: u32,
    /// Structured-mode (L0) output contract: fields published as
    /// `ai:{agent_id}:{field}`. None for Focused/Free agents.
    /// Tail-appended with serde defaults — existing bincode rows decode
    /// unchanged (same pattern as `enable_tool_chaining`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Vec<OperatorField>>,
    /// Structured-mode (L0) runtime tuning (debounce / budget / circuit
    /// breaker). None for Focused/Free agents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_config: Option<OperatorConfig>,
    /// Memory axis (2026-09-22): Tool = stateless per run, Assistant =
    /// carry the recent-execution narrative. None = mode-derived default
    /// (Structured→Tool, Free/Focused→Assistant). Tail-appended, serde
    /// defaulted — existing rows decode unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_mode: Option<MemoryMode>,
    /// Explicit notification routing; None keeps legacy behavior
    /// (intent-keyword alerts) untouched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notify: Option<AgentNotify>,
}

/// Tool configuration for AI Agent function calling mode.
#[derive(utoipa::ToSchema, Debug, Clone, Serialize, Deserialize)]
pub struct AgentToolConfig {
    /// Whether tool mode is enabled (default true). When false, the agent gets
    /// NO tools (forced to text-only responses).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Allowed tool names (empty = all available tools)
    #[serde(default)]
    pub allowed_tools: Vec<String>,
}

/// Serde default helper: `true`. Used by `AgentToolConfig::enabled` so clients
/// can omit it (tools on by default) instead of getting a 400 "missing field".
fn default_true() -> bool {
    true
}

/// Default value for context window size.
fn default_context_window() -> usize {
    10
}

/// Default value for max chain depth.
fn default_max_chain_depth() -> usize {
    5 // Allow up to 5 chain steps by default (enough for multi-step Focused analysis)
}

/// Default value for agent priority.
fn default_priority() -> u8 {
    128 // Middle priority (0-255 range)
}

/// Parsed intent from user's natural language description.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedIntent {
    /// Intent type
    pub intent_type: IntentType,
    /// Target metrics to monitor
    pub target_metrics: Vec<String>,
    /// Conditions to evaluate
    pub conditions: Vec<String>,
    /// Actions to take
    pub actions: Vec<String>,
    /// Confidence in parsing (0-1)
    pub confidence: f32,
}

/// Type of intent extracted from user prompt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IntentType {
    /// Monitor and alert on conditions
    Monitoring,
    /// Generate periodic reports
    ReportGeneration,
    /// Analyze data for anomalies
    AnomalyDetection,
    /// Execute control commands
    Control,
    /// Complex multi-step automation
    Automation,
}

/// A resource selected by the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResource {
    /// Resource type
    pub resource_type: ResourceType,
    /// Resource ID (device_id, metric_name, etc.)
    pub resource_id: String,
    /// Display name
    pub name: String,
    /// Additional configuration
    pub config: serde_json::Value,
}

/// Type of resource.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    Device,
    Metric,
    Command,
    DataStream,
    ExtensionTool,
    ExtensionMetric,
}

/// Agent schedule configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSchedule {
    /// Schedule type
    pub schedule_type: ScheduleType,
    /// Cron expression (for Cron type)
    pub cron_expression: Option<String>,
    /// Interval in seconds (for Interval type)
    pub interval_seconds: Option<u64>,
    /// Event filter (for Event type)
    pub event_filter: Option<String>,
    /// Timezone for schedule
    pub timezone: Option<String>,
}

/// One source inside a structured `EventFilter` (M2-1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventSource {
    /// Source kind as the trigger path names it: "device" | "extension" | ...
    #[serde(rename = "type")]
    pub source_type: String,
    /// Concrete source id, or "all" to match every source of this type.
    #[serde(default)]
    pub id: String,
    /// Metric/output field; None matches any field on the source.
    #[serde(default)]
    pub field: Option<String>,
}

/// Structured event filter (M2-1) — the polymorphic view over the stored
/// `event_filter` string. `any` fires on the first matching source (what the
/// legacy shape has always meant); `all` fires only once every listed source
/// has been seen inside `within_secs`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct EventFilter {
    /// Fires when ANY of these sources reports. Aliased from the legacy
    /// `sources` key so saved agents keep matching exactly as before.
    #[serde(default, alias = "sources")]
    pub any: Vec<EventSource>,
    /// Fires only when ALL of these sources have reported.
    #[serde(default)]
    pub all: Vec<EventSource>,
    /// Aggregation window for `all`, in seconds. None = the sources must be
    /// seen together with no window.
    #[serde(default)]
    pub within_secs: Option<u64>,
}

/// Aggregation state for one agent's `all` window (M2-1). Held in memory by
/// the trigger path, one entry per agent — never persisted.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowState {
    /// When the window opened — the first source hit that started the clock.
    pub opened_at: i64,
    /// Indexes into `EventFilter::all` already seen inside the window.
    pub seen: Vec<usize>,
}

/// What one event does to an `all` window.
#[derive(Debug, Clone, PartialEq)]
pub enum WindowOutcome {
    /// Not a trigger yet; carry this state forward.
    Pending(Option<WindowState>),
    /// Every source matched inside the window — fire, and clear the window.
    Fire,
}

impl EventFilter {
    /// Fold one event into the `all` window and decide whether it completes.
    pub fn observe(
        &self,
        state: Option<&WindowState>,
        source_type: &str,
        source_id: &str,
        field: &str,
        now: i64,
    ) -> WindowOutcome {
        // With no window configured, "the same instant, the same event" is
        // the only reading: nothing may carry over between events.
        let live = match self.within_secs {
            None => None,
            Some(window) => match state {
                // An expired window is dead. Reviving it would let a late
                // hit complete an AND whose sources never actually overlapped.
                Some(s) if now - s.opened_at > window as i64 => None,
                carried => carried,
            },
        };

        let mut seen = live.map(|s| s.seen.clone()).unwrap_or_default();
        let opened_at = live.map(|s| s.opened_at).unwrap_or(now);

        for (index, spec) in self.all.iter().enumerate() {
            if spec.matches_event(source_type, source_id, field) && !seen.contains(&index) {
                seen.push(index);
            }
        }

        if !self.all.is_empty() && seen.len() == self.all.len() {
            return WindowOutcome::Fire;
        }

        // Nothing to carry forward: either there is no window at all, or this
        // event matched none of the sources (a window only exists once
        // something has hit it — an empty one would be pruned state that
        // looks like an open window).
        if self.within_secs.is_none() || seen.is_empty() {
            return WindowOutcome::Pending(None);
        }

        WindowOutcome::Pending(Some(WindowState { opened_at, seen }))
    }
}

impl EventSource {
    /// Does this configured source match one concrete event?
    ///
    /// Semantics are the pre-M2 trigger path's, case for case — including the
    /// wildcard's early return, which the tests pin deliberately.
    pub fn matches_event(&self, source_type: &str, source_id: &str, field: &str) -> bool {
        if self.source_type != source_type {
            return false;
        }
        // "all" is the wildcard the editor offers for "any source of this
        // type". It answers before the field is consulted: the old path
        // returned here too, so a wildcard that also names a field still
        // matches every field.
        if self.id == "all" {
            return true;
        }
        // An empty id is ambiguous, not a wildcard.
        if self.id.is_empty() || self.id != source_id {
            return false;
        }
        // A configured field must match exactly; no field matches any.
        match self.field.as_deref() {
            Some(f) if !f.is_empty() => f == field,
            _ => true,
        }
    }
}

impl AgentSchedule {
    /// Parse the stored `event_filter` string into a structured filter.
    ///
    /// The column keeps its `Option<String>` type on purpose: it is the
    /// on-disk contract with every agent already in `agents.redb`.
    /// `None` means "no filter / unparseable" and callers then fall back to
    /// the resource bindings, exactly as before.
    pub fn parsed_event_filter(&self) -> Option<EventFilter> {
        let value: serde_json::Value = serde_json::from_str(self.event_filter.as_deref()?).ok()?;

        // A non-empty sources shape (the legacy `sources` key, or the new
        // `any`/`all` ones) wins outright — the trigger path has always
        // consulted `event_type` only when no sources are configured.
        let has_sources = ["any", "all", "sources"].iter().any(|key| {
            value
                .get(*key)
                .and_then(|v| v.as_array())
                .is_some_and(|sources| !sources.is_empty())
        });
        if has_sources {
            return serde_json::from_value(value).ok();
        }

        // The pre-`sources` shape names its source in `event_type`/`device_id`
        // instead of a sources array. Translate it rather than let it fall
        // through: a filter that parses into "no sources" is an agent that
        // silently never fires.
        let (source_type, id) = match value.get("event_type").and_then(|v| v.as_str()) {
            Some("device.metric") => ("device", value.get("device_id")),
            Some("extension.output") => ("extension", value.get("extension_id")),
            _ => return None,
        };
        let id = id.and_then(|v| v.as_str())?;

        Some(EventFilter {
            any: vec![EventSource {
                source_type: source_type.to_string(),
                id: id.to_string(),
                field: None,
            }],
            all: Vec::new(),
            within_secs: None,
        })
    }
}

/// Schedule type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleType {
    /// Event-triggered execution
    Event,
    /// Cron-based schedule
    Cron,
    /// Fixed interval
    Interval,
    /// Manual-only task: never auto-scheduled by the scheduler; runs any
    /// number of times via manual invoke/execute (or delegation, e.g.
    /// chat's run_agent), and shows `AgentStatus::Completed` while idle
    /// after a successful run (Completed is a ready-state, not terminal —
    /// re-invoking always works). First-class form of what the frontend
    /// used to encode as the `interval_seconds: 0` hack.
    Manual,
}

/// Agent status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus {
    /// Agent is active and running
    Active,
    /// Paused by user
    Paused,
    /// Stopped
    Stopped,
    /// In error state
    Error,
    /// Executing
    Executing,
    /// A manual (`ScheduleType::Manual`) task finished its latest run. Not
    /// auto-scheduled; manual invoke always works and re-Completes.
    Completed,
}

/// Agent execution mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ExecutionMode {
    /// Focused mode — user-defined scope, single-pass analysis with bound resources
    #[default]
    #[serde(rename = "focused", alias = "chat")]
    Focused,
    /// Free mode — LLM freely explores with full tool access, multi-round reasoning
    #[serde(rename = "free", alias = "react")]
    Free,
    /// Structured mode (L0 operator, 2026-09) — single constrained inference:
    /// no intent/situation preamble, no tool loop; input data sources in,
    /// schema-validated fields out, published as `ai:{agent_id}:{field}`.
    /// Appended LAST: bincode encodes variants by index, so Focused=0 / Free=1
    /// rows in agents.redb keep decoding unchanged.
    #[serde(rename = "structured")]
    Structured,
}

/// One output field of a Structured-mode agent's schema.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperatorField {
    /// Field name (becomes the metric name in `ai:{agent_id}:{name}`)
    pub name: String,
    /// Value type; `Enum` restricts the model to the listed values
    pub field_type: OperatorFieldType,
    /// Unit shown by dashboards (e.g. "℃", "件")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Field semantics, injected into the prompt so the model knows what to emit
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Allowed value types for [`OperatorField`].
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "type", content = "values")]
pub enum OperatorFieldType {
    Number,
    Text,
    Boolean,
    Enum(Vec<String>),
}

/// Runtime tuning for a Structured-mode agent (L0). All costs are bounded:
/// debounce caps frequency, budget caps daily volume, the failure threshold
/// trips the circuit breaker (agent degrades: keeps last values, marked stale).
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperatorConfig {
    /// Inputs are merged: at most one inference per `debounce_secs` (default 30)
    #[serde(default = "default_operator_debounce")]
    pub debounce_secs: u32,
    /// Daily inference cap; None = uncapped (local models) — exceeded pauses
    /// the agent until the next day
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_calls_per_day: Option<u32>,
    /// Per-inference timeout (default 60s — L0 is a single call, not the
    /// 300s agent loop)
    #[serde(default = "default_operator_timeout")]
    pub timeout_secs: u32,
    /// Consecutive failures before the circuit breaker opens (default 3)
    #[serde(default = "default_operator_failure_threshold")]
    pub consecutive_failure_threshold: u8,
}

/// Explicit notification routing (2026-09-23). Replaces the old
/// keyword-sniffing on decision text: the user picks the channels and when
/// to notify; the executor routes accordingly. Channels are names from the
/// messages domain (`listMessageChannels`).
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NotifyOn {
    /// Notify only when a run FAILS (the ops heartbeat — silence is health).
    #[default]
    Failure,
    /// Notify after every run (watch-style: each verdict is worth reading).
    Always,
}

/// Per-agent notification routing.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AgentNotify {
    /// Channel names to deliver to. Empty = config present but nowhere to
    /// send (valid; effectively muted).
    #[serde(default)]
    pub channels: Vec<String>,
    #[serde(default)]
    pub on: NotifyOn,
}

/// How much history an agent carries into each run (2026-09-22 review —
/// the memory axis made explicit, docs/designs/002 §3.5).
#[derive(utoipa::ToSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryMode {
    /// Stateless tool:每次只看当前输入 + instructed standard. No journal
    /// narrative, no knowledge-file preload into the *prompt* — the agent
    /// behaves like a scanner. Journal still RECORDS (for humans + the
    /// detail page); corrections still apply (they encode the user's
    /// standards, not past events). Cheapest, most predictable.
    #[default]
    Tool,
    /// Assistant with history: 每次运行带上最近执行的结果叙事（journal
    /// timeline），能说"比上次更差"这类判断。排查/汇总/自由发挥必须有它。
    Assistant,
}

impl MemoryMode {
    /// What an agent runs in when it carries no explicit override: a scanner
    /// (structured) has nothing to learn from past runs, so carrying their
    /// narrative is pure cost; everything that reasons across turns does.
    pub fn derived_for(execution_mode: ExecutionMode) -> Self {
        match execution_mode {
            ExecutionMode::Structured => MemoryMode::Tool,
            ExecutionMode::Focused | ExecutionMode::Free => MemoryMode::Assistant,
        }
    }
}

impl AiAgent {
    /// Resolve the memory axis for this agent — an explicit choice wins,
    /// otherwise it follows the execution mode.
    pub fn effective_memory_mode(&self) -> MemoryMode {
        self.memory_mode
            .unwrap_or_else(|| MemoryMode::derived_for(self.execution_mode))
    }
}

fn default_operator_debounce() -> u32 {
    30
}
fn default_operator_timeout() -> u32 {
    60
}
fn default_operator_failure_threshold() -> u8 {
    3
}

/// Agent execution statistics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentStats {
    /// Total executions
    pub total_executions: u64,
    /// Successful executions
    pub successful_executions: u64,
    /// Failed executions
    pub failed_executions: u64,
    /// Average execution duration in milliseconds
    pub avg_duration_ms: u64,
    /// Last execution duration in milliseconds
    pub last_duration_ms: Option<u64>,
}

/// Agent memory: execution journal + knowledge file index.
///
/// Keeps it simple — journal stores recent execution outcomes,
/// knowledge_files tracks markdown files the agent creates via the memory tool.
/// Old fields (short_term, long_term, baselines, task_profile) are accepted
/// via `#[serde(default)]` for backward compat and silently ignored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMemory {
    /// Execution journal — recent execution records
    #[serde(default)]
    pub journal: ExecutionJournal,
    /// Knowledge files created by the agent via memory tool
    #[serde(default)]
    pub knowledge_files: Vec<KnowledgeFileRef>,
    /// Last memory update
    #[serde(default = "default_timestamp")]
    pub updated_at: i64,
}

/// A single execution record in the journal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecord {
    pub timestamp: i64,
    pub execution_id: String,
    /// Brief outcome description (≤300 chars)
    pub outcome: String,
    /// Actions taken (≤150 chars), e.g. "sent alert" / "no action"
    pub action_taken: String,
    pub success: bool,
    /// Why the run ended — `StopReason::label()` from the tool loop
    /// (e.g. "max-rounds", "stuck", "natural-completion"). Empty for legacy
    /// records or paths without a tool loop. Lets the agent learn stop reasons
    /// across executions.
    #[serde(default)]
    pub stop_reason: String,
}

/// Execution journal — FIFO ring buffer of recent execution records.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionJournal {
    pub records: Vec<ExecutionRecord>,
    #[serde(default = "default_journal_limit")]
    pub max_records: usize,
}

impl Default for ExecutionJournal {
    fn default() -> Self {
        Self {
            records: Vec::new(),
            max_records: default_journal_limit(),
        }
    }
}

/// Reference to a knowledge file created by the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeFileRef {
    /// File name (without path), e.g. "device-patterns"
    pub name: String,
    /// Brief description (≤100 chars), written by LLM at creation time
    pub description: String,
    pub created_at: i64,
    pub updated_at: i64,
}

fn default_timestamp() -> i64 {
    chrono::Utc::now().timestamp()
}

fn default_journal_limit() -> usize {
    // 20 (was 10): failed runs are the key learning signal, and a FIFO of 10
    // meant an old failure was evicted before a run that could have learned
    // from it. Bigger window = more history for the failure-prioritized
    // injection (context.rs).
    20
}

impl Default for AgentMemory {
    fn default() -> Self {
        Self {
            journal: ExecutionJournal::default(),
            knowledge_files: Vec::new(),
            updated_at: default_timestamp(),
        }
    }
}

/// User message sent to an agent between executions.
///
/// Users can send messages to agents during the gap between executions
/// to provide additional context, corrections, or updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessage {
    /// Unique message ID
    pub id: String,
    /// Timestamp when the message was sent
    pub timestamp: i64,
    /// The message content from the user
    pub content: String,
    /// Optional message type/tag for categorization
    pub message_type: Option<String>,
}

/// Agent execution record with full decision process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentExecutionRecord {
    /// Unique execution ID
    pub id: String,
    /// Agent ID
    pub agent_id: String,
    /// Execution timestamp
    pub timestamp: i64,
    /// Trigger type (schedule, event, manual)
    pub trigger_type: String,
    /// Execution status
    pub status: ExecutionStatus,
    /// AI decision process with reasoning steps
    pub decision_process: DecisionProcess,
    /// Execution result
    pub result: Option<ExecutionResult>,
    /// Duration in milliseconds
    pub duration_ms: u64,
    /// Error message if failed
    pub error: Option<String>,
}

/// Execution status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionStatus {
    /// Running
    Running,
    /// Completed successfully
    Completed,
    /// Failed
    Failed,
    /// Partially completed
    Partial,
}

/// AI decision process with full reasoning trace.
///
/// This provides transparency into how the AI made its decisions,
/// enabling verification and debugging of agent behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionProcess {
    /// Initial understanding of the situation
    pub situation_analysis: String,
    /// Data collected for decision making
    pub data_collected: Vec<DataCollected>,
    /// Step-by-step reasoning
    pub reasoning_steps: Vec<ReasoningStep>,
    /// Decisions made
    pub decisions: Vec<Decision>,
    /// Final conclusion
    pub conclusion: String,
    /// Confidence level (0-1), as the model reported it. `None` when it did
    /// not report one — a missing claim rather than an invented default, since
    /// 002 §4.2 gates behaviour on this number. `default` keeps the records
    /// written before the field existed readable.
    #[serde(default)]
    pub confidence: Option<f32>,
    /// Why the tool loop ended (StopReason label); threaded to the journal so
    /// the agent can learn from prior stop reasons. Empty when no loop ran.
    #[serde(default)]
    pub stop_reason: String,
}

/// Data collected for decision making.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataCollected {
    /// Data source
    pub source: String,
    /// Data type
    pub data_type: String,
    /// Collected values
    pub values: serde_json::Value,
    /// Timestamp
    pub timestamp: i64,
}

/// A single reasoning step in the decision process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningStep {
    /// Step number
    pub step_number: u32,
    /// Step description
    pub description: String,
    /// Step type (analysis, comparison, inference, etc.)
    pub step_type: String,
    /// Input data for this step
    pub input: Option<String>,
    /// Output of this step
    pub output: String,
    /// Confidence in this step (0-1). `None` where nothing actually produced
    /// one — the same rule as [`DecisionProcess::confidence`].
    #[serde(default)]
    pub confidence: Option<f32>,
}

/// A decision made during execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    /// Decision type
    pub decision_type: String,
    /// Decision description
    pub description: String,
    /// Chosen action
    pub action: String,
    /// Rationale
    pub rationale: String,
    /// Expected outcome
    pub expected_outcome: String,
}

/// Execution result with actions taken.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// Actions executed
    pub actions_executed: Vec<ActionExecuted>,
    /// Generated report (if any)
    pub report: Option<GeneratedReport>,
    /// Notifications sent
    pub notifications_sent: Vec<NotificationSent>,
    /// Summary of execution
    pub summary: String,
    /// Success rate (0-1)
    pub success_rate: f32,
}

/// An action that was executed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionExecuted {
    /// Action type
    pub action_type: String,
    /// Action description
    pub description: String,
    /// Target of the action
    pub target: String,
    /// Parameters
    pub parameters: serde_json::Value,
    /// Success status
    pub success: bool,
    /// Result or error
    pub result: Option<String>,
}

/// A generated report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedReport {
    /// Report type
    pub report_type: String,
    /// Report content (markdown)
    pub content: String,
    /// Data included
    pub data_summary: Vec<DataSummary>,
    /// Generated at timestamp
    pub generated_at: i64,
}

/// Summary of data included in report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSummary {
    /// Data source
    pub source: String,
    /// Metric name
    pub metric: String,
    /// Data points count
    pub count: usize,
    /// Statistical summary
    pub statistics: serde_json::Value,
}

/// A notification that was sent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationSent {
    /// Notification channel
    pub channel: String,
    /// Recipient
    pub recipient: String,
    /// Message
    pub message: String,
    /// Sent timestamp
    pub sent_at: i64,
    /// Success status
    pub success: bool,
}

/// Query filter for agents.
#[derive(Debug, Clone, Default)]
pub struct AgentFilter {
    /// Filter by status
    pub status: Option<AgentStatus>,
    /// Filter by schedule type
    pub schedule_type: Option<ScheduleType>,
    /// Filter by creation time range (start)
    pub start_time: Option<i64>,
    /// Filter by creation time range (end)
    pub end_time: Option<i64>,
    /// Maximum number of results
    pub limit: Option<usize>,
    /// Offset for pagination
    pub offset: Option<usize>,
}

/// Query filter for execution records.
#[derive(Debug, Clone, Default)]
pub struct ExecutionFilter {
    /// Filter by agent ID
    pub agent_id: Option<String>,
    /// Filter by execution status
    pub status: Option<ExecutionStatus>,
    /// Filter by start time
    pub start_time: Option<i64>,
    /// Filter by end time
    pub end_time: Option<i64>,
    /// Maximum number of results
    pub limit: Option<usize>,
    /// Offset for pagination
    pub offset: Option<usize>,
}

impl AgentStore {
    /// Open or create an agent store at the given path.
    pub fn open<P: AsRef<std::path::Path>>(path: P) -> Result<Arc<Self>, Error> {
        Self::from_db(Database::create(path)?)
    }

    /// An in-memory agent store.
    ///
    /// Genuinely in memory. It used to create a real redb file in the temp
    /// directory and never remove it, so every call — and the test suite makes
    /// thousands — left ~1.5 MB behind for good.
    pub fn memory() -> Result<Arc<Self>, Error> {
        Self::from_db(
            Database::builder()
                .create_with_backend(redb::backends::InMemoryBackend::new())
                .map_err(|e| Error::Storage(e.to_string()))?,
        )
    }

    fn from_db(db: Database) -> Result<Arc<Self>, Error> {
        // Rollback guard: refuse databases stamped by a newer build (see schema.rs).
        crate::schema::check_or_stamp(&db)
            .map_err(|e| Error::Storage(format!("schema version: {e}")))?;
        let write_txn = db.begin_write()?;

        // Create tables if they don't exist
        write_txn.open_table(AGENTS_TABLE)?;
        write_txn.open_table(AGENT_EXECUTIONS_TABLE)?;
        write_txn.open_table(AGENT_MEMORY_TABLE)?;
        write_txn.commit()?;

        Ok(Arc::new(Self { db: Arc::new(db) }))
    }

    /// Save an agent to the store.
    pub async fn save_agent(&self, agent: &AiAgent) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AGENTS_TABLE)?;

            let value =
                serde_json::to_vec(agent).map_err(|e| Error::Serialization(e.to_string()))?;

            table.insert(agent.id.as_str(), value.as_slice())?;

            // Also save memory separately for efficient updates
            let memory_value = serde_json::to_vec(&agent.memory)
                .map_err(|e| Error::Serialization(e.to_string()))?;
            let mut memory_table = write_txn.open_table(AGENT_MEMORY_TABLE)?;
            memory_table.insert(agent.id.as_str(), memory_value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Get an agent by ID.
    pub async fn get_agent(&self, id: &str) -> Result<Option<AiAgent>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(AGENTS_TABLE)?;

        match table.get(id)? {
            Some(bytes) => {
                let agent: AiAgent = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;

                Ok(Some(agent))
            }
            None => Ok(None),
        }
    }

    /// Query agents with filters.
    pub async fn query_agents(&self, filter: AgentFilter) -> Result<Vec<AiAgent>, Error> {
        // [fake-async fix] Full-table scan + deserialize + sort on every call
        // blocks the executor thread; push it onto the blocking pool. Only
        // the database handle is needed.
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || Self::query_agents_impl(&db, filter))
            .await
            .map_err(|e| Error::Storage(format!("query_agents join error: {}", e)))?
    }

    fn query_agents_impl(db: &Database, filter: AgentFilter) -> Result<Vec<AiAgent>, Error> {
        let read_txn = db.begin_read()?;
        let table = read_txn.open_table(AGENTS_TABLE)?;

        let mut agents = Vec::new();

        for item in table.iter()? {
            let (_id, bytes) = item?;
            let agent: AiAgent = match serde_json::from_slice(bytes.value()) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("Skipping corrupted agent record {}: {}", _id.value(), e);
                    continue;
                }
            };

            if Self::matches_agent_filter(&agent, &filter) {
                agents.push(agent);
            }
        }

        // Sort by updated_at descending
        agents.sort_by_key(|a| std::cmp::Reverse(a.updated_at));

        // Apply pagination
        if let Some(offset) = filter.offset {
            if offset < agents.len() {
                agents = agents.into_iter().skip(offset).collect();
            } else {
                agents.clear();
            }
        }

        if let Some(limit) = filter.limit {
            agents.truncate(limit);
        }

        Ok(agents)
    }

    /// Update agent status.
    pub async fn update_agent_status(
        &self,
        id: &str,
        status: AgentStatus,
        error_message: Option<String>,
    ) -> Result<(), Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(AGENTS_TABLE)?;

        let agent = match table.get(id)? {
            Some(bytes) => {
                let mut ag: AiAgent = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                ag.status = status;
                ag.error_message = error_message;
                ag.updated_at = chrono::Utc::now().timestamp();
                ag
            }
            None => return Ok(()), // Agent doesn't exist
        };
        drop(table);
        drop(read_txn);

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AGENTS_TABLE)?;

            let value =
                serde_json::to_vec(&agent).map_err(|e| Error::Serialization(e.to_string()))?;

            table.insert(id, value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Update agent consecutive failure count.
    pub async fn update_agent_consecutive_failures(
        &self,
        id: &str,
        consecutive_failures: u32,
    ) -> Result<(), Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(AGENTS_TABLE)?;

        let agent = match table.get(id)? {
            Some(bytes) => {
                let mut ag: AiAgent = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                ag.consecutive_failures = consecutive_failures;
                ag.updated_at = chrono::Utc::now().timestamp();
                ag
            }
            None => return Ok(()), // Agent doesn't exist
        };
        drop(table);
        drop(read_txn);

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AGENTS_TABLE)?;

            let value =
                serde_json::to_vec(&agent).map_err(|e| Error::Serialization(e.to_string()))?;

            table.insert(id, value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Update agent parsed intent after initial parsing.
    pub async fn update_agent_parsed_intent(
        &self,
        id: &str,
        parsed_intent: Option<ParsedIntent>,
    ) -> Result<(), Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(AGENTS_TABLE)?;

        let agent = match table.get(id)? {
            Some(bytes) => {
                let mut ag: AiAgent = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                ag.parsed_intent = parsed_intent;
                ag.updated_at = chrono::Utc::now().timestamp();
                ag
            }
            None => return Ok(()), // Agent doesn't exist
        };
        drop(table);
        drop(read_txn);

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AGENTS_TABLE)?;

            let value =
                serde_json::to_vec(&agent).map_err(|e| Error::Serialization(e.to_string()))?;

            table.insert(id, value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Update agent memory after execution.
    /// First reads the agent, then updates both tables in a single write transaction.
    pub async fn update_agent_memory(&self, id: &str, memory: AgentMemory) -> Result<(), Error> {
        // First, read the current agent data (before starting write transaction)
        let agent = {
            let read_txn = self.db.begin_read()?;
            let table = read_txn.open_table(AGENTS_TABLE)?;
            match table.get(id)? {
                Some(bytes) => Some(
                    serde_json::from_slice::<AiAgent>(bytes.value())
                        .map_err(|e| Error::Serialization(e.to_string()))?,
                ),
                None => None,
            }
        };

        // Now start write transaction and update both tables
        let write_txn = self.db.begin_write()?;

        // Update memory in dedicated table
        {
            let memory_value =
                serde_json::to_vec(&memory).map_err(|e| Error::Serialization(e.to_string()))?;
            let mut memory_table = write_txn.open_table(AGENT_MEMORY_TABLE)?;
            memory_table.insert(id, memory_value.as_slice())?;
        }

        // Also update the agent record
        if let Some(mut ag) = agent {
            ag.memory = memory;
            ag.updated_at = chrono::Utc::now().timestamp();

            let value = serde_json::to_vec(&ag).map_err(|e| Error::Serialization(e.to_string()))?;
            let mut agent_table = write_txn.open_table(AGENTS_TABLE)?;
            agent_table.insert(id, value.as_slice())?;
        }

        write_txn.commit()?;
        Ok(())
    }

    /// Update agent stats after execution.
    /// Reads latest memory from AGENT_MEMORY_TABLE and only writes to AGENTS_TABLE.
    /// This avoids overwriting memory data that was just written by update_agent_memory.
    pub async fn update_agent_stats(
        &self,
        id: &str,
        success: bool,
        duration_ms: u64,
    ) -> Result<(), Error> {
        let read_txn = self.db.begin_read()?;

        // Read agent and latest memory in the same transaction
        let table = read_txn.open_table(AGENTS_TABLE)?;
        let memory_table = read_txn.open_table(AGENT_MEMORY_TABLE)?;

        let agent = match table.get(id)? {
            Some(bytes) => {
                let mut ag: AiAgent = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;

                // Try to get latest memory from memory table
                let latest_mem = match memory_table.get(id)? {
                    Some(mem_bytes) => Some(
                        serde_json::from_slice::<AgentMemory>(mem_bytes.value())
                            .map_err(|e| Error::Serialization(e.to_string()))?,
                    ),
                    None => None,
                };

                // Use latest memory if available
                if let Some(ref mem) = latest_mem {
                    ag.memory = mem.clone();
                }

                // Update stats
                ag.stats.total_executions += 1;
                if success {
                    ag.stats.successful_executions += 1;
                } else {
                    ag.stats.failed_executions += 1;
                }
                let total = ag.stats.total_executions as u64;
                ag.stats.avg_duration_ms =
                    (ag.stats.avg_duration_ms * (total - 1) + duration_ms) / total;
                ag.stats.last_duration_ms = Some(duration_ms);
                ag.last_execution_at = Some(chrono::Utc::now().timestamp());
                ag.updated_at = chrono::Utc::now().timestamp();

                ag
            }
            None => return Ok(()),
        };
        drop(table);
        drop(memory_table);
        drop(read_txn);

        // Only write to AGENTS_TABLE, not to AGENT_MEMORY_TABLE
        // (update_agent_memory handles writing to AGENT_MEMORY_TABLE)
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AGENTS_TABLE)?;

            let value =
                serde_json::to_vec(&agent).map_err(|e| Error::Serialization(e.to_string()))?;

            table.insert(id, value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Delete an agent by ID.
    pub async fn delete_agent(&self, id: &str) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AGENTS_TABLE)?;
            table.remove(id)?;
        }
        {
            let mut memory_table = write_txn.open_table(AGENT_MEMORY_TABLE)?;
            memory_table.remove(id)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Save an execution record.
    pub async fn save_execution(&self, execution: &AgentExecutionRecord) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AGENT_EXECUTIONS_TABLE)?;

            let value =
                serde_json::to_vec(execution).map_err(|e| Error::Serialization(e.to_string()))?;

            table.insert(execution.id.as_str(), value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Save execution record and optionally update agent's updated_at timestamp.
    /// Conversation history is no longer maintained — short-term memory is the single source.
    pub async fn save_execution_with_conversation(
        &self,
        execution: &AgentExecutionRecord,
        agent_id: Option<&str>,
        _conversation_turn: Option<&serde_json::Value>,
    ) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;

        // Save execution record
        {
            let mut table = write_txn.open_table(AGENT_EXECUTIONS_TABLE)?;
            let value =
                serde_json::to_vec(execution).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(execution.id.as_str(), value.as_slice())?;
        }

        // Update agent's updated_at timestamp in the same transaction
        if let Some(agent_id) = agent_id {
            let mut agent = {
                let result = match write_txn.open_table(AGENTS_TABLE)?.get(agent_id)? {
                    Some(bytes) => {
                        let value = bytes.value().to_vec();
                        let a: AiAgent = serde_json::from_slice(&value)
                            .map_err(|e| Error::Serialization(e.to_string()))?;
                        Ok(a)
                    }
                    None => Err(Error::NotFound(format!("Agent {} not found", agent_id))),
                };
                result?
            };

            agent.updated_at = chrono::Utc::now().timestamp();

            {
                let mut table = write_txn.open_table(AGENTS_TABLE)?;
                let value =
                    serde_json::to_vec(&agent).map_err(|e| Error::Serialization(e.to_string()))?;
                table.insert(agent_id, value.as_slice())?;
            }
        }

        write_txn.commit()?;
        Ok(())
    }

    /// Get an execution record by ID.
    pub async fn get_execution(&self, id: &str) -> Result<Option<AgentExecutionRecord>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(AGENT_EXECUTIONS_TABLE)?;

        match table.get(id)? {
            Some(bytes) => {
                let execution: AgentExecutionRecord = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                Ok(Some(execution))
            }
            None => Ok(None),
        }
    }

    /// Get the most recent execution record for an agent.
    pub async fn get_latest_execution(
        &self,
        agent_id: &str,
    ) -> Result<Option<AgentExecutionRecord>, Error> {
        let filter = ExecutionFilter {
            agent_id: Some(agent_id.to_string()),
            ..Default::default()
        };
        let mut executions = self.query_executions(filter).await?;
        // Sort by timestamp descending and return the first
        executions.sort_by_key(|e| std::cmp::Reverse(e.timestamp));
        Ok(executions.into_iter().next())
    }

    /// Query execution records with filters.
    pub async fn query_executions(
        &self,
        filter: ExecutionFilter,
    ) -> Result<Vec<AgentExecutionRecord>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(AGENT_EXECUTIONS_TABLE)?;

        let mut executions = Vec::new();

        for item in table.iter()? {
            let (_id, bytes) = item?;
            let execution: AgentExecutionRecord = match serde_json::from_slice(bytes.value()) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("Skipping corrupted execution record {}: {}", _id.value(), e);
                    continue;
                }
            };

            if self.matches_execution_filter(&execution, &filter) {
                executions.push(execution);
            }
        }

        // Sort by timestamp descending
        executions.sort_by_key(|e| std::cmp::Reverse(e.timestamp));

        // Apply pagination
        if let Some(offset) = filter.offset {
            if offset < executions.len() {
                executions = executions.into_iter().skip(offset).collect();
            } else {
                executions.clear();
            }
        }

        if let Some(limit) = filter.limit {
            executions.truncate(limit);
        }

        Ok(executions)
    }

    /// Get recent executions for an agent.
    pub async fn get_agent_executions(
        &self,
        agent_id: &str,
        limit: usize,
    ) -> Result<Vec<AgentExecutionRecord>, Error> {
        self.query_executions(ExecutionFilter {
            agent_id: Some(agent_id.to_string()),
            limit: Some(limit),
            ..Default::default()
        })
        .await
    }

    /// Delete old execution records.
    pub async fn cleanup_executions(&self, older_than: i64) -> Result<usize, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(AGENT_EXECUTIONS_TABLE)?;

        let mut to_remove: Vec<String> = Vec::new();
        for item in table.iter()? {
            let (id, bytes) = item?;
            let execution: AgentExecutionRecord = match serde_json::from_slice(bytes.value()) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("Skipping corrupted execution record {}: {}", id.value(), e);
                    // Remove corrupted records during cleanup
                    to_remove.push(id.value().to_string());
                    continue;
                }
            };

            if execution.timestamp < older_than {
                to_remove.push(id.value().to_string());
            }
        }
        drop(table);
        drop(read_txn);

        if to_remove.is_empty() {
            return Ok(0);
        }

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AGENT_EXECUTIONS_TABLE)?;
            for key in &to_remove {
                table.remove(key.as_str())?;
            }
        }
        write_txn.commit()?;

        Ok(to_remove.len())
    }

    /// Check if an agent matches the given filter.
    fn matches_agent_filter(agent: &AiAgent, filter: &AgentFilter) -> bool {
        if let Some(status) = filter.status {
            if agent.status != status {
                return false;
            }
        }

        if let Some(schedule_type) = &filter.schedule_type {
            if agent.schedule.schedule_type != *schedule_type {
                return false;
            }
        }

        if let Some(start_time) = filter.start_time {
            if agent.created_at < start_time {
                return false;
            }
        }

        if let Some(end_time) = filter.end_time {
            if agent.created_at > end_time {
                return false;
            }
        }

        true
    }

    /// Check if an execution matches the given filter.
    fn matches_execution_filter(
        &self,
        execution: &AgentExecutionRecord,
        filter: &ExecutionFilter,
    ) -> bool {
        if let Some(agent_id) = &filter.agent_id {
            if &execution.agent_id != agent_id {
                return false;
            }
        }

        if let Some(status) = filter.status {
            if execution.status != status {
                return false;
            }
        }

        if let Some(start_time) = filter.start_time {
            if execution.timestamp < start_time {
                return false;
            }
        }

        if let Some(end_time) = filter.end_time {
            if execution.timestamp > end_time {
                return false;
            }
        }

        true
    }

    // ========== User Message Methods ==========

    /// Maximum number of user messages to keep.
    const MAX_USER_MESSAGES: usize = 50;

    /// Add a user message to an agent.
    pub async fn add_user_message(
        &self,
        agent_id: &str,
        content: String,
        message_type: Option<String>,
    ) -> Result<UserMessage, Error> {
        let mut agent = self
            .get_agent(agent_id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("Agent {} not found", agent_id)))?;

        let message = UserMessage {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            content,
            message_type,
        };

        agent.user_messages.push(message.clone());

        // Trim old messages if needed
        if agent.user_messages.len() > Self::MAX_USER_MESSAGES {
            let removed_count = agent.user_messages.len() - Self::MAX_USER_MESSAGES;
            agent.user_messages = agent.user_messages.split_off(Self::MAX_USER_MESSAGES);

            tracing::debug!(
                agent_id = %agent_id,
                removed_count = removed_count,
                remaining_count = agent.user_messages.len(),
                "Trimmed user messages to prevent unbounded growth"
            );
        }

        agent.updated_at = chrono::Utc::now().timestamp();

        // Save the updated agent
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AGENTS_TABLE)?;
            let value =
                serde_json::to_vec(&agent).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(agent_id, value.as_slice())?;
        }
        write_txn.commit()?;

        Ok(message)
    }

    /// Get user messages for an agent.
    pub async fn get_user_messages(
        &self,
        agent_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<UserMessage>, Error> {
        let agent = self.get_agent(agent_id).await?;
        let agent =
            agent.ok_or_else(|| Error::NotFound(format!("Agent {} not found", agent_id)))?;

        let messages = &agent.user_messages;
        if let Some(limit) = limit {
            if messages.len() > limit {
                Ok(messages[messages.len() - limit..].to_vec())
            } else {
                Ok(messages.clone())
            }
        } else {
            Ok(messages.clone())
        }
    }

    /// Delete a specific user message.
    pub async fn delete_user_message(
        &self,
        agent_id: &str,
        message_id: &str,
    ) -> Result<bool, Error> {
        let mut agent = self
            .get_agent(agent_id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("Agent {} not found", agent_id)))?;

        let original_len = agent.user_messages.len();
        agent.user_messages.retain(|m| m.id != message_id);

        if agent.user_messages.len() < original_len {
            agent.updated_at = chrono::Utc::now().timestamp();

            // Save the updated agent
            let write_txn = self.db.begin_write()?;
            {
                let mut table = write_txn.open_table(AGENTS_TABLE)?;
                let value =
                    serde_json::to_vec(&agent).map_err(|e| Error::Serialization(e.to_string()))?;
                table.insert(agent_id, value.as_slice())?;
            }
            write_txn.commit()?;

            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Clear all user messages for an agent.
    pub async fn clear_user_messages(&self, agent_id: &str) -> Result<usize, Error> {
        let mut agent = self
            .get_agent(agent_id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("Agent {} not found", agent_id)))?;

        let count = agent.user_messages.len();
        agent.user_messages.clear();
        agent.updated_at = chrono::Utc::now().timestamp();

        // Save the updated agent
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AGENTS_TABLE)?;
            let value =
                serde_json::to_vec(&agent).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(agent_id, value.as_slice())?;
        }
        write_txn.commit()?;

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> Arc<AgentStore> {
        AgentStore::memory().unwrap()
    }

    /// Minimal agent for the memory-mode derivation tests.
    fn memory_mode_fixture(
        execution_mode: ExecutionMode,
        memory_mode: Option<MemoryMode>,
    ) -> AiAgent {
        AiAgent {
            id: "agent-m".to_string(),
            name: "fixture".to_string(),
            description: None,
            user_prompt: "p".to_string(),
            llm_backend_id: None,
            parsed_intent: None,
            resources: vec![],
            schedule: AgentSchedule {
                schedule_type: ScheduleType::Interval,
                cron_expression: None,
                interval_seconds: Some(300),
                event_filter: None,
                timezone: None,
            },
            status: AgentStatus::Active,
            priority: 128,
            created_at: 0,
            updated_at: 0,
            last_execution_at: None,
            stats: AgentStats::default(),
            memory: AgentMemory::default(),
            conversation_history: vec![],
            user_messages: vec![],
            conversation_summary: None,
            context_window_size: 10,
            enable_tool_chaining: false,
            max_chain_depth: 3,
            tool_config: None,
            execution_mode,
            error_message: None,
            system_prompt: None,
            max_retries: 0,
            consecutive_failures: 0,
            output_schema: None,
            operator_config: None,
            memory_mode,
        notify: None,
        }
    }

    /// A scanner (structured) has nothing to learn from past runs, so it must
    /// not pay to carry their narrative into the prompt.
    #[test]
    fn effective_memory_mode_derives_tool_for_structured_agents() {
        let agent = memory_mode_fixture(ExecutionMode::Structured, None);
        assert_eq!(agent.effective_memory_mode(), MemoryMode::Tool);
    }

    /// Everything that reasons across turns defaults to carrying history.
    #[test]
    fn effective_memory_mode_derives_assistant_for_the_rest() {
        for mode in [ExecutionMode::Focused, ExecutionMode::Free] {
            let agent = memory_mode_fixture(mode, None);
            assert_eq!(
                agent.effective_memory_mode(),
                MemoryMode::Assistant,
                "mode {mode:?} should carry history by default"
            );
        }
    }

    /// The derived default is only a default — an explicit choice wins, in
    /// both directions.
    #[test]
    fn effective_memory_mode_honours_an_explicit_override() {
        let pinned_stateless =
            memory_mode_fixture(ExecutionMode::Free, Some(MemoryMode::Tool));
        assert_eq!(pinned_stateless.effective_memory_mode(), MemoryMode::Tool);

        let pinned_history =
            memory_mode_fixture(ExecutionMode::Structured, Some(MemoryMode::Assistant));
        assert_eq!(pinned_history.effective_memory_mode(), MemoryMode::Assistant);
    }

    #[tokio::test]
    async fn test_save_and_get_agent() {
        let store = test_store();

        let agent = AiAgent {
            id: "agent-1".to_string(),
            name: "Temperature Monitor".to_string(),
            description: None,
            user_prompt: "Monitor warehouse temperatures and alert if above 30°C".to_string(),
            llm_backend_id: None,
            parsed_intent: None,
            resources: vec![],
            schedule: AgentSchedule {
                schedule_type: ScheduleType::Interval,
                cron_expression: None,
                interval_seconds: Some(300),
                event_filter: None,
                timezone: None,
            },
            status: AgentStatus::Active,
            priority: 128,
            created_at: chrono::Utc::now().timestamp(),
            updated_at: chrono::Utc::now().timestamp(),
            last_execution_at: None,
            stats: AgentStats::default(),
            memory: AgentMemory::default(),
            conversation_history: vec![],
            user_messages: vec![],
            conversation_summary: None,
            context_window_size: 10,
            enable_tool_chaining: false,
            max_chain_depth: 5,
            tool_config: None,
            execution_mode: ExecutionMode::Focused,
            error_message: None,
            system_prompt: None,
            max_retries: 0,
            consecutive_failures: 0,
            output_schema: None,
            operator_config: None,
            memory_mode: None,
            notify: None,
        };

        store.save_agent(&agent).await.unwrap();
        let retrieved = store.get_agent("agent-1").await.unwrap().unwrap();
        assert_eq!(retrieved.id, "agent-1");
        assert_eq!(retrieved.name, "Temperature Monitor");
    }

    #[tokio::test]
    async fn test_update_agent_status() {
        let store = test_store();

        let agent = AiAgent {
            id: "agent-1".to_string(),
            name: "Test Agent".to_string(),
            description: None,
            user_prompt: "Test".to_string(),
            llm_backend_id: None,
            parsed_intent: None,
            resources: vec![],
            schedule: AgentSchedule {
                schedule_type: ScheduleType::Interval,
                cron_expression: None,
                interval_seconds: Some(300),
                event_filter: None,
                timezone: None,
            },
            status: AgentStatus::Active,
            priority: 128,
            created_at: chrono::Utc::now().timestamp(),
            updated_at: chrono::Utc::now().timestamp(),
            last_execution_at: None,
            stats: AgentStats::default(),
            memory: AgentMemory::default(),
            conversation_history: vec![],
            user_messages: vec![],
            conversation_summary: None,
            context_window_size: 10,
            enable_tool_chaining: false,
            max_chain_depth: 5,
            tool_config: None,
            execution_mode: ExecutionMode::Focused,
            error_message: None,
            system_prompt: None,
            max_retries: 0,
            consecutive_failures: 0,
            output_schema: None,
            operator_config: None,
            memory_mode: None,
            notify: None,
        };

        store.save_agent(&agent).await.unwrap();
        store
            .update_agent_status("agent-1", AgentStatus::Paused, None)
            .await
            .unwrap();

        let retrieved = store.get_agent("agent-1").await.unwrap().unwrap();
        assert_eq!(retrieved.status, AgentStatus::Paused);
    }

    #[tokio::test]
    async fn test_save_and_get_execution() {
        let store = test_store();

        let execution = AgentExecutionRecord {
            id: "exec-1".to_string(),
            agent_id: "agent-1".to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            trigger_type: "schedule".to_string(),
            status: ExecutionStatus::Completed,
            decision_process: DecisionProcess {
                situation_analysis: "Temperature is normal".to_string(),
                data_collected: vec![],
                reasoning_steps: vec![],
                decisions: vec![],
                conclusion: "No action needed".to_string(),
                confidence: Some(0.95),
                stop_reason: String::new(),
            },
            result: None,
            duration_ms: 150,
            error: None,
        };

        store.save_execution(&execution).await.unwrap();
        let retrieved = store.get_execution("exec-1").await.unwrap().unwrap();
        assert_eq!(retrieved.id, "exec-1");
        assert_eq!(retrieved.agent_id, "agent-1");
    }

    #[tokio::test]
    async fn test_agent_memory_persistence() {
        let store = test_store();

        let mut agent = AiAgent {
            id: "agent-1".to_string(),
            name: "Learning Agent".to_string(),
            description: None,
            user_prompt: "Learn patterns".to_string(),
            llm_backend_id: None,
            parsed_intent: None,
            resources: vec![],
            schedule: AgentSchedule {
                schedule_type: ScheduleType::Interval,
                cron_expression: None,
                interval_seconds: Some(300),
                event_filter: None,
                timezone: None,
            },
            status: AgentStatus::Active,
            priority: 128,
            created_at: chrono::Utc::now().timestamp(),
            updated_at: chrono::Utc::now().timestamp(),
            last_execution_at: None,
            stats: AgentStats::default(),
            memory: AgentMemory::default(),
            conversation_history: vec![],
            user_messages: vec![],
            conversation_summary: None,
            context_window_size: 10,
            enable_tool_chaining: false,
            max_chain_depth: 5,
            tool_config: None,
            execution_mode: ExecutionMode::Focused,
            error_message: None,
            system_prompt: None,
            max_retries: 0,
            consecutive_failures: 0,
            output_schema: None,
            operator_config: None,
            memory_mode: None,
            notify: None,
        };

        // Save initial agent
        store.save_agent(&agent).await.unwrap();

        // Update memory
        agent.memory.journal.records.push(ExecutionRecord {
            timestamp: 1000,
            execution_id: "exec-1".into(),
            outcome: "Temperature normal".into(),
            action_taken: "no action".into(),
            success: true,
            stop_reason: String::new(),
        });

        store
            .update_agent_memory("agent-1", agent.memory.clone())
            .await
            .unwrap();

        // Retrieve and verify
        let retrieved = store.get_agent("agent-1").await.unwrap().unwrap();
        assert_eq!(retrieved.memory.journal.records.len(), 1);
        assert_eq!(
            retrieved.memory.journal.records[0].outcome,
            "Temperature normal"
        );
    }

    #[tokio::test]
    async fn test_stats_tracking() {
        let store = test_store();

        let agent = AiAgent {
            id: "agent-1".to_string(),
            name: "Stats Agent".to_string(),
            description: None,
            user_prompt: "Test stats".to_string(),
            llm_backend_id: None,
            parsed_intent: None,
            resources: vec![],
            schedule: AgentSchedule {
                schedule_type: ScheduleType::Interval,
                cron_expression: None,
                interval_seconds: Some(300),
                event_filter: None,
                timezone: None,
            },
            status: AgentStatus::Active,
            priority: 128,
            created_at: chrono::Utc::now().timestamp(),
            updated_at: chrono::Utc::now().timestamp(),
            last_execution_at: None,
            stats: AgentStats::default(),
            memory: AgentMemory::default(),
            conversation_history: vec![],
            user_messages: vec![],
            conversation_summary: None,
            context_window_size: 10,
            enable_tool_chaining: false,
            max_chain_depth: 5,
            tool_config: None,
            execution_mode: ExecutionMode::Focused,
            error_message: None,
            system_prompt: None,
            max_retries: 0,
            consecutive_failures: 0,
            output_schema: None,
            operator_config: None,
            memory_mode: None,
            notify: None,
        };

        store.save_agent(&agent).await.unwrap();

        // Record successful execution
        store
            .update_agent_stats("agent-1", true, 200)
            .await
            .unwrap();

        let retrieved = store.get_agent("agent-1").await.unwrap().unwrap();
        assert_eq!(retrieved.stats.total_executions, 1);
        assert_eq!(retrieved.stats.successful_executions, 1);
        assert_eq!(retrieved.stats.avg_duration_ms, 200);

        // Record failed execution
        store
            .update_agent_stats("agent-1", false, 100)
            .await
            .unwrap();

        let retrieved = store.get_agent("agent-1").await.unwrap().unwrap();
        assert_eq!(retrieved.stats.total_executions, 2);
        assert_eq!(retrieved.stats.failed_executions, 1);
        assert_eq!(retrieved.stats.avg_duration_ms, 150); // (200 + 100) / 2
    }
}

/// `EventFilter` structured (M2-1) — the polymorphic parse layer.
///
/// The stored `event_filter` column stays an `Option<String>`; this module is
/// what makes it polymorphic, so every shape already in `agents.redb` keeps
/// working. Change these tests only when the on-disk shape changes.
#[cfg(test)]
mod event_filter_tests {
    use super::*;

    fn schedule_with(filter: &str) -> AgentSchedule {
        AgentSchedule {
            schedule_type: ScheduleType::Event,
            cron_expression: None,
            interval_seconds: None,
            event_filter: Some(filter.to_string()),
            timezone: None,
        }
    }

    /// The shape the web editor has been writing since before this change
    /// (`AgentEditorFullScreen.tsx` builds `{sources:[{type,id,field}]}`).
    /// It must keep parsing to exactly the same source triples, or every
    /// event agent already saved silently stops firing.
    #[test]
    fn legacy_sources_shape_parses_to_any() {
        let s = schedule_with(
            r#"{"sources":[{"type":"device","id":"cam-01","field":"occupied"},{"type":"device","id":"gate","field":"reserved"}]}"#,
        );

        let f = s
            .parsed_event_filter()
            .expect("the legacy sources shape must parse");

        assert_eq!(f.any.len(), 2);
        assert_eq!(f.any[0].source_type, "device");
        assert_eq!(f.any[0].id, "cam-01");
        assert_eq!(f.any[0].field.as_deref(), Some("occupied"));
        assert_eq!(f.any[1].id, "gate");
        assert_eq!(f.any[1].field.as_deref(), Some("reserved"));
        assert!(f.all.is_empty(), "the legacy shape carries no AND semantics");
        assert_eq!(f.within_secs, None);
    }

    /// The pre-`sources` shape, still present in older rows. Today it matches
    /// through a dedicated branch in the trigger path; once matching runs off
    /// the parsed filter, a shape that "parses" into an empty filter turns
    /// the agent into one that silently never fires — the worst failure mode
    /// this feature has. So it must translate into an equivalent source.
    #[test]
    fn legacy_event_type_shape_parses_to_an_equivalent_source() {
        let f = schedule_with(r#"{"event_type":"device.metric","device_id":"sensor-01"}"#)
            .parsed_event_filter()
            .expect("the legacy event_type shape must parse");

        assert_eq!(f.any.len(), 1, "an empty filter would never fire");
        assert_eq!(f.any[0].source_type, "device");
        assert_eq!(f.any[0].id, "sensor-01");
        assert_eq!(
            f.any[0].field, None,
            "the legacy shape names no field, so it matches any field"
        );
    }

    #[test]
    fn new_any_and_all_shapes_parse_with_their_window() {
        let f = schedule_with(
            r#"{"any":[{"type":"device","id":"cam-01","field":"occupied"}],"all":[{"type":"device","id":"cam-01","field":"occupied"},{"type":"extension","id":"booking","field":"reserved"}],"within_secs":1200}"#,
        )
        .parsed_event_filter()
        .expect("the new shape must parse");

        assert_eq!(f.any.len(), 1);
        assert_eq!(f.any[0].field.as_deref(), Some("occupied"));
        assert_eq!(f.all.len(), 2);
        assert_eq!(f.all[1].source_type, "extension");
        assert_eq!(f.all[1].id, "booking");
        assert_eq!(f.within_secs, Some(1200));
    }

    /// An `all` entry without a field matches any field of that source.
    #[test]
    fn source_without_a_field_matches_any_field() {
        let f = schedule_with(r#"{"all":[{"type":"device","id":"cam-01"}]}"#)
            .parsed_event_filter()
            .expect("must parse");

        assert_eq!(f.all[0].field, None);
    }

    /// Nothing recognised must yield `None`, never an empty filter: `None`
    /// sends the caller down the resource-binding fallback (today's
    /// behaviour), whereas an empty filter would match nothing at all and
    /// turn the agent into one that silently never fires.
    #[test]
    fn unrecognised_filters_fall_back_instead_of_matching_nothing() {
        for raw in [
            r#"{"foo":1}"#,
            r#"{"event_type":"something.else","device_id":"x"}"#,
            r#"{"sources":[]}"#,
            r#"{"any":[],"all":[]}"#,
            "not json at all",
        ] {
            assert!(
                schedule_with(raw).parsed_event_filter().is_none(),
                "expected None (fall back to resources) for {raw}"
            );
        }
    }

    fn source(source_type: &str, id: &str, field: Option<&str>) -> EventSource {
        EventSource {
            source_type: source_type.to_string(),
            id: id.to_string(),
            field: field.map(str::to_string),
        }
    }

    /// The predicate every `any`/`all` decision runs through. Its semantics
    /// are read straight off the pre-M2 trigger path
    /// (`matches_data_source_filter`'s sources branch), so this test is the
    /// equivalence contract: a change here changes which agents fire.
    #[test]
    fn source_requires_the_exact_triple() {
        let cam = source("device", "cam-01", Some("occupied"));

        assert!(
            cam.matches_event("device", "cam-01", "occupied"),
            "the same triple must match"
        );
        assert!(
            !cam.matches_event("device", "cam-01", "temperature"),
            "a different field on the same source must not match"
        );
        assert!(
            !cam.matches_event("device", "cam-02", "occupied"),
            "a different source must not match"
        );
        assert!(
            !cam.matches_event("extension", "cam-01", "occupied"),
            "a different source type must not match"
        );
    }

    /// `id: "all"` is the wildcard the editor offers for "any source of this
    /// type".
    #[test]
    fn id_all_matches_every_source_of_its_type() {
        let any_device = source("device", "all", Some("occupied"));

        assert!(any_device.matches_event("device", "cam-99", "occupied"));
        assert!(
            !any_device.matches_event("extension", "cam-99", "occupied"),
            "the wildcard is scoped to its own source type"
        );
    }

    /// Pre-M2 behaviour, pinned deliberately rather than left as an
    /// accident: the old path returned on `id == "all"` *before* consulting
    /// the field, so a wildcard with a field set still matches every field.
    #[test]
    fn id_all_short_circuits_the_field_check() {
        let any_device = source("device", "all", Some("occupied"));

        assert!(
            any_device.matches_event("device", "cam-99", "temperature"),
            "id=all ignores the configured field (pre-M2 behaviour)"
        );
    }

    /// A source with no field matches any field reported by that source.
    #[test]
    fn a_source_without_a_field_matches_any_field() {
        let open = source("device", "cam-01", None);

        assert!(open.matches_event("device", "cam-01", "temperature"));
        assert!(open.matches_event("device", "cam-01", "occupied"));
    }

    /// An empty id is ambiguous — the old path skipped such entries rather
    /// than treating them as a wildcard.
    #[test]
    fn an_empty_id_never_matches() {
        let empty = source("device", "", None);

        assert!(!empty.matches_event("device", "cam-01", "temperature"));
    }

    // ---- the `all` window state machine (M2-1) ----

    fn filter_with_all(sources: &[(&str, &str, Option<&str>)], within_secs: Option<u64>) -> EventFilter {
        EventFilter {
            any: Vec::new(),
            all: sources.iter().map(|(t, i, f)| source(t, i, *f)).collect(),
            within_secs,
        }
    }

    /// Unwrap a Pending outcome, failing loudly if the filter fired.
    fn pending(outcome: WindowOutcome) -> Option<WindowState> {
        match outcome {
            WindowOutcome::Pending(state) => state,
            WindowOutcome::Fire => panic!("expected Pending, got Fire"),
        }
    }

    fn two_source_filter() -> EventFilter {
        filter_with_all(
            &[("device", "cam-01", Some("occupied")), ("extension", "booking", Some("reserved"))],
            Some(1200),
        )
    }

    /// The whole point of M2: "occupancy AND not-booked" must not fire on
    /// either source alone — that is exactly what today's OR-only filter
    /// does, and the reason this feature exists.
    #[test]
    fn all_fires_only_once_every_source_is_seen() {
        let f = two_source_filter();

        let first = pending(f.observe(None, "device", "cam-01", "occupied", 1_000));

        let second = f.observe(
            first.as_ref(),
            "extension",
            "booking",
            "reserved",
            1_010,
        );
        assert!(
            matches!(second, WindowOutcome::Fire),
            "both sources inside the window must fire"
        );
    }

    /// A source arriving after the window must NOT complete the AND — those
    /// two hits never actually happened together. It opens a fresh window.
    #[test]
    fn a_source_past_the_window_restarts_instead_of_firing() {
        let f = two_source_filter(); // within_secs = 1200
        let opened = pending(f.observe(None, "device", "cam-01", "occupied", 1_000));

        // 1300s later — outside the 1200s window.
        let outcome = f.observe(opened.as_ref(), "extension", "booking", "reserved", 2_300);

        let state = pending(outcome).expect("a late source opens a fresh window");
        assert_eq!(state.opened_at, 2_300, "the clock restarts from the late hit");
        assert_eq!(state.seen, vec![1], "only the late hit is in the new window");
    }

    /// An expired window is dropped rather than revived: otherwise it lingers
    /// for the life of the process and can be completed by an unrelated event
    /// hours later.
    #[test]
    fn an_expired_window_is_dropped_instead_of_revived() {
        let f = two_source_filter();
        let opened = pending(f.observe(None, "device", "cam-01", "occupied", 1_000));

        // Unrelated, and long past the window.
        let after = f.observe(opened.as_ref(), "device", "cam-01", "temperature", 9_999);

        assert_eq!(pending(after), None, "the expired window must be gone");
    }

    /// `within_secs: None` reads as "the same instant, the same event"
    /// (design 001 §5.2.2): nothing carries between events, so the AND must
    /// be satisfiable by one event alone or not at all.
    #[test]
    fn without_a_window_only_a_single_event_can_complete_the_all() {
        let two_separate = filter_with_all(
            &[("device", "cam-01", Some("occupied")), ("extension", "booking", Some("reserved"))],
            None,
        );
        let alone = two_separate.observe(None, "device", "cam-01", "occupied", 1_000);
        assert_eq!(
            pending(alone),
            None,
            "with no window, nothing carries over to the next event"
        );

        // One event that satisfies every entry (exact + wildcard) does fire.
        let one_event = filter_with_all(
            &[("device", "cam-01", Some("occupied")), ("device", "all", None)],
            None,
        );
        assert!(
            matches!(
                one_event.observe(None, "device", "cam-01", "occupied", 1_000),
                WindowOutcome::Fire
            ),
            "a single event satisfying every entry must fire"
        );
    }
}
