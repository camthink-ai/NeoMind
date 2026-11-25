//! Hold tip round-5: LookbackWindow evidence + model prompt share in-window set.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use neomind_ai_agent::harness::build_harness;
use neomind_ai_agent::model::{
    EvidenceDetail, EvidenceQuality, FreshnessPolicy, InputContract, InputModality,
    InputRequirement, InputSourceRef, MissingPolicy, OutputContract, OutputField, ToolPolicy,
};
use neomind_ai_agent::ports::{
    LlmBackendCapabilities, LlmPort, LlmRequest, LlmResponse, SourceReadResult,
};
use neomind_ai_agent::runner::SinglePassRunner;
use neomind_ai_agent::service::AgentApplicationService;
use neomind_ai_agent::side_effects::{Notifier, SideEffectBoundary};
use tokio_util::sync::CancellationToken;

struct CapturingLlm {
    calls: AtomicU64,
    last_user: Mutex<String>,
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
        *self.last_user.lock().unwrap() = request.user;
        Ok(LlmResponse {
            text: "ok\nVerdict: criteria not met\n".into(),
            alert_criteria_met: false,
        })
    }
}

#[tokio::test]
async fn lookback_mixed_window_prompt_drops_out_of_window_points() {
    let now = 1_700_000_000_000i64;
    let h = build_harness(now);
    let llm = Arc::new(CapturingLlm {
        calls: AtomicU64::new(0),
        last_user: Mutex::new(String::new()),
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

    let draft = neomind_ai_agent::model::AgentSpecDraft {
        name: "mixed-window".into(),
        description: None,
        user_prompt: "Diagnose meter window.".into(),
        llm_backend_id: None, // real model path — captures built prompt
        preset: None,
        trigger: neomind_ai_agent::model::TriggerSpec::Manual,
        pacing: neomind_ai_agent::model::PacingPolicy::default(),
        input: InputContract {
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
                on_missing: MissingPolicy::Undetermined,
            }],
        },
        tool_policy: ToolPolicy::Disabled,
        output: OutputContract {
            fields: vec![OutputField {
                name: "diag".into(),
                depends_on: vec!["meter_15m".into()],
                description: None,
            }],
            publish: false,
        },
        notify: neomind_ai_agent::model::NotifyPolicy::default(),
        controlled_actions: vec![],
    };

    let (spec, _) = service.create(draft).unwrap();
    // Mix: 2h ago 900 kW (out) + 5m 3 + 1m 4 (in).
    h.sources.set_device(
        "meter-1",
        "power_kw",
        SourceReadResult {
            source_id: "device:meter-1:power_kw".into(),
            binding_proof: "p".into(),
            observed_at: Some(now - 60_000),
            valid_window: None,
            value: serde_json::json!({
                "points": [
                    {"value": 900, "timestamp": now - 7_200_000},
                    {"value": 3, "timestamp": now - 300_000},
                    {"value": 4, "timestamp": now - 60_000}
                ]
            }),
        },
    );

    let resp = service.invoke(&spec.id).await.unwrap();
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
        matches!(item.quality, EvidenceQuality::Fresh),
        "mixed window with 2 in-window points must be Fresh: {:?}",
        item.quality
    );
    match &item.detail {
        Some(EvidenceDetail::TimeSeries { points, value }) => {
            assert_eq!(*points, 2, "EvidenceDetail must keep only in-window points");
            let body = value.as_ref().map(|v| v.to_string()).unwrap_or_default();
            assert!(
                !body.contains("900"),
                "out-of-window 900 must not remain in detail: {body}"
            );
            assert!(
                body.contains('3') && body.contains('4'),
                "in-window values retained: {body}"
            );
        }
        other => panic!("expected TimeSeries detail, got {other:?}"),
    }

    assert_eq!(llm.calls.load(Ordering::SeqCst), 1);
    let prompt = llm.last_user.lock().unwrap().clone();
    assert!(
        !prompt.contains("900"),
        "model request must not include out-of-window 900: {prompt}"
    );
    assert!(
        prompt.contains('3') && prompt.contains('4'),
        "model request must include the two in-window records: {prompt}"
    );
}
