//! First-time setup API handlers.
//!
//! Provides endpoints for initial system setup when no users exist.
//! This allows customers to create their admin account during first launch.

use axum::{extract::State, http::StatusCode, response::Json};
use serde::{Deserialize, Serialize};

use crate::auth_users::UserRole;
use crate::config;
use crate::models::error::ErrorResponse;
use crate::server::ServerState;

/// Basic email validation helper
fn is_valid_email(email: &str) -> bool {
    // Simple email validation: must contain @ and . after @
    if let Some(at_pos) = email.find('@') {
        if at_pos > 0 && at_pos < email.len() - 1 {
            let after_at = &email[at_pos + 1..];
            return after_at.contains('.')
                && !after_at.starts_with('.')
                && !after_at.ends_with('.');
        }
    }
    false
}

/// Setup status response.
#[derive(Debug, Serialize)]
pub struct SetupStatusResponse {
    /// Whether setup is required (no users exist)
    pub setup_required: bool,
    /// Whether this is the first time launching
    pub is_first_launch: bool,
    /// The API version for setup flow
    pub setup_version: &'static str,
}

/// Initialize admin request.
#[derive(Debug, Deserialize)]
pub struct InitializeAdminRequest {
    /// Admin username
    pub username: String,
    /// Admin password
    pub password: String,
    /// Email (optional)
    #[serde(default)]
    pub email: Option<String>,
}

/// Initialize admin response.
#[derive(Debug, Serialize)]
pub struct InitializeAdminResponse {
    /// Success message
    pub message: String,
    /// The created user info
    pub user: AdminUserInfo,
    /// JWT token for immediate login
    pub token: String,
}

/// Created admin user info.
#[derive(Debug, Serialize)]
pub struct AdminUserInfo {
    pub id: String,
    pub username: String,
    pub role: String,
    pub created_at: i64,
}

/// LLM configuration for setup.
#[derive(Debug, Deserialize)]
pub struct LlmConfigRequest {
    /// LLM provider (ollama, openai, anthropic, etc.)
    pub provider: String,
    /// Model name
    pub model: String,
    /// API endpoint (optional, uses default for provider)
    #[serde(default)]
    pub endpoint: Option<String>,
    /// API key (optional, for cloud providers)
    #[serde(default)]
    pub api_key: Option<String>,
}

/// Check setup status.
///
/// Returns whether the system needs initial setup (no users exist).
/// This endpoint is public and used by the frontend to decide whether
/// to show the login page or setup wizard.
pub async fn setup_status_handler(
    State(state): State<ServerState>,
) -> Result<Json<SetupStatusResponse>, ErrorResponse> {
    let users = state.auth.user_state.list_users().await;
    let setup_required = users.is_empty();

    Ok(Json(SetupStatusResponse {
        setup_required,
        is_first_launch: setup_required,
        setup_version: "1.0",
    }))
}

/// Initialize admin account.
///
/// Creates the first admin user. Only available when no users exist.
/// After successful creation, returns a JWT token for immediate login.
///
/// Per-IP throttled (every attempt counts): an unconfigured device exposes
/// this endpoint to the network, and racing to claim the admin account is
/// the highest-value brute-force target on the box.
pub async fn initialize_admin_handler(
    State(state): State<ServerState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    headers: axum::http::HeaderMap,
    Json(req): Json<InitializeAdminRequest>,
) -> Result<Json<InitializeAdminResponse>, ErrorResponse> {
    // Throttle first (pure in-memory check, before any DB reads).
    let ip = crate::auth_users::client_ip_for_throttle(&headers, &addr);
    if let Err(crate::auth_users::AuthError::TooManyAttempts(secs)) = state
        .auth
        .user_state
        .check_signup_throttle("setup", Some(&ip))
    {
        return Err(ErrorResponse {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: "TOO_MANY_ATTEMPTS".to_string(),
            message: format!("Too many setup attempts — try again in {secs} seconds"),
            request_id: None,
            hint: None,
        });
    }
    state
        .auth
        .user_state
        .record_signup_attempt("setup", Some(&ip));

    // Validate that this is truly first-time setup
    let users = state.auth.user_state.list_users().await;
    if !users.is_empty() {
        return Err(ErrorResponse {
            status: StatusCode::FORBIDDEN,
            code: "SETUP_ALREADY_COMPLETED".to_string(),
            message: "System has already been initialized. Use /api/auth/login to sign in."
                .to_string(),
            request_id: None,
            hint: None,
        });
    }

    // Validate username
    if req.username.len() < 3 {
        return Err(ErrorResponse {
            status: StatusCode::BAD_REQUEST,
            code: "INVALID_USERNAME".to_string(),
            message: "Username must be at least 3 characters".to_string(),
            request_id: None,
            hint: None,
        });
    }

    // Validate email format if provided
    if let Some(ref email_addr) = req.email {
        if !email_addr.is_empty() && !is_valid_email(email_addr) {
            return Err(ErrorResponse {
                status: StatusCode::BAD_REQUEST,
                code: "INVALID_EMAIL".to_string(),
                message: "Please provide a valid email address".to_string(),
                request_id: None,
                hint: None,
            });
        }
    }

    // Validate password strength
    if req.password.len() < 8 {
        return Err(ErrorResponse {
            status: StatusCode::BAD_REQUEST,
            code: "WEAK_PASSWORD".to_string(),
            message: "Password must be at least 8 characters".to_string(),
            request_id: None,
            hint: None,
        });
    }

    // Check for password complexity (at least one letter and one number)
    let has_letter = req.password.chars().any(|c| c.is_alphabetic());
    let has_number = req.password.chars().any(|c| c.is_numeric());
    if !has_letter || !has_number {
        return Err(ErrorResponse {
            status: StatusCode::BAD_REQUEST,
            code: "WEAK_PASSWORD".to_string(),
            message: "Password must contain both letters and numbers".to_string(),
            request_id: None,
            hint: None,
        });
    }

    // Create admin user
    let (user_info, token) = state
        .auth
        .user_state
        .register(&req.username, &req.password, UserRole::Admin)
        .await
        .map_err(|e| ErrorResponse {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "USER_CREATE_FAILED".to_string(),
            message: e.to_string(),
            request_id: None,
            hint: None,
        })?;

    tracing::info!(
        category = "setup",
        username = req.username,
        user_id = user_info.id,
        "Admin account created during setup"
    );

    Ok(Json(InitializeAdminResponse {
        message: "Admin account created successfully".to_string(),
        user: AdminUserInfo {
            id: user_info.id,
            username: user_info.username,
            role: user_info.role.as_str().to_string(),
            created_at: user_info.created_at,
        },
        token,
    }))
}

/// Complete setup.
///
/// Marks setup as complete. Called after all setup steps are done.
pub async fn complete_setup_handler(
    State(state): State<ServerState>,
) -> Result<Json<serde_json::Value>, ErrorResponse> {
    // Verify that at least one user exists
    let users = state.auth.user_state.list_users().await;
    if users.is_empty() {
        return Err(ErrorResponse {
            status: StatusCode::BAD_REQUEST,
            code: "SETUP_INCOMPLETE".to_string(),
            message: "Cannot complete setup before creating an admin account".to_string(),
            request_id: None,
            hint: None,
        });
    }

    tracing::info!(category = "setup", "Setup completed successfully");

    Ok(Json(serde_json::json!({
        "message": "Setup completed successfully",
        "redirect": "/dashboard"
    })))
}

/// Save LLM configuration during setup.
///
/// Allows configuring the LLM backend during the setup wizard.
pub async fn save_llm_config_handler(
    State(state): State<ServerState>,
    Json(req): Json<LlmConfigRequest>,
) -> Result<Json<serde_json::Value>, ErrorResponse> {
    // SECURITY: This route is on `public_routes`, so it is reachable
    // without authentication. Gate it by the same "first-time setup only"
    // invariant that `initialize_admin_handler` uses — once any user
    // exists, anonymous callers must not be able to overwrite the LLM
    // backend config (which would let them redirect agent traffic to an
    // attacker-controlled endpoint or exfiltrate API keys).
    let users = state.auth.user_state.list_users().await;
    if !users.is_empty() {
        return Err(ErrorResponse {
            status: StatusCode::FORBIDDEN,
            code: "SETUP_ALREADY_COMPLETED".to_string(),
            message: "LLM config can only be set via the setup endpoint before the first user is created. Use PUT /api/config/llm instead.".to_string(),
            request_id: None,
            hint: None,
        });
    }

    // Validate provider
    let valid_providers = ["ollama", "openai", "anthropic", "google", "xai", "llamacpp"];
    if !valid_providers.contains(&req.provider.as_str()) {
        return Err(ErrorResponse {
            status: StatusCode::BAD_REQUEST,
            code: "INVALID_PROVIDER".to_string(),
            message: format!("Provider must be one of: {}", valid_providers.join(", ")),
            request_id: None,
            hint: None,
        });
    }

    // Validate model (llama.cpp loads model at server startup, so model name is optional)
    if req.model.is_empty() && req.provider != "llamacpp" {
        return Err(ErrorResponse {
            status: StatusCode::BAD_REQUEST,
            code: "INVALID_MODEL".to_string(),
            message: "Model name cannot be empty".to_string(),
            request_id: None,
            hint: None,
        });
    }

    // Save to settings storage
    // Convert LlmConfigRequest to LlmSettingsRequest
    let settings_req = config::LlmSettingsRequest {
        backend: req.provider.clone(),
        endpoint: req.endpoint.clone(),
        model: req.model.clone(),
        api_key: req.api_key.clone(),
    };

    // Convert to LlmSettings and persist to database
    let llm_settings = settings_req.to_llm_settings();
    config::save_llm_settings(&llm_settings)
        .await
        .map_err(|e| ErrorResponse {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "SETTINGS_SAVE_FAILED".to_string(),
            message: format!("Failed to save LLM settings: {}", e),
            request_id: None,
            hint: None,
        })?;

    tracing::info!(
        category = "setup",
        provider = req.provider,
        model = req.model,
        "LLM configured and saved during setup"
    );

    Ok(Json(serde_json::json!({
        "message": "LLM configuration saved",
        "provider": req.provider,
        "model": req.model
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_setup_status_response_serialization() {
        let response = SetupStatusResponse {
            setup_required: true,
            is_first_launch: true,
            setup_version: "1.0",
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("setup_required"));
        assert!(json.contains("is_first_launch"));
    }

    #[test]
    fn test_initialize_admin_request_deserialization() {
        let json = r#"{"username":"admin","password":"SecurePass123","email":"admin@example.com"}"#;
        let req: InitializeAdminRequest = serde_json::from_str(json).unwrap();

        assert_eq!(req.username, "admin");
        assert_eq!(req.password, "SecurePass123");
        assert_eq!(req.email, Some("admin@example.com".to_string()));
    }
}
