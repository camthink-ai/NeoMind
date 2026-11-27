//! Host assembly for the [`neomind_ai_agent`] runtime.
//!
//! Opens the shared `agents.redb` via [`AgentRecordStore`], loads a
//! [`DurableAgentStore`], and builds [`AgentApplicationService`] +
//! [`ExecutionCoordinator`] with production host ports (LLM / sources /
//! notify / side-effects).

use std::sync::Arc;

use async_trait::async_trait;
use neomind_ai_agent::coordinator::{
    AgentScheduler, AgentSubmitPort, ExecutionCoordinator, SubmitSource,
};
use neomind_ai_agent::error::{AgentError, Result as AgentResult};
use neomind_ai_agent::input::InputPipeline;
use neomind_ai_agent::model::{
    AgentExecutionRecord, AgentSpec, AgentSpecDraft, AgentSpecPatch, AgentStatus, CallerInput,
    EffectivePlan, ExecutionStatus, RunResponse, EXECUTION_ID_KEY,
};
use neomind_ai_agent::persist::{persist_spec, DurableAgentStore};
use neomind_ai_agent::ports::{ClockPort, NotifyPort, SourceReadPort, SystemClock};
use neomind_ai_agent::runner::{MemoryContext, SinglePassRunner};
use neomind_ai_agent::service::{AgentApplicationService, RunOptions};
use neomind_ai_agent::side_effects::{deps_fresh, Notifier, SideEffectBoundary};
use neomind_core::eventbus::EventBus;
use neomind_core::NeoMindEvent;
use neomind_storage::agent_store::AgentRecordStore;
use neomind_storage::agents::ExecutionRecord as JournalExecutionRecord;
use neomind_storage::{AgentStore, MarkdownMemoryStore};

use crate::agent_ports::{
    HostAgentDeps, HostLlmPort, HostNotifyPort, HostSideEffectPort, HostSourceReadPort,
};
use crate::automation::metric_publish::{json_to_core_metric_value, VirtualMetricPublisher};

/// Hosted clean-slate agent runtime (service + coordinator + durable store).
pub struct HostedAiAgentRuntime {
    pub service: Arc<AgentApplicationService>,
    pub coordinator: Arc<ExecutionCoordinator>,
    pub durable: DurableAgentStore,
    pub clock: Arc<dyn ClockPort>,
    /// Production ports retained for acceptance counters / diagnostics.
    pub sources: Arc<HostSourceReadPort>,
    pub notify: Arc<HostNotifyPort>,
    pub side_effects: Arc<HostSideEffectPort>,
    pub llm: Arc<HostLlmPort>,
    event_bus: Option<Arc<EventBus>>,
    metric_publisher: Option<Arc<VirtualMetricPublisher>>,
    memory_store: Option<Arc<MarkdownMemoryStore>>,
    scheduler: Arc<std::sync::Mutex<Option<Arc<AgentScheduler>>>>,
    started: std::sync::atomic::AtomicBool,
}

/// Schedules ticks through [`HostedAiAgentRuntime::submit`] so Cron/Interval
/// share `mirror_after_run` with manual/event/rule paths without creating an `Arc` cycle.
struct HostedSubmit(std::sync::Weak<HostedAiAgentRuntime>);

#[async_trait]
impl AgentSubmitPort for HostedSubmit {
    async fn submit(&self, agent_id: &str, source: SubmitSource) -> AgentResult<RunResponse> {
        let rt = self.0.upgrade().ok_or(AgentError::Cancelled)?;
        rt.submit(agent_id, source).await
    }
}

impl HostedAiAgentRuntime {
    /// Open [`AgentRecordStore`] on the shared `agents.redb` and build service with real host ports.
    pub fn build(agent_store: Arc<AgentStore>, deps: HostAgentDeps) -> AgentResult<Self> {
        let record_store = AgentRecordStore::from_arc(agent_store.db())
            .map_err(|e| AgentError::Storage(e.to_string()))?;
        let clock: Arc<dyn ClockPort> = Arc::new(SystemClock);
        let now = clock.now_ms();
        let durable = DurableAgentStore::with_store(record_store, now)?;

        let event_bus = deps.event_bus.clone();
        let metric_publisher = deps.metric_publisher.clone();
        let memory_store = deps.memory_store.clone();
        let sources = Arc::new(HostSourceReadPort::new(deps.clone()));
        let notify = Arc::new(HostNotifyPort::new(deps.message_manager.clone()));
        let writes = Arc::new(HostSideEffectPort::new(
            deps.device_service.clone(),
            deps.extension_registry.clone(),
        ));
        let mut llm_port = match deps.session_manager.clone() {
            Some(sm) => HostLlmPort::with_session_manager(sm),
            None => HostLlmPort::new(),
        };
        if let Some(ref ms) = memory_store {
            llm_port = llm_port.with_memory_store(ms.clone());
        }
        let llm = Arc::new(llm_port);

        let input = InputPipeline::new(sources.clone() as Arc<dyn SourceReadPort>, clock.clone());
        let runner = Arc::new(SinglePassRunner::new(llm.clone()));
        // `durable.mem` carries the execution persist sink (installed in
        // DurableAgentStore::with_store). Pre-send IntentPending save_execution
        // therefore hits the same agents.redb restart recovery opens — do not
        // replace with a bare InMemoryAgentStore::new().
        let side_effects = SideEffectBoundary {
            store: durable.mem.clone(),
            notifier: Notifier::new(notify.clone() as Arc<dyn NotifyPort>),
            side_effects: writes.clone(),
            clock: clock.clone(),
        };

        let service = Arc::new(AgentApplicationService {
            store: durable.mem.clone(),
            clock: clock.clone(),
            input,
            runner,
            side_effects,
            notify_counter: notify.clone() as Arc<dyn neomind_ai_agent::service::NotifyCounter>,
            write_counter: writes.clone() as Arc<dyn neomind_ai_agent::service::WriteCounter>,
        });

        let coordinator = Arc::new(ExecutionCoordinator::new(
            service.clone(),
            durable.mem.clone(),
            clock.clone(),
        ));

        Ok(Self {
            service,
            coordinator,
            durable,
            clock,
            sources,
            notify,
            side_effects: writes,
            llm,
            event_bus,
            metric_publisher,
            memory_store,
            scheduler: Arc::new(std::sync::Mutex::new(None)),
            started: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// Start the Cron/Interval scheduler (1s tick). Idempotent.
    pub fn start(self: &Arc<Self>) {
        if self.started.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let submit: Arc<dyn AgentSubmitPort> = Arc::new(HostedSubmit(Arc::downgrade(self)));
        let scheduler = Arc::new(AgentScheduler::new(
            submit,
            self.durable.mem.clone(),
            self.clock.clone(),
        ));
        *self.scheduler.lock().expect("scheduler") = Some(scheduler.clone());
        scheduler.start();
        tracing::info!("AgentScheduler started");
    }

    /// Stop the Cron/Interval scheduler if running. Idempotent.
    pub fn stop(&self) {
        if let Some(scheduler) = self.scheduler.lock().expect("scheduler").take() {
            scheduler.stop();
        }
        self.started
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn has_agent(&self, id: &str) -> bool {
        self.durable.mem.get_spec(id).is_ok()
    }

    /// Ensure `id` is in the hot set by loading from durable storage.
    pub fn ensure_loaded(&self, id: &str) -> AgentResult<()> {
        self.durable.ensure_loaded(id, self.clock.now_ms())
    }

    /// Force-refresh hot set from durable storage after a store write that bypassed
    /// [`Self::create`] / [`Self::update`].
    pub fn reload_from_store(&self, id: &str) -> AgentResult<()> {
        self.durable.reload_from_store(id, self.clock.now_ms())
    }

    /// Update hot-set + durable runtime status (Active/Paused/…), cancelling any
    /// in-flight run when transitioning to `Paused` or `Stopped`.
    pub fn set_agent_status(&self, id: &str, status: AgentStatus) -> AgentResult<()> {
        let rt = self.service.set_status(id, status)?;
        self.durable.mirror_runtime(&rt)
    }

    pub fn create(&self, draft: AgentSpecDraft) -> AgentResult<(AgentSpec, EffectivePlan)> {
        let (spec, plan) = self.service.create(draft)?;
        self.mirror_spec(&spec)?;
        Ok((spec, plan))
    }

    pub fn list(&self) -> Vec<(AgentSpec, EffectivePlan)> {
        self.service.list()
    }

    pub fn get(&self, id: &str) -> AgentResult<(AgentSpec, EffectivePlan)> {
        let spec = self.service.get(id)?;
        let plan = self.service.effective_plan(id)?;
        Ok((spec, plan))
    }

    /// Runtime status for Chat/list (no AiAgent map).
    pub fn runtime_status(&self, id: &str) -> Option<AgentStatus> {
        self.durable.mem.get_runtime(id).ok().map(|r| r.status)
    }

    /// Runtime stats snapshot for Chat/list DTOs.
    pub fn runtime_stats_json(&self, id: &str) -> serde_json::Value {
        match self.durable.mem.get_runtime(id) {
            Ok(rt) => serde_json::json!({
                "total_runs": rt.stats.total_runs,
                "successful_runs": rt.stats.successful_runs,
                "failed_runs": rt.stats.failed_runs,
                "partial_runs": rt.stats.partial_runs,
                "skipped_daily_limit": rt.stats.skipped_daily_limit,
                "skipped_permit_timeout": rt.stats.skipped_permit_timeout,
                "consecutive_failures": rt.stats.consecutive_failures,
                "runs_today": rt.stats.runs_today,
                "quota_day": rt.stats.quota_day,
                "last_execution_at": rt.stats.last_execution_at,
                "last_execution_id": rt.stats.last_execution_id,
                "avg_duration_ms": rt.stats.avg_duration_ms,
                "last_duration_ms": rt.stats.last_duration_ms,
                "error_message": rt.error_message,
            }),
            Err(_) => serde_json::json!({}),
        }
    }

    pub fn update(
        &self,
        id: &str,
        patch: AgentSpecPatch,
    ) -> AgentResult<(AgentSpec, EffectivePlan)> {
        let (spec, plan) = self.service.update(id, patch)?;
        self.mirror_spec(&spec)?;
        Ok((spec, plan))
    }

    pub fn delete(&self, id: &str) -> AgentResult<()> {
        self.coordinator.state.cancel(id);
        self.durable.delete_spec(id)
    }

    pub async fn invoke(&self, id: &str) -> AgentResult<RunResponse> {
        self.invoke_with_caller(id, None).await
    }

    /// Manual invoke with optional this-turn ask text / structured payload.
    pub async fn invoke_with_caller(
        &self,
        id: &str,
        caller: Option<CallerInput>,
    ) -> AgentResult<RunResponse> {
        self.run_formal(id, SubmitSource::Manual, caller, None).await
    }

    pub async fn dry_run(&self, id: &str) -> AgentResult<RunResponse> {
        self.dry_run_with_caller(id, None).await
    }

    /// Ephemeral read-only dry-run directly on an unpersisted [`AgentSpec`]
    /// without creating or writing anything to `agents.redb` or `InMemoryAgentStore`.
    pub async fn dry_run_ephemeral(&self, spec: AgentSpec) -> AgentResult<RunResponse> {
        self.service.dry_run_spec(spec, RunOptions::default()).await
    }

    /// Authorized read-only dry-run with optional this-turn caller text / structured payload.
    pub async fn dry_run_with_caller(
        &self,
        id: &str,
        caller: Option<CallerInput>,
    ) -> AgentResult<RunResponse> {
        let operator_notes = self.load_operator_notes(id);
        let memory_context = self.load_memory_context(id);
        let resp = self
            .service
            .dry_run_with_options(
                id,
                RunOptions {
                    execution_id: None,
                    caller,
                    operator_notes,
                    memory_context,
                },
            )
            .await?;
        Ok(resp)
    }

    pub async fn submit(&self, id: &str, source: SubmitSource) -> AgentResult<RunResponse> {
        self.run_formal(id, source, None, None).await
    }

    pub async fn submit_with_caller(
        &self,
        id: &str,
        source: SubmitSource,
        caller: Option<CallerInput>,
    ) -> AgentResult<RunResponse> {
        self.run_formal(id, source, caller, None).await
    }

    pub async fn submit_with_caller_and_id(
        &self,
        id: &str,
        source: SubmitSource,
        caller: Option<CallerInput>,
        execution_id: Option<String>,
    ) -> AgentResult<RunResponse> {
        self.run_formal(id, source, caller, execution_id).await
    }

    async fn run_formal(
        &self,
        id: &str,
        source: SubmitSource,
        caller: Option<CallerInput>,
        preallocated_execution_id: Option<String>,
    ) -> AgentResult<RunResponse> {
        let started_ms = self.clock.now_ms();
        let execution_id = preallocated_execution_id
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("exec_{}", &uuid::Uuid::new_v4().to_string()[..8]));
        let operator_notes = self.load_operator_notes(id);
        let memory_context = self.load_memory_context(id);

        let mut emitted_started = false;
        if let Some(bus) = &self.event_bus {
            if let Ok(spec) = self.durable.mem.get_spec(id) {
                let runnable = self
                    .durable
                    .mem
                    .get_runtime(id)
                    .map(|rt| !matches!(rt.status, AgentStatus::Paused | AgentStatus::Stopped))
                    .unwrap_or(false);
                if runnable {
                    let _ = bus
                        .publish(NeoMindEvent::AgentExecutionStarted {
                            agent_id: id.to_string(),
                            agent_name: spec.name,
                            execution_id: execution_id.clone(),
                            trigger_type: source.as_str().to_string(),
                            timestamp: started_ms / 1000,
                        })
                        .await;
                    emitted_started = true;
                }
            }
        }

        let res = self
            .coordinator
            .submit_with_options(
                id,
                source,
                RunOptions {
                    execution_id: Some(execution_id.clone()),
                    caller,
                    operator_notes,
                    memory_context,
                },
            )
            .await;

        match res {
            Ok(resp) => {
                self.mirror_after_run(id, &resp).await?;
                Ok(resp)
            }
            Err(e) => {
                if emitted_started {
                    if let Some(bus) = &self.event_bus {
                        let now_ms = self.clock.now_ms();
                        let _ = bus
                            .publish(NeoMindEvent::AgentExecutionCompleted {
                                agent_id: id.to_string(),
                                execution_id,
                                success: false,
                                duration_ms: now_ms.saturating_sub(started_ms).max(0) as u64,
                                error: Some(e.to_string()),
                                timestamp: now_ms / 1000,
                            })
                            .await;
                    }
                }
                Err(e)
            }
        }
    }

    fn load_operator_notes(&self, id: &str) -> Vec<String> {
        let Some(store) = &self.durable.store else {
            return Vec::new();
        };
        let Ok(Some(msgs)) = store.get_user_messages(id) else {
            return Vec::new();
        };
        if msgs.is_empty() {
            return Vec::new();
        }
        let start = msgs.len().saturating_sub(20);
        msgs[start..]
            .iter()
            .filter_map(|m| {
                let text = m.content.trim();
                if text.is_empty() {
                    return None;
                }
                match m.message_type.as_deref() {
                    Some(tag) if !tag.trim().is_empty() => {
                        Some(format!("[{}] {}", tag.trim(), text))
                    }
                    _ => Some(text.to_string()),
                }
            })
            .collect()
    }

    /// Load recent execution journal entries, sliding multi-turn task history, and
    /// agent knowledge files from durable storage into a token-bounded [`MemoryContext`].
    pub(crate) fn load_memory_context(&self, id: &str) -> MemoryContext {
        const MAX_JOURNAL_ENTRIES: usize = 5;
        const MAX_ENTRY_CHARS: usize = 240;
        const MAX_FILE_CHARS: usize = 1200;
        const MAX_TOTAL_KNOWLEDGE_CHARS: usize = 3000;
        const MAX_HISTORY_TURNS: usize = 6;
        const MAX_HISTORY_CHARS: usize = 4800;

        let mut ctx = MemoryContext::default();
        let mut seen_files = std::collections::HashSet::new();
        let mut remaining_budget = MAX_TOTAL_KNOWLEDGE_CHARS;

        // Build structured Chat-style multi-turn history from recent formal executions
        // (oldest -> newest, bounded by MAX_HISTORY_TURNS and MAX_HISTORY_CHARS).
        ctx.prior_turns = self.load_recent_conversation_turns(id, MAX_HISTORY_TURNS, MAX_HISTORY_CHARS);

        if let Some(store) = &self.durable.store {
            if let Ok(Some(memory)) = store.get_memory(id) {
                let start = memory
                    .journal
                    .records
                    .len()
                    .saturating_sub(MAX_JOURNAL_ENTRIES);
                for rec in &memory.journal.records[start..] {
                    let ts = chrono::DateTime::from_timestamp(rec.timestamp, 0)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
                        .unwrap_or_else(|| rec.timestamp.to_string());
                    let status = if rec.success { "OK" } else { "FAIL" };
                    let outcome = truncate_chars(rec.outcome.trim(), MAX_ENTRY_CHARS);
                    if outcome.is_empty() {
                        continue;
                    }
                    let line = if rec.action_taken.trim().is_empty() {
                        format!("[{ts}] [{status}] {outcome}")
                    } else {
                        let action = truncate_chars(rec.action_taken.trim(), 80);
                        format!("[{ts}] [{status}] {outcome} (action: {action})")
                    };
                    ctx.journal_entries.push(line);
                }

                if let Some(mem_store) = &self.memory_store {
                    for f in &memory.knowledge_files {
                        if remaining_budget == 0 {
                            break;
                        }
                        seen_files.insert(f.name.clone());
                        if let Ok(raw) = mem_store.read_agent_custom_file(id, &f.name) {
                            let trimmed = raw.trim();
                            if trimmed.is_empty() {
                                continue;
                            }
                            let cap = remaining_budget.min(MAX_FILE_CHARS);
                            let content = truncate_chars(trimmed, cap);
                            remaining_budget =
                                remaining_budget.saturating_sub(content.chars().count());
                            ctx.knowledge_files.push((
                                f.name.clone(),
                                f.description.clone(),
                                content,
                            ));
                        }
                    }
                }
            }
        }

        // Also surface any markdown files placed directly in the agent's memory
        // directory (e.g. written by tools or API) even if not yet indexed in redb.
        if remaining_budget > 0 {
            if let Some(mem_store) = &self.memory_store {
                if let Ok(files) = mem_store.list_agent_custom_files(id) {
                    for (name, _) in files {
                        if remaining_budget == 0 {
                            break;
                        }
                        if seen_files.contains(&name) {
                            continue;
                        }
                        if let Ok(raw) = mem_store.read_agent_custom_file(id, &name) {
                            let trimmed = raw.trim();
                            if trimmed.is_empty() {
                                continue;
                            }
                            let cap = remaining_budget.min(MAX_FILE_CHARS);
                            let content = truncate_chars(trimmed, cap);
                            remaining_budget =
                                remaining_budget.saturating_sub(content.chars().count());
                            ctx.knowledge_files.push((name, String::new(), content));
                        }
                    }
                }
            }
        }

        ctx
    }

    /// Build bounded multi-turn `(user, assistant)` history from recent formal runs
    /// so the agent carries true Chat-style task continuity across executions.
    fn load_recent_conversation_turns(
        &self,
        id: &str,
        max_turns: usize,
        max_total_chars: usize,
    ) -> Vec<neomind_ai_agent::ports::ConversationTurn> {
        let mut raw_turns: Vec<neomind_ai_agent::ports::ConversationTurn> = Vec::new();

        // Prefer in-memory formal history (populated during this process lifetime),
        // falling back to durable redb rows after a server restart.
        let mem_hist = self.durable.mem.formal_history(id);
        if !mem_hist.is_empty() {
            let start = mem_hist.len().saturating_sub(max_turns);
            for exec in &mem_hist[start..] {
                if let Some(turn) = turn_from_exec_record(exec) {
                    raw_turns.push(turn);
                }
            }
        } else if let Some(store) = &self.durable.store {
            if let Ok(mut rows) = store.list_formal_executions(id) {
                rows.sort_by_key(|r| r.finished_at);
                let start = rows.len().saturating_sub(max_turns);
                for row in &rows[start..] {
                    if let Some(turn) = turn_from_storage_row(row) {
                        raw_turns.push(turn);
                    }
                }
            }
        }

        // Enforce total char budget by keeping the newest turns first.
        let mut kept: Vec<neomind_ai_agent::ports::ConversationTurn> = Vec::new();
        let mut used = 0usize;
        for turn in raw_turns.into_iter().rev() {
            let sz = turn.user.chars().count() + turn.assistant.chars().count();
            if used + sz > max_total_chars && !kept.is_empty() {
                break;
            }
            used += sz;
            kept.push(turn);
        }
        kept.reverse();
        kept
    }

    pub fn explain_last(&self, id: &str) -> AgentResult<String> {
        self.service.explain_last(id)
    }

    /// Latest formal execution (includes `structured_result` when present).
    pub fn latest_formal(&self, id: &str) -> Option<neomind_ai_agent::model::AgentExecutionRecord> {
        self.service.latest_formal(id)
    }

    fn mirror_spec(&self, spec: &AgentSpec) -> AgentResult<()> {
        if let Some(store) = &self.durable.store {
            let rt = self.durable.mem.get_runtime(&spec.id)?;
            persist_spec(store, spec, &rt)?;
        }
        Ok(())
    }

    async fn mirror_after_run(&self, id: &str, resp: &RunResponse) -> AgentResult<()> {
        if let Some(exec) = &resp.execution {
            if !exec.dry_run {
                self.durable.mirror_execution(exec)?;
                if let Ok(rt) = self.durable.mem.get_runtime(id) {
                    self.durable.mirror_runtime(&rt)?;
                }
                self.append_journal(exec);
                if let Ok(spec) = self.durable.mem.get_spec(id) {
                    self.publish_structured_outputs(&spec, exec).await;
                }
                if let Some(bus) = &self.event_bus {
                    let duration_ms =
                        exec.finished_at.saturating_sub(exec.started_at).max(0) as u64;
                    let success = matches!(
                        exec.status,
                        ExecutionStatus::Completed | ExecutionStatus::Partial
                    );
                    let _ = bus
                        .publish(NeoMindEvent::AgentExecutionCompleted {
                            agent_id: exec.agent_id.clone(),
                            execution_id: exec.id.clone(),
                            success,
                            duration_ms,
                            error: exec.error.clone(),
                            timestamp: exec.finished_at / 1000,
                        })
                        .await;
                }
            }
        }
        Ok(())
    }

    fn append_journal(&self, exec: &AgentExecutionRecord) {
        let Some(store) = &self.durable.store else {
            return;
        };
        let success = matches!(
            exec.status,
            ExecutionStatus::Completed | ExecutionStatus::Partial
        );
        let raw_outcome = exec
            .conclusion
            .as_deref()
            .or(exec.brief_text.as_deref())
            .or(exec.error.as_deref())
            .unwrap_or(match exec.status {
                ExecutionStatus::Completed => "completed",
                ExecutionStatus::Partial => "partial",
                ExecutionStatus::Skipped => "skipped",
                ExecutionStatus::Failed => "failed",
            });
        let outcome = truncate_chars(raw_outcome, 300);
        let actions: Vec<String> = exec
            .deliveries
            .iter()
            .take(5)
            .map(|d| {
                let s = format!(
                    "{} -> {} ({:?})",
                    d.action_fingerprint, d.destination, d.state
                );
                truncate_chars(&s, 150)
            })
            .collect();
        let action_taken = if actions.is_empty() {
            "no action".to_string()
        } else {
            actions.join("; ")
        };
        let stop_reason = match exec.status {
            ExecutionStatus::Completed => "completed",
            ExecutionStatus::Partial => "partial",
            ExecutionStatus::Skipped => "skipped",
            ExecutionStatus::Failed => "failed",
        }
        .to_string();
        let record = JournalExecutionRecord {
            timestamp: exec.finished_at / 1000,
            execution_id: exec.id.clone(),
            outcome,
            action_taken,
            success,
            stop_reason,
        };
        if let Err(e) = store.append_journal_entry(&exec.agent_id, record) {
            tracing::warn!(
                agent_id = %exec.agent_id,
                execution_id = %exec.id,
                error = %e,
                "Failed to append V2 execution journal entry"
            );
        }
    }

    async fn publish_structured_outputs(&self, spec: &AgentSpec, exec: &AgentExecutionRecord) {
        if !spec.output.publish || spec.output.fields.is_empty() {
            return;
        }
        if !matches!(
            exec.status,
            ExecutionStatus::Completed | ExecutionStatus::Partial
        ) {
            return;
        }
        let Some(publisher) = &self.metric_publisher else {
            return;
        };
        let Some(obj) = exec.structured_result.as_ref().and_then(|v| v.as_object()) else {
            return;
        };
        let confidence = obj
            .get("confidence")
            .and_then(|v| v.as_f64())
            .map(|f| f as f32);
        let primary_ns = format!("ai:{}", spec.id);
        let ts_secs = exec.finished_at / 1000;
        let meta = Some(serde_json::json!({ EXECUTION_ID_KEY: exec.id }));

        for field in &spec.output.fields {
            if !field.depends_on.is_empty() && !deps_fresh(&exec.evidence, &field.depends_on) {
                tracing::debug!(
                    agent_id = %spec.id,
                    field = %field.name,
                    "Skipping ai:* publish because field depends_on evidence is not fresh"
                );
                continue;
            }
            if let Some(val_json) = obj.get(&field.name) {
                if val_json.is_null() {
                    continue;
                }
                let metric_val = json_to_core_metric_value(val_json);
                publisher
                    .publish_virtual_metric_with_metadata(
                        &primary_ns,
                        None,
                        &field.name,
                        &metric_val,
                        ts_secs,
                        confidence,
                        Some(("ai", spec.id.as_str())),
                        meta.clone(),
                    )
                    .await;
            }
        }
    }
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let out: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        out
    } else {
        s.to_string()
    }
}

fn turn_from_exec_record(
    exec: &AgentExecutionRecord,
) -> Option<neomind_ai_agent::ports::ConversationTurn> {
    if exec.dry_run || matches!(exec.status, ExecutionStatus::Skipped) {
        return None;
    }
    let ts_secs = if exec.finished_at >= 1_000_000_000_000 {
        exec.finished_at / 1000
    } else {
        exec.finished_at
    };
    let ts = chrono::DateTime::from_timestamp(ts_secs, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| ts_secs.to_string());

    let mut input_parts: Vec<String> = Vec::new();
    for item in &exec.evidence.items {
        let brief = match &item.detail {
            Some(neomind_ai_agent::model::EvidenceDetail::Structured { value }) => {
                // Reuse prompt_builder sanitization indirectly via compact summary (no base64).
                let ev = neomind_ai_agent::model::EvidenceReport {
                    items: vec![item.clone()],
                    missing_keys: vec![],
                    skip_reason: None,
                };
                let sanitized = neomind_ai_agent::runner::build_evidence_prompt("", &ev);
                sanitized
                    .lines()
                    .find(|l| l.starts_with("- "))
                    .map(|l| truncate_chars(l.trim_start_matches("- "), 180))
                    .unwrap_or_else(|| truncate_chars(&value.to_string(), 120))
            }
            Some(neomind_ai_agent::model::EvidenceDetail::Text { text }) => {
                format!("{}: {}", item.key, truncate_chars(text, 120))
            }
            Some(neomind_ai_agent::model::EvidenceDetail::TimeSeries { points, .. }) => {
                format!("{}: {points} points", item.key)
            }
            Some(neomind_ai_agent::model::EvidenceDetail::Vision {
                image_source_field, ..
            }) => format!("{}: vision({image_source_field})", item.key),
            None => format!("{}: missing", item.key),
        };
        input_parts.push(brief);
    }

    let user = if input_parts.is_empty() {
        format!("[Prior run {ts} ({})]", exec.id)
    } else {
        format!(
            "[Prior run {ts} ({})]\nInputs: {}",
            exec.id,
            truncate_chars(&input_parts.join("; "), 360)
        )
    };

    let mut asst_parts: Vec<String> = Vec::new();
    if !exec.tool_calls.is_empty() {
        let tool_list: Vec<String> = exec
            .tool_calls
            .iter()
            .take(6)
            .map(|t| format!("{}({})", t.name, if t.success { "ok" } else { "err" }))
            .collect();
        asst_parts.push(format!("Tools used: {}", tool_list.join(", ")));
    }
    let has_structured = exec
        .structured_result
        .as_ref()
        .is_some_and(|sr| !sr.as_object().is_some_and(|m| m.is_empty()));
    if let Some(sr) = &exec.structured_result {
        if has_structured {
            asst_parts.push(format!("Output: {}", truncate_chars(&sr.to_string(), 240)));
        }
    }
    // Prefer verifiable structured Output over free-form prose to minimize
    // multi-turn hallucination drift; fall back to conclusion when unstructured.
    if !has_structured {
        if let Some(c) = exec
            .conclusion
            .as_deref()
            .or(exec.brief_text.as_deref())
            .or(exec.error.as_deref())
        {
            let trimmed = c.trim();
            if !trimmed.is_empty() {
                asst_parts.push(format!("Result: {}", truncate_chars(trimmed, 260)));
            }
        }
    }
    if asst_parts.is_empty() {
        return None;
    }

    Some(neomind_ai_agent::ports::ConversationTurn {
        user,
        assistant: asst_parts.join("\n"),
    })
}

fn turn_from_storage_row(
    row: &neomind_storage::agent_store::AgentExecutionRow,
) -> Option<neomind_ai_agent::ports::ConversationTurn> {
    if row.dry_run
        || matches!(
            row.status,
            neomind_storage::agent_store::AgentExecutionStatus::Skipped
        )
    {
        return None;
    }
    let ts_secs = if row.finished_at >= 1_000_000_000_000 {
        row.finished_at / 1000
    } else {
        row.finished_at
    };
    let ts = chrono::DateTime::from_timestamp(ts_secs, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| ts_secs.to_string());

    // Mirror `turn_from_exec_record` by extracting compact input summaries from
    // persisted `row.evidence` so pre-restart and post-restart turns match.
    let mut input_parts: Vec<String> = Vec::new();
    if let Ok(evidence) =
        serde_json::from_value::<neomind_ai_agent::model::EvidenceReport>(row.evidence.clone())
    {
        for item in &evidence.items {
            let brief = match &item.detail {
                Some(neomind_ai_agent::model::EvidenceDetail::Structured { value }) => {
                    let ev = neomind_ai_agent::model::EvidenceReport {
                        items: vec![item.clone()],
                        missing_keys: vec![],
                        skip_reason: None,
                    };
                    let sanitized = neomind_ai_agent::runner::build_evidence_prompt("", &ev);
                    sanitized
                        .lines()
                        .find(|l| l.starts_with("- "))
                        .map(|l| truncate_chars(l.trim_start_matches("- "), 180))
                        .unwrap_or_else(|| truncate_chars(&value.to_string(), 120))
                }
                Some(neomind_ai_agent::model::EvidenceDetail::Text { text }) => {
                    format!("{}: {}", item.key, truncate_chars(text, 120))
                }
                Some(neomind_ai_agent::model::EvidenceDetail::TimeSeries { points, .. }) => {
                    format!("{}: {points} points", item.key)
                }
                Some(neomind_ai_agent::model::EvidenceDetail::Vision {
                    image_source_field,
                    ..
                }) => format!("{}: vision({image_source_field})", item.key),
                None => format!("{}: missing", item.key),
            };
            input_parts.push(brief);
        }
    }

    let user = if input_parts.is_empty() {
        format!("[Prior run {ts} ({})]", row.id)
    } else {
        format!(
            "[Prior run {ts} ({})]\nInputs: {}",
            row.id,
            truncate_chars(&input_parts.join("; "), 360)
        )
    };
    let mut asst_parts: Vec<String> = Vec::new();
    if !row.tool_calls.is_empty() {
        let tool_list: Vec<String> = row
            .tool_calls
            .iter()
            .take(6)
            .map(|t| format!("{}({})", t.name, if t.success { "ok" } else { "err" }))
            .collect();
        asst_parts.push(format!("Tools used: {}", tool_list.join(", ")));
    }
    let has_structured = row
        .structured_result
        .as_ref()
        .is_some_and(|sr| !sr.as_object().is_some_and(|m| m.is_empty()));
    if let Some(sr) = &row.structured_result {
        if has_structured {
            asst_parts.push(format!("Output: {}", truncate_chars(&sr.to_string(), 240)));
        }
    }
    if !has_structured {
        if let Some(c) = row
            .conclusion
            .as_deref()
            .or(row.brief_text.as_deref())
            .or(row.error.as_deref())
        {
            let trimmed = c.trim();
            if !trimmed.is_empty() {
                asst_parts.push(format!("Result: {}", truncate_chars(trimmed, 260)));
            }
        }
    }
    if asst_parts.is_empty() {
        return None;
    }
    Some(neomind_ai_agent::ports::ConversationTurn {
        user,
        assistant: asst_parts.join("\n"),
    })
}

/// Convenience wrapper used by ServerState construction.
pub fn build_hosted_runtime(
    agent_store: Arc<AgentStore>,
    deps: HostAgentDeps,
) -> AgentResult<Arc<HostedAiAgentRuntime>> {
    let runtime = Arc::new(HostedAiAgentRuntime::build(agent_store, deps)?);
    Ok(runtime)
}

#[cfg(test)]
mod tests {
    use super::*;
    use neomind_ai_agent::model::{
        ControlledAction, EvidenceQuality, ExecutionStatus, FreshnessPolicy, InputContract,
        InputModality, InputRequirement, InputSourceRef, MissingPolicy, NotifyPolicy,
        OutputContract, PacingPolicy, ToolPolicy, TriggerSpec,
    };
    use neomind_ai_agent::ports::FixedClock;
    use neomind_ai_agent::runner::single_pass::LOCAL_BRIEF_BACKEND;
    use neomind_core::extension::system::{ExtensionCommand, ExtensionMetadata, MetricDefinition};
    use neomind_core::extension::{
        CommandEffect, CommandExecutionKind, CommandReplaySafety, Extension, ExtensionRegistry,
    };
    use neomind_core::EventBus;
    use neomind_devices::telemetry::DataPoint;
    use neomind_devices::{DeviceRegistry, DeviceService, MetricValue, TimeSeriesStorage};
    use neomind_messages::channels::MessageChannel;
    use neomind_messages::{MessageManager, Result as MsgResult};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};

    use crate::server::state::ExtensionMetricsStorage;

    struct CountingChannel {
        name: String,
        sends: AtomicU64,
    }

    #[async_trait]
    impl MessageChannel for CountingChannel {
        fn name(&self) -> &str {
            &self.name
        }
        fn channel_type(&self) -> &str {
            "test"
        }
        fn is_enabled(&self) -> bool {
            true
        }
        async fn send(&self, _message: &neomind_messages::Message) -> MsgResult<()> {
            self.sends.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    /// Fake extension: `query` is ReadOnly, `set` is Mutating — counts set calls.
    fn fake_rw_extension(set_calls: Arc<AtomicU64>) -> neomind_core::extension::DynExtension {
        struct FakeRw {
            set_calls: Arc<AtomicU64>,
        }
        #[async_trait]
        impl Extension for FakeRw {
            fn metadata(&self) -> &ExtensionMetadata {
                static M: OnceLock<ExtensionMetadata> = OnceLock::new();
                M.get_or_init(|| ExtensionMetadata::new("rw-ext", "RW Ext", "1.0.0"))
            }
            fn metrics(&self) -> Vec<MetricDefinition> {
                vec![]
            }
            fn commands(&self) -> Vec<ExtensionCommand> {
                vec![
                    ExtensionCommand {
                        name: "query".into(),
                        display_name: "query".into(),
                        description: String::new(),
                        payload_template: String::new(),
                        parameters: vec![],
                        fixed_values: Default::default(),
                        samples: vec![],
                        parameter_groups: vec![],
                        ai_exposed: true,
                        effect: Some(CommandEffect::ReadOnly),
                        replay_safety: Some(CommandReplaySafety::Safe),
                        execution: Some(CommandExecutionKind::Immediate),
                    },
                    ExtensionCommand {
                        name: "set".into(),
                        display_name: "set".into(),
                        description: String::new(),
                        payload_template: String::new(),
                        parameters: vec![],
                        fixed_values: Default::default(),
                        samples: vec![],
                        parameter_groups: vec![],
                        ai_exposed: true,
                        effect: Some(CommandEffect::Mutating),
                        replay_safety: Some(CommandReplaySafety::Unsafe),
                        execution: Some(CommandExecutionKind::Immediate),
                    },
                ]
            }
            async fn execute_command(
                &self,
                command: &str,
                _args: &serde_json::Value,
            ) -> neomind_core::extension::Result<serde_json::Value> {
                if command == "set" {
                    self.set_calls.fetch_add(1, Ordering::SeqCst);
                }
                Ok(serde_json::json!({ "ok": true, "cmd": command }))
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }
        Arc::new(tokio::sync::RwLock::new(Box::new(FakeRw { set_calls })))
    }

    /// Readonly weather extension whose `current` payload is swapped per test.
    fn fake_weather_extension(
        id: &str,
        payload: Arc<Mutex<serde_json::Value>>,
    ) -> neomind_core::extension::DynExtension {
        struct FakeWeather {
            metadata: ExtensionMetadata,
            payload: Arc<Mutex<serde_json::Value>>,
        }
        #[async_trait]
        impl Extension for FakeWeather {
            fn metadata(&self) -> &ExtensionMetadata {
                &self.metadata
            }
            fn metrics(&self) -> Vec<MetricDefinition> {
                vec![]
            }
            fn commands(&self) -> Vec<ExtensionCommand> {
                vec![ExtensionCommand {
                    name: "current".into(),
                    display_name: "current".into(),
                    description: String::new(),
                    payload_template: String::new(),
                    parameters: vec![],
                    fixed_values: Default::default(),
                    samples: vec![],
                    parameter_groups: vec![],
                    ai_exposed: true,
                    effect: Some(CommandEffect::ReadOnly),
                    replay_safety: Some(CommandReplaySafety::Safe),
                    execution: Some(CommandExecutionKind::Immediate),
                }]
            }
            async fn execute_command(
                &self,
                _command: &str,
                _args: &serde_json::Value,
            ) -> neomind_core::extension::Result<serde_json::Value> {
                Ok(self.payload.lock().unwrap().clone())
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }
        Arc::new(tokio::sync::RwLock::new(Box::new(FakeWeather {
            metadata: ExtensionMetadata::new(id, "Weather", "1.0.0"),
            payload,
        })))
    }

    async fn test_deps() -> (HostAgentDeps, Arc<CountingChannel>, Arc<TimeSeriesStorage>) {
        let mm = Arc::new(MessageManager::new());
        let ch = Arc::new(CountingChannel {
            name: "ops".into(),
            sends: AtomicU64::new(0),
        });
        mm.channels().await.read().await.register(ch.clone()).await;

        let telemetry = Arc::new(TimeSeriesStorage::memory().unwrap());
        let now = chrono::Utc::now().timestamp_millis();
        telemetry
            .write(
                "device:meter-1",
                "kw",
                DataPoint::new(now, MetricValue::Float(1.5)),
            )
            .await
            .unwrap();

        let bus = EventBus::new();
        let registry = Arc::new(DeviceRegistry::new());
        let device_service = Arc::new(DeviceService::new(registry, bus));
        device_service
            .set_telemetry_storage(telemetry.clone())
            .await;

        let ext_reg = Arc::new(ExtensionRegistry::new());
        let ext_metrics = Arc::new(ExtensionMetricsStorage::with_shared_storage(
            telemetry.clone(),
        ));

        (
            HostAgentDeps::new(mm, telemetry.clone(), device_service, ext_reg, ext_metrics),
            ch,
            telemetry,
        )
    }

    fn local_brief_draft(name: &str) -> AgentSpecDraft {
        AgentSpecDraft {
            name: name.into(),
            description: None,
            user_prompt: "report".into(),
            llm_backend_id: Some(LOCAL_BRIEF_BACKEND.into()),
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract::default(),
            tool_policy: ToolPolicy::Disabled,
            output: OutputContract::default(),
            notify: NotifyPolicy::default(),
            controlled_actions: vec![],
        }
    }

    #[tokio::test]
    async fn prod_builder_isolated_sinks_count_real_deliveries_and_reads() {
        let (deps, channel, _telemetry) = test_deps().await;
        let store = AgentStore::memory().unwrap();
        let runtime = HostedAiAgentRuntime::build(store, deps).unwrap();

        let mut draft = local_brief_draft("sink-check");
        draft.input = InputContract {
            requirements: vec![InputRequirement {
                key: "kw".into(),
                source: InputSourceRef::DeviceMetric {
                    device_id: "meter-1".into(),
                    field: "kw".into(),
                },
                operation: None,
                params: serde_json::json!({}),
                auth_scope: None,
                timeout_secs: 5,
                result_type: InputModality::Metric,
                freshness: FreshnessPolicy::Any,
                on_missing: MissingPolicy::ReportStale,
            }],
        };
        draft.notify = NotifyPolicy {
            channels: vec!["ops".into()],
            on_complete: true,
            on_alert: false,
            alert_depends_on: vec![],
            on_failure: true,
            pending_confirmation: false,
        };

        let (spec, _) = runtime.create(draft).unwrap();
        let resp = runtime.invoke(&spec.id).await.unwrap();
        assert!(resp.execution.is_some());
        assert!(runtime.sources.reads() >= 1);
        assert!(runtime.notify.deliveries() >= 1);
        assert!(channel.sends.load(Ordering::SeqCst) >= 1);
        assert_eq!(runtime.llm.requests(), 0, "local-brief skips HostLlmPort");
    }

    #[tokio::test]
    async fn dry_run_refuses_mutating_extension_ops_set_count_zero() {
        let set_calls = Arc::new(AtomicU64::new(0));
        let (mut deps, _, _) = test_deps().await;
        let ext_reg = Arc::new(ExtensionRegistry::new());
        ext_reg
            .register("rw-ext".into(), fake_rw_extension(set_calls.clone()))
            .await
            .unwrap();
        deps.extension_registry = ext_reg;

        let store = AgentStore::memory().unwrap();
        let runtime = HostedAiAgentRuntime::build(store, deps).unwrap();

        let mut draft = local_brief_draft("dry-ro");
        draft.input = InputContract {
            requirements: vec![
                InputRequirement {
                    key: "q".into(),
                    source: InputSourceRef::ExtensionInstance {
                        id: "rw-ext".into(),
                    },
                    operation: Some("query".into()),
                    params: serde_json::json!({}),
                    auth_scope: Some("extension:rw-ext:read".into()),
                    timeout_secs: 5,
                    result_type: InputModality::StructuredJson,
                    freshness: FreshnessPolicy::Any,
                    on_missing: MissingPolicy::ReportStale,
                },
                InputRequirement {
                    key: "w".into(),
                    source: InputSourceRef::ExtensionInstance {
                        id: "rw-ext".into(),
                    },
                    operation: Some("set".into()),
                    params: serde_json::json!({ "v": 1 }),
                    auth_scope: Some("extension:rw-ext:write".into()),
                    timeout_secs: 5,
                    result_type: InputModality::StructuredJson,
                    freshness: FreshnessPolicy::Any,
                    on_missing: MissingPolicy::ReportStale,
                },
            ],
        };

        let (spec, _) = runtime.create(draft).unwrap();
        let dry = runtime.dry_run(&spec.id).await.unwrap();
        let exec = dry.execution.as_ref().unwrap();
        let q = exec.evidence.items.iter().find(|i| i.key == "q").unwrap();
        assert!(
            matches!(q.quality, EvidenceQuality::Fresh),
            "query ReadOnly must succeed: {:?}",
            q.quality
        );
        let w = exec.evidence.items.iter().find(|i| i.key == "w").unwrap();
        assert!(
            !matches!(w.quality, EvidenceQuality::Fresh),
            "set must not succeed on input path: {:?}",
            w.quality
        );
        assert_eq!(
            set_calls.load(Ordering::SeqCst),
            0,
            "mutating set must never reach the extension receiver"
        );
    }

    #[tokio::test]
    async fn backend_5xx_settles_failed_not_fake_success() {
        let (deps, _, _) = test_deps().await;
        let store = AgentStore::memory().unwrap();
        let runtime = HostedAiAgentRuntime::build(store, deps).unwrap();
        runtime.llm.force_fail_next.store(true, Ordering::SeqCst);

        // Model task (not local-brief) — must call HostLlmPort and fail closed.
        let mut draft = local_brief_draft("need-model");
        draft.llm_backend_id = None;
        draft.notify = NotifyPolicy {
            channels: vec!["ops".into()],
            on_complete: true,
            on_alert: false,
            alert_depends_on: vec![],
            on_failure: false,
            pending_confirmation: false,
        };
        let (spec, _) = runtime.create(draft).unwrap();
        let resp = runtime.invoke(&spec.id).await.unwrap();
        let exec = resp.execution.as_ref().unwrap();
        assert_eq!(exec.status, ExecutionStatus::Failed);
        assert!(exec.brief_text.is_none() || exec.error.is_some());
        assert_eq!(runtime.llm.requests(), 1);
        assert_eq!(
            runtime.notify.deliveries(),
            0,
            "failed inference must not notify on_complete"
        );
    }

    #[tokio::test]
    async fn stale_weather_blocks_weather_dependent_action() {
        let (deps, _, telemetry) = test_deps().await;
        let store = AgentStore::memory().unwrap();
        let runtime = HostedAiAgentRuntime::build(store, deps).unwrap();
        let now = chrono::Utc::now().timestamp_millis();

        // Current meter window: several points in last 15m (Unix seconds —
        // matches MQTT/webhook/API telemetry writers).
        let now_s = now / 1000;
        let meter_points: Vec<_> = (0..5u64)
            .map(|i| {
                DataPoint::new(
                    now_s.saturating_sub((i as i64) * 60),
                    MetricValue::Float(8.0 + i as f64),
                )
            })
            .collect();
        telemetry
            .write_batch("device:meter-1", "power_kw", meter_points)
            .await
            .unwrap();
        // Sanity: second-unit range query must see the window.
        let probe = telemetry
            .query_limited(
                "device:meter-1",
                "power_kw",
                now_s - 900,
                now_s + 1,
                Some(60),
            )
            .await
            .unwrap();
        assert!(
            probe.len() >= 2,
            "precondition: meter window must have >=2 points, got {}",
            probe.len()
        );
        // Old weather cache (3h ago) beyond LatestWithin 1800s.
        telemetry
            .write(
                "extension:weather_cn",
                "current",
                DataPoint::new(now_s - 3 * 3_600, MetricValue::Float(30.0)),
            )
            .await
            .unwrap();
        let _ = telemetry.flush();

        let draft = AgentSpecDraft {
            name: "stale-weather".into(),
            description: None,
            user_prompt: "diagnose".into(),
            llm_backend_id: Some(LOCAL_BRIEF_BACKEND.into()),
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract {
                requirements: vec![
                    InputRequirement {
                        key: "meter_15m".into(),
                        source: InputSourceRef::DeviceMetric {
                            device_id: "meter-1".into(),
                            field: "power_kw".into(),
                        },
                        operation: None,
                        params: serde_json::json!({}),
                        auth_scope: None,
                        timeout_secs: 5,
                        result_type: InputModality::TimeSeries,
                        freshness: FreshnessPolicy::LookbackWindow {
                            lookback_secs: 900,
                            max_points: 60,
                        },
                        on_missing: MissingPolicy::Undetermined,
                    },
                    InputRequirement {
                        key: "weather".into(),
                        source: InputSourceRef::ExtensionInstance {
                            id: "weather_cn".into(),
                        },
                        operation: Some("current".into()),
                        params: serde_json::json!({}),
                        auth_scope: None,
                        timeout_secs: 5,
                        result_type: InputModality::StructuredJson,
                        freshness: FreshnessPolicy::LatestWithin { max_age_secs: 1800 },
                        on_missing: MissingPolicy::ReportStale,
                    },
                ],
            },
            tool_policy: ToolPolicy::Disabled,
            output: OutputContract::default(),
            notify: NotifyPolicy::default(),
            controlled_actions: vec![ControlledAction {
                name: "adjust_load".into(),
                destination: "ext-write-1".into(),
                depends_on: vec!["meter_15m".into(), "weather".into()],
                idempotent: false,
                params: serde_json::json!({}),
            }],
        };

        let (spec, _) = runtime.create(draft).unwrap();
        let resp = runtime.invoke(&spec.id).await.unwrap();
        let exec = resp.execution.as_ref().unwrap();
        let weather = exec
            .evidence
            .items
            .iter()
            .find(|i| i.key == "weather")
            .unwrap();
        assert!(
            matches!(weather.quality, EvidenceQuality::Stale),
            "old weather must be Stale, got {:?}",
            weather.quality
        );
        let meter = exec
            .evidence
            .items
            .iter()
            .find(|i| i.key == "meter_15m")
            .unwrap();
        assert!(
            matches!(meter.quality, EvidenceQuality::Fresh),
            "meter window must be usable: {:?}",
            meter.quality
        );
        assert_eq!(runtime.side_effects.invokes(), 0);
        assert!(
            exec.deliveries
                .iter()
                .any(|d| d.destination == "ext-write-1"
                    && matches!(
                        d.state,
                        neomind_ai_agent::model::DeliveryState::Rejected { .. }
                    )),
            "weather-dependent action must be blocked: {:?}",
            exec.deliveries
        );
    }

    #[tokio::test]
    async fn single_point_cannot_complete_lookback_window() {
        let (deps, _, telemetry) = test_deps().await;
        let store = AgentStore::memory().unwrap();
        let runtime = HostedAiAgentRuntime::build(store, deps).unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        telemetry
            .write(
                "device:meter-1",
                "power_kw",
                DataPoint::new(now / 1000 - 10, MetricValue::Float(9.0)),
            )
            .await
            .unwrap();
        let _ = telemetry.flush();

        let mut draft = local_brief_draft("one-point");
        draft.input = InputContract {
            requirements: vec![InputRequirement {
                key: "meter_15m".into(),
                source: InputSourceRef::DeviceMetric {
                    device_id: "meter-1".into(),
                    field: "power_kw".into(),
                },
                operation: None,
                params: serde_json::json!({}),
                auth_scope: None,
                timeout_secs: 5,
                result_type: InputModality::TimeSeries,
                freshness: FreshnessPolicy::LookbackWindow {
                    lookback_secs: 900,
                    max_points: 60,
                },
                on_missing: MissingPolicy::ReportStale,
            }],
        };
        let (spec, _) = runtime.create(draft).unwrap();
        let resp = runtime.invoke(&spec.id).await.unwrap();
        let item = resp
            .execution
            .as_ref()
            .unwrap()
            .evidence
            .items
            .iter()
            .find(|i| i.key == "meter_15m")
            .unwrap();
        assert!(
            !matches!(item.quality, EvidenceQuality::Fresh),
            "single point must not complete full-window calc: {:?}",
            item.quality
        );
    }

    #[tokio::test]
    async fn scheduler_tick_persists_execution_via_shared_submit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agents.redb");
        let store = AgentStore::open(&path).unwrap();
        let (deps, _ch, _) = test_deps().await;
        let runtime = Arc::new(HostedAiAgentRuntime::build(store.clone(), deps).unwrap());

        let clock = Arc::new(FixedClock::new(10_000));
        let mut draft = local_brief_draft("sched-persist");
        draft.trigger = TriggerSpec::Interval { every_secs: 1 };
        let (spec, _) = runtime.create(draft).unwrap();
        let submit: Arc<dyn AgentSubmitPort> = Arc::new(HostedSubmit(Arc::downgrade(&runtime)));
        let scheduler = AgentScheduler::new(submit, runtime.durable.mem.clone(), clock.clone());
        scheduler.tick_once().await;
        for _ in 0..50 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            if runtime.durable.mem.formal_history_len(&spec.id) >= 1 {
                break;
            }
        }
        assert!(runtime.durable.mem.formal_history_len(&spec.id) >= 1);
        let exec_id = runtime
            .durable
            .mem
            .formal_history(&spec.id)
            .last()
            .map(|e| e.id.clone())
            .expect("execution id");

        drop(scheduler);
        drop(runtime);
        drop(store);

        let reopened = AgentStore::open(&path).unwrap();
        let record_store = AgentRecordStore::from_arc(reopened.db()).unwrap();
        let formal = record_store.list_formal_executions(&spec.id).unwrap();
        assert!(!formal.is_empty());
        assert_eq!(formal.last().unwrap().id, exec_id);
    }

    #[tokio::test]
    async fn delete_surfaces_injected_fail_and_success_removes_on_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agents-del.redb");
        let store = AgentStore::open(&path).unwrap();
        let (deps, _, _) = test_deps().await;
        let runtime = HostedAiAgentRuntime::build(store.clone(), deps).unwrap();
        let (spec, _) = runtime.create(local_brief_draft("to-delete")).unwrap();

        runtime
            .durable
            .fail_next_store_delete
            .store(true, Ordering::SeqCst);
        let err = runtime.delete(&spec.id).unwrap_err();
        assert!(matches!(err, AgentError::Storage(_)));
        assert!(runtime.has_agent(&spec.id));

        runtime.delete(&spec.id).unwrap();
        assert!(!runtime.has_agent(&spec.id));
        drop(runtime);
        drop(store);

        let reopened = AgentStore::open(&path).unwrap();
        let record_store = AgentRecordStore::from_arc(reopened.db()).unwrap();
        assert!(record_store.get_spec(&spec.id).unwrap().is_none());
    }

    #[tokio::test]
    async fn host_lookback_matches_second_telemetry_window() {
        let (deps, _, telemetry) = test_deps().await;
        let port = HostSourceReadPort::new(deps);
        let now_s = chrono::Utc::now().timestamp();
        // Two in last 15m, one older than 15m — Unix seconds like MQTT/API.
        telemetry
            .write_batch(
                "device:meter-win",
                "power_kw",
                vec![
                    DataPoint::new(now_s - 60, MetricValue::Float(1.0)),
                    DataPoint::new(now_s - 120, MetricValue::Float(2.0)),
                    DataPoint::new(now_s - 2_000, MetricValue::Float(9.0)),
                ],
            )
            .await
            .unwrap();

        let params = serde_json::json!({
            "_freshness": {
                "kind": "lookback_window",
                "lookback_secs": 900,
                "max_points": 60
            },
            "lookback_secs": 900
        });
        let r = port
            .read_device_metric("meter-win", "power_kw", &params, 5)
            .await
            .unwrap();
        let points = r
            .value
            .get("points")
            .and_then(|p| p.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        assert_eq!(
            points, 2,
            "15m window must return exactly two in-window points"
        );
        let (start_ms, end_ms) = r.valid_window.expect("evidence ms window");
        let now_ms = chrono::Utc::now().timestamp_millis();
        assert!(
            end_ms >= now_ms - 5_000 && end_ms <= now_ms + 5_000,
            "valid_window end must be near now_ms: {end_ms} vs {now_ms}"
        );
        assert!(
            (end_ms - start_ms - 900_000).abs() < 5_000,
            "valid_window must span ~900s in ms: {start_ms}..{end_ms}"
        );
        assert!(
            r.observed_at.unwrap_or(0) >= 1_000_000_000_000,
            "observed_at should be normalized to ms"
        );
    }

    #[tokio::test]
    async fn extension_command_observed_at_from_payload_not_call_time() {
        let payload = Arc::new(Mutex::new(serde_json::json!({})));
        let (mut deps, _, _) = test_deps().await;
        let ext_reg = Arc::new(ExtensionRegistry::new());
        ext_reg
            .register(
                "weather_cn_obs".into(),
                fake_weather_extension("weather_cn_obs", payload.clone()),
            )
            .await
            .unwrap();
        deps.extension_registry = ext_reg;
        let store = AgentStore::memory().unwrap();
        let runtime = HostedAiAgentRuntime::build(store, deps).unwrap();

        let now_ms = chrono::Utc::now().timestamp_millis();
        let draft_for = |name: &str| {
            let mut d = local_brief_draft(name);
            d.input = InputContract {
                requirements: vec![InputRequirement {
                    key: "weather".into(),
                    source: InputSourceRef::ExtensionInstance {
                        id: "weather_cn_obs".into(),
                    },
                    operation: Some("current".into()),
                    params: serde_json::json!({}),
                    auth_scope: None,
                    timeout_secs: 5,
                    result_type: InputModality::StructuredJson,
                    freshness: FreshnessPolicy::LatestWithin { max_age_secs: 900 },
                    on_missing: MissingPolicy::ReportStale,
                }],
            };
            d
        };

        // Fresh observation in payload (ms), well within 900s.
        *payload.lock().unwrap() = serde_json::json!({
            "temp": 22,
            "observed_at": now_ms - 30_000
        });
        let (spec, _) = runtime.create(draft_for("wx-fresh")).unwrap();
        let resp = runtime.invoke(&spec.id).await.unwrap();
        let item = resp
            .execution
            .as_ref()
            .unwrap()
            .evidence
            .items
            .iter()
            .find(|i| i.key == "weather")
            .unwrap();
        assert!(
            matches!(item.quality, EvidenceQuality::Fresh),
            "fresh payload obs must pass LatestWithin: {:?} observed_at={:?}",
            item.quality,
            item.observed_at
        );

        // Yesterday's cache in payload must be Stale (not call-time).
        *payload.lock().unwrap() = serde_json::json!({
            "temp": 22,
            "observed_at": now_ms - 86_400_000
        });
        let (spec, _) = runtime.create(draft_for("wx-stale")).unwrap();
        let resp = runtime.invoke(&spec.id).await.unwrap();
        let item = resp
            .execution
            .as_ref()
            .unwrap()
            .evidence
            .items
            .iter()
            .find(|i| i.key == "weather")
            .unwrap();
        assert!(
            matches!(item.quality, EvidenceQuality::Stale),
            "stale payload obs must not use call time: {:?}",
            item.quality
        );

        // Missing observation time → Stale/missing under strict LatestWithin.
        *payload.lock().unwrap() = serde_json::json!({ "temp": 22 });
        let (spec, _) = runtime.create(draft_for("wx-missing")).unwrap();
        let resp = runtime.invoke(&spec.id).await.unwrap();
        let item = resp
            .execution
            .as_ref()
            .unwrap()
            .evidence
            .items
            .iter()
            .find(|i| i.key == "weather")
            .unwrap();
        assert!(
            !matches!(item.quality, EvidenceQuality::Fresh),
            "missing obs time must not be Fresh: {:?}",
            item.quality
        );
    }

    #[tokio::test]
    async fn task_invoke_is_sample_cannot_forge_fresh_observation() {
        let payload = Arc::new(Mutex::new(serde_json::json!({})));
        let (mut deps, _, _) = test_deps().await;
        let ext_reg = Arc::new(ExtensionRegistry::new());
        ext_reg
            .register(
                "weather_cn_forge".into(),
                fake_weather_extension("weather_cn_forge", payload.clone()),
            )
            .await
            .unwrap();
        deps.extension_registry = ext_reg;
        let store = AgentStore::memory().unwrap();
        let runtime = HostedAiAgentRuntime::build(store, deps).unwrap();

        let now_ms = chrono::Utc::now().timestamp_millis();
        // Yesterday in payload — task tries to forge sample-at-invoke.
        *payload.lock().unwrap() = serde_json::json!({
            "temp": 30,
            "observed_at": now_ms / 1000 - 86_400
        });

        let mut draft = local_brief_draft("forge-sample");
        draft.input = InputContract {
            requirements: vec![InputRequirement {
                key: "weather".into(),
                source: InputSourceRef::ExtensionInstance {
                    id: "weather_cn_forge".into(),
                },
                operation: Some("current".into()),
                params: serde_json::json!({ "_invoke_is_sample": true }),
                auth_scope: None,
                timeout_secs: 5,
                result_type: InputModality::StructuredJson,
                freshness: FreshnessPolicy::LatestWithin { max_age_secs: 900 },
                on_missing: MissingPolicy::ReportStale,
            }],
        };
        draft.controlled_actions = vec![ControlledAction {
            name: "adjust".into(),
            destination: "ext-write-1".into(),
            depends_on: vec!["weather".into()],
            idempotent: false,
            params: serde_json::json!({}),
        }];

        let (spec, _) = runtime.create(draft).unwrap();
        let resp = runtime.invoke(&spec.id).await.unwrap();
        let exec = resp.execution.as_ref().unwrap();
        let weather = exec
            .evidence
            .items
            .iter()
            .find(|i| i.key == "weather")
            .unwrap();
        assert!(
            matches!(weather.quality, EvidenceQuality::Stale),
            "task _invoke_is_sample must not forge Fresh: {:?}",
            weather.quality
        );
        assert_eq!(runtime.side_effects.invokes(), 0);
        assert!(
            exec.deliveries
                .iter()
                .any(|d| d.destination == "ext-write-1"
                    && matches!(
                        d.state,
                        neomind_ai_agent::model::DeliveryState::Rejected { .. }
                    )),
            "stale weather must block action: {:?}",
            exec.deliveries
        );
    }

    #[tokio::test]
    async fn fresh_db_boots_hosted_runtime_without_legacy_agents_table() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("agents.redb");

        let store = AgentStore::open(&path).unwrap();
        {
            let db = store.db();
            assert!(
                !neomind_storage::has_legacy_agents_table(&db).unwrap_or(true),
                "legacy agents table must be absent on fresh DB"
            );
        }

        let (deps, _, _) = test_deps().await;
        let runtime = HostedAiAgentRuntime::build(store, deps).unwrap();
        assert!(runtime.list().is_empty());
        let (spec, _) = runtime.create(local_brief_draft("gate-empty")).unwrap();
        runtime.ensure_loaded(&spec.id).unwrap();
        assert!(runtime.has_agent(&spec.id));
    }

    #[tokio::test]
    async fn output_publish_writes_ai_metric_with_provenance_and_feeds_upstream_ai() {
        let (mut base_deps, _, telemetry) = test_deps().await;
        let payload = Arc::new(Mutex::new(serde_json::json!({
            "status": "normal",
            "score": 92,
            "confidence": 0.9
        })));
        let ext_reg = Arc::new(ExtensionRegistry::new());
        ext_reg
            .register(
                "weather_cn_pub".into(),
                fake_weather_extension("weather_cn_pub", payload),
            )
            .await
            .unwrap();
        base_deps.extension_registry = ext_reg;

        let bus = Arc::new(EventBus::new());
        let value_provider = Arc::new(neomind_rules::UnifiedValueProvider::new());
        let rule_engine = Arc::new(neomind_rules::RuleEngine::new(value_provider.clone()));
        let publisher = Arc::new(VirtualMetricPublisher {
            event_bus: bus.clone(),
            time_series: telemetry.clone(),
            value_provider: value_provider.clone(),
            rule_engine,
        });
        let deps = base_deps
            .with_event_bus(bus.clone())
            .with_metric_publisher(publisher);

        let store = AgentStore::memory().unwrap();
        let runtime = HostedAiAgentRuntime::build(store, deps).unwrap();

        let mut upstream = local_brief_draft("upstream-pub");
        upstream.input = InputContract {
            requirements: vec![InputRequirement {
                key: "src".into(),
                source: InputSourceRef::ExtensionInstance {
                    id: "weather_cn_pub".into(),
                },
                operation: Some("current".into()),
                params: serde_json::json!({}),
                auth_scope: None,
                timeout_secs: 5,
                result_type: InputModality::StructuredJson,
                freshness: FreshnessPolicy::Any,
                on_missing: MissingPolicy::ReportStale,
            }],
        };
        upstream.output = OutputContract {
            fields: vec![
                neomind_ai_agent::model::OutputField {
                    name: "status".into(),
                    depends_on: vec!["src".into()],
                    description: Some("Overall status".into()),
                },
                neomind_ai_agent::model::OutputField {
                    name: "score".into(),
                    depends_on: vec!["src".into()],
                    description: Some("Score".into()),
                },
            ],
            publish: true,
        };

        let (up_spec, _) = runtime.create(upstream).unwrap();
        let up_resp = runtime.invoke(&up_spec.id).await.unwrap();
        let up_exec = up_resp.execution.expect("upstream exec");
        assert_eq!(up_exec.status, ExecutionStatus::Completed);
        assert!(up_exec.structured_result.is_some());

        let _ = telemetry.flush();
        let ai_ns = format!("ai:{}", up_spec.id);
        let score_pt = telemetry
            .latest(&ai_ns, "score")
            .await
            .unwrap()
            .expect("published ai:{agent}:score point");
        assert_eq!(score_pt.value, MetricValue::Integer(92));
        assert_eq!(score_pt.quality, Some(0.9));
        assert_eq!(
            score_pt
                .metadata
                .as_ref()
                .and_then(|m| m.get(EXECUTION_ID_KEY))
                .and_then(|v| v.as_str()),
            Some(up_exec.id.as_str()),
            "published AI metric must carry execution_id provenance"
        );

        // Downstream agent reads ai:{up_spec.id}:score via InputSourceRef::UpstreamAi.
        let mut downstream = local_brief_draft("downstream-reader");
        downstream.input = InputContract {
            requirements: vec![InputRequirement {
                key: "up_score".into(),
                source: InputSourceRef::UpstreamAi {
                    agent_id: up_spec.id.clone(),
                    field: "score".into(),
                },
                operation: None,
                params: serde_json::json!({}),
                auth_scope: None,
                timeout_secs: 5,
                result_type: InputModality::Metric,
                freshness: FreshnessPolicy::LatestWithin { max_age_secs: 60 },
                on_missing: MissingPolicy::ReportStale,
            }],
        };
        let (down_spec, _) = runtime.create(downstream).unwrap();
        let down_resp = runtime.invoke(&down_spec.id).await.unwrap();
        let down_exec = down_resp.execution.expect("downstream exec");
        let item = down_exec
            .evidence
            .items
            .iter()
            .find(|i| i.key == "up_score")
            .expect("upstream evidence");
        assert!(
            matches!(item.quality, EvidenceQuality::Fresh),
            "UpstreamAi must read published ai:* metric as Fresh: {:?}",
            item.quality
        );
    }

    #[tokio::test]
    async fn formal_run_emits_events_appends_journal_and_records_duration_stats() {
        let (base_deps, _, _) = test_deps().await;
        let bus = Arc::new(EventBus::new());
        let mut rx = bus.subscribe();
        let deps = base_deps.with_event_bus(bus);

        let store = AgentStore::memory().unwrap();
        let runtime = HostedAiAgentRuntime::build(store.clone(), deps).unwrap();

        let (spec, _) = runtime.create(local_brief_draft("journal-events")).unwrap();
        let ok_resp = runtime.invoke(&spec.id).await.unwrap();
        let ok_exec = ok_resp.execution.expect("ok exec");

        // Trigger a failure on a real-model agent to verify error-path journal write.
        let mut fail_draft = local_brief_draft("journal-fail");
        fail_draft.llm_backend_id = None;
        let (fail_spec, _) = runtime.create(fail_draft).unwrap();
        runtime.llm.force_fail_next.store(true, Ordering::SeqCst);
        let fail_resp = runtime.invoke(&fail_spec.id).await.unwrap();
        let fail_exec = fail_resp.execution.expect("fail exec");
        assert_eq!(fail_exec.status, ExecutionStatus::Failed);

        let record_store = AgentRecordStore::from_arc(store.db()).unwrap();
        let ok_mem = record_store
            .get_memory(&spec.id)
            .unwrap()
            .expect("ok memory");
        assert_eq!(ok_mem.journal.records.len(), 1);
        assert_eq!(ok_mem.journal.records[0].execution_id, ok_exec.id);
        assert!(ok_mem.journal.records[0].success);

        let fail_mem = record_store
            .get_memory(&fail_spec.id)
            .unwrap()
            .expect("fail memory");
        assert_eq!(fail_mem.journal.records.len(), 1);
        assert_eq!(fail_mem.journal.records[0].execution_id, fail_exec.id);
        assert!(!fail_mem.journal.records[0].success);

        let stats = runtime.runtime_stats_json(&spec.id);
        assert_eq!(stats["total_runs"], 1);
        assert!(stats["last_duration_ms"].is_u64());

        // Drain EventBus and confirm matching Started + Completed events for ok_exec.
        let mut saw_started = false;
        let mut saw_completed = false;
        while let Some((ev, _)) = rx.try_recv() {
            match ev {
                NeoMindEvent::AgentExecutionStarted {
                    agent_id,
                    execution_id,
                    ..
                } if agent_id == spec.id && execution_id == ok_exec.id => {
                    saw_started = true;
                }
                NeoMindEvent::AgentExecutionCompleted {
                    agent_id,
                    execution_id,
                    success,
                    ..
                } if agent_id == spec.id && execution_id == ok_exec.id && success => {
                    saw_completed = true;
                }
                _ => {}
            }
        }
        assert!(saw_started, "must emit AgentExecutionStarted on EventBus");
        assert!(
            saw_completed,
            "must emit AgentExecutionCompleted on EventBus"
        );
    }

    #[tokio::test]
    async fn memory_context_loads_journal_and_knowledge_files_without_mutating_spec() {
        let store = AgentStore::memory().unwrap();
        let mem_dir = tempfile::tempdir().unwrap();
        let mem_store = Arc::new(MarkdownMemoryStore::with_config(
            mem_dir.path().to_path_buf(),
            neomind_storage::MemoryConfig::default(),
        ));
        let (base_deps, _, _) = test_deps().await;
        let deps = base_deps.with_memory_store(mem_store.clone());
        let runtime = HostedAiAgentRuntime::build(store.clone(), deps).unwrap();

        let (spec, _) = runtime.create(local_brief_draft("memory-loop")).unwrap();
        let original_prompt = spec.user_prompt.clone();

        // First run appends to the execution journal.
        let _ = runtime.invoke(&spec.id).await.unwrap();

        // Write an agent knowledge file into the agent's memory directory.
        mem_store
            .write_agent_custom_file(&spec.id, "sop", "Escalate when temp > 30C.")
            .await
            .unwrap();

        let ctx = runtime.load_memory_context(&spec.id);
        assert_eq!(ctx.journal_entries.len(), 1);
        assert!(ctx.journal_entries[0].contains("[OK]"));
        assert_eq!(ctx.knowledge_files.len(), 1);
        assert_eq!(ctx.knowledge_files[0].0, "sop");
        assert!(ctx.knowledge_files[0]
            .2
            .contains("Escalate when temp > 30C."));

        // Second run injects memory context run-locally without mutating stored spec.
        let _ = runtime.invoke(&spec.id).await.unwrap();
        let (reloaded, _) = runtime.get(&spec.id).unwrap();
        assert_eq!(reloaded.user_prompt, original_prompt);
    }

    #[tokio::test]
    async fn dry_run_ephemeral_and_weak_scheduler_leave_zero_pollution_or_arc_cycle() {
        let store = AgentStore::memory().unwrap();
        let (deps, _, _) = test_deps().await;
        let runtime = Arc::new(HostedAiAgentRuntime::build(store.clone(), deps).unwrap());

        runtime.start();
        assert_eq!(
            Arc::strong_count(&runtime),
            1,
            "HostedSubmit must hold Weak<HostedAiAgentRuntime>, not strong Arc"
        );
        runtime.stop();

        let ephemeral = AgentSpec {
            id: String::new(),
            name: "ephemeral-preview".into(),
            description: None,
            user_prompt: "preview".into(),
            llm_backend_id: Some(LOCAL_BRIEF_BACKEND.into()),
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract::default(),
            tool_policy: ToolPolicy::Disabled,
            output: OutputContract::default(),
            notify: NotifyPolicy::default(),
            controlled_actions: vec![],
            config_version: 1,
            created_at: 0,
            updated_at: 0,
        };
        let resp = runtime.dry_run_ephemeral(ephemeral).await.unwrap();
        assert!(resp.dry_run);
        assert!(runtime.list().is_empty());

        let record_store = AgentRecordStore::from_arc(store.db()).unwrap();
        assert!(record_store.list_specs().unwrap().is_empty());

        let (persisted, _) = runtime.create(local_brief_draft("stats-fields")).unwrap();
        let _ = runtime.invoke(&persisted.id).await.unwrap();
        let stats = runtime.runtime_stats_json(&persisted.id);
        assert_eq!(stats["runs_today"], 1);
        assert!(stats.get("quota_day").is_some());
        assert_eq!(stats["skipped_daily_limit"], 0);
        assert_eq!(stats["skipped_permit_timeout"], 0);
    }
}
