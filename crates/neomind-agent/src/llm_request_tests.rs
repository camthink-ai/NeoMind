//! Assert on final requests, not just intermediate prompt helpers.
use super::*;
use crate::testing_helpers::mock_llm::{MockLlmRuntime, MockResponse};
use futures::StreamExt;
use neomind_core::llm::backend::{BackendCapabilities, BackendId, LlmOutput, StreamChunk};
use std::sync::Mutex;

struct CaptureRuntime {
    inner: MockLlmRuntime,
    requests: Arc<Mutex<Vec<serde_json::Value>>>,
}

impl CaptureRuntime {
    fn capture(&self, input: &LlmInput) {
        self.requests.lock().unwrap().push(serde_json::json!({
            "messages": input.messages,
            "tools_count": input.tools.as_ref().map_or(0, Vec::len),
            "thinking": input.params.thinking_enabled,
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
        }))
        .await;
    (iface, requests)
}

type ChatStream = Pin<Box<dyn Stream<Item = AgentResult<StreamEvent>> + Send>>;

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
async fn summary_is_retained_across_repeated_tool_rounds() {
    let call = MockResponse::tool_call("skill", serde_json::json!({"action":"load"}));
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
    assert_eq!(requests.len(), 5);
    assert_eq!(requests.last().unwrap()["tools_count"], 1);
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
async fn keyword_guidance_never_replaces_the_original_task() {
    let cases = [
        "查看 demo-agent 最近的对话历史",
        "查看代理温度查询的会话历史",
        "查看助手最近的温度记录",
        "查看定时任务的数据执行历史",
        "Show cron temperature query history",
        "Show message history",
        "Open the dashboard",
        "Create a new agent to read temperature",
        "查询历史",
        "What is the current configuration?",
        "Show photosensor metadata",
        "List devices",
        "读取设备历史",
        "Read temperature telemetry",
        "查询温度指标",
        "Show rule execution history",
        "Create an automation rule when the sensor temperature is high",
        "Turn off the temperature sensor",
        "关闭温度传感器",
    ];
    let (iface, requests) = harness(vec![MockResponse::text("done"); cases.len()]).await;
    for message in cases {
        drain(iface.chat_stream_with_history(message, &[]).await.unwrap()).await;
    }
    let requests = requests.lock().unwrap();
    for (request, message) in requests.iter().zip(cases) {
        assert!(
            !request["messages"][0]
                .to_string()
                .contains("## Current Task:"),
            "{message}"
        );
        assert!(
            request["messages"].to_string().contains(message),
            "original user request is preserved"
        );
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
            .contains("The current result below is fresh"));
    }
}

async fn audit_turn(
    iface: Arc<LlmInterface>,
    registry: Arc<crate::toolkit::ToolRegistry>,
    task: &str,
) -> Arc<tokio::sync::RwLock<crate::agent::types::AgentInternalState>> {
    let state = Arc::new(tokio::sync::RwLock::new(
        crate::agent::types::AgentInternalState::new("intervention-audit".into()),
    ));
    let mut stream = crate::agent::streaming::process_stream_events_with_safeguards(
        iface,
        state.clone(),
        registry,
        task,
        Default::default(),
        None,
        None,
    )
    .await
    .unwrap();
    while stream.next().await.is_some() {}
    state
}

#[tokio::test]
async fn intervention_audit_multimodal_respects_behavioral_guards_off() {
    let (iface, requests) = harness(vec![
        MockResponse::text("Let me check the image."),
        MockResponse::text("A plain scene."),
    ])
    .await;
    let state = Arc::new(tokio::sync::RwLock::new(
        crate::agent::types::AgentInternalState::new("audit-image".into()),
    ));
    let mut stream = crate::agent::streaming::process_multimodal_stream_events_with_safeguards(
        iface,
        state,
        Arc::new(crate::toolkit::ToolRegistry::new()),
        "Describe the scene in this image.",
        vec![],
        crate::agent::streaming::StreamSafeguards::default().with_behavioral_guards(false),
        None,
        None,
    )
    .await
    .unwrap();
    while stream.next().await.is_some() {}
    assert_eq!(
        requests.lock().unwrap().len(),
        1,
        "disabled behavioral guards still launched a narration nudge"
    );
}

#[tokio::test]
async fn intervention_audit_promise_directive_respects_behavioral_guards_off() {
    let call = |n| {
        MockResponse::tool_call("probe", serde_json::json!({"step":n}))
            .with_text("Let me run neomind device create now.")
    };
    let (iface, requests) = harness(vec![call(1), call(2), MockResponse::text("done")]).await;
    iface.set_tool_definitions(vec![tool("probe")]).await;
    let mut tools = crate::toolkit::ToolRegistry::new();
    tools.register(Arc::new(ProbeResult(serde_json::json!("prep result"))));
    audit_turn(iface, Arc::new(tools), "Investigate the device setup.").await;
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(
        !requests[2]["messages"]
            .to_string()
            .contains("STATED-INTENT MISMATCH"),
        "disabled behavioral guards still ordered a specific command NOW"
    );
}

#[tokio::test]
async fn intervention_audit_post_tool_round_keeps_requested_thinking() {
    let (iface, requests) = harness(vec![
        MockResponse::tool_call("probe", serde_json::json!({})),
        MockResponse::text("done"),
    ])
    .await;
    iface.set_thinking_enabled(true).await;
    iface.set_tool_definitions(vec![tool("probe")]).await;
    let mut tools = crate::toolkit::ToolRegistry::new();
    tools.register(Arc::new(ProbeResult(serde_json::json!("evidence"))));
    audit_turn(iface, Arc::new(tools), "Analyse the evidence.").await;
    let requests = requests.lock().unwrap();
    assert_eq!(requests[0]["thinking"], true);
    assert_eq!(
        requests[1]["thinking"], true,
        "a tool round silently overrode the user thinking choice"
    );
}

#[tokio::test]
async fn intervention_audit_truncated_history_does_not_disable_requested_thinking() {
    let (iface, requests) = harness(vec![MockResponse::text("done")]).await;
    iface.set_thinking_enabled(true).await;
    let history = vec![Message::assistant("historical evidence ".repeat(25_000))];
    drain(
        iface
            .chat_stream_with_history("Analyse the recent readings", &history)
            .await
            .unwrap(),
    )
    .await;
    let requests = requests.lock().unwrap();
    assert_eq!(
        requests[0]["messages"].as_array().unwrap().len(),
        2,
        "oversized history was discarded"
    );
    assert_eq!(
        requests[0]["thinking"], true,
        "a discarded history triggered the 18K thinking override"
    );
}

#[tokio::test]
async fn intervention_audit_explicit_empty_json_is_a_valid_answer() {
    let (iface, requests) = harness(vec![
        MockResponse::text("[]"),
        MockResponse::text("Replacement output"),
    ])
    .await;
    let state = audit_turn(
        iface,
        Arc::new(crate::toolkit::ToolRegistry::new()),
        "Return exactly the valid empty JSON array [] and nothing else.",
    )
    .await;
    let final_text = state
        .read()
        .await
        .memory
        .last()
        .unwrap()
        .content
        .to_string();
    assert_eq!(
        final_text,
        "[]",
        "valid requested output was replaced after an unsolicited retry; calls={}",
        requests.lock().unwrap().len()
    );
}

struct AuditProbe {
    ready: Arc<std::sync::atomic::AtomicBool>,
    calls: Arc<std::sync::atomic::AtomicUsize>,
    repair: bool,
}
#[async_trait::async_trait]
impl crate::toolkit::Tool for AuditProbe {
    fn name(&self) -> &str {
        if self.repair {
            "repair"
        } else {
            "probe"
        }
    }
    fn description(&self) -> &str {
        "test environment repair"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type":"object"})
    }
    async fn execute(
        &self,
        _: serde_json::Value,
    ) -> crate::toolkit::Result<crate::toolkit::ToolOutput> {
        use std::sync::atomic::Ordering::SeqCst;
        if self.repair {
            self.ready.store(true, SeqCst);
            Ok(crate::toolkit::ToolOutput::success("repaired"))
        } else {
            self.calls.fetch_add(1, SeqCst);
            Ok(if self.ready.load(SeqCst) {
                crate::toolkit::ToolOutput::success("READ_SUCCESS")
            } else {
                crate::toolkit::ToolOutput::error("missing resource")
            })
        }
    }
}
#[tokio::test]
async fn intervention_audit_failed_read_can_retry_after_environment_repair() {
    let read = MockResponse::tool_call("probe", serde_json::json!({}));
    let repair = MockResponse::tool_call("repair", serde_json::json!({}));
    let (iface, _) = harness(vec![
        read.clone(),
        read.clone(),
        repair,
        read,
        MockResponse::text("done"),
    ])
    .await;
    iface
        .set_tool_definitions(vec![tool("probe"), tool("repair")])
        .await;
    let ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut tools = crate::toolkit::ToolRegistry::new();
    for repair in [false, true] {
        tools.register(Arc::new(AuditProbe {
            ready: ready.clone(),
            calls: calls.clone(),
            repair,
        }));
    }
    audit_turn(
        iface,
        Arc::new(tools),
        "Read the resource after repairing it.",
    )
    .await;
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        3,
        "two failures permanently blocked the read even after repair"
    );
}

#[tokio::test]
async fn intervention_audit_latest_skill_body_reaches_model() {
    let (iface, requests) = harness(vec![MockResponse::text("done")]).await;
    iface
        .set_skill_context(
            "mutable-guide".into(),
            serde_json::json!({"id":"mutable-guide","guide":"OLD_GUIDE"}).to_string(),
        )
        .await;
    iface
        .set_skill_context(
            "mutable-guide".into(),
            serde_json::json!({"id":"mutable-guide","guide":"NEW_GUIDE"}).to_string(),
        )
        .await;
    drain(
        iface
            .chat_stream_with_history("Use the refreshed guide", &[])
            .await
            .unwrap(),
    )
    .await;
    let requests = requests.lock().unwrap();
    let system = requests[0]["messages"][0].to_string();
    assert!(
        system.contains("NEW_GUIDE") && !system.contains("OLD_GUIDE"),
        "freshly loaded guide was discarded by same-id context dedup"
    );
}
struct AuditLiveVision(Arc<std::sync::atomic::AtomicUsize>);
#[async_trait::async_trait]
impl crate::toolkit::Tool for AuditLiveVision {
    fn name(&self) -> &str {
        "vision"
    }
    fn description(&self) -> &str {
        "observe a live image URL"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type":"object"})
    }
    async fn execute(
        &self,
        _: serde_json::Value,
    ) -> crate::toolkit::Result<crate::toolkit::ToolOutput> {
        let n = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        Ok(crate::toolkit::ToolOutput::success(
            serde_json::json!({"frame":n}),
        ))
    }
}
#[tokio::test]
async fn intervention_audit_vision_reobserves_a_changed_live_source() {
    let vision = MockResponse::tool_call(
        "vision",
        serde_json::json!({"image":"https://example.test/live.jpg","prompt":"describe the current frame"}),
    );
    let (iface, _) = harness(vec![
        vision.clone(),
        MockResponse::tool_call("probe", serde_json::json!({})),
        vision,
        MockResponse::text("done"),
    ])
    .await;
    iface
        .set_tool_definitions(vec![tool("vision"), tool("probe")])
        .await;
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut tools = crate::toolkit::ToolRegistry::new();
    tools.register(Arc::new(AuditLiveVision(calls.clone())));
    tools.register(Arc::new(ProbeResult(serde_json::json!(
        "the source changed"
    ))));
    let state = Arc::new(tokio::sync::RwLock::new(
        crate::agent::types::AgentInternalState::new("audit-live-vision".into()),
    ));
    review_run_turn(iface, state, Arc::new(tools), None).await;
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "per-turn vision reuse bypassed the second live observation"
    );
}

#[tokio::test]
async fn intervention_audit_plan_only_request_does_not_receive_act_now_directive() {
    let call = |n| {
        MockResponse::tool_call("probe", serde_json::json!({"page":n}))
            .with_text("The documentation describes neomind device create as the command.")
    };
    let (iface, requests) = harness(vec![
        call(1),
        call(2),
        MockResponse::text("Explanation complete."),
    ])
    .await;
    iface.set_tool_definitions(vec![tool("probe")]).await;
    let mut tools = crate::toolkit::ToolRegistry::new();
    tools.register(Arc::new(ProbeResult(serde_json::json!(
        "read-only documentation"
    ))));
    let task = "只解释设备创建命令，不要执行任何修改。";
    assert!(crate::agent::user_requested_no_execution(task));
    let state = Arc::new(tokio::sync::RwLock::new(
        crate::agent::types::AgentInternalState::new("audit-plan-only".into()),
    ));
    let mut stream = crate::agent::streaming::process_stream_events_with_safeguards(
        iface,
        state,
        Arc::new(tools),
        task,
        Default::default(),
        None,
        None,
    )
    .await
    .unwrap();
    while stream.next().await.is_some() {}
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(
        !requests[2]["messages"]
            .to_string()
            .contains("STATED-INTENT MISMATCH"),
        "platform ordered execution even though it recognized an explicit plan-only request"
    );
}

#[tokio::test]
async fn intervention_audit_explanation_json_does_not_execute_as_a_tool_call() {
    let example = "Here is an example only:\n```json\n[{\"name\":\"shell\",\"arguments\":{\"command\":\"echo example\"}}]\n```";
    let (iface, _) = harness(vec![
        MockResponse::text(example),
        MockResponse::text("Explanation complete."),
    ])
    .await;
    iface.set_tool_definitions(vec![tool("shell")]).await;
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut tools = crate::toolkit::ToolRegistry::new();
    tools.register(Arc::new(ReviewShell {
        calls: calls.clone(),
        timed_out: false,
    }));
    let task = "Only show the JSON example. Do not execute the command.";
    assert!(crate::agent::user_requested_no_execution(task));
    let state = Arc::new(tokio::sync::RwLock::new(
        crate::agent::types::AgentInternalState::new("audit-example".into()),
    ));
    let mut stream = crate::agent::streaming::process_stream_events_with_safeguards(
        iface,
        state,
        Arc::new(tools),
        task,
        Default::default(),
        None,
        None,
    )
    .await
    .unwrap();
    while stream.next().await.is_some() {}
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "a documented code example was dispatched as a real tool call"
    );
}

#[tokio::test]
async fn recovery_followup_keeps_tools_task_and_completion_decision() {
    let (iface, requests) = harness(vec![
        MockResponse::tool_call(
            "shell",
            serde_json::json!({"command":"neomind device list"}),
        ),
        MockResponse::text("done"),
    ])
    .await;
    iface.set_tool_definitions(vec![tool("shell")]).await;
    let mut registry = crate::toolkit::ToolRegistry::new();
    registry.register(Arc::new(ReviewShell {
        calls: Default::default(),
        timed_out: false,
    }));
    let state = Arc::new(tokio::sync::RwLock::new(
        crate::agent::types::AgentInternalState::new("recovery-context".into()),
    ));
    review_run_turn(iface, state, Arc::new(registry), None).await;
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1]["tools_count"], 1);
    let followup = requests[1]["messages"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .to_string();
    for expected in [
        "perform the task",
        "Tool round 2",
        "Recent executed calls",
        "shell",
        "Discovery alone does not complete",
        "plan-only",
        "If a command failed",
    ] {
        assert!(
            followup.contains(expected),
            "missing {expected}: {followup}"
        );
    }
}

struct FailedUsageShell;
#[async_trait::async_trait]
impl crate::toolkit::tool::Tool for FailedUsageShell {
    fn name(&self) -> &str {
        "shell"
    }
    fn description(&self) -> &str {
        "report simulated CLI usage failure"
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
                "command":"neomind device update --id sensor-001 --name New",
                "exit_code":2,"timed_out":false,"stdout":"PARTIAL_USAGE_SENTINEL",
                "stderr":"Usage: neomind device update [OPTIONS] <ID>"
            }),
        ))
    }
}

#[tokio::test]
async fn recovery_cli_failure_reaches_next_model_request_with_partial_output() {
    let (iface, requests) = harness(vec![
        MockResponse::tool_call(
            "shell",
            serde_json::json!({"command":"neomind device update --id sensor-001 --name New"}),
        ),
        MockResponse::text("blocked"),
    ])
    .await;
    iface.set_tool_definitions(vec![tool("shell")]).await;
    let mut registry = crate::toolkit::ToolRegistry::new();
    registry.register(Arc::new(FailedUsageShell));
    let state = Arc::new(tokio::sync::RwLock::new(
        crate::agent::types::AgentInternalState::new("usage-failure".into()),
    ));
    review_run_turn(iface, state, Arc::new(registry), None).await;
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    let contents = requests[1]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["content"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    for expected in [
        "\"success\":false",
        "\"exit_code\":2",
        "Usage: neomind device update",
        "PARTIAL_USAGE_SENTINEL",
        "Tool round 2",
    ] {
        assert!(
            contents.contains(expected),
            "missing {expected}: {contents}"
        );
    }
}
