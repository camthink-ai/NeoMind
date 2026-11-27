//! API Key management handlers.

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde::{Deserialize, Serialize};

use crate::auth::{ApiKeyInfo, AuthError};
use crate::auth_users::{SessionInfo, UserRole};
use crate::server::ServerState;

/// Require an authenticated Admin session (JWT Admin or wildcard `*` API key).
fn require_admin_session(session: &SessionInfo) -> Result<(), AuthError> {
    if session.role != UserRole::Admin {
        return Err(AuthError::forbidden(
            "Administrator privileges are required to manage API keys",
        ));
    }
    Ok(())
}

/// Request to create a new API key.
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct CreateKeyRequest {
    /// Human-readable name for the key
    pub name: String,
    /// Permissions (informational only — never enforced; every key has full
    /// admin access regardless of this field. Empty defaults to ["*"].)
    #[serde(default)]
    pub permissions: Vec<String>,
}

/// Response for creating an API key.
#[derive(Debug, Serialize)]
pub struct CreateKeyResponse {
    /// The actual API key (only shown once)
    pub api_key: String,
    /// Key information
    pub info: ApiKeyInfo,
}

/// List of API keys (without the actual key values).
#[derive(Debug, Serialize)]
pub struct KeyListResponse {
    pub keys: Vec<KeyListItem>,
}

/// Single item in key list (without the actual key value).
#[derive(Debug, Serialize)]
pub struct KeyListItem {
    /// Key ID
    pub id: String,
    /// Key name
    pub name: String,
    /// Creation timestamp
    pub created_at: i64,
    /// Permissions
    pub permissions: Vec<String>,
    /// Active status
    pub active: bool,
    /// Masked key preview (first 8 chars only)
    pub preview: String,
}

impl From<(String, ApiKeyInfo)> for KeyListItem {
    fn from((key, info): (String, ApiKeyInfo)) -> Self {
        Self {
            id: info.id,
            name: info.name,
            created_at: info.created_at,
            permissions: info.permissions,
            active: info.active,
            preview: format!("{}...", &key[..key.len().min(12)]),
        }
    }
}

/// Response for API operations.
#[derive(Debug, Serialize)]
pub struct ApiResponse {
    pub message: String,
    pub success: bool,
}

impl IntoResponse for ApiResponse {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::OK, Json(self)).into_response()
    }
}

/// List all API keys (requires Admin role).
#[utoipa::path(
    get,
    path = "/api/auth/keys",
    tag = "auth",
    responses(
        (status = 200, description = "List of the caller's API keys (masked previews)"),
        (status = 401, description = "Not authenticated (unified envelope)"),
        (status = 403, description = "Admin privileges required"),
    )
)]
pub async fn list_keys_handler(
    State(state): State<ServerState>,
    Extension(session): Extension<SessionInfo>,
) -> Result<Json<KeyListResponse>, AuthError> {
    require_admin_session(&session)?;

    let keys = state.auth.api_key_state.list_keys().await;
    let items: Vec<KeyListItem> = keys.into_iter().map(Into::into).collect();

    Ok(Json(KeyListResponse { keys: items }))
}

/// Create a new API key (requires Admin role).
///
/// NOTE: the `permissions` field is informational only and never enforced —
/// every API key grants full administrator access. Create keys only for
/// parties you would trust with the admin account.
#[utoipa::path(
    post,
    path = "/api/auth/keys",
    tag = "auth",
    responses(
        (status = 200, description = "Key created (full value shown once)"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Admin privileges required"),
    )
)]
pub async fn create_key_handler(
    State(state): State<ServerState>,
    Extension(session): Extension<SessionInfo>,
    Json(req): Json<CreateKeyRequest>,
) -> Result<Json<CreateKeyResponse>, AuthError> {
    require_admin_session(&session)?;

    let permissions = if req.permissions.is_empty() {
        vec!["*".to_string()]
    } else {
        req.permissions
    };

    let (key, info) = state
        .auth
        .api_key_state
        .create_key(req.name, permissions)
        .await;

    Ok(Json(CreateKeyResponse { api_key: key, info }))
}

/// Delete an API key by ID (requires Admin role).
#[utoipa::path(
    delete,
    path = "/api/auth/keys/{id}",
    tag = "auth",
    params(
        ("id" = String, Path, description = "API key id"),
    ),
    responses(
        (status = 200, description = "Key revoked; it can no longer authenticate requests"),
        (status = 403, description = "Admin privileges required"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn delete_key_handler(
    State(state): State<ServerState>,
    Extension(session): Extension<SessionInfo>,
    Path(id): Path<String>,
) -> Result<ApiResponse, AuthError> {
    require_admin_session(&session)?;

    // `list_keys()` returns `(key_hash, ApiKeyInfo)` pairs; delete by hash directly
    // instead of calling `delete_key()` which would double-hash and silently fail.
    let keys = state.auth.api_key_state.list_keys().await;
    let key_hash_to_delete = keys
        .iter()
        .find(|(_, info)| info.id == id)
        .map(|(hash, _)| hash.clone());

    if let Some(key_hash) = key_hash_to_delete {
        if state.auth.api_key_state.delete_key_by_hash(&key_hash).await {
            Ok(ApiResponse {
                message: format!("API key {} deleted", id),
                success: true,
            })
        } else {
            Ok(ApiResponse {
                message: format!("Failed to delete API key {}", id),
                success: false,
            })
        }
    } else {
        Ok(ApiResponse {
            message: format!("API key {} not found", id),
            success: false,
        })
    }
}

/// Get authentication status (public endpoint - no auth required).
#[utoipa::path(
    get,
    path = "/api/auth/status",
    tag = "auth",
    responses(
        (status = 200, description = "Whether API-key auth is enabled (no credentials required)"),
    )
)]
pub async fn auth_status_handler(State(state): State<ServerState>) -> Json<AuthStatusResponse> {
    let keys = state.auth.api_key_state.list_keys().await;
    let key_count = keys.len();

    Json(AuthStatusResponse {
        enabled: true,
        key_count,
        has_default_key: key_count > 0,
    })
}

/// Authentication status response.
#[derive(Debug, Serialize)]
pub struct AuthStatusResponse {
    pub enabled: bool,
    pub key_count: usize,
    pub has_default_key: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_key_request_deserialize() {
        let json = r#"{"name":"Test Key","permissions":["read","write"]}"#;
        let req: CreateKeyRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "Test Key");
        assert_eq!(req.permissions.len(), 2);
    }

    #[test]
    fn test_create_key_request_default_permissions() {
        let json = r#"{"name":"Test Key"}"#;
        let req: CreateKeyRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "Test Key");
        assert!(req.permissions.is_empty());
    }

    #[test]
    fn test_api_response_serialize() {
        let resp = ApiResponse {
            message: "Success".to_string(),
            success: true,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("Success"));
    }

    fn session_with_role(role: UserRole) -> SessionInfo {
        SessionInfo {
            user_id: "uid-1".to_string(),
            username: "tester".to_string(),
            role,
            created_at: 0,
            expires_at: i64::MAX,
        }
    }

    #[tokio::test]
    async fn delete_key_handler_revokes_key_without_double_hashing() {
        let state = ServerState::new_for_testing().await;
        let admin = session_with_role(UserRole::Admin);

        let created = create_key_handler(
            State(state.clone()),
            Extension(admin.clone()),
            Json(CreateKeyRequest {
                name: "revokable".to_string(),
                permissions: vec!["*".to_string()],
            }),
        )
        .await
        .expect("admin should create key")
        .0;

        assert!(state.auth.api_key_state.validate_key(&created.api_key));

        let deleted = delete_key_handler(
            State(state.clone()),
            Extension(admin),
            Path(created.info.id.clone()),
        )
        .await
        .expect("admin should delete key");
        assert!(deleted.success, "delete_key_handler must report success");
        assert!(
            !state.auth.api_key_state.validate_key(&created.api_key),
            "revoked key must no longer validate"
        );
    }

    #[tokio::test]
    async fn non_admin_sessions_cannot_manage_api_keys() {
        let state = ServerState::new_for_testing().await;
        for role in [UserRole::User, UserRole::Viewer] {
            let session = session_with_role(role);
            let list_err = list_keys_handler(State(state.clone()), Extension(session.clone()))
                .await
                .unwrap_err();
            assert_eq!(list_err.status, StatusCode::FORBIDDEN);

            let create_err = create_key_handler(
                State(state.clone()),
                Extension(session.clone()),
                Json(CreateKeyRequest {
                    name: "escalate".to_string(),
                    permissions: vec!["*".to_string()],
                }),
            )
            .await
            .unwrap_err();
            assert_eq!(create_err.status, StatusCode::FORBIDDEN);

            let delete_err = delete_key_handler(
                State(state.clone()),
                Extension(session),
                Path("any-id".to_string()),
            )
            .await
            .unwrap_err();
            assert_eq!(delete_err.status, StatusCode::FORBIDDEN);
        }
    }
}
