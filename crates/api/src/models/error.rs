//! Unified error handling for the API.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Unified API error response with proper HTTP status codes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    /// Error code for programmatic handling.
    pub code: String,
    /// Human-readable error message.
    pub message: String,
    /// HTTP status code.
    #[serde(skip)]
    pub status: StatusCode,
    /// Optional request ID for tracing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

impl ErrorResponse {
    /// Create a new error response.
    pub fn new(code: impl Into<String>, message: impl Into<String>, status: StatusCode) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            status,
            request_id: None,
        }
    }

    /// Set the request ID.
    pub fn with_request_id(mut self, request_id: String) -> Self {
        self.request_id = Some(request_id);
        self
    }

    /// Create a simple error with just a message (defaults to internal server error).
    /// This provides backward compatibility with the old ErrorResponse { error: ... } pattern.
    pub fn with_message(message: impl Into<String>) -> Self {
        Self::internal(message.into())
    }

    // Common error constructors
    /// Bad request (400).
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new("BAD_REQUEST", message, StatusCode::BAD_REQUEST)
    }

    /// Unauthorized (401).
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new("UNAUTHORIZED", message, StatusCode::UNAUTHORIZED)
    }

    /// Not found (404).
    pub fn not_found(resource: impl Into<String>) -> Self {
        Self::new(
            "NOT_FOUND",
            format!("{} not found", resource.into()),
            StatusCode::NOT_FOUND,
        )
    }

    /// Conflict (409).
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new("CONFLICT", message, StatusCode::CONFLICT)
    }

    /// Validation error (422).
    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(
            "VALIDATION_ERROR",
            message,
            StatusCode::UNPROCESSABLE_ENTITY,
        )
    }

    /// Internal server error (500).
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("INTERNAL_ERROR", message, StatusCode::INTERNAL_SERVER_ERROR)
    }

    /// Service unavailable (503).
    pub fn service_unavailable(message: impl Into<String>) -> Self {
        Self::new(
            "SERVICE_UNAVAILABLE",
            message,
            StatusCode::SERVICE_UNAVAILABLE,
        )
    }

    /// Gone (410) - resource is no longer available.
    pub fn gone(message: impl Into<String>) -> Self {
        Self::new("GONE", message, StatusCode::GONE)
    }
}

impl IntoResponse for ErrorResponse {
    fn into_response(self) -> Response {
        let status = self.status;
        let body = serde_json::json!({
            "success": false,
            "error": {
                "code": self.code,
                "message": self.message,
                "request_id": self.request_id,
            }
        });
        (status, axum::Json(body)).into_response()
    }
}

impl fmt::Display for ErrorResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for ErrorResponse {}

/// Conversion from common error types.

impl From<anyhow::Error> for ErrorResponse {
    fn from(e: anyhow::Error) -> Self {
        Self::internal(e.to_string())
    }
}

impl From<std::io::Error> for ErrorResponse {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::NotFound => Self::not_found("resource"),
            std::io::ErrorKind::PermissionDenied => Self::unauthorized("permission denied"),
            _ => Self::internal(e.to_string()),
        }
    }
}

impl From<edge_ai_agent::NeoTalkError> for ErrorResponse {
    fn from(e: edge_ai_agent::NeoTalkError) -> Self {
        Self::internal(format!("Agent error: {}", e))
    }
}

impl From<edge_ai_devices::DeviceError> for ErrorResponse {
    fn from(e: edge_ai_devices::DeviceError) -> Self {
        match e {
            edge_ai_devices::DeviceError::NotFound(_) => Self::not_found("device"),
            edge_ai_devices::DeviceError::InvalidParameter(_) => Self::bad_request(e.to_string()),
            _ => Self::internal(e.to_string()),
        }
    }
}

impl From<edge_ai_rules::RuleError> for ErrorResponse {
    fn from(e: edge_ai_rules::RuleError) -> Self {
        Self::internal(format!("Rule error: {}", e))
    }
}

impl From<edge_ai_storage::Error> for ErrorResponse {
    fn from(e: edge_ai_storage::Error) -> Self {
        Self::internal(format!("Storage error: {}", e))
    }
}

/// Result type alias for API handlers.
pub type ApiResult<T> = Result<T, ErrorResponse>;

/// Helper macro for creating validation errors.
#[macro_export]
macro_rules! validation_error {
    ($msg:expr) => {
        Err($crate::models::error::ErrorResponse::validation($msg))
    };
    ($fmt:expr, $($arg:tt)*) => {
        Err($crate::models::error::ErrorResponse::validation(format!($fmt, $($arg)*)))
    };
}

/// Helper macro for creating not found errors.
#[macro_export]
macro_rules! not_found_error {
    ($resource:expr) => {
        Err($crate::models::error::ErrorResponse::not_found($resource))
    };
}

/// Helper macro for creating bad request errors.
#[macro_export]
macro_rules! bad_request_error {
    ($msg:expr) => {
        Err($crate::models::error::ErrorResponse::bad_request($msg))
    };
    ($fmt:expr, $($arg:tt)*) => {
        Err($crate::models::error::ErrorResponse::bad_request(format!($fmt, $($arg)*)))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_response() {
        let err = ErrorResponse::not_found("test resource");
        assert_eq!(err.code, "NOT_FOUND");
        assert_eq!(err.status, StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_error_with_request_id() {
        let err =
            ErrorResponse::bad_request("invalid input").with_request_id("req-123".to_string());
        assert_eq!(err.request_id, Some("req-123".to_string()));
    }
}
