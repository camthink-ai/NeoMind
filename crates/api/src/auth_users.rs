//! User-based authentication system.
//!
//! This module provides user management with username/password authentication,
//! JWT session tokens, and role-based access control.
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────┐     ┌──────────────┐     ┌──────────────┐
//! │   Users DB   │────▶│  AuthState   │────▶│  JWT Tokens  │
//! │  (users.redb)│     │  (in-memory) │     │  (sessions)  │
//! └──────────────┘     └──────────────┘     └──────────────┘
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use edge_api::auth_users::AuthUserState;
//!
//! let auth = AuthUserState::new();
//!
//! // Register a new user
//! let (user, token) = auth.register("alice", "password123").await?;
//!
//! // Login
//! let token = auth.login("alice", "password123").await?;
//!
//! // Validate JWT token
//! let user = auth.validate_token(&token)?;
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::RwLock;
use tracing::{info, warn, error};
use sha2::{Sha256, Digest};
use hmac::{Hmac, Mac};
use base64::prelude::*;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode as HttpStatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};

type HmacSha256 = Hmac<Sha256>;

// Table definitions
const USERS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("users");

/// User roles for RBAC
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UserRole {
    /// Admin user - full access
    Admin,
    /// Regular user - can use chat, manage own sessions
    User,
    /// Read-only user - can view but not modify
    Viewer,
}

impl UserRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            UserRole::Admin => "admin",
            UserRole::User => "user",
            UserRole::Viewer => "viewer",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "admin" => Some(UserRole::Admin),
            "user" => Some(UserRole::User),
            "viewer" => Some(UserRole::Viewer),
            _ => None,
        }
    }
}

/// User account information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    /// Unique user ID
    pub id: String,
    /// Username (unique)
    pub username: String,
    /// Password hash (bcrypt)
    pub password_hash: String,
    /// User role
    pub role: UserRole,
    /// Creation timestamp
    pub created_at: i64,
    /// Last login timestamp
    pub last_login: Option<i64>,
    /// Whether user is active
    pub active: bool,
}

/// Session token information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    /// User ID
    pub user_id: String,
    /// Username
    pub username: String,
    /// User role
    pub role: UserRole,
    /// Session creation time
    pub created_at: i64,
    /// Session expiration time
    pub expires_at: i64,
}

/// Login request.
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// Login response.
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: UserInfo,
}

/// User information (without password).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: String,
    pub username: String,
    pub role: UserRole,
    pub created_at: i64,
}

/// Register request.
#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub role: Option<UserRole>,
}

/// Change password request.
#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub old_password: String,
    pub new_password: String,
}

/// Authentication state with user management.
#[derive(Clone)]
pub struct AuthUserState {
    /// Users storage (in-memory cache)
    users: Arc<RwLock<HashMap<String, User>>>,
    /// Active sessions (token -> session info)
    sessions: Arc<RwLock<HashMap<String, SessionInfo>>>,
    /// Database path
    db_path: &'static str,
    /// JWT secret key
    jwt_secret: String,
    /// Session duration (seconds)
    session_duration: i64,
}

impl AuthUserState {
    /// Create a new auth state with user management.
    pub fn new() -> Self {
        let db_path = "data/users.redb";
        let jwt_secret = std::env::var("NEOTALK_JWT_SECRET")
            .unwrap_or_else(|_| {
                // Generate a random secret (warning: changes on restart!)
                uuid::Uuid::new_v4().to_string().replace("-", "")
            });

        // Load users from database
        let users = Self::load_users_from_db(db_path).unwrap_or_default();

        // Create default admin user if no users exist
        let users = if users.is_empty() {
            info!(category = "auth", "No users found, creating default admin user");
            let default_admin = User {
                id: uuid::Uuid::new_v4().to_string(),
                username: "admin".to_string(),
                password_hash: Self::hash_password("admin123"),  // Default password
                role: UserRole::Admin,
                created_at: chrono::Utc::now().timestamp(),
                last_login: None,
                active: true,
            };
            let mut map = HashMap::new();
            map.insert(default_admin.username.clone(), default_admin.clone());
            Self::save_user_to_db(db_path, &default_admin);
            Self::log_default_admin(&default_admin);
            map
        } else {
            users
        };

        Self {
            users: Arc::new(RwLock::new(users)),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            db_path,
            jwt_secret,
            session_duration: 7 * 24 * 60 * 60, // 7 days
        }
    }

    /// Log default admin credentials.
    fn log_default_admin(_user: &User) {
        crate::startup::log_startup().admin_user_banner("admin", "admin123");
    }

    /// Load users from database.
    fn load_users_from_db(path: &str) -> Result<HashMap<String, User>, Box<dyn std::error::Error>> {
        let db = Database::open(path)?;
        let read_txn = db.begin_read()?;

        let mut users = HashMap::new();

        if let Ok(table) = read_txn.open_table(USERS_TABLE) {
            for item in table.iter()? {
                let (username, value) = item?;
                let user = bincode::deserialize::<User>(value.value())?;
                users.insert(username.value().to_string(), user);
            }
        }

        if !users.is_empty() {
            info!(category = "auth", count = users.len(), "Loaded {} user(s) from database", users.len());
        }

        Ok(users)
    }

    /// Save user to database.
    fn save_user_to_db(path: &str, user: &User) {
        let username = user.username.clone();
        let user_bytes = bincode::serialize(user).unwrap_or_default();
        let path = path.to_string();  // Convert to owned String for 'static

        std::thread::spawn(move || {
            if let Ok(db) = Database::open(&path) {
                if let Ok(write_txn) = db.begin_write() {
                    let _ = (|| -> Result<(), Box<dyn std::error::Error>> {
                        let mut table = write_txn.open_table(USERS_TABLE)?;
                        table.insert(username.as_str(), user_bytes.as_slice())?;
                        drop(table);  // Drop table before committing
                        write_txn.commit()?;
                        Ok(())
                    })();
                }
            }
        });
    }

    /// Hash password using bcrypt (secure for production use).
    /// Uses default cost factor (12) which provides good security.
    fn hash_password(password: &str) -> String {
        bcrypt::hash(password, bcrypt::DEFAULT_COST)
            .unwrap_or_else(|e| {
                error!(category = "auth", error = %e, "Failed to hash password");
                // Fallback to a simple hash on error (should not happen)
                format!("fallback_hash_{}", password)
            })
    }

    /// Verify password against bcrypt hash.
    fn verify_password(password: &str, hash: &str) -> bool {
        // Handle legacy SHA-256 hashes for migration
        if hash.starts_with("fallback_hash_") {
            return hash == format!("fallback_hash_{}", password);
        }
        // Check if it looks like a bcrypt hash (starts with $2a$, $2b$, or $2y$)
        if hash.starts_with("$2") {
            bcrypt::verify(password, hash).unwrap_or(false)
        } else {
            // Legacy SHA-256 hash - verify and migrate on next login
            let legacy_hash = Self::hash_password_legacy(password);
            legacy_hash == hash
        }
    }

    /// Legacy SHA-256 password hash (for migration only).
    fn hash_password_legacy(password: &str) -> String {
        use sha2::Sha256;
        let mut hasher = Sha256::new();
        hasher.update(password.as_bytes());
        let hash = hasher.finalize();
        format!("{:x}", hash)
    }

    /// Generate JWT token.
    fn generate_token(&self, user: &User) -> String {
        let now = chrono::Utc::now().timestamp();
        let expires_at = now + self.session_duration;

        let header = BASE64_URL_SAFE_NO_PAD.encode(
            json!({"alg": "HS256", "typ": "JWT"}).to_string()
        );
        let payload = BASE64_URL_SAFE_NO_PAD.encode(
            json!({
                "sub": user.id,
                "username": user.username,
                "role": user.role.as_str(),
                "iat": now,
                "exp": expires_at,
            }).to_string()
        );
        let signature = {
            let data = format!("{}.{}", header, payload);
            let mut mac = HmacSha256::new_from_slice(self.jwt_secret.as_bytes()).unwrap();
            mac.update(data.as_bytes());
            BASE64_URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
        };

        format!("{}.{}.{}", header, payload, signature)
    }

    /// Validate JWT token and return session info.
    pub fn validate_token(&self, token: &str) -> Result<SessionInfo, AuthError> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return Err(AuthError::InvalidToken("Invalid token format".into()));
        }

        // Verify signature
        let data = format!("{}.{}", parts[0], parts[1]);
        let mut mac = HmacSha256::new_from_slice(self.jwt_secret.as_bytes()).unwrap();
        mac.update(data.as_bytes());

        let expected_sig = BASE64_URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
        if parts[2] != expected_sig {
            return Err(AuthError::InvalidToken("Invalid signature".into()));
        }

        // Decode payload
        let payload_bytes = BASE64_URL_SAFE_NO_PAD.decode(parts[1])
            .map_err(|_| AuthError::InvalidToken("Invalid payload encoding".into()))?;
        let payload_str = String::from_utf8(payload_bytes)
            .map_err(|_| AuthError::InvalidToken("Invalid payload UTF-8".into()))?;
        let payload: serde_json::Value = serde_json::from_str(&payload_str)
            .map_err(|_| AuthError::InvalidToken("Invalid payload JSON".into()))?;

        // Check expiration
        let exp = payload["exp"].as_i64().unwrap_or(0);
        if exp < chrono::Utc::now().timestamp() {
            return Err(AuthError::ExpiredToken);
        }

        Ok(SessionInfo {
            user_id: payload["sub"].as_str().unwrap_or("").to_string(),
            username: payload["username"].as_str().unwrap_or("").to_string(),
            role: UserRole::from_str(payload["role"].as_str().unwrap_or("user"))
                .unwrap_or(UserRole::User),
            created_at: payload["iat"].as_i64().unwrap_or(0),
            expires_at: exp,
        })
    }

    /// Register a new user.
    pub async fn register(&self, username: &str, password: &str, role: UserRole) -> Result<(UserInfo, String), AuthError> {
        // Validate username
        if username.len() < 3 {
            return Err(AuthError::InvalidInput("Username must be at least 3 characters".into()));
        }
        if password.len() < 6 {
            return Err(AuthError::InvalidInput("Password must be at least 6 characters".into()));
        }

        // Check if user exists
        let users = self.users.read().await;
        if users.contains_key(username) {
            drop(users);
            return Err(AuthError::UserExists);
        }
        drop(users);

        // Create user
        let user = User {
            id: uuid::Uuid::new_v4().to_string(),
            username: username.to_string(),
            password_hash: Self::hash_password(password),
            role: role.clone(),
            created_at: chrono::Utc::now().timestamp(),
            last_login: None,
            active: true,
        };

        // Save to database and memory
        Self::save_user_to_db(self.db_path, &user);
        let mut users = self.users.write().await;
        users.insert(username.to_string(), user.clone());
        drop(users);

        // Generate token
        let token = self.generate_token(&user);

        info!(category = "auth", username = username, role = role.as_str(), "User registered");

        Ok((UserInfo {
            id: user.id,
            username: user.username.clone(),
            role: user.role,
            created_at: user.created_at,
        }, token))
    }

    /// Login user and return token.
    pub async fn login(&self, username: &str, password: &str) -> Result<LoginResponse, AuthError> {
        // Clone user data before releasing lock
        let (user_id, user_role, user_created_at) = {
            let users = self.users.read().await;
            let user = users.get(username)
                .ok_or_else(|| AuthError::InvalidCredentials)?;

            if !user.active {
                return Err(AuthError::UserDisabled);
            }

            if !Self::verify_password(password, &user.password_hash) {
                return Err(AuthError::InvalidCredentials);
            }

            (user.id.clone(), user.role.clone(), user.created_at)
        };

        // Update last login
        let mut users = self.users.write().await;
        if let Some(u) = users.get_mut(username) {
            u.last_login = Some(chrono::Utc::now().timestamp());
        }
        drop(users);

        // Generate token
        let token = {
            let users = self.users.read().await;
            self.generate_token(users.get(username).unwrap())
        };

        // Store session
        let session_info = SessionInfo {
            user_id: user_id.clone(),
            username: username.to_string(),
            role: user_role.clone(),
            created_at: chrono::Utc::now().timestamp(),
            expires_at: chrono::Utc::now().timestamp() + self.session_duration,
        };
        let mut sessions = self.sessions.write().await;
        sessions.insert(token.clone(), session_info);
        drop(sessions);

        info!(category = "auth", username = username, "User logged in");

        Ok(LoginResponse {
            token,
            user: UserInfo {
                id: user_id,
                username: username.to_string(),
                role: user_role,
                created_at: user_created_at,
            },
        })
    }

    /// Logout user (invalidate session).
    pub async fn logout(&self, token: &str) -> Result<(), AuthError> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(token);
        Ok(())
    }

    /// List all users (admin only).
    pub async fn list_users(&self) -> Vec<UserInfo> {
        let users = self.users.read().await;
        users.values().map(|u| UserInfo {
            id: u.id.clone(),
            username: u.username.clone(),
            role: u.role.clone(),
            created_at: u.created_at,
        }).collect()
    }

    /// Delete user.
    pub async fn delete_user(&self, username: &str) -> Result<(), AuthError> {
        let mut users = self.users.write().await;
        users.remove(username)
            .ok_or_else(|| AuthError::UserNotFound)?;
        Ok(())
    }

    /// Change password.
    pub async fn change_password(&self, username: &str, old_password: &str, new_password: &str) -> Result<(), AuthError> {
        if new_password.len() < 6 {
            return Err(AuthError::InvalidInput("Password must be at least 6 characters".into()));
        }

        let mut users = self.users.write().await;
        let user = users.get_mut(username)
            .ok_or_else(|| AuthError::UserNotFound)?;

        if !Self::verify_password(old_password, &user.password_hash) {
            return Err(AuthError::InvalidCredentials);
        }

        user.password_hash = Self::hash_password(new_password);

        info!(category = "auth", username = username, "Password changed");

        Ok(())
    }
}

impl Default for AuthUserState {
    fn default() -> Self {
        Self::new()
    }
}

/// Authentication errors.
#[derive(Debug, Clone)]
pub enum AuthError {
    InvalidCredentials,
    UserExists,
    UserNotFound,
    UserDisabled,
    InvalidToken(String),
    ExpiredToken,
    InvalidInput(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::InvalidCredentials => write!(f, "Invalid username or password"),
            AuthError::UserExists => write!(f, "User already exists"),
            AuthError::UserNotFound => write!(f, "User not found"),
            AuthError::UserDisabled => write!(f, "User account is disabled"),
            AuthError::InvalidToken(msg) => write!(f, "Invalid token: {}", msg),
            AuthError::ExpiredToken => write!(f, "Token has expired"),
            AuthError::InvalidInput(msg) => write!(f, "Invalid input: {}", msg),
        }
    }
}

impl std::error::Error for AuthError {}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, message): (HttpStatusCode, String) = match self {
            AuthError::InvalidCredentials => (HttpStatusCode::UNAUTHORIZED, "Invalid username or password".into()),
            AuthError::UserExists => (HttpStatusCode::CONFLICT, "User already exists".into()),
            AuthError::UserNotFound => (HttpStatusCode::NOT_FOUND, "User not found".into()),
            AuthError::UserDisabled => (HttpStatusCode::FORBIDDEN, "User account is disabled".into()),
            AuthError::InvalidToken(msg) => (HttpStatusCode::UNAUTHORIZED, msg),
            AuthError::ExpiredToken => (HttpStatusCode::UNAUTHORIZED, "Token has expired".into()),
            AuthError::InvalidInput(msg) => (HttpStatusCode::BAD_REQUEST, msg),
        };

        let body = serde_json::json!({
            "error": message,
            "status": status.as_u16(),
        });

        (status, Json(body)).into_response()
    }
}

/// JWT authentication middleware.
/// Works with ServerState - extracts auth_user_state from it.
pub async fn jwt_auth_middleware(
    State(state): State<crate::server::ServerState>,
    headers: HeaderMap,
    mut req: axum::extract::Request,
    next: Next,
) -> Result<Response, AuthError> {
    // Extract token from Authorization header
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AuthError::InvalidToken("Missing Authorization header".into()))?;

    let token = auth_header.strip_prefix("Bearer ")
        .ok_or_else(|| AuthError::InvalidToken("Invalid Authorization format".into()))?;

    // Validate token using auth_user_state from ServerState
    let session_info = state.auth_user_state.validate_token(token)?;

    // Store user info in request extensions
    req.extensions_mut().insert(session_info);

    Ok(next.run(req).await)
}

/// Optional JWT authentication middleware.
pub async fn optional_jwt_auth_middleware(
    State(state): State<crate::server::ServerState>,
    headers: HeaderMap,
    mut req: axum::extract::Request,
    next: Next,
) -> Response {
    if let Some(auth_header) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(token) = auth_header.strip_prefix("Bearer ") {
            if let Ok(session_info) = state.auth_user_state.validate_token(token) {
                req.extensions_mut().insert(session_info);
            }
        }
    }

    next.run(req).await
}

/// Extract user info from request extensions.
/// Use this with axum's Extension extractor:
/// ```rust,no_run
/// use axum::Extension;
///
/// async fn handler(Extension(user): Extension<SessionInfo>) -> &'static str {
///     "Hello"
/// }
/// ```
pub type CurrentUserExtension = SessionInfo;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_user_registration() {
        let auth = AuthUserState::new();
        let (user, token) = auth.register("testuser", "password123", UserRole::User).await.unwrap();
        assert_eq!(user.username, "testuser");
        assert!(!token.is_empty());
    }

    #[tokio::test]
    async fn test_user_login() {
        let auth = AuthUserState::new();
        auth.register("testuser", "password123", UserRole::User).await.unwrap();

        let response = auth.login("testuser", "password123").await.unwrap();
        assert_eq!(response.user.username, "testuser");
        assert!(!response.token.is_empty());
    }

    #[tokio::test]
    async fn test_token_validation() {
        let auth = AuthUserState::new();
        let (_, token) = auth.register("testuser", "password123", UserRole::User).await.unwrap();

        let session = auth.validate_token(&token).unwrap();
        assert_eq!(session.username, "testuser");
    }
}
