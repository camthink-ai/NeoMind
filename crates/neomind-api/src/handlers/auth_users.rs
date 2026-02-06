//! User authentication API handlers.

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::Json,
};

use crate::auth_users::{
    AuthError, ChangePasswordRequest, LoginRequest, LoginResponse, RegisterRequest,
    SessionInfo, UserRole,
};
use crate::server::ServerState;

/// Login handler - authenticate user and return JWT token.
pub async fn login_handler(
    State(state): State<ServerState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AuthError> {
    let response = state
        .auth.user_state
        .login(&req.username, &req.password)
        .await?;
    Ok(Json(response))
}

/// Register handler - create a new user account.
/// Note: In production, you may want to require admin approval.
pub async fn register_handler(
    State(state): State<ServerState>,
    Json(req): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), AuthError> {
    let role = req.role.unwrap_or(UserRole::User);
    let (user, token) = state
        .auth.user_state
        .register(&req.username, &req.password, role)
        .await?;
    let response = serde_json::json!({
        "token": token,
        "user": user
    });
    Ok((StatusCode::CREATED, Json(response)))
}

/// Logout handler - invalidate the current session.
pub async fn logout_handler(
    State(_state): State<ServerState>,
    Extension(user): Extension<SessionInfo>,
) -> Result<Json<serde_json::Value>, AuthError> {
    // Note: In a real implementation, you'd track which token to invalidate
    // For now, we just acknowledge the logout
    tracing::info!(username = %user.username, "User logged out");
    Ok(Json(
        serde_json::json!({"message": "Logged out successfully"}),
    ))
}

/// Get current user info handler.
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

/// Change password handler.
pub async fn change_password_handler(
    State(state): State<ServerState>,
    Extension(user): Extension<SessionInfo>,
    Json(req): Json<ChangePasswordRequest>,
) -> Result<Json<serde_json::Value>, AuthError> {
    state
        .auth.user_state
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
        .auth.user_state
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
