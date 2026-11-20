//! Externally-reachable API origin for outbound consumers (device HTTP pulls,
//! webhooks without a request context).
//!
//! Priority (MQTT downlink has no browser Host header):
//! 1. `NEOMIND_SERVER_URL` — explicit operator origin (prod)
//! 2. `NEOMIND_PUBLIC_HOST` + **actual listen port** — public hostname / IP
//! 3. Primary outbound LAN IPv4 (UDP-connect route trick; no packet sent)
//! 4. `http://127.0.0.1:<port>` — same-machine last resort (**not** for devices)
//!
//! Listen port resolution (never invent a wrong default when the server already
//! bound elsewhere): recorded bind port → `NEOMIND_PORT` → [`DEFAULT_HTTP_PORT`].

use std::sync::atomic::{AtomicU16, Ordering};

use crate::host::primary_lan_ipv4;

/// Process-wide HTTP listen port recorded at server bind (0 = not yet bound).
static HTTP_LISTEN_PORT: AtomicU16 = AtomicU16::new(0);

/// Record the socket port the HTTP server actually bound (call from `serve`).
pub fn set_http_listen_port(port: u16) {
    HTTP_LISTEN_PORT.store(port, Ordering::Relaxed);
}

/// Last recorded HTTP listen port (`0` if the server has not bound yet).
pub fn http_listen_port() -> u16 {
    HTTP_LISTEN_PORT.load(Ordering::Relaxed)
}

/// Where [`resolve_advertise_base_url`] got its origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdvertiseUrlSource {
    /// `NEOMIND_SERVER_URL` env.
    Env,
    /// `NEOMIND_PUBLIC_HOST` (+ listen port).
    PublicHost,
    /// Auto-detected primary LAN IPv4.
    LanIp,
    /// Loopback last resort for same-machine consumers only.
    Localhost,
}

impl AdvertiseUrlSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Env => "env",
            Self::PublicHost => "public_host",
            Self::LanIp => "lan_ip",
            Self::Localhost => "localhost",
        }
    }

    /// True when a remote device (EK410, etc.) cannot fetch this origin.
    pub fn is_loopback(self) -> bool {
        matches!(self, Self::Localhost)
    }
}

/// Whether localhost may be returned as an advertise origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdvertiseAudience {
    /// Same-machine / webhook display — localhost last resort OK.
    LocalOk,
    /// Device AbsoluteHttp downlink — never advertise 127.0.0.1/localhost.
    DeviceReachable,
}

/// Default HTTP listen port when neither bind nor `NEOMIND_PORT` is known.
pub const DEFAULT_HTTP_PORT: u16 = 9375;

/// Listen port for auto-built advertise URLs.
///
/// Priority: actual bound port (see [`set_http_listen_port`]) → `NEOMIND_PORT`
/// → [`DEFAULT_HTTP_PORT`]. Matches the dashboards proxy pattern of preferring
/// the live bind over a hardcoded 9375.
pub fn advertise_http_port() -> u16 {
    let bound = http_listen_port();
    if bound != 0 {
        return bound;
    }
    std::env::var("NEOMIND_PORT")
        .ok()
        .and_then(|p| p.trim().parse::<u16>().ok())
        .filter(|p| *p != 0)
        .unwrap_or(DEFAULT_HTTP_PORT)
}

fn trim_base(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

fn is_loopback_host(host: &str) -> bool {
    let host = host.trim().trim_matches(|c| c == '[' || c == ']');
    if host.eq_ignore_ascii_case("localhost") || host.eq_ignore_ascii_case("localhost.localdomain")
    {
        return true;
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return ip.is_loopback();
    }
    false
}

/// True when `s` begins with `http://` or `https://` (ASCII case-insensitive).
fn has_http_scheme(s: &str) -> bool {
    let b = s.as_bytes();
    (b.len() >= 8 && b[..8].eq_ignore_ascii_case(b"https://"))
        || (b.len() >= 7 && b[..7].eq_ignore_ascii_case(b"http://"))
}

/// Host from an authority / bare `host` / `host:port` / `[ipv6]:port` string.
fn host_from_authority(authorityish: &str) -> Option<String> {
    let authority = authorityish.split(['/', '?', '#']).next()?.trim();
    if authority.is_empty() {
        return None;
    }
    let hostport = authority.rsplit('@').next().unwrap_or(authority);
    let host = if hostport.starts_with('[') {
        let end = hostport.find(']')?;
        hostport[1..end].to_string()
    } else if let Some((h, port)) = hostport.rsplit_once(':') {
        if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) {
            h.to_string()
        } else {
            hostport.to_string()
        }
    } else {
        hostport.to_string()
    };
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

/// True when a bare authority already includes an explicit numeric port
/// (`edge.lan:9444`, `[::1]:9444`).
fn authority_has_explicit_port(authorityish: &str) -> bool {
    let authority = authorityish
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .trim();
    let hostport = authority.rsplit('@').next().unwrap_or(authority);
    if hostport.starts_with('[') {
        let Some(end) = hostport.find(']') else {
            return false;
        };
        let after = &hostport[end + 1..];
        return after
            .strip_prefix(':')
            .is_some_and(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    }
    hostport
        .rsplit_once(':')
        .is_some_and(|(_, p)| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// Build an absolute origin from `NEOMIND_PUBLIC_HOST` + listen port.
///
/// - Already has `http(s)://` (any case) → trim only (do **not** append listen port).
/// - Bare `host:port` → `http://{host:port}` (do **not** append listen port again).
/// - Bare `host` → `http://{host}:{listen_port}`.
/// - Unparseable host → [`None`] (caller skips the candidate).
pub fn public_host_origin(host: &str, listen_port: u16) -> Option<String> {
    let host = host.trim();
    if host.is_empty() {
        return None;
    }
    let origin = if has_http_scheme(host) {
        trim_base(host)
    } else if authority_has_explicit_port(host) {
        format!("http://{host}")
    } else {
        format!("http://{host}:{listen_port}")
    };
    // Refuse candidates we cannot extract a host from (device path skips these).
    if origin_host(&origin).is_none() {
        return None;
    }
    Some(origin)
}

/// Extract the host from an absolute origin (`http://host:port`, bare host, …).
///
/// One entry for both shapes:
/// - Absolute `http(s)://…` (scheme case-insensitive) → [`url::Url`] host
/// - Bare `host` / `host:port` (e.g. `localhost:9444`) → authority parse  
///   (**not** `Url::parse`, which treats `localhost` as a scheme and yields no host)
pub fn origin_host(origin: &str) -> Option<String> {
    let s = origin.trim();
    if s.is_empty() {
        return None;
    }
    if has_http_scheme(s) {
        return url::Url::parse(s)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string));
    }
    // Bare host[:port] — never Url::parse ("localhost:9444" → scheme=localhost).
    host_from_authority(s)
}

/// True when the origin's host is loopback (`localhost`, `127.0.0.0/8`, `::1`).
///
/// Used by device AbsoluteHttp so `NEOMIND_SERVER_URL=http://127.0.0.1:…` and
/// `NEOMIND_PUBLIC_HOST=localhost` are rejected by **host**, not by source enum.
pub fn origin_host_is_loopback(origin: &str) -> bool {
    origin_host(origin).as_deref().is_some_and(is_loopback_host)
}

/// Resolve an absolute API origin (localhost last resort allowed).
pub fn resolve_advertise_base_url() -> Option<(String, AdvertiseUrlSource)> {
    resolve_advertise_base_url_for(AdvertiseAudience::LocalOk)
}

/// Resolve an origin a **remote device** can HTTP-fetch.
///
/// Same chain as [`resolve_advertise_base_url`], but never returns a loopback
/// **host** — including when `NEOMIND_SERVER_URL` / `NEOMIND_PUBLIC_HOST` point
/// at `127.0.0.1` / `localhost` (EK410 cannot pull from the host's loopback).
pub fn resolve_device_advertise_base_url() -> Option<(String, AdvertiseUrlSource)> {
    resolve_advertise_base_url_for(AdvertiseAudience::DeviceReachable)
}

fn accept_for_audience(
    audience: AdvertiseAudience,
    url: String,
    src: AdvertiseUrlSource,
) -> Option<(String, AdvertiseUrlSource)> {
    // Parse failure → skip candidate (do not advertise a broken origin).
    let Some(host) = origin_host(&url) else {
        return None;
    };
    if audience == AdvertiseAudience::DeviceReachable && is_loopback_host(&host) {
        return None;
    }
    Some((url, src))
}

/// Resolve an absolute API origin for the given audience.
pub fn resolve_advertise_base_url_for(
    audience: AdvertiseAudience,
) -> Option<(String, AdvertiseUrlSource)> {
    // 1. Explicit env — for DeviceReachable, skip loopback hosts and fall through
    //    (NEOMIND_SERVER_URL=http://127.0.0.1:… must not win for EK410).
    if let Ok(env_url) = std::env::var("NEOMIND_SERVER_URL") {
        let trimmed = trim_base(&env_url);
        if !trimmed.is_empty() {
            if let Some(accepted) = accept_for_audience(audience, trimmed, AdvertiseUrlSource::Env)
            {
                return Some(accepted);
            }
            // DeviceReachable + loopback → try PUBLIC_HOST / LAN next.
        }
    }

    let port = advertise_http_port();

    // 2. Public hostname / IP override (scheme check is case-insensitive;
    //    listen port appended only when the input has no explicit port).
    if let Ok(host) = std::env::var("NEOMIND_PUBLIC_HOST") {
        let host = host.trim();
        if !host.is_empty() {
            if let Some(candidate) = public_host_origin(host, port) {
                if let Some(accepted) =
                    accept_for_audience(audience, candidate, AdvertiseUrlSource::PublicHost)
                {
                    return Some(accepted);
                }
            }
            // Unparseable / loopback PUBLIC_HOST — fall through to LAN.
        }
    }

    // 3. Primary outbound LAN IPv4 (avoid wrong NIC by using the default route).
    if let Some(ip) = primary_lan_ipv4() {
        let host = ip.trim();
        if !host.is_empty() && !is_loopback_host(host) {
            let url = format!("http://{host}:{port}");
            return accept_for_audience(audience, url, AdvertiseUrlSource::LanIp);
        }
    }

    // 4. Loopback — only for same-machine consumers.
    match audience {
        AdvertiseAudience::LocalOk => Some((
            format!("http://127.0.0.1:{port}"),
            AdvertiseUrlSource::Localhost,
        )),
        AdvertiseAudience::DeviceReachable => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialise env + listen-port mutation across advertise tests in this process.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let prev = std::env::var(key).ok();
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
            Self { key, prev }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    struct ListenPortGuard(u16);

    impl ListenPortGuard {
        fn set(port: u16) -> Self {
            let prev = http_listen_port();
            set_http_listen_port(port);
            Self(prev)
        }
    }

    impl Drop for ListenPortGuard {
        fn drop(&mut self) {
            set_http_listen_port(self.0);
        }
    }

    #[test]
    fn env_server_url_wins_over_public_host_and_lan() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _a = EnvGuard::set("NEOMIND_SERVER_URL", Some("https://edge.example.com/"));
        let _b = EnvGuard::set("NEOMIND_PUBLIC_HOST", Some("ignored.example"));
        let (url, src) = resolve_advertise_base_url().expect("env must resolve");
        assert_eq!(url, "https://edge.example.com");
        assert_eq!(src, AdvertiseUrlSource::Env);
    }

    #[test]
    fn public_host_uses_recorded_listen_port_not_hardcoded_9375() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _a = EnvGuard::set("NEOMIND_SERVER_URL", None);
        let _b = EnvGuard::set("NEOMIND_PUBLIC_HOST", Some("camthink.local"));
        let _c = EnvGuard::set("NEOMIND_PORT", None);
        let _port = ListenPortGuard::set(9444);
        assert_eq!(advertise_http_port(), 9444);
        let (url, src) = resolve_advertise_base_url().expect("public host must resolve");
        assert_eq!(url, "http://camthink.local:9444");
        assert_eq!(src, AdvertiseUrlSource::PublicHost);
    }

    #[test]
    fn neomind_port_used_when_listen_port_not_yet_bound() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _port = ListenPortGuard::set(0);
        let _a = EnvGuard::set("NEOMIND_PORT", Some("9555"));
        assert_eq!(advertise_http_port(), 9555);
    }

    #[test]
    fn without_overrides_local_ok_returns_lan_or_localhost() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _a = EnvGuard::set("NEOMIND_SERVER_URL", None);
        let _b = EnvGuard::set("NEOMIND_PUBLIC_HOST", None);
        let _c = EnvGuard::set("NEOMIND_PORT", Some("9375"));
        let _port = ListenPortGuard::set(0);
        let (url, src) = resolve_advertise_base_url().expect("fallback must resolve");
        assert!(
            url.starts_with("http://") && !url.ends_with('/'),
            "expected absolute origin, got {url} ({src:?})"
        );
        assert!(
            matches!(
                src,
                AdvertiseUrlSource::LanIp | AdvertiseUrlSource::Localhost
            ),
            "unexpected source {src:?}"
        );
    }

    #[test]
    fn device_audience_never_returns_localhost() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _a = EnvGuard::set("NEOMIND_SERVER_URL", None);
        let _b = EnvGuard::set("NEOMIND_PUBLIC_HOST", None);
        let _port = ListenPortGuard::set(9375);
        match resolve_device_advertise_base_url() {
            Some((url, src)) => {
                assert_eq!(src, AdvertiseUrlSource::LanIp, "device path: {url}");
                assert!(!origin_host_is_loopback(&url), "{url}");
            }
            None => {
                // Air-gapped / no route — correct to refuse loopback for devices.
            }
        }
    }

    #[test]
    fn origin_host_detects_loopback_forms() {
        assert!(origin_host_is_loopback("http://127.0.0.1:9375"));
        assert!(origin_host_is_loopback("http://127.0.0.2:80/"));
        assert!(origin_host_is_loopback("https://localhost"));
        assert!(origin_host_is_loopback("http://[::1]:9375"));
        assert!(!origin_host_is_loopback("https://edge.example.com"));
        assert!(!origin_host_is_loopback("http://192.168.1.10:9444"));
        assert_eq!(
            origin_host("http://user:pass@192.168.1.5:9375/api"),
            Some("192.168.1.5".into())
        );
    }

    #[test]
    fn origin_host_parses_uppercase_scheme_not_as_host() {
        // Regression: lowercase-only strip left host = "HTTP" for HTTP://…
        assert_eq!(
            origin_host("HTTP://127.0.0.1:9375"),
            Some("127.0.0.1".into())
        );
        assert!(origin_host_is_loopback("HTTP://127.0.0.1:9375"));
        assert!(origin_host_is_loopback("HtTpS://LocalHost:9444"));
        assert_eq!(
            origin_host("HTTPS://edge.example.com/path"),
            Some("edge.example.com".into())
        );
    }

    #[test]
    fn origin_host_parses_bare_localhost_port() {
        // Url::parse("localhost:9444") treats localhost as a scheme → no host.
        assert_eq!(origin_host("localhost:9444"), Some("localhost".into()));
        assert!(origin_host_is_loopback("localhost:9444"));
        assert_eq!(
            origin_host("192.168.1.10:9375"),
            Some("192.168.1.10".into())
        );
        assert_eq!(origin_host("edge.lan"), Some("edge.lan".into()));
    }

    #[test]
    fn public_host_uppercase_scheme_not_double_prefixed() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _a = EnvGuard::set("NEOMIND_SERVER_URL", None);
        let _b = EnvGuard::set("NEOMIND_PUBLIC_HOST", Some("HTTPS://edge.example.com"));
        let _port = ListenPortGuard::set(9444);
        assert_eq!(
            public_host_origin("HTTPS://edge.example.com", 9444).as_deref(),
            Some("HTTPS://edge.example.com")
        );
        let (url, src) = resolve_advertise_base_url().expect("public host");
        assert_eq!(src, AdvertiseUrlSource::PublicHost);
        assert_eq!(url, "HTTPS://edge.example.com");
        assert!(
            !url.contains("http://HTTPS://"),
            "must not double-prefix scheme: {url}"
        );
        assert_eq!(origin_host(&url), Some("edge.example.com".into()));
    }

    #[test]
    fn public_host_origin_port_table() {
        // Absolute scheme URL — keep as-is (no listen-port append).
        assert_eq!(
            public_host_origin("HTTPS://edge.example.com", 9444).as_deref(),
            Some("HTTPS://edge.example.com")
        );
        // Bare host:port — http:// prefix only; do NOT append listen port again.
        assert_eq!(
            public_host_origin("edge.lan:9444", 9375).as_deref(),
            Some("http://edge.lan:9444")
        );
        assert!(
            !public_host_origin("edge.lan:9444", 9375)
                .unwrap()
                .contains(":9444:9375"),
            "must not double-append listen port"
        );
        // Bare host — append listen port.
        assert_eq!(
            public_host_origin("edge.lan", 9375).as_deref(),
            Some("http://edge.lan:9375")
        );
        // localhost:9444 builds a loopback origin (LocalOk may use; Device skips).
        assert_eq!(
            public_host_origin("localhost:9444", 9375).as_deref(),
            Some("http://localhost:9444")
        );
        assert!(!public_host_origin("localhost:9444", 9375)
            .unwrap()
            .contains(":9444:9375"));
    }

    #[test]
    fn device_skips_localhost_with_explicit_port_not_double_port() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _a = EnvGuard::set("NEOMIND_SERVER_URL", None);
        let _b = EnvGuard::set("NEOMIND_PUBLIC_HOST", Some("localhost:9444"));
        let _port = ListenPortGuard::set(9375);
        match resolve_device_advertise_base_url() {
            Some((url, src)) => {
                assert_eq!(src, AdvertiseUrlSource::LanIp);
                assert!(!url.contains("localhost"));
                assert!(!url.contains(":9444:9375"));
            }
            None => {
                // No LAN — refusing localhost:9444 is correct (not a double-port URL).
            }
        }
        // LocalOk still accepts http://localhost:9444 (single port).
        let (local_url, local_src) =
            resolve_advertise_base_url_for(AdvertiseAudience::LocalOk).expect("local ok");
        assert_eq!(local_src, AdvertiseUrlSource::PublicHost);
        assert_eq!(local_url, "http://localhost:9444");
    }

    #[test]
    fn device_skips_uppercase_scheme_loopback_server_url() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _a = EnvGuard::set("NEOMIND_SERVER_URL", Some("HTTP://127.0.0.1:9375"));
        let _b = EnvGuard::set("NEOMIND_PUBLIC_HOST", Some("edge.lan"));
        let _port = ListenPortGuard::set(9444);
        let (url, src) = resolve_device_advertise_base_url().expect("skip uppercase loopback");
        assert_eq!(src, AdvertiseUrlSource::PublicHost);
        assert_eq!(url, "http://edge.lan:9444");
    }

    #[test]
    fn device_skips_loopback_server_url_and_falls_through() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _a = EnvGuard::set("NEOMIND_SERVER_URL", Some("http://127.0.0.1:9375"));
        let _b = EnvGuard::set("NEOMIND_PUBLIC_HOST", Some("edge.lan"));
        let _port = ListenPortGuard::set(9444);
        let (url, src) = resolve_device_advertise_base_url().expect("public host after skip");
        assert_eq!(src, AdvertiseUrlSource::PublicHost);
        assert_eq!(url, "http://edge.lan:9444");
        assert!(!origin_host_is_loopback(&url));
    }

    #[test]
    fn device_skips_loopback_public_host() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _a = EnvGuard::set("NEOMIND_SERVER_URL", None);
        let _b = EnvGuard::set("NEOMIND_PUBLIC_HOST", Some("localhost"));
        let _port = ListenPortGuard::set(9375);
        match resolve_device_advertise_base_url() {
            Some((url, src)) => {
                assert_eq!(src, AdvertiseUrlSource::LanIp);
                assert!(!origin_host_is_loopback(&url), "{url}");
            }
            None => {
                // No LAN — refusing localhost PUBLIC_HOST is correct.
            }
        }
        // LocalOk still accepts explicit localhost PUBLIC_HOST.
        let (local_url, local_src) =
            resolve_advertise_base_url_for(AdvertiseAudience::LocalOk).expect("local ok");
        assert_eq!(local_src, AdvertiseUrlSource::PublicHost);
        assert!(origin_host_is_loopback(&local_url));
    }
}
