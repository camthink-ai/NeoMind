//! Streaming-path behavior tests for the narration-collapse guard, driven by
//! `MockLlmRuntime` — the same harness philosophy as the executor's
//! `behavior_tests.rs`, applied to the two CHAT streaming loops (text +
//! multimodal). The original field failure was an image chat where the model
//! narrated "我现在使用 image_edit 工具…" without emitting a tool call and
//! the turn ended with that promise; these tests pin the recovery.
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

/// Text chat path: narration round → nudge → recovered tool call → final
/// answer. The narration must NOT be the turn's last word.
#[tokio::test]
async fn text_path_narration_nudge_recovers_tool_call() {
    let rt = MockLlmRuntime::new(vec![
        MockResponse::text("好的，我现在使用 echo 工具来处理。"),
        MockResponse::tool_call("echo", serde_json::json!({ "msg": "recovered" })),
        MockResponse::text("done after recovery"),
    ]);
    let (iface, state, registry) = harness(&rt);

    let stream = crate::agent::streaming::process_stream_events_with_safeguards(
        iface,
        state,
        registry,
        "帮我处理",
        super::StreamSafeguards::default(),
        None,
        None,
    )
    .await
    .expect("stream");

    let events = collect(stream).await;
    assert!(
        has_tool_call_start(&events, "echo"),
        "nudge must recover the promised tool call; events: {:?}",
        events
            .iter()
            .map(|e| format!("{:?}", std::mem::discriminant(e)))
            .collect::<Vec<_>>()
    );
    let content = joined_content(&events);
    assert!(
        content.contains("done after recovery"),
        "final answer must be the recovered result, got content: {content}"
    );
}

/// Multimodal path: the original failure scenario (image chat narration).
#[tokio::test]
async fn multimodal_path_narration_nudge_recovers_tool_call() {
    let rt = MockLlmRuntime::new(vec![
        MockResponse::text("我现在使用 echo 工具来处理。"),
        MockResponse::tool_call("echo", serde_json::json!({ "msg": "recovered" })),
        MockResponse::text("summary of results"),
    ]);
    let (iface, state, registry) = harness(&rt);

    let stream = crate::agent::streaming::process_multimodal_stream_events_with_safeguards(
        iface,
        state,
        registry,
        "帮我处理",
        vec![],
        super::StreamSafeguards::default(),
        None,
        None,
    )
    .await
    .expect("stream");

    let events = collect(stream).await;
    assert!(
        has_tool_call_start(&events, "echo"),
        "multimodal nudge must recover the tool call; events: {:?}",
        events
            .iter()
            .map(|e| format!("{:?}", std::mem::discriminant(e)))
            .collect::<Vec<_>>()
    );
    let content = joined_content(&events);
    assert!(
        content.contains("summary of results"),
        "post-tool summary must reach the user, got content: {content}"
    );
}

/// A deterministic tool NAMED image_edit (so the reuse guard applies) that
/// counts real executions.
struct CountingImageEdit {
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

#[async_trait]
impl Tool for CountingImageEdit {
    fn name(&self) -> &str {
        "image_edit"
    }
    fn description(&self) -> &str {
        "counting fake image_edit"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }
    async fn execute(&self, _args: serde_json::Value) -> Result<ToolOutput, ToolError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(ToolOutput::success(serde_json::json!({"status": "ok"})))
    }
}

/// Identical re-calls of a deterministic tool within one turn are reused,
/// not re-executed: round 1 executes, round 2's byte-identical call returns
/// the cached result with a `_reused` marker instead of burning a real
/// execution (field pattern: two identical image_edit calls in a row).
#[tokio::test]
async fn identical_reusable_tool_call_is_served_from_turn_cache() {
    use crate::agent::tool_parser::parse_tool_calls;

    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call(
            "image_edit",
            serde_json::json!({ "image": "a.png", "operations": [{"type": "draw_text", "text": "CamThink"}] }),
        ),
        MockResponse::tool_call(
            "image_edit",
            serde_json::json!({ "image": "a.png", "operations": [{"type": "draw_text", "text": "CamThink"}] }),
        ),
        MockResponse::text("done after reuse"),
    ]);
    let iface = crate::llm::LlmInterface::default();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt.clone());
    futures::executor::block_on(iface.set_llm(rt_dyn));

    let mut registry = ToolRegistry::new();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    registry.register(Arc::new(CountingImageEdit {
        calls: calls.clone(),
    }));
    let registry = Arc::new(registry);
    let state = Arc::new(tokio::sync::RwLock::new(AgentInternalState::new(
        "reuse-test".to_string(),
    )));

    let stream = crate::agent::streaming::process_stream_events_with_safeguards(
        Arc::new(iface),
        state.clone(),
        registry,
        "给图片加水印",
        super::StreamSafeguards::default(),
        None,
        None,
    )
    .await
    .expect("stream");
    let events = collect(stream).await;

    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "the identical second call must be REUSED, not executed"
    );
    let content = joined_content(&events);
    assert!(
        content.contains("done after reuse"),
        "turn must complete: {content}"
    );
    // The reused result carries the marker so the model can stop re-calling.
    let _ = parse_tool_calls(""); // link check
    let reused_marker_seen = format!("{:?}", events).contains("_reused");
    assert!(reused_marker_seen, "the reuse marker must reach the model");
}
