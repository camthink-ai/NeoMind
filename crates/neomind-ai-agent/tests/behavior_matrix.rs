//! Deterministic behavior matrix coverage for M1 (Busy, pause CAS, crash intent, muted notify).

use neomind_ai_agent::coordinator::{ExecutionCoordinator, SubmitSource};
use neomind_ai_agent::harness::build_harness;
use neomind_ai_agent::model::{daily_brief_draft, AgentStatus, DeliveryState, NotifyPolicy};
use neomind_ai_agent::ports::{
    DisabledTools, SourceReadResult, ToolExecContext, ToolInvocation, ToolRegistryPort,
};
use neomind_ai_agent::post_run::PostRunPipeline;

fn seed_brief(h: &neomind_ai_agent::harness::TestHarness, now: i64) {
    h.sources.set_extension(
        "weather_cn",
        "current",
        SourceReadResult {
            source_id: "extension:weather_cn".into(),
            binding_proof: "extension:weather_cn:op=current".into(),
            observed_at: Some(now),
            valid_window: Some((now, now + 3_600_000)),
            value: serde_json::json!({ "summary": "晴", "temp_c": 20 }),
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

#[tokio::test]
async fn busy_rejects_second_submit() {
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
    seed_brief(&h, now);
    let coord = ExecutionCoordinator::new(h.service.clone(), h.store.clone(), h.clock.clone());
    assert!(coord.state.try_enter(&spec.id));
    let err = coord
        .submit(&spec.id, SubmitSource::Manual)
        .await
        .expect_err("busy");
    assert!(matches!(err, neomind_ai_agent::AgentError::Busy));
    coord.state.leave(&spec.id);
}

#[tokio::test]
async fn operator_pause_cas_wins() {
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
    let pipeline = PostRunPipeline {
        store: h.store.clone(),
    };
    pipeline.force_operator_pause(&spec.id, now).unwrap();
    let rt = h.service.runtime(&spec.id).unwrap();
    assert_eq!(rt.status, AgentStatus::Paused);
    let err = h.service.invoke(&spec.id).await.expect_err("paused");
    assert!(matches!(err, neomind_ai_agent::AgentError::NotRunnable(_)));
}

#[tokio::test]
async fn crash_before_receipt_marks_outcome_unknown_no_resend() {
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
    seed_brief(&h, now);
    h.notify.reset();
    let resp = h.service.invoke(&spec.id).await.unwrap();
    assert_eq!(h.notify.count(), 1);
    let exec_id = resp.execution.unwrap().id;
    let claimed = h
        .store
        .try_claim_delivery(neomind_ai_agent::model::ActionDeliveryRecord {
            execution_id: exec_id,
            destination: "test-channel".into(),
            action_fingerprint: "on_complete:crash-sim".into(),
            depends_on: vec![],
            idempotent: false,
            state: DeliveryState::IntentPending,
        })
        .unwrap();
    assert!(claimed);
    let recovered = h.store.recover_stale_intents();
    assert_eq!(recovered.len(), 1);
    assert!(matches!(
        recovered[0].state,
        DeliveryState::OutcomeUnknown { .. }
    ));
    assert_eq!(h.notify.count(), 1, "non-idempotent must not auto-resend");
}

#[tokio::test]
async fn silent_channels_deliver_zero() {
    let now = 1_700_000_000_000i64;
    let h = build_harness(now);
    let mut draft = daily_brief_draft("weather_cn", "tariff_tou", "test-channel");
    draft.notify = NotifyPolicy {
        channels: vec![],
        on_complete: true,
        on_alert: false,
        alert_depends_on: vec![],
        on_failure: true,
        pending_confirmation: false,
    };
    let (spec, _) = h.service.create(draft).unwrap();
    seed_brief(&h, now);
    h.notify.reset();
    let _ = h.service.invoke(&spec.id).await.unwrap();
    assert_eq!(h.notify.count(), 0);
}

#[tokio::test]
async fn tools_disabled_rejects_registry_execute() {
    let tools = DisabledTools;
    let err = tools
        .execute(
            ToolInvocation {
                name: "shell".into(),
                arguments: serde_json::json!({}),
                extension_id: None,
                operation: None,
            },
            &ToolExecContext {
                dry_run: false,
                execution_id: "x".into(),
            },
        )
        .await
        .expect_err("disabled");
    assert!(matches!(err, neomind_ai_agent::AgentError::ToolRejected(_)));
}
