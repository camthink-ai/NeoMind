//! `/api/docs` — the API reference index the CLI help text promises.
//!
//! Scope note: this is NOT a full OpenAPI generator (utoipa annotation
//! across ~109 routes is its own project). It is a honest, always-current
//! route INDEX generated from the router's own registration calls at
//! compile-time-mirrored constants: method, path, auth class, and the
//! handler's source location for drill-down. Two representations:
//! - `GET /api/docs` → human-readable HTML index (grouped by auth class)
//! - `GET /api/docs/routes.json` → machine-readable, for client tooling
//!
//! The previous state was worse than no docs: the CLI promised
//! "swagger at /api/docs" and the path 404'd into static-file serving.

use axum::extract::Path as AxumPath;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Json, Response};

/// One documented route.
#[derive(serde::Serialize, Clone)]
pub struct RouteDoc {
    pub method: &'static str,
    pub path: &'static str,
    /// public | jwt | api-key | webhook | ws
    pub auth: &'static str,
}

/// Route registry — single source consumed by both representations.
/// Kept hand-maintained next to the router; the router stays the truth and
/// a CI drift test (tests/api_docs.rs) fails when the two disagree.
pub static ROUTES: &[RouteDoc] = &[
    RouteDoc {
        method: "GET",
        path: "/api/docs",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/docs/routes.json",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/docs/*rest",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/health",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/health/status",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/health/live",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/health/ready",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/system/network-info",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/auth/status",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/auth/verify",
        auth: "public",
    },
    RouteDoc {
        method: "POST",
        path: "/api/auth/login",
        auth: "public",
    },
    RouteDoc {
        method: "POST",
        path: "/api/auth/register",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/setup/status",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/metrics",
        auth: "public",
    },
    RouteDoc {
        method: "POST",
        path: "/api/setup/initialize",
        auth: "public",
    },
    RouteDoc {
        method: "POST",
        path: "/api/setup/complete",
        auth: "public",
    },
    RouteDoc {
        method: "POST",
        path: "/api/setup/llm-config",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/llm-backends/types",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/llm-backends/types/:type/schema",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/messages/channels/types",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/messages/channels/types/:type/schema",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/extensions",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/extensions/types",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/extensions/dashboard-components",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/extensions/capabilities",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/capabilities",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/capabilities/:name",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/images/*path",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/tools",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/tools/:name",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/extensions/:id",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/extensions/:id/health",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/extensions/:id/commands",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/extensions/:id/components",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/extensions/:id/assets/*asset_path",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/extensions/:id/event-subscriptions",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/extensions/:id/stream/capability",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/extensions/:id/stream/sessions",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/suggestions",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/suggestions/categories",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/device-types/cloud/list",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/extensions/market/list",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/extensions/market/:id",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/extensions/market/:id/readme",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/extensions/market/updates",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/share/:token",
        auth: "public",
    },
    RouteDoc {
        method: "ANY",
        path: "/api/share/:token/proxy/*path",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/frontend-components/market/list",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/frontend-components/:id/bundle",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/onboarding/status",
        auth: "public",
    },
    RouteDoc {
        method: "POST",
        path: "/api/onboarding/dismiss",
        auth: "public",
    },
    RouteDoc {
        method: "POST",
        path: "/api/onboarding/reset",
        auth: "public",
    },
    RouteDoc {
        method: "GET",
        path: "/api/auth/me",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/auth/logout",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/auth/change-password",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/events/ws",
        auth: "ws",
    },
    RouteDoc {
        method: "GET",
        path: "/api/events/stream",
        auth: "ws",
    },
    RouteDoc {
        method: "GET",
        path: "/api/chat",
        auth: "ws",
    },
    RouteDoc {
        method: "GET",
        path: "/api/extensions/:id/stream",
        auth: "ws",
    },
    RouteDoc {
        method: "POST",
        path: "/api/devices/:id/webhook",
        auth: "webhook",
    },
    RouteDoc {
        method: "POST",
        path: "/api/devices/webhook",
        auth: "webhook",
    },
    RouteDoc {
        method: "GET",
        path: "/api/telemetry",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/telemetry/stats",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/data/sources",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/stats/system",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/logs/download",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/devices/:id/webhook-url",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/llm-backends",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/llm-backends/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/llm-backends/stats",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/llm-backends/ollama/models",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/llm-backends/llamacpp/server-info",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/messages/channels",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/messages/channels/:name",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/messages/channels/stats",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/im-bridges",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/im-bridges/:id/invites",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/im-bridges/:id/allowlist",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/im-bridges/:id/sessions",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/skills",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/skills/match",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/skills/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/extensions/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/extensions/:id/logs",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/extensions/:id/descriptor",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/extensions/:id/data-sources",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/extensions/:id/metrics/:metric/data",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/extensions/:id/push-metrics",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/extensions/:id/command",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/extensions/:id/invoke",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/extensions/:id/reload",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/events",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/sessions",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/sessions",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/sessions/cleanup",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/sessions/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/sessions/:id/history",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/sessions/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/sessions/:id/memory-toggle",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/sessions/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/sessions/:id/chat",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/skills",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/skills/reload",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/skills/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/sessions/:id/pending",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/sessions/:id/pending",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/devices",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/devices",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/devices/ble-provision",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/devices/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/devices/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/devices/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/devices/:id/current",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/devices/current-batch",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/devices/:id/command/:command",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/devices/:id/telemetry",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/devices/:id/metrics",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/devices/:id/telemetry/summary",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/devices/:id/commands",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/device-types",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/device-types/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/device-types",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/device-types",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/device-types/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/device-types/cloud/import",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/devices/generate-mdl",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/devices/drafts",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/devices/drafts/:device_id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/devices/drafts/:device_id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/devices/drafts/:device_id/approve",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/devices/drafts/:device_id/reject",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/devices/drafts/:device_id/analyze",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/devices/drafts/:device_id/enhance",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/devices/drafts/:device_id/suggest-types",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/devices/drafts/cleanup",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/devices/drafts/type-signatures",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/devices/drafts/config",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/devices/drafts/config",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/devices/drafts/upload",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/rules",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/rules",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/rules/export",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/rules/import",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/rules/resources",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/rules/validate",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/rules/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/rules/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/rules/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/rules/:id/enable",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/rules/:id/test",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/rules/:id/trigger",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/rules/:id/history",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/messages",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/messages",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/messages/stats",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/messages/cleanup",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/messages/acknowledge",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/messages/resolve",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/messages/delete",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/messages/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/messages/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/messages/:id/acknowledge",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/messages/:id/resolve",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/messages/:id/archive",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/messages/channels",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/messages/channels/:name",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/messages/channels/:name",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/messages/channels/:name/test",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/data-push",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/data-push",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/data-push/stats",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/data-push/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/data-push/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/data-push/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/data-push/:id/test",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/data-push/:id/start",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/data-push/:id/stop",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/data-push/:id/logs",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/messages/channels/:name/recipients",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/messages/channels/:name/recipients",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/messages/channels/:name/recipients/:email",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/messages/channels/:name/filter",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/messages/channels/:name/filter",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/messages/channels/:name/enabled",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/im-bridges",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/im-bridges/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/im-bridges/:id/invites",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/im-bridges/:id/invites/:token",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/im-bridges/:id/allowlist/:chat_id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/im-bridges/:id/sessions/:chat_id/reset",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/llm/generate",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/settings/timezone",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/settings/timezone",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/settings/timezones",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/settings/retention",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/settings/retention",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/settings/backup-config",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/settings/backup-config",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/settings/retention/cleanup",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/settings/agent",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/settings/device",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/automations",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/automations",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/automations/export",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/automations/import",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/automations/analyze-intent",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/automations/templates",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/automations/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/automations/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/automations/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/automations/:id/enable",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/automations/:id/executions",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/automations/transforms/process",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/automations/transforms/:id/test",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/automations/transforms/test-code",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/automations/transforms",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/automations/transforms/metrics",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/automations/transforms/data-sources",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/automations/transforms/:id/data-sources",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/automations/transforms/data-sources/:data_source_id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/agents",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/agents",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/agents/tools",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/agents/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/agents/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/agents/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/agents/:id/execute",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/agents/:id/invoke",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/agents/:id/status",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/agents/:id/executions",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/agents/:id/executions/:execution_id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/agents/:id/executions/details",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/agents/:id/memory",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/agents/:id/memory",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/agents/:id/stats",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/agents/:id/available-resources",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/agents/validate-cron",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/agents/validate-llm",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/agents/:id/messages",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/agents/:id/messages",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/agents/:id/messages",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/agents/:id/messages/:message_id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/memory",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/memory/export",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/memory/stats",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/memory/config",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/memory/compress",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/memory/category/:category",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/memory/:source_type/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/memory/file/:target",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/memory/custom",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/memory/custom/:name",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/mqtt/status",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/mqtt/subscriptions",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/mqtt/subscribe",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/mqtt/unsubscribe",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/mqtt/subscribe/:device_id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/mqtt/unsubscribe/:device_id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/brokers",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/brokers",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/brokers/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/brokers/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/brokers/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/brokers/:id/test",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/mqtt/broker-config",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/mqtt/broker-config",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/mqtt/broker-config/credentials",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/mqtt/broker-config/credentials/delete",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/mqtt/broker-config/tls",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/mqtt/broker-config/tls/generate",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/mqtt/broker-config/tls/ca-cert",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/stats/devices",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/stats/rules",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/config/export",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/config/import",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/config/validate",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/dashboards",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/dashboards",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/dashboards/reorder",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/dashboards/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/dashboards/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/dashboards/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/dashboards/:id/components",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/dashboards/:id/components",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/dashboards/:id/default",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/dashboards/:id/duplicate",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/dashboards/templates",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/dashboards/templates/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/dashboards/:id/share",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/dashboards/:id/share",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/dashboards/:id/share/:token",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/auth/keys",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/auth/keys",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/auth/keys/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/extensions",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/extensions/:id/uninstall",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/extensions/:id/start",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/extensions/:id/stop",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/extensions/:id/config",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/extensions/:id/config",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/extensions/market/install",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/extensions/sync",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/extensions/sync-status",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/llm-backends",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/llm-backends/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/llm-backends/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/llm-backends/:id/activate",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/llm-backends/:id/test",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/builtin-llm/status",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/builtin-llm/models",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/builtin-llm/download",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/builtin-llm/download/cancel",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/builtin-llm/import-local",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/builtin-llm/model",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/builtin-llm/restart",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/builtin-llm/activate",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/instances",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/instances",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/instances/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/instances/:id/test",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/frontend-components/from-path",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/frontend-components/market/install",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/frontend-components/updates",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/frontend-components",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/frontend-components/:id",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "GET",
        path: "/api/users",
        auth: "jwt-only",
    },
    RouteDoc {
        method: "POST",
        path: "/api/users",
        auth: "jwt-only",
    },
    RouteDoc {
        method: "DELETE",
        path: "/api/users/:username",
        auth: "jwt-only",
    },
    RouteDoc {
        method: "GET",
        path: "/api/settings/registration",
        auth: "jwt-only",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/settings/registration",
        auth: "jwt-only",
    },
    RouteDoc {
        method: "POST",
        path: "/api/settings/backup",
        auth: "jwt-only",
    },
    RouteDoc {
        method: "GET",
        path: "/api/settings/backups",
        auth: "jwt-only",
    },
    RouteDoc {
        method: "GET",
        path: "/api/settings/market",
        auth: "jwt-only",
    },
    RouteDoc {
        method: "PUT",
        path: "/api/settings/market",
        auth: "jwt-only",
    },
    RouteDoc {
        method: "GET",
        path: "/api/system/upgrade/check",
        auth: "jwt-only",
    },
    RouteDoc {
        method: "POST",
        path: "/api/system/upgrade",
        auth: "jwt-only",
    },
    RouteDoc {
        method: "GET",
        path: "/api/system/upgrade/status",
        auth: "jwt-only",
    },
    RouteDoc {
        method: "POST",
        path: "/api/extensions/upload/file",
        auth: "jwt-or-api-key",
    },
    RouteDoc {
        method: "POST",
        path: "/api/frontend-components",
        auth: "jwt-or-api-key",
    },
];

impl RouteDoc {
    fn line(&self) -> String {
        format!(
            "<tr><td><code>{}</code></td><td><code>{}</code></td><td>{}</td></tr>",
            self.method, self.path, self.auth
        )
    }
}

fn grouped_html() -> String {
    let mut out = String::from(
        "<!doctype html><html><head><meta charset='utf-8'><title>NeoMind API</title>\
         <style>body{font-family:system-ui;margin:2rem}\
         table{border-collapse:collapse;margin:1rem 0}\
         td,th{border:1px solid #ccc;padding:4px 10px;text-align:left}\
         code{background:#f4f4f4;padding:1px 4px;border-radius:3px}</style></head>\
         <body><h1>NeoMind API</h1>\
         <p>Route index — see <code>/api/docs/routes.json</code> for the machine-readable form. \
         Responses use the <code>{success, data|error:{code,message}}</code> envelope.</p>\
         <p><strong>Scope:</strong> this index is authoritative for METHOD, PATH and the AUTH \
         CLASS only (a CI test fails when it drifts from the router). It does <em>not</em> \
         document request/response schemas, parameter types, ranges or per-endpoint error \
         codes — for those, read the handler source (each route's docs live in \
         <code>crates/neomind-api/src/handlers/</code>). A full OpenAPI document is not yet \
         generated.</p>",
    );
    for group in ["public", "jwt-or-api-key", "jwt-only", "webhook", "ws"] {
        let rows: Vec<String> = ROUTES
            .iter()
            .filter(|r| r.auth == group)
            .map(|r| r.line())
            .collect();
        if rows.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "<h2>{}</h2><table><tr><th>Method</th><th>Path</th><th>Auth</th></tr>{}</table>",
            group,
            rows.join("")
        ));
    }
    out.push_str("</body></html>");
    out
}

/// GET /api/docs — human-readable index.
pub async fn docs_handler() -> Html<String> {
    Html(grouped_html())
}

/// GET /api/docs/routes.json — machine-readable index.
pub async fn routes_json_handler() -> Json<&'static [RouteDoc]> {
    Json(ROUTES)
}

/// Fallback inside /api/docs/* so unknown sub-paths 404 cleanly.
pub async fn docs_404(AxumPath(rest): AxumPath<String>) -> Response {
    (
        StatusCode::NOT_FOUND,
        format!("Unknown docs path: /api/docs/{rest}"),
    )
        .into_response()
}
