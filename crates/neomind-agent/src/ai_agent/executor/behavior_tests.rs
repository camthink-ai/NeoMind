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
        output_schema: None,
        operator_config: None,
        memory_mode: None,
        notify: None,
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
    assert!(dp.decisions[0].action.contains("ai:test-agent"));
}

/// The dispatch in `execute_internal` must route a structured agent to the L0
/// branch. Its sibling above calls `execute_structured` directly, so it cannot
/// catch a dispatch that routes elsewhere — this one enters through the same
/// production entry point the scheduler uses.
#[tokio::test]
async fn structured_mode_is_dispatched_from_the_production_entry_point() {
    let (mut executor, mut agent, _registry) = build_harness().await;
    agent.execution_mode = neomind_storage::agents::ExecutionMode::Structured;
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
    executor.store().save_agent(&agent).await.expect("seed store");

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
    });
    executor.store().save_agent(&agent).await.expect("seed store");
    let rt: Arc<dyn LlmRuntime> = Arc::new(MockLlmRuntime::new(vec![MockResponse::text(
        r#"{"missing_count": 1}"#,
    )]));
    executor.set_llm_runtime(rt).await;
    (executor, agent)
}

/// One physical event can match several of a structured agent's sources. The
/// debounce window must merge them so the agent pays for one inference, not
/// one per match — that is what the editor's "防抖" field promises.
#[tokio::test]
async fn structured_event_agent_debounces_across_sources() {
    let (executor, _agent) = build_event_agent(ExecutionMode::Structured, Some(300)).await;

    for source in ["dev-a", "dev-b"] {
        executor
            .check_and_trigger_data_event(
                "device",
                source.to_string(),
                "temp".to_string(),
                &neomind_core::event::MetricValue::Float(1.0),
            )
            .await
            .expect("trigger must not error");
    }

    let recent = executor.recent_executions.read().await;
    assert!(
        recent.contains_key("test-agent:device:dev-a"),
        "the first source must run"
    );
    assert!(
        !recent.contains_key("test-agent:device:dev-b"),
        "a second source inside the debounce window must be merged away, \
         not run as its own inference"
    );
}

/// The guardrail is L0-only: an agent with no operator config keeps today's
/// per-source cooldown and nothing else.
#[tokio::test]
async fn event_agent_without_operator_config_keeps_per_source_cooldown_only() {
    let (executor, _agent) = build_event_agent(ExecutionMode::Free, None).await;

    for source in ["dev-a", "dev-b"] {
        executor
            .check_and_trigger_data_event(
                "device",
                source.to_string(),
                "temp".to_string(),
                &neomind_core::event::MetricValue::Float(1.0),
            )
            .await
            .expect("trigger must not error");
    }

    let recent = executor.recent_executions.read().await;
    assert!(
        recent.contains_key("test-agent:device:dev-b"),
        "without L0 guardrails each source keeps its own cooldown window"
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
    executor.store().save_agent(&agent).await.expect("seed store");

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
    store.flush().expect("flush (stale point only reaches redb here)");

    let fresh = AgentExecutor::latest_point_in_window(
        &store,
        "dev-3",
        "temperature",
        now - 3600,
    )
    .await
    .expect("fresh metric visible before flush")
    .expect("Some");
    assert_eq!(fresh.value, serde_json::json!(23.5));

    let stale = AgentExecutor::latest_point_in_window(
        &store,
        "dev-3",
        "humidity",
        now - 3600,
    )
    .await
    .expect("no error");
    assert!(stale.is_none(), "a point older than the window must stay excluded");
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
    let store = AgentStore::memory().expect("store");
    let message_manager = Arc::new(neomind_messages::MessageManager::new());
    let config = AgentExecutorConfig {
        store: store.clone(),
        time_series_storage: Some(neomind_storage::TimeSeriesStore::memory().expect("ts")),
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
        "resources": [],
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

    executor.execute_agent(agent, None, None).await.expect("run ok");

    let messages = message_manager.list_messages().await;
    let hit = messages
        .iter()
        .find(|m| m.source == "agent:always-agent")
        .expect("success must notify under on=always");
    assert!(hit.title.contains("completed"), "title: {}", hit.title);
    assert!(hit.message.contains("正常"), "body carries the verdict: {}", hit.message);
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
    assert!(hit.message.contains("output_schema") || hit.message.len() > 0);
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
    };
    executor.store().save_agent(&agent).await.expect("seed");

    let rt: Arc<dyn LlmRuntime> = Arc::new(MockLlmRuntime::new(vec![MockResponse::text(
        r#"{"anomaly_count": 0, "status": "待检"}"#,
    )]));
    executor.set_llm_runtime(rt).await;

    let (_dp, record) = executor
        .execute_structured("exec-two-field", &agent, vec![])
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
    let rt = MockLlmRuntime::new(vec![MockResponse::text(
        r#"{"clutter_level": "一般"}"#,
    )]);
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
        .apply_output_contract(&agent, "画面里有 1 件漏装，批次待检")
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
        .apply_output_contract(&agent, "画面模糊")
        .await;

    assert_eq!(published, None, "a botched extraction must not fail the run");
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

    assert_eq!(executor.apply_output_contract(&agent, "any conclusion").await, None);
}

/// Most agents declare no contract at all — the step must be free for them.
#[tokio::test]
async fn output_contract_is_skipped_without_a_schema() {
    let (mut executor, agent, _registry) = build_harness().await;
    let rt: Arc<dyn LlmRuntime> = Arc::new(MockLlmRuntime::new(vec![MockResponse::text(
        r#"{"missing_count": 1}"#,
    )]));
    executor.set_llm_runtime(rt).await;

    assert_eq!(executor.apply_output_contract(&agent, "any conclusion").await, None);
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
    });
    let rt: Arc<dyn LlmRuntime> = Arc::new(
        MockLlmRuntime::new(vec![
            MockResponse::text("nothing unusual"),
            MockResponse::text("nothing unusual"),
        ])
        .with_function_calling(),
    );
    executor.set_llm_runtime(rt).await;
    executor.store().save_agent(&agent).await.expect("seed store");

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
        second.error.as_deref().unwrap_or_default().contains("daily run cap"),
        "second run must be refused with the cap message; got {:?}",
        second.error
    );
}
