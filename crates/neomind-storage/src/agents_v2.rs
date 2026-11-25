//! Schema-v2 agent tables and v1→v2 migration (M1 prep).
//!
//! M1 lands the five-table layout and the fidelity mapping helpers, but does
//! **not** bump [`crate::schema::CURRENT_SCHEMA_VERSION`] or wire this into
//! [`crate::agents::AgentStore::open`]. M2 activates startup migration.
//!
//! Storage must not depend on `neomind-ai-agent` — DTO shapes here mirror that
//! crate's contracts so rows stay JSON-compatible across the boundary.

use std::sync::Arc;

use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

use crate::agents::{
    AgentMemory, AgentNotify, AgentResource, AgentSchedule, AgentStats, AgentStatus,
    AgentToolConfig, AiAgent, ExecutionMode, MemoryMode, NotifyOn, OperatorConfig, OperatorField,
    ParsedIntent, ResourceType, ScheduleType, UserMessage,
};
use crate::error::{Error, Result};

// ── Table names (v2 layout) ─────────────────────────────────────────────────

const AGENTS_V1_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("agents");
const AGENT_SPECS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("agent_specs");
const AGENT_RUNTIME_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("agent_runtime");
const AGENT_EXECUTIONS_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("agent_executions");
const AGENT_MEMORY_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("agent_memory");
const AGENT_USER_MESSAGES_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("agent_user_messages");

// ── V2 DTO types (JSON-compatible with neomind-ai-agent) ────────────────────

/// Tool access policy for a managed agent.
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

/// Three-way notify policy (complete / alert / failure) + migration flag.
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: u32,
}

fn default_failure_threshold() -> u32 {
    3
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct InputContract {
    #[serde(default)]
    pub requirements: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OutputContract {
    #[serde(default)]
    pub fields: Vec<serde_json::Value>,
    #[serde(default)]
    pub publish: bool,
}

/// V1 API fields that are not yet first-class on the v2 spec.
///
/// Schema v2 activated while the REST `/api/agents` surface still reads/writes
/// an `AiAgent`. Without this bundle, `save_agent` → `get_agent` wiped
/// resources, output contracts, the parsed-intent cache, and pacing knobs —
/// breaking schedule-only updates, data-source listing, and prompt-edit
/// mission sync (CI residual after M3 residual #2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct V1CompatBundle {
    #[serde(default)]
    pub resources: Vec<AgentResource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parsed_intent: Option<ParsedIntent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Vec<OperatorField>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_config: Option<OperatorConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_mode: Option<MemoryMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens_per_run: Option<u64>,
    #[serde(default = "v1_compat_default_priority")]
    pub priority: u8,
    #[serde(default = "v1_compat_default_context_window")]
    pub context_window_size: usize,
    #[serde(default = "v1_compat_default_max_chain_depth")]
    pub max_chain_depth: usize,
    #[serde(default)]
    pub max_retries: u32,
}

fn v1_compat_default_priority() -> u8 {
    100
}

fn v1_compat_default_context_window() -> usize {
    10
}

fn v1_compat_default_max_chain_depth() -> usize {
    crate::agents::DEFAULT_MAX_CHAIN_DEPTH
}

/// Frozen static configuration written to `agent_specs`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSpecRecord {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub user_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_backend_id: Option<String>,
    pub trigger: TriggerSpec,
    #[serde(default)]
    pub pacing: PacingPolicy,
    #[serde(default)]
    pub input: InputContract,
    pub tool_policy: ToolPolicy,
    #[serde(default)]
    pub output: OutputContract,
    #[serde(default)]
    pub notify: NotifyPolicy,
    #[serde(default)]
    pub controlled_actions: Vec<serde_json::Value>,
    /// See [`V1CompatBundle`]. `#[serde(default)]` so rows written before this
    /// field still deserialize.
    #[serde(default)]
    pub v1_compat: V1CompatBundle,
    pub config_version: u64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AgentStatsV2 {
    pub total_runs: u64,
    pub successful_runs: u64,
    pub failed_runs: u64,
    #[serde(default)]
    pub partial_runs: u64,
    #[serde(default)]
    pub skipped_daily_limit: u64,
    #[serde(default)]
    pub skipped_permit_timeout: u64,
    #[serde(default)]
    pub consecutive_failures: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_execution_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_execution_id: Option<String>,
}

/// High-frequency runtime row written to `agent_runtime`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRuntimeRecord {
    pub agent_id: String,
    pub status: AgentStatus,
    pub stats: AgentStatsV2,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_probe_at: Option<i64>,
    pub updated_at: i64,
}

/// V2 execution outcome (distinct from legacy [`crate::agents::ExecutionStatus`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatusV2 {
    Completed,
    Partial,
    Skipped,
    Failed,
}

/// Delivery intent / receipt state for a side-effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DeliveryState {
    IntentPending,
    Rejected { reason: String },
    Accepted,
    Confirmed,
    OutcomeUnknown { reason: String },
}

/// Unique send-intent + receipt keyed by `(execution_id, destination, fingerprint)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionDeliveryRecord {
    pub execution_id: String,
    pub destination: String,
    pub action_fingerprint: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub idempotent: bool,
    pub state: DeliveryState,
}

/// Execution row written to `agent_executions` (v2 shape).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentExecutionRecordV2 {
    pub id: String,
    pub agent_id: String,
    pub config_version: u64,
    pub status: ExecutionStatusV2,
    pub started_at: i64,
    pub finished_at: i64,
    pub is_probe: bool,
    pub dry_run: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_config: Option<bool>,
    #[serde(default)]
    pub evidence: serde_json::Value,
    /// Deliveries stored on the execution row (unique claim via transactional RMW).
    #[serde(default)]
    pub deliveries: Vec<ActionDeliveryRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conclusion: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alert_verdict: Option<String>,
}

fn delivery_matches(rec: &ActionDeliveryRecord, destination: &str, fingerprint: &str) -> bool {
    rec.destination == destination && rec.action_fingerprint == fingerprint
}

// ── Fidelity mapping (pure; unit-tested) ────────────────────────────────────

/// Map v1 tool settings → v2 [`ToolPolicy`].
///
/// Priority: `Structured` always → `Disabled` (no privilege expansion). Else
/// `enabled == false` → `Disabled`; non-empty allowlist → `Scoped`; otherwise
/// `Unrestricted`. `ResourceType::Command` bindings become `allowed_commands`.
pub fn map_tool_policy(
    execution_mode: ExecutionMode,
    tool_config: &Option<AgentToolConfig>,
    resources: &[AgentResource],
) -> ToolPolicy {
    if matches!(execution_mode, ExecutionMode::Structured) {
        return ToolPolicy::Disabled;
    }

    let allowed_commands: Vec<String> = resources
        .iter()
        .filter(|r| matches!(r.resource_type, ResourceType::Command))
        .map(|r| r.resource_id.clone())
        .collect();

    match tool_config {
        Some(cfg) if !cfg.enabled => ToolPolicy::Disabled,
        Some(cfg) if !cfg.allowed_tools.is_empty() => ToolPolicy::Scoped {
            allowed_tools: cfg.allowed_tools.clone(),
            allowed_commands,
        },
        // enabled=true with empty allowlist, or tool_config absent → unrestricted
        _ => ToolPolicy::Unrestricted,
    }
}

/// Map v1 [`AgentNotify`] → v2 [`NotifyPolicy`].
///
/// - Channels are preserved verbatim (empty stays empty — never invent `IM`).
/// - `Always` → `on_complete`; `Failure` → `on_failure`.
/// - `Judgment` without a clear destination → `pending_confirmation`.
/// - Absent notify (legacy unrouted) keeps `on_failure` as the failure floor.
pub fn map_notify_policy(notify: &Option<AgentNotify>) -> NotifyPolicy {
    match notify {
        None => NotifyPolicy {
            channels: Vec::new(),
            on_failure: true,
            ..NotifyPolicy::default()
        },
        Some(n) => {
            let channels = n.channels.clone();
            let mut policy = NotifyPolicy {
                channels: channels.clone(),
                ..NotifyPolicy::default()
            };
            match n.on {
                NotifyOn::Always => {
                    policy.on_complete = true;
                }
                NotifyOn::Failure => {
                    policy.on_failure = true;
                }
                NotifyOn::Judgment => {
                    // No deterministic destination → park for operator confirm.
                    // Non-empty channels count as a clear dest; empty does not.
                    if channels.is_empty() {
                        policy.pending_confirmation = true;
                    }
                }
            }
            policy
        }
    }
}

fn map_trigger(schedule: &AgentSchedule) -> TriggerSpec {
    match schedule.schedule_type {
        ScheduleType::Cron => TriggerSpec::Cron {
            expr: schedule
                .cron_expression
                .clone()
                .unwrap_or_else(|| "0 * * * *".into()),
            timezone: schedule.timezone.clone().unwrap_or_else(|| "UTC".into()),
        },
        ScheduleType::Interval => TriggerSpec::Interval {
            every_secs: schedule.interval_seconds.unwrap_or(300),
        },
        ScheduleType::Event => {
            let filter = schedule
                .event_filter
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or(serde_json::Value::Null);
            TriggerSpec::Event { filter }
        }
        ScheduleType::Manual => TriggerSpec::Manual,
    }
}

fn map_pacing(agent: &AiAgent) -> PacingPolicy {
    match &agent.operator_config {
        Some(cfg) => PacingPolicy {
            burst_merge_secs: Some(cfg.debounce_secs),
            min_interval_secs: cfg.event_cooldown_secs,
            max_runs_per_day: cfg.max_calls_per_day,
            failure_threshold: u32::from(cfg.consecutive_failure_threshold),
        },
        None => PacingPolicy::default(),
    }
}

fn map_output(agent: &AiAgent) -> OutputContract {
    let fields: Vec<serde_json::Value> = agent
        .output_schema
        .as_ref()
        .map(|schema| {
            schema
                .iter()
                .map(|f| {
                    serde_json::json!({
                        "name": f.name,
                        "depends_on": [],
                        "description": f.description,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let publish = matches!(agent.execution_mode, ExecutionMode::Structured) && !fields.is_empty();
    OutputContract { fields, publish }
}

fn map_stats(
    stats: &AgentStats,
    consecutive_failures: u32,
    last_execution_at: Option<i64>,
) -> AgentStatsV2 {
    AgentStatsV2 {
        total_runs: stats.total_executions,
        successful_runs: stats.successful_executions,
        failed_runs: stats.failed_executions,
        partial_runs: 0,
        skipped_daily_limit: 0,
        skipped_permit_timeout: 0,
        consecutive_failures,
        last_execution_at,
        last_execution_id: None,
    }
}

/// Convert a v1 [`AiAgent`] row into the v2 spec + runtime + user-message slices.
pub fn map_agent_v1_to_v2(
    agent: &AiAgent,
) -> (
    AgentSpecRecord,
    AgentRuntimeRecord,
    Vec<UserMessage>,
    AgentMemory,
) {
    let spec = AgentSpecRecord {
        id: agent.id.clone(),
        name: agent.name.clone(),
        description: agent.description.clone(),
        user_prompt: agent.user_prompt.clone(),
        llm_backend_id: agent.llm_backend_id.clone(),
        trigger: map_trigger(&agent.schedule),
        pacing: map_pacing(agent),
        input: InputContract::default(),
        tool_policy: map_tool_policy(agent.execution_mode, &agent.tool_config, &agent.resources),
        output: map_output(agent),
        notify: map_notify_policy(&agent.notify),
        controlled_actions: Vec::new(),
        v1_compat: V1CompatBundle {
            resources: agent.resources.clone(),
            parsed_intent: agent.parsed_intent.clone(),
            output_schema: agent.output_schema.clone(),
            system_prompt: agent.system_prompt.clone(),
            operator_config: agent.operator_config.clone(),
            memory_mode: agent.memory_mode,
            max_tokens_per_run: agent.max_tokens_per_run,
            priority: agent.priority,
            context_window_size: agent.context_window_size,
            max_chain_depth: agent.max_chain_depth,
            max_retries: agent.max_retries,
        },
        config_version: 1,
        created_at: agent.created_at,
        updated_at: agent.updated_at,
    };
    let runtime = AgentRuntimeRecord {
        agent_id: agent.id.clone(),
        status: agent.status,
        stats: map_stats(
            &agent.stats,
            agent.consecutive_failures,
            agent.last_execution_at,
        ),
        error_message: agent.error_message.clone(),
        next_probe_at: None,
        updated_at: agent.updated_at,
    };
    (
        spec,
        runtime,
        agent.user_messages.clone(),
        agent.memory.clone(),
    )
}

/// Reconstruct a legacy [`AiAgent`] from v2 rows (list/get after migrate).
///
/// `execution_mode` / `tool_config` come from [`ToolPolicy`]. Resources,
/// output schema, parsed intent, and the other v1-only knobs come from
/// [`V1CompatBundle`] so the dual-read REST surface stays honest. Conversation
/// history is not restored (lives outside specs).
pub fn map_agent_v2_to_v1(
    spec: &AgentSpecRecord,
    runtime: Option<&AgentRuntimeRecord>,
    memory: Option<AgentMemory>,
    user_messages: Vec<UserMessage>,
) -> AiAgent {
    let (execution_mode, tool_config) = match &spec.tool_policy {
        ToolPolicy::Disabled => (
            ExecutionMode::Structured,
            Some(AgentToolConfig {
                enabled: false,
                allowed_tools: vec![],
            }),
        ),
        ToolPolicy::Scoped {
            allowed_tools,
            allowed_commands: _,
        } => (
            ExecutionMode::Focused,
            Some(AgentToolConfig {
                enabled: true,
                allowed_tools: allowed_tools.clone(),
            }),
        ),
        ToolPolicy::Unrestricted => (
            ExecutionMode::Free,
            Some(AgentToolConfig {
                enabled: true,
                allowed_tools: vec![],
            }),
        ),
    };
    let schedule = match &spec.trigger {
        TriggerSpec::Cron { expr, timezone } => AgentSchedule {
            schedule_type: ScheduleType::Cron,
            cron_expression: Some(expr.clone()),
            timezone: Some(timezone.clone()),
            interval_seconds: None,
            event_filter: None,
        },
        TriggerSpec::Interval { every_secs } => AgentSchedule {
            schedule_type: ScheduleType::Interval,
            cron_expression: None,
            timezone: None,
            interval_seconds: Some(*every_secs),
            event_filter: None,
        },
        TriggerSpec::Event { filter } => AgentSchedule {
            schedule_type: ScheduleType::Event,
            cron_expression: None,
            timezone: None,
            interval_seconds: None,
            event_filter: if filter.is_null() {
                None
            } else {
                Some(filter.to_string())
            },
        },
        TriggerSpec::Manual => AgentSchedule {
            schedule_type: ScheduleType::Manual,
            cron_expression: None,
            timezone: None,
            interval_seconds: None,
            event_filter: None,
        },
    };
    let notify = {
        let on = if spec.notify.on_alert {
            NotifyOn::Judgment
        } else if spec.notify.on_complete {
            NotifyOn::Always
        } else {
            NotifyOn::Failure
        };
        if spec.notify.channels.is_empty() && !spec.notify.on_complete && !spec.notify.on_alert {
            None
        } else {
            Some(AgentNotify {
                channels: spec.notify.channels.clone(),
                on,
            })
        }
    };
    let (status, error_message, consecutive_failures, last_execution_at, stats) =
        if let Some(rt) = runtime {
            (
                rt.status,
                rt.error_message.clone(),
                rt.stats.consecutive_failures,
                rt.stats.last_execution_at,
                AgentStats {
                    total_executions: rt.stats.total_runs,
                    successful_executions: rt.stats.successful_runs,
                    failed_executions: rt.stats.failed_runs,
                    avg_duration_ms: 0,
                    last_duration_ms: None,
                },
            )
        } else {
            (AgentStatus::Active, None, 0, None, AgentStats::default())
        };
    let compat = &spec.v1_compat;
    AiAgent {
        id: spec.id.clone(),
        name: spec.name.clone(),
        description: spec.description.clone(),
        user_prompt: spec.user_prompt.clone(),
        llm_backend_id: spec.llm_backend_id.clone(),
        parsed_intent: compat.parsed_intent.clone(),
        resources: compat.resources.clone(),
        schedule,
        status,
        priority: compat.priority,
        created_at: spec.created_at,
        updated_at: spec.updated_at,
        last_execution_at,
        stats,
        memory: memory.unwrap_or_default(),
        conversation_history: vec![],
        user_messages,
        conversation_summary: None,
        context_window_size: compat.context_window_size,
        enable_tool_chaining: false,
        max_chain_depth: compat.max_chain_depth,
        tool_config,
        execution_mode,
        error_message,
        system_prompt: compat.system_prompt.clone(),
        max_retries: compat.max_retries,
        consecutive_failures,
        output_schema: compat.output_schema.clone(),
        operator_config: compat.operator_config.clone(),
        memory_mode: compat.memory_mode,
        notify,
        max_tokens_per_run: compat.max_tokens_per_run,
    }
}

// ── Store (v2 tables only; does not migrate or delete legacy `agents`) ──────

/// Create the five v2 tables if missing.
///
/// Does **not** delete, rename, or touch a legacy `agents` table — even when
/// that table is present and empty. Safe to call from [`AgentStoreV2::open`].
pub fn ensure_v2_tables(db: &Database) -> Result<()> {
    let write = db.begin_write()?;
    write.open_table(AGENT_SPECS_TABLE)?;
    write.open_table(AGENT_RUNTIME_TABLE)?;
    write.open_table(AGENT_EXECUTIONS_TABLE)?;
    write.open_table(AGENT_MEMORY_TABLE)?;
    write.open_table(AGENT_USER_MESSAGES_TABLE)?;
    write.commit()?;
    Ok(())
}

/// Persistent agent store over the five v2 tables.
///
/// Opening ensures tables exist but never runs [`migrate_agents_v1_to_v2`] and
/// never bumps [`crate::schema::CURRENT_SCHEMA_VERSION`].
pub struct AgentStoreV2 {
    db: Arc<Database>,
}

impl AgentStoreV2 {
    /// Open or create a store at `path`, ensuring the five v2 tables exist.
    pub fn open<P: AsRef<std::path::Path>>(path: P) -> Result<Arc<Self>> {
        Self::from_db(Database::create(path)?)
    }

    /// In-memory store (tests).
    pub fn memory() -> Result<Arc<Self>> {
        Self::from_db(
            Database::builder()
                .create_with_backend(redb::backends::InMemoryBackend::new())
                .map_err(|e| Error::Storage(e.to_string()))?,
        )
    }

    /// Take ownership of a [`Database`] and ensure v2 tables.
    pub fn from_db(db: Database) -> Result<Arc<Self>> {
        Self::from_arc(Arc::new(db))
    }

    /// Share an existing [`Database`] (e.g. alongside legacy [`crate::agents::AgentStore`]).
    pub fn from_arc(db: Arc<Database>) -> Result<Arc<Self>> {
        ensure_v2_tables(&db)?;
        Ok(Arc::new(Self { db }))
    }

    pub fn db(&self) -> &Arc<Database> {
        &self.db
    }

    // ── Specs ───────────────────────────────────────────────────────────────

    pub fn save_spec(&self, spec: &AgentSpecRecord) -> Result<()> {
        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(AGENT_SPECS_TABLE)?;
            let bytes =
                serde_json::to_vec(spec).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(spec.id.as_str(), bytes.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    pub fn get_spec(&self, id: &str) -> Result<Option<AgentSpecRecord>> {
        let read = self.db.begin_read()?;
        let table = read.open_table(AGENT_SPECS_TABLE)?;
        match table.get(id)? {
            Some(bytes) => {
                let spec: AgentSpecRecord = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                Ok(Some(spec))
            }
            None => Ok(None),
        }
    }

    pub fn list_specs(&self) -> Result<Vec<AgentSpecRecord>> {
        let read = self.db.begin_read()?;
        let table = read.open_table(AGENT_SPECS_TABLE)?;
        let mut out = Vec::new();
        for entry in table.iter()? {
            let (_, bytes) = entry?;
            match serde_json::from_slice::<AgentSpecRecord>(bytes.value()) {
                Ok(spec) => out.push(spec),
                Err(e) => {
                    tracing::warn!("skip corrupt agent_specs row: {e}");
                }
            }
        }
        Ok(out)
    }

    pub fn delete_spec(&self, id: &str) -> Result<bool> {
        let write = self.db.begin_write()?;
        let removed = {
            let mut specs = write.open_table(AGENT_SPECS_TABLE)?;
            let existed = specs.get(id)?.is_some();
            if existed {
                specs.remove(id)?;
            }
            existed
        };
        if removed {
            let mut runtime = write.open_table(AGENT_RUNTIME_TABLE)?;
            let _ = runtime.remove(id)?;
        }
        write.commit()?;
        Ok(removed)
    }

    /// Drop every v2 row for `id` (spec, runtime, memory, user messages).
    pub fn delete_agent_rows(&self, id: &str) -> Result<bool> {
        let write = self.db.begin_write()?;
        let removed = {
            let mut specs = write.open_table(AGENT_SPECS_TABLE)?;
            let existed = specs.get(id)?.is_some();
            if existed {
                specs.remove(id)?;
            }
            existed
        };
        {
            let mut runtime = write.open_table(AGENT_RUNTIME_TABLE)?;
            let _ = runtime.remove(id)?;
        }
        {
            let mut memory = write.open_table(AGENT_MEMORY_TABLE)?;
            let _ = memory.remove(id)?;
        }
        {
            let mut messages = write.open_table(AGENT_USER_MESSAGES_TABLE)?;
            let _ = messages.remove(id)?;
        }
        write.commit()?;
        Ok(removed)
    }

    // ── Runtime ─────────────────────────────────────────────────────────────

    pub fn get_runtime(&self, agent_id: &str) -> Result<Option<AgentRuntimeRecord>> {
        let read = self.db.begin_read()?;
        let table = read.open_table(AGENT_RUNTIME_TABLE)?;
        match table.get(agent_id)? {
            Some(bytes) => {
                let rt: AgentRuntimeRecord = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                Ok(Some(rt))
            }
            None => Ok(None),
        }
    }

    pub fn set_runtime(&self, runtime: &AgentRuntimeRecord) -> Result<()> {
        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(AGENT_RUNTIME_TABLE)?;
            let bytes =
                serde_json::to_vec(runtime).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(runtime.agent_id.as_str(), bytes.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    /// Single-write-txn stats tick: increments counters without rewriting status.
    ///
    /// Does not revive a paused/stopped agent — only mutates `stats` (+ `updated_at`).
    /// Concurrent callers serialize on the redb write lock so both increments stick.
    pub fn apply_runtime_stats_tick(&self, agent_id: &str, success: bool, now: i64) -> Result<()> {
        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(AGENT_RUNTIME_TABLE)?;
            let specs = write.open_table(AGENT_SPECS_TABLE)?;
            let existing = table.get(agent_id)?.map(|g| g.value().to_vec());
            let mut rt = match existing {
                Some(bytes) => serde_json::from_slice::<AgentRuntimeRecord>(&bytes)
                    .map_err(|e| Error::Serialization(e.to_string()))?,
                None if specs.get(agent_id)?.is_some() => AgentRuntimeRecord {
                    agent_id: agent_id.to_string(),
                    status: AgentStatus::Active,
                    stats: AgentStatsV2::default(),
                    error_message: None,
                    next_probe_at: None,
                    updated_at: now,
                },
                None => {
                    // Agent gone — same silent no-op as the legacy RMW path.
                    return Ok(());
                }
            };
            rt.stats.total_runs = rt.stats.total_runs.saturating_add(1);
            if success {
                rt.stats.successful_runs = rt.stats.successful_runs.saturating_add(1);
                rt.stats.consecutive_failures = 0;
            } else {
                rt.stats.failed_runs = rt.stats.failed_runs.saturating_add(1);
            }
            rt.stats.last_execution_at = Some(now);
            rt.updated_at = now;
            let bytes = serde_json::to_vec(&rt).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(agent_id, bytes.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    /// Single-write-txn status update (does not invent a row after delete).
    pub fn apply_runtime_status(
        &self,
        agent_id: &str,
        status: AgentStatus,
        error_message: Option<String>,
        now: i64,
    ) -> Result<()> {
        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(AGENT_RUNTIME_TABLE)?;
            let specs = write.open_table(AGENT_SPECS_TABLE)?;
            let existing = table.get(agent_id)?.map(|g| g.value().to_vec());
            let mut rt = match existing {
                Some(bytes) => serde_json::from_slice::<AgentRuntimeRecord>(&bytes)
                    .map_err(|e| Error::Serialization(e.to_string()))?,
                None if specs.get(agent_id)?.is_some() => AgentRuntimeRecord {
                    agent_id: agent_id.to_string(),
                    status,
                    stats: AgentStatsV2::default(),
                    error_message: error_message.clone(),
                    next_probe_at: None,
                    updated_at: now,
                },
                None => return Ok(()),
            };
            rt.status = status;
            rt.error_message = error_message;
            rt.updated_at = now;
            let bytes = serde_json::to_vec(&rt).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(agent_id, bytes.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    // ── Executions ──────────────────────────────────────────────────────────

    pub fn save_execution(&self, execution: &AgentExecutionRecordV2) -> Result<()> {
        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(AGENT_EXECUTIONS_TABLE)?;
            let bytes =
                serde_json::to_vec(execution).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(execution.id.as_str(), bytes.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    pub fn get_execution(&self, id: &str) -> Result<Option<AgentExecutionRecordV2>> {
        let read = self.db.begin_read()?;
        let table = read.open_table(AGENT_EXECUTIONS_TABLE)?;
        match table.get(id)? {
            Some(bytes) => {
                let exec: AgentExecutionRecordV2 = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                Ok(Some(exec))
            }
            None => Ok(None),
        }
    }

    /// Formal (non-`dry_run`) executions for `agent_id`, newest `finished_at` first.
    pub fn list_formal_executions(&self, agent_id: &str) -> Result<Vec<AgentExecutionRecordV2>> {
        let read = self.db.begin_read()?;
        let table = read.open_table(AGENT_EXECUTIONS_TABLE)?;
        let mut out = Vec::new();
        for entry in table.iter()? {
            let (_, bytes) = entry?;
            let exec: AgentExecutionRecordV2 = match serde_json::from_slice(bytes.value()) {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("skip corrupt agent_executions row: {e}");
                    continue;
                }
            };
            if exec.agent_id == agent_id && !exec.dry_run {
                out.push(exec);
            }
        }
        out.sort_by(|a, b| b.finished_at.cmp(&a.finished_at));
        Ok(out)
    }

    /// Insert `IntentPending` uniquely on the execution row.
    ///
    /// Returns `Ok(true)` when this caller obtained send rights; `Ok(false)` if
    /// a delivery for the same `(execution_id, destination, fingerprint)` already
    /// exists (any state).
    pub fn try_claim_delivery(&self, record: ActionDeliveryRecord) -> Result<bool> {
        let write = self.db.begin_write()?;
        let claimed = {
            let mut table = write.open_table(AGENT_EXECUTIONS_TABLE)?;
            let existing_bytes = match table.get(record.execution_id.as_str())? {
                Some(guard) => guard.value().to_vec(),
                None => {
                    return Err(Error::NotFound(format!(
                        "execution {}",
                        record.execution_id
                    )));
                }
            };
            let mut exec: AgentExecutionRecordV2 = serde_json::from_slice(&existing_bytes)
                .map_err(|e| Error::Serialization(e.to_string()))?;
            if exec
                .deliveries
                .iter()
                .any(|d| delivery_matches(d, &record.destination, &record.action_fingerprint))
            {
                false
            } else {
                let exec_id = exec.id.clone();
                exec.deliveries.push(record);
                let bytes =
                    serde_json::to_vec(&exec).map_err(|e| Error::Serialization(e.to_string()))?;
                table.insert(exec_id.as_str(), bytes.as_slice())?;
                true
            }
        };
        write.commit()?;
        Ok(claimed)
    }

    pub fn update_delivery_state(
        &self,
        execution_id: &str,
        destination: &str,
        fingerprint: &str,
        state: DeliveryState,
    ) -> Result<()> {
        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(AGENT_EXECUTIONS_TABLE)?;
            let existing_bytes = match table.get(execution_id)? {
                Some(guard) => guard.value().to_vec(),
                None => {
                    return Err(Error::NotFound(format!("execution {execution_id}")));
                }
            };
            let mut exec: AgentExecutionRecordV2 = serde_json::from_slice(&existing_bytes)
                .map_err(|e| Error::Serialization(e.to_string()))?;
            let rec = exec
                .deliveries
                .iter_mut()
                .find(|d| delivery_matches(d, destination, fingerprint))
                .ok_or_else(|| {
                    Error::NotFound(format!(
                        "delivery {execution_id}/{destination}/{fingerprint}"
                    ))
                })?;
            rec.state = state;
            let bytes =
                serde_json::to_vec(&exec).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(execution_id, bytes.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    // ── Memory / user messages ──────────────────────────────────────────────

    pub fn save_memory(&self, agent_id: &str, memory: &AgentMemory) -> Result<()> {
        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(AGENT_MEMORY_TABLE)?;
            let bytes =
                serde_json::to_vec(memory).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(agent_id, bytes.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    pub fn get_memory(&self, agent_id: &str) -> Result<Option<AgentMemory>> {
        let read = self.db.begin_read()?;
        let table = read.open_table(AGENT_MEMORY_TABLE)?;
        match table.get(agent_id)? {
            Some(bytes) => {
                let mem: AgentMemory = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                Ok(Some(mem))
            }
            None => Ok(None),
        }
    }

    pub fn save_user_messages(&self, agent_id: &str, messages: &[UserMessage]) -> Result<()> {
        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(AGENT_USER_MESSAGES_TABLE)?;
            let bytes =
                serde_json::to_vec(messages).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(agent_id, bytes.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    pub fn get_user_messages(&self, agent_id: &str) -> Result<Option<Vec<UserMessage>>> {
        let read = self.db.begin_read()?;
        let table = read.open_table(AGENT_USER_MESSAGES_TABLE)?;
        match table.get(agent_id)? {
            Some(bytes) => {
                let msgs: Vec<UserMessage> = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                Ok(Some(msgs))
            }
            None => Ok(None),
        }
    }
}

// ── Migration ───────────────────────────────────────────────────────────────

fn table_exists(
    db: &Database,
    name: TableDefinition<&str, &[u8]>,
) -> std::result::Result<bool, String> {
    let read = db.begin_read().map_err(|e| e.to_string())?;
    Ok(read.open_table(name).is_ok())
}

/// Whether the legacy single-table `agents` layout is still present.
pub fn table_exists_agents_v1(db: &Database) -> std::result::Result<bool, String> {
    table_exists(db, AGENTS_V1_TABLE)
}

/// Copy v1 `agents` rows into the five v2 tables WITHOUT deleting the legacy table.
/// Safe while handlers still serve from `agents`. Idempotent upsert by id.
pub fn copy_agents_v1_to_v2_keep_legacy(db: &Database) -> std::result::Result<(), String> {
    ensure_v2_tables(db).map_err(|e| e.to_string())?;
    if !table_exists(db, AGENTS_V1_TABLE)? {
        return Ok(());
    }
    let v1_rows: Vec<(String, Vec<u8>)> = {
        let read = db.begin_read().map_err(|e| e.to_string())?;
        let table = read
            .open_table(AGENTS_V1_TABLE)
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for entry in table.iter().map_err(|e| e.to_string())? {
            let (k, v) = entry.map_err(|e| e.to_string())?;
            out.push((k.value().to_string(), v.value().to_vec()));
        }
        out
    };
    let write = db.begin_write().map_err(|e| e.to_string())?;
    {
        let mut specs = write
            .open_table(AGENT_SPECS_TABLE)
            .map_err(|e| e.to_string())?;
        let mut runtime = write
            .open_table(AGENT_RUNTIME_TABLE)
            .map_err(|e| e.to_string())?;
        let mut memory = write
            .open_table(AGENT_MEMORY_TABLE)
            .map_err(|e| e.to_string())?;
        let mut user_msgs = write
            .open_table(AGENT_USER_MESSAGES_TABLE)
            .map_err(|e| e.to_string())?;
        let _executions = write
            .open_table(AGENT_EXECUTIONS_TABLE)
            .map_err(|e| e.to_string())?;
        for (id, bytes) in &v1_rows {
            let agent: AiAgent = serde_json::from_slice(bytes)
                .map_err(|e| format!("deserialize agent {id}: {e}"))?;
            let (spec, rt, messages, mem) = map_agent_v1_to_v2(&agent);
            let spec_bytes =
                serde_json::to_vec(&spec).map_err(|e| format!("serialize spec {id}: {e}"))?;
            specs
                .insert(id.as_str(), spec_bytes.as_slice())
                .map_err(|e| e.to_string())?;
            let rt_bytes =
                serde_json::to_vec(&rt).map_err(|e| format!("serialize runtime {id}: {e}"))?;
            runtime
                .insert(id.as_str(), rt_bytes.as_slice())
                .map_err(|e| e.to_string())?;
            let mem_bytes =
                serde_json::to_vec(&mem).map_err(|e| format!("serialize memory {id}: {e}"))?;
            memory
                .insert(id.as_str(), mem_bytes.as_slice())
                .map_err(|e| e.to_string())?;
            let msg_bytes = serde_json::to_vec(&messages)
                .map_err(|e| format!("serialize user_messages {id}: {e}"))?;
            user_msgs
                .insert(id.as_str(), msg_bytes.as_slice())
                .map_err(|e| e.to_string())?;
        }
    }
    write.commit().map_err(|e| e.to_string())?;
    Ok(())
}

/// Create the five v2 tables and migrate rows out of the legacy `agents` table.
///
/// Idempotent: a second call when `agents` is already gone is a no-op beyond
/// ensuring the five tables exist. Does **not** stamp schema version 2.
///
/// **Do not call from [`crate::agents::AgentStore::from_db`] yet** — this deletes
/// the legacy `agents` table that handlers still read. Prefer
/// [`copy_agents_v1_to_v2_keep_legacy`] until handlers cut over.
pub fn migrate_agents_v1_to_v2(db: &Database) -> std::result::Result<(), String> {
    ensure_v2_tables(db).map_err(|e| e.to_string())?;

    if !table_exists(db, AGENTS_V1_TABLE)? {
        return Ok(());
    }

    // Snapshot v1 rows under a read transaction.
    let v1_rows: Vec<(String, Vec<u8>)> = {
        let read = db.begin_read().map_err(|e| e.to_string())?;
        let table = read
            .open_table(AGENTS_V1_TABLE)
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        let iter = table.iter().map_err(|e| e.to_string())?;
        for entry in iter {
            let (k, v) = entry.map_err(|e| e.to_string())?;
            out.push((k.value().to_string(), v.value().to_vec()));
        }
        out
    };

    let write = db.begin_write().map_err(|e| e.to_string())?;
    {
        let mut specs = write
            .open_table(AGENT_SPECS_TABLE)
            .map_err(|e| e.to_string())?;
        let mut runtime = write
            .open_table(AGENT_RUNTIME_TABLE)
            .map_err(|e| e.to_string())?;
        let mut memory = write
            .open_table(AGENT_MEMORY_TABLE)
            .map_err(|e| e.to_string())?;
        let mut user_msgs = write
            .open_table(AGENT_USER_MESSAGES_TABLE)
            .map_err(|e| e.to_string())?;
        // Touch executions so the table is definitely present in this txn.
        let _executions = write
            .open_table(AGENT_EXECUTIONS_TABLE)
            .map_err(|e| e.to_string())?;

        for (id, bytes) in &v1_rows {
            let agent: AiAgent = serde_json::from_slice(bytes)
                .map_err(|e| format!("deserialize agent {id}: {e}"))?;
            let (spec, rt, messages, mem) = map_agent_v1_to_v2(&agent);

            let spec_bytes =
                serde_json::to_vec(&spec).map_err(|e| format!("serialize spec {id}: {e}"))?;
            specs
                .insert(id.as_str(), spec_bytes.as_slice())
                .map_err(|e| e.to_string())?;

            let rt_bytes =
                serde_json::to_vec(&rt).map_err(|e| format!("serialize runtime {id}: {e}"))?;
            runtime
                .insert(id.as_str(), rt_bytes.as_slice())
                .map_err(|e| e.to_string())?;

            let mem_bytes =
                serde_json::to_vec(&mem).map_err(|e| format!("serialize memory {id}: {e}"))?;
            memory
                .insert(id.as_str(), mem_bytes.as_slice())
                .map_err(|e| e.to_string())?;

            let msg_bytes = serde_json::to_vec(&messages)
                .map_err(|e| format!("serialize user_messages {id}: {e}"))?;
            user_msgs
                .insert(id.as_str(), msg_bytes.as_slice())
                .map_err(|e| e.to_string())?;
        }
    }

    // Drop the legacy combined table inside the same write transaction.
    write
        .delete_table(AGENTS_V1_TABLE)
        .map_err(|e| e.to_string())?;
    write.commit().map_err(|e| e.to_string())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{AgentSchedule, AgentStats, AgentStatus, ExecutionMode, ScheduleType};

    fn bare_resource(resource_type: ResourceType, resource_id: &str) -> AgentResource {
        AgentResource {
            resource_type,
            resource_id: resource_id.into(),
            name: resource_id.into(),
            config: serde_json::Value::Null,
        }
    }

    fn fixture_agent() -> AiAgent {
        AiAgent {
            id: "a1".into(),
            name: "fixture".into(),
            description: None,
            user_prompt: "p".into(),
            llm_backend_id: None,
            parsed_intent: None,
            resources: vec![],
            schedule: AgentSchedule {
                schedule_type: ScheduleType::Manual,
                cron_expression: None,
                interval_seconds: None,
                event_filter: None,
                timezone: None,
            },
            status: AgentStatus::Active,
            priority: 128,
            created_at: 1,
            updated_at: 2,
            last_execution_at: None,
            stats: AgentStats::default(),
            memory: AgentMemory::default(),
            conversation_history: vec![],
            user_messages: vec![],
            conversation_summary: None,
            context_window_size: 10,
            enable_tool_chaining: false,
            max_chain_depth: 10,
            tool_config: None,
            execution_mode: ExecutionMode::Free,
            error_message: None,
            system_prompt: None,
            max_retries: 0,
            consecutive_failures: 0,
            output_schema: None,
            operator_config: None,
            memory_mode: None,
            notify: None,
            max_tokens_per_run: None,
        }
    }

    #[test]
    fn structured_maps_to_disabled_no_privilege_expansion() {
        // Even with a broad allowlist, Structured must not gain tools.
        let cfg = Some(AgentToolConfig {
            enabled: true,
            allowed_tools: vec!["shell".into(), "memory".into()],
        });
        let resources = vec![bare_resource(ResourceType::Command, "relay.on")];
        let policy = map_tool_policy(ExecutionMode::Structured, &cfg, &resources);
        assert_eq!(
            policy,
            ToolPolicy::Disabled,
            "Structured must stay Disabled (no privilege expansion)"
        );
    }

    #[test]
    fn tool_config_disabled_maps_to_disabled() {
        let cfg = Some(AgentToolConfig {
            enabled: false,
            allowed_tools: vec!["shell".into()],
        });
        assert_eq!(
            map_tool_policy(ExecutionMode::Free, &cfg, &[]),
            ToolPolicy::Disabled
        );
    }

    #[test]
    fn non_empty_allowlist_maps_to_scoped_with_commands() {
        let cfg = Some(AgentToolConfig {
            enabled: true,
            allowed_tools: vec!["shell".into()],
        });
        let resources = vec![
            bare_resource(ResourceType::Command, "cam.snapshot"),
            bare_resource(ResourceType::Device, "dev-1"),
        ];
        match map_tool_policy(ExecutionMode::Focused, &cfg, &resources) {
            ToolPolicy::Scoped {
                allowed_tools,
                allowed_commands,
            } => {
                assert_eq!(allowed_tools, vec!["shell".to_string()]);
                assert_eq!(allowed_commands, vec!["cam.snapshot".to_string()]);
            }
            other => panic!("expected Scoped, got {other:?}"),
        }
    }

    #[test]
    fn empty_or_absent_allowlist_maps_to_unrestricted() {
        assert_eq!(
            map_tool_policy(ExecutionMode::Free, &None, &[]),
            ToolPolicy::Unrestricted
        );
        let cfg = Some(AgentToolConfig {
            enabled: true,
            allowed_tools: vec![],
        });
        assert_eq!(
            map_tool_policy(ExecutionMode::Focused, &cfg, &[]),
            ToolPolicy::Unrestricted
        );
    }

    #[test]
    fn empty_channels_stay_silent() {
        let notify = Some(AgentNotify {
            channels: vec![],
            on: NotifyOn::Always,
        });
        let policy = map_notify_policy(&notify);
        assert!(
            policy.channels.is_empty(),
            "empty channels must stay empty (never invent IM)"
        );
        assert!(policy.on_complete);
        assert!(!policy.on_failure);
        assert!(!policy.pending_confirmation);
    }

    #[test]
    fn failure_notify_maps_on_failure_and_keeps_channels() {
        let notify = Some(AgentNotify {
            channels: vec!["ops".into()],
            on: NotifyOn::Failure,
        });
        let policy = map_notify_policy(&notify);
        assert_eq!(policy.channels, vec!["ops".to_string()]);
        assert!(policy.on_failure);
        assert!(!policy.on_complete);
        assert!(!policy.pending_confirmation);
    }

    #[test]
    fn unrouted_agent_keeps_failure_floor_without_inventing_im() {
        let policy = map_notify_policy(&None);
        assert!(policy.channels.is_empty());
        assert!(policy.on_failure);
        assert!(!policy.on_complete);
        assert!(!policy.pending_confirmation);
    }

    #[test]
    fn judgment_without_clear_dest_sets_pending_confirmation() {
        let notify = Some(AgentNotify {
            channels: vec![],
            on: NotifyOn::Judgment,
        });
        let policy = map_notify_policy(&notify);
        assert!(
            policy.pending_confirmation,
            "Judgment with no channels must park for operator confirmation"
        );
        assert!(
            policy.channels.is_empty(),
            "must not auto-route Judgment to IM"
        );
        assert!(!policy.on_complete);
        assert!(!policy.on_alert);
        assert!(!policy.on_failure);
    }

    #[test]
    fn judgment_with_channels_is_not_pending() {
        let notify = Some(AgentNotify {
            channels: vec!["ops".into()],
            on: NotifyOn::Judgment,
        });
        let policy = map_notify_policy(&notify);
        assert!(!policy.pending_confirmation);
        assert_eq!(policy.channels, vec!["ops".to_string()]);
    }

    #[test]
    fn migrate_creates_five_tables_and_is_idempotent() {
        let db = Database::builder()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .unwrap();

        // Seed a v1 agents row (mimic AgentStore layout).
        {
            let write = db.begin_write().unwrap();
            {
                let mut agents = write.open_table(AGENTS_V1_TABLE).unwrap();
                let mut execs = write.open_table(AGENT_EXECUTIONS_TABLE).unwrap();
                let mut mem = write.open_table(AGENT_MEMORY_TABLE).unwrap();
                let mut agent = fixture_agent();
                agent.execution_mode = ExecutionMode::Structured;
                agent.tool_config = Some(AgentToolConfig {
                    enabled: true,
                    allowed_tools: vec!["shell".into()],
                });
                agent.notify = Some(AgentNotify {
                    channels: vec![],
                    on: NotifyOn::Judgment,
                });
                agent.user_messages = vec![UserMessage {
                    id: "m1".into(),
                    timestamp: 10,
                    content: "hi".into(),
                    message_type: None,
                }];
                let bytes = serde_json::to_vec(&agent).unwrap();
                agents.insert(agent.id.as_str(), bytes.as_slice()).unwrap();
                // Touch legacy sibling tables so they exist like a real store.
                let _ = &mut execs;
                let _ = &mut mem;
            }
            write.commit().unwrap();
        }

        migrate_agents_v1_to_v2(&db).unwrap();

        {
            let read = db.begin_read().unwrap();
            assert!(read.open_table(AGENT_SPECS_TABLE).is_ok());
            assert!(read.open_table(AGENT_RUNTIME_TABLE).is_ok());
            assert!(read.open_table(AGENT_EXECUTIONS_TABLE).is_ok());
            assert!(read.open_table(AGENT_MEMORY_TABLE).is_ok());
            assert!(read.open_table(AGENT_USER_MESSAGES_TABLE).is_ok());
            assert!(
                read.open_table(AGENTS_V1_TABLE).is_err(),
                "legacy agents table must be removed"
            );

            let specs = read.open_table(AGENT_SPECS_TABLE).unwrap();
            let bytes = specs.get("a1").unwrap().unwrap().value().to_vec();
            let spec: AgentSpecRecord = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(spec.tool_policy, ToolPolicy::Disabled);
            assert!(spec.notify.pending_confirmation);
            assert!(spec.notify.channels.is_empty());

            let msgs = read.open_table(AGENT_USER_MESSAGES_TABLE).unwrap();
            let msg_bytes = msgs.get("a1").unwrap().unwrap().value().to_vec();
            let messages: Vec<UserMessage> = serde_json::from_slice(&msg_bytes).unwrap();
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0].content, "hi");
        }

        // Schema stamp is owned by schema.rs; migrate helper itself does not stamp.
        assert_eq!(crate::schema::CURRENT_SCHEMA_VERSION, 2);

        // Idempotent second pass.
        migrate_agents_v1_to_v2(&db).unwrap();
        let read = db.begin_read().unwrap();
        let specs = read.open_table(AGENT_SPECS_TABLE).unwrap();
        assert!(specs.get("a1").unwrap().is_some());
    }

    fn sample_spec(id: &str) -> AgentSpecRecord {
        AgentSpecRecord {
            id: id.into(),
            name: "roundtrip".into(),
            description: Some("spec round-trip".into()),
            user_prompt: "do the thing".into(),
            llm_backend_id: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract::default(),
            tool_policy: ToolPolicy::Disabled,
            output: OutputContract::default(),
            notify: NotifyPolicy::default(),
            controlled_actions: Vec::new(),
            v1_compat: V1CompatBundle::default(),
            config_version: 1,
            created_at: 100,
            updated_at: 200,
        }
    }

    #[test]
    fn ensure_v2_tables_preserves_empty_legacy_agents() {
        let db = Database::builder()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .unwrap();
        {
            let write = db.begin_write().unwrap();
            {
                let mut agents = write.open_table(AGENTS_V1_TABLE).unwrap();
                // Empty legacy table — still must survive ensure_v2_tables.
                let _ = &mut agents;
            }
            write.commit().unwrap();
        }

        ensure_v2_tables(&db).unwrap();

        let read = db.begin_read().unwrap();
        assert!(
            read.open_table(AGENTS_V1_TABLE).is_ok(),
            "ensure_v2_tables must not delete legacy agents"
        );
        assert!(read.open_table(AGENT_SPECS_TABLE).is_ok());
        assert!(read.open_table(AGENT_RUNTIME_TABLE).is_ok());
        assert!(read.open_table(AGENT_EXECUTIONS_TABLE).is_ok());
        assert!(read.open_table(AGENT_MEMORY_TABLE).is_ok());
        assert!(read.open_table(AGENT_USER_MESSAGES_TABLE).is_ok());
        assert_eq!(crate::schema::CURRENT_SCHEMA_VERSION, 2);
    }

    #[test]
    fn soft_copy_keeps_legacy_agents_table() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Database::create(tmp.path().join("soft.redb")).unwrap();
        {
            let write = db.begin_write().unwrap();
            {
                let mut t = write.open_table(AGENTS_V1_TABLE).unwrap();
                let agent = fixture_agent();
                let bytes = serde_json::to_vec(&agent).unwrap();
                t.insert(agent.id.as_str(), bytes.as_slice()).unwrap();
            }
            write.commit().unwrap();
        }
        copy_agents_v1_to_v2_keep_legacy(&db).unwrap();
        copy_agents_v1_to_v2_keep_legacy(&db).unwrap();
        assert!(table_exists(&db, AGENTS_V1_TABLE).unwrap());
        let store = AgentStoreV2::from_db(db).unwrap();
        assert!(store.get_spec("a1").unwrap().is_some());
    }

    #[test]
    fn agent_store_v2_spec_roundtrip() {
        let store = AgentStoreV2::memory().unwrap();
        let spec = sample_spec("spec-1");
        store.save_spec(&spec).unwrap();

        let got = store.get_spec("spec-1").unwrap().expect("spec present");
        assert_eq!(got, spec);
        assert_eq!(store.list_specs().unwrap().len(), 1);

        assert!(store.delete_spec("spec-1").unwrap());
        assert!(store.get_spec("spec-1").unwrap().is_none());
        assert!(store.list_specs().unwrap().is_empty());
        assert_eq!(crate::schema::CURRENT_SCHEMA_VERSION, 2);
    }

    #[test]
    fn agent_store_v2_runtime_and_execution_delivery() {
        let store = AgentStoreV2::memory().unwrap();
        store.save_spec(&sample_spec("a1")).unwrap();

        let runtime = AgentRuntimeRecord {
            agent_id: "a1".into(),
            status: AgentStatus::Active,
            stats: AgentStatsV2::default(),
            error_message: None,
            next_probe_at: None,
            updated_at: 50,
        };
        store.set_runtime(&runtime).unwrap();
        assert_eq!(store.get_runtime("a1").unwrap().unwrap(), runtime);

        let formal = AgentExecutionRecordV2 {
            id: "exec-formal".into(),
            agent_id: "a1".into(),
            config_version: 1,
            status: ExecutionStatusV2::Completed,
            started_at: 10,
            finished_at: 20,
            is_probe: false,
            dry_run: false,
            stale_config: None,
            evidence: serde_json::json!({}),
            deliveries: Vec::new(),
            brief_text: Some("ok".into()),
            conclusion: None,
            error: None,
            alert_verdict: None,
        };
        let dry = AgentExecutionRecordV2 {
            id: "exec-dry".into(),
            dry_run: true,
            finished_at: 30,
            ..formal.clone()
        };
        store.save_execution(&formal).unwrap();
        store.save_execution(&dry).unwrap();

        let formal_list = store.list_formal_executions("a1").unwrap();
        assert_eq!(formal_list.len(), 1);
        assert_eq!(formal_list[0].id, "exec-formal");

        let claim = ActionDeliveryRecord {
            execution_id: "exec-formal".into(),
            destination: "dev-1".into(),
            action_fingerprint: "fp".into(),
            depends_on: vec!["src".into()],
            idempotent: false,
            state: DeliveryState::IntentPending,
        };
        assert!(store.try_claim_delivery(claim.clone()).unwrap());
        assert!(!store.try_claim_delivery(claim).unwrap());

        store
            .update_delivery_state("exec-formal", "dev-1", "fp", DeliveryState::Accepted)
            .unwrap();
        let exec = store.get_execution("exec-formal").unwrap().unwrap();
        assert_eq!(exec.deliveries.len(), 1);
        assert_eq!(exec.deliveries[0].state, DeliveryState::Accepted);
    }
}
