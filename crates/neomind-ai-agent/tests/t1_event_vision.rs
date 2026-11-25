//! T1 / link C — Event vision same-report ownership (PR #39 contract).
//!
//! 1. Matching report_id on event + image → Fresh evidence with Vision detail
//! 2. Mismatched report_id → unconfirmed / not Fresh for vision binding
//! 3. Non-vision task runs without any image inputs

use neomind_ai_agent::harness::build_harness;
use neomind_ai_agent::input::{fixture_image_with_report_id, FrameBinding, ReportMetricPoint};
use neomind_ai_agent::model::{
    event_vision_draft, temperature_glance_draft, EvidenceDetail, EvidenceQuality, ExecutionStatus,
    FreshnessPolicy, InputModality,
};
use neomind_ai_agent::ports::SourceReadResult;
use serde_json::json;

fn seed_frame(
    h: &neomind_ai_agent::harness::TestHarness,
    device_id: &str,
    field: &str,
    value: serde_json::Value,
    ts: i64,
) {
    h.sources.set_device(
        device_id,
        field,
        SourceReadResult {
            source_id: format!("device:{device_id}:{field}"),
            binding_proof: format!("device:{device_id}:{field}"),
            observed_at: Some(ts),
            valid_window: Some((ts, ts)),
            value,
        },
    );
}

#[tokio::test]
async fn t1_same_report_id_yields_fresh_vision_detail() {
    let now = 1_700_000_100_000i64;
    let ts = now / 1000; // second-level anchor like timeseries
    let h = build_harness(now);

    let mut draft = event_vision_draft("cam-1", "image_data", "test-channel");
    // Event listener injects the triggering report_id into params.
    draft.input.requirements[0].params = json!({
        "report_id": "rpt-A",
        "anchor_ts": ts,
    });
    let (spec, _) = h.service.create(draft).expect("create");

    seed_frame(
        &h,
        "cam-1",
        "image_data",
        fixture_image_with_report_id("rpt-A", b'A'),
        ts,
    );

    let resp = h.service.invoke(&spec.id).await.expect("invoke");
    let exec = resp.execution.as_ref().expect("execution");
    assert_eq!(exec.status, ExecutionStatus::Completed);

    let frame = exec
        .evidence
        .items
        .iter()
        .find(|i| i.key == "frame")
        .expect("frame evidence");
    assert!(
        matches!(frame.quality, EvidenceQuality::Fresh),
        "same report_id must be Fresh: {:?}",
        frame.quality
    );
    assert!(
        frame.binding_proof.contains("same_report") && frame.binding_proof.contains("rpt-A"),
        "binding proof: {}",
        frame.binding_proof
    );
    match frame.detail.as_ref() {
        Some(EvidenceDetail::Vision {
            image_source_field,
            vision_path,
        }) => {
            assert_eq!(image_source_field, "image_data");
            assert!(
                vision_path.contains("rpt-A"),
                "vision detail must cite report: {vision_path}"
            );
        }
        other => panic!("expected Vision detail, got {other:?}"),
    }
    assert!(!exec.evidence.missing_keys.iter().any(|k| k == "frame"));
}

#[tokio::test]
async fn t1_mismatched_report_id_is_unconfirmed_not_fresh() {
    let now = 1_700_000_200_000i64;
    let ts = now / 1000;
    let h = build_harness(now);

    let mut draft = event_vision_draft("cam-1", "image_data", "test-channel");
    draft.input.requirements[0].params = json!({
        "report_id": "rpt-A",
        "anchor_ts": ts,
    });
    let (spec, _) = h.service.create(draft).expect("create");

    // Image proves ownership of B — must not bind to event A.
    seed_frame(
        &h,
        "cam-1",
        "image_data",
        fixture_image_with_report_id("rpt-B", b'B'),
        ts,
    );

    let resp = h.service.invoke(&spec.id).await.expect("invoke");
    let exec = resp.execution.as_ref().expect("execution");

    let frame = exec
        .evidence
        .items
        .iter()
        .find(|i| i.key == "frame")
        .expect("frame evidence");
    assert!(
        !matches!(frame.quality, EvidenceQuality::Fresh),
        "mismatched report_id must not be Fresh: {:?}",
        frame.quality
    );
    assert!(
        frame.binding_proof.starts_with("unconfirmed:"),
        "expected unconfirmed binding, got {}",
        frame.binding_proof
    );
    assert!(frame.detail.is_none(), "no vision detail on unconfirmed");
    assert!(
        exec.evidence.missing_keys.iter().any(|k| k == "frame"),
        "frame must be listed in missing_keys"
    );
    assert!(
        matches!(
            exec.status,
            ExecutionStatus::Partial | ExecutionStatus::Failed | ExecutionStatus::Skipped
        ),
        "unconfirmed vision must not Complete as if Fresh: {:?}",
        exec.status
    );
}

#[tokio::test]
async fn t1_non_vision_task_runs_without_images() {
    let now = 1_700_000_300_000i64;
    let h = build_harness(now);
    let draft = temperature_glance_draft("sensor-1", "test-channel");
    let (spec, plan) = h.service.create(draft).expect("create");

    // Spec has no Image / SameReport requirements.
    assert!(spec
        .input
        .requirements
        .iter()
        .all(|r| !matches!(r.result_type, InputModality::Image)));
    assert!(spec
        .input
        .requirements
        .iter()
        .all(|r| !matches!(r.freshness, FreshnessPolicy::SameReport)));
    assert_eq!(plan.external_read_counts.total(), 0);

    h.sources.set_device(
        "sensor-1",
        "temperature",
        SourceReadResult {
            source_id: "device:sensor-1:temperature".into(),
            binding_proof: "device:sensor-1:temperature".into(),
            observed_at: Some(now - 1_000),
            valid_window: Some((now - 600_000, now)),
            value: json!({ "celsius": 22.5 }),
        },
    );

    let resp = h.service.invoke(&spec.id).await.expect("invoke");
    let exec = resp.execution.as_ref().expect("execution");
    assert_eq!(exec.status, ExecutionStatus::Completed);
    let temp = exec
        .evidence
        .items
        .iter()
        .find(|i| i.key == "temp_now")
        .expect("temp");
    assert!(matches!(temp.quality, EvidenceQuality::Fresh));
    assert!(
        !matches!(temp.detail, Some(EvidenceDetail::Vision { .. })),
        "non-vision task must not emit Vision detail"
    );
    assert!(exec.evidence.missing_keys.is_empty());
}

#[test]
fn t1_unit_orphan_co_stamp_stays_unconfirmed() {
    use neomind_ai_agent::input::{bind_image_to_report, SameReportAnchor};

    let anchor = SameReportAnchor {
        report_id: "rpt-A".into(),
        anchor_ts: Some(42),
    };
    let image = ReportMetricPoint {
        name: "image_data".into(),
        timestamp: 42,
        value: json!("/api/images/orphan_b.jpg"),
        metadata: None,
    };
    let siblings = [ReportMetricPoint {
        name: "metadata.image_id".into(),
        timestamp: 42,
        value: json!("rpt-A"),
        metadata: None,
    }];
    let binding = bind_image_to_report(&anchor, "image_data", &image, &siblings);
    assert!(
        matches!(binding, FrameBinding::Unconfirmed { .. }),
        "{binding:?}"
    );
}
