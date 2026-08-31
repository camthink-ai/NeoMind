//! Chat Stream Capability
//!
//! Streaming chat session invocation backed by NeoMind's SessionManager.
//!
//! `invoke()` triggers a chat turn for a given (or new) session and returns
//! immediately with the `session_id`. The actual `AgentEvent` stream produced
//! by `SessionManager::process_message_events` is published onto the platform
//! EventBus as `NeoMindEvent::AgentStreamChunk` events, each tagged with the
//! same `session_id`. Extensions consume the stream by subscribing via the
//! existing `event::subscribe` capability and filtering by `session_id`.
//!
//! # Parameters
//!
//! - `message` (required): user message text
//! - `images` (optional): array of data-URL strings — multimodal turn; the
//!   host routes it to a vision-capable backend (text-only otherwise)
//! - `session_id` (optional): existing session to continue; omit to let the
//!   host create a new ad-hoc session.
//!
//! # Return
//!
//! ```json
//! { "session_id": "uuid", "created": true }
//! ```
//!
//! `created=true` means a new session was allocated for this call; `false`
//! means an existing session was reused.
//!
//! # Event payload
//!
//! Each `AgentStreamChunk` event carries the full AgentEvent JSON, e.g.:
//!
//! ```json
//! {
//!   "type": "agent_stream_chunk",
//!   "session_id": "...",
//!   "chunk": { "type": "Content", "content": "hello" },
//!   "timestamp": 0
//! }
//! ```

use serde_json::{json, Value};

pub type CapabilityError = String;

#[cfg(not(target_arch = "wasm32"))]
pub type Context = crate::host::ExtensionContext;

#[cfg(not(target_arch = "wasm32"))]
use crate::host::ExtensionCapability;

/// Invoke a streaming chat turn.
///
/// Returns the `session_id` to subscribe to. Callers should already have an
/// event handler registered (via `event::register_handler`) and pass a
/// subscription filter matching `"agent_stream_chunk"` for this `session_id`.
#[cfg(not(target_arch = "wasm32"))]
pub async fn invoke(
    context: &Context,
    message: &str,
    session_id: Option<&str>,
) -> Result<Value, CapabilityError> {
    let mut params = json!({ "message": message });
    if let Some(sid) = session_id {
        params["session_id"] = json!(sid);
    }
    context
        .invoke_capability(ExtensionCapability::ChatStream, &params)
        .await
        .map_err(|e| e.to_string())
}

/// Multimodal variant: attach images to the chat turn.
///
/// `images` are data-URL strings (`data:image/jpeg;base64,…`). The platform
/// routes them to a multimodal-capable LLM backend (the same path the chat
/// UI's image upload takes); backends without vision degrade to text-only.
///
/// Mind the runner's capability-input cap (~1 MB): downscale frames first —
/// roughly ≤ 1024px JPEG keeps the base64 comfortably under it.
#[cfg(not(target_arch = "wasm32"))]
pub async fn invoke_with_images(
    context: &Context,
    message: &str,
    images: &[String],
    session_id: Option<&str>,
) -> Result<Value, CapabilityError> {
    let mut params = json!({ "message": message, "images": images });
    if let Some(sid) = session_id {
        params["session_id"] = json!(sid);
    }
    context
        .invoke_capability(ExtensionCapability::ChatStream, &params)
        .await
        .map_err(|e| e.to_string())
}

/// Convenience: invoke and extract `session_id` from the response.
#[cfg(not(target_arch = "wasm32"))]
pub async fn invoke_for_session_id(
    context: &Context,
    message: &str,
    session_id: Option<&str>,
) -> Result<String, CapabilityError> {
    let resp = invoke(context, message, session_id).await?;
    resp.get("session_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "missing session_id in ChatStream response".to_string())
}

// ============================================================================
// Persistent session-stream API (Phase 2: Direct Stream Pattern)
// ============================================================================
//
// `chat_stream` (above) wraps each turn into a one-shot capability call:
// convenient, but spawns a fresh task per turn and uses AgentEvent::End as
// the terminator — which is ambiguous with reasoning models / tool loops.
//
// The `chat_session_*` family decouples session lifetime from per-turn
// streaming:
//
//   open(existing_session_id?)
//     → get_or_create_session
//     → returns { session_id, created }
//     → contract: AgentEvents for this session will be delivered to
//       subscribers (via AgentStreamChunk events on the bus, plus the
//       host-internal direct mpsc for low-latency consumers).
//
//   send(session_id, message)
//     → generates turn_id (UUID)
//     → spawns task driving process_message_events
//     → injects turn_id into each chunk wrapper
//     → returns { turn_id } immediately (does NOT wait for LLM completion)
//
//   close(session_id)
//     → cancel_session + remove_subscriber
//
//   cancel_turn(session_id, turn_id?) → cancels in-flight turn
//
// turn_id is transport-layer metadata injected by the provider; AgentEvent
// itself is unchanged.

/// Open a persistent chat session subscription.
///
/// Returns `{ session_id, created }`. `created=true` means a new session
/// was allocated; `false` means an existing session was reused.
#[cfg(not(target_arch = "wasm32"))]
pub async fn open_session(
    context: &Context,
    existing_session_id: Option<&str>,
) -> Result<Value, CapabilityError> {
    let mut params = json!({});
    if let Some(sid) = existing_session_id {
        params["session_id"] = json!(sid);
    }
    context
        .invoke_capability(ExtensionCapability::ChatSessionOpen, &params)
        .await
        .map_err(|e| e.to_string())
}

/// Send a message to an open chat session. Returns immediately with
/// `{ turn_id }`. LLM events for this turn arrive via AgentStreamChunk
/// events; each `chunk` carries `turn_id` matching the returned value.
#[cfg(not(target_arch = "wasm32"))]
pub async fn send_message(
    context: &Context,
    session_id: &str,
    message: &str,
) -> Result<Value, CapabilityError> {
    let params = json!({ "session_id": session_id, "message": message });
    context
        .invoke_capability(ExtensionCapability::ChatSessionSend, &params)
        .await
        .map_err(|e| e.to_string())
}

/// Close a chat session subscription. Cancels any in-flight turn and
/// removes the subscriber from the session.
#[cfg(not(target_arch = "wasm32"))]
pub async fn close_session(context: &Context, session_id: &str) -> Result<Value, CapabilityError> {
    let params = json!({ "session_id": session_id });
    context
        .invoke_capability(ExtensionCapability::ChatSessionClose, &params)
        .await
        .map_err(|e| e.to_string())
}

/// Cancel the in-flight turn within an open chat session. Does NOT close
/// the session — the caller can immediately `send_message` again.
#[cfg(not(target_arch = "wasm32"))]
pub async fn cancel_turn(
    context: &Context,
    session_id: &str,
    turn_id: Option<&str>,
) -> Result<Value, CapabilityError> {
    let mut params = json!({ "session_id": session_id });
    if let Some(t) = turn_id {
        params["turn_id"] = json!(t);
    }
    context
        .invoke_capability(ExtensionCapability::ChatStreamCancelTurn, &params)
        .await
        .map_err(|e| e.to_string())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invoke_params_with_existing_session() {
        let params = json!({
            "message": "hello",
            "session_id": "sess-123",
        });
        assert_eq!(params["message"], "hello");
        assert_eq!(params["session_id"], "sess-123");
    }

    #[test]
    fn test_invoke_params_new_session() {
        // When session_id is omitted, only message is required.
        let params = json!({ "message": "hello" });
        assert_eq!(params["message"], "hello");
        assert!(params.get("session_id").is_none());
    }

    #[test]
    fn test_invoke_for_session_id_extracts_field() {
        let resp = json!({ "session_id": "abc-xyz", "created": true });
        let sid = resp
            .get("session_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        assert_eq!(sid.as_deref(), Some("abc-xyz"));
    }

    #[test]
    fn test_invoke_for_session_id_missing_field() {
        let resp = json!({ "created": true });
        let sid = resp.get("session_id").and_then(|v| v.as_str());
        assert!(sid.is_none());
    }
}
