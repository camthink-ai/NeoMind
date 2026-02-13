//! Audit logging system for API events.
//!
//! This module provides structured logging for security-relevant events including:
//! - Authentication attempts (success/failure)
//! - Authorization checks (granted/denied)
//! - Data access (read/write operations)
//! - Configuration changes
//!
//! Logs are written to both stdout (for development) and a rotating file at `data/audit.log`.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Audit log event severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

/// Audit event categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditCategory {
    Authentication,
    Authorization,
    DataAccess,
    DataModification,
    Configuration,
    System,
}

/// A single audit log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Timestamp of the event (ISO 8601)
    pub timestamp: String,

    /// Severity level
    pub severity: AuditSeverity,

    /// Event category
    pub category: AuditCategory,

    /// Event action description
    pub action: String,

    /// IP address of the client
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_address: Option<String>,

    /// User ID or API key identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// Resource that was accessed/modified
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,

    /// HTTP method for API requests
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,

    /// HTTP path for API requests
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    /// HTTP status code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,

    /// Additional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl AuditEntry {
    /// Create a new audit entry with timestamp.
    pub fn new(
        severity: AuditSeverity,
        category: AuditCategory,
        action: impl Into<String>,
    ) -> Self {
        Self {
            timestamp: Utc::now().to_rfc3339(),
            severity,
            category,
            action: action.into(),
            ip_address: None,
            user_id: None,
            resource: None,
            method: None,
            path: None,
            status_code: None,
            metadata: None,
        }
    }

    /// Set the IP address.
    pub fn with_ip(mut self, ip: impl Into<String>) -> Self {
        self.ip_address = Some(ip.into());
        self
    }

    /// Set the user ID.
    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user_id = Some(user.into());
        self
    }

    /// Set the resource.
    pub fn with_resource(mut self, resource: impl Into<String>) -> Self {
        self.resource = Some(resource.into());
        self
    }

    /// Set the HTTP method.
    pub fn with_method(mut self, method: impl Into<String>) -> Self {
        self.method = Some(method.into());
        self
    }

    /// Set the HTTP path.
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Set the HTTP status code.
    pub fn with_status(mut self, status: u16) -> Self {
        self.status_code = Some(status);
        self
    }

    /// Set the metadata.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Format as JSON for logging.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| format!("{{\"action\": \"{}\"}}", self.action))
    }

    /// Format as a human-readable log line.
    pub fn to_log_line(&self) -> String {
        let severity = match self.severity {
            AuditSeverity::Info => "INFO",
            AuditSeverity::Warning => "WARN",
            AuditSeverity::Error => "ERROR",
            AuditSeverity::Critical => "CRIT",
        };

        let category = match self.category {
            AuditCategory::Authentication => "AUTH",
            AuditCategory::Authorization => "AUTHZ",
            AuditCategory::DataAccess => "DATA_READ",
            AuditCategory::DataModification => "DATA_WRITE",
            AuditCategory::Configuration => "CONFIG",
            AuditCategory::System => "SYSTEM",
        };

        let mut parts = vec![
            self.timestamp.clone(),
            severity.to_string(),
            category.to_string(),
            self.action.clone(),
        ];

        if let Some(ref user) = self.user_id {
            parts.push(format!("user={}", user));
        }

        if let Some(ref ip) = self.ip_address {
            parts.push(format!("ip={}", ip));
        }

        if let Some(ref resource) = self.resource {
            parts.push(format!("resource={}", resource));
        }

        if let Some(status) = self.status_code {
            parts.push(format!("status={}", status));
        }

        parts.join(" | ")
    }
}

/// Audit log configuration.
#[derive(Debug, Clone)]
pub struct AuditConfig {
    /// Path to the audit log file
    pub log_path: String,

    /// Whether to log to stdout
    pub log_to_stdout: bool,

    /// Whether to log to file
    pub log_to_file: bool,

    /// Minimum severity to log
    pub min_severity: AuditSeverity,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            log_path: "data/audit.log".to_string(),
            log_to_stdout: true,
            log_to_file: true,
            min_severity: AuditSeverity::Info,
        }
    }
}

/// The audit logger service.
#[derive(Clone)]
pub struct AuditLogger {
    config: Arc<RwLock<AuditConfig>>,
}

impl AuditLogger {
    /// Create a new audit logger with default configuration.
    pub fn new() -> Self {
        Self::with_config(AuditConfig::default())
    }

    /// Create a new audit logger with custom configuration.
    pub fn with_config(config: AuditConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
        }
    }

    /// Log an audit entry.
    pub async fn log(&self, entry: AuditEntry) {
        let config = self.config.read().await;

        // Check severity threshold
        if !self.should_log(&entry, &config) {
            return;
        }

        let log_line = entry.to_log_line();
        let json_line = entry.to_json();

        // Log to stdout
        if config.log_to_stdout {
            println!("[AUDIT] {}", log_line);
        }

        // Log to file
        if config.log_to_file {
            if let Err(e) = self.write_to_file(&config.log_path, &json_line).await {
                tracing::warn!(category = "audit", error = %e, "Failed to write audit log");
            }
        }
    }

    /// Log authentication success.
    pub async fn log_auth_success(&self, user_id: &str, ip: &str) {
        self.log(
            AuditEntry::new(
                AuditSeverity::Info,
                AuditCategory::Authentication,
                "Authentication successful",
            )
            .with_user(user_id)
            .with_ip(ip),
        )
        .await;
    }

    /// Log authentication failure.
    pub async fn log_auth_failure(&self, ip: &str, reason: &str) {
        self.log(
            AuditEntry::new(
                AuditSeverity::Warning,
                AuditCategory::Authentication,
                format!("Authentication failed: {}", reason),
            )
            .with_ip(ip),
        )
        .await;
    }

    /// Log authorization success.
    pub async fn log_authz_success(&self, user_id: &str, resource: &str, action: &str) {
        self.log(
            AuditEntry::new(
                AuditSeverity::Info,
                AuditCategory::Authorization,
                format!("Authorized: {}", action),
            )
            .with_user(user_id)
            .with_resource(resource),
        )
        .await;
    }

    /// Log authorization denial.
    pub async fn log_authz_denied(&self, user_id: &str, resource: &str, action: &str) {
        self.log(
            AuditEntry::new(
                AuditSeverity::Warning,
                AuditCategory::Authorization,
                format!("Access denied: {}", action),
            )
            .with_user(user_id)
            .with_resource(resource),
        )
        .await;
    }

    /// Log data access.
    pub async fn log_data_access(&self, user_id: &str, resource: &str, method: &str, path: &str) {
        self.log(
            AuditEntry::new(
                AuditSeverity::Info,
                AuditCategory::DataAccess,
                "Data accessed",
            )
            .with_user(user_id)
            .with_resource(resource)
            .with_method(method)
            .with_path(path),
        )
        .await;
    }

    /// Log data modification.
    pub async fn log_data_modification(&self, user_id: &str, resource: &str, action: &str) {
        self.log(
            AuditEntry::new(
                AuditSeverity::Info,
                AuditCategory::DataModification,
                format!("Data modified: {}", action),
            )
            .with_user(user_id)
            .with_resource(resource),
        )
        .await;
    }

    /// Log configuration change.
    pub async fn log_config_change(&self, user_id: &str, setting: &str) {
        self.log(
            AuditEntry::new(
                AuditSeverity::Warning,
                AuditCategory::Configuration,
                format!("Configuration changed: {}", setting),
            )
            .with_user(user_id),
        )
        .await;
    }

    /// Log API request.
    pub async fn log_api_request(
        &self,
        method: &str,
        path: &str,
        status: u16,
        ip: &str,
        user_id: Option<String>,
    ) {
        let mut entry = AuditEntry::new(
            if status >= 500 {
                AuditSeverity::Error
            } else {
                AuditSeverity::Info
            },
            AuditCategory::DataAccess,
            "API request",
        )
        .with_method(method)
        .with_path(path)
        .with_status(status)
        .with_ip(ip);

        if let Some(user) = user_id {
            entry = entry.with_user(user);
        }

        self.log(entry).await;
    }

    /// Check if the entry should be logged based on severity.
    fn should_log(&self, entry: &AuditEntry, config: &AuditConfig) -> bool {
        let severity_order = |s: &AuditSeverity| -> u8 {
            match s {
                AuditSeverity::Info => 0,
                AuditSeverity::Warning => 1,
                AuditSeverity::Error => 2,
                AuditSeverity::Critical => 3,
            }
        };

        severity_order(&entry.severity) >= severity_order(&config.min_severity)
    }

    /// Write to the audit log file.
    async fn write_to_file(&self, path: &str, line: &str) -> std::io::Result<()> {
        // Ensure the directory exists
        if let Some(parent) = Path::new(path).parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Open file in append mode
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;

        writeln!(file, "{}", line)?;
        Ok(())
    }

    /// Get the current configuration.
    pub async fn get_config(&self) -> AuditConfig {
        self.config.read().await.clone()
    }

    /// Update the configuration.
    pub async fn update_config(&self, config: AuditConfig) {
        *self.config.write().await = config;
    }
}

impl Default for AuditLogger {
    fn default() -> Self {
        Self::new()
    }
}

/// Static audit logger instance.
static GLOBAL_AUDIT_LOGGER: std::sync::OnceLock<Arc<AuditLogger>> = std::sync::OnceLock::new();

/// Initialize the global audit logger.
pub fn init_audit_logger(config: AuditConfig) {
    GLOBAL_AUDIT_LOGGER
        .set(Arc::new(AuditLogger::with_config(config)))
        .ok();
}

/// Get the global audit logger instance.
pub fn audit_logger() -> Option<Arc<AuditLogger>> {
    GLOBAL_AUDIT_LOGGER.get().cloned()
}

/// Convenience function to log an audit entry using the global logger.
pub async fn log_audit(entry: AuditEntry) {
    if let Some(logger) = audit_logger() {
        logger.log(entry).await;
    }
}

/// Middleware for logging API requests.
pub fn audit_middleware() -> impl Fn(
    axum::extract::Request,
    axum::middleware::Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = axum::response::Response> + Send>>
       + Clone {
    move |mut req: axum::extract::Request, next: axum::middleware::Next| {
        Box::pin(async move {
            let method = req.method().to_string();
            let path = req.uri().path().to_string();
            let ip = req
                .headers()
                .get("x-forwarded-for")
                .and_then(|v| v.to_str().ok())
                .or_else(|| req.headers().get("x-real-ip").and_then(|v| v.to_str().ok()))
                .unwrap_or("unknown")
                .to_string();

            // Extract user ID from request extensions if available
            let user_id = req
                .extensions_mut()
                .remove::<crate::auth::ValidatedApiKey>()
                .map(|k| k.0);

            let response = next.run(req).await;
            let status = response.status().as_u16();

            // Log the request
            if let Some(logger) = audit_logger() {
                logger
                    .log_api_request(&method, &path, status, &ip, user_id)
                    .await;
            }

            response
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_entry_creation() {
        let entry = AuditEntry::new(
            AuditSeverity::Info,
            AuditCategory::Authentication,
            "Test action",
        )
        .with_user("user123")
        .with_ip("127.0.0.1");

        assert_eq!(entry.action, "Test action");
        assert_eq!(entry.user_id, Some("user123".to_string()));
        assert_eq!(entry.ip_address, Some("127.0.0.1".to_string()));
    }

    #[test]
    fn test_audit_entry_to_log_line() {
        let entry = AuditEntry::new(
            AuditSeverity::Info,
            AuditCategory::Authentication,
            "Test action",
        )
        .with_user("user123")
        .with_ip("127.0.0.1");

        let log_line = entry.to_log_line();
        assert!(log_line.contains("INFO"));
        assert!(log_line.contains("AUTH"));
        assert!(log_line.contains("Test action"));
        assert!(log_line.contains("user=user123"));
        assert!(log_line.contains("ip=127.0.0.1"));
    }

    #[test]
    fn test_severity_filtering() {
        let config = AuditConfig {
            min_severity: AuditSeverity::Warning,
            ..Default::default()
        };

        let logger = AuditLogger::with_config(config.clone());

        // Info should not be logged
        let info_entry = AuditEntry::new(AuditSeverity::Info, AuditCategory::System, "Test");
        assert!(!logger.should_log(&info_entry, &config));

        // Warning should be logged
        let warn_entry = AuditEntry::new(AuditSeverity::Warning, AuditCategory::System, "Test");
        assert!(logger.should_log(&warn_entry, &config));
    }

    #[tokio::test]
    async fn test_log_methods() {
        let logger = AuditLogger::with_config(AuditConfig {
            log_to_file: false,
            log_to_stdout: false,
            ..Default::default()
        });

        // These should not panic
        logger.log_auth_success("user123", "127.0.0.1").await;
        logger.log_auth_failure("127.0.0.1", "invalid key").await;
        logger
            .log_authz_success("user123", "/api/devices", "read")
            .await;
        logger
            .log_authz_denied("user123", "/api/admin", "write")
            .await;
        logger
            .log_data_access("user123", "/api/devices", "GET", "/api/devices")
            .await;
        logger
            .log_data_modification("user123", "/api/devices", "create")
            .await;
        logger.log_config_change("admin", "llm.model").await;
        logger
            .log_api_request(
                "GET",
                "/api/test",
                200,
                "127.0.0.1",
                Some("user123".to_string()),
            )
            .await;
    }
}
