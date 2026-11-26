//! Agent handler integration tests.

use axum::extract::{Path, Query, State};
use axum::Json;
use neomind_ai_agent::model::{AgentSpecPatch, InputContract, TriggerSpec};
use neomind_api::handlers::agents::{create_agent, get_agent, list_agents, update_agent};
use neomind_api::handlers::data::{list_all_data_sources_handler, ListDataSourcesQuery};
use neomind_api::handlers::ServerState;
use serde_json::json;

async fn create_test_server_state() -> ServerState {
    crate::common::create_test_server_state().await
}

fn base_draft(overrides: serde_json::Value) -> neomind_ai_agent::model::AgentSpecDraft {
    let mut draft = json!({
        "name": "包装台质检",
        "user_prompt": "看一下包装台画面有没有错装漏装",
        "trigger": { "kind": "interval", "every_secs": 3600 },
        "input": { "requirements": [] },
        "tool_policy": "unrestricted",
        "output": {
            "fields": [{ "name": "missing_count", "depends_on": [] }],
            "publish": true
        },
        "notify": {
            "channels": ["IM"],
            "on_complete": false,
            "on_alert": false,
            "on_failure": true,
            "pending_confirmation": false
        }
    });
    if let Some(obj) = overrides.as_object() {
        for (k, v) in obj {
            draft[k] = v.clone();
        }
    }
    serde_json::from_value(draft).expect("valid AgentSpecDraft fixture")
}

async fn create_agent_with(state: &ServerState, overrides: serde_json::Value) -> String {
    let draft = base_draft(overrides);
    let resp = create_agent(State(state.clone()), Json(draft))
        .await
        .expect("create_agent must accept the draft")
        .0
        .data
        .expect("create returns data");
    resp.agent.id
}

async fn read_back(state: &ServerState, id: &str) -> serde_json::Value {
    get_agent(State(state.clone()), Path(id.to_string()))
        .await
        .expect("created agent must be readable")
        .0
        .data
        .expect("get_agent returns a data payload")
}

#[tokio::test]
async fn list_and_get_return_agent_contract_dtos() {
    let state = create_test_server_state().await;
    let id = create_agent_with(&state, json!({})).await;

    let listed = list_agents(
        State(state.clone()),
        Query(serde_json::from_value(json!({})).unwrap()),
    )
    .await
    .expect("list")
    .0
    .data
    .expect("payload");
    assert_eq!(listed["schema"], "agent-contract/v2");
    let agent = listed["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == id)
        .expect("agent in list");
    assert_eq!(agent["schema"], "agent-contract/v2");
    assert!(agent.get("trigger").is_some(), "{agent}");
    assert!(agent.get("execution_count").is_some(), "{agent}");
    assert_eq!(agent["execution_mode"], "structured");
    assert_eq!(agent["output_fields"], json!(["missing_count"]));

    let got = read_back(&state, &id).await;
    assert_eq!(got["schema"], "agent-contract/v2");
    assert_eq!(got["id"], id);
    assert!(got.get("effective_plan").is_some());
    assert_eq!(got["user_prompt"], "看一下包装台画面有没有错装漏装");
}

#[tokio::test]
async fn update_via_v2_patch_preserves_input_when_trigger_only() {
    let state = create_test_server_state().await;
    let id = create_agent_with(
        &state,
        json!({
            "input": {
                "requirements": [{
                    "key": "sensor",
                    "source": { "kind": "whole_device", "device_id": "sensor-001" },
                    "params": {},
                    "timeout_secs": 10,
                    "result_type": "structured_json",
                    "freshness": { "kind": "any" },
                    "on_missing": "report_stale"
                }]
            }
        }),
    )
    .await;

    let before = read_back(&state, &id).await;
    let before_reqs = before["input"]["requirements"].clone();
    assert_eq!(before_reqs.as_array().unwrap().len(), 1);

    let patch = AgentSpecPatch {
        trigger: Some(TriggerSpec::Interval { every_secs: 120 }),
        ..Default::default()
    };
    let _ = update_agent(State(state.clone()), Path(id.clone()), Json(patch))
        .await
        .expect("trigger-only patch");

    let after = read_back(&state, &id).await;
    assert_eq!(after["trigger"]["kind"], "interval");
    assert_eq!(after["trigger"]["every_secs"], 120);
    assert_eq!(
        after["input"]["requirements"], before_reqs,
        "input must survive a trigger-only patch"
    );

    let clear = AgentSpecPatch {
        input: Some(InputContract {
            requirements: vec![],
        }),
        ..Default::default()
    };
    let _ = update_agent(State(state.clone()), Path(id.clone()), Json(clear))
        .await
        .expect("explicit empty input");
    let cleared = read_back(&state, &id).await;
    assert_eq!(
        cleared["input"]["requirements"].as_array().map(|a| a.len()),
        Some(0)
    );
}

#[tokio::test]
async fn create_accepts_manual_trigger_and_prompt_only() {
    let state = create_test_server_state().await;
    let id = create_agent_with(
        &state,
        json!({
            "trigger": { "kind": "manual" },
            "tool_policy": "disabled",
            "output": { "fields": [], "publish": false }
        }),
    )
    .await;
    let got = read_back(&state, &id).await;
    assert_eq!(got["trigger"]["kind"], "manual");
    assert_eq!(got["execution_mode"], "focused");
}

#[tokio::test]
async fn reasoning_agent_output_fields_are_listed_as_data_sources() {
    let state = create_test_server_state().await;
    let id = create_agent_with(
        &state,
        json!({
            "tool_policy": "unrestricted",
            "output": {
                "fields": [{ "name": "missing_count", "depends_on": [] }],
                "publish": true
            }
        }),
    )
    .await;

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
        "a publishing agent's field must be bindable; got {ids:?}"
    );
}

#[tokio::test]
async fn ai_series_detail_endpoint_accepts_ai_source() {
    let state = create_test_server_state().await;
    let id = create_agent_with(&state, json!({})).await;

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

#[tokio::test]
async fn ai_series_read_back_carries_the_run_it_came_from() {
    let state = create_test_server_state().await;
    let id = create_agent_with(&state, json!({})).await;

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
    assert_eq!(point["metadata"]["execution_id"], "exec-boundary-1");
    let quality = point["quality"].as_f64().expect("quality");
    assert!((quality - 0.42).abs() < 1e-6);
}

#[tokio::test]
async fn a_new_agent_is_created_active() {
    let state = create_test_server_state().await;
    let id = create_agent_with(&state, json!({})).await;
    let agent = read_back(&state, &id).await;
    assert_eq!(agent["status"], "Active");
}

#[tokio::test]
async fn a_five_field_cron_expression_is_accepted() {
    let state = create_test_server_state().await;
    for expression in ["0 8 * * *", "*/5 * * * *", "30 2 * * 1-5"] {
        let id = create_agent_with(
            &state,
            json!({
                "trigger": { "kind": "cron", "expr": expression, "timezone": "UTC" }
            }),
        )
        .await;
        let agent = read_back(&state, &id).await;
        assert_eq!(agent["trigger"]["expr"], expression);
    }
}

#[tokio::test]
async fn prompt_patch_updates_user_prompt() {
    let state = create_test_server_state().await;
    let id = create_agent_with(&state, json!({})).await;
    let patch = AgentSpecPatch {
        user_prompt: Some("只在工作日告警".into()),
        ..Default::default()
    };
    let _ = update_agent(State(state.clone()), Path(id.clone()), Json(patch))
        .await
        .expect("prompt patch");
    assert_eq!(
        read_back(&state, &id).await["user_prompt"],
        "只在工作日告警"
    );
}

#[tokio::test]
async fn explicit_notify_channels_are_preserved() {
    let state = create_test_server_state().await;
    let id = create_agent_with(
        &state,
        json!({
            "notify": {
                "channels": ["webhook:ops"],
                "on_complete": true,
                "on_alert": false,
                "on_failure": false,
                "pending_confirmation": false
            }
        }),
    )
    .await;
    let agent = read_back(&state, &id).await;
    assert_eq!(agent["notify"]["channels"], json!(["webhook:ops"]));
    assert_eq!(agent["notify"]["on_complete"], true);
}
