//! Scenario: read an agent's existing conclusion through the CLI read path.
//!
//! The dedicated chat `query_conclusion` tool was removed from model
//! registration (2026-11-17); reading an agent's latest conclusion now goes
//! `neomind agent latest-execution <ID>` → `GET /agents/{id}/executions?limit=1`.
//! This pins that the record — conclusion, status, duration — is readable
//! back after a run recorded it, i.e. the business capability the tool
//! wrapped survives in the unified CLI entry.

use axum::extract::{Path, State};
use axum::Json;
use neomind_api::handlers::agents::{create_agent, get_agent_executions, CreateAgentRequest};
use neomind_api::handlers::ServerState;
use neomind_storage::{AgentExecutionRecord, DecisionProcess, ExecutionStatus};
use serde_json::{json, Value};

async fn seed_executed_agent(state: &ServerState) -> String {
    let request: CreateAgentRequest = serde_json::from_value(json!({
        "name": "coldroom-watch",
        "user_prompt": "检查冷库温度并总结",
        "schedule": { "schedule_type": "manual" },
    }))
    .expect("valid create fixture");
    let created = create_agent(State(state.clone()), Json(request))
        .await
        .expect("create agent")
        .0
        .data
        .expect("create returns data");
    let agent_id = created["id"].as_str().expect("id").to_string();

    let execution = AgentExecutionRecord {
        id: "exec-readback-1".into(),
        agent_id: agent_id.clone(),
        timestamp: chrono::Utc::now().timestamp(),
        trigger_type: "manual".into(),
        status: ExecutionStatus::Completed,
        decision_process: DecisionProcess {
            situation_analysis: "冷库温度 4.2°C,处于正常范围".into(),
            data_collected: vec![],
            reasoning_steps: vec![],
            decisions: vec![],
            conclusion: "冷库温度正常,无需处理".into(),
            confidence: Some(0.9),
            stop_reason: String::new(),
            tokens_used: 0,
        },
        result: None,
        duration_ms: 1500,
        error: None,
        input_summary: None,
    };
    state
        .agents
        .agent_store
        .save_execution(&execution)
        .await
        .expect("seed execution");
    agent_id
}

#[tokio::test]
async fn latest_execution_conclusion_is_readable_via_cli_read_path() {
    let state = crate::common::create_test_server_state().await;
    let agent_id = seed_executed_agent(&state).await;

    // The exact read `neomind agent latest-execution <ID>` performs.
    let resp = get_agent_executions(State(state), Path(agent_id))
        .await
        .expect("executions readable");
    let body = resp.0.data.expect("success payload carries data");
    let execs = body["executions"]
        .as_array()
        .cloned()
        .or_else(|| body.as_array().cloned())
        .expect("executions list");
    let conclusions: Vec<&str> = execs
        .iter()
        .filter_map(|e| e["conclusion"].as_str())
        .collect();
    assert!(
        conclusions.iter().any(|c| c.contains("冷库温度正常")),
        "conclusion must be readable through the CLI read path, got: {body}"
    );
    // The fields the chat model needs to answer a state question honestly.
    let latest = &execs[0];
    assert!(latest.get("status").is_some());
    assert!(latest.get("duration_ms").is_some() || latest.get("durationMs").is_some());
}

#[tokio::test]
async fn unknown_agent_is_an_honest_404_not_empty_success() {
    let state = crate::common::create_test_server_state().await;
    let err = get_agent_executions(State(state), Path("no-such-agent".into()))
        .await
        .expect_err("must fail");
    assert!(
        err.message.to_lowercase().contains("not found"),
        "expected honest not-found, got: {}",
        err.message
    );
}

// Keep Value import busy if assertions change shape.
#[allow(dead_code)]
fn _type_anchor(_v: Value) {}
