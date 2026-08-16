//! SSRF guard — shared private/local address detection for any code
//! fetching URLs from untrusted input (device payloads, extension data).
//!
//! Extracted from the agent `web_fetch` tool (which had the only copy);
//! the transform engine's device-controlled `url_to_base64` fetch now uses
//! the same rules instead of none at all.

/// Check if a hostname points to a private/local address.
pub fn is_private_host(host: &str) -> bool {
    // Literal names
    match host {
        "localhost" | "127.0.0.1" | "0.0.0.0" | "::1" => return true,
        _ => {}
    }

    // Try parsing as IP for private range checks
    // Strip brackets from IPv6 URLs: [::ffff:127.0.0.1] -> ::ffff:127.0.0.1
    let host_trimmed = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = host_trimmed.parse::<std::net::IpAddr>() {
        return is_private_ip(&ip);
    }

    // Hostnames that look like local addresses
    if host.ends_with(".local") || host.ends_with(".localhost") || host == "localhost.localdomain" {
        return true;
    }

    false
}

/// Check if an IP address is private/local (covers IPv4, IPv6, and
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
            // 127.0.0.0/8
            if octets[0] == 127 {
                return true;
            }
            // 169.254.0.0/16 (link-local)
            if octets[0] == 169 && octets[1] == 254 {
                return true;
            }
            // 0.0.0.0/8 (current network)
            if octets[0] == 0 {
                return true;
            }
            // 100.64.0.0/10 (Carrier-grade NAT)
            if octets[0] == 100 && (64..=127).contains(&octets[1]) {
                return true;
            }
            // 192.0.2.0/24, 198.51.100.0/24, 203.0.113.0/24 (documentation)
            // 224.0.0.0/4 (multicast), 240.0.0.0/4 (reserved)
            if v4.is_broadcast() || v4.is_multicast() || v4.is_unspecified() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_hosts_detected() {
        for host in [
            "localhost",
            "127.0.0.1",
            "0.0.0.0",
            "::1",
            "10.0.0.5",
            "172.16.1.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254", // cloud metadata
            "[::ffff:127.0.0.1]",
            "fd12::1",
            "fe80::1",
            "printer.local",
        ] {
            assert!(is_private_host(host), "should be private: {host}");
        }
        for host in ["example.com", "8.8.8.8", "1.1.1.1", "api.openai.com"] {
            assert!(!is_private_host(host), "should be public: {host}");
        }
    }
}
