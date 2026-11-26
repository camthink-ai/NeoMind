//! One-way startup migration from residual legacy `agents` tables into the
//! five-table agent layout.
//!
//! Called exclusively during [`crate::agents::AgentStore`] initialization when
//! an upgrade opens a database still containing the legacy `agents` table. All
//! valid rows are migrated into `agent_specs` / `agent_runtime` /
//! `agent_memory` / `agent_user_messages` and the legacy `agents` table is
//! deleted in the same write transaction.

use redb::{Database, ReadableTable, TableDefinition};

use crate::agent_store::{
    ensure_agent_tables, AgentRuntimeRecord, AgentRuntimeStats, AgentSpecRecord, InputContract,
    NotifyPolicy, OutputContract, PacingPolicy, ToolPolicy, TriggerSpec, AGENT_EXECUTIONS_TABLE,
    AGENT_MEMORY_TABLE, AGENT_RUNTIME_TABLE, AGENT_SPECS_TABLE, AGENT_USER_MESSAGES_TABLE,
    LEGACY_AGENTS_TABLE,
};
use crate::agents::{
    AgentMemory, AgentNotify, AgentResource, AgentSchedule, AgentStats, AgentToolConfig, AiAgent,
    ExecutionMode, NotifyOn, ResourceType, ScheduleType, UserMessage,
};

// ── Fidelity mapping (pure; unit-tested) ────────────────────────────────────

/// Map legacy tool settings → [`ToolPolicy`].
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
        _ => ToolPolicy::Unrestricted,
    }
}

/// Map legacy [`AgentNotify`] → [`NotifyPolicy`].
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
            timezone: None,
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
) -> AgentRuntimeStats {
    AgentRuntimeStats {
        total_runs: stats.total_executions,
        successful_runs: stats.successful_executions,
        failed_runs: stats.failed_executions,
        partial_runs: 0,
        skipped_daily_limit: 0,
        skipped_permit_timeout: 0,
        consecutive_failures,
        runs_today: 0,
        quota_day: None,
        last_execution_at,
        last_execution_id: None,
        avg_duration_ms: stats.avg_duration_ms,
        last_duration_ms: stats.last_duration_ms,
    }
}

/// Convert a legacy [`AiAgent`] row into the five-table spec, runtime, user
/// messages, and memory slices.
pub fn map_legacy_agent_record(
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

fn table_exists(
    db: &Database,
    name: TableDefinition<&str, &[u8]>,
) -> std::result::Result<bool, String> {
    let read = db.begin_read().map_err(|e| e.to_string())?;
    Ok(read.open_table(name).is_ok())
}

/// Ensure the five agent tables exist, migrate all readable rows out of the
/// legacy `agents` table, and delete the legacy `agents` table in the same
/// write transaction.
///
/// Idempotent: a second call when `agents` is already gone is a no-op beyond
/// ensuring the five tables exist. Corrupt legacy rows are logged and skipped
/// so an unreadable legacy entry never blocks server boot.
pub fn migrate_legacy_agents_table(db: &Database) -> std::result::Result<(), String> {
    ensure_agent_tables(db).map_err(|e| e.to_string())?;

    if !table_exists(db, LEGACY_AGENTS_TABLE)? {
        return Ok(());
    }

    let legacy_rows: Vec<(String, Vec<u8>)> = {
        let read = db.begin_read().map_err(|e| e.to_string())?;
        let table = read
            .open_table(LEGACY_AGENTS_TABLE)
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        let iter = table.iter().map_err(|e| e.to_string())?;
        for entry in iter {
            let (k, v) = entry.map_err(|e| e.to_string())?;
            out.push((k.value().to_string(), v.value().to_vec()));
        }
        out
    };

    let mut migrated = 0usize;
    let mut skipped = 0usize;
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

        for (id, bytes) in &legacy_rows {
            let agent: AiAgent = match serde_json::from_slice(bytes) {
                Ok(a) => a,
                Err(e) => {
                    tracing::warn!(
                        category = "storage",
                        agent_id = %id,
                        error = %e,
                        "skipping unreadable legacy agent row during auto-migration"
                    );
                    skipped += 1;
                    continue;
                }
            };
            let (spec, rt, messages, mem) = map_legacy_agent_record(&agent);

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
            migrated += 1;
        }
    }

    // Drop the legacy combined table inside the same write transaction.
    write
        .delete_table(LEGACY_AGENTS_TABLE)
        .map_err(|e| e.to_string())?;
    write.commit().map_err(|e| e.to_string())?;

    tracing::info!(
        category = "storage",
        migrated,
        skipped,
        "auto-migrated legacy agents table and deleted legacy table"
    );

    Ok(())
}
