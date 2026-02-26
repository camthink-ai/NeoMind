//! Server middleware.

use axum::{body::Body, extract::ConnectInfo, extract::State, http::Request, middleware::Next};
use std::net::SocketAddr;

use super::types::ServerState;

/// Rate limiting middleware.
///
/// Uses API key (if authenticated) or IP address for rate limiting.
/// Public endpoints have higher limits; protected endpoints have standard limits.
/// WebSocket and SSE endpoints are excluded from rate limiting.
///
/// NOTE: Rate limiting is currently disabled to allow for development and testing.
/// To re-enable, uncomment the rate limit check code below.
pub async fn rate_limit_middleware(
    State(_state): State<ServerState>,
    _connect_info: Option<ConnectInfo<SocketAddr>>,
    request: Request<Body>,
    next: Next,
) -> axum::response::Response {
    // Rate limiting disabled - proceed with request
    next.run(request).await

    // To re-enable rate limiting, uncomment the code below:
    /*
    let uri = request.uri().path();
    if uri.contains("/chat") || uri.contains("/ws") || uri.contains("/events/stream") {
        return next.run(request).await;
    }

    let client_id = extract_client_id(request.headers(), connect_info.as_ref());

    // Note: check_rate_limit is now synchronous (no .await needed)
    match state.rate_limiter.check_rate_limit(&client_id) {
        Ok(_) => next.run(request).await,
        Err(e) => {
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
    */
}
