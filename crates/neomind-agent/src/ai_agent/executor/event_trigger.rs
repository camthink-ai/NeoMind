//! Event-triggered agent execution.
//!
//! Handles matching incoming data events to agents and spawning
//! background executions for matching event-type agents.

use super::*;

impl AgentExecutor {
    /// Prune stale entries from the event dedup map.
    /// Entries older than 360 s (6× the 60 s cooldown) are removed.
    async fn cleanup_stale_dedup_entries(&self) {
        let now = chrono::Utc::now().timestamp();
        let mut recent = self.recent_executions.write().await;
        recent.retain(|_, &mut ts| now - ts < 360);
    }

    /// Mark a manual execution of an event-type agent in the dedup map.
    ///
    /// Without this, `execute_agent_now` runs without registering in
    /// `recent_executions`, so the next real event for any of the agent's
    /// bound sources can fire within the 60 s cooldown window — yielding a
    /// duplicate execution (duplicate journal entry, potential duplicate
    /// notification, wasted LLM tokens). Manual execution should cool the
    /// same window.
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

        // Cool every bound source so any incoming event for one of them
        // is deduped. The event-trigger path keys on
        // `{agent_id}:{source_type}:{source_id}` (line 265), so we
        // pre-insert entries for each candidate the agent would match.
        // This is symmetric with how the event path itself behaves after
        // a successful firing.
        let candidates = Self::extract_trigger_candidates(&agent);
        let now = chrono::Utc::now().timestamp();
        let mut recent = self.recent_executions.write().await;

        // If the agent has no concrete candidates (e.g. only `"all"` wildcards
        // in its event_filter), fall back to a synthetic agent-scoped key.
        // This doesn't prevent a real event from firing (no matching key to
        // consult), but at least records that a manual run happened — useful
        // for debugging and gives the cleanup pass something to prune.
        if candidates.is_empty() {
            let key = format!("{}:manual:wildcard", agent_id);
            recent.insert(key, now);
        } else {
            for (s_type, s_id, _field) in &candidates {
                // Field is intentionally NOT in the dedup key — matches
                // the event-trigger path's keying convention, so a real
                // event firing on the same `(agent, source)` is properly
                // deduped regardless of which specific field triggered it.
                let key = format!("{}:{}:{}", agent_id, s_type, s_id);
                recent.insert(key, now);
            }
        }

        // Opportunistic cleanup — mirrors the event-trigger entry path.
        recent.retain(|_, &mut ts| now - ts < 360);

        tracing::debug!(
            agent_id = %agent_id,
            candidate_count = candidates.len(),
            "Marked manual execution in event dedup map"
        );
    }

    /// Check if an event should trigger any agent and execute it (legacy device-only entry point).
    pub async fn check_and_trigger_event(
        &self,
        device_id: String,
        metric: &str,
        value: &MetricValue,
    ) -> AgentResult<()> {
        // Refresh event-triggered agents cache
        self.refresh_event_agents().await;

        let event_agents = self.event_agents.read().await;

        tracing::debug!(
            device_id = %device_id,
            metric = %metric,
            event_agent_count = event_agents.len(),
            "[EVENT] Checking device event against {} event-triggered agents",
            event_agents.len()
        );

        // Clone device_id for use in spawned tasks
        let device_id_for_spawn = device_id.clone();

        // Clean up old entries from recent_executions (older than cooldown window)
        self.cleanup_stale_dedup_entries().await;
        let now = chrono::Utc::now().timestamp();

        for (_agent_id, agent) in event_agents.iter() {
            // Check if this agent has event-based schedule
            if matches!(
                agent.schedule.schedule_type,
                neomind_storage::ScheduleType::Event
            ) {
                // Check if agent's event filter matches this event
                if self
                    .matches_data_source_filter(agent, "device", &device_id, metric)
                    .await
                {
                    // Cooldown: one execution per (agent, source) per 60s window
                    const COOLDOWN_SECS: i64 = 60;
                    let dedup_key = format!("{}:device:{}", agent.id, device_id);
                    let recent = self.recent_executions.read().await;
                    let is_duplicate = recent
                        .get(&dedup_key)
                        .map(|&timestamp| now - timestamp < COOLDOWN_SECS)
                        .unwrap_or(false);
                    drop(recent);

                    if is_duplicate {
                        tracing::info!(
                            agent_name = %agent.name,
                            device_id = %device_id,
                            metric = %metric,
                            "Skipping event-triggered execution (cooldown: {}s)",
                            COOLDOWN_SECS
                        );
                        continue;
                    }

                    // Clone dedup_key before it's moved into the recent_executions map;
                    // the spawned task needs a copy to clear the cooldown on failure.
                    let dedup_key_clone = dedup_key.clone();

                    // Mark this execution as recent
                    {
                        let mut recent = self.recent_executions.write().await;
                        recent.insert(dedup_key, now);
                    }

                    tracing::debug!(
                        agent_name = %agent.name,
                        device_id = %device_id,
                        metric = %metric,
                        "Event-triggered agent execution"
                    );

                    // Clone the agent and event data for execution
                    let agent_clone = agent.clone();
                    let metric_clone = metric.to_string();
                    let value_clone = value.clone();
                    let device_id_for_task = device_id_for_spawn.clone();
                    let timestamp = chrono::Utc::now().timestamp();

                    // Build executor config for spawned task
                    let executor_config = self.build_spawn_config(agent);
                    let agent_id_for_log = agent.id.clone();
                    let recent_executions_clone = self.recent_executions.clone();

                    let handle = tokio::spawn(async move {
                        // Acquire the GLOBAL execution semaphore first (WAIT):
                        // scheduled executions hold this same permit, and event
                        // bursts used to stack past the global bound because
                        // only the per-backend permit was held. Held for the
                        // whole run (dropped when this task ends).
                        let _global_permit = match executor_config.execution_semaphore.clone() {
                            Some(sem) => sem.acquire_owned().await.ok(),
                            None => None,
                        };
                        // Acquire per-backend semaphore (WAIT, not fail)
                        Self::acquire_backend_permit(
                            &executor_config.backend_semaphores,
                            &agent_id_for_log,
                            &agent_clone
                                .llm_backend_id
                                .clone()
                                .unwrap_or_else(|| "default".to_string()),
                        )
                        .await;

                        // Create event trigger data
                        let event_trigger_data = EventTriggerData {
                            source: DataSourceRef {
                                source_type: "device".to_string(),
                                source_id: device_id_for_task,
                                field: metric_clone,
                            },
                            value: value_clone,
                            timestamp,
                        };

                        match AgentExecutor::new(executor_config).await {
                            Ok(executor) => {
                                tracing::debug!(
                                    agent_id = %agent_id_for_log,
                                    trigger_device = %event_trigger_data.source.source_id,
                                    trigger_metric = %event_trigger_data.source.field,
                                    "Executing event-triggered agent with event data"
                                );

                                // Execute the agent with event data (includes the triggering metric value directly).
                                // On failure, perform one inline retry with a short backoff; if the retry
                                // also fails, clear the cooldown marker so transient errors (API hiccups,
                                // network blips) don't lock out subsequent events for 60 seconds.
                                let result = Self::execute_with_retry(
                                    &executor,
                                    agent_clone,
                                    event_trigger_data,
                                    1,
                                    &agent_id_for_log,
                                )
                                .await;

                                if result.is_err() {
                                    let mut recent = recent_executions_clone.write().await;
                                    recent.remove(&dedup_key_clone);
                                    tracing::info!(
                                        agent_id = %agent_id_for_log,
                                        dedup_key = %dedup_key_clone,
                                        "Cleared event cooldown after failed execution"
                                    );
                                }
                            }
                            Err(e) => {
                                tracing::error!(
                                    agent_id = %agent_id_for_log,
                                    error = %e,
                                    "Failed to create executor for event-triggered agent"
                                );
                                // Executor creation failed — clear cooldown so the next event can retry.
                                let mut recent = recent_executions_clone.write().await;
                                recent.remove(&dedup_key_clone);
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
            }
        }

        Ok(())
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

        for (_agent_id, agent) in event_agents.iter() {
            // Check if this agent has event-based schedule
            if !matches!(
                agent.schedule.schedule_type,
                neomind_storage::ScheduleType::Event
            ) {
                continue;
            }

            // Check if agent's data source filter matches this event
            if !self
                .matches_data_source_filter(agent, source_type, &source_id, &field)
                .await
            {
                continue;
            }

            // Cooldown: one execution per (agent, source) per 60s window
            const COOLDOWN_SECS: i64 = 60;
            let dedup_key = format!("{}:{}:{}", agent.id, source_type, source_id);
            let recent = self.recent_executions.read().await;
            let is_duplicate = recent
                .get(&dedup_key)
                .map(|&timestamp| now - timestamp < COOLDOWN_SECS)
                .unwrap_or(false);
            drop(recent);

            if is_duplicate {
                tracing::info!(
                    agent_name = %agent.name,
                    source_type = %source_type,
                    source_id = %source_id,
                    field = %field,
                    "Skipping data event-triggered execution (cooldown: {}s)",
                    COOLDOWN_SECS
                );
                continue;
            }

            // Clone dedup_key before it's moved into the recent_executions map;
            // the spawned task needs a copy to clear the cooldown on failure.
            let dedup_key_clone = dedup_key.clone();

            // Mark this execution as recent
            {
                let mut recent = self.recent_executions.write().await;
                recent.insert(dedup_key, now);
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
                // Acquire per-backend semaphore (WAIT, not fail)
                Self::acquire_backend_permit(
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
                        let result = Self::execute_with_retry(
                            &executor,
                            agent_clone,
                            event_trigger_data,
                            1,
                            &agent_id_for_log,
                        )
                        .await;

                        if result.is_err() {
                            let mut recent = recent_executions_clone.write().await;
                            recent.remove(&dedup_key_clone);
                            tracing::info!(
                                agent_id = %agent_id_for_log,
                                dedup_key = %dedup_key_clone,
                                "Cleared event cooldown after failed execution"
                            );
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
        let candidates = Self::extract_trigger_candidates(agent);

        for (src_type, src_id, field) in candidates {
            match ts.query_latest(&src_id, &field).await {
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

    /// Extract candidate `(source_type, source_id, field)` tuples from an agent's
    /// trigger configuration — first from `event_filter.sources`, then from
    /// resource bindings (Metric / ExtensionMetric). Sources with `id == "all"`
    /// or empty fields are skipped (they cannot be resolved to a concrete query).
    fn extract_trigger_candidates(agent: &AiAgent) -> Vec<(String, String, String)> {
        let mut out: Vec<(String, String, String)> = Vec::new();

        // 1. event_filter.sources
        if let Some(ref filter_json) = agent.schedule.event_filter {
            if let Ok(filter) = serde_json::from_str::<serde_json::Value>(filter_json) {
                if let Some(sources) = filter.get("sources").and_then(|v| v.as_array()) {
                    for s in sources {
                        let s_type = match s.get("type").and_then(|v| v.as_str()) {
                            Some(t) if !t.is_empty() => t.to_string(),
                            _ => continue,
                        };
                        let s_id = s.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        if s_id.is_empty() || s_id == "all" {
                            continue;
                        }
                        let s_field = match s.get("field").and_then(|v| v.as_str()) {
                            Some(f) if !f.is_empty() => f.to_string(),
                            _ => continue,
                        };
                        out.push((s_type, s_id.to_string(), s_field));
                    }
                }
            }
        }

        // 2. Fallback: resource bindings (device:metric or extension:metric)
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

    /// Execute an event-triggered agent with up to `retries` inline retries.
    ///
    /// Between attempts, a 5-second backoff is applied. The agent clone and event
    /// data are reused across attempts. Returns `Ok(())` on success (logging the
    /// execution record), or `Err` if all attempts fail — callers should clear
    /// the event cooldown marker on `Err` so transient failures don't lock out
    /// subsequent events.
    async fn execute_with_retry(
        executor: &AgentExecutor,
        agent: AiAgent,
        event_data: EventTriggerData,
        retries: u32,
        agent_id: &str,
    ) -> Result<(), crate::error::NeoMindError> {
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
                    if record.status == neomind_storage::ExecutionStatus::Failed {
                        if attempt <= retries {
                            tracing::warn!(
                                agent_id = %agent_id,
                                execution_id = %record.id,
                                attempt = attempt,
                                retries = retries,
                                "Event-triggered execution FAILED — retrying after 5s backoff"
                            );
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                            continue;
                        }
                        tracing::error!(
                            agent_id = %agent_id,
                            execution_id = %record.id,
                            attempt = attempt,
                            "Event-triggered execution failed (status=Failed) after all retries"
                        );
                        return Err(crate::error::NeoMindError::Llm(format!(
                            "event-triggered execution failed (status=Failed) after {} attempts",
                            attempt
                        )));
                    }
                    return Ok(());
                }
                Err(e) => {
                    if attempt <= retries {
                        tracing::warn!(
                            agent_id = %agent_id,
                            attempt = attempt,
                            retries = retries,
                            error = %e,
                            "Event-triggered agent execution failed, retrying after 5s backoff"
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        continue;
                    }
                    tracing::error!(
                        agent_id = %agent_id,
                        attempt = attempt,
                        error = %e,
                        "Event-triggered agent execution failed (final)"
                    );
                    return Err(e);
                }
            }
        }
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

    /// Refresh the cache of event-triggered agents.
    async fn refresh_event_agents(&self) {
        let filter = neomind_storage::AgentFilter {
            status: Some(neomind_storage::AgentStatus::Active),
            ..Default::default()
        };

        if let Ok(agents) = self.store.query_agents(filter).await {
            let total_active = agents.len();
            let event_agents: HashMap<String, AiAgent> = agents
                .into_iter()
                .filter(|a| {
                    matches!(
                        a.schedule.schedule_type,
                        neomind_storage::ScheduleType::Event
                    )
                })
                .map(|a| (a.id.clone(), a))
                .collect();

            let mut cache = self.event_agents.write().await;
            let previous_count = cache.len();
            *cache = event_agents;

            tracing::debug!(
                total_active_agents = total_active,
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
