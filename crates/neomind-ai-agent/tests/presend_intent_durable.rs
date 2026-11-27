//! NM-403: pre-send IntentPending must land in the same redb restart opens.
//!
//! Acceptance (in-process): receiver gets the command → `invoke.abort()` while
//! the side-effect hangs after accept → drop store → reopen the same DB →
//! receive count stays 1 and the delivery is `OutcomeUnknown`.
//! This is **not** an OS process kill; a hard-kill gate is optional follow-up.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use neomind_ai_agent::input::{CountingSourcePort, InputPipeline};
use neomind_ai_agent::model::{daily_brief_draft, ControlledAction, DeliveryState, NotifyPolicy};
use neomind_ai_agent::persist::DurableAgentStore;
use neomind_ai_agent::ports::{
    FixedClock, LocalBriefLlm, NotifyPort, SideEffectPort, SideEffectReceipt, SourceReadResult,
};
use neomind_ai_agent::runner::SinglePassRunner;
use neomind_ai_agent::service::{AgentApplicationService, NotifyCounter, WriteCounter};
use neomind_ai_agent::side_effects::{Notifier, SideEffectBoundary};
use neomind_storage::agent_store::{AgentRecordStore, DeliveryState as StorageDeliveryState};
use neomind_storage::AgentStore;

/// Counts accepts, signals the test, then hangs so the task can be aborted.
struct AcceptThenHang {
    hits: Arc<AtomicU64>,
    accepted: Arc<tokio::sync::Notify>,
}

#[async_trait]
impl SideEffectPort for AcceptThenHang {
    async fn invoke(
        &self,
        _destination: &str,
        _action: &str,
        _params: &serde_json::Value,
        _execution_id: &str,
    ) -> neomind_ai_agent::Result<SideEffectReceipt> {
        self.hits.fetch_add(1, Ordering::SeqCst);
        self.accepted.notify_waiters();
        // Device already accepted; hang so the test can `invoke.abort()` before
        // post-send update_delivery_state / final mirror.
        std::future::pending::<()>().await;
        unreachable!()
    }
}

impl WriteCounter for AcceptThenHang {
    fn count(&self) -> u64 {
        self.hits.load(Ordering::SeqCst)
    }
}

#[derive(Default)]
struct NoopNotify;

#[async_trait]
impl NotifyPort for NoopNotify {
    async fn deliver(
        &self,
        _channel: &str,
        _subject: &str,
        _body: &str,
        _execution_id: &str,
    ) -> neomind_ai_agent::Result<()> {
        Ok(())
    }
}

impl NotifyCounter for NoopNotify {
    fn count(&self) -> u64 {
        0
    }
}

fn seed_brief(sources: &CountingSourcePort, now: i64) {
    let window = (now - 3_600_000, now + 3_600_000);
    sources.set_extension(
        "weather_cn",
        "current",
        SourceReadResult {
            source_id: "extension:weather_cn".into(),
            binding_proof: "extension:weather_cn:op=current".into(),
            observed_at: Some(now - 60_000),
            valid_window: Some(window),
            value: serde_json::json!({ "temp_c": 22 }),
        },
    );
    sources.set_extension(
        "tariff_tou",
        "tou_today",
        SourceReadResult {
            source_id: "extension:tariff_tou".into(),
            binding_proof: "extension:tariff_tou:op=tou_today".into(),
            observed_at: Some(now - 120_000),
            valid_window: Some(window),
            value: serde_json::json!({ "price": 0.8 }),
        },
    );
}

#[tokio::test]
async fn pre_send_intent_survives_abort_reopen_receiver_count_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agents.redb");
    let now = 1_700_000_000_000i64;

    let hits = Arc::new(AtomicU64::new(0));
    let accepted = Arc::new(tokio::sync::Notify::new());
    let writes = Arc::new(AcceptThenHang {
        hits: hits.clone(),
        accepted: accepted.clone(),
    });

    let agent_id = {
        let store = AgentStore::open(&path).unwrap();
        let record_store = AgentRecordStore::from_arc(store.db()).unwrap();
        let durable = DurableAgentStore::with_store(record_store, now).unwrap();
        let clock = Arc::new(FixedClock::new(now));
        let sources = Arc::new(CountingSourcePort::new());
        seed_brief(&sources, now);
        let notify = Arc::new(NoopNotify);
        let input = InputPipeline::new(sources.clone(), clock.clone());
        let runner = Arc::new(SinglePassRunner::new(Arc::new(LocalBriefLlm)));
        // Same production wiring shape: SideEffectBoundary gets durable.mem
        // (persist sink installed by with_store), not a bare mem store.
        let side_effects = SideEffectBoundary {
            store: durable.mem.clone(),
            notifier: Notifier::new(notify.clone()),
            side_effects: writes.clone(),
            clock: clock.clone(),
        };
        let service = Arc::new(AgentApplicationService {
            store: durable.mem.clone(),
            clock: clock.clone(),
            input,
            runner,
            side_effects,
            notify_counter: notify.clone(),
            write_counter: writes.clone(),
        });

        let mut draft = daily_brief_draft("weather_cn", "tariff_tou", "test-channel");
        draft.controlled_actions = vec![ControlledAction {
            name: "open".into(),
            destination: "device:valve".into(),
            depends_on: vec!["weather_now".into()],
            idempotent: false,
            params: serde_json::json!({}),
        }];
        draft.notify = NotifyPolicy {
            channels: vec![],
            on_complete: false,
            on_alert: false,
            alert_depends_on: vec![],
            on_failure: false,
            pending_confirmation: false,
        };
        let (spec, _) = service.create(draft).unwrap();
        let agent_id = spec.id.clone();

        let invoke = tokio::spawn({
            let service = service.clone();
            let id = agent_id.clone();
            async move { service.invoke(&id).await }
        });

        tokio::select! {
            _ = accepted.notified() => {}
            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                panic!("side effect never accepted before hang");
            }
        }
        assert_eq!(hits.load(Ordering::SeqCst), 1, "receiver got the command");

        // Abort the in-process invoke task after accept (not an OS process kill).
        invoke.abort();
        let _ = invoke.await;

        // Prove IntentPending is already on disk before drop/reopen.
        let store_live = durable.store.as_ref().expect("durable store");
        let formal = store_live.list_formal_executions(&agent_id).unwrap();
        assert_eq!(
            formal.len(),
            1,
            "pre-send save must have written formal exec"
        );
        assert!(
            formal[0]
                .deliveries
                .iter()
                .any(|d| matches!(d.state, StorageDeliveryState::IntentPending)),
            "IntentPending must be durable before abort/reopen: {:?}",
            formal[0].deliveries
        );

        drop(service);
        drop(durable);
        drop(store);
        agent_id
    };

    // Restart same DB — recover_stale_intents runs inside with_store.
    let reopened = AgentStore::open(&path).unwrap();
    let record_store = AgentRecordStore::from_arc(reopened.db()).unwrap();
    let _durable = DurableAgentStore::with_store(record_store.clone(), now + 1).unwrap();
    let formal = record_store.list_formal_executions(&agent_id).unwrap();
    assert_eq!(formal.len(), 1);
    assert!(
        formal[0]
            .deliveries
            .iter()
            .any(|d| matches!(d.state, StorageDeliveryState::OutcomeUnknown { .. })),
        "restart must mark IntentPending as OutcomeUnknown: {:?}",
        formal[0].deliveries
    );
    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "recover must not resend; receiver count stays 1"
    );
}

#[test]
fn with_store_save_execution_mirrors_without_mirror_after_run() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agents.redb");
    let store = AgentStore::open(&path).unwrap();
    let record_store = AgentRecordStore::from_arc(store.db()).unwrap();
    let durable = DurableAgentStore::with_store(record_store.clone(), 1000).unwrap();

    let draft = daily_brief_draft("weather_cn", "tariff_tou", "test-channel");
    let spec = neomind_ai_agent::model::AgentSpec {
        id: "agent-x".into(),
        name: draft.name,
        description: draft.description,
        user_prompt: draft.user_prompt,
        llm_backend_id: draft.llm_backend_id,
        preset: draft.preset,
        trigger: draft.trigger,
        pacing: draft.pacing,
        input: draft.input,
        tool_policy: draft.tool_policy,
        output: draft.output,
        notify: draft.notify,
        controlled_actions: draft.controlled_actions,
        config_version: 1,
        created_at: 1000,
        updated_at: 1000,
    };
    durable.save_spec(spec, 1000).unwrap();

    let exec = neomind_ai_agent::model::AgentExecutionRecord {
        id: "exec-pre".into(),
        agent_id: "agent-x".into(),
        config_version: 1,
        status: neomind_ai_agent::model::ExecutionStatus::Completed,
        started_at: 1000,
        finished_at: 1000,
        is_probe: false,
        dry_run: false,
        trigger_source: Some("manual".into()),
        stale_config: None,
        evidence: Default::default(),
        deliveries: vec![neomind_ai_agent::model::ActionDeliveryRecord {
            execution_id: "exec-pre".into(),
            destination: "device:valve".into(),
            action_fingerprint: "open:device:valve".into(),
            depends_on: vec![],
            idempotent: false,
            state: DeliveryState::IntentPending,
        }],
        tool_calls: vec![],
        brief_text: None,
        conclusion: None,
        structured_result: None,
        error: None,
        alert_verdict: None,
    };
    // Production path: service/side_effects call durable.mem.save_execution.
    durable.mem.save_execution(exec).unwrap();

    let row = record_store
        .get_execution("exec-pre")
        .unwrap()
        .expect("mirrored");
    assert!(row
        .deliveries
        .iter()
        .any(|d| matches!(d.state, StorageDeliveryState::IntentPending)));
}
