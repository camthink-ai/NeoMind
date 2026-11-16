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
#[derive(utoipa::ToSchema, Debug, Clone, Deserialize, Serialize)]
pub struct ChatImage {
    /// Base64-encoded image data with data URL scheme (e.g., "data:image/png;base64,...")
    pub data: String,
    /// MIME type (e.g., "image/png", "image/jpeg")
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// A structured object reference attached to a chat message (@-mention).
#[derive(utoipa::ToSchema, Debug, Clone, Deserialize, serde::Serialize)]
pub struct ChatReference {
    /// One of: device | rule | transform | agent.
    #[serde(rename = "type")]
    pub r#type: String,
    /// Authoritative object id.
    pub id: String,
    /// Display-only label at selection time; may be stale after renames.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Ordered content node for the v1 inline-@ protocol (plan 1.3 iter C).
///
/// `kind` is `"text"` or `"reference"`. Text nodes carry `text`; reference
/// nodes carry `type`/`id`/`labelSnapshot`. Unknown kinds are rejected.
#[derive(utoipa::ToSchema, Debug, Clone, Deserialize, serde::Serialize)]
pub struct ChatContentNode {
    /// `"text"` | `"reference"`.
    pub kind: String,
    /// Plain text for `kind=text`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Entity type for `kind=reference` (device|rule|transform|agent).
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    /// Authoritative object id for `kind=reference`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Display-only name snapshot at selection time (never trusted for auth).
    #[serde(
        rename = "labelSnapshot",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub label_snapshot: Option<String>,
}

/// Explicit Unknown re-run authorization submit (request-id keyed).
///
/// Host/UI echoes the structured confirm event fields (`requestId`,
/// `priorOperationId`, `writeFingerprint`) — fingerprints may contain spaces
/// and must travel as JSON, never via whitespace-split text parse. Server
/// atomically validates + consumes the pending confirm before granting.
#[derive(utoipa::ToSchema, Debug, Clone, Deserialize, Serialize)]
pub struct AuthorizeNewOperationRequest {
    /// Server-issued confirm request id (required for host submit).
    #[serde(rename = "requestId")]
    pub request_id: String,
    /// Prior Unknown Operation ID to re-run.
    #[serde(rename = "priorOperationId")]
    pub prior_operation_id: String,
    /// Business write fingerprint that must match that Operation.
    #[serde(rename = "writeFingerprint")]
    pub write_fingerprint: String,
}

/// Chat request from the web client.
#[derive(utoipa::ToSchema, Debug, Clone, Deserialize)]
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
    /// Structured object references attached by the composer (@-mentions).
    /// Legacy attachment-chip path: resolved into a bounded preamble.
    /// Prefer `contentNodes` for in-sentence position.
    #[serde(rename = "references", default)]
    pub references: Option<Vec<ChatReference>>,
    /// Protocol version for `contentNodes` (currently only `1`). Omitted with
    /// no nodes = legacy plain/`references` path.
    #[serde(rename = "contentVersion", default)]
    pub content_version: Option<u32>,
    /// Ordered text/reference nodes (inline @). When present and non-empty,
    /// the server renders references **in place** into the model message
    /// (ids + server-resolved names). Caps are hard errors (no silent drop).
    #[serde(rename = "contentNodes", default)]
    pub content_nodes: Option<Vec<ChatContentNode>>,
    /// Optional per-session config overrides. Honored **only** when this
    /// request triggers creation of a new session (no `sessionId`, or
    /// `sessionId` not found). For an existing session this field is
    /// silently ignored — the session keeps the config it was created with.
    /// Use this for things like the voice-assistant baking in a system
    /// prompt without polluting every user message via `pageContext`.
    #[serde(rename = "sessionConfig", default)]
    pub session_config: Option<SessionConfigPatch>,
    /// Host/UI Unknown re-run authorization target. When set, binds the
    /// one-shot grant to this Operation (required when multiple Unknowns
    /// exist). Bare phrase shortcuts still work for a unique Unknown.
    #[serde(rename = "authorizeNewOperation", default)]
    pub authorize_new_operation: Option<AuthorizeNewOperationRequest>,
    /// Client-supplied correlation id for a receive-ack (`MessageReceived`).
    /// Older clients omit it — the server then emits no ack frame. Used by
    /// the composer draft lifecycle: local `ws.send` / queue success is NOT
    /// a server accept. Accepted is emitted only AFTER `prepare_chat_input`
    /// succeeds; prepare/validation failures reject with the same id.
    #[serde(rename = "clientRequestId", default)]
    pub client_request_id: Option<String>,
}

/// Patch-style session config: every field is optional, and `None` means
/// "inherit the platform default". Translated into a
/// `CreateSessionOptions` on the agent side. Field names use camelCase to
/// match the JS/TS client convention expected by NeoMind's REST/WS API.
#[derive(utoipa::ToSchema, Debug, Clone, Default, Deserialize)]
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
#[derive(utoipa::ToSchema, Debug, Deserialize)]
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

// ---------------------------------------------------------------------------
// OpenAPI mirrors of cross-crate DTO field types.
//
// utoipa 4 names refs to FOREIGN field types "crate.Type" but registers the
// same type under its bare name — the ref can never resolve, so codegen
// clients break. Mirroring the two affected small types here (kept in sync
// with their originals) gives the fields a resolvable local schema.
// ---------------------------------------------------------------------------

/// OpenAPI mirror of `neomind_core::ThinkingEffort`.
#[derive(
    utoipa::ToSchema, Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize,
)]
pub enum ThinkingEffortMirror {
    None,
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

/// OpenAPI mirror of `neomind_storage::AgentToolConfig`.
#[derive(utoipa::ToSchema, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentToolConfigMirror {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
}

fn default_true() -> bool {
    true
}
