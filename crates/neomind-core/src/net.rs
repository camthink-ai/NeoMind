//! SSRF guard — shared private/local address detection for any code
//! fetching URLs from untrusted input (device payloads, extension data).
//!
//! Extracted from the agent `web_fetch` tool (which had the only copy);
//! the transform engine's device-controlled `url_to_base64` fetch now uses
//! the same rules instead of none at all.

/// Check if a hostname points to a private/local address.
pub fn is_private_host(host: &str) -> bool {
    let trimmed = host.trim().trim_end_matches('.');
    let lower = trimmed.to_ascii_lowercase();

    // Literal names
    match lower.as_str() {
        "localhost" | "127.0.0.1" | "0.0.0.0" | "::1" | "localhost.localdomain" => return true,
        _ => {}
    }

    // Try parsing as IP for private range checks
    // Strip brackets from IPv6 URLs: [::ffff:127.0.0.1] -> ::ffff:127.0.0.1
    let host_trimmed = trimmed.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = host_trimmed.parse::<std::net::IpAddr>() {
        return is_private_ip(&ip);
    }

    // Hostnames that look like local addresses
    lower.ends_with(".local") || lower.ends_with(".localhost")
}

/// Check if an IP address is private/local/reserved (covers IPv4, IPv6, and
/// IPv4-mapped IPv6).
pub fn is_private_ip(ip: &std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let octets = v4.octets();
            // 10.0.0.0/8
            if octets[0] == 10 {
                return true;
            }
            // 172.16.0.0/12
            if octets[0] == 172 && (16..=31).contains(&octets[1]) {
                return true;
            }
            // 192.168.0.0/16
            if octets[0] == 192 && octets[1] == 168 {
                return true;
            }
            // 127.0.0.0/8 (loopback)
            if octets[0] == 127 {
                return true;
            }
            // 169.254.0.0/16 (link-local / cloud metadata)
            if octets[0] == 169 && octets[1] == 254 {
                return true;
            }
            // 0.0.0.0/8 (current network / unspecified)
            if octets[0] == 0 {
                return true;
            }
            // 100.64.0.0/10 (Carrier-grade NAT, RFC 6598)
            if octets[0] == 100 && (64..=127).contains(&octets[1]) {
                return true;
            }
            // 224.0.0.0/4 (multicast), 240.0.0.0/4 (reserved), 255.255.255.255 (broadcast)
            if v4.is_broadcast() || v4.is_multicast() || v4.is_unspecified() || octets[0] >= 240 {
                return true;
            }
        }
        std::net::IpAddr::V6(v6) => {
            // Standard IPv6 checks
            if v6.is_loopback() || v6.is_multicast() || v6.is_unspecified() {
                return true;
            }
            // IPv6 unique local (fc00::/7 — includes fd00::/8)
            let segments = v6.segments();
            if (segments[0] & 0xfe00) == 0xfc00 {
                return true;
            }
            // IPv6 link-local (fe80::/10)
            if (segments[0] & 0xffc0) == 0xfe80 {
                return true;
            }
            // IPv4-mapped (::ffff:x.x.x.x) and IPv4-compatible (::x.x.x.x)
            // to_ipv4() handles both forms
            if let Some(v4) = v6.to_ipv4() {
                return is_private_ip(&std::net::IpAddr::V4(v4));
            }
        }
    }
    false
}

/// Parse a URL, validate its scheme (`http`/`https`) and host, resolve DNS via
/// `tokio::net::lookup_host`, and reject if ANY resolved IP address is private,
/// loopback, link-local, unspecified, multicast, reserved, or CGNAT
/// (`100.64.0.0/10`).
///
/// Returns the validated list of `SocketAddr`s so callers can pin them (e.g.
/// via `reqwest::ClientBuilder::resolve_to_addrs`) to prevent DNS rebinding.
pub async fn resolve_and_validate_public_url(
    url: &str,
) -> Result<Vec<std::net::SocketAddr>, String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("Invalid URL: {}", e))?;

    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(format!(
                "Only http:// and https:// URLs are allowed (got '{}')",
                other
            ));
        }
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| "URL has no host".to_string())?;

    if is_private_host(host) {
        return Err(format!(
            "Access to '{}' is not allowed (private/local network address)",
            host
        ));
    }

    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| "URL has no port".to_string())?;

    let host_unbracketed = host.trim_start_matches('[').trim_end_matches(']');

    // Fast path for literal IPs: no DNS lookup required.
    if let Ok(ip) = host_unbracketed.parse::<std::net::IpAddr>() {
        if is_private_ip(&ip) {
            return Err(format!(
                "Access to '{}' is not allowed (private/local IP address)",
                ip
            ));
        }
        return Ok(vec![std::net::SocketAddr::new(ip, port)]);
    }

    let addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host((host_unbracketed, port))
        .await
        .map_err(|e| format!("Failed to resolve host '{}': {}", host, e))?
        .collect();

    if addrs.is_empty() {
        return Err(format!("Host '{}' did not resolve to any address", host));
    }

    for addr in &addrs {
        if is_private_ip(&addr.ip()) {
            return Err(format!(
                "Access to '{}' is not allowed: resolved to private/local IP {}",
                host,
                addr.ip()
            ));
        }
    }

    Ok(addrs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_hosts_detected() {
        for host in [
            "localhost",
            "LOCALHOST",
            "localhost.",
            "sub.localhost",
            "127.0.0.1",
            "0.0.0.0",
            "::1",
            "10.0.0.5",
            "172.16.1.1",
            "172.31.255.255",
            "192.168.1.1",
            "100.64.0.1",      // CGNAT
            "100.127.255.254", // CGNAT upper bound
            "169.254.169.254", // cloud metadata
            "[::ffff:127.0.0.1]",
            "[::ffff:169.254.169.254]",
            "fd12::1",
            "fe80::1",
            "printer.local",
            "PRINTER.LOCAL.",
        ] {
            assert!(is_private_host(host), "should be private: {host}");
        }
        for host in [
            "example.com",
            "8.8.8.8",
            "1.1.1.1",
            "100.63.255.255",
            "100.128.0.1",
            "api.openai.com",
        ] {
            assert!(!is_private_host(host), "should be public: {host}");
        }
    }

    #[tokio::test]
    async fn resolve_and_validate_rejects_bad_schemes_and_private_targets() {
        for bad in [
            "ftp://8.8.8.8/file",
            "file:///etc/passwd",
            "http://localhost/admin",
            "http://127.0.0.1:9375/api/devices",
            "http://169.254.169.254/latest/meta-data/",
            "http://10.0.0.1/",
            "http://100.64.0.1/",
            "http://[::1]:8080/",
            "http://[::ffff:127.0.0.1]/",
            "http://[fe80::1]/",
        ] {
            assert!(
                resolve_and_validate_public_url(bad).await.is_err(),
                "expected rejection for {bad}"
            );
        }
    }

    #[tokio::test]
    async fn resolve_and_validate_accepts_public_ip_literal() {
        let addrs = resolve_and_validate_public_url("https://1.1.1.1/dns-query")
            .await
            .expect("public literal IP should be allowed");
        assert_eq!(addrs, vec!["1.1.1.1:443".parse().unwrap()]);

        let addrs_http = resolve_and_validate_public_url("http://8.8.8.8:8080/test")
            .await
            .expect("public literal IP with port should be allowed");
        assert_eq!(addrs_http, vec!["8.8.8.8:8080".parse().unwrap()]);
    }
}
