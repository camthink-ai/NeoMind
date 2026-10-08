//! Integration tests for the explore subagent (W1) — the cross-component
//! flows that unit tests can't cover. The task card (W2) was removed after
//! live testing showed no user value; its tests went with it.

use neomind_api::handlers::ServerState;

async fn create_test_server_state() -> ServerState {
    crate::common::create_test_server_state().await
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
