//! Common test utilities for API tests.

use std::sync::Arc;
use tokio::sync::broadcast;

use neomind_core::EventBus;
use neomind_agent::SessionManager;
use neomind_rules::InMemoryValueProvider;
use neomind_commands::{CommandManager, CommandQueue, CommandStateStore};
use neomind_devices::{DeviceRegistry, DeviceService, TimeSeriesStorage};
use neomind_messages::MessageManager;

use neomind_api::auth::AuthState;
use neomind_api::auth_users::AuthUserState;
use neomind_api::cache::ResponseCache;
use neomind_api::rate_limit::{RateLimitConfig, RateLimiter};

/// Create a mock server state for testing.
///
/// Note: This uses ServerState::new() which initializes all components properly.
/// For a more minimal test state, consider using mock builders in the future.
pub async fn create_test_server_state() -> neomind_api::handlers::ServerState {
    neomind_api::handlers::ServerState::new().await
}
