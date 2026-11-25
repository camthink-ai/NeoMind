//! ExecutionCoordinator — sole admission point.

mod runtime_state;
pub mod scheduler;

pub use runtime_state::AgentRuntimeState;
pub use scheduler::AgentScheduler;

use std::sync::Arc;

use async_trait::async_trait;

use crate::error::{AgentError, Result};
use crate::model::{AgentStatus, RunResponse};
use crate::ports::ClockPort;
use crate::service::AgentApplicationService;
use crate::store::InMemoryAgentStore;

pub struct ExecutionCoordinator {
    pub service: Arc<AgentApplicationService>,
    pub state: AgentRuntimeState,
    pub store: InMemoryAgentStore,
    pub clock: Arc<dyn ClockPort>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitSource {
    Schedule,
    Event,
    Manual,
    Rule,
    Extension,
}

/// Shared post-submit path used by the scheduler and host wrappers.
///
/// Host implements this so Cron/Interval ticks go through the same
/// `mirror_after_run` persistence as manual/event/rule submits.
#[async_trait]
pub trait AgentSubmitPort: Send + Sync {
    async fn submit(&self, agent_id: &str, source: SubmitSource) -> Result<RunResponse>;
}

#[async_trait]
impl AgentSubmitPort for ExecutionCoordinator {
    async fn submit(&self, agent_id: &str, source: SubmitSource) -> Result<RunResponse> {
        ExecutionCoordinator::submit(self, agent_id, source).await
    }
}

impl ExecutionCoordinator {
    pub fn new(
        service: Arc<AgentApplicationService>,
        store: InMemoryAgentStore,
        clock: Arc<dyn ClockPort>,
    ) -> Self {
        Self {
            service,
            state: AgentRuntimeState::new(),
            store,
            clock,
        }
    }

    pub async fn submit(&self, agent_id: &str, source: SubmitSource) -> Result<RunResponse> {
        let runtime = self.store.get_runtime(agent_id)?;
        if matches!(runtime.status, AgentStatus::Paused | AgentStatus::Stopped) {
            return Err(AgentError::NotRunnable(format!("{:?}", runtime.status)));
        }
        if !self.state.try_enter(agent_id) {
            return Err(AgentError::Busy);
        }
        let _guard = InflightGuard {
            state: &self.state,
            id: agent_id.to_string(),
        };

        // Daily limit (auto triggers only)
        let mut runtime = self.store.get_runtime(agent_id)?;
        let spec = self.store.get_spec(agent_id)?;
        if let Some(max) = spec.pacing.max_runs_per_day {
            if runtime.stats.total_runs >= max as u64 && !matches!(source, SubmitSource::Manual) {
                runtime.stats.skipped_daily_limit += 1;
                runtime.updated_at = self.clock.now_ms();
                self.store.set_runtime(runtime)?;
                return Err(AgentError::Other("daily limit".into()));
            }
        }

        // Error probe backoff for auto triggers
        if !matches!(source, SubmitSource::Manual) && runtime.status == AgentStatus::Error {
            if let Some(next) = runtime.next_probe_at {
                if self.clock.now_ms() < next {
                    return Err(AgentError::Other("probe backoff".into()));
                }
            }
        }

        self.service.invoke(agent_id).await
    }
}

struct InflightGuard<'a> {
    state: &'a AgentRuntimeState,
    id: String,
}

impl Drop for InflightGuard<'_> {
    fn drop(&mut self) {
        self.state.leave(&self.id);
    }
}
