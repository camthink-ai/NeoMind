//! Tests for auth_users handlers.

use edge_ai_api::handlers::auth_users::*;
use edge_ai_api::handlers::ServerState;
use edge_ai_api::auth_users::{LoginRequest, RegisterRequest, ChangePasswordRequest, SessionInfo, UserRole};
use axum::extract::{Extension, Path, State};
use axum::Json;
use axum::http::StatusCode;

async fn create_test_server_state() -> ServerState {
    crate::common::create_test_server_state().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_login_handler_invalid_credentials() {
        let state = create_test_server_state().await;
        let req = LoginRequest {
            username: "nonexistent".to_string(),
            password: "wrongpassword".to_string(),
        };
        let result = login_handler(State(state), Json(req)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_register_handler() {
        let state = create_test_server_state().await;
        let username = format!("test_user_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));
        let req = RegisterRequest {
            username: username.clone(),
            password: "test_password_123".to_string(),
            role: Some(UserRole::User),
        };
        let result = register_handler(State(state), Json(req)).await;
        assert!(result.is_ok());
        let (status, response) = result.unwrap();
        assert_eq!(status, StatusCode::CREATED);
        let value = response.0;
        assert!(value.get("token").is_some());
        assert!(value.get("user").is_some());
    }

    #[tokio::test]
    async fn test_register_handler_admin_role() {
        let state = create_test_server_state().await;
        let username = format!("admin_user_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));
        let req = RegisterRequest {
            username: username.clone(),
            password: "admin_password_123".to_string(),
            role: Some(UserRole::Admin),
        };
        let result = register_handler(State(state), Json(req)).await;
        assert!(result.is_ok());
        let (status, response) = result.unwrap();
        assert_eq!(status, StatusCode::CREATED);
        let value = response.0;
        assert!(value.get("token").is_some());
    }

    #[tokio::test]
    async fn test_register_handler_default_role() {
        let state = create_test_server_state().await;
        let username = format!("default_user_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));
        let req = RegisterRequest {
            username: username.clone(),
            password: "password123".to_string(),
            role: None, // Should default to User
        };
        let result = register_handler(State(state), Json(req)).await;
        assert!(result.is_ok());
        let (status, _response) = result.unwrap();
        assert_eq!(status, StatusCode::CREATED);
    }

    #[tokio::test]
    async fn test_logout_handler() {
        let state = create_test_server_state().await;
        let now = chrono::Utc::now().timestamp();
        let user_info = SessionInfo {
            user_id: "test_id".to_string(),
            username: "testuser".to_string(),
            role: UserRole::User,
            created_at: now,
            expires_at: now + 3600,
        };
        let result = logout_handler(State(state), Extension(user_info)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0;
        assert!(value.get("message").is_some());
    }

    #[tokio::test]
    async fn test_get_current_user_handler() {
        let now = chrono::Utc::now().timestamp();
        let user_info = SessionInfo {
            user_id: "test_id".to_string(),
            username: "testuser".to_string(),
            role: UserRole::Admin,
            created_at: now,
            expires_at: now + 3600,
        };
        let result = get_current_user_handler(Extension(user_info)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0;
        assert_eq!(value.get("id").unwrap().as_str().unwrap(), "test_id");
        assert_eq!(value.get("username").unwrap().as_str().unwrap(), "testuser");
        assert_eq!(value.get("role").unwrap().as_str().unwrap(), "admin");
    }

    #[tokio::test]
    async fn test_change_password_handler_invalid_user() {
        let state = create_test_server_state().await;
        let now = chrono::Utc::now().timestamp();
        let user_info = SessionInfo {
            user_id: "nonexistent_id".to_string(),
            username: "nonexistent".to_string(),
            role: UserRole::User,
            created_at: now,
            expires_at: now + 3600,
        };
        let req = ChangePasswordRequest {
            old_password: "wrong_old".to_string(),
            new_password: "new_password".to_string(),
        };
        let result = change_password_handler(State(state), Extension(user_info), Json(req)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_users_handler_non_admin() {
        let state = create_test_server_state().await;
        let now = chrono::Utc::now().timestamp();
        let user_info = SessionInfo {
            user_id: "test_id".to_string(),
            username: "testuser".to_string(),
            role: UserRole::User, // Not an admin
            created_at: now,
            expires_at: now + 3600,
        };
        let result = list_users_handler(State(state), Extension(user_info)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_users_handler_admin() {
        let state = create_test_server_state().await;
        let now = chrono::Utc::now().timestamp();
        let user_info = SessionInfo {
            user_id: "admin_id".to_string(),
            username: "admin".to_string(),
            role: UserRole::Admin,
            created_at: now,
            expires_at: now + 3600,
        };
        let result = list_users_handler(State(state), Extension(user_info)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0;
        assert!(value.get("users").is_some());
    }

    #[tokio::test]
    async fn test_create_user_handler_non_admin() {
        let state = create_test_server_state().await;
        let now = chrono::Utc::now().timestamp();
        let user_info = SessionInfo {
            user_id: "test_id".to_string(),
            username: "testuser".to_string(),
            role: UserRole::User, // Not an admin
            created_at: now,
            expires_at: now + 3600,
        };
        let req = RegisterRequest {
            username: "newuser".to_string(),
            password: "password123".to_string(),
            role: Some(UserRole::User),
        };
        let result = create_user_handler(State(state), Extension(user_info), Json(req)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_user_handler_admin() {
        let state = create_test_server_state().await;
        let now = chrono::Utc::now().timestamp();
        let admin_info = SessionInfo {
            user_id: "admin_id".to_string(),
            username: "admin".to_string(),
            role: UserRole::Admin,
            created_at: now,
            expires_at: now + 3600,
        };
        let username = format!("created_user_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));
        let req = RegisterRequest {
            username: username.clone(),
            password: "password123".to_string(),
            role: Some(UserRole::User),
        };
        let result = create_user_handler(State(state), Extension(admin_info), Json(req)).await;
        assert!(result.is_ok());
        let (status, response) = result.unwrap();
        assert_eq!(status, StatusCode::CREATED);
        let value = response.0;
        assert!(value.get("user").is_some());
    }

    #[tokio::test]
    async fn test_delete_user_handler_non_admin() {
        let state = create_test_server_state().await;
        let now = chrono::Utc::now().timestamp();
        let user_info = SessionInfo {
            user_id: "test_id".to_string(),
            username: "testuser".to_string(),
            role: UserRole::User, // Not an admin
            created_at: now,
            expires_at: now + 3600,
        };
        let result = delete_user_handler(State(state), Extension(user_info), Path("someuser".to_string())).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_user_handler_self_deletion() {
        let state = create_test_server_state().await;
        let now = chrono::Utc::now().timestamp();
        let admin_info = SessionInfo {
            user_id: "admin_id".to_string(),
            username: "admin".to_string(),
            role: UserRole::Admin,
            created_at: now,
            expires_at: now + 3600,
        };
        let result = delete_user_handler(State(state), Extension(admin_info), Path("admin".to_string())).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Cannot delete your own account") ||
                err.to_string().contains("own account"));
    }

    #[tokio::test]
    async fn test_user_role_display() {
        assert_eq!(UserRole::Admin.as_str(), "admin");
        assert_eq!(UserRole::User.as_str(), "user");
        assert_eq!(UserRole::Viewer.as_str(), "viewer");
    }

    #[tokio::test]
    async fn test_session_info() {
        let now = chrono::Utc::now().timestamp();
        let info = SessionInfo {
            user_id: "user123".to_string(),
            username: "testuser".to_string(),
            role: UserRole::Admin,
            created_at: 1234567890,
            expires_at: 1234577890,
        };
        assert_eq!(info.user_id, "user123");
        assert_eq!(info.username, "testuser");
        assert_eq!(info.role, UserRole::Admin);
        assert_eq!(info.created_at, 1234567890);
        assert_eq!(info.expires_at, 1234577890);
    }

    #[tokio::test]
    async fn test_login_request() {
        let req = LoginRequest {
            username: "testuser".to_string(),
            password: "password123".to_string(),
        };
        assert_eq!(req.username, "testuser");
        assert_eq!(req.password, "password123");
    }

    #[tokio::test]
    async fn test_register_request() {
        let req = RegisterRequest {
            username: "newuser".to_string(),
            password: "newpass".to_string(),
            role: Some(UserRole::Admin),
        };
        assert_eq!(req.username, "newuser");
        assert_eq!(req.password, "newpass");
        assert_eq!(req.role.unwrap(), UserRole::Admin);
    }

    #[tokio::test]
    async fn test_change_password_request() {
        let req = ChangePasswordRequest {
            old_password: "oldpass".to_string(),
            new_password: "newpass".to_string(),
        };
        assert_eq!(req.old_password, "oldpass");
        assert_eq!(req.new_password, "newpass");
    }
}
