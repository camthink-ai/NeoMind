//! In-memory / file-backed agent stores used by M1 (schema version stays 1).
//!
//! Five logical tables mirror the planned redb layout without activating
//! `CURRENT_SCHEMA_VERSION = 2`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::error::{AgentError, Result};
use crate::model::{
    ActionDeliveryRecord, AgentExecutionRecord, AgentRuntimeRecord, AgentSpec, AgentStats,
    AgentStatus, DeliveryState,
};

/// Optional durable sink invoked after a successful in-memory `save_execution`.
///
/// Production installs this via [`crate::persist::DurableAgentStore::with_store`] so
/// pre-send `IntentPending` rows land in the same redb the next process opens.
pub type ExecutionPersistFn = Arc<dyn Fn(&AgentExecutionRecord) -> Result<()> + Send + Sync>;

#[derive(Clone, Default)]
pub struct InMemoryAgentStore {
    inner: Arc<Mutex<StoreInner>>,
}

#[derive(Default)]
struct StoreInner {
    specs: HashMap<String, AgentSpec>,
    runtime: HashMap<String, AgentRuntimeRecord>,
    executions: HashMap<String, AgentExecutionRecord>,
    /// Formal (non-dry-run) execution ids per agent, ordered.
    formal_history: HashMap<String, Vec<String>>,
    /// Unique delivery intents: (execution_id, destination, fingerprint) → record
    deliveries: HashMap<String, ActionDeliveryRecord>,
    knowledge: HashMap<String, String>,
    #[allow(dead_code)]
    user_messages: HashMap<String, Vec<String>>,
    /// When set (hosted V2), every `save_execution` mirrors to durable storage.
    execution_persist: Option<ExecutionPersistFn>,
}

fn delivery_key(execution_id: &str, destination: &str, fingerprint: &str) -> String {
    format!("{execution_id}\0{destination}\0{fingerprint}")
}

impl InMemoryAgentStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Install a durable mirror for `save_execution` (shared across clones).
    pub fn set_execution_persist(&self, persist: ExecutionPersistFn) {
        self.inner.lock().expect("store lock").execution_persist = Some(persist);
    }

    pub fn save_spec(&self, spec: AgentSpec, now_ms: i64) -> Result<AgentSpec> {
        let mut g = self.inner.lock().expect("store lock");
        let id = spec.id.clone();
        g.specs.insert(id.clone(), spec.clone());
        g.runtime
            .entry(id.clone())
            .or_insert_with(|| AgentRuntimeRecord {
                agent_id: id,
                status: AgentStatus::Active,
                stats: AgentStats::default(),
                error_message: None,
                next_probe_at: None,
                updated_at: now_ms,
            });
        Ok(spec)
    }

    pub fn get_spec(&self, id: &str) -> Result<AgentSpec> {
        self.inner
            .lock()
            .expect("store lock")
            .specs
            .get(id)
            .cloned()
            .ok_or_else(|| AgentError::NotFound(id.into()))
    }

    pub fn list_specs(&self) -> Vec<AgentSpec> {
        self.inner
            .lock()
            .expect("store lock")
            .specs
            .values()
            .cloned()
            .collect()
    }

    pub fn delete_spec(&self, id: &str) -> Result<()> {
        let mut g = self.inner.lock().expect("store lock");
        if g.specs.remove(id).is_none() {
            return Err(AgentError::NotFound(id.into()));
        }
        g.runtime.remove(id);
        if let Some(exec_ids) = g.formal_history.remove(id) {
            for exec_id in exec_ids {
                g.executions.remove(&exec_id);
            }
        }
        g.executions.retain(|_, exec| exec.agent_id != id);
        Ok(())
    }

    pub fn get_runtime(&self, id: &str) -> Result<AgentRuntimeRecord> {
        self.inner
            .lock()
            .expect("store lock")
            .runtime
            .get(id)
            .cloned()
            .ok_or_else(|| AgentError::NotFound(id.into()))
    }

    pub fn set_runtime(&self, runtime: AgentRuntimeRecord) -> Result<()> {
        let mut g = self.inner.lock().expect("store lock");
        g.runtime.insert(runtime.agent_id.clone(), runtime);
        Ok(())
    }

    pub fn save_execution(&self, exec: AgentExecutionRecord) -> Result<()> {
        let persist = {
            let mut g = self.inner.lock().expect("store lock");
            let agent_id = exec.agent_id.clone();
            let id = exec.id.clone();
            let dry_run = exec.dry_run;
            g.executions.insert(id.clone(), exec.clone());
            // Formal history is idempotent by execution ID — re-saving the same
            // row (pre-settle skeleton → post-settle receipt) must not duplicate.
            if !dry_run {
                const MAX_INMEM_FORMAL_PER_AGENT: usize = 50;
                let hist = g.formal_history.entry(agent_id).or_default();
                if !hist.iter().any(|existing| existing == &id) {
                    hist.push(id);
                }
                let evicted: Vec<String> = if hist.len() > MAX_INMEM_FORMAL_PER_AGENT {
                    let excess = hist.len() - MAX_INMEM_FORMAL_PER_AGENT;
                    hist.drain(..excess).collect()
                } else {
                    Vec::new()
                };
                for old_id in evicted {
                    g.executions.remove(&old_id);
                }
            }
            g.execution_persist.clone()
        };
        // Persist outside the mem lock so a durable write cannot deadlock.
        if let Some(persist) = persist {
            persist(&exec)?;
        }
        Ok(())
    }

    pub fn get_execution(&self, id: &str) -> Result<AgentExecutionRecord> {
        self.inner
            .lock()
            .expect("store lock")
            .executions
            .get(id)
            .cloned()
            .ok_or_else(|| AgentError::NotFound(id.into()))
    }

    pub fn formal_history(&self, agent_id: &str) -> Vec<AgentExecutionRecord> {
        let g = self.inner.lock().expect("store lock");
        g.formal_history
            .get(agent_id)
            .into_iter()
            .flatten()
            .filter_map(|id| g.executions.get(id).cloned())
            .collect()
    }

    pub fn formal_history_len(&self, agent_id: &str) -> usize {
        self.inner
            .lock()
            .expect("store lock")
            .formal_history
            .get(agent_id)
            .map(|v| v.len())
            .unwrap_or(0)
    }

    /// Insert IntentPending uniquely. Returns true if this caller got send rights.
    pub fn try_claim_delivery(&self, record: ActionDeliveryRecord) -> Result<bool> {
        let mut g = self.inner.lock().expect("store lock");
        let key = delivery_key(
            &record.execution_id,
            &record.destination,
            &record.action_fingerprint,
        );
        if g.deliveries.contains_key(&key) {
            return Ok(false);
        }
        g.deliveries.insert(key, record);
        Ok(true)
    }

    pub fn update_delivery_state(
        &self,
        execution_id: &str,
        destination: &str,
        fingerprint: &str,
        state: DeliveryState,
    ) -> Result<()> {
        let mut g = self.inner.lock().expect("store lock");
        let key = delivery_key(execution_id, destination, fingerprint);
        let rec = g
            .deliveries
            .get_mut(&key)
            .ok_or_else(|| AgentError::NotFound(format!("delivery {key}")))?;
        rec.state = state;
        Ok(())
    }

    pub fn get_delivery(
        &self,
        execution_id: &str,
        destination: &str,
        fingerprint: &str,
    ) -> Option<ActionDeliveryRecord> {
        let g = self.inner.lock().expect("store lock");
        g.deliveries
            .get(&delivery_key(execution_id, destination, fingerprint))
            .cloned()
    }

    pub fn recover_stale_intents(&self) -> Vec<ActionDeliveryRecord> {
        let mut g = self.inner.lock().expect("store lock");
        let mut out = Vec::new();
        for rec in g.deliveries.values_mut() {
            if matches!(rec.state, DeliveryState::IntentPending) {
                rec.state = DeliveryState::OutcomeUnknown {
                    reason: "crashed_before_receipt".into(),
                };
                out.push(rec.clone());
            }
        }
        out
    }

    pub fn set_knowledge(&self, agent_id: &str, body: String) {
        self.inner
            .lock()
            .expect("store lock")
            .knowledge
            .insert(agent_id.into(), body);
    }

    pub fn knowledge(&self, agent_id: &str) -> Option<String> {
        self.inner
            .lock()
            .expect("store lock")
            .knowledge
            .get(agent_id)
            .cloned()
    }
}
