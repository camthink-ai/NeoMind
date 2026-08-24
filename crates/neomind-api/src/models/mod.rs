//! Request and response models for the web API.

use serde::{Deserialize, Serialize};

pub mod common;
pub mod error;
pub mod pagination;

pub use common::{ApiError, ApiResponse, ErrorCode, ResponseMeta, ToApiResponse};
pub use error::{ApiResult, ErrorResponse};
pub use pagination::{
    paginated, PaginatedResponse, Pagination, PaginationMeta, PaginationParams, DEFAULT_PAGE_SIZE,
    MAX_PAGE_SIZE,
};

// ============================================================================
// Chat & Session Models
// ============================================================================

/// Image data in a chat message (for multimodal LLMs).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatImage {
    /// Base64-encoded image data with data URL scheme (e.g., "data:image/png;base64,...")
    pub data: String,
    /// MIME type (e.g., "image/png", "image/jpeg")
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// Chat request from the web client.
#[derive(Debug, Clone, Deserialize)]
pub struct ChatRequest {
    /// The user's message text.
    pub message: String,
    /// Optional images for multimodal models (base64 data URLs).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<ChatImage>>,
    /// Optional session ID.
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
    /// Optional LLM backend ID to use for this request.
    #[serde(rename = "backendId")]
    pub backend_id: Option<String>,
    /// Optional skill IDs pinned by the user for this session.
    /// `None` (field omitted) = leave pins untouched; `Some(vec![])` =
    /// explicit clear (user unpinned everything). The old `Vec` type made
    /// "omitted" and "cleared" indistinguishable, so unpinning all skills
    /// silently kept injecting the stale pins every turn.
    #[serde(rename = "selectedSkills", default)]
    pub selected_skills: Option<Vec<String>>,
    /// Optional page context for first message (short neutral description of current page).
    #[serde(rename = "pageContext", default)]
    pub page_context: Option<String>,
    /// Optional per-session config overrides. Honored **only** when this
    /// request triggers creation of a new session (no `sessionId`, or
    /// `sessionId` not found). For an existing session this field is
    /// silently ignored — the session keeps the config it was created with.
    /// Use this for things like the voice-assistant baking in a system
    /// prompt without polluting every user message via `pageContext`.
    #[serde(rename = "sessionConfig", default)]
    pub session_config: Option<SessionConfigPatch>,
}

/// Patch-style session config: every field is optional, and `None` means
/// "inherit the platform default". Translated into a
/// `CreateSessionOptions` on the agent side. Field names use camelCase to
/// match the JS/TS client convention expected by NeoMind's REST/WS API.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SessionConfigPatch {
    /// Override the agent's system prompt.
    #[serde(rename = "systemPrompt")]
    pub system_prompt: Option<String>,
    /// Append to the agent's (default) system prompt — page-scoped focus
    /// without replacing the platform base. Ignored when `systemPrompt` is set.
    #[serde(rename = "systemPromptSuffix")]
    pub system_prompt_suffix: Option<String>,
    /// Override the LLM sampling temperature.
    pub temperature: Option<f32>,
    /// Override the model identifier.
    pub model: Option<String>,
    /// Enable or disable tool calling for this session.
    #[serde(rename = "enableTools")]
    pub enable_tools: Option<bool>,
    /// Restrict the session to these tool names (empty = all tools).
    #[serde(rename = "allowedTools", default)]
    pub allowed_tools: Vec<String>,
}

impl From<SessionConfigPatch> for neomind_agent::CreateSessionOptions {
    fn from(p: SessionConfigPatch) -> Self {
        Self {
            system_prompt: p.system_prompt,
            system_prompt_suffix: p.system_prompt_suffix,
            temperature: p.temperature,
            model: p.model,
            enable_tools: p.enable_tools,
            allowed_tools: p.allowed_tools,
        }
    }
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
    /// Thinking content (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
}

/// Create session request.
#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    /// Optional agent configuration (legacy full-struct form; only the four
    /// commonly-overridden fields flow through).
    pub config: Option<AgentConfig>,
    /// Optional session config patch — preferred over `config` when present.
    #[serde(rename = "sessionConfig", default)]
    pub session_config: Option<SessionConfigPatch>,
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

/// Ollama model details.
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct OllamaModelDetails {
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub family: String,
    #[serde(default)]
    pub families: Vec<String>,
    #[serde(default)]
    pub parameter_size: String,
    #[serde(default)]
    pub quantization_level: String,
}

/// Ollama model info.
#[derive(Debug, Deserialize, Serialize)]
pub struct OllamaModel {
    pub name: String,
    #[serde(default)]
    pub modified_at: String,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub details: OllamaModelDetails,
}

/// Model capability info for API responses.
#[derive(Debug, Serialize, Default)]
pub struct ModelCapabilities {
    pub supports_thinking: bool,
    pub supports_tools: bool,
    pub supports_multimodal: bool,
}

// ============================================================================
// Agent Models (re-export from neomind_agent)
// ============================================================================

pub use neomind_agent::AgentConfig;

// ============================================================================
// WebSocket Message Models
// ============================================================================

/// WebSocket message from client.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WsClientMessage {
    /// Chat message.
    Chat {
        message: String,
        #[serde(rename = "sessionId")]
        session_id: Option<String>,
    },
    /// Ping message.
    Ping,
}

/// WebSocket message to client.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WsServerMessage {
    /// Chat response.
    Chat {
        response: String,
        #[serde(rename = "sessionId")]
        session_id: String,
    },
    /// Error message.
    Error { error: String },
    /// Pong message.
    Pong,
}
