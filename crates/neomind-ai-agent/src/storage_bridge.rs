//! Bridge between runtime `AgentSpec` and `neomind_storage::agent_store` records.
//!
//! Used when persisting agents into the five-table layout without depending on
//! storage from the hot path in unit tests (`InMemoryAgentStore`).

use neomind_storage::agent_store::{
    AgentRuntimeRecord as StorageRuntime, AgentRuntimeStats, AgentSpecRecord,
    InputContract as StorageInput, NotifyPolicy as StorageNotify, OutputContract as StorageOutput,
    PacingPolicy as StoragePacing, ToolPolicy as StorageToolPolicy, TriggerSpec as StorageTrigger,
};

use crate::model::{
    AgentRuntimeRecord, AgentSpec, AgentStatus, NotifyPolicy, PacingPolicy, ToolPolicy, TriggerSpec,
};

pub fn spec_to_record(spec: &AgentSpec) -> AgentSpecRecord {
    AgentSpecRecord {
        id: spec.id.clone(),
        name: spec.name.clone(),
        description: spec.description.clone(),
        user_prompt: spec.user_prompt.clone(),
        llm_backend_id: spec.llm_backend_id.clone(),
        trigger: trigger_to_storage(&spec.trigger),
        pacing: pacing_to_storage(&spec.pacing),
        input: StorageInput {
            requirements: spec
                .input
                .requirements
                .iter()
                .filter_map(|r| serde_json::to_value(r).ok())
                .collect(),
        },
        tool_policy: tool_to_storage(&spec.tool_policy),
        output: StorageOutput {
            fields: spec
                .output
                .fields
                .iter()
                .filter_map(|f| serde_json::to_value(f).ok())
                .collect(),
            publish: spec.output.publish,
        },
        notify: notify_to_storage(&spec.notify),
        controlled_actions: spec
            .controlled_actions
            .iter()
            .filter_map(|a| serde_json::to_value(a).ok())
            .collect(),
        config_version: spec.config_version,
        created_at: spec.created_at,
        updated_at: spec.updated_at,
    }
}

pub fn record_to_spec(rec: &AgentSpecRecord) -> Result<AgentSpec, String> {
    let requirements = rec
        .input
        .requirements
        .iter()
        .map(|v| serde_json::from_value(v.clone()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let fields = rec
        .output
        .fields
        .iter()
        .map(|v| serde_json::from_value(v.clone()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let controlled_actions = rec
        .controlled_actions
        .iter()
        .map(|v| serde_json::from_value(v.clone()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(AgentSpec {
        id: rec.id.clone(),
        name: rec.name.clone(),
        description: rec.description.clone(),
        user_prompt: rec.user_prompt.clone(),
        llm_backend_id: rec.llm_backend_id.clone(),
        preset: None,
        trigger: trigger_from_storage(&rec.trigger),
        pacing: pacing_from_storage(&rec.pacing),
        input: crate::model::InputContract { requirements },
        tool_policy: tool_from_storage(&rec.tool_policy),
        output: crate::model::OutputContract {
            fields,
            publish: rec.output.publish,
        },
        notify: notify_from_storage(&rec.notify),
        controlled_actions,
        config_version: rec.config_version,
        created_at: rec.created_at,
        updated_at: rec.updated_at,
    })
}

pub fn runtime_to_storage(rt: &AgentRuntimeRecord) -> StorageRuntime {
    StorageRuntime {
        agent_id: rt.agent_id.clone(),
        status: status_to_storage(rt.status),
        stats: AgentRuntimeStats {
            total_runs: rt.stats.total_runs,
            successful_runs: rt.stats.successful_runs,
            failed_runs: rt.stats.failed_runs,
            partial_runs: rt.stats.partial_runs,
            skipped_daily_limit: rt.stats.skipped_daily_limit,
            skipped_permit_timeout: rt.stats.skipped_permit_timeout,
            consecutive_failures: rt.stats.consecutive_failures,
            runs_today: rt.stats.runs_today,
            quota_day: rt.stats.quota_day.clone(),
            last_execution_at: rt.stats.last_execution_at,
            last_execution_id: rt.stats.last_execution_id.clone(),
            avg_duration_ms: rt.stats.avg_duration_ms,
            last_duration_ms: rt.stats.last_duration_ms,
        },
        error_message: rt.error_message.clone(),
        next_probe_at: rt.next_probe_at,
        updated_at: rt.updated_at,
    }
}

fn trigger_to_storage(t: &TriggerSpec) -> StorageTrigger {
    match t {
        TriggerSpec::Cron { expr, timezone } => StorageTrigger::Cron {
            expr: expr.clone(),
            timezone: timezone.clone(),
        },
        TriggerSpec::Interval { every_secs } => StorageTrigger::Interval {
            every_secs: *every_secs,
        },
        TriggerSpec::Event { filter } => StorageTrigger::Event {
            filter: filter.clone(),
        },
        TriggerSpec::Manual => StorageTrigger::Manual,
    }
}

fn trigger_from_storage(t: &StorageTrigger) -> TriggerSpec {
    match t {
        StorageTrigger::Cron { expr, timezone } => TriggerSpec::Cron {
            expr: expr.clone(),
            timezone: timezone.clone(),
        },
        StorageTrigger::Interval { every_secs } => TriggerSpec::Interval {
            every_secs: *every_secs,
        },
        StorageTrigger::Event { filter } => TriggerSpec::Event {
            filter: filter.clone(),
        },
        StorageTrigger::Manual => TriggerSpec::Manual,
    }
}

fn pacing_to_storage(p: &PacingPolicy) -> StoragePacing {
    StoragePacing {
        burst_merge_secs: p.burst_merge_secs,
        min_interval_secs: p.min_interval_secs,
        max_runs_per_day: p.max_runs_per_day,
        timezone: p.timezone.clone(),
        failure_threshold: p.failure_threshold,
    }
}

fn pacing_from_storage(p: &StoragePacing) -> PacingPolicy {
    PacingPolicy {
        burst_merge_secs: p.burst_merge_secs,
        min_interval_secs: p.min_interval_secs,
        max_runs_per_day: p.max_runs_per_day,
        timezone: p.timezone.clone(),
        failure_threshold: p.failure_threshold,
    }
}

fn tool_to_storage(t: &ToolPolicy) -> StorageToolPolicy {
    match t {
        ToolPolicy::Disabled => StorageToolPolicy::Disabled,
        ToolPolicy::Unrestricted => StorageToolPolicy::Unrestricted,
        ToolPolicy::Scoped {
            allowed_tools,
            allowed_commands,
        } => StorageToolPolicy::Scoped {
            allowed_tools: allowed_tools.clone(),
            allowed_commands: allowed_commands.clone(),
        },
    }
}

fn tool_from_storage(t: &StorageToolPolicy) -> ToolPolicy {
    match t {
        StorageToolPolicy::Disabled => ToolPolicy::Disabled,
        StorageToolPolicy::Unrestricted => ToolPolicy::Unrestricted,
        StorageToolPolicy::Scoped {
            allowed_tools,
            allowed_commands,
        } => ToolPolicy::Scoped {
            allowed_tools: allowed_tools.clone(),
            allowed_commands: allowed_commands.clone(),
        },
    }
}

fn notify_to_storage(n: &NotifyPolicy) -> StorageNotify {
    StorageNotify {
        channels: n.channels.clone(),
        on_complete: n.on_complete,
        on_alert: n.on_alert,
        alert_depends_on: n.alert_depends_on.clone(),
        on_failure: n.on_failure,
        pending_confirmation: n.pending_confirmation,
    }
}

fn notify_from_storage(n: &StorageNotify) -> NotifyPolicy {
    NotifyPolicy {
        channels: n.channels.clone(),
        on_complete: n.on_complete,
        on_alert: n.on_alert,
        alert_depends_on: n.alert_depends_on.clone(),
        on_failure: n.on_failure,
        pending_confirmation: n.pending_confirmation,
    }
}

pub fn status_to_storage(s: AgentStatus) -> neomind_storage::AgentStatus {
    match s {
        AgentStatus::Active => neomind_storage::AgentStatus::Active,
        AgentStatus::Executing => neomind_storage::AgentStatus::Executing,
        AgentStatus::Paused => neomind_storage::AgentStatus::Paused,
        AgentStatus::Stopped => neomind_storage::AgentStatus::Stopped,
        AgentStatus::Error => neomind_storage::AgentStatus::Error,
        AgentStatus::Completed => neomind_storage::AgentStatus::Completed,
    }
}

pub fn status_from_storage(s: neomind_storage::AgentStatus) -> AgentStatus {
    match s {
        neomind_storage::AgentStatus::Active => AgentStatus::Active,
        neomind_storage::AgentStatus::Executing => AgentStatus::Executing,
        neomind_storage::AgentStatus::Paused => AgentStatus::Paused,
        neomind_storage::AgentStatus::Stopped => AgentStatus::Stopped,
        neomind_storage::AgentStatus::Error => AgentStatus::Error,
        neomind_storage::AgentStatus::Completed => AgentStatus::Completed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::daily_brief_draft;

    #[test]
    fn daily_brief_spec_roundtrips_through_storage_record() {
        let draft = daily_brief_draft("weather_cn", "tariff_tou", "test-channel");
        let now = 1i64;
        let spec = AgentSpec {
            id: "agt_x".into(),
            name: draft.name,
            description: draft.description,
            user_prompt: draft.user_prompt,
            llm_backend_id: draft.llm_backend_id,
            preset: draft.preset,
            trigger: draft.trigger,
            pacing: draft.pacing,
            input: draft.input,
            tool_policy: draft.tool_policy,
            output: draft.output,
            notify: draft.notify,
            controlled_actions: draft.controlled_actions,
            config_version: 1,
            created_at: now,
            updated_at: now,
        };
        let rec = spec_to_record(&spec);
        let back = record_to_spec(&rec).unwrap();
        assert_eq!(back.id, spec.id);
        assert_eq!(back.notify.channels, spec.notify.channels);
        assert_eq!(back.input.requirements.len(), 2);
        assert!(matches!(back.tool_policy, ToolPolicy::Disabled));
    }
}
