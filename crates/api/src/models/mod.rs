//! Request and response models for the web API.

use serde::{Deserialize, Serialize};

pub mod common;
pub mod error;
pub mod pagination;

pub use common::{ApiResponse, ApiError, ErrorCode, ResponseMeta, ToApiResponse};
pub use error::{ErrorResponse, ApiResult};
pub use pagination::{Pagination, PaginationParams, PaginatedResponse, PaginationMeta, paginated, DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE};


// ============================================================================
// Chat & Session Models
// ============================================================================

/// Chat request from the web client.
#[derive(Debug, Clone, Deserialize)]
pub struct ChatRequest {
    /// The user's message.
    pub message: String,
    /// Optional session ID.
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
}

/// Chat response to the web client.
#[derive(Debug, Clone, Serialize)]
pub struct ChatResponse {
    /// The assistant's response.
    pub response: String,
    /// Session ID.
    #[serde(rename = "sessionId")]
    pub session_id: String,
    /// Tools used.
    #[serde(rename = "toolsUsed")]
    pub tools_used: Vec<String>,
    /// Processing time in milliseconds.
    #[serde(rename = "processingTimeMs")]
    pub processing_time_ms: u64,
}

/// Create session request.
#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    /// Optional agent configuration.
    pub config: Option<AgentConfig>,
}

/// Create session response.
#[derive(Debug, Serialize)]
pub struct CreateSessionResponse {
    /// The created session ID.
    #[serde(rename = "sessionId")]
    pub session_id: String,
}

/// Session info response.
#[derive(Debug, Serialize)]
pub struct SessionInfo {
    /// Session ID.
    #[serde(rename = "sessionId")]
    pub session_id: String,
    /// Number of messages in the session.
    #[serde(rename = "messageCount")]
    message_count: usize,
    /// Whether the session is active.
    active: bool,
}

/// Session history response.
#[derive(Debug, Serialize)]
pub struct SessionHistoryResponse {
    /// Session ID.
    #[serde(rename = "sessionId")]
    pub session_id: String,
    /// Messages in the session.
    messages: Vec<MessageInfo>,
}

/// Message info.
#[derive(Debug, Serialize)]
pub struct MessageInfo {
    /// Message role.
    role: String,
    /// Message content (truncated).
    content: String,
    /// Message timestamp.
    timestamp: i64,
}

// ============================================================================
// Rules Models
// ============================================================================

/// Create rule request.
#[derive(Debug, Deserialize)]
pub struct CreateRuleRequest {
    /// Rule name.
    pub name: String,
    /// Rule DSL definition.
    pub definition: String,
}

/// Rule info response.
#[derive(Debug, Serialize)]
pub struct RuleInfo {
    /// Rule ID.
    pub id: String,
    /// Rule name.
    pub name: String,
    /// Rule definition (truncated).
    pub definition: String,
    /// Whether the rule is enabled.
    pub enabled: bool,
}

// ============================================================================
// Alerts Models
// ============================================================================

/// Create alert request.
#[derive(Debug, Deserialize)]
pub struct CreateAlertRequest {
    /// Alert name.
    pub name: String,
    /// Alert condition.
    pub condition: String,
    /// Severity level.
    #[serde(default = "default_alert_severity")]
    pub severity: String,
}

fn default_alert_severity() -> String {
    "warning".to_string()
}

/// Alert info response.
#[derive(Debug, Serialize)]
pub struct AlertInfo {
    /// Alert ID.
    pub id: String,
    /// Alert name.
    pub name: String,
    /// Alert condition.
    pub condition: String,
    /// Severity level.
    pub severity: String,
    /// Whether the alert is active.
    pub active: bool,
    /// Whether the alert is acknowledged.
    pub acknowledged: bool,
}

// ============================================================================
// Settings/LLM Models
// ============================================================================

/// Ollama model response.
#[derive(Debug, Deserialize)]
pub struct OllamaModelsResponse {
    pub models: Vec<OllamaModel>,
}

/// Ollama model info.
#[derive(Debug, Deserialize, Serialize)]
pub struct OllamaModel {
    pub name: String,
    #[serde(default)]
    pub modified_at: String,
    #[serde(default)]
    pub size: Option<u64>,
}

// ============================================================================
// Agent Models (re-export from edge_ai_agent)
// ============================================================================

pub use edge_ai_agent::AgentConfig;

// ============================================================================
// WebSocket Message Models
// ============================================================================

/// WebSocket message from client.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WsClientMessage {
    /// Chat message.
    Chat { message: String, #[serde(rename = "sessionId")] session_id: Option<String> },
    /// Ping message.
    Ping,
}

/// WebSocket message to client.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WsServerMessage {
    /// Chat response.
    Chat { response: String, #[serde(rename = "sessionId")] session_id: String },
    /// Error message.
    Error { error: String },
    /// Pong message.
    Pong,
}
