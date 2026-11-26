//! Hold tip round-2 acceptance: overload negatives + real model request count.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use neomind_ai_agent::harness::build_harness;
use neomind_ai_agent::model::{partial_meter_draft, AlertVerdict, ExecutionStatus, ToolPolicy};
use neomind_ai_agent::ports::{
    LlmBackendCapabilities, LlmPort, LlmRequest, LlmResponse, SourceReadResult,
};
use neomind_ai_agent::runner::SinglePassRunner;
use neomind_ai_agent::service::AgentApplicationService;
use neomind_ai_agent::side_effects::{Notifier, SideEffectBoundary};
use tokio_util::sync::CancellationToken;

struct CountingLlm {
    calls: AtomicU64,
    fail: AtomicU64,
}

#[async_trait]
impl LlmPort for CountingLlm {
    async fn resolve_backend(
        &self,
        _backend_id: Option<&str>,
    ) -> neomind_ai_agent::Result<LlmBackendCapabilities> {
        Ok(LlmBackendCapabilities {
            multimodal: false,
            tools: false,
            model: "count".into(),
        })
    }

    async fn complete(
        &self,
        request: LlmRequest,
        _cancel: CancellationToken,
    ) -> neomind_ai_agent::Result<LlmResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail.load(Ordering::SeqCst) > 0 {
            self.fail.fetch_sub(1, Ordering::SeqCst);
            return Err(neomind_ai_agent::AgentError::Llm("simulated 5xx".into()));
        }
        Ok(LlmResponse {
            text: format!("Brief.\n{}", request.user),
            alert_criteria_met: false,
            tool_calls: Vec::new(),
        })
    }
}

#[tokio::test]
async fn zero_tool_model_task_makes_exactly_one_real_model_request() {
    let now = 1_700_000_000_000i64;
    let h = build_harness(now);
    let llm = Arc::new(CountingLlm {
        calls: AtomicU64::new(0),
        fail: AtomicU64::new(0),
    });
    let runner = Arc::new(SinglePassRunner::new(llm.clone()));
    let service = Arc::new(AgentApplicationService {
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
    });

    let mut draft = neomind_ai_agent::model::daily_brief_draft("weather_cn", "tariff_tou", "ch");
    // Model task: tools disabled but NOT local-brief — must call LLM once.
    draft.llm_backend_id = None;
    draft.tool_policy = ToolPolicy::Disabled;
    let (spec, _) = service.create(draft).unwrap();
    h.sources.set_extension(
        "weather_cn",
        "current",
        SourceReadResult {
            source_id: "extension:weather_cn".into(),
            binding_proof: "p".into(),
            observed_at: Some(now),
            valid_window: Some((now - 1000, now)),
            value: serde_json::json!({"summary": "sunny"}),
        },
    );
    h.sources.set_extension(
        "tariff_tou",
        "tou_today",
        SourceReadResult {
            source_id: "extension:tariff_tou".into(),
            binding_proof: "p".into(),
            observed_at: Some(now),
            valid_window: Some((now - 1000, now)),
            value: serde_json::json!({"peaks":[{"price":1.2}]}),
        },
    );

    let resp = service.invoke(&spec.id).await.unwrap();
    assert_eq!(
        resp.execution.as_ref().unwrap().status,
        ExecutionStatus::Completed
    );
    assert_eq!(llm.calls.load(Ordering::SeqCst), 1);

    llm.fail.store(1, Ordering::SeqCst);
    let resp2 = service.invoke(&spec.id).await.unwrap();
    let exec2 = resp2.execution.as_ref().unwrap();
    assert_eq!(exec2.status, ExecutionStatus::Failed);
    assert!(exec2.brief_text.is_none());
    assert_eq!(llm.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn overload_negatives_do_not_notify_positive_does() {
    async fn run_with_meter(value: serde_json::Value) -> (u64, Option<AlertVerdict>) {
        let now = 1_700_000_000_000i64;
        let h = build_harness(now);
        let draft = partial_meter_draft("meter-1", "weather_cn", "ext-write-1", "test-channel");
        // Explicit local template so alert comes from structured evidence only.
        let mut draft = draft;
        draft.llm_backend_id = Some("local-brief".into());
        let (spec, _) = h.service.create(draft).unwrap();
        h.sources.set_device(
            "meter-1",
            "power_kw",
            SourceReadResult {
                source_id: "device:meter-1:power_kw".into(),
                binding_proof: "p".into(),
                observed_at: Some(now - 5_000),
                valid_window: Some((now - 900_000, now)),
                value,
            },
        );
        h.sources.set_extension(
            "weather_cn",
            "current",
            SourceReadResult {
                source_id: "extension:weather_cn".into(),
                binding_proof: "p".into(),
                observed_at: Some(now),
                valid_window: Some((now - 1000, now)),
                value: serde_json::json!({"temp": 20}),
            },
        );
        h.notify.reset();
        let resp = h.service.invoke(&spec.id).await.unwrap();
        let verdict = resp.execution.as_ref().unwrap().alert_verdict.clone();
        (h.notify.count(), verdict)
    }

    let (n, v) = run_with_meter(serde_json::json!({
        "points": [1.0, 2.0, 3.0],
        "overload": false
    }))
    .await;
    assert_eq!(n, 0, "boolean false must not notify");
    assert_ne!(v, Some(AlertVerdict::CriteriaMet));

    let (n, v) = run_with_meter(serde_json::json!({
        "points": [1.0, 2.0, 3.0],
        "note": "No overload detected"
    }))
    .await;
    assert_eq!(n, 0, "English negation must not notify");
    assert_ne!(v, Some(AlertVerdict::CriteriaMet));

    let (n, v) = run_with_meter(serde_json::json!({
        "points": [1.0, 2.0, 3.0],
        "note": "If power exceeds the threshold there may be overload"
    }))
    .await;
    assert_eq!(n, 0, "threshold discussion must not notify");
    assert_ne!(v, Some(AlertVerdict::CriteriaMet));

    let (n, v) = run_with_meter(serde_json::json!({
        "points": [1.0, 2.0, 9.5],
        "overload": true
    }))
    .await;
    assert_eq!(n, 1, "explicit positive must notify once");
    assert_eq!(v, Some(AlertVerdict::CriteriaMet));
}
