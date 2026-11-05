//! Assert on final requests, not just intermediate prompt helpers.
use super::*;
use crate::testing_helpers::mock_llm::{MockLlmRuntime, MockResponse};
use futures::StreamExt;
use neomind_core::llm::backend::{BackendCapabilities, BackendId, LlmOutput, StreamChunk};
use std::sync::Mutex;

struct CaptureRuntime {
    inner: MockLlmRuntime,
    requests: Arc<Mutex<Vec<serde_json::Value>>>,
    overflow_once: std::sync::atomic::AtomicBool,
}

impl CaptureRuntime {
    fn capture(&self, input: &LlmInput) {
        self.requests.lock().unwrap().push(serde_json::json!({
            "messages": input.messages,
            "tools_count": input.tools.as_ref().map_or(0, Vec::len),
            "tool_names": input.tools.as_ref().map(|tools| tools.iter().map(|t| t.name.clone()).collect::<Vec<_>>()).unwrap_or_default(),
            "thinking": input.params.thinking_enabled,
            "effort": input.params.thinking_effort,
            "temperature": input.params.temperature,
            "top_p": input.params.top_p,
            "top_k": input.params.top_k,
        }));
    }
}

#[async_trait::async_trait]
impl LlmRuntime for CaptureRuntime {
    fn backend_id(&self) -> BackendId {
        self.inner.backend_id()
    }
    fn model_name(&self) -> &str {
        "request-test"
    }
    fn capabilities(&self) -> BackendCapabilities {
        let mut caps = self.inner.capabilities();
        caps.max_context = Some(32768);
        caps.multimodal = true;
        caps.supports_images = true;
        caps
    }
    fn max_context_length(&self) -> usize {
        32768
    }
    async fn generate(&self, input: LlmInput) -> std::result::Result<LlmOutput, LlmError> {
        self.capture(&input);
        self.inner.generate(input).await
    }
    async fn generate_stream(
        &self,
        input: LlmInput,
    ) -> std::result::Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>, LlmError> {
        self.capture(&input);
        if self
            .overflow_once
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Ok(Box::pin(futures::stream::iter(vec![Err(
                LlmError::ContextOverflow {
                    prompt_tokens: 40000,
                    max_context: 32768,
                },
            )])));
        }
        self.inner.generate_stream(input).await
    }
}

type Requests = Arc<Mutex<Vec<serde_json::Value>>>;

async fn harness(script: Vec<MockResponse>) -> (Arc<LlmInterface>, Requests) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let iface = Arc::new(LlmInterface::new(ChatConfig::default()));
    iface
        .set_llm(Arc::new(CaptureRuntime {
            inner: MockLlmRuntime::new(script),
            requests: requests.clone(),
            overflow_once: std::sync::atomic::AtomicBool::new(false),
        }))
        .await;
    (iface, requests)
}

type ChatStream = Pin<Box<dyn Stream<Item = AgentResult<(String, bool)>> + Send>>;

async fn drain(mut stream: ChatStream) {
    while let Some(chunk) = stream.next().await {
        chunk.unwrap();
    }
}

fn tool(name: &str) -> neomind_core::llm::backend::ToolDefinition {
    neomind_core::llm::backend::ToolDefinition {
        name: name.into(),
        description: "test tool".into(),
        parameters: serde_json::json!({"type":"object"}),
    }
}

#[tokio::test]
async fn streaming_keeps_time_and_session_context_in_system_and_user_text_unchanged() {
    let (iface, requests) = harness(vec![MockResponse::text("done")]).await;
    iface
        .set_system_prompt_suffix(Some("SESSION_SUFFIX".into()))
        .await;
    iface
        .set_memory_context(Some("MEMORY_SNAPSHOT".into()))
        .await;
    iface
        .set_skill_context("current".into(), "CURRENT_SKILL".into())
        .await;
    let user = "Send a message to demo-agent: 'Any anomalies today?'";
    drain(iface.chat_stream_with_history(user, &[]).await.unwrap()).await;
    let requests = requests.lock().unwrap();
    let messages = requests[0]["messages"].as_array().unwrap();
    let prompt = messages[0].to_string();
    assert!(prompt.contains("- Current Time (UTC):"));
    assert!(!prompt.contains("{{CURRENT_TIME}}"));
    assert!(!prompt.contains("{{LOCAL_TIME}}"));
    assert!(!prompt.contains("{{TIMEZONE}}"));
    for marker in ["SESSION_SUFFIX", "MEMORY_SNAPSHOT", "CURRENT_SKILL"] {
        assert!(prompt.contains(marker), "lost {marker}");
    }
    let current = messages.last().unwrap();
    assert_eq!(current["role"], "user");
    assert_eq!(current["content"], user);
    assert!(!prompt.contains("provided in the [Context] footer"));
}

#[tokio::test]
async fn tool_followup_keeps_original_goal_and_does_not_reclassify_tool_context() {
    let (iface, requests) = harness(vec![MockResponse::text("done")]).await;
    let original = "Send a message to demo-agent: 'Any anomalies today?'";
    let context = "Check device status and query device history for temperature data.";
    drain(
        iface
            .chat_stream_tool_followup(original, context, &[], None)
            .await
            .unwrap(),
    )
    .await;
    let requests = requests.lock().unwrap();
    let messages = requests[0]["messages"].as_array().unwrap();
    let system = messages[0].to_string();
    assert!(!system.contains("## Current Task: Device Management"));
    assert!(!system.contains("## Current Task: Data Analysis"));
    let current = messages.last().unwrap().to_string();
    assert!(current.contains(original));
    assert!(current.contains(context));
}

#[tokio::test]
async fn summary_disables_tools_and_retains_history_summary() {
    let (iface, requests) =
        harness(vec![MockResponse::text("done"), MockResponse::text("done")]).await;
    iface.set_tool_definitions(vec![tool("shell")]).await;
    let history = vec![
        Message::system(format!(
            "{}SUMMARY_SENTINEL",
            crate::agent::types::CONVERSATION_SUMMARY_PREFIX
        )),
        Message::user("recent conversation"),
    ];
    drain(
        iface
            .chat_stream_summary("answer from history", &history)
            .await
            .unwrap(),
    )
    .await;
    drain(
        iface
            .chat_stream_with_history("continue task", &history)
            .await
            .unwrap(),
    )
    .await;
    let requests = requests.lock().unwrap();
    assert_eq!(requests[0]["tools_count"], 0);
    assert_eq!(
        requests[1]["tools_count"], 1,
        "normal tool rounds must retain their tools"
    );
    for request in requests.iter() {
        let messages = request["messages"].to_string();
        assert_eq!(messages.matches("SUMMARY_SENTINEL").count(), 1);
        assert!(messages.contains("recent conversation"));
    }
}

#[tokio::test]
async fn all_chat_paths_keep_summary_but_do_not_replay_old_platform_prompts() {
    let (iface, requests) = harness(vec![MockResponse::text("done"); 4]).await;
    iface.set_tool_definitions(vec![tool("shell")]).await;
    let history = vec![
        Message::system("OLD_PLATFORM_PROMPT"),
        Message::system(format!(
            "{}SUMMARY_SENTINEL",
            crate::agent::types::CONVERSATION_SUMMARY_PREFIX
        )),
        Message::user("recent conversation"),
    ];
    iface
        .chat_with_history("continue task", &history)
        .await
        .unwrap();
    iface
        .chat_multimodal_with_history(Message::user("continue task"), &history)
        .await
        .unwrap();
    drain(
        iface
            .chat_stream_with_history("continue task", &history)
            .await
            .unwrap(),
    )
    .await;
    drain(
        iface
            .chat_stream_multimodal_with_history(Message::user("continue task"), &history)
            .await
            .unwrap(),
    )
    .await;
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    for request in requests.iter() {
        let messages = request["messages"].to_string();
        assert_eq!(messages.matches("SUMMARY_SENTINEL").count(), 1);
        assert!(!messages.contains("OLD_PLATFORM_PROMPT"));
    }
}

#[tokio::test]
async fn summary_survives_when_oversized_history_is_truncated() {
    let (iface, requests) = harness(vec![MockResponse::text("done")]).await;
    let history = vec![
        Message::system(format!(
            "{}SUMMARY_SENTINEL",
            crate::agent::types::CONVERSATION_SUMMARY_PREFIX
        )),
        Message::assistant("old oversized history ".repeat(40_000)),
        Message::assistant("recent short answer"),
    ];
    drain(
        iface
            .chat_stream_summary("answer from history", &history)
            .await
            .unwrap(),
    )
    .await;
    assert!(iface.take_history_truncated());
    let requests = requests.lock().unwrap();
    let messages = requests[0]["messages"].to_string();
    assert!(messages.contains("SUMMARY_SENTINEL"));
    assert!(messages.contains("recent short answer"));
    assert!(!messages.contains("old oversized history"));
}

struct LoadSkill;
#[async_trait::async_trait]
impl crate::toolkit::tool::Tool for LoadSkill {
    fn name(&self) -> &str {
        "skill"
    }
    fn description(&self) -> &str {
        "load test skill"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type":"object"})
    }
    async fn execute(
        &self,
        _: serde_json::Value,
    ) -> crate::toolkit::Result<crate::toolkit::tool::ToolOutput> {
        Ok(crate::toolkit::tool::ToolOutput::success(
            serde_json::json!({
                "id": "current", "guide": "CURRENT_SKILL_SENTINEL"
            }),
        ))
    }
}

#[tokio::test]
async fn new_text_turn_clears_old_skill_but_keeps_skills_loaded_in_its_tool_rounds() {
    let (iface, requests) = harness(vec![
        MockResponse::tool_call("skill", serde_json::json!({"action":"load"})),
        MockResponse::text("done"),
        MockResponse::text("next task done"),
    ])
    .await;
    iface.set_tool_definitions(vec![tool("skill")]).await;
    iface
        .set_skill_context("old".into(), "STALE_SKILL_SENTINEL".into())
        .await;
    let mut registry = crate::toolkit::ToolRegistry::new();
    registry.register(Arc::new(LoadSkill));
    let registry = Arc::new(registry);
    let state = Arc::new(tokio::sync::RwLock::new(
        crate::agent::types::AgentInternalState::new("request-test".into()),
    ));
    for message in ["load the required skill", "new unrelated task"] {
        let mut stream = crate::agent::streaming::process_stream_events_with_safeguards(
            iface.clone(),
            state.clone(),
            registry.clone(),
            message,
            Default::default(),
            None,
            None,
        )
        .await
        .unwrap();
        while stream.next().await.is_some() {}
    }
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(!requests[0]["messages"]
        .to_string()
        .contains("STALE_SKILL_SENTINEL"));
    assert!(requests[1]["messages"][0]
        .to_string()
        .contains("CURRENT_SKILL_SENTINEL"));
    assert!(!requests[2]["messages"][0]
        .to_string()
        .contains("CURRENT_SKILL_SENTINEL"));
}

#[tokio::test]
async fn new_multimodal_turn_clears_old_skill_before_first_request() {
    let (iface, requests) = harness(vec![MockResponse::text("done")]).await;
    iface
        .set_skill_context("old".into(), "STALE_SKILL_SENTINEL".into())
        .await;
    let state = Arc::new(tokio::sync::RwLock::new(
        crate::agent::types::AgentInternalState::new("request-test".into()),
    ));
    let mut stream = crate::agent::streaming::process_multimodal_stream_events_with_safeguards(
        iface,
        state,
        Arc::new(crate::toolkit::ToolRegistry::new()),
        "describe the image",
        vec![],
        Default::default(),
        None,
        None,
    )
    .await
    .unwrap();
    while stream.next().await.is_some() {}
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(!requests[0]["messages"]
        .to_string()
        .contains("STALE_SKILL_SENTINEL"));
}

async fn review_run_turn(
    iface: Arc<LlmInterface>,
    state: Arc<tokio::sync::RwLock<crate::agent::types::AgentInternalState>>,
    registry: Arc<crate::toolkit::ToolRegistry>,
    summary: Option<String>,
) {
    let mut stream = crate::agent::streaming::process_stream_events_with_safeguards(
        iface,
        state,
        registry,
        "perform the task",
        Default::default(),
        summary,
        None,
    )
    .await
    .unwrap();
    while stream.next().await.is_some() {}
}

#[tokio::test]
async fn summary_survives_after_first_tool_round() {
    let (iface, requests) = harness(vec![
        MockResponse::tool_call("skill", serde_json::json!({"action":"load"})),
        MockResponse::text("done"),
    ])
    .await;
    iface.set_tool_definitions(vec![tool("skill")]).await;
    let mut registry = crate::toolkit::ToolRegistry::new();
    registry.register(Arc::new(LoadSkill));
    let state = Arc::new(tokio::sync::RwLock::new(
        crate::agent::types::AgentInternalState::new("review".into()),
    ));
    review_run_turn(
        iface,
        state,
        Arc::new(registry),
        Some("REVIEW_SUMMARY_SENTINEL".into()),
    )
    .await;
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[0]["messages"]
        .to_string()
        .contains("REVIEW_SUMMARY_SENTINEL"));
    assert!(requests[1]["messages"]
        .to_string()
        .contains("REVIEW_SUMMARY_SENTINEL"));
}

struct ReviewShell {
    calls: Arc<std::sync::atomic::AtomicUsize>,
    timed_out: bool,
}
#[async_trait::async_trait]
impl crate::toolkit::tool::Tool for ReviewShell {
    fn name(&self) -> &str {
        "shell"
    }
    fn description(&self) -> &str {
        "review tool"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type":"object"})
    }
    async fn execute(
        &self,
        _: serde_json::Value,
    ) -> crate::toolkit::Result<crate::toolkit::tool::ToolOutput> {
        let n = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        Ok(crate::toolkit::tool::ToolOutput::success(
            serde_json::json!({
                "timed_out":self.timed_out,"exit_code":if self.timed_out {None} else {Some(0)},
                "stdout":if self.timed_out {"PARTIAL_EFFECT_SENTINEL".to_string()} else {format!("STATE_{n}_SENTINEL")},
                "stderr":""
            }),
        ))
    }
}

#[tokio::test]
async fn partial_timeout_evidence_reaches_next_model_request() {
    let (iface, requests) = harness(vec![
        MockResponse::tool_call("shell", serde_json::json!({"command":"echo effect"})),
        MockResponse::text("done"),
    ])
    .await;
    iface.set_tool_definitions(vec![tool("shell")]).await;
    let mut registry = crate::toolkit::ToolRegistry::new();
    registry.register(Arc::new(ReviewShell {
        calls: Default::default(),
        timed_out: true,
    }));
    let state = Arc::new(tokio::sync::RwLock::new(
        crate::agent::types::AgentInternalState::new("review".into()),
    ));
    review_run_turn(iface, state, Arc::new(registry), None).await;
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    let next = requests[1]["messages"].to_string();
    assert!(next.contains("OUTCOME UNKNOWN"));
    assert!(next.contains("PARTIAL_EFFECT_SENTINEL"));
}

#[tokio::test]
async fn repeated_poll_executes_again_and_observes_new_state() {
    let call = MockResponse::tool_call(
        "shell",
        serde_json::json!({"command":"neomind device latest sensor-1"}),
    );
    let (iface, requests) = harness(vec![call.clone(), call, MockResponse::text("done")]).await;
    iface.set_tool_definitions(vec![tool("shell")]).await;
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut registry = crate::toolkit::ToolRegistry::new();
    registry.register(Arc::new(ReviewShell {
        calls: calls.clone(),
        timed_out: false,
    }));
    let state = Arc::new(tokio::sync::RwLock::new(
        crate::agent::types::AgentInternalState::new("review".into()),
    ));
    review_run_turn(iface, state, Arc::new(registry), None).await;
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests[1]["messages"]
        .to_string()
        .contains("STATE_1_SENTINEL"));
    assert!(requests[2]["messages"]
        .to_string()
        .contains("STATE_2_SENTINEL"));
}

#[tokio::test]
async fn summary_is_retained_in_forced_final_response() {
    let call = MockResponse::tool_call("skill", serde_json::json!({"action":"list"}));
    let (iface, requests) = harness(vec![
        call.clone(),
        call.clone(),
        call.clone(),
        call,
        MockResponse::text("final"),
    ])
    .await;
    iface.set_tool_definitions(vec![tool("skill")]).await;
    let mut registry = crate::toolkit::ToolRegistry::new();
    registry.register(Arc::new(LoadSkill));
    let state = Arc::new(tokio::sync::RwLock::new(
        crate::agent::types::AgentInternalState::new("summary-final".into()),
    ));
    review_run_turn(
        iface,
        state,
        Arc::new(registry),
        Some("FINAL_SUMMARY_SENTINEL".into()),
    )
    .await;
    let requests = requests.lock().unwrap();
    // A repeated index lookup exercises the general loop summary, not guide exhaustion.
    assert_eq!(requests.len(), 5);
    assert!(requests.last().unwrap()["messages"]
        .to_string()
        .contains("execution budget or loop guard ended this turn"));
    assert_eq!(requests.last().unwrap()["tools_count"], 0);
    for request in requests.iter() {
        assert!(request["messages"]
            .to_string()
            .contains("FINAL_SUMMARY_SENTINEL"));
    }
}

#[tokio::test]
async fn read_after_write_never_reuses_pre_write_state() {
    let read = MockResponse::tool_call(
        "shell",
        serde_json::json!({"command":"neomind device latest sensor-1"}),
    );
    let write = MockResponse::tool_call(
        "shell",
        serde_json::json!({"command":"neomind device control sensor-1 on"}),
    );
    let (iface, requests) =
        harness(vec![read.clone(), write, read, MockResponse::text("done")]).await;
    iface.set_tool_definitions(vec![tool("shell")]).await;
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut registry = crate::toolkit::ToolRegistry::new();
    registry.register(Arc::new(ReviewShell {
        calls: calls.clone(),
        timed_out: false,
    }));
    let state = Arc::new(tokio::sync::RwLock::new(
        crate::agent::types::AgentInternalState::new("read-write-read".into()),
    ));
    review_run_turn(iface, state, Arc::new(registry), None).await;
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 3);
    assert!(requests.lock().unwrap().last().unwrap()["messages"]
        .to_string()
        .contains("STATE_3_SENTINEL"));
}

#[tokio::test]
async fn duplicate_advisory_reaches_model_with_narrated_tool_calls() {
    let call = MockResponse::tool_call(
        "shell",
        serde_json::json!({"command":"neomind device list"}),
    )
    .with_text("Checking the device list again.");
    let (iface, requests) = harness(vec![call.clone(), call, MockResponse::text("done")]).await;
    iface.set_tool_definitions(vec![tool("shell")]).await;
    let mut registry = crate::toolkit::ToolRegistry::new();
    registry.register(Arc::new(ReviewShell {
        calls: Default::default(),
        timed_out: false,
    }));
    let state = Arc::new(tokio::sync::RwLock::new(
        crate::agent::types::AgentInternalState::new("review-advisory".into()),
    ));
    review_run_turn(iface, state, Arc::new(registry), None).await;
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(
        requests[2]["messages"].to_string().contains("_duplicate"),
        "duplicate advisory was emitted for display but never sent to the model"
    );
}

#[tokio::test]
async fn agent_conversation_does_not_get_device_history_guidance() {
    let (iface, requests) = harness(vec![MockResponse::text("done")]).await;
    drain(
        iface
            .chat_stream_with_history("Can I see demo-agent's recent conversation history?", &[])
            .await
            .unwrap(),
    )
    .await;
    let requests = requests.lock().unwrap();
    assert!(
        !requests[0]["messages"][0]
            .to_string()
            .contains("Use `neomind device history"),
        "agent conversation query received device telemetry instructions in its system prompt"
    );
}

#[tokio::test]
async fn task_guidance_requires_unambiguous_domain_evidence() {
    let cases = [
        ("查看 demo-agent 最近的对话历史", None),
        ("查看代理温度查询的会话历史", None),
        ("查看助手最近的温度记录", None),
        ("查看定时任务的数据执行历史", None),
        ("Show cron temperature query history", None),
        ("Show message history", None),
        ("Open the dashboard", None),
        ("Create a new agent to read temperature", None),
        ("查询历史", None),
        ("What is the current configuration?", None),
        ("Show photosensor metadata", None),
        ("List devices", Some("Device Management")),
        ("读取设备历史", Some("Device Management")),
        ("Read temperature telemetry", Some("Data Analysis")),
        ("查询温度指标", Some("Data Analysis")),
        ("Show rule execution history", Some("Rule Management")),
        (
            "Create an automation rule when the sensor temperature is high",
            Some("Rule Management"),
        ),
    ];
    let (iface, requests) = harness(vec![MockResponse::text("done"); cases.len()]).await;
    for (message, _) in cases {
        drain(iface.chat_stream_with_history(message, &[]).await.unwrap()).await;
    }
    let requests = requests.lock().unwrap();
    for (request, (message, expected)) in requests.iter().zip(cases) {
        let system = request["messages"][0].to_string();
        match expected {
            Some(task) => assert!(
                system.contains(&format!("## Current Task: {task}")),
                "{message}"
            ),
            None => assert!(!system.contains("## Current Task:"), "{message}"),
        }
    }
}

struct ProbeResult(serde_json::Value);

#[async_trait::async_trait]
impl crate::toolkit::tool::Tool for ProbeResult {
    fn name(&self) -> &str {
        "probe"
    }
    fn description(&self) -> &str {
        "return a probe payload"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type":"object"})
    }
    async fn execute(
        &self,
        _: serde_json::Value,
    ) -> crate::toolkit::Result<crate::toolkit::ToolOutput> {
        Ok(crate::toolkit::ToolOutput::success(self.0.clone()))
    }
}

#[tokio::test]
async fn duplicate_advisory_preserves_scalar_and_array_results() {
    for payload in [
        serde_json::json!("PAYLOAD_SENTINEL"),
        serde_json::json!(["PAYLOAD_SENTINEL"]),
    ] {
        let call =
            MockResponse::tool_call("probe", serde_json::json!({})).with_text("Checking again.");
        let (iface, requests) = harness(vec![call.clone(), call, MockResponse::text("done")]).await;
        iface.set_tool_definitions(vec![tool("probe")]).await;
        let mut tools = crate::toolkit::ToolRegistry::new();
        tools.register(Arc::new(ProbeResult(payload.clone())));
        let state = Arc::new(tokio::sync::RwLock::new(
            crate::agent::types::AgentInternalState::new("advisory-payload".into()),
        ));
        review_run_turn(iface, state, Arc::new(tools), None).await;
        let requests = requests.lock().unwrap();
        let result = requests[2]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .filter_map(|m| m["content"].as_str())
            .find_map(|s| s.strip_prefix("[Tool: probe returned]\n"))
            .unwrap();
        let result: serde_json::Value = serde_json::from_str(result).unwrap();
        assert_eq!(result["data"], payload);
        assert!(result["_duplicate"]
            .as_str()
            .unwrap()
            .contains("if you are verifying a change"));
    }
}

#[tokio::test]
async fn bound_backend_settings_reach_streaming_and_nonstreaming_requests() {
    let (iface, requests) = harness(vec![MockResponse::text("done"); 4]).await;
    let (other, other_requests) = harness(vec![MockResponse::text("other")]).await;
    let mut instance = neomind_storage::LlmBackendInstance::new(
        "selected".into(),
        "selected".into(),
        neomind_storage::LlmBackendType::LlamaCpp,
    );
    instance.temperature = 1.0;
    instance.top_p = 0.95;
    instance.top_k = 40;
    instance.thinking_enabled = true;
    iface
        .set_backend_defaults(BackendRequestDefaults::from(&instance))
        .await;
    instance.temperature = 0.2;
    instance.top_p = 0.6;
    other
        .set_backend_defaults(BackendRequestDefaults::from(&instance))
        .await;
    drain(iface.chat_stream_with_history("hello", &[]).await.unwrap()).await;
    // Non-streaming chat() has a greeting fast path that returns a canned
    // reply without touching the LLM ("hello" matches it) — use a real
    // request so the captured params can be asserted.
    iface.chat("list the devices").await.unwrap();
    other.chat("list the devices").await.unwrap();
    {
        let captured = requests.lock().unwrap();
        assert_eq!(captured.len(), 2);
        for request in captured.iter() {
            assert!((request["temperature"].as_f64().unwrap() - 1.0).abs() < 1e-6);
            assert!((request["top_p"].as_f64().unwrap() - 0.95).abs() < 1e-6);
            assert_eq!(request["top_k"], 40);
            assert_eq!(request["thinking"], true);
        }
        assert!(
            (other_requests.lock().unwrap()[0]["temperature"]
                .as_f64()
                .unwrap()
                - 0.2)
                .abs()
                < 1e-6
        );
    }
    // An explicit session override survives a backend switch; other knobs do not leak.
    iface.set_temperature_override(0.4).await;
    iface
        .set_backend_defaults(BackendRequestDefaults::from(&instance))
        .await;
    iface.chat("again").await.unwrap();
    assert_eq!(
        iface.get_effective_params().await,
        (0.4, 0.6, 40, instance.max_tokens)
    );
    // Installing a plain runtime must drop the previous backend's defaults.
    iface.set_llm(Arc::new(MockLlmRuntime::new(vec![]))).await;
    let params = iface.get_effective_params().await;
    assert_eq!(params.0, 0.4);
    assert_eq!(params.1, iface.top_p);
    assert_eq!(iface.resolve_thinking_control().await, (None, None));
}

#[tokio::test]
async fn guide_recovery_excludes_only_skill_for_one_request() {
    let (iface, requests) = harness(vec![
        MockResponse::text("recover"),
        MockResponse::text("normal"),
    ])
    .await;
    iface
        .set_tool_definitions(vec![tool("shell"), tool("skill")])
        .await;
    drain(
        iface
            .chat_stream_tool_followup_without_guides(
                "创建 webhook 推送目标",
                "Guide lookup repeated without new evidence",
                &[],
                None,
            )
            .await
            .unwrap(),
    )
    .await;
    drain(
        iface
            .chat_stream_tool_followup("创建 webhook 推送目标", "continue", &[], None)
            .await
            .unwrap(),
    )
    .await;
    let captured = requests.lock().unwrap();
    assert_eq!(captured[0]["tool_names"], serde_json::json!(["shell"]));
    assert_eq!(
        captured[1]["tool_names"],
        serde_json::json!(["shell", "skill"])
    );
    drop(captured);
    assert_eq!(
        iface.get_tool_definitions().await.len(),
        2,
        "request must not mutate the session's tools"
    );
}

fn thinking_instance(integral: bool) -> neomind_storage::LlmBackendInstance {
    let mut inst = neomind_storage::LlmBackendInstance::new(
        "policy-test".into(),
        "policy-test".into(),
        neomind_storage::LlmBackendType::LlamaCpp,
    );
    inst.thinking_enabled = true;
    inst.thinking_effort = Some(neomind_core::ThinkingEffort::from_bool(true));
    inst.is_builtin = integral;
    inst.thinking_is_integral = integral;
    inst
}

#[test]
fn large_prompt_guard_scales_with_context_window() {
    assert_eq!(long_prompt_thinking_threshold(32_768), 18_000);
    assert_eq!(long_prompt_thinking_threshold(131_072), 65_536);
    assert!(25_363 <= long_prompt_thinking_threshold(131_072));
    assert!(25_363 > long_prompt_thinking_threshold(32_768));
}

#[tokio::test]
async fn dynamic_thinking_large_prompt_updates_bool_and_effort_together() {
    let (iface, requests) = harness(vec![MockResponse::text("done")]).await;
    iface
        .set_backend_defaults(BackendRequestDefaults::from(&thinking_instance(false)))
        .await;
    drain(
        iface
            .chat_stream_with_history("measured value context ".repeat(7000), &[])
            .await
            .unwrap(),
    )
    .await;
    let r = requests.lock().unwrap();
    assert_eq!(r[0]["thinking"], false);
    assert_eq!(
        r[0]["effort"],
        serde_json::json!(neomind_core::ThinkingEffort::None)
    );
}

#[tokio::test]
async fn dynamic_thinking_bound_integral_model_resists_dynamic_disable() {
    let (iface, requests) = harness(vec![MockResponse::text("done"); 2]).await;
    iface
        .set_backend_defaults(BackendRequestDefaults::from(&thinking_instance(true)))
        .await;
    drain(
        iface
            .chat_stream_with_history("measured value context ".repeat(7000), &[])
            .await
            .unwrap(),
    )
    .await;
    drain(iface.chat_stream_summary("summarize", &[]).await.unwrap()).await;
    let r = requests.lock().unwrap();
    assert_eq!(r.len(), 2);
    for request in r.iter() {
        assert_eq!(request["thinking"], true);
        assert_eq!(
            request["effort"],
            serde_json::json!(neomind_core::ThinkingEffort::from_bool(true))
        );
    }
}

#[tokio::test]
async fn dynamic_thinking_overflow_retry_explicitly_disables_when_supported() {
    for integral in [false, true] {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let iface = LlmInterface::new(ChatConfig::default());
        iface
            .set_llm(Arc::new(CaptureRuntime {
                inner: MockLlmRuntime::new(vec![MockResponse::text("recovered")]),
                requests: requests.clone(),
                overflow_once: std::sync::atomic::AtomicBool::new(true),
            }))
            .await;
        iface
            .set_backend_defaults(BackendRequestDefaults::from(&thinking_instance(integral)))
            .await;
        drain(
            iface
                .chat_stream_with_history("check status", &[])
                .await
                .unwrap(),
        )
        .await;
        let r = requests.lock().unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[0]["thinking"], true);
        assert_eq!(r[1]["thinking"], integral);
        assert_eq!(
            r[1]["effort"],
            serde_json::json!(neomind_core::ThinkingEffort::from_bool(integral))
        );
    }
}

#[tokio::test]
async fn dynamic_thinking_effort_precedence_and_unset_defaults_are_preserved() {
    let (iface, _) = harness(vec![]).await;
    iface.restore_thinking_enabled(None).await;
    assert_eq!(iface.effective_thinking_control(None).await, (None, None));
    let mut inst = thinking_instance(false);
    inst.thinking_enabled = true;
    inst.thinking_effort = Some(neomind_core::ThinkingEffort::None);
    iface
        .set_backend_defaults(BackendRequestDefaults::from(&inst))
        .await;
    assert_eq!(
        iface.effective_thinking_control(None).await,
        (Some(false), Some(neomind_core::ThinkingEffort::None))
    );
    iface.set_thinking_enabled(true).await;
    assert_eq!(
        iface.effective_thinking_control(None).await,
        (
            Some(true),
            Some(neomind_core::ThinkingEffort::from_bool(true))
        )
    );
    assert_eq!(
        iface.effective_thinking_control(Some(false)).await,
        (Some(false), Some(neomind_core::ThinkingEffort::None))
    );
}
