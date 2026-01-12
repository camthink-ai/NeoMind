//! API server for Edge AI Agent.
//!
//! This crate provides the HTTP/WebSocket API server for the Edge AI Agent system.

pub mod config;
pub mod models;
pub mod handlers;
pub mod server;
pub mod openapi;
pub mod shutdown;
pub mod auth;
pub mod rate_limit;
pub mod validator;
pub mod crypto;
pub mod audit;
pub mod cache;
pub mod startup;
pub mod auth_users;

pub use config::{load_llm_config, LlmSettingsRequest};
pub use server::{run, create_router, ServerState};
pub use auth::{AuthState, ApiKeyInfo, ValidatedApiKey};
pub use validator::{
    Validate, ValidationError, ValidationErrors,
    PageQuery, SearchQuery, DeviceQuery, RuleQuery, AlertQuery, TimeRangeQuery, SortOrder,
    validate_not_empty, validate_length, validate_range, validate_device_id,
    validate_session_id, validate_ip_address, validate_url,
    validation_middleware,
};
pub use crypto::{CryptoService, CryptoError};
pub use audit::{
    AuditLogger, AuditEntry, AuditSeverity, AuditCategory, AuditConfig,
    init_audit_logger, audit_logger, log_audit, audit_middleware,
};
pub use cache::{
    ResponseCache, CachedResponse, CacheStats, CacheConfig, cache_key,
};
pub use rate_limit::{
    RateLimiter, RateLimitConfig, RateLimitExceeded,
    extract_client_id, cleanup_task,
};
