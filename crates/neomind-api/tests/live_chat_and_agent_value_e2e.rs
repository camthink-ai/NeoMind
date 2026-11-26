//! Comprehensive live E2E verification with real local models:
//! 1. AI Agent real value & P0/P1 hardening (`dry_run` tool rejection, custom
//!    non-`status` alert boolean, `MissingPolicy::Skip` zero-token short-circuit,
//!    circuit-breaker threshold + recovery, real `AgentEventBridge` trigger).
//! 2. Chat building & managing AI Agents end-to-end via real HTTP API + real
//!    `neomind` CLI (`ShellTool`) + real local Ollama `qwen3.5:4b`.
//!
//! Run manually with:
//! `cargo test -p neomind-api --test live_chat_and_agent_value_e2e -- --ignored --nocapture`

use std::sync::Arc;
use std::time::Duration;

use neomind_agent::agent::{Agent, AgentConfig};
use neomind_agent::llm_backends::instance_manager::{
    set_instance_manager, LlmBackendInstanceManager,
};
use neomind_agent::toolkit::shell::{ShellConfig, ShellTool};
use neomind_agent::toolkit::ToolRegistry;
use neomind_agent::SessionManager;
use neomind_ai_agent::model::{
    AgentSpecDraft, AgentStatus, AlertVerdict, ExecutionStatus, FreshnessPolicy, InputContract,
    InputModality, InputRequirement, InputSourceRef, MissingPolicy, NotifyPolicy, OutputContract,
    OutputField, PacingPolicy, ToolPolicy, TriggerSpec,
};
use neomind_api::agent_events::AgentEventBridge;
use neomind_api::agent_ports::HostAgentDeps;
use neomind_api::ai_agent_runtime::HostedAiAgentRuntime;
use neomind_api::automation::metric_publish::VirtualMetricPublisher;
use neomind_api::handlers::ServerState;
use neomind_api::server::router::create_router_with_state;
use neomind_api::server::state::ExtensionMetricsStorage;
use neomind_core::eventbus::EventBus;
use neomind_core::extension::ExtensionRegistry;
use neomind_devices::telemetry::DataPoint;
use neomind_devices::{DeviceRegistry, DeviceService, MetricValue, TimeSeriesStorage};
use neomind_messages::MessageManager;
use neomind_storage::llm_backends::{BackendCapabilities, LlmBackendInstance, LlmBackendStore};
use neomind_storage::{AgentStore, LlmBackendType, MarkdownMemoryStore, MemoryConfig};

fn make_instance(
    id: &str,
    backend_type: LlmBackendType,
    endpoint: &str,
    model: &str,
    multimodal: bool,
    tools: bool,
    max_context: usize,
) -> LlmBackendInstance {
    LlmBackendInstance {
        id: id.into(),
        name: id.into(),
        backend_type,
        endpoint: Some(endpoint.into()),
        model: model.into(),
        api_key: None,
        is_active: false,
        is_builtin: false,
        thinking_is_integral: false,
        temperature: 0.2,
        top_p: 0.9,
        max_tokens: 1024,
        top_k: 40,
        thinking_enabled: false,
        thinking_effort: None,
        capabilities: BackendCapabilities {
            supports_streaming: true,
            supports_multimodal: multimodal,
            supports_thinking: false,
            supports_tools: tools,
            max_context,
            ..Default::default()
        },
        updated_at: chrono::Utc::now().timestamp(),
    }
}

async fn setup_local_models(dir: &std::path::Path) -> Arc<LlmBackendInstanceManager> {
    let llm_store = LlmBackendStore::open(dir.join("llm_backends.redb")).expect("llm store");
    let mgr = Arc::new(LlmBackendInstanceManager::new(llm_store));

    let mut qwen = make_instance(
        "ollama-qwen35",
        LlmBackendType::Ollama,
        "http://127.0.0.1:11434",
        "qwen3.5:4b",
        true,
        true,
        262144,
    );
    qwen.is_active = true;
    mgr.upsert_instance(qwen).await.expect("upsert qwen");

    let spark = make_instance(
        "llamacpp-spark",
        LlmBackendType::LlamaCpp,
        "http://127.0.0.1:29375",
        "sparkx2.6-1.7b",
        false,
        true,
        131072,
    );
    mgr.upsert_instance(spark).await.expect("upsert spark");

    set_instance_manager(mgr.clone());
    mgr
}

#[tokio::test]
#[ignore = "requires local Ollama (11434 qwen3.5:4b) and llama-server (29375 sparkx2.6-1.7b)"]
async fn live_agent_value_and_hardening_suite() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _mgr = setup_local_models(dir.path()).await;

    let mm = Arc::new(MessageManager::new());
    let telemetry = Arc::new(TimeSeriesStorage::memory().expect("ts"));
    let bus = Arc::new(EventBus::new());
    let dev_reg = Arc::new(DeviceRegistry::new());
    let device_service = Arc::new(DeviceService::new(dev_reg, (*bus).clone()));
    device_service.set_telemetry_storage(telemetry.clone()).await;
    let ext_reg = Arc::new(ExtensionRegistry::new());
    let ext_metrics = Arc::new(ExtensionMetricsStorage::with_shared_storage(telemetry.clone()));

    let value_provider = Arc::new(neomind_rules::UnifiedValueProvider::new());
    let rule_engine = Arc::new(neomind_rules::RuleEngine::new(value_provider.clone()));
    let publisher = Arc::new(VirtualMetricPublisher {
        event_bus: bus.clone(),
        time_series: telemetry.clone(),
        value_provider,
        rule_engine,
    });

    let mem_store = Arc::new(MarkdownMemoryStore::with_config(
        dir.path().join("memory"),
        MemoryConfig::default(),
    ));
    let sm = Arc::new(SessionManager::memory());
    let mut tool_reg = ToolRegistry::new();
    tool_reg.register(Arc::new(ShellTool::new(ShellConfig {
        enabled: true,
        timeout_secs: 30,
        max_output_chars: 16_000,
    })));
    sm.set_tool_registry(Arc::new(tool_reg)).await;

    let deps = HostAgentDeps::new(mm, telemetry.clone(), device_service, ext_reg, ext_metrics)
        .with_event_bus(bus.clone())
        .with_metric_publisher(publisher)
        .with_memory_store(mem_store)
        .with_session_manager(sm);

    let agent_store = AgentStore::open(dir.path().join("agents.redb")).expect("agent store");
    let rt = Arc::new(HostedAiAgentRuntime::build(agent_store, deps).expect("build runtime"));

    let now_secs = chrono::Utc::now().timestamp();
    let mut failures: Vec<String> = Vec::new();
    let mut check = |ok: bool, msg: String| {
        if ok {
            println!("  [PASS] {msg}");
        } else {
            println!("  [FAIL] {msg}");
            failures.push(msg);
        }
    };

    // ── V1: Dry-run must reject side-effect `shell` tool via SafetyPolicyHook ──
    println!("\n=== V1: Dry-run blocks shell side-effect tool (qwen3.5:4b) ===");
    let (v1_spec, _) = rt
        .create(AgentSpecDraft {
            name: "V1 DryRun Shell Guard".into(),
            description: None,
            user_prompt: "请调用 shell 工具执行 `uname -a` 并汇报结果。".into(),
            llm_backend_id: Some("ollama-qwen35".into()),
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract::default(),
            tool_policy: ToolPolicy::Scoped {
                allowed_tools: vec!["shell".into()],
                allowed_commands: vec!["uname".into()],
            },
            output: OutputContract::default(),
            notify: NotifyPolicy::default(),
            controlled_actions: vec![],
        })
        .expect("create v1");
    let v1_resp = rt.dry_run(&v1_spec.id).await.expect("dry_run v1");
    let v1_exec = v1_resp.execution.as_ref().unwrap();
    println!("  tool_calls: {:?}", v1_exec.tool_calls);
    println!("  conclusion: {:?}", v1_exec.conclusion);
    check(v1_resp.dry_run, "V1 dry_run=true".into());
    check(
        v1_exec.tool_calls.iter().all(|t| !t.success),
        format!(
            "V1 zero shell calls succeeded during dry_run (traces={})",
            v1_exec.tool_calls.len()
        ),
    );
    if !v1_exec.tool_calls.is_empty() {
        check(
            v1_exec.tool_calls.iter().any(|t| {
                t.error
                    .as_deref()
                    .unwrap_or("")
                    .contains("dry-run rejects side effect")
            }),
            "V1 recorded SafetyPolicyHook dry-run rejection error".into(),
        );
    }

    // ── V2: Custom non-`status` output fields + alert boolean in json_mode ──
    println!("\n=== V2: Custom output fields [peak_kw, summary] with on_alert=true (qwen3.5:4b) ===");
    telemetry
        .write(
            "device:meter-peak",
            "power_kw",
            DataPoint {
                timestamp: now_secs,
                value: MetricValue::Float(148.5),
                quality: None,
                metadata: None,
            },
        )
        .await
        .expect("write power_kw");
    let notifies_before = rt.notify.deliveries();
    let (v2_spec, _) = rt
        .create(AgentSpecDraft {
            name: "V2 Peak Power Alert".into(),
            description: None,
            user_prompt: "检查电表功率 power_kw。阈值 120 kW，超过 120 kW 必须触发告警。".into(),
            llm_backend_id: Some("ollama-qwen35".into()),
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract {
                requirements: vec![InputRequirement {
                    key: "meter".into(),
                    source: InputSourceRef::DeviceMetric {
                        device_id: "meter-peak".into(),
                        field: "power_kw".into(),
                    },
                    operation: None,
                    params: serde_json::json!({}),
                    result_type: InputModality::Metric,
                    freshness: FreshnessPolicy::Any,
                    on_missing: MissingPolicy::ReportStale,
                    timeout_secs: 10,
                    auth_scope: None,
                }],
            },
            tool_policy: ToolPolicy::Disabled,
            output: OutputContract {
                publish: true,
                fields: vec![
                    OutputField {
                        name: "peak_kw".into(),
                        depends_on: vec!["meter".into()],
                        description: Some("Current power in kW".into()),
                    },
                    OutputField {
                        name: "summary".into(),
                        depends_on: vec!["meter".into()],
                        description: Some("One-sentence Chinese summary".into()),
                    },
                ],
            },
            notify: NotifyPolicy {
                channels: vec!["ops-alert".into()],
                on_complete: false,
                on_alert: true,
                alert_depends_on: vec!["meter".into()],
                on_failure: false,
                pending_confirmation: false,
            },
            controlled_actions: vec![],
        })
        .expect("create v2");
    let v2_resp = rt.invoke(&v2_spec.id).await.expect("invoke v2");
    let v2_exec = v2_resp.execution.as_ref().unwrap();
    println!("  alert={:?}, structured={:?}", v2_exec.alert_verdict, v2_exec.structured_result);
    check(
        v2_exec.alert_verdict == Some(AlertVerdict::CriteriaMet),
        format!("V2 alert=CriteriaMet via raw alert boolean (got {:?})", v2_exec.alert_verdict),
    );
    check(
        rt.notify.deliveries() == notifies_before + 1,
        format!(
            "V2 sent 1 alert notification (delta={})",
            rt.notify.deliveries() - notifies_before
        ),
    );
    check(
        v2_exec
            .structured_result
            .as_ref()
            .and_then(|v| v.get("alert"))
            .is_none(),
        "V2 structured_result stays strictly projected without leaking `alert` key".into(),
    );

    // ── V3: MissingPolicy::Skip short-circuits with 0 LLM calls ──
    println!("\n=== V3: MissingPolicy::Skip zero-token short-circuit ===");
    let llm_before = rt.llm.requests();
    let (v3_spec, _) = rt
        .create(AgentSpecDraft {
            name: "V3 Skip Offline".into(),
            description: None,
            user_prompt: "Analyze sensor.".into(),
            llm_backend_id: Some("llamacpp-spark".into()),
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract {
                requirements: vec![InputRequirement {
                    key: "offline_sensor".into(),
                    source: InputSourceRef::DeviceMetric {
                        device_id: "offline-dev-01".into(),
                        field: "temp".into(),
                    },
                    operation: None,
                    params: serde_json::json!({}),
                    result_type: InputModality::Metric,
                    freshness: FreshnessPolicy::Any,
                    on_missing: MissingPolicy::Skip,
                    timeout_secs: 5,
                    auth_scope: None,
                }],
            },
            tool_policy: ToolPolicy::Disabled,
            output: OutputContract::default(),
            notify: NotifyPolicy::default(),
            controlled_actions: vec![],
        })
        .expect("create v3");
    let v3_resp = rt.invoke(&v3_spec.id).await.expect("invoke v3");
    let v3_exec = v3_resp.execution.as_ref().unwrap();
    check(
        v3_exec.status == ExecutionStatus::Skipped,
        format!("V3 status=Skipped (got {:?})", v3_exec.status),
    );
    check(
        rt.llm.requests() == llm_before,
        format!(
            "V3 made 0 LLM calls (delta={})",
            rt.llm.requests() - llm_before
        ),
    );

    // ── V4: Circuit Breaker respects failure_threshold=2 + sets next_probe_at ──
    println!("\n=== V4: Circuit breaker threshold=2 & next_probe_at ===");
    let (v4_spec, _) = rt
        .create(AgentSpecDraft {
            name: "V4 Breaker Check".into(),
            description: None,
            user_prompt: "Check missing.".into(),
            llm_backend_id: Some("llamacpp-spark".into()),
            preset: None,
            trigger: TriggerSpec::Interval { every_secs: 60 },
            pacing: PacingPolicy {
                failure_threshold: 2,
                ..Default::default()
            },
            input: InputContract {
                requirements: vec![InputRequirement {
                    key: "missing_m".into(),
                    source: InputSourceRef::DeviceMetric {
                        device_id: "missing-breaker-dev".into(),
                        field: "temp".into(),
                    },
                    operation: None,
                    params: serde_json::json!({}),
                    result_type: InputModality::Metric,
                    freshness: FreshnessPolicy::Any,
                    on_missing: MissingPolicy::ReportStale,
                    timeout_secs: 5,
                    auth_scope: None,
                }],
            },
            tool_policy: ToolPolicy::Disabled,
            output: OutputContract::default(),
            notify: NotifyPolicy::default(),
            controlled_actions: vec![],
        })
        .expect("create v4");
    let _ = rt.invoke(&v4_spec.id).await.expect("v4 fail 1");
    let rt_after_1 = rt.service.runtime(&v4_spec.id).expect("rt1");
    check(
        rt_after_1.status == AgentStatus::Active && rt_after_1.stats.consecutive_failures == 1,
        format!(
            "V4 after 1st failure stays Active (status={:?}, consecutive={})",
            rt_after_1.status, rt_after_1.stats.consecutive_failures
        ),
    );
    let _ = rt.invoke(&v4_spec.id).await.expect("v4 fail 2");
    let rt_after_2 = rt.service.runtime(&v4_spec.id).expect("rt2");
    check(
        rt_after_2.status == AgentStatus::Error && rt_after_2.next_probe_at.is_some(),
        format!(
            "V4 after 2nd failure trips Error with next_probe_at (status={:?}, probe={:?})",
            rt_after_2.status, rt_after_2.next_probe_at
        ),
    );

    // ── V5: Real AgentEventBridge triggers event agent asynchronously ──
    println!("\n=== V5: Real AgentEventBridge triggers event agent (sparkx2.6-1.7b) ===");
    telemetry
        .write(
            "device:cold-ev-01",
            "temperature",
            DataPoint {
                timestamp: now_secs,
                value: MetricValue::Float(9.5),
                quality: None,
                metadata: None,
            },
        )
        .await
        .expect("write cold-ev-01");
    let (v5_spec, _) = rt
        .create(AgentSpecDraft {
            name: "V5 Event Bridge Cold Room".into(),
            description: None,
            user_prompt: "冷库温度阈值 4°C，当前超过 4°C 必须告警。".into(),
            llm_backend_id: Some("llamacpp-spark".into()),
            preset: None,
            trigger: TriggerSpec::Event {
                filter: serde_json::json!({
                    "any": [{ "type": "device", "id": "cold-ev-01", "field": "temperature" }]
                }),
            },
            pacing: PacingPolicy::default(),
            input: InputContract {
                requirements: vec![InputRequirement {
                    key: "cold_temp".into(),
                    source: InputSourceRef::DeviceMetric {
                        device_id: "cold-ev-01".into(),
                        field: "temperature".into(),
                    },
                    operation: None,
                    params: serde_json::json!({}),
                    result_type: InputModality::Metric,
                    freshness: FreshnessPolicy::Any,
                    on_missing: MissingPolicy::ReportStale,
                    timeout_secs: 10,
                    auth_scope: None,
                }],
            },
            tool_policy: ToolPolicy::Disabled,
            output: OutputContract {
                publish: false,
                fields: vec![
                    OutputField {
                        name: "status".into(),
                        depends_on: vec!["cold_temp".into()],
                        description: Some("normal or alert".into()),
                    },
                    OutputField {
                        name: "summary".into(),
                        depends_on: vec!["cold_temp".into()],
                        description: Some("Short Chinese summary".into()),
                    },
                ],
            },
            notify: NotifyPolicy {
                channels: vec!["cold-alert".into()],
                on_complete: false,
                on_alert: true,
                alert_depends_on: vec!["cold_temp".into()],
                on_failure: false,
                pending_confirmation: false,
            },
            controlled_actions: vec![],
        })
        .expect("create v5");
    let bridge = AgentEventBridge::new(rt.clone());
    bridge
        .check_and_trigger("device", "cold-ev-01", "temperature", now_secs)
        .await;
    let mut v5_done = None;
    for _ in 0..80 {
        if let Some(exec) = rt.latest_formal(&v5_spec.id) {
            v5_done = Some(exec);
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    check(
        v5_done.is_some(),
        "V5 AgentEventBridge asynchronously executed event agent".into(),
    );
    if let Some(exec) = v5_done {
        println!(
            "  V5 exec: status={:?}, alert={:?}, structured={:?}",
            exec.status, exec.alert_verdict, exec.structured_result
        );
        check(
            exec.status == ExecutionStatus::Completed
                && exec.alert_verdict == Some(AlertVerdict::CriteriaMet),
            format!(
                "V5 event run Completed & CriteriaMet (status={:?}, alert={:?})",
                exec.status, exec.alert_verdict
            ),
        );
    }

    assert!(
        failures.is_empty(),
        "V1-V5 failures:\n{}",
        failures.join("\n")
    );
}

#[tokio::test]
#[ignore = "requires local Ollama (11434 qwen3.5:4b) and built target/debug/neomind binary"]
async fn live_chat_builds_and_manages_ai_agent_full_lifecycle() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mgr = setup_local_models(dir.path()).await;

    // 1. Boot a real in-process NeoMind API HTTP server on an ephemeral port.
    let state = ServerState::new_for_testing().await;
    let now_secs = chrono::Utc::now().timestamp();
    state
        .devices
        .telemetry
        .write(
            "device:cold_room_01",
            "temperature",
            DataPoint {
                timestamp: now_secs,
                value: MetricValue::Float(-8.5),
                quality: None,
                metadata: None,
            },
        )
        .await
        .expect("seed cold_room_01 telemetry");

    let (api_key, _key_info) = state
        .auth
        .api_key_state
        .create_key("live-chat-test".to_string(), vec!["admin".to_string()])
        .await;

    let app = create_router_with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local_addr");
    let server_handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    // Ensure `target/debug/neomind` is first on PATH for ShellTool.
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root");
    let debug_bin_dir = workspace_root.join("target/debug");
    assert!(
        debug_bin_dir.join("neomind").exists(),
        "Build CLI first: cargo build -p neomind-cli --bin neomind"
    );

    let orig_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{orig_path}", debug_bin_dir.display());
    std::env::set_var("PATH", &new_path);
    std::env::set_var("NEOMIND_API_BASE", format!("http://{addr}/api"));
    std::env::set_var("NEOMIND_API_KEY", &api_key);

    // 2. Build a real Chat Agent with ShellTool + real Ollama qwen3.5:4b runtime.
    let runtime = mgr
        .get_runtime("ollama-qwen35")
        .await
        .expect("ollama-qwen35 runtime");
    let mut reg = ToolRegistry::new();
    reg.register(Arc::new(ShellTool::new(ShellConfig {
        enabled: true,
        timeout_secs: 30,
        max_output_chars: 16_000,
    })));
    let system_prompt = format!(
        "你是 NeoMind 平台运维助手。你可以调用 `shell` 工具执行 `neomind` CLI 命令来创建、查询、调用、更新、暂停和删除 AI Agent。\n\
         常用命令：\n\
         - 创建：`neomind agent create --name '<名称>' --prompt '<任务提示>' --schedule-type interval --every 15m --execution-mode focused --device-ids <dev_id> --no-tools`\n\
         - 列表：`neomind agent list`\n\
         - 立即执行：`neomind agent invoke <id>`\n\
         - 更新为事件触发：`neomind agent update <id> --schedule-type event --event-filter '{{\"any\":[{{\"type\":\"device\",\"id\":\"cold_room_01\"}}]}}'`\n\
         - 发送备注消息：`neomind agent send-message <id> '<内容>'`\n\
         - 暂停/恢复：`neomind agent control <id> paused`\n\
         - 删除：`neomind agent delete <id>`\n\
         每次操作必须实际调用 `shell` 执行命令并基于真实输出回复，绝不编造 ID 或执行结果。"
    );

    let config = AgentConfig {
        name: "live-chat-manager".into(),
        system_prompt,
        max_context_tokens: 32768,
        temperature: 0.1,
        enable_tools: true,
        ..Default::default()
    };
    let chat = Agent::with_tools(config, "live-chat-session-1".into(), Arc::new(reg));
    chat.set_custom_llm(runtime).await;

    // Helper: consume the real production streaming ReAct loop (`process_stream_events`).
    async fn run_chat_turn(chat: &Agent, msg: &str) -> (String, Vec<String>) {
        use futures::StreamExt;
        use neomind_agent::agent::AgentEvent;
        let mut stream = chat
            .process_stream_events(msg, None, None)
            .await
            .expect("process_stream_events");
        let mut reply = String::new();
        let mut tools = Vec::new();
        while let Some(ev) = stream.next().await {
            match ev {
                AgentEvent::Content { content } => reply.push_str(&content),
                AgentEvent::ToolCallStart { tool, arguments, .. } => {
                    println!("    [tool_start] {tool} args={arguments}");
                    tools.push(tool);
                }
                AgentEvent::ToolCallEnd {
                    tool,
                    result,
                    success,
                    ..
                } => {
                    let preview: String = result.chars().take(240).collect();
                    println!("    [tool_end]   {tool} ok={success} -> {preview}");
                }
                AgentEvent::Error { message } => {
                    println!("    [chat_error] {message}");
                }
                _ => {}
            }
        }
        (reply, tools)
    }

    let mut failures: Vec<String> = Vec::new();
    let mut check = |ok: bool, msg: String| {
        if ok {
            println!("  [PASS] {msg}");
        } else {
            println!("  [FAIL] {msg}");
            failures.push(msg);
        }
    };

    // ── Turn 1: Create AI Agent via Chat ──
    println!("\n=== Chat Turn 1: Create '冷库巡检员' ===");
    let (reply1, tools1) = run_chat_turn(
        &chat,
        "帮我创建一个 AI Agent，名字叫「冷库巡检员」，每 15 分钟检查一次设备 cold_room_01 的温度，超过 -15°C 就告警，使用 focused 模式且不开启工具（--no-tools）。请立刻执行创建并告诉我它的 ID。",
    )
    .await;
    println!("  Chat reply 1:\n{reply1}");
    println!("  Tools called 1: {tools1:?}");

    let rt = state.require_clean_slate().await.expect("clean_slate");
    let agents_after_t1 = rt.list();
    check(
        agents_after_t1.len() == 1,
        format!(
            "Turn 1 created exactly 1 agent in backend (count={})",
            agents_after_t1.len()
        ),
    );
    let created_id = agents_after_t1
        .first()
        .map(|(s, _)| s.id.clone())
        .unwrap_or_default();
    if let Some((spec, _)) = agents_after_t1.first() {
        println!(
            "  Created AgentSpec: id={}, name={}, trigger={:?}, inputs={:?}, tool_policy={:?}",
            spec.id, spec.name, spec.trigger, spec.input.requirements, spec.tool_policy
        );
        check(
            matches!(spec.trigger, TriggerSpec::Interval { every_secs: 900 }),
            format!("Turn 1 trigger is Interval(900s) (got {:?})", spec.trigger),
        );
        check(
            !spec.input.requirements.is_empty(),
            format!(
                "Turn 1 bound cold_room_01 input (count={})",
                spec.input.requirements.len()
            ),
        );
    }

    // ── Turn 2: List & Invoke created Agent via Chat ──
    println!("\n=== Chat Turn 2: List & Invoke ===");
    let (reply2, tools2) = run_chat_turn(
        &chat,
        &format!(
            "请查看当前的 AI Agent 列表，并对刚刚创建的冷库巡检员（ID: {created_id}）执行一次 invoke，告诉我实际的巡检结论。"
        ),
    )
    .await;
    println!("  Chat reply 2:\n{reply2}");
    println!("  Tools called 2: {tools2:?}");
    check(
        !tools2.is_empty(),
        format!("Turn 2 called shell tool (tools={tools2:?})"),
    );
    let latest_exec = rt.latest_formal(&created_id);
    check(
        latest_exec.is_some(),
        format!(
            "Turn 2 actually executed agent {created_id} in backend (exec={:?})",
            latest_exec.as_ref().map(|e| (&e.id, &e.status))
        ),
    );

    // ── Turn 3: Update to Event trigger + Send operator message ──
    println!("\n=== Chat Turn 3: Update to Event trigger & Send operator message ===");
    let (reply3, tools3) = run_chat_turn(
        &chat,
        &format!(
            "请把冷库巡检员（ID: {created_id}）更新为事件触发（--schedule-type event，监听设备 cold_room_01），并给它发送一条备注消息：重点关注凌晨化霜周期。"
        ),
    )
    .await;
    println!("  Chat reply 3:\n{reply3}");
    println!("  Tools called 3: {tools3:?}");

    if let Ok((updated_spec, _)) = rt.get(&created_id) {
        println!("  Updated trigger: {:?}", updated_spec.trigger);
        let event_ok = match &updated_spec.trigger {
            TriggerSpec::Event { filter } => {
                // Verify P0-4 fix: filter must NOT be a raw JSON string that breaks EventFilter!
                let parsed: neomind_storage::agents::EventFilter =
                    serde_json::from_value(filter.clone()).unwrap_or_default();
                !filter.is_string() && (!parsed.any.is_empty() || !parsed.all.is_empty())
            }
            _ => false,
        };
        check(
            event_ok,
            format!(
                "Turn 3 updated trigger to valid EventFilter object (got {:?})",
                updated_spec.trigger
            ),
        );
    } else {
        check(false, "Turn 3 agent still exists after update".into());
    }
    let user_msgs = state
        .agents
        .agent_store
        .get_user_messages(&created_id, None)
        .await
        .unwrap_or_default();
    check(
        user_msgs.iter().any(|m| m.content.contains("化霜")),
        format!("Turn 3 persisted operator note (count={})", user_msgs.len()),
    );

    // ── Turn 4: Pause & Delete lifecycle ──
    println!("\n=== Chat Turn 4: Pause & Delete ===");
    let (reply4, tools4) = run_chat_turn(
        &chat,
        &format!(
            "先暂停冷库巡检员（ID: {created_id}），然后再删除它，最后确认删除结果。"
        ),
    )
    .await;
    println!("  Chat reply 4:\n{reply4}");
    println!("  Tools called 4: {tools4:?}");
    check(
        rt.list().is_empty(),
        format!(
            "Turn 4 cleanly deleted agent from backend (remaining={})",
            rt.list().len()
        ),
    );

    server_handle.abort();
    assert!(
        failures.is_empty(),
        "Chat-manages-Agent failures:\n{}",
        failures.join("\n")
    );
}
