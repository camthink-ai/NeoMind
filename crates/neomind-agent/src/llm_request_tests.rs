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
    /// Override reported context window (default 32768). Tiny values force
    /// ZeroBudget history_select on the real assemble path.
    max_context: usize,
}

impl CaptureRuntime {
    fn capture(&self, input: &LlmInput) {
        self.requests.lock().unwrap().push(serde_json::json!({
            "messages": input.messages,
            "tools_count": input.tools.as_ref().map_or(0, Vec::len),
            "tool_names": input.tools.as_ref().map(|tools| tools.iter().map(|t| t.name.clone()).collect::<Vec<_>>()).unwrap_or_default(),
            "tools": input.tools,
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
        caps.max_context = Some(self.max_context);
        caps.multimodal = true;
        caps.supports_images = true;
        caps
    }
    fn max_context_length(&self) -> usize {
        self.max_context
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
    harness_with_ctx(script, 32768).await
}

async fn harness_with_ctx(
    script: Vec<MockResponse>,
    max_context: usize,
) -> (Arc<LlmInterface>, Requests) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let iface = Arc::new(LlmInterface::new(ChatConfig::default()));
    iface
        .set_llm(Arc::new(CaptureRuntime {
            inner: MockLlmRuntime::new(script),
            requests: requests.clone(),
            overflow_once: std::sync::atomic::AtomicBool::new(false),
            max_context,
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
    for marker in ["SESSION_SUFFIX", "CURRENT_SKILL"] {
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
            0,
        )
        .await
        .unwrap();
        while stream.next().await.is_some() {}
    }
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    // Old turn's stale transient skill must be cleared before turn 1.
    assert!(!requests[0]["messages"]
        .to_string()
        .contains("STALE_SKILL_SENTINEL"));
    // P1-3 single-copy invariant: mid-turn `skill(load)` must NOT rewrite the
    // system prompt (keeps llama.cpp KV-prefix cache byte-identical across
    // rounds within a turn). The guide lives once in conversation `tool` history.
    assert_eq!(
        requests[0]["messages"][0]["content"], requests[1]["messages"][0]["content"],
        "system prompt content must remain byte-identical after mid-turn skill(load)"
    );
    assert!(
        !requests[1]["messages"][0]
            .to_string()
            .contains("CURRENT_SKILL_SENTINEL"),
        "skill body must not be duplicated into the system prompt"
    );
    assert!(
        requests[1]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .skip(1)
            .any(|m| m.to_string().contains("CURRENT_SKILL_SENTINEL")),
        "skill body must be retained in conversation tool history"
    );
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
        0,
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
    // Use a valid read-only skill action. Unknown actions (e.g. legacy "list")
    // fail closed into the write gate and no longer trip the identical-round
    // progress halt that this test relies on.
    let call = MockResponse::tool_call(
        "skill",
        serde_json::json!({"action":"search","query":"index"}),
    );
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
    {
        let captured = requests.lock().unwrap();
        assert_eq!(captured[0]["tool_names"], serde_json::json!(["shell"]));
        assert_eq!(
            captured[1]["tool_names"],
            serde_json::json!(["shell", "skill"])
        );
    }
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
                max_context: 32768,
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

/// Isolate the process-global data-directory resolver from parallel tests and
/// from the operator's real memory files. This exercises the real session path.
#[test]
fn memory_on_demand_session_requests() {
    let dir = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "llm::request_tests::memory_on_demand_isolated_child",
            "--ignored",
            "--nocapture",
        ])
        .env("NEOMIND_DATA_DIR", dir.path())
        .env("NEOMIND_MEMORY_TEST_CHILD", "1")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}

#[tokio::test]
#[ignore = "run by memory_on_demand_session_requests with isolated data directory"]
async fn memory_on_demand_isolated_child() {
    assert_eq!(std::env::var("NEOMIND_MEMORY_TEST_CHILD").unwrap(), "1");
    use crate::session::SessionManager;
    use crate::toolkit::{MemoryTool, ToolRegistry};
    let cfg = neomind_storage::MemoryConfig::load();
    let store = neomind_storage::MarkdownMemoryStore::with_config(&cfg.storage_path, cfg.clone());
    store.init().unwrap();
    const OLD: &str = "OLD_WIDGET_TASK_7F92: meteor widget component unavailable";
    store.write_file("user", OLD).await.unwrap();
    store
        .write_file("knowledge", "FACT_91BD: lab sensor uses MQTT")
        .await
        .unwrap();
    store
        .write_file("procedures", "SOP_21AE: inspect sensor before reset")
        .await
        .unwrap();
    let store = Arc::new(tokio::sync::RwLock::new(store));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(MemoryTool::new(store.clone())));
    let manager = SessionManager::memory();
    manager.set_tool_registry(Arc::new(registry)).await;
    let sid = manager.create_session().await.unwrap();
    let agent = manager.get_session(&sid).await.unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let rt = |script| {
        Arc::new(CaptureRuntime {
            inner: MockLlmRuntime::new(script),
            requests: requests.clone(),
            overflow_once: std::sync::atomic::AtomicBool::new(false),
            max_context: 32768,
        })
    };
    agent
        .llm_interface()
        .set_llm(rt(vec![
            MockResponse::text(
                "AI chips perform parallel computation."
            );
            8
        ]))
        .await;
    manager
        .process_message(&sid, "Explain AI hardware trends.")
        .await
        .unwrap();
    let mut stream = manager.process_message_events(&sid, "继续").await.unwrap();
    while stream.next().await.is_some() {}
    {
        let captured = requests.lock().unwrap();
        assert!(
            !captured.is_empty(),
            "must reach the real LLM request boundary"
        );
        for request in captured.iter() {
            let text = request["messages"].to_string();
            for marker in [
                "OLD_WIDGET_TASK_7F92",
                "FACT_91BD",
                "SOP_21AE",
                "<memory-context>",
            ] {
                assert!(
                    !text.contains(marker),
                    "unsolicited memory entered request: {marker}"
                );
            }
        }
        assert!(captured.last().unwrap()["messages"]
            .to_string()
            .contains("Explain AI hardware trends."));
    }
    requests.lock().unwrap().clear();
    agent
        .llm_interface()
        .set_llm(rt(vec![
            MockResponse::tool_call(
                "memory",
                serde_json::json!({"action":"read", "target":"knowledge"}),
            ),
            MockResponse::text("The saved lab sensor uses MQTT."),
        ]))
        .await;
    let mut stream = manager
        .process_message_events(&sid, "Recall the saved lab sensor protocol.")
        .await
        .unwrap();
    while stream.next().await.is_some() {}
    {
        let captured = requests.lock().unwrap();
        assert!(
            captured.len() >= 2,
            "explicit read must execute and reach the follow-up request"
        );
        assert!(!captured[0]["messages"].to_string().contains("FACT_91BD"));
        assert!(captured[1..]
            .iter()
            .any(|r| r["messages"].to_string().contains("FACT_91BD")));
        assert!(captured
            .iter()
            .all(|r| !r["messages"].to_string().contains("OLD_WIDGET_TASK_7F92")));
    }
    // A hallucinated tool call must still be rejected when the schema hid it.
    let disabled_sid = manager.create_session().await.unwrap();
    let disabled_agent = manager.get_session(&disabled_sid).await.unwrap();
    manager.toggle_memory(&disabled_sid, false).await.unwrap();
    requests.lock().unwrap().clear();
    disabled_agent
        .llm_interface()
        .set_llm(rt(vec![
            MockResponse::tool_call(
                "memory",
                serde_json::json!({"action":"read", "target":"user"}),
            ),
            MockResponse::text("Memory is disabled for this session."),
        ]))
        .await;
    let mut stream = manager
        .process_message_events(&disabled_sid, "Recall my earlier preference.")
        .await
        .unwrap();
    while stream.next().await.is_some() {}
    {
        let captured = requests.lock().unwrap();
        assert!(captured.len() >= 2);
        assert!(captured.iter().all(|r| !r["tool_names"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n == "memory")));
        assert!(captured
            .iter()
            .all(|r| !r["messages"].to_string().contains("OLD_WIDGET_TASK_7F92")));
        assert!(captured[1..].iter().any(|r| r["messages"]
            .to_string()
            .to_lowercase()
            .contains("disabled")));
    }
    assert_eq!(store.read().await.read_file("user").await.unwrap(), OLD);
}

#[tokio::test]
async fn zero_budget_history_strips_tools_on_real_stream_request() {
    // Tiny window + fat system/tools + non-empty history → ZeroBudget select.
    // Production assemble must degrade: tools_count == 0 on the captured request.
    let (iface, requests) = harness_with_ctx(vec![MockResponse::text("ok")], 1024).await;
    // Oversized tool schema so reserved tokens eat the prompt budget.
    let fat_desc = "TOOLDESC ".repeat(400);
    iface
        .set_tool_definitions(vec![neomind_core::llm::backend::ToolDefinition {
            name: "shell".into(),
            description: fat_desc,
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "command": { "type": "string" } },
            }),
        }])
        .await;
    let history = vec![
        Message::user("把天气写到 EK410"),
        Message::assistant("calling shell"),
        Message::user(format!("[Tool: shell returned]\n{}", "RESULT ".repeat(200))),
    ];
    drain(
        iface
            .chat_stream_with_history("继续查询设备列表", &history)
            .await
            .unwrap(),
    )
    .await;
    let captured = requests.lock().unwrap();
    assert_eq!(captured.len(), 1);
    assert_eq!(
        captured[0]["tools_count"], 0,
        "ZeroBudget history_select must not assemble a tool-enabled request: {:?}",
        captured[0]["tool_names"]
    );
}

#[tokio::test]
async fn outer_window_feedback_drop_strips_tools_on_real_request() {
    // Streaming-layer outer window evicted the newest complete tool group
    // (mark_history_window_dropped_latest_feedback). The inner selector only
    // sees the already-trimmed window and would vacuously report "feedback
    // survives" — the assemble gate must still refuse tools on the wire.
    let (iface, requests) = harness(vec![MockResponse::text("ok"), MockResponse::text("ok")]).await;
    iface.set_tool_definitions(vec![tool("shell")]).await;
    let history = vec![
        Message::user("把天气写到 EK410"),
        Message::assistant("calling shell"),
        Message::user("[Tool: shell returned]\nLATEST_RESULT_MARK_7C31"),
    ];

    // Flagged round: history fully fits the inner selection, only the outer
    // flag says the latest tool feedback is gone → no tools on the request.
    iface.mark_history_window_dropped_latest_feedback();
    drain(
        iface
            .chat_stream_with_history("继续查询设备列表", &history)
            .await
            .unwrap(),
    )
    .await;
    {
        let captured = requests.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(
            captured[0]["tools_count"], 0,
            "outer-window feedback drop must not assemble a tool-enabled request: {:?}",
            captured[0]["tool_names"]
        );
    }

    // Control: identical request without the flag keeps tools (the flag was
    // consumed by the previous request — take semantics).
    requests.lock().unwrap().clear();
    drain(
        iface
            .chat_stream_with_history("继续查询设备列表", &history)
            .await
            .unwrap(),
    )
    .await;
    {
        let captured = requests.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(
            captured[0]["tools_count"], 1,
            "without the outer drop flag a fitting history must keep tools: {:?}",
            captured[0]["tool_names"]
        );
        assert!(captured[0]["tool_names"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n == "shell"));
    }
}

#[tokio::test]
async fn task_state_prompt_injected_into_system_on_stream_request() {
    let (iface, requests) = harness(vec![MockResponse::text("ok")]).await;
    iface
        .set_task_state_prompt(Some(
            "## Active Task State\n- Goal: 写天气到 EK410\n- Next: render (source: planner)\n"
                .into(),
        ))
        .await;
    drain(iface.chat_stream_with_history("继续", &[]).await.unwrap()).await;
    let captured = requests.lock().unwrap();
    let system = captured[0]["messages"].as_array().unwrap()[0].to_string();
    assert!(
        system.contains("Active Task State") && system.contains("写天气到 EK410"),
        "TaskState must reach the model context: {system}"
    );
}

// =========================================================================
// Phase 5 (P1-2, P1-3, P1-4) end-to-end regressions
// =========================================================================

fn tool_has_new_operation_prop(req: &serde_json::Value, tool_name: &str) -> bool {
    req["tools"]
        .as_array()
        .and_then(|arr| arr.iter().find(|t| t["name"] == tool_name))
        .and_then(|t| t["parameters"]["properties"].get("_new_operation"))
        .is_some()
}

/// P1-2:
/// - Clean turn schemas do NOT expose `_new_operation` noise;
/// - Pre-submission `Rejected` (exit code 2) stays exempt from `PreferKnown`
///   and does NOT expose `_new_operation`;
/// - After a terminal `Succeeded` write on `shell`, ONLY `shell` exposes
///   `_new_operation` while unrelated mutating tools (`run_now`) stay clean;
/// - Repeating the same succeeded write returns `PreferKnown` (never preempted
///   by `failed_tool_signatures`);
/// - Reordered JSON keys on read-only failing calls still trip the canonical
///   `failed_tool_signatures` loop break on the 3rd attempt.
#[tokio::test]
async fn p1_2_two_layer_gate_canonical_fingerprints_and_dynamic_new_operation_exposure() {
    struct ScriptedShell {
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }
    #[async_trait::async_trait]
    impl crate::toolkit::tool::Tool for ScriptedShell {
        fn name(&self) -> &str {
            "shell"
        }
        fn description(&self) -> &str {
            "scripted shell for P1-2"
        }
        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string"},
                    "params": {}
                }
            })
        }
        async fn execute(
            &self,
            args: serde_json::Value,
        ) -> crate::toolkit::Result<crate::toolkit::tool::ToolOutput> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let cmd = args["command"].as_str().unwrap_or("");
            if cmd.contains("--bad-flag") {
                return Ok(crate::toolkit::tool::ToolOutput::success(
                    serde_json::json!({
                        "exit_code": 2,
                        "stdout": "",
                        "stderr": "error: unexpected argument '--bad-flag'",
                        "command": cmd,
                        "timed_out": false
                    }),
                ));
            }
            Ok(crate::toolkit::tool::ToolOutput::success(
                serde_json::json!({
                    "exit_code": 0,
                    "stdout": r#"{"ok":true,"id":"rule-1"}"#,
                    "stderr": "",
                    "command": cmd,
                    "timed_out": false
                }),
            ))
        }
    }

    struct FailingReadTool {
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }
    #[async_trait::async_trait]
    impl crate::toolkit::tool::Tool for FailingReadTool {
        fn name(&self) -> &str {
            "web_fetch"
        }
        fn description(&self) -> &str {
            "failing read tool"
        }
        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({"type":"object","properties":{"url":{"type":"string"},"mode":{"type":"string"}}})
        }
        async fn execute(
            &self,
            _: serde_json::Value,
        ) -> crate::toolkit::Result<crate::toolkit::tool::ToolOutput> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(crate::toolkit::ToolError::Execution("404 not found".into()))
        }
    }

    let (iface, requests) = harness(vec![
        // Round 1: syntax error (exit 2 → OperationStatus::Rejected)
        MockResponse::tool_call(
            "shell",
            serde_json::json!({"command": "neomind rule create --bad-flag"}),
        ),
        // Round 2: fixed syntax → Succeeded write on shell
        MockResponse::tool_call(
            "shell",
            serde_json::json!({"command": "neomind rule create --name ok-rule"}),
        ),
        // Round 3: repeat same write without `_new_operation` → PreferKnown resume (not executed again)
        MockResponse::tool_call(
            "shell",
            serde_json::json!({"command": "neomind rule create --name ok-rule"}),
        ),
        // Rounds 4-6: failing read tool with reordered JSON keys → 3rd call must LOOP BREAK
        MockResponse::tool_call(
            "web_fetch",
            serde_json::json!({"url": "https://example.invalid/a", "mode": "text"}),
        ),
        MockResponse::tool_call(
            "web_fetch",
            serde_json::json!({"mode": "text", "url": "https://example.invalid/a"}),
        ),
        MockResponse::tool_call(
            "web_fetch",
            serde_json::json!({"url": "https://example.invalid/a", "mode": "text"}),
        ),
        MockResponse::text("all checked"),
    ])
    .await;

    iface
        .set_tool_definitions(vec![tool("shell"), tool("run_now"), tool("web_fetch")])
        .await;

    let shell_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let read_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut registry = crate::toolkit::ToolRegistry::new();
    registry.register(Arc::new(ScriptedShell {
        calls: shell_calls.clone(),
    }));
    registry.register(Arc::new(FailingReadTool {
        calls: read_calls.clone(),
    }));
    let state = Arc::new(tokio::sync::RwLock::new(
        crate::agent::types::AgentInternalState::new("p1-2-test".into()),
    ));

    let mut stream = crate::agent::streaming::process_stream_events_with_safeguards(
        iface,
        state.clone(),
        Arc::new(registry),
        "create rule ok-rule",
        Default::default(),
        None,
        None,
        0,
    )
    .await
    .unwrap();
    let mut events = Vec::new();
    while let Some(ev) = stream.next().await {
        events.push(ev);
    }

    // Shell executed twice (Round 1 Rejected + Round 2 Succeeded); Round 3 was served by PreferKnown.
    assert_eq!(shell_calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    // Failing read tool executed twice; 3rd call with reordered keys tripped canonical LOOP BREAK.
    assert_eq!(read_calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert!(
        events.iter().any(|e| matches!(
            e,
            crate::agent::types::AgentEvent::ToolCallEnd { tool, result, success: false, .. }
            if tool == "web_fetch" && result.contains("LOOP BREAK")
        )),
        "canonical failure circuit breaker must refuse the 3rd reordered-key failure"
    );

    let captured = requests.lock().unwrap();
    // Round 1 (clean session): neither shell nor run_now exposes `_new_operation`.
    assert!(!tool_has_new_operation_prop(&captured[0], "shell"));
    assert!(!tool_has_new_operation_prop(&captured[0], "run_now"));
    // Round 2 (after exit-2 Rejected only): STILL no `_new_operation` noise!
    assert!(
        !tool_has_new_operation_prop(&captured[1], "shell"),
        "Rejected pre-submission error must not expose _new_operation"
    );
    assert!(!tool_has_new_operation_prop(&captured[1], "run_now"));
    // Round 3 (after Succeeded write on shell): ONLY `shell` exposes `_new_operation`; `run_now` stays clean.
    assert!(
        tool_has_new_operation_prop(&captured[2], "shell"),
        "terminal write on shell must expose _new_operation on shell schema"
    );
    assert!(
        !tool_has_new_operation_prop(&captured[2], "run_now"),
        "unrelated mutating tool must not expose _new_operation"
    );
}

/// P1-3: Single-copy skill loading across a multi-round ReAct turn using the
/// real `SkillTool` and builtin registry:
/// 1) System prompt stays 100% byte-identical across every round in the turn
///    (no mid-turn system prompt mutation breaking KV-prefix cache);
/// 2) Repeating the same `(skill_id, None)` (even via alias vs canonical id)
///    returns lightweight `{"already_present": true, ...}`;
/// 3) Loading a distinct `section` of the same skill still returns full chapter text.
#[tokio::test]
async fn p1_3_single_copy_skill_keeps_system_prompt_byte_identical_across_rounds() {
    let builtin_registry = Arc::new(tokio::sync::RwLock::new(
        crate::skills::SkillRegistry::load_all(None),
    ));
    let sample_section = {
        let reg = builtin_registry.read().await;
        let skill = reg
            .get("dashboard-management")
            .expect("builtin dashboard-management");
        let overview = skill.load_guide(None).unwrap();
        overview["sections"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .expect("dashboard-management section")
            .to_string()
    };

    let (iface, requests) = harness(vec![
        // Round 1: load overview via alias
        MockResponse::tool_call(
            "skill",
            serde_json::json!({"action": "load", "id": "dashboard"}),
        ),
        // Round 2: repeat same overview via canonical id → lightweight already_present
        MockResponse::tool_call(
            "skill",
            serde_json::json!({"action": "load", "id": "dashboard-management"}),
        ),
        // Round 3: load distinct section of same skill → full chapter
        MockResponse::tool_call(
            "skill",
            serde_json::json!({
                "action": "load",
                "id": "dashboard-management",
                "section": sample_section,
            }),
        ),
        MockResponse::text("ready to build dashboard"),
    ])
    .await;
    iface.set_tool_definitions(vec![tool("skill")]).await;

    let mut registry = crate::toolkit::ToolRegistry::new();
    registry.register(Arc::new(crate::toolkit::skill_tool::SkillTool::new(
        builtin_registry,
    )));
    let state = Arc::new(tokio::sync::RwLock::new(
        crate::agent::types::AgentInternalState::new("p1-3-single-copy".into()),
    ));

    let mut stream = crate::agent::streaming::process_stream_events_with_safeguards(
        iface,
        state.clone(),
        Arc::new(registry),
        "explain how to add dashboard components",
        Default::default(),
        None,
        None,
        0,
    )
    .await
    .unwrap();
    let mut tool_ends: Vec<String> = Vec::new();
    while let Some(ev) = stream.next().await {
        if let crate::agent::types::AgentEvent::ToolCallEnd { result, .. } = ev {
            tool_ends.push(result);
        }
    }

    assert_eq!(tool_ends.len(), 3, "3 skill calls must complete");
    let r1: serde_json::Value = serde_json::from_str(&tool_ends[0]).unwrap();
    let r2: serde_json::Value = serde_json::from_str(&tool_ends[1]).unwrap();
    let r3: serde_json::Value = serde_json::from_str(&tool_ends[2]).unwrap();
    assert!(r1["guide"].as_str().is_some_and(|g| g.len() > 50));
    assert_eq!(r2["already_present"], true);
    assert_eq!(
        r2["hint"],
        "Guide already in context — run the next CLI command now."
    );
    assert!(r2.get("guide").is_none());
    assert_eq!(r3["section"], sample_section);
    assert!(r3["guide"].as_str().is_some_and(|g| g.starts_with("## ")));

    let captured = requests.lock().unwrap();
    assert_eq!(captured.len(), 4);
    let base_system = &captured[0]["messages"][0]["content"];
    for (idx, req) in captured.iter().enumerate().skip(1) {
        assert_eq!(
            &req["messages"][0]["content"],
            base_system,
            "system prompt content changed on round {} after skill(load) — KV cache would break",
            idx + 1
        );
    }
}

/// P1-4 (empty reply recovery): when a turn already has tool results and the
/// follow-up produces an empty/degenerate response (`[]`), the recovery path
/// MUST call the tool-free summary route (`tools_count == 0`, `thinking == false`)
/// and return a usable text answer.
#[tokio::test]
async fn p1_4_empty_reply_recovery_uses_tool_free_summary_path() {
    struct StaticShell;
    #[async_trait::async_trait]
    impl crate::toolkit::tool::Tool for StaticShell {
        fn name(&self) -> &str {
            "shell"
        }
        fn description(&self) -> &str {
            "static shell"
        }
        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}})
        }
        async fn execute(
            &self,
            _: serde_json::Value,
        ) -> crate::toolkit::Result<crate::toolkit::tool::ToolOutput> {
            Ok(crate::toolkit::tool::ToolOutput::success(
                serde_json::json!({
                    "exit_code": 0,
                    "stdout": r#"{"devices":[{"id":"sensor-1","temp":23.5}]}"#,
                    "stderr": "",
                    "timed_out": false
                }),
            ))
        }
    }

    let (iface, requests) = harness(vec![
        MockResponse::tool_call(
            "shell",
            serde_json::json!({"command": "neomind device list"}),
        ),
        // Round 2 degenerates to bare `[]` (thinking budget exhausted)
        MockResponse::text("[]").with_thinking("burned all tokens thinking"),
        // Recovery summary produces clean final text
        MockResponse::text("sensor-1 当前温度 23.5°C。"),
    ])
    .await;
    iface.set_tool_definitions(vec![tool("shell")]).await;

    let mut registry = crate::toolkit::ToolRegistry::new();
    registry.register(Arc::new(StaticShell));
    let state = Arc::new(tokio::sync::RwLock::new(
        crate::agent::types::AgentInternalState::new("p1-4-empty-recovery".into()),
    ));

    let mut stream = crate::agent::streaming::process_stream_events_with_safeguards(
        iface,
        state,
        Arc::new(registry),
        "查看设备温度",
        Default::default(),
        None,
        None,
        0,
    )
    .await
    .unwrap();
    let mut visible = String::new();
    while let Some(ev) = stream.next().await {
        if let crate::agent::types::AgentEvent::Content { content } = ev {
            visible.push_str(&content);
        }
    }

    assert!(
        visible.contains("sensor-1 当前温度 23.5°C"),
        "recovery answer must reach user, got: {visible}"
    );
    let captured = requests.lock().unwrap();
    assert_eq!(captured.len(), 3);
    // Recovery request (index 2) MUST be tool-free and have thinking disabled.
    assert_eq!(
        captured[2]["tools_count"], 0,
        "empty-reply recovery must omit tool schema so model cannot loop on tool JSON"
    );
    assert_eq!(
        captured[2]["thinking"],
        serde_json::json!(false),
        "empty-reply recovery must force thinking_enabled=false"
    );
}

/// P1-4 (unified multimodal state machine):
/// - Supports >2 tool rounds (3 distinct rounds + final answer);
/// - Propagates `PreparedInvoke` host context (`_host_prepared_operation_id`
///   restored across canonicalize) and structured `params` to `shell`;
/// - Emits `ToolCallEnd.success = effective_success` (false for non-zero exit);
/// - Fires exactly 1 bounded NM-250 `foreshadow_continue` under default
///   `behavioral_guards = false` and stops honestly with `本次未取得结果。` if
///   the model foreshadows again.
#[tokio::test]
async fn p1_4_multimodal_unified_engine_multi_round_prepared_invoke_and_foreshadow() {
    struct InspectingShell {
        seen_prepared_ids: Arc<Mutex<Vec<Option<String>>>>,
        seen_params: Arc<Mutex<Vec<Option<serde_json::Value>>>>,
    }
    #[async_trait::async_trait]
    impl crate::toolkit::tool::Tool for InspectingShell {
        fn name(&self) -> &str {
            "shell"
        }
        fn description(&self) -> &str {
            "inspecting shell"
        }
        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string"},
                    "params": {}
                }
            })
        }
        async fn execute(
            &self,
            args: serde_json::Value,
        ) -> crate::toolkit::Result<crate::toolkit::tool::ToolOutput> {
            let cmd = args["command"].as_str().unwrap_or("");
            let host_op = args
                .get(crate::agent::task_continuity::HOST_PREPARED_OPERATION_ARG)
                .and_then(|v| v.as_str())
                .map(str::to_string);
            self.seen_prepared_ids.lock().unwrap().push(host_op);
            self.seen_params
                .lock()
                .unwrap()
                .push(args.get("params").cloned());
            if cmd.contains("missing-device") {
                return Ok(crate::toolkit::tool::ToolOutput::success(
                    serde_json::json!({
                        "exit_code": 1,
                        "stdout": r#"{"success":false,"error":"not found"}"#,
                        "stderr": "",
                        "command": cmd,
                        "timed_out": false
                    }),
                ));
            }
            Ok(crate::toolkit::tool::ToolOutput::success(
                serde_json::json!({
                    "exit_code": 0,
                    "stdout": format!(r#"{{"ok":true,"cmd":"{cmd}"}}"#),
                    "stderr": "",
                    "command": cmd,
                    "timed_out": false
                }),
            ))
        }
    }

    // Publish a real ReadOnly extension command snapshot so
    // `attach_chat_prepared_invoke` mints a live PreparedInvoke ticket on the
    // multimodal path (verifying the full PreparedInvoke + structured params
    // carriage across canonicalize → execute).
    let ext_id = "p1-4-mm-weather-test";
    {
        use neomind_core::extension::{
            begin_instance_generation, invalidate_instance_capability, CommandEffect,
            ExtensionCommand, ExtensionCommandSnapshot, MetricDataType, ParameterDefinition,
        };
        invalidate_instance_capability(ext_id);
        let gen = begin_instance_generation(ext_id);
        let cmd = ExtensionCommand {
            name: "get_weather".into(),
            display_name: "Get Weather".into(),
            description: "Query weather".into(),
            parameters: vec![ParameterDefinition {
                name: "city".into(),
                display_name: "City".into(),
                description: "City name".into(),
                param_type: MetricDataType::String,
                required: true,
                default_value: None,
                min: None,
                max: None,
                options: vec![],
            }],
            effect: Some(CommandEffect::ReadOnly),
            replay_safety: None,
            execution: None,
            ..Default::default()
        };
        ExtensionCommandSnapshot::build(ext_id, "1.0.0", None, vec![cmd], gen)
            .publish_capabilities()
            .unwrap();
    }

    // Part A: 3 tool rounds (>2!) including a failed shell, an extension invoke
    // carrying PreparedInvoke + structured params, and a final text reply.
    let (iface, _requests) = harness(vec![
        MockResponse::tool_call(
            "shell",
            serde_json::json!({"command": "neomind device get missing-device"}),
        ),
        MockResponse::tool_call(
            "shell",
            serde_json::json!({"command": "neomind device list"}),
        ),
        MockResponse::tool_call(
            "shell",
            serde_json::json!({
                "command": format!("neomind extension invoke {ext_id} get_weather"),
                "params": {"city": "Xiamen"}
            }),
        ),
        MockResponse::text("三轮排查与天气查询全部完成。"),
    ])
    .await;
    iface.set_tool_definitions(vec![tool("shell")]).await;

    let seen_prepared_ids = Arc::new(Mutex::new(Vec::new()));
    let seen_params = Arc::new(Mutex::new(Vec::new()));
    let mut registry = crate::toolkit::ToolRegistry::new();
    registry.register(Arc::new(InspectingShell {
        seen_prepared_ids: seen_prepared_ids.clone(),
        seen_params: seen_params.clone(),
    }));
    let state = Arc::new(tokio::sync::RwLock::new(
        crate::agent::types::AgentInternalState::new("p1-4-mm-multi".into()),
    ));

    let mut stream = crate::agent::streaming::process_multimodal_stream_events_with_safeguards(
        iface,
        state,
        Arc::new(registry),
        "看图排查设备并查询厦门天气",
        vec!["data:image/png;base64,iVBORw0KGgo=".to_string()],
        Default::default(),
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let mut tool_end_statuses: Vec<(usize, bool)> = Vec::new();
    let mut visible = String::new();
    while let Some(ev) = stream.next().await {
        match ev {
            crate::agent::types::AgentEvent::ToolCallEnd { success, round, .. } => {
                tool_end_statuses.push((round.unwrap_or(0), success));
            }
            crate::agent::types::AgentEvent::Content { content } => {
                visible.push_str(&content);
            }
            _ => {}
        }
    }

    assert_eq!(
        tool_end_statuses,
        vec![(1, false), (2, true), (3, true)],
        "multimodal unified engine must run >2 rounds and emit ToolCallEnd.success = effective_success"
    );
    assert!(visible.contains("三轮排查与天气查询全部完成"));
    // Round 3 extension invoke must have carried a host PreparedInvoke id + structured params.
    {
        let prepared = seen_prepared_ids.lock().unwrap();
        assert_eq!(prepared.len(), 3);
        assert!(
            prepared[2].as_deref().is_some_and(|id| !id.is_empty()),
            "multimodal extension invoke must preserve host PreparedInvoke ticket: {prepared:?}"
        );
        let params = seen_params.lock().unwrap();
        assert_eq!(params[2], Some(serde_json::json!({"city": "Xiamen"})));
    }

    // Part B: NM-250 bounded 1x foreshadow_continue under default behavioral_guards=false.
    let (iface_fs, requests_fs) = harness(vec![
        MockResponse::text("好的，我马上帮您查询设备状态："),
        MockResponse::text("我现在来查看一下设备情况："),
    ])
    .await;
    let state_fs = Arc::new(tokio::sync::RwLock::new(
        crate::agent::types::AgentInternalState::new("p1-4-mm-foreshadow".into()),
    ));
    let mut stream_fs = crate::agent::streaming::process_multimodal_stream_events_with_safeguards(
        iface_fs,
        state_fs,
        Arc::new(crate::toolkit::ToolRegistry::new()),
        "帮我查一下设备状态",
        vec!["data:image/png;base64,iVBORw0KGgo=".to_string()],
        Default::default(),
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let mut notes = Vec::new();
    while let Some(ev) = stream_fs.next().await {
        if let crate::agent::types::AgentEvent::SystemNote { note } = ev {
            notes.push(note);
        }
    }
    assert_eq!(
        requests_fs.lock().unwrap().len(),
        2,
        "multimodal stream must trigger exactly 1 bounded foreshadow_continue"
    );
    assert!(
        notes.iter().any(|n| n.contains("本次未取得结果")),
        "after spending the 1x foreshadow budget, multimodal must emit honest no-result note: {notes:?}"
    );
}
