//! In-flight / cancel handles for ExecutionCoordinator.

use std::collections::HashSet;
use std::sync::Mutex;

#[derive(Default)]
pub struct AgentRuntimeState {
    in_flight: Mutex<HashSet<String>>,
}

impl AgentRuntimeState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn try_enter(&self, agent_id: &str) -> bool {
        let mut g = self.in_flight.lock().expect("lock");
        g.insert(agent_id.to_string())
    }

    pub fn leave(&self, agent_id: &str) {
        self.in_flight.lock().expect("lock").remove(agent_id);
    }

    pub fn is_in_flight(&self, agent_id: &str) -> bool {
        self.in_flight.lock().expect("lock").contains(agent_id)
    }
}
