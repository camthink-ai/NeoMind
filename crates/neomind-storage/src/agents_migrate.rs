//! Offline V1↔V2 agent row mapping and migration.
//!
//! Production Chat/REST/runtime paths must not call these maps. Intended
//! call sites: this module's migrate/copy helpers, [`crate::agents_purge`],
//! inventory docs, and `cfg(test)` dual-read fixtures on [`crate::AgentStore`].

use redb::{Database, ReadableTable, TableDefinition};

use crate::agents::{
    AgentMemory, AgentNotify, AgentResource, AgentSchedule, AgentStats, AgentStatus,
    AgentToolConfig, AiAgent, ExecutionMode, NotifyOn, ResourceType, ScheduleType, UserMessage,
};
use crate::agents_v2::{
    ensure_v2_tables, AgentRuntimeRecord, AgentSpecRecord, AgentStatsV2, InputContract,
    NotifyPolicy, OutputContract, PacingPolicy, ToolPolicy, TriggerSpec, V1CompatBundle,
    AGENTS_V1_TABLE, AGENT_EXECUTIONS_TABLE, AGENT_MEMORY_TABLE, AGENT_RUNTIME_TABLE,
    AGENT_SPECS_TABLE, AGENT_USER_MESSAGES_TABLE,
};

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
) -> AgentStatsV2 {
    AgentStatsV2 {
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
                    avg_duration_ms: rt.stats.avg_duration_ms,
                    last_duration_ms: rt.stats.last_duration_ms,
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

fn table_exists(
    db: &Database,
    name: TableDefinition<&str, &[u8]>,
) -> std::result::Result<bool, String> {
    let read = db.begin_read().map_err(|e| e.to_string())?;
    Ok(read.open_table(name).is_ok())
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
