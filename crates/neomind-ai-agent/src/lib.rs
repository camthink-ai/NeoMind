//! NeoMind standalone managed AI agent runtime (clean-slate).
//!
//! M0 extracted shared loop primitives into `neomind-llm-loop`. This crate owns
//! AgentSpec, ports, InputPipeline, runners, side-effect boundary, and the
//! application service used by M1 isolated tests (T12+) before M2 web cutover.

pub mod coordinator;
pub mod error;
pub mod input;
pub mod model;
pub mod persist;
pub mod ports;
pub mod post_run;
pub mod preprocess;
pub mod runner;
pub mod service;
pub mod side_effects;
pub mod storage_bridge;
pub mod store;

pub use coordinator::{AgentScheduler, AgentSubmitPort, ExecutionCoordinator, SubmitSource};
pub use error::{AgentError, Result};
pub use model::*;
pub use persist::DurableAgentStore;
pub use ports::*;
pub use runner::LOCAL_BRIEF_BACKEND;
pub use service::AgentApplicationService;
pub use store::InMemoryAgentStore;

/// Test/harness builder used by T12–T14 and behavior_matrix.
pub mod harness {
    use std::sync::Arc;

    use crate::input::{CountingSourcePort, InputPipeline};
    use crate::ports::{FixedClock, LocalBriefLlm};
    use crate::runner::SinglePassRunner;
    use crate::service::{AgentApplicationService, NotifyCounter, WriteCounter};
    use crate::side_effects::{FakeNotifyPort, FakeSideEffectPort, Notifier, SideEffectBoundary};
    use crate::store::InMemoryAgentStore;

    pub struct TestHarness {
        pub service: Arc<AgentApplicationService>,
        pub store: InMemoryAgentStore,
        pub clock: Arc<FixedClock>,
        pub sources: Arc<CountingSourcePort>,
        pub notify: Arc<FakeNotifyPort>,
        pub writes: Arc<FakeSideEffectPort>,
    }

    impl NotifyCounter for FakeNotifyPort {
        fn count(&self) -> u64 {
            FakeNotifyPort::count(self)
        }
    }

    impl WriteCounter for FakeSideEffectPort {
        fn count(&self) -> u64 {
            FakeSideEffectPort::call_count(self)
        }
    }

    pub fn build_harness(now_ms: i64) -> TestHarness {
        let store = InMemoryAgentStore::new();
        let clock = Arc::new(FixedClock::new(now_ms));
        let sources = Arc::new(CountingSourcePort::new());
        let notify = Arc::new(FakeNotifyPort::new());
        let writes = Arc::new(FakeSideEffectPort::new());
        let input = InputPipeline::new(sources.clone(), clock.clone());
        let runner = Arc::new(SinglePassRunner::new(Arc::new(LocalBriefLlm)));
        let side_effects = SideEffectBoundary {
            store: store.clone(),
            notifier: Notifier::new(notify.clone()),
            side_effects: writes.clone(),
            clock: clock.clone(),
        };
        let service = Arc::new(AgentApplicationService {
            store: store.clone(),
            clock: clock.clone(),
            input,
            runner,
            side_effects,
            notify_counter: notify.clone(),
            write_counter: writes.clone(),
        });
        TestHarness {
            service,
            store,
            clock,
            sources,
            notify,
            writes,
        }
    }
}
