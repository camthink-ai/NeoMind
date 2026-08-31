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

use base64::prelude::*;
use hmac::{Hmac, Mac};
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;
use tracing::{error, info};

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode as HttpStatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};

type HmacSha256 = Hmac<Sha256>;

/// Helper function to safely create HMAC instance
fn create_hmac(key: &[u8]) -> Result<HmacSha256, AuthError> {
    HmacSha256::new_from_slice(key)
        .map_err(|_| AuthError::InvalidInput("Invalid JWT secret length".to_string()))
}

// Table definitions
const USERS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("users");

/// Active sessions (token-hash → `SessionInfo`), persisted so a server
/// restart doesn't invalidate every login. The persisted `.jwt_secret`
/// keeps token *signatures* valid across restarts; this table keeps the
/// stateful-revocation allowlist valid across restarts too (previously the
/// in-memory map was rebuilt empty on boot, so `validate_token` rejected
/// every pre-restart token with `SessionRevoked` — defeating the persisted
/// secret and logging everyone out on every restart). Keys are
/// SHA-256(token) — raw tokens are never written at rest.
const SESSIONS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("user_sessions");

/// Auth-related KV settings (currently: whether unauthenticated
/// self-registration is allowed). String values, admin-controlled.
const AUTH_SETTINGS_TABLE: TableDefinition<&str, &str> = TableDefinition::new("auth_settings");

/// Stable lookup key for a session token — SHA-256 hex. Used as BOTH the
/// in-memory map key and the persisted row key, so the allowlist can be
/// rebuilt from disk at boot without ever storing raw tokens.
fn token_key(token: &str) -> String {
    use sha2::Digest;
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

/// Clock-skew tolerance (seconds) applied to JWT `exp` during validation, and
/// mirrored by the sessions reaper so it never evicts a session whose token is
/// still within this grace window. Hoisted to module scope to keep the two
/// call sites in lockstep (a drift here can wrongly `SessionRevoked` a valid
/// token, or let expired sessions linger).
const JWT_CLOCK_SKEW_SECS: i64 = 30;

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

    pub fn parse(s: &str) -> Option<Self> {
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

/// Sliding-window event counter backing the auth throttles.
///
/// Counts events per key over a fixed window; a key at/over the cap is
/// locked out until its oldest event slides out. Memory is bounded: the map
/// is swept for expired entries once it exceeds [`SWEEP_THRESHOLD`] keys, so
/// a flood of distinct keys cannot grow it unboundedly.
struct SlidingWindowCounter {
    events: std::sync::Mutex<HashMap<String, Vec<std::time::Instant>>>,
    max_events: usize,
    window: std::time::Duration,
}

/// Map-size trigger for the expiry sweep (see [`SlidingWindowCounter`]).
const SWEEP_THRESHOLD: usize = 10_000;

impl SlidingWindowCounter {
    fn new(max_events: usize, window: std::time::Duration) -> Self {
        Self {
            events: std::sync::Mutex::new(HashMap::new()),
            max_events,
            window,
        }
    }

    /// Ok if `key` is under the cap; otherwise `Err(retry_after_secs)`.
    /// Expired events are dropped, so the lockout ends exactly when the
    /// oldest counted event leaves the window.
    fn check(&self, key: &str) -> Result<(), u64> {
        let mut map = self.events.lock().unwrap();
        if let Some(times) = map.get_mut(key) {
            let now = std::time::Instant::now();
            times.retain(|t| now.duration_since(*t) < self.window);
            if times.len() >= self.max_events {
                let retry = self.window - now.duration_since(times[0]);
                return Err(retry.as_secs().max(1));
            }
        }
        Ok(())
    }

    /// Count one event against `key`.
    fn record(&self, key: &str) {
        let mut map = self.events.lock().unwrap();
        if map.len() > SWEEP_THRESHOLD {
            let now = std::time::Instant::now();
            map.retain(|_, times| {
                times.retain(|t| now.duration_since(*t) < self.window);
                !times.is_empty()
            });
        }
        map.entry(key.to_string())
            .or_default()
            .push(std::time::Instant::now());
    }

    /// Drop all counted events for `key` (used when a login succeeds — an
    /// honest user who mistypes a few times then gets it right starts clean).
    fn clear(&self, key: &str) {
        self.events.lock().unwrap().remove(key);
    }
}

/// Brute-force throttles for the public auth endpoints.
///
/// The global HTTP rate limiter sits at flood scale (thousands of req/min on
/// adaptive hardware) — no defense against password guessing. These windows
/// are failure-aware instead: login counts only *credential* failures (a
/// successful login clears the counters, so an honest user mistyping a few
/// times is never locked out), while register/setup count every attempt.
///
/// Login is keyed two ways — per username AND per client IP — and blocks when
/// either key is over the cap: the username key stops distributed guessing at
/// one account, the IP key stops one host spraying many usernames. An IP is
/// only a secondary signal (a direct-connection attacker can forge
/// `X-Forwarded-For` to dodge it; the username key is unaffected).
pub struct AuthThrottle {
    /// Credential failures: 5 per 15 min per key.
    login_failures: SlidingWindowCounter,
    /// Register + first-setup attempts: 10 per 15 min per IP. Every attempt
    /// counts (not just failures) — each creates a user or claims the
    /// device's admin account, so there is no honest high-volume caller.
    signup_attempts: SlidingWindowCounter,
}

impl AuthThrottle {
    /// Production defaults.
    fn production() -> Self {
        Self {
            login_failures: SlidingWindowCounter::new(5, std::time::Duration::from_secs(15 * 60)),
            signup_attempts: SlidingWindowCounter::new(10, std::time::Duration::from_secs(15 * 60)),
        }
    }
}

/// Authentication state with user management.
#[derive(Clone)]
pub struct AuthUserState {
    /// Users storage (in-memory cache)
    users: Arc<RwLock<HashMap<String, User>>>,
    /// Active sessions (token -> session info). std::sync::RwLock (not tokio)
    /// so validate_token can consult it synchronously without forcing every
    /// caller (some in sync closures) to .await. Locks are never held across
    /// an await, so no deadlock risk.
    sessions: Arc<std::sync::RwLock<HashMap<String, SessionInfo>>>,
    /// Database path
    db_path: &'static str,
    /// JWT secret key
    jwt_secret: String,
    /// Session duration (seconds)
    session_duration: i64,
    /// Whether unauthenticated self-registration (`POST /api/auth/register`)
    /// is allowed. Defaults to CLOSED: the first admin comes from the setup
    /// wizard, additional users are created by an admin (`POST /api/users`) —
    /// on a 0.0.0.0-bound edge box, open self-registration lets any LAN
    /// client mint an account. std::sync::RwLock: read on the hot register
    /// path, written rarely by an admin.
    allow_registration: Arc<std::sync::RwLock<bool>>,
    /// Brute-force throttles for the public auth endpoints (login/register/
    /// setup). See [`AuthThrottle`].
    auth_throttle: Arc<AuthThrottle>,
}

impl AuthUserState {
    /// Create a new auth state with user management.
    ///
    /// Note: This no longer creates a default admin user automatically.
    /// The setup wizard should be used to create the first admin account.
    pub fn new() -> Self {
        // Check if running in test mode
        if std::env::var("NEOMIND_TEST_MODE").is_ok() {
            tracing::warn!(
                category = "auth",
                "NEOMIND_TEST_MODE active: using in-memory auth store. NOT suitable for production!"
            );
            return Self::new_with_memory_store();
        }

        let data_dir = neomind_core::paths::data_dir()
            .to_string_lossy()
            .to_string();
        let db_path: &'static str = Box::leak(format!("{}/users.redb", data_dir).into_boxed_str());
        let jwt_secret = std::env::var("NEOMIND_JWT_SECRET").unwrap_or_else(|_| {
            // No env var: load or create a persisted secret so JWTs survive
            // restarts. (Previously generated a new random secret every restart
            // → every user logged out on every server restart.)
            let secret_path = format!("{}/.jwt_secret", data_dir);
            if let Ok(persisted) = std::fs::read_to_string(&secret_path) {
                let trimmed = persisted.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
            let new_secret = uuid::Uuid::new_v4().to_string().replace("-", "");
            // Persist with 0600 — this secret forges any auth token, so it must
            // not be world/group-readable. std::fs::write defaults to 0644. We
            // also set_permissions afterward to harden a file a prior 0.9.12
            // build may have written 0644 (OpenOptions::mode only applies at
            // creation, not when opening an existing file).
            #[cfg(unix)]
            {
                use std::io::Write;
                use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
                let write_result = std::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .mode(0o600)
                    .open(&secret_path)
                    .and_then(|mut f| f.write_all(new_secret.as_bytes()));
                // Surface persistence failures: a failed write silently
                // falls back to an in-memory-only secret that rotates on the
                // next restart (every user logged out) with no trace of why.
                if let Err(e) = write_result {
                    error!(
                        category = "auth",
                        error = %e,
                        path = %secret_path,
                        "Failed to persist JWT secret — it will rotate on next restart and all sessions will be invalidated"
                    );
                }
                let _ =
                    std::fs::set_permissions(&secret_path, std::fs::Permissions::from_mode(0o600));
            }
            #[cfg(not(unix))]
            {
                if let Err(e) = std::fs::write(&secret_path, &new_secret) {
                    error!(
                        category = "auth",
                        error = %e,
                        path = %secret_path,
                        "Failed to persist JWT secret — it will rotate on next restart and all sessions will be invalidated"
                    );
                }
            }
            tracing::warn!(
                category = "auth",
                secret_path = %secret_path,
                "NEOMIND_JWT_SECRET not set — generated and persisted a new JWT secret. \
                 Set NEOMIND_JWT_SECRET env var for multi-instance deployments."
            );
            new_secret
        });

        // Load users from database
        // If no users exist, the setup wizard will handle creating the first admin
        let users = Self::load_users_from_db(db_path).unwrap_or_default();
        // Self-registration flag: persisted so an admin's choice survives
        // restarts; missing row = default (closed).
        let allow_registration = Self::load_allow_registration_from_db(db_path);
        // Load persisted sessions so tokens issued before this boot stay
        // valid (expired ones are dropped + purged inside the loader).
        let sessions = Self::load_sessions_from_db(db_path);
        if !sessions.is_empty() {
            info!(
                category = "auth",
                count = sessions.len(),
                "Restored {} active session(s) from database (restart-surviving logins)",
                sessions.len()
            );
        }

        if users.is_empty() {
            info!(
                category = "auth",
                "No users found. Setup wizard will be shown to create admin account."
            );
        }

        Self {
            users: Arc::new(RwLock::new(users)),
            sessions: Arc::new(std::sync::RwLock::new(sessions)),
            db_path,
            jwt_secret,
            session_duration: 7 * 24 * 60 * 60, // 7 days
            allow_registration: Arc::new(std::sync::RwLock::new(allow_registration)),
            auth_throttle: Arc::new(AuthThrottle::production()),
        }
    }

    /// Create a new auth state with custom configuration (for testing).
    pub fn with_config(db_path: String, jwt_secret: String) -> Self {
        let users = Self::load_users_from_db(&db_path).unwrap_or_default();
        let sessions = Self::load_sessions_from_db(&db_path);
        let allow_registration = Self::load_allow_registration_from_db(&db_path);
        // Leak the strings to get &'static str for db_path
        let db_path_static: &'static str = Box::leak(db_path.into_boxed_str());
        let jwt_secret_owned = jwt_secret;

        Self {
            users: Arc::new(RwLock::new(users)),
            sessions: Arc::new(std::sync::RwLock::new(sessions)),
            db_path: db_path_static,
            jwt_secret: jwt_secret_owned,
            session_duration: 7 * 24 * 60 * 60,
            allow_registration: Arc::new(std::sync::RwLock::new(allow_registration)),
            auth_throttle: Arc::new(AuthThrottle::production()),
        }
    }

    /// Create a new auth state with in-memory storage (for testing).
    ///
    /// This version uses a placeholder database path and doesn't persist to disk.
    /// All user data is lost when the state is dropped.
    pub fn new_with_memory_store() -> Self {
        let jwt_secret = "test_jwt_secret_for_testing_only".to_string();

        Self {
            users: Arc::new(RwLock::new(HashMap::new())),
            sessions: Arc::new(std::sync::RwLock::new(HashMap::new())),
            db_path: ":memory:", // Placeholder, won't be used
            jwt_secret,
            session_duration: 7 * 24 * 60 * 60,
            allow_registration: Arc::new(std::sync::RwLock::new(false)),
            auth_throttle: Arc::new(AuthThrottle::production()),
        }
    }

    /// Load users from database.
    /// Returns empty HashMap if database doesn't exist yet (first run).
    fn load_users_from_db(path: &str) -> Result<HashMap<String, User>, Box<dyn std::error::Error>> {
        // Check if file exists first
        if !std::path::Path::new(path).exists() {
            return Ok(HashMap::new());
        }

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
            info!(
                category = "auth",
                count = users.len(),
                "Loaded {} user(s) from database",
                users.len()
            );
        }

        Ok(users)
    }

    /// Load persisted sessions at boot, keeping only unexpired ones and
    /// purging expired rows. Errors are non-fatal: an unreadable session
    /// table degrades to "everyone re-logins" (the old behavior), never to
    /// a failed boot.
    fn load_sessions_from_db(path: &str) -> HashMap<String, SessionInfo> {
        let mut out: HashMap<String, SessionInfo> = HashMap::new();
        if path == ":memory:" || !std::path::Path::new(path).exists() {
            return out;
        }
        let now = chrono::Utc::now().timestamp();
        let mut expired: Vec<String> = Vec::new();

        if let Ok(db) = Database::open(path) {
            if let Ok(read_txn) = db.begin_read() {
                if let Ok(table) = read_txn.open_table(SESSIONS_TABLE) {
                    if let Ok(iter) = table.iter() {
                        for item in iter.flatten() {
                            let (k, v) = item;
                            if let Ok(info) = bincode::deserialize::<SessionInfo>(v.value()) {
                                // Same grace as the login-time reaper, so a
                                // boot never drops a session whose token is
                                // still inside the clock-skew window.
                                if info.expires_at + JWT_CLOCK_SKEW_SECS > now {
                                    out.insert(k.value().to_string(), info);
                                } else {
                                    expired.push(k.value().to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
        if !expired.is_empty() {
            if let Ok(db) = Database::open(path) {
                if let Ok(w) = db.begin_write() {
                    if let Ok(mut t) = w.open_table(SESSIONS_TABLE) {
                        for k in &expired {
                            let _ = t.remove(k.as_str());
                        }
                    }
                    let _ = w.commit();
                }
            }
        }
        out
    }

    /// Persist one session row (write-through on login/register). Best-effort:
    /// a failed write only means the session won't survive a restart.
    fn save_session_to_db(path: &str, key: &str, info: &SessionInfo) {
        if path == ":memory:" {
            return;
        }
        let Ok(bytes) = bincode::serialize(info) else {
            return;
        };
        if let Some(parent) = std::path::Path::new(path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let db = if std::path::Path::new(path).exists() {
            match Database::open(path) {
                Ok(d) => d,
                Err(_) => return,
            }
        } else {
            match Database::create(path) {
                Ok(d) => d,
                Err(_) => return,
            }
        };
        if let Ok(w) = db.begin_write() {
            let mut inserted = false;
            if let Ok(mut t) = w.open_table(SESSIONS_TABLE) {
                inserted = t.insert(key, bytes.as_slice()).is_ok();
            }
            if inserted {
                let _ = w.commit();
            }
        }
    }

    /// Remove one session row (write-through on logout). Best-effort.
    /// Delete every persisted session belonging to `username` (sweeps the
    /// sessions table directly — covers rows not present in memory). Used by
    /// delete_user / change_password so a deleted or re-credentialed user's
    /// JWTs die immediately instead of surviving up to `session_duration`.
    fn delete_user_sessions_from_db(path: &str, username: &str) {
        if path == ":memory:" || !std::path::Path::new(path).exists() {
            return;
        }
        let Ok(db) = Database::open(path) else {
            return;
        };
        // Collect matching keys (read), then delete (write) — redb cannot
        // mutate while iterating the same table.
        let mut keys_to_remove: Vec<String> = Vec::new();
        if let Ok(read_txn) = db.begin_read() {
            if let Ok(table) = read_txn.open_table(SESSIONS_TABLE) {
                if let Ok(iter) = table.iter() {
                    for item in iter.flatten() {
                        let (k, v) = item;
                        let ok_match = bincode::deserialize::<SessionInfo>(v.value())
                            .map(|info| info.username == username)
                            .unwrap_or(false);
                        if ok_match {
                            keys_to_remove.push(k.value().to_string());
                        }
                    }
                }
            }
        }
        if keys_to_remove.is_empty() {
            return;
        }
        if let Ok(w) = db.begin_write() {
            if let Ok(mut t) = w.open_table(SESSIONS_TABLE) {
                for k in &keys_to_remove {
                    let _ = t.remove(k.as_str());
                }
            }
            let _ = w.commit();
        }
    }

    /// Revoke all of `username`'s sessions — in-memory map AND persisted
    /// rows. Called on user deletion and password change.
    fn revoke_user_sessions(&self, username: &str) {
        let keys: Vec<String> = {
            let sessions = self.sessions.read().unwrap();
            sessions
                .iter()
                .filter(|(_, info)| info.username == username)
                .map(|(k, _)| k.clone())
                .collect()
        };
        if !keys.is_empty() {
            self.sessions
                .write()
                .unwrap()
                .retain(|_, info| info.username != username);
        }
        Self::delete_user_sessions_from_db(self.db_path, username);
        info!(
            category = "auth",
            username = username,
            count = keys.len(),
            "Revoked user sessions"
        );
    }

    fn delete_session_from_db(path: &str, key: &str) {
        if path == ":memory:" || !std::path::Path::new(path).exists() {
            return;
        }
        let Ok(db) = Database::open(path) else {
            return;
        };
        if let Ok(w) = db.begin_write() {
            if let Ok(mut t) = w.open_table(SESSIONS_TABLE) {
                let _ = t.remove(key);
            }
            let _ = w.commit();
        }
    }

    /// Whether unauthenticated self-registration is currently allowed.
    pub fn allow_registration(&self) -> bool {
        *self
            .allow_registration
            .read()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// Set whether unauthenticated self-registration is allowed (admin
    /// action). Persists to users.redb so the choice survives restarts;
    /// in-memory test stores only update the flag.
    pub fn set_allow_registration(&self, allow: bool) {
        *self
            .allow_registration
            .write()
            .unwrap_or_else(|e| e.into_inner()) = allow;
        if let Err(e) = Self::save_allow_registration_to_db(self.db_path, allow) {
            tracing::error!(
                category = "auth",
                error = %e,
                "Failed to persist allow_registration setting — it will revert on restart"
            );
        }
    }

    /// Load the persisted self-registration flag. Missing row/database = the
    /// default (closed).
    fn load_allow_registration_from_db(path: &str) -> bool {
        if !std::path::Path::new(path).exists() {
            return false;
        }
        let Ok(db) = Database::open(path) else {
            return false;
        };
        let Ok(read_txn) = db.begin_read() else {
            return false;
        };
        let Ok(table) = read_txn.open_table(AUTH_SETTINGS_TABLE) else {
            return false;
        };
        matches!(table.get("allow_registration"), Ok(Some(v)) if v.value() == "true")
    }

    /// Persist the self-registration flag (mirrors `save_user_to_db`).
    fn save_allow_registration_to_db(
        path: &str,
        allow: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if path == ":memory:" {
            return Ok(());
        }
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = if std::path::Path::new(path).exists() {
            Database::open(path)?
        } else {
            Database::create(path)?
        };
        let write_txn = db.begin_write()?;
        {
            let mut table = write_txn.open_table(AUTH_SETTINGS_TABLE)?;
            table.insert("allow_registration", if allow { "true" } else { "false" })?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Save user to database synchronously.
    /// This ensures data is persisted before returning.
    fn save_user_to_db(path: &str, user: &User) -> Result<(), Box<dyn std::error::Error>> {
        // Skip database operations for in-memory storage
        if path == ":memory:" {
            return Ok(());
        }

        let username = user.username.clone();
        let user_bytes = bincode::serialize(user)?;

        // Ensure parent directory exists
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Open or create database (redb requires create() for new files)
        let db = if std::path::Path::new(path).exists() {
            Database::open(path)?
        } else {
            Database::create(path)?
        };

        let write_txn = db.begin_write()?;
        {
            let mut table = write_txn.open_table(USERS_TABLE)?;
            table.insert(username.as_str(), user_bytes.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Remove a user from the database (mirrors `save_user_to_db`).
    /// No-op for in-memory storage. Used by `delete_user` so deletions survive
    /// a restart (previously only the in-memory map was mutated, so deleted
    /// users — including admins removed for security — resurrected on restart).
    fn delete_user_from_db(path: &str, username: &str) -> Result<(), Box<dyn std::error::Error>> {
        if path == ":memory:" {
            return Ok(());
        }
        if !std::path::Path::new(path).exists() {
            return Ok(());
        }
        let db = Database::open(path)?;
        let write_txn = db.begin_write()?;
        {
            let mut table = write_txn.open_table(USERS_TABLE)?;
            table.remove(username)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Hash password using bcrypt (secure for production use).
    /// Uses default cost factor (12) which provides good security.
    ///
    /// Returns Err on bcrypt failure (e.g., password >72 bytes, RNG unavailable)
    /// rather than degrading to a plaintext fallback — the previous
    /// `format!("fallback_hash_{}", password)` fallback stored passwords in
    /// cleartext, defeating the purpose of hashing entirely. Callers MUST
    /// surface the error to the user (typically as a 500 / "internal error
    /// during password reset, please try a shorter password").
    fn hash_password(password: &str) -> Result<String, AuthError> {
        bcrypt::hash(password, bcrypt::DEFAULT_COST).map_err(|e| {
            error!(category = "auth", error = %e, "bcrypt password hashing failed");
            AuthError::InvalidInput(format!(
                "Password could not be hashed (likely too long; bcrypt limit is 72 bytes): {}",
                e
            ))
        })
    }

    /// Verify password against bcrypt hash.
    fn verify_password(password: &str, hash: &str) -> bool {
        // Legacy `fallback_hash_<plaintext>` entries from prior versions stored
        // passwords in cleartext. We deliberately refuse to verify against
        // them — affected users must reset their password via admin tooling.
        // (No new entries can be created after the hash_password fix above.)
        if hash.starts_with("fallback_hash_") {
            error!(
                category = "auth",
                "Refusing login against legacy plaintext password fallback — \
                 admin must reset this user's password"
            );
            return false;
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
    fn generate_token(&self, user: &User) -> Result<String, AuthError> {
        let now = chrono::Utc::now().timestamp();
        let expires_at = now + self.session_duration;

        let header =
            BASE64_URL_SAFE_NO_PAD.encode(json!({"alg": "HS256", "typ": "JWT"}).to_string());
        let payload = BASE64_URL_SAFE_NO_PAD.encode(
            json!({
                "sub": user.id,
                "username": user.username,
                "role": user.role.as_str(),
                "iat": now,
                "exp": expires_at,
            })
            .to_string(),
        );
        let signature = {
            let data = format!("{}.{}", header, payload);
            let mut mac = create_hmac(self.jwt_secret.as_bytes())?;
            mac.update(data.as_bytes());
            BASE64_URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
        };

        Ok(format!("{}.{}.{}", header, payload, signature))
    }

    /// Validate JWT token and return session info.
    pub fn validate_token(&self, token: &str) -> Result<SessionInfo, AuthError> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return Err(AuthError::InvalidToken("Invalid token format".into()));
        }

        // Verify signature
        let data = format!("{}.{}", parts[0], parts[1]);
        let mut mac = create_hmac(self.jwt_secret.as_bytes())?;
        mac.update(data.as_bytes());

        let expected_sig = BASE64_URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
        if !crate::auth::constant_time_eq_str(parts[2], &expected_sig) {
            return Err(AuthError::InvalidToken("Invalid signature".into()));
        }

        // Decode payload
        let payload_bytes = BASE64_URL_SAFE_NO_PAD
            .decode(parts[1])
            .map_err(|_| AuthError::InvalidToken("Invalid payload encoding".into()))?;
        let payload_str = String::from_utf8(payload_bytes)
            .map_err(|_| AuthError::InvalidToken("Invalid payload UTF-8".into()))?;
        let payload: serde_json::Value = serde_json::from_str(&payload_str)
            .map_err(|_| AuthError::InvalidToken("Invalid payload JSON".into()))?;

        // Check expiration with ±30s clock-skew tolerance. Without this,
        // clients whose clock is a few seconds ahead of the server get
        // intermittent 401s right at token expiry, and clients behind on
        // slow networks can hit the boundary mid-request. 30s matches the
        // de-facto standard used by most JWT libraries.
        let exp = payload["exp"].as_i64().unwrap_or(0);
        if exp + JWT_CLOCK_SKEW_SECS < chrono::Utc::now().timestamp() {
            return Err(AuthError::ExpiredToken);
        }

        // Stateful revocation: the token must still be in the active sessions
        // map. login/register insert, logout removes — without this check
        // logout is a no-op (a JWT's signature/exp alone cannot be revoked).
        // Map keys are SHA-256(token); the same hash is the persisted row
        // key, so a session restored from disk at boot matches here.
        if !self
            .sessions
            .read()
            .unwrap()
            .contains_key(&token_key(token))
        {
            return Err(AuthError::SessionRevoked);
        }

        Ok(SessionInfo {
            user_id: payload["sub"].as_str().unwrap_or("").to_string(),
            username: payload["username"].as_str().unwrap_or("").to_string(),
            role: UserRole::parse(payload["role"].as_str().unwrap_or("user"))
                .unwrap_or(UserRole::User),
            created_at: payload["iat"].as_i64().unwrap_or(0),
            expires_at: exp,
        })
    }

    /// Register a new user.
    pub async fn register(
        &self,
        username: &str,
        password: &str,
        role: UserRole,
    ) -> Result<(UserInfo, String), AuthError> {
        // Validate username
        if username.len() < 3 {
            return Err(AuthError::InvalidInput(
                "Username must be at least 3 characters".into(),
            ));
        }
        if password.len() < 6 {
            return Err(AuthError::InvalidInput(
                "Password must be at least 6 characters".into(),
            ));
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
            password_hash: Self::hash_password(password)?,
            role: role.clone(),
            created_at: chrono::Utc::now().timestamp(),
            last_login: None,
            active: true,
        };

        // Save to database synchronously (ensures persistence before returning)
        if let Err(e) = Self::save_user_to_db(self.db_path, &user) {
            error!(category = "auth", username = username, error = %e, "Failed to save user to database");
            return Err(AuthError::DatabaseError(format!(
                "Failed to save user: {}",
                e
            )));
        }

        // Add to in-memory cache after successful DB save
        let mut users = self.users.write().await;
        users.insert(username.to_string(), user.clone());
        drop(users);

        // Generate token
        let token = self.generate_token(&user)?;

        // Register issues a usable token — insert into the active sessions
        // map so validate_token accepts it (same as login). Without this the
        // stateful-revocation check added to validate_token would reject it.
        {
            let session_info = SessionInfo {
                user_id: user.id.clone(),
                username: user.username.clone(),
                role: user.role.clone(),
                created_at: chrono::Utc::now().timestamp(),
                expires_at: chrono::Utc::now().timestamp() + self.session_duration,
            };
            let key = token_key(&token);
            Self::save_session_to_db(self.db_path, &key, &session_info);
            self.sessions.write().unwrap().insert(key, session_info);
        }

        info!(
            category = "auth",
            username = username,
            role = role.as_str(),
            "User registered"
        );

        Ok((
            UserInfo {
                id: user.id,
                username: user.username.clone(),
                role: user.role,
                created_at: user.created_at,
            },
            token,
        ))
    }

    /// Check the login brute-force throttle. Blocks ([`AuthError::TooManyAttempts`])
    /// when either the username key or the IP key has too many recent
    /// credential failures. `ip` is the client address for HTTP callers;
    /// `None` for in-process callers (CLI/tests), which skips the IP key.
    pub fn check_login_throttle(&self, username: &str, ip: Option<&str>) -> Result<(), AuthError> {
        // Cap the key: the request body allows ~2MB usernames and the key is
        // retained in the counter map for the window — unbounded keys turn
        // the throttle itself into a memory-exhaustion vector.
        let username: String = username.chars().take(64).collect();
        let throttle = &self.auth_throttle;
        throttle
            .login_failures
            .check(&format!("fail:u:{username}"))
            .map_err(AuthError::TooManyAttempts)?;
        if let Some(ip) = ip {
            throttle
                .login_failures
                .check(&format!("fail:ip:{ip}"))
                .map_err(AuthError::TooManyAttempts)?;
        }
        Ok(())
    }

    /// Count a login failure against both keys. Only call this for credential
    /// failures (wrong password / unknown user) — a database error must not
    /// punish the user trying to log in.
    pub fn record_login_failure(&self, username: &str, ip: Option<&str>) {
        let username: String = username.chars().take(64).collect();
        let throttle = &self.auth_throttle;
        throttle
            .login_failures
            .record(&format!("fail:u:{username}"));
        if let Some(ip) = ip {
            throttle.login_failures.record(&format!("fail:ip:{ip}"));
        }
    }

    /// Clear the login throttle after a successful login (both keys) — an
    /// honest user who mistyped a few times starts clean.
    pub fn clear_login_throttle(&self, username: &str, ip: Option<&str>) {
        let username: String = username.chars().take(64).collect();
        let throttle = &self.auth_throttle;
        throttle.login_failures.clear(&format!("fail:u:{username}"));
        if let Some(ip) = ip {
            throttle.login_failures.clear(&format!("fail:ip:{ip}"));
        }
    }

    /// Check the signup throttle for a public account-creating endpoint.
    /// `action` namespaces the key (`"reg"` for self-service register,
    /// `"setup"` for first-run admin initialization); every attempt counts.
    /// In-process callers (`ip: None`) skip throttling — the admin CLI is not
    /// a brute-force surface.
    pub fn check_signup_throttle(&self, action: &str, ip: Option<&str>) -> Result<(), AuthError> {
        if let Some(ip) = ip {
            self.auth_throttle
                .signup_attempts
                .check(&format!("{action}:ip:{ip}"))
                .map_err(AuthError::TooManyAttempts)?;
        }
        Ok(())
    }

    /// Count one register/setup attempt (called whether or not it succeeds).
    pub fn record_signup_attempt(&self, action: &str, ip: Option<&str>) {
        if let Some(ip) = ip {
            self.auth_throttle
                .signup_attempts
                .record(&format!("{action}:ip:{ip}"));
        }
    }

    /// Login user and return token.
    pub async fn login(&self, username: &str, password: &str) -> Result<LoginResponse, AuthError> {
        // Clone user data before releasing lock
        let (user_id, user_role, user_created_at) = {
            let users = self.users.read().await;
            let user = users.get(username).ok_or(AuthError::InvalidCredentials)?;

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
            let user = users.get(username).ok_or(AuthError::UserNotFound)?;
            self.generate_token(user)?
        };

        // Store session
        let session_info = SessionInfo {
            user_id: user_id.clone(),
            username: username.to_string(),
            role: user_role.clone(),
            created_at: chrono::Utc::now().timestamp(),
            expires_at: chrono::Utc::now().timestamp() + self.session_duration,
        };
        let mut sessions = self.sessions.write().unwrap();
        let key = token_key(&token);
        Self::save_session_to_db(self.db_path, &key, &session_info);
        sessions.insert(key, session_info);
        // Piggyback: reap expired sessions on each login (was: never cleaned →
        // unbounded growth over months of operation). Honor the same
        // JWT_CLOCK_SKEW_SECS grace the validator grants, so a login-triggered
        // reap never evicts a session whose token is still within that grace
        // window (would wrongly `SessionRevoked` a valid token at the tail of
        // its lifetime).
        let now = chrono::Utc::now().timestamp();
        sessions.retain(|_, info| info.expires_at + JWT_CLOCK_SKEW_SECS > now);
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
        let key = token_key(token);
        Self::delete_session_from_db(self.db_path, &key);
        self.sessions.write().unwrap().remove(&key);
        Ok(())
    }

    /// List all users (admin only).
    pub async fn list_users(&self) -> Vec<UserInfo> {
        let users = self.users.read().await;
        users
            .values()
            .map(|u| UserInfo {
                id: u.id.clone(),
                username: u.username.clone(),
                role: u.role.clone(),
                created_at: u.created_at,
            })
            .collect()
    }

    /// Delete user.
    pub async fn delete_user(&self, username: &str) -> Result<(), AuthError> {
        let mut users = self.users.write().await;
        // Verify existence first (UserNotFound semantics), but persist the
        // deletion BEFORE mutating memory — mirrors change_password's atomic
        // ordering. If the DB write fails the in-memory map stays intact, so
        // the user does NOT silently disappear from list_users only to
        // resurrect from users.redb on the next restart (the exact bug this
        // path previously had when it removed from memory first).
        if !users.contains_key(username) {
            return Err(AuthError::UserNotFound);
        }
        if let Err(e) = Self::delete_user_from_db(self.db_path, username) {
            error!(category = "auth", username = username, error = %e, "Failed to persist user deletion");
            return Err(AuthError::DatabaseError(format!(
                "Failed to delete user: {}",
                e
            )));
        }
        users.remove(username);
        drop(users);
        // The user is gone — their JWTs must die with them, not survive in
        // the session whitelist for up to `session_duration` (7 days).
        self.revoke_user_sessions(username);
        Ok(())
    }

    /// Change password.
    pub async fn change_password(
        &self,
        username: &str,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), AuthError> {
        if new_password.len() < 6 {
            return Err(AuthError::InvalidInput(
                "Password must be at least 6 characters".into(),
            ));
        }

        let mut users = self.users.write().await;
        let user = users.get_mut(username).ok_or(AuthError::UserNotFound)?;

        if !Self::verify_password(old_password, &user.password_hash) {
            return Err(AuthError::InvalidCredentials);
        }

        let new_hash = Self::hash_password(new_password)?;
        // Persist FIRST (atomic): if the db write fails, the in-memory password
        // stays unchanged. Previously the change was in-memory only and silently
        // reverted on restart.
        let mut persisted = user.clone();
        persisted.password_hash = new_hash.clone();
        if let Err(e) = Self::save_user_to_db(self.db_path, &persisted) {
            error!(category = "auth", username = username, error = %e, "Failed to persist password change");
            return Err(AuthError::DatabaseError(format!(
                "Failed to change password: {}",
                e
            )));
        }
        user.password_hash = new_hash;
        drop(users);

        // Password changed — invalidate all existing sessions for this user.
        // Tokens minted before the change must not remain valid (a leaked
        // token from the old-password era otherwise survives the rotation).
        self.revoke_user_sessions(username);

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
    /// Token was revoked via logout — not in the active sessions map.
    SessionRevoked,
    InvalidInput(String),
    DatabaseError(String),
    /// Self-registration is disabled on this instance (admin toggle —
    /// `PUT /api/settings/registration`).
    RegistrationDisabled,
    /// Brute-force throttle engaged (login failures / signup flood).
    /// Carries the seconds until the oldest counted event leaves the window.
    TooManyAttempts(u64),
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
            AuthError::SessionRevoked => write!(f, "Session has been revoked"),
            AuthError::InvalidInput(msg) => write!(f, "Invalid input: {}", msg),
            AuthError::DatabaseError(msg) => write!(f, "Database error: {}", msg),
            AuthError::RegistrationDisabled => {
                write!(
                    f,
                    "Self-registration is disabled. Ask an administrator to create your account."
                )
            }
            AuthError::TooManyAttempts(secs) => {
                write!(f, "Too many attempts — try again in {} seconds", secs)
            }
        }
    }
}

impl std::error::Error for AuthError {}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, message): (HttpStatusCode, String) = match self {
            AuthError::InvalidCredentials => (
                HttpStatusCode::UNAUTHORIZED,
                "Invalid username or password".into(),
            ),
            AuthError::UserExists => (HttpStatusCode::CONFLICT, "User already exists".into()),
            AuthError::UserNotFound => (HttpStatusCode::NOT_FOUND, "User not found".into()),
            AuthError::UserDisabled => {
                (HttpStatusCode::FORBIDDEN, "User account is disabled".into())
            }
            AuthError::InvalidToken(msg) => (HttpStatusCode::UNAUTHORIZED, msg),
            AuthError::ExpiredToken => (HttpStatusCode::UNAUTHORIZED, "Token has expired".into()),
            AuthError::SessionRevoked => (
                HttpStatusCode::UNAUTHORIZED,
                "Session has been revoked".into(),
            ),
            AuthError::InvalidInput(msg) => (HttpStatusCode::BAD_REQUEST, msg),
            AuthError::DatabaseError(msg) => (HttpStatusCode::INTERNAL_SERVER_ERROR, msg),
            AuthError::RegistrationDisabled => (
                HttpStatusCode::FORBIDDEN,
                "Self-registration is disabled. Ask an administrator to create your account."
                    .into(),
            ),
            // 429 + Retry-After so well-behaved clients back off on their own.
            AuthError::TooManyAttempts(secs) => {
                let resp = (
                    HttpStatusCode::TOO_MANY_REQUESTS,
                    [("Retry-After", secs.to_string())],
                    Json(serde_json::json!({
                        "error": format!("Too many attempts — try again in {secs} seconds"),
                        "status": 429,
                        "retry_after": secs,
                    })),
                )
                    .into_response();
                return resp;
            }
        };

        let body = serde_json::json!({
            "error": message,
            "status": status.as_u16(),
        });

        (status, Json(body)).into_response()
    }
}

/// Client IP for auth throttling: prefer the proxy headers (production sits
/// behind nginx, where the socket address is always the proxy itself and a
/// per-IP key would rate-limit every user as one client), fall back to the
/// socket address.
///
/// Trust caveat: a *direct*-connection attacker can forge these headers to
/// mint fresh per-IP keys. That defeats the IP half of the login throttle,
/// but the per-username half — the one that protects a single account from
/// distributed guessing — is keyed off the username and unaffected.
pub fn client_ip_for_throttle(headers: &HeaderMap, addr: &std::net::SocketAddr) -> String {
    if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if let Some(first) = xff.split(',').next() {
            let first = first.trim();
            if !first.is_empty() {
                return first.to_string();
            }
        }
    }
    if let Some(rip) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
        let rip = rip.trim();
        if !rip.is_empty() {
            return rip.to_string();
        }
    }
    addr.ip().to_string()
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

    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or_else(|| AuthError::InvalidToken("Invalid Authorization format".into()))?;

    // Validate token using auth_user_state from ServerState
    let session_info = state.auth.user_state.validate_token(token)?;

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
            if let Ok(session_info) = state.auth.user_state.validate_token(token) {
                req.extensions_mut().insert(session_info);
            }
        }
    }

    next.run(req).await
}

/// Extract user info from request extensions.
/// Use this with axum's Extension extractor:
/// ```rust,ignore
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

    fn get_project_data_path(filename: &str) -> std::path::PathBuf {
        // Use temp directory for test databases to avoid polluting the project
        std::env::temp_dir().join(format!(
            "neomind_test_{}_{}.redb",
            filename.replace(".redb", ""),
            std::process::id()
        ))
    }

    fn cleanup_test_db(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(format!("{}.lock", path.display()));
    }

    fn make_test_auth(test_name: &str) -> (AuthUserState, std::path::PathBuf) {
        let db_path = get_project_data_path(&format!("test_{}.redb", test_name));
        cleanup_test_db(&db_path);
        let jwt_secret = std::env::var("NEOMIND_JWT_SECRET")
            .unwrap_or_else(|_| "test_secret_key_12345678".to_string());
        (
            AuthUserState::with_config(db_path.display().to_string(), jwt_secret),
            db_path,
        )
    }

    #[tokio::test]
    async fn test_user_registration() {
        let (auth, db_path) = make_test_auth("registration");
        let (user, token) = auth
            .register("testuser", "password123", UserRole::User)
            .await
            .unwrap();
        assert_eq!(user.username, "testuser");
        assert!(!token.is_empty());
        cleanup_test_db(&db_path);
    }

    #[tokio::test]
    async fn test_user_login() {
        let (auth, _) = make_test_auth("login");
        auth.register("testuser", "password123", UserRole::User)
            .await
            .unwrap();

        let response = auth.login("testuser", "password123").await.unwrap();
        assert_eq!(response.user.username, "testuser");
        assert!(!response.token.is_empty());
    }

    /// Password change must revoke the user's sessions — a token minted before
    /// the change surviving the rotation would keep a leaked token alive.
    #[tokio::test]
    async fn test_change_password_revokes_sessions() {
        let (auth, _) = make_test_auth("pw_revoke");
        auth.register("alice", "oldpass", UserRole::User)
            .await
            .unwrap();
        let token = auth.login("alice", "oldpass").await.unwrap().token;
        assert!(auth.validate_token(&token).is_ok());

        auth.change_password("alice", "oldpass", "newpass123")
            .await
            .unwrap();

        assert!(
            matches!(auth.validate_token(&token), Err(AuthError::SessionRevoked)),
            "old-era token must be dead after password change"
        );
        // The new password works and issues a fresh valid token.
        let fresh = auth.login("alice", "newpass123").await.unwrap().token;
        assert!(auth.validate_token(&fresh).is_ok());
    }

    /// Deleting a user must kill their sessions immediately — not leave them
    /// valid in the whitelist for up to `session_duration`.
    #[tokio::test]
    async fn test_delete_user_revokes_sessions() {
        let (auth, _) = make_test_auth("del_revoke");
        auth.register("bob", "pass123", UserRole::User)
            .await
            .unwrap();
        let token = auth.login("bob", "pass123").await.unwrap().token;
        assert!(auth.validate_token(&token).is_ok());

        auth.delete_user("bob").await.unwrap();

        assert!(
            matches!(auth.validate_token(&token), Err(AuthError::SessionRevoked)),
            "deleted user's token must be revoked"
        );
    }

    /// Test auth with tight throttle windows (3 events / `window_ms`).
    fn make_test_auth_throttled(
        test_name: &str,
        window_ms: u64,
    ) -> (AuthUserState, std::path::PathBuf) {
        let (mut auth, db_path) = make_test_auth(test_name);
        auth.auth_throttle = Arc::new(AuthThrottle {
            login_failures: SlidingWindowCounter::new(
                3,
                std::time::Duration::from_millis(window_ms),
            ),
            signup_attempts: SlidingWindowCounter::new(
                3,
                std::time::Duration::from_millis(window_ms),
            ),
        });
        (auth, db_path)
    }

    /// Once the failure cap is hit, even the CORRECT password is rejected —
    /// the throttle must not become an oracle that reveals when guessing works.
    #[tokio::test]
    async fn test_login_throttle_blocks_after_failures() {
        let (auth, _) = make_test_auth_throttled("throttle_block", 60_000);
        auth.register("carol", "rightpass", UserRole::User)
            .await
            .unwrap();

        for _ in 0..3 {
            assert!(matches!(
                auth.login("carol", "wrongpass").await,
                Err(AuthError::InvalidCredentials)
            ));
            auth.record_login_failure("carol", None);
        }

        assert!(
            matches!(
                auth.check_login_throttle("carol", None),
                Err(AuthError::TooManyAttempts(_))
            ),
            "4th attempt must be throttled even with the correct password"
        );
    }

    /// A successful login clears the user's failure count — an honest user
    /// who mistypes a few times then gets it right is not one failure from
    /// a lockout.
    #[tokio::test]
    async fn test_login_throttle_success_clears() {
        let (auth, _) = make_test_auth_throttled("throttle_clear", 60_000);
        auth.register("dave", "rightpass", UserRole::User)
            .await
            .unwrap();

        for _ in 0..2 {
            assert!(auth.login("dave", "wrongpass").await.is_err());
            auth.record_login_failure("dave", None);
        }
        // Below the cap (2 < 3) and the correct password still works…
        assert!(auth.login("dave", "rightpass").await.is_ok());
        auth.clear_login_throttle("dave", None);
        // …and the counter starts fresh: two more failures leave room for one.
        for _ in 0..2 {
            assert!(auth.login("dave", "wrongpass").await.is_err());
            auth.record_login_failure("dave", None);
        }
        assert!(auth.check_login_throttle("dave", None).is_ok());
    }

    /// The username key is per-account: a locked-out user does not lock out
    /// everyone else (in-process callers have no IP key).
    #[tokio::test]
    async fn test_login_throttle_per_username_isolation() {
        let (auth, _) = make_test_auth_throttled("throttle_isolation", 60_000);
        auth.register("eve", "pass123", UserRole::User)
            .await
            .unwrap();
        auth.register("frank", "pass456", UserRole::User)
            .await
            .unwrap();

        for _ in 0..3 {
            assert!(auth.login("eve", "nope").await.is_err());
            auth.record_login_failure("eve", None);
        }
        assert!(matches!(
            auth.check_login_throttle("eve", None),
            Err(AuthError::TooManyAttempts(_))
        ));
        // Frank is untouched by Eve's failures.
        assert!(auth.check_login_throttle("frank", None).is_ok());
        assert!(auth.login("frank", "pass456").await.is_ok());
    }

    /// The IP key is shared across usernames: one host spraying many accounts
    /// (password spraying) gets the whole IP throttled.
    #[tokio::test]
    async fn test_login_throttle_ip_key_spans_users() {
        let (auth, _) = make_test_auth_throttled("throttle_ip", 60_000);
        auth.register("gina", "pass123", UserRole::User)
            .await
            .unwrap();
        auth.register("hank", "pass456", UserRole::User)
            .await
            .unwrap();

        for _ in 0..3 {
            assert!(auth.login("gina", "nope").await.is_err());
            auth.record_login_failure("gina", Some("10.0.0.1"));
        }
        // Hank never failed, but the shared IP has 3 failures.
        assert!(matches!(
            auth.check_login_throttle("hank", Some("10.0.0.1")),
            Err(AuthError::TooManyAttempts(_))
        ));
        // …while Hank from a different address is fine.
        assert!(auth.check_login_throttle("hank", Some("10.0.0.2")).is_ok());
    }

    /// The lockout is a sliding window, not a permanent ban: once the oldest
    /// failure ages out, the account works again.
    #[tokio::test]
    async fn test_login_throttle_window_expires() {
        // Window must comfortably exceed the ~3 argon2 verifications above it
        // (each deliberate-failure login runs a full password hash).
        let (auth, _) = make_test_auth_throttled("throttle_expiry", 1_000);
        auth.register("iris", "pass123", UserRole::User)
            .await
            .unwrap();

        for _ in 0..3 {
            assert!(auth.login("iris", "nope").await.is_err());
            auth.record_login_failure("iris", None);
        }
        assert!(matches!(
            auth.check_login_throttle("iris", None),
            Err(AuthError::TooManyAttempts(_))
        ));

        tokio::time::sleep(std::time::Duration::from_millis(1_200)).await;
        assert!(auth.check_login_throttle("iris", None).is_ok());
        assert!(auth.login("iris", "pass123").await.is_ok());
    }

    /// Signup endpoints count EVERY attempt (each creates an account) — no
    /// failure-awareness, no per-username key.
    #[test]
    fn test_signup_throttle_counts_every_attempt() {
        let (auth, _) = make_test_auth_throttled("signup_throttle", 60_000);
        for i in 0..3 {
            assert!(auth.check_signup_throttle("reg", Some("10.1.1.1")).is_ok());
            auth.record_signup_attempt("reg", Some("10.1.1.1"));
            let _ = i;
        }
        assert!(matches!(
            auth.check_signup_throttle("reg", Some("10.1.1.1")),
            Err(AuthError::TooManyAttempts(_))
        ));
        // Different action namespace / IP unaffected; in-process callers skip.
        assert!(auth
            .check_signup_throttle("setup", Some("10.1.1.1"))
            .is_ok());
        assert!(auth.check_signup_throttle("reg", Some("10.1.1.2")).is_ok());
        assert!(auth.check_signup_throttle("reg", None).is_ok());
    }

    #[tokio::test]
    async fn test_session_survives_restart() {
        // The whole point of session persistence: a token issued before a
        // restart must stay valid after it (same db + same secret), unless
        // logged out. Previously the in-memory allowlist was rebuilt empty
        // on boot → every pre-restart token got SessionRevoked.
        let (auth, db_path) = make_test_auth("session_restart");
        auth.register("testuser", "password123", UserRole::User)
            .await
            .unwrap();
        let response = auth.login("testuser", "password123").await.unwrap();
        let token = response.token;

        // Simulate a restart: brand-new state from the same database.
        let restarted = AuthUserState::with_config(
            db_path.display().to_string(),
            "test_secret_key_12345678".to_string(),
        );
        assert!(
            restarted.validate_token(&token).is_ok(),
            "token must survive a restart"
        );

        // Logout revokes persistently too: a further restart must NOT
        // resurrect the logged-out session.
        restarted.logout(&token).await.unwrap();
        let restarted2 = AuthUserState::with_config(
            db_path.display().to_string(),
            "test_secret_key_12345678".to_string(),
        );
        assert!(
            restarted2.validate_token(&token).is_err(),
            "logged-out token must stay revoked across restarts"
        );
    }

    #[tokio::test]
    async fn test_token_validation() {
        let (auth, db_path) = make_test_auth("token_validation");
        let (_, token) = auth
            .register("testuser", "password123", UserRole::User)
            .await
            .unwrap();

        let session = auth.validate_token(&token).unwrap();
        assert_eq!(session.username, "testuser");
        cleanup_test_db(&db_path);
    }

    #[tokio::test]
    async fn test_logout_revokes_token() {
        // Regression: logout must actually invalidate the JWT, not just remove
        // a dead sessions-map entry. validate_token consults the map now, so a
        // logged-out token is rejected with SessionRevoked.
        let (auth, db_path) = make_test_auth("logout_revokes");
        auth.register("testuser", "password123", UserRole::User)
            .await
            .unwrap();
        let resp = auth.login("testuser", "password123").await.unwrap();
        let token = resp.token;

        // Valid before logout
        assert!(auth.validate_token(&token).is_ok());

        // Logout revokes — token no longer validates
        auth.logout(&token).await.unwrap();
        assert!(matches!(
            auth.validate_token(&token),
            Err(AuthError::SessionRevoked)
        ));
        cleanup_test_db(&db_path);
    }

    #[tokio::test]
    async fn test_cli_reset_password_roundtrip_across_crates() {
        // Proves `neomind user reset-password` (neomind-cli-ops, offline) can
        // read a user row written by this crate and write one this crate can
        // read back — i.e. the duplicated User struct + bincode format in
        // neomind-cli-ops/src/user_cmd.rs stays in lockstep with ours.
        let data_dir =
            std::env::temp_dir().join(format!("neomind_cross_reset_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data_dir);
        std::fs::create_dir_all(&data_dir).unwrap();
        let db_path = data_dir.join("users.redb");
        let db_str = db_path.display().to_string();
        let dir_str = data_dir.to_str().unwrap();

        let auth =
            AuthUserState::with_config(db_str.clone(), "test_secret_key_12345678".to_string());
        auth.register("admin", "oldpass123", UserRole::Admin)
            .await
            .unwrap();

        // Offline reset via the CLI ops module (no running server).
        neomind_cli_ops::user_cmd::reset_user_password(dir_str, "admin", "newpass456").unwrap();

        // A fresh server state reloads the row the CLI rewrote.
        let auth2 = AuthUserState::with_config(db_str, "test_secret_key_12345678".to_string());
        assert!(
            auth2.login("admin", "newpass456").await.is_ok(),
            "new password must log in after cli reset"
        );
        assert!(
            auth2.login("admin", "oldpass123").await.is_err(),
            "old password must be rejected after cli reset"
        );

        let _ = std::fs::remove_dir_all(&data_dir);
    }
}
