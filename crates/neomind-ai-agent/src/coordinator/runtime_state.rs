//! In-flight / cancel handles for ExecutionCoordinator and AgentApplicationService.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use tokio_util::sync::CancellationToken;

#[derive(Clone, Default)]
pub struct AgentRuntimeState {
    inner: Arc<Mutex<RuntimeStateInner>>,
}

#[derive(Default)]
struct RuntimeStateInner {
    in_flight: HashSet<String>,
    next_run_seq: u64,
    tokens: HashMap<String, Vec<(u64, CancellationToken)>>,
}

/// RAII guard that unregisters an active run's [`CancellationToken`] on drop.
pub struct RunCancelGuard {
    state: AgentRuntimeState,
    agent_id: String,
    seq: u64,
    token: CancellationToken,
}

impl RunCancelGuard {
    pub fn token(&self) -> CancellationToken {
        self.token.clone()
    }
}

impl Drop for RunCancelGuard {
    fn drop(&mut self) {
        self.state.untrack_run(&self.agent_id, self.seq);
    }
}

impl AgentRuntimeState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn try_enter(&self, agent_id: &str) -> bool {
        let mut g = self.inner.lock().expect("lock");
        if !g.in_flight.insert(agent_id.to_string()) {
            return false;
        }
        true
    }

    pub fn leave(&self, agent_id: &str) {
        self.inner.lock().expect("lock").in_flight.remove(agent_id);
    }

    pub fn is_in_flight(&self, agent_id: &str) -> bool {
        self.inner
            .lock()
            .expect("lock")
            .in_flight
            .contains(agent_id)
    }

    /// Register a per-run [`CancellationToken`] for `agent_id` until the guard drops.
    pub fn track_run(&self, agent_id: &str) -> RunCancelGuard {
        let token = CancellationToken::new();
        let seq = {
            let mut g = self.inner.lock().expect("lock");
            g.next_run_seq = g.next_run_seq.wrapping_add(1);
            let seq = g.next_run_seq;
            g.tokens
                .entry(agent_id.to_string())
                .or_default()
                .push((seq, token.clone()));
            seq
        };
        RunCancelGuard {
            state: self.clone(),
            agent_id: agent_id.to_string(),
            seq,
            token,
        }
    }

    /// Cancel all active runs for `agent_id` (e.g. on Pause / Stop / Delete).
    pub fn cancel(&self, agent_id: &str) {
        let tokens: Vec<CancellationToken> = {
            let g = self.inner.lock().expect("lock");
            g.tokens
                .get(agent_id)
                .map(|list| list.iter().map(|(_, t)| t.clone()).collect())
                .unwrap_or_default()
        };
        for tok in tokens {
            tok.cancel();
        }
    }

    fn untrack_run(&self, agent_id: &str, seq: u64) {
        let mut g = self.inner.lock().expect("lock");
        let remove_key = if let Some(list) = g.tokens.get_mut(agent_id) {
            list.retain(|(s, _)| *s != seq);
            list.is_empty()
        } else {
            false
        };
        if remove_key {
            g.tokens.remove(agent_id);
        }
    }
}
