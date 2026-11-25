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
    build_harness_with_memory(None).await
}

/// [`build_harness`] with a knowledge-file store, so a test can exercise
/// what gets inlined from `memory/agents/<id>/custom/` — and what gets
/// written back.
async fn build_harness_with_memory(
    memory_store: Option<Arc<neomind_storage::MarkdownMemoryStore>>,
) -> (AgentExecutor, AiAgent, Arc<ToolRegistry>) {
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
        memory_store,
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
        // Unique per harness: the in-flight re-entrancy guard and the daily
        // run counters are PROCESS-global, so parallel tests sharing a
        // literal "test-agent" id would reject each other's runs.
        id: format!("test-agent-{}", uuid::Uuid::new_v4().simple()),
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
        output_schema: None,
        operator_config: None,
        memory_mode: None,
        notify: None,
        enable_tool_chaining: false,
        max_chain_depth: 3,
        max_tokens_per_run: None,
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
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt.clone());
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
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt.clone());
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
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt.clone());
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

#[tokio::test]
async fn structured_mode_publishes_schema_fields_on_first_pass() {
    let (mut executor, mut agent, _registry) = build_harness().await;
    agent.execution_mode = neomind_storage::agents::ExecutionMode::Structured;
    agent.output_schema = Some(vec![
        neomind_storage::OperatorField {
            name: "missing_count".into(),
            field_type: neomind_storage::OperatorFieldType::Number,
            unit: Some("件".into()),
            description: Some("漏装数量".into()),
        },
        neomind_storage::OperatorField {
            name: "batch_status".into(),
            field_type: neomind_storage::OperatorFieldType::Enum(vec![
                "正常".into(),
                "待检".into(),
            ]),
            unit: None,
            description: None,
        },
    ]);
    // Route resolution through the executor default runtime (harness has no
    // instance manager) — the mock scripts the one inference call.
    let rt: Arc<dyn LlmRuntime> = Arc::new(MockLlmRuntime::new(vec![MockResponse::text(
        r#"{"missing_count": 1, "batch_status": "待检"}"#,
    )]));
    executor.set_llm_runtime(rt).await;

    let (dp, er) = executor
        .execute_structured(
            "exec-structured-1",
            &agent,
            vec![neomind_storage::DataCollected {
                source: "cam-01".into(),
                data_type: "image".into(),
                values: serde_json::json!({"描述": "conveyor 有 1 件空位"}),
                timestamp: 0,
            }],
        )
        .await
        .expect("structured execution succeeds");

    assert_eq!(er.success_rate, 1.0);
    assert!(er.summary.contains("2 field(s)"), "summary: {}", er.summary);
    assert!(dp.conclusion.contains("待检"));
    assert_eq!(dp.stop_reason, "structured");
    assert_eq!(dp.decisions.len(), 1);
    assert!(dp.decisions[0].action.contains(&format!("ai:{}", agent.id)));
}

/// A structured agent is inference over its bound sources. When none of them
/// produced anything inside the window there is nothing to infer from — and the
/// memory summary it would otherwise fall back on is its *own previous
/// conclusion*. Publishing that is reasoning in a circle.
///
/// This is the 2026-09-23 report: a camera went idle for three hours while the
/// agent kept stamping out a confident "正常" every 15 minutes, each one feeding
/// the next, with downstream dashboards reading it as fresh data.
#[tokio::test]
async fn structured_mode_refuses_to_publish_without_input() {
    let (mut executor, mut agent, _registry) = build_harness().await;
    agent.execution_mode = neomind_storage::agents::ExecutionMode::Structured;
    agent.output_schema = Some(vec![neomind_storage::OperatorField {
        name: "anomaly_count".into(),
        field_type: neomind_storage::OperatorFieldType::Number,
        unit: None,
        description: None,
    }]);
    // What the branch would answer if it were allowed to run at all.
    let rt: Arc<dyn LlmRuntime> = Arc::new(MockLlmRuntime::new(vec![MockResponse::text(
        r#"{"anomaly_count": 0}"#,
    )]));
    executor.set_llm_runtime(rt).await;

    // Exactly what the collector hands over when every bound source missed the
    // window: the memory summary, and nothing else.
    let outcome = executor
        .execute_structured(
            "exec-no-input",
            &agent,
            vec![neomind_storage::DataCollected {
                source: "memory".into(),
                data_type: "summary".into(),
                values: serde_json::json!({
                    "last_conclusion": "{\"anomaly_count\":0}",
                    "total_executions": 20,
                }),
                timestamp: 0,
            }],
        )
        .await;

    assert!(
        outcome.is_err(),
        "an inference with no observation behind it must not be published as a conclusion"
    );
    let message = outcome.expect_err("checked above").to_string();
    assert!(
        message.contains("No time-series storage"),
        "the failure must say what was missing, not just that it failed: {message}"
    );
}

/// Design 002 §4.2, "every output carries its evidence": click an AI field and
/// see what it actually read. The execution record already holds that — the
/// collected sources, the rendered context, the conclusion. What a published
/// value lacks is the way *back* to it, so a dashboard showing `ai:x:status`
/// has no route to the run behind it.
#[tokio::test]
async fn published_fields_point_back_at_the_execution_that_produced_them() {
    let (mut executor, mut agent, _registry) = build_harness().await;
    let ts = neomind_storage::TimeSeriesStore::memory().expect("memory timeseries");
    executor.set_time_series_storage(ts.clone());
    agent.execution_mode = neomind_storage::agents::ExecutionMode::Structured;
    agent.output_schema = Some(vec![neomind_storage::OperatorField {
        name: "anomaly_count".into(),
        field_type: neomind_storage::OperatorFieldType::Number,
        unit: None,
        description: None,
    }]);
    let rt: Arc<dyn LlmRuntime> = Arc::new(MockLlmRuntime::new(vec![MockResponse::text(
        r#"{"anomaly_count": 2}"#,
    )]));
    executor.set_llm_runtime(rt).await;

    executor
        .execute_structured(
            "exec-pointer-1",
            &agent,
            vec![neomind_storage::DataCollected {
                source: "cam-01".into(),
                data_type: "values.parts".into(),
                values: serde_json::json!({"parts": 3}),
                timestamp: 0,
            }],
        )
        .await
        .expect("structured execution succeeds");

    ts.flush().expect("flush buffer");
    let point = ts
        .query_latest(&format!("ai:{}", agent.id), "anomaly_count")
        .await
        .expect("query succeeds")
        .expect("the field was published");

    assert_eq!(
        point
            .metadata
            .as_ref()
            .and_then(|m| m.get("execution_id"))
            .and_then(|v| v.as_str()),
        Some("exec-pointer-1"),
        "a published AI field must say which run produced it: {:?}",
        point.metadata
    );
    // The mock answered without a confidence, and the published value says so
    // rather than filling in a default.
    assert_eq!(
        point.quality, None,
        "no reported confidence means no quality claim, not a made-up one"
    );
}

/// The number the model reports is the number everything downstream sees —
/// the record and the published value both. A fabricated constant made every
/// threshold compare against the same figure forever; this is the assertion
/// that there is nothing left to fabricate.
#[tokio::test]
async fn the_reported_confidence_reaches_the_record_and_the_published_value() {
    let (mut executor, mut agent, _registry) = build_harness().await;
    let ts = neomind_storage::TimeSeriesStore::memory().expect("memory timeseries");
    executor.set_time_series_storage(ts.clone());
    agent.execution_mode = neomind_storage::agents::ExecutionMode::Structured;
    agent.output_schema = Some(vec![neomind_storage::OperatorField {
        name: "anomaly_count".into(),
        field_type: neomind_storage::OperatorFieldType::Number,
        unit: None,
        description: None,
    }]);
    let rt: Arc<dyn LlmRuntime> = Arc::new(MockLlmRuntime::new(vec![MockResponse::text(
        r#"{"anomaly_count": 2, "confidence": 0.42}"#,
    )]));
    executor.set_llm_runtime(rt).await;

    let (dp, _record) = executor
        .execute_structured(
            "exec-confidence-1",
            &agent,
            vec![neomind_storage::DataCollected {
                source: "cam-01".into(),
                data_type: "values.parts".into(),
                values: serde_json::json!({"parts": 3}),
                timestamp: 0,
            }],
        )
        .await
        .expect("structured execution succeeds");

    assert_eq!(
        dp.confidence,
        Some(0.42),
        "the record keeps the model's own number"
    );

    ts.flush().expect("flush buffer");
    let point = ts
        .query_latest(&format!("ai:{}", agent.id), "anomaly_count")
        .await
        .expect("query succeeds")
        .expect("the field was published");
    assert_eq!(
        point.quality,
        Some(0.42),
        "and the value downstream reads carries it too"
    );
    assert!(
        !point.value.to_string().contains("0.42"),
        "confidence is not one of the user's fields: {:?}",
        point.value
    );
}

/// The error has to say *why*, not just that. "No data" sends the operator
/// looking at the agent; "device:cam-01/occupied last reported 3h ago" sends
/// them to the camera, which is where the problem actually is.
#[tokio::test]
async fn a_starved_run_names_the_source_that_went_quiet_and_when() {
    use neomind_storage::timeseries::DataPoint as TsPoint;

    let (mut executor, mut agent, _registry) = build_harness().await;
    let ts = neomind_storage::TimeSeriesStore::memory().expect("memory timeseries");
    ts.write(
        "device:cam-01",
        "occupied",
        TsPoint {
            timestamp: chrono::Utc::now().timestamp() - 3 * 3600,
            value: serde_json::json!(true),
            quality: None,
            metadata: None,
        },
    )
    .await
    .expect("seed an old reading");
    ts.flush().expect("flush");
    executor.set_time_series_storage(ts);

    agent.execution_mode = neomind_storage::agents::ExecutionMode::Structured;
    agent.resources = vec![neomind_storage::AgentResource {
        resource_type: neomind_storage::ResourceType::Metric,
        resource_id: "cam-01:occupied".to_string(),
        name: "occupied".to_string(),
        config: serde_json::json!({}),
    }];
    agent.output_schema = Some(vec![neomind_storage::OperatorField {
        name: "status".into(),
        field_type: neomind_storage::OperatorFieldType::Number,
        unit: None,
        description: None,
    }]);

    let message = executor
        .execute_structured("exec-silent", &agent, vec![])
        .await
        .expect_err("a run with nothing to read must fail")
        .to_string();

    assert!(
        message.contains("device:cam-01"),
        "the source has to be named: {message}"
    );
    assert!(
        message.contains("3h"),
        "and how long it has been quiet: {message}"
    );
    assert!(
        !message.contains("Configuration"),
        "the camera being offline is not a configuration mistake: {message}"
    );
}

/// The dispatch in `execute_internal` must route a structured agent to the L0
/// branch. Its sibling above calls `execute_structured` directly, so it cannot
/// catch a dispatch that routes elsewhere — this one enters through the same
/// production entry point the scheduler uses.
#[tokio::test]
async fn structured_mode_is_dispatched_from_the_production_entry_point() {
    use neomind_storage::timeseries::DataPoint as TsPoint;
    use neomind_storage::TimeSeriesStore;

    // This test is about routing, not about data — but since 2026-09-23 a
    // structured agent with nothing to read is refused before the branch, so
    // reaching it now means giving the agent something to read.
    let storage = TimeSeriesStore::memory().expect("memory timeseries");
    storage
        .write(
            "device:dev-1",
            "temperature",
            TsPoint {
                timestamp: chrono::Utc::now().timestamp(),
                value: serde_json::json!(23.5),
                quality: None,
                metadata: None,
            },
        )
        .await
        .expect("seed telemetry");
    storage.flush().expect("flush to storage");

    let (mut executor, mut agent, _registry) = build_harness().await;
    executor.set_time_series_storage(storage);
    agent.execution_mode = neomind_storage::agents::ExecutionMode::Structured;
    agent.resources = vec![neomind_storage::AgentResource {
        resource_type: neomind_storage::ResourceType::Metric,
        resource_id: "dev-1:temperature".to_string(),
        name: "temperature".to_string(),
        config: serde_json::json!({}),
    }];
    agent.output_schema = Some(vec![neomind_storage::OperatorField {
        name: "missing_count".into(),
        field_type: neomind_storage::OperatorFieldType::Number,
        unit: None,
        description: None,
    }]);
    let rt: Arc<dyn LlmRuntime> = Arc::new(MockLlmRuntime::new(vec![MockResponse::text(
        r#"{"missing_count": 1}"#,
    )]));
    executor.set_llm_runtime(rt).await;
    // `execute_agent` persists an execution record, so the agent must exist.
    executor
        .store()
        .save_agent(&agent)
        .await
        .expect("seed store");

    let record = executor
        .execute_agent(agent, None, None)
        .await
        .expect("structured execution through the production entry point");

    assert_eq!(
        record.decision_process.stop_reason, "structured",
        "a structured agent must reach the L0 branch, not the tool loop"
    );
}

/// Build an event-triggered agent whose filter matches every device source,
/// seed it, and hand back the executor. `debounce_secs` of `None` omits the
/// operator config entirely (no L0 guardrails).
async fn build_event_agent(
    mode: ExecutionMode,
    debounce_secs: Option<u32>,
) -> (AgentExecutor, AiAgent) {
    let (mut executor, mut agent, _registry) = build_harness().await;
    agent.schedule = AgentSchedule {
        schedule_type: ScheduleType::Event,
        interval_seconds: None,
        cron_expression: None,
        timezone: None,
        event_filter: Some(r#"{"sources":[{"type":"device","id":"all"}]}"#.to_string()),
    };
    agent.execution_mode = mode;
    agent.output_schema = Some(vec![neomind_storage::OperatorField {
        name: "missing_count".into(),
        field_type: neomind_storage::OperatorFieldType::Number,
        unit: None,
        description: None,
    }]);
    agent.operator_config = debounce_secs.map(|debounce_secs| neomind_storage::OperatorConfig {
        debounce_secs,
        max_calls_per_day: None,
        timeout_secs: 60,
        consecutive_failure_threshold: 3,
        event_cooldown_secs: None,
    });
    executor
        .store()
        .save_agent(&agent)
        .await
        .expect("seed store");
    let rt: Arc<dyn LlmRuntime> = Arc::new(MockLlmRuntime::new(vec![MockResponse::text(
        r#"{"missing_count": 1}"#,
    )]));
    executor.set_llm_runtime(rt).await;
    (executor, agent)
}

/// Wait for every execution the event trigger has spawned to finish.
///
/// `refresh_event_agents` only caches agents whose status is `Active`, and a run
/// in flight has set it to `Executing`. So an event that arrives while the
/// previous one is still running finds an *empty* cache and is dropped before
/// the cooldown is ever consulted. That is the production behaviour — an agent
/// does not re-enter itself — and it is a race in any test that fires twice
/// back to back: under load the second event loses, and the assertion then reads
/// a map the first run never touched.
///
/// Draining between fires makes the trigger actually reach the code under test.
async fn drain_event_tasks(executor: &AgentExecutor) -> usize {
    let mut total = 0usize;
    loop {
        let handles: Vec<_> = executor.event_task_handles.lock().drain(..).collect();
        if handles.is_empty() {
            return total;
        }
        total += handles.len();
        for handle in handles {
            let _ = handle.await;
        }
    }
}

/// One physical event can match several of a structured agent's sources. The
/// debounce window must merge them so the agent pays for one inference, not
/// one per match — that is what the editor's "防抖" field promises.
#[tokio::test]
async fn structured_event_agent_debounces_across_sources() {
    let (executor, agent) = build_event_agent(ExecutionMode::Structured, Some(300)).await;

    let mut runs = 0usize;
    for source in ["dev-a", "dev-b"] {
        executor
            .check_and_trigger_data_event(
                "device",
                source.to_string(),
                "temp".to_string(),
                &neomind_core::event::MetricValue::Float(1.0),
                1_700_000_000,
            )
            .await
            .expect("trigger must not error");
        // Let the run this just spawned finish, so the next source meets an
        // agent that is Active again instead of an empty cache.
        runs += drain_event_tasks(&executor).await;
    }

    assert_eq!(
        runs, 1,
        "a second source inside the debounce window must be merged away, \
         not run as its own inference"
    );
    let recent = executor.recent_executions.read().await;
    assert!(
        recent.contains_key(&agent.id),
        "the cooldown is AGENT-scoped (bare agent id key: {:?})",
        recent.keys().collect::<Vec<_>>()
    );
}

/// Free/Focused event agents merge bursts too (30s fixed window): the old
/// design keyed cooldown per (agent, source) and skipped debounce entirely
/// for non-structured modes, so N sources reporting in the same instant
/// fired N concurrent runs of one agent — racing each other's journal
/// read-modify-write and burning the concurrency budget.
#[tokio::test]
async fn free_event_agent_merges_same_instant_sources() {
    let (executor, _agent) = build_event_agent(ExecutionMode::Free, None).await;

    let mut runs = 0usize;
    for source in ["dev-a", "dev-b", "dev-c"] {
        executor
            .check_and_trigger_data_event(
                "device",
                source.to_string(),
                "temp".to_string(),
                &neomind_core::event::MetricValue::Float(1.0),
                1_700_000_000,
            )
            .await
            .expect("trigger must not error");
        runs += drain_event_tasks(&executor).await;
    }

    assert_eq!(
        runs, 1,
        "three sources in the same instant must produce ONE run, not three \
         concurrent executions of the same agent"
    );
}

/// Re-save an event agent with a full operator config, so a test can pin the
/// cooldown/debounce pair precisely (the `build_event_agent` helper only
/// takes a debounce).
async fn with_operator_config(
    executor: &AgentExecutor,
    agent: &mut AiAgent,
    config: neomind_storage::OperatorConfig,
) {
    agent.operator_config = Some(config);
    executor.store().save_agent(agent).await.expect("re-seed");
}

/// `event_cooldown_secs: 0` (on-demand): an idle agent must admit the very
/// next event. The hard-coded 60s floor used to idle fast agents — a 5s run
/// spent 55s dropping events that had arrived after it finished. Structured
/// mode with `debounce_secs: 0` leaves every pacing gate open, so this is
/// the purest form of the regression.
#[tokio::test]
async fn on_demand_cooldown_admits_next_event_once_idle() {
    let (executor, mut agent) = build_event_agent(ExecutionMode::Structured, Some(0)).await;
    let mut op = crate::ai_agent::executor::structured::default_operator_config();
    op.debounce_secs = 0;
    op.event_cooldown_secs = Some(0);
    with_operator_config(&executor, &mut agent, op).await;

    fire_event(&executor, "device", "dev-a", "temp").await;
    let runs = drain_event_tasks(&executor).await;
    fire_event(&executor, "device", "dev-a", "temp").await;
    let runs = runs + drain_event_tasks(&executor).await;

    assert_eq!(
        runs, 2,
        "with cooldown 0 an idle agent must admit the next event immediately — \
         no fixed one-minute lockout between runs"
    );
}

/// Free/Focused keep a 1s burst-merge floor even at cooldown 0 (one physical
/// report fans out to N per-metric events), but the floor must not become a
/// pacing gate: an event after the floor has elapsed admits.
#[tokio::test]
async fn on_demand_free_agent_admits_after_the_burst_floor() {
    let (executor, mut agent) = build_event_agent(ExecutionMode::Free, None).await;
    let mut op = crate::ai_agent::executor::structured::default_operator_config();
    op.event_cooldown_secs = Some(0);
    with_operator_config(&executor, &mut agent, op).await;

    fire_event(&executor, "device", "dev-a", "temp").await;
    let runs = drain_event_tasks(&executor).await;
    // Free/Focused merge window is clamp(cooldown, 1s..30s) = 1s here.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    fire_event(&executor, "device", "dev-b", "temp").await;
    let runs = runs + drain_event_tasks(&executor).await;

    assert_eq!(
        runs, 2,
        "after the 1s burst floor an idle free agent must admit the next event"
    );
}

/// The trap that bit the field deployment: a Structured agent with the
/// DEFAULT 30s debounce and an explicit cooldown of 0 — the debounce
/// silently overrode the cooldown, keeping an "on-demand" image agent on a
/// 30s cadence. An explicit cooldown now caps the debounce (floored at the
/// 1s burst merge), so the agent's own saved config heals without an edit.
#[tokio::test]
async fn explicit_cooldown_caps_the_structured_debounce() {
    let (executor, mut agent) = build_event_agent(ExecutionMode::Structured, Some(30)).await;
    let mut op = crate::ai_agent::executor::structured::default_operator_config();
    op.debounce_secs = 30; // the untouched default that used to win
    op.event_cooldown_secs = Some(0);
    with_operator_config(&executor, &mut agent, op).await;

    fire_event(&executor, "device", "dev-a", "temp").await;
    let runs = drain_event_tasks(&executor).await;
    // Debounce capped at the 1s burst floor, not the configured 30s.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    fire_event(&executor, "device", "dev-b", "temp").await;
    let runs = runs + drain_event_tasks(&executor).await;

    assert_eq!(
        runs, 2,
        "cooldown 0 + default debounce 30 must behave as on-demand (1s merge \
         floor), not as a 30s-paced agent"
    );
}

/// Without an explicit cooldown the Structured debounce keeps its historical
/// meaning — long merges set before the cooldown existed still hold.
#[tokio::test]
async fn structured_debounce_without_cooldown_is_unchanged() {
    let (executor, _agent) = build_event_agent(ExecutionMode::Structured, Some(300)).await;

    fire_event(&executor, "device", "dev-a", "temp").await;
    let runs = drain_event_tasks(&executor).await;
    // Well inside the 300s window: must merge, exactly as before.
    fire_event(&executor, "device", "dev-b", "temp").await;
    let runs = runs + drain_event_tasks(&executor).await;

    assert_eq!(
        runs, 1,
        "no explicit cooldown means the debounce semantics are untouched"
    );
}

/// A short configured cooldown both paces admissions AND shrinks the
/// Free/Focused merge window to match — otherwise the fixed 30s merge would
/// silently override a 2s cooldown and the setting would do nothing.
#[tokio::test]
async fn short_cooldown_paces_and_tracks_the_configured_window() {
    let (executor, mut agent) = build_event_agent(ExecutionMode::Free, None).await;
    let mut op = crate::ai_agent::executor::structured::default_operator_config();
    op.event_cooldown_secs = Some(2);
    with_operator_config(&executor, &mut agent, op).await;

    fire_event(&executor, "device", "dev-a", "temp").await;
    let runs = drain_event_tasks(&executor).await;

    fire_event(&executor, "device", "dev-b", "temp").await;
    let after_burst = runs + drain_event_tasks(&executor).await;
    assert_eq!(
        after_burst, runs,
        "an event inside the 2s window must be paced away, not admitted"
    );

    tokio::time::sleep(std::time::Duration::from_millis(2200)).await;
    fire_event(&executor, "device", "dev-c", "temp").await;
    let runs = after_burst + drain_event_tasks(&executor).await;
    assert_eq!(runs, 2, "once the 2s window elapses the next event admits");
}

/// Agents without an explicit cooldown keep the historical 60s floor: the
/// default is unchanged for every existing deployment.
#[tokio::test]
async fn default_event_cooldown_still_gates_back_to_back_events() {
    let (executor, _agent) = build_event_agent(ExecutionMode::Structured, None).await;

    fire_event(&executor, "device", "dev-a", "temp").await;
    let runs = drain_event_tasks(&executor).await;
    fire_event(&executor, "device", "dev-b", "temp").await;
    let runs = runs + drain_event_tasks(&executor).await;

    assert_eq!(
        runs, 1,
        "no configured cooldown means the 60s default floor, exactly as before"
    );
}

/// A busy admission is a DROP, not a retry: "already executing" means another
/// run owns the agent, and the old 5s-backoff retry queued a phantom second
/// run behind it. Must return Ok (busy is not a failure — mirrors the
/// scheduler's treatment) and must not sit through the backoff.
#[tokio::test]
async fn busy_event_admission_drops_without_the_retry_backoff() {
    let (executor, agent) = build_event_agent(ExecutionMode::Free, None).await;
    // Occupy the process-global in-flight set without running anything, so
    // execute_agent rejects with "already executing".
    super::in_flight_agents()
        .lock()
        .unwrap()
        .insert(agent.id.clone());

    let event = super::context::EventTriggerData {
        source: super::context::DataSourceRef {
            source_type: "device".into(),
            source_id: "dev-a".into(),
            field: "temp".into(),
        },
        value: neomind_core::event::MetricValue::Float(1.0),
        timestamp: 0,
    };
    let started = std::time::Instant::now();
    let out =
        AgentExecutor::execute_with_retry(&executor, agent.clone(), event, 1, &agent.id).await;
    super::in_flight_agents().lock().unwrap().remove(&agent.id);

    assert!(
        matches!(out, super::event_trigger::EventRunOutcome::Completed),
        "busy is a drop, not an error: {out:?}"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(3),
        "must return without the 5s retry backoff (took {:?})",
        started.elapsed()
    );
}

/// The breaker threshold is finally REAL: one permanent-classified failure
/// keeps the agent Active; only `consecutive_failure_threshold` in a row
/// demotes it. (The field existed in the editor for months while nothing
/// compared it — a single small-model glitch killed event agents outright.)
/// Driven by the silence refusal: a structured run with no observable input
/// produces a permanent-classified Failed record, deterministically.
#[tokio::test]
async fn breaker_demotes_only_after_consecutive_threshold() {
    let (executor, agent) = build_event_agent(ExecutionMode::Structured, None).await;

    for attempt in 1..=3u32 {
        let record = executor
            .execute_agent(agent.clone(), None, None)
            .await
            .expect("failure must still be recorded as a run");
        assert_eq!(
            record.status,
            neomind_storage::ExecutionStatus::Failed,
            "silence refusal is a Failed record"
        );
        let stored = executor
            .store()
            .get_agent(&agent.id)
            .await
            .expect("store")
            .expect("agent exists");
        if attempt < 3 {
            assert_eq!(
                stored.status,
                neomind_storage::AgentStatus::Active,
                "permanent failure {}/3 must NOT demote before the threshold",
                attempt
            );
        } else {
            assert_eq!(
                stored.status,
                neomind_storage::AgentStatus::Error,
                "the third consecutive permanent failure trips the breaker"
            );
        }
    }
    // Demotion seeds the recovery clock for the event path.
    assert!(
        super::global_error_probe_state()
            .read()
            .contains_key(&agent.id),
        "Active→Error must seed the recovery-probe clock"
    );
    super::global_error_probe_state().write().remove(&agent.id);
}

/// A demoted event agent is PROBED by matching events once the backoff has
/// elapsed, and a successful probe restores Active — full self-healing, the
/// thing the old design could not do at all (Error event agents were only
/// reachable by hand).
#[tokio::test]
async fn error_agent_probes_on_events_and_recovers() {
    let (executor, mut agent) = build_event_agent(ExecutionMode::Structured, Some(0)).await;
    let mut op = crate::ai_agent::executor::structured::default_operator_config();
    op.debounce_secs = 0;
    op.event_cooldown_secs = Some(0);
    with_operator_config(&executor, &mut agent, op).await;

    // Demote by hand and age the probe clock past its backoff.
    executor
        .store()
        .update_agent_status(
            &agent.id,
            neomind_storage::AgentStatus::Error,
            Some("test demotion".into()),
        )
        .await
        .unwrap();
    let old = chrono::Utc::now().timestamp() - 3600;
    super::global_error_probe_state().write().insert(
        agent.id.clone(),
        super::ErrorProbeEntry {
            last_attempt: old,
            consecutive_failures: 2,
        },
    );

    fire_event(&executor, "device", "dev-a", "temp").await;
    let runs = drain_event_tasks(&executor).await;

    assert_eq!(runs, 1, "an aged-backoff Error agent must admit the probe");
    let stored = executor
        .store()
        .get_agent(&agent.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.status,
        neomind_storage::AgentStatus::Active,
        "a successful probe must restore Active"
    );
    assert!(
        !super::global_error_probe_state()
            .read()
            .contains_key(&agent.id),
        "recovery must clear the backoff ladder"
    );
}

/// Inside the backoff window the probe gate holds: matching events are
/// skipped, no run spawns, the agent stays Error.
#[tokio::test]
async fn error_agent_in_backoff_skips_events() {
    let (executor, mut agent) = build_event_agent(ExecutionMode::Structured, Some(0)).await;
    let mut op = crate::ai_agent::executor::structured::default_operator_config();
    op.debounce_secs = 0;
    op.event_cooldown_secs = Some(0);
    with_operator_config(&executor, &mut agent, op).await;

    executor
        .store()
        .update_agent_status(
            &agent.id,
            neomind_storage::AgentStatus::Error,
            Some("test demotion".into()),
        )
        .await
        .unwrap();
    let now = chrono::Utc::now().timestamp();
    super::global_error_probe_state().write().insert(
        agent.id.clone(),
        super::ErrorProbeEntry {
            last_attempt: now,
            consecutive_failures: 0,
        },
    );

    fire_event(&executor, "device", "dev-a", "temp").await;
    let runs = drain_event_tasks(&executor).await;

    assert_eq!(runs, 0, "a probe inside its backoff window must not run");
    super::global_error_probe_state().write().remove(&agent.id);
}

/// A FAILED probe steps the backoff ladder up instead of restarting it —
/// the difference between "broken agent costs ~24 attempts/day" and "it
/// retries every 5 minutes forever".
#[tokio::test]
async fn failed_probe_climbs_the_backoff_ladder() {
    let (mut executor, mut agent) = build_event_agent(ExecutionMode::Structured, Some(0)).await;
    let mut op = crate::ai_agent::executor::structured::default_operator_config();
    op.debounce_secs = 0;
    op.event_cooldown_secs = Some(0);
    with_operator_config(&executor, &mut agent, op).await;
    // Every inference fails: empty script → the exhausted sentinel is not
    // valid JSON for the structured contract.
    executor
        .set_llm_runtime(Arc::new(MockLlmRuntime::new(vec![])))
        .await;

    executor
        .store()
        .update_agent_status(
            &agent.id,
            neomind_storage::AgentStatus::Error,
            Some("test demotion".into()),
        )
        .await
        .unwrap();
    let old = chrono::Utc::now().timestamp() - 3600;
    super::global_error_probe_state().write().insert(
        agent.id.clone(),
        super::ErrorProbeEntry {
            last_attempt: old,
            consecutive_failures: 0,
        },
    );

    fire_event(&executor, "device", "dev-a", "temp").await;
    let runs = drain_event_tasks(&executor).await;

    assert_eq!(runs, 1, "aged backoff admits the probe");
    let stored = executor
        .store()
        .get_agent(&agent.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.status,
        neomind_storage::AgentStatus::Error,
        "a failed probe keeps the agent demoted"
    );
    let entry = super::global_error_probe_state()
        .read()
        .get(&agent.id)
        .copied();
    super::global_error_probe_state().write().remove(&agent.id);
    let entry = entry.expect("probe outcome must be recorded");
    assert!(
        entry.consecutive_failures >= 1,
        "a failed probe must climb the ladder (got {entry:?})"
    );
    assert!(
        entry.last_attempt > old,
        "the failed probe re-arms the clock from its own attempt"
    );
}

/// The daily run cap must survive across executor INSTANCES: the event
/// path constructs a fresh `AgentExecutor` per trigger, and a per-instance
/// counter map meant the cap reset on every event execution — the budget
/// guardrail was a no-op on the most frequent trigger path.
#[tokio::test]
async fn daily_cap_survives_across_executor_instances() {
    let (executor, mut agent, _registry) = build_harness().await;
    agent.id = "daily-cap-agent".into(); // unique id: the counters are process-global
    let mut op_cfg = crate::ai_agent::executor::structured::default_operator_config();
    op_cfg.max_calls_per_day = Some(1);
    agent.operator_config = Some(op_cfg);
    executor.store().save_agent(&agent).await.unwrap();

    let first = executor.execute_agent(agent.clone(), None, None).await;
    assert!(
        first.is_ok(),
        "first run of the day must proceed: {first:?}"
    );

    // A SECOND executor instance, as the event path builds per trigger.
    let config = AgentExecutorConfig {
        store: executor.store().clone(),
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
    let executor2 = AgentExecutor::new(config).await.expect("executor2");

    let second = executor2.execute_agent(agent.clone(), None, None).await;
    let record = second.expect("a capped run records as a Failed execution, not an Err");
    let err = record.error.as_deref().unwrap_or_default();
    assert!(
        err.contains("daily run cap reached"),
        "the second instance's run must be refused by the shared cap, got: {err:?}"
    );
}

/// Two concurrent executions of the SAME agent must not both run: their
/// finalize paths read-modify-write the same journal and would silently
/// erase each other's entries. The process-wide in-flight guard in
/// `execute_agent` rejects the second with a clear error. (Manual invoke
/// used to be the unguarded entry point.)
#[tokio::test]
async fn concurrent_same_agent_execution_is_rejected() {
    let (executor, agent, _registry) = build_harness().await;
    // execute_agent records against the store; the bare harness agent must
    // be persisted first.
    executor.store().save_agent(&agent).await.unwrap();

    let a = executor.execute_agent(agent.clone(), None, None);
    let b = executor.execute_agent(agent.clone(), None, None);
    let (ra, rb) = tokio::join!(a, b);

    let ok_count = [&ra, &rb].iter().filter(|r| r.is_ok()).count();
    assert_eq!(
        ok_count, 1,
        "exactly one concurrent run of an agent may proceed (got {ra:?} / {rb:?})"
    );
    let rejected = if ra.is_err() { &ra } else { &rb };
    let msg = format!("{}", rejected.as_ref().unwrap_err());
    assert!(
        msg.contains("already executing"),
        "rejection must say why, got: {msg}"
    );

    // And once the run finished, the agent is runnable again.
    drain_event_tasks(&executor).await;
    let again = executor.execute_agent(agent.clone(), None, None).await;
    assert!(
        again.is_ok(),
        "guard must release after completion: {again:?}"
    );
}

/// A FAILED run must never publish: the structured executor's promise is
/// "failure keeps the old value" (design: a scanner whose camera went
/// offline keeps its last good reading rather than publishing from
/// memory). Publish used to run BEFORE the journaling step, so a run that
/// failed after inference still served "fresh" values to dashboards/rules.
/// The observable contract, whatever stage fails: inference error → no
/// publish, the previously-published value stands, journal records the
/// failure.
#[tokio::test]
async fn failed_run_publishes_nothing_and_keeps_the_old_value() {
    let (mut executor, mut agent, _registry) = build_harness().await;
    let ts = neomind_storage::TimeSeriesStore::memory().expect("memory timeseries");
    executor.set_time_series_storage(ts.clone());
    agent.execution_mode = neomind_storage::agents::ExecutionMode::Structured;
    agent.output_schema = Some(vec![neomind_storage::OperatorField {
        name: "anomaly_count".into(),
        field_type: neomind_storage::OperatorFieldType::Number,
        unit: None,
        description: None,
    }]);
    let data = vec![neomind_storage::DataCollected {
        source: "cam-01".into(),
        data_type: "values.parts".into(),
        values: serde_json::json!({"parts": 3}),
        timestamp: 0,
    }];

    // 1) A good run publishes the first value.
    let rt: Arc<dyn LlmRuntime> = Arc::new(MockLlmRuntime::new(vec![MockResponse::text(
        r#"{"anomaly_count": 2}"#,
    )]));
    executor.set_llm_runtime(rt).await;
    executor
        .execute_structured("exec-ok", &agent, data.clone())
        .await
        .expect("first run succeeds");
    ts.flush().unwrap();

    // 2) A failing run (inference error) must NOT touch the published value.
    let rt_err: Arc<dyn LlmRuntime> = Arc::new(MockLlmRuntime::new(vec![MockResponse::error(
        "backend down",
    )]));
    executor.set_llm_runtime(rt_err).await;
    let failed = executor.execute_structured("exec-fail", &agent, data).await;
    assert!(failed.is_err(), "an inference error must fail the run");
    ts.flush().unwrap();

    let latest = ts
        .query_latest(&format!("ai:{}", agent.id), "anomaly_count")
        .await
        .expect("query succeeds")
        .expect("the old value must still stand");
    assert_eq!(
        point_value_as_i64(&latest),
        Some(2),
        "the failed run must not publish over the last good value"
    );
    assert_eq!(
        latest
            .metadata
            .as_ref()
            .and_then(|m| m.get("execution_id"))
            .and_then(|v| v.as_str()),
        Some("exec-ok"),
        "the standing value must still point at the run that produced it"
    );
}

/// Pull a small integer out of a published point value, for assertions.
fn point_value_as_i64(p: &neomind_storage::DataPoint) -> Option<i64> {
    p.value
        .as_i64()
        .or_else(|| p.value.as_f64().map(|f| f as i64))
}

/// Seed an event agent whose filter uses the M2 `all` form: the agent runs
/// only once BOTH sources have reported inside the window.
async fn build_event_agent_with_filter(filter: String) -> (AgentExecutor, AiAgent) {
    let (mut executor, mut agent, _registry) = build_harness().await;
    agent.schedule = AgentSchedule {
        schedule_type: ScheduleType::Event,
        interval_seconds: None,
        cron_expression: None,
        timezone: None,
        event_filter: Some(filter),
    };
    executor
        .store()
        .save_agent(&agent)
        .await
        .expect("seed store");
    let rt: Arc<dyn LlmRuntime> = Arc::new(MockLlmRuntime::new(vec![MockResponse::text("ok")]));
    executor.set_llm_runtime(rt).await;
    (executor, agent)
}

async fn build_all_filter_event_agent(within_secs: Option<u64>) -> (AgentExecutor, AiAgent) {
    let window = within_secs
        .map(|w| format!(r#","within_secs":{w}"#))
        .unwrap_or_default();
    build_event_agent_with_filter(format!(
        r#"{{"all":[{{"type":"device","id":"cam-01","field":"occupied"}},{{"type":"extension","id":"booking","field":"reserved"}}]{window}}}"#
    ))
    .await
}

/// Drive one data event through the production trigger entry point.
async fn fire_event(executor: &AgentExecutor, source_type: &str, source_id: &str, field: &str) {
    executor
        .check_and_trigger_data_event(
            source_type,
            source_id.to_string(),
            field.to_string(),
            &neomind_core::event::MetricValue::Boolean(true),
            1_700_000_000,
        )
        .await
        .expect("trigger must not error");
}

/// The reason multi-source triggering exists at all: "occupancy AND not
/// booked" must not fire on either source alone — firing on one of them is
/// exactly what today's OR-only filter does.
#[tokio::test]
async fn all_filter_waits_for_every_source_before_firing() {
    let (executor, agent) = build_all_filter_event_agent(Some(1200)).await;

    executor
        .check_and_trigger_data_event(
            "device",
            "cam-01".to_string(),
            "occupied".to_string(),
            &neomind_core::event::MetricValue::Boolean(true),
            1_700_000_000,
        )
        .await
        .expect("trigger must not error");

    assert!(
        !executor
            .recent_executions
            .read()
            .await
            .contains_key(&agent.id),
        "one of the two sources must NOT fire the agent"
    );

    executor
        .check_and_trigger_data_event(
            "extension",
            "booking".to_string(),
            "reserved".to_string(),
            &neomind_core::event::MetricValue::Boolean(false),
            1_700_000_000,
        )
        .await
        .expect("trigger must not error");

    assert!(
        executor
            .recent_executions
            .read()
            .await
            .contains_key(&agent.id),
        "the source that completes the window must fire the agent"
    );
}

/// `any` and `all` are two independent groups in one filter, not nested
/// logic: an `any` match fires immediately and does not wait for the `all`
/// window (design 001 §5.2.2 — the filter is `any` OR `all`, nothing more).
#[tokio::test]
async fn an_any_group_fires_on_its_own_alongside_an_all_group() {
    let (executor, agent) = build_event_agent_with_filter(
        r#"{"any":[{"type":"device","id":"door-01","field":"open"}],"all":[{"type":"device","id":"cam-01","field":"occupied"},{"type":"extension","id":"booking","field":"reserved"}],"within_secs":1200}"#
            .to_string(),
    )
    .await;

    fire_event(&executor, "device", "door-01", "open").await;

    assert!(
        executor
            .recent_executions
            .read()
            .await
            .contains_key(&agent.id),
        "an `any` match must fire straight away, without waiting for the `all` window"
    );
}

/// The regression that matters most: a saved agent still using the legacy
/// `sources` shape must keep firing exactly as it did before M2.
#[tokio::test]
async fn a_legacy_sources_filter_still_fires_on_a_single_match() {
    let (executor, agent) = build_event_agent_with_filter(
        r#"{"sources":[{"type":"device","id":"dev-a","field":"temp"}]}"#.to_string(),
    )
    .await;

    fire_event(&executor, "device", "dev-a", "temp").await;

    assert!(
        executor
            .recent_executions
            .read()
            .await
            .contains_key(&agent.id),
        "the legacy sources shape must keep firing on one match"
    );
}

/// The contract step must actually run inside a real execution. The helper
/// tests below would still pass if nothing ever called `apply_output_contract` —
/// this one drives `execute_agent`, the entry point the scheduler uses.
#[tokio::test]
async fn execute_agent_applies_the_output_contract_for_a_reasoning_agent() {
    let (mut executor, mut agent, registry) = build_harness().await;
    executor.set_tool_registry(registry);
    agent.execution_mode = ExecutionMode::Free;
    // Intent is parsed once and cached on the agent, so a re-run never asks again.
    agent.parsed_intent = Some(neomind_storage::ParsedIntent {
        intent_type: neomind_storage::IntentType::Monitoring,
        target_metrics: vec![],
        conditions: vec![],
        actions: vec![],
        confidence: 0.9,
    });
    agent.output_schema = Some(vec![neomind_storage::OperatorField {
        name: "missing_count".into(),
        field_type: neomind_storage::OperatorFieldType::Number,
        unit: None,
        description: None,
    }]);
    executor
        .store()
        .save_agent(&agent)
        .await
        .expect("seed store");

    // 1) the tool loop's answer, 2) the contract extraction over it.
    let rt: Arc<dyn LlmRuntime> = Arc::new(
        MockLlmRuntime::new(vec![
            MockResponse::text("画面里有 1 件漏装，批次待检"),
            MockResponse::text(r#"{"missing_count": 1}"#),
        ])
        .with_function_calling(),
    );
    executor.set_llm_runtime(rt).await;

    let record = executor
        .execute_agent(agent, None, None)
        .await
        .expect("run completes");

    assert!(
        record
            .decision_process
            .decisions
            .iter()
            .any(|d| d.decision_type == "output_contract"),
        "the contract step must run and be recorded; got {:?}",
        record.decision_process.decisions
    );
}

/// Freshness with an age gate, through the shared windowed-latest helper the
/// device-level collector uses: a point written milliseconds ago (still in
/// the write buffer) MUST be visible; a point older than the window MUST
/// stay excluded — cache-first must not turn into latest-ever.
#[tokio::test]
async fn windowed_latest_is_fresh_but_not_stale() {
    use neomind_storage::timeseries::DataPoint as TsPoint;
    use neomind_storage::TimeSeriesStore;

    let store = TimeSeriesStore::memory().expect("memory timeseries");
    let now = chrono::Utc::now().timestamp();

    // Fresh point — written, deliberately NOT flushed.
    store
        .write(
            "device:dev-3",
            "temperature",
            TsPoint {
                timestamp: now,
                value: serde_json::json!(23.5),
                quality: None,
                metadata: None,
            },
        )
        .await
        .expect("seed fresh");
    // Stale point — 2h old, flushed to redb (the durable old world).
    store
        .write(
            "device:dev-3",
            "humidity",
            TsPoint {
                timestamp: now - 7200,
                value: serde_json::json!(60),
                quality: None,
                metadata: None,
            },
        )
        .await
        .expect("seed stale");
    store
        .flush()
        .expect("flush (stale point only reaches redb here)");

    let fresh = AgentExecutor::latest_point_in_window(&store, "dev-3", "temperature", now - 3600)
        .await
        .expect("fresh metric visible before flush")
        .expect("Some");
    assert_eq!(fresh.value, serde_json::json!(23.5));

    let stale = AgentExecutor::latest_point_in_window(&store, "dev-3", "humidity", now - 3600)
        .await
        .expect("no error");
    assert!(
        stale.is_none(),
        "a point older than the window must stay excluded"
    );
}

/// Telemetry writes are buffered; `query_range` reads only redb. A device
/// report triggers execution within milliseconds, so collection ran before
/// the flush and saw none of the just-written values — 12 bound metrics,
/// 0 collected, every time the freshest report is the interesting one.
/// The latest-value cache IS updated synchronously on write, so a
/// no-history collection must read through it.
#[tokio::test]
async fn metric_collection_sees_just_written_values_before_flush() {
    use neomind_storage::timeseries::DataPoint as TsPoint;
    use neomind_storage::TimeSeriesStore;

    let store = TimeSeriesStore::memory().expect("memory timeseries");
    store
        .write(
            "device:dev-2",
            "battery",
            TsPoint {
                timestamp: chrono::Utc::now().timestamp(),
                value: serde_json::json!(84),
                quality: None,
                metadata: None,
            },
        )
        .await
        .expect("seed telemetry");
    // Deliberately NO flush: the point sits in the write buffer + latest
    // cache, exactly like a report that triggered this very execution.

    let collected = AgentExecutor::collect_single_metric(
        store,
        "dev-2",
        "battery",
        "dev-2:battery".to_string(),
        60,
        false, // include_history = false — only the latest value matters
        1000,
        false,
        false,
        chrono::Utc::now().timestamp(),
    )
    .await
    .expect("collection must not error");

    let item = collected.expect("a value written milliseconds ago MUST be visible");
    assert_eq!(item.values.get("value"), Some(&serde_json::json!(84)));
}

/// Telemetry is written under `device:{id}` (device service, capability
/// providers — every write path). Metric collection queried the bare id, so
/// every bound-metric agent collected nothing: 31 resources → 0 data points,
/// and a 1-second "successful" run.
#[tokio::test]
async fn metric_collection_reads_the_key_telemetry_is_written_under() {
    use neomind_storage::timeseries::DataPoint as TsPoint;
    use neomind_storage::TimeSeriesStore;

    let store = TimeSeriesStore::memory().expect("memory timeseries");
    // The write path: source key `device:{id}` — see DeviceService and the
    // capability provider.
    store
        .write(
            "device:dev-1",
            "temperature",
            TsPoint {
                timestamp: chrono::Utc::now().timestamp(),
                value: serde_json::json!(23.5),
                quality: None,
                metadata: None,
            },
        )
        .await
        .expect("seed telemetry");
    // Writes are buffered; query_range reads redb, not the cache.
    store.flush().expect("flush to storage");

    let collected = AgentExecutor::collect_single_metric(
        store,
        "dev-1",
        "temperature",
        "dev-1:temperature".to_string(),
        60,
        false,
        1000,
        false,
        false,
        chrono::Utc::now().timestamp(),
    )
    .await
    .expect("collection must not error");

    let item = collected.expect("the bound metric MUST be found");
    assert_eq!(
        item.values.get("value"),
        Some(&serde_json::json!(23.5)),
        "the value read back must be the one written"
    );
}

/// The watch-style trigger: on=always notifies on SUCCESS too, at Info
/// severity, so a 盯-style agent reports every verdict.
#[tokio::test]
async fn notify_on_always_reports_successes() {
    use neomind_storage::timeseries::DataPoint as TsPoint;

    let store = AgentStore::memory().expect("store");
    let message_manager = Arc::new(neomind_messages::MessageManager::new());
    // A structured agent is inference over its bound sources; since 2026-09-23
    // it refuses to run with nothing to read, so this test has to give it a
    // reading before it can have a success to notify about.
    let ts = neomind_storage::TimeSeriesStore::memory().expect("ts");
    ts.write(
        "device:dev-1",
        "temperature",
        TsPoint {
            timestamp: chrono::Utc::now().timestamp(),
            value: serde_json::json!(23.5),
            quality: None,
            metadata: None,
        },
    )
    .await
    .expect("seed telemetry");
    ts.flush().expect("flush");
    let config = AgentExecutorConfig {
        store: store.clone(),
        time_series_storage: Some(ts),
        device_service: None,
        event_bus: None,
        message_manager: Some(message_manager.clone()),
        llm_runtime: None,
        llm_backend_store: None,
        extension_registry: None,
        tool_registry: None,
        memory_store: None,
        backend_semaphores: None,
        skill_registry: None,
        execution_semaphore: None,
    };
    let mut executor = AgentExecutor::new(config).await.expect("executor");
    let agent: AiAgent = serde_json::from_value(serde_json::json!({
        "id": "always-agent",
        "name": "盯守测试",
        "user_prompt": "判断状态",
        "resources": [
            { "resource_type": "metric", "resource_id": "dev-1:temperature",
              "name": "temperature", "config": {} }
        ],
        "schedule": { "schedule_type": "manual" },
        "status": "active",
        "created_at": 0, "updated_at": 0,
        "execution_mode": "structured",
        "output_schema": [
            { "name": "status", "field_type": { "type": "enum", "values": ["正常", "异常"] } }
        ],
        "stats": {
            "total_executions": 0, "successful_executions": 0,
            "failed_executions": 0, "avg_duration_ms": 0, "last_duration_ms": null
        },
        "memory": {},
        "notify": { "channels": ["webhook:ops"], "on": "always" },
    }))
    .expect("fixture");
    store.save_agent(&agent).await.expect("seed");
    executor
        .set_llm_runtime(Arc::new(MockLlmRuntime::new(vec![MockResponse::text(
            r#"{"status": "正常"}"#,
        )])))
        .await;

    executor
        .execute_agent(agent, None, None)
        .await
        .expect("run ok");

    let messages = message_manager.list_messages().await;
    let hit = messages
        .iter()
        .find(|m| m.source == "agent:always-agent")
        .expect("success must notify under on=always");
    assert!(hit.title.contains("completed"), "title: {}", hit.title);
    assert!(
        hit.message.contains("正常"),
        "body carries the verdict: {}",
        hit.message
    );
}

/// `judgment` hands the decision to the agent: the machinery sends nothing,
/// and must not fall back to sniffing the conclusion for keywords either.
///
/// That second half is the point. A silent run is the expected outcome here,
/// and a "run completed" message would be a second voice saying something the
/// operator chose this setting to stop hearing. The agent's own judgement
/// reaches them through `neomind message send`, which this test does not
/// exercise — it only asserts the machinery keeps quiet.
#[tokio::test]
async fn notify_on_judgment_keeps_the_machinery_silent() {
    use neomind_storage::timeseries::DataPoint as TsPoint;

    let store = AgentStore::memory().expect("store");
    let message_manager = Arc::new(neomind_messages::MessageManager::new());
    let ts = neomind_storage::TimeSeriesStore::memory().expect("ts");
    ts.write(
        "device:dev-1",
        "temperature",
        TsPoint {
            timestamp: chrono::Utc::now().timestamp(),
            value: serde_json::json!(23.5),
            quality: None,
            metadata: None,
        },
    )
    .await
    .expect("seed telemetry");
    ts.flush().expect("flush");

    let config = AgentExecutorConfig {
        store: store.clone(),
        time_series_storage: Some(ts),
        device_service: None,
        event_bus: None,
        message_manager: Some(message_manager.clone()),
        llm_runtime: None,
        llm_backend_store: None,
        extension_registry: None,
        tool_registry: None,
        memory_store: None,
        backend_semaphores: None,
        skill_registry: None,
        execution_semaphore: None,
    };
    let mut executor = AgentExecutor::new(config).await.expect("executor");

    // The verdict is deliberately alarming: under the old keyword sniffing a
    // conclusion like this would have been picked up and sent.
    let agent: AiAgent = serde_json::from_value(serde_json::json!({
        "id": "judgment-agent",
        "name": "判断档",
        "user_prompt": "判断状态",
        "resources": [
            { "resource_type": "metric", "resource_id": "dev-1:temperature",
              "name": "temperature", "config": {} }
        ],
        "schedule": { "schedule_type": "manual" },
        "status": "active",
        "created_at": 0, "updated_at": 0,
        "execution_mode": "structured",
        "output_schema": [
            { "name": "status", "field_type": { "type": "enum", "values": ["正常", "异常"] } }
        ],
        "stats": {
            "total_executions": 0, "successful_executions": 0,
            "failed_executions": 0, "avg_duration_ms": 0, "last_duration_ms": null
        },
        "memory": {},
        "notify": { "channels": ["webhook:ops"], "on": "judgment" },
    }))
    .expect("fixture");
    store.save_agent(&agent).await.expect("seed");
    executor
        .set_llm_runtime(Arc::new(MockLlmRuntime::new(vec![MockResponse::text(
            r#"{"status": "异常"}"#,
        )])))
        .await;

    executor
        .execute_agent(agent, None, None)
        .await
        .expect("run ok");

    let messages = message_manager.list_messages().await;
    let from_agent: Vec<_> = messages
        .iter()
        .filter(|m| m.source == "agent:judgment-agent")
        .collect();
    assert!(
        from_agent.is_empty(),
        "the machinery must send nothing under on=judgment, got: {:?}",
        from_agent.iter().map(|m| &m.title).collect::<Vec<_>>()
    );
}

/// An alert is the *record* of what happened; channels are only how it
/// travels. The rule path already works that way — `create_message` stores
/// unconditionally and fans out best-effort ("don't fail if channels fail -
/// message is already stored").
///
/// This path returned early when no channel was configured, so switching
/// notifications on with nowhere to push them produced nothing at all: no
/// external delivery *and* no in-app entry. Turning notifications on should
/// never mean "tell nobody".
#[tokio::test]
async fn an_agent_without_channels_still_records_its_alert_in_app() {
    let store = AgentStore::memory().expect("store");
    let message_manager = Arc::new(neomind_messages::MessageManager::new());
    let config = AgentExecutorConfig {
        store: store.clone(),
        time_series_storage: None,
        device_service: None,
        event_bus: None,
        message_manager: Some(message_manager.clone()),
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

    let agent: AiAgent = serde_json::from_value(serde_json::json!({
        "id": "no-channel-agent",
        "name": "盯守",
        "user_prompt": "判断状态",
        "resources": [],
        "schedule": { "schedule_type": "manual" },
        "status": "active",
        "created_at": 0, "updated_at": 0,
        "stats": {
            "total_executions": 0, "successful_executions": 0,
            "failed_executions": 0, "avg_duration_ms": 0, "last_duration_ms": null
        },
        "memory": {},
        "notify": { "channels": [], "on": "always" },
    }))
    .expect("fixture");
    store.save_agent(&agent).await.expect("seed");

    let record = neomind_storage::AgentExecutionRecord {
        id: "exec-no-channel".to_string(),
        agent_id: agent.id.clone(),
        timestamp: 0,
        trigger_type: "schedule".to_string(),
        status: neomind_storage::ExecutionStatus::Completed,
        decision_process: neomind_storage::DecisionProcess {
            situation_analysis: String::new(),
            data_collected: vec![],
            reasoning_steps: vec![],
            decisions: vec![],
            conclusion: "冷库温度正常".to_string(),
            confidence: None,
            stop_reason: String::new(),
            tokens_used: 0,
        },
        result: None,
        duration_ms: 12,
        error: None,
        input_summary: None,
    };

    executor
        .dispatch_agent_notifications(&agent.id, &agent.name, &record, false)
        .await;

    let sent = message_manager.list_messages().await;
    assert_eq!(
        sent.len(),
        1,
        "the alert must exist even when there is nowhere to push it"
    );
    assert_eq!(sent[0].source, "agent:no-channel-agent");
    assert!(sent[0].message.contains("冷库温度正常"));
    assert_eq!(
        sent[0].target_channels,
        Some(Vec::new()),
        "and it must be addressed to nobody externally, not broadcast"
    );
}

/// A **failed** run reaches the user even from an agent with no routing.
///
/// `notify: null` is the shape every agent created before the create-time floor
/// has. Its fallback is the keyword path, which is driven by `Decision`s — and a
/// failure produces none: the record the executor writes for one carries
/// `decisions: vec![]`. So those agents failed silently, run after run, with
/// nothing anywhere saying so. Found in a live log: an agent bound to 31 sources
/// failing every five minutes, no notification.
///
/// A failure is a status, not a judgement about the content, so it takes the
/// floor a new agent would have been given. Success keeps the legacy behaviour.
#[tokio::test]
async fn a_failed_run_reports_even_without_routing() {
    let store = AgentStore::memory().expect("store");
    let message_manager = Arc::new(neomind_messages::MessageManager::new());
    let config = AgentExecutorConfig {
        store: store.clone(),
        time_series_storage: None,
        device_service: None,
        event_bus: None,
        message_manager: Some(message_manager.clone()),
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

    // No `notify` key at all — the pre-floor shape.
    let agent: AiAgent = serde_json::from_value(serde_json::json!({
        "id": "legacy-agent",
        "name": "数据监控分析",
        "user_prompt": "判断状态",
        "resources": [],
        "schedule": { "schedule_type": "interval", "interval_seconds": 300 },
        "status": "active",
        "created_at": 0, "updated_at": 0,
        "stats": {
            "total_executions": 0, "successful_executions": 0,
            "failed_executions": 0, "avg_duration_ms": 0, "last_duration_ms": null
        },
        "memory": {},
    }))
    .expect("fixture");
    assert!(agent.notify.is_none(), "the point of the fixture");
    store.save_agent(&agent).await.expect("seed");

    let record = |status: neomind_storage::ExecutionStatus,
                  conclusion: &str,
                  error: Option<&str>| neomind_storage::AgentExecutionRecord {
        id: format!("exec-{status:?}"),
        agent_id: agent.id.clone(),
        timestamp: 0,
        trigger_type: "schedule".to_string(),
        status,
        decision_process: neomind_storage::DecisionProcess {
            situation_analysis: String::new(),
            data_collected: vec![],
            reasoning_steps: vec![],
            decisions: vec![],
            conclusion: conclusion.to_string(),
            confidence: None,
            stop_reason: String::new(),
            tokens_used: 0,
        },
        result: None,
        duration_ms: 12,
        error: error.map(str::to_string),
        input_summary: None,
    };

    // A failure: one message, and it carries the reason.
    executor
        .dispatch_agent_notifications(
            &agent.id,
            &agent.name,
            &record(
                neomind_storage::ExecutionStatus::Failed,
                "Failed: nothing to work from",
                Some("agent '数据监控分析' has nothing to work from. All 5 devices and 31 metrics are silent"),
            ),
            false,
        )
        .await;

    let sent = message_manager.list_messages().await;
    assert_eq!(sent.len(), 1, "a failure must not be silent");
    assert!(
        sent[0].message.contains("31 metrics are silent"),
        "the reason is the useful part of a failure notice: {:?}",
        sent[0].message
    );

    // A success: the legacy keyword path still decides, and here it says nothing.
    executor
        .dispatch_agent_notifications(
            &agent.id,
            &agent.name,
            &record(
                neomind_storage::ExecutionStatus::Completed,
                "冷库温度正常",
                None,
            ),
            false,
        )
        .await;
    assert_eq!(
        message_manager.list_messages().await.len(),
        1,
        "a successful run of a legacy agent stays quiet — that behaviour is not ours to change"
    );
}

/// Explicit notification routing (2026-09-23): a failed run with
/// notify={channels, on: failure} must land a message targeted at exactly
/// those channels; a passing run must stay silent for on: failure.
#[tokio::test]
async fn notify_routes_failures_to_configured_channels() {
    let store = AgentStore::memory().expect("store");
    let ts = neomind_storage::TimeSeriesStore::memory().expect("ts");
    let message_manager = Arc::new(neomind_messages::MessageManager::new());
    let config = AgentExecutorConfig {
        store: store.clone(),
        time_series_storage: Some(ts),
        device_service: None,
        event_bus: None,
        message_manager: Some(message_manager.clone()),
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

    // Agent whose runs fail (no output schema → structured rejection).
    let mut agent: AiAgent = serde_json::from_value(serde_json::json!({
        "id": "notify-agent",
        "name": "通知测试",
        "user_prompt": "p",
        "resources": [],
        "schedule": { "schedule_type": "manual" },
        "status": "active",
        "created_at": 0,
        "updated_at": 0,
        "execution_mode": "structured",
        "stats": {
            "total_executions": 0, "successful_executions": 0,
            "failed_executions": 0, "avg_duration_ms": 0, "last_duration_ms": null
        },
        "memory": {},
        "notify": { "channels": ["webhook:ops", "telegram:main"], "on": "failure" },
    }))
    .expect("fixture");
    agent.output_schema = None; // force the failure
    store.save_agent(&agent).await.expect("seed");

    executor
        .execute_agent(agent.clone(), None, None)
        .await
        .expect("returns a failed record");

    // The message landed, targeted at exactly the configured channels.
    let messages = message_manager.list_messages().await;
    let hit = messages
        .iter()
        .find(|m| m.source == "agent:notify-agent")
        .expect("a notification was sent");
    assert_eq!(
        hit.target_channels,
        Some(vec!["webhook:ops".to_string(), "telegram:main".to_string()]),
        "routing must be explicit, not broadcast"
    );
    assert!(hit.title.contains("failed"), "title: {}", hit.title);
    assert!(hit.message.contains("output_schema"));
}

/// Two fields, one inference, one publish — both must land in telemetry.
/// (A live instance showed the string field stored and the NUMBER field
/// missing from the listing; this test decides which side drops it.)
#[tokio::test]
async fn two_field_contract_publishes_both_metrics() {
    let ts = neomind_storage::TimeSeriesStore::memory().expect("memory timeseries");
    let agent_store = AgentStore::memory().expect("memory agent store");
    let config = AgentExecutorConfig {
        store: agent_store,
        time_series_storage: Some(ts.clone()),
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
    let mut executor = AgentExecutor::new(config).await.expect("executor");
    let agent = AiAgent {
        id: "two-field-agent".into(),
        name: "two".into(),
        description: None,
        user_prompt: "巡检".into(),
        llm_backend_id: None,
        parsed_intent: None,
        resources: vec![],
        schedule: AgentSchedule {
            schedule_type: ScheduleType::Manual,
            interval_seconds: None,
            cron_expression: None,
            timezone: None,
            event_filter: None,
        },
        status: AgentStatus::Active,
        priority: 128,
        created_at: 0,
        updated_at: 0,
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
            updated_at: 0,
        },
        conversation_history: vec![],
        user_messages: vec![],
        conversation_summary: None,
        context_window_size: 5,
        tool_config: None,
        execution_mode: ExecutionMode::Structured,
        error_message: None,
        system_prompt: None,
        max_retries: 0,
        consecutive_failures: 0,
        output_schema: Some(vec![
            neomind_storage::OperatorField {
                name: "anomaly_count".into(),
                field_type: neomind_storage::OperatorFieldType::Number,
                unit: None,
                description: None,
            },
            neomind_storage::OperatorField {
                name: "status".into(),
                field_type: neomind_storage::OperatorFieldType::Enum(vec![
                    "正常".into(),
                    "待检".into(),
                ]),
                unit: None,
                description: None,
            },
        ]),
        operator_config: None,
        memory_mode: None,
        notify: None,
        enable_tool_chaining: false,
        max_chain_depth: 3,
        max_tokens_per_run: None,
    };
    executor.store().save_agent(&agent).await.expect("seed");

    let rt: Arc<dyn LlmRuntime> = Arc::new(MockLlmRuntime::new(vec![MockResponse::text(
        r#"{"anomaly_count": 0, "status": "待检"}"#,
    )]));
    executor.set_llm_runtime(rt).await;

    // Since 2026-09-23 a structured run with no observation is refused, so the
    // subject of this test — that BOTH schema fields land in telemetry — needs
    // something to infer from.
    let (_dp, record) = executor
        .execute_structured(
            "exec-two-field",
            &agent,
            vec![neomind_storage::DataCollected {
                source: "dev-1".into(),
                data_type: "values.parts".into(),
                values: serde_json::json!({"parts": 3}),
                timestamp: 0,
            }],
        )
        .await
        .expect("run");
    assert!(record.success_rate >= 1.0);

    ts.flush().expect("flush buffer");

    let ns = "ai:two-field-agent";
    let num = ts.query_latest(ns, "anomaly_count").await.expect("num");
    let txt = ts.query_latest(ns, "status").await.expect("txt");
    assert!(
        num.is_some(),
        "the NUMBER field must be stored — live instance showed it missing"
    );
    assert_eq!(num.unwrap().value, serde_json::json!(0));
    assert!(txt.is_some());
    assert_eq!(txt.unwrap().value, serde_json::json!("待检"));
}

/// S1 closed: a structured agent with an image input attaches the pixels as a
/// multimodal part — the base64 never enters the prompt text (where the char
/// cap truncated it into garbage the model rightly refused to read).
#[tokio::test]
async fn structured_inference_sees_the_image_as_a_part() {
    let (mut executor, mut agent, _registry) = build_harness().await;
    agent.execution_mode = neomind_storage::agents::ExecutionMode::Structured;
    agent.output_schema = Some(vec![neomind_storage::OperatorField {
        name: "clutter_level".into(),
        field_type: neomind_storage::OperatorFieldType::Enum(vec![
            "整洁".into(),
            "一般".into(),
            "混乱".into(),
        ]),
        unit: None,
        description: None,
    }]);
    let rt = MockLlmRuntime::new(vec![MockResponse::text(r#"{"clutter_level": "一般"}"#)]);
    let rt_handle = rt.clone();
    executor.set_llm_runtime(Arc::new(rt)).await;

    let image_item = neomind_storage::DataCollected {
        source: "cam-01:values.image".into(),
        data_type: "values.image".into(),
        values: serde_json::json!({
            "_is_image": true,
            "_is_event_data": true,
            "image_base64": "aGVsbG8gd29ybGQgaW1hZ2UgYnl0ZXM=",
            "image_mime_type": "image/jpeg",
        }),
        timestamp: 0,
    };
    let (decision, record) = executor
        .execute_structured("exec-s1-image", &agent, vec![image_item])
        .await
        .expect("structured run with an image succeeds");

    assert!(record.success_rate >= 1.0, "record: {:?}", record.summary);
    assert!(decision.conclusion.contains("一般"));

    // The part, not the text: the message the model received must carry the
    // image, and the text payload must NOT contain the base64.
    let captured = rt_handle.captured_messages();
    assert_eq!(captured.len(), 1, "one inference call");
    let dump = captured[0].to_string();
    assert!(
        dump.contains("image_base64"),
        "the model must receive the image as a part: {dump}"
    );
    assert!(
        !dump.contains("aGVsbG8gd29ybGQgaW1hZ2UgYnl0ZXM=")
            || dump.matches("aGVsbG8gd29ybGQgaW1hZ2UgYnl0ZXM=").count() == 1,
        "base64 must appear exactly once (the part), never also as prompt text"
    );
    assert!(
        dump.contains("attached as image part"),
        "the text says where the image went instead of dumping bytes"
    );
}

/// M2-2: a *reasoning* agent (not structured) that declares an output contract
/// gets its conclusion rendered as schema fields, published as ai:* sources.
#[tokio::test]
async fn output_contract_publishes_fields_for_a_reasoning_agent() {
    let (mut executor, mut agent, _registry) = build_harness().await;
    agent.execution_mode = ExecutionMode::Focused;
    agent.output_schema = Some(vec![neomind_storage::OperatorField {
        name: "missing_count".into(),
        field_type: neomind_storage::OperatorFieldType::Number,
        unit: None,
        description: None,
    }]);
    let rt: Arc<dyn LlmRuntime> = Arc::new(MockLlmRuntime::new(vec![MockResponse::text(
        r#"{"missing_count": 1}"#,
    )]));
    executor.set_llm_runtime(rt).await;

    let published = executor
        .apply_output_contract(&agent, "exec-contract", "画面里有 1 件漏装，批次待检")
        .await;

    assert_eq!(published, Some(1));
}

/// The contract is best-effort: the run already succeeded, so an extraction the
/// model botches costs fields — never the run.
#[tokio::test]
async fn output_contract_failure_does_not_propagate() {
    let (mut executor, mut agent, _registry) = build_harness().await;
    agent.execution_mode = ExecutionMode::Focused;
    agent.output_schema = Some(vec![neomind_storage::OperatorField {
        name: "missing_count".into(),
        field_type: neomind_storage::OperatorFieldType::Number,
        unit: None,
        description: None,
    }]);
    let rt: Arc<dyn LlmRuntime> = Arc::new(MockLlmRuntime::new(vec![MockResponse::text(
        "画面太模糊了，我判断不出来",
    )]));
    executor.set_llm_runtime(rt).await;

    let published = executor
        .apply_output_contract(&agent, "exec-contract", "画面模糊")
        .await;

    assert_eq!(
        published, None,
        "a botched extraction must not fail the run"
    );
}

/// Structured agents already produce their schema directly — running the
/// contract on them would be a second inference for the same answer.
#[tokio::test]
async fn output_contract_is_skipped_for_structured_agents() {
    let (mut executor, mut agent, _registry) = build_harness().await;
    agent.execution_mode = ExecutionMode::Structured;
    agent.output_schema = Some(vec![neomind_storage::OperatorField {
        name: "missing_count".into(),
        field_type: neomind_storage::OperatorFieldType::Number,
        unit: None,
        description: None,
    }]);
    let rt: Arc<dyn LlmRuntime> = Arc::new(MockLlmRuntime::new(vec![MockResponse::text(
        r#"{"missing_count": 1}"#,
    )]));
    executor.set_llm_runtime(rt).await;

    assert_eq!(
        executor
            .apply_output_contract(&agent, "exec-contract", "any conclusion")
            .await,
        None
    );
}

/// Most agents declare no contract at all — the step must be free for them.
#[tokio::test]
async fn output_contract_is_skipped_without_a_schema() {
    let (mut executor, agent, _registry) = build_harness().await;
    let rt: Arc<dyn LlmRuntime> = Arc::new(MockLlmRuntime::new(vec![MockResponse::text(
        r#"{"missing_count": 1}"#,
    )]));
    executor.set_llm_runtime(rt).await;

    assert_eq!(
        executor
            .apply_output_contract(&agent, "exec-contract", "any conclusion")
            .await,
        None
    );
}

#[tokio::test]
async fn structured_mode_without_schema_is_rejected() {
    let (executor, mut agent, _registry) = build_harness().await;
    agent.execution_mode = neomind_storage::agents::ExecutionMode::Structured;
    agent.output_schema = None;

    let err = executor
        .execute_structured("exec-structured-2", &agent, vec![])
        .await
        .expect_err("missing schema must fail");
    assert!(err.to_string().contains("output_schema"), "err: {}", err);
}

/// The cap limits how often the agent may RUN, so it has to bite in every mode.
/// This drives the run entry point for a *focused* agent — the branch that used
/// to be uncapped — and expects the second run of the day to be refused.
#[tokio::test]
async fn daily_run_cap_applies_to_every_mode() {
    let (mut executor, mut agent, registry) = build_harness().await;
    executor.set_tool_registry(registry);
    agent.execution_mode = ExecutionMode::Focused;
    agent.parsed_intent = Some(neomind_storage::ParsedIntent {
        intent_type: neomind_storage::IntentType::Monitoring,
        target_metrics: vec![],
        conditions: vec![],
        actions: vec![],
        confidence: 0.9,
    });
    agent.operator_config = Some(neomind_storage::OperatorConfig {
        debounce_secs: 30,
        max_calls_per_day: Some(1),
        timeout_secs: 60,
        consecutive_failure_threshold: 3,
        event_cooldown_secs: None,
    });
    let rt: Arc<dyn LlmRuntime> = Arc::new(
        MockLlmRuntime::new(vec![
            MockResponse::text("nothing unusual"),
            MockResponse::text("nothing unusual"),
        ])
        .with_function_calling(),
    );
    executor.set_llm_runtime(rt).await;
    executor
        .store()
        .save_agent(&agent)
        .await
        .expect("seed store");

    // First run of the day: allowed.
    let first = executor.execute_agent(agent.clone(), None, None).await;
    assert!(first.is_ok(), "first run within cap: {:?}", first.err());

    // Second run the same day: refused at the run entry point. The executor
    // reports failures as a failed RECORD rather than an Err, so assert there.
    let second = executor
        .execute_agent(agent, None, None)
        .await
        .expect("returns a record");
    assert_eq!(second.status, neomind_storage::ExecutionStatus::Failed);
    assert!(
        second
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("daily run cap"),
        "second run must be refused with the cap message; got {:?}",
        second.error
    );
}

/// Narration collapse: round 1 PROMISES a tool action in words ("我现在使用
/// echo 工具…") without emitting a call. Without the guard the run would end
/// with that promise as the conclusion (the original field failure); with the
/// guard, one nudge round recovers the call and the task completes.
#[tokio::test]
async fn narration_collapse_nudges_once_and_recovers() {
    let rt = MockLlmRuntime::new(vec![
        MockResponse::text("好的，我现在使用 echo 工具来处理。"),
        MockResponse::tool_call("echo", serde_json::json!({ "msg": "recovered" })),
        MockResponse::text("done after recovery"),
    ]);
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt.clone());
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
            "exec-narration",
            30,
            &tool_name_map,
            None,
        )
        .await;

    assert_eq!(
        out.final_text, "done after recovery",
        "the narration promise must NOT be the run's conclusion"
    );
    assert_eq!(rt.call_count(), 3, "nudge round + tool round + final round");
    assert_eq!(out.stop_reason, StopReason::NaturalCompletion);
}

/// The guard fires AT MOST ONCE: if the model narrates again after the
/// nudge, the second narration is accepted as the final answer — no loop.
#[tokio::test]
async fn repeated_narration_is_accepted_as_final() {
    let rt = MockLlmRuntime::new(vec![
        MockResponse::text("好的，我现在使用 echo 工具来处理。"),
        MockResponse::text("我将使用 echo 工具处理这件事。"),
    ]);
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt.clone());
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
            "exec-narration-2",
            30,
            &tool_name_map,
            None,
        )
        .await;

    assert_eq!(
        out.final_text, "我将使用 echo 工具处理这件事。",
        "second narration is the accepted final answer"
    );
    assert_eq!(rt.call_count(), 2, "exactly one nudge — no loop");
}

// ── Multimodal vision false-success (NM-331 / T4 Dongpingshan) ─────────────

struct StubVisionTool;

#[async_trait]
impl Tool for StubVisionTool {
    fn name(&self) -> &str {
        "vision"
    }
    fn description(&self) -> &str {
        "analyze an image"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "image": { "type": "string" },
                "prompt": { "type": "string" }
            }
        })
    }
    async fn execute(&self, _args: serde_json::Value) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::success("vision ok"))
    }
}

/// Vision tool that always soft-fails (success=false) — drives Partial path.
struct FailingVisionTool;

#[async_trait]
impl Tool for FailingVisionTool {
    fn name(&self) -> &str {
        "vision"
    }
    fn description(&self) -> &str {
        "analyze an image (always fails)"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "image": { "type": "string" },
                "prompt": { "type": "string" }
            }
        })
    }
    async fn execute(&self, _args: serde_json::Value) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::error("vision backend timeout"))
    }
}

fn embeddable_image_data() -> neomind_storage::DataCollected {
    neomind_storage::DataCollected {
        source: "device:NE301PIR-200366:image_data".into(),
        data_type: "image".into(),
        values: serde_json::json!({
            "_is_image": true,
            "_is_event_data": true,
            "image_base64": "aGVsbG8gd29ybGQ=",
            "image_mime_type": "image/jpeg",
        }),
        timestamp: 0,
    }
}

/// Positive / NM-388 control: multimodal primary + image → strip vision tool,
/// Completes when the model analyzes the attached image directly (including a
/// "未发现人员" conclusion — ViaVisionTool blind gate must NOT fire here).
#[tokio::test]
async fn multimodal_inline_vision_succeeds_without_vision_tool() {
    let rt = MockLlmRuntime::new(vec![MockResponse::text(
        "未发现人员 — empty corridor, no gambling activity.",
    )])
    .with_function_calling()
    .with_supports_images();
    let rt_handle = rt.clone();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    agent.user_prompt = "用 vision 工具分析图像，疑似赌博再告警".into();
    agent.execution_mode = ExecutionMode::Free;

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool));
    registry.register(Arc::new(StubVisionTool));
    let registry = Arc::new(registry);
    executor.set_tool_registry(registry);
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let (dp, er, _) = executor
        .execute_with_tools(
            &agent,
            &[embeddable_image_data()],
            rt_dyn,
            "exec-inline-vision-ok",
            None,
        )
        .await
        .expect("inline multimodal run must succeed");

    assert!(er.success_rate >= 1.0);
    assert_ne!(
        dp.stop_reason,
        super::vision_path::VISION_TOOLS_FAILED_STOP_REASON,
        "inline multimodal must Complete, not Partial"
    );
    assert!(
        dp.conclusion.contains("未发现人员") || dp.conclusion.contains("empty corridor"),
        "conclusion={}",
        dp.conclusion
    );

    let captured = rt_handle.captured_messages();
    assert!(!captured.is_empty(), "LLM must be called");
    let dump = captured[0].to_string();
    assert!(
        dump.contains("Do NOT call a `vision` tool"),
        "system prompt must rewrite vision guidance: {dump}"
    );
    assert!(
        dump.contains("attached as multimodal content") || dump.contains("analyze directly"),
        "must advertise inline multimodal path: {dump}"
    );
}

/// Negative: multimodal strip + conclusion admits "Vision tool not available"
/// → Partial undetermined markers (not Completed / false-success).
#[tokio::test]
async fn multimodal_conclusion_admitting_vision_failure_is_rejected() {
    let rt = MockLlmRuntime::new(vec![MockResponse::text(
        "Vision tool not available for NE301PIR-200366; cannot analyze image.",
    )])
    .with_function_calling()
    .with_supports_images();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    agent.user_prompt = "用 vision 工具分析图像".into();
    agent.execution_mode = ExecutionMode::Free;

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(StubVisionTool));
    let registry = Arc::new(registry);
    executor.set_tool_registry(registry);
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let (dp, er, _) = executor
        .execute_with_tools(
            &agent,
            &[embeddable_image_data()],
            rt_dyn,
            "exec-inline-vision-fail",
            None,
        )
        .await
        .expect("blind conclusion must return Ok Partial markers, not Err");
    assert_eq!(
        dp.stop_reason,
        super::vision_path::VISION_TOOLS_FAILED_STOP_REASON
    );
    assert!(
        dp.conclusion.starts_with("未判定 / Undetermined"),
        "conclusion: {}",
        dp.conclusion
    );
    assert_eq!(er.success_rate, 0.0);
}

/// Negative: images present, text-only LLM, no vision tool → Unavailable.
#[tokio::test]
async fn vision_unavailable_refuses_completed() {
    let rt =
        MockLlmRuntime::new(vec![MockResponse::text("should not run")]).with_function_calling();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    agent.user_prompt = "analyze camera image".into();
    agent.execution_mode = ExecutionMode::Free;

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool));
    let registry = Arc::new(registry);
    executor.set_tool_registry(registry);
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let err = executor
        .execute_with_tools(
            &agent,
            &[embeddable_image_data()],
            rt_dyn,
            "exec-vision-unavailable",
            None,
        )
        .await
        .expect_err("must refuse when vision is required but unavailable");
    assert!(
        err.to_string().contains("Vision required but unavailable"),
        "got: {err}"
    );
}

/// D2 / PR3: ViaVisionTool + all vision calls fail → ExecutionStatus::Partial,
/// stop_reason=vision_tools_failed, consecutive_failures not reset, Failure
/// notify allowed, Judgment notify silent.
#[tokio::test]
async fn vision_tools_all_failed_becomes_partial_undetermined() {
    let store = AgentStore::memory().expect("store");
    let message_manager = Arc::new(neomind_messages::MessageManager::new());
    let config = AgentExecutorConfig {
        store: store.clone(),
        time_series_storage: None,
        device_service: None,
        event_bus: None,
        message_manager: Some(message_manager.clone()),
        llm_runtime: None,
        llm_backend_store: None,
        extension_registry: None,
        tool_registry: None,
        memory_store: None,
        backend_semaphores: None,
        skill_registry: None,
        execution_semaphore: None,
    };
    let mut executor = AgentExecutor::new(config).await.expect("executor");

    let mut agent: AiAgent = serde_json::from_value(serde_json::json!({
        "id": "vision-partial-agent",
        "name": "看图未判定",
        "user_prompt": "analyze camera image with vision tool; alert on gambling",
        "resources": [],
        "schedule": { "schedule_type": "manual" },
        "status": "active",
        "execution_mode": "free",
        "created_at": 0, "updated_at": 0,
        "stats": {
            "total_executions": 0, "successful_executions": 0,
            "failed_executions": 0, "avg_duration_ms": 0, "last_duration_ms": null
        },
        "memory": {},
        "consecutive_failures": 2,
        "notify": { "channels": [], "on": "failure" },
        "parsed_intent": {
            "intent_type": "monitoring",
            "target_metrics": [],
            "conditions": [],
            "actions": [],
            "confidence": 0.9
        }
    }))
    .expect("fixture");
    agent.execution_mode = ExecutionMode::Free;
    store.save_agent(&agent).await.expect("seed");

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(FailingVisionTool));
    executor.set_tool_registry(Arc::new(registry));

    // Text-only LLM so path is ViaVisionTool (tool kept).
    let rt: Arc<dyn LlmRuntime> = Arc::new(
        MockLlmRuntime::new(vec![
            MockResponse::tool_call(
                "vision",
                serde_json::json!({
                    "image": "data:image/jpeg;base64,aGVsbG8=",
                    "prompt": "describe"
                }),
            ),
            // Model still invents "现场正常" — Partial must neutralize it.
            MockResponse::text("现场正常，无异常。\nVerdict: criteria met"),
        ])
        .with_function_calling(),
    );
    executor.set_llm_runtime(rt).await;

    // No event required: any vision-tool-all-failed run is Partial even when
    // the collected data had no image payload (tool was still attempted).
    let record = executor
        .execute_agent(agent.clone(), None, None)
        .await
        .expect("Partial run must Ok(record)");

    assert_eq!(
        record.status,
        neomind_storage::ExecutionStatus::Partial,
        "error={:?}",
        record.error
    );
    assert_eq!(
        record.decision_process.stop_reason,
        super::vision_path::VISION_TOOLS_FAILED_STOP_REASON
    );
    assert!(
        record
            .decision_process
            .conclusion
            .starts_with("未判定 / Undetermined"),
        "conclusion: {}",
        record.decision_process.conclusion
    );
    assert!(
        !record
            .decision_process
            .conclusion
            .contains("Verdict: criteria met"),
        "Judgment must not see criteria met: {}",
        record.decision_process.conclusion
    );
    assert!(record
        .error
        .as_ref()
        .is_some_and(|e| e.contains("Vision tools failed")));

    let stored = store
        .get_agent(&agent.id)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(
        stored.stats.successful_executions, 0,
        "Partial must not count as success"
    );
    assert_eq!(
        stored.stats.failed_executions, 1,
        "Partial still bumps failed_executions for breaker pacing"
    );
    // consecutive_failures was 2; Partial must NOT reset to 0 (increments).
    assert!(
        stored.consecutive_failures >= 2,
        "consecutive_failures must not reset on Partial, got {}",
        stored.consecutive_failures
    );

    // Failure notify allowed for Partial.
    let sent = message_manager.list_messages().await;
    assert_eq!(sent.len(), 1, "Failure notify must fire for Partial");
    assert!(
        sent[0].title.contains("undetermined") || sent[0].title.contains("failed"),
        "title: {}",
        sent[0].title
    );

    // Judgment notify stays silent even with same Partial record.
    agent.notify = Some(neomind_storage::AgentNotify {
        channels: vec![],
        on: neomind_storage::NotifyOn::Judgment,
    });
    store.save_agent(&agent).await.expect("update notify");
    let before = message_manager.list_messages().await.len();
    executor
        .dispatch_agent_notifications(&agent.id, &agent.name, &record, false)
        .await;
    assert_eq!(
        message_manager.list_messages().await.len(),
        before,
        "Judgment notify must not route for Partial"
    );
}

/// NM-388: ViaVisionTool + images + zero tool calls + "未发现人员" must be
/// Partial (not Completed). `vision_tools_all_failed([])` is false — the
/// end gate must still require a successful vision call.
#[tokio::test]
async fn via_vision_tool_zero_calls_blind_conclusion_is_partial() {
    let rt = MockLlmRuntime::new(vec![MockResponse::text("未发现人员")]).with_function_calling();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    agent.user_prompt = "用 vision 工具分析图像；未发现人员则报告正常".into();
    agent.execution_mode = ExecutionMode::Free;

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(StubVisionTool));
    executor.set_tool_registry(Arc::new(registry));
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let (dp, er, _) = executor
        .execute_with_tools(
            &agent,
            &[embeddable_image_data()],
            rt_dyn,
            "exec-blind-zero-tools",
            None,
        )
        .await
        .expect("Partial path returns Ok");

    assert_eq!(
        dp.stop_reason,
        super::vision_path::VISION_TOOLS_FAILED_STOP_REASON,
        "conclusion={}",
        dp.conclusion
    );
    assert!(
        dp.conclusion.starts_with("未判定 / Undetermined"),
        "conclusion={}",
        dp.conclusion
    );
    assert_eq!(er.success_rate, 0.0);
    assert!(
        er.actions_executed
            .iter()
            .all(|a| !super::vision_path::is_vision_tool_name(&a.target)),
        "fixture must not have called vision: {:?}",
        er.actions_executed
    );
}

/// NM-388: a successful non-vision tool (device-info style) does not satisfy
/// the ViaVisionTool evidence gate — still Partial on "未发现人员".
#[tokio::test]
async fn via_vision_tool_only_non_vision_success_is_partial() {
    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call("echo", serde_json::json!({ "msg": "device_info ok" })),
        MockResponse::text("未发现人员"),
    ])
    .with_function_calling();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    agent.user_prompt = "先读设备信息，再用 vision 看图；未发现人员则报告正常".into();
    agent.execution_mode = ExecutionMode::Free;

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool));
    registry.register(Arc::new(StubVisionTool));
    executor.set_tool_registry(Arc::new(registry));
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let (dp, er, _) = executor
        .execute_with_tools(
            &agent,
            &[embeddable_image_data()],
            rt_dyn,
            "exec-blind-echo-only",
            None,
        )
        .await
        .expect("Partial path returns Ok");

    assert_eq!(
        dp.stop_reason,
        super::vision_path::VISION_TOOLS_FAILED_STOP_REASON
    );
    assert!(
        er.actions_executed
            .iter()
            .any(|a| a.target == "echo" && a.success),
        "echo must have succeeded: {:?}",
        er.actions_executed
    );
    assert!(
        !er.actions_executed
            .iter()
            .any(|a| super::vision_path::is_vision_tool_name(&a.target) && a.success),
        "no successful vision: {:?}",
        er.actions_executed
    );
    assert!(dp.conclusion.starts_with("未判定 / Undetermined"));
}

/// Positive: text LLM + vision tool available → keep vision path (no strip).
#[tokio::test]
async fn text_llm_keeps_vision_tool_path() {
    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call(
            "vision",
            serde_json::json!({
                "image": "data:image/jpeg;base64,aGVsbG8=",
                "prompt": "describe"
            }),
        ),
        MockResponse::text("Frame analyzed via vision tool: empty road."),
    ])
    .with_function_calling();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    agent.user_prompt = "analyze camera image with vision tool".into();
    agent.execution_mode = ExecutionMode::Free;

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(StubVisionTool));
    let registry = Arc::new(registry);
    executor.set_tool_registry(registry);
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let (dp, er, _) = executor
        .execute_with_tools(
            &agent,
            &[embeddable_image_data()],
            rt_dyn,
            "exec-via-vision-tool",
            None,
        )
        .await
        .expect("via-vision-tool path must succeed");

    assert!(er.success_rate >= 1.0, "success_rate={}", er.success_rate);
    assert!(
        dp.conclusion.contains("vision tool") || dp.conclusion.contains("empty road"),
        "conclusion: {}",
        dp.conclusion
    );
    assert!(
        er.actions_executed.iter().any(|a| a.target == "vision"),
        "vision tool must have been executed: {:?}",
        er.actions_executed
    );
}

/// T4 Dongpingshan: free-mode ZH gambling prompt still demands vision after
/// multimodal strip — standing rewrite must follow the Task and forbid the tool.
#[tokio::test]
async fn dongpingshan_free_mode_prompt_rewrite_after_vision_strip() {
    let rt = MockLlmRuntime::new(vec![MockResponse::text(
        "画面为空旷走廊与灌木，未见人员赌博行为。",
    )])
    .with_function_calling()
    .with_supports_images();
    let rt_handle = rt.clone();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    // Mirrors operator prompt shape: demand vision tool + judgment notify.
    agent.name = "东坪山-200366-赌博行为监测".into();
    agent.user_prompt =
        "收到设备 image_data 后，用 vision 工具分析图像；仅当有人且疑似赌博时再 neomind message send 告警"
            .into();
    agent.execution_mode = ExecutionMode::Free;

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool));
    registry.register(Arc::new(StubVisionTool));
    executor.set_tool_registry(Arc::new(registry));
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let (dp, er, _) = executor
        .execute_with_tools(
            &agent,
            &[embeddable_image_data()],
            rt_dyn,
            "exec-dongpingshan-rewrite",
            None,
        )
        .await
        .expect("honest inline analysis must Complete");

    assert!(er.success_rate >= 1.0);
    assert!(
        dp.conclusion.contains("未见") || dp.conclusion.contains("走廊"),
        "conclusion: {}",
        dp.conclusion
    );

    let dump = rt_handle.captured_messages()[0].to_string();
    let task_pos = dump
        .find("用 vision 工具分析图像")
        .expect("original task must remain");
    let rewrite_pos = dump
        .find("Do NOT call a `vision` tool")
        .expect("inline rewrite must be injected");
    assert!(
        rewrite_pos > task_pos,
        "rewrite must follow Task so it overrides the agent prompt"
    );
    assert!(
        dump.contains("## Vision Path (IMPORTANT)"),
        "standing Vision Path section required: {dump}"
    );
    assert!(
        dump.contains("attached as multimodal content"),
        "Free resource note must say images are inline: {dump}"
    );
}

/// T4: ZH honest multimodal analysis → Completed (not false-fail).
#[tokio::test]
async fn dongpingshan_honest_zh_image_analysis_completes() {
    let rt = MockLlmRuntime::new(vec![MockResponse::text(
        "直接分析了附带图像：帐篷与车辆，无赌博迹象。未调用 vision 工具。",
    )])
    .with_function_calling()
    .with_supports_images();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    agent.user_prompt = "用 vision 工具分析东坪山摄像头图像".into();
    agent.execution_mode = ExecutionMode::Free;

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(StubVisionTool));
    executor.set_tool_registry(Arc::new(registry));
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let (dp, er, _) = executor
        .execute_with_tools(
            &agent,
            &[embeddable_image_data()],
            rt_dyn,
            "exec-dongpingshan-honest-zh",
            None,
        )
        .await
        .expect("honest ZH analysis must Complete");

    assert!(er.success_rate >= 1.0);
    assert!(
        !super::vision_path::conclusion_admits_vision_failure(&dp.conclusion),
        "honest analysis must not trip failure markers: {}",
        dp.conclusion
    );
    assert!(dp.conclusion.contains("无赌博"));
}

/// T4 table: ZH/EN conclusions that admit vision failure must Partial, not Completed.
#[tokio::test]
async fn dongpingshan_vision_failure_conclusions_table_fail() {
    let cases = [
        "Vision tool not available for NE301PIR-200366…",
        "视觉不可用，无法分析图像。",
        "Vision extension not installed… skill load skill=\"vision\"",
        "Available domains: device, message, extension; cannot use vision.",
        "视觉工具不可用，改找 extension 失败。",
    ];

    for (i, conclusion) in cases.iter().enumerate() {
        let rt = MockLlmRuntime::new(vec![MockResponse::text((*conclusion).to_string())])
            .with_function_calling()
            .with_supports_images();
        let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

        let (mut executor, mut agent, _) = build_harness().await;
        agent.user_prompt = "用 vision 工具分析图像，疑似赌博再告警".into();
        agent.execution_mode = ExecutionMode::Free;

        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(StubVisionTool));
        executor.set_tool_registry(Arc::new(registry));
        executor.set_llm_runtime(rt_dyn.clone()).await;

        let (dp, er, _) = executor
            .execute_with_tools(
                &agent,
                &[embeddable_image_data()],
                rt_dyn,
                &format!("exec-dongpingshan-fail-{i}"),
                None,
            )
            .await
            .unwrap_or_else(|e| panic!("case {i} ({conclusion}) must Ok Partial, got Err: {e}"));
        assert_eq!(
            dp.stop_reason,
            super::vision_path::VISION_TOOLS_FAILED_STOP_REASON,
            "case {i} ({conclusion})"
        );
        assert_eq!(er.success_rate, 0.0, "case {i}");
        assert!(
            dp.conclusion.starts_with("未判定 / Undetermined"),
            "case {i}: {}",
            dp.conclusion
        );
    }
}

/// T4: text-only LLM + bound camera image + no vision tool → Unavailable Err
/// (ZH gambling agent wording; must not Complete).
#[tokio::test]
async fn dongpingshan_text_llm_no_vision_tool_unavailable() {
    let rt = MockLlmRuntime::new(vec![MockResponse::text(
        "should never run — vision path unavailable",
    )])
    .with_function_calling();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    agent.name = "东坪山-20035E-赌博行为监测".into();
    agent.user_prompt = "用 vision 工具分析 image_data".into();
    agent.execution_mode = ExecutionMode::Free;

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool)); // deliberately no vision
    executor.set_tool_registry(Arc::new(registry));
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let err = executor
        .execute_with_tools(
            &agent,
            &[embeddable_image_data()],
            rt_dyn,
            "exec-dongpingshan-unavailable",
            None,
        )
        .await
        .expect_err("text LLM without vision tool must Unavailable");
    assert!(
        err.to_string().contains("Vision required but unavailable"),
        "got: {err}"
    );
}

// ── R2-P0-1: success_rate self-heal / all-fail / empty-event skip ──────────

/// Soft-fail then succeed: ToolOutput{success:false} then success.
struct ConditionalEchoTool;

#[async_trait]
impl Tool for ConditionalEchoTool {
    fn name(&self) -> &str {
        "echo"
    }
    fn description(&self) -> &str {
        "echo that fails when msg starts with FAIL"
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
        if msg.starts_with("FAIL") {
            Ok(ToolOutput::error(format!("echo failed: {msg}")))
        } else {
            Ok(ToolOutput::success(format!("echo: {msg}")))
        }
    }
}

/// Round 1 tool fails, Round 2 succeeds → Completed + successful_executions==1.
#[tokio::test]
async fn self_heal_partial_success_rate_is_completed() {
    let (mut executor, mut agent, _) = build_harness().await;
    agent.execution_mode = ExecutionMode::Free;
    agent.parsed_intent = Some(neomind_storage::ParsedIntent {
        intent_type: neomind_storage::IntentType::Monitoring,
        target_metrics: vec![],
        conditions: vec![],
        actions: vec![],
        confidence: 0.9,
    });
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(ConditionalEchoTool));
    executor.set_tool_registry(Arc::new(registry));
    executor.store().save_agent(&agent).await.expect("seed");

    let rt: Arc<dyn LlmRuntime> = Arc::new(
        MockLlmRuntime::new(vec![
            MockResponse::tool_call("echo", serde_json::json!({ "msg": "FAIL first" })),
            MockResponse::tool_call("echo", serde_json::json!({ "msg": "ok second" })),
            MockResponse::text("recovered after self-heal"),
        ])
        .with_function_calling(),
    );
    executor.set_llm_runtime(rt).await;

    let record = executor
        .execute_agent(agent.clone(), None, None)
        .await
        .expect("run");
    assert_eq!(record.status, neomind_storage::ExecutionStatus::Completed);
    assert!(record.error.is_none());
    let rate = record
        .result
        .as_ref()
        .map(|r| r.success_rate)
        .unwrap_or(0.0);
    assert!(
        rate > 0.0 && rate < 1.0,
        "expected partial success_rate, got {rate}"
    );

    let stored = executor
        .store()
        .get_agent(&agent.id)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(stored.stats.successful_executions, 1);
    assert_eq!(stored.stats.failed_executions, 0);
    assert_eq!(stored.consecutive_failures, 0);
}

/// All tools fail → Failed + failed_executions==1 + no output_contract publish.
#[tokio::test]
async fn all_tools_failed_is_business_failure_without_output_contract() {
    let (mut executor, mut agent, _) = build_harness().await;
    agent.execution_mode = ExecutionMode::Free;
    agent.parsed_intent = Some(neomind_storage::ParsedIntent {
        intent_type: neomind_storage::IntentType::Monitoring,
        target_metrics: vec![],
        conditions: vec![],
        actions: vec![],
        confidence: 0.9,
    });
    agent.output_schema = Some(vec![neomind_storage::OperatorField {
        name: "status".into(),
        field_type: neomind_storage::OperatorFieldType::Enum(vec!["ok".into(), "bad".into()]),
        unit: None,
        description: None,
    }]);
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(ConditionalEchoTool));
    executor.set_tool_registry(Arc::new(registry));
    executor.store().save_agent(&agent).await.expect("seed");

    let rt: Arc<dyn LlmRuntime> = Arc::new(
        MockLlmRuntime::new(vec![
            MockResponse::tool_call("echo", serde_json::json!({ "msg": "FAIL only" })),
            MockResponse::text(r#"{"status":"ok"}"#),
        ])
        .with_function_calling(),
    );
    executor.set_llm_runtime(rt).await;

    let record = executor
        .execute_agent(agent.clone(), None, None)
        .await
        .expect("run");
    assert_eq!(record.status, neomind_storage::ExecutionStatus::Failed);
    assert!(record.error.is_some());
    assert!(
        !record
            .decision_process
            .decisions
            .iter()
            .any(|d| d.decision_type == "output_contract"),
        "failed run must not publish output_contract"
    );
    let stored = executor
        .store()
        .get_agent(&agent.id)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(stored.stats.failed_executions, 1);
    assert_eq!(stored.stats.successful_executions, 0);
}

// ---------------------------------------------------------------------------
// 1.3.3 Task 1 — Change A (event-image input status) + Change B (judgment gate)
// ---------------------------------------------------------------------------

/// Minimal JPEG SOI + filler + EOI. `pad_byte` distinguishes fixtures that
/// share MIME `image/jpeg` but differ in content (byte-compare control).
fn test_jpeg_bytes(pad_byte: u8) -> Vec<u8> {
    let mut bytes = vec![0xFFu8, 0xD8, 0xFF, 0xE0]; // JPEG SOI + APP0 start
    bytes.extend(std::iter::repeat_n(pad_byte, 2048));
    bytes.extend_from_slice(&[0xFF, 0xD9]); // EOI
    bytes
}

fn test_jpeg_base64(pad_byte: u8) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(test_jpeg_bytes(pad_byte))
}

fn event_jpeg_data(b64: &str) -> neomind_storage::DataCollected {
    neomind_storage::DataCollected {
        source: "200366:image_data".into(),
        data_type: "image_data".into(),
        values: serde_json::json!({
            "_is_image": true,
            "_is_event_data": true,
            "image_base64": b64,
            "image_mime_type": "image/jpeg",
        }),
        timestamp: chrono::Utc::now().timestamp(),
    }
}

/// Walk a MockLlm captured messages JSON value and collect every
/// `image_base64.data` payload (decoded). MIME alone must not pass tests.
fn extract_decoded_images_from_captured(messages: &serde_json::Value) -> Vec<(Vec<u8>, String)> {
    use base64::Engine;
    let mut out = Vec::new();
    let Some(arr) = messages.as_array() else {
        return out;
    };
    for msg in arr {
        let Some(content) = msg.get("content") else {
            continue;
        };
        // Content::Parts serializes as an array of tagged parts, or as an
        // object with a `Parts` / `parts` wrapper depending on serde shape.
        let parts = content
            .as_array()
            .or_else(|| content.get("Parts").and_then(|v| v.as_array()))
            .or_else(|| content.get("parts").and_then(|v| v.as_array()));
        let Some(parts) = parts else {
            continue;
        };
        for part in parts {
            let is_b64 = part.get("type").and_then(|t| t.as_str()) == Some("image_base64")
                || part.get("ImageBase64").is_some();
            if !is_b64 && part.get("data").is_none() {
                continue;
            }
            let data = part
                .get("data")
                .or_else(|| part.pointer("/ImageBase64/data"))
                .and_then(|v| v.as_str());
            let mime = part
                .get("mime_type")
                .or_else(|| part.pointer("/ImageBase64/mime_type"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let Some(data) = data else {
                continue;
            };
            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) {
                out.push((bytes, mime));
            }
        }
    }
    out
}

fn input_status_row() -> neomind_storage::DataCollected {
    neomind_storage::DataCollected {
        source: "system".into(),
        data_type: "input_status".into(),
        values: serde_json::json!({
            "event_image": "provided",
            "bound_metrics": "none",
            "message": "Event-carried image provided this turn; no metrics from bound resources."
        }),
        timestamp: chrono::Utc::now().timestamp(),
    }
}

/// Fake `shell` tool that counts `message send` side effects without hitting
/// the real message API — isolation sink for judgment-notify acceptance.
struct FakeNotifySink {
    sends: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

#[async_trait]
impl Tool for FakeNotifySink {
    fn name(&self) -> &str {
        "shell"
    }
    fn description(&self) -> &str {
        "fake shell / notify sink"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "command": { "type": "string" } },
            "required": ["command"]
        })
    }
    async fn execute(&self, args: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let cmd = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
        if super::judgment_notify::is_message_send_command(cmd) {
            self.sends.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(ToolOutput::success("Platform message created"))
        } else {
            Ok(ToolOutput::success(format!("ok: {cmd}")))
        }
    }
}

/// Change A: known JPEG + resources=[] → model request carries **byte-identical**
/// image (not MIME-only) and prompt distinguishes event image vs bound metrics.
#[tokio::test]
async fn event_image_empty_resources_reaches_model_with_honest_status() {
    let fixture_bytes = test_jpeg_bytes(0x5A);
    let jpeg_b64 = test_jpeg_base64(0x5A);
    let control_bytes = test_jpeg_bytes(0xA5); // same MIME, different content
    assert_ne!(
        fixture_bytes, control_bytes,
        "control fixture must differ in bytes"
    );
    assert_eq!(
        fixture_bytes.len(),
        control_bytes.len(),
        "control keeps size; only pad differs"
    );

    let rt = MockLlmRuntime::new(vec![MockResponse::text(
        "Attached frame inspected; corridor empty.",
    )])
    .with_function_calling()
    .with_supports_images();
    let rt_handle = rt.clone();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    agent.resources.clear();
    agent.user_prompt = "分析事件图像；仅当疑似赌博再告警".into();
    agent.execution_mode = ExecutionMode::Free;

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool));
    executor.set_tool_registry(Arc::new(registry));
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let data = vec![event_jpeg_data(&jpeg_b64), input_status_row()];
    let (_dp, er, _) = executor
        .execute_with_tools(&agent, &data, rt_dyn, "exec-event-image-status", None)
        .await
        .expect("event-image run must succeed");
    assert!(er.success_rate >= 1.0);

    let captured = rt_handle.captured_messages();
    assert!(!captured.is_empty(), "LLM must be called");
    let images = extract_decoded_images_from_captured(&captured[0]);
    assert!(
        !images.is_empty(),
        "must extract image_base64 from multimodal request, not rely on MIME string alone; dump={}",
        &captured[0].to_string()[..captured[0].to_string().len().min(600)]
    );
    let (got_bytes, mime) = &images[0];
    assert_eq!(mime, "image/jpeg");
    assert_eq!(
        got_bytes, &fixture_bytes,
        "model request image bytes must match fixture (byte-compare)"
    );
    assert_ne!(
        got_bytes, &control_bytes,
        "same-MIME control image must not match — proves compare is content-based"
    );

    let dump = captured[0].to_string();
    assert!(
        dump.contains("Event image this turn: provided")
            || dump.contains("event-carried image")
            || dump.contains("Event-carried image provided"),
        "prompt must distinguish event image: {}",
        &dump[..dump.len().min(1200)]
    );
    assert!(
        dump.contains("Bound metrics: none") || dump.contains("no metrics from bound"),
        "prompt must say bound metrics absent: {}",
        &dump[..dump.len().min(1200)]
    );
    assert!(
        !dump.contains("NO DATA COLLECTED"),
        "must not inject NO DATA COLLECTED when event image present"
    );
    assert!(
        !dump.contains("No pre-collected data"),
        "must not claim no pre-collected data when event image present"
    );
}

/// Change B: labeled criteria-not-met → patrol Completes, sink gets 0 sends
/// even if the model tries `message send`.
#[tokio::test]
async fn judgment_gate_blocks_send_when_criteria_not_met() {
    let sends = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call(
            "shell",
            serde_json::json!({
                "command": "neomind message send --title \"告警\" --body \"疑似？\""
            }),
        )
        .with_text("画面为空旷走廊，无赌博行为证据。"),
        MockResponse::text("巡检完成：无赌博，无需告警。"),
    ])
    .with_function_calling()
    .with_supports_images();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    agent.user_prompt = "仅当有人且疑似赌博时再 neomind message send".into();
    agent.execution_mode = ExecutionMode::Free;
    agent.notify = Some(neomind_storage::AgentNotify {
        channels: vec!["webhook:ops".into()],
        on: neomind_storage::NotifyOn::Judgment,
    });

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(FakeNotifySink {
        sends: sends.clone(),
    }));
    executor.set_tool_registry(Arc::new(registry));
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let (_dp, er, _) = executor
        .execute_with_tools(
            &agent,
            &[event_jpeg_data(&test_jpeg_base64(0x5A))],
            rt_dyn,
            "exec-judgment-not-met",
            None,
        )
        .await
        .expect("patrol must Complete");
    assert_eq!(
        sends.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "criteria not met must deliver 0 notifications"
    );
    assert!(
        er.actions_executed.iter().any(|a| {
            !a.success
                && a.result
                    .as_deref()
                    .unwrap_or("")
                    .contains("Judgment notify gate")
        }),
        "blocked send must surface Judgment notify gate in action result; actions={:?}",
        er.actions_executed
    );
}

/// Change B: no independent judgment + self-serving title → sink 0
/// (title/body must not self-approve).
#[tokio::test]
async fn judgment_gate_blocks_title_only_self_approval() {
    let sends = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call(
            "shell",
            serde_json::json!({
                "command": "neomind message send --title \"疑似赌博\" --body \"告警正文\""
            }),
        )
        // Empty / non-judgment assistant text — only the pending title has markers.
        .with_text(""),
        MockResponse::text("done"),
    ])
    .with_function_calling()
    .with_supports_images();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    agent.user_prompt = "赌博监测".into();
    agent.execution_mode = ExecutionMode::Free;
    agent.notify = Some(neomind_storage::AgentNotify {
        channels: vec!["webhook:ops".into()],
        on: neomind_storage::NotifyOn::Judgment,
    });

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(FakeNotifySink {
        sends: sends.clone(),
    }));
    executor.set_tool_registry(Arc::new(registry));
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let _ = executor
        .execute_with_tools(
            &agent,
            &[event_jpeg_data(&test_jpeg_base64(0x5A))],
            rt_dyn,
            "exec-judgment-title-only",
            None,
        )
        .await
        .expect("run completes (send blocked)");
    assert_eq!(
        sends.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "title-only 疑似赌博 must not self-approve a notification"
    );
}

/// Last-line protocol: mid-message backtick example must not authorize
/// → FakeNotifySink receives 0.
#[tokio::test]
async fn judgment_gate_blocks_mid_message_backtick_verdict_example() {
    let sends = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call(
            "shell",
            serde_json::json!({
                "command": "neomind message send --title \"example\" --body \"still analyzing\""
            }),
        )
        .with_text("阳性判定的格式示例如下：\n`Verdict: criteria met`\n本帧还在分析中。"),
        MockResponse::text("done"),
    ])
    .with_function_calling()
    .with_supports_images();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    agent.user_prompt = "仅当有人且疑似赌博时再告警".into();
    agent.execution_mode = ExecutionMode::Free;
    agent.notify = Some(neomind_storage::AgentNotify {
        channels: vec!["webhook:ops".into()],
        on: neomind_storage::NotifyOn::Judgment,
    });

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(FakeNotifySink {
        sends: sends.clone(),
    }));
    executor.set_tool_registry(Arc::new(registry));
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let (_dp, er, _) = executor
        .execute_with_tools(
            &agent,
            &[event_jpeg_data(&test_jpeg_base64(0x5A))],
            rt_dyn,
            "exec-judgment-mid-message-example",
            None,
        )
        .await
        .expect("mid-message example Completes with send blocked");
    assert_eq!(
        sends.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "mid-message backtick Verdict example must not authorize (sink 0)"
    );
    assert!(
        er.actions_executed.iter().any(|a| {
            !a.success
                && a.result
                    .as_deref()
                    .unwrap_or("")
                    .contains("Judgment notify gate")
        }),
        "gate must block mid-message example; actions={:?}",
        er.actions_executed
    );
}

/// Protocol parse: explanatory prose mentioning "criteria met" must not
/// authorize → FakeNotifySink receives 0.
#[tokio::test]
async fn judgment_gate_blocks_explanatory_criteria_met_mention() {
    let sends = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call(
            "shell",
            serde_json::json!({
                "command": "neomind message send --title \"probe\" --body \"explanatory\""
            }),
        )
        .with_text("I have not yet determined whether the criteria met label applies."),
        MockResponse::text("done"),
    ])
    .with_function_calling()
    .with_supports_images();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    agent.user_prompt = "仅当有人且疑似赌博时再告警".into();
    agent.execution_mode = ExecutionMode::Free;
    agent.notify = Some(neomind_storage::AgentNotify {
        channels: vec!["webhook:ops".into()],
        on: neomind_storage::NotifyOn::Judgment,
    });

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(FakeNotifySink {
        sends: sends.clone(),
    }));
    executor.set_tool_registry(Arc::new(registry));
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let (_dp, er, _) = executor
        .execute_with_tools(
            &agent,
            &[event_jpeg_data(&test_jpeg_base64(0x5A))],
            rt_dyn,
            "exec-judgment-explanatory-mention",
            None,
        )
        .await
        .expect("explanatory-mention run Completes with send blocked");
    assert_eq!(
        sends.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "explanatory 'criteria met' mention must not authorize (sink 0)"
    );
    assert!(
        er.actions_executed.iter().any(|a| {
            !a.success
                && a.result
                    .as_deref()
                    .unwrap_or("")
                    .contains("Judgment notify gate")
        }),
        "gate must block explanatory mention; actions={:?}",
        er.actions_executed
    );
}

/// Structural auth: open question ("需要告警吗？") is not a finished judgment
/// → FakeNotifySink receives 0 (lexicon substring must not authorize).
#[tokio::test]
async fn judgment_gate_blocks_question_need_alert_without_verdict() {
    let sends = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call(
            "shell",
            serde_json::json!({
                "command": "neomind message send --title \"疑问\" --body \"question only\""
            }),
        )
        .with_text("需要告警吗？"),
        MockResponse::text("done"),
    ])
    .with_function_calling()
    .with_supports_images();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    agent.user_prompt = "仅当有人且疑似赌博时再告警".into();
    agent.execution_mode = ExecutionMode::Free;
    agent.notify = Some(neomind_storage::AgentNotify {
        channels: vec!["webhook:ops".into()],
        on: neomind_storage::NotifyOn::Judgment,
    });

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(FakeNotifySink {
        sends: sends.clone(),
    }));
    executor.set_tool_registry(Arc::new(registry));
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let (_dp, er, _) = executor
        .execute_with_tools(
            &agent,
            &[event_jpeg_data(&test_jpeg_base64(0x5A))],
            rt_dyn,
            "exec-judgment-question-only",
            None,
        )
        .await
        .expect("question-only run Completes with send blocked");
    assert_eq!(
        sends.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "question 需要告警吗？ must not authorize (sink 0)"
    );
    assert!(
        er.actions_executed.iter().any(|a| {
            !a.success
                && a.result
                    .as_deref()
                    .unwrap_or("")
                    .contains("Judgment notify gate")
        }),
        "gate must block question-only send; actions={:?}",
        er.actions_executed
    );
}

/// Source-review #3: progress / intent ("我先核对画面是否需要告警") is not a
/// finished this-event judgment → FakeNotifySink receives 0.
#[tokio::test]
async fn judgment_gate_blocks_progress_intent_without_verdict() {
    let sends = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call(
            "shell",
            serde_json::json!({
                "command": "neomind message send --title \"核对中\" --body \"progress\""
            }),
        )
        .with_text("我先核对画面是否需要告警"),
        MockResponse::text("done"),
    ])
    .with_function_calling()
    .with_supports_images();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    agent.user_prompt = "仅当有人且疑似赌博时再告警".into();
    agent.execution_mode = ExecutionMode::Free;
    agent.notify = Some(neomind_storage::AgentNotify {
        channels: vec!["webhook:ops".into()],
        on: neomind_storage::NotifyOn::Judgment,
    });

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(FakeNotifySink {
        sends: sends.clone(),
    }));
    executor.set_tool_registry(Arc::new(registry));
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let (_dp, er, _) = executor
        .execute_with_tools(
            &agent,
            &[event_jpeg_data(&test_jpeg_base64(0x5A))],
            rt_dyn,
            "exec-judgment-progress-only",
            None,
        )
        .await
        .expect("progress-only run Completes with send blocked");
    assert_eq!(
        sends.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "progress intent must not count as finished judgment (sink 0)"
    );
    assert!(
        er.actions_executed.iter().any(|a| {
            !a.success
                && a.result
                    .as_deref()
                    .unwrap_or("")
                    .contains("Judgment notify gate")
        }),
        "gate must block progress-only send; actions={:?}",
        er.actions_executed
    );
}

/// Acceptance #4: assistant restates policy ("如果发现赌博，就需要告警") with
/// no this-event judgment → FakeNotifySink receives 0.
#[tokio::test]
async fn judgment_gate_blocks_policy_conditional_without_verdict() {
    let sends = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call(
            "shell",
            serde_json::json!({
                "command": "neomind message send --title \"巡检提醒\" --body \"规则复述\""
            }),
        )
        .with_text("如果发现赌博，就需要告警"),
        MockResponse::text("done"),
    ])
    .with_function_calling()
    .with_supports_images();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    agent.user_prompt = "仅当有人且疑似赌博时再告警".into();
    agent.execution_mode = ExecutionMode::Free;
    agent.notify = Some(neomind_storage::AgentNotify {
        channels: vec!["webhook:ops".into()],
        on: neomind_storage::NotifyOn::Judgment,
    });

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(FakeNotifySink {
        sends: sends.clone(),
    }));
    executor.set_tool_registry(Arc::new(registry));
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let (_dp, er, _) = executor
        .execute_with_tools(
            &agent,
            &[event_jpeg_data(&test_jpeg_base64(0x5A))],
            rt_dyn,
            "exec-judgment-policy-only",
            None,
        )
        .await
        .expect("policy-only run Completes with send blocked");
    assert_eq!(
        sends.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "policy conditional without this-event judgment must deliver 0"
    );
    assert!(
        er.actions_executed.iter().any(|a| {
            !a.success
                && a.result
                    .as_deref()
                    .unwrap_or("")
                    .contains("Judgment notify gate")
        }),
        "gate must block policy-only send; actions={:?}",
        er.actions_executed
    );
}

/// Acceptance #5: prior event inconclusive (dark); this event has a valid
/// positive judgment → deliver 1; old conclusion must not interfere.
#[tokio::test]
async fn judgment_gate_allows_positive_after_prior_inconclusive() {
    let sends = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let rt = MockLlmRuntime::new(vec![
        // Prior-event style conclusion still present in the transcript.
        MockResponse::tool_call(
            "shell",
            serde_json::json!({
                "command": "neomind device get cam-1"
            }),
        )
        .with_text("画面过暗，证据不足，无法判断。")
        .with_backend_text_shape(),
        // This event: clear positive judgment + send.
        MockResponse::tool_call(
            "shell",
            serde_json::json!({
                "command": "neomind message send --title \"Alert\" --body \"details\""
            }),
        )
        .with_text("有人且疑似赌博。\nVerdict: criteria met")
        .with_backend_text_shape(),
        MockResponse::text("已发送告警。"),
    ])
    .with_function_calling()
    .with_supports_images();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    agent.user_prompt = "仅当有人且疑似赌博时再告警".into();
    agent.execution_mode = ExecutionMode::Free;
    agent.notify = Some(neomind_storage::AgentNotify {
        channels: vec!["webhook:ops".into()],
        on: neomind_storage::NotifyOn::Judgment,
    });

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(FakeNotifySink {
        sends: sends.clone(),
    }));
    executor.set_tool_registry(Arc::new(registry));
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let _ = executor
        .execute_with_tools(
            &agent,
            &[event_jpeg_data(&test_jpeg_base64(0x5A))],
            rt_dyn,
            "exec-judgment-prior-dark-this-met",
            None,
        )
        .await
        .expect("this-event positive after prior dark must Complete");
    assert_eq!(
        sends.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "prior inconclusive must not interfere with this event's positive judgment"
    );
}

/// Change B: non-gambling task + clear positive judgment → sink 1.
#[tokio::test]
async fn judgment_gate_allows_non_gambling_positive_judgment() {
    let sends = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call(
            "shell",
            // Neutral title/body — markers live only in assistant judgment.
            serde_json::json!({
                "command": "neomind message send --title \"Smoke detected\" --body \"Zone B sensor trip\""
            }),
        )
        .with_text("Smoke plume confirmed on camera.\nVerdict: criteria met")
        .with_backend_text_shape(),
        MockResponse::text("Alert sent."),
    ])
    .with_function_calling()
    .with_supports_images();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    agent.user_prompt = "Fire / smoke patrol: notify only when smoke is confirmed".into();
    agent.execution_mode = ExecutionMode::Free;
    agent.notify = Some(neomind_storage::AgentNotify {
        channels: vec!["webhook:ops".into()],
        on: neomind_storage::NotifyOn::Judgment,
    });

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(FakeNotifySink {
        sends: sends.clone(),
    }));
    executor.set_tool_registry(Arc::new(registry));
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let _ = executor
        .execute_with_tools(
            &agent,
            &[event_jpeg_data(&test_jpeg_base64(0x5A))],
            rt_dyn,
            "exec-judgment-smoke",
            None,
        )
        .await
        .expect("positive non-gambling judgment must Complete");
    assert_eq!(
        sends.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "clear positive judgment on a non-gambling task must deliver 1 notification"
    );
}

/// Finished this-event protocol `Verdict: criteria met` → sink gets exactly 1.
/// The response carries the text shape every real backend produces (tool-call
/// array appended to the text) — the gate reads the verdict before it.
#[tokio::test]
async fn judgment_gate_allows_send_when_criteria_met() {
    let sends = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call(
            "shell",
            serde_json::json!({
                "command": "neomind message send --title \"Alert\" --body \"details\""
            }),
        )
        .with_text("有人且疑似赌博。\nVerdict: criteria met")
        .with_backend_text_shape(),
        MockResponse::text("已发送告警。"),
    ])
    .with_function_calling()
    .with_supports_images();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    agent.user_prompt = "仅当有人且疑似赌博时再告警".into();
    agent.execution_mode = ExecutionMode::Free;
    agent.notify = Some(neomind_storage::AgentNotify {
        channels: vec!["webhook:ops".into()],
        on: neomind_storage::NotifyOn::Judgment,
    });

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(FakeNotifySink {
        sends: sends.clone(),
    }));
    executor.set_tool_registry(Arc::new(registry));
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let (_dp, _er, _) = executor
        .execute_with_tools(
            &agent,
            &[event_jpeg_data(&test_jpeg_base64(0x5A))],
            rt_dyn,
            "exec-judgment-met",
            None,
        )
        .await
        .expect("criteria-met run must Complete");
    assert_eq!(
        sends.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "criteria met must deliver exactly 1 notification"
    );
}

/// Verdict stated alongside an earlier tool call (backend text shape), then
/// `message send` with EMPTY content — the latest non-empty assistant prose
/// is that verdict → sink 1. Before the fix the appended array hid it.
#[tokio::test]
async fn judgment_gate_allows_empty_content_send_after_backend_shaped_verdict() {
    let sends = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call(
            "shell",
            serde_json::json!({ "command": "neomind device get cam-1" }),
        )
        .with_text("有人且疑似赌博。\nVerdict: criteria met")
        .with_backend_text_shape(),
        MockResponse::tool_call(
            "shell",
            serde_json::json!({
                "command": "neomind message send --title \"Alert\" --body \"details\""
            }),
        )
        .with_text(""),
        MockResponse::text("已发送告警。"),
    ])
    .with_function_calling()
    .with_supports_images();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    agent.user_prompt = "仅当有人且疑似赌博时再告警".into();
    agent.execution_mode = ExecutionMode::Free;
    agent.notify = Some(neomind_storage::AgentNotify {
        channels: vec!["webhook:ops".into()],
        on: neomind_storage::NotifyOn::Judgment,
    });

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(FakeNotifySink {
        sends: sends.clone(),
    }));
    executor.set_tool_registry(Arc::new(registry));
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let _ = executor
        .execute_with_tools(
            &agent,
            &[event_jpeg_data(&test_jpeg_base64(0x5A))],
            rt_dyn,
            "exec-judgment-met-empty-content",
            None,
        )
        .await
        .expect("criteria-met run must Complete");
    assert_eq!(
        sends.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "prior-round verdict must authorize the empty-content send"
    );
}

/// Backend text shape does not loosen the gate: an explicit `criteria not
/// met` before the appended array still blocks → sink 0.
#[tokio::test]
async fn judgment_gate_blocks_backend_shaped_not_met_verdict() {
    let sends = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call(
            "shell",
            serde_json::json!({
                "command": "neomind message send --title \"告警\" --body \"疑似？\""
            }),
        )
        .with_text("画面为空旷走廊，无赌博行为证据。\nVerdict: criteria not met")
        .with_backend_text_shape(),
        MockResponse::text("巡检完成：无赌博，无需告警。"),
    ])
    .with_function_calling()
    .with_supports_images();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    agent.user_prompt = "仅当有人且疑似赌博时再 neomind message send".into();
    agent.execution_mode = ExecutionMode::Free;
    agent.notify = Some(neomind_storage::AgentNotify {
        channels: vec!["webhook:ops".into()],
        on: neomind_storage::NotifyOn::Judgment,
    });

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(FakeNotifySink {
        sends: sends.clone(),
    }));
    executor.set_tool_registry(Arc::new(registry));
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let (_dp, er, _) = executor
        .execute_with_tools(
            &agent,
            &[event_jpeg_data(&test_jpeg_base64(0x5A))],
            rt_dyn,
            "exec-judgment-not-met-backend-shape",
            None,
        )
        .await
        .expect("patrol must Complete");
    assert_eq!(
        sends.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "criteria not met must deliver 0 notifications"
    );
    assert!(
        er.actions_executed.iter().any(|a| {
            !a.success
                && a.result
                    .as_deref()
                    .unwrap_or("")
                    .contains("alert criteria not met")
        }),
        "block reason must cite the not-met verdict; actions={:?}",
        er.actions_executed
    );
}

/// Change B: dark / undecidable → insufficient evidence recorded, 0 alerts.
#[tokio::test]
async fn judgment_gate_blocks_insufficient_evidence() {
    let sends = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call(
            "shell",
            serde_json::json!({
                "command": "neomind message send --title \"dark\" --body \"maybe\""
            }),
        )
        .with_text("画面过暗，证据不足，无法判断是否有赌博。"),
        MockResponse::text("证据不足，本轮不告警。"),
    ])
    .with_function_calling()
    .with_supports_images();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    agent.user_prompt = "赌博监测".into();
    agent.execution_mode = ExecutionMode::Free;
    agent.notify = Some(neomind_storage::AgentNotify {
        channels: vec!["webhook:ops".into()],
        on: neomind_storage::NotifyOn::Judgment,
    });

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(FakeNotifySink {
        sends: sends.clone(),
    }));
    executor.set_tool_registry(Arc::new(registry));
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let (dp, _, _) = executor
        .execute_with_tools(
            &agent,
            &[event_jpeg_data(&test_jpeg_base64(0x5A))],
            rt_dyn,
            "exec-judgment-insufficient",
            None,
        )
        .await
        .expect("insufficient-evidence patrol Completes");
    assert!(
        dp.conclusion.contains("证据不足") || dp.conclusion.contains("不告警"),
        "must record insufficient evidence: {}",
        dp.conclusion
    );
    assert_eq!(
        sends.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "insufficient evidence must deliver 0 alerts"
    );
}

/// Change B: hallucinated CLI-domain `message` tool is rewritten to shell and
/// hits the **same** judgment gate (title-only → 0).
#[tokio::test]
async fn judgment_gate_blocks_rewritten_message_tool_title_only() {
    let sends = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call(
            "message",
            serde_json::json!({
                "title": "疑似赌博",
                "content": "self-approve attempt"
            }),
        )
        .with_text(""),
        MockResponse::text("done"),
    ])
    .with_function_calling()
    .with_supports_images();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    agent.user_prompt = "赌博监测".into();
    agent.execution_mode = ExecutionMode::Free;
    agent.notify = Some(neomind_storage::AgentNotify {
        channels: vec!["webhook:ops".into()],
        on: neomind_storage::NotifyOn::Judgment,
    });

    let mut registry = ToolRegistry::new();
    // Only shell is registered — `message` must be rewritten to shell first,
    // then gated by judgment_deny_for_tool_call (shared entry point).
    registry.register(Arc::new(FakeNotifySink {
        sends: sends.clone(),
    }));
    executor.set_tool_registry(Arc::new(registry));
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let (_dp, er, _) = executor
        .execute_with_tools(
            &agent,
            &[event_jpeg_data(&test_jpeg_base64(0x5A))],
            rt_dyn,
            "exec-judgment-rewritten-message",
            None,
        )
        .await
        .expect("rewritten path Completes with send blocked");
    assert_eq!(
        sends.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "rewritten message→shell must share judgment gate (0 sends)"
    );
    assert!(
        er.actions_executed.iter().any(|a| {
            a.target == "shell"
                && !a.success
                && a.result
                    .as_deref()
                    .unwrap_or("")
                    .contains("Judgment notify gate")
        }),
        "gate must fire on rewritten shell call; actions={:?}",
        er.actions_executed
    );
}

/// Change B: ordinary Always notify keeps old contract (send not gated).
#[tokio::test]
async fn always_notify_agent_send_not_gated() {
    let sends = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call(
            "shell",
            serde_json::json!({
                "command": "neomind message send --title \"note\" --body \"无赌博但仍通知\""
            }),
        )
        .with_text("无赌博，但 Always 路径仍可发送。"),
        MockResponse::text("done"),
    ])
    .with_function_calling()
    .with_supports_images();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    agent.execution_mode = ExecutionMode::Free;
    agent.notify = Some(neomind_storage::AgentNotify {
        channels: vec!["webhook:ops".into()],
        on: neomind_storage::NotifyOn::Always,
    });

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(FakeNotifySink {
        sends: sends.clone(),
    }));
    executor.set_tool_registry(Arc::new(registry));
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let _ = executor
        .execute_with_tools(
            &agent,
            &[event_jpeg_data(&test_jpeg_base64(0x5A))],
            rt_dyn,
            "exec-always-ungated",
            None,
        )
        .await
        .expect("Always notify run");
    assert_eq!(
        sends.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "Always notify must keep the old send contract"
    );
}

/// Empty-event image extraction skip → Completed, no stats bump, no breaker.
#[tokio::test]
async fn empty_event_skip_does_not_trip_breaker_or_stats() {
    let (executor, mut agent, _) = build_harness().await;
    agent.execution_mode = ExecutionMode::Free;
    agent.schedule.schedule_type = ScheduleType::Event;
    agent.consecutive_failures = 1; // pre-existing strike must stay
    agent.parsed_intent = Some(neomind_storage::ParsedIntent {
        intent_type: neomind_storage::IntentType::Monitoring,
        target_metrics: vec![],
        conditions: vec![],
        actions: vec![],
        confidence: 0.9,
    });
    executor.store().save_agent(&agent).await.expect("seed");

    // A value that carries an image nobody can read (no frame behind the
    // path) → the run has nothing to look at → empty skip. (A field that is
    // merely image-NAMED, with a plain value, no longer skips: see
    // `image_like_name_with_a_plain_value_is_text_not_a_skip`.)
    let event = super::context::EventTriggerData {
        source: super::context::DataSourceRef {
            source_type: "device".into(),
            source_id: "cam-1".into(),
            field: "image_data".into(),
        },
        value: neomind_core::event::MetricValue::String("/api/images/nope/image_data/0.jpg".into()),
        timestamp: 0,
    };

    let record = executor
        .execute_agent(agent.clone(), Some(event), None)
        .await
        .expect("skip returns Ok record");
    assert_eq!(record.status, neomind_storage::ExecutionStatus::Completed);
    assert!(record.error.is_none());
    assert!(
        record
            .result
            .as_ref()
            .is_some_and(|r| r.summary.starts_with("Skipped:")),
        "expected skip summary, got {:?}",
        record.result.as_ref().map(|r| &r.summary)
    );
    assert_eq!(
        record
            .input_summary
            .as_ref()
            .and_then(|s| s.skip_reason.as_deref()),
        Some("event_image_unreadable"),
        "the record says why the run was skipped"
    );

    let stored = executor
        .store()
        .get_agent(&agent.id)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(stored.stats.successful_executions, 0);
    assert_eq!(stored.stats.failed_executions, 0);
    assert_eq!(
        stored.consecutive_failures, 1,
        "skip must not increment or reset breaker strikes"
    );
    assert_eq!(stored.status, AgentStatus::Active);
}

// ─── What T4's three Dongpingshan camera agents exposed (2026-11-21 .. 11-23) ───

/// A knowledge-file store in a private temp dir; the caller removes the dir.
fn temp_memory_store() -> (
    Arc<neomind_storage::MarkdownMemoryStore>,
    std::path::PathBuf,
) {
    let dir = std::env::temp_dir().join(format!(
        "neomind-behavior-{}",
        uuid::Uuid::new_v4().simple()
    ));
    (
        Arc::new(neomind_storage::MarkdownMemoryStore::new(&dir)),
        dir,
    )
}

fn judgment_notify() -> neomind_storage::AgentNotify {
    neomind_storage::AgentNotify {
        channels: vec!["webhook:ops".into()],
        on: neomind_storage::NotifyOn::Judgment,
    }
}

fn task_understanding_ref() -> neomind_storage::KnowledgeFileRef {
    neomind_storage::KnowledgeFileRef {
        name: neomind_storage::TASK_UNDERSTANDING_FILE.into(),
        description: String::new(),
        created_at: 0,
        updated_at: 0,
    }
}

/// A `web_search` that only counts calls — the side effect that must never
/// happen for a judgment-notify agent.
struct WebSearchProbe {
    calls: Arc<std::sync::atomic::AtomicUsize>,
}

#[async_trait]
impl Tool for WebSearchProbe {
    fn name(&self) -> &str {
        "web_search"
    }
    fn description(&self) -> &str {
        "web search probe"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "query": { "type": "string" } },
            "required": ["query"]
        })
    }
    async fn execute(&self, _args: serde_json::Value) -> Result<ToolOutput, ToolError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(ToolOutput::success("[]"))
    }
}

/// The task definition is inlined from `custom:task-understanding` on every
/// run. A prompt edit that never reached the file kept feeding the model the
/// old mission — T4's agents read "用 vision 工具分析该图片" for two days
/// after the operator removed it from the prompt. The executor repairs the
/// file before inlining it, so the model never sees the stale copy.
///
/// This path has no previous prompt to match the old mission by, so the
/// repair is for the prompt only: what is under the heading may be the
/// operator's, and the file is theirs to fix.
#[tokio::test]
async fn stale_task_understanding_mission_is_resynced_before_it_is_inlined() {
    const MISSION: &str = "NEW_MISSION 直接看附带的图片";
    const STALE_FILE: &str = "# Task Understanding\n\n## Mission\nOLD_MISSION 用 vision 工具分析该图片\n\n## Notes\nLEARNED_NOTE\n";
    let (memory, dir) = temp_memory_store();
    let (mut executor, mut agent, _) = build_harness_with_memory(Some(memory.clone())).await;
    agent.user_prompt = MISSION.into();
    memory
        .write_agent_custom_file(
            &agent.id,
            neomind_storage::TASK_UNDERSTANDING_FILE,
            STALE_FILE,
        )
        .await
        .expect("seed a stale file");
    agent.memory.knowledge_files.push(task_understanding_ref());

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool));
    executor.set_tool_registry(Arc::new(registry));
    let rt = MockLlmRuntime::new(vec![MockResponse::text("无人，无赌博行为。")])
        .with_function_calling()
        .with_supports_images();
    let handle = rt.clone();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let _ = executor
        .execute_with_tools(&agent, &[], rt_dyn, "exec-stale-mission", None)
        .await
        .expect("run completes");

    let system = handle.captured_messages()[0].to_string();
    assert!(
        system.contains("#### custom:task-understanding"),
        "the knowledge file is inlined: {system}"
    );
    assert!(
        !system.contains("OLD_MISSION"),
        "a stale mission must not reach the model: {system}"
    );
    assert!(
        system.contains(MISSION),
        "the current mission reaches the model: {system}"
    );
    assert!(
        system.contains("LEARNED_NOTE"),
        "the agent's own notes stay inlined: {system}"
    );

    // The old mission's extent is not known here, so nothing is written.
    let on_disk = memory
        .read_agent_custom_file(&agent.id, neomind_storage::TASK_UNDERSTANDING_FILE)
        .expect("file");
    assert_eq!(on_disk, STALE_FILE, "the file is left as it was");
    let _ = std::fs::remove_dir_all(dir);
}

/// The operator writes in this file too. A note they filed under the
/// mission, with or without a heading of its own, survives every run — the
/// prompt shows the task prompt in the mission's place, the file stays.
#[tokio::test]
async fn an_operators_note_under_a_stale_mission_is_never_written_over() {
    const MISSION: &str = "NEW_MISSION 直接看附带的图片";
    for operator_file in [
        // Their own section, filed between the mission and the template's
        // next heading.
        "# Task Understanding\n\n## Mission\nOLD_MISSION\n\n## Operator Notes\nKEEP-ME\n\n## Bound Resources\nNone\n",
        // A bare note written right under the mission.
        "# Task Understanding\n\n## Mission\nOLD_MISSION\n\nKEEP-ME, no heading\n\n## Bound Resources\nNone\n",
    ] {
        let (memory, dir) = temp_memory_store();
        let (mut executor, mut agent, _) =
            build_harness_with_memory(Some(memory.clone())).await;
        agent.user_prompt = MISSION.into();
        memory
            .write_agent_custom_file(
                &agent.id,
                neomind_storage::TASK_UNDERSTANDING_FILE,
                operator_file,
            )
            .await
            .expect("seed the operator's file");
        agent.memory.knowledge_files.push(task_understanding_ref());

        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(EchoTool));
        executor.set_tool_registry(Arc::new(registry));
        let rt = MockLlmRuntime::new(vec![MockResponse::text("无人，无赌博行为。")])
            .with_function_calling()
            .with_supports_images();
        let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);
        executor.set_llm_runtime(rt_dyn.clone()).await;

        let _ = executor
            .execute_with_tools(&agent, &[], rt_dyn, "exec-operator-note", None)
            .await
            .expect("run completes");

        let on_disk = memory
            .read_agent_custom_file(&agent.id, neomind_storage::TASK_UNDERSTANDING_FILE)
            .expect("file");
        assert_eq!(on_disk, operator_file, "the operator's file is theirs");
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// T4's 200366 replaced its task with one per-run observation
/// ("无图像数据路径，需设备日志搜索") and then reasoned from that note on
/// every later run. Whatever the agent writes into the file, the end of a run
/// puts the mission back on top — the note survives below it.
#[tokio::test]
async fn a_task_understanding_overwritten_by_the_agent_gets_its_mission_back() {
    const MISSION: &str = "MISSION_TEXT";
    let (memory, dir) = temp_memory_store();
    let (executor, mut agent, _) = build_harness_with_memory(Some(memory.clone())).await;
    agent.user_prompt = MISSION.into();
    agent.memory.knowledge_files.push(task_understanding_ref());
    let note = "# NE301PIR-200366 任务理解\n## 图像数据路径\n- 无图像数据路径，需设备日志搜索\n";
    memory
        .write_agent_custom_file(&agent.id, neomind_storage::TASK_UNDERSTANDING_FILE, note)
        .await
        .expect("the agent overwrote its own file");

    // What the executor runs at the end of every execution.
    executor
        .finalize_execution_memory(
            &agent,
            &[],
            "无人，无赌博行为。",
            "exec-overwritten-mission",
            true,
            "natural-completion",
            &None,
        )
        .await
        .expect("memory finalization");

    let on_disk = memory
        .read_agent_custom_file(&agent.id, neomind_storage::TASK_UNDERSTANDING_FILE)
        .expect("file");
    assert_eq!(
        neomind_storage::task_understanding_mission(&on_disk),
        Some(MISSION),
        "the mission comes back: {on_disk}"
    );
    assert!(
        on_disk.ends_with(note),
        "the agent's note is its own to keep: {on_disk}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// The tool names the executor advertises to the model for `agent` on its
/// first LLM call, with an `echo` tool and a `web_search` probe registered.
async fn offered_tool_names(agent: &AiAgent) -> Vec<String> {
    let (mut executor, _, _) = build_harness().await;
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool));
    registry.register(Arc::new(WebSearchProbe {
        calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    }));
    executor.set_tool_registry(Arc::new(registry));
    let rt = MockLlmRuntime::new(vec![MockResponse::text("done")]).with_function_calling();
    let handle = rt.clone();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);
    executor.set_llm_runtime(rt_dyn.clone()).await;
    let _ = executor
        .execute_with_tools(agent, &[], rt_dyn, "exec-offered-tools", None)
        .await
        .expect("run completes");
    handle.captured_tool_names()[0].clone()
}

/// A judgment-notify agent decides from the input attached to this run. Web
/// tools only send the model hunting for what it believes is missing, and
/// they leave the site with the query (T4: 233 searches carrying device IDs
/// and the site name). Not offered unless the allowlist names them.
#[tokio::test]
async fn judgment_agent_is_not_offered_web_tools_unless_allowlisted() {
    // Stock agent: every tool goes out.
    let mut agent = build_harness().await.1;
    let names = offered_tool_names(&agent).await;
    assert!(names.contains(&"echo".to_string()), "{names:?}");
    assert!(names.contains(&"web_search".to_string()), "{names:?}");

    // Judgment-notify: the web tool stays home.
    agent.notify = Some(judgment_notify());
    let names = offered_tool_names(&agent).await;
    assert!(names.contains(&"echo".to_string()), "{names:?}");
    assert!(
        !names.contains(&"web_search".to_string()),
        "web tools are off for a judgment-notify agent: {names:?}"
    );

    // …unless the operator's allowlist says otherwise.
    agent.tool_config = Some(neomind_storage::AgentToolConfig {
        enabled: true,
        allowed_tools: vec!["echo".into(), "web_search".into()],
    });
    let names = offered_tool_names(&agent).await;
    assert!(names.contains(&"web_search".to_string()), "{names:?}");

    // Any other notify trigger is untouched.
    agent.tool_config = None;
    agent.notify.as_mut().expect("notify").on = neomind_storage::NotifyOn::Always;
    let names = offered_tool_names(&agent).await;
    assert!(names.contains(&"web_search".to_string()), "{names:?}");
}

/// Not offering the tool is not enough — a small model invents tool names.
/// The call itself is refused, so the registry never runs it.
///
/// Keep a non-web tool (`EchoTool`) registered: after judgment filtering drops
/// `web_search`, an empty allowlist would enter zero-tool mode and skip
/// parse/execute entirely (HIGH filtered_tools gate). The invent-and-block
/// path needs at least one remaining tool so the loop still honors calls and
/// `judgment_tool_off_by_default` can record the refusal.
#[tokio::test]
async fn judgment_agent_web_tool_call_is_blocked_even_when_invented() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call(
            "web_search",
            serde_json::json!({ "query": "NE301PIR-200366 image data path" }),
        ),
        MockResponse::text("done"),
    ])
    .with_function_calling();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);

    let (mut executor, mut agent, _) = build_harness().await;
    agent.notify = Some(judgment_notify());
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool));
    registry.register(Arc::new(WebSearchProbe {
        calls: calls.clone(),
    }));
    executor.set_tool_registry(Arc::new(registry));
    executor.set_llm_runtime(rt_dyn.clone()).await;

    let (_, er, _) = executor
        .execute_with_tools(&agent, &[], rt_dyn, "exec-blocked-web-tool", None)
        .await
        .expect("run completes with the call blocked");
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "a judgment-notify agent must never run a web tool"
    );
    assert!(
        er.actions_executed.iter().any(|a| a.target == "web_search"
            && !a.success
            && a.result
                .as_deref()
                .unwrap_or("")
                .contains("Web tools are off")),
        "the block must be visible to the model and in the record; actions={:?}",
        er.actions_executed
    );
}

// ── Context-overflow retry ──────────────────────────────────────────────

use std::pin::Pin;
use std::sync::Mutex;

use neomind_core::llm::backend::{
    BackendCapabilities, BackendId, LlmError, LlmInput, LlmOutput, StreamChunk,
};

/// Fails the FIRST call with a backend-reported context overflow, then answers
/// normally. Records each call's prompt size and generation budget.
struct OverflowOnceRuntime {
    calls: Mutex<Vec<(usize, usize)>>, // (prompt text chars, max_tokens)
    inner: MockLlmRuntime,
}

#[async_trait]
impl LlmRuntime for OverflowOnceRuntime {
    fn backend_id(&self) -> BackendId {
        BackendId::new("overflow-once")
    }
    fn model_name(&self) -> &str {
        "overflow-once-model"
    }
    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            streaming: false,
            multimodal: false,
            function_calling: true,
            multiple_models: false,
            max_context: Some(8192),
            modalities: vec![],
            reasoning: Default::default(),
            supports_images: false,
            thinking_display: false,
        }
    }
    fn max_context_length(&self) -> usize {
        8192
    }
    async fn generate(&self, input: LlmInput) -> Result<LlmOutput, LlmError> {
        let prompt_chars = input
            .messages
            .iter()
            .map(|m| m.content.as_text().len())
            .sum::<usize>();
        let max_tokens = input.params.max_tokens.unwrap_or(0);
        let first = {
            let mut calls = self.calls.lock().unwrap();
            let first = calls.is_empty();
            calls.push((prompt_chars, max_tokens));
            first
        };
        if first {
            Err(LlmError::ContextOverflow {
                prompt_tokens: 20_000,
                max_context: 8192,
            })
        } else {
            self.inner.generate(input).await
        }
    }
    async fn generate_stream(
        &self,
        _input: LlmInput,
    ) -> Result<Pin<Box<dyn futures::Stream<Item = StreamChunk> + Send>>, LlmError> {
        Err(LlmError::Generation("unused".into()))
    }
}

/// T4 2026-11-24: the overflow retry logged "hard-compacting messages and
/// retrying" and then re-sent the request frozen before the retry loop —
/// the backend reported 186,732 tokens, then 186,734. The retry must carry
/// the compacted messages, with the generation budget recomputed for them.
#[tokio::test]
async fn overflow_retry_resends_the_compacted_request() {
    let rt = Arc::new(OverflowOnceRuntime {
        calls: Mutex::new(Vec::new()),
        inner: MockLlmRuntime::new(vec![MockResponse::text("done")]),
    });
    let rt_dyn: Arc<dyn LlmRuntime> = rt.clone();
    let (executor, agent, registry) = build_harness().await;
    let (filtered_tools, tool_name_map) =
        AgentExecutor::filter_tools(&registry, &agent.tool_config);
    // ~48K chars of base64 with no spaces: the estimator under-counts it, so
    // the first request goes out oversized exactly like a raw-payload event.
    let big = "QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo=".repeat(1_500);
    let mut messages = vec![
        Message::new(
            MessageRole::System,
            format!(
                "You are a test agent.\n\n## Resources & Data\n**cam:_raw** [now]: value=\"{}\"",
                big
            ),
        ),
        Message::new(MessageRole::User, "do the thing"),
    ];
    let out = executor
        .run_tool_loop(
            &agent,
            &registry,
            &rt_dyn,
            &filtered_tools,
            &mut messages,
            "exec-overflow-retry",
            30,
            &tool_name_map,
            None,
        )
        .await;

    let calls = rt.calls.lock().unwrap().clone();
    assert_eq!(
        calls.len(),
        2,
        "one overflowed attempt, then one retry — got {calls:?}"
    );
    assert!(
        calls[1].0 * 4 < calls[0].0,
        "retry must send the compacted messages, got {calls:?}"
    );
    assert!(
        calls[1].1 > calls[0].1,
        "generation budget must be recomputed for the smaller request, got {calls:?}"
    );
    assert_eq!(
        out.stop_reason,
        StopReason::NaturalCompletion,
        "calls: {calls:?}"
    );
}

// ── Whole-device trigger: the report is the input ───────────────────────

use super::data_collector::{classify_image_value, EventInput, ImageValue};
use super::{DataSourceRef, EventTriggerData};
use neomind_storage::timeseries::DataPoint as TsPoint;

const CAM: &str = "test-device-001";
const FRAME_URL: &str = "/api/images/test-device-001/image/1234567890001.jpg";

fn event_for(field: &str, value: &str, ts: i64) -> EventTriggerData {
    EventTriggerData {
        source: DataSourceRef {
            source_type: "device".into(),
            source_id: CAM.into(),
            field: field.into(),
        },
        value: neomind_core::MetricValue::String(value.into()),
        timestamp: ts,
    }
}

fn point(timestamp: i64, value: serde_json::Value) -> TsPoint {
    TsPoint {
        timestamp,
        value,
        quality: None,
        metadata: None,
    }
}

async fn seed_report(
    ts: &Arc<neomind_storage::TimeSeriesStore>,
    report_ts: i64,
    metrics: &[(&str, serde_json::Value)],
) {
    let key = format!("device:{CAM}");
    for (metric, value) in metrics {
        ts.write(&key, metric, point(report_ts, value.clone()))
            .await
            .expect("seed report");
    }
    // The store's metric-name index is filled at flush time.
    ts.flush().expect("flush seeded report");
}

/// A camera uplink as the extractor emits it where the device type stores
/// `_raw`: the envelope first, then the template's metrics.
async fn seed_camera_report(ts: &Arc<neomind_storage::TimeSeriesStore>, report_ts: i64) -> String {
    let envelope = format!(
        r#"{{"metadata":{{"image_id":"cam01_{report_ts}"}},"image_data":"{FRAME_URL}","data":"{}"}}"#,
        "QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo=".repeat(2_000)
    );
    seed_report(
        ts,
        report_ts,
        &[
            ("_raw", serde_json::json!(envelope)),
            (
                "metadata.image_id",
                serde_json::json!(format!("cam01_{report_ts}")),
            ),
            ("device_info.battery_percent", serde_json::json!(88)),
            ("image_data", serde_json::json!(FRAME_URL)),
        ],
    )
    .await;
    // A reading from an earlier uplink — not part of this report.
    ts.write(
        &format!("device:{CAM}"),
        "temperature",
        point(report_ts - 600, serde_json::json!(21.5)),
    )
    .await
    .expect("seed older uplink");
    ts.flush().expect("flush older uplink");
    envelope
}

fn whole_device_filter() -> Option<String> {
    Some(format!(r#"{{"any":[{{"type":"device","id":"{CAM}"}}]}}"#))
}

/// T4 2026-11-24. A trigger on the whole device used to be fed whichever
/// field reached it first: the `_raw` envelope where the device type stores
/// one (242K chars of base64 as text — 186,732 prompt tokens on a 65K window,
/// the picture never read), or `metadata.image_id` where it does not (every
/// run skipped as an unreadable image). Both doorbells must yield the same
/// input: the report itself, frame attached once.
#[serial_test::serial]
#[tokio::test]
async fn whole_device_trigger_reads_the_report_whatever_field_rang() {
    let data_dir = std::env::temp_dir().join(format!(
        "neomind_test_whole_device_{}",
        uuid::Uuid::new_v4()
    ));
    crate::testing_helpers::setup_test_image_dir(&data_dir).expect("image dir");
    // The fixture is tens of bytes; pad it past the usable-image threshold.
    let frame = data_dir.join("images/test-device-001/image/1234567890001.jpg");
    let mut bytes = std::fs::read(&frame).expect("fixture");
    bytes.extend(std::iter::repeat_n(0u8, 1024));
    std::fs::write(&frame, &bytes).expect("pad fixture");
    std::env::set_var("NEOMIND_DATA_DIR", data_dir.to_str().unwrap());

    let report_ts = chrono::Utc::now().timestamp() - 5;
    let (mut executor, mut agent, _registry) = build_harness().await;
    let ts = neomind_storage::TimeSeriesStore::memory().expect("memory timeseries");
    executor.set_time_series_storage(ts.clone());
    let envelope = seed_camera_report(&ts, report_ts).await;

    // The shape the CLI/AI creates: a device-level filter, nothing bound.
    agent.schedule.schedule_type = ScheduleType::Event;
    agent.schedule.event_filter = whole_device_filter();

    let doorbells = [
        (
            "type that stores _raw",
            event_for("_raw", &envelope, report_ts),
        ),
        (
            "type without _raw",
            event_for(
                "metadata.image_id",
                &format!("cam01_{report_ts}"),
                report_ts,
            ),
        ),
    ];
    for (label, event) in &doorbells {
        let input = executor
            .collect_data_with_event(&agent, event)
            .await
            .expect("collect");
        let EventInput::Data(data) = &input else {
            panic!("{label}: the report carries a readable frame, got {input:?}");
        };
        let sources: Vec<&str> = data.iter().map(|d| d.source.as_str()).collect();
        assert!(
            !sources.contains(&format!("{CAM}:_raw").as_str()),
            "{label}: the envelope leaked into the input: {sources:?}"
        );
        assert!(
            !sources.contains(&format!("{CAM}:temperature").as_str()),
            "{label}: an older uplink's reading got in: {sources:?}"
        );
        let image_id = data
            .iter()
            .find(|d| d.data_type == "metadata.image_id")
            .expect("image_id stays visible");
        assert_eq!(image_id.values["value"], format!("cam01_{report_ts}"));
        assert!(
            image_id.values.get("_is_image").is_none(),
            "{label}: an ID string is not an image: {}",
            image_id.values
        );
        let frames: Vec<_> = data
            .iter()
            .filter(|d| d.values.get("_is_image").and_then(|v| v.as_bool()) == Some(true))
            .collect();
        assert_eq!(frames.len(), 1, "{label}: the frame is attached once");
        assert_eq!(frames[0].data_type, "image_data");
        assert_eq!(
            frames[0].values["value"], FRAME_URL,
            "the reference stays beside the bytes"
        );
        assert!(
            frames[0].values["image_base64"]
                .as_str()
                .is_some_and(|b| b.len() >= 1024),
            "{label}: frame bytes are attached"
        );
    }

    std::env::remove_var("NEOMIND_DATA_DIR");
    let _ = std::fs::remove_dir_all(&data_dir);
}

/// NM-389: report A triggers; B lands before collection — A's run must still
/// receive A's frame bytes (or skip with trigger_report_unavailable), never B.
#[serial_test::serial]
#[tokio::test]
async fn whole_device_snapshot_binds_triggering_report_not_latest() {
    let data_dir = std::env::temp_dir().join(format!(
        "neomind_test_whole_device_bind_{}",
        uuid::Uuid::new_v4()
    ));
    crate::testing_helpers::setup_test_image_dir(&data_dir).expect("image dir");
    let frame_a_path = data_dir.join("images/test-device-001/image/frame_a.jpg");
    let frame_b_path = data_dir.join("images/test-device-001/image/frame_b.jpg");
    std::fs::create_dir_all(frame_a_path.parent().unwrap()).expect("mkdir");
    // Distinct JPEG-magic payloads past the usable-image / magic-byte gates.
    let mut bytes_a = vec![0xFF, 0xD8, 0xFF, 0xE0];
    bytes_a.extend(std::iter::repeat_n(b'A', 2048));
    let mut bytes_b = vec![0xFF, 0xD8, 0xFF, 0xE0];
    bytes_b.extend(std::iter::repeat_n(b'B', 2048));
    std::fs::write(&frame_a_path, &bytes_a).expect("write A");
    std::fs::write(&frame_b_path, &bytes_b).expect("write B");
    std::env::set_var("NEOMIND_DATA_DIR", data_dir.to_str().unwrap());

    let url_a = "/api/images/test-device-001/image/frame_a.jpg";
    let url_b = "/api/images/test-device-001/image/frame_b.jpg";
    let ts_a = chrono::Utc::now().timestamp() - 30;
    let ts_b = ts_a + 10;

    let (mut executor, mut agent, _registry) = build_harness().await;
    let ts = neomind_storage::TimeSeriesStore::memory().expect("memory timeseries");
    executor.set_time_series_storage(ts.clone());
    agent.schedule.schedule_type = ScheduleType::Event;
    agent.schedule.event_filter = whole_device_filter();

    // Seed A, then B (B is latest by the time we collect).
    seed_report(
        &ts,
        ts_a,
        &[
            ("metadata.image_id", serde_json::json!("cam01_A")),
            ("image_data", serde_json::json!(url_a)),
            ("device_info.battery_percent", serde_json::json!(80)),
        ],
    )
    .await;
    seed_report(
        &ts,
        ts_b,
        &[
            ("metadata.image_id", serde_json::json!("cam01_B")),
            ("image_data", serde_json::json!(url_b)),
            ("device_info.battery_percent", serde_json::json!(79)),
        ],
    )
    .await;

    let event_a = event_for("image_data", url_a, ts_a);
    let input = executor
        .collect_data_with_event(&agent, &event_a)
        .await
        .expect("collect A");
    let EventInput::Data(data) = &input else {
        panic!("A's report must be recoverable, got {input:?}");
    };
    let frames: Vec<_> = data
        .iter()
        .filter(|d| d.values.get("_is_image").and_then(|v| v.as_bool()) == Some(true))
        .collect();
    assert_eq!(frames.len(), 1, "exactly one frame for A's run");
    let url = frames[0]
        .values
        .get("image_url")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let b64 = frames[0]
        .values
        .get("image_base64")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    // /api/images/ extraction returns base64 (url cleared); A's fill byte is 'A'.
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .expect("A's image_base64 must decode");
    assert!(
        decoded.iter().skip(4).any(|b| *b == b'A'),
        "A's run must get A's bytes, got len={} url={url}",
        decoded.len()
    );
    assert!(
        !decoded.iter().skip(4).any(|b| *b == b'B'),
        "must not inherit B's frame bytes"
    );
    let image_id = data
        .iter()
        .find(|d| d.data_type == "metadata.image_id")
        .and_then(|d| d.values.get("value").cloned())
        .unwrap_or(serde_json::Value::Null);
    assert_eq!(
        image_id,
        serde_json::json!("cam01_A"),
        "sidecar metrics must come from A's uplink too"
    );

    std::env::remove_var("NEOMIND_DATA_DIR");
    let _ = std::fs::remove_dir_all(&data_dir);
}

/// NM-389: same-second A then B overwrite — timeseries only keeps B, but the
/// triggering event still carries A. Collection must prefer the trigger frame
/// (decode to A's fill byte), never silently feed B to vision.
#[serial_test::serial]
#[tokio::test]
async fn whole_device_same_second_overwrite_prefers_trigger_frame_a() {
    let data_dir = std::env::temp_dir().join(format!(
        "neomind_test_whole_device_same_sec_{}",
        uuid::Uuid::new_v4()
    ));
    crate::testing_helpers::setup_test_image_dir(&data_dir).expect("image dir");
    let frame_a_path = data_dir.join("images/test-device-001/image/frame_a_ss.jpg");
    let frame_b_path = data_dir.join("images/test-device-001/image/frame_b_ss.jpg");
    std::fs::create_dir_all(frame_a_path.parent().unwrap()).expect("mkdir");
    let mut bytes_a = vec![0xFF, 0xD8, 0xFF, 0xE0];
    bytes_a.extend(std::iter::repeat_n(b'A', 2048));
    let mut bytes_b = vec![0xFF, 0xD8, 0xFF, 0xE0];
    bytes_b.extend(std::iter::repeat_n(b'B', 2048));
    std::fs::write(&frame_a_path, &bytes_a).expect("write A");
    std::fs::write(&frame_b_path, &bytes_b).expect("write B");
    std::env::set_var("NEOMIND_DATA_DIR", data_dir.to_str().unwrap());

    let url_a = "/api/images/test-device-001/image/frame_a_ss.jpg";
    let url_b = "/api/images/test-device-001/image/frame_b_ss.jpg";
    let ts = chrono::Utc::now().timestamp() - 20;

    let (mut executor, mut agent, _registry) = build_harness().await;
    let store = neomind_storage::TimeSeriesStore::memory().expect("memory timeseries");
    executor.set_time_series_storage(store.clone());
    agent.schedule.schedule_type = ScheduleType::Event;
    agent.schedule.event_filter = whole_device_filter();

    seed_report(
        &store,
        ts,
        &[
            ("metadata.image_id", serde_json::json!("cam01_A")),
            ("image_data", serde_json::json!(url_a)),
        ],
    )
    .await;
    // Same second overwrite — metric key now only holds B.
    seed_report(
        &store,
        ts,
        &[
            ("metadata.image_id", serde_json::json!("cam01_B")),
            ("image_data", serde_json::json!(url_b)),
        ],
    )
    .await;

    let event_a = event_for("image_data", url_a, ts);
    let input = executor
        .collect_data_with_event(&agent, &event_a)
        .await
        .expect("collect A");
    let EventInput::Data(data) = &input else {
        panic!("trigger-carried A must be recoverable, got {input:?}");
    };
    let frames: Vec<_> = data
        .iter()
        .filter(|d| d.values.get("_is_image").and_then(|v| v.as_bool()) == Some(true))
        .collect();
    assert_eq!(frames.len(), 1, "exactly one frame");
    use base64::Engine;
    let b64 = frames[0]
        .values
        .get("image_base64")
        .and_then(|v| v.as_str())
        .expect("image_base64");
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .expect("decode");
    assert!(
        decoded.iter().skip(4).any(|b| *b == b'A'),
        "model input must decode to A's bytes after same-second overwrite"
    );
    assert!(
        !decoded.iter().skip(4).any(|b| *b == b'B'),
        "must not silently feed B"
    );

    std::env::remove_var("NEOMIND_DATA_DIR");
    let _ = std::fs::remove_dir_all(&data_dir);
}

/// NM-389: scalar trigger A has no image; B uploads an image 1s later inside
/// the ±2s nearest window. A's run must not attach B — record unconfirmed.
#[serial_test::serial]
#[tokio::test]
async fn whole_device_scalar_trigger_rejects_nearby_later_frame() {
    let data_dir = std::env::temp_dir().join(format!(
        "neomind_test_whole_device_scalar_no_b_{}",
        uuid::Uuid::new_v4()
    ));
    crate::testing_helpers::setup_test_image_dir(&data_dir).expect("image dir");
    let frame_b_path = data_dir.join("images/test-device-001/image/frame_b_near.jpg");
    std::fs::create_dir_all(frame_b_path.parent().unwrap()).expect("mkdir");
    let mut bytes_b = vec![0xFF, 0xD8, 0xFF, 0xE0];
    bytes_b.extend(std::iter::repeat_n(b'B', 2048));
    std::fs::write(&frame_b_path, &bytes_b).expect("write B");
    std::env::set_var("NEOMIND_DATA_DIR", data_dir.to_str().unwrap());

    let url_b = "/api/images/test-device-001/image/frame_b_near.jpg";
    let ts_a = chrono::Utc::now().timestamp() - 20;
    let ts_b = ts_a + 1;

    let (mut executor, mut agent, _registry) = build_harness().await;
    let store = neomind_storage::TimeSeriesStore::memory().expect("memory timeseries");
    executor.set_time_series_storage(store.clone());
    agent.schedule.schedule_type = ScheduleType::Event;
    agent.schedule.event_filter = whole_device_filter();

    seed_report(&store, ts_a, &[("temperature", serde_json::json!(21.5))]).await;
    seed_report(&store, ts_b, &[("image_data", serde_json::json!(url_b))]).await;

    let event_a = event_for("temperature", "21.5", ts_a);
    let input = executor
        .collect_data_with_event(&agent, &event_a)
        .await
        .expect("collect A");
    let EventInput::Data(data) = &input else {
        panic!("scalar A should still yield Data, got {input:?}");
    };
    let frames: Vec<_> = data
        .iter()
        .filter(|d| d.values.get("_is_image").and_then(|v| v.as_bool()) == Some(true))
        .collect();
    assert!(
        frames.is_empty(),
        "A's request must not include B's frame: {frames:?}"
    );
    let status = data
        .iter()
        .find(|d| d.data_type == "frame_status")
        .expect("unconfirmed frame_status");
    assert_eq!(
        status.values.get("message").and_then(|v| v.as_str()),
        Some("本次画面无法确定")
    );
    assert_eq!(
        status
            .values
            .get("_frame_unconfirmed")
            .and_then(|v| v.as_bool()),
        Some(true)
    );
    assert!(data.iter().any(|d| d.data_type == "temperature"));

    std::env::remove_var("NEOMIND_DATA_DIR");
    let _ = std::fs::remove_dir_all(&data_dir);
}

/// NM-389: temp A at T with no image; image B lands in the *same second*.
/// Second-level time alone is not same-report identity — A's collect must
/// not attach B (keep `_frame_unconfirmed`).
#[serial_test::serial]
#[tokio::test]
async fn whole_device_scalar_same_second_image_is_not_same_report() {
    let data_dir = std::env::temp_dir().join(format!(
        "neomind_test_whole_device_scalar_same_sec_{}",
        uuid::Uuid::new_v4()
    ));
    crate::testing_helpers::setup_test_image_dir(&data_dir).expect("image dir");
    let frame_b_path = data_dir.join("images/test-device-001/image/frame_b_same_sec.jpg");
    std::fs::create_dir_all(frame_b_path.parent().unwrap()).expect("mkdir");
    let mut bytes_b = vec![0xFF, 0xD8, 0xFF, 0xE0];
    bytes_b.extend(std::iter::repeat_n(b'B', 2048));
    std::fs::write(&frame_b_path, &bytes_b).expect("write B");
    std::env::set_var("NEOMIND_DATA_DIR", data_dir.to_str().unwrap());

    let url_b = "/api/images/test-device-001/image/frame_b_same_sec.jpg";
    let ts = chrono::Utc::now().timestamp() - 20;

    let (mut executor, mut agent, _registry) = build_harness().await;
    let store = neomind_storage::TimeSeriesStore::memory().expect("memory timeseries");
    executor.set_time_series_storage(store.clone());
    agent.schedule.schedule_type = ScheduleType::Event;
    agent.schedule.event_filter = whole_device_filter();

    seed_report(&store, ts, &[("temperature", serde_json::json!(21.5))]).await;
    // Same second as A — must still not be treated as A's frame.
    seed_report(&store, ts, &[("image_data", serde_json::json!(url_b))]).await;

    let event_a = event_for("temperature", "21.5", ts);
    let input = executor
        .collect_data_with_event(&agent, &event_a)
        .await
        .expect("collect A");
    let EventInput::Data(data) = &input else {
        panic!("scalar A should still yield Data, got {input:?}");
    };
    let frames: Vec<_> = data
        .iter()
        .filter(|d| d.values.get("_is_image").and_then(|v| v.as_bool()) == Some(true))
        .collect();
    assert!(
        frames.is_empty(),
        "same-second B must not attach to temp A: {frames:?}"
    );
    let status = data
        .iter()
        .find(|d| d.data_type == "frame_status")
        .expect("unconfirmed frame_status");
    assert_eq!(
        status
            .values
            .get("_frame_unconfirmed")
            .and_then(|v| v.as_bool()),
        Some(true)
    );

    std::env::remove_var("NEOMIND_DATA_DIR");
    let _ = std::fs::remove_dir_all(&data_dir);
}

/// NM-389 E2E: A temp-only, B image same second, delay until B is stored,
/// then process A. Scripted model says "现场正常". Assert: model request has
/// no B bytes; settle 无法判定 / Partial; success count unchanged; person
/// alerts 0.
#[serial_test::serial]
#[tokio::test]
async fn whole_device_scalar_same_second_b_settles_partial_no_person_alert() {
    let data_dir = std::env::temp_dir().join(format!(
        "neomind_test_whole_device_scalar_e2e_{}",
        uuid::Uuid::new_v4()
    ));
    crate::testing_helpers::setup_test_image_dir(&data_dir).expect("image dir");
    let frame_b_path = data_dir.join("images/test-device-001/image/frame_b_e2e.jpg");
    std::fs::create_dir_all(frame_b_path.parent().unwrap()).expect("mkdir");
    let mut bytes_b = vec![0xFF, 0xD8, 0xFF, 0xE0];
    bytes_b.extend(std::iter::repeat_n(b'B', 2048));
    std::fs::write(&frame_b_path, &bytes_b).expect("write B");
    std::env::set_var("NEOMIND_DATA_DIR", data_dir.to_str().unwrap());

    let url_b = "/api/images/test-device-001/image/frame_b_e2e.jpg";
    let ts = chrono::Utc::now().timestamp() - 15;

    let message_manager = Arc::new(neomind_messages::MessageManager::new());
    let store = AgentStore::memory().expect("store");
    let config = AgentExecutorConfig {
        store: store.clone(),
        time_series_storage: None,
        device_service: None,
        event_bus: None,
        message_manager: Some(message_manager.clone()),
        llm_runtime: None,
        llm_backend_store: None,
        extension_registry: None,
        tool_registry: None,
        memory_store: None,
        backend_semaphores: None,
        skill_registry: None,
        execution_semaphore: None,
    };
    let mut executor = AgentExecutor::new(config).await.expect("executor");
    let ts_store = neomind_storage::TimeSeriesStore::memory().expect("memory timeseries");
    executor.set_time_series_storage(ts_store.clone());

    let agent = AiAgent {
        id: format!("patrol-scalar-{}", uuid::Uuid::new_v4().simple()),
        name: "vision-required-patrol".into(),
        description: None,
        // No camera/照片 keywords — create-time vision confirmation is the
        // explicit `vision` allowlist below (structured input contract).
        user_prompt: "判断入口是否有人".into(),
        llm_backend_id: None,
        parsed_intent: Some(neomind_storage::ParsedIntent {
            intent_type: neomind_storage::IntentType::Monitoring,
            target_metrics: vec![],
            conditions: vec![],
            actions: vec![],
            confidence: 0.9,
        }),
        resources: vec![],
        schedule: AgentSchedule {
            schedule_type: ScheduleType::Event,
            interval_seconds: None,
            cron_expression: None,
            timezone: None,
            // Numeric device id, whole-device filter, no image field / resource.
            event_filter: whole_device_filter(),
        },
        status: AgentStatus::Active,
        priority: 128,
        created_at: 0,
        updated_at: 0,
        last_execution_at: None,
        stats: AgentStats {
            total_executions: 0,
            successful_executions: 3, // must stay 3 after Partial
            failed_executions: 0,
            avg_duration_ms: 0,
            last_duration_ms: Some(0),
        },
        memory: AgentMemory {
            journal: ExecutionJournal::default(),
            knowledge_files: vec![],
            updated_at: 0,
        },
        conversation_history: vec![],
        user_messages: vec![],
        conversation_summary: None,
        context_window_size: 5,
        tool_config: Some(neomind_storage::AgentToolConfig {
            enabled: true,
            allowed_tools: vec!["vision".into()],
        }),
        execution_mode: ExecutionMode::Free,
        error_message: None,
        system_prompt: None,
        max_retries: 0,
        consecutive_failures: 0,
        output_schema: None,
        operator_config: None,
        memory_mode: None,
        notify: Some(neomind_storage::AgentNotify {
            channels: vec![],
            on: neomind_storage::NotifyOn::Failure,
        }),
        enable_tool_chaining: false,
        max_chain_depth: 3,
        max_tokens_per_run: None,
    };
    store.save_agent(&agent).await.expect("seed");

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(StubVisionTool));
    executor.set_tool_registry(Arc::new(registry));

    // Multimodal so a leaked B frame would appear as image bytes in the
    // request — the gate under test is that B never reaches the model.
    let rt = MockLlmRuntime::new(vec![MockResponse::text("现场正常")])
        .with_function_calling()
        .with_supports_images();
    let rt_handle = rt.clone();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);
    executor.set_llm_runtime(rt_dyn).await;

    // A first (temp only), then B's image in the same second — process A only
    // after B is already stored (the race the review called out).
    seed_report(&ts_store, ts, &[("temperature", serde_json::json!(21.5))]).await;
    seed_report(&ts_store, ts, &[("image_data", serde_json::json!(url_b))]).await;

    let event_a = event_for("temperature", "21.5", ts);
    let record = executor
        .execute_agent(agent.clone(), Some(event_a), None)
        .await
        .expect("Partial path returns Ok");

    assert_eq!(
        record.status,
        neomind_storage::ExecutionStatus::Partial,
        "error={:?}",
        record.error
    );
    assert!(
        record.decision_process.conclusion.contains("无法判定")
            || record
                .decision_process
                .conclusion
                .contains("本次画面无法确定"),
        "settle must be 无法判定, got {}",
        record.decision_process.conclusion
    );
    assert_eq!(
        record.decision_process.stop_reason,
        super::vision_path::VISION_TOOLS_FAILED_STOP_REASON
    );

    let dump = serde_json::to_string(&rt_handle.captured_messages()).unwrap_or_default();
    assert!(
        !dump.contains("frame_b_e2e") && !dump.contains(&"B".repeat(32)),
        "model request must not carry B's path or fill bytes"
    );
    // Decoded multimodal / base64 payloads must not contain B's JPEG fill.
    use base64::Engine;
    for msg in rt_handle.captured_messages() {
        let s = msg.to_string();
        for chunk in s.split('"') {
            if chunk.len() < 1024 {
                continue;
            }
            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(chunk) {
                assert!(
                    !bytes.iter().skip(4).any(|b| *b == b'B'),
                    "model request must not decode to B's frame bytes"
                );
            }
        }
    }

    let stored = store
        .get_agent(&agent.id)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(
        stored.stats.successful_executions, 3,
        "Partial must not bump success"
    );

    let person_alerts = message_manager
        .list_messages()
        .await
        .into_iter()
        .filter(|m| {
            let hay = format!("{} {}", m.title, m.message);
            hay.contains("人员") || hay.to_lowercase().contains("person")
        })
        .count();
    assert_eq!(person_alerts, 0, "no person alerts on unconfirmed frame");

    std::env::remove_var("NEOMIND_DATA_DIR");
    let _ = std::fs::remove_dir_all(&data_dir);
}

/// Positive control for NM-389: same input as the vision-patrol Partial case
/// (temp A + same-second orphan image B) but a **non-vision** temp task must
/// still Complete — `_frame_unconfirmed` does not demote settle.
#[serial_test::serial]
#[tokio::test]
async fn whole_device_plain_temp_still_completes() {
    let data_dir = std::env::temp_dir().join(format!(
        "neomind_test_plain_temp_with_orphan_b_{}",
        uuid::Uuid::new_v4()
    ));
    crate::testing_helpers::setup_test_image_dir(&data_dir).expect("image dir");
    let frame_b_path = data_dir.join("images/test-device-001/image/frame_b_orphan.jpg");
    std::fs::create_dir_all(frame_b_path.parent().unwrap()).expect("mkdir");
    let mut bytes_b = vec![0xFF, 0xD8, 0xFF, 0xE0];
    bytes_b.extend(std::iter::repeat_n(b'B', 2048));
    std::fs::write(&frame_b_path, &bytes_b).expect("write B");
    std::env::set_var("NEOMIND_DATA_DIR", data_dir.to_str().unwrap());

    let (mut executor, mut agent, registry) = build_harness().await;
    let store = neomind_storage::TimeSeriesStore::memory().expect("memory timeseries");
    executor.set_time_series_storage(store.clone());
    agent.schedule.schedule_type = ScheduleType::Event;
    agent.schedule.event_filter = whole_device_filter();
    agent.execution_mode = ExecutionMode::Free;
    // Mentions 摄像头 but explicitly not analyzing images — keyword scan
    // would false-positive; structured contract (no vision allowlist / no
    // image metric binding) must keep Completes.
    agent.user_prompt = "只读取摄像头温度，不分析图片".into();
    agent.tool_config = Some(neomind_storage::AgentToolConfig {
        enabled: true,
        allowed_tools: vec!["echo".into()],
    });
    executor.store().save_agent(&agent).await.expect("seed");
    executor.set_tool_registry(registry);

    let ts = chrono::Utc::now().timestamp() - 10;
    seed_report(&store, ts, &[("temperature", serde_json::json!(22.0))]).await;
    seed_report(
        &store,
        ts,
        &[(
            "image_data",
            serde_json::json!("/api/images/test-device-001/image/frame_b_orphan.jpg"),
        )],
    )
    .await;

    let rt = MockLlmRuntime::new(vec![MockResponse::text("现场正常")]).with_function_calling();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);
    executor.set_llm_runtime(rt_dyn).await;

    let record = executor
        .execute_agent(
            agent.clone(),
            Some(event_for("temperature", "22.0", ts)),
            None,
        )
        .await
        .expect("plain temp Completes");
    assert_eq!(
        record.status,
        neomind_storage::ExecutionStatus::Completed,
        "temp-only structured contract must ignore orphan same-sec frame; error={:?}",
        record.error
    );
    let stored = executor
        .store()
        .get_agent(&agent.id)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(stored.stats.successful_executions, 1);

    std::env::remove_var("NEOMIND_DATA_DIR");
    let _ = std::fs::remove_dir_all(&data_dir);
}

/// NM-389: `cam-12` must not own a same-second `cam-123` image via URL/ID
/// substring contains. Final model request must carry no B bytes.
#[serial_test::serial]
#[tokio::test]
async fn whole_device_report_id_cam12_does_not_own_cam123_image() {
    let data_dir = std::env::temp_dir().join(format!(
        "neomind_test_cam12_vs_cam123_{}",
        uuid::Uuid::new_v4()
    ));
    crate::testing_helpers::setup_test_image_dir(&data_dir).expect("image dir");
    let frame_b_path = data_dir.join("images/test-device-001/image/cam-123_frame.jpg");
    std::fs::create_dir_all(frame_b_path.parent().unwrap()).expect("mkdir");
    let mut bytes_b = vec![0xFF, 0xD8, 0xFF, 0xE0];
    bytes_b.extend(std::iter::repeat_n(b'B', 2048));
    std::fs::write(&frame_b_path, &bytes_b).expect("write B");
    std::env::set_var("NEOMIND_DATA_DIR", data_dir.to_str().unwrap());

    // URL contains "cam-123" — substring of "cam-12" must NOT link.
    let url_b = "/api/images/test-device-001/image/cam-123_frame.jpg";
    let ts = chrono::Utc::now().timestamp() - 14;

    let (mut executor, mut agent, _) = build_harness().await;
    let store = neomind_storage::TimeSeriesStore::memory().expect("memory timeseries");
    executor.set_time_series_storage(store.clone());
    agent.schedule.schedule_type = ScheduleType::Event;
    agent.schedule.event_filter = whole_device_filter();
    agent.execution_mode = ExecutionMode::Free;
    agent.user_prompt = "判断入口是否有人".into();
    agent.tool_config = Some(neomind_storage::AgentToolConfig {
        enabled: true,
        allowed_tools: vec!["vision".into()],
    });
    executor.store().save_agent(&agent).await.expect("seed");

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(StubVisionTool));
    executor.set_tool_registry(Arc::new(registry));

    seed_report(
        &store,
        ts,
        &[("metadata.image_id", serde_json::json!("cam-12"))],
    )
    .await;
    // Orphan B: URL embeds cam-123; structured id on the image is cam-123.
    let key = format!("device:{CAM}");
    store
        .write(
            &key,
            "image_data",
            TsPoint {
                timestamp: ts,
                value: serde_json::json!(url_b),
                quality: None,
                metadata: Some(serde_json::json!({ "image_id": "cam-123" })),
            },
        )
        .await
        .expect("seed B");
    store.flush().expect("flush B");

    let rt = MockLlmRuntime::new(vec![MockResponse::text("现场正常")])
        .with_function_calling()
        .with_supports_images();
    let rt_handle = rt.clone();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);
    executor.set_llm_runtime(rt_dyn).await;

    let record = executor
        .execute_agent(
            agent.clone(),
            Some(event_for("metadata.image_id", "cam-12", ts)),
            None,
        )
        .await
        .expect("Partial path Ok");
    assert_eq!(
        record.status,
        neomind_storage::ExecutionStatus::Partial,
        "unprovable cam-12 ownership must Partial; error={:?}",
        record.error
    );

    let dump = serde_json::to_string(&rt_handle.captured_messages()).unwrap_or_default();
    assert!(
        !dump.contains("cam-123_frame") && !dump.contains(&"B".repeat(32)),
        "model request must not carry cam-123 / B bytes"
    );
    use base64::Engine;
    for msg in rt_handle.captured_messages() {
        let s = msg.to_string();
        for chunk in s.split('"') {
            if chunk.len() < 1024 {
                continue;
            }
            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(chunk) {
                assert!(
                    !bytes.iter().skip(4).any(|b| *b == b'B'),
                    "request must not decode to B's frame"
                );
            }
        }
    }

    std::env::remove_var("NEOMIND_DATA_DIR");
    let _ = std::fs::remove_dir_all(&data_dir);
}

/// NM-389: report_id=A co-stamped with orphan image B (B did not update
/// report_id) must not feed B — image point itself must prove ownership.
#[serial_test::serial]
#[tokio::test]
async fn whole_device_report_id_a_plus_orphan_image_b_stays_unconfirmed() {
    let data_dir = std::env::temp_dir().join(format!(
        "neomind_test_report_id_orphan_b_{}",
        uuid::Uuid::new_v4()
    ));
    crate::testing_helpers::setup_test_image_dir(&data_dir).expect("image dir");
    let frame_b_path = data_dir.join("images/test-device-001/image/frame_b_orphan_id.jpg");
    std::fs::create_dir_all(frame_b_path.parent().unwrap()).expect("mkdir");
    let mut bytes_b = vec![0xFF, 0xD8, 0xFF, 0xE0];
    bytes_b.extend(std::iter::repeat_n(b'B', 2048));
    std::fs::write(&frame_b_path, &bytes_b).expect("write B");
    std::env::set_var("NEOMIND_DATA_DIR", data_dir.to_str().unwrap());

    let url_b = "/api/images/test-device-001/image/frame_b_orphan_id.jpg";
    let ts = chrono::Utc::now().timestamp() - 12;

    let (mut executor, mut agent, _registry) = build_harness().await;
    let store = neomind_storage::TimeSeriesStore::memory().expect("memory timeseries");
    executor.set_time_series_storage(store.clone());
    agent.schedule.schedule_type = ScheduleType::Event;
    agent.schedule.event_filter = whole_device_filter();

    // A: report_id only. B: image only, same second — classic wrong link.
    seed_report(
        &store,
        ts,
        &[("metadata.image_id", serde_json::json!("cam01_A"))],
    )
    .await;
    seed_report(&store, ts, &[("image_data", serde_json::json!(url_b))]).await;

    let event_a = event_for("metadata.image_id", "cam01_A", ts);
    let input = executor
        .collect_data_with_event(&agent, &event_a)
        .await
        .expect("collect A");
    let EventInput::Data(data) = &input else {
        panic!("A should still yield Data, got {input:?}");
    };
    let frames: Vec<_> = data
        .iter()
        .filter(|d| d.values.get("_is_image").and_then(|v| v.as_bool()) == Some(true))
        .collect();
    assert!(
        frames.is_empty(),
        "orphan B must not attach via co-stamped report_id alone: {frames:?}"
    );
    let status = data
        .iter()
        .find(|d| d.data_type == "frame_status")
        .expect("unconfirmed frame_status");
    assert_eq!(
        status.values.get("message").and_then(|v| v.as_str()),
        Some("本次画面无法确定")
    );

    std::env::remove_var("NEOMIND_DATA_DIR");
    let _ = std::fs::remove_dir_all(&data_dir);
}

/// NM-389: relative `/api/images/…` provenance must not displace fixture
/// bytes — the mock vision/multimodal backend's actual request carries
/// decodable JPEG fill bytes from the in-site path.
#[serial_test::serial]
#[tokio::test]
async fn multimodal_backend_request_carries_fixture_bytes_not_relative_url() {
    let data_dir = std::env::temp_dir().join(format!(
        "neomind_test_multimodal_bytes_{}",
        uuid::Uuid::new_v4()
    ));
    crate::testing_helpers::setup_test_image_dir(&data_dir).expect("image dir");
    let frame_path = data_dir.join("images/test-device-001/image/frame_bytes_a.jpg");
    std::fs::create_dir_all(frame_path.parent().unwrap()).expect("mkdir");
    let mut bytes_a = vec![0xFF, 0xD8, 0xFF, 0xE0];
    bytes_a.extend(std::iter::repeat_n(b'A', 2048));
    std::fs::write(&frame_path, &bytes_a).expect("write A");
    std::env::set_var("NEOMIND_DATA_DIR", data_dir.to_str().unwrap());

    let url = "/api/images/test-device-001/image/frame_bytes_a.jpg";
    let ts = chrono::Utc::now().timestamp() - 8;

    let (mut executor, mut agent, _) = build_harness().await;
    let store = neomind_storage::TimeSeriesStore::memory().expect("memory timeseries");
    executor.set_time_series_storage(store.clone());
    agent.schedule.schedule_type = ScheduleType::Event;
    agent.schedule.event_filter = whole_device_filter();
    agent.execution_mode = ExecutionMode::Free;
    agent.user_prompt = "用 vision 分析摄像头画面".into();
    executor.store().save_agent(&agent).await.expect("seed");

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(StubVisionTool));
    executor.set_tool_registry(Arc::new(registry));

    seed_report(
        &store,
        ts,
        &[
            ("metadata.image_id", serde_json::json!("cam01_A")),
            ("image_data", serde_json::json!(url)),
            (
                "_raw",
                serde_json::json!(format!(
                    r#"{{"metadata":{{"image_id":"cam01_A"}},"image_data":"{url}"}}"#
                )),
            ),
        ],
    )
    .await;

    let rt = MockLlmRuntime::new(vec![MockResponse::text("现场正常")])
        .with_function_calling()
        .with_supports_images();
    let rt_handle = rt.clone();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);
    executor.set_llm_runtime(rt_dyn).await;

    let record = executor
        .execute_agent(agent.clone(), Some(event_for("image_data", url, ts)), None)
        .await
        .expect("run");
    assert_eq!(
        record.status,
        neomind_storage::ExecutionStatus::Completed,
        "error={:?}",
        record.error
    );

    let captured = rt_handle.captured_messages();
    assert!(!captured.is_empty(), "mock backend must see a request");
    use base64::Engine;
    let mut saw_fixture_bytes = false;
    let dump = serde_json::to_string(&captured).unwrap_or_default();
    // Prefer structured image_base64 / data fields from the actual request.
    fn walk(v: &serde_json::Value, found: &mut bool) {
        match v {
            serde_json::Value::Object(map) => {
                if let Some(data) = map.get("data").and_then(|d| d.as_str()) {
                    if data.len() >= 1024 {
                        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) {
                            if bytes.iter().skip(4).any(|b| *b == b'A') {
                                *found = true;
                            }
                        }
                    }
                }
                if let Some(b64) = map.get("image_base64").and_then(|d| d.as_str()) {
                    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
                        if bytes.iter().skip(4).any(|b| *b == b'A') {
                            *found = true;
                        }
                    }
                }
                for child in map.values() {
                    walk(child, found);
                }
            }
            serde_json::Value::Array(items) => {
                for child in items {
                    walk(child, found);
                }
            }
            _ => {}
        }
    }
    walk(&serde_json::Value::Array(captured), &mut saw_fixture_bytes);
    assert!(
        saw_fixture_bytes,
        "mock vision backend request must carry fixture fill-A bytes"
    );
    // Relative path may appear as provenance text, but must not be the only
    // image part (image_url ContentPart with relative path).
    assert!(
        !dump.contains(r#""url":"/api/images/test-device-001/image/frame_bytes_a.jpg""#)
            || saw_fixture_bytes,
        "relative URL alone must not replace bytes"
    );

    std::env::remove_var("NEOMIND_DATA_DIR");
    let _ = std::fs::remove_dir_all(&data_dir);
}

/// The snapshot is for triggers that cover the WHOLE device only. A trigger
/// on one field keeps reading that field alone.
#[tokio::test]
async fn field_specific_trigger_still_reads_only_that_field() {
    let report_ts = chrono::Utc::now().timestamp() - 5;
    let (mut executor, mut agent, _registry) = build_harness().await;
    let ts = neomind_storage::TimeSeriesStore::memory().expect("memory timeseries");
    executor.set_time_series_storage(ts.clone());
    seed_report(
        &ts,
        report_ts,
        &[
            (
                "_raw",
                serde_json::json!(r#"{"temperature":21.5,"humidity":60}"#),
            ),
            ("temperature", serde_json::json!(21.5)),
            ("humidity", serde_json::json!(60)),
        ],
    )
    .await;
    agent.schedule.schedule_type = ScheduleType::Event;
    agent.schedule.event_filter = Some(format!(
        r#"{{"any":[{{"type":"device","id":"{CAM}","field":"humidity"}}]}}"#
    ));

    let input = executor
        .collect_data_with_event(&agent, &event_for("humidity", "60", report_ts))
        .await
        .expect("collect");
    let EventInput::Data(data) = input else {
        panic!("a text field is usable input");
    };
    let sources: Vec<&str> = data.iter().map(|d| d.source.as_str()).collect();
    assert_eq!(sources, vec![format!("{CAM}:humidity").as_str()]);

    // An unrelated field arriving does not open the whole report either.
    let input = executor
        .collect_data_with_event(&agent, &event_for("temperature", "21.5", report_ts))
        .await
        .expect("collect");
    let EventInput::Data(data) = input else {
        panic!("a text field is usable input");
    };
    let sources: Vec<&str> = data.iter().map(|d| d.source.as_str()).collect();
    assert_eq!(sources, vec![format!("{CAM}:temperature").as_str()]);
}

/// "Run now" on a whole-device agent used to run with no event context at
/// all: the candidate list skips field-less sources. It now opens the latest
/// report, like a real event does.
#[tokio::test]
async fn run_now_on_a_whole_device_agent_reads_the_latest_report() {
    let report_ts = chrono::Utc::now().timestamp() - 5;
    let (mut executor, mut agent, _registry) = build_harness().await;
    let ts = neomind_storage::TimeSeriesStore::memory().expect("memory timeseries");
    executor.set_time_series_storage(ts.clone());
    seed_report(
        &ts,
        report_ts,
        &[
            ("battery", serde_json::json!(88)),
            ("temperature", serde_json::json!(21.5)),
        ],
    )
    .await;
    agent.schedule.schedule_type = ScheduleType::Event;
    agent.schedule.event_filter = whole_device_filter();

    let event = executor
        .build_synthetic_event_data(&agent)
        .await
        .expect("a whole-source event is synthesized");
    assert_eq!(
        event.source.field, "",
        "no single field stands for the device"
    );
    assert!(
        executor
            .trigger_covers_whole_source(&agent, &event.source)
            .await
    );
    let input = executor
        .collect_data_with_event(&agent, &event)
        .await
        .expect("collect");
    let EventInput::Data(data) = input else {
        panic!("the report is usable input");
    };
    for metric in ["battery", "temperature"] {
        assert!(
            data.iter().any(|d| d.data_type == metric),
            "{metric} missing from the run's input"
        );
    }

    // A field-specific trigger is not whole-device, and "Run now" quotes
    // that field.
    agent.schedule.event_filter = Some(format!(
        r#"{{"any":[{{"type":"device","id":"{CAM}","field":"battery"}}]}}"#
    ));
    assert!(
        !executor
            .trigger_covers_whole_source(&agent, &event_for("battery", "88", report_ts).source)
            .await
    );
    let event = executor
        .build_synthetic_event_data(&agent)
        .await
        .expect("a field named in the `any` shape yields its candidate");
    assert_eq!(event.source.field, "battery");
    assert_eq!(event.timestamp, report_ts, "quotes the stored point");
}

/// A report whose only picture cannot be read leaves the run without its
/// input — whichever field rang — and the record says so instead of a generic
/// "image extraction failed".
#[tokio::test]
async fn whole_device_report_with_an_unreadable_frame_is_skipped_with_a_reason() {
    let report_ts = chrono::Utc::now().timestamp() - 5;
    let (mut executor, mut agent, _registry) = build_harness().await;
    let ts = neomind_storage::TimeSeriesStore::memory().expect("memory timeseries");
    executor.set_time_series_storage(ts.clone());
    agent.schedule.schedule_type = ScheduleType::Event;
    agent.schedule.event_filter = whole_device_filter();

    // A device with no picture at all is ordinary input, not a skip.
    seed_report(
        &ts,
        report_ts,
        &[
            ("battery", serde_json::json!(88)),
            ("temperature", serde_json::json!(21.5)),
        ],
    )
    .await;
    let input = executor
        .collect_data_with_event(&agent, &event_for("battery", "88", report_ts))
        .await
        .expect("collect");
    let EventInput::Data(data) = input else {
        panic!("a report without pictures is usable input");
    };
    assert_eq!(
        data.iter().filter(|d| d.data_type != "device_info").count(),
        2
    );

    // The same device reports a frame that is not there to read
    // (NEOMIND_DATA_DIR is unset in this test).
    seed_report(
        &ts,
        report_ts,
        &[
            ("metadata.image_id", serde_json::json!("cam01_1")),
            (
                "image_data",
                serde_json::json!("/api/images/nope/image_data/0.jpg"),
            ),
        ],
    )
    .await;
    for field in ["image_data", "metadata.image_id"] {
        let input = executor
            .collect_data_with_event(&agent, &event_for(field, "cam01_1", report_ts))
            .await
            .expect("collect");
        let EventInput::Skipped { code, detail } = input else {
            panic!("{field}: a camera report with no readable frame must not run blind");
        };
        assert_eq!(code, "report_image_unreadable", "{field}");
        assert!(detail.contains("could not be read"), "{field}: {detail}");
        assert!(detail.contains("image_data"), "{field}: {detail}");
    }
}

/// A field NAME is a hint, not proof. `metadata.image_id` carries an ID
/// string; it used to be treated as an image whose extraction failed.
#[tokio::test]
async fn image_like_name_with_a_plain_value_is_text_not_a_skip() {
    let report_ts = chrono::Utc::now().timestamp() - 5;
    let (executor, mut agent, _registry) = build_harness().await;
    agent.schedule.schedule_type = ScheduleType::Event;
    agent.schedule.event_filter = Some(format!(
        r#"{{"any":[{{"type":"device","id":"{CAM}","field":"metadata.image_id"}}]}}"#
    ));

    let input = executor
        .collect_data_with_event(
            &agent,
            &event_for("metadata.image_id", "cam01_1795509428", report_ts),
        )
        .await
        .expect("collect");
    let EventInput::Data(data) = input else {
        panic!("an ID string is usable text input");
    };
    assert_eq!(data[0].values["value"], "cam01_1795509428");
    assert!(data[0].values.get("_is_image").is_none());

    // A value that IS an image but cannot be read still skips the run.
    let input = executor
        .collect_data_with_event(
            &agent,
            &event_for(
                "metadata.image_id",
                "/api/images/nope/image_id/0.jpg",
                report_ts,
            ),
        )
        .await
        .expect("collect");
    match &input {
        EventInput::Skipped {
            code: "event_image_unreadable",
            ..
        } => {}
        other => panic!("an unreadable image value must skip, got {other:?}"),
    }
    assert_eq!(
        classify_image_value("metadata.image_id", &serde_json::json!("cam01_1")),
        ImageValue::NotImage
    );
}

// ── Event-triggered failure classes ─────────────────────────────────────

fn plain_event() -> EventTriggerData {
    EventTriggerData {
        source: DataSourceRef {
            source_type: "device".into(),
            source_id: "dev-a".into(),
            field: "temp".into(),
        },
        value: neomind_core::event::MetricValue::Float(1.0),
        timestamp: 0,
    }
}

/// T4 2026-11-24: a context overflow is a permanent failure, yet the event
/// path replayed the identical input five seconds later — two Failed runs,
/// two failure notifications and two breaker strikes for one event.
///
/// The Err(e) branch must also consult `is_retryable_event_failure` before
/// `attempt <= retries` (same predicate as Ok(Failed)) — otherwise a
/// permanent storage/admission Err burned the replay budget too.
#[tokio::test]
async fn permanent_event_failure_is_not_replayed() {
    let (mut executor, agent) = build_event_agent(ExecutionMode::Structured, None).await;
    // Every inference fails: empty script → the exhausted sentinel is not
    // valid JSON for the structured contract.
    executor
        .set_llm_runtime(Arc::new(MockLlmRuntime::new(vec![])))
        .await;

    let started = std::time::Instant::now();
    let out =
        AgentExecutor::execute_with_retry(&executor, agent.clone(), plain_event(), 1, &agent.id)
            .await;
    assert_eq!(out, super::event_trigger::EventRunOutcome::PermanentFailure);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(3),
        "a permanent failure must not sit through the 5s replay (took {:?})",
        started.elapsed()
    );
    let stored = executor
        .store()
        .get_agent(&agent.id)
        .await
        .expect("store")
        .expect("agent");
    assert_eq!(stored.stats.total_executions, 1, "exactly one run");
    assert_eq!(
        stored.consecutive_failures, 1,
        "one breaker strike per event"
    );
    assert_eq!(
        stored.status,
        neomind_storage::AgentStatus::Active,
        "one strike of three"
    );
    super::global_error_probe_state().write().remove(&agent.id);
}

/// A transient failure is still reported as one, so the listener frees the
/// cooldown for the next event.
#[tokio::test]
async fn transient_event_failure_is_reported_transient() {
    let (mut executor, agent) = build_event_agent(ExecutionMode::Structured, None).await;
    let network_blip = MockResponse::error("error sending request for url (http://llm.local)");
    executor
        .set_llm_runtime(Arc::new(MockLlmRuntime::new(vec![network_blip])))
        .await;

    let out =
        AgentExecutor::execute_with_retry(&executor, agent.clone(), plain_event(), 0, &agent.id)
            .await;
    assert_eq!(out, super::event_trigger::EventRunOutcome::TransientFailure);
}

/// The cooldown is freed only for a transient failure. After a permanent one
/// the next report carries the same unrunnable input: re-firing on it used to
/// burn the breaker within seconds.
#[tokio::test]
async fn permanent_failure_keeps_the_event_cooldown() {
    let (mut executor, mut agent) = build_event_agent(ExecutionMode::Structured, Some(0)).await;
    let mut op = crate::ai_agent::executor::structured::default_operator_config();
    op.debounce_secs = 0;
    op.event_cooldown_secs = Some(60);
    with_operator_config(&executor, &mut agent, op).await;
    executor
        .set_llm_runtime(Arc::new(MockLlmRuntime::new(vec![])))
        .await;

    fire_event(&executor, "device", "dev-a", "temp").await;
    assert_eq!(
        drain_event_tasks(&executor).await,
        1,
        "the first event runs"
    );
    let stored = executor
        .store()
        .get_agent(&agent.id)
        .await
        .expect("store")
        .expect("agent");
    assert_eq!(stored.stats.total_executions, 1);
    assert_eq!(stored.consecutive_failures, 1);

    fire_event(&executor, "device", "dev-a", "temp").await;
    assert_eq!(
        drain_event_tasks(&executor).await,
        0,
        "inside the cooldown the same unrunnable input must not run again"
    );
    super::global_error_probe_state().write().remove(&agent.id);
}

/// The execution record says what the run was given — counts and a field
/// name, never values or image bytes — so a "successful" camera run that
/// never received the picture is visible (T4 2026-11-24: eleven such runs,
/// and nothing in their records showed it).
#[tokio::test]
async fn input_summary_counts_metrics_and_names_the_image_without_values() {
    use super::data_collector::build_execution_input_summary;
    use neomind_storage::{DataCollected, ExecutionInputSummary};

    let now = chrono::Utc::now().timestamp();
    let entry = |source: &str, data_type: &str, values: serde_json::Value| DataCollected {
        source: source.into(),
        data_type: data_type.into(),
        values,
        timestamp: now,
    };
    let data = vec![
        entry(
            "test-device-001:metadata.image_id",
            "metadata.image_id",
            serde_json::json!({ "value": "cam01_1", "timestamp": now }),
        ),
        entry(
            "test-device-001:device_info.battery_percent",
            "device_info.battery_percent",
            serde_json::json!({ "value": 88 }),
        ),
        entry(
            "test-device-001:image_data",
            "image_data",
            serde_json::json!({
                "_is_image": true,
                "image_url": "/api/images/cam/image_data/1.jpg",
                "image_base64": "QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo=",
            }),
        ),
        entry(
            "test-device-001",
            "device_info",
            serde_json::json!({ "name": "cam" }),
        ),
        entry(
            "memory",
            "summary",
            serde_json::json!({ "text": "last time: nothing" }),
        ),
    ];
    assert_eq!(
        build_execution_input_summary(&data, None),
        ExecutionInputSummary {
            // Two metrics and the image; device metadata and memory are not
            // observations.
            metrics_collected: 3,
            image_extracted: Some(true),
            image_source_field: Some("image_data".into()),
            vision_path: None,
            skip_reason: None,
        }
    );

    // A run without pictures says so; a skipped one carries its code.
    let no_images = build_execution_input_summary(&data[..2], None);
    assert_eq!(no_images.image_extracted, Some(false));
    assert_eq!(no_images.image_source_field, None);
    let skipped = build_execution_input_summary(&[], Some("report_image_unreadable"));
    assert_eq!(skipped.metrics_collected, 0);
    assert_eq!(
        skipped.skip_reason.as_deref(),
        Some("report_image_unreadable")
    );
}

/// The summary reaches the record, with the vision path the run actually
/// took — all three of them.
#[tokio::test]
async fn execution_record_carries_the_input_summary() {
    let (mut executor, mut agent, _registry) = build_harness().await;
    agent.schedule.schedule_type = ScheduleType::Event;
    agent.schedule.event_filter = Some(format!(
        r#"{{"any":[{{"type":"device","id":"{CAM}","field":"image_data"}}]}}"#
    ));
    agent.execution_mode = ExecutionMode::Free;
    executor.store().save_agent(&agent).await.expect("seed");
    let tool_capable = || {
        Arc::new(MockLlmRuntime::new(vec![MockResponse::text("all fine")]).with_function_calling())
    };
    let frame = format!("data:image/jpeg;base64,{}", test_jpeg_base64(0x5A));
    let registry_with = |tool: Arc<dyn Tool>| {
        let mut registry = ToolRegistry::new();
        registry.register(tool);
        Arc::new(registry)
    };

    // The tool path with no image in sight.
    executor.set_llm_runtime(tool_capable()).await;
    executor.set_tool_registry(registry_with(Arc::new(EchoTool)));
    let summary = executor
        .execute_agent(agent.clone(), Some(event_for("battery", "88", 0)), None)
        .await
        .expect("run")
        .input_summary
        .expect("every recorded run says what it was given");
    assert_eq!(summary.metrics_collected, 1);
    assert_eq!(summary.image_extracted, Some(false));
    assert_eq!(summary.vision_path.as_deref(), Some("none"));
    assert_eq!(summary.skip_reason, None);

    // An image a text-only model can only reach through the vision tool —
    // the T4 camera setup.
    executor.set_llm_runtime(tool_capable()).await;
    executor.set_tool_registry(registry_with(Arc::new(StubVisionTool)));
    let summary = executor
        .execute_agent(
            agent.clone(),
            Some(event_for("image_data", &frame, 0)),
            None,
        )
        .await
        .expect("run")
        .input_summary
        .expect("summary");
    assert_eq!(summary.image_extracted, Some(true));
    assert_eq!(summary.image_source_field.as_deref(), Some("image_data"));
    assert_eq!(summary.vision_path.as_deref(), Some("vision_tool"));

    // A multimodal model sees the picture in its own message.
    let multimodal = Arc::new(
        MockLlmRuntime::new(vec![MockResponse::text("all fine")])
            .with_function_calling()
            .with_supports_images(),
    );
    executor.set_llm_runtime(multimodal).await;
    let summary = executor
        .execute_agent(
            agent.clone(),
            Some(event_for("image_data", &frame, 0)),
            None,
        )
        .await
        .expect("run")
        .input_summary
        .expect("summary");
    assert_eq!(summary.image_extracted, Some(true));
    assert_eq!(summary.vision_path.as_deref(), Some("inline_multimodal"));
}

/// The labels end up in stored records — they are a contract, not prose.
#[test]
fn vision_path_labels_are_stable() {
    use super::vision_path::VisionExecutionPath as Path;
    assert_eq!(Path::None.label(), "none");
    assert_eq!(Path::InlineMultimodal.label(), "inline_multimodal");
    assert_eq!(Path::ViaVisionTool.label(), "vision_tool");
    assert_eq!(Path::Unavailable.label(), "unavailable");
}

/// The event path decides "worth one replay?" on its own list. The breaker's
/// `is_transient_failure` answers a different question and also counts
/// `already executing` / `daily run cap` as transient — reusing it would
/// replay those or free their cooldown.
#[test]
fn event_retry_predicate_is_independent_of_the_breaker() {
    let retryable = AgentExecutor::is_retryable_event_failure;
    for transient in [
        "Network error: connection refused",
        "error sending request for url (http://llm.local/v1)",
        "request timed out after 300s",
        "Rate limited by API",
        "HTTP 429 too many requests",
        "operation timed out",
    ] {
        assert!(
            retryable(Some(transient)),
            "{transient} is a transport fault"
        );
    }
    for permanent in [
        "Context overflow: 186732 prompt tokens exceed 65536 context limit",
        "LLM tool-calling failed: context overflow",
        "invalid api key",
        "model not found",
        "agent is already executing",
        "daily run cap reached",
    ] {
        assert!(
            !retryable(Some(permanent)),
            "{permanent} must not be replayed"
        );
    }
    assert!(!retryable(None), "an unknown error is not replayed");
}

/// A transient failure gets exactly ONE replay — no more — and is reported as
/// transient when the replay fails too, so the caller frees the cooldown.
#[tokio::test]
async fn transient_event_failure_is_replayed_once_at_most() {
    let (mut executor, agent) = build_event_agent(ExecutionMode::Structured, None).await;
    let blip = || MockResponse::error("error sending request for url (http://llm.local)");
    let no_backoff = std::time::Duration::ZERO;

    let rt = Arc::new(MockLlmRuntime::new(vec![blip(), blip()]));
    executor.set_llm_runtime(rt.clone()).await;
    let out = AgentExecutor::execute_with_retry_with_backoff(
        &executor,
        agent.clone(),
        plain_event(),
        1,
        &agent.id,
        no_backoff,
    )
    .await;
    assert_eq!(out, super::event_trigger::EventRunOutcome::TransientFailure);
    assert_eq!(rt.call_count(), 2, "the first attempt plus one replay");

    // The replay succeeding completes the run.
    let rt = Arc::new(MockLlmRuntime::new(vec![
        blip(),
        MockResponse::text(r#"{"missing_count": 1}"#),
    ]));
    executor.set_llm_runtime(rt.clone()).await;
    let out = AgentExecutor::execute_with_retry_with_backoff(
        &executor,
        agent.clone(),
        plain_event(),
        1,
        &agent.id,
        no_backoff,
    )
    .await;
    assert_eq!(out, super::event_trigger::EventRunOutcome::Completed);
    assert_eq!(rt.call_count(), 2);
}

/// "Run now" finds an event for every way a trigger can be written: a
/// whole-device source in the `all` shape, a whole-device resource with no
/// filter at all (`--device-ids`), and one field named in the M2-1 `any`
/// shape — which the old `sources`-only reader never saw.
#[tokio::test]
async fn run_now_covers_every_trigger_shape() {
    let report_ts = chrono::Utc::now().timestamp() - 5;
    let (mut executor, mut agent, _registry) = build_harness().await;
    let ts = neomind_storage::TimeSeriesStore::memory().expect("memory timeseries");
    executor.set_time_series_storage(ts.clone());
    seed_report(
        &ts,
        report_ts,
        &[
            ("battery", serde_json::json!(88)),
            ("temperature", serde_json::json!(21.5)),
        ],
    )
    .await;
    agent.schedule.schedule_type = ScheduleType::Event;

    // Whole-device source written in the `all` shape.
    agent.schedule.event_filter = Some(format!(
        r#"{{"all":[{{"type":"device","id":"{CAM}"}}],"within_secs":60}}"#
    ));
    let event = executor
        .build_synthetic_event_data(&agent)
        .await
        .expect("`all`-shaped whole-device source is synthesized");
    assert_eq!(
        event.source.field, "",
        "no single field stands for the device"
    );

    // `--device-ids`: a whole-device resource and no filter at all.
    agent.schedule.event_filter = None;
    agent.resources = vec![neomind_storage::AgentResource {
        resource_type: neomind_storage::ResourceType::Device,
        resource_id: CAM.into(),
        name: CAM.into(),
        config: serde_json::json!({}),
    }];
    let event = executor
        .build_synthetic_event_data(&agent)
        .await
        .expect("a `--device-ids` binding is synthesized");
    assert_eq!(event.source.field, "");

    // One field, written in the `any` shape.
    agent.resources = vec![];
    agent.schedule.event_filter = Some(format!(
        r#"{{"any":[{{"type":"device","id":"{CAM}","field":"battery"}}]}}"#
    ));
    let event = executor
        .build_synthetic_event_data(&agent)
        .await
        .expect("a field named in the `any` shape yields its candidate");
    assert_eq!(event.source.field, "battery");
    assert_eq!(event.source.source_id, CAM);
    assert_eq!(event.timestamp, report_ts, "quotes the stored point");
    assert!(
        matches!(&event.value, neomind_core::MetricValue::Json(v) if *v == serde_json::json!(88)),
        "telemetry lives under `device:{{id}}` — the bare id found nothing: {:?}",
        event.value
    );
}
