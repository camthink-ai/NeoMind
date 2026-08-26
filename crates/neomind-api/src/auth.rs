//! API Key authentication middleware.
//!
//! Simple API Key based authentication system for protecting API endpoints.
//! API keys are persisted in the redb database at `data/api_keys.redb`.
//!
//! # Security
//!
//! API keys are stored encrypted using AES-256-GCM. The key is derived from
//! the `NEOMIND_ENCRYPTION_KEY` environment variable or generated randomly
//! (not persistent across restarts).

use std::collections::HashMap;
use std::sync::Arc;

use dashmap::DashMap;
use redb::{Database, ReadableTable, TableDefinition};
use tracing::{error, info, warn};

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::crypto::CryptoService;
use crate::server::ServerState;

// Table definition for API keys storage (encrypted)
const API_KEYS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("api_keys");

// Table definition for API key hashes (for validation)
const API_KEY_HASHES_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("api_key_hashes");

/// API Key information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyInfo {
    /// Unique ID for this key
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Creation timestamp
    pub created_at: i64,
    /// Permissions (simple list, "*" means all).
    ///
    /// DOCUMENTED BEHAVIOR (single-user edge platform): this field is
    /// informational only — it is accepted at creation, stored, and echoed
    /// back, but NEVER enforced. Every API key authenticates as a full
    /// administrator. Treat any key you create as equivalent to the admin
    /// account; revoke keys you don't fully trust.
    pub permissions: Vec<String>,
    /// Whether this key is active
    pub active: bool,
}

/// Authentication state with persistent storage.
#[derive(Clone)]
pub struct AuthState {
    /// API Keys storage (in-memory for fast access) - using DashMap for concurrent access
    /// Maps hash -> (encrypted_key, ApiKeyInfo)
    api_keys: Arc<DashMap<String, (String, ApiKeyInfo)>>,
    /// Database path for persistence
    db_path: String,
    /// Cryptographic service for key encryption
    crypto: Arc<CryptoService>,
}

impl AuthState {
    /// Create a new auth state with persistent storage.
    /// Loads existing keys from database, or creates a default key if none exist.
    pub fn new() -> Self {
        let db_path = "data/api_keys.redb";
        let crypto = Arc::new(CryptoService::from_env_or_generate());

        // Ensure data directory exists
        if let Some(parent) = std::path::Path::new(db_path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        // Try to load from database first
        let keys = Self::load_from_db(db_path, &crypto).unwrap_or_else(|e| {
            warn!(category = "auth", error = %e, "Failed to load API keys from database, using defaults");
            Self::load_default_keys(&crypto)
        });

        // If no keys exist, generate a default one
        let keys = if keys.is_empty() {
            info!(
                category = "auth",
                "No API keys found, generating default key"
            );
            Self::generate_default_key(&crypto)
        } else {
            keys
        };

        let state = Self {
            api_keys: Arc::new(DashMap::from_iter(keys)),
            db_path: db_path.to_string(),
            crypto,
        };

        // Persist keys to database (ensures newly generated keys are saved)
        if let Err(e) = state.save_to_db(db_path) {
            warn!(category = "auth", error = %e, "Failed to persist API keys to database");
        }

        state
    }

    /// Create a new auth state for testing.
    ///
    /// This creates an in-memory auth state without any API keys,
    /// suitable for parallel test execution.
    #[cfg(any(test, feature = "testing"))]
    pub fn new_for_testing() -> Self {
        let crypto = Arc::new(CryptoService::from_env_or_generate());

        Self {
            api_keys: Arc::new(DashMap::new()),
            db_path: ":memory:".to_string(),
            crypto,
        }
    }

    /// Create a new auth state with a custom data directory.
    /// Used by the CLI for `--data-dir` support.
    pub fn new_with_data_dir(data_dir: &str) -> Self {
        let db_path = format!("{}/api_keys.redb", data_dir);
        let crypto = Arc::new(CryptoService::from_env_or_generate_with_data_dir(data_dir));

        // Ensure directory exists
        let _ = std::fs::create_dir_all(data_dir);

        let keys = Self::load_from_db(&db_path, &crypto).unwrap_or_else(|e| {
            warn!(category = "auth", error = %e, "Failed to load API keys from database, using defaults");
            Self::load_default_keys(&crypto)
        });

        let keys = if keys.is_empty() {
            info!(
                category = "auth",
                "No API keys found, generating default key"
            );
            Self::generate_default_key_silent(&crypto)
        } else {
            keys
        };

        let state = Self {
            api_keys: Arc::new(DashMap::from_iter(keys)),
            db_path,
            crypto,
        };

        // Persist keys to database (ensures newly generated keys are saved)
        if let Err(e) = state.save_to_db(&state.db_path) {
            warn!(category = "auth", error = %e, "Failed to persist API keys to database");
        }

        state
    }

    /// Load API keys from redb database.
    fn load_from_db(
        path: &str,
        crypto: &CryptoService,
    ) -> Result<HashMap<String, (String, ApiKeyInfo)>, Box<dyn std::error::Error>> {
        let path_ref = std::path::Path::new(path);
        if !path_ref.exists() {
            return Err("Database file does not exist".into());
        }
        let db = Database::open(path_ref)?;
        let read_txn = db.begin_read()?;

        let mut keys = HashMap::new();

        // Load encrypted keys from the new table format
        if let Ok(table) = read_txn.open_table(API_KEYS_TABLE) {
            for item in table.iter()? {
                let (hash, value) = item?;
                let hash_str = hash.value();
                let encrypted = String::from_utf8(value.value().to_vec())?;

                // Decrypt the key (verify it can be decrypted)
                let _decrypted_key = crypto.decrypt(&encrypted)?;

                // Load the metadata from the hashes table
                let info = if let Ok(hash_table) = read_txn.open_table(API_KEY_HASHES_TABLE) {
                    if let Ok(Some(value)) = hash_table.get(hash_str) {
                        bincode::deserialize(value.value())?
                    } else {
                        // Fallback for old format
                        ApiKeyInfo {
                            id: Uuid::new_v4().to_string(),
                            name: "Migrated Key".to_string(),
                            created_at: chrono::Utc::now().timestamp(),
                            permissions: vec!["*".to_string()],
                            active: true,
                        }
                    }
                } else {
                    ApiKeyInfo {
                        id: Uuid::new_v4().to_string(),
                        name: "Migrated Key".to_string(),
                        created_at: chrono::Utc::now().timestamp(),
                        permissions: vec!["*".to_string()],
                        active: true,
                    }
                };

                keys.insert(hash_str.to_string(), (encrypted, info));
            }
        }

        if !keys.is_empty() {
            info!(
                category = "auth",
                count = keys.len(),
                "Loaded {} API key(s) from encrypted database",
                keys.len()
            );
        }

        Ok(keys)
    }

    /// Save API keys to database with encryption.
    fn save_to_db(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let path_ref = std::path::Path::new(path);
        let db = if path_ref.exists() {
            Database::open(path_ref)?
        } else {
            if let Some(parent) = path_ref.parent() {
                std::fs::create_dir_all(parent)?;
            }
            Database::create(path_ref)?
        };
        let write_txn = db.begin_write()?;
        {
            let mut table = write_txn.open_table(API_KEYS_TABLE)?;
            let mut hash_table = write_txn.open_table(API_KEY_HASHES_TABLE)?;

            // Clear existing keys
            let mut to_delete = Vec::new();
            for item in table.iter()? {
                let (key, _) = item?;
                to_delete.push(key.value().to_string());
            }
            for key in &to_delete {
                table.remove(&**key)?;
                hash_table.remove(&**key)?;
            }

            // Insert all current keys (already encrypted) - DashMap iter is lock-free
            for ref_item in self.api_keys.iter() {
                let (hash, (encrypted, info)) = ref_item.pair();
                table.insert(&**hash, encrypted.as_bytes())?;
                let info_bytes = bincode::serialize(info)?;
                hash_table.insert(&**hash, &*info_bytes)?;
            }
        }
        write_txn.commit()?;

        Ok(())
    }

    /// Generate a default API key for first-time setup.
    fn generate_default_key(crypto: &CryptoService) -> HashMap<String, (String, ApiKeyInfo)> {
        let key = format!("nmk_{}", Uuid::new_v4().to_string().replace("-", ""));
        let info = ApiKeyInfo {
            id: Uuid::new_v4().to_string(),
            name: "Default API Key".to_string(),
            created_at: chrono::Utc::now().timestamp(),
            permissions: vec!["*".to_string()],
            active: true,
        };

        let hash = crypto.hash_api_key(&key);
        let encrypted = crypto.encrypt_str(&key).unwrap_or_else(|_| key.clone());

        let mut keys = HashMap::new();
        keys.insert(hash.clone(), (encrypted, info.clone()));

        // Print the key prominently for the user
        crate::startup::log_startup().api_key_banner(&key, &info.name);

        keys
    }

    /// Generate a default API key without printing banner (for CLI use).
    fn generate_default_key_silent(
        crypto: &CryptoService,
    ) -> HashMap<String, (String, ApiKeyInfo)> {
        let key = format!("nmk_{}", Uuid::new_v4().to_string().replace("-", ""));
        let info = ApiKeyInfo {
            id: Uuid::new_v4().to_string(),
            name: "Default API Key".to_string(),
            created_at: chrono::Utc::now().timestamp(),
            permissions: vec!["*".to_string()],
            active: true,
        };

        let hash = crypto.hash_api_key(&key);
        let encrypted = crypto.encrypt_str(&key).unwrap_or_else(|_| key.clone());

        let mut keys = HashMap::new();
        keys.insert(hash, (encrypted, info));

        keys
    }

    /// Load default API keys from environment variable.
    fn load_default_keys(crypto: &CryptoService) -> HashMap<String, (String, ApiKeyInfo)> {
        let mut keys = HashMap::new();

        // Load from NEOMIND_API_KEY environment variable
        if let Ok(default_key) = std::env::var("NEOMIND_API_KEY") {
            let info = ApiKeyInfo {
                id: Uuid::new_v4().to_string(),
                name: "Default API Key (from env)".to_string(),
                created_at: chrono::Utc::now().timestamp(),
                permissions: vec!["*".to_string()],
                active: true,
            };
            let hash = crypto.hash_api_key(&default_key);
            let encrypted = crypto
                .encrypt_str(&default_key)
                .unwrap_or_else(|_| default_key.clone());
            keys.insert(hash, (encrypted, info));
            info!(
                category = "auth",
                "Loaded default API key from NEOMIND_API_KEY"
            );
        }

        keys
    }

    /// Validate an API key.
    pub fn validate_key(&self, key: &str) -> bool {
        let hash = self.crypto.hash_api_key(key);
        if let Some(item) = self.api_keys.get(&hash) {
            return item.value().1.active;
        }
        // Miss: the in-memory map is a startup snapshot — a key created
        // after boot (e.g. `neomind api-key create` from another process)
        // is in the DB but not here. Reload once and retry. (Previously such
        // keys 401'd until the server restarted.)
        self.reload_keys_from_db();
        self.api_keys
            .get(&hash)
            .map(|item| item.value().1.active)
            .unwrap_or(false)
    }

    /// Throttled re-read of the key table. Unknown keys arrive in bursts
    /// (scanners probing an exposed edge device) and each reload opens the
    /// DB — capped so invalid-key spray can't become per-request DB load.
    fn reload_keys_from_db(&self) {
        static LAST_RELOAD_MS: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(0);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let last = LAST_RELOAD_MS.load(std::sync::atomic::Ordering::SeqCst);
        if now.saturating_sub(last) < 5_000 {
            return; // a recent reload already ran; this key just isn't there
        }
        // CAS so concurrent misses trigger at most one reload.
        if LAST_RELOAD_MS
            .compare_exchange(
                last,
                now,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            )
            .is_err()
        {
            return;
        }
        match Self::load_from_db(&self.db_path, &self.crypto) {
            Ok(keys) => {
                // Insert-then-retain, never clear: the map stays populated
                // throughout, so a valid key validated mid-reload cannot
                // transiently 401 (clear-then-insert had that window).
                let fresh: std::collections::HashSet<String> =
                    keys.keys().cloned().collect();
                for (k, v) in keys {
                    self.api_keys.insert(k, v);
                }
                self.api_keys.retain(|k, _| fresh.contains(k));
            }
            Err(e) => {
                warn!(category = "auth", error = %e, "Failed to reload API keys from database");
            }
        }
    }

    /// Validate an API key and return its info.
    /// Returns `None` if the key is invalid or inactive.
    pub fn validate_key_info(&self, key: &str) -> Option<ApiKeyInfo> {
        let hash = self.crypto.hash_api_key(key);
        self.api_keys
            .get(&hash)
            .map(|item| item.value().1.clone())
            .filter(|info| info.active)
    }

    /// List all API keys (for admin endpoints).
    /// Returns the masked keys (first 8 chars only) with info.
    pub async fn list_keys(&self) -> Vec<(String, ApiKeyInfo)> {
        self.api_keys
            .iter()
            .map(|item| (item.key().clone(), item.value().1.clone()))
            .collect()
    }

    /// Create a new API key and persist to database.
    pub async fn create_key(&self, name: String, permissions: Vec<String>) -> (String, ApiKeyInfo) {
        let key = format!("nmk_{}", Uuid::new_v4().to_string().replace("-", ""));
        let info = ApiKeyInfo {
            id: Uuid::new_v4().to_string(),
            name,
            created_at: chrono::Utc::now().timestamp(),
            permissions,
            active: true,
        };

        let hash = self.crypto.hash_api_key(&key);
        let encrypted = self
            .crypto
            .encrypt_str(&key)
            .unwrap_or_else(|_| key.clone());

        // DashMap insert is lock-free
        self.api_keys
            .insert(hash.clone(), (encrypted, info.clone()));

        // Persist to database
        if let Err(e) = self.save_to_db(&self.db_path) {
            warn!(category = "auth", error = %e, "Failed to save API key to database");
        }

        (key, info)
    }

    /// Delete an API key and persist to database.
    pub async fn delete_key(&self, key: &str) -> bool {
        let hash = self.crypto.hash_api_key(key);
        let removed = self.api_keys.remove(&hash).is_some();

        if removed {
            // Persist to database
            if let Err(e) = self.save_to_db(&self.db_path) {
                warn!(category = "auth", error = %e, "Failed to save API keys to database");
            }
        }

        removed
    }

    /// Delete an API key by its hash and persist to database.
    /// Used by CLI when only the hash is available (e.g. from list_keys).
    pub async fn delete_key_by_hash(&self, hash: &str) -> bool {
        let removed = self.api_keys.remove(hash).is_some();

        if removed {
            if let Err(e) = self.save_to_db(&self.db_path) {
                warn!(category = "auth", error = %e, "Failed to save API keys to database");
            }
        }

        removed
    }

    /// Ensure persistent storage is in sync with in-memory state.
    pub async fn init_storage(&self) {
        if let Some(parent) = std::path::Path::new(&self.db_path).parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                error!(category = "auth", error = %e, "Failed to create data directory");
            }
        }

        // Persist all in-memory keys to database
        if let Err(e) = self.save_to_db(&self.db_path) {
            error!(category = "auth", error = %e, "Failed to persist API keys to database");
        } else {
            info!(
                category = "auth",
                count = self.api_keys.len(),
                "API keys persisted to storage"
            );
        }
    }

    /// Check if a key has a specific permission.
    pub fn check_permission(&self, key: &str, permission: &str) -> bool {
        let hash = self.crypto.hash_api_key(key);
        self.api_keys
            .get(&hash)
            .map(|item| {
                let info = &item.value().1;
                if !info.active {
                    return false;
                }
                // Wildcard permission grants all
                if info.permissions.contains(&"*".to_string()) {
                    return true;
                }
                // Check specific permission
                info.permissions.contains(&permission.to_string())
            })
            .unwrap_or(false)
    }
}

impl Default for AuthState {
    fn default() -> Self {
        Self::new()
    }
}

/// Authentication error response.
#[derive(Debug)]
pub struct AuthError {
    pub status: StatusCode,
    pub message: String,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "error": self.message,
            "status": self.status.as_u16(),
        });
        (self.status, Json(body)).into_response()
    }
}

impl AuthError {
    pub fn unauthorized(message: &str) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.to_string(),
        }
    }

    pub fn forbidden(message: &str) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.to_string(),
        }
    }
}

/// API Key authentication middleware.
///
/// Checks for X-API-Key header and validates against stored keys.
pub async fn api_key_middleware(
    State(auth): State<Arc<AuthState>>,
    headers: HeaderMap,
    mut req: axum::extract::Request,
    next: Next,
) -> Result<Response, AuthError> {
    // Extract API key from header
    let api_key = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .or_else(|| {
            // Also check Authorization header with Bearer token
            headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
        });

    let api_key = api_key.ok_or_else(|| {
        AuthError::unauthorized(
            "Missing API key. Provide X-API-Key header or Authorization: Bearer <key>",
        )
    })?;

    // Validate the key
    if !auth.validate_key(api_key) {
        return Err(AuthError::unauthorized("Invalid API key"));
    }

    // Store the validated key in request extensions for later use
    req.extensions_mut()
        .insert(ValidatedApiKey(api_key.to_string()));

    Ok(next.run(req).await)
}

/// Marker struct for validated API key stored in request extensions.
#[derive(Debug, Clone)]
pub struct ValidatedApiKey(pub String);

/// Optional authentication middleware.
///
/// Allows requests without authentication but validates the key if provided.
/// Use this for endpoints that work with or without auth.
pub async fn optional_auth_middleware(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    mut req: axum::extract::Request,
    next: Next,
) -> Response {
    let api_key = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .or_else(|| {
            headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
        });

    if let Some(key) = api_key {
        if auth.validate_key(key) {
            req.extensions_mut()
                .insert(ValidatedApiKey(key.to_string()));
        }
    }

    next.run(req).await
}

/// Hybrid authentication middleware.
///
/// Supports both JWT tokens (for user authentication) and API keys (for tools/scripts).
/// This is the preferred middleware for most protected endpoints.
pub async fn hybrid_auth_middleware(
    State(state): State<ServerState>,
    headers: HeaderMap,
    mut req: axum::extract::Request,
    next: Next,
) -> Result<Response, AuthError> {
    // Internal share proxy bypass — requests forwarded from the share proxy
    // handler already validated the share token, so we trust them.
    //
    // SECURITY: requires BOTH `x-internal-proxy: share` AND a per-process
    // random secret in `x-internal-proxy-secret`. Without the secret check,
    // any network client (server binds 0.0.0.0 by default) could spoof the
    // header and bypass auth entirely. The share proxy handler in
    // dashboards.rs sets both headers when forwarding via loopback reqwest.
    if headers
        .get("x-internal-proxy")
        .and_then(|v| v.to_str().ok())
        == Some("share")
    {
        let secret_ok = headers
            .get("x-internal-proxy-secret")
            .and_then(|v| v.to_str().ok())
            .map(|s| constant_time_eq_str(s, state.internal_proxy_secret.as_str()))
            .unwrap_or(false);

        if !secret_ok {
            tracing::warn!(
                category = "auth",
                "x-internal-proxy: share header without matching secret — \
                 likely spoofed request, rejecting"
            );
            return Err(AuthError::unauthorized(
                "Internal proxy secret missing or invalid",
            ));
        }

        // Insert a service account session for logging purposes
        let proxy_session = crate::auth_users::SessionInfo {
            user_id: "share-proxy".to_string(),
            username: "share-proxy".to_string(),
            role: crate::auth_users::UserRole::User,
            created_at: 0,
            expires_at: i64::MAX,
        };
        req.extensions_mut().insert(proxy_session);
        return Ok(next.run(req).await);
    }

    // First, try to extract and validate JWT token from Authorization header
    if let Some(auth_header) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(token) = auth_header.strip_prefix("Bearer ") {
            // Try JWT authentication first
            match state.auth.user_state.validate_token(token) {
                Ok(session_info) => {
                    // JWT token is valid, store session info and proceed
                    req.extensions_mut().insert(session_info);
                    return Ok(next.run(req).await);
                }
                Err(_) => {
                    // JWT token is invalid or expired, fall through to API key check
                    // (but don't fail yet - maybe they're using API key)
                }
            }
        }
    }

    // If JWT didn't work, try API key authentication
    let api_key = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .or_else(|| {
            headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
        });

    if let Some(key) = api_key {
        if let Some(info) = state.auth.api_key_state.validate_key_info(key) {
            req.extensions_mut()
                .insert(ValidatedApiKey(key.to_string()));
            // Construct service account SessionInfo from API key info
            let service_account = crate::auth_users::SessionInfo {
                user_id: format!("apikey:{}", info.id),
                username: info.name,
                role: if info.permissions.contains(&"*".to_string()) {
                    crate::auth_users::UserRole::Admin
                } else {
                    crate::auth_users::UserRole::User
                },
                created_at: info.created_at,
                expires_at: i64::MAX,
            };
            req.extensions_mut().insert(service_account);
            return Ok(next.run(req).await);
        }
    }

    // Neither JWT nor API key was provided/valid
    Err(AuthError::unauthorized(
        "Authentication required. Provide a valid JWT token or API key.",
    ))
}

/// Constant-time string comparison. Prevents timing side-channels when
/// comparing secrets. Returns false immediately if lengths differ (this
/// leaks length info, which is acceptable for random secrets where length
/// is fixed and known).
pub(crate) fn constant_time_eq_str(a: &str, b: &str) -> bool {
    let (a_bytes, b_bytes) = (a.as_bytes(), b.as_bytes());
    if a_bytes.len() != b_bytes.len() {
        return false;
    }
    let mut result: u8 = 0;
    for (x, y) in a_bytes.iter().zip(b_bytes.iter()) {
        result |= x ^ y;
    }
    result == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_state_creation() {
        let auth = AuthState::new();
        // Auth state should create successfully
        assert!(!auth.validate_key("invalid-key"));
    }

    #[test]
    fn test_api_key_validation() {
        let auth = AuthState::new();
        // Invalid key should fail
        assert!(!auth.validate_key("invalid-key"));
    }

    #[tokio::test]
    async fn test_create_and_delete_key() {
        let auth = AuthState::new();

        let (key, info) = auth
            .create_key("Test Key".to_string(), vec!["*".to_string()])
            .await;
        assert!(auth.validate_key(&key));
        assert_eq!(info.name, "Test Key");

        assert!(auth.delete_key(&key).await);
        assert!(!auth.validate_key(&key));
    }
}
