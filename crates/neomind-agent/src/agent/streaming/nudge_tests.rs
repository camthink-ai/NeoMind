//! Streaming protocol and noninterference tests with deterministic native calls.
//!
//! Gated by `test-utils` so it never compiles into a release binary. Run
//! with: `cargo test -p neomind-agent --features test-utils nudge_tests`.

#![cfg(all(test, feature = "test-utils"))]

use std::sync::Arc;

use async_trait::async_trait;

use neomind_core::llm::backend::LlmRuntime;

use crate::agent::types::{AgentEvent, AgentInternalState};
use crate::testing_helpers::mock_llm::{MockLlmRuntime, MockResponse};
use crate::toolkit::error::ToolError;
use crate::toolkit::registry::ToolRegistry;
use crate::toolkit::tool::{Tool, ToolOutput};

/// Deterministic side-effect-free tool, same idea as the executor's EchoTool.
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

fn harness(
    rt: &MockLlmRuntime,
) -> (
    Arc<crate::llm::LlmInterface>,
    Arc<tokio::sync::RwLock<AgentInternalState>>,
    Arc<ToolRegistry>,
) {
    let iface = crate::llm::LlmInterface::default();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt.clone());
    futures::executor::block_on(iface.set_llm(rt_dyn));

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool));
    let registry = Arc::new(registry);

    let state = Arc::new(tokio::sync::RwLock::new(AgentInternalState::new(
        "nudge-test".to_string(),
    )));

    (Arc::new(iface), state, registry)
}

/// Collect every event from a stream (bounded by count — the loops under test
/// have their own iteration caps; this is just a hard stop for a runaway).
async fn collect(
    stream: std::pin::Pin<Box<dyn futures::Stream<Item = AgentEvent> + Send>>,
) -> Vec<AgentEvent> {
    use futures::StreamExt;
    let mut events = Vec::new();
    let mut stream = stream;
    while let Some(ev) = stream.next().await {
        events.push(ev);
        if events.len() > 200 {
            break;
        }
    }
    events
}

fn has_tool_call_start(events: &[AgentEvent], name: &str) -> bool {
    events
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolCallStart { tool, .. } if tool == name))
}

fn joined_content(events: &[AgentEvent]) -> String {
    events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Content { content, .. } => Some(content.clone()),
            _ => None,
        })
        .collect()
}

/// Promise-shaped text remains an ordinary answer, even for callers using
/// the retired behavior-guard option. It never authorizes an extra action.
#[tokio::test]
async fn promise_text_never_launches_an_unrequested_action() {
    for enabled in [false, true] {
        let narration = "好的，我现在使用 echo 工具来处理。";
        let rt = MockLlmRuntime::new(vec![
            MockResponse::text(narration),
            MockResponse::tool_call("echo", serde_json::json!({"msg": "must not run"})),
        ]);
        let (iface, state, registry) = harness(&rt);
        let stream = super::process_stream_events_with_safeguards(
            iface,
            state.clone(),
            registry,
            "先不要执行，只说计划",
            super::StreamSafeguards::default().with_behavioral_guards(enabled),
            None,
            None,
        )
        .await
        .expect("stream");
        let events = collect(stream).await;
        assert_eq!(rt.call_count(), 1);
        assert!(!has_tool_call_start(&events, "echo"));
        assert_eq!(joined_content(&events), narration);
        assert!(!events
            .iter()
            .any(|e| matches!(e, AgentEvent::SystemNote { .. })));
        assert_eq!(
            &*state.read().await.memory.last().unwrap().content,
            narration
        );
        assert!(matches!(events.last(), Some(AgentEvent::End { .. })));
    }
}

/// A native tool call is still executed and its real result reaches the next
/// round. A JSON final is valid output and must survive both UI and persistence.
#[tokio::test]
async fn native_call_followed_by_json_preserves_the_final_answer() {
    for final_text in ["[]", "{}", "null"] {
        let rt = MockLlmRuntime::new(vec![
            MockResponse::tool_call("echo", serde_json::json!({"msg": "checked"})),
            MockResponse::text(final_text),
        ]);
        let (iface, state, registry) = harness(&rt);
        let stream = super::process_stream_events_with_safeguards(
            iface,
            state.clone(),
            registry,
            "检查后返回 JSON",
            super::StreamSafeguards::default(),
            None,
            None,
        )
        .await
        .expect("stream");
        let events = collect(stream).await;
        assert_eq!(rt.call_count(), 2);
        assert!(has_tool_call_start(&events, "echo"));
        assert!(joined_content(&events).ends_with(final_text));
        assert_eq!(
            &*state.read().await.memory.last().unwrap().content,
            final_text
        );
        assert!(matches!(events.last(), Some(AgentEvent::End { .. })));
    }
}

#[tokio::test]
async fn multimodal_native_call_still_produces_tool_events_and_a_summary() {
    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call("echo", serde_json::json!({"msg": "checked"})),
        MockResponse::text("summary of results"),
    ]);
    let (iface, state, registry) = harness(&rt);
    let stream = super::process_multimodal_stream_events_with_safeguards(
        iface,
        state,
        registry,
        "检查图片",
        vec![],
        super::StreamSafeguards::default(),
        None,
        None,
    )
    .await
    .expect("stream");
    let events = collect(stream).await;
    assert!(has_tool_call_start(&events, "echo"));
    assert!(joined_content(&events).contains("summary of results"));
    assert!(matches!(events.last(), Some(AgentEvent::End { .. })));
}

/// The resource round budget still terminates a genuinely unbounded tool loop.
#[tokio::test]
async fn explicit_round_budget_stops_calls_and_summarizes() {
    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call("echo", serde_json::json!({"msg": "checked"})),
        MockResponse::text("budget summary").with_thinking("summary reasoning"),
    ]);
    let (iface, state, registry) = harness(&rt);
    let stream = super::process_stream_events_with_safeguards(
        iface,
        state.clone(),
        registry,
        "检查",
        super::StreamSafeguards {
            max_tool_rounds: 1,
            ..Default::default()
        },
        None,
        None,
    )
    .await
    .expect("stream");
    let events = collect(stream).await;
    assert_eq!(rt.call_count(), 2);
    assert_eq!(joined_content(&events), "budget summary");
    assert!(state
        .read()
        .await
        .memory
        .last()
        .unwrap()
        .thinking
        .as_ref()
        .unwrap()
        .contains("summary reasoning"));
    assert!(matches!(events.last(), Some(AgentEvent::End { .. })));
}

#[tokio::test]
async fn multimodal_plain_json_streams_once_and_matches_saved_history() {
    let reply = "```json\n[]\n```";
    let rt = MockLlmRuntime::new(vec![
        MockResponse::text(reply).with_thinking("image reasoning")
    ]);
    let (iface, state, registry) = harness(&rt);
    let stream = super::process_multimodal_stream_events_with_safeguards(
        iface,
        state.clone(),
        registry,
        "用 JSON 回答",
        vec![],
        super::StreamSafeguards::default(),
        None,
        None,
    )
    .await
    .unwrap();
    let events = collect(stream).await;
    assert_eq!(
        joined_content(&events),
        reply,
        "content must be emitted only once"
    );
    assert!(!has_tool_call_start(&events, "echo"));
    let state = state.read().await;
    let saved = state.memory.last().unwrap();
    assert_eq!(&*saved.content, reply);
    assert_eq!(saved.thinking.as_deref(), Some("image reasoning"));
    assert!(matches!(events.last(), Some(AgentEvent::End { .. })));
}

#[test]
fn tool_history_preserves_calls_without_prohibiting_verification() {
    use crate::agent::types::{AgentMessage, ToolCall};
    for narration in ["", "Checking now"] {
        let message = AgentMessage::assistant_with_tools(
            narration,
            vec![ToolCall {
                id: "call1".into(),
                name: "shell".into(),
                arguments: serde_json::json!({"command":"neomind device latest sensor-1"}),
                result: None,
                round: Some(1),
            }],
        );
        let history = message.to_core().content.as_text();
        assert!(history.contains("neomind device latest sensor-1"));
        assert!(history.contains(narration));
        assert!(!history.contains("should not be repeated"));
    }
}
