//! Tests for the agent handlers — what the editor sends must be what gets stored.
//!
//! Regression guards for two hand-rolled string→enum mappings that had drifted
//! from the storage enums the rest of the system agrees on.

use axum::extract::{Path, State};
use axum::Json;
use neomind_api::handlers::agents::{
    create_agent, get_agent, update_agent, CreateAgentRequest, UpdateAgentRequest,
};
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
