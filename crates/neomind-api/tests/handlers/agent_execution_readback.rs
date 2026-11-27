//! Scenario: read an agent's existing conclusion through the CLI read path.
//!
//! Reading an agent's latest conclusion goes
//! `neomind agent latest-execution <ID>` → `GET /agents/v2/{id}/latest-execution`
//! (list path `GET /agents/{id}/executions` also reads V2 formal rows).
//!
//! NM-407: list / single / batch detail must agree on trigger_type + input_summary
//! for V2-only history (Monitor card batch previously hit the legacy store only).

use axum::extract::{Path, State};
use axum::Json;
use neomind_ai_agent::model::{
    AgentSpecDraft, InputContract, OutputContract, ToolPolicy, TriggerSpec,
};
use neomind_api::handlers::agents::{
    batch_get_executions, create_agent, get_agent_executions, get_execution, BatchExecutionIds,
};
use neomind_api::handlers::ServerState;
use neomind_storage::agent_store::{AgentExecutionRow, AgentExecutionStatus};
use neomind_storage::AgentRecordStore;
use serde_json::json;

fn vision_evidence() -> serde_json::Value {
    json!({
        "items": [{
            "key": "frame",
            "source_id": "device:cam-1:image_data",
            "result_type": "vision",
            "fetched_at": 1_700_000_000_000i64,
            "quality": "fresh",
            "binding_proof": "test",
            "detail": {
                "kind": "vision",
                "image_source_field": "image_data",
                "vision_path": "inline_multimodal"
            }
        }],
        "missing_keys": []
    })
}

async fn seed_executed_agent(state: &ServerState) -> (String, String) {
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
    let created = create_agent(State(state.clone()), Json(draft))
        .await
        .expect("create agent")
        .0
        .data
        .expect("create returns data");
    let agent_id = created.agent.id;

    let now_ms = chrono::Utc::now().timestamp_millis();
    let exec_id = "exec-readback-1".to_string();
    let execution = AgentExecutionRow {
        id: exec_id.clone(),
        agent_id: agent_id.clone(),
        config_version: created.agent.config_version,
        status: AgentExecutionStatus::Completed,
        started_at: now_ms - 1500,
        finished_at: now_ms,
        is_probe: false,
        dry_run: false,
        trigger_source: Some("manual".into()),
        stale_config: None,
        evidence: vision_evidence(),
        deliveries: vec![],
        tool_calls: vec![],
        brief_text: Some("冷库温度 4.2°C,处于正常范围".into()),
        conclusion: Some("冷库温度正常,无需处理".into()),
        structured_result: None,
        error: None,
        alert_verdict: None,
    };
    let record_store =
        AgentRecordStore::from_arc(state.agents.agent_store.db()).expect("open AgentRecordStore");
    record_store
        .save_execution(&execution)
        .expect("seed execution");
    (agent_id, exec_id)
}

#[tokio::test]
async fn latest_execution_conclusion_is_readable_via_cli_read_path() {
    let state = crate::common::create_test_server_state().await;
    let (agent_id, _) = seed_executed_agent(&state).await;

    let resp = get_agent_executions(
        State(state),
        Path(agent_id),
        axum::extract::Query(Default::default()),
    )
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

#[tokio::test]
async fn v2_list_card_modal_agree_on_trigger_and_input_summary() {
    let state = crate::common::create_test_server_state().await;
    let (agent_id, exec_id) = seed_executed_agent(&state).await;

    // List
    let list_resp = get_agent_executions(
        State(state.clone()),
        Path(agent_id.clone()),
        axum::extract::Query(Default::default()),
    )
        .await
        .expect("list");
    let list_body = list_resp.0.data.expect("list data");
    let list_row = list_body["executions"]
        .as_array()
        .and_then(|a| a.first())
        .cloned()
        .expect("one execution");
    assert_eq!(list_row["trigger_type"], "manual");
    assert_eq!(
        list_row["input_summary"]["vision_path"],
        "inline_multimodal"
    );
    assert_eq!(list_row["input_summary"]["image_extracted"], true);
    assert_eq!(
        list_row["input_summary"]["image_source_field"],
        "image_data"
    );

    // Modal / single detail
    let detail_resp = get_execution(
        State(state.clone()),
        Path((agent_id.clone(), exec_id.clone())),
    )
    .await
    .expect("detail");
    let detail = detail_resp.0.data.expect("detail data");
    assert_eq!(detail["trigger_type"], list_row["trigger_type"]);
    assert_eq!(detail["input_summary"], list_row["input_summary"]);
    assert!(
        detail["decision_process"]["data_collected"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false),
        "detail must project evidence into data_collected: {detail}"
    );
    assert!(
        detail["decision_process"]["conclusion"]
            .as_str()
            .unwrap_or("")
            .contains("冷库温度正常"),
        "detail conclusion missing: {detail}"
    );

    // Card batch detail (previously legacy-only → empty for V2-only history)
    let batch_resp = batch_get_executions(
        State(state),
        Path(agent_id),
        Json(BatchExecutionIds {
            ids: vec![exec_id.clone()],
        }),
    )
    .await
    .expect("batch");
    let batch = batch_resp.0.data.expect("batch data");
    let batch_row = batch["details"][&exec_id]
        .as_object()
        .cloned()
        .expect("batch detail for seeded id");
    assert_eq!(batch_row["trigger_type"], list_row["trigger_type"]);
    assert_eq!(batch_row["input_summary"], list_row["input_summary"]);
    assert_eq!(
        batch_row["decision_process"]["conclusion"],
        detail["decision_process"]["conclusion"]
    );
    assert_eq!(
        batch_row["decision_process"]["data_collected"],
        detail["decision_process"]["data_collected"]
    );
}

#[tokio::test]
async fn v2_list_falls_back_to_agent_trigger_when_source_missing() {
    let state = crate::common::create_test_server_state().await;
    let draft = AgentSpecDraft {
        name: "event-cam".into(),
        description: None,
        user_prompt: "watch".into(),
        llm_backend_id: None,
        preset: None,
        trigger: TriggerSpec::Event {
            filter: json!({"device_id": "cam-1"}),
        },
        pacing: Default::default(),
        input: InputContract {
            requirements: vec![],
        },
        tool_policy: ToolPolicy::Unrestricted,
        output: OutputContract::default(),
        notify: Default::default(),
        controlled_actions: vec![],
    };
    let created = create_agent(State(state.clone()), Json(draft))
        .await
        .expect("create")
        .0
        .data
        .expect("data");
    let agent_id = created.agent.id;
    let now_ms = chrono::Utc::now().timestamp_millis();
    let execution = AgentExecutionRow {
        id: "exec-legacy-source".into(),
        agent_id: agent_id.clone(),
        config_version: created.agent.config_version,
        status: AgentExecutionStatus::Completed,
        started_at: now_ms - 100,
        finished_at: now_ms,
        is_probe: false,
        dry_run: false,
        trigger_source: None, // pre-provenance row
        stale_config: None,
        evidence: json!({"items": [], "missing_keys": []}),
        deliveries: vec![],
        tool_calls: vec![],
        brief_text: Some("ok".into()),
        conclusion: Some("ok".into()),
        structured_result: None,
        error: None,
        alert_verdict: None,
    };
    let record_store =
        AgentRecordStore::from_arc(state.agents.agent_store.db()).expect("record store");
    record_store.save_execution(&execution).expect("seed");

    let resp = get_agent_executions(
        State(state),
        Path(agent_id),
        axum::extract::Query(Default::default()),
    )
        .await
        .expect("list");
    let body = resp.0.data.expect("data");
    let row = body["executions"]
        .as_array()
        .and_then(|a| a.first())
        .expect("row");
    assert_eq!(
        row["trigger_type"], "event",
        "missing trigger_source should fall back to agent TriggerSpec::Event"
    );
}

#[tokio::test]
async fn v2_user_messages_and_memory_endpoints_work_under_schema_v2() {
    use neomind_api::handlers::agents::{
        add_user_message, clear_user_messages, delete_user_message, get_agent_memory,
        get_user_messages,
    };

    let state = crate::common::create_test_server_state().await;
    let draft = AgentSpecDraft {
        name: "notes-agent".into(),
        description: None,
        user_prompt: "watch".into(),
        llm_backend_id: Some("local-brief".into()),
        preset: None,
        trigger: TriggerSpec::Manual,
        pacing: Default::default(),
        input: InputContract {
            requirements: vec![],
        },
        tool_policy: ToolPolicy::Disabled,
        output: OutputContract::default(),
        notify: Default::default(),
        controlled_actions: vec![],
    };
    let created = create_agent(State(state.clone()), Json(draft))
        .await
        .expect("create")
        .0
        .data
        .expect("data");
    let agent_id = created.agent.id;

    // Add two user messages via HTTP handler (was failing under schema v2 before).
    let added1 = add_user_message(
        State(state.clone()),
        Path(agent_id.clone()),
        Json(
            serde_json::from_value(
                json!({ "content": "Focus on sensor A", "message_type": "instruction" }),
            )
            .unwrap(),
        ),
    )
    .await
    .expect("add msg 1")
    .0
    .data
    .expect("data");
    let msg1_id = added1["id"].as_str().expect("id").to_string();

    let _added2 = add_user_message(
        State(state.clone()),
        Path(agent_id.clone()),
        Json(serde_json::from_value(json!({ "content": "Ignore transient spikes" })).unwrap()),
    )
    .await
    .expect("add msg 2");

    let list = get_user_messages(
        State(state.clone()),
        Path(agent_id.clone()),
        axum::extract::Query(Default::default()),
    )
        .await
        .expect("list msgs")
        .0
        .data
        .expect("data");
    assert_eq!(list.as_array().unwrap().len(), 2);

    // Invoke the agent and verify V2 memory journal records the execution.
    let rt = state.require_clean_slate().await.expect("rt");
    let _ = rt.invoke(&agent_id).await.expect("invoke");
    let mem = get_agent_memory(State(state.clone()), Path(agent_id.clone()))
        .await
        .expect("memory")
        .0
        .data
        .expect("data");
    assert_eq!(
        mem["journal"]["records"].as_array().unwrap().len(),
        1,
        "V2 execution must append to agent memory journal"
    );

    let _ = delete_user_message(State(state.clone()), Path((agent_id.clone(), msg1_id)))
        .await
        .expect("delete one");
    let cleared = clear_user_messages(State(state.clone()), Path(agent_id.clone()))
        .await
        .expect("clear remaining")
        .0
        .data
        .expect("data");
    assert_eq!(cleared["count"], 1);
}
