//! User authentication API handlers.

use axum::{
    extract::{ConnectInfo, Extension, Path, State},
    http::{HeaderMap, StatusCode},
    response::Json,
};
use std::net::SocketAddr;

use crate::auth_users::{
    client_ip_for_throttle, AuthError, ChangePasswordRequest, LoginRequest, LoginResponse,
    RegisterRequest, SessionInfo, UserRole,
};
use crate::server::ServerState;

/// Login handler - authenticate user and return JWT token.
///
/// Brute-force throttled: the request is checked (per username AND per client
/// IP) before the password is verified, and only credential failures count —
/// a successful login clears both keys.
pub async fn login_handler(
    State(state): State<ServerState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AuthError> {
    let ip = client_ip_for_throttle(&headers, &addr);
    state
        .auth
        .user_state
        .check_login_throttle(&req.username, Some(&ip))?;

    match state
        .auth
        .user_state
        .login(&req.username, &req.password)
        .await
    {
        Ok(response) => {
            state
                .auth
                .user_state
                .clear_login_throttle(&req.username, Some(&ip));
            Ok(Json(response))
        }
        // Only credential failures count — a database error must not push an
        // honest user toward a lockout.
        Err(e @ (AuthError::InvalidCredentials | AuthError::UserNotFound)) => {
            state
                .auth
                .user_state
                .record_login_failure(&req.username, Some(&ip));
            Err(e)
        }
        Err(e) => Err(e),
    }
}

/// Register handler - create a new user account.
///
/// SECURITY: Self-service registration ALWAYS creates a `UserRole::User`
/// account, regardless of any `role` field supplied in the request body.
/// Promoting to admin must go through the admin-only `create_user_handler`.
/// The `role` field on `RegisterRequest` is accepted (for backwards-
/// compatibility with older clients) but silently ignored.
///
/// Every attempt counts against the per-IP signup throttle — each call
/// creates a user, so there is no honest high-volume caller.
pub async fn register_handler(
    State(state): State<ServerState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), AuthError> {
    let ip = client_ip_for_throttle(&headers, &addr);
    state
        .auth
        .user_state
        .check_signup_throttle("reg", Some(&ip))?;
    state
        .auth
        .user_state
        .record_signup_attempt("reg", Some(&ip));

    let (user, token) = state
        .auth
        .user_state
        .register(&req.username, &req.password, UserRole::User)
        .await?;
    let response = serde_json::json!({
        "token": token,
        "user": user
    });
    Ok((StatusCode::CREATED, Json(response)))
}

/// Logout handler - invalidate the current session.
pub async fn logout_handler(
    State(state): State<ServerState>,
    Extension(user): Extension<SessionInfo>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AuthError> {
    // Invalidate the session server-side so the bearer token can't be reused.
    // (Previously a no-op: returned 200 but left the token valid for its full
    // lifetime — anyone who grabbed the token stayed logged in for days.)
    if let Some(token) = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        let _ = state.auth_user_state().logout(token).await;
    }
    tracing::info!(username = %user.username, "User logged out");
    Ok(Json(
        serde_json::json!({"message": "Logged out successfully"}),
    ))
}

/// Get current user info handler.
/// Requires JWT authentication (API key auth is not supported for user info).
pub async fn get_current_user_handler(
    Extension(user): Extension<SessionInfo>,
) -> Result<Json<serde_json::Value>, AuthError> {
    Ok(Json(serde_json::json!({
        "id": user.user_id,
        "username": user.username,
        "role": user.role.as_str(),
        "created_at": user.created_at,
    })))
}

/// Get auth status handler for API key auth.
/// Returns basic info when authenticated via API key (no user session).
pub async fn get_auth_status_handler(
    State(state): State<ServerState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AuthError> {
    // Check JWT first
    if let Some(auth_header) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(token) = auth_header.strip_prefix("Bearer ") {
            if let Ok(session_info) = state.auth.user_state.validate_token(token) {
                return Ok(Json(serde_json::json!({
                    "authenticated": true,
                    "method": "jwt",
                    "user": {
                        "id": session_info.user_id,
                        "username": session_info.username,
                        "role": session_info.role.as_str(),
                    }
                })));
            }
        }
    }

    // Check API key
    if let Some(key) = headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
        if state.auth.api_key_state.validate_key(key) {
            return Ok(Json(serde_json::json!({
                "authenticated": true,
                "method": "api_key"
            })));
        }
    }

    Err(AuthError::InvalidToken(
        "Authentication required".to_string(),
    ))
}

/// Change password handler.
pub async fn change_password_handler(
    State(state): State<ServerState>,
    Extension(user): Extension<SessionInfo>,
    Json(req): Json<ChangePasswordRequest>,
) -> Result<Json<serde_json::Value>, AuthError> {
    state
        .auth
        .user_state
        .change_password(&user.username, &req.old_password, &req.new_password)
        .await?;
    Ok(Json(
        serde_json::json!({"message": "Password changed successfully"}),
    ))
}

/// List all users handler (admin only).
pub async fn list_users_handler(
    State(state): State<ServerState>,
    Extension(user): Extension<SessionInfo>,
) -> Result<Json<serde_json::Value>, AuthError> {
    // Check admin permission
    if user.role != UserRole::Admin {
        return Err(AuthError::InvalidInput("Admin access required".into()));
    }

    let users = state.auth.user_state.list_users().await;
    Ok(Json(serde_json::json!({"users": users})))
}

/// Create a new user handler (admin only).
pub async fn create_user_handler(
    State(state): State<ServerState>,
    Extension(admin_user): Extension<SessionInfo>,
    Json(req): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), AuthError> {
    // Check admin permission
    if admin_user.role != UserRole::Admin {
        return Err(AuthError::InvalidInput("Admin access required".into()));
    }

    let role = req.role.unwrap_or(UserRole::User);
    let role_str = role.as_str(); // Store role string before moving role
    let (user, _token) = state
        .auth
        .user_state
        .register(&req.username, &req.password, role)
        .await?;

    tracing::info!(
        admin = %admin_user.username,
        new_user = %user.username,
        role = role_str,
        "Admin created new user"
    );

    Ok((StatusCode::CREATED, Json(serde_json::json!({"user": user}))))
}

/// Delete user handler (admin only).
pub async fn delete_user_handler(
    State(state): State<ServerState>,
    Extension(admin_user): Extension<SessionInfo>,
    Path(username): Path<String>,
) -> Result<Json<serde_json::Value>, AuthError> {
    // Check admin permission
    if admin_user.role != UserRole::Admin {
        return Err(AuthError::InvalidInput("Admin access required".into()));
    }

    // Prevent self-deletion
    if username == admin_user.username {
        return Err(AuthError::InvalidInput(
            "Cannot delete your own account".into(),
        ));
    }

    state.auth.user_state.delete_user(&username).await?;

    tracing::info!(
        admin = %admin_user.username,
        deleted_user = %username,
        "Admin deleted user"
    );

    Ok(Json(
        serde_json::json!({"message": format!("User '{}' deleted successfully", username)}),
    ))
}
