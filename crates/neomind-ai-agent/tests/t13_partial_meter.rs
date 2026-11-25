//! T13 — Partial meter: one alert fires, weather-gated control blocked.

use neomind_ai_agent::harness::build_harness;
use neomind_ai_agent::model::{
    partial_meter_draft, AlertVerdict, EvidenceQuality, ExecutionStatus,
};
use neomind_ai_agent::ports::SourceReadResult;

#[tokio::test]
async fn t13_overload_alert_one_control_zero_when_weather_missing() {
    let now = 1_700_000_000_000i64;
    let h = build_harness(now);
    let draft = partial_meter_draft("meter-1", "weather_cn", "ext-write-1", "test-channel");
    let (spec, _) = h.service.create(draft).expect("create");

    // Meter fresh + overload; weather times out
    h.sources.set_device(
        "meter-1",
        "power_kw",
        SourceReadResult {
            source_id: "device:meter-1:power_kw".into(),
            binding_proof: "device:meter-1:power_kw".into(),
            observed_at: Some(now - 5_000),
            valid_window: Some((now - 900_000, now)),
            value: serde_json::json!({
                "points": [1.0, 2.0, 9.5],
                "overload": true,
                "note": "OVERLOAD"
            }),
        },
    );
    h.sources
        .set_extension_error("weather_cn", "current", "timeout");

    h.notify.reset();
    let resp = h.service.invoke(&spec.id).await.expect("invoke");
    let exec = resp.execution.as_ref().unwrap();
    assert_eq!(exec.status, ExecutionStatus::Partial, "step: Partial");

    let meter = exec
        .evidence
        .items
        .iter()
        .find(|i| i.key == "meter_15m")
        .unwrap();
    assert!(matches!(meter.quality, EvidenceQuality::Fresh));
    let weather = exec
        .evidence
        .items
        .iter()
        .find(|i| i.key == "weather")
        .unwrap();
    assert!(
        !matches!(weather.quality, EvidenceQuality::Fresh),
        "weather reason visible: {:?}",
        weather.quality
    );
    assert!(exec.evidence.missing_keys.iter().any(|k| k == "weather"));

    let brief = exec.brief_text.as_deref().unwrap_or("");
    assert!(
        brief.contains("依据不足") || brief.contains("weather"),
        "heat attribution insufficient: {brief}"
    );
    assert_eq!(
        exec.alert_verdict,
        Some(AlertVerdict::CriteriaMet),
        "overload alert"
    );
    assert_eq!(h.notify.count(), 1, "alert delivered once");
    assert_eq!(h.writes.call_count(), 0, "control must be 0");
    assert!(
        exec.deliveries.iter().any(|d| matches!(
            d.state,
            neomind_ai_agent::model::DeliveryState::Rejected { .. }
        ) && d.destination == "ext-write-1"),
        "control rejected: {:?}",
        exec.deliveries
    );
}

#[tokio::test]
async fn t13_model_omitted_deps_still_blocked_by_frozen_spec() {
    // Even if runner would omit weather from a proposed action, host uses frozen depends_on.
    let now = 1_700_000_000_000i64;
    let h = build_harness(now);
    let draft = partial_meter_draft("meter-1", "weather_cn", "ext-write-1", "test-channel");
    let (spec, _) = h.service.create(draft).unwrap();
    h.sources.set_device(
        "meter-1",
        "power_kw",
        SourceReadResult {
            source_id: "device:meter-1:power_kw".into(),
            binding_proof: "device:meter-1:power_kw".into(),
            observed_at: Some(now),
            valid_window: Some((now - 900_000, now)),
            value: serde_json::json!({ "overload": true, "note": "OVERLOAD", "points": [9.0] }),
        },
    );
    h.sources
        .set_extension_error("weather_cn", "current", "timeout");
    h.notify.reset();
    let resp = h.service.invoke(&spec.id).await.unwrap();
    assert_eq!(h.writes.call_count(), 0);
    assert!(resp
        .execution
        .unwrap()
        .deliveries
        .iter()
        .any(|d| d.destination == "ext-write-1"
            && matches!(
                d.state,
                neomind_ai_agent::model::DeliveryState::Rejected { .. }
            )));
}

#[tokio::test]
async fn t13_dry_run_reads_ok_writes_zero() {
    let now = 1_700_000_000_000i64;
    let h = build_harness(now);
    let draft = partial_meter_draft("meter-1", "weather_cn", "ext-write-1", "test-channel");
    let (spec, _) = h.service.create(draft).unwrap();
    h.sources.set_device(
        "meter-1",
        "power_kw",
        SourceReadResult {
            source_id: "device:meter-1:power_kw".into(),
            binding_proof: "x".into(),
            observed_at: Some(now),
            valid_window: Some((now - 900_000, now)),
            value: serde_json::json!({ "points": [1.0], "overload": false }),
        },
    );
    h.sources.set_extension(
        "weather_cn",
        "current",
        SourceReadResult {
            source_id: "extension:weather_cn".into(),
            binding_proof: "extension:weather_cn:op=current".into(),
            observed_at: Some(now),
            valid_window: Some((now, now + 1000)),
            value: serde_json::json!({ "temp_c": 30 }),
        },
    );
    h.notify.reset();
    let dry = h.service.dry_run(&spec.id).await.unwrap();
    assert!(dry.dry_run);
    assert!(dry.execution.unwrap().evidence.items.len() >= 2);
    assert_eq!(h.notify.count(), 0);
    assert_eq!(h.writes.call_count(), and_zero());
}

fn and_zero() -> u64 {
    0
}
