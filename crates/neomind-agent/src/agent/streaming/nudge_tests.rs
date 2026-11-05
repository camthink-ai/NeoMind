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

/// A real tool result contract, exercised through the default chat entry.
struct GuideTool;

#[async_trait]
impl Tool for GuideTool {
    fn name(&self) -> &str {
        "skill"
    }
    fn description(&self) -> &str {
        "workflow guides"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type":"object"})
    }
    async fn execute(&self, args: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let data = if args["action"] == "search" {
            serde_json::json!({"found":false,"scope":"workflow_guides","matches":[]})
        } else {
            serde_json::json!({"id":args["id"],"guide":"Known platform workflow, not vendor evidence."})
        };
        Ok(ToolOutput::success(data))
    }
}

async fn guide_turn(rt: &MockLlmRuntime) -> Vec<AgentEvent> {
    let (iface, state, _) = harness(rt);
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(GuideTool));
    let stream = crate::agent::streaming::process_stream_events_with_safeguards(
        iface,
        state,
        Arc::new(registry),
        "搜索小米设备的协议",
        super::StreamSafeguards::default(),
        None,
        None,
    )
    .await
    .expect("stream");
    collect(stream).await
}

#[tokio::test]
async fn default_chat_ends_rephrased_empty_searches_with_grounded_summary() {
    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call(
            "skill",
            serde_json::json!({"action":"search","query":"小米协议"}),
        ),
        MockResponse::tool_call(
            "skill",
            serde_json::json!({"action":"search","query":"Xiaomi protocols"}),
        ),
        MockResponse::text("请提供具体的小米设备型号或官方协议文档，以便核实接入方式。")
            .with_thinking("The user wants a fabricated CamThink protocol list."),
    ]);
    let events = guide_turn(&rt).await;
    assert_eq!(rt.call_count(), 3);
    let content = joined_content(&events);
    assert!(content.contains("请提供具体的小米设备型号"));
    assert!(!content.contains("CamThink"));
    assert!(!content.contains("The user wants"));
    let messages = rt.captured_messages();
    let final_request = messages.last().unwrap().to_string();
    assert!(final_request.contains("Guide lookup repeated without new evidence"));
    assert!(
        !final_request.contains("Known platform workflow"),
        "irrelevant guides must not contaminate the final brief"
    );
}

#[tokio::test]
async fn default_chat_ends_repeated_guide_loads_but_allows_new_guides() {
    let load =
        |id: &str| MockResponse::tool_call("skill", serde_json::json!({"action":"load","id":id}));
    let repeating = MockLlmRuntime::new(vec![
        load("device"),
        load("device"),
        load("device"),
        MockResponse::text("请提供具体型号。"),
    ]);
    guide_turn(&repeating).await;
    assert_eq!(repeating.call_count(), 4);
    assert!(repeating
        .captured_messages()
        .last()
        .unwrap()
        .to_string()
        .contains("Guide lookup repeated without new evidence"));
    let progressing = MockLlmRuntime::new(vec![
        load("device"),
        load("rule"),
        load("dashboard"),
        load("agent"),
        MockResponse::text("已获得不同领域的操作指引。"),
    ]);
    guide_turn(&progressing).await;
    assert_eq!(progressing.call_count(), 5);
    assert!(!progressing
        .captured_messages()
        .last()
        .unwrap()
        .to_string()
        .contains("Verified execution state"));
}

/// Text chat DEFAULT POLICY: a promise-shaped reply ends the turn with a
/// LABEL — no behavioural retry (the 2026-10-21 A/B showed no lift and false
/// labels were the observable cost). One LLM call, one SystemNote.
#[tokio::test]
async fn text_path_chat_policy_labels_promise_without_extra_round() {
    let narration = "好的，我现在使用 echo 工具来处理。";
    let rt = MockLlmRuntime::new(vec![MockResponse::text(narration)]);
    let (iface, state, registry) = harness(&rt);

    let stream = crate::agent::streaming::process_stream_events_with_safeguards(
        iface,
        state.clone(),
        registry,
        "帮我处理",
        super::StreamSafeguards::default(),
        None,
        None,
    )
    .await
    .expect("stream");
    let events = collect(stream).await;

    assert_eq!(
        rt.call_count(),
        1,
        "chat policy must not launch a behavioural retry round"
    );
    assert!(!has_tool_call_start(&events, "echo"));
    assert!(
        joined_content(&events).contains("我现在使用 echo"),
        "the narration itself must reach the user"
    );
    let guard = state.read().await;
    let last = guard.memory.last().expect("final message persisted");
    assert_eq!(&*last.content, narration);
    assert_eq!(
        last.system_notes.as_ref().map(|n| n.len()).unwrap_or(0),
        1,
        "a promise-shaped chat reply carries the progress-report label"
    );
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::SystemNote { .. })));
}

/// The latent behavioural path stays tested (and re-enableable): with the
/// opt-in, narration → nudge → recovered tool call → final answer.
#[tokio::test]
async fn text_path_opt_in_behavioral_guards_still_recover_tool_call() {
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
        super::StreamSafeguards::default().with_behavioral_guards(true),
        None,
        None,
    )
    .await
    .expect("stream");

    let events = collect(stream).await;
    assert!(
        has_tool_call_start(&events, "echo"),
        "opt-in nudge must recover the promised tool call"
    );
    let content = joined_content(&events);
    assert!(
        content.contains("done after recovery"),
        "final answer must be the recovered result, got content: {content}"
    );
}

/// Live 2026-10-20 incident, verbatim shape: a COMPLETE capability-list
/// answer ending in the politeness closing "随时告诉我，我来帮你一步步实
/// 现！". The old classifier matched "我来帮" anywhere, launched a nudge
/// round after the answer had already streamed ("r2 thinking after reply
/// complete"), and the model answered the nudge with a bare `[]`. With the
/// position-sensitive classifier the turn must end after ONE round: no
/// nudge call, the reply itself persisted as the final message.
#[tokio::test]
async fn complete_reply_with_tail_politeness_closing_never_enters_round_two() {
    let incident_reply = format!(
        "你好！我是 NeoMind，一个驻扎在 IoT 边缘平台上的 AI 工程师助手。我可以帮你处理以下工作：{}如果你有具体需求，随时告诉我，我来帮你一步步实现！",
        "平台操作、设备管理、数据处理、模型调用、系统设置。".repeat(6)
    );
    assert!(incident_reply.chars().count() > 200);
    let rt = MockLlmRuntime::new(vec![MockResponse::text(incident_reply.clone())]);
    let (iface, state, registry) = harness(&rt);

    let stream = crate::agent::streaming::process_stream_events_with_safeguards(
        iface,
        state.clone(),
        registry,
        "你可以做什么事情？",
        super::StreamSafeguards::default(),
        None,
        None,
    )
    .await
    .expect("stream");
    let events = collect(stream).await;

    assert_eq!(
        rt.call_count(),
        1,
        "a complete answer with a tail politeness closing must NOT launch a nudge round"
    );
    let content = joined_content(&events);
    assert!(
        content.contains("我来帮你一步步实现"),
        "the full reply must stream to the user, got: {content}"
    );
    let guard = state.read().await;
    let last = guard
        .memory
        .last()
        .expect("final assistant message persisted");
    assert_eq!(
        &*last.content, &incident_reply,
        "the persisted final message must be the reply itself"
    );
    assert!(
        last.system_notes.is_none(),
        "no progress-report label on a complete answer"
    );
}

/// A user who explicitly asks for a PLAN only must not be nudged into
/// executing it, and the promise-shaped reply must not be labelled as a
/// failed turn — both would contradict the instruction.
#[tokio::test]
async fn plan_only_request_suppresses_nudge_and_label() {
    let plan = "我现在使用 shell 工具执行 neomind device list 来检查设备状态。";
    let rt = MockLlmRuntime::new(vec![MockResponse::text(plan)]);
    let (iface, state, registry) = harness(&rt);

    let stream = crate::agent::streaming::process_stream_events_with_safeguards(
        iface,
        state.clone(),
        registry,
        "先不要执行，只告诉我你打算怎么检查设备状态。",
        super::StreamSafeguards::default().with_behavioral_guards(true),
        None,
        None,
    )
    .await
    .expect("stream");
    let events = collect(stream).await;

    assert_eq!(
        rt.call_count(),
        1,
        "plan-only request must not launch a nudge round"
    );
    let guard = state.read().await;
    let last = guard.memory.last().expect("final message persisted");
    assert_eq!(&*last.content, plan);
    assert!(
        last.system_notes.is_none(),
        "a requested plan is not a failed turn"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, AgentEvent::SystemNote { .. })),
        "no progress-report callout for a requested plan"
    );
}

/// A LATER round (after the nudge recovered real tool calls and they ran)
/// ending degenerate must NOT resurrect the round-1 narration: that would
/// discard the tool results and re-surface a promise as the final answer.
/// The restore is scoped to the nudge round; later rounds go through the
/// no-thinking retry instead.
#[tokio::test]
async fn later_round_degenerate_output_does_not_resurrect_stale_narration() {
    let narration = "我来检查仪表盘和组件的当前状态，确认是否正确绑定。";
    let rt = MockLlmRuntime::new(vec![
        // Round 1: promise-led narration → nudge.
        MockResponse::text(narration),
        // The NUDGE round recovers and emits a real tool call.
        MockResponse::tool_call("echo", serde_json::json!({ "msg": "checked" })),
        // A LATER round degenerates.
        MockResponse::text("[]"),
        // The no-thinking retry produces the real conclusion.
        MockResponse::text("检查完成：绑定正常。"),
    ]);
    let (iface, state, registry) = harness(&rt);

    let stream = crate::agent::streaming::process_stream_events_with_safeguards(
        iface,
        state.clone(),
        registry,
        "检查仪表盘绑定状态",
        super::StreamSafeguards::default().with_behavioral_guards(true),
        None,
        None,
    )
    .await
    .expect("stream");
    let _events = collect(stream).await;

    assert!(
        rt.call_count() >= 4,
        "narration → nudge(tool) → degenerate round → retry; got {} calls",
        rt.call_count()
    );
    let guard = state.read().await;
    let last = guard.memory.last().expect("final message persisted");
    assert_ne!(
        &*last.content, narration,
        "the stale pre-nudge narration must NOT be restored after the tools ran"
    );
    assert_eq!(
        &*last.content, "检查完成：绑定正常。",
        "the retry's real conclusion must be the final message"
    );
}

/// The nudge CAN still fire for a real promise-led collapse — and when the
/// nudged round answers the JSON-array directive with a bare `[]` (the
/// small-model literal-compliance failure), the pre-nudge narration must be
/// restored as the final reply: no `[]` reaches the user, the restored
/// promise-shaped narration is labeled as a progress report.
#[tokio::test]
async fn nudge_round_answering_empty_array_restores_the_narration() {
    let narration = "我来检查仪表盘和组件的当前状态，确认是否正确绑定。";
    let rt = MockLlmRuntime::new(vec![
        MockResponse::text(narration),
        MockResponse::text("[]"),
    ]);
    let (iface, state, registry) = harness(&rt);

    let stream = crate::agent::streaming::process_stream_events_with_safeguards(
        iface,
        state.clone(),
        registry,
        "检查仪表盘绑定状态",
        super::StreamSafeguards::default().with_behavioral_guards(true),
        None,
        None,
    )
    .await
    .expect("stream");
    let events = collect(stream).await;

    assert_eq!(
        rt.call_count(),
        2,
        "promise-led collapse nudges exactly once more"
    );
    // Round 2's request carried the nudge prompt as its user message.
    let captured = rt.captured_messages();
    let call2 =
        serde_json::to_string(captured.get(1).expect("two calls captured")).expect("serializable");
    assert!(
        call2.contains("承诺了要执行操作"),
        "the nudge prompt must reach round 2, got: {call2}"
    );
    // The degenerate empty array never reaches the user.
    let content = joined_content(&events);
    assert!(
        !content.contains('['),
        "no bare JSON container may be displayed, got: {content}"
    );
    // The narration was already streamed and stays the turn's visible reply.
    assert!(
        content.contains(narration),
        "the narration must remain the visible reply, got: {content}"
    );
    // The persisted final message is the restored narration — not `[]`.
    let guard = state.read().await;
    let last = guard
        .memory
        .last()
        .expect("final assistant message persisted");
    assert_eq!(
        &*last.content, narration,
        "the final message must be the restored narration, not the degenerate round's output"
    );
    assert_eq!(
        last.system_notes.as_ref().map(|n| n.len()).unwrap_or(0),
        1,
        "the restored promise-shaped narration carries the progress-report label"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::SystemNote { .. })),
        "the progress-report SystemNote must reach the UI"
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

/// Multimodal recovery-channel guards (leakfix-r2 mechanism B): reasoning
/// arriving with `is_thinking = true` on the four multimodal summary /
/// continuation / nudge consumers must never reach Content events or
/// persisted memory — the same contract the text path got in 7ccbffa7.
mod multimodal_recovery_guards {
    use super::{collect, harness, joined_content, ProgressShell};
    use crate::agent::types::{AgentEvent, AgentInternalState};
    use crate::testing_helpers::mock_llm::{MockLlmRuntime, MockResponse};
    use crate::toolkit::registry::ToolRegistry;
    use std::sync::Arc;

    const SECRET_SUMMARY: &str = "PRIVATE_MM_SUMMARY_REASONING";
    const SECRET_CONTINUATION: &str = "PRIVATE_MM_CONTINUATION_REASONING";
    const SECRET_NUDGE: &str = "PRIVATE_MM_NUDGE_REASONING";
    const PUBLIC_SUMMARY: &str = "PUBLIC_MM_SUMMARY_ANSWER";

    async fn run_multimodal(
        rt: &MockLlmRuntime,
        user_message: &str,
    ) -> (
        Vec<AgentEvent>,
        Arc<tokio::sync::RwLock<AgentInternalState>>,
    ) {
        let (iface, state, _) = harness(rt);
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(super::EchoTool));
        registry.register(Arc::new(ProgressShell {
            commands: Arc::new(std::sync::Mutex::new(Vec::new())),
        }));
        let stream = crate::agent::streaming::process_multimodal_stream_events_with_safeguards(
            iface,
            state.clone(),
            Arc::new(registry),
            user_message,
            vec![],
            crate::agent::StreamSafeguards::default(),
            None,
            None,
        )
        .await
        .expect("stream");
        (collect(stream).await, state)
    }

    async fn assert_no_secret(
        events: &[AgentEvent],
        state: &Arc<tokio::sync::RwLock<AgentInternalState>>,
    ) {
        let visible = joined_content(events);
        for secret in [SECRET_SUMMARY, SECRET_CONTINUATION, SECRET_NUDGE] {
            assert!(
                !visible.contains(secret),
                "reasoning leaked into Content: {visible}"
            );
        }
        let guard = state.read().await;
        for msg in guard.memory.iter() {
            let text = msg.content.to_string();
            for secret in [SECRET_SUMMARY, SECRET_CONTINUATION, SECRET_NUDGE] {
                assert!(
                    !text.contains(secret),
                    "reasoning leaked into memory: {text}"
                );
            }
        }
    }

    /// Main tool-path summary (chat_stream_summary after a tool round).
    #[tokio::test]
    async fn multimodal_main_summary_drops_reasoning_channel() {
        let rt = MockLlmRuntime::new(vec![
            MockResponse::tool_call("echo", serde_json::json!({ "msg": "checked" })),
            MockResponse::text(PUBLIC_SUMMARY).with_thinking(SECRET_SUMMARY),
        ]);
        let (events, state) = run_multimodal(&rt, "帮我处理").await;
        assert!(joined_content(&events).contains(PUBLIC_SUMMARY));
        assert_no_secret(&events, &state).await;
    }

    /// Dead-end continuation buffer + the summary that follows it: both
    /// consumers must drop the reasoning channel.
    #[tokio::test]
    async fn multimodal_continuation_buffer_drops_reasoning_channel() {
        let rt = MockLlmRuntime::new(vec![
            MockResponse::tool_call(
                "shell",
                serde_json::json!({ "command": "neomind device list" }),
            ),
            // Continuation answer: reasoning plus a public line, no tool
            // calls — falls through to the summary.
            MockResponse::text("I will create the dashboard next.")
                .with_thinking(SECRET_CONTINUATION),
            MockResponse::text(PUBLIC_SUMMARY).with_thinking(SECRET_SUMMARY),
        ]);
        let (events, state) = run_multimodal(&rt, "创建一个温湿度仪表盘").await;
        assert!(events.iter().any(|e| matches!(
            e,
            AgentEvent::Progress { ref stage, .. } if stage.as_deref() == Some("continuing")
        )));
        assert!(joined_content(&events).contains(PUBLIC_SUMMARY));
        assert_no_secret(&events, &state).await;
    }

    /// Nudge round degenerating to `[]` with reasoning: the restored
    /// narration must stay clean of the reasoning channel.
    #[tokio::test]
    async fn multimodal_nudge_degenerate_round_drops_reasoning_channel() {
        let narration = "我现在使用 echo 工具来处理。";
        let rt = MockLlmRuntime::new(vec![
            MockResponse::text(narration),
            MockResponse::text("[]").with_thinking(SECRET_NUDGE),
        ]);
        let (events, state) = run_multimodal(&rt, "帮我处理").await;
        let visible = joined_content(&events);
        assert!(
            visible.contains(narration),
            "narration must be kept: {visible}"
        );
        assert_no_secret(&events, &state).await;
    }

    /// Post-nudge summary after a recovered tool call.
    #[tokio::test]
    async fn multimodal_post_nudge_summary_drops_reasoning_channel() {
        let rt = MockLlmRuntime::new(vec![
            MockResponse::text("我现在使用 echo 工具来处理。"),
            MockResponse::tool_call("echo", serde_json::json!({ "msg": "recovered" })),
            MockResponse::text(PUBLIC_SUMMARY).with_thinking(SECRET_SUMMARY),
        ]);
        let (events, state) = run_multimodal(&rt, "帮我处理").await;
        assert!(joined_content(&events).contains(PUBLIC_SUMMARY));
        assert_no_secret(&events, &state).await;
    }
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
/// Records actual shell executions while returning distinct evidence per command.
struct ProgressShell {
    commands: Arc<std::sync::Mutex<Vec<String>>>,
}

#[async_trait]
impl Tool for ProgressShell {
    fn name(&self) -> &str {
        "shell"
    }
    fn description(&self) -> &str {
        "successful platform command"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type":"object","properties":{"command":{"type":"string"}}})
    }
    async fn execute(&self, args: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let command = args["command"].as_str().unwrap().to_string();
        self.commands.lock().unwrap().push(command.clone());
        Ok(ToolOutput::success(serde_json::json!({
            "exit_code": 0,
            "stdout": command,
            "stderr": ""
        })))
    }
}

fn tool_batch(responses: Vec<MockResponse>) -> MockResponse {
    let mut batch = MockResponse::text("");
    batch.tool_calls = responses
        .into_iter()
        .flat_map(|response| response.tool_calls)
        .collect();
    batch
}

async fn mixed_progress_turn(rt: &MockLlmRuntime) -> (Vec<AgentEvent>, Vec<String>, String) {
    mixed_progress_request(rt, "检查 sensor-1 并将名称更新为 verified-sensor").await
}

async fn mixed_progress_request(
    rt: &MockLlmRuntime,
    request: &str,
) -> (Vec<AgentEvent>, Vec<String>, String) {
    let (iface, state, _) = harness(rt);
    let commands = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(GuideTool));
    registry.register(Arc::new(ProgressShell {
        commands: commands.clone(),
    }));
    let stream = crate::agent::streaming::process_stream_events_with_safeguards(
        iface,
        state.clone(),
        Arc::new(registry),
        request,
        super::StreamSafeguards::default(),
        None,
        None,
    )
    .await
    .expect("stream");
    let events = collect(stream).await;
    let executed = commands.lock().unwrap().clone();
    let last_reply = state
        .read()
        .await
        .memory
        .last()
        .expect("final reply persisted")
        .content
        .clone();
    (events, executed, last_reply.to_string())
}

#[tokio::test]
async fn successful_mutation_in_a_batch_resets_skill_budget_in_both_orders() {
    const UPDATE: &str = "neomind device update sensor-1 --name verified-sensor";
    const FINAL: &str = "sensor-1 已更新为 verified-sensor。";

    for mutation_first in [false, true] {
        let first_miss = MockResponse::tool_call(
            "skill",
            serde_json::json!({"action":"search","query":"设备改名指南"}),
        );
        let second_miss = MockResponse::tool_call(
            "skill",
            serde_json::json!({"action":"search","query":"rename device workflow"}),
        );
        let mutation = MockResponse::tool_call("shell", serde_json::json!({"command":UPDATE}));
        let calls = if mutation_first {
            vec![mutation, first_miss, second_miss]
        } else {
            vec![first_miss, second_miss, mutation]
        };
        let rt = MockLlmRuntime::new(vec![tool_batch(calls), MockResponse::text(FINAL)]);
        let (events, commands, last_reply) = mixed_progress_turn(&rt).await;

        assert_eq!(
            commands,
            vec![UPDATE.to_string()],
            "mutation must actually execute"
        );
        assert_eq!(rt.call_count(), 2);
        let requests = rt.captured_messages();
        let final_request = requests.last().unwrap().to_string();
        assert!(
            final_request.contains("Tool-round context"),
            "must continue the original task"
        );
        assert!(
            final_request.contains(UPDATE),
            "the continuation must carry mutation evidence"
        );
        assert!(
            !final_request.contains("execution budget or loop guard ended this turn"),
            "concurrent batch order must not turn successful progress into exhaustion"
        );
        assert!(joined_content(&events).contains(FINAL));
        assert_eq!(
            last_reply, FINAL,
            "the normal final response must be persisted"
        );
    }
}

#[tokio::test]
async fn useful_reads_allow_action_after_two_empty_guide_searches() {
    const LIST: &str = "neomind device list";
    const GET: &str = "neomind device get sensor-1";
    const UPDATE: &str = "neomind device update sensor-1 --name verified-sensor";
    const FINAL: &str = "根据读取到的设备信息，sensor-1 已更新为 verified-sensor。";
    let miss_and_read = |query: &str, command: &str| {
        tool_batch(vec![
            MockResponse::tool_call(
                "skill",
                serde_json::json!({"action":"search","query":query}),
            ),
            MockResponse::tool_call("shell", serde_json::json!({"command":command})),
        ])
    };
    let rt = MockLlmRuntime::new(vec![
        miss_and_read("设备改名指南", LIST),
        miss_and_read("rename device workflow", GET),
        MockResponse::tool_call("shell", serde_json::json!({"command":UPDATE})),
        MockResponse::text(FINAL),
    ]);
    let (events, commands, last_reply) = mixed_progress_turn(&rt).await;

    assert_eq!(rt.call_count(), 4);
    for expected in [LIST, GET, UPDATE] {
        assert_eq!(
            commands
                .iter()
                .filter(|command| command.as_str() == expected)
                .count(),
            1,
            "useful reads must allow the intended mutation to execute exactly once"
        );
    }
    let requests = rt.captured_messages();
    for request in requests.iter().skip(1) {
        let request = request.to_string();
        assert!(request.contains("Tool-round context"));
        assert!(
            !request.contains("execution budget or loop guard ended this turn"),
            "new device evidence must not be mistaken for a guide-only exploration loop"
        );
    }
    assert!(
        requests[2].to_string().contains(GET),
        "the action request must retain discovered evidence"
    );
    assert!(requests.last().unwrap().to_string().contains(UPDATE));
    assert!(joined_content(&events).contains(FINAL));
    assert_eq!(
        last_reply, FINAL,
        "persist the action result rather than an exhaustion reply"
    );
}

#[tokio::test]
async fn grounded_exploration_uses_fallback_only_when_generation_is_unusable() {
    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call(
            "skill",
            serde_json::json!({"action":"search","query":"missing one"}),
        ),
        MockResponse::tool_call(
            "skill",
            serde_json::json!({"action":"search","query":"missing two"}),
        ),
        MockResponse::tool_call(
            "skill",
            serde_json::json!({"action":"load","id":"ignored-guide"}),
        ),
        MockResponse::text("{}"),
    ]);
    let events = guide_turn(&rt).await;
    assert_eq!(
        rt.call_count(),
        4,
        "bounded final expression, no rewrite loop"
    );
    assert!(joined_content(&events)
        .ends_with(super::guard::guide_exploration_reply("搜索小米设备的协议")));
    assert!(!joined_content(&events).contains("{}"));
}

#[tokio::test]
async fn guide_recovery_keeps_action_and_verification_available() {
    let load = || {
        MockResponse::tool_call(
            "skill",
            serde_json::json!({"action":"load","id":"connector-management"}),
        )
    };
    let rt = MockLlmRuntime::new(vec![
        load(),
        load(),
        load(),
        MockResponse::tool_call(
            "shell",
            serde_json::json!({"command":"neomind push create --name ops-webhook --type webhook --config '{}'"}),
        ),
        MockResponse::tool_call("shell", serde_json::json!({"command":"neomind push list"})),
        MockResponse::text("已创建并确认推送目标。"),
    ]);
    let (events, commands, reply) = mixed_progress_request(
        &rt,
        "创建 webhook 推送目标 ops-webhook，URL 已提供，并验证列表",
    )
    .await;
    assert_eq!(rt.call_count(), 6);
    assert!(commands.iter().any(|s| s.contains("push create")));
    assert!(commands.iter().any(|s| s.contains("push list")));
    assert_eq!(reply, "已创建并确认推送目标。");
    assert!(!joined_content(&events).contains("设备型号"));
    let requests = rt.captured_messages();
    assert!(requests[3]
        .to_string()
        .contains("Guide lookup repeated without new evidence"));
    assert!(
        !requests[3].to_string().contains("Known platform workflow"),
        "{}",
        requests[3]
    );
}

#[tokio::test]
async fn guide_recovery_cannot_execute_an_unadvertised_skill_again() {
    let load = || {
        MockResponse::tool_call(
            "skill",
            serde_json::json!({"action":"load","id":"connector-management"}),
        )
    };
    let rt = MockLlmRuntime::new(vec![
        load(),
        load(),
        load(),
        load(),
        MockResponse::text("这次尚未完成操作。"),
    ]);
    let events = guide_turn(&rt).await;
    assert_eq!(
        rt.call_count(),
        5,
        "one recovery request and one final expression only"
    );
    assert!(events
        .iter()
        .any(|e| matches!(e,AgentEvent::ToolCallEnd{tool,success:false,..} if tool=="skill")));
    assert!(joined_content(&events).contains("尚未完成"));
}

async fn assert_recovery_keeps_reasoning_out_of_content(
    script: Vec<MockResponse>,
    persisted: bool,
) {
    const SECRET: &str = "PRIVATE_RECOVERY_REASONING";
    let rt = MockLlmRuntime::new(script);
    let (iface, state, registry) = harness(&rt);
    let stream = crate::agent::streaming::process_stream_events_with_safeguards(
        iface,
        state.clone(),
        registry,
        "Report the result.",
        super::StreamSafeguards::default(),
        None,
        None,
    )
    .await
    .unwrap();
    let events = collect(stream).await;
    let visible = joined_content(&events);
    assert!(
        !visible.contains(SECRET),
        "reasoning leaked into Content: {visible}"
    );
    assert!(visible.contains("PUBLIC_RECOVERY_ANSWER"));
    assert!(visible.contains("literal </think> remains"));
    assert!(rt.call_count() >= 2, "must exercise recovery");
    let guard = state.read().await;
    if persisted {
        assert_eq!(
            guard.memory.last().unwrap().content.to_string(),
            "PUBLIC_RECOVERY_ANSWER; literal </think> remains."
        );
    }
    for msg in guard.memory.iter() {
        assert!(!msg.content.to_string().contains(SECRET));
    }
}

fn recovery_answer() -> MockResponse {
    MockResponse::text("PUBLIC_RECOVERY_ANSWER; literal </think> remains.")
        .with_thinking("PRIVATE_RECOVERY_REASONING")
}

#[tokio::test]
async fn recovery_channels_empty_answer_does_not_expose_reasoning() {
    assert_recovery_keeps_reasoning_out_of_content(
        vec![MockResponse::text("[]"), recovery_answer()],
        true,
    )
    .await;
}

#[tokio::test]
async fn recovery_channels_incomplete_json_does_not_expose_reasoning() {
    assert_recovery_keeps_reasoning_out_of_content(
        vec![
            MockResponse::text("[{\"name\":\"echo\",\"arguments\":{"),
            recovery_answer(),
        ],
        true,
    )
    .await;
}

#[tokio::test]
async fn recovery_channels_failed_followup_does_not_expose_reasoning() {
    assert_recovery_keeps_reasoning_out_of_content(
        vec![
            MockResponse::tool_call("echo", serde_json::json!({"msg":"checked"})),
            MockResponse::error("followup request failed"),
            recovery_answer(),
        ],
        false,
    )
    .await;
}
