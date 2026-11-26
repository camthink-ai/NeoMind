//! Ask-path: this-turn caller text must enter the model request and stay
//! isolated across asks / scheduled runs (NM-406).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use neomind_ai_agent::coordinator::{ExecutionCoordinator, SubmitSource};
use neomind_ai_agent::harness::build_harness;
use neomind_ai_agent::model::{
    CallerInput, InputContract, NotifyPolicy, OutputContract, PacingPolicy, ToolPolicy, TriggerSpec,
};
use neomind_ai_agent::ports::{LlmBackendCapabilities, LlmPort, LlmRequest, LlmResponse};
use neomind_ai_agent::runner::SinglePassRunner;
use neomind_ai_agent::service::AgentApplicationService;
use neomind_ai_agent::side_effects::{Notifier, SideEffectBoundary};
use tokio_util::sync::CancellationToken;

struct CapturingLlm {
    calls: AtomicU64,
    users: Mutex<Vec<String>>,
}

#[async_trait]
impl LlmPort for CapturingLlm {
    async fn resolve_backend(
        &self,
        _backend_id: Option<&str>,
    ) -> neomind_ai_agent::Result<LlmBackendCapabilities> {
        Ok(LlmBackendCapabilities {
            multimodal: false,
            tools: false,
            model: "capture".into(),
        })
    }

    async fn complete(
        &self,
        request: LlmRequest,
        _cancel: CancellationToken,
    ) -> neomind_ai_agent::Result<LlmResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.users.lock().unwrap().push(request.user);
        Ok(LlmResponse {
            text: format!(
                "answer-{}\nVerdict: criteria not met\n",
                self.calls.load(Ordering::SeqCst)
            ),
            alert_criteria_met: false,
        })
    }
}

fn build_service(
    h: &neomind_ai_agent::harness::TestHarness,
    llm: Arc<CapturingLlm>,
) -> Arc<AgentApplicationService> {
    let runner = Arc::new(SinglePassRunner::new(llm));
    Arc::new(AgentApplicationService {
        store: h.store.clone(),
        clock: h.clock.clone(),
        input: neomind_ai_agent::input::InputPipeline::new(h.sources.clone(), h.clock.clone()),
        runner,
        side_effects: SideEffectBoundary {
            store: h.store.clone(),
            notifier: Notifier::new(h.notify.clone()),
            side_effects: h.writes.clone(),
            clock: h.clock.clone(),
        },
        notify_counter: h.notify.clone(),
        write_counter: h.writes.clone(),
    })
}

fn manual_draft(name: &str) -> neomind_ai_agent::model::AgentSpecDraft {
    neomind_ai_agent::model::AgentSpecDraft {
        name: name.into(),
        description: None,
        user_prompt: "Standing analyst mission.".into(),
        llm_backend_id: None,
        preset: None,
        trigger: TriggerSpec::Manual,
        pacing: PacingPolicy::default(),
        input: InputContract {
            requirements: vec![],
        },
        tool_policy: ToolPolicy::Disabled,
        output: OutputContract::default(),
        notify: NotifyPolicy::default(),
        controlled_actions: vec![],
    }
}

#[tokio::test]
async fn two_asks_each_model_request_contains_its_question() {
    let now = 1_700_000_000_000i64;
    let h = build_harness(now);
    let llm = Arc::new(CapturingLlm {
        calls: AtomicU64::new(0),
        users: Mutex::new(Vec::new()),
    });
    let service = build_service(&h, llm.clone());
    let (spec, _) = service.create(manual_draft("analyst-ask")).unwrap();

    let r1 = service
        .invoke_with_caller(
            &spec.id,
            Some(CallerInput {
                content: Some("Question Alpha about temperature".into()),
                data: None,
                source: Some("api".into()),
            }),
        )
        .await
        .expect("ask1");
    let r2 = service
        .invoke_with_caller(
            &spec.id,
            Some(CallerInput {
                content: Some("Question Beta about humidity".into()),
                data: None,
                source: Some("api".into()),
            }),
        )
        .await
        .expect("ask2");

    let users = llm.users.lock().unwrap().clone();
    assert_eq!(users.len(), 2);
    assert!(
        users[0].contains("Question Alpha about temperature"),
        "first request missing Q1: {}",
        users[0]
    );
    assert!(
        !users[0].contains("Question Beta"),
        "first request leaked Q2"
    );
    assert!(
        users[1].contains("Question Beta about humidity"),
        "second request missing Q2: {}",
        users[1]
    );
    assert!(
        !users[1].contains("Question Alpha"),
        "second request leaked Q1"
    );

    let e1 = r1.execution.expect("exec1");
    let e2 = r2.execution.expect("exec2");
    assert_ne!(e1.id, e2.id);
    assert!(e1.conclusion.as_deref().unwrap_or("").contains("answer-1"));
    assert!(e2.conclusion.as_deref().unwrap_or("").contains("answer-2"));
    // Standing mission stays on the stored spec — ask text is run-local.
    let stored = service.get(&spec.id).unwrap();
    assert_eq!(stored.user_prompt, "Standing analyst mission.");
    assert!(!stored.user_prompt.contains("Question Alpha"));
}

#[tokio::test]
async fn scheduled_submit_between_asks_does_not_carry_prior_question() {
    let now = 1_700_000_000_000i64;
    let h = build_harness(now);
    let llm = Arc::new(CapturingLlm {
        calls: AtomicU64::new(0),
        users: Mutex::new(Vec::new()),
    });
    let service = build_service(&h, llm.clone());
    let (spec, _) = service.create(manual_draft("analyst-sched")).unwrap();
    let coord = ExecutionCoordinator::new(service.clone(), h.store.clone(), h.clock.clone());

    let ask = service
        .invoke_with_caller(
            &spec.id,
            Some(CallerInput {
                content: Some("Interactive ask one".into()),
                data: None,
                source: Some("api".into()),
            }),
        )
        .await
        .expect("ask");
    let ask_id = ask.execution.as_ref().unwrap().id.clone();

    let scheduled = coord
        .submit(&spec.id, SubmitSource::Schedule)
        .await
        .expect("schedule");
    let sched_id = scheduled.execution.as_ref().unwrap().id.clone();
    assert_ne!(ask_id, sched_id);

    let ask2 = service
        .invoke_with_caller(
            &spec.id,
            Some(CallerInput {
                content: Some("Interactive ask two".into()),
                data: None,
                source: Some("api".into()),
            }),
        )
        .await
        .expect("ask2");
    let ask2_id = ask2.execution.as_ref().unwrap().id.clone();
    assert_ne!(ask_id, ask2_id);
    assert_ne!(sched_id, ask2_id);

    let users = llm.users.lock().unwrap().clone();
    assert_eq!(users.len(), 3);
    assert!(users[0].contains("Interactive ask one"));
    assert!(
        !users[1].contains("Interactive ask one") && !users[1].contains("Interactive ask two"),
        "scheduled run must not carry ask text: {}",
        users[1]
    );
    assert!(users[2].contains("Interactive ask two"));
    assert!(!users[2].contains("Interactive ask one"));

    assert_eq!(
        ask.execution.unwrap().conclusion.as_deref(),
        Some("answer-1\nVerdict: criteria not met\n")
    );
    assert_eq!(
        scheduled.execution.unwrap().conclusion.as_deref(),
        Some("answer-2\nVerdict: criteria not met\n")
    );
    assert_eq!(
        ask2.execution.unwrap().conclusion.as_deref(),
        Some("answer-3\nVerdict: criteria not met\n")
    );
}
