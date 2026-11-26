//! Opt-in live E2E harness exercising real `HostedAiAgentRuntime` against local
//! llama.cpp and Ollama models (`sparkx2.6-1.7b`, `minicpm-v-4.7-1b`,
//! `qwen3.5:4b`, `lfm2.5-2.6b:latest`).
//!
//! Run manually with:
//! `cargo test -p neomind-api --test live_agent_e2e -- --ignored --nocapture`

use std::sync::Arc;

use base64::Engine as _;
use neomind_agent::llm_backends::instance_manager::{
    set_instance_manager, LlmBackendInstanceManager,
};
use neomind_agent::toolkit::shell::{ShellConfig, ShellTool};
use neomind_agent::toolkit::{
    FileEditTool, FileReadTool, FileWriteTool, ToolRegistry, WebFetchTool, WebSearchTool,
};
use neomind_agent::SessionManager;
use neomind_ai_agent::model::{
    AgentSpecDraft, AlertVerdict, CallerInput, ControlledAction, DeliveryState, ExecutionStatus,
    FreshnessPolicy, InputContract, InputModality, InputRequirement, InputSourceRef, MissingPolicy,
    NotifyPolicy, OutputContract, OutputField, PacingPolicy, ToolPolicy, TriggerSpec,
};
use neomind_api::agent_ports::HostAgentDeps;
use neomind_api::ai_agent_runtime::HostedAiAgentRuntime;
use neomind_api::automation::metric_publish::VirtualMetricPublisher;
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
    thinking: bool,
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
        thinking_is_integral: model.contains("lfm2.5"),
        temperature: 0.2,
        top_p: 0.9,
        max_tokens: 1024,
        top_k: 40,
        thinking_enabled: false,
        thinking_effort: None,
        capabilities: BackendCapabilities {
            supports_streaming: true,
            supports_multimodal: multimodal,
            supports_thinking: thinking,
            supports_tools: tools,
            max_context,
            ..Default::default()
        },
        updated_at: chrono::Utc::now().timestamp(),
    }
}

async fn setup_live_runtime() -> (tempfile::TempDir, Arc<HostedAiAgentRuntime>, Arc<TimeSeriesStorage>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let llm_store = LlmBackendStore::open(dir.path().join("llm_backends.redb")).expect("llm store");
    let mgr = Arc::new(LlmBackendInstanceManager::new(llm_store));

    // Register local llama.cpp + Ollama models currently running on this host.
    let mut spark = make_instance(
        "llamacpp-spark",
        LlmBackendType::LlamaCpp,
        "http://127.0.0.1:29375",
        "sparkx2.6-1.7b",
        false,
        true,
        false,
        131072,
    );
    spark.is_active = true;
    mgr.upsert_instance(spark).await.expect("upsert spark");

    let minicpm = make_instance(
        "llamacpp-minicpm-v",
        LlmBackendType::LlamaCpp,
        "http://127.0.0.1:29385",
        "minicpm-v-4.7-1b",
        true,
        false,
        false,
        32768,
    );
    mgr.upsert_instance(minicpm).await.expect("upsert minicpm");

    let qwen = make_instance(
        "ollama-qwen35",
        LlmBackendType::Ollama,
        "http://127.0.0.1:11434",
        "qwen3.5:4b",
        true,
        true,
        true,
        262144,
    );
    mgr.upsert_instance(qwen).await.expect("upsert qwen");

    let lfm = make_instance(
        "ollama-lfm25",
        LlmBackendType::Ollama,
        "http://127.0.0.1:11434",
        "lfm2.5-2.6b:latest",
        false,
        true,
        true,
        131072,
    );
    mgr.upsert_instance(lfm).await.expect("upsert lfm");

    set_instance_manager(mgr);

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

    let mem_dir = dir.path().join("memory");
    let mem_store = Arc::new(MarkdownMemoryStore::with_config(
        &mem_dir,
        MemoryConfig::default(),
    ));
    let _ = mem_store.init();

    let session_manager = Arc::new(SessionManager::memory());
    let mut tool_reg = ToolRegistry::new();
    tool_reg.register(Arc::new(ShellTool::new(ShellConfig::default())));
    tool_reg.register(Arc::new(WebSearchTool::new()));
    tool_reg.register(Arc::new(WebFetchTool::new()));
    tool_reg.register(Arc::new(FileReadTool::new(dir.path().to_path_buf())));
    tool_reg.register(Arc::new(FileWriteTool::new(dir.path().to_path_buf())));
    tool_reg.register(Arc::new(FileEditTool::new(dir.path().to_path_buf())));
    session_manager.set_tool_registry(Arc::new(tool_reg)).await;

    let deps = HostAgentDeps::new(mm, telemetry.clone(), device_service, ext_reg, ext_metrics)
        .with_event_bus(bus)
        .with_metric_publisher(publisher)
        .with_memory_store(mem_store)
        .with_session_manager(session_manager);
    let agent_store = AgentStore::memory().expect("agent store");
    let runtime = Arc::new(HostedAiAgentRuntime::build(agent_store, deps).expect("build runtime"));
    (dir, runtime, telemetry)
}

fn load_camera_data_url() -> String {
    let sample_path = "/Users/shenmingming/Library/Application Support/com.neomind.neomind/data/images/9999/values.image/1740640441_6266eb4b07905d8e8ed9de55ef4240c5.jpg";
    if let Ok(bytes) = std::fs::read(sample_path) {
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        return format!("data:image/jpeg;base64,{b64}");
    }
    // Fallback tiny 1x1 red PNG
    "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==".into()
}

fn print_exec(label: &str, resp: &neomind_ai_agent::model::RunResponse, explain: &str) {
    println!("\n==================== {label} ====================");
    if let Some(exec) = &resp.execution {
        println!("status      : {:?}", exec.status);
        println!("alert       : {:?}", exec.alert_verdict);
        println!("error       : {:?}", exec.error);
        println!("structured  : {:?}", exec.structured_result);
        println!("conclusion  : {}", exec.conclusion.as_deref().unwrap_or("<none>"));
    } else {
        println!("(no execution record)");
    }
    println!("--- explain_last ---\n{explain}");
}

#[tokio::test]
#[ignore = "Requires live local llama.cpp and Ollama servers"]
async fn live_multi_model_agent_matrix() {
    let (_dir, rt, telemetry) = setup_live_runtime().await;
    let now = chrono::Utc::now().timestamp();

    // Seed sensor telemetry
    telemetry
        .write("device:sensor-01", "temperature", DataPoint::new(now, MetricValue::Float(36.8)))
        .await
        .unwrap();
    telemetry
        .write("device:sensor-01", "humidity", DataPoint::new(now, MetricValue::Float(72.0)))
        .await
        .unwrap();

    // Seed camera telemetry with LARGE base64 image in BOTH `image` AND `_raw`
    let data_url = load_camera_data_url();
    let raw_payload = serde_json::json!({
        "devName": "NE101 Sensing Camera",
        "battery": 84,
        "image": data_url,
    })
    .to_string();
    telemetry
        .write("device:cam-9999", "values.battery", DataPoint::new(now, MetricValue::Integer(84)))
        .await
        .unwrap();
    telemetry
        .write("device:cam-9999", "values.image", DataPoint::new(now, MetricValue::String(data_url)))
        .await
        .unwrap();
    telemetry
        .write("device:cam-9999", "_raw", DataPoint::new(now, MetricValue::String(raw_payload)))
        .await
        .unwrap();

    // 1A. sparkx2.6-1.7b — tool whitelist (shell), no pre-bound input sources
    let (spec_1a, _) = rt
        .create(AgentSpecDraft {
            name: "E2E-1A Spark Shell Tool".into(),
            description: None,
            user_prompt: "请调用 shell 工具执行 `uname -s` 检查当前操作系统类型，然后给出简短中文总结，并按要求输出 JSON 字段 os 和 summary。".into(),
            llm_backend_id: Some("llamacpp-spark".into()),
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract::default(),
            tool_policy: ToolPolicy::Scoped {
                allowed_tools: vec!["shell".into()],
                allowed_commands: vec![],
            },
            output: OutputContract {
                publish: false,
                fields: vec![
                    OutputField { name: "os".into(), depends_on: vec![], description: Some("Operating system name from uname -s".into()) },
                    OutputField { name: "summary".into(), depends_on: vec![], description: Some("Brief Chinese summary".into()) },
                ],
            },
            notify: NotifyPolicy::default(),
            controlled_actions: vec![],
        })
        .expect("create 1a");
    let resp_1a = rt.invoke(&spec_1a.id).await.expect("invoke 1a");
    let exp_1a = rt.explain_last(&spec_1a.id).unwrap_or_default();
    print_exec("1A: sparkx2.6-1.7b (shell tool)", &resp_1a, &exp_1a);

    // 1B. sparkx2.6-1.7b — device telemetry alert + structured output (zero tools)
    let (spec_1b, _) = rt
        .create(AgentSpecDraft {
            name: "E2E-1B Spark Sensor Alert".into(),
            description: None,
            user_prompt: "检查温度传感器数据。如果温度超过 30°C，则视为异常并满足告警条件（末尾必须写 Verdict: criteria met），并输出 JSON 字段 status 和 summary。".into(),
            llm_backend_id: Some("llamacpp-spark".into()),
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract {
                requirements: vec![InputRequirement {
                    key: "sensor_01".into(),
                    source: InputSourceRef::WholeDevice { device_id: "sensor-01".into() },
                    operation: None,
                    params: serde_json::json!({}),
                    result_type: InputModality::StructuredJson,
                    freshness: FreshnessPolicy::Any,
                    on_missing: MissingPolicy::ReportStale,
                    timeout_secs: 15,
                    auth_scope: None,
                }],
            },
            tool_policy: ToolPolicy::Disabled,
            output: OutputContract {
                publish: false,
                fields: vec![
                    OutputField { name: "status".into(), depends_on: vec![], description: Some("normal or alert".into()) },
                    OutputField { name: "summary".into(), depends_on: vec![], description: Some("Concise summary with temperature value".into()) },
                ],
            },
            notify: NotifyPolicy {
                on_alert: true,
                ..Default::default()
            },
            controlled_actions: vec![],
        })
        .expect("create 1b");
    let resp_1b = rt.invoke(&spec_1b.id).await.expect("invoke 1b");
    let exp_1b = rt.explain_last(&spec_1b.id).unwrap_or_default();
    print_exec("1B: sparkx2.6-1.7b (sensor alert)", &resp_1b, &exp_1b);

    // 2A. minicpm-v-4.7-1b — camera vision with large base64 in WholeDevice + _raw
    let (spec_2a, _) = rt
        .create(AgentSpecDraft {
            name: "E2E-2A MiniCPM-V Camera".into(),
            description: None,
            user_prompt: "请分析摄像头拍摄的现场画面和设备电量，判断画面是否正常，并输出 JSON 字段 status 和 summary。".into(),
            llm_backend_id: Some("llamacpp-minicpm-v".into()),
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract {
                requirements: vec![InputRequirement {
                    key: "cam_9999".into(),
                    source: InputSourceRef::WholeDevice { device_id: "cam-9999".into() },
                    operation: None,
                    params: serde_json::json!({}),
                    result_type: InputModality::StructuredJson,
                    freshness: FreshnessPolicy::Any,
                    on_missing: MissingPolicy::ReportStale,
                    timeout_secs: 15,
                    auth_scope: None,
                }],
            },
            tool_policy: ToolPolicy::Disabled,
            output: OutputContract {
                publish: false,
                fields: vec![
                    OutputField { name: "status".into(), depends_on: vec![], description: Some("Normal or abnormal".into()) },
                    OutputField { name: "summary".into(), depends_on: vec![], description: Some("Key visual observations in Chinese".into()) },
                ],
            },
            notify: NotifyPolicy::default(),
            controlled_actions: vec![],
        })
        .expect("create 2a");
    let resp_2a = rt.invoke(&spec_2a.id).await.expect("invoke 2a");
    let exp_2a = rt.explain_last(&spec_2a.id).unwrap_or_default();
    print_exec("2A: minicpm-v-4.7-1b (camera vision + large base64)", &resp_2a, &exp_2a);

    // 3A. ollama qwen3.5:4b — tool whitelist (shell) + structured JSON
    let (spec_3a, _) = rt
        .create(AgentSpecDraft {
            name: "E2E-3A Qwen3.5 Shell Tool".into(),
            description: None,
            user_prompt: "请调用 shell 工具运行 `echo NEOMIND_QWEN_OK`，把命令输出填入 result 字段，并用中文在 summary 字段总结。".into(),
            llm_backend_id: Some("ollama-qwen35".into()),
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract::default(),
            tool_policy: ToolPolicy::Scoped {
                allowed_tools: vec!["shell".into()],
                allowed_commands: vec![],
            },
            output: OutputContract {
                publish: false,
                fields: vec![
                    OutputField { name: "result".into(), depends_on: vec![], description: Some("Exact stdout from shell command".into()) },
                    OutputField { name: "summary".into(), depends_on: vec![], description: Some("Brief Chinese summary".into()) },
                ],
            },
            notify: NotifyPolicy::default(),
            controlled_actions: vec![],
        })
        .expect("create 3a");
    let resp_3a = rt.invoke(&spec_3a.id).await.expect("invoke 3a");
    let exp_3a = rt.explain_last(&spec_3a.id).unwrap_or_default();
    print_exec("3A: ollama qwen3.5:4b (shell tool)", &resp_3a, &exp_3a);

    // 3B. ollama qwen3.5:4b — vision camera inspection
    let (spec_3b, _) = rt
        .create(AgentSpecDraft {
            name: "E2E-3B Qwen3.5 Vision".into(),
            description: None,
            user_prompt: "请描述摄像头画面中的场景，判断是否正常，输出 JSON 字段 status 和 summary。".into(),
            llm_backend_id: Some("ollama-qwen35".into()),
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract {
                requirements: vec![InputRequirement {
                    key: "cam_9999".into(),
                    source: InputSourceRef::WholeDevice { device_id: "cam-9999".into() },
                    operation: None,
                    params: serde_json::json!({}),
                    result_type: InputModality::StructuredJson,
                    freshness: FreshnessPolicy::Any,
                    on_missing: MissingPolicy::ReportStale,
                    timeout_secs: 15,
                    auth_scope: None,
                }],
            },
            tool_policy: ToolPolicy::Disabled,
            output: OutputContract {
                publish: false,
                fields: vec![
                    OutputField { name: "status".into(), depends_on: vec![], description: Some("Normal or abnormal".into()) },
                    OutputField { name: "summary".into(), depends_on: vec![], description: Some("Visual observation in Chinese".into()) },
                ],
            },
            notify: NotifyPolicy::default(),
            controlled_actions: vec![],
        })
        .expect("create 3b");
    let resp_3b = rt.invoke(&spec_3b.id).await.expect("invoke 3b");
    let exp_3b = rt.explain_last(&spec_3b.id).unwrap_or_default();
    print_exec("3B: ollama qwen3.5:4b (camera vision)", &resp_3b, &exp_3b);

    // 4A. ollama lfm2.5-2.6b:latest — tool whitelist (shell) + structured JSON
    let (spec_4a, _) = rt
        .create(AgentSpecDraft {
            name: "E2E-4A LFM2.5 Shell Tool".into(),
            description: None,
            user_prompt: "请调用 shell 工具运行 `echo NEOMIND_LFM_OK`，把输出填入 result 字段，并用中文在 summary 字段总结。".into(),
            llm_backend_id: Some("ollama-lfm25".into()),
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract::default(),
            tool_policy: ToolPolicy::Scoped {
                allowed_tools: vec!["shell".into()],
                allowed_commands: vec![],
            },
            output: OutputContract {
                publish: false,
                fields: vec![
                    OutputField { name: "result".into(), depends_on: vec![], description: Some("Exact stdout from shell command".into()) },
                    OutputField { name: "summary".into(), depends_on: vec![], description: Some("Brief Chinese summary".into()) },
                ],
            },
            notify: NotifyPolicy::default(),
            controlled_actions: vec![],
        })
        .expect("create 4a");
    let resp_4a = rt.invoke(&spec_4a.id).await.expect("invoke 4a");
    let exp_4a = rt.explain_last(&spec_4a.id).unwrap_or_default();
    print_exec("4A: ollama lfm2.5-2.6b (shell tool)", &resp_4a, &exp_4a);

    // Closed-loop quality assertions across all 6 live runs:
    for (name, resp) in [
        ("1A", &resp_1a),
        ("1B", &resp_1b),
        ("2A", &resp_2a),
        ("3A", &resp_3a),
        ("3B", &resp_3b),
        ("4A", &resp_4a),
    ] {
        let exec = resp.execution.as_ref().expect("execution record");
        assert_eq!(
            exec.status,
            neomind_ai_agent::model::ExecutionStatus::Completed,
            "{name} should complete cleanly, error={:?}",
            exec.error
        );
        let sr = exec
            .structured_result
            .as_ref()
            .and_then(|v| v.as_object())
            .expect("structured_result should be populated");
        assert!(!sr.is_empty(), "{name} structured_result must be non-empty");
        let concl = exec.conclusion.as_deref().unwrap_or("");
        assert!(!concl.is_empty(), "{name} conclusion must be non-empty");
        assert!(
            !concl.contains("<tool_call>") && !concl.contains("<json>") && !concl.contains("```json"),
            "{name} conclusion leaked raw protocol tags: {concl}"
        );
    }

    // Verify tool outputs actually came from real shell execution:
    assert_eq!(
        resp_1a.execution.as_ref().unwrap().structured_result.as_ref().unwrap()["os"],
        "Darwin"
    );
    assert!(
        resp_3a.execution.as_ref().unwrap().structured_result.as_ref().unwrap()["result"]
            .as_str()
            .unwrap_or("")
            .contains("NEOMIND_QWEN_OK")
    );
    assert!(
        resp_4a.execution.as_ref().unwrap().structured_result.as_ref().unwrap()["result"]
            .as_str()
            .unwrap_or("")
            .contains("NEOMIND_LFM_OK")
    );
    // Verify alert precision (1B alert = CriteriaMet; 3B & 4A normal = CriteriaNotMet):
    assert_eq!(
        resp_1b.execution.as_ref().unwrap().alert_verdict,
        Some(neomind_ai_agent::model::AlertVerdict::CriteriaMet),
        "1B (36.8°C > 30°C) must trigger CriteriaMet"
    );
    assert_eq!(
        resp_3b.execution.as_ref().unwrap().alert_verdict,
        Some(neomind_ai_agent::model::AlertVerdict::CriteriaNotMet),
        "3B normal camera scene must NOT false-alert"
    );
    assert_eq!(
        resp_4a.execution.as_ref().unwrap().alert_verdict,
        Some(neomind_ai_agent::model::AlertVerdict::CriteriaNotMet),
        "4A non-alerting shell task must NOT false-alert"
    );
}

#[tokio::test]
#[ignore = "Requires live local llama.cpp and Ollama servers"]
async fn live_business_scenarios_stress_matrix() {
    let (dir, rt, telemetry) = setup_live_runtime().await;
    let now = chrono::Utc::now().timestamp();

    // ── Seed realistic multi-device telemetry ─────────────────────────────
    // B1: Cold room abnormal (8.4°C, door open) & normal (2.1°C, door closed)
    telemetry
        .write("device:cold-room-hot", "temperature", DataPoint::new(now, MetricValue::Float(8.4)))
        .await
        .unwrap();
    telemetry
        .write("device:cold-room-hot", "humidity", DataPoint::new(now, MetricValue::Float(82.0)))
        .await
        .unwrap();
    telemetry
        .write("device:cold-room-hot", "door_open", DataPoint::new(now, MetricValue::Boolean(true)))
        .await
        .unwrap();
    telemetry
        .write("device:cold-room-ok", "temperature", DataPoint::new(now, MetricValue::Float(2.1)))
        .await
        .unwrap();
    telemetry
        .write("device:cold-room-ok", "door_open", DataPoint::new(now, MetricValue::Boolean(false)))
        .await
        .unwrap();

    // B2: Factory meter 12-point lookback series ramping 42.5 -> 98.6 kW over last 30m
    let series_vals = [42.5, 46.0, 51.2, 58.0, 64.5, 71.0, 77.3, 82.1, 86.4, 91.2, 95.8, 98.6];
    for (idx, kw) in series_vals.iter().enumerate() {
        let ts = now - ((series_vals.len() - 1 - idx) as i64) * 150;
        telemetry
            .write("device:factory-meter-01", "power_kw", DataPoint::new(ts, MetricValue::Float(*kw)))
            .await
            .unwrap();
    }

    // B3: Greenhouse — fresh temp (34.2°C) + STALE soil moisture (2 hours ago)
    telemetry
        .write("device:gh-sensor-1", "temperature", DataPoint::new(now - 30, MetricValue::Float(34.2)))
        .await
        .unwrap();
    telemetry
        .write("device:gh-sensor-2", "moisture", DataPoint::new(now - 7200, MetricValue::Float(18.0)))
        .await
        .unwrap();

    // B4: Plaza camera (176KB base64) + environment sensor for upstream -> downstream chaining
    let data_url = load_camera_data_url();
    telemetry
        .write("device:plaza-cam-01", "values.image", DataPoint::new(now, MetricValue::String(data_url)))
        .await
        .unwrap();
    telemetry
        .write("device:plaza-cam-01", "values.battery", DataPoint::new(now, MetricValue::Integer(84)))
        .await
        .unwrap();
    telemetry
        .write("device:plaza-env-01", "pm25", DataPoint::new(now, MetricValue::Float(28.0)))
        .await
        .unwrap();
    telemetry
        .write("device:plaza-env-01", "noise_db", DataPoint::new(now, MetricValue::Float(54.0)))
        .await
        .unwrap();

    // B5b: Seed a real CSV file inside data_dir for multi-step file_read + file_write task
    std::fs::write(
        dir.path().join("site_log.csv"),
        "device_id,status,error\nsensor-A,ok,\nsensor-B,fail,timeout\nsensor-C,ok,\nsensor-D,fail,crc_error\nsensor-E,fail,offline\n",
    )
    .expect("write site_log.csv");

    let mut failures: Vec<String> = Vec::new();
    let mut check = |cond: bool, msg: String| {
        if !cond {
            println!("  [FAIL] {msg}");
            failures.push(msg);
        } else {
            println!("  [PASS] {msg}");
        }
    };

    // ── B1a: Cold room over-temp (8.4°C) -> Alert + Notify + Controlled Action ──
    let (spec_b1a, _) = rt
        .create(AgentSpecDraft {
            name: "B1a Cold Room Over-Temp".into(),
            description: None,
            user_prompt: "冷库标准温度应 ≤ 4°C。检查冷库传感器数据，若温度超过 4°C 或冷库门打开，判定为告警(status=\"alert\")，并说明风险与建议措施。".into(),
            llm_backend_id: Some("llamacpp-spark".into()),
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract {
                requirements: vec![InputRequirement {
                    key: "cold_room".into(),
                    source: InputSourceRef::WholeDevice { device_id: "cold-room-hot".into() },
                    operation: None,
                    params: serde_json::json!({}),
                    result_type: InputModality::StructuredJson,
                    freshness: FreshnessPolicy::LatestWithin { max_age_secs: 300 },
                    on_missing: MissingPolicy::ReportStale,
                    timeout_secs: 15,
                    auth_scope: None,
                }],
            },
            tool_policy: ToolPolicy::Disabled,
            output: OutputContract {
                publish: true,
                fields: vec![
                    OutputField { name: "status".into(), depends_on: vec!["cold_room".into()], description: Some("normal or alert".into()) },
                    OutputField { name: "risk_level".into(), depends_on: vec!["cold_room".into()], description: Some("low, medium, or high".into()) },
                    OutputField { name: "summary".into(), depends_on: vec!["cold_room".into()], description: Some("Chinese summary with temperature and door state".into()) },
                ],
            },
            notify: NotifyPolicy {
                channels: vec!["ops-alert".into()],
                on_complete: false,
                on_alert: true,
                alert_depends_on: vec!["cold_room".into()],
                on_failure: false,
                pending_confirmation: false,
            },
            controlled_actions: vec![ControlledAction {
                name: "boost_cooling".into(),
                destination: "device:cold-room-fan".into(),
                params: serde_json::json!({ "power": "high" }),
                idempotent: true,
                depends_on: vec!["cold_room".into()],
            }],
        })
        .expect("create b1a");

    // B6a FIRST: Dry-run on B1a MUST produce zero notifications and zero side-effect writes!
    let notify_before = rt.notify.deliveries();
    let writes_before = rt.side_effects.invokes();
    let dry_resp = rt.dry_run(&spec_b1a.id).await.expect("dry_run b1a");
    print_exec("B6a: dry_run cold room over-temp (must be zero side effects)", &dry_resp, "");
    check(dry_resp.dry_run, "B6a dry_run flag is true".into());
    check(rt.notify.deliveries() == notify_before, "B6a dry_run sent 0 notifications".into());
    check(rt.side_effects.invokes() == writes_before, "B6a dry_run executed 0 device writes".into());
    check(rt.service.formal_history_len(&spec_b1a.id) == 0, "B6a dry_run wrote 0 formal history rows".into());

    // Now formal invoke B1a
    let resp_b1a = rt.invoke(&spec_b1a.id).await.expect("invoke b1a");
    let exp_b1a = rt.explain_last(&spec_b1a.id).unwrap_or_default();
    print_exec("B1a: Cold room 8.4°C over-temp (formal)", &resp_b1a, &exp_b1a);
    let exec_b1a = resp_b1a.execution.as_ref().unwrap();
    check(exec_b1a.status == ExecutionStatus::Completed, format!("B1a status=Completed (got {:?})", exec_b1a.status));
    check(exec_b1a.alert_verdict == Some(AlertVerdict::CriteriaMet), format!("B1a alert=CriteriaMet (got {:?})", exec_b1a.alert_verdict));
    check(rt.notify.deliveries() > notify_before, format!("B1a delivered alert notify (count={})", rt.notify.deliveries() - notify_before));
    check(!exec_b1a.deliveries.is_empty(), format!("B1a recorded delivery receipts (len={})", exec_b1a.deliveries.len()));

    // ── B1b: Cold room normal (2.1°C, door closed) -> NO alert, NO notify, NO action ──
    let notify_before_b1b = rt.notify.deliveries();
    let writes_before_b1b = rt.side_effects.invokes();
    let (spec_b1b, _) = rt
        .create(AgentSpecDraft {
            name: "B1b Cold Room Normal".into(),
            description: None,
            user_prompt: "冷库标准温度应 ≤ 4°C。检查冷库传感器数据，若温度超过 4°C 或冷库门打开则告警，否则判定正常(status=\"normal\")。".into(),
            llm_backend_id: Some("llamacpp-spark".into()),
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract {
                requirements: vec![InputRequirement {
                    key: "cold_room".into(),
                    source: InputSourceRef::WholeDevice { device_id: "cold-room-ok".into() },
                    operation: None,
                    params: serde_json::json!({}),
                    result_type: InputModality::StructuredJson,
                    freshness: FreshnessPolicy::LatestWithin { max_age_secs: 300 },
                    on_missing: MissingPolicy::ReportStale,
                    timeout_secs: 15,
                    auth_scope: None,
                }],
            },
            tool_policy: ToolPolicy::Disabled,
            output: OutputContract {
                publish: false,
                fields: vec![
                    OutputField { name: "status".into(), depends_on: vec!["cold_room".into()], description: Some("normal or alert".into()) },
                    OutputField { name: "summary".into(), depends_on: vec!["cold_room".into()], description: Some("Chinese summary".into()) },
                ],
            },
            notify: NotifyPolicy {
                channels: vec!["ops-alert".into()],
                on_complete: false,
                on_alert: true,
                alert_depends_on: vec!["cold_room".into()],
                on_failure: false,
                pending_confirmation: false,
            },
            controlled_actions: vec![ControlledAction {
                name: "boost_cooling".into(),
                destination: "device:cold-room-fan".into(),
                params: serde_json::json!({ "power": "high" }),
                idempotent: true,
                depends_on: vec!["cold_room".into()],
            }],
        })
        .expect("create b1b");
    let resp_b1b = rt.invoke(&spec_b1b.id).await.expect("invoke b1b");
    let exp_b1b = rt.explain_last(&spec_b1b.id).unwrap_or_default();
    print_exec("B1b: Cold room 2.1°C normal", &resp_b1b, &exp_b1b);
    let exec_b1b = resp_b1b.execution.as_ref().unwrap();
    check(exec_b1b.alert_verdict == Some(AlertVerdict::CriteriaNotMet), format!("B1b normal must NOT alert (got {:?})", exec_b1b.alert_verdict));
    check(rt.notify.deliveries() == notify_before_b1b, "B1b normal sent 0 notifications".into());
    check(rt.side_effects.invokes() == writes_before_b1b, "B1b normal executed 0 device actions".into());

    // ── B2: Factory meter LookbackWindow time-series peak analysis ──
    let (spec_b2, _) = rt
        .create(AgentSpecDraft {
            name: "B2 Factory Power Lookback".into(),
            description: None,
            user_prompt: "分析工厂电表过去 1 小时的功率时序数据（power_kw）。计算最高峰值、判断变化趋势（上升/下降/平稳），如果峰值超过 90 kW 判定为告警(status=\"alert\")。".into(),
            llm_backend_id: Some("ollama-qwen35".into()),
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract {
                requirements: vec![InputRequirement {
                    key: "meter_series".into(),
                    source: InputSourceRef::DeviceMetric { device_id: "factory-meter-01".into(), field: "power_kw".into() },
                    operation: None,
                    params: serde_json::json!({}),
                    result_type: InputModality::TimeSeries,
                    freshness: FreshnessPolicy::LookbackWindow { lookback_secs: 3600, max_points: 60 },
                    on_missing: MissingPolicy::ReportStale,
                    timeout_secs: 15,
                    auth_scope: None,
                }],
            },
            tool_policy: ToolPolicy::Disabled,
            output: OutputContract {
                publish: true,
                fields: vec![
                    OutputField { name: "peak_kw".into(), depends_on: vec!["meter_series".into()], description: Some("Peak kW numeric value".into()) },
                    OutputField { name: "trend".into(), depends_on: vec!["meter_series".into()], description: Some("rising, falling, or stable".into()) },
                    OutputField { name: "status".into(), depends_on: vec!["meter_series".into()], description: Some("normal or alert".into()) },
                    OutputField { name: "summary".into(), depends_on: vec!["meter_series".into()], description: Some("Brief Chinese analysis".into()) },
                ],
            },
            notify: NotifyPolicy {
                on_alert: true,
                ..Default::default()
            },
            controlled_actions: vec![],
        })
        .expect("create b2");
    let resp_b2 = rt.invoke(&spec_b2.id).await.expect("invoke b2");
    let exp_b2 = rt.explain_last(&spec_b2.id).unwrap_or_default();
    print_exec("B2: Factory meter LookbackWindow (12 pts, peak 98.6kW)", &resp_b2, &exp_b2);
    let exec_b2 = resp_b2.execution.as_ref().unwrap();
    check(exec_b2.status == ExecutionStatus::Completed, format!("B2 LookbackWindow Completed (got {:?})", exec_b2.status));
    check(exec_b2.alert_verdict == Some(AlertVerdict::CriteriaMet), format!("B2 peak 98.6 > 90 triggers alert (got {:?})", exec_b2.alert_verdict));
    check(exec_b2.structured_result.as_ref().and_then(|v| v.get("peak_kw")).is_some(), format!("B2 extracted peak_kw ({:?})", exec_b2.structured_result));

    // ── B3: Greenhouse Partial source (fresh temp + stale soil) + depends_on gating ──
    let (spec_b3, _) = rt
        .create(AgentSpecDraft {
            name: "B3 Greenhouse Partial & DependsOn Gate".into(),
            description: None,
            user_prompt: "检查温室温度（超过 30°C 告警）与土壤湿度。如果温度超标触发告警；对于缺失或过期的数据源必须标注依据不足。".into(),
            llm_backend_id: Some("ollama-qwen35".into()),
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract {
                requirements: vec![
                    InputRequirement {
                        key: "gh_temp".into(),
                        source: InputSourceRef::DeviceMetric { device_id: "gh-sensor-1".into(), field: "temperature".into() },
                        operation: None,
                        params: serde_json::json!({}),
                        result_type: InputModality::Metric,
                        freshness: FreshnessPolicy::LatestWithin { max_age_secs: 300 },
                        on_missing: MissingPolicy::ReportStale,
                        timeout_secs: 15,
                        auth_scope: None,
                    },
                    InputRequirement {
                        key: "gh_soil".into(),
                        source: InputSourceRef::DeviceMetric { device_id: "gh-sensor-2".into(), field: "moisture".into() },
                        operation: None,
                        params: serde_json::json!({}),
                        result_type: InputModality::Metric,
                        freshness: FreshnessPolicy::LatestWithin { max_age_secs: 300 },
                        on_missing: MissingPolicy::ReportStale,
                        timeout_secs: 15,
                        auth_scope: None,
                    },
                ],
            },
            tool_policy: ToolPolicy::Disabled,
            output: OutputContract {
                publish: false,
                fields: vec![
                    OutputField { name: "status".into(), depends_on: vec!["gh_temp".into()], description: Some("normal or alert".into()) },
                    OutputField { name: "summary".into(), depends_on: vec![], description: Some("Chinese summary noting fresh temp and stale soil".into()) },
                ],
            },
            notify: NotifyPolicy {
                channels: vec!["ops-alert".into()],
                on_alert: true,
                alert_depends_on: vec!["gh_temp".into()],
                ..Default::default()
            },
            controlled_actions: vec![
                ControlledAction {
                    name: "open_vent".into(),
                    destination: "device:gh-vent".into(),
                    params: serde_json::json!({ "open": true }),
                    idempotent: true,
                    depends_on: vec!["gh_temp".into()],
                },
                ControlledAction {
                    name: "start_irrigation".into(),
                    destination: "device:gh-pump".into(),
                    params: serde_json::json!({ "on": true }),
                    idempotent: true,
                    depends_on: vec!["gh_soil".into()],
                },
            ],
        })
        .expect("create b3");
    let resp_b3 = rt.invoke(&spec_b3.id).await.expect("invoke b3");
    let exp_b3 = rt.explain_last(&spec_b3.id).unwrap_or_default();
    print_exec("B3: Greenhouse Partial (fresh temp + stale soil + depends_on gate)", &resp_b3, &exp_b3);
    let exec_b3 = resp_b3.execution.as_ref().unwrap();
    check(exec_b3.status == ExecutionStatus::Partial, format!("B3 status=Partial (got {:?})", exec_b3.status));
    let irrigation_rejected = exec_b3.deliveries.iter().any(|d| {
        d.destination == "device:gh-pump" && matches!(d.state, DeliveryState::Rejected { .. })
    });
    check(irrigation_rejected, format!("B3 stale-dependent start_irrigation was REJECTED (deliveries={:?})", exec_b3.deliveries));

    // ── B4: Upstream Vision Agent (minicpm-v) -> Downstream Shift Brief (sparkx2.6) ──
    let (spec_b4_up, _) = rt
        .create(AgentSpecDraft {
            name: "B4-Up Plaza Vision Inspector".into(),
            description: None,
            user_prompt: "结合广场摄像头画面和环境传感器（PM2.5、噪音）评估广场整体状况，输出 status 和 summary。".into(),
            llm_backend_id: Some("llamacpp-minicpm-v".into()),
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract {
                requirements: vec![
                    InputRequirement {
                        key: "plaza_cam".into(),
                        source: InputSourceRef::WholeDevice { device_id: "plaza-cam-01".into() },
                        operation: None,
                        params: serde_json::json!({}),
                        result_type: InputModality::StructuredJson,
                        freshness: FreshnessPolicy::Any,
                        on_missing: MissingPolicy::ReportStale,
                        timeout_secs: 15,
                        auth_scope: None,
                    },
                    InputRequirement {
                        key: "plaza_env".into(),
                        source: InputSourceRef::WholeDevice { device_id: "plaza-env-01".into() },
                        operation: None,
                        params: serde_json::json!({}),
                        result_type: InputModality::StructuredJson,
                        freshness: FreshnessPolicy::Any,
                        on_missing: MissingPolicy::ReportStale,
                        timeout_secs: 15,
                        auth_scope: None,
                    },
                ],
            },
            tool_policy: ToolPolicy::Disabled,
            output: OutputContract {
                publish: true,
                fields: vec![
                    OutputField { name: "status".into(), depends_on: vec![], description: Some("normal or alert".into()) },
                    OutputField { name: "summary".into(), depends_on: vec![], description: Some("Chinese plaza observation".into()) },
                ],
            },
            notify: NotifyPolicy::default(),
            controlled_actions: vec![],
        })
        .expect("create b4_up");
    let resp_b4_up = rt.invoke(&spec_b4_up.id).await.expect("invoke b4_up");
    let exp_b4_up = rt.explain_last(&spec_b4_up.id).unwrap_or_default();
    print_exec("B4-Up: minicpm-v Vision + Env publisher", &resp_b4_up, &exp_b4_up);

    let _ = telemetry.flush();
    let (spec_b4_down, _) = rt
        .create(AgentSpecDraft {
            name: "B4-Down Shift Handover Brief".into(),
            description: None,
            user_prompt: "基于上游巡检 AI 发布的广场状态与摘要，生成一份简洁的中文交班简报。".into(),
            llm_backend_id: Some("llamacpp-spark".into()),
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract {
                requirements: vec![
                    InputRequirement {
                        key: "upstream_status".into(),
                        source: InputSourceRef::UpstreamAi { agent_id: spec_b4_up.id.clone(), field: "status".into() },
                        operation: None,
                        params: serde_json::json!({}),
                        result_type: InputModality::Metric,
                        freshness: FreshnessPolicy::Any,
                        on_missing: MissingPolicy::ReportStale,
                        timeout_secs: 15,
                        auth_scope: None,
                    },
                    InputRequirement {
                        key: "upstream_summary".into(),
                        source: InputSourceRef::UpstreamAi { agent_id: spec_b4_up.id.clone(), field: "summary".into() },
                        operation: None,
                        params: serde_json::json!({}),
                        result_type: InputModality::Metric,
                        freshness: FreshnessPolicy::Any,
                        on_missing: MissingPolicy::ReportStale,
                        timeout_secs: 15,
                        auth_scope: None,
                    },
                ],
            },
            tool_policy: ToolPolicy::Disabled,
            output: OutputContract {
                publish: false,
                fields: vec![
                    OutputField { name: "handover_status".into(), depends_on: vec!["upstream_status".into()], description: Some("ok or attention".into()) },
                    OutputField { name: "brief".into(), depends_on: vec!["upstream_summary".into()], description: Some("Concise Chinese shift handover brief".into()) },
                ],
            },
            notify: NotifyPolicy::default(),
            controlled_actions: vec![],
        })
        .expect("create b4_down");
    let resp_b4_down = rt.invoke(&spec_b4_down.id).await.expect("invoke b4_down");
    let exp_b4_down = rt.explain_last(&spec_b4_down.id).unwrap_or_default();
    print_exec("B4-Down: sparkx2.6 Downstream reading UpstreamAi", &resp_b4_down, &exp_b4_down);
    let exec_b4_down = resp_b4_down.execution.as_ref().unwrap();
    check(exec_b4_down.status == ExecutionStatus::Completed, format!("B4-Down UpstreamAi Completed (got {:?}, missing={:?})", exec_b4_down.status, exec_b4_down.evidence.missing_keys));
    check(exec_b4_down.evidence.missing_keys.is_empty(), format!("B4-Down read both upstream AI metrics cleanly (missing={:?})", exec_b4_down.evidence.missing_keys));

    // ── B5a: Shell allowed_commands security boundary (qwen3.5:4b) ──
    let (spec_b5a, _) = rt
        .create(AgentSpecDraft {
            name: "B5a Shell Allowed Commands Guard".into(),
            description: None,
            user_prompt: "先尝试用 shell 执行 `ls /` 查看根目录，再执行 `uname -m` 获取 CPU 架构。在输出 JSON 中如实汇报 ls_allowed（true/false）和 arch（uname -m 输出）以及 summary。".into(),
            llm_backend_id: Some("ollama-qwen35".into()),
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract::default(),
            tool_policy: ToolPolicy::Scoped {
                allowed_tools: vec!["shell".into()],
                allowed_commands: vec!["uname".into()],
            },
            output: OutputContract {
                publish: false,
                fields: vec![
                    OutputField { name: "ls_allowed".into(), depends_on: vec![], description: Some("Boolean whether ls succeeded".into()) },
                    OutputField { name: "arch".into(), depends_on: vec![], description: Some("CPU architecture from uname -m".into()) },
                    OutputField { name: "summary".into(), depends_on: vec![], description: Some("Brief Chinese summary".into()) },
                ],
            },
            notify: NotifyPolicy::default(),
            controlled_actions: vec![],
        })
        .expect("create b5a");
    let resp_b5a = rt.invoke(&spec_b5a.id).await.expect("invoke b5a");
    let exp_b5a = rt.explain_last(&spec_b5a.id).unwrap_or_default();
    print_exec("B5a: qwen3.5 allowed_commands guard (ls rejected, uname allowed)", &resp_b5a, &exp_b5a);
    let exec_b5a = resp_b5a.execution.as_ref().unwrap();
    check(exec_b5a.status == ExecutionStatus::Completed, format!("B5a Completed (got {:?})", exec_b5a.status));
    if let Some(sr) = &exec_b5a.structured_result {
        check(sr.get("ls_allowed").and_then(|v| v.as_bool()) == Some(false), format!("B5a ls_allowed=false (got {:?})", sr.get("ls_allowed")));
        check(sr.get("arch").and_then(|v| v.as_str()).is_some_and(|s| s.contains("arm64") || s.contains("x86")), format!("B5a arch populated (got {:?})", sr.get("arch")));
    } else {
        check(false, "B5a structured_result was None".into());
    }

    // ── B5b: Multi-step file_read + file_write inspection (qwen3.5:4b) ──
    let (spec_b5b, _) = rt
        .create(AgentSpecDraft {
            name: "B5b Multi-Step File Inspection".into(),
            description: None,
            user_prompt: "使用 file_read 读取 `site_log.csv`，统计其中 status 为 fail 的设备数量和设备名，再用 file_write 把简短巡检结果写入 `daily_report.md`，最后返回 JSON 字段 failed_count 和 summary。".into(),
            llm_backend_id: Some("ollama-qwen35".into()),
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract::default(),
            tool_policy: ToolPolicy::Scoped {
                allowed_tools: vec!["file_read".into(), "file_write".into()],
                allowed_commands: vec![],
            },
            output: OutputContract {
                publish: false,
                fields: vec![
                    OutputField { name: "failed_count".into(), depends_on: vec![], description: Some("Number of failed devices in site_log.csv".into()) },
                    OutputField { name: "summary".into(), depends_on: vec![], description: Some("Chinese summary naming failed devices".into()) },
                ],
            },
            notify: NotifyPolicy::default(),
            controlled_actions: vec![],
        })
        .expect("create b5b");
    let resp_b5b = rt.invoke(&spec_b5b.id).await.expect("invoke b5b");
    let exp_b5b = rt.explain_last(&spec_b5b.id).unwrap_or_default();
    print_exec("B5b: qwen3.5 multi-step file_read + file_write", &resp_b5b, &exp_b5b);
    let exec_b5b = resp_b5b.execution.as_ref().unwrap();
    check(exec_b5b.status == ExecutionStatus::Completed, format!("B5b Completed (got {:?})", exec_b5b.status));
    let report_exists = dir.path().join("daily_report.md").exists();
    check(report_exists, format!("B5b daily_report.md created on disk ({report_exists})"));
    if let Some(sr) = &exec_b5b.structured_result {
        check(sr.get("failed_count").and_then(|v| v.as_i64()) == Some(3), format!("B5b failed_count == 3 (got {:?})", sr.get("failed_count")));
    } else {
        check(false, "B5b structured_result was None".into());
    }

    // ── B6b: Interactive CallerInput ask on existing agent B1a (with prior execution journal) ──
    let resp_b6b = rt
        .invoke_with_caller(
            &spec_b1a.id,
            Some(CallerInput {
                content: Some("请直接回答：当前冷库门是开着还是关着的？温度是多少？".into()),
                data: None,
                source: Some("api".into()),
            }),
        )
        .await
        .expect("invoke b6b");
    let exp_b6b = rt.explain_last(&spec_b1a.id).unwrap_or_default();
    print_exec("B6b: CallerInput interactive ask on B1a", &resp_b6b, &exp_b6b);
    let exec_b6b = resp_b6b.execution.as_ref().unwrap();
    let concl_b6b = exec_b6b.conclusion.as_deref().unwrap_or("");
    check(concl_b6b.contains("8.4"), format!("B6b answered with real temperature 8.4 (got: {concl_b6b})"));

    // ── B7a: Free-form Chinese analysis (output.fields = []) ──
    let (spec_b7a, _) = rt
        .create(AgentSpecDraft {
            name: "B7a Free-Form Chinese Analysis".into(),
            description: None,
            user_prompt: "请基于冷库数据用自然中文写一段运维分析，不要输出 JSON，给出具体排查步骤。".into(),
            llm_backend_id: Some("ollama-lfm25".into()),
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract {
                requirements: vec![InputRequirement {
                    key: "cold_room".into(),
                    source: InputSourceRef::WholeDevice { device_id: "cold-room-hot".into() },
                    operation: None,
                    params: serde_json::json!({}),
                    result_type: InputModality::StructuredJson,
                    freshness: FreshnessPolicy::Any,
                    on_missing: MissingPolicy::ReportStale,
                    timeout_secs: 15,
                    auth_scope: None,
                }],
            },
            tool_policy: ToolPolicy::Disabled,
            output: OutputContract { publish: false, fields: vec![] },
            notify: NotifyPolicy::default(),
            controlled_actions: vec![],
        })
        .expect("create b7a");
    let resp_b7a = rt.invoke(&spec_b7a.id).await.expect("invoke b7a");
    let exp_b7a = rt.explain_last(&spec_b7a.id).unwrap_or_default();
    print_exec("B7a: lfm2.5 Free-form Chinese report (no output fields)", &resp_b7a, &exp_b7a);
    let exec_b7a = resp_b7a.execution.as_ref().unwrap();
    check(exec_b7a.status == ExecutionStatus::Completed, format!("B7a Completed (got {:?})", exec_b7a.status));
    check(exec_b7a.structured_result.is_none(), "B7a structured_result is None when output.fields empty".into());
    check(exec_b7a.conclusion.as_deref().unwrap_or("").chars().count() >= 30, "B7a produced rich Chinese prose".into());

    // ── B7b: All sources missing diagnostic check ──
    let (spec_b7b, _) = rt
        .create(AgentSpecDraft {
            name: "B7b All Sources Missing".into(),
            description: None,
            user_prompt: "分析不存在的设备数据。".into(),
            llm_backend_id: Some("llamacpp-spark".into()),
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract {
                requirements: vec![InputRequirement {
                    key: "ghost_dev".into(),
                    source: InputSourceRef::DeviceMetric { device_id: "nonexistent-999".into(), field: "temp".into() },
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
        .expect("create b7b");
    let resp_b7b = rt.invoke(&spec_b7b.id).await.expect("invoke b7b");
    let exp_b7b = rt.explain_last(&spec_b7b.id).unwrap_or_default();
    print_exec("B7b: All sources missing diagnostic", &resp_b7b, &exp_b7b);
    let exec_b7b = resp_b7b.execution.as_ref().unwrap();
    check(exec_b7b.status == ExecutionStatus::Failed, format!("B7b status=Failed (got {:?})", exec_b7b.status));
    check(exec_b7b.error.as_deref().unwrap_or("").contains("ghost_dev"), format!("B7b error names missing key ghost_dev ({:?})", exec_b7b.error));

    println!("\n================ STRESS SUMMARY ================");
    if failures.is_empty() {
        println!("ALL REAL-WORLD BUSINESS SCENARIOS PASSED!");
    } else {
        println!("FOUND {} FAILURES:", failures.len());
        for f in &failures {
            println!("  - {f}");
        }
        panic!("{} business scenario checks failed", failures.len());
    }
}
