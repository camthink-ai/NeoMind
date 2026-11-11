//! Closed-loop `$cached:` read-back tests (2026-11-09 round).
//!
//! These drive the REAL streaming dispatch chain — `MockLlmRuntime` scripts
//! the model, real tools execute through the real registry, and the session
//! `AgentInternalState` carries the cache across rounds — proving the whole
//! wiring, not just `LargeDataCache` in isolation:
//!
//! 1. a big tool output is slimmed into a REAL `$cached:` reference;
//! 2. the NEXT turn's `file_read(path="$cached:…")` calls are served through
//!    the same dispatch chain and paginate losslessly;
//! 3. the same reference in a DIFFERENT session is explicitly rejected;
//! 4. a disabled `file_read` falls through to the registry's own Disabled
//!    error — the cache door never bypasses the registry gate.

use std::sync::Arc;

use async_trait::async_trait;

use crate::agent::streaming::StreamSafeguards;
use crate::testing_helpers::mock_llm::{MockLlmRuntime, MockResponse};
use crate::toolkit::registry::ToolRegistry;
use crate::toolkit::tool::{Tool, ToolOutput};
use crate::AgentEvent;

use neomind_core::llm::backend::{LlmInput, LlmRuntime};

/// Non-repetitive payload: 65,600 chars of UNIQUE blocks (each embeds its
/// block index), ≈78KB UTF-8 — over the 64KiB slim threshold, and a wrong
/// window offset can never reproduce the expected text.
fn big_payload() -> String {
    let mut payload = String::new();
    let mut block = 0u32;
    while payload.chars().count() < 65_600 {
        payload.push_str(&format!("块{:06}·payload-block·", block));
        block += 1;
    }
    payload.chars().take(65_600).collect()
}

/// A tool whose result contains a >64KB string — the realistic "large tool
/// output" producer that triggers the slim layer.
struct BigOutputTool;

#[async_trait]
impl Tool for BigOutputTool {
    fn name(&self) -> &str {
        "big_output"
    }
    fn description(&self) -> &str {
        "returns a large log payload"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }
    async fn execute(
        &self,
        _args: serde_json::Value,
    ) -> Result<ToolOutput, crate::toolkit::error::ToolError> {
        Ok(ToolOutput::success(serde_json::json!({
            "log": big_payload()
        })))
    }
}

fn harness_with_state(rt: &MockLlmRuntime) -> (Arc<crate::llm::LlmInterface>, Arc<ToolRegistry>) {
    let iface = crate::llm::LlmInterface::default();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt.clone());
    futures::executor::block_on(iface.set_llm(rt_dyn));

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(BigOutputTool));
    registry.register(Arc::new(crate::toolkit::file_read::FileReadTool::new(
        std::env::temp_dir().join(format!("cached-read-chain-{}", std::process::id())),
    )));
    let registry = Arc::new(registry);

    (Arc::new(iface), registry)
}

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

fn tool_end_results(events: &[AgentEvent], tool: &str) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ToolCallEnd {
                tool: t,
                result,
                success,
                ..
            } if t == tool => Some(format!("{}|{}", success, result)),
            _ => None,
        })
        .collect()
}

/// Extract a top-level field from a tool-end result's JSON payload
/// (the part after the `success|` marker). Handles both the bare-data and
/// data-wrapped shapes; returns the raw JSON fragment as a String.
fn extract_field(result: &str, field: &str) -> Option<String> {
    let json_part = result.split_once('|')?.1;
    let v: serde_json::Value = serde_json::from_str(json_part).ok()?;
    let value = v
        .get(field)
        .or_else(|| v.pointer(&format!("/data/{field}")))?;
    match value {
        serde_json::Value::String(s) => Some(s.to_string()),
        serde_json::Value::Null => Some("null".to_string()),
        other => Some(other.to_string()),
    }
}

#[tokio::test]
async fn big_output_refs_are_readable_next_turn_through_real_dispatch() {
    let payload = big_payload();

    // ── Turn 1: the model runs the big tool; the real slim layer caches
    // the payload and replaces it with a $cached reference in history.
    let rt1 = MockLlmRuntime::new(vec![
        MockResponse::tool_call("big_output", serde_json::json!({})),
        MockResponse::text("收到大输出摘要"),
    ]);
    let state = Arc::new(tokio::sync::RwLock::new(
        crate::agent::types::AgentInternalState::new("chain-session".to_string()),
    ));
    let (iface1, registry1) = harness_with_state(&rt1);
    let events1 = collect(
        crate::agent::streaming::process_stream_events_with_safeguards(
            iface1,
            state.clone(),
            registry1,
            "拉取大日志",
            StreamSafeguards::default(),
            None,
            None,
        )
        .await
        .expect("stream"),
    )
    .await;
    assert!(
        tool_end_results(&events1, "big_output")
            .iter()
            .any(|r| r.starts_with("true|")),
        "big_output must succeed"
    );

    // The REAL reference the slim layer generated (extracted from session
    // state — no key-derivation shortcuts).
    let keys = state.read().await.large_data_cache.entry_keys();
    assert_eq!(keys.len(), 1, "exactly one cache entry after slim");
    let reference = format!("$cached:{}", keys[0]);

    // ── Turn 2 (same session): the model reads the reference back through
    // file_read, following next_char_offset to the END, via the same
    // dispatch chain. Windows: [0, 30k) → [30k, 60k) → [60k, end).
    let rt2 = MockLlmRuntime::new(vec![
        MockResponse::tool_call(
            "file_read",
            serde_json::json!({"path": reference, "char_offset": 0, "char_length": 30000}),
        ),
        MockResponse::tool_call(
            "file_read",
            serde_json::json!({"path": reference, "char_offset": 30000, "char_length": 30000}),
        ),
        MockResponse::tool_call(
            "file_read",
            serde_json::json!({"path": reference, "char_offset": 60000}),
        ),
        MockResponse::text("统计完成"),
    ]);
    // Same SESSION state, fresh iface/registry binding for the new runtime.
    let (iface2, registry2) = harness_with_state(&rt2);
    let events2 = collect(
        crate::agent::streaming::process_stream_events_with_safeguards(
            iface2,
            state.clone(),
            registry2,
            "读回缓存并统计",
            StreamSafeguards::default(),
            None,
            None,
        )
        .await
        .expect("stream"),
    )
    .await;

    let reads = tool_end_results(&events2, "file_read");
    assert_eq!(reads.len(), 3, "three windowed reads, got {reads:?}");
    let mut rebuilt = String::new();
    for (i, r) in reads.iter().enumerate() {
        assert!(r.starts_with("true|"), "read {i} must succeed: {r}");
        let window = extract_field(r, "content").unwrap_or_else(|| panic!("read {i} content: {r}"));
        rebuilt.push_str(&window);
    }
    // Full-payload equality: the windows ARE the complete payload, char for
    // char — no loss, no duplication, offsets provably correct against a
    // non-repetitive payload.
    assert_eq!(
        rebuilt, payload,
        "windows followed to the end must reconstruct the whole payload"
    );
    assert_eq!(rebuilt.chars().count(), 65_600);

    // Per-window continuation metadata.
    assert_eq!(
        extract_field(&reads[0], "next_char_offset").as_deref(),
        Some("30000"),
        "{}",
        reads[0]
    );
    assert_eq!(
        extract_field(&reads[1], "next_char_offset").as_deref(),
        Some("60000"),
        "{}",
        reads[1]
    );
    assert!(
        reads[2].contains("\"next_char_offset\":null"),
        "final window has no continuation: {}",
        reads[2]
    );
    assert!(
        reads[2].contains("\"truncated\":false"),
        "final window is complete: {}",
        reads[2]
    );
    assert!(
        reads[0].contains("\"truncated\":true"),
        "non-final windows are truncated: {}",
        reads[0]
    );

    // ── Turn 3 (DIFFERENT session): the same reference must be explicitly
    // rejected — cache instances are per-session.
    let rt3 = MockLlmRuntime::new(vec![
        MockResponse::tool_call(
            "file_read",
            serde_json::json!({"path": reference, "char_offset": 0}),
        ),
        MockResponse::text("引用失效"),
    ]);
    let other_state = Arc::new(tokio::sync::RwLock::new(
        crate::agent::types::AgentInternalState::new("other-session".to_string()),
    ));
    let (iface3, registry3) = harness_with_state(&rt3);
    let events3 = collect(
        crate::agent::streaming::process_stream_events_with_safeguards(
            iface3,
            other_state,
            registry3,
            "读引用",
            StreamSafeguards::default(),
            None,
            None,
        )
        .await
        .expect("stream"),
    )
    .await;
    let denied = tool_end_results(&events3, "file_read");
    assert_eq!(denied.len(), 1);
    assert!(
        denied[0].starts_with("false|"),
        "cross-session read must fail: {}",
        denied[0]
    );
    assert!(
        denied[0].contains("Unknown $cached reference"),
        "explicit rejection with recovery hint: {}",
        denied[0]
    );
}

/// A disabled `file_read` gets the REGISTRY's Disabled error — the cache
/// door falls through instead of serving a read behind the gate's back.
#[tokio::test]
async fn disabled_file_read_falls_through_to_registry_gate() {
    let state = Arc::new(tokio::sync::RwLock::new(
        crate::agent::types::AgentInternalState::new("gate-session".to_string()),
    ));
    // Seed the cache directly so the reference exists.
    state
        .write()
        .await
        .large_data_cache
        .store("shell.stdout", &big_payload());

    let rt = MockLlmRuntime::new(vec![
        MockResponse::tool_call(
            "file_read",
            serde_json::json!({"path": "$cached:shell.stdout", "char_offset": 0}),
        ),
        MockResponse::text("工具被禁用"),
    ]);
    let (iface, registry_holder) = harness_with_state(&rt);
    registry_holder.set_disabled(std::collections::HashSet::from(["file_read".to_string()]));

    let events = collect(
        crate::agent::streaming::process_stream_events_with_safeguards(
            iface,
            state.clone(),
            registry_holder,
            "读缓存",
            StreamSafeguards::default(),
            None,
            None,
        )
        .await
        .expect("stream"),
    )
    .await;

    let results = tool_end_results(&events, "file_read");
    assert_eq!(results.len(), 1);
    assert!(
        results[0].starts_with("false|"),
        "disabled tool must fail: {}",
        results[0]
    );
    assert!(
        results[0].to_lowercase().contains("disabl"),
        "error must come from the registry gate: {}",
        results[0]
    );
    // The door must NOT have served a window behind the gate.
    assert!(
        !results[0].contains("total_chars"),
        "no cached window may be served while disabled: {}",
        results[0]
    );
}

// Silence unused warnings for the LlmInput import used by the runtime trait.
#[allow(dead_code)]
fn _runtime_type_witness(_input: LlmInput) {}

/// 2026-11-09 A2 integration fix: windowed read-backs must not blow the
/// NEXT user turn's request. After a turn with three 8K windows, the next
/// turn's assembled request (captured from the mock runtime) must (a) not
/// carry the window BODIES, (b) still carry the $cached reference and
/// window metadata so re-reads remain possible, (c) include the new user
/// message and stay within a sane char budget.
#[tokio::test]
async fn multi_window_readback_keeps_next_turn_request_in_budget() {
    let payload = big_payload();

    // Turn 1: big output, then three windowed reads of the reference.
    let state = Arc::new(tokio::sync::RwLock::new(
        crate::agent::types::AgentInternalState::new("budget-session".to_string()),
    ));
    {
        // Stage A: produce the reference first.
        let rt0 = MockLlmRuntime::new(vec![
            MockResponse::tool_call("big_output", serde_json::json!({})),
            MockResponse::text("got it"),
        ])
        .with_max_context(32_768);
        let (iface0, registry0) = harness_with_state(&rt0);
        collect(
            crate::agent::streaming::process_stream_events_with_safeguards(
                iface0,
                state.clone(),
                registry0,
                "拉取大日志",
                StreamSafeguards::default(),
                None,
                None,
            )
            .await
            .expect("stream"),
        )
        .await;
    }
    let reference = {
        let keys = state.read().await.large_data_cache.entry_keys();
        assert_eq!(keys.len(), 1);
        format!("$cached:{}", keys[0])
    };

    let rt1 = MockLlmRuntime::new(vec![
        MockResponse::tool_call(
            "file_read",
            serde_json::json!({"path": reference, "char_offset": 0, "char_length": 8000}),
        ),
        MockResponse::tool_call(
            "file_read",
            serde_json::json!({"path": reference, "char_offset": 8000, "char_length": 8000}),
        ),
        MockResponse::tool_call(
            "file_read",
            serde_json::json!({"path": reference, "char_offset": 16000, "char_length": 8000}),
        ),
        MockResponse::text("读取完成"),
    ])
    .with_max_context(32_768);
    {
        let (iface1, registry1) = harness_with_state(&rt1);
        let events = collect(
            crate::agent::streaming::process_stream_events_with_safeguards(
                iface1,
                state.clone(),
                registry1,
                "分页读取缓存",
                StreamSafeguards::default(),
                None,
                None,
            )
            .await
            .expect("stream"),
        )
        .await;
        let reads = tool_end_results(&events, "file_read");
        assert_eq!(reads.len(), 3);
        assert!(reads.iter().all(|r| r.starts_with("true|")), "{reads:?}");
    }

    // Turn 2: a fresh user message in the SAME session. The request the
    // model backend receives is captured by the mock runtime.
    let rt2 = MockLlmRuntime::new(vec![MockResponse::text("好的")]).with_max_context(32_768);
    let (iface2, registry2) = harness_with_state(&rt2);
    collect(
        crate::agent::streaming::process_stream_events_with_safeguards(
            iface2,
            state.clone(),
            registry2,
            "继续核对",
            StreamSafeguards::default(),
            None,
            None,
        )
        .await
        .expect("stream"),
    )
    .await;

    let captured = rt2.captured_messages();
    assert!(!captured.is_empty(), "turn-2 request must be captured");
    let request_json = serde_json::to_string(&captured).unwrap();

    // (a) Window bodies are gone — deep payload blocks must not appear.
    let deep_marker: String = payload.chars().skip(12_000).take(40).collect();
    assert!(
        !request_json.contains(&deep_marker),
        "deep window body leaked into the next request"
    );
    // (b) The reference and metadata survive — re-reads stay possible.
    assert!(
        request_json.contains("$cached:"),
        "reference metadata must survive compaction"
    );
    // (c) The newest window's body is still INTACT (synthesis over the
    // page just read must remain possible), while stale ones are gone.
    let newest_marker: String = payload.chars().skip(20_000).take(40).collect();
    assert!(
        request_json.contains(&newest_marker),
        "newest window body must survive for synthesis"
    );
    // (d) The new user message is present and the request is bounded.
    assert!(request_json.contains("继续核对"));
    assert!(
        request_json.chars().count() < 30_000,
        "next-turn request must stay bounded: {} chars",
        request_json.chars().count()
    );
}

/// Cross-window REVISIT, data-delivery path only (review round 2026-11-10):
/// a scripted model reads window 1, window 2, then goes BACK to window 1.
/// This proves the DELIVERY semantics — re-reads succeed, the current
/// window stays intact in the request, stale windows stay addressable via
/// their structured reference, and the next user turn stays bounded. It
/// does NOT prove a real model will synthesize or avoid redundant reads;
/// that is what the small-scope real-model re-run (A2/A3/A5) is for.
#[tokio::test]
async fn cross_window_synthesis_with_revisit_stays_coherent() {
    let payload = big_payload();
    let state = Arc::new(tokio::sync::RwLock::new(
        crate::agent::types::AgentInternalState::new("synth-session".to_string()),
    ));

    // Produce the reference.
    let rt0 = MockLlmRuntime::new(vec![
        MockResponse::tool_call("big_output", serde_json::json!({})),
        MockResponse::text("got it"),
    ])
    .with_max_context(32_768);
    {
        let (iface, registry) = harness_with_state(&rt0);
        collect(
            crate::agent::streaming::process_stream_events_with_safeguards(
                iface,
                state.clone(),
                registry,
                "拉取大日志",
                StreamSafeguards::default(),
                None,
                None,
            )
            .await
            .expect("stream"),
        )
        .await;
    }
    let reference = {
        let keys = state.read().await.large_data_cache.entry_keys();
        format!("$cached:{}", keys[0])
    };

    // w1 → w2 → revisit w1 → final answer.
    let rt1 = MockLlmRuntime::new(vec![
        MockResponse::tool_call(
            "file_read",
            serde_json::json!({"path": reference, "char_offset": 0, "char_length": 8000}),
        ),
        MockResponse::tool_call(
            "file_read",
            serde_json::json!({"path": reference, "char_offset": 8000, "char_length": 8000}),
        ),
        MockResponse::tool_call(
            "file_read",
            serde_json::json!({"path": reference, "char_offset": 0, "char_length": 8000}),
        ),
        MockResponse::text("综合两窗完成"),
    ])
    .with_max_context(32_768);
    let events = {
        let (iface, registry) = harness_with_state(&rt1);
        let evs = collect(
            crate::agent::streaming::process_stream_events_with_safeguards(
                iface,
                state.clone(),
                registry,
                "跨窗统计",
                StreamSafeguards::default(),
                None,
                None,
            )
            .await
            .expect("stream"),
        )
        .await;
        evs
    };
    let reads = tool_end_results(&events, "file_read");
    assert_eq!(reads.len(), 3, "w1, w2, revisit-w1");
    assert!(
        reads.iter().all(|r| r.starts_with("true|")),
        "every window (including the revisit) must be servable: {reads:?}"
    );

    // At the FINAL round, the CURRENT window (revisited w1) is intact in the
    // request, w2 is a structured stale reference, w1-as-first-read too.
    let requests = rt1.captured_messages();
    let final_request = serde_json::to_string(&requests.last().unwrap()).unwrap();
    let w1_marker: String = payload.chars().skip(4_000).take(40).collect();
    assert!(
        final_request.contains(&w1_marker),
        "the revisited (current) window body must be in the final request"
    );
    assert!(
        final_request.contains("re-read $cached:"),
        "stale window 2 must remain as a structured re-readable reference"
    );

    // Next user turn: bounded, bodies gone (incl. the revisit), refs kept.
    let rt2 = MockLlmRuntime::new(vec![MockResponse::text("好的")]).with_max_context(32_768);
    {
        let (iface, registry) = harness_with_state(&rt2);
        collect(
            crate::agent::streaming::process_stream_events_with_safeguards(
                iface,
                state.clone(),
                registry,
                "下一题",
                StreamSafeguards::default(),
                None,
                None,
            )
            .await
            .expect("stream"),
        )
        .await;
    }
    let next_request = serde_json::to_string(&rt2.captured_messages()[0]).unwrap();
    let w2_marker: String = payload.chars().skip(12_000).take(40).collect();
    assert!(!next_request.contains(&w2_marker), "stale bodies are gone");
    // The REVISITED w1 was the newest window of the previous turn — the
    // retention contract keeps exactly that one intact; it is NOT expected
    // to disappear (that would resurrect the "re-read forever" loop).
    assert!(
        next_request.contains(&w1_marker),
        "current window is retained"
    );
    assert!(next_request.contains("re-read $cached:"), "refs survive");
    assert!(
        next_request.chars().count() < 30_000,
        "next-turn request bounded: {}",
        next_request.chars().count()
    );
}
