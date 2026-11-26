//! ExecutionCoordinator — sole admission point.

pub mod quota;
mod runtime_state;
pub mod scheduler;

pub use runtime_state::AgentRuntimeState;
pub use scheduler::AgentScheduler;

use std::sync::Arc;

use async_trait::async_trait;

use crate::error::{AgentError, Result};
use crate::model::{AgentStatus, CallerInput, RunResponse};
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

impl SubmitSource {
    /// Wire value persisted on execution rows / exposed as `trigger_type`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Schedule => "schedule",
            Self::Event => "event",
            Self::Manual => "manual",
            Self::Rule => "rule",
            Self::Extension => "extension",
        }
    }
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
        self.submit_with_caller(agent_id, source, None).await
    }

    pub async fn submit_with_caller(
        &self,
        agent_id: &str,
        source: SubmitSource,
        caller: Option<CallerInput>,
    ) -> Result<RunResponse> {
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

        // Daily limit (auto triggers only) — business day in config TZ.
        let mut runtime = self.store.get_runtime(agent_id)?;
        let spec = self.store.get_spec(agent_id)?;
        let tz = quota::quota_timezone(&spec.pacing, &spec.trigger);
        let now = self.clock.now_ms();
        quota::roll_quota_day(&mut runtime.stats, now, &tz);
        if let Some(max) = spec.pacing.max_runs_per_day {
            if runtime.stats.runs_today >= max as u64 && !matches!(source, SubmitSource::Manual) {
                let day = runtime
                    .stats
                    .quota_day
                    .clone()
                    .unwrap_or_else(|| "unknown".into());
                runtime.stats.skipped_daily_limit += 1;
                runtime.updated_at = now;
                self.store.set_runtime(runtime)?;
                return Err(AgentError::Other(format!(
                    "daily limit ({max}/day, day={day}, tz={tz})"
                )));
            }
        }
        // Persist rolled quota_day even when under the limit so restart sees it.
        runtime.updated_at = now;
        let status = runtime.status;
        let next_probe_at = runtime.next_probe_at;
        self.store.set_runtime(runtime)?;

        // Error probe backoff for auto triggers
        if !matches!(source, SubmitSource::Manual) && status == AgentStatus::Error {
            if let Some(next) = next_probe_at {
                if self.clock.now_ms() < next {
                    return Err(AgentError::Other("probe backoff".into()));
                }
            }
        }

        // Scheduled/event/rule submits never carry ask text — only Manual.
        let caller = if matches!(source, SubmitSource::Manual) {
            caller
        } else {
            None
        };
        self.service
            .invoke_with_source_and_caller(agent_id, source, caller)
            .await
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
