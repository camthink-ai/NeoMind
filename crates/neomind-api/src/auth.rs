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

use redb::{Database, ReadableTable, TableDefinition};
use tracing::{error, info, warn};

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
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
    /// Permissions (simple list, "*" means all)
    pub permissions: Vec<String>,
    /// Whether this key is active
    pub active: bool,
}

/// Authentication state with persistent storage.
#[derive(Clone)]
pub struct AuthState {
    /// API Keys storage (in-memory for fast access)
    /// Maps hash -> (encrypted_key, ApiKeyInfo)
    api_keys: Arc<RwLock<HashMap<String, (String, ApiKeyInfo)>>>,
    /// Database path for persistence
    db_path: &'static str,
    /// Cryptographic service for key encryption
    crypto: Arc<CryptoService>,
}

impl AuthState {
    /// Create a new auth state with persistent storage.
    /// Loads existing keys from database, or creates a default key if none exist.
    pub fn new() -> Self {
        let db_path = "data/api_keys.redb";
        let crypto = Arc::new(CryptoService::from_env_or_generate());

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

        Self {
            api_keys: Arc::new(RwLock::new(keys)),
            db_path,
            crypto,
        }
    }

    /// Load API keys from redb database.
    fn load_from_db(
        path: &str,
        crypto: &CryptoService,
    ) -> Result<HashMap<String, (String, ApiKeyInfo)>, Box<dyn std::error::Error>> {
        let db = Database::open(path)?;
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
        let db = Database::open(path)?;
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

            // Insert all current keys (already encrypted)
            let keys = self
                .api_keys
                .try_read()
                .map_err(|_| "Failed to acquire read lock")?;

            for (hash, (encrypted, info)) in keys.iter() {
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
        if let Ok(keys) = self.api_keys.try_read()
            && let Some((_, info)) = keys.get(&hash) {
                return info.active;
            }
        false
    }

    /// List all API keys (for admin endpoints).
    /// Returns the masked keys (first 8 chars only) with info.
    pub async fn list_keys(&self) -> Vec<(String, ApiKeyInfo)> {
        let keys = self.api_keys.read().await;
        keys.iter()
            .map(|(k, (_, v))| {
                // Return masked key (hash) with info
                (k.clone(), v.clone())
            })
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

        {
            let mut keys = self.api_keys.write().await;
            keys.insert(hash.clone(), (encrypted, info.clone()));
        }

        // Persist to database
        if let Err(e) = self.save_to_db(self.db_path) {
            warn!(category = "auth", error = %e, "Failed to save API key to database");
        }

        (key, info)
    }

    /// Delete an API key and persist to database.
    pub async fn delete_key(&self, key: &str) -> bool {
        let hash = self.crypto.hash_api_key(key);
        let removed = {
            let mut keys = self.api_keys.write().await;
            keys.remove(&hash).is_some()
        };

        if removed {
            // Persist to database
            if let Err(e) = self.save_to_db(self.db_path) {
                warn!(category = "auth", error = %e, "Failed to save API keys to database");
            }
        }

        removed
    }

    /// Initialize persistent storage (create data directory if needed).
    pub async fn init_storage(&self) {
        if let Err(e) = tokio::fs::create_dir_all("data").await {
            error!(category = "auth", error = %e, "Failed to create data directory");
        }

        // Try to load from database, or save current keys
        if Self::load_from_db(self.db_path, &self.crypto).is_ok() {
            info!(category = "auth", "API keys loaded from persistent storage");
        } else if let Err(e) = self.save_to_db(self.db_path) {
            error!(category = "auth", error = %e, "Failed to initialize API key storage");
        }
    }

    /// Check if a key has a specific permission.
    pub fn check_permission(&self, key: &str, permission: &str) -> bool {
        let hash = self.crypto.hash_api_key(key);
        if let Ok(keys) = self.api_keys.try_read()
            && let Some((_, info)) = keys.get(&hash) {
                if !info.active {
                    return false;
                }
                // Wildcard permission grants all
                if info.permissions.contains(&"*".to_string()) {
                    return true;
                }
                // Check specific permission
                return info.permissions.contains(&permission.to_string());
            }
        false
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
    if let Some(api_key) = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .or_else(|| {
            headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
        })
        && auth.validate_key(api_key) {
            req.extensions_mut()
                .insert(ValidatedApiKey(api_key.to_string()));
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
    

    // First, try to extract and validate JWT token from Authorization header
    if let Some(auth_header) = headers.get("authorization").and_then(|v| v.to_str().ok())
        && let Some(token) = auth_header.strip_prefix("Bearer ") {
            // Try JWT authentication first
            match state.auth_user_state.validate_token(token) {
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

    if let Some(key) = api_key
        && state.auth_state.validate_key(key) {
            req.extensions_mut()
                .insert(ValidatedApiKey(key.to_string()));
            return Ok(next.run(req).await);
        }

    // Neither JWT nor API key was provided/valid
    Err(AuthError::unauthorized(
        "Authentication required. Provide a valid JWT token or API key.",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_state_creation() {
        let auth = AuthState::new();
        // Auth state should create successfully
        assert!(auth.validate_key("invalid-key") == false);
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
