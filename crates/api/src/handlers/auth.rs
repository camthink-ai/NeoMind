//! API Key management handlers.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
};
use serde::{Deserialize, Serialize};

use crate::auth::{ApiKeyInfo, AuthError};
use crate::server::ServerState;

/// Helper to extract validated API key from headers.
fn extract_api_key(headers: &HeaderMap) -> Result<String, AuthError> {
    headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .or_else(|| {
            headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
        })
        .map(|s| s.to_string())
        .ok_or_else(|| AuthError::unauthorized("Missing API key"))
}

/// Request to create a new API key.
#[derive(Debug, Deserialize)]
pub struct CreateKeyRequest {
    /// Human-readable name for the key
    pub name: String,
    /// Permissions (empty means full access)
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

/// List all API keys (requires authentication).
pub async fn list_keys_handler(
    State(state): State<ServerState>,
    headers: HeaderMap,
) -> Result<Json<KeyListResponse>, AuthError> {
    // Validate API key
    let api_key = extract_api_key(&headers)?;
    if !state.auth_state.validate_key(&api_key) {
        return Err(AuthError::unauthorized("Invalid API key"));
    }

    let keys = state.auth_state.list_keys().await;
    let items: Vec<KeyListItem> = keys.into_iter().map(Into::into).collect();

    Ok(Json(KeyListResponse { keys: items }))
}

/// Create a new API key (requires authentication).
pub async fn create_key_handler(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Json(req): Json<CreateKeyRequest>,
) -> Result<Json<CreateKeyResponse>, AuthError> {
    // Validate API key
    let api_key = extract_api_key(&headers)?;
    if !state.auth_state.validate_key(&api_key) {
        return Err(AuthError::unauthorized("Invalid API key"));
    }

    let permissions = if req.permissions.is_empty() {
        vec!["*".to_string()]
    } else {
        req.permissions
    };

    let (key, info) = state.auth_state.create_key(req.name, permissions).await;

    Ok(Json(CreateKeyResponse {
        api_key: key,
        info,
    }))
}

/// Delete an API key by ID (requires authentication).
pub async fn delete_key_handler(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<ApiResponse, AuthError> {
    // Validate API key
    let api_key = extract_api_key(&headers)?;
    if !state.auth_state.validate_key(&api_key) {
        return Err(AuthError::unauthorized("Invalid API key"));
    }
    // Find the key by ID and delete it
    let keys = state.auth_state.list_keys().await;
    let key_to_delete = keys.iter().find(|(_, info)| info.id == id).map(|(k, _)| k.clone());

    if let Some(key) = key_to_delete {
        if state.auth_state.delete_key(&key).await {
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
pub async fn auth_status_handler(
    State(state): State<ServerState>,
) -> Json<AuthStatusResponse> {
    let keys = state.auth_state.list_keys().await;
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
}
