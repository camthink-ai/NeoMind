//! Extension streaming WebSocket handlers.
//!
//! Provides universal streaming support for extensions:
//! - Stateless mode: single-chunk processing (image analysis, data transformation)
//! - Stateful mode: session-based processing (video streams, audio processing)
//! - Push mode: extension-driven data pushing (sensor streams, log tailing)
//!
//! # Protocol
//!
//! ## Message Types
//!
//! ### Client → Server
//! - `hello`: Initial handshake
//! - `init`: Initialize session (stateful mode)
//! - `chunk`: Binary data chunk
//! - `close`: Close connection/session
//! - `ack`: Acknowledge received chunks (flow control)
//!
//! ### Server → Client
//! - `capability`: Stream capability description
//! - `session_created`: Session initialized (stateful mode)
//! - `result`: Processing result
//! - `push_output`: Extension-pushed data (push mode)
//! - `error`: Error occurred
//! - `session_closed`: Session terminated
//! - `heartbeat`: Keep-alive message
//!
//! # Binary Frame Format
//!
//! Client → server binary frames:
//! ```text
//! [sequence: u64 (8 bytes, big endian)][data...]
//! ```
//!
//! Server → client binary push frames (opt-in via `init` config `{"binary": true}`,
//! acknowledged by `session_created.binary`):
//! ```text
//! [kind: u8 = 1][version: u8 = 1][sequence: u64 BE][meta_len: u32 BE]
//! [meta: JSON utf8][payload bytes]
//! ```
//! `meta` mirrors the Text `push_output` envelope minus `data`/`sequence`
//! (`session_id`, `data_type`, `timestamp`, `metadata`). Payload is the raw
//! `PushOutputMessage.data` — no base64 anywhere. Control messages
//! (`session_created`, `error`, …) always stay on Text frames; the WebSocket
//! frame type itself is the first-level discriminator.
//!
//! # Push Mode Architecture
//!
//! For Push mode, extensions actively push data to clients:
//! 1. Client connects via WebSocket and sends `init` message
//! 2. Server creates session and registers WebSocket sender
//! 3. Server calls `extension.start_push(session_id)`
//! 4. Extension pushes data via `PushOutputMessage` channel
//! 5. Server forwards pushed data to WebSocket client
//! 6. On disconnect, server calls `extension.stop_push()` and cleans up

use axum::{
    extract::ws::{Message as WsMessage, WebSocket},
    extract::{Path, Query, State, WebSocketUpgrade},
    response::IntoResponse,
};
use base64::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

use crate::handlers::common::{ok, HandlerResult};
use crate::models::error::ErrorResponse;
use crate::server::ServerState;
use neomind_core::extension::{
    DataChunk, PushOutputMessage, SessionStats, StreamCapability, StreamDataType, StreamDirection,
    StreamError, StreamMode, StreamSession,
};

// ============================================================================
// WebSocket Message Types
// ============================================================================

/// Message type from client
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    /// Initial handshake
    Hello,
    /// Initialize session (stateful mode)
    Init { config: Option<serde_json::Value> },
    /// Close connection/session
    Close,
    /// Flow control acknowledgment
    Ack { sequence: u64 },
}

/// Message type to client
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMessage {
    /// Stream capability
    Capability { capability: StreamCapabilityDto },
    /// Session created (stateful mode)
    SessionCreated {
        session_id: String,
        server_time: i64,
        /// Whether push_output frames for this session will use binary frames
        /// (only meaningful in Push mode; always false otherwise).
        binary: bool,
    },
    /// Processing result
    Result {
        input_sequence: Option<u64>,
        output_sequence: u64,
        data: String, // base64 encoded
        data_type: String,
        processing_ms: f32,
        metadata: Option<serde_json::Value>,
    },
    /// Extension-pushed output (Push mode)
    PushOutput {
        session_id: String,
        sequence: u64,
        data: String, // base64 encoded
        data_type: String,
        timestamp: i64,
        metadata: Option<serde_json::Value>,
    },
    /// Error occurred
    Error {
        code: String,
        message: String,
        retryable: bool,
    },
    /// Session closed
    SessionClosed {
        session_id: String,
        total_frames: u64,
        duration_ms: u64,
        stats: SessionStatsDto,
    },
}

/// DTO for stream capability
#[derive(Debug, Serialize)]
struct StreamCapabilityDto {
    direction: String,
    mode: String,
    supported_data_types: Vec<String>,
    max_chunk_size: usize,
    preferred_chunk_size: usize,
    max_concurrent_sessions: usize,
    flow_control: FlowControlDto,
}

impl From<&StreamCapability> for StreamCapabilityDto {
    fn from(cap: &StreamCapability) -> Self {
        Self {
            direction: match cap.direction {
                StreamDirection::Upload => "upload".to_string(),
                StreamDirection::Download => "download".to_string(),
                StreamDirection::Bidirectional => "bidirectional".to_string(),
            },
            mode: match cap.mode {
                StreamMode::Stateless => "stateless".to_string(),
                StreamMode::Stateful => "stateful".to_string(),
                StreamMode::Push => "push".to_string(),
            },
            supported_data_types: cap
                .supported_data_types
                .iter()
                .map(|dt| dt.mime_type())
                .collect(),
            max_chunk_size: cap.max_chunk_size,
            preferred_chunk_size: cap.preferred_chunk_size,
            max_concurrent_sessions: cap.max_concurrent_sessions,
            flow_control: FlowControlDto {
                supports_backpressure: cap.flow_control.supports_backpressure,
                window_size: cap.flow_control.window_size,
                supports_throttling: cap.flow_control.supports_throttling,
                max_rate: cap.flow_control.max_rate,
            },
        }
    }
}

/// DTO for flow control
#[derive(Debug, Serialize)]
struct FlowControlDto {
    supports_backpressure: bool,
    window_size: u32,
    supports_throttling: bool,
    max_rate: u32,
}

/// DTO for session stats
#[derive(Debug, Serialize)]
struct SessionStatsDto {
    input_chunks: u64,
    output_chunks: u64,
    input_bytes: u64,
    output_bytes: u64,
    errors: u64,
}

impl From<&SessionStats> for SessionStatsDto {
    fn from(stats: &SessionStats) -> Self {
        Self {
            input_chunks: stats.input_chunks,
            output_chunks: stats.output_chunks,
            input_bytes: stats.input_bytes,
            output_bytes: stats.output_bytes,
            errors: stats.errors,
        }
    }
}

// ============================================================================
// Push Output Router
// ============================================================================

/// Router for pushing extension outputs to WebSocket clients
///
/// This structure manages the routing of PushOutputMessage from extensions
/// to the appropriate WebSocket client connections.
pub struct PushOutputRouter {
    /// Map of session_id -> WebSocket sender
    /// Each sender can be used to forward pushed data to the client
    senders: RwLock<HashMap<String, mpsc::Sender<PushOutputMessage>>>,
}

impl PushOutputRouter {
    /// Create a new router
    pub fn new() -> Self {
        Self {
            senders: RwLock::new(HashMap::new()),
        }
    }

    /// Register a sender for a session
    pub async fn register(&self, session_id: String, sender: mpsc::Sender<PushOutputMessage>) {
        let mut senders: tokio::sync::RwLockWriteGuard<
            '_,
            HashMap<String, mpsc::Sender<PushOutputMessage>>,
        > = self.senders.write().await;
        senders.insert(session_id.clone(), sender);
        tracing::debug!("Registered push output sender for session: {}", session_id);
    }

    /// Unregister a session
    pub async fn unregister(&self, session_id: &str) {
        let mut senders: tokio::sync::RwLockWriteGuard<
            '_,
            HashMap<String, mpsc::Sender<PushOutputMessage>>,
        > = self.senders.write().await;
        senders.remove(session_id);
        tracing::debug!(
            "Unregistered push output sender for session: {}",
            session_id
        );
    }

    /// Route a push output message to the appropriate session.
    ///
    /// Uses `try_send()` instead of `send().await` so that the caller (receiver
    /// thread) is never blocked when the WebSocket consumer is slow. Dropped
    /// messages are logged as a warning.
    pub async fn route(&self, output: PushOutputMessage) -> bool {
        let senders: tokio::sync::RwLockReadGuard<
            '_,
            HashMap<String, mpsc::Sender<PushOutputMessage>>,
        > = self.senders.read().await;
        if let Some(sender) = senders.get(&output.session_id) {
            match sender.try_send(output) {
                Ok(_) => true,
                Err(mpsc::error::TrySendError::Full(output)) => {
                    tracing::warn!(
                        session_id = %output.session_id,
                        sequence = output.sequence,
                        "Push output dropped: WebSocket consumer too slow"
                    );
                    false
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    tracing::warn!("Failed to route push output: channel closed");
                    false
                }
            }
        } else {
            tracing::debug!("No sender registered for session: {}", output.session_id);
            false
        }
    }

    /// Get the count of active sessions
    pub async fn session_count(&self) -> usize {
        let senders: tokio::sync::RwLockReadGuard<
            '_,
            HashMap<String, mpsc::Sender<PushOutputMessage>>,
        > = self.senders.read().await;
        senders.len()
    }
}

impl Default for PushOutputRouter {
    fn default() -> Self {
        Self::new()
    }
}

// Global push router instance
static PUSH_ROUTER: std::sync::OnceLock<Arc<PushOutputRouter>> = std::sync::OnceLock::new();

/// Get the global push router
pub fn get_push_router() -> Arc<PushOutputRouter> {
    PUSH_ROUTER
        .get_or_init(|| Arc::new(PushOutputRouter::new()))
        .clone()
}

/// Client info message format
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClientInfoMessage {
    client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ip_addr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_agent: Option<String>,
}

// ============================================================================
// WebSocket Handler
// ============================================================================

/// GET /api/extensions/:id/stream
///
/// WebSocket endpoint for extension streaming.
pub async fn extension_stream_ws(
    Path(extension_id): Path<String>,
    ws: WebSocketUpgrade,
    State(state): State<ServerState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    // Authenticate via ?token= query parameter
    if let Some(token) = params.get("token") {
        let is_valid = state.auth.api_key_state.validate_key(token)
            || state.auth.user_state.validate_token(token).is_ok();
        if !is_valid {
            return (
                axum::http::StatusCode::UNAUTHORIZED,
                "Invalid or expired token",
            )
                .into_response();
        }
    } else {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            "Authentication required. Provide ?token= query parameter.",
        )
            .into_response();
    }

    ws.on_upgrade(move |socket| handle_stream_socket(socket, extension_id, state))
}

async fn handle_stream_socket(mut socket: WebSocket, extension_id: String, state: ServerState) {
    tracing::info!("Extension stream connection opened for: {}", extension_id);

    let safety_manager = state.extensions.registry.safety_manager();

    let extension = match state.extensions.runtime.get_extension(&extension_id).await {
        Some(ext) => {
            tracing::debug!("Found extension proxy {}", extension_id);
            ext
        }
        None => {
            send_error(
                &mut socket,
                "EXTENSION_NOT_FOUND",
                format!("Extension '{}' not available for streaming", extension_id),
            )
            .await;
            return;
        }
    };

    let isolated_extension = state
        .extensions
        .runtime
        .isolated_manager()
        .get(&extension_id)
        .await;

    // Check streaming capability
    let ext_read = extension.read().await;
    let capability = ext_read.stream_capability();
    drop(ext_read);

    let cap = match capability {
        Some(c) => c,
        None => {
            send_error(
                &mut socket,
                "NOT_SUPPORTED",
                "Extension does not support streaming".to_string(),
            )
            .await;
            return;
        }
    };

    // Send capability
    let msg = ServerMessage::Capability {
        capability: StreamCapabilityDto::from(&cap),
    };
    tracing::debug!(
        "Sending capability: mode={:?}, direction={:?}",
        cap.mode,
        cap.direction
    );
    send_message(&mut socket, &msg).await;

    // Track session if stateful
    let mut session_id: Option<String> = None;
    let mut output_sequence = 0u64;
    // Negotiated per-connection: the client opted into binary push frames via
    // `init` config. Until opted in, every push goes out as legacy Text+base64
    // (byte-identical to pre-negotiation behavior).
    let mut binary_push_session = false;

    // For Push mode: channel to receive push outputs from extension
    let mut push_rx: Option<mpsc::Receiver<PushOutputMessage>> = None;
    let push_router = get_push_router();
    // Handle for the shared IPC→router forwarder task (Push mode).
    // Stored to keep the task alive; NOT used for cancellation (the forwarder
    // is shared across all sessions and must outlive any single connection).
    let mut _push_forwarder_handle: Option<tokio::task::JoinHandle<()>> = None;

    // Message loop
    loop {
        // For Push mode: also check for push outputs
        let msg_result = if let Some(ref mut rx) = push_rx {
            // Use tokio::select to handle both WebSocket messages and push outputs
            // Type annotation needed for the receiver
            let rx: &mut mpsc::Receiver<PushOutputMessage> = rx;
            tokio::select! {
                msg = socket.recv() => msg,
                output = rx.recv() => {
                    match output {
                        Some(output) => {
                            // Forward push output to WebSocket
                            let msg = encode_push_output(&output, binary_push_session);
                            let _ = send_with_timeout(&mut socket, msg).await;
                            continue;
                        }
                        None => {
                            // Channel closed
                            tracing::debug!("Push output channel closed");
                            break;
                        }
                    }
                }
            }
        } else {
            socket.recv().await
        };

        match msg_result {
            Some(Ok(msg)) => {
                match msg {
                    WsMessage::Text(text) => {
                        // Parse client message
                        if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                            match client_msg {
                                ClientMessage::Hello => {
                                    // Already sent capability, ignore
                                }
                                ClientMessage::Init { config } => {
                                    if matches!(cap.mode, StreamMode::Stateful | StreamMode::Push) {
                                        // Binary push frames only exist in Push mode.
                                        binary_push_session =
                                            cap.mode == StreamMode::Push && binary_requested(&config);
                                        // Create session
                                        let sid = Uuid::new_v4().to_string();
                                        let client_info = ClientInfoMessage {
                                            client_id: Uuid::new_v4().to_string(),
                                            ip_addr: None,
                                            user_agent: None,
                                        };

                                        let session = StreamSession::new(
                                            sid.clone(),
                                            extension_id.clone(),
                                            config.clone().unwrap_or_default(),
                                            neomind_core::extension::ClientInfo {
                                                client_id: client_info.client_id.clone(),
                                                ip_addr: client_info.ip_addr.clone(),
                                                user_agent: client_info.user_agent.clone(),
                                            },
                                        );

                                        // Initialize in extension
                                        let ext = extension.read().await;
                                        tracing::info!(
                                            "Initializing session {} for extension {}",
                                            sid,
                                            extension_id
                                        );
                                        if let Err(e) = ext.init_session(&session).await {
                                            tracing::error!("Failed to init session: {}", e);
                                            send_error(
                                                &mut socket,
                                                "SESSION_INIT_FAILED",
                                                format!("Failed to init session: {}", e),
                                            )
                                            .await;
                                            continue;
                                        }
                                        tracing::debug!("Session {} initialized successfully", sid);

                                        // For Push mode: bridge runner PushOutput IPC into the websocket router.
                                        if cap.mode == StreamMode::Push {
                                            // Create per-session channel for push outputs
                                            let (tx, mut rx) =
                                                mpsc::channel::<PushOutputMessage>(32);

                                            // Register with router
                                            push_router.register(sid.clone(), tx).await;

                                            let Some(isolated) = isolated_extension.clone() else {
                                                send_error(
                                                    &mut socket,
                                                    "PUSH_NOT_AVAILABLE",
                                                    "Push mode requires isolated extension runtime"
                                                        .to_string(),
                                                )
                                                .await;
                                                push_router.unregister(&sid).await;
                                                continue;
                                            };

                                            // Set up the IPC→router channel if not already set.
                                            // The same shared channel is used for ALL sessions.
                                            // The router dispatches by session_id, so all sessions
                                            // share one forwarder and one IPC channel safely.
                                            if isolated.get_push_output_channel().await.is_none() {
                                                let (push_tx, mut push_rx_ipc) = mpsc::channel(256);
                                                isolated.set_push_output_channel(push_tx).await;

                                                let router = push_router.clone();
                                                let handle = tokio::spawn(async move {
                                                    while let Some(output) =
                                                        push_rx_ipc.recv().await
                                                    {
                                                        let _ = router
                                                            .route(PushOutputMessage {
                                                                session_id: output.session_id,
                                                                sequence: output.sequence,
                                                                data: output.data,
                                                                data_type: output.data_type,
                                                                timestamp: output.timestamp,
                                                                metadata: output.metadata,
                                                            })
                                                            .await;
                                                    }
                                                    tracing::debug!("Push IPC forwarder stopped");
                                                });
                                                _push_forwarder_handle = Some(handle);
                                            }

                                            // Start pushing for this session
                                            if let Err(e) = ext.start_push(&sid).await {
                                                send_error(
                                                    &mut socket,
                                                    "PUSH_START_FAILED",
                                                    format!("Failed to start push: {}", e),
                                                )
                                                .await;
                                                push_router.unregister(&sid).await;
                                                continue;
                                            }

                                            drop(ext);

                                            // Send session created (before moving socket)
                                            send_message(
                                                &mut socket,
                                                &ServerMessage::SessionCreated {
                                                    session_id: sid.clone(),
                                                    server_time: chrono::Utc::now()
                                                        .timestamp_millis(),
                                                    binary: binary_push_session,
                                                },
                                            )
                                            .await;

                                            tracing::info!(
                                                "Push session {} entering non-blocking stream loop",
                                                sid
                                            );

                                            // ── Bidirectional Push mode ─────────────────────
                                            // For Bidirectional extensions (e.g. voice-assistant),
                                            // every frame matters: audio PCM chunks arriving faster
                                            // than the socket drains must NOT be dropped. watch's
                                            // "latest only" semantics would silently discard them.
                                            // Use a bounded mpsc instead, and also forward inbound
                                            // binary frames to process_session_chunk (mic PCM).
                                            if cap.direction == StreamDirection::Bidirectional {
                                                let (ws_out_tx, mut ws_out_rx) =
                                                    mpsc::channel::<WsMessage>(64);
                                                let (ws_in_tx, mut ws_in_rx) =
                                                    mpsc::channel::<String>(8);
                                                let (binary_in_tx, mut binary_in_rx) =
                                                    mpsc::channel::<(u64, Vec<u8>)>(64);
                                                let (ws_done_tx, mut ws_done_rx) =
                                                    mpsc::channel::<()>(1);

                                                let ws_task = tokio::spawn(async move {
                                                    let mut frame_count: u64 = 0;
                                                    let result = async {
                                                        loop {
                                                            tokio::select! {
                                                                msg = socket.recv() => {
                                                                    match msg {
                                                                        Some(Ok(WsMessage::Text(text))) => {
                                                                            let _ = ws_in_tx.send(text).await;
                                                                        }
                                                                        Some(Ok(WsMessage::Binary(data))) => {
                                                                            if let Some((seq, chunk)) = parse_binary_frame(data) {
                                                                                let _ = binary_in_tx.send((seq, chunk)).await;
                                                                            }
                                                                        }
                                                                        Some(Ok(WsMessage::Close(_))) | None => {
                                                                            tracing::info!(
                                                                                frames_sent = frame_count,
                                                                                "WebSocket closed by client or EOF"
                                                                            );
                                                                            return;
                                                                        }
                                                                        Some(Ok(WsMessage::Ping(data))) => {
                                                                            match send_with_timeout(&mut socket, WsMessage::Pong(data)).await {
                                                                                Ok(_) => {}
                                                                                Err(_) => {
                                                                                    tracing::info!(frames_sent = frame_count, "Pong send timeout (2s)");
                                                                                    return;
                                                                                }
                                                                            }
                                                                        }
                                                                        _ => {}
                                                                    }
                                                                }
                                                                json = ws_out_rx.recv() => {
                                                                    match json {
                                                                        Some(msg) => {
                                                                            let send_start = std::time::Instant::now();
                                                                            match send_with_timeout(&mut socket, msg).await {
                                                                                Ok(_) => {
                                                                                    let elapsed = send_start.elapsed();
                                                                                    if elapsed > std::time::Duration::from_millis(500) {
                                                                                        tracing::warn!(elapsed_ms = elapsed.as_millis() as u64, "Slow WS send — client throttled");
                                                                                    }
                                                                                }
                                                                                Err(_) => {
                                                                                    tracing::info!(
                                                                                        frames_sent = frame_count,
                                                                                        "WS send timeout (2s) — dead connection"
                                                                                    );
                                                                                    return;
                                                                                }
                                                                            }
                                                                            frame_count += 1;
                                                                        }
                                                                        None => return,
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }.await;
                                                    let _ = ws_done_tx.send(()).await;
                                                    result
                                                });

                                                let mut frames_received: u64 = 0;
                                                loop {
                                                    tokio::select! {
                                                        output = rx.recv() => {
                                                            match output {
                                                                Some(output) => {
                                                                    frames_received += 1;
                                                                    let msg = encode_push_output(&output, binary_push_session);
                                                                    if let Err(e) = ws_out_tx.try_send(msg) {
                                                                        tracing::warn!(
                                                                            frames_received,
                                                                            error = %e,
                                                                            "ws_out_tx full — dropping push frame (client too slow)"
                                                                        );
                                                                    }
                                                                }
                                                                None => {
                                                                    tracing::debug!("Push output channel closed");
                                                                    break;
                                                                }
                                                            }
                                                        }
                                                        text = ws_in_rx.recv() => {
                                                            match text {
                                                                Some(text) => {
                                                                    if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                                                                        if matches!(client_msg, ClientMessage::Close) {
                                                                            break;
                                                                        }
                                                                    }
                                                                }
                                                                None => break,
                                                            }
                                                        }
                                                        Some((seq, data)) = binary_in_rx.recv() => {
                                                            let ext = extension.read().await;
                                                            let chunk = DataChunk {
                                                                sequence: seq,
                                                                data_type: StreamDataType::Binary,
                                                                data,
                                                                timestamp: chrono::Utc::now().timestamp_millis(),
                                                                metadata: None,
                                                                is_last: false,
                                                            };
                                                            let _ = ext.process_session_chunk(&sid, chunk).await;
                                                        }
                                                        _ = ws_done_rx.recv() => {
                                                            tracing::warn!("WebSocket I/O task terminated — connection lost");
                                                            break;
                                                        }
                                                        _ = tokio::time::sleep(PUSH_STALL_TIMEOUT) => {
                                                            tracing::error!(
                                                                session_id = %sid,
                                                                received = frames_received,
                                                                "Push source stalled for {}s — extension pump dead, closing session",
                                                                PUSH_STALL_TIMEOUT.as_secs()
                                                            );
                                                            let stall_msg = ServerMessage::Error {
                                                                code: "STREAM_STALLED".to_string(),
                                                                message: format!(
                                                                    "No frames received for {} seconds",
                                                                    PUSH_STALL_TIMEOUT.as_secs()
                                                                ),
                                                                retryable: true,
                                                            };
                                                            if let Ok(json) = serde_json::to_string(&stall_msg) {
                                                                let _ = ws_out_tx.try_send(WsMessage::Text(json));
                                                            }
                                                            break;
                                                        }
                                                    }
                                                }

                                                // Cleanup: stop pushing
                                                let ext = extension.read().await;
                                                let _ = ext.stop_push(&sid).await;
                                                drop(ext);
                                                push_router.unregister(&sid).await;

                                                let ext = extension.read().await;
                                                if let Ok(stats) = ext.close_session(&sid).await {
                                                    let msg = ServerMessage::SessionClosed {
                                                        session_id: sid.clone(),
                                                        total_frames: stats.input_chunks,
                                                        duration_ms: (chrono::Utc::now()
                                                            .timestamp_millis()
                                                            - stats.last_activity)
                                                            as u64,
                                                        stats: SessionStatsDto::from(&stats),
                                                    };
                                                    if let Ok(json) = serde_json::to_string(&msg) {
                                                        let _ = ws_out_tx.try_send(WsMessage::Text(json));
                                                    }
                                                }

                                                drop(ws_out_tx);
                                                let _ = ws_task.await;

                                                tracing::info!(
                                                    "Extension stream disconnected for: {}",
                                                    extension_id
                                                );
                                                return; // Bidirectional path complete — skip watch-based code
                                            }

                                            // ── Non-blocking push mode (Outbound-only) ─────
                                            // Spawn a dedicated WebSocket I/O task that owns the
                                            // socket. Uses a watch channel so only the LATEST frame
                                            // is kept — old frames are automatically replaced.
                                            // This is ideal for video streaming where showing the
                                            // most recent frame matters more than delivering every one.
                                            let (ws_out_tx, mut ws_out_rx) =
                                                tokio::sync::watch::channel(WsMessage::Text(String::new()));
                                            let (ws_in_tx, mut ws_in_rx) =
                                                mpsc::channel::<String>(8);
                                            // Done signal: avoids JoinHandle panic on re-poll
                                            let (ws_done_tx, mut ws_done_rx) =
                                                mpsc::channel::<()>(1);

                                            let ws_task = tokio::spawn(async move {
                                                let mut frame_count: u64 = 0;
                                                let result = async {
                                                    loop {
                                                        tokio::select! {
                                                            msg = socket.recv() => {
                                                                match msg {
                                                                    Some(Ok(WsMessage::Text(text))) => {
                                                                        let _ = ws_in_tx.send(text).await;
                                                                    }
                                                                    Some(Ok(WsMessage::Close(_))) | None => {
                                                                        tracing::info!(
                                                                            frames_sent = frame_count,
                                                                            "WebSocket closed by client or EOF"
                                                                        );
                                                                        return;
                                                                    }
                                                                    Some(Ok(WsMessage::Ping(data))) => {
                                                                        match send_with_timeout(&mut socket, WsMessage::Pong(data)).await {
                                                                            Ok(_) => {}
                                                                            Err(_) => {
                                                                                tracing::info!(frames_sent = frame_count, "Pong send timeout (2s)");
                                                                                return;
                                                                            }
                                                                        }
                                                                    }
                                                                    _ => {}
                                                                }
                                                            }
                                                            _ = ws_out_rx.changed() => {
                                                                let msg = ws_out_rx.borrow_and_update().clone();
                                                                let send_start = std::time::Instant::now();
                                                                match send_with_timeout(&mut socket, msg).await {
                                                                    Ok(_) => {
                                                                        let elapsed = send_start.elapsed();
                                                                        if elapsed > std::time::Duration::from_millis(500) {
                                                                            tracing::warn!(elapsed_ms = elapsed.as_millis() as u64, "Slow WS send — client throttled");
                                                                        }
                                                                    }
                                                                    Err(_) => {
                                                                        tracing::info!(
                                                                            frames_sent = frame_count,
                                                                            "WS send timeout (2s) — dead connection"
                                                                        );
                                                                        return;
                                                                    }
                                                                }
                                                                frame_count += 1;
                                                            }
                                                        }
                                                    }
                                                }.await;
                                                let _ = ws_done_tx.send(()).await;
                                                result
                                            });

                                            // Push output loop — never blocks on socket I/O
                                            let mut frames_received: u64 = 0;
                                            let mut frames_dropped: u64 = 0;
                                            loop {
                                                tokio::select! {
                                                    output = rx.recv() => {
                                                        match output {
                                                            Some(output) => {
                                                                frames_received += 1;
                                                                let msg = encode_push_output(&output, binary_push_session);
                                                                // watch::send replaces old value (latest frame wins)
                                                                let is_new = ws_out_tx.send(msg).is_ok();
                                                                if !is_new {
                                                                    frames_dropped += 1;
                                                                }
                                                                // Periodic diagnostics
                                                                if frames_received.is_multiple_of(500) {
                                                                    tracing::info!(
                                                                        received = frames_received,
                                                                        dropped = frames_dropped,
                                                                        "Push stream stats"
                                                                    );
                                                                    frames_dropped = 0;
                                                                }
                                                            }
                                                            None => {
                                                                tracing::debug!("Push output channel closed");
                                                                break;
                                                            }
                                                        }
                                                    }
                                                    text = ws_in_rx.recv() => {
                                                        match text {
                                                            Some(text) => {
                                                                if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                                                                    if matches!(client_msg, ClientMessage::Close) {
                                                                        break;
                                                                    }
                                                                }
                                                            }
                                                            None => break,
                                                        }
                                                    }
                                                    _ = ws_done_rx.recv() => {
                                                        tracing::warn!("WebSocket I/O task terminated — connection lost");
                                                        break;
                                                    }
                                                    // Stall guard: if the extension pump produces no frame for
                                                    // PUSH_STALL_TIMEOUT, the source (e.g. FFmpeg/YOLO pump) is
                                                    // dead — no amount of WS-send retrying will help. Closing
                                                    // the session lets the client reconnect and start fresh.
                                                    // Each select! iteration restarts the timer, so steady
                                                    // frame flow never trips it.
                                                    _ = tokio::time::sleep(PUSH_STALL_TIMEOUT) => {
                                                        tracing::error!(
                                                            session_id = %sid,
                                                            received = frames_received,
                                                            "Push source stalled for {}s — extension pump dead, closing session",
                                                            PUSH_STALL_TIMEOUT.as_secs()
                                                        );
                                                        let stall_msg = ServerMessage::Error {
                                                            code: "STREAM_STALLED".to_string(),
                                                            message: format!(
                                                                "No frames received for {} seconds",
                                                                PUSH_STALL_TIMEOUT.as_secs()
                                                            ),
                                                            retryable: true,
                                                        };
                                                        if let Ok(json) = serde_json::to_string(&stall_msg) {
                                                            let _ = ws_out_tx.send(WsMessage::Text(json));
                                                        }
                                                        break;
                                                    }
                                                }
                                            }

                                            // Cleanup: stop pushing
                                            let ext = extension.read().await;
                                            let _ = ext.stop_push(&sid).await;
                                            drop(ext);
                                            push_router.unregister(&sid).await;

                                            // Close session (best-effort send stats)
                                            let ext = extension.read().await;
                                            if let Ok(stats) = ext.close_session(&sid).await {
                                                let msg = ServerMessage::SessionClosed {
                                                    session_id: sid.clone(),
                                                    total_frames: stats.input_chunks,
                                                    duration_ms: (chrono::Utc::now()
                                                        .timestamp_millis()
                                                        - stats.last_activity)
                                                        as u64,
                                                    stats: SessionStatsDto::from(&stats),
                                                };
                                                if let Ok(json) = serde_json::to_string(&msg) {
                                                    let _ = ws_out_tx.send(WsMessage::Text(json));
                                                }
                                            }

                                            drop(ws_out_tx); // Signal ws_task to stop
                                            let _ = ws_task.await;

                                            tracing::info!(
                                                "Extension stream disconnected for: {}",
                                                extension_id
                                            );
                                            return; // Exit handle_stream_socket entirely
                                        }

                                        drop(ext);

                                        // Track locally
                                        session_id = Some(sid.clone());

                                        // Send session created
                                        send_message(
                                            &mut socket,
                                            &ServerMessage::SessionCreated {
                                                session_id: sid.clone(),
                                                server_time: chrono::Utc::now().timestamp_millis(),
                                                binary: binary_push_session,
                                            },
                                        )
                                        .await;

                                        tracing::debug!(
                                            "Session created: {} for extension: {} (mode: {:?})",
                                            sid,
                                            extension_id,
                                            cap.mode
                                        );
                                    } else {
                                        send_error(
                                            &mut socket,
                                            "INVALID_MODE",
                                            "Cannot init session in stateless mode".to_string(),
                                        )
                                        .await;
                                    }
                                }
                                ClientMessage::Close => {
                                    tracing::debug!("Client requested close");
                                    break;
                                }
                                ClientMessage::Ack { sequence } => {
                                    // Handle acknowledgment for flow control
                                    tracing::debug!("Received ack for sequence: {}", sequence);
                                }
                            }
                        }
                    }
                    WsMessage::Binary(data) => {
                        // Process binary chunk
                        let (sequence, chunk_data) = match parse_binary_frame(data) {
                            Some((s, d)) => (s, d),
                            None => {
                                send_error(
                                    &mut socket,
                                    "INVALID_FRAME",
                                    "Invalid binary frame format".to_string(),
                                )
                                .await;
                                continue;
                            }
                        };

                        let ext = extension.read().await;
                        let start = std::time::Instant::now();

                        // Create data chunk
                        let data_type = StreamDataType::Binary;
                        let chunk = DataChunk {
                            sequence,
                            data_type: data_type.clone(),
                            data: chunk_data,
                            timestamp: chrono::Utc::now().timestamp_millis(),
                            metadata: None,
                            is_last: false,
                        };

                        // Before executing, check safety manager
                        if !safety_manager.is_allowed(&extension_id).await {
                            send_error(
                                &mut socket,
                                "EXTENSION_DISABLED",
                                format!(
                                    "Extension '{}' is temporarily disabled by safety policy",
                                    extension_id
                                ),
                            )
                            .await;
                            continue;
                        }

                        let result = if let Some(ref sid) = session_id {
                            // Stateful processing with timeout
                            tokio::time::timeout(
                                std::time::Duration::from_secs(30),
                                ext.process_session_chunk(sid, chunk),
                            )
                            .await
                            .map_err(|_| StreamError {
                                code: "TIMEOUT".to_string(),
                                message: "Session chunk processing timed out".to_string(),
                                retryable: true,
                            })
                        } else {
                            // Stateless processing with timeout
                            tokio::time::timeout(
                                std::time::Duration::from_secs(30),
                                ext.process_chunk(chunk),
                            )
                            .await
                            .map_err(|_| StreamError {
                                code: "TIMEOUT".to_string(),
                                message: "Chunk processing timed out".to_string(),
                                retryable: true,
                            })
                        };

                        let processing_ms = start.elapsed().as_secs_f32() * 1000.0;

                        match result {
                            Ok(Ok(stream_result)) => {
                                // Record success with safety manager
                                safety_manager.record_success(&extension_id).await;
                                output_sequence = output_sequence.wrapping_add(1);
                                send_message(
                                    &mut socket,
                                    &ServerMessage::Result {
                                        input_sequence: stream_result.input_sequence,
                                        output_sequence: stream_result.output_sequence,
                                        data: BASE64_STANDARD.encode(&stream_result.data),
                                        data_type: stream_result.data_type.mime_type(),
                                        processing_ms,
                                        metadata: stream_result.metadata,
                                    },
                                )
                                .await;

                                // Check for error in result
                                if let Some(err) = stream_result.error {
                                    tracing::warn!(
                                        "Stream processing error: {} - {}",
                                        err.code,
                                        err.message
                                    );
                                }
                            }
                            Ok(Err(e)) => {
                                // Logical failure from extension
                                safety_manager.record_failure(&extension_id).await;
                                send_error(
                                    &mut socket,
                                    "PROCESSING_ERROR",
                                    format!("Failed to process chunk: {}", e),
                                )
                                .await;
                            }
                            Err(_timeout_err) => {
                                // Timeout wrapper already converted into StreamError
                                safety_manager.record_failure(&extension_id).await;
                                send_error(
                                    &mut socket,
                                    "PROCESSING_TIMEOUT",
                                    "Stream processing timed out".to_string(),
                                )
                                .await;
                            }
                        }
                    }
                    WsMessage::Ping(data) => {
                        // Respond with pong (with timeout — see send_with_timeout)
                        match send_with_timeout(&mut socket, WsMessage::Pong(data)).await {
                            Ok(_) => {}
                            Err(_) => {
                                tracing::info!("Pong send timeout (2s) — non-push connection");
                                break;
                            }
                        }
                    }
                    WsMessage::Pong(_) => {
                        // Received pong, ignore
                    }
                    WsMessage::Close(_) => {
                        tracing::debug!("Client disconnected");
                        break;
                    }
                }
            }
            Some(Err(e)) => {
                tracing::error!("WebSocket error: {}", e);
                break;
            }
            None => {
                // socket.recv() returned None - connection closed
                break;
            }
        }
    }

    // Cleanup session
    if let Some(sid) = session_id {
        // For Push mode: stop pushing and clean up routing
        if cap.mode == StreamMode::Push {
            // Do NOT abort the shared IPC→router forwarder task.
            // It serves all sessions — only unregister this session from the router.
            // The forwarder will naturally stop when the extension process exits.

            let ext = extension.read().await;
            if let Err(e) = ext.stop_push(&sid).await {
                tracing::warn!("Failed to stop push for session {}: {}", sid, e);
            }
            push_router.unregister(&sid).await;

            // Do NOT replace push_output_channel with a dummy channel.
            // Other active sessions' forwarders may still be reading from it.
            // The router.unregister above ensures this session's messages are dropped.
        }

        let ext = extension.read().await;
        if let Ok(stats) = ext.close_session(&sid).await {
            send_message(
                &mut socket,
                &ServerMessage::SessionClosed {
                    session_id: sid.clone(),
                    total_frames: stats.input_chunks,
                    duration_ms: (chrono::Utc::now().timestamp_millis() - stats.last_activity)
                        as u64,
                    stats: SessionStatsDto::from(&stats),
                },
            )
            .await;
        }
    }

    tracing::info!("Extension stream disconnected for: {}", extension_id);
}

/// Parse binary frame: [sequence: u64 (8 bytes, big endian)][data...]
fn parse_binary_frame(mut data: Vec<u8>) -> Option<(u64, Vec<u8>)> {
    if data.len() < 8 {
        return None;
    }

    // Extract sequence number (big endian)
    let seq_bytes = data[..8].try_into().ok()?;
    let sequence = u64::from_be_bytes(seq_bytes);

    // Extract data
    data.drain(..8);
    Some((sequence, data))
}

// ============================================================================
// Outbound Binary Push Frames
// ============================================================================

/// Outbound binary frame kind: push output (first header byte).
const BINARY_FRAME_KIND_PUSH: u8 = 1;
/// Outbound binary frame format version (second header byte).
const BINARY_FRAME_VERSION: u8 = 1;
/// Outbound binary frame header size: kind(1) + version(1) + seq(8) + meta_len(4).
const BINARY_FRAME_HEADER_LEN: usize = 14;

/// Extract the client's binary-frame opt-in from `init` config.
///
/// Missing, malformed, or non-boolean values all fall back to `false` so that
/// legacy clients (and legacy servers paired with new clients) keep the
/// Text+base64 behavior byte-for-byte.
fn binary_requested(config: &Option<serde_json::Value>) -> bool {
    config
        .as_ref()
        .and_then(|c| c.get("binary"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Encode a push output for the wire.
///
/// All push paths must go through here so the two formats can never diverge:
/// - `binary == true`: one WS Binary frame, payload is raw bytes (no base64).
/// - `binary == false`: legacy Text JSON with base64 `data` (pre-negotiation
///   behavior, unchanged).
fn encode_push_output(output: &PushOutputMessage, binary: bool) -> WsMessage {
    if binary {
        return encode_binary_push_frame(output);
    }
    let msg = ServerMessage::PushOutput {
        session_id: output.session_id.clone(),
        sequence: output.sequence,
        data: BASE64_STANDARD.encode(&output.data),
        data_type: output.data_type.clone(),
        timestamp: output.timestamp,
        metadata: output.metadata.clone(),
    };
    WsMessage::Text(serde_json::to_string(&msg).unwrap_or_default())
}

/// Build an outbound binary push frame:
///
/// ```text
/// [kind: u8][version: u8][sequence: u64 BE][meta_len: u32 BE][meta JSON][payload]
/// ```
///
/// `meta` mirrors the Text `push_output` envelope minus `data`/`sequence`.
fn encode_binary_push_frame(output: &PushOutputMessage) -> WsMessage {
    let meta = serde_json::json!({
        "session_id": output.session_id,
        "data_type": output.data_type,
        "timestamp": output.timestamp,
        "metadata": output.metadata,
    });
    // Serialization of this plain object cannot fail; `{}` is a safe fallback.
    let meta_bytes = serde_json::to_vec(&meta).unwrap_or_else(|_| b"{}".to_vec());
    let mut frame =
        Vec::with_capacity(BINARY_FRAME_HEADER_LEN + meta_bytes.len() + output.data.len());
    frame.push(BINARY_FRAME_KIND_PUSH);
    frame.push(BINARY_FRAME_VERSION);
    frame.extend_from_slice(&output.sequence.to_be_bytes());
    frame.extend_from_slice(&(meta_bytes.len() as u32).to_be_bytes());
    frame.extend_from_slice(&meta_bytes);
    frame.extend_from_slice(&output.data);
    WsMessage::Binary(frame)
}

/// Decode an outbound binary push frame. Returns
/// `(kind, version, sequence, meta_bytes, payload_bytes)`.
///
/// Mirror of the browser-side parser; kept test-only until a consumer needs it.
#[cfg(test)]
fn decode_binary_push_frame(frame: &[u8]) -> Option<(u8, u8, u64, &[u8], &[u8])> {
    if frame.len() < BINARY_FRAME_HEADER_LEN {
        return None;
    }
    let kind = frame[0];
    let version = frame[1];
    let seq = u64::from_be_bytes(frame[2..10].try_into().ok()?);
    let meta_len = u32::from_be_bytes(frame[10..14].try_into().ok()?) as usize;
    let meta_end = BINARY_FRAME_HEADER_LEN.checked_add(meta_len)?;
    if meta_end > frame.len() {
        return None;
    }
    Some((kind, version, seq, &frame[14..meta_end], &frame[meta_end..]))
}

/// Send error message to client
async fn send_error(socket: &mut WebSocket, code: &str, message: String) {
    let msg = ServerMessage::Error {
        code: code.to_string(),
        message,
        retryable: false,
    };
    send_message(socket, &msg).await;
}

/// Send message to client
async fn send_message(socket: &mut WebSocket, msg: &ServerMessage) {
    if let Ok(json) = serde_json::to_string(msg) {
        let _ = send_with_timeout(socket, axum::extract::ws::Message::Text(json)).await;
    }
}

/// Maximum time to wait for a WebSocket send to complete.
///
/// Background: when a client tab is hidden or its JS main thread is stalled,
/// the browser stops draining its WS receive buffer → TCP receive window
/// closes → no ACKs → sender's TCP buffer fills → `socket.send().await`
/// blocks forever waiting for writable. This 2s timeout turns a permanently
/// stuck connection into a dead one that the client auto-reconnects from.
const WS_SEND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Wrap `socket.send(...)` with a deadline. Returns `Err(Elapsed)` if the send
/// did not complete within [`WS_SEND_TIMEOUT`].
async fn send_with_timeout(
    socket: &mut WebSocket,
    msg: axum::extract::ws::Message,
) -> Result<(), tokio::time::error::Elapsed> {
    // Map `Result<Result<(), axum::Error>, Elapsed>` → `Result<(), Elapsed>`.
    // The inner axum::Error (send failure) is treated as success here — callers
    // only care whether the deadline was exceeded. A real send error surfaces
    // on the next recv() as None / Err, which the I/O loop already handles by
    // returning.
    tokio::time::timeout(WS_SEND_TIMEOUT, socket.send(msg))
        .await
        .map(|_| ())
}

/// Maximum time the push loop will wait for a new frame from the extension
/// pump before declaring the source stalled.
///
/// This is distinct from [`WS_SEND_TIMEOUT`]: a stalled extension pump never
/// produces a frame, so the send timeout never fires. Without this guard a
/// dead FFmpeg/YOLO pump would keep the session alive forever, holding the
/// client in a "permanently frozen stream" state until manual refresh.
/// 30s is chosen to comfortably exceed the slowest legitimate gap (e.g. RTSP
/// reconnect timeout, model warmup) while still bounded.
const PUSH_STALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

// ============================================================================
// HTTP Endpoints (for capability checking without WebSocket)
// ============================================================================

/// GET /api/extensions/:id/stream/capability
///
/// Get streaming capability without establishing WebSocket connection.
pub async fn get_stream_capability_handler(
    State(state): State<ServerState>,
    Path(extension_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let extension = state
        .extensions
        .runtime
        .get_extension(&extension_id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", extension_id)))?;

    let ext_read = extension.read().await;
    let capability = ext_read.stream_capability();

    match capability {
        Some(cap) => {
            let cap_dto = StreamCapabilityDto::from(&cap);
            match serde_json::to_value(cap_dto) {
                Ok(v) => ok(v),
                Err(e) => {
                    tracing::error!(error = %e, "Failed to serialize stream capability");
                    ok(serde_json::json!({
                        "error": "INTERNAL_ERROR",
                        "message": "Failed to serialize stream capability"
                    }))
                }
            }
        }
        None => ok(serde_json::json!({
            "error": "NOT_SUPPORTED",
            "message": "Extension does not support streaming"
        })),
    }
}

/// GET /api/extensions/:id/stream/sessions
///
/// List active stream sessions for an extension.
pub async fn list_stream_sessions_handler(
    State(state): State<ServerState>,
    Path(extension_id): Path<String>,
) -> HandlerResult<Vec<serde_json::Value>> {
    let sessions = state
        .extensions
        .runtime
        .get_active_sessions(&extension_id)
        .await;

    let result: Vec<serde_json::Value> = sessions
        .into_iter()
        .map(|session_id| {
            serde_json::json!({
                "session_id": session_id,
                "extension_id": extension_id,
                "status": "active",
            })
        })
        .collect();

    ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_binary_frame() {
        // Valid frame
        let data = vec![0u8, 0, 0, 0, 0, 0, 0, 42, 1, 2, 3, 4, 5];
        let (seq, payload) = parse_binary_frame(data).unwrap();
        assert_eq!(seq, 42);
        assert_eq!(payload, vec![1, 2, 3, 4, 5]);

        // Too short
        let data = vec![1, 2, 3];
        assert!(parse_binary_frame(data).is_none());
    }

    #[test]
    fn test_capability_dto() {
        let cap = StreamCapability {
            direction: StreamDirection::Upload,
            mode: StreamMode::Stateless,
            supported_data_types: vec![
                StreamDataType::Image {
                    format: "jpeg".into(),
                },
                StreamDataType::Image {
                    format: "png".into(),
                },
            ],
            max_chunk_size: 1024 * 1024,
            preferred_chunk_size: 64 * 1024,
            max_concurrent_sessions: 5,
            flow_control: Default::default(),
            config_schema: None,
        };

        let dto = StreamCapabilityDto::from(&cap);
        assert_eq!(dto.direction, "upload");
        assert_eq!(dto.mode, "stateless");
        assert_eq!(dto.supported_data_types.len(), 2);
        assert!(dto.supported_data_types.contains(&"image/jpeg".to_string()));
    }

    fn sample_push_output() -> PushOutputMessage {
        PushOutputMessage {
            session_id: "sess-1".to_string(),
            sequence: 42,
            data: vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3],
            data_type: "image/jpeg".to_string(),
            timestamp: 1_725_000_000_000,
            metadata: Some(serde_json::json!({"seq_hint": "k"})),
        }
    }

    #[test]
    fn test_binary_requested_negotiation_matrix() {
        // Missing config / no binary key → legacy Text mode
        assert!(!binary_requested(&None));
        assert!(!binary_requested(&Some(serde_json::json!({}))));

        // Explicit opt-in
        assert!(binary_requested(&Some(serde_json::json!({"binary": true}))));

        // Wrong types and explicit false must NOT enable binary — a malformed
        // flag silently downgrading to Text is the safe direction.
        assert!(!binary_requested(&Some(serde_json::json!({"binary": false}))));
        assert!(!binary_requested(&Some(serde_json::json!({"binary": "yes"}))));
        assert!(!binary_requested(&Some(serde_json::json!({"binary": 1}))));
    }

    #[test]
    fn test_encode_binary_push_frame_roundtrip() {
        let output = sample_push_output();
        let WsMessage::Binary(frame) = encode_push_output(&output, true) else {
            panic!("expected binary frame");
        };

        let (kind, version, seq, meta_bytes, payload) =
            decode_binary_push_frame(&frame).expect("frame must decode");
        assert_eq!(kind, BINARY_FRAME_KIND_PUSH);
        assert_eq!(version, BINARY_FRAME_VERSION);
        assert_eq!(seq, 42);
        assert_eq!(payload, output.data);

        // Meta mirrors the Text envelope minus data/sequence
        let meta: serde_json::Value = serde_json::from_slice(meta_bytes).unwrap();
        assert_eq!(meta["session_id"], "sess-1");
        assert_eq!(meta["data_type"], "image/jpeg");
        assert_eq!(meta["timestamp"], serde_json::json!(1_725_000_000_000i64));
        assert_eq!(meta["metadata"]["seq_hint"], "k");
        assert!(meta.get("data").is_none());
        assert!(meta.get("sequence").is_none());
    }

    #[test]
    fn test_decode_binary_push_frame_rejects_malformed() {
        // Too short for the fixed header
        assert!(decode_binary_push_frame(&[0u8; 13]).is_none());

        // meta_len pointing past the end of the frame
        let output = sample_push_output();
        let WsMessage::Binary(frame) = encode_binary_push_frame(&output) else {
            panic!("expected binary frame");
        };
        let mut forged = frame[..BINARY_FRAME_HEADER_LEN].to_vec();
        // Header only, meta_len claims the full meta + payload but bytes absent
        forged[10..14].copy_from_slice(&1_000u32.to_be_bytes());
        assert!(decode_binary_push_frame(&forged).is_none());
    }

    #[test]
    fn test_encode_push_output_text_legacy_unchanged() {
        let output = sample_push_output();
        let WsMessage::Text(json) = encode_push_output(&output, false) else {
            panic!("expected text frame");
        };

        // Wire format must stay byte-compatible with pre-negotiation behavior
        let msg: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(msg["type"], "push_output");
        assert_eq!(msg["session_id"], "sess-1");
        assert_eq!(msg["sequence"], 42);
        assert_eq!(msg["data_type"], "image/jpeg");
        assert_eq!(
            msg["data"],
            BASE64_STANDARD.encode(&output.data).as_str()
        );
    }
}
