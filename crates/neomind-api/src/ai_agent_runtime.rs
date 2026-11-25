//! Host assembly for the clean-slate [`neomind_ai_agent`] runtime (M2).
//!
//! Opens the shared agents.redb via [`AgentStoreV2`], loads a
//! [`DurableAgentStore`], and builds [`AgentApplicationService`] +
//! [`ExecutionCoordinator`] with production host ports (LLM / sources /
//! notify / side-effects).

use std::sync::Arc;

use async_trait::async_trait;
use neomind_ai_agent::coordinator::{
    AgentScheduler, AgentSubmitPort, ExecutionCoordinator, SubmitSource,
};
use neomind_ai_agent::error::{AgentError, Result as AgentResult};
use neomind_ai_agent::input::InputPipeline;
use neomind_ai_agent::model::{
    AgentSpec, AgentSpecDraft, AgentSpecPatch, AgentStatus, EffectivePlan, RunResponse,
};
use neomind_ai_agent::persist::{persist_spec, DurableAgentStore};
use neomind_ai_agent::ports::{ClockPort, NotifyPort, SourceReadPort, SystemClock};
use neomind_ai_agent::runner::SinglePassRunner;
use neomind_ai_agent::service::AgentApplicationService;
use neomind_ai_agent::side_effects::{Notifier, SideEffectBoundary};
use neomind_storage::agents_v2::AgentStoreV2;
use neomind_storage::AgentStore;

use crate::agent_ports::{
    HostAgentDeps, HostLlmPort, HostNotifyPort, HostSideEffectPort, HostSourceReadPort,
};

/// Hosted clean-slate agent runtime (service + coordinator + durable store).
pub struct HostedAiAgentRuntime {
    pub service: Arc<AgentApplicationService>,
    pub coordinator: Arc<ExecutionCoordinator>,
    pub durable: DurableAgentStore,
    pub clock: Arc<dyn ClockPort>,
    /// Production ports retained for acceptance counters / diagnostics.
    pub sources: Arc<HostSourceReadPort>,
    pub notify: Arc<HostNotifyPort>,
    pub side_effects: Arc<HostSideEffectPort>,
    pub llm: Arc<HostLlmPort>,
    scheduler: Arc<std::sync::Mutex<Option<Arc<AgentScheduler>>>>,
    started: std::sync::atomic::AtomicBool,
}

/// Schedules ticks through [`HostedAiAgentRuntime::submit`] so Cron/Interval
/// share `mirror_after_run` with manual/event/rule paths.
struct HostedSubmit(Arc<HostedAiAgentRuntime>);

#[async_trait]
impl AgentSubmitPort for HostedSubmit {
    async fn submit(&self, agent_id: &str, source: SubmitSource) -> AgentResult<RunResponse> {
        self.0.submit(agent_id, source).await
    }
}

impl HostedAiAgentRuntime {
    /// Open AgentStoreV2 on the shared agents.redb and build service with real host ports.
    pub fn build(agent_store: Arc<AgentStore>, deps: HostAgentDeps) -> AgentResult<Self> {
        let v2 = AgentStoreV2::from_arc(agent_store.db())
            .map_err(|e| AgentError::Storage(e.to_string()))?;
        let clock: Arc<dyn ClockPort> = Arc::new(SystemClock);
        let now = clock.now_ms();
        let durable = if neomind_storage::schema::CURRENT_SCHEMA_VERSION >= 2 {
            DurableAgentStore::with_v2(v2, now)?
        } else {
            DurableAgentStore {
                mem: neomind_ai_agent::store::InMemoryAgentStore::new(),
                v2: Some(v2),
                fail_next_v2_delete: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            }
        };

        let sources = Arc::new(HostSourceReadPort::new(deps.clone()));
        let notify = Arc::new(HostNotifyPort::new(deps.message_manager.clone()));
        let writes = Arc::new(HostSideEffectPort::new(
            deps.device_service.clone(),
            deps.extension_registry.clone(),
        ));
        let llm = Arc::new(HostLlmPort::new());

        let input = InputPipeline::new(sources.clone() as Arc<dyn SourceReadPort>, clock.clone());
        let runner = Arc::new(SinglePassRunner::new(llm.clone()));
        let side_effects = SideEffectBoundary {
            store: durable.mem.clone(),
            notifier: Notifier::new(notify.clone() as Arc<dyn NotifyPort>),
            side_effects: writes.clone(),
            clock: clock.clone(),
        };

        let service = Arc::new(AgentApplicationService {
            store: durable.mem.clone(),
            clock: clock.clone(),
            input,
            runner,
            side_effects,
            notify_counter: notify.clone() as Arc<dyn neomind_ai_agent::service::NotifyCounter>,
            write_counter: writes.clone() as Arc<dyn neomind_ai_agent::service::WriteCounter>,
        });

        let coordinator = Arc::new(ExecutionCoordinator::new(
            service.clone(),
            durable.mem.clone(),
            clock.clone(),
        ));

        Ok(Self {
            service,
            coordinator,
            durable,
            clock,
            sources,
            notify,
            side_effects: writes,
            llm,
            scheduler: Arc::new(std::sync::Mutex::new(None)),
            started: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// Start the Cron/Interval scheduler (1s tick). Idempotent.
    ///
    /// No-op while schema version stays at 1 (migrate not yet activated).
    pub fn start(self: &Arc<Self>) {
        if neomind_storage::schema::CURRENT_SCHEMA_VERSION < 2 {
            tracing::info!(
                "Clean-slate scheduler deferred until schema v2 (CURRENT_SCHEMA_VERSION={})",
                neomind_storage::schema::CURRENT_SCHEMA_VERSION
            );
            return;
        }
        if self.started.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let submit: Arc<dyn AgentSubmitPort> = Arc::new(HostedSubmit(self.clone()));
        let scheduler = Arc::new(AgentScheduler::new(
            submit,
            self.durable.mem.clone(),
            self.clock.clone(),
        ));
        *self.scheduler.lock().expect("scheduler") = Some(scheduler.clone());
        scheduler.start();
        tracing::info!("Clean-slate AgentScheduler started (schema v2)");
    }

    pub fn has_agent(&self, id: &str) -> bool {
        self.durable.mem.get_spec(id).is_ok()
    }

    /// Ensure `id` is in the hot set by loading from durable V2 (no AiAgent map).
    pub fn ensure_loaded(&self, id: &str) -> AgentResult<()> {
        self.durable.ensure_loaded(id, self.clock.now_ms())
    }

    /// Force-refresh hot set from durable V2 after a store write that bypassed
    /// [`Self::create`] / [`Self::update`].
    pub fn reload_from_v2(&self, id: &str) -> AgentResult<()> {
        self.durable.reload_from_v2(id, self.clock.now_ms())
    }

    /// Update hot-set + durable runtime status (Active/Paused/…).
    pub fn set_agent_status(&self, id: &str, status: AgentStatus) -> AgentResult<()> {
        let mut rt = self.durable.mem.get_runtime(id)?;
        rt.status = status;
        rt.updated_at = self.clock.now_ms();
        self.durable.mirror_runtime(&rt)
    }

    pub fn create(&self, draft: AgentSpecDraft) -> AgentResult<(AgentSpec, EffectivePlan)> {
        let (spec, plan) = self.service.create(draft)?;
        self.mirror_spec(&spec)?;
        Ok((spec, plan))
    }

    pub fn list(&self) -> Vec<(AgentSpec, EffectivePlan)> {
        self.service.list()
    }

    pub fn get(&self, id: &str) -> AgentResult<(AgentSpec, EffectivePlan)> {
        let spec = self.service.get(id)?;
        let plan = self.service.effective_plan(id)?;
        Ok((spec, plan))
    }

    /// Runtime status for Chat/list (no AiAgent map).
    pub fn runtime_status(&self, id: &str) -> Option<AgentStatus> {
        self.durable.mem.get_runtime(id).ok().map(|r| r.status)
    }

    /// Runtime stats snapshot for Chat/list DTOs.
    pub fn runtime_stats_json(&self, id: &str) -> serde_json::Value {
        match self.durable.mem.get_runtime(id) {
            Ok(rt) => serde_json::json!({
                "total_runs": rt.stats.total_runs,
                "successful_runs": rt.stats.successful_runs,
                "failed_runs": rt.stats.failed_runs,
                "partial_runs": rt.stats.partial_runs,
                "consecutive_failures": rt.stats.consecutive_failures,
                "last_execution_at": rt.stats.last_execution_at,
                "last_execution_id": rt.stats.last_execution_id,
                "error_message": rt.error_message,
            }),
            Err(_) => serde_json::json!({}),
        }
    }

    pub fn update(
        &self,
        id: &str,
        patch: AgentSpecPatch,
    ) -> AgentResult<(AgentSpec, EffectivePlan)> {
        let (spec, plan) = self.service.update(id, patch)?;
        self.mirror_spec(&spec)?;
        Ok((spec, plan))
    }

    pub fn delete(&self, id: &str) -> AgentResult<()> {
        self.durable.delete_spec(id)
    }

    pub async fn invoke(&self, id: &str) -> AgentResult<RunResponse> {
        let resp = self.coordinator.submit(id, SubmitSource::Manual).await?;
        self.mirror_after_run(id, &resp)?;
        Ok(resp)
    }

    pub async fn dry_run(&self, id: &str) -> AgentResult<RunResponse> {
        let resp = self.service.dry_run(id).await?;
        Ok(resp)
    }

    pub async fn submit(&self, id: &str, source: SubmitSource) -> AgentResult<RunResponse> {
        let resp = self.coordinator.submit(id, source).await?;
        self.mirror_after_run(id, &resp)?;
        Ok(resp)
    }

    pub fn explain_last(&self, id: &str) -> AgentResult<String> {
        self.service.explain_last(id)
    }

    fn mirror_spec(&self, spec: &AgentSpec) -> AgentResult<()> {
        if let Some(v2) = &self.durable.v2 {
            let rt = self.durable.mem.get_runtime(&spec.id)?;
            persist_spec(v2, spec, &rt)?;
        }
        Ok(())
    }

    fn mirror_after_run(&self, id: &str, resp: &RunResponse) -> AgentResult<()> {
        if let Some(exec) = &resp.execution {
            if !exec.dry_run {
                self.durable.mirror_execution(exec)?;
                if let Ok(rt) = self.durable.mem.get_runtime(id) {
                    self.durable.mirror_runtime(&rt)?;
                }
            }
        }
        Ok(())
    }
}

/// Convenience wrapper used by ServerState construction.
pub fn build_hosted_runtime(
    agent_store: Arc<AgentStore>,
    deps: HostAgentDeps,
) -> AgentResult<Arc<HostedAiAgentRuntime>> {
    let runtime = Arc::new(HostedAiAgentRuntime::build(agent_store, deps)?);
    Ok(runtime)
}

#[cfg(test)]
mod tests {
    use super::*;
    use neomind_ai_agent::model::{
        ControlledAction, EvidenceQuality, ExecutionStatus, FreshnessPolicy, InputContract,
        InputModality, InputRequirement, InputSourceRef, MissingPolicy, NotifyPolicy,
        OutputContract, PacingPolicy, ToolPolicy, TriggerSpec,
    };
    use neomind_ai_agent::ports::FixedClock;
    use neomind_ai_agent::runner::single_pass::LOCAL_BRIEF_BACKEND;
    use neomind_core::extension::system::{ExtensionCommand, ExtensionMetadata, MetricDefinition};
    use neomind_core::extension::{
        CommandEffect, CommandExecutionKind, CommandReplaySafety, Extension, ExtensionRegistry,
    };
    use neomind_core::EventBus;
    use neomind_devices::telemetry::DataPoint;
    use neomind_devices::{DeviceRegistry, DeviceService, MetricValue, TimeSeriesStorage};
    use neomind_messages::channels::MessageChannel;
    use neomind_messages::{MessageManager, Result as MsgResult};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};

    use crate::server::state::ExtensionMetricsStorage;

    struct CountingChannel {
        name: String,
        sends: AtomicU64,
    }

    #[async_trait]
    impl MessageChannel for CountingChannel {
        fn name(&self) -> &str {
            &self.name
        }
        fn channel_type(&self) -> &str {
            "test"
        }
        fn is_enabled(&self) -> bool {
            true
        }
        async fn send(&self, _message: &neomind_messages::Message) -> MsgResult<()> {
            self.sends.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    /// Fake extension: `query` is ReadOnly, `set` is Mutating — counts set calls.
    fn fake_rw_extension(set_calls: Arc<AtomicU64>) -> neomind_core::extension::DynExtension {
        struct FakeRw {
            set_calls: Arc<AtomicU64>,
        }
        #[async_trait]
        impl Extension for FakeRw {
            fn metadata(&self) -> &ExtensionMetadata {
                static M: OnceLock<ExtensionMetadata> = OnceLock::new();
                M.get_or_init(|| ExtensionMetadata::new("rw-ext", "RW Ext", "1.0.0"))
            }
            fn metrics(&self) -> Vec<MetricDefinition> {
                vec![]
            }
            fn commands(&self) -> Vec<ExtensionCommand> {
                vec![
                    ExtensionCommand {
                        name: "query".into(),
                        display_name: "query".into(),
                        description: String::new(),
                        payload_template: String::new(),
                        parameters: vec![],
                        fixed_values: Default::default(),
                        samples: vec![],
                        parameter_groups: vec![],
                        ai_exposed: true,
                        effect: Some(CommandEffect::ReadOnly),
                        replay_safety: Some(CommandReplaySafety::Safe),
                        execution: Some(CommandExecutionKind::Immediate),
                    },
                    ExtensionCommand {
                        name: "set".into(),
                        display_name: "set".into(),
                        description: String::new(),
                        payload_template: String::new(),
                        parameters: vec![],
                        fixed_values: Default::default(),
                        samples: vec![],
                        parameter_groups: vec![],
                        ai_exposed: true,
                        effect: Some(CommandEffect::Mutating),
                        replay_safety: Some(CommandReplaySafety::Unsafe),
                        execution: Some(CommandExecutionKind::Immediate),
                    },
                ]
            }
            async fn execute_command(
                &self,
                command: &str,
                _args: &serde_json::Value,
            ) -> neomind_core::extension::Result<serde_json::Value> {
                if command == "set" {
                    self.set_calls.fetch_add(1, Ordering::SeqCst);
                }
                Ok(serde_json::json!({ "ok": true, "cmd": command }))
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }
        Arc::new(tokio::sync::RwLock::new(Box::new(FakeRw { set_calls })))
    }

    /// Readonly weather extension whose `current` payload is swapped per test.
    fn fake_weather_extension(
        payload: Arc<Mutex<serde_json::Value>>,
    ) -> neomind_core::extension::DynExtension {
        struct FakeWeather {
            payload: Arc<Mutex<serde_json::Value>>,
        }
        #[async_trait]
        impl Extension for FakeWeather {
            fn metadata(&self) -> &ExtensionMetadata {
                static M: OnceLock<ExtensionMetadata> = OnceLock::new();
                M.get_or_init(|| ExtensionMetadata::new("weather_cn", "Weather", "1.0.0"))
            }
            fn metrics(&self) -> Vec<MetricDefinition> {
                vec![]
            }
            fn commands(&self) -> Vec<ExtensionCommand> {
                vec![ExtensionCommand {
                    name: "current".into(),
                    display_name: "current".into(),
                    description: String::new(),
                    payload_template: String::new(),
                    parameters: vec![],
                    fixed_values: Default::default(),
                    samples: vec![],
                    parameter_groups: vec![],
                    ai_exposed: true,
                    effect: Some(CommandEffect::ReadOnly),
                    replay_safety: Some(CommandReplaySafety::Safe),
                    execution: Some(CommandExecutionKind::Immediate),
                }]
            }
            async fn execute_command(
                &self,
                _command: &str,
                _args: &serde_json::Value,
            ) -> neomind_core::extension::Result<serde_json::Value> {
                Ok(self.payload.lock().unwrap().clone())
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }
        Arc::new(tokio::sync::RwLock::new(Box::new(FakeWeather { payload })))
    }

    async fn test_deps() -> (HostAgentDeps, Arc<CountingChannel>, Arc<TimeSeriesStorage>) {
        let mm = Arc::new(MessageManager::new());
        let ch = Arc::new(CountingChannel {
            name: "ops".into(),
            sends: AtomicU64::new(0),
        });
        mm.channels().await.read().await.register(ch.clone()).await;

        let telemetry = Arc::new(TimeSeriesStorage::memory().unwrap());
        let now = chrono::Utc::now().timestamp_millis();
        telemetry
            .write(
                "device:meter-1",
                "kw",
                DataPoint::new(now, MetricValue::Float(1.5)),
            )
            .await
            .unwrap();

        let bus = EventBus::new();
        let registry = Arc::new(DeviceRegistry::new());
        let device_service = Arc::new(DeviceService::new(registry, bus));
        device_service
            .set_telemetry_storage(telemetry.clone())
            .await;

        let ext_reg = Arc::new(ExtensionRegistry::new());
        let ext_metrics = Arc::new(ExtensionMetricsStorage::with_shared_storage(
            telemetry.clone(),
        ));

        (
            HostAgentDeps::new(mm, telemetry.clone(), device_service, ext_reg, ext_metrics),
            ch,
            telemetry,
        )
    }

    fn local_brief_draft(name: &str) -> AgentSpecDraft {
        AgentSpecDraft {
            name: name.into(),
            description: None,
            user_prompt: "report".into(),
            llm_backend_id: Some(LOCAL_BRIEF_BACKEND.into()),
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract::default(),
            tool_policy: ToolPolicy::Disabled,
            output: OutputContract::default(),
            notify: NotifyPolicy::default(),
            controlled_actions: vec![],
        }
    }

    #[tokio::test]
    async fn prod_builder_isolated_sinks_count_real_deliveries_and_reads() {
        let (deps, channel, _telemetry) = test_deps().await;
        let store = AgentStore::memory().unwrap();
        let runtime = HostedAiAgentRuntime::build(store, deps).unwrap();

        let mut draft = local_brief_draft("sink-check");
        draft.input = InputContract {
            requirements: vec![InputRequirement {
                key: "kw".into(),
                source: InputSourceRef::DeviceMetric {
                    device_id: "meter-1".into(),
                    field: "kw".into(),
                },
                operation: None,
                params: serde_json::json!({}),
                auth_scope: None,
                timeout_secs: 5,
                result_type: InputModality::Metric,
                freshness: FreshnessPolicy::Any,
                on_missing: MissingPolicy::ReportStale,
            }],
        };
        draft.notify = NotifyPolicy {
            channels: vec!["ops".into()],
            on_complete: true,
            on_alert: false,
            alert_depends_on: vec![],
            on_failure: true,
            pending_confirmation: false,
        };

        let (spec, _) = runtime.create(draft).unwrap();
        let resp = runtime.invoke(&spec.id).await.unwrap();
        assert!(resp.execution.is_some());
        assert!(runtime.sources.reads() >= 1);
        assert!(runtime.notify.deliveries() >= 1);
        assert!(channel.sends.load(Ordering::SeqCst) >= 1);
        assert_eq!(runtime.llm.requests(), 0, "local-brief skips HostLlmPort");
    }

    #[tokio::test]
    async fn dry_run_refuses_mutating_extension_ops_set_count_zero() {
        let set_calls = Arc::new(AtomicU64::new(0));
        let (mut deps, _, _) = test_deps().await;
        let ext_reg = Arc::new(ExtensionRegistry::new());
        ext_reg
            .register("rw-ext".into(), fake_rw_extension(set_calls.clone()))
            .await
            .unwrap();
        deps.extension_registry = ext_reg;

        let store = AgentStore::memory().unwrap();
        let runtime = HostedAiAgentRuntime::build(store, deps).unwrap();

        let mut draft = local_brief_draft("dry-ro");
        draft.input = InputContract {
            requirements: vec![
                InputRequirement {
                    key: "q".into(),
                    source: InputSourceRef::ExtensionInstance {
                        id: "rw-ext".into(),
                    },
                    operation: Some("query".into()),
                    params: serde_json::json!({}),
                    auth_scope: Some("extension:rw-ext:read".into()),
                    timeout_secs: 5,
                    result_type: InputModality::StructuredJson,
                    freshness: FreshnessPolicy::Any,
                    on_missing: MissingPolicy::ReportStale,
                },
                InputRequirement {
                    key: "w".into(),
                    source: InputSourceRef::ExtensionInstance {
                        id: "rw-ext".into(),
                    },
                    operation: Some("set".into()),
                    params: serde_json::json!({ "v": 1 }),
                    auth_scope: Some("extension:rw-ext:write".into()),
                    timeout_secs: 5,
                    result_type: InputModality::StructuredJson,
                    freshness: FreshnessPolicy::Any,
                    on_missing: MissingPolicy::ReportStale,
                },
            ],
        };

        let (spec, _) = runtime.create(draft).unwrap();
        let dry = runtime.dry_run(&spec.id).await.unwrap();
        let exec = dry.execution.as_ref().unwrap();
        let q = exec.evidence.items.iter().find(|i| i.key == "q").unwrap();
        assert!(
            matches!(q.quality, EvidenceQuality::Fresh),
            "query ReadOnly must succeed: {:?}",
            q.quality
        );
        let w = exec.evidence.items.iter().find(|i| i.key == "w").unwrap();
        assert!(
            !matches!(w.quality, EvidenceQuality::Fresh),
            "set must not succeed on input path: {:?}",
            w.quality
        );
        assert_eq!(
            set_calls.load(Ordering::SeqCst),
            0,
            "mutating set must never reach the extension receiver"
        );
    }

    #[tokio::test]
    async fn backend_5xx_settles_failed_not_fake_success() {
        let (deps, _, _) = test_deps().await;
        let store = AgentStore::memory().unwrap();
        let runtime = HostedAiAgentRuntime::build(store, deps).unwrap();
        runtime.llm.force_fail_next.store(true, Ordering::SeqCst);

        // Model task (not local-brief) — must call HostLlmPort and fail closed.
        let mut draft = local_brief_draft("need-model");
        draft.llm_backend_id = None;
        draft.notify = NotifyPolicy {
            channels: vec!["ops".into()],
            on_complete: true,
            on_alert: false,
            alert_depends_on: vec![],
            on_failure: false,
            pending_confirmation: false,
        };
        let (spec, _) = runtime.create(draft).unwrap();
        let resp = runtime.invoke(&spec.id).await.unwrap();
        let exec = resp.execution.as_ref().unwrap();
        assert_eq!(exec.status, ExecutionStatus::Failed);
        assert!(exec.brief_text.is_none() || exec.error.is_some());
        assert_eq!(runtime.llm.requests(), 1);
        assert_eq!(
            runtime.notify.deliveries(),
            0,
            "failed inference must not notify on_complete"
        );
    }

    #[tokio::test]
    async fn stale_weather_blocks_weather_dependent_action() {
        let (deps, _, telemetry) = test_deps().await;
        let store = AgentStore::memory().unwrap();
        let runtime = HostedAiAgentRuntime::build(store, deps).unwrap();
        let now = chrono::Utc::now().timestamp_millis();

        // Current meter window: several points in last 15m (Unix seconds —
        // matches MQTT/webhook/API telemetry writers).
        let now_s = now / 1000;
        let meter_points: Vec<_> = (0..5u64)
            .map(|i| {
                DataPoint::new(
                    now_s.saturating_sub((i as i64) * 60),
                    MetricValue::Float(8.0 + i as f64),
                )
            })
            .collect();
        telemetry
            .write_batch("device:meter-1", "power_kw", meter_points)
            .await
            .unwrap();
        // Sanity: second-unit range query must see the window.
        let probe = telemetry
            .query_limited(
                "device:meter-1",
                "power_kw",
                now_s - 900,
                now_s + 1,
                Some(60),
            )
            .await
            .unwrap();
        assert!(
            probe.len() >= 2,
            "precondition: meter window must have >=2 points, got {}",
            probe.len()
        );
        // Old weather cache (3h ago) beyond LatestWithin 1800s.
        telemetry
            .write(
                "extension:weather_cn",
                "current",
                DataPoint::new(now_s - 3 * 3_600, MetricValue::Float(30.0)),
            )
            .await
            .unwrap();
        let _ = telemetry.flush();

        let draft = AgentSpecDraft {
            name: "stale-weather".into(),
            description: None,
            user_prompt: "diagnose".into(),
            llm_backend_id: Some(LOCAL_BRIEF_BACKEND.into()),
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract {
                requirements: vec![
                    InputRequirement {
                        key: "meter_15m".into(),
                        source: InputSourceRef::DeviceMetric {
                            device_id: "meter-1".into(),
                            field: "power_kw".into(),
                        },
                        operation: None,
                        params: serde_json::json!({}),
                        auth_scope: None,
                        timeout_secs: 5,
                        result_type: InputModality::TimeSeries,
                        freshness: FreshnessPolicy::LookbackWindow {
                            lookback_secs: 900,
                            max_points: 60,
                        },
                        on_missing: MissingPolicy::Undetermined,
                    },
                    InputRequirement {
                        key: "weather".into(),
                        source: InputSourceRef::ExtensionInstance {
                            id: "weather_cn".into(),
                        },
                        operation: Some("current".into()),
                        params: serde_json::json!({}),
                        auth_scope: None,
                        timeout_secs: 5,
                        result_type: InputModality::StructuredJson,
                        freshness: FreshnessPolicy::LatestWithin { max_age_secs: 1800 },
                        on_missing: MissingPolicy::ReportStale,
                    },
                ],
            },
            tool_policy: ToolPolicy::Disabled,
            output: OutputContract::default(),
            notify: NotifyPolicy::default(),
            controlled_actions: vec![ControlledAction {
                name: "adjust_load".into(),
                destination: "ext-write-1".into(),
                depends_on: vec!["meter_15m".into(), "weather".into()],
                idempotent: false,
                params: serde_json::json!({}),
            }],
        };

        let (spec, _) = runtime.create(draft).unwrap();
        let resp = runtime.invoke(&spec.id).await.unwrap();
        let exec = resp.execution.as_ref().unwrap();
        let weather = exec
            .evidence
            .items
            .iter()
            .find(|i| i.key == "weather")
            .unwrap();
        assert!(
            matches!(weather.quality, EvidenceQuality::Stale),
            "old weather must be Stale, got {:?}",
            weather.quality
        );
        let meter = exec
            .evidence
            .items
            .iter()
            .find(|i| i.key == "meter_15m")
            .unwrap();
        assert!(
            matches!(meter.quality, EvidenceQuality::Fresh),
            "meter window must be usable: {:?}",
            meter.quality
        );
        assert_eq!(runtime.side_effects.invokes(), 0);
        assert!(
            exec.deliveries
                .iter()
                .any(|d| d.destination == "ext-write-1"
                    && matches!(
                        d.state,
                        neomind_ai_agent::model::DeliveryState::Rejected { .. }
                    )),
            "weather-dependent action must be blocked: {:?}",
            exec.deliveries
        );
    }

    #[tokio::test]
    async fn single_point_cannot_complete_lookback_window() {
        let (deps, _, telemetry) = test_deps().await;
        let store = AgentStore::memory().unwrap();
        let runtime = HostedAiAgentRuntime::build(store, deps).unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        telemetry
            .write(
                "device:meter-1",
                "power_kw",
                DataPoint::new(now / 1000 - 10, MetricValue::Float(9.0)),
            )
            .await
            .unwrap();
        let _ = telemetry.flush();

        let mut draft = local_brief_draft("one-point");
        draft.input = InputContract {
            requirements: vec![InputRequirement {
                key: "meter_15m".into(),
                source: InputSourceRef::DeviceMetric {
                    device_id: "meter-1".into(),
                    field: "power_kw".into(),
                },
                operation: None,
                params: serde_json::json!({}),
                auth_scope: None,
                timeout_secs: 5,
                result_type: InputModality::TimeSeries,
                freshness: FreshnessPolicy::LookbackWindow {
                    lookback_secs: 900,
                    max_points: 60,
                },
                on_missing: MissingPolicy::ReportStale,
            }],
        };
        let (spec, _) = runtime.create(draft).unwrap();
        let resp = runtime.invoke(&spec.id).await.unwrap();
        let item = resp
            .execution
            .as_ref()
            .unwrap()
            .evidence
            .items
            .iter()
            .find(|i| i.key == "meter_15m")
            .unwrap();
        assert!(
            !matches!(item.quality, EvidenceQuality::Fresh),
            "single point must not complete full-window calc: {:?}",
            item.quality
        );
    }

    #[tokio::test]
    async fn scheduler_tick_persists_execution_via_shared_submit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agents.redb");
        let store = AgentStore::open(&path).unwrap();
        let (deps, _ch, _) = test_deps().await;
        let runtime = Arc::new(HostedAiAgentRuntime::build(store.clone(), deps).unwrap());

        let clock = Arc::new(FixedClock::new(10_000));
        let mut draft = local_brief_draft("sched-persist");
        draft.trigger = TriggerSpec::Interval { every_secs: 1 };
        let (spec, _) = runtime.create(draft).unwrap();
        let submit: Arc<dyn AgentSubmitPort> = Arc::new(HostedSubmit(runtime.clone()));
        let scheduler = AgentScheduler::new(submit, runtime.durable.mem.clone(), clock.clone());
        scheduler.tick_once().await;
        for _ in 0..50 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            if runtime.durable.mem.formal_history_len(&spec.id) >= 1 {
                break;
            }
        }
        assert!(runtime.durable.mem.formal_history_len(&spec.id) >= 1);
        let exec_id = runtime
            .durable
            .mem
            .formal_history(&spec.id)
            .last()
            .map(|e| e.id.clone())
            .expect("execution id");

        drop(scheduler);
        drop(runtime);
        drop(store);

        let reopened = AgentStore::open(&path).unwrap();
        let v2 = AgentStoreV2::from_arc(reopened.db()).unwrap();
        let formal = v2.list_formal_executions(&spec.id).unwrap();
        assert!(!formal.is_empty());
        assert_eq!(formal.last().unwrap().id, exec_id);
    }

    #[tokio::test]
    async fn delete_surfaces_injected_fail_and_success_removes_on_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agents-del.redb");
        let store = AgentStore::open(&path).unwrap();
        let (deps, _, _) = test_deps().await;
        let runtime = HostedAiAgentRuntime::build(store.clone(), deps).unwrap();
        let (spec, _) = runtime.create(local_brief_draft("to-delete")).unwrap();

        runtime
            .durable
            .fail_next_v2_delete
            .store(true, Ordering::SeqCst);
        let err = runtime.delete(&spec.id).unwrap_err();
        assert!(matches!(err, AgentError::Storage(_)));
        assert!(runtime.has_agent(&spec.id));

        runtime.delete(&spec.id).unwrap();
        assert!(!runtime.has_agent(&spec.id));
        drop(runtime);
        drop(store);

        let reopened = AgentStore::open(&path).unwrap();
        let v2 = AgentStoreV2::from_arc(reopened.db()).unwrap();
        assert!(v2.get_spec(&spec.id).unwrap().is_none());
    }

    #[tokio::test]
    async fn host_lookback_matches_second_telemetry_window() {
        let (deps, _, telemetry) = test_deps().await;
        let port = HostSourceReadPort::new(deps);
        let now_s = chrono::Utc::now().timestamp();
        // Two in last 15m, one older than 15m — Unix seconds like MQTT/API.
        telemetry
            .write_batch(
                "device:meter-win",
                "power_kw",
                vec![
                    DataPoint::new(now_s - 60, MetricValue::Float(1.0)),
                    DataPoint::new(now_s - 120, MetricValue::Float(2.0)),
                    DataPoint::new(now_s - 2_000, MetricValue::Float(9.0)),
                ],
            )
            .await
            .unwrap();

        let params = serde_json::json!({
            "_freshness": {
                "kind": "lookback_window",
                "lookback_secs": 900,
                "max_points": 60
            },
            "lookback_secs": 900
        });
        let r = port
            .read_device_metric("meter-win", "power_kw", &params, 5)
            .await
            .unwrap();
        let points = r
            .value
            .get("points")
            .and_then(|p| p.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        assert_eq!(
            points, 2,
            "15m window must return exactly two in-window points"
        );
        let (start_ms, end_ms) = r.valid_window.expect("evidence ms window");
        let now_ms = chrono::Utc::now().timestamp_millis();
        assert!(
            end_ms >= now_ms - 5_000 && end_ms <= now_ms + 5_000,
            "valid_window end must be near now_ms: {end_ms} vs {now_ms}"
        );
        assert!(
            (end_ms - start_ms - 900_000).abs() < 5_000,
            "valid_window must span ~900s in ms: {start_ms}..{end_ms}"
        );
        assert!(
            r.observed_at.unwrap_or(0) >= 1_000_000_000_000,
            "observed_at should be normalized to ms"
        );
    }

    #[tokio::test]
    async fn extension_command_observed_at_from_payload_not_call_time() {
        let payload = Arc::new(Mutex::new(serde_json::json!({})));
        let (mut deps, _, _) = test_deps().await;
        let ext_reg = Arc::new(ExtensionRegistry::new());
        ext_reg
            .register("weather_cn".into(), fake_weather_extension(payload.clone()))
            .await
            .unwrap();
        deps.extension_registry = ext_reg;
        let store = AgentStore::memory().unwrap();
        let runtime = HostedAiAgentRuntime::build(store, deps).unwrap();

        let now_ms = chrono::Utc::now().timestamp_millis();
        let draft_for = |name: &str| {
            let mut d = local_brief_draft(name);
            d.input = InputContract {
                requirements: vec![InputRequirement {
                    key: "weather".into(),
                    source: InputSourceRef::ExtensionInstance {
                        id: "weather_cn".into(),
                    },
                    operation: Some("current".into()),
                    params: serde_json::json!({}),
                    auth_scope: None,
                    timeout_secs: 5,
                    result_type: InputModality::StructuredJson,
                    freshness: FreshnessPolicy::LatestWithin { max_age_secs: 900 },
                    on_missing: MissingPolicy::ReportStale,
                }],
            };
            d
        };

        // Fresh observation in payload (ms), well within 900s.
        *payload.lock().unwrap() = serde_json::json!({
            "temp": 22,
            "observed_at": now_ms - 30_000
        });
        let (spec, _) = runtime.create(draft_for("wx-fresh")).unwrap();
        let resp = runtime.invoke(&spec.id).await.unwrap();
        let item = resp
            .execution
            .as_ref()
            .unwrap()
            .evidence
            .items
            .iter()
            .find(|i| i.key == "weather")
            .unwrap();
        assert!(
            matches!(item.quality, EvidenceQuality::Fresh),
            "fresh payload obs must pass LatestWithin: {:?} observed_at={:?}",
            item.quality,
            item.observed_at
        );

        // Yesterday's cache in payload must be Stale (not call-time).
        *payload.lock().unwrap() = serde_json::json!({
            "temp": 22,
            "observed_at": now_ms - 86_400_000
        });
        let (spec, _) = runtime.create(draft_for("wx-stale")).unwrap();
        let resp = runtime.invoke(&spec.id).await.unwrap();
        let item = resp
            .execution
            .as_ref()
            .unwrap()
            .evidence
            .items
            .iter()
            .find(|i| i.key == "weather")
            .unwrap();
        assert!(
            matches!(item.quality, EvidenceQuality::Stale),
            "stale payload obs must not use call time: {:?}",
            item.quality
        );

        // Missing observation time → Stale/missing under strict LatestWithin.
        *payload.lock().unwrap() = serde_json::json!({ "temp": 22 });
        let (spec, _) = runtime.create(draft_for("wx-missing")).unwrap();
        let resp = runtime.invoke(&spec.id).await.unwrap();
        let item = resp
            .execution
            .as_ref()
            .unwrap()
            .evidence
            .items
            .iter()
            .find(|i| i.key == "weather")
            .unwrap();
        assert!(
            !matches!(item.quality, EvidenceQuality::Fresh),
            "missing obs time must not be Fresh: {:?}",
            item.quality
        );
    }

    #[tokio::test]
    async fn task_invoke_is_sample_cannot_forge_fresh_observation() {
        let payload = Arc::new(Mutex::new(serde_json::json!({})));
        let (mut deps, _, _) = test_deps().await;
        let ext_reg = Arc::new(ExtensionRegistry::new());
        ext_reg
            .register("weather_cn".into(), fake_weather_extension(payload.clone()))
            .await
            .unwrap();
        deps.extension_registry = ext_reg;
        let store = AgentStore::memory().unwrap();
        let runtime = HostedAiAgentRuntime::build(store, deps).unwrap();

        let now_ms = chrono::Utc::now().timestamp_millis();
        // Yesterday in payload — task tries to forge sample-at-invoke.
        *payload.lock().unwrap() = serde_json::json!({
            "temp": 30,
            "observed_at": now_ms / 1000 - 86_400
        });

        let mut draft = local_brief_draft("forge-sample");
        draft.input = InputContract {
            requirements: vec![InputRequirement {
                key: "weather".into(),
                source: InputSourceRef::ExtensionInstance {
                    id: "weather_cn".into(),
                },
                operation: Some("current".into()),
                params: serde_json::json!({ "_invoke_is_sample": true }),
                auth_scope: None,
                timeout_secs: 5,
                result_type: InputModality::StructuredJson,
                freshness: FreshnessPolicy::LatestWithin { max_age_secs: 900 },
                on_missing: MissingPolicy::ReportStale,
            }],
        };
        draft.controlled_actions = vec![ControlledAction {
            name: "adjust".into(),
            destination: "ext-write-1".into(),
            depends_on: vec!["weather".into()],
            idempotent: false,
            params: serde_json::json!({}),
        }];

        let (spec, _) = runtime.create(draft).unwrap();
        let resp = runtime.invoke(&spec.id).await.unwrap();
        let exec = resp.execution.as_ref().unwrap();
        let weather = exec
            .evidence
            .items
            .iter()
            .find(|i| i.key == "weather")
            .unwrap();
        assert!(
            matches!(weather.quality, EvidenceQuality::Stale),
            "task _invoke_is_sample must not forge Fresh: {:?}",
            weather.quality
        );
        assert_eq!(runtime.side_effects.invokes(), 0);
        assert!(
            exec.deliveries
                .iter()
                .any(|d| d.destination == "ext-write-1"
                    && matches!(
                        d.state,
                        neomind_ai_agent::model::DeliveryState::Rejected { .. }
                    )),
            "stale weather must block action: {:?}",
            exec.deliveries
        );
    }

    #[tokio::test]
    async fn empty_v2_only_db_boots_hosted_runtime_without_v1() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("agents.redb");
        let report = neomind_storage::purge_agents_to_v2_only(&path).unwrap();
        assert_eq!(report.v2_specs_after, 0);
        assert!(!report.had_v1_table);

        let store = AgentStore::open(&path).unwrap();
        {
            let db = store.db();
            assert!(neomind_storage::is_v2_only(&*db).unwrap());
            assert!(
                !neomind_storage::agents_v2::table_exists_agents_v1(&*db).unwrap_or(true),
                "v1 agents table must be absent on empty v2_only DB"
            );
        }

        let (deps, _, _) = test_deps().await;
        let runtime = HostedAiAgentRuntime::build(store, deps).unwrap();
        assert!(runtime.list().is_empty());
        // create → ensure_loaded path (no ingest): draft round-trip
        let (spec, _) = runtime.create(local_brief_draft("gate-empty")).unwrap();
        runtime.ensure_loaded(&spec.id).unwrap();
        assert!(runtime.has_agent(&spec.id));
    }
}
