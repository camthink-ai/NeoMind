//! T11 — dry-run write reject, daily-limit skip (no execution row), busy already covered.

use neomind_ai_agent::harness::build_harness;
use neomind_ai_agent::model::ToolPolicy;
use neomind_ai_agent::model::{daily_brief_draft, PacingPolicy};
use neomind_ai_agent::ports::SourceReadResult;
use neomind_ai_agent::ports::{CapabilityClass, OperationCapability, ToolInvocation};
use neomind_ai_agent::runner::hooks::{SafetyPolicyHook, ToolCallHook};

fn seed(h: &neomind_ai_agent::harness::TestHarness, now: i64) {
    h.sources.set_extension(
        "weather_cn",
        "current",
        SourceReadResult {
            source_id: "extension:weather_cn".into(),
            binding_proof: "extension:weather_cn:op=current".into(),
            observed_at: Some(now),
            valid_window: Some((now, now + 3_600_000)),
            value: serde_json::json!({ "summary": "晴" }),
        },
    );
    h.sources.set_extension(
        "tariff_tou",
        "tou_today",
        SourceReadResult {
            source_id: "extension:tariff_tou".into(),
            binding_proof: "extension:tariff_tou:op=tou_today".into(),
            observed_at: Some(now),
            valid_window: Some((now, now + 86_400_000)),
            value: serde_json::json!({ "off_peak_price": 0.4 }),
        },
    );
}

#[test]
fn t11_dry_run_hook_rejects_side_effect() {
    let hook = SafetyPolicyHook;
    let call = ToolInvocation {
        name: "device_control".into(),
        arguments: serde_json::json!({}),
        extension_id: Some("ext".into()),
        operation: Some("set".into()),
    };
    let cap = OperationCapability {
        class: CapabilityClass::SideEffect,
        scope: None,
        idempotent: false,
    };
    let err = hook
        .before_dispatch(&call, &cap, true, &ToolPolicy::Unrestricted)
        .expect_err("side effect in dry-run");
    assert!(err.contains("dry-run"), "{err}");
}

#[tokio::test]
async fn t11_dry_run_does_not_increment_formal_stats() {
    let now = 1_700_000_000_000i64;
    let h = build_harness(now);
    let (spec, _) = h
        .service
        .create(daily_brief_draft(
            "weather_cn",
            "tariff_tou",
            "test-channel",
        ))
        .unwrap();
    seed(&h, now);
    let before = h.service.runtime(&spec.id).unwrap().stats.total_runs;
    let _ = h.service.dry_run(&spec.id).await.unwrap();
    let after = h.service.runtime(&spec.id).unwrap().stats.total_runs;
    assert_eq!(before, after);
    assert_eq!(h.service.formal_history_len(&spec.id), 0);
}

#[tokio::test]
async fn t11_pacing_max_runs_recorded_on_runtime_when_coordinator_skips() {
    use neomind_ai_agent::coordinator::{ExecutionCoordinator, SubmitSource};
    let now = 1_700_000_000_000i64;
    let h = build_harness(now);
    let mut draft = daily_brief_draft("weather_cn", "tariff_tou", "test-channel");
    draft.pacing = PacingPolicy {
        max_runs_per_day: Some(0),
        ..PacingPolicy::default()
    };
    let (spec, _) = h.service.create(draft).unwrap();
    seed(&h, now);
    let coord = ExecutionCoordinator::new(h.service.clone(), h.store.clone(), h.clock.clone());
    // max_runs_per_day=0 → auto submit skipped; manual still allowed by coordinator logic
    let err = coord
        .submit(&spec.id, SubmitSource::Schedule)
        .await
        .expect_err("daily limit");
    assert!(err.to_string().contains("daily limit"));
    let rt = h.service.runtime(&spec.id).unwrap();
    assert!(rt.stats.skipped_daily_limit >= 1);
    assert_eq!(h.service.formal_history_len(&spec.id), 0);
}
