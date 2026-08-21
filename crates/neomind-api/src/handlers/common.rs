//! Common API handler utilities.
//!
//! This module provides shared utilities for API handlers including
//! unified error handling, response builders, and common patterns.

use axum::response::Json;
use base64::Engine;
use serde::Deserialize;

use crate::models::{common::ApiResponse, error::ErrorResponse, pagination::PaginationMeta};

/// Unified Result type for all API handlers.
///
/// All handlers should return this type for consistent error handling.
/// The success value is automatically wrapped in ApiResponse.
pub type HandlerResult<T> = Result<Json<ApiResponse<T>>, ErrorResponse>;

/// Result type for utility functions that return parsed values (not full responses).
pub type ExtractResult<T> = Result<T, ErrorResponse>;

/// Pagination query parameters.
///
/// Supports two modes:
/// - **Offset-based** (default): use `page` + `page_size` for traditional pagination.
/// - **Cursor-based** (opt-in): provide `cursor` to skip directly to the last-seen key
///   for O(1) deep-page performance. When `cursor` is present, `page`/`page_size` are
///   ignored in favour of `limit`.
#[derive(Debug, Clone, Deserialize)]
pub struct PaginationQuery {
    /// Page number (1-indexed). Ignored when `cursor` is provided.
    #[serde(default = "default_page")]
    pub page: usize,

    /// Items per page. Ignored when `cursor` is provided.
    #[serde(default = "default_page_size")]
    pub page_size: usize,

    /// Opaque cursor for cursor-based pagination (base64-encoded last key).
    /// When provided, takes precedence over `page`/`page_size` for better performance
    /// on deep pagination.
    pub cursor: Option<String>,

    /// Page size for cursor-based pagination (default: 10).
    /// Only used when `cursor` is provided.
    pub limit: Option<u32>,
}

fn default_page() -> usize {
    1
}

fn default_page_size() -> usize {
    20
}

impl PaginationQuery {
    /// Returns `true` if this request uses cursor-based pagination.
    pub fn is_cursor_based(&self) -> bool {
        self.cursor.is_some()
    }

    /// Decode the cursor to get the underlying last key.
    ///
    /// Returns `None` if no cursor was provided or if it is not valid base64 / UTF-8.
    pub fn decode_cursor(&self) -> Option<String> {
        self.cursor.as_ref().and_then(|c| {
            base64::engine::general_purpose::STANDARD
                .decode(c)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
        })
    }

    /// Encode a key as an opaque cursor string suitable for the next page.
    pub fn encode_cursor(key: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(key.as_bytes())
    }

    /// Get the effective limit for cursor-based pagination.
    /// Falls back to `page_size` (capped at 100) when `limit` is not set.
    pub fn cursor_limit(&self) -> usize {
        self.limit.unwrap_or(self.page_size as u32).min(100) as usize
    }

    /// Get the offset for database queries (offset-based mode).
    pub fn offset(&self) -> usize {
        (self.page.saturating_sub(1)) * self.page_size
    }

    /// Get the limit for database queries (offset-based mode).
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
        if let Some(lim) = self.limit {
            if lim == 0 {
                self.limit = Some(10);
            } else if lim > 100 {
                return Err(ErrorResponse::bad_request("limit cannot exceed 100"));
            }
        }
        Ok(self)
    }

    /// Convert to PaginationMeta for API responses (offset-based mode).
    pub fn to_meta(&self, total: u32) -> PaginationMeta {
        let total_pages = (total as usize).div_ceil(self.page_size);
        PaginationMeta {
            page: self.page as u32,
            page_size: self.page_size as u32,
            total_count: total,
            total_pages: total_pages as u32,
            has_next: self.page < total_pages,
            has_prev: self.page > 1,
            next_cursor: None,
        }
    }

    /// Build PaginationMeta for a cursor-based response.
    ///
    /// `total_count` is the total number of items (may be approximate for some stores).
    /// `last_key` is the key of the last item returned; it will be encoded as `next_cursor`
    /// when there are more results.
    pub fn to_cursor_meta(
        &self,
        total_count: u32,
        returned_count: usize,
        last_key: Option<&str>,
    ) -> PaginationMeta {
        let effective_limit = self.cursor_limit();
        let has_more = returned_count >= effective_limit;
        PaginationMeta {
            page: 0, // not applicable in cursor mode
            page_size: effective_limit as u32,
            total_count,
            total_pages: if effective_limit > 0 {
                total_count.div_ceil(effective_limit as u32)
            } else {
                0
            },
            has_next: has_more,
            has_prev: false, // cursor pagination is forward-only
            next_cursor: if has_more {
                last_key.map(Self::encode_cursor)
            } else {
                None
            },
        }
    }
}

/// Extract a path parameter or return a 400 error.
///
/// # Example
/// ```rust,ignore
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
            let parsed = v.parse::<T>().map_err(|_| {
                ErrorResponse::bad_request(format!("Invalid query parameter: {}", v))
            })?;
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
        Some(v) => v.parse::<T>().map_err(|_| {
            ErrorResponse::bad_request(format!("Invalid query parameter '{}': {}", name, v))
        }),
        None => Err(ErrorResponse::bad_request(format!(
            "Missing required query parameter: {}",
            name
        ))),
    }
}

/// Environment variable for overriding the public host (IP or domain).
///
/// Set this when deploying to a public server so that the embedded MQTT broker
/// and device onboarding show the correct public address instead of a private LAN IP.
///
/// Example: `NEOMIND_PUBLIC_HOST=example.com` or `NEOMIND_PUBLIC_HOST=203.0.113.5`
const PUBLIC_HOST_ENV: &str = "NEOMIND_PUBLIC_HOST";

/// Get the server host address for device connectivity.
///
/// Priority:
/// 1. `NEOMIND_PUBLIC_HOST` environment variable (IP or domain — for public deployments)
/// 2. Auto-detected local IP (for LAN / development use)
pub fn get_server_host() -> String {
    // 1. Explicit public host override (supports both IP and domain)
    if let Ok(host) = std::env::var(PUBLIC_HOST_ENV) {
        let host = host.trim().to_string();
        if !host.is_empty() {
            return host;
        }
    }

    // 2. Auto-detect local IP
    detect_local_ip()
}

/// Auto-detect the local LAN IP address.
fn detect_local_ip() -> String {
    use std::net::IpAddr;

    // Try UDP socket trick to find the default-route interface IP
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(local_addr) = socket.local_addr() {
                let ip = local_addr.ip();
                if let IpAddr::V4(ipv4) = ip {
                    let o = ipv4.octets();
                    if is_private_ip(o) {
                        return ip.to_string();
                    }
                    // Public IP from default route — return it directly
                    return ip.to_string();
                }
            }
        }
    }

    // Fallback: scan network interfaces
    if let Ok(interfaces) = get_if_addrs::get_if_addrs() {
        for iface in interfaces {
            if iface.is_loopback() {
                continue;
            }
            if let get_if_addrs::IfAddr::V4(v4) = iface.addr {
                let o = v4.ip.octets();
                if is_private_ip(o) {
                    return v4.ip.to_string();
                }
            }
        }
    }

    std::env::var("HOSTNAME").unwrap_or_else(|_| "localhost".to_string())
}

/// Check if IPv4 octets represent a private/RFC1918 address.
fn is_private_ip(o: [u8; 4]) -> bool {
    (o[0] == 192 && o[1] == 168) || o[0] == 10 || (o[0] == 172 && o[1] >= 16 && o[1] <= 31)
}

/// Create a successful response with data.
pub fn ok<T: serde::Serialize>(data: T) -> HandlerResult<T> {
    Ok(Json(ApiResponse::success(data)))
}

/// Create a successful response with data and metadata (for pagination).
pub fn ok_with_meta<T: serde::Serialize>(data: T, meta: PaginationMeta) -> HandlerResult<T> {
    Ok(Json(ApiResponse::paginated(data, meta)))
}

/// Source attribution for [`resolve_server_url`]. Useful for surfacing to the
/// client whether the returned URL should be trusted as the public address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerUrlSource {
    /// `NEOMIND_SERVER_URL` env var — explicit operator configuration, always trusted.
    Env,
    /// `X-Forwarded-Proto` + `Host` headers — inferred from reverse proxy.
    /// Trusted iff the server is behind a proxy that sets these headers.
    ProxyHeader,
    /// Hardcoded `http://localhost:9375` fallback. Almost certainly wrong for
    /// remote clients; clients should treat as a placeholder.
    Fallback,
}

impl ServerUrlSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ServerUrlSource::Env => "env",
            ServerUrlSource::ProxyHeader => "proxy_header",
            ServerUrlSource::Fallback => "fallback",
        }
    }
}

/// Resolve the server's externally-visible base URL.
///
/// Priority:
/// 1. `NEOMIND_SERVER_URL` env var (explicit operator config — use this in prod)
/// 2. `X-Forwarded-Proto` + `Host` request headers (reverse proxy inference)
/// 3. `http://localhost:9375` hardcoded fallback
///
/// ## When to use what
///
/// - **HTTPS reverse-proxy deployments** (nginx TLS termination): set
///   `NEOMIND_SERVER_URL=https://your.domain` in the env. If you forget, the
///   ProxyHeader branch will still produce a correct URL as long as nginx
///   forwards the standard headers.
/// - **Closed LAN / Tauri desktop**: leave unset; the localhost fallback is
///   correct for those clients.
/// - **Direct internet exposure** (no proxy): MUST set the env var — without a
///   proxy there's no `X-Forwarded-Proto`, and the fallback is wrong.
///
/// ## Security note on the ProxyHeader branch
///
/// Only trust these headers when the server is behind a proxy you control.
/// If the server is directly internet-exposed, an attacker can spoof
/// `X-Forwarded-Proto` to trick the server into generating `https://` URLs
/// pointing at an attacker-controlled host. Set the env var to lock it down.
pub fn resolve_server_url(headers: Option<&axum::http::HeaderMap>) -> (String, ServerUrlSource) {
    // 1. Explicit env var wins everywhere.
    if let Ok(env_url) = std::env::var("NEOMIND_SERVER_URL") {
        let trimmed = env_url.trim_end_matches('/');
        if !trimmed.is_empty() {
            return (trimmed.to_string(), ServerUrlSource::Env);
        }
    }

    // 2. Infer from reverse-proxy headers.
    //
    // IMPORTANT: We must NOT trust the `Host` header on its own. Every HTTP
    // request carries `Host`, and its value is simply whatever the client
    // typed in the URL — so a user browsing `http://localhost:9375` sends
    // `Host: localhost:9375`, which is meaningless for device-facing display.
    //
    // We only trust `Host` when a clear reverse-proxy signal is also present
    // (`X-Forwarded-Proto` or `X-Forwarded-Host`). nginx/caddy/traefik always
    // set at least one of these when configured with `proxy_set_header`, so
    // this is a reliable reverse-proxy discriminator.
    if let Some(hdrs) = headers {
        let forwarded_proto = hdrs
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty());
        let forwarded_host = hdrs
            .get("x-forwarded-host")
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty());
        let has_reverse_proxy_signal = forwarded_proto.is_some() || forwarded_host.is_some();

        if has_reverse_proxy_signal {
            // Prefer X-Forwarded-Host (nginx `proxy_set_header X-Forwarded-Host $host`),
            // fall back to the raw Host header. Either way, this is only trusted
            // BECAUSE the reverse-proxy signal was seen.
            let host = forwarded_host
                .or_else(|| {
                    hdrs.get(axum::http::header::HOST)
                        .and_then(|v| v.to_str().ok())
                        .filter(|s| !s.is_empty())
                })
                .map(|s| s.to_string());
            let scheme = forwarded_proto.unwrap_or("http");
            if let Some(host) = host {
                return (
                    format!("{}://{}", scheme, host),
                    ServerUrlSource::ProxyHeader,
                );
            }
        }
    }

    // 3. Reachability-aware: if the server is bound to loopback only, the LAN
    // IP is NOT reachable — return localhost (works for same-machine users and
    // no device could reach a loopback-bound server anyway). If bound to
    // 0.0.0.0 (production), auto-detect the LAN IP so devices can POST.
    let port = crate::server::http_bind_port();
    let port = if port == 0 { 9375 } else { port };
    if crate::server::http_bind_is_loopback() {
        return (
            format!("http://localhost:{port}"),
            ServerUrlSource::Fallback,
        );
    }
    let host = get_server_host();
    (format!("http://{}:{port}", host), ServerUrlSource::Fallback)
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
            cursor: None,
            limit: None,
        };
        assert_eq!(query.offset(), 10);
        assert_eq!(query.limit(), 10);
    }

    #[test]
    fn test_pagination_validate() {
        let query = PaginationQuery {
            page: 0,
            page_size: 50,
            cursor: None,
            limit: None,
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
            cursor: None,
            limit: None,
        };
        let result = query.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_pagination_to_meta() {
        let query = PaginationQuery {
            page: 1,
            page_size: 10,
            cursor: None,
            limit: None,
        };
        let meta = query.to_meta(25);
        assert_eq!(meta.page, 1);
        assert_eq!(meta.page_size, 10);
        assert_eq!(meta.total_count, 25);
        assert_eq!(meta.total_pages, 3);
        assert!(meta.next_cursor.is_none());
    }

    #[test]
    fn test_ok_helper() {
        let result: HandlerResult<String> = ok("test".to_string());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0.data, Some("test".to_string()));
    }

    #[test]
    fn test_ok_with_meta_helper() {
        let meta = PaginationMeta {
            page: 1,
            page_size: 10,
            total_count: 100,
            total_pages: 10,
            has_next: true,
            has_prev: false,
            next_cursor: None,
        };
        let result: HandlerResult<String> = ok_with_meta("test".to_string(), meta);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0.data, Some("test".to_string()));
    }

    #[test]
    fn test_cursor_encode_decode() {
        let key = "device:sensor-001";
        let encoded = PaginationQuery::encode_cursor(key);
        let query = PaginationQuery {
            page: 1,
            page_size: 10,
            cursor: Some(encoded),
            limit: None,
        };
        assert!(query.is_cursor_based());
        let decoded = query.decode_cursor().unwrap();
        assert_eq!(decoded, key);
    }

    #[test]
    fn test_cursor_decode_invalid_base64() {
        let query = PaginationQuery {
            page: 1,
            page_size: 10,
            cursor: Some("not-valid-base64!!!".to_string()),
            limit: None,
        };
        assert!(query.is_cursor_based());
        assert!(query.decode_cursor().is_none());
    }

    #[test]
    fn test_cursor_decode_invalid_utf8() {
        let query = PaginationQuery {
            page: 1,
            page_size: 10,
            cursor: Some(base64::engine::general_purpose::STANDARD.encode([0xff, 0xfe])),
            limit: None,
        };
        // [0xff, 0xfe] is not valid UTF-8, so decode_cursor should return None
        assert!(query.decode_cursor().is_none());
    }

    #[test]
    fn test_cursor_no_cursor_provided() {
        let query = PaginationQuery {
            page: 1,
            page_size: 10,
            cursor: None,
            limit: None,
        };
        assert!(!query.is_cursor_based());
        assert!(query.decode_cursor().is_none());
    }

    #[test]
    fn test_cursor_limit_falls_back_to_page_size() {
        let query = PaginationQuery {
            page: 1,
            page_size: 25,
            cursor: Some("abc".to_string()),
            limit: None,
        };
        assert_eq!(query.cursor_limit(), 25);
    }

    #[test]
    fn test_cursor_limit_capped_at_100() {
        let query = PaginationQuery {
            page: 1,
            page_size: 10,
            cursor: Some("abc".to_string()),
            limit: Some(200),
        };
        // limit > 100 should be caught by validate(), but cursor_limit still caps
        assert_eq!(query.cursor_limit(), 100);
    }

    #[test]
    fn test_cursor_validate_zero_limit() {
        let query = PaginationQuery {
            page: 1,
            page_size: 10,
            cursor: Some("abc".to_string()),
            limit: Some(0),
        };
        let result = query.validate().unwrap();
        assert_eq!(result.limit, Some(10));
    }

    #[test]
    fn test_cursor_validate_limit_exceeds_max() {
        let query = PaginationQuery {
            page: 1,
            page_size: 10,
            cursor: Some("abc".to_string()),
            limit: Some(200),
        };
        let result = query.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_to_cursor_meta_has_more() {
        let query = PaginationQuery {
            page: 1,
            page_size: 10,
            cursor: Some("abc".to_string()),
            limit: Some(10),
        };
        // returned_count == limit => has_more = true
        let meta = query.to_cursor_meta(50, 10, Some("last-key-123"));
        assert_eq!(meta.page, 0); // not applicable
        assert_eq!(meta.page_size, 10);
        assert!(meta.has_next);
        assert!(!meta.has_prev);
        assert!(meta.next_cursor.is_some());
        // Verify the cursor encodes the last key
        assert_eq!(
            meta.next_cursor.unwrap(),
            PaginationQuery::encode_cursor("last-key-123")
        );
    }

    #[test]
    fn test_to_cursor_meta_no_more() {
        let query = PaginationQuery {
            page: 1,
            page_size: 10,
            cursor: Some("abc".to_string()),
            limit: Some(10),
        };
        // returned_count < limit => no more results
        let meta = query.to_cursor_meta(8, 8, Some("last-key-123"));
        assert!(!meta.has_next);
        assert!(meta.next_cursor.is_none());
    }

    // ----- resolve_server_url priority chain -----
    //
    // env vars are process-global, so all three branches are exercised in a
    // single #[test] to avoid races between parallel tests. Each branch is a
    // labeled block for clarity.

    fn make_headers(host: Option<&str>, xfp: Option<&str>) -> axum::http::HeaderMap {
        let mut h = axum::http::HeaderMap::new();
        if let Some(host) = host {
            h.insert(axum::http::header::HOST, host.parse().unwrap());
        }
        if let Some(xfp) = xfp {
            h.insert("x-forwarded-proto", xfp.parse().unwrap());
        }
        h
    }

    #[test]
    fn test_resolve_server_url_priority_chain() {
        const KEY: &str = "NEOMIND_SERVER_URL";
        let prev = std::env::var(KEY).ok();
        // Start each sub-case from a clean slate.
        std::env::remove_var(KEY);

        // 1. Fallback when nothing else applies: auto-detected LAN IP.
        //    The host varies by machine, so only check the structural shape.
        {
            let (url, src) = resolve_server_url(None);
            assert!(
                url.starts_with("http://") && url.ends_with(":9375"),
                "fallback URL should be http://<host>:9375, got {}",
                url
            );
            assert!(
                !url.contains("localhost"),
                "fallback should not be localhost: {}",
                url
            );
            assert_eq!(src, ServerUrlSource::Fallback);
        }

        // 2. Proxy headers branch (Host + X-Forwarded-Proto).
        {
            let (url, src) =
                resolve_server_url(Some(&make_headers(Some("actual.host:8443"), Some("https"))));
            assert_eq!(url, "https://actual.host:8443");
            assert_eq!(src, ServerUrlSource::ProxyHeader);
        }

        // 3. Env var wins over both proxy headers and fallback.
        {
            std::env::set_var(KEY, "https://from-env.example.com");
            let (url, src) =
                resolve_server_url(Some(&make_headers(Some("proxy.host"), Some("http"))));
            assert_eq!(url, "https://from-env.example.com");
            assert_eq!(src, ServerUrlSource::Env);
        }

        // Restore so we don't leak state into other tests.
        match prev {
            Some(v) => std::env::set_var(KEY, v),
            None => std::env::remove_var(KEY),
        }
    }

    /// Host header alone (no X-Forwarded-Proto/Host) must NOT be trusted —
    /// every HTTP request carries Host, and its value just echoes the URL
    /// the client typed. A user browsing `http://localhost:9375` would
    /// otherwise trick the server into advertising localhost as the canonical URL.
    #[test]
    fn test_resolve_server_url_host_alone_not_trusted() {
        let (url, src) = resolve_server_url(Some(&make_headers(Some("localhost:9375"), None)));
        assert_eq!(src, ServerUrlSource::Fallback);
        assert!(
            url.starts_with("http://") && url.ends_with(":9375"),
            "should fall through to LAN-IP fallback, got {}",
            url
        );
        assert!(
            !url.contains("localhost"),
            "Host-only header must not yield localhost URL: {}",
            url
        );
    }

    /// `X-Forwarded-Host` alone (no X-Forwarded-Proto) is also a valid reverse-proxy signal.
    #[test]
    fn test_resolve_server_url_forwarded_host_alone_trusted() {
        let mut h = axum::http::HeaderMap::new();
        h.insert("x-forwarded-host", "example.com".parse().unwrap());
        let (url, src) = resolve_server_url(Some(&h));
        assert_eq!(src, ServerUrlSource::ProxyHeader);
        assert_eq!(url, "http://example.com");
    }
}
