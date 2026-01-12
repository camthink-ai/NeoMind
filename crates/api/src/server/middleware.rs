//! Server middleware.

use std::net::SocketAddr;
use axum::{extract::ConnectInfo, extract::State, middleware::Next, http::Request, body::Body, response::IntoResponse};

use crate::rate_limit::{extract_client_id, RateLimitExceeded};
use super::types::ServerState;

/// Rate limiting middleware.
///
/// Uses API key (if authenticated) or IP address for rate limiting.
/// Public endpoints have higher limits; protected endpoints have standard limits.
pub async fn rate_limit_middleware(
    State(state): State<ServerState>,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    request: Request<Body>,
    next: Next,
) -> axum::response::Response {
    // Extract client identifier from the request headers
    let client_id = extract_client_id(request.headers(), connect_info.as_ref());

    // Check rate limit
    match state.rate_limiter.check_rate_limit(&client_id).await {
        Ok(_) => {
            // Rate limit OK, proceed
            next.run(request).await
        }
        Err(e) => {
            // Only log if this is the first warning in the debounce window
            if e.should_log() {
                tracing::warn!(
                    category = "rate_limit",
                    client = %client_id,
                    wait_seconds = e.wait_seconds,
                    "Rate limit exceeded"
                );
            }
            e.into_response()
        }
    }
}
