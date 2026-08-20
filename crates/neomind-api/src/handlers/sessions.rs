//! Session management handlers.

use super::ws::create_connection_metadata;

use axum::extract::ws::{Message as AxumMessage, WebSocket, WebSocketUpgrade};
use axum::{
    extract::{Path, Query, State},
    response::Json,
};
use futures::stream::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::info;

use neomind_agent::AgentEvent;
use neomind_storage::{PendingStreamState, StreamStage};

/// Stream event sent from the LLM processing task to the WebSocket handler.
#[derive(Debug, Clone)]
struct StreamEvent {
    json: String,
}

/// Process the LLM stream in a spawned task and send events through a channel.
///
/// This function runs asynchronously and doesn't block the WebSocket event loop,
/// allowing ping/pong frames to be handled properly.
async fn process_stream_to_channel(
    mut stream: Pin<Box<dyn Stream<Item = AgentEvent> + Send>>,
    session_id: String,
    user_message: String,
    tx: mpsc::Sender<StreamEvent>,
    state: super::ServerState,
) {
    let mut end_event_sent = false;
    let mut event_count = 0u32;

    // Debounce pending-stream DB writes: only persist every N events or after T elapsed.
    // Avoids a blocking redb write transaction per Thinking/Content chunk (thinking models
    // can emit 100+ chunks per request).
    let mut pending_save_counter: u32 = 0;
    let mut last_pending_save = std::time::Instant::now();
    const PENDING_SAVE_EVENT_INTERVAL: u32 = 20;
    const PENDING_SAVE_TIME_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

    // P0.3: Create pending stream state for recovery
    let session_store = state.agents.session_manager.session_store();
    let mut pending_state = PendingStreamState::new(session_id.clone(), user_message);
    if let Err(e) = session_store.save_pending_stream(&pending_state) {
        tracing::warn!(error = %e, "Failed to save initial pending stream state");
    }

    // Track stream start time for progress reporting
    let stream_start = std::time::Instant::now();

    // Stream timeout: 1200 seconds (20 minutes) to support thinking models
    // This is synchronized with StreamConfig::max_stream_duration_secs
    // qwen3-vl:2b with extended thinking can take significant time for complex queries
    // with image analysis or multi-step reasoning.
    let stream_timeout = Duration::from_secs(1200);
    let max_duration_secs = 1200u64;

    loop {
        let next_event = tokio::time::timeout(stream_timeout, StreamExt::next(&mut stream)).await;

        match next_event {
            Ok(Some(event)) => {
                event_count += 1;
                let event_json = match &event {
                    AgentEvent::Thinking { content } => {
                        // P0.3: Update pending state with thinking content
                        pending_state.update_thinking(content);
                        pending_save_counter += 1;
                        if pending_save_counter >= PENDING_SAVE_EVENT_INTERVAL
                            || last_pending_save.elapsed() >= PENDING_SAVE_TIME_INTERVAL
                        {
                            if let Err(e) = session_store.save_pending_stream(&pending_state) {
                                tracing::warn!(error = %e, "Failed to update pending stream (thinking)");
                            }
                            pending_save_counter = 0;
                            last_pending_save = std::time::Instant::now();
                        }

                        tracing::debug!(
                            "Sending Thinking event: {} chars",
                            content.chars().count()
                        );
                        json!({
                            "type": "Thinking",
                            "content": content,
                            "sessionId": session_id,
                        })
                    }
                    AgentEvent::Content { content } => {
                        // P0.3: Update pending state with content
                        pending_state.update_content(content);
                        pending_state.set_stage(StreamStage::Generating);
                        pending_save_counter += 1;
                        if pending_save_counter >= PENDING_SAVE_EVENT_INTERVAL
                            || last_pending_save.elapsed() >= PENDING_SAVE_TIME_INTERVAL
                        {
                            if let Err(e) = session_store.save_pending_stream(&pending_state) {
                                tracing::warn!(error = %e, "Failed to update pending stream (content)");
                            }
                            pending_save_counter = 0;
                            last_pending_save = std::time::Instant::now();
                        }

                        json!({
                            "type": "Content",
                            "content": content,
                            "sessionId": session_id,
                        })
                    }
                    AgentEvent::ToolCallStart {
                        tool,
                        arguments,
                        round,
                    } => {
                        let mut json = json!({
                            "type": "ToolCallStart",
                            "tool": tool,
                            "arguments": arguments,
                            "sessionId": session_id,
                        });
                        if let Some(r) = round {
                            json["round"] = json!(r);
                        }
                        json
                    }
                    AgentEvent::ToolCallEnd {
                        tool,
                        result,
                        success,
                        round,
                    } => {
                        let mut json = json!({
                            "type": "ToolCallEnd",
                            "tool": tool,
                            "result": result,
                            "success": success,
                            "sessionId": session_id,
                        });
                        if let Some(r) = round {
                            json["round"] = json!(r);
                        }
                        json
                    }
                    AgentEvent::Error { message } => {
                        json!({
                            "type": "Error",
                            "message": message,
                            "sessionId": session_id,
                        })
                    }
                    AgentEvent::Intent {
                        category,
                        display_name,
                        confidence,
                        keywords,
                    } => {
                        json!({
                            "type": "Intent",
                            "category": category,
                            "displayName": display_name,
                            "confidence": confidence,
                            "keywords": keywords,
                            "sessionId": session_id,
                        })
                    }
                    AgentEvent::Plan { step, stage } => {
                        json!({
                            "type": "Plan",
                            "step": step,
                            "stage": stage,
                            "sessionId": session_id,
                        })
                    }
                    AgentEvent::IntermediateEnd => {
                        // Intermediate end - for multi-round tool calling
                        // This indicates the current round is complete but more processing is coming
                        // Send this to frontend but don't exit the loop
                        tracing::debug!(
                            "*** Sending IntermediateEnd event (round {}) ***",
                            event_count
                        );
                        json!({
                            "type": "intermediate_end",
                            "sessionId": session_id,
                        })
                    }
                    AgentEvent::End { prompt_tokens } => {
                        // P0.3: Delete pending state on successful completion
                        let _ = session_store.delete_pending_stream(&session_id);

                        // === Context Summarization ===
                        // If context usage exceeds 60%, trigger background summarization
                        if let Some(pt) = prompt_tokens {
                            let pt_val = *pt; // Copy to owned value for 'static spawn
                            let sum_session_id = session_id.clone();
                            let sum_state = state.clone();
                            tokio::spawn(async move {
                                if let Err(e) =
                                    crate::handlers::summarization::trigger_summarization(
                                        &sum_session_id,
                                        &sum_state,
                                        pt_val,
                                    )
                                    .await
                                {
                                    tracing::warn!("Background summarization failed: {}", e);
                                }
                            });
                        }

                        tracing::info!("*** Sending End event (total events: {}) ***", event_count);
                        end_event_sent = true;
                        let mut end_json = json!({
                            "type": "end",
                            "sessionId": session_id,
                        });
                        if let Some(pt) = prompt_tokens {
                            end_json["tokenUsage"] = json!({
                                "promptTokens": pt
                            });
                        }
                        end_json
                    }
                    AgentEvent::Progress {
                        message,
                        stage,
                        elapsed_ms,
                        ..
                    } => {
                        let elapsed = elapsed_ms
                            .unwrap_or_else(|| stream_start.elapsed().as_millis() as u64)
                            / 1000;
                        let remaining = max_duration_secs.saturating_sub(elapsed);
                        tracing::debug!(
                            "Sending Progress event: {} ({})",
                            message,
                            stage.as_deref().unwrap_or("unknown")
                        );
                        json!({
                            "type": "Progress",
                            "message": message,
                            "stage": stage,
                            "elapsed": elapsed,
                            "remainingTime": remaining,
                            "sessionId": session_id,
                        })
                    }
                    AgentEvent::Heartbeat { timestamp } => {
                        json!({
                            "type": "Heartbeat",
                            "timestamp": timestamp,
                            "sessionId": session_id,
                        })
                    }
                    AgentEvent::Warning { message } => {
                        let elapsed = stream_start.elapsed().as_secs();
                        let remaining = max_duration_secs.saturating_sub(elapsed);
                        tracing::debug!("Sending Warning event: {}", message);
                        json!({
                            "type": "Warning",
                            "message": message,
                            "elapsed": elapsed,
                            "remainingTime": remaining,
                            "sessionId": session_id,
                        })
                    }
                };

                let stream_event = StreamEvent {
                    json: event_json.to_string(),
                };

                // Try to send, but don't block if channel is closed
                if tx.send(stream_event).await.is_err() {
                    tracing::warn!("Failed to send stream event through channel");
                    break;
                }

                // If this was the End event, exit the loop
                // (IntermediateEnd does NOT exit the loop - more events are coming)
                if matches!(event, AgentEvent::End { .. }) {
                    break;
                }
            }
            Ok(None) => {
                // Stream ended naturally
                tracing::debug!("Stream ended naturally (total events: {})", event_count);
                // Send End event if not already sent
                if !end_event_sent {
                    let end_json = json!({
                        "type": "end",
                        "sessionId": session_id,
                    });
                    let _ = tx
                        .send(StreamEvent {
                            json: end_json.to_string(),
                        })
                        .await;
                }
                break;
            }
            Err(_) => {
                // Timeout occurred - CRITICAL: clean up pending state to prevent memory leak
                tracing::warn!(
                    "Stream timeout after {:?} - cleaning up pending state",
                    stream_timeout
                );
                let _ = session_store.delete_pending_stream(&session_id);

                let timeout_json = json!({
                    "type": "Error",
                    "message": "Stream timeout: response took too long",
                    "sessionId": session_id,
                });
                let _ = tx
                    .send(StreamEvent {
                        json: timeout_json.to_string(),
                    })
                    .await;
                // Send end event after timeout
                if !end_event_sent {
                    let end_json = json!({
                        "type": "end",
                        "sessionId": session_id,
                    });
                    let _ = tx
                        .send(StreamEvent {
                            json: end_json.to_string(),
                        })
                        .await;
                }
                break;
            }
        }
    }

    // Persist history after stream completes
    if let Err(e) = state
        .agents
        .session_manager
        .persist_history(&session_id)
        .await
    {
        tracing::warn!(category = "session", error = %e, "Failed to persist history");
    }
}
use crate::models::{
    common::ApiResponse, pagination::Pagination, ChatRequest, ChatResponse, CreateSessionRequest,
    ErrorResponse,
};

use super::ServerState;

/// Heartbeat interval for WebSocket connections (seconds)
const HEARTBEAT_INTERVAL_SECS: u64 = 30;

/// Session list item.
#[derive(Debug, Clone, Serialize)]
pub struct SessionListItem {
    pub id: String,
    pub message_count: usize,
    pub created_at: String,
}

/// Create a new session.
///
/// Body is optional for backward compatibility — older callers POST with no
/// body and get the default-config session. Newer callers may send
/// `{"config": { ...AgentConfig }}` (legacy field, kept for compat) or the
/// more granular patch form understood by `CreateSessionRequest`/`ChatRequest`
/// to override per-session fields like `system_prompt`.
pub async fn create_session_handler(
    State(state): State<ServerState>,
    body: Option<Json<Option<CreateSessionRequest>>>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ErrorResponse> {
    // Unwrap the doubly-optional body: outer Option = "was there a body at
    // all", inner Option = "did the body parse to a real struct". Either
    // layer being None means "no override requested" → fall back to default.
    let req = body.and_then(|Json(b)| b);

    let (session_patch, legacy_cfg) = match req {
        Some(r) => (r.session_config, r.config),
        None => (None, None),
    };

    let session_id = match session_patch {
        Some(patch) => {
            // Preferred path: `sessionConfig` patch (systemPromptSuffix /
            // allowedTools / …) merged on top of the platform default.
            state
                .agents
                .session_manager
                .create_session_with_options(patch.into())
                .await
                .map_err(|e| ErrorResponse::with_message(e.to_string()))?
        }
        None => match legacy_cfg {
            Some(cfg) => {
                // Legacy `config: AgentConfig` path — full struct. Translate to
                // options patch (only the four commonly-overridden fields flow
                // through; the rest of AgentConfig stays at platform default).
                let opts = neomind_agent::CreateSessionOptions {
                    system_prompt: Some(cfg.system_prompt),
                    temperature: Some(cfg.temperature),
                    model: Some(cfg.model),
                    enable_tools: Some(cfg.enable_tools),
                    ..Default::default()
                };
                state
                    .agents
                    .session_manager
                    .create_session_with_options(opts)
                    .await
                    .map_err(|e| ErrorResponse::with_message(e.to_string()))?
            }
            None => state
                .agents
                .session_manager
                .create_session()
                .await
                .map_err(|e| ErrorResponse::with_message(e.to_string()))?,
        },
    };

    Ok(Json(ApiResponse::success(json!({
        "sessionId": session_id,
    }))))
}

/// Query parameters for listing sessions with pagination.
#[derive(Debug, Deserialize)]
pub struct ListSessionsQuery {
    /// Page number (1-indexed)
    #[serde(default = "default_page")]
    pub page: u32,
    /// Page size
    #[serde(default = "default_page_size")]
    pub page_size: u32,
}

fn default_page() -> u32 {
    1
}
fn default_page_size() -> u32 {
    20
}

/// List all sessions with pagination.
///
/// Performance optimization: Uses lightweight session info (without message count/preview)
/// to avoid N+1 database queries. For detailed session info, use individual session endpoints.
pub async fn list_sessions_handler(
    State(state): State<ServerState>,
    Query(query): Query<ListSessionsQuery>,
) -> Result<Json<ApiResponse<Vec<serde_json::Value>>>, ErrorResponse> {
    let pagination = Pagination {
        page: query.page.max(1),
        page_size: query.page_size.clamp(1, 100),
    };

    // Use lightweight version to avoid loading message count/preview for every session
    let all_sessions = state
        .agents
        .session_manager
        .list_sessions_with_info_light()
        .await;
    let total_count = all_sessions.len() as u32;

    // Calculate pagination
    let offset = pagination.offset();
    let limit = pagination.limit();
    let paginated_sessions: Vec<serde_json::Value> = all_sessions
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .map(|s| json!(s))
        .collect();

    let meta = pagination.meta(total_count);

    Ok(Json(ApiResponse::paginated(paginated_sessions, meta)))
}

/// Get session info.
pub async fn get_session_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ErrorResponse> {
    let agent = state
        .agents
        .session_manager
        .get_session(&id)
        .await
        .map_err(|_| ErrorResponse::not_found("Session"))?;

    Ok(Json(ApiResponse::success(json!({
        "sessionId": id,
        "state": agent.state().await,
    }))))
}

/// Get session history.
pub async fn get_session_history_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ErrorResponse> {
    // First check if session exists (get_history returns empty for NotFound)
    let _agent = state
        .agents
        .session_manager
        .get_session(&id)
        .await
        .map_err(|_| ErrorResponse::not_found("Session"))?;

    let history = state
        .agents
        .session_manager
        .get_history(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get history: {}", e)))?;

    Ok(Json(ApiResponse::success(json!({
        "messages": history,
        "count": history.len(),
    }))))
}

/// Delete a session.
pub async fn delete_session_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ErrorResponse> {
    state
        .agents
        .session_manager
        .remove_session(&id)
        .await
        .map_err(|e| {
            // Check if it's a NotFound error
            if format!("{}", e).contains("Session:") || format!("{}", e).contains("not found") {
                ErrorResponse::not_found("Session")
            } else {
                // Other error - return the actual message
                ErrorResponse::with_message(e.to_string())
            }
        })?;

    Ok(Json(ApiResponse::success(json!({
        "deleted": true,
        "sessionId": id,
    }))))
}

/// P0.3: Get pending stream state for a session (for recovery after disconnection).
pub async fn get_pending_stream_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ErrorResponse> {
    let session_store = state.agents.session_manager.session_store();

    match session_store.get_pending_stream(&id) {
        Ok(Some(pending)) => Ok(Json(ApiResponse::success(json!({
            "hasPending": true,
            "sessionId": id,
            "userMessage": pending.user_message,
            "content": pending.content,
            "thinking": pending.thinking,
            "stage": pending.stage,
            "elapsed": pending.elapsed_secs(),
            "startedAt": pending.started_at,
        })))),
        Ok(None) => Ok(Json(ApiResponse::success(json!({
            "hasPending": false,
            "sessionId": id,
        })))),
        Err(e) => Err(ErrorResponse::with_message(format!(
            "Failed to check pending stream: {}",
            e
        ))),
    }
}

/// P0.3: Clear pending stream state for a session (user chose to discard).
pub async fn clear_pending_stream_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ErrorResponse> {
    let session_store = state.agents.session_manager.session_store();

    session_store.delete_pending_stream(&id).map_err(|e| {
        ErrorResponse::with_message(format!("Failed to clear pending stream: {}", e))
    })?;

    Ok(Json(ApiResponse::success(json!({
        "cleared": true,
        "sessionId": id,
    }))))
}

/// Request body for updating session.
#[derive(Debug, Deserialize)]
pub struct UpdateSessionRequest {
    /// Session title (optional)
    pub title: Option<String>,
}

/// Update a session (e.g., rename).
pub async fn update_session_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateSessionRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ErrorResponse> {
    state
        .agents
        .session_manager
        .update_session_title(&id, req.title)
        .await
        .map_err(|e| ErrorResponse::with_message(e.to_string()))?;

    Ok(Json(ApiResponse::success(json!({
        "sessionId": id,
        "updated": true,
    }))))
}

/// Request body for toggling memory.
#[derive(Debug, Deserialize)]
pub struct ToggleMemoryRequest {
    /// Whether memory should be enabled
    pub enabled: bool,
}

/// Toggle memory enabled state for a session.
pub async fn toggle_memory_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<ToggleMemoryRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ErrorResponse> {
    state
        .agents
        .session_manager
        .toggle_memory(&id, req.enabled)
        .await
        .map_err(|e| ErrorResponse::with_message(e.to_string()))?;

    Ok(Json(ApiResponse::success(json!({
        "sessionId": id,
        "memoryEnabled": req.enabled,
    }))))
}

/// Clean up invalid sessions (dirty data).
/// Removes sessions that appear in the list but don't have valid data.
pub async fn cleanup_sessions_handler(
    State(state): State<ServerState>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ErrorResponse> {
    let cleaned_count = state
        .agents
        .session_manager
        .cleanup_invalid_sessions()
        .await;

    Ok(Json(ApiResponse::success(json!({
        "cleaned": cleaned_count,
        "message": format!("Cleaned up {} invalid session(s)", cleaned_count),
    }))))
}

/// Chat handler (REST).
pub async fn chat_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, ErrorResponse> {
    use futures::StreamExt;
    use tokio::time::{timeout, Duration};

    tracing::info!(
        session_id = %id,
        message_len = req.message.chars().count(),
        backend_id = ?req.backend_id,
        pinned_skills = req.selected_skills.len(),
        "chat_handler: HTTP chat via multi-round event stream"
    );

    // Multi-round ReAct over HTTP — same event stream the WS path uses
    // (process_stream_events_with_safeguards, MAX_TOOL_ITERATIONS=30). Previously
    // chat_handler called process_message (single LLM call, tool results never
    // fed back), so any HTTP client silently degraded to a single-round agent.
    // Using the _with_backend_and_skills variant also honors ChatRequest.backend_id
    // + selected_skills, which the old handler dropped on the floor.
    // Prepend page context if provided (mirrors the WS path; the old single-round
    // HTTP handler dropped this).
    let final_message = if let Some(ref ctx) = req.page_context {
        if !ctx.is_empty() {
            format!("{}\n\n{}", ctx, req.message)
        } else {
            req.message.clone()
        }
    } else {
        req.message.clone()
    };

    // Branch on multimodal input for REST parity with WS. The old HTTP handler
    // dropped req.images entirely, so a REST vision client silently degraded to
    // text-only analysis.
    let has_images = req.images.as_ref().is_some_and(|i| !i.is_empty());
    let stream_result = if has_images {
        // The multimodal stream variant has no selected_skills param; set pinned
        // skills on the agent directly first (same as the WS multimodal path).
        if !req.selected_skills.is_empty() {
            if let Ok(agent) = state.agents.session_manager.get_session(&id).await {
                agent.set_pinned_skills(req.selected_skills.clone()).await;
            }
        }
        let images: Vec<String> = req
            .images
            .as_ref()
            .unwrap()
            .iter()
            .map(|img| img.data.clone())
            .collect();
        state
            .agents
            .session_manager
            .process_message_multimodal_with_backend_stream(
                &id,
                &final_message,
                images,
                req.backend_id.as_deref(),
            )
            .await
    } else {
        state
            .agents
            .session_manager
            .process_message_events_with_backend_and_skills(
                &id,
                &final_message,
                req.backend_id.as_deref(),
                &req.selected_skills,
            )
            .await
    };

    let mut stream = match stream_result {
        Ok(s) => s,
        Err(e) => {
            let err_msg = e.to_string();
            if err_msg.contains("Not found") || err_msg.contains("Session:") {
                return Err(ErrorResponse::not_found("Session"));
            }
            return Err(ErrorResponse::with_message(err_msg));
        }
    };

    // Multi-round ReAct with a thinking model can exceed the old 120s cap (eval
    // observed 130s+). Default 300s matches the global agent execution budget;
    // override via HTTP_CHAT_TIMEOUT_SECS.
    let timeout_secs = std::env::var("HTTP_CHAT_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300u64);

    let started = std::time::Instant::now();
    let mut response = String::new();
    let mut thinking: Option<String> = None;
    let mut tools_used: Vec<String> = Vec::new();
    let mut tools_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut error_msg: Option<String> = None;

    let timed_out = timeout(Duration::from_secs(timeout_secs), async {
        while let Some(event) = stream.next().await {
            match event {
                AgentEvent::Content { content } => response.push_str(&content),
                AgentEvent::Thinking { content } => {
                    thinking.get_or_insert_with(String::new).push_str(&content);
                }
                AgentEvent::ToolCallStart { tool, .. } => {
                    // Dedup by tool name, preserve first-seen order.
                    if tools_seen.insert(tool.clone()) {
                        tools_used.push(tool);
                    }
                }
                AgentEvent::Error { message } => {
                    // Do NOT swallow — surface to the caller (see WS-B#1: the
                    // stream must emit real errors, not silently keyword-fallback).
                    error_msg = Some(message);
                }
                AgentEvent::End { .. } => break,
                _ => {}
            }
        }
    })
    .await
    .is_err();

    let processing_time_ms = started.elapsed().as_millis() as u64;

    // Surface error / timeout inline rather than returning an empty/blank response,
    // so the caller can distinguish a model limitation from a system failure.
    if let Some(msg) = &error_msg {
        if !response.is_empty() {
            response.push_str("\n\n");
        }
        response.push_str("[error: ");
        response.push_str(msg);
        response.push(']');
    }
    if timed_out {
        if !response.is_empty() {
            response.push_str("\n\n");
        }
        response.push_str(&format!(
            "[timed out after {}s — showing partial result]",
            timeout_secs
        ));
    }

    tracing::info!(
        session_id = %id,
        tools = ?tools_used,
        elapsed_ms = processing_time_ms,
        timed_out,
        errored = error_msg.is_some(),
        "chat_handler: completed"
    );

    // Persist the conversation turn to disk. The streaming path updates the
    // agent's live history (agent.history()) but doesn't save it; persist_history
    // reads that live history and saves it (same thing WS does on disconnect).
    // Without this, HTTP chat turns vanish on server restart.
    if let Err(e) = state.agents.session_manager.persist_history(&id).await {
        tracing::warn!(session_id = %id, error = %e, "chat_handler: failed to persist history");
    }

    Ok(Json(ChatResponse {
        response,
        session_id: id,
        tools_used,
        processing_time_ms,
        thinking,
    }))
}

/// WebSocket chat handler.
///
/// Supports two authentication methods:
/// - JWT token via `?token=xxx` parameter (local instance)
/// - API key via `?api_key=xxx` parameter (remote instance)
pub async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(state): State<ServerState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    // Try JWT token authentication first
    let session_info = match params.get("token") {
        Some(token) => match state.auth.user_state.validate_token(token) {
            Ok(info) => {
                info!(
                    username = %info.username,
                    role = info.role.as_str(),
                    "WebSocket authenticated via JWT"
                );
                Some(info)
            }
            Err(e) => {
                tracing::warn!(error = %e, "JWT validation failed, rejecting WebSocket connection");
                return ws.on_upgrade(|mut socket| async move {
                    let _ = socket
                        .send(AxumMessage::Text(
                            json!({"type": "Error", "message": "Invalid or expired token"})
                                .to_string(),
                        ))
                        .await;
                    let _ = socket.close().await;
                });
            }
        },
        None => None,
    };

    // If no valid JWT, try API key authentication
    if session_info.is_none() {
        if let Some(api_key) = params.get("api_key") {
            if state.auth.api_key_state.validate_key(api_key) {
                info!("WebSocket authenticated via API key");
                // No user session for API key auth — proceed without session_info
            } else {
                tracing::warn!("Invalid API key, rejecting WebSocket connection");
                return ws.on_upgrade(|mut socket| async move {
                    let _ = socket
                        .send(AxumMessage::Text(
                            json!({"type": "Error", "message": "Invalid API key"}).to_string(),
                        ))
                        .await;
                    let _ = socket
                        .send(AxumMessage::Close(Some(axum::extract::ws::CloseFrame {
                            code: axum::extract::ws::CloseCode::from(4001u16),
                            reason: "Invalid API key".into(),
                        })))
                        .await;
                });
            }
        } else {
            tracing::warn!("No authentication provided, rejecting WebSocket connection");
            return ws.on_upgrade(|mut socket| {
                async move {
                    let _ = socket.send(AxumMessage::Text(
                        json!({"type": "Error", "message": "Authentication required. Provide ?token=xxx or ?api_key=xxx."}).to_string()
                    )).await;
                    let _ = socket
                        .send(AxumMessage::Close(Some(axum::extract::ws::CloseFrame {
                            code: axum::extract::ws::CloseCode::from(4001u16),
                            reason: "Authentication required".into(),
                        })))
                        .await;
                }
            });
        }
    }

    let session_id = params.get("sessionId").cloned();
    ws.on_upgrade(|socket| handle_ws_socket(socket, state, session_id, session_info))
}

/// Send session history to the client.
///
/// This is called when a session is restored or switched to send the conversation
/// history to the frontend for display.
async fn send_session_history(
    socket: &mut WebSocket,
    session_id: &str,
    state: &ServerState,
) -> Result<(), Box<dyn std::error::Error>> {
    match state.agents.session_manager.get_history(session_id).await {
        Ok(messages) => {
            if !messages.is_empty() {
                tracing::info!(
                    session_id = %session_id,
                    count = messages.len(),
                    "Sending session history to client"
                );

                // Send each message as a separate event
                for msg in &messages {
                    let mut history_json = json!({
                        "type": "history",
                        "sessionId": session_id,
                        "role": msg.role,
                        "content": msg.content,
                        "thinking": msg.thinking,
                        "toolCalls": msg.tool_calls,
                        "timestamp": msg.timestamp,
                    });
                    // Include round_contents if present (for multi-step tool call display)
                    if let Some(ref rc) = msg.round_contents {
                        if let Some(obj) = history_json.as_object_mut() {
                            obj.insert("round_contents".to_string(), rc.clone());
                        }
                    }
                    // Include round_thinking if present (for per-round thinking display)
                    if let Some(ref rt) = msg.round_thinking {
                        if let Some(obj) = history_json.as_object_mut() {
                            obj.insert("round_thinking".to_string(), rt.clone());
                        }
                    }
                    let history_msg = history_json.to_string();

                    if socket.send(AxumMessage::Text(history_msg)).await.is_err() {
                        return Err("Failed to send history message".into());
                    }
                }

                // Send history complete marker
                let complete_msg = json!({
                    "type": "history_complete",
                    "sessionId": session_id,
                    "count": messages.len(),
                })
                .to_string();

                socket.send(AxumMessage::Text(complete_msg)).await?;
            }
            Ok(())
        }
        Err(e) => {
            tracing::warn!(
                session_id = %session_id,
                error = %e,
                "Failed to get session history"
            );
            Err(e.into())
        }
    }
}

/// Handle WebSocket connection.
async fn handle_ws_socket(
    mut socket: WebSocket,
    state: ServerState,
    session_id: Option<String>,
    _session_info: Option<crate::auth_users::SessionInfo>,
) {
    // Create connection metadata for tracking state and heartbeat
    let conn_meta = create_connection_metadata();
    conn_meta
        .set_state(super::ws::ConnectionState::Authenticated)
        .await;

    // Track the current session for this connection
    let current_session_id = Arc::new(tokio::sync::RwLock::new(session_id.clone()));

    // Subscribe to device status updates
    let mut device_update_rx = state.devices.update_tx.subscribe();

    // Heartbeat interval
    let mut heartbeat_interval =
        tokio::time::interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));

    // Heartbeat timeout - 60 seconds without pong = disconnect
    let heartbeat_timeout = Duration::from_secs(60);

    // Channel for receiving LLM stream events from spawned tasks
    // This keeps the main event loop responsive to WebSocket pings
    let (stream_tx, mut stream_rx) = mpsc::channel::<StreamEvent>(100);

    // Send welcome message
    let welcome = json!({
        "type": "system",
        "content": "Connected to Edge AI Agent",
        "sessionId": session_id,
    })
    .to_string();

    if socket.send(AxumMessage::Text(welcome)).await.is_err() {
        conn_meta.mark_closed().await;
        return;
    }

    // Send session history if reconnecting with an existing session
    if let Some(ref sid) = session_id {
        if !sid.is_empty() {
            let _ = send_session_history(&mut socket, sid, &state).await;
        }
    }

    // Main event loop - handle client messages, device updates, and heartbeat
    loop {
        tokio::select! {
            // Handle incoming client messages
            msg_result = socket.next() => {
                match msg_result {
                    Some(Ok(msg)) => {
                        match msg {
                            AxumMessage::Text(text) => {
                                // Track message received
                                conn_meta.increment_received();

                                // Check for pong response to our heartbeat ping
                                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                                    if value.get("type") == Some(&json!("pong")) {
                                        conn_meta.record_pong().await;
                                        tracing::debug!("Received pong from client");
                                        continue;
                                    }
                                }

                                if let Ok(chat_req) = serde_json::from_str::<ChatRequest>(&text) {
                                    // Check for cancel request
                                    if chat_req.message == "__CANCEL__" {
                                        tracing::info!("Received cancel request from client");

                                        // Determine which session to cancel
                                        let cancel_session_id = if let Some(ref sid) = chat_req.session_id {
                                            sid.clone()
                                        } else {
                                            let guard = current_session_id.read().await;
                                            guard.clone().unwrap_or_default()
                                        };

                                        // Send interrupt signal to the active stream
                                        if !cancel_session_id.is_empty() {
                                            state.agents.session_manager.cancel_session(&cancel_session_id).await;
                                        }

                                        // Send acknowledgment
                                        let cancel_msg = json!({
                                            "type": "cancelled",
                                            "message": "Request cancelled by user"
                                        }).to_string();
                                        if socket.send(AxumMessage::Text(cancel_msg)).await.is_err() {
                                            return;
                                        }
                                        continue;
                                    }

                                    // Use the sessionId from the request if provided, otherwise use current
                                    let requested_session_id = chat_req.session_id;

                                    // Session resolution helper - minimizes lock time
                                    let session_id = {
                                        // Clone current ID and drop read lock immediately
                                        let current_guard = current_session_id.read().await;
                                        let empty = String::new();
                                        let current_id = current_guard.as_ref().unwrap_or(&empty).clone();
                                        let has_valid_session = !current_id.is_empty();
                                        drop(current_guard);

                                        // Check if we need to switch sessions (async ops outside lock)
                                        let needs_switch = if let Some(ref req_id) = requested_session_id {
                                            req_id != &current_id && state.agents.session_manager.get_session(req_id).await.is_ok()
                                        } else {
                                            false
                                        };

                                        if needs_switch {
                                            // Switch to requested session
                                            let req_id = requested_session_id.as_ref().expect("requested_session_id validated in needs_switch check");
                                            {
                                                let mut write_guard = current_session_id.write().await;
                                                *write_guard = Some(req_id.clone());
                                            }

                                            // Notify client of session switch (outside lock)
                                            let msg = json!({
                                                "type": "session_switched",
                                                "sessionId": req_id,
                                            }).to_string();
                                            if socket.send(AxumMessage::Text(msg)).await.is_err() {
                                                return;
                                            }

                                            // Send session history after switching
                                            let _ = send_session_history(&mut socket, req_id, &state).await;

                                            req_id.clone()
                                        } else if !has_valid_session {
                                            // Create new session. Honor `sessionConfig`
                                            // from the request only at this creation
                                            // moment — subsequent frames targeting an
                                            // existing session ignore the field (an
                                            // explicit safety property, see
                                            // `get_or_create_session_with_options`).
                                            let new_id = if let Some(patch) = chat_req.session_config.clone() {
                                                state.agents.session_manager
                                                    .create_session_with_options(patch.into())
                                                    .await
                                                    .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string())
                                            } else {
                                                state.agents.session_manager.create_session().await
                                                    .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string())
                                            };

                                            {
                                                let mut write_guard = current_session_id.write().await;
                                                *write_guard = Some(new_id.clone());
                                            }

                                            // Notify client of the new session (outside lock)
                                            let msg = json!({
                                                "type": "session_created",
                                                "sessionId": new_id,
                                            }).to_string();
                                            if socket.send(AxumMessage::Text(msg)).await.is_err() {
                                                return;
                                            }
                                            new_id
                                        } else {
                                            // Use current session - no need to resend history
                                            current_id
                                        }
                                    };

                                    // Filter out control messages (commands starting with '/')
                                    // These are not user messages and should not be sent to the LLM
                                    let message = chat_req.message.trim();
                                    if message.starts_with('/') {
                                        tracing::info!(
                                            "Ignoring control message: '{}', length={}",
                                            message,
                                            message.chars().count()
                                        );
                                        // Control messages are handled by the sessionId field above,
                                        // no need to process them through the LLM
                                        continue;
                                    }

                                    // Try event streaming first (rich response with tool calls)
                                    // Spawn a task to process the stream asynchronously, keeping the main loop responsive
                                    let backend_id = chat_req.backend_id.as_deref();

                                    // Check if request contains images (multimodal input)
                                    let has_images = chat_req.images.as_ref().is_some_and(|i| !i.is_empty());

                                    if has_images {
                                        // Process multimodal message with images - now with streaming support!
                                        let images: Vec<String> = chat_req.images
                                            .as_ref()
                                            .unwrap_or(&vec![])
                                            .iter()
                                            .map(|img| img.data.clone())
                                            .collect();

                                        let backend_id_str = backend_id.map(|s| s.to_string());
                                        let task_session_id = session_id.clone();
                                        let task_state = state.clone();

                                        // Set session ID on memory tool for session-scoped operations
                                        // (now handled inside Agent::process to avoid cross-session races)

                                        // Apply pinned skills for multimodal messages
                                        let selected_skills = chat_req.selected_skills.clone();
                                        if !selected_skills.is_empty() {
                                            if let Ok(agent) = task_state.agents.session_manager.get_session(&task_session_id).await {
                                                agent.set_pinned_skills(selected_skills).await;
                                            }
                                        }

                                        // Prepend page context if provided
                                        let final_message = if let Some(ref ctx) = chat_req.page_context {
                                            if !ctx.is_empty() {
                                                format!("{}\n\n{}", ctx, chat_req.message)
                                            } else {
                                                chat_req.message.clone()
                                            }
                                        } else {
                                            chat_req.message.clone()
                                        };

                                        // Use streaming for multimodal messages
                                        match task_state.agents.session_manager.process_message_multimodal_with_backend_stream(
                                            &task_session_id,
                                            &final_message,
                                            images.clone(),
                                            backend_id_str.as_deref(),
                                        ).await {
                                            Ok(stream) => {
                                                // Clone the channel sender and session ID for the spawned task
                                                let task_tx = stream_tx.clone();
                                                let task_session_id = session_id.clone();
                                                let task_state = state.clone();

                                                // Spawn a task to process the LLM stream and send events through the channel
                                                tokio::spawn(async move {
                                                    process_stream_to_channel(stream, task_session_id, chat_req.message.clone(), task_tx, task_state).await;
                                                });
                                            }
                                            Err(e) => {
                                                // Fallback to non-streaming on error
                                                tracing::error!(error = %e, session_id = %task_session_id, backend_id = ?backend_id_str, "Streaming multimodal failed, falling back to non-streaming");
                                                let response = match task_state.agents.session_manager.process_message_multimodal_with_backend(
                                                    &task_session_id,
                                                    &chat_req.message,
                                                    images,
                                                    backend_id_str.as_deref(),
                                                ).await {
                                                    Ok(resp) => json!({
                                                        "type": "response",
                                                        "content": resp.message.content,
                                                        "sessionId": task_session_id,
                                                        "toolsUsed": resp.tools_used,
                                                        "processingTimeMs": resp.processing_time_ms,
                                                    }).to_string(),
                                                    Err(inner_e) => json!({
                                                        "type": "Error",
                                                        "message": inner_e.to_string(),
                                                    }).to_string(),
                                                };

                                                if socket.send(AxumMessage::Text(response)).await.is_err() {
                                                    break;
                                                }
                                            }
                                        }
                                    } else {
                                        // Regular text-only message - use streaming
                                        let selected_skills = chat_req.selected_skills.clone();
                                        // Prepend page context if provided (only on first message per session)
                                        let final_message = if let Some(ref ctx) = chat_req.page_context {
                                            if !ctx.is_empty() {
                                                format!("{}\n\n{}", ctx, chat_req.message)
                                            } else {
                                                chat_req.message.clone()
                                            }
                                        } else {
                                            chat_req.message.clone()
                                        };
                                        // Set session ID on memory tool for session-scoped operations
                                        // (now handled inside Agent::process to avoid cross-session races)
                                        // [non-blocking stream creation] Stream creation used to
                                        // be awaited INLINE in this select arm: during the initial LLM
                                        // request (tens of seconds on local models before the first
                                        // chunk) no pings were sent and `__CANCEL__` frames could not
                                        // be processed — the whole socket looked stalled. The entire
                                        // creation + fallback path now runs in a spawned task; the
                                        // fallback response is delivered through the event channel
                                        // like any other stream event, so this loop keeps serving
                                        // ping/pong and cancel frames immediately.
                                        {
                                            let task_tx = stream_tx.clone();
                                            let task_session_id = session_id.clone();
                                            let task_state = state.clone();
                                            let task_user_message = chat_req.message.clone();
                                            let task_backend_id = backend_id.map(|s| s.to_string());
                                            let task_skills = selected_skills.clone();
                                            let task_final_message = final_message;
                                            let task_req_backend = chat_req.backend_id.clone();
                                            tokio::spawn(async move {
                                                match task_state
                                                    .agents
                                                    .session_manager
                                                    .process_message_events_with_backend_and_skills(
                                                        &task_session_id,
                                                        &task_final_message,
                                                        task_backend_id.as_deref(),
                                                        &task_skills,
                                                    )
                                                    .await
                                                {
                                                    Ok(stream) => {
                                                        process_stream_to_channel(
                                                            stream,
                                                            task_session_id,
                                                            task_user_message,
                                                            task_tx,
                                                            task_state,
                                                        )
                                                        .await;
                                                    }
                                                    Err(e) => {
                                                        // Fallback to non-streaming on error
                                                        tracing::error!(error = %e, session_id = %task_session_id, backend_id = ?task_req_backend, "Streaming text failed, falling back to non-streaming");
                                                        let response = match task_state
                                                            .agents
                                                            .session_manager
                                                            .process_message_with_backend(
                                                                &task_session_id,
                                                                &task_user_message,
                                                                task_req_backend.as_deref(),
                                                            )
                                                            .await
                                                        {
                                                            Ok(resp) => json!({
                                                                "type": "response",
                                                                "content": resp.message.content,
                                                                "sessionId": task_session_id,
                                                                "toolsUsed": resp.tools_used,
                                                                "processingTimeMs": resp.processing_time_ms,
                                                            })
                                                            .to_string(),
                                                            Err(inner_e) => json!({
                                                                "type": "Error",
                                                                "message": inner_e.to_string(),
                                                            })
                                                            .to_string(),
                                                        };
                                                        let _ = task_tx
                                                            .send(StreamEvent { json: response })
                                                            .await;
                                                    }
                                                }
                                            });
                                        }
                                    }
                                }
                            }
                            AxumMessage::Close(_) => {
                                return;
                            }
                            _ => {}
                        }
                    }
                    Some(Err(_)) => {
                        return;
                    }
                    None => {
                        return;
                    }
                }
            }
            // Handle device status updates
            update_result = device_update_rx.recv() => {
                match update_result {
                    Ok(device_update) => {
                        let msg = json!({
                            "type": "device_update",
                            "updateType": device_update.update_type,
                            "deviceId": device_update.device_id,
                            "status": device_update.status,
                            "lastSeen": device_update.last_seen,
                        }).to_string();

                        if socket.send(AxumMessage::Text(msg)).await.is_err() {
                            // Client disconnected, stop listening to device updates
                            break;
                        }
                    }
                    Err(_) => {
                        // Channel closed, stop listening
                        break;
                    }
                }
            }
            // Handle LLM stream events from spawned tasks
            stream_event = stream_rx.recv() => {
                match stream_event {
                    Some(event) => {
                        // Normal operation - no need to log every message
                        if socket.send(AxumMessage::Text(event.json)).await.is_err() {
                            // Client disconnected, stop processing stream events
                            break;
                        }
                    }
                    None => {
                        // Channel closed (all tasks dropped their senders)
                        // This is normal - continue waiting for new messages
                    }
                }
            }
            // Handle heartbeat - send periodic ping to detect dead connections
            _ = heartbeat_interval.tick() => {
                // Check for heartbeat timeout before sending ping
                if conn_meta.check_heartbeat_timeout(heartbeat_timeout).await {
                    tracing::warn!(
                        "Heartbeat timeout - no pong received for {:?}",
                        heartbeat_timeout
                    );
                    break;
                }

                conn_meta.record_ping().await;

                let ping = json!({
                    "type": "ping",
                    "timestamp": chrono::Utc::now().timestamp(),
                })
                .to_string();

                if socket.send(AxumMessage::Text(ping)).await.is_err() {
                    // Client disconnected
                    break;
                }

                conn_meta.increment_sent();
            }
        }
    }

    // Mark connection as closed
    conn_meta.mark_closed().await;
    tracing::info!(
        "WebSocket connection closed. Sent: {}, Received: {}, Duration: {:?}",
        conn_meta.get_sent_count(),
        conn_meta.get_received_count(),
        conn_meta.connection_duration()
    );

    // Cleanup: persist session history AFTER loop ends (when connection closes)
    let session_id_opt = current_session_id.read().await.clone();
    if let Some(session_id) = session_id_opt.as_ref() {
        // Cancel any in-flight LLM stream for this session.
        // Without this, a client disconnect leaves the stream running in its
        // spawned task (burning tokens) and the cancel_senders entry leaks
        // because the wrapped cleanup_stream never reaches its end-of-loop remove.
        let cancelled = state
            .agents
            .session_manager
            .cancel_session(session_id)
            .await;
        if cancelled {
            tracing::info!(
                category = "session",
                session_id = %session_id,
                "Cancelled in-flight LLM stream on WebSocket disconnect"
            );
        }
        if let Err(e) = state
            .agents
            .session_manager
            .persist_history(session_id)
            .await
        {
            tracing::warn!(category = "session", error = %e, "Failed to persist history on disconnect");
        }
    }
}
