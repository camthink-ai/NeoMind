//! Common API handler utilities.
//!
//! This module provides shared utilities for API handlers including
//! unified error handling, response builders, and common patterns.

use axum::response::{IntoResponse, Response, Json};
use axum::http::StatusCode;
use serde::Deserialize;

use crate::models::{error::ErrorResponse, common::ApiResponse, pagination::PaginationMeta};

/// Unified Result type for all API handlers.
///
/// All handlers should return this type for consistent error handling.
/// The success value is automatically wrapped in ApiResponse.
pub type HandlerResult<T> = Result<Json<ApiResponse<T>>, ErrorResponse>;

/// Result type for utility functions that return parsed values (not full responses).
pub type ExtractResult<T> = Result<T, ErrorResponse>;

/// Pagination query parameters.
#[derive(Debug, Clone, Deserialize)]
pub struct PaginationQuery {
    /// Page number (1-indexed).
    #[serde(default = "default_page")]
    pub page: usize,

    /// Items per page.
    #[serde(default = "default_page_size")]
    pub page_size: usize,
}

fn default_page() -> usize {
    1
}

fn default_page_size() -> usize {
    20
}

impl PaginationQuery {
    /// Get the offset for database queries.
    pub fn offset(&self) -> usize {
        (self.page.saturating_sub(1)) * self.page_size
    }

    /// Get the limit for database queries.
    pub fn limit(&self) -> usize {
        self.page_size
    }

    /// Validate and clamp the pagination parameters.
    pub fn validate(mut self) -> ExtractResult<Self> {
        if self.page == 0 {
            self.page = 1;
        }
        if self.page_size == 0 {
            self.page_size = 20;
        }
        if self.page_size > 1000 {
            return Err(ErrorResponse::bad_request("page_size cannot exceed 1000"));
        }
        Ok(self)
    }

    /// Convert to PaginationMeta for API responses.
    pub fn to_meta(&self, total: u32) -> PaginationMeta {
        let total_pages = (total as usize + self.page_size - 1) / self.page_size;
        PaginationMeta {
            page: self.page as u32,
            page_size: self.page_size as u32,
            total_count: total,
            total_pages: total_pages as u32,
            has_next: self.page < total_pages,
            has_prev: self.page > 1,
        }
    }
}

/// Extract a path parameter or return a 400 error.
///
/// # Example
/// ```rust,no_run
/// use axum::extract::Path;
/// use crate::handlers::common::extract_path;
///
/// async fn get_item(Path(id): Path<String>) -> HandlerResult<Item> {
///     let id = extract_path::<i32>(&id)?;
///     // ...
///     # todo!()
/// }
/// ```
pub fn extract_path<T>(value: &str) -> ExtractResult<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value
        .parse::<T>()
        .map_err(|_| ErrorResponse::bad_request(format!("Invalid path parameter: {}", value)))
}

/// Extract an optional query parameter.
pub fn extract_query_opt<T>(value: &Option<String>) -> ExtractResult<Option<T>>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match value {
        Some(v) => {
            let parsed = v
                .parse::<T>()
                .map_err(|_| ErrorResponse::bad_request(format!("Invalid query parameter: {}", v)))?;
            Ok(Some(parsed))
        }
        None => Ok(None),
    }
}

/// Extract a required query parameter or return a 400 error.
pub fn extract_query<T>(value: &Option<String>, name: &str) -> ExtractResult<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match value {
        Some(v) => v
            .parse::<T>()
            .map_err(|_| ErrorResponse::bad_request(format!("Invalid query parameter '{}': {}", name, v))),
        None => Err(ErrorResponse::bad_request(format!("Missing required query parameter: {}", name))),
    }
}

/// Create a successful response with data.
pub fn ok<T: serde::Serialize>(data: T) -> HandlerResult<T> {
    Ok(Json(ApiResponse::success(data)))
}

/// Create a successful response with data and metadata (for pagination).
pub fn ok_with_meta<T: serde::Serialize>(
    data: T,
    meta: PaginationMeta,
) -> HandlerResult<T> {
    Ok(Json(ApiResponse::paginated(data, meta)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_path_valid() {
        let result: ExtractResult<i32> = extract_path("42");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_extract_path_invalid() {
        let result: ExtractResult<i32> = extract_path("abc");
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_query_opt_some() {
        let value = Some("42".to_string());
        let result: ExtractResult<Option<i32>> = extract_query_opt(&value);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(42));
    }

    #[test]
    fn test_extract_query_opt_none() {
        let value: Option<String> = None;
        let result: ExtractResult<Option<i32>> = extract_query_opt(&value);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);
    }

    #[test]
    fn test_pagination_offset() {
        let query = PaginationQuery {
            page: 2,
            page_size: 10,
        };
        assert_eq!(query.offset(), 10);
        assert_eq!(query.limit(), 10);
    }

    #[test]
    fn test_pagination_validate() {
        let query = PaginationQuery {
            page: 0,
            page_size: 50,
        };
        let result = query.validate().unwrap();
        assert_eq!(result.page, 1); // Fixed to 1
        assert_eq!(result.page_size, 50);
    }

    #[test]
    fn test_pagination_validate_max_page_size() {
        let query = PaginationQuery {
            page: 1,
            page_size: 2000,
        };
        let result = query.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_pagination_to_meta() {
        let query = PaginationQuery {
            page: 1,
            page_size: 10,
        };
        let meta = query.to_meta(25);
        assert_eq!(meta.page, 1);
        assert_eq!(meta.page_size, 10);
        assert_eq!(meta.total_count, 25);
        assert_eq!(meta.total_pages, 3);
    }

    #[test]
    fn test_ok_helper() {
        let result: HandlerResult<String> = ok("test".to_string());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0 .data, "test");
    }

    #[test]
    fn test_ok_with_meta_helper() {
        let meta = PaginationMeta {
            page: 1,
            page_size: 10,
            total_count: 100,
            total_pages: 10,
        };
        let result: HandlerResult<String> = ok_with_meta("test".to_string(), meta);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0 .data, "test");
    }
}
