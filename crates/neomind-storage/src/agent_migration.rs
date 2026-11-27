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
/// `Unrestricted`. V1 `ResourceType::Command` bindings are IoT device commands
/// migrated to `controlled_actions`, never POSIX shell `allowed_commands`.
pub fn map_tool_policy(
    execution_mode: ExecutionMode,
    tool_config: &Option<AgentToolConfig>,
    _resources: &[AgentResource],
) -> ToolPolicy {
    if matches!(execution_mode, ExecutionMode::Structured) {
        return ToolPolicy::Disabled;
    }

    match tool_config {
        Some(cfg) if !cfg.enabled => ToolPolicy::Disabled,
        Some(cfg) if !cfg.allowed_tools.is_empty() => ToolPolicy::Scoped {
            allowed_tools: cfg.allowed_tools.clone(),
            allowed_commands: Vec::new(),
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
                    if !channels.is_empty() {
                        policy.on_failure = true;
                    }
                }
                NotifyOn::Failure => {
                    policy.on_failure = true;
                }
                NotifyOn::Judgment => {
                    if channels.is_empty() {
                        policy.pending_confirmation = true;
                    } else {
                        policy.on_alert = true;
                    }
                }
            }
            policy
        }
    }
}

fn to_millis(ts: i64) -> i64 {
    if ts > 0 && ts < 1_000_000_000_000 {
        ts.saturating_mul(1000)
    } else {
        ts
    }
}

fn sanitize_input_key(raw: &str, used: &mut std::collections::HashSet<String>) -> String {
    let mut base: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    while base.contains("__") {
        base = base.replace("__", "_");
    }
    let base = base.trim_matches('_').to_string();
    let base = if base.is_empty() {
        "source".to_string()
    } else {
        base
    };
    if used.insert(base.clone()) {
        return base;
    }
    for i in 2..=999 {
        let candidate = format!("{base}_{i}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    base
}

fn strip_prefix_repeated<'a>(mut s: &'a str, prefix: &str) -> &'a str {
    while let Some(rest) = s.strip_prefix(prefix) {
        s = rest;
    }
    s
}

fn push_extension_metric_requirement(
    rid: &str,
    requirements: &mut Vec<serde_json::Value>,
    used_keys: &mut std::collections::HashSet<String>,
) {
    let stripped = strip_prefix_repeated(rid, "extension:").trim();
    if stripped.is_empty() {
        return;
    }
    let (ext_id, op) = match stripped.split_once(':') {
        Some((e, o)) if !e.is_empty() && !o.is_empty() => (e, Some(o)),
        Some((e, _)) if !e.is_empty() => (e, None),
        _ => (stripped, None),
    };
    let key = sanitize_input_key(
        &format!("ext_{}_{}", ext_id, op.unwrap_or("read")),
        used_keys,
    );
    requirements.push(serde_json::json!({
        "key": key,
        "source": { "kind": "extension_instance", "id": ext_id },
        "operation": op,
        "params": {},
        "auth_scope": format!("extension:{ext_id}:read"),
        "timeout_secs": 10,
        "result_type": "structured_json",
        "freshness": { "kind": "any" },
        "on_missing": "report_stale",
    }));
}

fn extension_event_source(rid: &str) -> Option<serde_json::Value> {
    let stripped = strip_prefix_repeated(rid, "extension:").trim();
    if stripped.is_empty() {
        return None;
    }
    let (e, f) = stripped.split_once(':').unwrap_or((stripped, ""));
    if e.is_empty() {
        return None;
    }
    if f.is_empty() || f == "*" {
        Some(serde_json::json!({ "type": "extension", "id": e }))
    } else {
        Some(serde_json::json!({ "type": "extension", "id": e, "field": f }))
    }
}

fn map_resources(resources: &[AgentResource]) -> (InputContract, Vec<serde_json::Value>) {
    let mut requirements = Vec::new();
    let mut controlled_actions = Vec::new();
    let mut used_keys = std::collections::HashSet::new();

    for res in resources {
        let rid = res.resource_id.trim();
        if rid.is_empty() {
            continue;
        }
        match res.resource_type {
            ResourceType::Device => {
                let dev_id = strip_prefix_repeated(rid, "device:").trim();
                if dev_id.is_empty() {
                    continue;
                }
                let key = sanitize_input_key(&format!("device_{dev_id}"), &mut used_keys);
                requirements.push(serde_json::json!({
                    "key": key,
                    "source": { "kind": "whole_device", "device_id": dev_id },
                    "operation": null,
                    "params": {},
                    "auth_scope": format!("device:{dev_id}:read"),
                    "timeout_secs": 10,
                    "result_type": "structured_json",
                    "freshness": { "kind": "any" },
                    "on_missing": "report_stale",
                }));
            }
            ResourceType::Metric | ResourceType::DataStream => {
                if rid.starts_with("ai:") {
                    let stripped = strip_prefix_repeated(rid, "ai:").trim();
                    if stripped.is_empty() {
                        continue;
                    }
                    let (agent_id, field) = match stripped.split_once(':') {
                        Some((a, f)) if !a.is_empty() && !f.is_empty() => (a, f),
                        Some((a, _)) if !a.is_empty() => (a, "*"),
                        _ => (stripped, "*"),
                    };
                    let key =
                        sanitize_input_key(&format!("ai_{agent_id}_{field}"), &mut used_keys);
                    requirements.push(serde_json::json!({
                        "key": key,
                        "source": {
                            "kind": "upstream_ai",
                            "agent_id": agent_id,
                            "field": field,
                        },
                        "operation": null,
                        "params": {},
                        "auth_scope": format!("ai:{agent_id}:read"),
                        "timeout_secs": 10,
                        "result_type": "structured_json",
                        "freshness": { "kind": "any" },
                        "on_missing": "report_stale",
                    }));
                } else if rid.starts_with("extension:") {
                    push_extension_metric_requirement(rid, &mut requirements, &mut used_keys);
                } else {
                    let stripped = strip_prefix_repeated(rid, "device:").trim();
                    if stripped.is_empty() {
                        continue;
                    }
                    let (device_id, field) = match stripped.split_once(':') {
                        Some((d, f)) if !d.is_empty() && !f.is_empty() => (d, f),
                        Some((d, _)) if !d.is_empty() => (d, "*"),
                        _ => (stripped, "*"),
                    };
                    let key = sanitize_input_key(&format!("{device_id}_{field}"), &mut used_keys);
                    let source = if field == "*" {
                        serde_json::json!({ "kind": "whole_device", "device_id": device_id })
                    } else {
                        serde_json::json!({
                            "kind": "device_metric",
                            "device_id": device_id,
                            "field": field,
                        })
                    };
                    requirements.push(serde_json::json!({
                        "key": key,
                        "source": source,
                        "operation": null,
                        "params": {},
                        "auth_scope": format!("device:{device_id}:read"),
                        "timeout_secs": 10,
                        "result_type": "structured_json",
                        "freshness": { "kind": "any" },
                        "on_missing": "report_stale",
                    }));
                }
            }
            ResourceType::ExtensionMetric => {
                push_extension_metric_requirement(rid, &mut requirements, &mut used_keys);
            }
            ResourceType::Command => {
                let stripped = strip_prefix_repeated(rid, "device:").trim();
                if stripped.is_empty() {
                    continue;
                }
                let (device_id, cmd_name) = match stripped.split_once(':') {
                    Some((d, c)) if !d.is_empty() && !c.is_empty() => (d, c),
                    Some((d, _)) if !d.is_empty() => (d, res.name.as_str()),
                    _ => (
                        stripped,
                        if res.name == res.resource_id {
                            "command"
                        } else {
                            res.name.as_str()
                        },
                    ),
                };
                controlled_actions.push(serde_json::json!({
                    "name": if cmd_name.is_empty() { "command" } else { cmd_name },
                    "destination": format!("device:{device_id}"),
                    "params": {},
                    "depends_on": [],
                    "idempotent": false,
                }));
            }
            ResourceType::ExtensionTool => {
                let stripped = strip_prefix_repeated(rid, "extension:").trim();
                if stripped.is_empty() {
                    continue;
                }
                let (ext_id, cmd_name) = match stripped.split_once(':') {
                    Some((e, c)) if !e.is_empty() && !c.is_empty() => (e, c),
                    Some((e, _)) if !e.is_empty() => (e, "invoke"),
                    _ => (stripped, "invoke"),
                };
                controlled_actions.push(serde_json::json!({
                    "name": cmd_name,
                    "destination": format!("extension:{ext_id}"),
                    "params": {},
                    "depends_on": [],
                    "idempotent": false,
                }));
            }
        }
    }

    (InputContract { requirements }, controlled_actions)
}

fn map_trigger(schedule: &AgentSchedule, resources: &[AgentResource]) -> TriggerSpec {
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
            let parsed = schedule.parsed_event_filter();
            let filter = if let Some(ef) = parsed {
                serde_json::to_value(ef).unwrap_or_else(|_| serde_json::json!({}))
            } else {
                // Fall back to deriving event sources from bound resources (matching v1.3.3).
                let sources: Vec<serde_json::Value> = resources
                    .iter()
                    .filter_map(|r| {
                        let rid = r.resource_id.trim();
                        if rid.is_empty() {
                            return None;
                        }
                        match r.resource_type {
                            ResourceType::Device => {
                                let dev_id = strip_prefix_repeated(rid, "device:").trim();
                                if dev_id.is_empty() {
                                    None
                                } else {
                                    Some(serde_json::json!({ "type": "device", "id": dev_id }))
                                }
                            }
                            ResourceType::Metric | ResourceType::DataStream => {
                                if rid.starts_with("ai:") {
                                    let stripped = strip_prefix_repeated(rid, "ai:").trim();
                                    let (a, f) = stripped.split_once(':').unwrap_or((stripped, ""));
                                    if a.is_empty() {
                                        None
                                    } else if f.is_empty() || f == "*" {
                                        Some(serde_json::json!({ "type": "ai", "id": a }))
                                    } else {
                                        Some(serde_json::json!({ "type": "ai", "id": a, "field": f }))
                                    }
                                } else if rid.starts_with("extension:") {
                                    extension_event_source(rid)
                                } else {
                                    let stripped = strip_prefix_repeated(rid, "device:").trim();
                                    let (d, f) = stripped.split_once(':').unwrap_or((stripped, ""));
                                    if d.is_empty() {
                                        None
                                    } else if f.is_empty() || f == "*" {
                                        Some(serde_json::json!({ "type": "device", "id": d }))
                                    } else {
                                        Some(serde_json::json!({ "type": "device", "id": d, "field": f }))
                                    }
                                }
                            }
                            ResourceType::ExtensionMetric => extension_event_source(rid),
                            _ => None,
                        }
                    })
                    .collect();
                if sources.is_empty() {
                    serde_json::json!({})
                } else {
                    serde_json::json!({ "any": sources })
                }
            };
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
        None if matches!(agent.schedule.schedule_type, ScheduleType::Event) => PacingPolicy {
            burst_merge_secs: Some(30),
            min_interval_secs: Some(60),
            max_runs_per_day: None,
            timezone: None,
            failure_threshold: 3,
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
    let (input, controlled_actions) = map_resources(&agent.resources);
    let created_at_ms = to_millis(agent.created_at);
    let updated_at_ms = to_millis(agent.updated_at);
    let last_exec_ms = agent.last_execution_at.map(to_millis);

    let spec = AgentSpecRecord {
        id: agent.id.clone(),
        name: agent.name.clone(),
        description: agent.description.clone(),
        user_prompt: agent.user_prompt.clone(),
        llm_backend_id: agent.llm_backend_id.clone(),
        preset: None,
        trigger: map_trigger(&agent.schedule, &agent.resources),
        pacing: map_pacing(agent),
        input,
        tool_policy: map_tool_policy(agent.execution_mode, &agent.tool_config, &agent.resources),
        output: map_output(agent),
        notify: map_notify_policy(&agent.notify),
        controlled_actions,
        config_version: 1,
        created_at: created_at_ms,
        updated_at: updated_at_ms,
    };
    let runtime = AgentRuntimeRecord {
        agent_id: agent.id.clone(),
        status: agent.status,
        stats: map_stats(&agent.stats, agent.consecutive_failures, last_exec_ms),
        error_message: agent.error_message.clone(),
        next_probe_at: None,
        updated_at: updated_at_ms,
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

            let preserve_existing_memory = memory
                .get(id.as_str())
                .map_err(|e| e.to_string())?
                .and_then(|g| serde_json::from_slice::<AgentMemory>(g.value()).ok())
                .is_some_and(|existing| existing.updated_at >= mem.updated_at);
            if !preserve_existing_memory {
                let mem_bytes =
                    serde_json::to_vec(&mem).map_err(|e| format!("serialize memory {id}: {e}"))?;
                memory
                    .insert(id.as_str(), mem_bytes.as_slice())
                    .map_err(|e| e.to_string())?;
            }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{AgentSchedule, AgentStats, AgentStatus, ExecutionRecord};

    fn res(resource_type: ResourceType, resource_id: &str, name: &str) -> AgentResource {
        AgentResource {
            resource_type,
            resource_id: resource_id.into(),
            name: name.into(),
            config: serde_json::Value::Null,
        }
    }

    fn base_agent(resources: Vec<AgentResource>) -> AiAgent {
        AiAgent {
            id: "agt-v133".into(),
            name: "v1.3.3 Agent".into(),
            description: None,
            user_prompt: "monitor".into(),
            llm_backend_id: None,
            parsed_intent: None,
            resources,
            schedule: AgentSchedule {
                schedule_type: ScheduleType::Event,
                cron_expression: None,
                interval_seconds: None,
                event_filter: None,
                timezone: None,
            },
            status: AgentStatus::Active,
            priority: 128,
            created_at: 1_700_000_000,
            updated_at: 1_700_000_100,
            last_execution_at: None,
            stats: AgentStats::default(),
            memory: AgentMemory::default(),
            conversation_history: vec![],
            user_messages: vec![],
            conversation_summary: None,
            context_window_size: 10,
            enable_tool_chaining: false,
            max_chain_depth: 10,
            tool_config: Some(AgentToolConfig {
                enabled: true,
                allowed_tools: vec!["device".into()],
            }),
            execution_mode: ExecutionMode::Focused,
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
    fn migrates_multipart_v133_datasource_ids_losslessly() {
        let agent = base_agent(vec![
            res(ResourceType::ExtensionMetric, "extension:weather:temp", "Temp"),
            res(
                ResourceType::ExtensionMetric,
                "extension:extension:tariff:price",
                "Price",
            ),
            res(ResourceType::Metric, "device:sensor1:temp", "Sensor Temp"),
            res(ResourceType::DataStream, "ai:agt1:score", "Upstream Score"),
            res(ResourceType::Metric, "extension:solar:kw", "Solar kW"),
            res(ResourceType::Device, "device:cam1", "Camera 1"),
            res(ResourceType::Command, "device:relay1:on", "Relay On"),
            res(ResourceType::ExtensionTool, "extension:weather:refresh", "Refresh"),
            res(ResourceType::ExtensionTool, "extension:notifier", "Notifier"),
        ]);

        let (spec, _, _, _) = map_legacy_agent_record(&agent);

        // Verify input requirements (6 read sources)
        let reqs = &spec.input.requirements;
        assert_eq!(reqs.len(), 6);

        assert_eq!(
            reqs[0]["source"],
            serde_json::json!({ "kind": "extension_instance", "id": "weather" })
        );
        assert_eq!(reqs[0]["operation"], serde_json::json!("temp"));
        assert_eq!(reqs[0]["auth_scope"], serde_json::json!("extension:weather:read"));

        assert_eq!(
            reqs[1]["source"],
            serde_json::json!({ "kind": "extension_instance", "id": "tariff" })
        );
        assert_eq!(reqs[1]["operation"], serde_json::json!("price"));
        assert_eq!(reqs[1]["auth_scope"], serde_json::json!("extension:tariff:read"));

        assert_eq!(
            reqs[2]["source"],
            serde_json::json!({ "kind": "device_metric", "device_id": "sensor1", "field": "temp" })
        );
        assert_eq!(reqs[2]["auth_scope"], serde_json::json!("device:sensor1:read"));

        assert_eq!(
            reqs[3]["source"],
            serde_json::json!({ "kind": "upstream_ai", "agent_id": "agt1", "field": "score" })
        );

        assert_eq!(
            reqs[4]["source"],
            serde_json::json!({ "kind": "extension_instance", "id": "solar" })
        );
        assert_eq!(reqs[4]["operation"], serde_json::json!("kw"));
        assert_eq!(reqs[4]["auth_scope"], serde_json::json!("extension:solar:read"));

        assert_eq!(
            reqs[5]["source"],
            serde_json::json!({ "kind": "whole_device", "device_id": "cam1" })
        );
        assert_eq!(reqs[5]["auth_scope"], serde_json::json!("device:cam1:read"));

        // Verify controlled_actions (3 write targets)
        assert_eq!(spec.controlled_actions.len(), 3);
        assert_eq!(spec.controlled_actions[0]["destination"], "device:relay1");
        assert_eq!(spec.controlled_actions[0]["name"], "on");

        assert_eq!(spec.controlled_actions[1]["destination"], "extension:weather");
        assert_eq!(spec.controlled_actions[1]["name"], "refresh");

        assert_eq!(spec.controlled_actions[2]["destination"], "extension:notifier");
        assert_eq!(spec.controlled_actions[2]["name"], "invoke");

        // Verify Scoped.allowed_commands stays empty even with ResourceType::Command present
        assert_eq!(
            spec.tool_policy,
            ToolPolicy::Scoped {
                allowed_tools: vec!["device".into()],
                allowed_commands: Vec::new(),
            }
        );

        // Verify event trigger fallback strips prefixes cleanly
        match &spec.trigger {
            TriggerSpec::Event { filter } => {
                let any = filter["any"].as_array().expect("any array");
                assert!(any.contains(&serde_json::json!({
                    "type": "extension",
                    "id": "weather",
                    "field": "temp"
                })));
                assert!(any.contains(&serde_json::json!({
                    "type": "device",
                    "id": "sensor1",
                    "field": "temp"
                })));
                assert!(any.contains(&serde_json::json!({
                    "type": "device",
                    "id": "cam1"
                })));
                assert!(any.contains(&serde_json::json!({
                    "type": "ai",
                    "id": "agt1",
                    "field": "score"
                })));
            }
            other => panic!("expected Event trigger, got {other:?}"),
        }
    }

    #[test]
    fn migrate_preserves_newer_agent_memory_table_entry() {
        let db = Database::builder()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .unwrap();

        {
            let write = db.begin_write().unwrap();
            {
                let mut legacy = write.open_table(LEGACY_AGENTS_TABLE).unwrap();
                let mut mem_table = write.open_table(AGENT_MEMORY_TABLE).unwrap();

                // Agent 1: AGENT_MEMORY_TABLE has NEWER memory than legacy agent.memory
                let mut a1 = base_agent(vec![]);
                a1.id = "keep-newer".into();
                a1.memory.updated_at = 1000;
                a1.memory.journal.records.push(ExecutionRecord {
                    timestamp: 1000,
                    execution_id: "stale".into(),
                    outcome: "stale".into(),
                    action_taken: "none".into(),
                    success: true,
                    stop_reason: String::new(),
                });
                legacy
                    .insert("keep-newer", serde_json::to_vec(&a1).unwrap().as_slice())
                    .unwrap();

                let mut newer_mem = AgentMemory::default();
                newer_mem.updated_at = 2000;
                newer_mem.journal.records.push(ExecutionRecord {
                    timestamp: 2000,
                    execution_id: "fresh".into(),
                    outcome: "fresh".into(),
                    action_taken: "sent".into(),
                    success: true,
                    stop_reason: String::new(),
                });
                mem_table
                    .insert("keep-newer", serde_json::to_vec(&newer_mem).unwrap().as_slice())
                    .unwrap();

                // Agent 2: legacy agent.memory is NEWER than AGENT_MEMORY_TABLE
                let mut a2 = base_agent(vec![]);
                a2.id = "upgrade-older".into();
                a2.memory.updated_at = 3000;
                legacy
                    .insert("upgrade-older", serde_json::to_vec(&a2).unwrap().as_slice())
                    .unwrap();

                let mut older_mem = AgentMemory::default();
                older_mem.updated_at = 1500;
                mem_table
                    .insert(
                        "upgrade-older",
                        serde_json::to_vec(&older_mem).unwrap().as_slice(),
                    )
                    .unwrap();
            }
            write.commit().unwrap();
        }

        migrate_legacy_agents_table(&db).unwrap();

        let read = db.begin_read().unwrap();
        let mem_table = read.open_table(AGENT_MEMORY_TABLE).unwrap();

        let kept: AgentMemory = serde_json::from_slice(
            mem_table.get("keep-newer").unwrap().unwrap().value(),
        )
        .unwrap();
        assert_eq!(kept.updated_at, 2000);
        assert_eq!(kept.journal.records[0].execution_id, "fresh");

        let upgraded: AgentMemory = serde_json::from_slice(
            mem_table.get("upgrade-older").unwrap().unwrap().value(),
        )
        .unwrap();
        assert_eq!(upgraded.updated_at, 3000);
    }
}
