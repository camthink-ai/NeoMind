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
//! Binary frames use the following format:
//! ```text
//! [sequence: u64 (8 bytes, big endian)][data...]
//! ```
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
    extract::{
        Path, State, WebSocketUpgrade,
    },
    extract::ws::{Message as WsMessage, WebSocket},
    response::IntoResponse,
};
use futures::sink::SinkExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc, broadcast};
use uuid::Uuid;
use base64::prelude::*;

use crate::handlers::common::{HandlerResult, ok};
use crate::models::error::ErrorResponse;
use crate::server::ServerState;
use neomind_core::extension::{
    DataChunk, StreamCapability, StreamDataType, StreamDirection, StreamError,
    StreamResult, StreamMode, StreamSession, SessionStats, PushOutputMessage,
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
    Init {
        config: Option<serde_json::Value>,
    },
    /// Close connection/session
    Close,
    /// Flow control acknowledgment
    Ack {
        sequence: u64,
    },
}

/// Message type to client
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMessage {
    /// Stream capability
    Capability {
        capability: StreamCapabilityDto,
    },
    /// Session created (stateful mode)
    SessionCreated {
        session_id: String,
        server_time: i64,
    },
    /// Processing result
    Result {
        input_sequence: Option<u64>,
        output_sequence: u64,
        data: String,  // base64 encoded
        data_type: String,
        processing_ms: f32,
        metadata: Option<serde_json::Value>,
    },
    /// Extension-pushed output (Push mode)
    PushOutput {
        session_id: String,
        sequence: u64,
        data: String,  // base64 encoded
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
    /// Heartbeat
    Heartbeat {
        timestamp: i64,
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
            supported_data_types: cap.supported_data_types.iter()
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
// Session Manager
// ============================================================================

/// Active session data
#[derive(Debug, Clone)]
struct ActiveSession {
    id: String,
    extension_id: String,
    config: serde_json::Value,
    client_info: ClientInfoMessage,
    created_at: i64,
    frame_count: Arc<std::sync::atomic::AtomicU64>,
    stats: Arc<RwLock<SessionStats>>,
}

/// Session manager for tracking active stream sessions
struct SessionManager {
    sessions: Arc<RwLock<HashMap<String, ActiveSession>>>,
}

impl SessionManager {
    fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn create(
        &self,
        id: String,
        extension_id: String,
        config: serde_json::Value,
        client_info: ClientInfoMessage,
    ) -> Result<(), ActiveSession> {
        let mut sessions = self.sessions.write().await;

        if sessions.contains_key(&id) {
            return Err(ActiveSession {
                id: id.clone(),
                extension_id,
                config,
                client_info,
                created_at: 0,
                frame_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
                stats: Arc::new(RwLock::new(SessionStats::default())),
            });
        }

        let session = ActiveSession {
            id: id.clone(),
            extension_id,
            config,
            client_info,
            created_at: chrono::Utc::now().timestamp_millis(),
            frame_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            stats: Arc::new(RwLock::new(SessionStats::default())),
        };

        sessions.insert(id.clone(), session);
        Ok(())
    }

    async fn get(&self, id: &str) -> Option<ActiveSession> {
        let sessions = self.sessions.read().await;
        sessions.get(id).cloned()
    }

    async fn remove(&self, id: &str) -> Option<ActiveSession> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(id)
    }

    async fn count(&self) -> usize {
        let sessions = self.sessions.read().await;
        sessions.len()
    }

    /// Cleanup inactive sessions (older than timeout_ms)
    async fn cleanup_inactive(&self, timeout_ms: i64) {
        let mut to_remove = vec![];
        let now = chrono::Utc::now().timestamp_millis();

        {
            let sessions = self.sessions.read().await;
            for (id, session) in sessions.iter() {
                let stats = session.stats.read().await;
                if now - stats.last_activity > timeout_ms {
                    to_remove.push(id.clone());
                }
            }
        }

        for id in to_remove {
            tracing::info!("Cleaning up inactive session: {}", id);
            self.remove(&id).await;
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
        let mut senders: tokio::sync::RwLockWriteGuard<'_, HashMap<String, mpsc::Sender<PushOutputMessage>>> = self.senders.write().await;
        senders.insert(session_id.clone(), sender);
        tracing::debug!("Registered push output sender for session: {}", session_id);
    }

    /// Unregister a session
    pub async fn unregister(&self, session_id: &str) {
        let mut senders: tokio::sync::RwLockWriteGuard<'_, HashMap<String, mpsc::Sender<PushOutputMessage>>> = self.senders.write().await;
        senders.remove(session_id);
        tracing::debug!("Unregistered push output sender for session: {}", session_id);
    }

    /// Route a push output message to the appropriate session
    pub async fn route(&self, output: PushOutputMessage) -> bool {
        let senders: tokio::sync::RwLockReadGuard<'_, HashMap<String, mpsc::Sender<PushOutputMessage>>> = self.senders.read().await;
        if let Some(sender) = senders.get(&output.session_id) {
            match sender.send(output).await {
                Ok(_) => true,
                Err(_) => {
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
        let senders: tokio::sync::RwLockReadGuard<'_, HashMap<String, mpsc::Sender<PushOutputMessage>>> = self.senders.read().await;
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
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_stream_socket(socket, extension_id, state))
}

async fn handle_stream_socket(
    mut socket: WebSocket,
    extension_id: String,
    state: ServerState,
) {
    tracing::info!("Extension stream connection opened for: {}", extension_id);

    // Get extension and safety manager
    let registry = &state.extensions.registry;
    let unified_service = &state.extensions.unified_service;
    let safety_manager = registry.safety_manager();

    // Try to get extension from registry first, then from isolated manager
    let extension = match registry.get(&extension_id).await {
        Some(ext) => {
            tracing::debug!("Found extension {} in registry", extension_id);
            ext
        }
        None => {
            // Check if it's in the unified service's isolated manager
            tracing::debug!("Extension {} not in registry, checking isolated manager", extension_id);
            
            // For isolated extensions, we need to create a proxy that implements Extension trait
            // and communicates via IPC for streaming operations
            // This is a complex task - for now, return an error
            send_error(&mut socket, "EXTENSION_NOT_FOUND",
                       format!("Extension '{}' not found in registry. Isolated extensions require streaming support via IPC.", extension_id)).await;
            return;
        }
    };

    // Check streaming capability
    let ext_read = extension.read().await;
    let capability = ext_read.stream_capability();
    drop(ext_read);

    let cap = match capability {
        Some(c) => c,
        None => {
            send_error(&mut socket, "NOT_SUPPORTED",
                       "Extension does not support streaming".to_string()).await;
            return;
        }
    };

    // Send capability
    let msg = ServerMessage::Capability {
        capability: StreamCapabilityDto::from(&cap),
    };
    tracing::info!("Sending capability: mode={:?}, direction={:?}", cap.mode, cap.direction);
    send_message(&mut socket, &msg).await;

    // Track session if stateful
    let mut session_id: Option<String> = None;
    let mut output_sequence = 0u64;

    // For Push mode: channel to receive push outputs from extension
    let mut push_rx: Option<mpsc::Receiver<PushOutputMessage>> = None;
    let push_router = get_push_router();

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
                            let push_msg = ServerMessage::PushOutput {
                                session_id: output.session_id,
                                sequence: output.sequence,
                                data: BASE64_STANDARD.encode(&output.data),
                                data_type: output.data_type,
                                timestamp: output.timestamp,
                                metadata: output.metadata,
                            };
                            send_message(&mut socket, &push_msg).await;
                            continue;
                        }
                        None => {
                            // Channel closed
                            tracing::info!("Push output channel closed");
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
                                        tracing::info!("Initializing session {} for extension {}", sid, extension_id);
                                        if let Err(e) = ext.init_session(&session).await {
                                            tracing::error!("Failed to init session: {}", e);
                                            send_error(&mut socket, "SESSION_INIT_FAILED",
                                                       format!("Failed to init session: {}", e)).await;
                                            continue;
                                        }
                                        tracing::info!("Session {} initialized successfully", sid);

                                        // For Push mode: setup output channel
                                        if cap.mode == StreamMode::Push {
                                            // Create channel for push outputs
                                            let (tx, rx) = mpsc::channel::<PushOutputMessage>(32);
                                            push_rx = Some(rx);

                                            // Register with router
                                            push_router.register(sid.clone(), tx).await;

                                            // Set output sender on extension
                                            let output_sender = Arc::new(
                                                create_push_forwarder(sid.clone(), push_router.clone())
                                            );
                                            ext.set_output_sender(output_sender);

                                            // Start pushing
                                            if let Err(e) = ext.start_push(&sid).await {
                                                send_error(&mut socket, "PUSH_START_FAILED",
                                                           format!("Failed to start push: {}", e)).await;
                                                push_router.unregister(&sid).await;
                                                push_rx = None;
                                                continue;
                                            }
                                        }

                                        drop(ext);

                                        // Track locally
                                        session_id = Some(sid.clone());

                                        // Send session created
                                        send_message(&mut socket, &ServerMessage::SessionCreated {
                                            session_id: sid.clone(),
                                            server_time: chrono::Utc::now().timestamp_millis(),
                                        }).await;

                                        tracing::info!("Session created: {} for extension: {} (mode: {:?})",
                                                       sid, extension_id, cap.mode);
                                    } else {
                                        send_error(&mut socket, "INVALID_MODE",
                                                   "Cannot init session in stateless mode".to_string()).await;
                                    }
                                }
                                ClientMessage::Close => {
                                    tracing::info!("Client requested close");
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
                                send_error(&mut socket, "INVALID_FRAME",
                                           "Invalid binary frame format".to_string()).await;
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
                                send_message(&mut socket, &ServerMessage::Result {
                                    input_sequence: stream_result.input_sequence,
                                    output_sequence: stream_result.output_sequence,
                                    data: BASE64_STANDARD.encode(&stream_result.data),
                                    data_type: stream_result.data_type.mime_type(),
                                    processing_ms,
                                    metadata: stream_result.metadata,
                                }).await;

                                // Check for error in result
                                if let Some(err) = stream_result.error {
                                    tracing::warn!("Stream processing error: {} - {}", err.code, err.message);
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
                        // Respond with pong
                        if let Err(e) = socket.send(WsMessage::Pong(data)).await {
                            tracing::error!("Failed to send pong: {}", e);
                            break;
                        }
                    }
                    WsMessage::Pong(_) => {
                        // Received pong, ignore
                    }
                    WsMessage::Close(_) => {
                        tracing::info!("Client disconnected");
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
                tracing::info!("WebSocket connection closed");
                break;
            }
        }
    }

    // Cleanup session
    if let Some(sid) = session_id {
        // For Push mode: stop pushing
        if cap.mode == StreamMode::Push {
            let ext = extension.read().await;
            if let Err(e) = ext.stop_push(&sid).await {
                tracing::warn!("Failed to stop push for session {}: {}", sid, e);
            }
            push_router.unregister(&sid).await;
        }

        let ext = extension.read().await;
        if let Ok(stats) = ext.close_session(&sid).await {
            send_message(&mut socket, &ServerMessage::SessionClosed {
                session_id: sid.clone(),
                total_frames: stats.input_chunks,
                duration_ms: (chrono::Utc::now().timestamp_millis() - stats.last_activity) as u64,
                stats: SessionStatsDto::from(&stats),
            }).await;
        }
    }

    tracing::info!("Extension stream disconnected for: {}", extension_id);
}

/// Create a push forwarder that sends PushOutputMessage to the router
fn create_push_forwarder(
    session_id: String,
    router: Arc<PushOutputRouter>,
) -> mpsc::Sender<PushOutputMessage> {
    let (tx, mut rx) = mpsc::channel::<PushOutputMessage>(64);

    tokio::spawn(async move {
        while let Some(output) = rx.recv().await {
            // Ensure session_id matches
            let mut output = output;
            output.session_id = session_id.clone();
            router.route(output).await;
        }
        tracing::debug!("Push forwarder stopped for session: {}", session_id);
    });

    tx
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
        let _ = socket.send(axum::extract::ws::Message::Text(json)).await;
    }
}

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
    let extension = state.extensions.registry.get(&extension_id).await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", extension_id)))?;

    let ext_read = extension.read().await;
    let capability = ext_read.stream_capability();

    match capability {
        Some(cap) => {
            let cap_dto = StreamCapabilityDto::from(&cap);
            ok(serde_json::to_value(cap_dto).unwrap())
        }
        None => {
            ok(serde_json::json!({
                "error": "NOT_SUPPORTED",
                "message": "Extension does not support streaming"
            }))
        }
    }
}

/// GET /api/extensions/:id/stream/sessions
///
/// List active stream sessions for an extension.
pub async fn list_stream_sessions_handler(
    State(_state): State<ServerState>,
    Path(_extension_id): Path<String>,
) -> HandlerResult<Vec<serde_json::Value>> {
    // Return empty array for now
    // In production, you'd query the session manager
    ok(vec![])
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
                StreamDataType::Image { format: "jpeg".into() },
                StreamDataType::Image { format: "png".into() },
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
}
