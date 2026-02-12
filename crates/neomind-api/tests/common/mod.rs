//! Common test utilities for API tests.
//!
//! Tests should use unique usernames/IDs to avoid conflicts when running in parallel.

use std::sync::Arc;

/// Create a mock server state for testing.
///
/// Note: Tests should use unique usernames/IDs to avoid conflicts when running in parallel.
/// Each call creates a completely isolated ServerState with in-memory storage.
pub async fn create_test_server_state() -> neomind_api::handlers::ServerState {
    neomind_api::handlers::ServerState::new_for_testing().await
}
