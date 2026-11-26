//! Optional redb persistence for [`crate::store::InMemoryAgentStore`].
//!
//! M1 tests stay in-memory. M2 host assembly attaches an [`AgentStoreV2`] so
//! create/update/invoke survive process restarts while schema version is still 1
//! (soft-copy) or 2 (migrated).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use neomind_storage::agents_v2::{AgentExecutionRecordV2, AgentStoreV2, ExecutionStatusV2};

use crate::error::{AgentError, Result};
use crate::model::{
    AgentExecutionRecord, AgentRuntimeRecord, AgentSpec, AgentStatus, ExecutionStatus,
};
use crate::storage_bridge::{
    record_to_spec, runtime_to_storage, spec_to_record, status_from_storage,
};
use crate::store::InMemoryAgentStore;

/// Load every v2 spec (+ runtime) into an in-memory working set.
pub fn load_from_v2(v2: &AgentStoreV2, mem: &InMemoryAgentStore, now_ms: i64) -> Result<usize> {
    let specs = v2
        .list_specs()
        .map_err(|e| AgentError::Other(e.to_string()))?;
    let mut n = 0;
    for rec in specs {
        let spec = record_to_spec(&rec).map_err(AgentError::Other)?;
        let id = spec.id.clone();
        mem.save_spec(spec, now_ms)?;
        if let Ok(Some(rt)) = v2.get_runtime(&id) {
            let mapped = AgentRuntimeRecord {
                agent_id: rt.agent_id,
                status: status_from_storage(rt.status),
                stats: crate::model::AgentStats {
                    total_runs: rt.stats.total_runs,
                    successful_runs: rt.stats.successful_runs,
                    failed_runs: rt.stats.failed_runs,
                    partial_runs: rt.stats.partial_runs,
                    skipped_daily_limit: rt.stats.skipped_daily_limit,
                    skipped_permit_timeout: rt.stats.skipped_permit_timeout,
                    consecutive_failures: rt.stats.consecutive_failures,
                    runs_today: rt.stats.runs_today,
                    quota_day: rt.stats.quota_day,
                    last_execution_at: rt.stats.last_execution_at,
                    last_execution_id: rt.stats.last_execution_id,
                    avg_duration_ms: rt.stats.avg_duration_ms,
                    last_duration_ms: rt.stats.last_duration_ms,
                },
                error_message: rt.error_message,
                next_probe_at: rt.next_probe_at,
                updated_at: rt.updated_at,
            };
            mem.set_runtime(mapped)?;
        }
        n += 1;
    }
    Ok(n)
}

/// Persist a spec + runtime snapshot to the five-table store.
pub fn persist_spec(
    v2: &AgentStoreV2,
    spec: &AgentSpec,
    runtime: &AgentRuntimeRecord,
) -> Result<()> {
    v2.save_spec(&spec_to_record(spec))
        .map_err(|e| AgentError::Other(e.to_string()))?;
    v2.set_runtime(&runtime_to_storage(runtime))
        .map_err(|e| AgentError::Other(e.to_string()))?;
    Ok(())
}

fn exec_status_to_v2(s: &ExecutionStatus) -> ExecutionStatusV2 {
    match s {
        ExecutionStatus::Completed => ExecutionStatusV2::Completed,
        ExecutionStatus::Partial => ExecutionStatusV2::Partial,
        ExecutionStatus::Skipped => ExecutionStatusV2::Skipped,
        ExecutionStatus::Failed => ExecutionStatusV2::Failed,
    }
}

/// Persist an execution row (formal or dry-run) into agent_executions.
pub fn persist_execution(v2: &AgentStoreV2, exec: &AgentExecutionRecord) -> Result<()> {
    let row = AgentExecutionRecordV2 {
        id: exec.id.clone(),
        agent_id: exec.agent_id.clone(),
        config_version: exec.config_version,
        status: exec_status_to_v2(&exec.status),
        started_at: exec.started_at,
        finished_at: exec.finished_at,
        is_probe: exec.is_probe,
        dry_run: exec.dry_run,
        trigger_source: exec.trigger_source.clone(),
        stale_config: exec.stale_config,
        evidence: serde_json::to_value(&exec.evidence).unwrap_or_default(),
        deliveries: exec
            .deliveries
            .iter()
            .filter_map(|d| serde_json::to_value(d).ok())
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect(),
        brief_text: exec.brief_text.clone(),
        conclusion: exec.conclusion.clone(),
        structured_result: exec.structured_result.clone(),
        error: exec.error.clone(),
        alert_verdict: exec.alert_verdict.as_ref().map(|v| format!("{v:?}")),
    };
    v2.save_execution(&row)
        .map_err(|e| AgentError::Other(e.to_string()))?;
    Ok(())
}

/// Host-facing handle: in-memory hot path + optional durable mirror.
#[derive(Clone)]
pub struct DurableAgentStore {
    pub mem: InMemoryAgentStore,
    pub v2: Option<Arc<AgentStoreV2>>,
    /// When set, the next durable delete fails once (tests inject store failure).
    pub fail_next_v2_delete: Arc<AtomicBool>,
}

impl DurableAgentStore {
    pub fn memory_only() -> Self {
        Self {
            mem: InMemoryAgentStore::new(),
            v2: None,
            fail_next_v2_delete: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn with_v2(v2: Arc<AgentStoreV2>, now_ms: i64) -> Result<Self> {
        // Restart recovery: IntentPending → OutcomeUnknown (never auto-resend).
        match v2.recover_stale_intents() {
            Ok(recovered) if !recovered.is_empty() => {
                tracing::warn!(
                    count = recovered.len(),
                    "marked stale IntentPending deliveries as OutcomeUnknown"
                );
            }
            Err(e) => tracing::warn!("recover_stale_intents failed: {e}"),
            _ => {}
        }
        let mem = InMemoryAgentStore::new();
        load_from_v2(&v2, &mem, now_ms)?;
        // Production hot path (`HostedAiAgentRuntime`) injects `durable.mem` into
        // SideEffectBoundary / AgentApplicationService. Wire save_execution so
        // pre-send IntentPending is flushed to the same redb restart reopens —
        // not mem-only until mirror_after_run.
        let v2_persist = v2.clone();
        mem.set_execution_persist(Arc::new(move |exec| persist_execution(&v2_persist, exec)));
        Ok(Self {
            mem,
            v2: Some(v2),
            fail_next_v2_delete: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn save_spec(&self, spec: AgentSpec, now_ms: i64) -> Result<AgentSpec> {
        let saved = self.mem.save_spec(spec, now_ms)?;
        if let Some(v2) = &self.v2 {
            let rt = self.mem.get_runtime(&saved.id)?;
            persist_spec(v2, &saved, &rt)?;
        }
        Ok(saved)
    }

    /// Delete from mem + durable store. Disk errors surface and mem is restored.
    pub fn delete_spec(&self, id: &str) -> Result<()> {
        let spec = self.mem.get_spec(id)?;
        let runtime = self.mem.get_runtime(id).ok();
        self.mem.delete_spec(id)?;

        if let Some(v2) = &self.v2 {
            if self.fail_next_v2_delete.swap(false, Ordering::SeqCst) {
                self.restore_mem(&spec, runtime.as_ref())?;
                return Err(AgentError::Storage(
                    "injected durable delete failure".into(),
                ));
            }
            if let Err(e) = v2.delete_agent_rows(id) {
                self.restore_mem(&spec, runtime.as_ref())?;
                return Err(AgentError::Storage(e.to_string()));
            }
        }
        Ok(())
    }

    fn restore_mem(&self, spec: &AgentSpec, runtime: Option<&AgentRuntimeRecord>) -> Result<()> {
        let now = runtime.map(|r| r.updated_at).unwrap_or(0);
        self.mem.save_spec(spec.clone(), now)?;
        if let Some(rt) = runtime {
            self.mem.set_runtime(rt.clone())?;
        }
        Ok(())
    }

    pub fn mirror_runtime(&self, runtime: &AgentRuntimeRecord) -> Result<()> {
        self.mem.set_runtime(runtime.clone())?;
        if let Some(v2) = &self.v2 {
            v2.set_runtime(&runtime_to_storage(runtime))
                .map_err(|e| AgentError::Other(e.to_string()))?;
        }
        Ok(())
    }

    /// Load `id` from durable V2 into the hot set if missing (no AiAgent map).
    pub fn ensure_loaded(&self, id: &str, now_ms: i64) -> Result<()> {
        if self.mem.get_spec(id).is_ok() {
            return Ok(());
        }
        self.reload_from_v2(id, now_ms)
    }

    /// Always refresh hot-set spec+runtime from durable V2 (after legacy save/update).
    pub fn reload_from_v2(&self, id: &str, now_ms: i64) -> Result<()> {
        let Some(v2) = &self.v2 else {
            return Err(AgentError::NotFound(id.into()));
        };
        let rec = v2
            .get_spec(id)
            .map_err(|e| AgentError::Other(e.to_string()))?
            .ok_or_else(|| AgentError::NotFound(id.into()))?;
        let spec = record_to_spec(&rec).map_err(AgentError::Other)?;
        self.mem.save_spec(spec, now_ms)?;
        if let Ok(Some(rt)) = v2.get_runtime(id) {
            let mapped = AgentRuntimeRecord {
                agent_id: rt.agent_id,
                status: status_from_storage(rt.status),
                stats: crate::model::AgentStats {
                    total_runs: rt.stats.total_runs,
                    successful_runs: rt.stats.successful_runs,
                    failed_runs: rt.stats.failed_runs,
                    partial_runs: rt.stats.partial_runs,
                    skipped_daily_limit: rt.stats.skipped_daily_limit,
                    skipped_permit_timeout: rt.stats.skipped_permit_timeout,
                    consecutive_failures: rt.stats.consecutive_failures,
                    runs_today: rt.stats.runs_today,
                    quota_day: rt.stats.quota_day,
                    last_execution_at: rt.stats.last_execution_at,
                    last_execution_id: rt.stats.last_execution_id,
                    avg_duration_ms: rt.stats.avg_duration_ms,
                    last_duration_ms: rt.stats.last_duration_ms,
                },
                error_message: rt.error_message,
                next_probe_at: rt.next_probe_at,
                updated_at: rt.updated_at,
            };
            self.mem.set_runtime(mapped)?;
        }
        Ok(())
    }

    pub fn mirror_execution(&self, exec: &AgentExecutionRecord) -> Result<()> {
        self.mem.save_execution(exec.clone())?;
        if let Some(v2) = &self.v2 {
            persist_execution(v2, exec)?;
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn status_active_ids(&self) -> Vec<String> {
        self.mem
            .list_specs()
            .into_iter()
            .filter(|s| {
                self.mem
                    .get_runtime(&s.id)
                    .map(|r| matches!(r.status, AgentStatus::Active))
                    .unwrap_or(false)
            })
            .map(|s| s.id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        InputContract, NotifyPolicy, OutputContract, PacingPolicy, ToolPolicy, TriggerSpec,
    };
    use neomind_storage::AgentStore;

    fn sample_spec(id: &str) -> AgentSpec {
        AgentSpec {
            id: id.into(),
            name: id.into(),
            description: None,
            user_prompt: "ping".into(),
            llm_backend_id: None,
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract::default(),
            tool_policy: ToolPolicy::Disabled,
            output: OutputContract::default(),
            notify: NotifyPolicy::default(),
            controlled_actions: vec![],
            config_version: 1,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn delete_spec_surfaces_injected_disk_failure_and_reconciles_mem() {
        let agent_store = AgentStore::memory().unwrap();
        let v2 = AgentStoreV2::from_arc(agent_store.db()).unwrap();
        let durable = DurableAgentStore::with_v2(v2, 1000).unwrap();
        durable.save_spec(sample_spec("del-1"), 1000).unwrap();
        assert!(durable.mem.get_spec("del-1").is_ok());

        durable.fail_next_v2_delete.store(true, Ordering::SeqCst);
        let err = durable.delete_spec("del-1").unwrap_err();
        assert!(matches!(err, AgentError::Storage(_)));
        // Mem reconciled — still present after failed durable delete.
        assert!(durable.mem.get_spec("del-1").is_ok());
        assert!(durable
            .v2
            .as_ref()
            .unwrap()
            .get_spec("del-1")
            .unwrap()
            .is_some());
    }

    #[test]
    fn delete_spec_success_missing_after_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agents.redb");
        let agent_store = AgentStore::open(&path).unwrap();
        let v2 = AgentStoreV2::from_arc(agent_store.db()).unwrap();
        let durable = DurableAgentStore::with_v2(v2, 2000).unwrap();
        durable.save_spec(sample_spec("gone"), 2000).unwrap();
        durable.delete_spec("gone").unwrap();
        drop(durable);
        drop(agent_store);

        let reopened = AgentStore::open(&path).unwrap();
        let v2b = AgentStoreV2::from_arc(reopened.db()).unwrap();
        assert!(v2b.get_spec("gone").unwrap().is_none());
        assert!(v2b.get_runtime("gone").unwrap().is_none());
    }
}
