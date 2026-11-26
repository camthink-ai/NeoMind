//! AgentApplicationService: CRUD, EffectivePlan, dry-run, formal invoke.

use std::collections::HashSet;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::{AgentError, Result};
use crate::input::InputPipeline;
use crate::model::{
    AgentRuntimeRecord, AgentSpec, AgentSpecDraft, AgentSpecPatch, AgentStats, AgentStatus,
    CallerInput, EffectivePlan, EvidenceDetail, EvidenceItem, EvidenceQuality, EvidenceReport,
    ExternalReadCounts, InputModality, InputSourceRef, PlanRead, ReceiverCounts, RunResponse,
    ToolPolicy, TriggerSpec,
};
use crate::ports::{CapabilityClass, ClockPort};
use crate::runner::{
    apply_outcome, prompt_with_caller_input, prompt_with_memory_context,
    prompt_with_operator_notes, AgentRunner, MemoryContext,
};
use crate::side_effects::SideEffectBoundary;
use crate::store::InMemoryAgentStore;

/// Per-run options (run-local only; never persisted onto `AgentSpec`).
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// Pre-allocated execution id so host events (Started/Completed) match the persisted row.
    pub execution_id: Option<String>,
    /// Optional this-turn caller input (ask text / structured payload).
    pub caller: Option<CallerInput>,
    /// Optional persisted operator notes (`user_messages`) appended to the run prompt.
    pub operator_notes: Vec<String>,
    /// Optional persisted agent memory (recent execution journal + knowledge files).
    pub memory_context: MemoryContext,
}

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
        self.dry_run_with_options(id, RunOptions::default()).await
    }

    /// Authorized read-only dry-run with run-local options (operator notes / memory context).
    pub async fn dry_run_with_options(&self, id: &str, options: RunOptions) -> Result<RunResponse> {
        self.execute_inner(id, true, false, None, options).await
    }

    /// Formal invoke — writes history and may notify (manual submit provenance).
    pub async fn invoke(&self, id: &str) -> Result<RunResponse> {
        self.invoke_with_options(
            id,
            crate::coordinator::SubmitSource::Manual,
            RunOptions::default(),
        )
        .await
    }

    /// Formal invoke with optional this-turn caller text/data (ask path).
    pub async fn invoke_with_caller(
        &self,
        id: &str,
        caller: Option<CallerInput>,
    ) -> Result<RunResponse> {
        self.invoke_with_options(
            id,
            crate::coordinator::SubmitSource::Manual,
            RunOptions {
                caller,
                ..Default::default()
            },
        )
        .await
    }

    /// Formal invoke with explicit submit provenance (schedule/event/manual/…).
    pub async fn invoke_with_source(
        &self,
        id: &str,
        source: crate::coordinator::SubmitSource,
    ) -> Result<RunResponse> {
        self.invoke_with_options(id, source, RunOptions::default())
            .await
    }

    /// Formal invoke with both explicit submit provenance and optional caller input.
    pub async fn invoke_with_source_and_caller(
        &self,
        id: &str,
        source: crate::coordinator::SubmitSource,
        caller: Option<CallerInput>,
    ) -> Result<RunResponse> {
        self.invoke_with_options(
            id,
            source,
            RunOptions {
                caller,
                ..Default::default()
            },
        )
        .await
    }

    /// Formal invoke with explicit submit provenance and per-run options.
    pub async fn invoke_with_options(
        &self,
        id: &str,
        source: crate::coordinator::SubmitSource,
        options: RunOptions,
    ) -> Result<RunResponse> {
        self.execute_inner(id, false, false, Some(source), options)
            .await
    }

    async fn execute_inner(
        &self,
        id: &str,
        dry_run: bool,
        is_probe: bool,
        source: Option<crate::coordinator::SubmitSource>,
        options: RunOptions,
    ) -> Result<RunResponse> {
        let spec = self.store.get_spec(id)?;
        let runtime = self.store.get_runtime(id)?;
        if matches!(runtime.status, AgentStatus::Paused | AgentStatus::Stopped) && !dry_run {
            return Err(AgentError::NotRunnable(format!("{:?}", runtime.status)));
        }

        let mut requirements = spec.input.requirements.clone();
        if let Some(ref c) = options.caller {
            if let Some(ref data) = c.data {
                let ev_report_id = data.get("report_id").and_then(|v| v.as_str());
                let ev_ts = data.get("report_timestamp").and_then(|v| v.as_i64());
                for req in &mut requirements {
                    if matches!(req.freshness, crate::model::FreshnessPolicy::SameReport) {
                        if let Some(obj) = req.params.as_object_mut() {
                            if !obj.contains_key("report_id") {
                                if let Some(rid) = ev_report_id {
                                    obj.insert(
                                        "report_id".into(),
                                        serde_json::Value::String(rid.to_string()),
                                    );
                                }
                            }
                            if !obj.contains_key("anchor_ts") {
                                if let Some(ts) = ev_ts {
                                    obj.insert("anchor_ts".into(), serde_json::json!(ts));
                                }
                            }
                        }
                    }
                }
            }
        }
        let (mut evidence, read_counts) = self.input.collect(&requirements).await?;
        let now = self.clock.now_ms();
        if let Some(ref c) = options.caller {
            merge_caller_into_evidence(&mut evidence, &spec, c, now);
        }
        let execution_id = options
            .execution_id
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("exec_{}", &Uuid::new_v4().to_string()[..8]));
        let started = now;
        let plan = plan_from_spec(&spec);

        // Short-circuit `MissingPolicy::Skip` without calling the LLM.
        if let Some(skip_reason) = evidence.skip_reason.clone() {
            let exec = crate::model::AgentExecutionRecord {
                id: execution_id,
                agent_id: spec.id.clone(),
                config_version: spec.config_version,
                status: crate::model::ExecutionStatus::Skipped,
                started_at: started,
                finished_at: self.clock.now_ms(),
                is_probe,
                dry_run,
                trigger_source: source.map(|s| s.as_str().to_string()),
                stale_config: None,
                evidence,
                deliveries: vec![],
                tool_calls: vec![],
                brief_text: None,
                conclusion: Some(format!("Skipped: {skip_reason}")),
                structured_result: None,
                alert_verdict: None,
                error: Some(skip_reason),
            };
            if !dry_run && !is_probe {
                self.store.save_execution(exec.clone())?;
            }
            return Ok(RunResponse {
                dry_run,
                execution: Some(exec),
                effective_plan: Some(plan),
                external_read_counts: read_counts,
                receiver_counts: ReceiverCounts {
                    notify: 0,
                    device_control: 0,
                    extension_write: 0,
                },
            });
        }

        // Run-local prompt only — never mutate the stored AgentSpec.
        let mut run_spec = spec.clone();
        if !options.memory_context.is_empty() {
            run_spec.user_prompt =
                prompt_with_memory_context(&run_spec.user_prompt, &options.memory_context);
        }
        if !options.operator_notes.is_empty() {
            run_spec.user_prompt =
                prompt_with_operator_notes(&run_spec.user_prompt, &options.operator_notes);
        }
        if let Some(ref c) = options.caller {
            if !c.is_empty() {
                run_spec.user_prompt = prompt_with_caller_input(&run_spec.user_prompt, c);
            }
        }

        let run_result = self
            .runner
            .run_with_history(
                &run_spec,
                &evidence,
                &execution_id,
                &options.memory_context.prior_turns,
                dry_run,
                CancellationToken::new(),
            )
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
            trigger_source: source.map(|s| s.as_str().to_string()),
            stale_config: None,
            evidence,
            deliveries: vec![],
            tool_calls: vec![],
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
            let duration_ms = exec.finished_at.saturating_sub(exec.started_at).max(0) as u64;
            let total = rt.stats.total_runs.max(1);
            rt.stats.avg_duration_ms = (rt
                .stats
                .avg_duration_ms
                .saturating_mul(total - 1)
                .saturating_add(duration_ms))
                / total;
            rt.stats.last_duration_ms = Some(duration_ms);
            if matches!(exec.status, crate::model::ExecutionStatus::Failed) {
                rt.error_message = exec.error.clone();
                let threshold = spec.pacing.failure_threshold.max(1);
                if rt.stats.consecutive_failures >= threshold {
                    rt.status = AgentStatus::Error;
                    let over = rt.stats.consecutive_failures.saturating_sub(threshold).min(4);
                    let backoff_ms = (300_000i64 << over).min(3_600_000);
                    rt.next_probe_at = Some(self.clock.now_ms().saturating_add(backoff_ms));
                }
            } else if matches!(spec.trigger, TriggerSpec::Manual) {
                rt.status = AgentStatus::Completed;
                rt.error_message = None;
                rt.next_probe_at = None;
            } else {
                rt.status = AgentStatus::Active;
                rt.error_message = None;
                rt.next_probe_at = None;
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
        let status_label = match exec.status {
            crate::model::ExecutionStatus::Completed => "Completed",
            crate::model::ExecutionStatus::Partial => "Partial",
            crate::model::ExecutionStatus::Skipped => "Skipped",
            crate::model::ExecutionStatus::Failed => "Failed",
        };
        let mut lines = vec![format!("Status: {status_label} ({})", exec.id)];

        if !exec.evidence.items.is_empty() {
            let sources: Vec<String> = exec
                .evidence
                .items
                .iter()
                .map(|item| {
                    let q = match &item.quality {
                        EvidenceQuality::Fresh => "fresh",
                        EvidenceQuality::Stale => "stale",
                        EvidenceQuality::Missing => "missing",
                        EvidenceQuality::Error { .. } => "error",
                    };
                    format!("{} ({q})", item.key)
                })
                .collect();
            lines.push(format!("Inputs: {}", sources.join(", ")));
        }
        if !exec.evidence.missing_keys.is_empty() {
            lines.push(format!("Missing: {}", exec.evidence.missing_keys.join(", ")));
        }
        if !exec.tool_calls.is_empty() {
            let names: Vec<&str> = exec.tool_calls.iter().map(|t| t.name.as_str()).collect();
            lines.push(format!(
                "Tools: {} ({} call{})",
                names.join(" -> "),
                exec.tool_calls.len(),
                if exec.tool_calls.len() == 1 { "" } else { "s" }
            ));
        }
        if let Some(err) = &exec.error {
            lines.push(format!("Error: {err}"));
        } else {
            if let Some(sr) = &exec.structured_result {
                if !sr.as_object().is_some_and(|m| m.is_empty()) {
                    lines.push(format!("Output: {sr}"));
                }
            }
            if let Some(c) = &exec.conclusion {
                let cleaned =
                    crate::runner::clean_model_response(c, exec.structured_result.as_ref());
                let preview: String = cleaned.chars().take(240).collect();
                if !preview.trim().is_empty() {
                    lines.push(format!("Summary: {preview}"));
                }
            }
        }

        let notify_n = exec
            .deliveries
            .iter()
            .filter(|d| d.action_fingerprint.starts_with("on_"))
            .count();
        let on_complete = self.store.get_spec(id)?.notify.on_complete;
        let verdict_label = match exec.alert_verdict {
            Some(crate::model::AlertVerdict::CriteriaMet) => "alert criteria met",
            Some(crate::model::AlertVerdict::CriteriaNotMet) => "no alert triggered",
            Some(crate::model::AlertVerdict::InsufficientEvidence) => "insufficient evidence",
            None => "no alert evaluation",
        };
        if notify_n > 0 {
            lines.push(format!("Notification: sent ({notify_n}, {verdict_label})"));
        } else if !on_complete {
            lines.push(format!(
                "Notification: not sent ({verdict_label}; completion notify off)"
            ));
        } else {
            lines.push(format!("Notification: not sent ({verdict_label})"));
        }
        Ok(lines.join("\n"))
    }

    pub fn runtime(&self, id: &str) -> Result<AgentRuntimeRecord> {
        self.store.get_runtime(id)
    }
}

fn validate_draft(draft: &AgentSpecDraft) -> Result<()> {
    validate_common_fields(
        &draft.name,
        draft.description.as_deref(),
        &draft.user_prompt,
        &draft.trigger,
    )?;
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
    validate_common_fields(
        &spec.name,
        spec.description.as_deref(),
        &spec.user_prompt,
        &spec.trigger,
    )?;
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

fn validate_common_fields(
    name: &str,
    description: Option<&str>,
    user_prompt: &str,
    trigger: &TriggerSpec,
) -> Result<()> {
    if name.trim().is_empty() {
        return Err(AgentError::InvalidConfig(
            "agent name must not be empty".into(),
        ));
    }
    if name.chars().count() > 120 {
        return Err(AgentError::InvalidConfig(
            "agent name exceeds 120 characters".into(),
        ));
    }
    if let Some(desc) = description {
        if desc.chars().count() > 2_000 {
            return Err(AgentError::InvalidConfig(
                "agent description exceeds 2000 characters".into(),
            ));
        }
    }
    if user_prompt.chars().count() > 32_000 {
        return Err(AgentError::InvalidConfig(
            "user_prompt exceeds 32000 characters".into(),
        ));
    }
    if let TriggerSpec::Interval { every_secs } = trigger {
        if *every_secs == 0 {
            return Err(AgentError::InvalidConfig(
                "interval every_secs must be >= 1".into(),
            ));
        }
    }
    Ok(())
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

/// Fill `CallerPayload` requirements from this-turn invoke input.
fn merge_caller_into_evidence(
    evidence: &mut EvidenceReport,
    spec: &AgentSpec,
    caller: &CallerInput,
    now: i64,
) {
    if caller.is_empty() {
        return;
    }
    let text = caller.content.clone().unwrap_or_default();
    let detail = if let Some(ref data) = caller.data {
        if !text.is_empty() {
            EvidenceDetail::Structured {
                value: serde_json::json!({ "text": text, "data": data }),
            }
        } else {
            EvidenceDetail::Structured {
                value: data.clone(),
            }
        }
    } else {
        EvidenceDetail::Text { text }
    };

    for req in &spec.input.requirements {
        if !matches!(req.source, InputSourceRef::CallerPayload) {
            continue;
        }
        evidence.missing_keys.retain(|k| k != &req.key);
        if let Some(item) = evidence.items.iter_mut().find(|i| i.key == req.key) {
            item.quality = EvidenceQuality::Fresh;
            item.fetched_at = now;
            item.observed_at = Some(now);
            item.source_id = "caller".into();
            item.binding_proof = "caller_payload:invoke".into();
            item.result_type = InputModality::CallerInput;
            item.detail = Some(detail.clone());
        } else {
            evidence.items.push(EvidenceItem {
                key: req.key.clone(),
                source_id: "caller".into(),
                result_type: InputModality::CallerInput,
                observed_at: Some(now),
                valid_window: None,
                fetched_at: now,
                quality: EvidenceQuality::Fresh,
                binding_proof: "caller_payload:invoke".into(),
                detail: Some(detail.clone()),
            });
        }
    }
}
