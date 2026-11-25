//! T12 — Zero-device daily brief (plan §5.2 four steps).
//!
//! Real vendors and real recipients are replaced by CountingSourcePort +
//! FakeNotifyPort. Schema version stays 1.

use std::path::PathBuf;

use neomind_ai_agent::harness::build_harness;
use neomind_ai_agent::model::{
    daily_brief_draft, AgentSpecPatch, EvidenceQuality, ExecutionStatus, ToolPolicy, TriggerSpec,
};
use neomind_ai_agent::ports::SourceReadResult;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/agent-contract/v2/daily-brief.json")
}

fn load_fixture_json() -> serde_json::Value {
    let raw = std::fs::read_to_string(fixture_path()).expect("daily-brief fixture");
    serde_json::from_str(&raw).expect("parse fixture")
}

fn shanghai_0900_ms() -> i64 {
    // 2026-11-25 09:00:00 Asia/Shanghai = 2026-11-25 01:00:00 UTC
    chrono::DateTime::parse_from_rfc3339("2026-11-25T01:00:00Z")
        .unwrap()
        .timestamp_millis()
}

fn seed_sources(
    h: &neomind_ai_agent::harness::TestHarness,
    weather: serde_json::Value,
    tariff: serde_json::Value,
    now: i64,
) {
    let window = (now - 3_600_000, now + 3_600_000);
    h.sources.set_extension(
        "weather_cn",
        "current",
        SourceReadResult {
            source_id: "extension:weather_cn".into(),
            binding_proof: "extension:weather_cn:op=current".into(),
            observed_at: Some(now - 60_000),
            valid_window: Some(window),
            value: weather,
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
            value: tariff,
        },
    );
}

#[tokio::test]
async fn t12_four_steps_zero_device_daily_brief() {
    let fixture = load_fixture_json();
    let now = shanghai_0900_ms();
    let h = build_harness(now);

    // --- Step 1: save+readback; static preview external reads = 0 ---
    let draft = daily_brief_draft("weather_cn", "tariff_tou", "test-channel");
    // Confirm fixture agrees with preset on critical fields
    assert_eq!(fixture["draft"]["trigger"]["timezone"], "Asia/Shanghai");
    assert_eq!(fixture["draft"]["tool_policy"], "disabled");

    h.sources.reset_counts();
    let (spec, plan) = h.service.create(draft).expect("create");
    let readback = h.service.get(&spec.id).expect("readback");
    assert_eq!(readback.id, spec.id);
    assert_eq!(readback.name, "daily-brief");
    match &readback.trigger {
        TriggerSpec::Cron { expr, timezone } => {
            assert_eq!(expr, "0 9 * * *");
            assert_eq!(timezone, "Asia/Shanghai");
        }
        other => panic!("expected cron, got {other:?}"),
    }
    assert!(matches!(readback.tool_policy, ToolPolicy::Disabled));
    assert_eq!(readback.notify.channels, vec!["test-channel".to_string()]);
    assert_eq!(readback.input.requirements.len(), 2);
    assert_eq!(
        plan.external_read_counts.weather, 0,
        "step1: weather reads must be 0"
    );
    assert_eq!(
        plan.external_read_counts.tariff, 0,
        "step1: tariff reads must be 0"
    );
    assert_eq!(h.sources.weather_reads(), 0, "step1: source port weather=0");
    assert_eq!(h.sources.tariff_reads(), 0, "step1: source port tariff=0");
    assert_eq!(h.notify.count(), 0);

    // --- Step 2: dry-run — both sources read; valid windows; notify=0; history unchanged ---
    let weather_a = fixture["source_fixtures"]["weather_a"].clone();
    let tariff_a = fixture["source_fixtures"]["tariff_a"].clone();
    seed_sources(&h, weather_a.clone(), tariff_a.clone(), now);
    h.sources.reset_counts();
    h.notify.reset();
    let hist_before = h.service.formal_history_len(&spec.id);

    let dry = h.service.dry_run(&spec.id).await.expect("dry-run");
    assert!(dry.dry_run, "step2: dry_run flag");
    assert_eq!(
        dry.external_read_counts.weather, 1,
        "step2: weather read once"
    );
    assert_eq!(
        dry.external_read_counts.tariff, 1,
        "step2: tariff read once"
    );
    assert_eq!(h.sources.weather_reads(), 1);
    assert_eq!(h.sources.tariff_reads(), 1);
    assert_eq!(
        dry.receiver_counts.notify, 0,
        "step2: fake receiver must be 0"
    );
    assert_eq!(h.notify.count(), 0, "step2: notify port count 0");
    assert_eq!(
        h.service.formal_history_len(&spec.id),
        hist_before,
        "step2: formal history unchanged"
    );
    let dry_exec = dry.execution.as_ref().expect("dry execution");
    for key in ["weather_now", "tariff_today"] {
        let item = dry_exec
            .evidence
            .items
            .iter()
            .find(|i| i.key == key)
            .unwrap_or_else(|| panic!("step2: missing evidence {key}"));
        assert!(
            matches!(item.quality, EvidenceQuality::Fresh),
            "step2: {key} fresh"
        );
        assert!(
            item.valid_window.is_some(),
            "step2: {key} must carry valid_window"
        );
    }
    assert!(
        matches!(readback.tool_policy, ToolPolicy::Disabled),
        "step2: model tools stay disabled"
    );

    // --- Step 3: formal trigger with DISTINCT fixtures; receiver exactly 1; time-only patch ---
    let weather_b = fixture["source_fixtures"]["weather_b"].clone();
    let tariff_b = fixture["source_fixtures"]["tariff_b"].clone();
    seed_sources(&h, weather_b.clone(), tariff_b.clone(), now);
    h.sources.reset_counts();
    h.notify.reset();

    let formal = h.service.invoke(&spec.id).await.expect("formal invoke");
    assert!(!formal.dry_run);
    let exec = formal.execution.as_ref().expect("formal execution");
    assert_eq!(exec.status, ExecutionStatus::Completed);
    let brief = exec.brief_text.as_deref().unwrap_or("");
    let weather_summary = weather_b["summary"].as_str().unwrap();
    let tariff_peak = tariff_b["peaks"][0]["price"].as_f64().unwrap();
    assert!(
        brief.contains(weather_summary),
        "step3: brief must contain weather fixture `{weather_summary}`; brief={brief}"
    );
    assert!(
        brief.contains(&tariff_peak.to_string()) || brief.contains("1.5"),
        "step3: brief must contain tariff price; brief={brief}"
    );
    for key in ["weather_now", "tariff_today"] {
        let item = exec
            .evidence
            .items
            .iter()
            .find(|i| i.key == key)
            .expect("traceable evidence");
        assert!(
            item.binding_proof.contains("extension:"),
            "step3: {key} binding_proof traceable: {}",
            item.binding_proof
        );
    }
    assert_eq!(
        h.notify.count(),
        1,
        "step3: fake receiver must be exactly 1, got {}",
        h.notify.count()
    );
    assert_eq!(formal.receiver_counts.notify, 1);
    assert_eq!(h.service.formal_history_len(&spec.id), 1);

    // Time-only change → same id / permissions / destination / timezone
    let (patched, _) = h
        .service
        .update(
            &spec.id,
            AgentSpecPatch {
                trigger: Some(TriggerSpec::Cron {
                    expr: "0 10 * * *".into(),
                    timezone: "Asia/Shanghai".into(),
                }),
                ..Default::default()
            },
        )
        .expect("patch time");
    assert_eq!(patched.id, spec.id, "step3: id preserved");
    assert_eq!(patched.notify.channels, spec.notify.channels);
    assert_eq!(
        patched.input.requirements, spec.input.requirements,
        "step3: inputs/permissions preserved"
    );
    assert!(matches!(patched.tool_policy, ToolPolicy::Disabled));
    match &patched.trigger {
        TriggerSpec::Cron { expr, timezone } => {
            assert_eq!(expr, "0 10 * * *");
            assert_eq!(timezone, "Asia/Shanghai");
        }
        other => panic!("{other:?}"),
    }

    // A → B → query sequence (§4.2)
    // A already ran (formal). B = dry-run. Query last formal reason — no new formal runs / notify.
    h.notify.reset();
    let hist = h.service.formal_history_len(&spec.id);
    let notify_before_query = h.notify.count();
    let _b = h.service.dry_run(&spec.id).await.expect("B dry-run");
    let explain = h.service.explain_last(&spec.id).expect("query");
    assert!(
        explain.contains(&exec.id),
        "query must return formal A execution id {}; got {explain}",
        exec.id
    );
    assert_eq!(
        h.service.formal_history_len(&spec.id),
        hist,
        "query must not add formal history"
    );
    assert_eq!(
        h.notify.count(),
        notify_before_query,
        "query must not notify"
    );

    // --- Step 4: report identifiers (asserted so CI captures them) ---
    eprintln!(
        "T12 report: baseline=main@d63605af7 candidate=neomind-ai-agent agent_id={} exec_a={} fixture={} cmds=cargo test -p neomind-ai-agent --test t12_daily_brief",
        spec.id,
        exec.id,
        fixture_path().display()
    );
}
