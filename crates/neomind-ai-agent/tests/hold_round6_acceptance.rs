//! Hold tip round-6: strip stale lookback aggregates; LatestWithin obs time.

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
            tool_calls: Vec::new(),
        })
    }
}

fn capturing_service(
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

#[tokio::test]
async fn lookback_filter_drops_stale_aggregates() {
    let now = 1_700_000_000_000i64;
    let h = build_harness(now);
    let llm = Arc::new(CapturingLlm {
        calls: AtomicU64::new(0),
        last_user: Mutex::new(String::new()),
    });
    let service = capturing_service(&h, llm.clone());

    let draft = neomind_ai_agent::model::AgentSpecDraft {
        name: "agg-filter".into(),
        description: None,
        user_prompt: "Diagnose meter window.".into(),
        llm_backend_id: None,
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
    h.sources.set_device(
        "meter-1",
        "power_kw",
        SourceReadResult {
            source_id: "device:meter-1:power_kw".into(),
            binding_proof: "p".into(),
            observed_at: Some(now - 60_000),
            valid_window: None,
            value: serde_json::json!({
                "unit": "kW",
                "max_kw": 900,
                "avg": 302.3,
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
    assert!(matches!(item.quality, EvidenceQuality::Fresh));

    let value = match &item.detail {
        Some(EvidenceDetail::TimeSeries { points, value }) => {
            assert_eq!(*points, 2);
            value.as_ref().expect("timeseries value").clone()
        }
        other => panic!("expected TimeSeries, got {other:?}"),
    };
    let pts = value
        .get("points")
        .and_then(|p| p.as_array())
        .expect("points array");
    let vals: Vec<i64> = pts
        .iter()
        .map(|p| p.get("value").and_then(|v| v.as_i64()).unwrap())
        .collect();
    assert_eq!(vals, vec![3, 4], "in-window values exactly [3, 4]");
    assert_eq!(value.get("unit").and_then(|v| v.as_str()), Some("kW"));
    assert!(
        value.get("max_kw").is_none() && value.get("avg").is_none(),
        "stale aggregates must be stripped: {value}"
    );

    let prompt = llm.last_user.lock().unwrap().clone();
    assert!(
        !prompt.contains("900") && !prompt.contains("max_kw") && !prompt.contains("302"),
        "model request must omit stale aggregates: {prompt}"
    );
    assert!(
        prompt.contains('3') && prompt.contains('4'),
        "model request keeps in-window values: {prompt}"
    );
}

#[tokio::test]
async fn latest_within_keeps_source_observed_at_for_decision() {
    let now = 1_700_000_000_000i64;
    let h = build_harness(now);
    let decision_obs = now - 30_000;
    let inner_point = now - 5_000; // different from source observed_at

    let mut draft = neomind_ai_agent::model::temperature_glance_draft("sensor-1", "ops");
    draft.llm_backend_id = Some("local-brief".into());
    draft.input.requirements[0].freshness = FreshnessPolicy::LatestWithin { max_age_secs: 600 };
    draft.input.requirements[0].on_missing = MissingPolicy::ReportStale;
    let (spec, _) = h.service.create(draft).unwrap();

    h.sources.set_device(
        "sensor-1",
        "temperature",
        SourceReadResult {
            source_id: "device:sensor-1:temperature".into(),
            binding_proof: "p".into(),
            observed_at: Some(decision_obs),
            valid_window: None,
            value: serde_json::json!({
                "temperature": 21.5,
                "timestamp": inner_point
            }),
        },
    );

    let resp = h.service.invoke(&spec.id).await.unwrap();
    let item = resp
        .execution
        .as_ref()
        .unwrap()
        .evidence
        .items
        .iter()
        .find(|i| i.key == "temp_now")
        .unwrap();
    assert!(
        matches!(item.quality, EvidenceQuality::Fresh),
        "source obs within max_age must be Fresh: {:?}",
        item.quality
    );
    assert_eq!(
        item.observed_at,
        Some(decision_obs),
        "evidence must record the LatestWithin decision time, not inner point time"
    );
}
