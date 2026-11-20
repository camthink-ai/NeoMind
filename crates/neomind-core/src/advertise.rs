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
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

/// Resolve an absolute API origin (localhost last resort allowed).
pub fn resolve_advertise_base_url() -> Option<(String, AdvertiseUrlSource)> {
    resolve_advertise_base_url_for(AdvertiseAudience::LocalOk)
}

/// Resolve an origin a **remote device** can HTTP-fetch.
///
/// Same chain as [`resolve_advertise_base_url`], but returns [`None`] instead of
/// advertising `127.0.0.1` / `localhost` (EK410 cannot pull from the host's
/// loopback).
pub fn resolve_device_advertise_base_url() -> Option<(String, AdvertiseUrlSource)> {
    resolve_advertise_base_url_for(AdvertiseAudience::DeviceReachable)
}

/// Resolve an absolute API origin for the given audience.
pub fn resolve_advertise_base_url_for(
    audience: AdvertiseAudience,
) -> Option<(String, AdvertiseUrlSource)> {
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
                assert!(!url.contains("127.0.0.1") && !url.contains("localhost"));
            }
            None => {
                // Air-gapped / no route — correct to refuse loopback for devices.
            }
        }
    }
}
