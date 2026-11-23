//! Multimodal (text + images) streaming response processing.
//!
//! Differs from `stream_core` only in first-round user message construction
//! (image caching + `Content::Parts` / vision-companion delegation). The entire
//! multi-round ReAct loop, two-layer gating, `PreparedInvoke` host context,
//! single-copy skill tracking, `SkillExplorationBudget`, bounded NM-250
//! `foreshadow_continue`, `ToolCallEnd` (`effective_success`), and tool-free
//! empty-reply recovery are delegated to [`super::stream_core::drive_react_stream`].

use std::pin::Pin;
use std::sync::Arc;

use futures::Stream;

use super::stream_core::{
    drive_react_stream, prepare_turn_history_and_budget, InitialStreamMode, PreparedTurn,
    StreamSafeguards,
};
use crate::agent::types::{AgentEvent, AgentInternalState, AgentMessage, AgentMessageImage};
use crate::error::{NeoMindError, Result};
use crate::llm::LlmInterface;

/// Process multimodal message with configurable safeguards.
#[allow(clippy::too_many_arguments)]
pub async fn process_multimodal_stream_events_with_safeguards(
    llm_interface: Arc<LlmInterface>,
    internal_state: Arc<tokio::sync::RwLock<AgentInternalState>>,
    tools: Arc<crate::toolkit::ToolRegistry>,
    user_message: &str,
    images: Vec<String>,
    safeguards: StreamSafeguards,
    conversation_summary: Option<String>,
    summary_up_to_index: Option<u64>,
    // Host/UI AuthorizeNewOperation target traveling with this message.
    authorization: Option<crate::agent::task_continuity::AuthorizeNewOpSubmit>,
    // Content nodes / @-refs traveling with this message (no shared pending hop).
    content_nodes: Option<(Vec<serde_json::Value>, u32)>,
) -> Result<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> {
    use neomind_core::ContentPart;

    // New user turn: clear transient skills, then accept user (G2 directives)
    // BEFORE the first model request — same contract as the text stream path.
    super::super::task_continuity::begin_user_turn_clear_transient_skills(&llm_interface).await;
    let turn_context =
        super::context::TurnContext::new(conversation_summary.clone(), summary_up_to_index);
    let user_message = user_message.to_string();

    // Build multimodal message content with images
    let mut parts = vec![ContentPart::text(&user_message)];
    for image_data in &images {
        if let Some(parsed) = crate::image_utils::parse_image_data(image_data) {
            parts.push(ContentPart::image_base64(parsed.base64, parsed.mime_type));
        }
    }

    // Store uploaded images in large_data_cache so image-aware tools can resolve them.
    if !images.is_empty() {
        let mut state = internal_state.write().await;
        for (i, url) in images.iter().enumerate() {
            let cache_key = if i == 0 {
                "user_image".to_string()
            } else {
                format!("user_image_{}", i)
            };
            state.large_data_cache.store_user_image(&cache_key, url);
        }
    }
    // Persist eagerly (content-hash idempotent) so album / NM-231 strip see
    // durable `/api/images/…` handles on this turn; attach companion captions.
    let captions = llm_interface
        .take_image_delegation_captions()
        .await
        .unwrap_or_default();
    let images_dir = neomind_core::paths::data_dir().join("images");
    let user_images: Vec<AgentMessageImage> = images
        .iter()
        .enumerate()
        .map(|(i, data_url)| {
            let mime_type =
                crate::image_utils::parse_image_data(data_url).map(|p| p.mime_type.to_string());
            let data = if data_url.starts_with("/api/images/") {
                data_url.clone()
            } else {
                crate::session::SessionManager::persist_image_to_file(data_url, &images_dir)
                    .unwrap_or_else(|| data_url.clone())
            };
            AgentMessageImage {
                data,
                mime_type,
                caption: captions.get(i).cloned(),
            }
        })
        .collect();
    let call_site_turn = {
        let user_msg = AgentMessage::user_with_images(&user_message, user_images);
        let mut state = internal_state.write().await;
        state
            .accept_user_message(user_msg, authorization, content_nodes)
            .call_site_turn
    };
    {
        llm_interface.pin_turn_timestamp().await;
        let (section, new_op_tools) = {
            let state = internal_state.read().await;
            (
                state.task_state.to_prompt_section(),
                state.task_state.tools_needing_new_operation_protocol(),
            )
        };
        llm_interface.set_task_state_prompt(section).await;
        llm_interface
            .set_new_operation_protocol_tools(new_op_tools)
            .await;
    }

    let PreparedTurn {
        history_for_llm,
        effective_max,
        first_call_without_feedback,
    } = match prepare_turn_history_and_budget(
        &llm_interface,
        &internal_state,
        &user_message,
        conversation_summary.as_deref(),
        summary_up_to_index,
    )
    .await
    {
        Ok(ready) => ready,
        Err(overflow_stream) => return Ok(overflow_stream),
    };

    tracing::debug!(
        "Passing {} messages from history to LLM (multimodal)",
        history_for_llm.len()
    );

    // Create multimodal user message (or delegated plain-text prompt when using a vision companion).
    let multimodal_user_msg = neomind_core::Message::new(
        neomind_core::MessageRole::User,
        neomind_core::Content::Parts(parts),
    );
    let llm_user_msg = match llm_interface.take_image_delegation_prompt().await {
        Some(prompt) => neomind_core::Message::new(
            neomind_core::MessageRole::User,
            neomind_core::Content::Text(prompt),
        ),
        None => multimodal_user_msg,
    };

    let stream_result = llm_interface
        .chat_stream_multimodal_with_history(llm_user_msg, &history_for_llm)
        .await;
    let stream = stream_result.map_err(|e| NeoMindError::Llm(e.to_string()))?;

    let has_images = !images.is_empty();

    Ok(drive_react_stream(
        llm_interface,
        internal_state,
        tools,
        user_message,
        stream,
        safeguards,
        turn_context,
        effective_max,
        first_call_without_feedback,
        call_site_turn,
        InitialStreamMode::Multimodal { has_images },
    ))
}

#[cfg(all(test, feature = "test-utils"))]
mod drop_lease_tests {
    use super::*;
    use crate::agent::task_continuity::{
        evaluate_mutating_gate, MutatingGateOutcome, OperationStatus, NEW_OPERATION_ARG,
    };
    use crate::testing_helpers::mock_llm::{MockLlmRuntime, MockResponse};
    use crate::toolkit::{Tool, ToolError, ToolOutput, ToolRegistry};
    use futures::StreamExt;
    use neomind_core::llm::backend::LlmRuntime;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct WaitingTool {
        started: Arc<tokio::sync::Notify>,
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl Tool for WaitingTool {
        fn name(&self) -> &str {
            "run_now"
        }
        fn description(&self) -> &str {
            "waits forever after signaling start"
        }
        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({"type":"object","properties":{"agent_id":{"type":"string"}}})
        }
        async fn execute(
            &self,
            _: serde_json::Value,
        ) -> std::result::Result<ToolOutput, ToolError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.started.notify_one();
            std::future::pending::<std::result::Result<ToolOutput, ToolError>>().await
        }
    }

    /// Consumer abort (no interrupt watch) must still finalize Running → Unknown
    /// so an authorized `_new_operation` can Proceed.
    #[tokio::test]
    async fn consumer_abort_abandons_running_lease() {
        let args = serde_json::json!({"agent_id": "review-only"});
        let rt = MockLlmRuntime::new(vec![MockResponse::tool_call("run_now", args.clone())])
            .with_max_context(32768);
        let iface = Arc::new(crate::llm::LlmInterface::default());
        let rt_dyn: Arc<dyn LlmRuntime> = Arc::new(rt);
        iface.set_llm(rt_dyn).await;

        let started = Arc::new(tokio::sync::Notify::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(WaitingTool {
            started: started.clone(),
            calls: calls.clone(),
        }));

        let state = Arc::new(tokio::sync::RwLock::new(AgentInternalState::new(
            "drop-lease".into(),
        )));
        let safeguards = StreamSafeguards {
            heartbeat_interval: std::time::Duration::from_millis(10),
            ..StreamSafeguards::default()
        };

        let mut stream = process_multimodal_stream_events_with_safeguards(
            iface,
            state.clone(),
            Arc::new(registry),
            "run isolated review task",
            vec![],
            safeguards,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("stream");

        let consumer = tokio::spawn(async move { while stream.next().await.is_some() {} });
        tokio::time::timeout(std::time::Duration::from_secs(5), started.notified())
            .await
            .expect("tool must start");
        consumer.abort();
        let _ = consumer.await;

        // Give Drop guard a moment if it had to spawn (try_write missed).
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut st = state.write().await;
        let op = st.task_state.operations.last().expect("operation created");
        assert_eq!(
            op.status,
            OperationStatus::Unknown,
            "abort/drop must finalize Running"
        );
        assert!(op.owner_id.is_some(), "stream-owned op must carry owner_id");
        let fp = op.write_fingerprint.clone().expect("fp");
        let mut new_args = args;
        new_args
            .as_object_mut()
            .unwrap()
            .insert(NEW_OPERATION_ARG.into(), serde_json::json!(true));
        // Unknown: model boolean alone must Block.
        let call_site_turn = st.task_state.turn_count;
        match evaluate_mutating_gate(
            &mut st.task_state,
            "run_now",
            &new_args,
            &fp,
            None,
            call_site_turn,
        ) {
            MutatingGateOutcome::Block { .. } => {}
            other => panic!("model-alone after Unknown must Block, got {other:?}"),
        }
        let op = st
            .task_state
            .operations
            .last()
            .unwrap()
            .operation_id
            .clone();
        st.task_state.apply_directive(
            crate::agent::task_continuity::TaskDirective::AuthorizeNewOperation {
                prior_operation_id: Some(op),
                write_fingerprint: Some(fp.clone()),
            },
        );
        let owner2 = crate::agent::task_continuity::new_stream_owner();
        let call_site_turn = st.task_state.turn_count;
        match evaluate_mutating_gate(
            &mut st.task_state,
            "run_now",
            &new_args,
            &fp,
            Some(owner2.as_str()),
            call_site_turn,
        ) {
            MutatingGateOutcome::Proceed { .. } => {}
            other => panic!("user-authorized new-op after drop must Proceed, got {other:?}"),
        }
        assert!(calls.load(Ordering::SeqCst) >= 1);
    }

    /// Delayed Drop of stream A must not clear stream B's Running lease.
    #[tokio::test]
    async fn delayed_old_stream_drop_preserves_sibling_running() {
        use crate::agent::task_continuity::{new_stream_owner, StreamLeaseGuard};

        let state = Arc::new(tokio::sync::RwLock::new(AgentInternalState::new(
            "handover-mm".into(),
        )));
        let args_a = serde_json::json!({"agent_id": "stream-A"});
        let args_b = serde_json::json!({"agent_id": "stream-B"});

        // Stream A: acquire lease, complete tool, keep guard for delayed Drop.
        let owner_a = new_stream_owner();
        let guard_a = {
            let mut st = state.write().await;
            let fp = crate::agent::streaming::mutating_write_fingerprint("run_now", &args_a)
                .expect("fp A");
            let call_site_turn = st.task_state.turn_count;
            let MutatingGateOutcome::Proceed { operation_id } = evaluate_mutating_gate(
                &mut st.task_state,
                "run_now",
                &args_a,
                &fp,
                Some(owner_a.as_str()),
                call_site_turn,
            ) else {
                panic!("A Proceed");
            };
            st.task_state.complete_operation_succeeded(
                &operation_id,
                crate::agent::task_continuity::StructuredResult::with_body(true, "ok", "{}", None),
                None::<String>,
            );
            StreamLeaseGuard::new(state.clone(), owner_a, "delayed A")
        };

        // Stream B: Running on same session.
        let started = Arc::new(tokio::sync::Notify::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(WaitingTool {
            started: started.clone(),
            calls: calls.clone(),
        }));
        let rt = MockLlmRuntime::new(vec![MockResponse::tool_call("run_now", args_b.clone())])
            .with_max_context(32768);
        let iface = Arc::new(crate::llm::LlmInterface::default());
        iface.set_llm(Arc::new(rt) as Arc<dyn LlmRuntime>).await;
        let safeguards = StreamSafeguards {
            heartbeat_interval: std::time::Duration::from_millis(10),
            ..StreamSafeguards::default()
        };
        let mut stream_b = process_multimodal_stream_events_with_safeguards(
            iface,
            state.clone(),
            Arc::new(registry),
            "run B",
            vec![],
            safeguards,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("stream B");
        let consumer_b = tokio::spawn(async move { while stream_b.next().await.is_some() {} });
        tokio::time::timeout(std::time::Duration::from_secs(5), started.notified())
            .await
            .expect("B tool start");

        let op_b_id = {
            let st = state.read().await;
            let op = st
                .task_state
                .operations
                .iter()
                .rev()
                .find(|o| o.status == OperationStatus::Running)
                .expect("B Running");
            assert_eq!(op.status, OperationStatus::Running);
            op.operation_id.clone()
        };

        // Delayed Drop of A — must not touch B.
        drop(guard_a);
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;

        {
            let mut st = state.write().await;
            assert_eq!(
                st.task_state.operation(&op_b_id).unwrap().status,
                OperationStatus::Running,
                "B must remain Running after delayed A Drop"
            );
            let fp_b = st
                .task_state
                .operation(&op_b_id)
                .unwrap()
                .write_fingerprint
                .clone()
                .expect("fp B");
            let mut retry = args_b.clone();
            retry
                .as_object_mut()
                .unwrap()
                .insert(NEW_OPERATION_ARG.into(), serde_json::json!(true));
            let call_site_turn = st.task_state.turn_count;
            match evaluate_mutating_gate(
                &mut st.task_state,
                "run_now",
                &retry,
                &fp_b,
                None,
                call_site_turn,
            ) {
                MutatingGateOutcome::InProgress { operation_id } => {
                    assert_eq!(operation_id, op_b_id);
                }
                other => panic!("expected InProgress, got {other:?}"),
            }
        }

        consumer_b.abort();
        let _ = consumer_b.await;
        assert!(calls.load(Ordering::SeqCst) >= 1);
    }
}
