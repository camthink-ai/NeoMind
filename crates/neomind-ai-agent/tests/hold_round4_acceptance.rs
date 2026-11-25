//! Hold tip round-4: LookbackWindow timestamp coverage + LatestWithin future.

use neomind_ai_agent::harness::build_harness;
use neomind_ai_agent::model::{
    EvidenceQuality, FreshnessPolicy, InputContract, InputModality, InputRequirement,
    InputSourceRef, MissingPolicy, ToolPolicy,
};
use neomind_ai_agent::ports::SourceReadResult;

fn lookback_draft(name: &str) -> neomind_ai_agent::model::AgentSpecDraft {
    let mut d = neomind_ai_agent::model::temperature_glance_draft("meter-1", "ops");
    d.name = name.into();
    d.llm_backend_id = Some("local-brief".into());
    d.tool_policy = ToolPolicy::Disabled;
    d.input = InputContract {
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
    };
    d.output = neomind_ai_agent::model::OutputContract {
        fields: vec![neomind_ai_agent::model::OutputField {
            name: "diag".into(),
            depends_on: vec!["meter_15m".into()],
            description: None,
        }],
        publish: false,
    };
    d.controlled_actions = vec![neomind_ai_agent::model::ControlledAction {
        name: "adjust".into(),
        destination: "ext-write-1".into(),
        depends_on: vec!["meter_15m".into()],
        idempotent: false,
        params: serde_json::json!({}),
    }];
    d
}

#[tokio::test]
async fn lookback_untimestamped_points_not_fresh() {
    let now = 1_700_000_000_000i64;
    let h = build_harness(now);
    let (spec, _) = h.service.create(lookback_draft("no-ts")).unwrap();
    // Host did not supply trusted valid_window; points lack timestamps.
    h.sources.set_device(
        "meter-1",
        "power_kw",
        SourceReadResult {
            source_id: "device:meter-1:power_kw".into(),
            binding_proof: "p".into(),
            observed_at: None,
            valid_window: None,
            value: serde_json::json!({"points":[{"value":3},{"value":4}]}),
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
        .find(|i| i.key == "meter_15m")
        .unwrap();
    assert!(
        !matches!(item.quality, EvidenceQuality::Fresh),
        "untimestamped points must not be Fresh: {:?}",
        item.quality
    );
    assert_eq!(h.writes.call_count(), 0);
}

#[tokio::test]
async fn lookback_all_out_of_window_not_fresh() {
    let now = 1_700_000_000_000i64;
    let h = build_harness(now);
    let (spec, _) = h.service.create(lookback_draft("oot")).unwrap();
    h.sources.set_device(
        "meter-1",
        "power_kw",
        SourceReadResult {
            source_id: "device:meter-1:power_kw".into(),
            binding_proof: "p".into(),
            observed_at: Some(now - 3_600_000),
            valid_window: None,
            value: serde_json::json!({
                "points": [
                    {"value": 1, "timestamp": now - 3_600_000},
                    {"value": 2, "timestamp": now - 3_500_000}
                ]
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
        .find(|i| i.key == "meter_15m")
        .unwrap();
    assert!(
        !matches!(item.quality, EvidenceQuality::Fresh),
        "out-of-window points must not be Fresh: {:?}",
        item.quality
    );
    assert_eq!(h.writes.call_count(), 0);
}

#[tokio::test]
async fn lookback_two_in_window_timestamped_passes() {
    let now = 1_700_000_000_000i64;
    let h = build_harness(now);
    let (spec, _) = h.service.create(lookback_draft("ok")).unwrap();
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
                    {"value": 1, "timestamp": now - 120_000},
                    {"value": 2, "timestamp": now - 60_000}
                ]
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
        .find(|i| i.key == "meter_15m")
        .unwrap();
    assert!(
        matches!(item.quality, EvidenceQuality::Fresh),
        "two in-window timestamped points must pass: {:?}",
        item.quality
    );
}

#[tokio::test]
async fn latest_within_rejects_future_observation() {
    let now = 1_700_000_000_000i64;
    let h = build_harness(now);
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
            observed_at: Some(now + 60_000), // future
            valid_window: None,
            value: serde_json::json!({"temperature": 21}),
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
        !matches!(item.quality, EvidenceQuality::Fresh),
        "future observed_at must not be Fresh: {:?}",
        item.quality
    );
}
