//! API server for Edge AI Agent.
//!
//! This crate provides the HTTP/WebSocket API server for the Edge AI Agent system.

pub mod audit;
pub mod auth;
pub mod auth_users;
pub mod cache;
pub mod config;
pub mod crypto;
pub mod event_services;
pub mod handlers;
pub mod models;
pub mod openapi;
pub mod rate_limit;
pub mod server;
pub mod shutdown;
pub mod startup;
pub mod validator;

pub use audit::{
    audit_logger, audit_middleware, init_audit_logger, log_audit, AuditCategory, AuditConfig,
    AuditEntry, AuditLogger, AuditSeverity,
};
pub use auth::{ApiKeyInfo, AuthState, ValidatedApiKey};
pub use cache::{cache_key, CacheConfig, CacheStats, CachedResponse, ResponseCache};
pub use config::{load_llm_config, LlmSettingsRequest};
pub use crypto::{CryptoError, CryptoService};
pub use rate_limit::{
    cleanup_task, extract_client_id, RateLimitConfig, RateLimitExceeded, RateLimiter,
};
pub use server::{create_router, run, start_server, ServerState};
pub use validator::{
    validate_device_id, validate_ip_address, validate_length, validate_not_empty, validate_range,
    validate_session_id, validate_url, validation_middleware, AlertQuery, DeviceQuery, PageQuery,
    RuleQuery, SearchQuery, SortOrder, TimeRangeQuery, Validate, ValidationError, ValidationErrors,
};
