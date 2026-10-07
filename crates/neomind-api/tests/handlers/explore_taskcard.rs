//! Integration tests for W1 (explore subagent) + W2 (task card) — the
//! cross-component flows that unit tests can't cover.

use axum::extract::{Path, State};
use serde_json::json;

use neomind_api::handlers::sessions::get_session_task_card_handler;
use neomind_api::handlers::ServerState;

async fn create_test_server_state() -> ServerState {
    crate::common::create_test_server_state().await
}

/// W2: the task-card endpoint reads what the tool wrote.
#[tokio::test]
async fn task_card_endpoint_round_trips() {
    let state = create_test_server_state().await;

    let session_id = state
        .agents
        .session_manager
        .create_session()
        .await
        .expect("session created");

    let store = state.agents.system_memory_store.clone();
    let card_md =
        "<!-- task-card -->\n```json\n{\"goal\":\"巡检冷库\",\"params\":\"厦门市集美区\"}\n```\n";
    store
        .write_session_file(&session_id, "todo", card_md)
        .await
        .unwrap();

    let response = get_session_task_card_handler(State(state.clone()), Path(session_id.clone()))
        .await
        .expect("handler must succeed");

    let body = response.0;
    let data = body.data.expect("response carries data");
    assert_eq!(data["goal"], json!("巡检冷库"));
    assert_eq!(data["params"], json!("厦门市集美区"));
    assert_eq!(data["isEmpty"], json!(false));
}

/// W2: the endpoint 404s for unknown sessions.
#[tokio::test]
async fn task_card_endpoint_404_for_unknown_session() {
    let state = create_test_server_state().await;
    let result =
        get_session_task_card_handler(State(state), Path("nonexistent-session-id".to_string()))
            .await;
    assert!(result.is_err(), "unknown session must 404");
}

/// W1: explore is registered in the production registry.
#[tokio::test]
async fn explore_registered_in_production_registry() {
    let state = create_test_server_state().await;
    state.init_tools().await;
    let registry = state
        .agents
        .session_manager
        .get_tool_registry()
        .await
        .expect("registry set");
    let names: Vec<String> = registry
        .definitions_for_llm()
        .iter()
        .map(|d| d.name.clone())
        .collect();
    assert!(
        names.contains(&"explore".to_string()),
        "explore must be in the registry"
    );
}

/// W2: update_task_card is registered in the production registry.
#[tokio::test]
async fn task_card_tool_registered_in_production_registry() {
    let state = create_test_server_state().await;
    state.init_tools().await;
    let registry = state
        .agents
        .session_manager
        .get_tool_registry()
        .await
        .expect("registry set");
    let names: Vec<String> = registry
        .definitions_for_llm()
        .iter()
        .map(|d| d.name.clone())
        .collect();
    assert!(
        names.contains(&"update_task_card".to_string()),
        "update_task_card must be in the registry"
    );
}

/// W1+W2 cross-check: both tools exist in the registry simultaneously.
#[tokio::test]
async fn both_tools_in_registry_simultaneously() {
    let state = create_test_server_state().await;
    state.init_tools().await;
    let registry = state
        .agents
        .session_manager
        .get_tool_registry()
        .await
        .expect("registry set");

    let names: Vec<String> = registry
        .definitions_for_llm()
        .iter()
        .map(|d| d.name.clone())
        .collect();
    assert!(names.contains(&"explore".to_string()));
    assert!(names.contains(&"update_task_card".to_string()));
}

/// W1+W2: context injection — the card's params appear in the context
/// block after session bind + disk load.
#[tokio::test]
async fn task_card_context_injection_populates() {
    let state = create_test_server_state().await;
    state.init_tools().await;
    let session_id = state
        .agents
        .session_manager
        .create_session()
        .await
        .expect("session");

    let store = state.agents.system_memory_store.clone();
    let card_md = "<!-- task-card -->\n```json\n{\"params\":\"温度阈值-18°C\"}\n```\n";
    store
        .write_session_file(&session_id, "todo", card_md)
        .await
        .unwrap();

    let registry = state
        .agents
        .session_manager
        .get_tool_registry()
        .await
        .expect("registry");

    registry
        .set_task_card_session(Some(session_id.clone()))
        .await;

    let block = registry.get_task_card_block();
    assert!(
        block.is_some(),
        "context block must be populated after session bind + disk load"
    );
    let block = block.unwrap();
    assert!(
        block.contains("-18°C"),
        "card params must appear in the context block"
    );
    assert!(
        block.contains("<task-card>"),
        "must be the structured injection form"
    );
}
