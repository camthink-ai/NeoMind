//! Production batch 1 — items 2→3→4 then 5,6,1 (NM-396).

use neomind_ai_agent::coordinator::{ExecutionCoordinator, SubmitSource};
use neomind_ai_agent::harness::build_harness;
use neomind_ai_agent::model::{
    daily_brief_draft, partial_meter_draft, AgentSpecPatch, ControlledAction, DeliveryState,
    ExecutionStatus, NotifyPolicy, PacingPolicy, TriggerSpec,
};
use neomind_ai_agent::ports::{SideEffectReceipt, SourceReadResult};
use neomind_ai_agent::runner::{AgentRunner, RunOutcome};
use neomind_ai_agent::AgentError;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

fn seed_brief(h: &neomind_ai_agent::harness::TestHarness, now: i64) {
    let window = (now - 3_600_000, now + 3_600_000);
    h.sources.set_extension(
        "weather_cn",
        "current",
        SourceReadResult {
            source_id: "extension:weather_cn".into(),
            binding_proof: "extension:weather_cn:op=current".into(),
            observed_at: Some(now - 60_000),
            valid_window: Some(window),
            value: serde_json::json!({ "temp_c": 22 }),
        },
    );
    h.sources.set_extension(
        "tariff_tou",
        "tou_today",
        SourceReadResult {
            source_id: "extension:tariff_tou".into(),
            binding_proof: "extension:tariff_tou:op=tou_today".into(),
            observed_at: Some(now - 120_000),
            valid_window: Some(window),
            value: serde_json::json!({ "price": 0.8 }),
        },
    );
}

/// Runner that always fails — settles as Failed.
struct AlwaysFailRunner;

#[async_trait::async_trait]
impl AgentRunner for AlwaysFailRunner {
    async fn run(
        &self,
        _spec: &neomind_ai_agent::model::AgentSpec,
        _evidence: &neomind_ai_agent::model::EvidenceReport,
        _execution_id: &str,
        _cancel: CancellationToken,
    ) -> neomind_ai_agent::Result<RunOutcome> {
        Err(AgentError::Other("injected inference failure".into()))
    }
}

#[tokio::test]
async fn item2_failed_forbids_controlled_actions() {
    let now = 1_700_000_000_000i64;
    let h = build_harness(now);
    let mut draft = daily_brief_draft("weather_cn", "tariff_tou", "test-channel");
    draft.controlled_actions = vec![ControlledAction {
        name: "open_valve".into(),
        destination: "device:valve-1".into(),
        depends_on: vec![],
        idempotent: false,
        params: serde_json::json!({}),
    }];
    draft.notify = NotifyPolicy {
        channels: vec!["test-channel".into()],
        on_complete: true,
        on_alert: false,
        alert_depends_on: vec![],
        on_failure: true,
        pending_confirmation: false,
    };
    let (spec, _) = h.service.create(draft).unwrap();
    seed_brief(&h, now);

    // Swap in failing runner via a fresh service boundary is awkward; simulate
    // by settling a Failed exec directly through the harness side_effects path.
    let exec = neomind_ai_agent::model::AgentExecutionRecord {
        id: "exec_fail1".into(),
        agent_id: spec.id.clone(),
        config_version: spec.config_version,
        status: ExecutionStatus::Failed,
        started_at: now,
        finished_at: now,
        is_probe: false,
        dry_run: false,
        trigger_source: Some("manual".into()),
        stale_config: None,
        evidence: Default::default(),
        deliveries: vec![],
        brief_text: None,
        conclusion: None,
        structured_result: None,
        error: Some("boom".into()),
        alert_verdict: None,
    };
    h.store.save_execution(exec.clone()).unwrap();
    let settled = h
        .service
        .side_effects
        .settle(&spec, exec, false)
        .await
        .unwrap();
    assert_eq!(h.writes.call_count(), 0, "Failed must not control-write");
    assert!(
        !settled
            .deliveries
            .iter()
            .any(|d| d.destination == "device:valve-1"
                && matches!(
                    d.state,
                    DeliveryState::Accepted
                        | DeliveryState::Confirmed
                        | DeliveryState::IntentPending
                )),
        "no control delivery accepted: {:?}",
        settled.deliveries
    );
    // on_failure notify still allowed
    assert_eq!(h.notify.count(), 1);
    assert!(settled.deliveries.iter().any(|d| {
        d.action_fingerprint.starts_with("on_failure:")
            && matches!(d.state, DeliveryState::Accepted)
    }));
}

#[tokio::test]
async fn item2_skipped_and_probe_zero_writes() {
    let now = 1_700_000_000_000i64;
    let h = build_harness(now);
    let mut draft = partial_meter_draft("meter-1", "weather_cn", "ext-write-1", "test-channel");
    draft.notify.on_complete = false;
    draft.notify.on_alert = false;
    let (spec, _) = h.service.create(draft).unwrap();

    let skipped = neomind_ai_agent::model::AgentExecutionRecord {
        id: "exec_skip".into(),
        agent_id: spec.id.clone(),
        config_version: 1,
        status: ExecutionStatus::Skipped,
        started_at: now,
        finished_at: now,
        is_probe: false,
        dry_run: false,
        trigger_source: Some("schedule".into()),
        stale_config: None,
        evidence: Default::default(),
        deliveries: vec![],
        brief_text: None,
        conclusion: None,
        structured_result: None,
        error: None,
        alert_verdict: None,
    };
    let _ = h
        .service
        .side_effects
        .settle(&spec, skipped, false)
        .await
        .unwrap();
    assert_eq!(h.writes.call_count(), 0);

    let probe = neomind_ai_agent::model::AgentExecutionRecord {
        id: "exec_probe".into(),
        agent_id: spec.id.clone(),
        config_version: 1,
        status: ExecutionStatus::Completed,
        started_at: now,
        finished_at: now,
        is_probe: true,
        dry_run: false,
        trigger_source: Some("schedule".into()),
        stale_config: None,
        evidence: Default::default(),
        deliveries: vec![],
        brief_text: Some("ok".into()),
        conclusion: None,
        structured_result: None,
        error: None,
        alert_verdict: None,
    };
    let _ = h
        .service
        .side_effects
        .settle(&spec, probe, false)
        .await
        .unwrap();
    assert_eq!(h.writes.call_count(), 0);
    assert_eq!(h.notify.count(), 0);
}

#[tokio::test]
async fn item3_receipts_accepted_vs_confirmed_and_reject_mirrored() {
    let now = 1_700_000_000_000i64;
    let h = build_harness(now);
    let mut draft = daily_brief_draft("weather_cn", "tariff_tou", "test-channel");
    draft.notify.on_complete = false;
    draft.controlled_actions = vec![ControlledAction {
        name: "ping".into(),
        destination: "device:x".into(),
        depends_on: vec!["weather_now".into()],
        idempotent: true,
        params: serde_json::json!({}),
    }];
    let (spec, _) = h.service.create(draft).unwrap();
    seed_brief(&h, now);

    // Default fake → Accepted (not Confirmed).
    let resp = h.service.invoke(&spec.id).await.unwrap();
    let exec = resp.execution.unwrap();
    let ctrl = exec
        .deliveries
        .iter()
        .find(|d| d.destination == "device:x")
        .expect("control receipt");
    assert!(
        matches!(ctrl.state, DeliveryState::Accepted),
        "adapter accept ≠ device ack: {:?}",
        ctrl.state
    );
    assert!(!matches!(ctrl.state, DeliveryState::Confirmed));

    // Confirmed only when the port reports device ACK.
    h.writes.set_confirm(true);
    let mut draft2 = daily_brief_draft("weather_cn", "tariff_tou", "ch2");
    draft2.notify.on_complete = false;
    draft2.controlled_actions = vec![ControlledAction {
        name: "ping".into(),
        destination: "device:y".into(),
        depends_on: vec!["weather_now".into()],
        idempotent: true,
        params: serde_json::json!({}),
    }];
    let (spec2, _) = h.service.create(draft2).unwrap();
    seed_brief(&h, now);
    let exec2 = h
        .service
        .invoke(&spec2.id)
        .await
        .unwrap()
        .execution
        .unwrap();
    assert!(exec2
        .deliveries
        .iter()
        .any(|d| { d.destination == "device:y" && matches!(d.state, DeliveryState::Confirmed) }));

    let _ = SideEffectReceipt::Accepted;
}

#[tokio::test]
async fn item3_rejected_control_mirrored_on_execution() {
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
    let exec = h.service.invoke(&spec.id).await.unwrap().execution.unwrap();
    assert_eq!(exec.status, ExecutionStatus::Partial);
    assert!(
        exec.deliveries
            .iter()
            .any(|d| d.destination == "ext-write-1"
                && matches!(d.state, DeliveryState::Rejected { .. })),
        "rejected control must be on exec: {:?}",
        exec.deliveries
    );
}

#[tokio::test]
async fn item4_non_idempotent_intent_survives_crash_without_resend() {
    let now = 1_700_000_000_000i64;
    let h = build_harness(now);
    let mut draft = daily_brief_draft("weather_cn", "tariff_tou", "test-channel");
    draft.controlled_actions = vec![ControlledAction {
        name: "open".into(),
        destination: "device:valve".into(),
        depends_on: vec!["weather_now".into()],
        idempotent: false,
        params: serde_json::json!({}),
    }];
    draft.notify.on_complete = false;
    let (spec, _) = h.service.create(draft).unwrap();
    seed_brief(&h, now);

    let resp = h.service.invoke(&spec.id).await.unwrap();
    let exec_id = resp.execution.as_ref().unwrap().id.clone();
    assert_eq!(h.writes.call_count(), 1);
    assert_eq!(h.service.formal_history_len(&spec.id), 1);

    // Simulate crash after a second non-idempotent claim with no completion.
    let claimed = h
        .store
        .try_claim_delivery(neomind_ai_agent::model::ActionDeliveryRecord {
            execution_id: exec_id.clone(),
            destination: "device:valve".into(),
            action_fingerprint: "open:device:valve:retry".into(),
            depends_on: vec![],
            idempotent: false,
            state: DeliveryState::IntentPending,
        })
        .unwrap();
    assert!(claimed);
    let recovered = h.store.recover_stale_intents();
    assert!(recovered
        .iter()
        .any(|d| matches!(d.state, DeliveryState::OutcomeUnknown { .. })));
    assert_eq!(
        h.writes.call_count(),
        1,
        "recover must not resend; receiver stays at 1"
    );
}

#[tokio::test]
async fn item5_cross_day_quota_rolls_and_skips() {
    // 2024-01-01 10:00 UTC
    let day1 = 1_704_103_200_000i64;
    let h = build_harness(day1);
    let mut draft = daily_brief_draft("weather_cn", "tariff_tou", "test-channel");
    draft.trigger = TriggerSpec::Interval { every_secs: 60 };
    draft.pacing = PacingPolicy {
        max_runs_per_day: Some(1),
        timezone: Some("UTC".into()),
        ..PacingPolicy::default()
    };
    draft.notify.on_complete = false;
    let (spec, _) = h.service.create(draft).unwrap();
    seed_brief(&h, day1);

    let coord = ExecutionCoordinator::new(h.service.clone(), h.store.clone(), h.clock.clone());
    coord
        .submit(&spec.id, SubmitSource::Schedule)
        .await
        .expect("first run");
    let rt = h.service.runtime(&spec.id).unwrap();
    assert_eq!(rt.stats.runs_today, 1);
    assert_eq!(rt.stats.quota_day.as_deref(), Some("2024-01-01"));

    let err = coord
        .submit(&spec.id, SubmitSource::Schedule)
        .await
        .expect_err("quota");
    assert!(err.to_string().contains("daily limit"));
    assert!(
        h.service
            .runtime(&spec.id)
            .unwrap()
            .stats
            .skipped_daily_limit
            >= 1
    );

    // Midnight restore (next UTC day).
    h.clock.set(day1 + 24 * 3_600_000);
    seed_brief(&h, day1 + 24 * 3_600_000);
    coord
        .submit(&spec.id, SubmitSource::Schedule)
        .await
        .expect("next day");
    let rt = h.service.runtime(&spec.id).unwrap();
    assert_eq!(rt.stats.runs_today, 1);
    assert_eq!(rt.stats.quota_day.as_deref(), Some("2024-01-02"));
}

#[tokio::test]
async fn item6_formal_history_idempotent_by_execution_id() {
    let now = 1_700_000_000_000i64;
    let h = build_harness(now);
    let mut draft = daily_brief_draft("weather_cn", "tariff_tou", "test-channel");
    draft.notify.on_complete = false;
    let (spec, _) = h.service.create(draft).unwrap();
    seed_brief(&h, now);

    let resp = h.service.invoke(&spec.id).await.unwrap();
    let exec = resp.execution.unwrap();
    assert_eq!(h.service.formal_history_len(&spec.id), 1);

    // Re-save same id (pre/post settle pattern) must not duplicate.
    h.store.save_execution(exec.clone()).unwrap();
    h.store.save_execution(exec).unwrap();
    assert_eq!(h.service.formal_history_len(&spec.id), 1);

    let dry = h.service.dry_run(&spec.id).await.unwrap();
    assert!(dry.dry_run);
    assert_eq!(
        h.service.formal_history_len(&spec.id),
        1,
        "dry-run must not enter formal history"
    );
}

#[tokio::test]
async fn item1_name_only_patch_preserves_contracts_and_plan() {
    let now = 1_700_000_000_000i64;
    let h = build_harness(now);
    let draft = daily_brief_draft("weather_cn", "tariff_tou", "test-channel");
    let (spec, plan_before) = h.service.create(draft).unwrap();

    let (updated, plan_after) = h
        .service
        .update(
            &spec.id,
            AgentSpecPatch {
                name: Some("renamed-brief".into()),
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(updated.name, "renamed-brief");
    assert_eq!(updated.user_prompt, spec.user_prompt);
    assert_eq!(updated.input, spec.input);
    assert_eq!(updated.output, spec.output);
    assert_eq!(updated.notify, spec.notify);
    assert_eq!(updated.tool_policy, spec.tool_policy);
    assert_eq!(updated.controlled_actions, spec.controlled_actions);
    assert_eq!(updated.pacing, spec.pacing);
    assert_eq!(updated.trigger, spec.trigger);
    assert_eq!(updated.config_version, spec.config_version + 1);

    // Static plan preview stays aligned with saved contracts (name change only).
    assert_eq!(plan_after.authorized_reads, plan_before.authorized_reads);
    assert_eq!(plan_after.notify, plan_before.notify);
    assert_eq!(plan_after.tool_policy, plan_before.tool_policy);
    assert_eq!(
        plan_after.controlled_actions,
        plan_before.controlled_actions
    );
    assert_eq!(plan_after.external_read_counts.total(), 0);
}

// Keep AlwaysFailRunner referenced so the type stays available for future harness swaps.
#[allow(dead_code)]
fn _touch_fail_runner() -> Arc<dyn AgentRunner> {
    Arc::new(AlwaysFailRunner)
}
