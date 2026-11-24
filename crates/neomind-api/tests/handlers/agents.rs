//! Tests for the agent handlers — what the editor sends must be what gets stored.
//!
//! Regression guards for two hand-rolled string→enum mappings that had drifted
//! from the storage enums the rest of the system agrees on.

use axum::extract::{Path, Query, State};
use axum::Json;
use neomind_api::handlers::agents::{
    create_agent, get_agent, update_agent, CreateAgentRequest, UpdateAgentRequest,
};
use neomind_api::handlers::data::{list_all_data_sources_handler, ListDataSourcesQuery};
use neomind_api::handlers::ServerState;
use serde_json::json;

async fn create_test_server_state() -> ServerState {
    crate::common::create_test_server_state().await
}

/// POST a minimal valid agent, with `overrides` layered on top, and return its id.
///
/// The base body is the shape the editor produces for a structured agent:
/// an interval schedule and an output contract.
async fn create_agent_with(state: &ServerState, overrides: serde_json::Value) -> String {
    let mut body = json!({
        "name": "包装台质检",
        "user_prompt": "看一下包装台画面有没有错装漏装",
        "schedule": { "schedule_type": "interval", "interval_seconds": 3600 },
        "output_schema": [
            { "name": "missing_count", "field_type": { "type": "number" } }
        ],
    });
    for (key, value) in overrides.as_object().expect("overrides must be an object") {
        body[key] = value.clone();
    }
    let request: CreateAgentRequest = serde_json::from_value(body).expect("valid create fixture");

    create_agent(State(state.clone()), Json(request))
        .await
        .expect("create must accept the request")
        .0
        .data
        .expect("create returns a data payload")["id"]
        .as_str()
        .expect("create returns an id")
        .to_string()
}

async fn read_back(state: &ServerState, id: &str) -> serde_json::Value {
    get_agent(State(state.clone()), Path(id.to_string()))
        .await
        .expect("created agent must be readable")
        .0
        .data
        .expect("get_agent returns a data payload")
}

/// A structured agent has no tools and no actions — its inputs are plain data
/// sources — so it must be creatable without any resource binding.
#[tokio::test]
async fn test_create_agent_keeps_structured_execution_mode() {
    let state = create_test_server_state().await;

    let id = create_agent_with(&state, json!({ "execution_mode": "structured" })).await;

    assert_eq!(
        read_back(&state, &id).await["execution_mode"],
        "structured",
        "create must persist the mode the editor sent"
    );
}

/// Switching an existing agent to structured — what the editor's mode card
/// does on edit — must land on the same mode, not silently fall back.
#[tokio::test]
async fn test_update_agent_keeps_structured_execution_mode() {
    let state = create_test_server_state().await;

    // Free mode needs no resource bindings, so it is the cheapest starting point.
    let id = create_agent_with(&state, json!({ "execution_mode": "free" })).await;

    let switch: UpdateAgentRequest = serde_json::from_value(json!({
        "execution_mode": "structured",
    }))
    .expect("valid update fixture");

    let _response = update_agent(State(state.clone()), Path(id.clone()), Json(switch))
        .await
        .expect("switching a free agent to structured must be accepted");

    assert_eq!(
        read_back(&state, &id).await["execution_mode"],
        "structured",
        "update must persist the mode the editor sent"
    );
}

/// The task definition is copied into `custom:task-understanding` when the
/// agent is created, and every run inlines that copy. A prompt edit must
/// reach the copy too — on T4 "用 vision 工具分析该图片" outlived its prompt
/// edit by two days because only `user_prompt` was updated.
#[tokio::test]
async fn test_update_agent_prompt_resyncs_task_understanding_mission() {
    const OLD_MISSION: &str = "看一下包装台画面有没有错装漏装";
    const NEW_MISSION: &str = "NEW_MISSION 直接看附带的图片";

    let state = create_test_server_state().await;
    let id = create_agent_with(&state, json!({ "execution_mode": "free" })).await;
    let memory = state.agents.system_memory_store.clone();

    let before = memory
        .read_agent_custom_file(&id, "task-understanding")
        .expect("created agents get a task-understanding file");
    assert_eq!(
        neomind_storage::task_understanding_mission(&before),
        Some(OLD_MISSION),
        "{before}"
    );

    let edit: UpdateAgentRequest = serde_json::from_value(json!({ "user_prompt": NEW_MISSION }))
        .expect("valid update fixture");
    let _ = update_agent(State(state.clone()), Path(id.clone()), Json(edit))
        .await
        .expect("a prompt edit must be accepted");

    let after = memory
        .read_agent_custom_file(&id, "task-understanding")
        .expect("file still there");
    assert_eq!(
        neomind_storage::task_understanding_mission(&after),
        Some(NEW_MISSION),
        "the prompt edit must reach the inlined copy: {after}"
    );
    // Only the Mission section moves; identity, resources, and whatever the
    // agent appended stay byte for byte.
    let strip = |s: &str| s.replace(OLD_MISSION, "").replace(NEW_MISSION, "");
    assert_eq!(strip(&after), strip(&before), "{after}");
    // The index entry shown when a file is not inlined follows the edit.
    let listed = read_back(&state, &id).await;
    assert_eq!(
        listed["memory"]["knowledge_files"][0]["description"],
        format!("Task context: {NEW_MISSION}")
    );
}

/// An agent that replaced its own task with a per-run observation (T4's
/// 200366: "无图像数据路径，需设备日志搜索") gets the mission back on top
/// the next time its prompt is edited — the observation is kept as a note.
#[tokio::test]
async fn test_update_agent_prompt_restores_a_missing_mission() {
    let state = create_test_server_state().await;
    let id = create_agent_with(&state, json!({ "execution_mode": "free" })).await;
    let memory = state.agents.system_memory_store.clone();

    let note = "# NE301PIR-200366 任务理解\n## 图像数据路径\n- 无图像数据路径，需设备日志搜索\n";
    memory
        .write_agent_custom_file(&id, "task-understanding", note)
        .await
        .expect("the agent overwrote its own file");

    let edit: UpdateAgentRequest =
        serde_json::from_value(json!({ "user_prompt": "只在工作日告警" })).expect("valid update");
    let _ = update_agent(State(state.clone()), Path(id.clone()), Json(edit))
        .await
        .expect("prompt edit accepted");

    let after = memory
        .read_agent_custom_file(&id, "task-understanding")
        .expect("file");
    assert_eq!(
        neomind_storage::task_understanding_mission(&after),
        Some("只在工作日告警"),
        "{after}"
    );
    assert!(
        after.ends_with(note),
        "the agent's note is kept below: {after}"
    );
}

/// Operators edit this file too. A prompt edit replaces exactly the text of
/// the previous prompt: a section they filed between Mission and Resources
/// stays byte for byte, and a previous prompt with headings of its own is
/// taken out whole instead of up to its first heading.
#[tokio::test]
async fn test_update_agent_prompt_keeps_the_operators_section_next_to_the_mission() {
    const NEW_MISSION: &str = "NEW_MISSION 直接看附带的图片";
    for old_mission in [
        "看一下包装台画面有没有错装漏装",
        "每天巡检。\n## 判定标准\n用 vision 工具分析该图片\n\n---\n周日休息",
    ] {
        let state = create_test_server_state().await;
        let id = create_agent_with(
            &state,
            json!({ "execution_mode": "free", "user_prompt": old_mission }),
        )
        .await;
        let memory = state.agents.system_memory_store.clone();

        let before = memory
            .read_agent_custom_file(&id, "task-understanding")
            .expect("created agents get a task-understanding file");
        assert!(
            before.contains(&format!("## Mission\n{old_mission}\n\n## Bound Resources")),
            "{before}"
        );
        // The operator files a section of their own right under the mission.
        let filed = before.replacen(
            "## Bound Resources",
            "## Operator Notes\nKEEP-ME\n\n## Bound Resources",
            1,
        );
        memory
            .write_agent_custom_file(&id, "task-understanding", &filed)
            .await
            .expect("the operator edits the file");

        let edit: UpdateAgentRequest =
            serde_json::from_value(json!({ "user_prompt": NEW_MISSION })).expect("valid update");
        let _ = update_agent(State(state.clone()), Path(id.clone()), Json(edit))
            .await
            .expect("a prompt edit must be accepted");

        let after = memory
            .read_agent_custom_file(&id, "task-understanding")
            .expect("file");
        assert_eq!(
            after,
            filed.replacen(old_mission, NEW_MISSION, 1),
            "{after}"
        );
        assert!(after.contains("## Operator Notes\nKEEP-ME\n\n## Bound Resources"));
        assert!(!after.contains("vision"), "{after}");
    }
}

/// When the section no longer starts with the previous prompt — the operator
/// reworded it, the agent overwrote it — its extent is not known, and what
/// sits under the heading may be theirs. The edit leaves the file alone; the
/// task takes effect from `user_prompt` when the agent runs.
#[tokio::test]
async fn test_update_agent_prompt_leaves_a_file_it_cannot_match_untouched() {
    let state = create_test_server_state().await;
    let id = create_agent_with(&state, json!({ "execution_mode": "free" })).await;
    let memory = state.agents.system_memory_store.clone();

    let before = memory
        .read_agent_custom_file(&id, "task-understanding")
        .expect("file");
    let hand_edited = before.replacen(
        "## Mission\n看一下包装台画面有没有错装漏装\n",
        "## Mission\n我自己改过的任务说明\n\n## Operator Notes\nKEEP-ME\n",
        1,
    );
    assert_ne!(hand_edited, before);
    memory
        .write_agent_custom_file(&id, "task-understanding", &hand_edited)
        .await
        .expect("the operator edits the file");

    let edit: UpdateAgentRequest =
        serde_json::from_value(json!({ "user_prompt": "只在工作日告警" })).expect("valid update");
    let _ = update_agent(State(state.clone()), Path(id.clone()), Json(edit))
        .await
        .expect("a prompt edit must be accepted");

    let after = memory
        .read_agent_custom_file(&id, "task-understanding")
        .expect("file");
    assert_eq!(after, hand_edited, "the operator's file is theirs to fix");
    assert_eq!(
        read_back(&state, &id).await["user_prompt"],
        "只在工作日告警",
        "the task itself did change"
    );
}

/// The parsed intent is derived from the prompt once and then cached. After a
/// prompt edit it has to be re-derived: a stale intent still evaluates its
/// old conditions into `condition_met` decisions, which run the agent's
/// bound commands.
#[tokio::test]
async fn test_update_agent_prompt_drops_the_cached_parsed_intent() {
    let state = create_test_server_state().await;
    let id = create_agent_with(&state, json!({ "execution_mode": "free" })).await;
    let store = &state.agents.agent_store;

    let mut agent = store
        .get_agent(&id)
        .await
        .expect("read back")
        .expect("agent");
    agent.parsed_intent = Some(neomind_storage::ParsedIntent {
        intent_type: neomind_storage::IntentType::AnomalyDetection,
        target_metrics: vec!["temperature".into()],
        conditions: vec!["temperature > 30".into()],
        actions: vec!["send_alert".into()],
        confidence: 0.9,
    });
    store
        .save_agent(&agent)
        .await
        .expect("seed a cached intent");

    async fn cached_intent(
        store: &neomind_storage::AgentStore,
        id: &str,
    ) -> Option<neomind_storage::ParsedIntent> {
        store
            .get_agent(id)
            .await
            .expect("read back")
            .expect("agent")
            .parsed_intent
    }

    // An unrelated edit leaves the cache alone.
    let rename: UpdateAgentRequest =
        serde_json::from_value(json!({ "name": "包装台质检二号" })).expect("valid update fixture");
    let _ = update_agent(State(state.clone()), Path(id.clone()), Json(rename))
        .await
        .expect("rename accepted");
    assert!(
        cached_intent(&state.agents.agent_store, &id)
            .await
            .is_some(),
        "an unrelated edit must not drop the cached intent"
    );

    // A prompt edit drops it; the next execution re-parses.
    let edit: UpdateAgentRequest =
        serde_json::from_value(json!({ "user_prompt": "每天早上汇总产量" })).expect("valid update");
    let _ = update_agent(State(state.clone()), Path(id.clone()), Json(edit))
        .await
        .expect("prompt edit accepted");
    assert!(
        cached_intent(&state.agents.agent_store, &id)
            .await
            .is_none(),
        "a prompt edit must drop the cached intent"
    );
}

/// `manual` is a first-class schedule type: manual-only, never auto-scheduled,
/// repeatable via invoke. The editor compiles both the on-demand strategy card
/// and the "替我查" hire mode to it, so create must accept it.
#[tokio::test]
async fn test_create_agent_accepts_manual_schedule() {
    let state = create_test_server_state().await;

    // Free needs no resource bindings, so the schedule is the only thing
    // under test — and this is exactly the "替我查" card's combination.
    let id = create_agent_with(
        &state,
        json!({
            "execution_mode": "free",
            "schedule": { "schedule_type": "manual" },
        }),
    )
    .await;

    assert_eq!(
        read_back(&state, &id).await["schedule"]["schedule_type"],
        "manual",
        "create must accept the on-demand schedule the editor sends"
    );
}

/// An output contract is no longer structured-only (M2-2), so every agent that
/// declares one has to appear in the dashboard's data-source picker — not just
/// the L0 ones. Publishing a field the picker refuses to list is a dead end:
/// the data lands in telemetry and nothing can bind to it.
#[tokio::test]
async fn test_reasoning_agent_output_fields_are_listed_as_data_sources() {
    let state = create_test_server_state().await;
    // `free`, not `structured`: the case that used to be filtered out.
    let id = create_agent_with(&state, json!({ "execution_mode": "free" })).await;

    let listed = list_all_data_sources_handler(
        State(state.clone()),
        Query(ListDataSourcesQuery {
            source_type: Some("ai".to_string()),
            source: None,
            search: None,
            offset: None,
            limit: None,
            skip_telemetry: Some(true),
        }),
    )
    .await
    .expect("data sources must be listable");

    let payload = listed.0.data.expect("handler returns a payload");
    let ids: Vec<&str> = payload.data.iter().map(|s| s.id.as_str()).collect();
    assert!(
        ids.contains(&format!("ai:{id}:missing_count").as_str()),
        "a reasoning agent's field must be bindable; got {ids:?}"
    );
}

/// The most basic agent there is: a prompt and nothing else. The derivation
/// calls it `focused`, and "focused" means "no commands, no output contract" —
/// so requiring a resource binding here rejects the form's plainest path.
#[tokio::test]
async fn test_create_agent_without_resources_is_allowed() {
    let state = create_test_server_state().await;

    let request: CreateAgentRequest = serde_json::from_value(json!({
        "name": "只看一眼",
        "user_prompt": "看看车间现在有没有异常",
        "schedule": { "schedule_type": "interval", "interval_seconds": 3600 },
        "execution_mode": "focused",
    }))
    .expect("valid create fixture");

    let _created = create_agent(State(state.clone()), Json(request))
        .await
        .expect("a prompt-only agent must be creatable");
}

/// Schedule-only PUT must not wipe bindings (Bug 3 / PR2 B1). Before the fix,
/// `has_schedule_update` was OR'd into the resources rewrite predicate, so a
/// PUT that only changed `schedule` cleared `agent.resources`.
#[tokio::test]
async fn test_schedule_only_update_preserves_resources() {
    let state = create_test_server_state().await;
    let id = create_agent_with(
        &state,
        json!({
            "execution_mode": "focused",
            "resources": [{
                "resource_id": "sensor-001",
                "resource_type": "device",
                "name": "Temp Sensor"
            }],
        }),
    )
    .await;

    let before = read_back(&state, &id).await;
    let before_resources = before["resources"]
        .as_array()
        .expect("created with resources")
        .clone();
    assert_eq!(before_resources.len(), 1);
    assert_eq!(before_resources[0]["resource_id"], "sensor-001");

    let schedule_only: UpdateAgentRequest = serde_json::from_value(json!({
        "schedule": { "schedule_type": "interval", "interval_seconds": 120 },
    }))
    .expect("schedule-only update body");
    let _ = update_agent(State(state.clone()), Path(id.clone()), Json(schedule_only))
        .await
        .expect("schedule-only update must succeed");

    let after = read_back(&state, &id).await;
    assert_eq!(
        after["schedule"]["interval_seconds"], 120,
        "schedule must update"
    );
    assert_eq!(
        after["resources"].as_array().expect("resources array"),
        &before_resources,
        "resources must survive a schedule-only PUT"
    );

    // Explicit empty array still clears bindings (editor clear path).
    let clear: UpdateAgentRequest = serde_json::from_value(json!({
        "resources": [],
    }))
    .expect("explicit clear body");
    let _ = update_agent(State(state.clone()), Path(id.clone()), Json(clear))
        .await
        .expect("explicit resources:[] must succeed");
    let cleared = read_back(&state, &id).await;
    assert_eq!(
        cleared["resources"].as_array().map(|a| a.len()),
        Some(0),
        "resources:[] must clear bindings"
    );
}

/// Data Explorer's detail view queries `?source=ai:{agent_id}&metric={field}` —
/// the fallback source parser only knew device/extension/transform, so
/// clicking into an AI-published series was a 400.
#[tokio::test]
async fn test_ai_series_detail_endpoint_accepts_ai_source() {
    let state = create_test_server_state().await;
    let id = create_agent_with(&state, json!({ "execution_mode": "structured" })).await;

    let _resp = neomind_api::handlers::data::query_telemetry_handler(
        State(state),
        Query(neomind_api::handlers::data::TelemetryQueryParams {
            source: Some(format!("ai:{id}")),
            metric: Some("missing_count".to_string()),
            start: None,
            end: None,
            limit: None,
            offset: None,
            aggregate: None,
            bucketed: None,
        }),
    )
    .await
    .expect("ai:{agent} must parse as a source, not 400");
}

/// The pointer an AI field carries — back to the run that produced it — has to
/// survive the read path, or the provenance feature is invisible end to end.
///
/// It did not: the storage point has had a `metadata` field all along, but the
/// device-side view type the API reads through had no such field, so anything
/// attached to a point was silently dropped one layer below the response.
/// Verified as far as storage before; this is the assertion that it reaches a
/// caller.
#[tokio::test]
async fn test_ai_series_read_back_carries_the_run_it_came_from() {
    let state = create_test_server_state().await;
    let id = create_agent_with(&state, json!({ "execution_mode": "structured" })).await;

    state
        .devices
        .telemetry
        .write(
            &format!("ai:{id}"),
            "missing_count",
            neomind_devices::telemetry::DataPoint {
                timestamp: chrono::Utc::now().timestamp(),
                value: neomind_devices::MetricValue::Integer(0),
                quality: Some(0.42),
                metadata: Some(json!({ "execution_id": "exec-boundary-1" })),
            },
        )
        .await
        .expect("seed a published field");
    // Writes are buffered; this handler reads redb, not the cache.
    state.devices.telemetry.flush().expect("flush the buffer");

    let response = neomind_api::handlers::data::query_telemetry_handler(
        State(state),
        Query(neomind_api::handlers::data::TelemetryQueryParams {
            source: Some(format!("ai:{id}")),
            metric: Some("missing_count".to_string()),
            start: None,
            end: None,
            limit: None,
            offset: None,
            aggregate: None,
            bucketed: None,
        }),
    )
    .await
    .expect("the series must be queryable");

    let data = response.0.data.expect("the envelope carries data");
    let point = &data["data"][0];

    assert_eq!(
        point["metadata"]["execution_id"], "exec-boundary-1",
        "a value read back has to say which run produced it: {point}"
    );
    // `quality` is an f32, so it arrives as 0.41999998688697815.
    let quality = point["quality"]
        .as_f64()
        .expect("the point carries a quality");
    assert!(
        (quality - 0.42).abs() < 1e-6,
        "and carry the confidence it was published with: {quality}"
    );
}

/// The detail view's output-fields section needs more than the reading: when it
/// was published, how sure the model was, and which run produced it. The DTO
/// kept only the value, so freshness and provenance were dropped at the last
/// step — the same shape of bug as the telemetry boundary, one layer up.
#[tokio::test]
async fn test_agent_list_carries_output_field_provenance() {
    let state = create_test_server_state().await;
    let id = create_agent_with(&state, json!({ "execution_mode": "structured" })).await;

    state
        .devices
        .telemetry
        .write(
            &format!("ai:{id}"),
            "missing_count",
            neomind_devices::telemetry::DataPoint {
                timestamp: 1_700_000_000,
                value: neomind_devices::MetricValue::Integer(2),
                quality: Some(0.42),
                metadata: Some(json!({ "execution_id": "exec-field-1" })),
            },
        )
        .await
        .expect("seed a published field");
    state.devices.telemetry.flush().expect("flush the buffer");

    let response = neomind_api::handlers::agents::list_agents(
        State(state),
        Query(serde_json::from_value(json!({})).expect("an empty query is the default view")),
    )
    .await
    .expect("list agents");

    let data = response.0.data.expect("the envelope carries data");
    let agent = data["agents"]
        .as_array()
        .expect("agents is a list")
        .iter()
        .find(|a| a["id"] == id.as_str())
        .expect("our agent is in the list");

    let field = &agent["latest_output_state"]["missing_count"];
    assert_eq!(field["value"], 2, "the reading: {field}");
    assert_eq!(field["at"], 1_700_000_000, "when it was published");
    assert_eq!(
        field["execution_id"], "exec-field-1",
        "and which run produced it, so the value can be checked"
    );
    let confidence = field["confidence"].as_f64().expect("a confidence");
    assert!((confidence - 0.42).abs() < 1e-6, "got {confidence}");
}

/// The CLI's create receipt tells the model the agent is already running, and
/// the skill says the same. That has to match what the API does.
///
/// It did not, once: the receipt said "created paused — it will NOT run until
/// activated", which was never true, because `create_agent` sets
/// `AgentStatus::Active`. Nothing failed, because nothing compared the two.
#[tokio::test]
async fn a_new_agent_is_created_active() {
    let state = create_test_server_state().await;
    let id = create_agent_with(&state, json!({})).await;

    let agent = read_back(&state, &id).await;
    assert_eq!(
        agent["status"], "Active",
        "the CLI receipt and the agent skill both tell the model this agent is \
         already running; a paused default would make them wrong"
    );
}

/// The cron form the CLI help and the rule validator both recommend must work.
///
/// They say `"0 8 * * *"` — five fields, seconds implied. The `cron` crate wants
/// six (seconds first), so following the documentation produced
/// `Invalid cron expression` and the agent was never created. Observed live: a
/// chat that built a daily-8am agent failed twice in a row, both times with an
/// expression copied straight out of `--help`.
#[tokio::test]
async fn a_five_field_cron_expression_is_accepted() {
    let state = create_test_server_state().await;

    for expression in ["0 8 * * *", "*/5 * * * *", "30 2 * * 1-5"] {
        let id = create_agent_with(
            &state,
            json!({
                "schedule": { "schedule_type": "cron", "cron_expression": expression },
            }),
        )
        .await;
        let agent = read_back(&state, &id).await;
        assert_eq!(
            agent["schedule"]["cron_expression"], expression,
            "the stored expression should be what the user wrote"
        );
    }
}

/// Read `export const <name> = '<value>'` out of the editor's shared constants
/// module. `None` when the file is absent (source tarball without `web/`) or
/// the constant is not declared.
fn editor_string_constant(name: &str) -> Option<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../web/src/pages/agents-components/agent-editor/constants.ts");
    let text = std::fs::read_to_string(path).ok()?;
    let marker = format!("export const {name} = '");
    let start = text.find(&marker)? + marker.len();
    let rest = &text[start..];
    Some(rest[..rest.find('\'')?].to_string())
}

/// Silence was the old default, and it is indistinguishable from a broken
/// agent: a scheduled run that reports only into the in-app Messages page looks
/// exactly like one that never ran. Every agent must come out of creation with
/// somewhere to report to.
///
/// Asserted against the editor's own constants rather than literals: the notify
/// card is initialled from those, so they are what the user reads while filling
/// the form. Literals here would let the two drift into showing one routing and
/// creating another.
#[tokio::test]
async fn a_new_agent_defaults_to_somewhere_it_can_report() {
    let state = create_test_server_state().await;
    let id = create_agent_with(&state, json!({})).await;

    let agent = read_back(&state, &id).await;
    let notify = agent
        .get("notify")
        .and_then(|n| n.as_object())
        .expect("a new agent carries a notify target");

    let (Some(channel), Some(on)) = (
        editor_string_constant("DEFAULT_NOTIFY_CHANNEL"),
        editor_string_constant("DEFAULT_NOTIFY_ON"),
    ) else {
        return; // no web/ tree to compare against
    };

    assert_eq!(
        notify.get("channels"),
        Some(&json!([channel])),
        "the built-in channel that reaches a human is the default target — and \
         the one the editor's notify card displays"
    );
    assert_eq!(
        notify.get("on"),
        Some(&json!(on)),
        "silence is still health — a report only when there is one"
    );
}

/// The default is a floor, not an override: an agent that names its own target
/// must keep it.
#[tokio::test]
async fn an_explicit_notify_target_is_not_overwritten() {
    let state = create_test_server_state().await;
    let id = create_agent_with(
        &state,
        json!({ "notify": { "channels": ["webhook:ops"], "on": "always" } }),
    )
    .await;

    let agent = read_back(&state, &id).await;
    assert_eq!(agent["notify"]["channels"], json!(["webhook:ops"]));
    assert_eq!(agent["notify"]["on"], json!("always"));
}

/// The event listener must IGNORE virtual metrics (`ai:<agent>:<field>` /
/// transform aliases): an agent listening on a wildcard — or on another
/// agent's output — used to be re-triggered by its own publish, forming an
/// unbounded self/mutual-trigger loop that burned an LLM call every
/// cooldown expiry. Real-device metrics still trigger.
///
/// Drives the REAL spawned listener (`init_agent_events`) against the REAL
/// event bus of the test server state.
#[tokio::test]
async fn virtual_metrics_do_not_retrigger_event_agents() {
    let state = create_test_server_state().await;
    state.init_agent_events().await;
    // The listener subscribes from a spawned task — give it a beat before
    // publishing, or the broadcast delivers to nobody.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // An event agent listening on ANY temp reading from the device.
    let id = create_agent_with(
        &state,
        json!({
            "schedule": {
                "schedule_type": "event",
                // The API DTO carries the filter as a JSON STRING (the
                // polymorphic parse layer lives in storage).
                "event_filter": "{\"any\":[{\"type\":\"device\",\"id\":\"dev-virt\",\"field\":\"temp\"}]}"
            },
            "execution_mode": "free"
        }),
    )
    .await;

    let bus = state.event_bus().expect("test state has an event bus");

    // 1) A VIRTUAL metric for the watched source must NOT run the agent.
    bus.publish(neomind_core::NeoMindEvent::DeviceMetric {
        device_id: "dev-virt".to_string(),
        metric: "temp".to_string(),
        value: neomind_core::MetricValue::Float(42.0),
        timestamp: chrono::Utc::now().timestamp(),
        quality: None,
        is_virtual: Some(true),
    })
    .await;

    // 2) A REAL metric must (the agent has no LLM configured, so the run
    // FAILS fast — but stats still count the execution attempt).
    bus.publish(neomind_core::NeoMindEvent::DeviceMetric {
        device_id: "dev-virt".to_string(),
        metric: "temp".to_string(),
        value: neomind_core::MetricValue::Float(41.0),
        timestamp: chrono::Utc::now().timestamp(),
        quality: None,
        is_virtual: None,
    })
    .await;

    // Poll for the real event to land (the listener is async); assert both
    // directions with one wait.
    let mut ran = 0u32;
    for _ in 0..100 {
        ran = read_back(&state, &id).await["stats"]["total_executions"]
            .as_u64()
            .unwrap_or(0) as u32;
        if ran >= 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(ran >= 1, "a real metric must trigger the event agent");
    // Give any would-be virtual trigger plenty of time to (wrongly) land,
    // then require exactly the real one.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let final_count = read_back(&state, &id).await["stats"]["total_executions"]
        .as_u64()
        .unwrap_or(0);
    assert_eq!(
        final_count, 1,
        "exactly ONE execution: the virtual metric must not re-trigger (got {final_count})"
    );
}
