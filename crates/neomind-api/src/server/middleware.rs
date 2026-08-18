//! Server middleware.

use axum::{
    body::Body, extract::ConnectInfo, extract::State, http::Request, middleware::Next,
    response::IntoResponse,
};
use std::net::SocketAddr;

use super::types::ServerState;
use crate::rate_limit::extract_client_id;

/// Rate limiting middleware.
///
/// Uses API key (if authenticated) or IP address for rate limiting.
/// Public endpoints have higher limits; protected endpoints have standard limits.
/// WebSocket and SSE endpoints are excluded from rate limiting.
///
pub async fn rate_limit_middleware(
    State(state): State<ServerState>,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    request: Request<Body>,
    next: Next,
) -> axum::response::Response {
    let uri = request.uri().path();
    if uri.contains("/chat") || uri.contains("/ws") || uri.contains("/events/stream") {
        return next.run(request).await;
    }

    let client_id = extract_client_id(request.headers(), connect_info.as_ref());

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
}

/// Webhook-specific rate-limit middleware.
///
/// Same engine as `rate_limit_middleware`, but produces a composite `client_id`
/// that embeds the `device_id` from the URL path. Without this, devices that
/// share an adapter-level `X-API-Key` would all land in the same rate-limit
/// bucket (`apikey:<hash>`), so one chatty device could starve its neighbors.
///
/// Per-device endpoint (`POST /api/devices/:id/webhook`) → bucket becomes
/// `apikey:<hash>:<device_id>` (or `ip:<addr>:<device_id>` when no API key is
/// present), giving each device its own quota even under a shared key.
///
/// Generic endpoint (`POST /api/devices/webhook`) — the device_id lives in the
/// request body and can't be read here without consuming it. Devices on this
/// endpoint that share an adapter API key share a rate-limit bucket. Workaround:
/// prefer the per-device URL endpoint, or configure per-device `webhook_token`s
/// (each gets a unique `Authorization: Bearer` hash → unique bucket).
pub async fn webhook_rate_limit_middleware(
    State(state): State<ServerState>,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    request: Request<Body>,
    next: Next,
) -> axum::response::Response {
    let uri = request.uri().path();
    let headers = request.headers();

    let base_id = extract_client_id(headers, connect_info.as_ref());

    // Extract device_id from /api/devices/:id/webhook. The literal `webhook`
    // segment is filtered out so the generic endpoint (`/api/devices/webhook`)
    // falls through to the un-composited base_id.
    let device_segment = uri
        .strip_prefix("/api/devices/")
        .and_then(|rest| rest.split('/').next())
        .filter(|seg| !seg.is_empty() && *seg != "webhook");

    let client_id = match device_segment {
        Some(id) => format!("{}:{}", base_id, id),
        None => base_id,
    };

    match state.rate_limiter.check_rate_limit(&client_id) {
        Ok(_) => next.run(request).await,
        Err(e) => {
            if e.should_log() {
                tracing::warn!(
                    category = "rate_limit",
                    client = %client_id,
                    wait_seconds = e.wait_seconds,
                    "Webhook rate limit exceeded"
                );
            }
            e.into_response()
        }
    }
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
