//! Pagination support for list endpoints.

use axum::extract::Query;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Default page size.
pub const DEFAULT_PAGE_SIZE: u32 = 20;
/// Maximum page size.
pub const MAX_PAGE_SIZE: u32 = 100;

/// Pagination parameters extracted from query string.
#[derive(Debug, Clone, Deserialize)]
pub struct PaginationParams {
    /// Page number (1-indexed).
    #[serde(default = "default_page")]
    pub page: u32,
    /// Number of items per page.
    #[serde(default = "default_page_size")]
    pub page_size: u32,
}

fn default_page() -> u32 {
    1
}

fn default_page_size() -> u32 {
    DEFAULT_PAGE_SIZE
}

impl PaginationParams {
    /// Create pagination params from a query map.
    pub fn from_query_map(query: &HashMap<String, String>) -> Self {
        Self {
            page: query.get("page")
                .and_then(|s| s.parse().ok())
                .unwrap_or(1),
            page_size: query.get("page_size")
                .and_then(|s| s.parse().ok())
                .unwrap_or(DEFAULT_PAGE_SIZE)
                .min(MAX_PAGE_SIZE),
        }
    }

    /// Get the offset for database queries.
    pub fn offset(&self) -> u32 {
        (self.page - 1) * self.page_size
    }

    /// Get the limit for database queries.
    pub fn limit(&self) -> u32 {
        self.page_size
    }

    /// Calculate total pages.
    pub fn total_pages(&self, total_count: u32) -> u32 {
        (total_count + self.page_size - 1) / self.page_size
    }
}

/// Paginated response wrapper.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedResponse<T> {
    /// The data items for the current page.
    pub data: Vec<T>,
    /// Pagination metadata.
    pub pagination: PaginationMeta,
}

/// Pagination metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginationMeta {
    /// Current page number.
    pub page: u32,
    /// Number of items per page.
    pub page_size: u32,
    /// Total number of items across all pages.
    pub total_count: u32,
    /// Total number of pages.
    pub total_pages: u32,
    /// Whether there is a next page.
    pub has_next: bool,
    /// Whether there is a previous page.
    pub has_prev: bool,
}

impl PaginationMeta {
    /// Create pagination metadata.
    pub fn new(page: u32, page_size: u32, total_count: u32) -> Self {
        let total_pages = if page_size > 0 {
            (total_count + page_size - 1) / page_size
        } else {
            0
        };

        Self {
            page,
            page_size,
            total_count,
            total_pages,
            has_next: page < total_pages,
            has_prev: page > 1,
        }
    }
}

/// Extractor for pagination parameters.
pub struct Pagination {
    pub page: u32,
    pub page_size: u32,
}

impl Pagination {
    /// Get the offset for database queries.
    pub fn offset(&self) -> u32 {
        (self.page - 1) * self.page_size
    }

    /// Get the limit for database queries.
    pub fn limit(&self) -> u32 {
        self.page_size
    }

    /// Create pagination metadata.
    pub fn meta(&self, total_count: u32) -> PaginationMeta {
        PaginationMeta::new(self.page, self.page_size, total_count)
    }
}

impl From<PaginationParams> for Pagination {
    fn from(params: PaginationParams) -> Self {
        Self {
            page: params.page.max(1),
            page_size: params.page_size.clamp(1, MAX_PAGE_SIZE),
        }
    }
}

impl From<Query<PaginationParams>> for Pagination {
    fn from(Query(params): Query<PaginationParams>) -> Self {
        Self::from(params)
    }
}

/// Helper to create a paginated response.
pub fn paginated<T>(data: Vec<T>, pagination: &Pagination, total_count: u32) -> PaginatedResponse<T> {
    PaginatedResponse {
        data,
        pagination: pagination.meta(total_count),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pagination_params() {
        let params = PaginationParams {
            page: 2,
            page_size: 10,
        };

        assert_eq!(params.offset(), 10);
        assert_eq!(params.limit(), 10);
        assert_eq!(params.total_pages(25), 3);
    }

    #[test]
    fn test_pagination_meta() {
        let meta = PaginationMeta::new(2, 10, 25);

        assert_eq!(meta.page, 2);
        assert_eq!(meta.page_size, 10);
        assert_eq!(meta.total_count, 25);
        assert_eq!(meta.total_pages, 3);
        assert!(meta.has_next);
        assert!(meta.has_prev);
    }
}
