//! Deterministic agent-loop behavior tests, driven by `MockLlmRuntime`.
//!
//! Validates the design-hardening changes end-to-end (without a real LLM):
//!   - natural completion returns the LLM's final text
//!   - `AllDuplicate` breaks to Phase 2 instead of burning rounds (#5)
//!   - hitting the round budget runs the Phase 2 graceful-exit summary (#3)
//!
//! Gated by `test-utils` so it never compiles into a release binary. Lives
//! inside the crate (not `tests/`) so it can call the `pub(crate)` loop +
//! `filter_tools`. Run with: `cargo test -p neomind-agent --features test-utils
//! behavior_tests`.

#![cfg(all(test, feature = "test-utils"))]

use std::sync::Arc;

use async_trait::async_trait;

use neomind_core::llm::backend::LlmRuntime;
use neomind_core::message::{Message, MessageRole};
use neomind_storage::{
    AgentMemory, AgentSchedule, AgentStats, AgentStatus, AgentStore, AiAgent, ExecutionJournal,
    ExecutionMode, ScheduleType,
};

use crate::ai_agent::executor::{AgentExecutor, AgentExecutorConfig, StopReason};
use crate::testing_helpers::mock_llm::{MockLlmRuntime, MockResponse};
use crate::toolkit::error::ToolError;
use crate::toolkit::registry::ToolRegistry;
use crate::toolkit::tool::{Tool, ToolOutput};

/// A tool that always succeeds with a fixed echo — lets the loop execute calls
/// deterministically with no real side effect.
struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }
    fn description(&self) -> &str {
        "echo a message back"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "msg": { "type": "string" } },
            "required": ["msg"]
        })
    }
    async fn execute(&self, args: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let msg = args.get("msg").and_then(|v| v.as_str()).unwrap_or("");
        Ok(ToolOutput::success(format!("echo: {}", msg)))
    }
}

async fn build_harness() -> (AgentExecutor, AiAgent, Arc<ToolRegistry>) {
    let store = AgentStore::memory().expect("memory store");
    let config = AgentExecutorConfig {
        store,
        time_series_storage: None,
        device_service: None,
        event_bus: None,
        message_manager: None,
        llm_runtime: None,
        llm_backend_store: None,
        extension_registry: None,
        tool_registry: None,
        memory_store: None,
        backend_semaphores: None,
        skill_registry: None,
        execution_semaphore: None,
    };
    let executor = AgentExecutor::new(config).await.expect("executor");

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool));
    let registry = Arc::new(registry);

    let now: i64 = 0;
    let agent = AiAgent {
        id: "test-agent".into(),
        name: "Test".into(),
        description: None,
        user_prompt: "test".into(),
        llm_backend_id: None,
        parsed_intent: None,
        resources: vec![],
        schedule: AgentSchedule {
            schedule_type: ScheduleType::Interval,
            interval_seconds: Some(60),
            cron_expression: None,
            timezone: None,
            event_filter: None,
        },
        status: AgentStatus::Active,
        priority: 128,
        created_at: now,
        updated_at: now,
        last_execution_at: None,
        stats: AgentStats {
            total_executions: 0,
            successful_executions: 0,
            failed_executions: 0,
            avg_duration_ms: 0,
            last_duration_ms: Some(0),
        },
        memory: AgentMemory {
            journal: ExecutionJournal::default(),
            knowledge_files: vec![],
            updated_at: now,
        },
        conversation_history: vec![],
        user_messages: vec![],
        conversation_summary: None,
        context_window_size: 5,
        tool_config: None,
        execution_mode: ExecutionMode::Free,
        error_message: None,
        system_prompt: None,
        max_retries: 0,
        consecutive_failures: 0,
        enable_tool_chaining: false,
        max_chain_depth: 3,
    };

    (executor, agent, registry)
}

fn base_messages() -> Vec<Message> {
    vec![
        Message::new(MessageRole::System, "You are a test agent."),
        Message::new(MessageRole::User, "do the thing"),
    ]
}

#[tokio::test]
async fn update_memory_records_stop_reason() {
    // The journal must record WHY the run ended so the agent's next execution
    // can learn from it (e.g. "last time I hit max-rounds / got stuck").
    let (executor, agent, _registry) = build_harness().await;
    let mem = executor
        .update_memory(
            &agent,
            &[],
            "ran out of budget",
            "exec-sr",
            true,
            "max-rounds",
        )
        .await
        .expect("update_memory");
    let last = mem
        .journal
        .records
        .last()
        .expect("a journal record was pushed");
    assert_eq!(last.stop_reason, "max-rounds");
}

#[tokio::test]
async fn normal_completion_returns_final_text() {
    // Round 1: tool call. Round 2: final text (no tools) → natural completion.
    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call("echo", serde_json::json!({ "msg": "hi" })),
        MockResponse::text("all done"),
    ]);
    let rt_dyn: Arc<dyn LlmRuntime + Send + Sync> = Arc::new(rt.clone());
    let (executor, agent, registry) = build_harness().await;
    let (filtered_tools, tool_name_map) =
        AgentExecutor::filter_tools(&registry, &agent.tool_config);
    let mut messages = base_messages();
    let out = executor
        .run_tool_loop(
            &agent,
            &registry,
            &rt_dyn,
            &filtered_tools,
            &mut messages,
            "exec-normal",
            30,
            &tool_name_map,
            None,
        )
        .await;

    assert_eq!(out.final_text, "all done");
    assert_eq!(rt.call_count(), 2);
    assert_eq!(out.stop_reason, StopReason::NaturalCompletion);
}

#[tokio::test]
async fn all_duplicate_breaks_to_phase2() {
    // Round 1: tool call (sig X). Round 2: the SAME tool call → cross-round
    // dedup filters it → AllDuplicate. With results already in hand, the loop
    // must break to Phase 2 (#5), NOT burn 30 rounds nudging the model.
    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call("echo", serde_json::json!({ "msg": "x" })),
        MockResponse::tool_call("echo", serde_json::json!({ "msg": "x" })), // duplicate
        MockResponse::text("summary from results so far"),
    ]);
    let rt_dyn: Arc<dyn LlmRuntime + Send + Sync> = Arc::new(rt.clone());
    let (executor, agent, registry) = build_harness().await;
    let (filtered_tools, tool_name_map) =
        AgentExecutor::filter_tools(&registry, &agent.tool_config);
    let mut messages = base_messages();
    let out = executor
        .run_tool_loop(
            &agent,
            &registry,
            &rt_dyn,
            &filtered_tools,
            &mut messages,
            "exec-dup",
            30,
            &tool_name_map,
            None,
        )
        .await;

    assert!(
        rt.call_count() <= 3,
        "AllDuplicate should break early (not burn 30 rounds); got {} calls",
        rt.call_count()
    );
    assert!(
        !out.final_text.is_empty(),
        "Phase 2 summary should produce final text, got: {:?}",
        out.final_text
    );
    assert_eq!(out.stop_reason, StopReason::AllDuplicate);
}

#[tokio::test]
async fn max_rounds_graceful_exit_runs_phase2() {
    // Distinct tool calls each round (different sig → not deduped) → the loop
    // runs to max_rounds=2, then the Phase 2 graceful-exit summary synthesizes
    // a final answer from the accumulated results (#3 — no max_rounds+=10 hack).
    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call("echo", serde_json::json!({ "msg": "r1" })),
        MockResponse::tool_call("echo", serde_json::json!({ "msg": "r2" })),
        MockResponse::text("synthesized conclusion"),
    ]);
    let rt_dyn: Arc<dyn LlmRuntime + Send + Sync> = Arc::new(rt.clone());
    let (executor, agent, registry) = build_harness().await;
    let (filtered_tools, tool_name_map) =
        AgentExecutor::filter_tools(&registry, &agent.tool_config);
    let mut messages = base_messages();
    let out = executor
        .run_tool_loop(
            &agent,
            &registry,
            &rt_dyn,
            &filtered_tools,
            &mut messages,
            "exec-max",
            2,
            &tool_name_map,
            None,
        )
        .await;

    assert_eq!(
        rt.call_count(),
        3,
        "2 loop rounds + 1 Phase 2 summary call; got {}",
        rt.call_count()
    );
    assert_eq!(out.final_text, "synthesized conclusion");
    assert_eq!(out.stop_reason, StopReason::MaxRounds);
}
