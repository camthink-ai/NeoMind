//! AgentApplicationService: CRUD, EffectivePlan, dry-run, formal invoke.

use std::collections::HashSet;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::{AgentError, Result};
use crate::input::InputPipeline;
use crate::model::{
    AgentRuntimeRecord, AgentSpec, AgentSpecDraft, AgentSpecPatch, AgentStats, AgentStatus,
    EffectivePlan, ExternalReadCounts, PlanRead, ReceiverCounts, RunResponse, ToolPolicy,
    TriggerSpec,
};
use crate::ports::{CapabilityClass, ClockPort};
use crate::runner::{apply_outcome, AgentRunner};
use crate::side_effects::SideEffectBoundary;
use crate::store::InMemoryAgentStore;

pub struct AgentApplicationService {
    pub store: InMemoryAgentStore,
    pub clock: Arc<dyn ClockPort>,
    pub input: InputPipeline,
    pub runner: Arc<dyn AgentRunner>,
    pub side_effects: SideEffectBoundary,
    /// Shared fake counters exposed to tests.
    pub notify_counter: Arc<dyn NotifyCounter>,
    pub write_counter: Arc<dyn WriteCounter>,
}

pub trait NotifyCounter: Send + Sync {
    fn count(&self) -> u64;
}

pub trait WriteCounter: Send + Sync {
    fn count(&self) -> u64;
}

impl AgentApplicationService {
    pub fn create(&self, draft: AgentSpecDraft) -> Result<(AgentSpec, EffectivePlan)> {
        validate_draft(&draft)?;
        let now = self.clock.now_ms();
        let id = format!("agt_{}", &Uuid::new_v4().to_string()[..8]);
        let spec = AgentSpec {
            id: id.clone(),
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
        let saved = self.store.save_spec(spec, now)?;
        let plan = self.effective_plan(&saved.id)?;
        Ok((saved, plan))
    }

    pub fn get(&self, id: &str) -> Result<AgentSpec> {
        self.store.get_spec(id)
    }

    pub fn list(&self) -> Vec<(AgentSpec, EffectivePlan)> {
        self.store
            .list_specs()
            .into_iter()
            .map(|spec| {
                let plan = plan_from_spec(&spec);
                (spec, plan)
            })
            .collect()
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        self.store.delete_spec(id)
    }

    pub fn update(&self, id: &str, patch: AgentSpecPatch) -> Result<(AgentSpec, EffectivePlan)> {
        let mut spec = self.store.get_spec(id)?;
        if let Some(v) = patch.name {
            spec.name = v;
        }
        if let Some(v) = patch.description {
            spec.description = v;
        }
        if let Some(v) = patch.user_prompt {
            spec.user_prompt = v;
        }
        if let Some(v) = patch.llm_backend_id {
            spec.llm_backend_id = v;
        }
        if let Some(v) = patch.preset {
            spec.preset = v;
        }
        if let Some(v) = patch.trigger {
            spec.trigger = v;
        }
        if let Some(v) = patch.pacing {
            spec.pacing = v;
        }
        if let Some(v) = patch.input {
            spec.input = v;
        }
        if let Some(v) = patch.tool_policy {
            spec.tool_policy = v;
        }
        if let Some(v) = patch.output {
            spec.output = v;
        }
        if let Some(v) = patch.notify {
            spec.notify = v;
        }
        if let Some(v) = patch.controlled_actions {
            spec.controlled_actions = v;
        }
        validate_spec(&spec)?;
        spec.config_version += 1;
        spec.updated_at = self.clock.now_ms();
        let saved = self.store.save_spec(spec, self.clock.now_ms())?;
        let plan = self.effective_plan(&saved.id)?;
        Ok((saved, plan))
    }

    /// Static EffectivePlan — zero external reads.
    pub fn effective_plan(&self, id: &str) -> Result<EffectivePlan> {
        let spec = self.store.get_spec(id)?;
        Ok(plan_from_spec(&spec))
    }

    /// Authorized read-only dry-run; does not enter formal history; notify=0.
    pub async fn dry_run(&self, id: &str) -> Result<RunResponse> {
        self.execute_inner(id, true, false).await
    }

    /// Formal invoke — writes history and may notify.
    pub async fn invoke(&self, id: &str) -> Result<RunResponse> {
        self.execute_inner(id, false, false).await
    }

    async fn execute_inner(&self, id: &str, dry_run: bool, is_probe: bool) -> Result<RunResponse> {
        let spec = self.store.get_spec(id)?;
        let runtime = self.store.get_runtime(id)?;
        if matches!(runtime.status, AgentStatus::Paused | AgentStatus::Stopped) && !dry_run {
            return Err(AgentError::NotRunnable(format!("{:?}", runtime.status)));
        }

        let (evidence, read_counts) = self.input.collect(&spec.input.requirements).await?;
        let execution_id = format!("exec_{}", &Uuid::new_v4().to_string()[..8]);
        let started = self.clock.now_ms();
        let plan = plan_from_spec(&spec);

        let run_result = self
            .runner
            .run(&spec, &evidence, &execution_id, CancellationToken::new())
            .await;

        let mut exec = crate::model::AgentExecutionRecord {
            id: execution_id.clone(),
            agent_id: spec.id.clone(),
            config_version: spec.config_version,
            status: crate::model::ExecutionStatus::Completed,
            started_at: started,
            finished_at: self.clock.now_ms(),
            is_probe,
            dry_run,
            stale_config: None,
            evidence,
            deliveries: vec![],
            brief_text: None,
            conclusion: None,
            structured_result: None,
            error: None,
            alert_verdict: None,
        };

        match run_result {
            Ok(outcome) => {
                exec = apply_outcome(exec, outcome);
            }
            Err(e) => {
                // Inference / runner failure settles as Failed — never invent a
                // successful local brief when the configured backend failed.
                exec.status = crate::model::ExecutionStatus::Failed;
                exec.error = Some(e.to_string());
                exec.conclusion = None;
                exec.brief_text = None;
                exec.structured_result = None;
                exec.alert_verdict = Some(crate::model::AlertVerdict::InsufficientEvidence);
            }
        }
        exec.finished_at = self.clock.now_ms();

        // Persist the execution skeleton before settle so non-idempotent
        // IntentPending claims are restart-visible (formal history upserts by id).
        if !dry_run {
            self.store.save_execution(exec.clone())?;
        }

        let exec = self.side_effects.settle(&spec, exec, dry_run).await?;

        if !dry_run {
            self.store.save_execution(exec.clone())?;
            let mut rt = self.store.get_runtime(id)?;
            let tz = crate::coordinator::quota::quota_timezone(&spec.pacing, &spec.trigger);
            crate::coordinator::quota::roll_quota_day(&mut rt.stats, self.clock.now_ms(), &tz);
            rt.stats.total_runs += 1;
            if !matches!(exec.status, crate::model::ExecutionStatus::Skipped) {
                rt.stats.runs_today = rt.stats.runs_today.saturating_add(1);
            }
            match exec.status {
                crate::model::ExecutionStatus::Completed => {
                    rt.stats.successful_runs += 1;
                    rt.stats.consecutive_failures = 0;
                }
                crate::model::ExecutionStatus::Partial => {
                    rt.stats.partial_runs += 1;
                    // Partial does not increment consecutive_failures
                }
                crate::model::ExecutionStatus::Failed => {
                    rt.stats.failed_runs += 1;
                    rt.stats.consecutive_failures += 1;
                }
                crate::model::ExecutionStatus::Skipped => {}
            }
            rt.stats.last_execution_at = Some(exec.finished_at);
            rt.stats.last_execution_id = Some(exec.id.clone());
            if matches!(exec.status, crate::model::ExecutionStatus::Failed) {
                rt.status = AgentStatus::Error;
                rt.error_message = exec.error.clone();
            } else if matches!(spec.trigger, TriggerSpec::Manual) {
                rt.status = AgentStatus::Completed;
            } else {
                rt.status = AgentStatus::Active;
            }
            rt.updated_at = self.clock.now_ms();
            self.store.set_runtime(rt)?;
        }

        Ok(RunResponse {
            dry_run,
            execution: Some(exec),
            effective_plan: Some(plan),
            external_read_counts: read_counts,
            receiver_counts: ReceiverCounts {
                notify: if dry_run {
                    0
                } else {
                    self.notify_counter.count()
                },
                device_control: if dry_run {
                    0
                } else {
                    self.write_counter.count()
                },
                extension_write: 0,
            },
        })
    }

    pub fn formal_history_len(&self, id: &str) -> usize {
        self.store.formal_history_len(id)
    }

    pub fn latest_formal(&self, id: &str) -> Option<crate::model::AgentExecutionRecord> {
        self.store.formal_history(id).into_iter().next_back()
    }

    pub fn explain_last(&self, id: &str) -> Result<String> {
        let Some(exec) = self.latest_formal(id) else {
            return Ok("No formal execution yet.".into());
        };
        let mut lines = vec![
            format!("execution_id={}", exec.id),
            format!("status={:?}", exec.status),
        ];
        for item in &exec.evidence.items {
            lines.push(format!(
                "evidence {}: quality={:?} source={} proof={}",
                item.key, item.quality, item.source_id, item.binding_proof
            ));
        }
        if exec.evidence.missing_keys.is_empty() {
            lines.push("missing: (none)".into());
        } else {
            lines.push(format!(
                "missing: {}",
                exec.evidence.missing_keys.join(", ")
            ));
        }
        if let Some(sr) = &exec.structured_result {
            lines.push(format!("structured_result={sr}"));
        } else if let Some(c) = &exec.conclusion {
            let preview: String = c.chars().take(240).collect();
            lines.push(format!("conclusion={preview}"));
        }
        let notify_n = exec
            .deliveries
            .iter()
            .filter(|d| d.action_fingerprint.starts_with("on_"))
            .count();
        lines.push(format!(
            "why notify: deliveries_with_notify_fingerprint={notify_n}, on_complete_policy={}, alert={:?}",
            self.store.get_spec(id)?.notify.on_complete,
            exec.alert_verdict
        ));
        Ok(lines.join("\n"))
    }

    pub fn runtime(&self, id: &str) -> Result<AgentRuntimeRecord> {
        self.store.get_runtime(id)
    }
}

fn validate_draft(draft: &AgentSpecDraft) -> Result<()> {
    validate_keys_and_deps(
        &draft
            .input
            .requirements
            .iter()
            .map(|r| r.key.clone())
            .collect::<Vec<_>>(),
        &draft
            .output
            .fields
            .iter()
            .flat_map(|f| f.depends_on.clone())
            .chain(draft.notify.alert_depends_on.clone())
            .chain(
                draft
                    .controlled_actions
                    .iter()
                    .flat_map(|a| a.depends_on.clone()),
            )
            .collect::<Vec<_>>(),
        &draft
            .input
            .requirements
            .iter()
            .map(|r| r.key.as_str())
            .collect::<Vec<_>>(),
    )
}

fn validate_spec(spec: &AgentSpec) -> Result<()> {
    validate_keys_and_deps(
        &spec
            .input
            .requirements
            .iter()
            .map(|r| r.key.clone())
            .collect::<Vec<_>>(),
        &spec
            .output
            .fields
            .iter()
            .flat_map(|f| f.depends_on.clone())
            .chain(spec.notify.alert_depends_on.clone())
            .chain(
                spec.controlled_actions
                    .iter()
                    .flat_map(|a| a.depends_on.clone()),
            )
            .collect::<Vec<_>>(),
        &spec
            .input
            .requirements
            .iter()
            .map(|r| r.key.as_str())
            .collect::<Vec<_>>(),
    )
}

fn validate_keys_and_deps(keys: &[String], deps: &[String], key_refs: &[&str]) -> Result<()> {
    let mut seen = HashSet::new();
    for k in keys {
        if !seen.insert(k.as_str()) {
            return Err(AgentError::InvalidConfig(format!(
                "duplicate input key: {k}"
            )));
        }
    }
    for d in deps {
        if !key_refs.contains(&d.as_str()) {
            return Err(AgentError::InvalidConfig(format!(
                "depends_on key `{d}` not in InputContract"
            )));
        }
    }
    Ok(())
}

pub fn plan_from_spec(spec: &AgentSpec) -> EffectivePlan {
    let (trigger_summary, trigger_tz) = match &spec.trigger {
        TriggerSpec::Cron { expr, timezone } => (
            format!("cron `{expr}` ({timezone})"),
            Some(timezone.clone()),
        ),
        TriggerSpec::Interval { every_secs } => (format!("every {every_secs}s"), None),
        TriggerSpec::Event { .. } => ("event".into(), None),
        TriggerSpec::Manual => ("manual".into(), None),
    };
    // Preview timezone prefers pacing.timezone (quota window), then cron TZ.
    let timezone = spec
        .pacing
        .timezone
        .clone()
        .filter(|s| !s.is_empty())
        .or(trigger_tz);
    let authorized_reads = spec
        .input
        .requirements
        .iter()
        .map(|r| {
            let source_id = match &r.source {
                crate::model::InputSourceRef::ExtensionInstance { id } => {
                    format!("extension:{id}")
                }
                crate::model::InputSourceRef::DeviceMetric { device_id, field } => {
                    format!("device:{device_id}:{field}")
                }
                crate::model::InputSourceRef::WholeDevice { device_id } => {
                    format!("device:{device_id}")
                }
                crate::model::InputSourceRef::UpstreamAi { agent_id, field } => {
                    format!("ai:{agent_id}:{field}")
                }
                crate::model::InputSourceRef::CallerPayload => "caller".into(),
            };
            PlanRead {
                key: r.key.clone(),
                source_id,
                operation: r.operation.clone(),
                auth_scope: r.auth_scope.clone(),
                capability_class: format!("{:?}", CapabilityClass::ReadOnly).to_ascii_lowercase(),
            }
        })
        .collect();
    let mut warnings = Vec::new();
    if spec.notify.pending_confirmation {
        warnings.push("notify pending_confirmation: operator must confirm destination".into());
    }
    if matches!(spec.tool_policy, ToolPolicy::Disabled) {
        warnings.push("model tools disabled; host InputPipeline reads authorized sources".into());
    }
    EffectivePlan {
        agent_id: spec.id.clone(),
        config_version: spec.config_version,
        trigger_summary,
        timezone,
        authorized_reads,
        tool_policy: spec.tool_policy.clone(),
        notify: spec.notify.clone(),
        controlled_actions: spec.controlled_actions.clone(),
        warnings,
        external_read_counts: ExternalReadCounts::default(), // always 0 for static preview
    }
}

/// Helper so tests can seed runtime.
pub fn active_runtime(agent_id: &str, now: i64) -> AgentRuntimeRecord {
    AgentRuntimeRecord {
        agent_id: agent_id.into(),
        status: AgentStatus::Active,
        stats: AgentStats::default(),
        error_message: None,
        next_probe_at: None,
        updated_at: now,
    }
}
