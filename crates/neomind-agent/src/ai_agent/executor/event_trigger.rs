//! Event-triggered agent execution.
//!
//! Handles matching incoming data events to agents and spawning
//! background executions for matching event-type agents.

use super::*;

/// Burst-merge window for Free/Focused event agents — see the debounce
/// comment in `check_and_trigger_data_event`. Structured agents use their
/// OperatorConfig value instead.
const DEFAULT_FREE_EVENT_DEBOUNCE_SECS: i64 = 30;

/// Floor for the Free/Focused burst-merge window. One physical report often
/// fans out to N per-metric events on the bus; with an on-demand cooldown
/// (0s) and no floor, each of those would spawn its own admission task and
/// race the in-flight guard. One second collapses the fan-out into the run
/// that actually processes it.
const MIN_BURST_MERGE_SECS: i64 = 1;

/// How long an admission timestamp stays in the event dedup map. Must exceed
/// any cooldown a user can configure for the cooldown check to see it — 1h
/// covers practical values (beyond that the entry ages out and the window
/// degrades to "no cooldown", the fail-open direction).
const EVENT_DEDUP_RETENTION_SECS: i64 = 3600;

/// How an event-triggered run ended — the caller's cue for its cooldown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EventRunOutcome {
    /// The run is over: it completed, or ended in a way that is not a
    /// fault of the agent (daily-cap refusal, busy, empty-event skip).
    /// The cooldown stays.
    Completed,
    /// The LLM side hiccuped and a retry did not help. The cooldown is
    /// cleared so the next event gets its chance.
    TransientFailure,
    /// The run itself cannot succeed right now — context overflow, a
    /// permanent LLM error, a failure after side effects. Replaying the
    /// same input seconds later cannot end differently; it only doubled
    /// the breaker count and the failure notifications (T4 2026-11-24:
    /// one overflow became two Failed runs and two notifications per
    /// event, and tripped the breaker at twice the speed).
    PermanentFailure,
}

impl AgentExecutor {
    /// Prune stale entries from the event dedup map. Entries older than
    /// `EVENT_DEDUP_RETENTION_SECS` are removed. Also prunes recovery-probe
    /// state untouched for a day (deleted agents, long-recovered ones the
    /// success path already removed).
    async fn cleanup_stale_dedup_entries(&self) {
        let now = chrono::Utc::now().timestamp();
        let mut recent = self.recent_executions.write().await;
        recent.retain(|_, &mut ts| now - ts < EVENT_DEDUP_RETENTION_SECS);
        drop(recent);
        super::global_error_probe_state()
            .write()
            .retain(|_, e| now - e.last_attempt < 86_400);
    }

    /// Mark a manual execution of an event-type agent in the dedup map.
    ///
    /// Without this, `execute_agent_now` runs without registering in
    /// `recent_executions`, so the next real event for any of the agent's
    /// bound sources can fire inside the cooldown window — yielding a
    /// duplicate execution (duplicate journal entry, potential duplicate
    /// notification, wasted LLM tokens). Manual execution cools the same
    /// window.
    ///
    /// For non-event agents this is a no-op (the dedup key is only
    /// consulted by the event-trigger path).
    pub async fn mark_manual_execution(&self, agent_id: &str) {
        use neomind_storage::ScheduleType;

        // Only event-type agents participate in the event-dedup window.
        let agent = match self.store.get_agent(agent_id).await {
            Ok(Some(a)) => a,
            _ => return,
        };
        if agent.schedule.schedule_type != ScheduleType::Event {
            return;
        }

        // The event-trigger path keys its cooldown on the bare agent id,
        // so a manual run simply occupies that same key — symmetric with
        // how the event path records a successful firing.
        let now = chrono::Utc::now().timestamp();
        let mut recent = self.recent_executions.write().await;
        recent.insert(agent_id.to_string(), now);

        // Opportunistic cleanup — mirrors the event-trigger entry path.
        recent.retain(|_, &mut ts| now - ts < EVENT_DEDUP_RETENTION_SECS);

        tracing::debug!(
            agent_id = %agent_id,
            "Marked manual execution in event dedup map"
        );
    }

    /// Unified entry point for triggering agents on any data source update.
    /// Called from the EventBus listener when any data source produces new values.
    pub async fn check_and_trigger_data_event(
        &self,
        source_type: &str,
        source_id: String,
        field: String,
        value: &MetricValue,
    ) -> AgentResult<()> {
        // Refresh event-triggered agents cache
        self.refresh_event_agents().await;

        let event_agents = self.event_agents.read().await;

        tracing::debug!(
            source_type = %source_type,
            source_id = %source_id,
            field = %field,
            event_agent_count = event_agents.len(),
            "[DATA_EVENT] Checking data event against {} event-triggered agents",
            event_agents.len()
        );

        let source_id_for_spawn = source_id.clone();

        // Clean up old entries from recent_executions (older than cooldown window)
        self.cleanup_stale_dedup_entries().await;
        let now = chrono::Utc::now().timestamp();

        for agent in event_agents.values() {
            // Check if this agent has event-based schedule
            if !matches!(
                agent.schedule.schedule_type,
                neomind_storage::ScheduleType::Event
            ) {
                continue;
            }

            // Does this event fire the agent? With an `all` filter this folds
            // the event into the agent's window and answers `true` only on the
            // one that completes it.
            if !self
                .event_fires(agent, source_type, &source_id, &field, now)
                .await
            {
                continue;
            }

            // Resolve this agent's operator config once: both pacing knobs
            // (debounce below, cooldown after) read from it.
            let op_cfg = agent
                .operator_config
                .clone()
                .unwrap_or_else(super::structured::default_operator_config);

            // Burst-merge guardrail for EVERY mode. One physical event can
            // match several of this agent's sources, and N sources reporting
            // in the same instant used to fire N CONCURRENT runs for
            // Free/Focused agents (debounce was Structured-only) — concurrent
            // runs of one agent race its journal read-modify-write and burn
            // the concurrency budget. Free/Focused track the cooldown,
            // floored at 1s. Structured keeps its OperatorConfig window —
            // EXCEPT when an explicit cooldown is set: the user who set
            // event_cooldown_secs stated their pacing intent, and a stale
            // debounce default must not silently override it (cooldown 0 +
            // the default 30s debounce kept image agents on a 30s cadence
            // while their UI said "on-demand"). The cap only fires on an
            // explicit cooldown, and no pre-existing agent can have one —
            // the field shipped with the cooldown feature itself.
            let cooldown_secs = op_cfg.effective_event_cooldown_secs();
            let debounce_secs = match agent.execution_mode {
                neomind_storage::agents::ExecutionMode::Structured => {
                    match op_cfg.event_cooldown_secs {
                        None => op_cfg.debounce_secs as i64,
                        Some(cooldown) => (op_cfg.debounce_secs as i64)
                            .min(cooldown.max(MIN_BURST_MERGE_SECS as u32) as i64),
                    }
                }
                _ => DEFAULT_FREE_EVENT_DEBOUNCE_SECS
                    .min(cooldown_secs)
                    .max(MIN_BURST_MERGE_SECS),
            };
            if debounce_secs > 0 {
                let last = self
                    .last_event_inference
                    .read()
                    .get(&agent.id)
                    .copied()
                    .unwrap_or(0);
                if now - last < debounce_secs {
                    tracing::info!(
                        agent_name = %agent.name,
                        source_id = %source_id,
                        field = %field,
                        debounce_secs,
                        "Merged into the debounce window — skipping data event execution"
                    );
                    continue;
                }
            }

            // Cooldown: minimum spacing between two ADMITTED runs of this
            // agent, measured from the previous admission — per-agent via
            // `OperatorConfig::event_cooldown_secs` (default 60s, 0 =
            // on-demand). The old hard-coded 60s floor idled fast agents:
            // a 5s run spent 55s dropping events that had arrived after it
            // finished. On-demand setups rely on execute_agent's
            // one-run-at-a-time guard as their only backpressure.
            let dedup_key = agent.id.clone();
            let recent = self.recent_executions.read().await;
            let is_duplicate = recent
                .get(&dedup_key)
                .map(|&timestamp| now - timestamp < cooldown_secs)
                .unwrap_or(false);
            drop(recent);

            if is_duplicate {
                tracing::info!(
                    agent_name = %agent.name,
                    source_type = %source_type,
                    source_id = %source_id,
                    field = %field,
                    "Skipping data event-triggered execution (cooldown: {}s)",
                    cooldown_secs
                );
                continue;
            }

            // Recovery probe gate for demoted (Error) agents. A matching
            // event is a probe OPPORTUNITY, but probes run at backoff
            // intervals — a broken agent must not burn an LLM call per
            // event. Placed after the debounce/cooldown gates so only a run
            // that will actually spawn claims the probe slot; the spawn
            // records the outcome (recovered → entry cleared, still broken →
            // next probe one rung further up the backoff ladder).
            let was_probe = agent.status == neomind_storage::AgentStatus::Error;
            if was_probe {
                let backoff_elapsed = {
                    let probes = super::global_error_probe_state().read();
                    match probes.get(&agent.id) {
                        None => true,
                        Some(entry) => {
                            now - entry.last_attempt
                                >= super::error_probe_backoff_secs(entry.consecutive_failures)
                        }
                    }
                };
                if !backoff_elapsed {
                    tracing::trace!(
                        agent_name = %agent.name,
                        source_id = %source_id,
                        field = %field,
                        "Error agent probe still in backoff — skipping"
                    );
                    continue;
                }
                super::global_error_probe_state()
                    .write()
                    .entry(agent.id.clone())
                    .and_modify(|e| e.last_attempt = now)
                    .or_insert(super::ErrorProbeEntry {
                        last_attempt: now,
                        consecutive_failures: 0,
                    });
                tracing::info!(
                    agent_name = %agent.name,
                    source_id = %source_id,
                    field = %field,
                    "Error agent — admitting recovery probe"
                );
            }

            // Clone dedup_key before it's moved into the recent_executions map;
            // the spawned task needs a copy to clear the cooldown on failure.
            let dedup_key_clone = dedup_key.clone();

            // Mark this execution as recent. The AND window (if any) is
            // consumed HERE — past the debounce merge and the cooldown, the
            // run is admitted and a retained complete window must not
            // re-fire duplicates after the run finishes.
            {
                let mut recent = self.recent_executions.write().await;
                recent.insert(dedup_key, now);
                self.clear_event_window(&agent.id);
            }
            if debounce_secs > 0 {
                self.last_event_inference
                    .write()
                    .insert(agent.id.clone(), now);
            }

            tracing::debug!(
                agent_name = %agent.name,
                source_type = %source_type,
                source_id = %source_id,
                field = %field,
                "Data event-triggered agent execution"
            );

            // Clone the agent and event data for execution
            let agent_clone = agent.clone();
            let field_clone = field.clone();
            let value_clone = value.clone();
            let source_id_for_task = source_id_for_spawn.clone();
            let source_type_for_task = source_type.to_string();
            let timestamp = chrono::Utc::now().timestamp();

            // Build executor config for spawned task
            let executor_config = self.build_spawn_config(agent);
            let agent_id_for_log = agent.id.clone();
            let recent_executions_clone = self.recent_executions.clone();

            let handle = tokio::spawn(async move {
                // Acquire the GLOBAL execution semaphore first (WAIT) — see
                // the sibling spawn above for rationale.
                let _global_permit = match executor_config.execution_semaphore.clone() {
                    Some(sem) => sem.acquire_owned().await.ok(),
                    None => None,
                };
                // Acquire per-backend semaphore (WAIT, not fail). BIND the
                // permit — the return used to be discarded, so the guard
                // released the instant acquire returned and the per-backend
                // limit never actually held on this path.
                let _backend_permit = Self::acquire_backend_permit(
                    &executor_config.backend_semaphores,
                    &agent_id_for_log,
                    &agent_clone
                        .llm_backend_id
                        .clone()
                        .unwrap_or_else(|| "default".to_string()),
                )
                .await;

                // Create event trigger data with unified DataSourceRef
                let event_trigger_data = EventTriggerData {
                    source: DataSourceRef {
                        source_type: source_type_for_task,
                        source_id: source_id_for_task,
                        field: field_clone,
                    },
                    value: value_clone,
                    timestamp,
                };

                match AgentExecutor::new(executor_config).await {
                    Ok(executor) => {
                        tracing::debug!(
                            agent_id = %agent_id_for_log,
                            trigger_source_type = %event_trigger_data.source.source_type,
                            trigger_source_id = %event_trigger_data.source.source_id,
                            trigger_field = %event_trigger_data.source.field,
                            "Executing data event-triggered agent with event data"
                        );

                        // Execute with one inline retry; clear cooldown on persistent failure
                        // so transient errors (API hiccups, network blips) don't lock out
                        // subsequent events for the full 60s cooldown window.
                        let outcome = Self::execute_with_retry(
                            &executor,
                            agent_clone,
                            event_trigger_data,
                            1,
                            &agent_id_for_log,
                        )
                        .await;

                        // Only a transient failure frees the cooldown, so
                        // the next event gets its chance. A permanent one
                        // keeps it: the next report carries the same
                        // unrunnable input, and re-firing on it would trip
                        // the breaker within seconds.
                        if matches!(outcome, EventRunOutcome::TransientFailure) {
                            let mut recent = recent_executions_clone.write().await;
                            recent.remove(&dedup_key_clone);
                            tracing::info!(
                                agent_id = %agent_id_for_log,
                                dedup_key = %dedup_key_clone,
                                "Cleared event cooldown after a transient failure"
                            );
                        }

                        // Recovery-probe outcome: the agent's stored status is
                        // the ground truth — execute_agent resets Error →
                        // Active only on a completed run. Recovered clears
                        // the ladder; still broken climbs one backoff rung.
                        if was_probe {
                            let recovered = executor
                                .store()
                                .get_agent(&agent_id_for_log)
                                .await
                                .ok()
                                .flatten()
                                .map(|a| a.status == neomind_storage::AgentStatus::Active)
                                .unwrap_or(false);
                            if recovered {
                                super::global_error_probe_state()
                                    .write()
                                    .remove(&agent_id_for_log);
                                tracing::info!(
                                    agent_id = %agent_id_for_log,
                                    "Recovery probe SUCCEEDED — agent back to Active"
                                );
                            } else {
                                super::global_error_probe_state()
                                    .write()
                                    .entry(agent_id_for_log.clone())
                                    .and_modify(|e| {
                                        e.consecutive_failures =
                                            e.consecutive_failures.saturating_add(1)
                                    })
                                    .or_insert(super::ErrorProbeEntry {
                                        last_attempt: timestamp,
                                        consecutive_failures: 1,
                                    });
                                tracing::info!(
                                    agent_id = %agent_id_for_log,
                                    "Recovery probe failed — backoff ladder stepped up"
                                );
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!(
                            agent_id = %agent_id_for_log,
                            error = %e,
                            "Failed to create executor for data event-triggered agent"
                        );
                        let mut recent = recent_executions_clone.write().await;
                        recent.remove(&dedup_key_clone);
                        if was_probe {
                            super::global_error_probe_state()
                                .write()
                                .entry(agent_id_for_log.clone())
                                .and_modify(|e| {
                                    e.consecutive_failures =
                                        e.consecutive_failures.saturating_add(1)
                                });
                        }
                    }
                }
            });
            // [cancellation] register the handle so shutdown can abort it;
            // prune finished entries to keep the registry bounded.
            {
                let mut handles = self.event_task_handles.lock();
                handles.retain(|h| !h.is_finished());
                handles.push(handle);
            }
        }

        Ok(())
    }

    /// Build an `AgentExecutorConfig` by cloning all necessary fields from `self`.
    ///
    /// This replaces the pattern of individually cloning 16+ variables before
    /// each `tokio::spawn`, reducing duplication across both event trigger paths.
    fn build_spawn_config(&self, _agent: &AiAgent) -> AgentExecutorConfig {
        AgentExecutorConfig {
            store: self.store.clone(),
            time_series_storage: self.time_series_storage.clone(),
            device_service: self.device_service.clone(),
            event_bus: self.event_bus.clone(),
            message_manager: self.message_manager.clone(),
            llm_runtime: self.llm_runtime.clone(),
            llm_backend_store: self.llm_backend_store.clone(),
            extension_registry: self.extension_registry.clone(),
            tool_registry: self.tool_registry.read().clone(),
            memory_store: self.memory_store.clone(),
            backend_semaphores: self.backend_semaphores.clone(),
            skill_registry: self._config.skill_registry.clone(),
            execution_semaphore: self._config.execution_semaphore.clone(),
        }
    }

    /// Acquire a per-backend semaphore permit (WAIT, not fail). Shared by
    /// the event path and the manual-invoke path so both are accounted
    /// against the same per-backend limits as the scheduler.
    pub(crate) async fn acquire_backend_permit_public(
        semaphores: &Option<crate::ai_agent::scheduler::BackendSemaphores>,
        agent_id: &str,
        backend_id: &str,
    ) -> Option<tokio::sync::OwnedSemaphorePermit> {
        Self::acquire_backend_permit(semaphores, agent_id, backend_id).await
    }

    /// Acquire a per-backend semaphore permit for event-triggered execution.
    async fn acquire_backend_permit(
        semaphores: &Option<crate::ai_agent::scheduler::BackendSemaphores>,
        agent_id: &str,
        backend_id: &str,
    ) -> Option<tokio::sync::OwnedSemaphorePermit> {
        if let Some(ref sems) = semaphores {
            let backend_sem = sems.get(backend_id).await;
            let available = backend_sem.available_permits();
            if available == 0 {
                tracing::debug!(
                    agent_id = %agent_id,
                    backend_id = %backend_id,
                    "Event agent waiting for backend permit"
                );
            }
            match backend_sem.clone().acquire_owned().await {
                Ok(p) => {
                    tracing::debug!(
                        agent_id = %agent_id,
                        backend_id = %backend_id,
                        "Event agent acquired backend permit"
                    );
                    Some(p)
                }
                Err(e) => {
                    tracing::error!(
                        agent_id = %agent_id,
                        backend_id = %backend_id,
                        error = %e,
                        "Backend semaphore closed, skipping execution"
                    );
                    None
                }
            }
        } else {
            None
        }
    }

    /// Build a synthetic `EventTriggerData` for manual execution of event-type agents.
    ///
    /// Picks the first bound source (from `event_filter.sources`, falling back to
    /// resource bindings) that has a recent data point in time-series storage, and
    /// returns it as an `EventTriggerData`. This lets manual "Run now" invocations
    /// of event agents run with the same triggering context as a real event.
    ///
    /// Returns `None` if time-series storage is unavailable or no candidate has data.
    pub async fn build_synthetic_event_data(&self, agent: &AiAgent) -> Option<EventTriggerData> {
        let ts = self.time_series_storage.as_ref()?;

        // A whole-device trigger has no single field to quote, so no concrete
        // candidate below can represent it — "Run now" used to fall through
        // to a run with no event context at all. The run reads the device's
        // latest report itself (`collect_device_report_snapshot`).
        if let Some((src_type, src_id)) = Self::whole_source_candidate(agent) {
            tracing::info!(
                agent_id = %agent.id,
                source_type = %src_type,
                source_id = %src_id,
                "Manual execution of a whole-source event agent — the run reads the source's latest report"
            );
            return Some(EventTriggerData {
                source: DataSourceRef {
                    source_type: src_type,
                    source_id: src_id,
                    field: String::new(),
                },
                value: neomind_core::MetricValue::Json(serde_json::Value::Null),
                timestamp: chrono::Utc::now().timestamp(),
            });
        }

        let candidates = Self::extract_trigger_candidates(agent);

        for (src_type, src_id, field) in candidates {
            // Telemetry is written under `device:{id}` by every write path;
            // the bare id read a key that never exists, so a field-specific
            // "Run now" always ran without event context.
            let storage_key = if src_type == "device" {
                format!("device:{src_id}")
            } else {
                src_id.clone()
            };
            match ts.query_latest(&storage_key, &field).await {
                Ok(Some(point)) => {
                    tracing::info!(
                        agent_id = %agent.id,
                        source_type = %src_type,
                        source_id = %src_id,
                        field = %field,
                        timestamp = point.timestamp,
                        "Synthesized event data from latest data point for manual execution"
                    );
                    return Some(EventTriggerData {
                        source: DataSourceRef {
                            source_type: src_type,
                            source_id: src_id,
                            field,
                        },
                        value: neomind_core::MetricValue::Json(point.value),
                        timestamp: point.timestamp,
                    });
                }
                Ok(None) => continue,
                Err(e) => {
                    tracing::warn!(
                        agent_id = %agent.id,
                        source_id = %src_id,
                        field = %field,
                        error = %e,
                        "Failed to query latest data point while synthesizing event data"
                    );
                    continue;
                }
            }
        }

        tracing::warn!(
            agent_id = %agent.id,
            "No recent data point found for any bound source; manual execution will run without event context"
        );
        None
    }

    /// A field name no device reports. Asking whether a trigger ALSO fires
    /// for the same source with this field answers "is the match on the
    /// source, or on one of its fields?" without duplicating the matcher.
    const ANY_FIELD: &str = "\u{1}any-field";

    /// Does the agent's trigger cover the WHOLE source that raised `source`,
    /// rather than one of its fields?
    ///
    /// True for a field-less source (`{"type":"device","id":"d1"}`), the
    /// `all` wildcard, and the resource fallback matching a whole-device
    /// resource. Such a run is fed the device's report as a whole — the
    /// field that reached the trigger first is only the doorbell.
    pub(crate) async fn trigger_covers_whole_source(
        &self,
        agent: &AiAgent,
        source: &DataSourceRef,
    ) -> bool {
        match agent.schedule.parsed_event_filter() {
            Some(filter) => filter.any.iter().chain(filter.all.iter()).any(|spec| {
                spec.matches_event(&source.source_type, &source.source_id, Self::ANY_FIELD)
            }),
            None => {
                self.matches_data_source_filter(
                    agent,
                    &source.source_type,
                    &source.source_id,
                    Self::ANY_FIELD,
                )
                .await
            }
        }
    }

    /// The first whole-DEVICE source an event agent listens on, if any.
    ///
    /// Only devices: a device report is what `collect_device_report_snapshot`
    /// can read back. The `all` wildcard names no concrete device.
    fn whole_source_candidate(agent: &AiAgent) -> Option<(String, String)> {
        let is_whole = |source_type: &str, id: &str, field: Option<&str>| {
            source_type == "device"
                && !id.is_empty()
                && id != "all"
                && field.is_none_or(str::is_empty)
        };
        if let Some(filter) = agent.schedule.parsed_event_filter() {
            return filter
                .any
                .iter()
                .chain(filter.all.iter())
                .find(|spec| is_whole(&spec.source_type, &spec.id, spec.field.as_deref()))
                .map(|spec| (spec.source_type.clone(), spec.id.clone()));
        }
        agent
            .resources
            .iter()
            .find(|r| matches!(r.resource_type, ResourceType::Device))
            .map(|r| ("device".to_string(), r.resource_id.clone()))
    }

    /// Extract candidate `(source_type, source_id, field)` tuples for a
    /// manual run, from the agent's trigger configuration: the field-specific
    /// sources of `event_filter` — `any` and `all`, in the legacy `sources`
    /// and `event_type` shapes too — then the metric resources.
    ///
    /// Read through `parsed_event_filter()`. A hand-rolled read of the
    /// `sources` key missed the M2-1 `{"any":[…]}` / `{"all":[…]}` shapes
    /// entirely, so those agents got no event context on "Run now". Sources
    /// naming no single field (`id == "all"`, or a whole-device source) are
    /// skipped here; `whole_source_candidate` handles whole-device ones.
    fn extract_trigger_candidates(agent: &AiAgent) -> Vec<(String, String, String)> {
        let mut out: Vec<(String, String, String)> = Vec::new();

        // 1. event_filter sources that name one field
        if let Some(filter) = agent.schedule.parsed_event_filter() {
            for spec in filter.any.iter().chain(filter.all.iter()) {
                let Some(field) = spec.field.as_deref().filter(|f| !f.is_empty()) else {
                    continue;
                };
                if spec.source_type.is_empty() || spec.id.is_empty() || spec.id == "all" {
                    continue;
                }
                let candidate = (spec.source_type.clone(), spec.id.clone(), field.to_string());
                if !out.contains(&candidate) {
                    out.push(candidate);
                }
            }
        }

        // 2. Metric resources (device:metric or extension:metric)
        for r in &agent.resources {
            match r.resource_type {
                ResourceType::Metric => {
                    if let Some((dev, metric)) = r.resource_id.split_once(':') {
                        if !dev.is_empty() && !metric.is_empty() {
                            out.push(("device".to_string(), dev.to_string(), metric.to_string()));
                        }
                    }
                }
                ResourceType::ExtensionMetric => {
                    if let Some((ext, metric)) = r.resource_id.split_once(':') {
                        if !ext.is_empty() && !metric.is_empty() {
                            out.push((
                                "extension".to_string(),
                                ext.to_string(),
                                metric.to_string(),
                            ));
                        }
                    }
                }
                _ => {}
            }
        }

        out
    }

    /// Backoff between an event run's attempts.
    const EVENT_RETRY_BACKOFF: std::time::Duration = std::time::Duration::from_secs(5);

    /// Is a failed event run worth ONE replay?
    ///
    /// Deliberately not the breaker's `is_transient_failure`. That one answers
    /// "should this agent be demoted?" and counts `already executing` and
    /// `daily run cap` as transient. For an event run those two are settled
    /// earlier (dropped, cooldown kept) and must never reach a replay or a
    /// cooldown clear; two separate lists also keep a future change to the
    /// breaker from silently turning into event replays.
    pub(super) fn is_retryable_event_failure(error: Option<&str>) -> bool {
        // An unknown error is not replayed.
        let Some(msg) = error else {
            return false;
        };
        let lower = msg.to_lowercase();
        // Transport faults (reqwest::Error display) and DNS.
        lower.contains("network")
            || lower.contains("error sending request")
            || lower.contains("dns")
            || lower.contains("connection reset")
            || lower.contains("connection refused")
            || lower.contains("broken pipe")
            // Timeouts (per-request and global).
            || lower.contains("timed out")
            || lower.contains("timeout")
            // Rate limiting — backends emit the words, not a bare "429".
            || lower.contains("rate limit")
            || lower.contains("too many requests")
    }

    /// Execute an event-triggered agent, retrying TRANSIENT failures with up
    /// to `retries` inline retries (a 5-second backoff between attempts).
    ///
    /// The agent clone and event data are reused across attempts. A
    /// permanent failure is returned after its first attempt — see
    /// [`EventRunOutcome::PermanentFailure`].
    pub(super) async fn execute_with_retry(
        executor: &AgentExecutor,
        agent: AiAgent,
        event_data: EventTriggerData,
        retries: u32,
        agent_id: &str,
    ) -> EventRunOutcome {
        Self::execute_with_retry_with_backoff(
            executor,
            agent,
            event_data,
            retries,
            agent_id,
            Self::EVENT_RETRY_BACKOFF,
        )
        .await
    }

    /// [`Self::execute_with_retry`] with the backoff made explicit, so tests
    /// do not sit through it.
    pub(super) async fn execute_with_retry_with_backoff(
        executor: &AgentExecutor,
        agent: AiAgent,
        event_data: EventTriggerData,
        retries: u32,
        agent_id: &str,
        backoff: std::time::Duration,
    ) -> EventRunOutcome {
        let mut attempt: u32 = 0;
        loop {
            attempt += 1;
            match executor
                .execute_agent(agent.clone(), Some(event_data.clone()), None)
                .await
            {
                Ok(record) => {
                    tracing::debug!(
                        agent_id = %agent_id,
                        execution_id = %record.id,
                        status = ?record.status,
                        attempt = attempt,
                        "Event-triggered agent execution completed"
                    );
                    // execute_agent reports LLM/tool failures as Ok(Failed-record)
                    // (only storage-level errors return Err). Treating every Ok
                    // as success made the inline retry and the caller's
                    // cooldown-clear dead code — transient LLM faults got no
                    // retry and the 60s window stayed locked.
                    //
                    // Vision Partial (undetermined): do not replay and do not
                    // clear cooldown — same input cannot grow eyes in 5s.
                    if record.status == neomind_storage::ExecutionStatus::Partial {
                        tracing::info!(
                            agent_id = %agent_id,
                            execution_id = %record.id,
                            "Event-triggered execution Partial (vision undetermined) — not retrying"
                        );
                        return EventRunOutcome::PermanentFailure;
                    }
                    if record.status != neomind_storage::ExecutionStatus::Failed {
                        return EventRunOutcome::Completed;
                    }
                    // Daily-cap refusals arrive as Failed records (the
                    // budget check lives inside execute_agent). Retrying a
                    // midnight-resetting cap from a 5s backoff ladder just
                    // burns attempts — skip cleanly; tomorrow's first
                    // event proceeds under a fresh budget.
                    if record
                        .error
                        .as_deref()
                        .map(|e| e.to_lowercase().contains("daily run cap"))
                        .unwrap_or(false)
                    {
                        tracing::info!(
                            agent_id = %agent_id,
                            execution_id = %record.id,
                            "Event trigger refused by daily run cap — skipping (not a failure)"
                        );
                        return EventRunOutcome::Completed;
                    }
                    // Empty-event skips are Completed in execute_agent; if a
                    // Failed record still carries the skip marker, do not
                    // 5s×N replay / breaker-storm it.
                    if record
                        .result
                        .as_ref()
                        .is_some_and(super::is_empty_event_skip)
                    {
                        tracing::info!(
                            agent_id = %agent_id,
                            execution_id = %record.id,
                            "Event trigger empty-data skip — not retrying"
                        );
                        return EventRunOutcome::Completed;
                    }
                    // Vision reject / partial business failure after a
                    // successful message send / device control must not be
                    // replayed — that would duplicate side effects.
                    if record
                        .result
                        .as_ref()
                        .is_some_and(|r| r.actions_executed.iter().any(|a| a.success))
                    {
                        tracing::warn!(
                            agent_id = %agent_id,
                            execution_id = %record.id,
                            "Event-triggered execution Failed after successful side effects — not retrying"
                        );
                        return EventRunOutcome::PermanentFailure;
                    }
                    let transient = Self::is_retryable_event_failure(record.error.as_deref());
                    if transient && attempt <= retries {
                        tracing::warn!(
                            agent_id = %agent_id,
                            execution_id = %record.id,
                            attempt = attempt,
                            retries = retries,
                            "Event-triggered execution FAILED (transient) — retrying after backoff"
                        );
                        tokio::time::sleep(backoff).await;
                        continue;
                    }
                    if transient {
                        tracing::error!(
                            agent_id = %agent_id,
                            execution_id = %record.id,
                            attempt = attempt,
                            "Event-triggered execution failed (status=Failed) after all retries"
                        );
                        return EventRunOutcome::TransientFailure;
                    }
                    tracing::error!(
                        agent_id = %agent_id,
                        execution_id = %record.id,
                        error = ?record.error,
                        "Event-triggered execution failed (status=Failed) — permanent, not replaying"
                    );
                    return EventRunOutcome::PermanentFailure;
                }
                Err(e) => {
                    // Daily-cap refusals are a budget pause, not a fault:
                    // retrying within the backoff window just burns 5s per
                    // attempt against a cap that resets at midnight. The
                    // listener's cooldown still applies, so tomorrow's first
                    // event proceeds normally.
                    if e.to_string().to_lowercase().contains("daily run cap") {
                        tracing::info!(
                            agent_id = %agent_id,
                            error = %e,
                            "Event trigger refused by daily run cap — skipping (not a failure)"
                        );
                        return EventRunOutcome::Completed;
                    }
                    // Busy, not broken: "already executing" means another run
                    // (an earlier admission from the same burst, or a manual
                    // invoke) holds the agent. Retrying after the 5s backoff
                    // queued a phantom second run behind it — with an
                    // on-demand cooldown (0s) every burst turned into a
                    // back-to-back run queue. Drop instead: the next event
                    // after the in-flight run finishes re-triggers
                    // naturally. Mirrors the scheduler's treatment.
                    if e.to_string().to_lowercase().contains("already executing") {
                        tracing::debug!(
                            agent_id = %agent_id,
                            "Event admission found the agent busy — dropping (in-flight run owns it)"
                        );
                        return EventRunOutcome::Completed;
                    }
                    // Mirror the Failed-status branch: only transport faults
                    // replay. Permanent Err (context overflow, bad key, …)
                    // must not burn the retry budget just because attempt
                    // is still ≤ retries.
                    let transient = Self::is_retryable_event_failure(Some(&e.to_string()));
                    if transient && attempt <= retries {
                        tracing::warn!(
                            agent_id = %agent_id,
                            attempt = attempt,
                            retries = retries,
                            error = %e,
                            "Event-triggered agent execution failed (transient) — retrying after backoff"
                        );
                        tokio::time::sleep(backoff).await;
                        continue;
                    }
                    tracing::error!(
                        agent_id = %agent_id,
                        attempt = attempt,
                        error = %e,
                        "Event-triggered agent execution failed (final)"
                    );
                    if transient {
                        return EventRunOutcome::TransientFailure;
                    }
                    return EventRunOutcome::PermanentFailure;
                }
            }
        }
    }

    /// Decide whether one event fires `agent`, folding it into the agent's
    /// `all` window when the filter has one (M2-1).
    ///
    /// `any` keeps the pre-M2 behaviour exactly — the first matching source
    /// fires immediately. `all` fires only on the event that completes the
    /// window. A filter the parser does not recognise falls back to the
    /// resource bindings, which is what it has always done.
    async fn event_fires(
        &self,
        agent: &AiAgent,
        source_type: &str,
        source_id: &str,
        field: &str,
        now: i64,
    ) -> bool {
        let filter = match agent.schedule.parsed_event_filter() {
            Some(filter) => filter,
            None => {
                return self
                    .matches_data_source_filter(agent, source_type, source_id, field)
                    .await
            }
        };

        if filter
            .any
            .iter()
            .any(|spec| spec.matches_event(source_type, source_id, field))
        {
            self.clear_event_window(&agent.id);
            return true;
        }

        if filter.all.is_empty() {
            return false;
        }

        let state = self.event_windows.read().get(&agent.id).cloned();
        match filter.observe(state.as_ref(), source_type, source_id, field, now) {
            neomind_storage::agents::WindowOutcome::Fire => {
                // The window is NOT consumed here: the caller may still skip
                // this firing (debounce merge, cooldown, spawn failure), and
                // clearing up front silently discarded a genuinely-completed
                // AND — the aggregation held, but no run ever happened and no
                // later event could re-complete the window. The caller clears
                // once the run is actually admitted.
                true
            }
            neomind_storage::agents::WindowOutcome::Pending(next) => {
                match next {
                    Some(window) => {
                        self.event_windows.write().insert(agent.id.clone(), window);
                    }
                    // Nothing to carry forward — make sure no stale window is
                    // left behind to be completed by some later event.
                    None => self.clear_event_window(&agent.id),
                }
                false
            }
        }
    }

    /// Drop an agent's open `all` window. Called whenever the agent fires, so a
    /// half-collected window cannot complete straight after a run and produce
    /// a duplicate execution.
    fn clear_event_window(&self, agent_id: &str) {
        self.event_windows.write().remove(agent_id);
    }

    /// Check if a data source update matches an agent's trigger conditions.
    /// For event-type agents: prefers event_filter.sources, falls back to resource bindings.
    /// Agents without any trigger source will NOT be triggered by data events.
    async fn matches_data_source_filter(
        &self,
        agent: &AiAgent,
        source_type: &str,
        source_id: &str,
        field: &str,
    ) -> bool {
        // Build the expected compound resource ID
        let compound_id = format!("{}:{}", source_id, field);

        // 1. Check event_filter.sources — explicit trigger configuration
        // Format: {"sources": [{"type": "device", "id": "sensor-01"}, {"type": "extension", "id": "weather"}]}
        if let Some(ref filter_json) = agent.schedule.event_filter {
            if let Ok(filter) = serde_json::from_str::<serde_json::Value>(filter_json) {
                // New sources-based matching
                if let Some(sources) = filter.get("sources").and_then(|v| v.as_array()) {
                    if !sources.is_empty() {
                        let matches_source = sources.iter().any(|s| {
                            let s_type = match s.get("type").and_then(|v| v.as_str()) {
                                Some(t) if !t.is_empty() => t,
                                _ => return false, // skip entries with missing/empty type
                            };
                            let s_id = s.get("id").and_then(|v| v.as_str()).unwrap_or("");
                            let s_field = s.get("field").and_then(|v| v.as_str());

                            if s_type != source_type {
                                return false;
                            }
                            // If id is "all", match any source of this type
                            if s_id == "all" {
                                return true;
                            }
                            // Empty id without explicit "all" is ambiguous — skip
                            if s_id.is_empty() {
                                return false;
                            }
                            if s_id != source_id {
                                return false;
                            }
                            // If field specified, must match exactly
                            if let Some(f) = s_field {
                                if !f.is_empty() && f != field {
                                    return false;
                                }
                            }
                            true
                        });

                        // When explicit sources are configured, ONLY use them —
                        // do NOT fall through to resource bindings.
                        return matches_source;
                    }
                }

                // Legacy event_type-based matching (backward compat)
                if let Some(event_type) = filter.get("event_type").and_then(|v| v.as_str()) {
                    if event_type == "device.metric" {
                        if let Some(filter_device) =
                            filter.get("device_id").and_then(|v| v.as_str())
                        {
                            if (filter_device == "all" || filter_device == source_id)
                                && source_type == "device"
                            {
                                return true;
                            }
                        }
                    } else if event_type == "extension.output" {
                        if let Some(filter_ext) =
                            filter.get("extension_id").and_then(|v| v.as_str())
                        {
                            if (filter_ext == "all" || filter_ext == source_id)
                                && source_type == "extension"
                            {
                                return true;
                            }
                        }
                    }
                }
            }
        }

        // 2. Fallback: check resource bindings (backward compat for agents
        //    without explicit event_filter.sources)
        let has_matching_resource = agent.resources.iter().any(|r| {
            match r.resource_type {
                ResourceType::Device => source_type == "device" && r.resource_id == source_id,
                ResourceType::Metric => {
                    if source_type == "device" {
                        if r.resource_id.contains(':') {
                            // Exact match: "device_id:metric" == "device_id:field"
                            if r.resource_id == compound_id {
                                return true;
                            }
                            // Suffix match: resource "device_id:image" matches field "values.image"
                            // Split resource_id into (res_device, res_field) and compare
                            let parts: Vec<&str> = r.resource_id.splitn(2, ':').collect();
                            if parts.len() == 2 {
                                let res_device = parts[0];
                                let res_field = parts[1];
                                res_device == source_id
                                    && (field == res_field
                                        || field.ends_with(&format!(".{}", res_field)))
                            } else {
                                false
                            }
                        } else {
                            r.resource_id == field
                                || field.ends_with(&format!(".{}", r.resource_id))
                        }
                    } else {
                        false
                    }
                }
                ResourceType::ExtensionMetric => {
                    if source_type == "extension" {
                        let ext_metric_id = format!("{}:{}", source_id, field);
                        r.resource_id == source_id || r.resource_id == ext_metric_id
                    } else {
                        false
                    }
                }
                ResourceType::ExtensionTool => {
                    source_type == "extension" && r.resource_id == source_id
                }
                _ => false,
            }
        });

        if has_matching_resource {
            return true;
        }

        // No resources and no matching trigger sources
        tracing::debug!(
            agent_name = %agent.name,
            source_type = %source_type,
            source_id = %source_id,
            field = %field,
            resources = ?agent.resources.iter().map(|r| &r.resource_id).collect::<Vec<_>>(),
            "[EVENT] Agent {} no matching trigger source",
            agent.name
        );
        false
    }

    /// Refresh the cache of event-triggered agents. Loads Active AND Error
    /// agents: Error agents are invisible to every scheduled probe (their
    /// `next_execution` is `i64::MAX`), so without loading them here the
    /// breaker would be a silent death sentence for event agents — the
    /// probe gate in `check_and_trigger_data_event` is their only way back.
    async fn refresh_event_agents(&self) {
        // No status filter: paused/stopped are excluded below. Agent counts
        // are small; the in-memory filter is equivalent to two queries.
        let filter = neomind_storage::AgentFilter::default();

        if let Ok(agents) = self.store.query_agents(filter).await {
            let total_loaded = agents.len();
            let event_agents: HashMap<String, AiAgent> = agents
                .into_iter()
                .filter(|a| {
                    matches!(
                        a.schedule.schedule_type,
                        neomind_storage::ScheduleType::Event
                    ) && matches!(
                        a.status,
                        neomind_storage::AgentStatus::Active | neomind_storage::AgentStatus::Error
                    )
                })
                .map(|a| (a.id.clone(), a))
                .collect();

            let mut cache = self.event_agents.write().await;
            let previous_count = cache.len();
            *cache = event_agents;

            tracing::debug!(
                total_loaded_agents = total_loaded,
                event_triggered_agents = cache.len(),
                previous_count = previous_count,
                "[EVENT] Refreshed event-triggered agents cache"
            );

            // Log each event-triggered agent for debugging
            for (id, agent) in cache.iter() {
                tracing::debug!(
                    agent_id = %id,
                    agent_name = %agent.name,
                    resource_count = agent.resources.len(),
                    "[EVENT] Event-triggered agent: {} with {} resources",
                    agent.name,
                    agent.resources.len()
                );
            }
        }
    }

    /// Remove an agent from the event-triggered agents cache.
    ///
    /// This should be called when an agent is deleted to immediately remove it
    /// from the cache, preventing it from being triggered by events before the
    /// next scheduled refresh.
    pub async fn remove_event_agent(&self, agent_id: &str) {
        let mut cache = self.event_agents.write().await;
        if cache.remove(agent_id).is_some() {
            tracing::debug!(
                agent_id = %agent_id,
                "[EVENT] Removed agent from event-triggered cache"
            );
        }
    }
}
