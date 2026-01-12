//! Request validation layer.

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

use crate::models::{ErrorResponse, ErrorCode};

/// Validation error details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    /// Field that failed validation.
    pub field: String,

    /// Error message.
    pub message: String,

    /// The value that failed validation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
}

/// Collection of validation errors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationErrors {
    pub errors: Vec<ValidationError>,
}

impl ValidationErrors {
    /// Create a new validation errors collection.
    pub fn new() -> Self {
        Self { errors: Vec::new() }
    }

    /// Add a validation error.
    pub fn add(mut self, field: impl Into<String>, message: impl Into<String>) -> Self {
        self.errors.push(ValidationError {
            field: field.into(),
            message: message.into(),
            value: None,
        });
        self
    }

    /// Add a validation error with value.
    pub fn add_with_value(
        mut self,
        field: impl Into<String>,
        message: impl Into<String>,
        value: serde_json::Value,
    ) -> Self {
        self.errors.push(ValidationError {
            field: field.into(),
            message: message.into(),
            value: Some(value),
        });
        self
    }

    /// Check if there are any errors.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Convert to ErrorResponse.
    pub fn to_response_error(&self) -> ErrorResponse {
        ErrorResponse {
            code: ErrorCode::ValidationFailed.as_str().to_string(),
            message: format!("Validation failed: {} error(s)", self.errors.len()),
            status: StatusCode::UNPROCESSABLE_ENTITY,
            request_id: None,
        }
    }
}

impl Default for ValidationErrors {
    fn default() -> Self {
        Self::new()
    }
}

impl IntoResponse for ValidationErrors {
    fn into_response(self) -> Response {
        let status = StatusCode::UNPROCESSABLE_ENTITY;
        let body = serde_json::json!({
            "success": false,
            "error": {
                "code": ErrorCode::ValidationFailed.as_str(),
                "message": format!("Validation failed: {} error(s)", self.errors.len()),
                "details": {
                    "errors": self.errors
                }
            },
            "meta": {
                "timestamp": chrono::Utc::now().to_rfc3339(),
            }
        });
        (status, axum::Json(body)).into_response()
    }
}

/// Trait for validatable request bodies.
pub trait Validate {
    /// Validate the request data.
    fn validate(&self) -> Result<(), ValidationErrors>;
}

/// Helper function to validate and convert to API response.
pub fn validate_request<T: Validate>(data: &T) -> Result<(), ValidationErrors> {
    data.validate()
}

// ============================================================================
// Common Validation Rules
// ============================================================================

/// Validate that a string is not empty.
pub fn validate_not_empty(field: &str, value: &str) -> Result<(), ValidationErrors> {
    if value.trim().is_empty() {
        Err(ValidationErrors::new().add(field, "cannot be empty"))
    } else {
        Ok(())
    }
}

/// Validate string length.
pub fn validate_length(
    field: &str,
    value: &str,
    min: usize,
    max: usize,
) -> Result<(), ValidationErrors> {
    let len = value.len();
    if len < min {
        Err(ValidationErrors::new().add(
            field,
            format!("must be at least {} characters", min),
        ))
    } else if len > max {
        Err(ValidationErrors::new().add(
            field,
            format!("must be at most {} characters", max),
        ))
    } else {
        Ok(())
    }
}

/// Validate that a value is within a range.
pub fn validate_range<T>(
    field: &str,
    value: T,
    min: T,
    max: T,
) -> Result<(), ValidationErrors>
where
    T: PartialOrd + std::fmt::Display + Copy,
{
    if value < min {
        Err(ValidationErrors::new().add(
            field,
            format!("must be at least {}", min),
        ))
    } else if value > max {
        Err(ValidationErrors::new().add(
            field,
            format!("must be at most {}", max),
        ))
    } else {
        Ok(())
    }
}

/// Validate device ID format.
pub fn validate_device_id(id: &str) -> Result<(), ValidationErrors> {
    if id.is_empty() {
        return Err(ValidationErrors::new().add("device_id", "cannot be empty"));
    }
    if id.len() > 100 {
        return Err(ValidationErrors::new().add("device_id", "ID too long (max 100 characters)"));
    }
    if !id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return Err(ValidationErrors::new().add(
            "device_id",
            "can only contain alphanumeric characters, hyphens, and underscores",
        ));
    }
    Ok(())
}

/// Validate session ID format.
pub fn validate_session_id(id: &str) -> Result<(), ValidationErrors> {
    if id.is_empty() {
        return Err(ValidationErrors::new().add("session_id", "cannot be empty"));
    }
    // Session IDs should be valid UUIDs
    if uuid::Uuid::parse_str(id).is_err() {
        return Err(ValidationErrors::new().add(
            "session_id",
            "must be a valid UUID",
        ));
    }
    Ok(())
}

/// Validate IP address format.
pub fn validate_ip_address(addr: &str) -> Result<(), ValidationErrors> {
    if addr.parse::<std::net::IpAddr>().is_err() {
        return Err(ValidationErrors::new().add("ip_address", "must be a valid IP address"));
    }
    Ok(())
}

/// Validate URL format.
pub fn validate_url(url: &str) -> Result<(), ValidationErrors> {
    if url.parse::<reqwest::Url>().is_err() {
        return Err(ValidationErrors::new().add("url", "must be a valid URL"));
    }
    Ok(())
}

// ============================================================================
// Common Query Parameter Validators
// ============================================================================

/// Standard pagination query with validation.
#[derive(Debug, Clone, Deserialize)]
pub struct PageQuery {
    /// Page number (1-indexed).
    #[serde(default = "default_page")]
    pub page: usize,

    /// Number of items per page.
    #[serde(default = "default_page_size")]
    pub page_size: usize,

    /// Sort by field.
    pub sort_by: Option<String>,

    /// Sort order.
    pub sort_order: Option<SortOrder>,
}

fn default_page() -> usize {
    1
}

fn default_page_size() -> usize {
    20
}

impl Validate for PageQuery {
    fn validate(&self) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();

        if self.page < 1 {
            errors = errors.add("page", "must be >= 1");
        }

        if self.page_size < 1 {
            errors = errors.add("page_size", "must be >= 1");
        } else if self.page_size > 100 {
            errors = errors.add("page_size", "must be <= 100");
        }

        if errors.has_errors() {
            Err(errors)
        } else {
            Ok(())
        }
    }
}

impl PageQuery {
    /// Get the offset for pagination.
    pub fn offset(&self) -> usize {
        (self.page - 1) * self.page_size
    }

    /// Get the limit for pagination.
    pub fn limit(&self) -> usize {
        self.page_size
    }
}

/// Sort order enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SortOrder {
    Asc,
    Desc,
}

/// Search query parameters.
#[derive(Debug, Clone, Deserialize)]
pub struct SearchQuery {
    /// Search query string.
    pub q: String,

    /// Domains to search (empty = all).
    #[serde(default)]
    pub domains: Vec<String>,

    /// Maximum results per domain.
    #[serde(default = "default_search_limit")]
    pub limit: usize,
}

fn default_search_limit() -> usize {
    20
}

impl Validate for SearchQuery {
    fn validate(&self) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();

        if let Err(e) = validate_length("q", &self.q, 1, 100) {
            errors = errors.add("q", &e.errors[0].message);
        }

        if self.limit > 100 {
            errors = errors.add("limit", "must be <= 100");
        }

        if errors.has_errors() {
            Err(errors)
        } else {
            Ok(())
        }
    }
}

/// Device query parameters.
#[derive(Debug, Clone, Deserialize)]
pub struct DeviceQuery {
    /// Filter by device type.
    pub device_type: Option<String>,

    /// Filter by status.
    pub status: Option<String>,

    /// Search in device name.
    pub search: Option<String>,

    /// Page number.
    #[serde(default = "default_page")]
    pub page: usize,

    /// Page size.
    #[serde(default = "default_page_size")]
    pub page_size: usize,
}

impl Validate for DeviceQuery {
    fn validate(&self) -> Result<(), ValidationErrors> {
        PageQuery {
            page: self.page,
            page_size: self.page_size,
            sort_by: None,
            sort_order: None,
        }.validate()
    }
}

/// Rule query parameters.
#[derive(Debug, Clone, Deserialize)]
pub struct RuleQuery {
    /// Filter by enabled status.
    pub enabled: Option<bool>,

    /// Search in rule name.
    pub search: Option<String>,

    /// Page number.
    #[serde(default = "default_page")]
    pub page: usize,

    /// Page size.
    #[serde(default = "default_page_size")]
    pub page_size: usize,
}

impl Validate for RuleQuery {
    fn validate(&self) -> Result<(), ValidationErrors> {
        PageQuery {
            page: self.page,
            page_size: self.page_size,
            sort_by: None,
            sort_order: None,
        }.validate()
    }
}

/// Alert query parameters.
#[derive(Debug, Clone, Deserialize)]
pub struct AlertQuery {
    /// Filter by severity.
    pub severity: Option<String>,

    /// Filter by active status.
    pub active: Option<bool>,

    /// Filter by acknowledged status.
    pub acknowledged: Option<bool>,

    /// Page number.
    #[serde(default = "default_page")]
    pub page: usize,

    /// Page size.
    #[serde(default = "default_page_size")]
    pub page_size: usize,
}

impl Validate for AlertQuery {
    fn validate(&self) -> Result<(), ValidationErrors> {
        PageQuery {
            page: self.page,
            page_size: self.page_size,
            sort_by: None,
            sort_order: None,
        }.validate()
    }
}

/// Time range query parameters.
#[derive(Debug, Clone, Deserialize)]
pub struct TimeRangeQuery {
    /// Start timestamp (Unix seconds).
    pub start: i64,

    /// End timestamp (Unix seconds).
    pub end: i64,
}

impl Validate for TimeRangeQuery {
    fn validate(&self) -> Result<(), ValidationErrors> {
        if self.start >= self.end {
            return Err(ValidationErrors::new().add("time_range", "start must be before end"));
        }
        Ok(())
    }
}

// ============================================================================
// Middleware
// ============================================================================

/// Validation middleware for request size checking.
///
/// This middleware checks that the Content-Length header doesn't exceed
/// the maximum allowed size before processing the request.
///
/// # Example
///
/// ```ignore
/// use axum::Router;
/// use edge_ai_api::validation_middleware;
///
/// let app = Router::new()
///     .layer(axum::middleware::from_fn(
///         validation_middleware(1024 * 1024) // 1MB max
///     ));
/// ```
pub fn validation_middleware(
    max_size: usize,
) -> impl Fn(Request, Next) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> + Clone {
    move |req: Request, next: Next| {
        Box::pin(async move {
            // Check Content-Length header
            if let Some(content_length) = req.headers().get("content-length") {
                if let Ok(length_str) = content_length.to_str() {
                    if let Ok(length) = length_str.parse::<usize>() {
                        if length > max_size {
                            let error = ErrorResponse {
                                code: ErrorCode::BadRequest.as_str().to_string(),
                                message: format!("Request body too large (max {} bytes)", max_size),
                                status: StatusCode::PAYLOAD_TOO_LARGE,
                                request_id: None,
                            };
                            return error.into_response();
                        }
                    }
                }
            }

            next.run(req).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_not_empty() {
        assert!(validate_not_empty("field", "test").is_ok());
        assert!(validate_not_empty("field", "").is_err());
        assert!(validate_not_empty("field", "   ").is_err());
    }

    #[test]
    fn test_validate_length() {
        assert!(validate_length("field", "test", 1, 10).is_ok());
        assert!(validate_length("field", "", 1, 10).is_err());
        assert!(validate_length("field", "abcdefghijk", 1, 10).is_err());
    }

    #[test]
    fn test_validate_range() {
        assert!(validate_range("field", 5, 1, 10).is_ok());
        assert!(validate_range("field", 0, 1, 10).is_err());
        assert!(validate_range("field", 11, 1, 10).is_err());
    }

    #[test]
    fn test_validate_device_id() {
        assert!(validate_device_id("device-123").is_ok());
        assert!(validate_device_id("device_456").is_ok());
        assert!(validate_device_id("device@789").is_err());
        assert!(validate_device_id(&"a".repeat(101)).is_err());
    }

    #[test]
    fn test_validate_session_id() {
        let uuid = uuid::Uuid::new_v4().to_string();
        assert!(validate_session_id(&uuid).is_ok());
        assert!(validate_session_id("").is_err());
        assert!(validate_session_id("not-a-uuid").is_err());
    }

    #[test]
    fn test_page_query_validation() {
        let query = PageQuery {
            page: 1,
            page_size: 20,
            sort_by: None,
            sort_order: None,
        };
        assert!(query.validate().is_ok());

        let query = PageQuery {
            page: 0,
            page_size: 20,
            sort_by: None,
            sort_order: None,
        };
        assert!(query.validate().is_err());

        let query = PageQuery {
            page: 1,
            page_size: 101,
            sort_by: None,
            sort_order: None,
        };
        assert!(query.validate().is_err());
    }

    #[test]
    fn test_search_query_validation() {
        let query = SearchQuery {
            q: "test".to_string(),
            domains: vec![],
            limit: 10,
        };
        assert!(query.validate().is_ok());

        let query = SearchQuery {
            q: "".to_string(),
            domains: vec![],
            limit: 10,
        };
        assert!(query.validate().is_err());

        let query = SearchQuery {
            q: "test".to_string(),
            domains: vec![],
            limit: 101,
        };
        assert!(query.validate().is_err());
    }

    #[test]
    fn test_time_range_validation() {
        let query = TimeRangeQuery {
            start: 1000,
            end: 2000,
        };
        assert!(query.validate().is_ok());

        let query = TimeRangeQuery {
            start: 2000,
            end: 1000,
        };
        assert!(query.validate().is_err());
    }
}
