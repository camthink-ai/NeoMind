//! Early End (cancel before first assistant) must leave a persistable
//! terminal row with goalStatus/endReason in agent memory.
//!
//! Full save→reload coverage lives in `session::tests` (needs SessionManager
//! private save_history). Run:
//! `cargo test -p neomind-agent --features test-utils early_interrupt`

#![cfg(all(test, feature = "test-utils"))]

use std::sync::Arc;

use futures::StreamExt;
use neomind_core::llm::backend::LlmRuntime;

use crate::agent::task_continuity::StreamEndReason;
use crate::agent::types::{AgentEvent, AgentInternalState, AgentMessage};
use crate::testing_helpers::mock_llm::{MockLlmRuntime, MockResponse};
use crate::toolkit::registry::ToolRegistry;

async fn collect(
    stream: std::pin::Pin<Box<dyn futures::Stream<Item = AgentEvent> + Send>>,
) -> Vec<AgentEvent> {
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

/// Interrupt before first assistant output → memory gains a terminal row.
#[tokio::test]
async fn early_interrupt_before_assistant_stamps_terminal_in_memory() {
    let rt = MockLlmRuntime::new(vec![MockResponse::text(
        "this reply must never be persisted — cancel wins first",
    )]);
    let iface = crate::llm::LlmInterface::default();
    let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt.clone());
    futures::executor::block_on(iface.set_llm(rt_dyn));
    let iface = Arc::new(iface);

    let state = Arc::new(tokio::sync::RwLock::new(AgentInternalState::new(
        "early-end-persist".to_string(),
    )));
    state
        .write()
        .await
        .push_message(AgentMessage::user("please start the task"));

    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    cancel_tx.send(true).expect("pre-cancel");

    let stream = crate::agent::streaming::process_stream_events_with_safeguards(
        iface,
        state.clone(),
        Arc::new(ToolRegistry::new()),
        "please start the task",
        super::StreamSafeguards::default().with_interrupt_signal(cancel_rx),
        None,
        None,
        0,
    )
    .await
    .expect("stream starts");
    let events = collect(stream).await;

    let end = events
        .iter()
        .rev()
        .find(|e| matches!(e, AgentEvent::End { .. }))
        .expect("End event");
    match end {
        AgentEvent::End {
            end_reason: Some(StreamEndReason::Interrupted),
            goal_status: Some(_),
            ..
        } => {}
        other => panic!("expected Interrupted End with goal_status, got {other:?}"),
    }

    let memory = state.read().await.memory.clone();
    let last = memory
        .iter()
        .rev()
        .find(|m| m.role == "assistant")
        .expect("terminal assistant must exist after early End");
    assert_eq!(last.end_reason, Some(StreamEndReason::Interrupted));
    assert!(last.goal_status.is_some());
    assert!(
        last.content.contains("Interrupted"),
        "terminal content marks the early cancel: {:?}",
        last.content
    );
}
