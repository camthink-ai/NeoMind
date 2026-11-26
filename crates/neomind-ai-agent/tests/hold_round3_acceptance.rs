//! Hold tip round-3: model verdict drives zero-tool alerts (not overload-only).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use neomind_ai_agent::harness::build_harness;
use neomind_ai_agent::model::{
    ExecutionStatus, FreshnessPolicy, InputContract, InputModality, InputRequirement,
    InputSourceRef, MissingPolicy, NotifyPolicy, ToolPolicy,
};
use neomind_ai_agent::ports::{
    LlmBackendCapabilities, LlmPort, LlmRequest, LlmResponse, SourceReadResult,
};
use neomind_ai_agent::runner::SinglePassRunner;
use neomind_ai_agent::service::AgentApplicationService;
use neomind_ai_agent::side_effects::{Notifier, SideEffectBoundary};
use tokio_util::sync::CancellationToken;

struct ScriptedLlm {
    calls: AtomicU64,
}

#[async_trait]
impl LlmPort for ScriptedLlm {
    async fn resolve_backend(
        &self,
        _backend_id: Option<&str>,
    ) -> neomind_ai_agent::Result<LlmBackendCapabilities> {
        Ok(LlmBackendCapabilities {
            multimodal: false,
            tools: false,
            model: "scripted".into(),
        })
    }

    async fn complete(
        &self,
        request: LlmRequest,
        _cancel: CancellationToken,
    ) -> neomind_ai_agent::Result<LlmResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        // Simulate a model that judges the business threshold in the prompt
        // against evidence temp=35 — not an overload heuristic.
        let met = request.user.contains(">30") && !request.user.contains(">40");
        let text = if met {
            "Temperature is 35°C, above threshold.\nVerdict: criteria met\n".into()
        } else {
            "Temperature is 35°C, below threshold.\nVerdict: criteria not met\n".into()
        };
        Ok(LlmResponse {
            text,
            alert_criteria_met: false,
            tool_calls: Vec::new(),
        })
    }
}

async fn run_temp_threshold(prompt: &str) -> u64 {
    let now = 1_700_000_000_000i64;
    let h = build_harness(now);
    let llm = Arc::new(ScriptedLlm {
        calls: AtomicU64::new(0),
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

    let mut draft = neomind_ai_agent::model::temperature_glance_draft("sensor-1", "ops");
    draft.user_prompt = prompt.into();
    draft.llm_backend_id = None; // real model path, not local-brief
    draft.tool_policy = ToolPolicy::Disabled;
    draft.notify = NotifyPolicy {
        channels: vec!["ops".into()],
        on_complete: false,
        on_alert: true,
        alert_depends_on: vec!["temp_now".into()],
        on_failure: false,
        pending_confirmation: false,
    };
    draft.input = InputContract {
        requirements: vec![InputRequirement {
            key: "temp_now".into(),
            source: InputSourceRef::DeviceMetric {
                device_id: "sensor-1".into(),
                field: "temperature".into(),
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

    let (spec, _) = service.create(draft).unwrap();
    h.sources.set_device(
        "sensor-1",
        "temperature",
        SourceReadResult {
            source_id: "device:sensor-1:temperature".into(),
            binding_proof: "p".into(),
            observed_at: Some(now - 1_000),
            valid_window: Some((now - 600_000, now)),
            value: serde_json::json!({"temperature": 35}),
        },
    );
    h.notify.reset();
    let resp = service.invoke(&spec.id).await.unwrap();
    assert_eq!(
        resp.execution.as_ref().unwrap().status,
        ExecutionStatus::Completed
    );
    assert_eq!(llm.calls.load(Ordering::SeqCst), 1);
    h.notify.count()
}

#[tokio::test]
async fn zero_tool_model_verdict_temp_threshold_not_overload_only() {
    // Same temp=35 evidence: >30°C criteria met → notify 1; >40°C not met → 0.
    let n30 = run_temp_threshold("Alert when temperature >30°C. Cite temp_now.").await;
    assert_eq!(
        n30, 1,
        ">30°C with temp=35 must notify once via model verdict"
    );

    let n40 = run_temp_threshold("Alert when temperature >40°C. Cite temp_now.").await;
    assert_eq!(n40, 0, ">40°C with temp=35 must not notify");
}
