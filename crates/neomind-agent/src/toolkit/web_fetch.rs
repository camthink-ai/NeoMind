//! Web fetch tool for retrieving URL content.

use async_trait::async_trait;
use serde_json::Value;
use std::sync::LazyLock;

use neomind_core::tools::ToolCategory;

use super::error::{Result, ToolError};
use super::timeouts;
use super::tool::{object_schema, Tool, ToolOutput};

/// Pre-compiled regexes (compiled once, reused across calls).
static RE_SCRIPT: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap());
static RE_STYLE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap());
static RE_TAG: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"<[^>]+>").unwrap());
static RE_WS: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"\s+").unwrap());

/// Web fetch tool — retrieves URL content with SSRF protection.
pub struct WebFetchTool {
    client: reqwest::Client,
}

/// Default max returned characters.
const DEFAULT_MAX_LENGTH: usize = 5000;

/// Maximum allowed max_length value (50K characters).
const MAX_ALLOWED_LENGTH: usize = 50_000;

/// Maximum response body size (1 MB).
const MAX_RESPONSE_BODY: usize = 1024 * 1024;

impl WebFetchTool {
    pub fn new() -> Self {
        // Custom redirect policy: validate each redirect target against SSRF rules
        let redirect_policy = reqwest::redirect::Policy::custom(|attempt| {
            let url = attempt.url().clone();
            let redirect_count = attempt.previous().len();
            if let Err(e) = Self::validate_url(&url) {
                tracing::warn!(url = %url, error = %e, "Redirect blocked by SSRF check");
                return attempt.error(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("Redirect to '{}' blocked: {}", url, e),
                ));
            }
            if redirect_count >= 5 {
                return attempt.error(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Too many redirects",
                ));
            }
            attempt.follow()
        });

        let client = reqwest::Client::builder()
            .timeout(timeouts::web_fetch())
            .redirect(redirect_policy)
            .no_proxy()
            .build()
            .expect("Failed to build reqwest client");
        Self { client }
    }

    /// Validate a reqwest::Url against SSRF rules.
    fn validate_url(url: &reqwest::Url) -> Result<()> {
        // Only allow http/https
        match url.scheme() {
            "http" | "https" => {}
            _ => {
                return Err(ToolError::PermissionDenied(
                    "Only http:// and https:// URLs are allowed".into(),
                ))
            }
        }

        let host = url
            .host_str()
            .ok_or_else(|| ToolError::InvalidArguments("URL has no host".into()))?;

        if Self::is_private_host(host) {
            tracing::warn!(url = %url, host = %host, "SSRF: blocked access to private address");
            return Err(ToolError::PermissionDenied(format!(
                "Access to '{}' is not allowed (private/local network address)",
                host
            )));
        }

        Ok(())
    }

    /// Check if a URL string is safe to fetch (SSRF protection).
    fn is_safe_url(url: &str) -> Result<reqwest::Url> {
        let parsed = reqwest::Url::parse(url)
            .map_err(|e| ToolError::InvalidArguments(format!("Invalid URL: {}", e)))?;
        Self::validate_url(&parsed)?;
        Ok(parsed)
    }

    /// Check if a hostname points to a private/local address.
    fn is_private_host(host: &str) -> bool {
        // Literal names
        match host {
            "localhost" | "127.0.0.1" | "0.0.0.0" | "::1" => return true,
            _ => {}
        }

        // Try parsing as IP for private range checks
        // Strip brackets from IPv6 URLs: [::ffff:127.0.0.1] -> ::ffff:127.0.0.1
        let host_trimmed = host.trim_start_matches('[').trim_end_matches(']');
        if let Ok(ip) = host_trimmed.parse::<std::net::IpAddr>() {
            return Self::is_private_ip(&ip);
        }

        // Hostnames that look like local addresses
        if host.ends_with(".local")
            || host.ends_with(".localhost")
            || host == "localhost.localdomain"
        {
            return true;
        }

        false
    }

    /// Check if an IP address is private/local (covers IPv4, IPv6, and IPv4-mapped IPv6).
    fn is_private_ip(ip: &std::net::IpAddr) -> bool {
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
                    return Self::is_private_ip(&std::net::IpAddr::V4(v4));
                }
            }
        }
        false
    }

    /// Case-insensitive search for a byte pattern in a string.
    /// Returns the byte offset of the first match, or None.
    fn find_tag_offset(html: &str, tag: &[u8]) -> Option<usize> {
        let html_bytes = html.as_bytes();
        let tag_lower: Vec<u8> = tag.iter().map(|b| b.to_ascii_lowercase()).collect();
        html_bytes.windows(tag_lower.len()).position(|window| {
            window
                .iter()
                .zip(tag_lower.iter())
                .all(|(a, b)| a.to_ascii_lowercase() == *b)
        })
    }

    /// Strip HTML tags and extract body text.
    fn html_to_text(html: &str) -> String {
        // Case-insensitive search for <body using a simple scan (avoids to_lowercase index misalignment)
        let body_start = Self::find_tag_offset(html, b"<body");
        let body_content = if let Some(start) = body_start {
            // Find the '>' after the <body tag
            let after_body_tag = &html[start..];
            let content_start = after_body_tag
                .find('>')
                .map(|i| start + i + 1)
                .unwrap_or(start);
            // Case-insensitive search for </body>
            let content_end = Self::find_tag_offset(html, b"</body")
                .map(|pos| if pos > content_start { pos } else { html.len() })
                .unwrap_or(html.len());
            &html[content_start..content_end]
        } else {
            html
        };

        // Remove script and style blocks (using pre-compiled regexes)
        let no_script = RE_SCRIPT.replace_all(body_content, "");
        let no_style = RE_STYLE.replace_all(&no_script, "");

        // Remove HTML tags
        let text = RE_TAG.replace_all(&no_style, "");

        // Decode common HTML entities
        let text = text
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&nbsp;", " ");

        // Compress whitespace
        let compressed = RE_WS.replace_all(&text, " ");

        compressed.trim().to_string()
    }
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        r#"Fetch content from a URL and return cleaned text.

Use this tool to retrieve web pages, API responses, or any HTTP-accessible content.
Returns text with HTML tags stripped by default.

Use for EXTERNAL web content — documentation pages, reference material, public APIs,
or a search-engine results URL when you need to look something up online.
For NeoMind platform data (devices, rules, agents, telemetry, etc.) use `shell`
(`neomind ...`), NOT this tool — the platform's own data never needs a web fetch.

Security: Cannot access private/local network addresses (localhost, 127.0.0.1, 10.x, 192.168.x, etc.).
Redirects to private addresses are also blocked.
Timeout: 15 seconds. Max response: 1MB."#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "url": {
                    "type": "string",
                    "description": "The URL to fetch (http:// or https:// only)"
                },
                "format": {
                    "type": "string",
                    "enum": ["text", "raw"],
                    "description": "Output format: 'text' strips HTML tags (default), 'raw' returns content as-is"
                },
                "max_length": {
                    "type": "number",
                    "description": "Maximum characters to return (default: 5000, max: 50000)"
                }
            }),
            vec!["url".to_string()],
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let url = args["url"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("url is required".into()))?;

        // SSRF check on initial URL
        let parsed_url = Self::is_safe_url(url)?;

        let format = args["format"].as_str().unwrap_or("text");
        let max_length = args["max_length"]
            .as_u64()
            .unwrap_or(DEFAULT_MAX_LENGTH as u64) as usize;
        // Cap max_length to prevent token budget explosion
        let max_length = max_length.min(MAX_ALLOWED_LENGTH);

        tracing::info!(url = %url, format = %format, "Fetching URL");

        // Pre-check Content-Length header before downloading body
        let response = self
            .client
            .get(parsed_url.as_str())
            .header("User-Agent", "NeoMind-Agent/1.0")
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    ToolError::Timeout
                } else {
                    ToolError::Execution(format!("Request failed: {}", e))
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            return Ok(ToolOutput::error(format!(
                "HTTP {}: {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("Unknown")
            )));
        }

        // Check Content-Length header before downloading body
        if let Some(content_length) = response.headers().get("content-length") {
            if let Ok(len_str) = content_length.to_str() {
                if let Ok(len) = len_str.parse::<usize>() {
                    if len > MAX_RESPONSE_BODY {
                        return Ok(ToolOutput::error(format!(
                            "Response too large (Content-Length: {} bytes, max: {} bytes)",
                            len, MAX_RESPONSE_BODY
                        )));
                    }
                }
            }
        }

        // Check content type — parse media type (type/subtype) before parameters
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();

        let media_type = content_type.split(';').next().unwrap_or("").trim();

        let is_allowed = media_type.starts_with("text/")
            || media_type.contains("html")
            || media_type.contains("json")
            || media_type.contains("xml")
            || media_type.contains("yaml")
            || media_type.contains("csv")
            || media_type.is_empty();

        if !is_allowed {
            return Ok(ToolOutput::error(format!(
                "Unsupported content type: {}. Only text/html/json/xml/yaml/csv is supported.",
                content_type
            )));
        }

        // Download body with size check
        let body = response
            .bytes()
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to read response body: {}", e)))?;

        if body.len() > MAX_RESPONSE_BODY {
            return Ok(ToolOutput::error(format!(
                "Response too large: {} bytes (max: {} bytes)",
                body.len(),
                MAX_RESPONSE_BODY
            )));
        }

        let body_text = String::from_utf8_lossy(&body);

        // Format output
        let content = if format == "raw" {
            body_text.into_owned()
        } else if content_type.contains("html") {
            Self::html_to_text(&body_text)
        } else if content_type.contains("json") {
            // Pretty-print JSON
            match serde_json::from_str::<Value>(&body_text) {
                Ok(val) => serde_json::to_string_pretty(&val).unwrap_or(body_text.into_owned()),
                Err(_) => body_text.into_owned(),
            }
        } else {
            body_text.into_owned()
        };

        // Truncate
        let (final_content, truncated) = if content.len() > max_length {
            let mut end = max_length;
            while end > 0 && !content.is_char_boundary(end) {
                end -= 1;
            }
            (
                format!(
                    "{}...\n[truncated, {} chars omitted]",
                    &content[..end],
                    content.len() - end
                ),
                true,
            )
        } else {
            (content, false)
        };

        Ok(ToolOutput::success(serde_json::json!({
            "url": url,
            "status": status.as_u16(),
            "content_type": content_type,
            "content": final_content,
            "truncated": truncated,
            "length": final_content.len(),
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_private_host_localhost() {
        assert!(WebFetchTool::is_private_host("localhost"));
        assert!(WebFetchTool::is_private_host("127.0.0.1"));
        assert!(WebFetchTool::is_private_host("0.0.0.0"));
        assert!(WebFetchTool::is_private_host("::1"));
    }

    #[test]
    fn test_is_private_host_private_ranges() {
        assert!(WebFetchTool::is_private_host("10.0.0.1"));
        assert!(WebFetchTool::is_private_host("10.255.255.255"));
        assert!(WebFetchTool::is_private_host("172.16.0.1"));
        assert!(WebFetchTool::is_private_host("172.31.255.255"));
        assert!(WebFetchTool::is_private_host("192.168.0.1"));
        assert!(WebFetchTool::is_private_host("192.168.1.1"));
        assert!(WebFetchTool::is_private_host("169.254.1.1"));
    }

    #[test]
    fn test_is_private_host_public() {
        assert!(!WebFetchTool::is_private_host("8.8.8.8"));
        assert!(!WebFetchTool::is_private_host("1.1.1.1"));
        assert!(!WebFetchTool::is_private_host("example.com"));
        assert!(!WebFetchTool::is_private_host("172.15.0.1"));
        assert!(!WebFetchTool::is_private_host("172.32.0.1"));
    }

    #[test]
    fn test_is_private_ipv6() {
        // Loopback
        assert!(WebFetchTool::is_private_host("::1"));
        // IPv6 unique local
        assert!(WebFetchTool::is_private_host("fd00::1"));
        assert!(WebFetchTool::is_private_host("fc00::1"));
        // IPv6 link-local
        assert!(WebFetchTool::is_private_host("fe80::1"));
        // IPv4-mapped IPv6 pointing to private addresses
        assert!(WebFetchTool::is_private_host("::ffff:127.0.0.1"));
        assert!(WebFetchTool::is_private_host("::ffff:192.168.1.1"));
        assert!(WebFetchTool::is_private_host("::ffff:10.0.0.1"));
        // Public IPv6 should be allowed
        assert!(!WebFetchTool::is_private_host("2001:4860:4860::8888"));
        assert!(!WebFetchTool::is_private_host("2606:4700:4700::1111"));
    }

    #[test]
    fn test_is_safe_url_rejects_ftp() {
        let result = WebFetchTool::is_safe_url("ftp://example.com");
        assert!(result.is_err());
    }

    #[test]
    fn test_is_safe_url_rejects_localhost() {
        let result = WebFetchTool::is_safe_url("http://localhost:9375");
        assert!(result.is_err());
    }

    #[test]
    fn test_is_safe_url_rejects_ipv4_mapped_ipv6() {
        let result = WebFetchTool::is_safe_url("http://[::ffff:127.0.0.1]:9375");
        assert!(result.is_err());
        let result = WebFetchTool::is_safe_url("http://[::ffff:192.168.1.1]");
        assert!(result.is_err());
    }

    #[test]
    fn test_is_safe_url_accepts_public() {
        let result = WebFetchTool::is_safe_url("https://example.com");
        assert!(result.is_ok());
    }

    #[test]
    fn test_html_to_text() {
        let html =
            "<html><head><title>Test</title></head><body><h1>Hello</h1><p>World</p></body></html>";
        let text = WebFetchTool::html_to_text(html);
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
        assert!(!text.contains("<h1>"));
    }

    #[test]
    fn test_html_to_text_strips_script() {
        let html = "<body><script>alert('xss')</script><p>Content</p></body>";
        let text = WebFetchTool::html_to_text(html);
        assert!(!text.contains("alert"));
        assert!(text.contains("Content"));
    }

    #[test]
    fn test_html_to_text_case_insensitive_body() {
        let html = "<HTML><BODY><p>Test</p></BODY></HTML>";
        let text = WebFetchTool::html_to_text(html);
        assert!(text.contains("Test"));
    }

    #[test]
    fn test_tool_name() {
        let tool = WebFetchTool::new();
        assert_eq!(tool.name(), "web_fetch");
    }
}
