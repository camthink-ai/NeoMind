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
use neomind_agent::toolkit::{ToolRegistry, WebFetchTool, WebSearchTool};
use neomind_agent::SessionManager;
use neomind_ai_agent::model::{
    AgentSpecDraft, FreshnessPolicy, InputContract, InputModality, InputRequirement,
    InputSourceRef, MissingPolicy, NotifyPolicy, OutputContract, OutputField, PacingPolicy,
    ToolPolicy, TriggerSpec,
};
use neomind_api::agent_ports::HostAgentDeps;
use neomind_api::ai_agent_runtime::HostedAiAgentRuntime;
use neomind_api::server::state::ExtensionMetricsStorage;
use neomind_core::eventbus::EventBus;
use neomind_core::extension::ExtensionRegistry;
use neomind_devices::telemetry::DataPoint;
use neomind_devices::{DeviceRegistry, DeviceService, MetricValue, TimeSeriesStorage};
use neomind_messages::MessageManager;
use neomind_storage::llm_backends::{BackendCapabilities, LlmBackendInstance, LlmBackendStore};
use neomind_storage::{AgentStore, LlmBackendType};

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
    let bus = EventBus::new();
    let dev_reg = Arc::new(DeviceRegistry::new());
    let device_service = Arc::new(DeviceService::new(dev_reg, bus));
    device_service.set_telemetry_storage(telemetry.clone()).await;
    let ext_reg = Arc::new(ExtensionRegistry::new());
    let ext_metrics = Arc::new(ExtensionMetricsStorage::with_shared_storage(telemetry.clone()));

    let session_manager = Arc::new(SessionManager::memory());
    let mut tool_reg = ToolRegistry::new();
    tool_reg.register(Arc::new(ShellTool::new(ShellConfig::default())));
    tool_reg.register(Arc::new(WebSearchTool::new()));
    tool_reg.register(Arc::new(WebFetchTool::new()));
    session_manager.set_tool_registry(Arc::new(tool_reg)).await;

    let deps = HostAgentDeps::new(mm, telemetry.clone(), device_service, ext_reg, ext_metrics)
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
