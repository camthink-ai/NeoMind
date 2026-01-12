//! Unified API response models.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{error::ErrorResponse, pagination::PaginationMeta};

/// Unified API response wrapper.
///
/// All API endpoints should return this wrapper for consistency.
///
/// # Examples
///
/// ```json
/// {
///   "success": true,
///   "data": { ... },
///   "error": null,
///   "meta": {
///     "timestamp": 1704067200,
///     "request_id": "550e8400-e29b-41d4-a716-446655440000"
///   }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    /// Whether the request was successful.
    pub success: bool,

    /// Response data (present on success).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,

    /// Error information (present on failure).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,

    /// Response metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<ResponseMeta>,
}

impl<T> ApiResponse<T> {
    /// Create a success response with data.
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            meta: Some(ResponseMeta::default()),
        }
    }

    /// Create a success response with data and custom metadata.
    pub fn success_with_meta(data: T, meta: ResponseMeta) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            meta: Some(meta),
        }
    }

    /// Create an error response.
    pub fn error(error: ApiError) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(error),
            meta: Some(ResponseMeta::default()),
        }
    }

    /// Create an error response from ErrorResponse.
    pub fn from_error_response(err: ErrorResponse) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(ApiError {
                code: err.code,
                message: err.message,
                details: None,
            }),
            meta: Some(ResponseMeta {
                timestamp: Utc::now(),
                request_id: err.request_id.unwrap_or_else(|| Uuid::new_v4().to_string()),
                pagination: None,
            }),
        }
    }

    /// Convert to paginated response.
    pub fn paginated(data: T, pagination: PaginationMeta) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            meta: Some(ResponseMeta {
                timestamp: Utc::now(),
                request_id: Uuid::new_v4().to_string(),
                pagination: Some(pagination),
            }),
        }
    }

    /// Create a response without data (e.g., for DELETE operations).
    pub fn empty() -> Self {
        Self {
            success: true,
            data: None,
            error: None,
            meta: Some(ResponseMeta::default()),
        }
    }
}

impl<T> IntoResponse for ApiResponse<T>
where
    T: Serialize,
{
    fn into_response(self) -> axum::response::Response {
        axum::Json(self).into_response()
    }
}

/// Response metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMeta {
    /// Response timestamp.
    pub timestamp: DateTime<Utc>,

    /// Unique request ID for tracing.
    pub request_id: String,

    /// Pagination metadata (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pagination: Option<PaginationMeta>,
}

impl Default for ResponseMeta {
    fn default() -> Self {
        Self {
            timestamp: Utc::now(),
            request_id: Uuid::new_v4().to_string(),
            pagination: None,
        }
    }
}

impl ResponseMeta {
    /// Create metadata with pagination.
    pub fn with_pagination(pagination: PaginationMeta) -> Self {
        Self {
            timestamp: Utc::now(),
            request_id: Uuid::new_v4().to_string(),
            pagination: Some(pagination),
        }
    }

    /// Set a custom request ID.
    pub fn with_request_id(mut self, request_id: String) -> Self {
        self.request_id = request_id;
        self
    }
}

/// Standardized API error format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    /// Machine-readable error code.
    pub code: String,

    /// Human-readable error message.
    pub message: String,

    /// Additional error details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ApiError {
    /// Create a new API error.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    /// Add details to the error.
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}

impl From<ErrorResponse> for ApiError {
    fn from(err: ErrorResponse) -> Self {
        Self {
            code: err.code,
            message: err.message,
            details: None,
        }
    }
}

/// Standard error codes.
///
/// Error codes are organized by domain:
/// - 1xxx: General errors
/// - 2xxx: Device errors
/// - 3xxx: Rule errors
/// - 4xxx: Alert errors
/// - 5xxx: Authentication/Authorization errors
/// - 6xxx: Workflow errors
/// - 7xxx: LLM/Agent errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    // General errors (1xxx)
    BadRequest = 1000,
    Unauthorized = 1001,
    Forbidden = 1002,
    NotFound = 1003,
    Conflict = 1004,
    RateLimited = 1005,
    ValidationFailed = 1006,
    InternalError = 1007,
    ServiceUnavailable = 1008,

    // Device errors (2xxx)
    DeviceNotFound = 2001,
    DeviceTypeNotFound = 2002,
    DeviceCommandFailed = 2003,
    DeviceOffline = 2004,
    InvalidDeviceConfig = 2005,

    // Rule errors (3xxx)
    RuleNotFound = 3001,
    RuleValidationFailed = 3002,
    RuleExecutionFailed = 3003,
    InvalidRuleDefinition = 3004,

    // Alert errors (4xxx)
    AlertNotFound = 4001,
    AlertAlreadyAcknowledged = 4002,
    InvalidAlertCondition = 4003,

    // Auth errors (5xxx)
    InvalidApiKey = 5001,
    ApiKeyExpired = 5002,
    InsufficientPermissions = 5003,
    InvalidToken = 5004,

    // Workflow errors (6xxx)
    WorkflowNotFound = 6001,
    WorkflowExecutionFailed = 6002,
    InvalidWorkflowDefinition = 6003,

    // LLM/Agent errors (7xxx)
    LlmUnavailable = 7001,
    LlmTimeout = 7002,
    InvalidLlmResponse = 7003,
    SessionNotFound = 7004,
}

impl ErrorCode {
    /// Get the error code as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            // General errors
            Self::BadRequest => "BAD_REQUEST",
            Self::Unauthorized => "UNAUTHORIZED",
            Self::Forbidden => "FORBIDDEN",
            Self::NotFound => "NOT_FOUND",
            Self::Conflict => "CONFLICT",
            Self::RateLimited => "RATE_LIMITED",
            Self::ValidationFailed => "VALIDATION_FAILED",
            Self::InternalError => "INTERNAL_ERROR",
            Self::ServiceUnavailable => "SERVICE_UNAVAILABLE",

            // Device errors
            Self::DeviceNotFound => "DEVICE_NOT_FOUND",
            Self::DeviceTypeNotFound => "DEVICE_TYPE_NOT_FOUND",
            Self::DeviceCommandFailed => "DEVICE_COMMAND_FAILED",
            Self::DeviceOffline => "DEVICE_OFFLINE",
            Self::InvalidDeviceConfig => "INVALID_DEVICE_CONFIG",

            // Rule errors
            Self::RuleNotFound => "RULE_NOT_FOUND",
            Self::RuleValidationFailed => "RULE_VALIDATION_FAILED",
            Self::RuleExecutionFailed => "RULE_EXECUTION_FAILED",
            Self::InvalidRuleDefinition => "INVALID_RULE_DEFINITION",

            // Alert errors
            Self::AlertNotFound => "ALERT_NOT_FOUND",
            Self::AlertAlreadyAcknowledged => "ALERT_ALREADY_ACKNOWLEDGED",
            Self::InvalidAlertCondition => "INVALID_ALERT_CONDITION",

            // Auth errors
            Self::InvalidApiKey => "INVALID_API_KEY",
            Self::ApiKeyExpired => "API_KEY_EXPIRED",
            Self::InsufficientPermissions => "INSUFFICIENT_PERMISSIONS",
            Self::InvalidToken => "INVALID_TOKEN",

            // Workflow errors
            Self::WorkflowNotFound => "WORKFLOW_NOT_FOUND",
            Self::WorkflowExecutionFailed => "WORKFLOW_EXECUTION_FAILED",
            Self::InvalidWorkflowDefinition => "INVALID_WORKFLOW_DEFINITION",

            // LLM/Agent errors
            Self::LlmUnavailable => "LLM_UNAVAILABLE",
            Self::LlmTimeout => "LLM_TIMEOUT",
            Self::InvalidLlmResponse => "INVALID_LLM_RESPONSE",
            Self::SessionNotFound => "SESSION_NOT_FOUND",
        }
    }

    /// Get the numeric error code.
    pub fn as_number(&self) -> u16 {
        *self as u16
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Helper trait for converting domain-specific results to API responses.
pub trait ToApiResponse {
    type Output;

    /// Convert to a success API response.
    fn to_api_response(self) -> ApiResponse<Self::Output>;

    /// Convert to an error API response.
    fn to_error_response(self) -> ApiResponse<Self::Output>
    where
        Self: Sized,
    {
        ApiResponse::error(ApiError::new(
            ErrorCode::InternalError.as_str(),
            "An error occurred".to_string(),
        ))
    }
}

impl<T, E> ToApiResponse for Result<T, E>
where
    T: Serialize,
    E: Into<ErrorResponse>,
{
    type Output = T;

    fn to_api_response(self) -> ApiResponse<T> {
        match self {
            Ok(data) => ApiResponse::success(data),
            Err(err) => ApiResponse::from_error_response(err.into()),
        }
    }
}

// Re-export IntoResponse for convenience
pub use axum::response::IntoResponse;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_success_response() {
        let response: ApiResponse<String> = ApiResponse::success("test data".to_string());

        assert!(response.success);
        assert_eq!(response.data, Some("test data".to_string()));
        assert!(response.error.is_none());
        assert!(response.meta.is_some());
    }

    #[test]
    fn test_error_response() {
        let error = ApiError::new("TEST_ERROR", "Something went wrong");
        let response: ApiResponse<String> = ApiResponse::error(error);

        assert!(!response.success);
        assert!(response.data.is_none());
        assert!(response.error.is_some());
    }

    #[test]
    fn test_paginated_response() {
        let pagination = PaginationMeta::new(1, 10, 100);
        let response: ApiResponse<Vec<String>> =
            ApiResponse::paginated(vec!["a".to_string()], pagination);

        assert!(response.success);
        assert!(response.meta.as_ref().unwrap().pagination.is_some());
    }

    #[test]
    fn test_error_codes() {
        assert_eq!(ErrorCode::NotFound.as_str(), "NOT_FOUND");
        assert_eq!(ErrorCode::NotFound.as_number(), 1003);

        assert_eq!(ErrorCode::DeviceNotFound.as_str(), "DEVICE_NOT_FOUND");
        assert_eq!(ErrorCode::DeviceNotFound.as_number(), 2001);
    }

    #[test]
    fn test_response_meta_default() {
        let meta = ResponseMeta::default();

        assert!(meta.request_id.len() > 0);
        assert!(meta.pagination.is_none());
    }

    #[test]
    fn test_response_meta_with_pagination() {
        let pagination = PaginationMeta::new(1, 10, 100);
        let meta = ResponseMeta::with_pagination(pagination);

        assert!(meta.pagination.is_some());
    }
}
