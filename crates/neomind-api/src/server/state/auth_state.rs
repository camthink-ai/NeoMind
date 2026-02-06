//! Authentication state.
//!
//! Contains authentication-related services:
//! - AuthState for API key validation
//! - AuthUserState for JWT token validation

use std::sync::Arc;

use crate::auth::AuthState as ApiKeyAuthState;
use crate::auth_users::AuthUserState;

/// Authentication state.
///
/// Provides access to API key and JWT authentication services.
#[derive(Clone)]
pub struct AuthState {
    /// API key authentication state.
    pub api_key_state: Arc<ApiKeyAuthState>,

    /// User authentication state for JWT token validation.
    pub user_state: Arc<AuthUserState>,
}

impl AuthState {
    /// Create a new authentication state.
    pub fn new(
        api_key_state: Arc<ApiKeyAuthState>,
        user_state: Arc<AuthUserState>,
    ) -> Self {
        Self {
            api_key_state,
            user_state,
        }
    }

    /// Create a default authentication state.
    pub fn default() -> Self {
        Self {
            api_key_state: Arc::new(ApiKeyAuthState::new()),
            user_state: Arc::new(AuthUserState::new()),
        }
    }
}
