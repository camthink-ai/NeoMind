//! Post-run pipeline: CAS status updates and memory writer stubs.

mod state_machine;
pub use state_machine::{apply_operator_status, CasOutcome};

use crate::model::{AgentRuntimeRecord, AgentStatus};
use crate::store::InMemoryAgentStore;

pub struct PostRunPipeline {
    pub store: InMemoryAgentStore,
}

impl PostRunPipeline {
    pub fn cas_status(
        &self,
        agent_id: &str,
        expected: AgentStatus,
        next: AgentStatus,
        now_ms: i64,
    ) -> CasOutcome {
        let Ok(mut rt) = self.store.get_runtime(agent_id) else {
            return CasOutcome::Missing;
        };
        if rt.status != expected {
            return CasOutcome::Conflict { current: rt.status };
        }
        rt.status = next;
        rt.updated_at = now_ms;
        let _ = self.store.set_runtime(rt);
        CasOutcome::Applied
    }

    pub fn force_operator_pause(&self, agent_id: &str, now_ms: i64) -> Result<(), String> {
        let mut rt: AgentRuntimeRecord = self
            .store
            .get_runtime(agent_id)
            .map_err(|e| e.to_string())?;
        rt.status = AgentStatus::Paused;
        rt.updated_at = now_ms;
        self.store.set_runtime(rt).map_err(|e| e.to_string())
    }
}
