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
