//! Externally-reachable API origin for outbound consumers (device HTTP pulls,
//! webhooks without a request context).
//!
//! Priority (MQTT downlink has no browser Host header):
//! 1. `NEOMIND_SERVER_URL` — explicit operator origin (prod)
//! 2. `NEOMIND_PUBLIC_HOST` + listen port — public hostname / IP override
//! 3. Primary outbound LAN IPv4 (UDP-connect route trick; no packet sent)
//! 4. `http://127.0.0.1:<port>` — same-machine last resort
//!
//! Never returns a bare relative path. Callers that need AbsoluteHttp should
//! hard-fail only when this returns [`None`] (currently always [`Some`] once
//! a port can be resolved).

use crate::host::primary_lan_ipv4;

/// Where [`resolve_advertise_base_url`] got its origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdvertiseUrlSource {
    /// `NEOMIND_SERVER_URL` env.
    Env,
    /// `NEOMIND_PUBLIC_HOST` (+ `NEOMIND_PORT` / default 9375).
    PublicHost,
    /// Auto-detected primary LAN IPv4.
    LanIp,
    /// Loopback last resort for same-machine consumers.
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
}

/// Default HTTP listen port when `NEOMIND_PORT` is unset / invalid.
pub const DEFAULT_HTTP_PORT: u16 = 9375;

/// Listen port for auto-built advertise URLs (`NEOMIND_PORT` or [`DEFAULT_HTTP_PORT`]).
pub fn advertise_http_port() -> u16 {
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
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

/// Resolve an absolute API origin devices / remote pullers can fetch.
///
/// Returns `(base_url_without_trailing_slash, source)`.
pub fn resolve_advertise_base_url() -> Option<(String, AdvertiseUrlSource)> {
    // 1. Explicit env wins (same knob as webhooks).
    if let Ok(env_url) = std::env::var("NEOMIND_SERVER_URL") {
        let trimmed = trim_base(&env_url);
        if !trimmed.is_empty() {
            return Some((trimmed, AdvertiseUrlSource::Env));
        }
    }

    let port = advertise_http_port();

    // 2. Public hostname / IP override (MQTT / device-facing display).
    if let Ok(host) = std::env::var("NEOMIND_PUBLIC_HOST") {
        let host = host.trim();
        if !host.is_empty() {
            if host.starts_with("http://") || host.starts_with("https://") {
                return Some((trim_base(host), AdvertiseUrlSource::PublicHost));
            }
            return Some((
                format!("http://{host}:{port}"),
                AdvertiseUrlSource::PublicHost,
            ));
        }
    }

    // 3. Primary outbound LAN IPv4 (avoid wrong NIC by using the default route).
    if let Some(ip) = primary_lan_ipv4() {
        let host = ip.trim();
        if !host.is_empty() && !is_loopback_host(host) {
            return Some((format!("http://{host}:{port}"), AdvertiseUrlSource::LanIp));
        }
    }

    // 4. Same-machine last resort — absolute, never a relative `/api/images/`.
    Some((
        format!("http://127.0.0.1:{port}"),
        AdvertiseUrlSource::Localhost,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialise env mutation across advertise tests in this process.
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
    fn public_host_builds_http_origin_when_server_url_unset() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _a = EnvGuard::set("NEOMIND_SERVER_URL", None);
        let _b = EnvGuard::set("NEOMIND_PUBLIC_HOST", Some("camthink.local"));
        let _c = EnvGuard::set("NEOMIND_PORT", Some("9444"));
        let (url, src) = resolve_advertise_base_url().expect("public host must resolve");
        assert_eq!(url, "http://camthink.local:9444");
        assert_eq!(src, AdvertiseUrlSource::PublicHost);
    }

    #[test]
    fn without_overrides_returns_absolute_lan_or_localhost() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _a = EnvGuard::set("NEOMIND_SERVER_URL", None);
        let _b = EnvGuard::set("NEOMIND_PUBLIC_HOST", None);
        let _c = EnvGuard::set("NEOMIND_PORT", Some("9375"));
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
        assert!(!url.contains("/api/images/"));
    }
}
