//! Pure CAS status transition helpers.

use crate::model::AgentStatus;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CasOutcome {
    Applied,
    Conflict { current: AgentStatus },
    Missing,
}

/// Operator pause/stop/delete always wins over in-flight completion.
pub fn apply_operator_status(current: AgentStatus, operator: AgentStatus) -> AgentStatus {
    match operator {
        AgentStatus::Paused | AgentStatus::Stopped => operator,
        _ => current,
    }
}
