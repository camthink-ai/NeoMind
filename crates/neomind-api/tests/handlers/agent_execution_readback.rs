//! Scenario: read an agent's existing conclusion through the CLI read path.
//!
//! Reading an agent's latest conclusion goes
//! `neomind agent latest-execution <ID>` → `GET /agents/v2/{id}/latest-execution`
//! (list path `GET /agents/{id}/executions` also reads V2 formal rows).

use axum::extract::{Path, State};
use axum::Json;
use neomind_ai_agent::model::{
    AgentSpecDraft, InputContract, OutputContract, ToolPolicy, TriggerSpec,
};
use neomind_api::handlers::agents::get_agent_executions;
use neomind_api::handlers::agents_v2::create_agent_v2;
use neomind_api::handlers::ServerState;
use neomind_storage::agents_v2::{AgentExecutionRecordV2, ExecutionStatusV2};
use neomind_storage::AgentStoreV2;

async fn seed_executed_agent(state: &ServerState) -> String {
    let draft = AgentSpecDraft {
        name: "coldroom-watch".into(),
        description: None,
        user_prompt: "检查冷库温度并总结".into(),
        llm_backend_id: None,
        preset: None,
        trigger: TriggerSpec::Manual,
        pacing: Default::default(),
        input: InputContract {
            requirements: vec![],
        },
        tool_policy: ToolPolicy::Unrestricted,
        output: OutputContract::default(),
        notify: Default::default(),
        controlled_actions: vec![],
    };
    let created = create_agent_v2(State(state.clone()), Json(draft))
        .await
        .expect("create agent")
        .0
        .data
        .expect("create returns data");
    let agent_id = created.agent.id;

    let now_ms = chrono::Utc::now().timestamp_millis();
    let execution = AgentExecutionRecordV2 {
        id: "exec-readback-1".into(),
        agent_id: agent_id.clone(),
        config_version: created.agent.config_version,
        status: ExecutionStatusV2::Completed,
        started_at: now_ms - 1500,
        finished_at: now_ms,
        is_probe: false,
        dry_run: false,
        stale_config: None,
        evidence: serde_json::json!({}),
        deliveries: vec![],
        brief_text: Some("冷库温度 4.2°C,处于正常范围".into()),
        conclusion: Some("冷库温度正常,无需处理".into()),
        structured_result: None,
        error: None,
        alert_verdict: None,
    };
    let v2 = AgentStoreV2::from_arc(state.agents.agent_store.db()).expect("open AgentStoreV2");
    v2.save_execution(&execution).expect("seed V2 execution");
    agent_id
}

#[tokio::test]
async fn latest_execution_conclusion_is_readable_via_cli_read_path() {
    let state = crate::common::create_test_server_state().await;
    let agent_id = seed_executed_agent(&state).await;

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
        "conclusion must be readable on the list path; got {conclusions:?} from {execs:?}"
    );
}
