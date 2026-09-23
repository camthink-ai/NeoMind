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
