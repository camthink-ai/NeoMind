//! Server middleware.

use axum::{
    body::Body, extract::ConnectInfo, extract::State, http::Request, middleware::Next,
    response::IntoResponse,
};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;
use std::time::Duration;

use super::types::ServerState;
use crate::rate_limit::{extract_client_id, RateLimitConfig, RateLimiter};

/// Dedicated per-IP rate limiter for WebSocket upgrade handshakes.
/// Generous enough for normal multi-tab UI reconnects (120 handshakes / 60s
/// per IP) while blocking unauthenticated Slowloris / WS connection storms.
static WS_HANDSHAKE_LIMITER: LazyLock<RateLimiter> = LazyLock::new(|| {
    RateLimiter::with_config(RateLimitConfig {
        max_requests: 120,
        per_duration: Duration::from_secs(60),
        warn_interval: Duration::from_secs(5),
    })
});

static WS_HANDSHAKE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn check_ws_handshake_limit(
    headers: &axum::http::HeaderMap,
    connect_info: Option<&ConnectInfo<SocketAddr>>,
) -> Result<(), crate::rate_limit::RateLimitExceeded> {
    // Periodic cleanup every 1024 handshakes to bound memory under IP churn.
    if WS_HANDSHAKE_COUNTER
        .fetch_add(1, Ordering::Relaxed)
        .is_multiple_of(1024)
    {
        WS_HANDSHAKE_LIMITER.cleanup_old_entries();
    }

    // Prefer raw source IP so rotating unauthenticated headers cannot bypass.
    let key = match connect_info {
        Some(ci) => format!("ws:ip:{}", ci.0.ip()),
        None => format!("ws:{}", extract_client_id(headers, None)),
    };
    WS_HANDSHAKE_LIMITER.check_rate_limit(&key)
}

/// Rate limiting middleware.
///
/// Uses API key (if authenticated) or IP address for rate limiting.
/// Public endpoints have higher limits; protected endpoints have standard limits.
/// WebSocket upgrade handshakes are guarded by a dedicated per-IP handshake
/// limiter so unauthenticated connection storms are blocked while normal UI
/// reconnects work. Long-lived chat and SSE endpoints are excluded from the
/// standard per-request limiter.
pub async fn rate_limit_middleware(
    State(state): State<ServerState>,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    request: Request<Body>,
    next: Next,
) -> axum::response::Response {
    let uri = request.uri().path();
    let is_ws = uri.contains("/ws")
        || request
            .headers()
            .get(axum::http::header::UPGRADE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.eq_ignore_ascii_case("websocket"))
            .unwrap_or(false);

    if is_ws {
        return match check_ws_handshake_limit(request.headers(), connect_info.as_ref()) {
            Ok(_) => next.run(request).await,
            Err(e) => {
                if e.should_log() {
                    tracing::warn!(
                        category = "rate_limit",
                        path = %uri,
                        wait_seconds = e.wait_seconds,
                        "WebSocket handshake rate limit exceeded"
                    );
                }
                e.into_response()
            }
        };
    }

    if uri.contains("/chat") || uri.contains("/events/stream") {
        return next.run(request).await;
    }

    // 1. Enforce an aggregate per-IP floor so rotating `X-Session-Id` or fake
    //    `Authorization` / `X-API-Key` headers cannot bypass rate limiting.
    if let Some(ci) = connect_info.as_ref() {
        let ip_key = format!("agg:ip:{}", ci.0.ip());
        if let Err(e) = state.rate_limiter.check_rate_limit(&ip_key) {
            if e.should_log() {
                tracing::warn!(
                    category = "rate_limit",
                    client = %ip_key,
                    wait_seconds = e.wait_seconds,
                    "Aggregate per-IP rate limit exceeded"
                );
            }
            return e.into_response();
        }
    }

    let client_id = extract_client_id(request.headers(), connect_info.as_ref());

    // Skip a redundant second check when `client_id` already fell back to raw IP.
    let is_raw_ip_key = connect_info
        .as_ref()
        .is_some_and(|ci| client_id == format!("ip:{}", ci.0.ip()));
    if !is_raw_ip_key {
        if let Err(e) = state.rate_limiter.check_rate_limit(&client_id) {
            if e.should_log() {
                tracing::warn!(
                    category = "rate_limit",
                    client = %client_id,
                    wait_seconds = e.wait_seconds,
                    "Rate limit exceeded"
                );
            }
            return e.into_response();
        }
    }

    next.run(request).await
}

/// Webhook-specific rate-limit middleware.
///
/// Enforces an aggregate per-IP / per-credential (`base_id`) rate limit BEFORE
/// allocating or checking any per-device bucket, so rotating `:id` in
/// `POST /api/devices/:id/webhook` (or rotating fake credentials) cannot bypass
/// rate limiting or flood `RateLimiter.clients` with unbounded keys.
///
/// Once the aggregate budget passes, per-device endpoints additionally enforce
/// a per-device quota (`<base_id>:<device_id>`).
pub async fn webhook_rate_limit_middleware(
    State(state): State<ServerState>,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    request: Request<Body>,
    next: Next,
) -> axum::response::Response {
    let uri = request.uri().path();
    let headers = request.headers();

    // 1. Aggregate per-source-IP check first (cannot be bypassed by rotating
    //    device_id or fake Authorization/X-API-Key headers).
    if let Some(ci) = connect_info.as_ref() {
        let ip_bucket = format!("webhook:ip:{}", ci.0.ip());
        if let Err(e) = state.rate_limiter.check_rate_limit(&ip_bucket) {
            if e.should_log() {
                tracing::warn!(
                    category = "rate_limit",
                    client = %ip_bucket,
                    wait_seconds = e.wait_seconds,
                    "Webhook aggregate IP rate limit exceeded"
                );
            }
            return e.into_response();
        }
    }

    let base_id = extract_client_id(headers, connect_info.as_ref());

    // 2. Aggregate per-client (`base_id`) check BEFORE creating per-device
    //    keys so rotating `:id` cannot bypass rate limiting or flood clients.
    if let Err(e) = state.rate_limiter.check_rate_limit(&base_id) {
        if e.should_log() {
            tracing::warn!(
                category = "rate_limit",
                client = %base_id,
                wait_seconds = e.wait_seconds,
                "Webhook aggregate rate limit exceeded"
            );
        }
        return e.into_response();
    }

    // 3. Per-device quota for /api/devices/:id/webhook. The literal `webhook`
    //    segment is filtered out so the generic endpoint (`/api/devices/webhook`)
    //    doesn't double-charge the same bucket.
    let device_segment = uri
        .strip_prefix("/api/devices/")
        .and_then(|rest| rest.split('/').next())
        .filter(|seg| !seg.is_empty() && *seg != "webhook");

    if let Some(id) = device_segment {
        let client_id = format!("{}:{}", base_id, id);
        if let Err(e) = state.rate_limiter.check_rate_limit(&client_id) {
            if e.should_log() {
                tracing::warn!(
                    category = "rate_limit",
                    client = %client_id,
                    wait_seconds = e.wait_seconds,
                    "Webhook per-device rate limit exceeded"
                );
            }
            return e.into_response();
        }
    }

    next.run(request).await
}

/// Data-change publisher middleware.
///
/// After a successful mutating request (POST/PUT/PATCH/DELETE) on a data
/// domain, publishes a `NeoMindEvent::DataChanged` on the event bus so
/// connected clients (web pages, the chat panel) refresh their caches —
/// changes made by ANY actor (AI agent via CLI, another user, background
/// job) show up without a manual reload.
///
/// Domain allow-list (first path segment after /api/): keeps telemetry
/// ingestion and auth/event chatter off the bus. Device webhook ingestion
/// paths are excluded explicitly — they are data-plane, not CRUD.
pub async fn data_change_publisher(
    State(state): State<ServerState>,
    request: Request<Body>,
    next: Next,
) -> axum::response::Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let response = next.run(request).await;

    if !response.status().is_success() {
        return response;
    }
    let is_mutating = method == axum::http::Method::POST
        || method == axum::http::Method::PUT
        || method == axum::http::Method::PATCH
        || method == axum::http::Method::DELETE;
    if !is_mutating {
        return response;
    }
    let Some(domain) = data_change_domain(&path) else {
        return response;
    };

    if let Some(bus) = state.core.event_bus.clone() {
        let event = neomind_core::NeoMindEvent::DataChanged {
            domain,
            method: method.to_string(),
            path,
            timestamp: chrono::Utc::now().timestamp_millis(),
        };
        tokio::spawn(async move {
            bus.publish(event).await;
        });
    }
    response
}

/// Resolve the data domain for a mutating path, or None if the path should
/// not emit change events.
fn data_change_domain(path: &str) -> Option<String> {
    let rest = path.strip_prefix("/api/")?;
    let domain = rest.split('/').next()?;
    if domain.is_empty() {
        return None;
    }
    match domain {
        "devices" if path.contains("/webhook") => None, // telemetry ingestion
        "devices"
        | "device-types"
        | "automations"
        | "rules"
        | "dashboards"
        | "data-push"
        | "message-channels"
        | "im-bridges"
        | "brokers"
        | "extensions"
        | "frontend-components"
        | "agents"
        | "skills"
        | "instances"
        | "sessions"
        | "users"
        | "llm" => Some(domain.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod data_change_tests {
    use super::data_change_domain;

    #[test]
    fn crud_domains_resolve() {
        assert_eq!(
            data_change_domain("/api/devices"),
            Some("devices".to_string())
        );
        assert_eq!(
            data_change_domain("/api/automations/transforms"),
            Some("automations".to_string())
        );
        assert_eq!(
            data_change_domain("/api/extensions/pkg-1/enable"),
            Some("extensions".to_string())
        );
    }

    #[test]
    fn ingestion_and_chatter_are_excluded() {
        // device data-plane webhook → no event storm
        assert_eq!(data_change_domain("/api/devices/dev-1/webhook"), None);
        assert_eq!(data_change_domain("/api/devices/webhook"), None);
        // auth / events / telemetry / chat-ws never emit
        assert_eq!(data_change_domain("/api/auth/login"), None);
        assert_eq!(data_change_domain("/api/events/publish"), None);
        assert_eq!(data_change_domain("/api/telemetry/query"), None);
        assert_eq!(data_change_domain("/api/chat/anything"), None);
        // GET-style non-api path
        assert_eq!(data_change_domain("/assets/index.js"), None);
    }
}
