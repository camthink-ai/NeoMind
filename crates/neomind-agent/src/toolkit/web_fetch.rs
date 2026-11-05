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
/// Block-level tags become paragraph breaks instead of vanishing into one
/// line of text — the model needs the structure to tell content from chrome.
static RE_BLOCK_TAG: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"(?i)</?(?:p|div|h[1-6]|li|ul|ol|table|thead|tbody|tr|blockquote|pre|section|article|header|footer|nav|aside|main|form|dl|dt|dd)\b[^>]*>",
    )
    .unwrap()
});
static RE_BR: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?i)<br\s*/?>").unwrap());
/// Horizontal whitespace runs (not newlines) — collapse those, keep breaks.
static RE_HWS: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"[^\S\n]+").unwrap());
static RE_MANY_NL: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"\n{3,}").unwrap());
static RE_TITLE_TAG: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?is)<title[^>]*>(.*?)</title>").unwrap());
/// Anchors with quoted or unquoted hrefs; URLs are resolved against the final page URL.
static RE_ANCHOR: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r##"(?is)<a\b[^>]*\bhref\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))[^>]*>(.*?)</a>"##,
    )
    .unwrap()
});

/// Maximum number of page links returned alongside the text.
const MAX_LINKS: usize = 15;

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
    /// Delegates to the shared SSRF guard in neomind_core::net (extracted
    /// from here — the transform engine's URL fetch uses the same rules).
    fn is_private_host(host: &str) -> bool {
        neomind_core::net::is_private_host(host)
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

    /// Strip HTML tags and extract body text, preserving paragraph breaks.
    fn html_to_text(html: &str) -> String {
        let body_content = Self::isolate_body(html);

        // Remove script and style blocks (using pre-compiled regexes)
        let no_script = RE_SCRIPT.replace_all(body_content, "");
        let no_style = RE_STYLE.replace_all(&no_script, "");

        // Block boundaries and <br> become newlines
        let with_breaks = RE_BLOCK_TAG.replace_all(&no_style, "\n");
        let with_breaks = RE_BR.replace_all(&with_breaks, "\n");

        // Remove remaining HTML tags
        let text = RE_TAG.replace_all(&with_breaks, "");

        // Decode common HTML entities
        let text = decode_entities(&text);

        // Collapse horizontal whitespace runs, then 3+ newlines to a blank line
        let collapsed = RE_HWS.replace_all(&text, " ");
        let collapsed = RE_MANY_NL.replace_all(&collapsed, "\n\n");

        collapsed.trim().to_string()
    }

    /// Slice of the HTML inside `<body>…</body>` (whole input when absent).
    fn isolate_body(html: &str) -> &str {
        let body_start = Self::find_tag_offset(html, b"<body");
        let content_start = if let Some(start) = body_start {
            // Find the '>' after the <body tag
            html[start..]
                .find('>')
                .map(|i| start + i + 1)
                .unwrap_or(start)
        } else {
            0
        };
        let content_end = Self::find_tag_offset(html, b"</body")
            .map(|pos| if pos > content_start { pos } else { html.len() })
            .unwrap_or(html.len());
        &html[content_start..content_end]
    }

    /// Up to [`MAX_LINKS`] resolved HTTP(S) links found in the body, with their
    /// anchor text. Extracted before tag stripping so the model can open them.
    fn extract_links(html: &str, base: &reqwest::Url) -> Vec<(String, String)> {
        let body = Self::isolate_body(html);
        let no_script = RE_SCRIPT.replace_all(body, "");
        let no_style = RE_STYLE.replace_all(&no_script, "");

        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for cap in RE_ANCHOR.captures_iter(&no_style) {
            let href = decode_entities(
                cap.get(1)
                    .or_else(|| cap.get(2))
                    .or_else(|| cap.get(3))
                    .map(|m| m.as_str())
                    .unwrap_or(""),
            );
            if href.is_empty() || href.starts_with('#') {
                continue;
            }
            let Ok(mut parsed) = base.join(&href) else {
                continue;
            };
            if !matches!(parsed.scheme(), "http" | "https")
                || !parsed.username().is_empty()
                || parsed.password().is_some()
            {
                continue;
            }
            parsed.set_fragment(None);
            let url = parsed.to_string();
            if url.len() > 500 || !seen.insert(url.clone()) {
                continue;
            }
            let text = text_of(&cap[4]);
            if text.is_empty() {
                continue; // image/icon anchors carry no label
            }
            let label: String = text.chars().take(80).collect();
            out.push((label, url));
            if out.len() >= MAX_LINKS {
                break;
            }
        }
        out
    }

    /// `<title>` content, entity-decoded, for the output's `title` field.
    fn extract_title(html: &str) -> Option<String> {
        let raw = RE_TITLE_TAG.captures(html).map(|c| text_of(&c[1]))?;
        let title: String = raw.chars().take(200).collect();
        (!title.is_empty()).then_some(title)
    }
}

/// Decode the handful of entities that appear in titles, snippets and anchor
/// text. Numeric entities are left as-is — pages lean on the named ones.
fn decode_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

/// Strip tags + decode entities + collapse whitespace, for short fragments.
fn text_of(fragment: &str) -> String {
    let no_tags = RE_TAG.replace_all(fragment, "");
    RE_WS
        .replace_all(&decode_entities(&no_tags), " ")
        .trim()
        .to_string()
}

/// Read window over the content starting at `offset`. Returns
/// `(text, truncated, next_offset)`; `None` when the offset is past the end.
fn window(
    content: &str,
    offset: usize,
    max_length: usize,
) -> Option<(String, bool, Option<usize>)> {
    let mut start = offset.min(content.len());
    while start > 0 && !content.is_char_boundary(start) {
        start -= 1;
    }
    if start >= content.len() {
        return None;
    }
    let slice = &content[start..];
    if slice.len() > max_length {
        let mut end = max_length;
        while end > 0 && !slice.is_char_boundary(end) {
            end -= 1;
        }
        if end == 0 {
            end = slice.chars().next().map(char::len_utf8).unwrap_or(0);
        }
        let next = start + end;
        Some((
            format!(
                "{}...\n[truncated, {} chars remaining — pass offset={} to continue]",
                &slice[..end],
                slice.len() - end,
                next
            ),
            true,
            Some(next),
        ))
    } else {
        Some((slice.to_string(), false, None))
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

    /// Read-only fetch — re-running after an interruption cannot double-apply.
    fn replay_safety(&self) -> crate::toolkit::tool::ReplaySafety {
        crate::toolkit::tool::ReplaySafety::Safe
    }

    fn description(&self) -> &str {
        r#"Fetch content from a URL and return cleaned text with paragraph structure.

Use for EXTERNAL web content — documentation pages, reference material, public APIs.
To FIND pages on a topic, use `web_search` first, then open the URLs it returns here.
For NeoMind platform data (devices, rules, agents, telemetry, etc.) use `shell`
(`neomind ...`), NOT this tool — the platform's own data never needs a web fetch.

Returns the page title, the final URL after redirects, readable text, and up to
15 unique links found on the page (use them or `web_search` to drill deeper).
When `truncated` is true, call again with the returned `next_offset` as `offset`
to continue reading from where it stopped.

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
                },
                "offset": {
                    "type": "number",
                    "description": "Character offset to start reading at (use next_offset from a truncated response to continue)"
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
        if max_length == 0 {
            return Err(ToolError::InvalidArguments(
                "max_length must be greater than zero".into(),
            ));
        }
        let max_length = max_length.clamp(4, MAX_ALLOWED_LENGTH);
        let offset = args["offset"].as_u64().unwrap_or(0) as usize;

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
                    // Connection-class failures carry a redirect note: the
                    // live incident had a small model hammering an INVENTED
                    // endpoint (api.neomind.com) because each echoed-URL
                    // error read as "real address, transient problem".
                    // Name the alternative at the point of failure.
                    ToolError::Execution(format!(
                        "Request failed: {}. NOTE: if this host was your guess for a NeoMind platform API, that is the wrong interface. Right tool per task: CLI-backed platform domains (dashboards, widgets, devices, rules, agents) → `shell` tool with `neomind ...` (e.g. `neomind dashboard add-components <ID> --components '<json>'`); file work → file_read/file_write/file_edit; image work → vision/image_edit; memory facts → memory. web_fetch is for EXTERNAL URLs only — the platform has no public HTTP API at external domains.",
                        e
                    ))
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
        // (final URL captured first — `bytes()` consumes the response)
        let final_url = response.url().to_string();
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
        let is_html = content_type.contains("html");
        // Title/links borrow the raw body; compute before `into_owned` consumes it.
        let title = if is_html && format != "raw" {
            Self::extract_title(&body_text)
        } else {
            None
        };
        let links = if is_html && format != "raw" {
            Self::extract_links(
                &body_text,
                &reqwest::Url::parse(&final_url).expect("response URL is valid"),
            )
        } else {
            Vec::new()
        };
        let content = if format == "raw" {
            body_text.into_owned()
        } else if is_html {
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

        // Read window at the requested offset
        let Some((final_content, truncated, next_offset)) = window(&content, offset, max_length)
        else {
            return Ok(ToolOutput::error(format!(
                "offset {} is beyond the end of the content ({} chars)",
                offset,
                content.len()
            )));
        };

        let mut out = serde_json::json!({
            "url": final_url,
            "status": status.as_u16(),
            "content_type": content_type,
            "content": final_content,
            "truncated": truncated,
            "offset": offset,
            "length": content.len(),
        });
        if final_url != url {
            out["requested_url"] = serde_json::Value::String(url.to_string());
        }
        if let Some(next) = next_offset {
            out["next_offset"] = serde_json::json!(next);
        }
        if let Some(title) = title {
            out["title"] = serde_json::Value::String(title);
        }
        let links: Vec<_> = links
            .into_iter()
            .map(|(text, url)| serde_json::json!({"text": text, "url": url}))
            .collect();
        if !links.is_empty() {
            out["links"] = serde_json::json!(links);
        }

        Ok(ToolOutput::success(out))
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

    #[test]
    fn test_html_to_text_preserves_paragraphs() {
        let html = "<body><p>First paragraph.</p><p>Second paragraph.</p><ul><li>one</li><li>two</li></ul></body>";
        let text = WebFetchTool::html_to_text(html);
        // Block close + open each contribute a newline → blank line between blocks.
        assert!(text.contains("First paragraph.\n\nSecond paragraph."));
        assert!(text.contains("one\n\ntwo"));
        // Nav chrome and content must not collapse into a single line.
        assert!(text.lines().count() >= 3);
    }

    #[test]
    fn test_extract_links() {
        let html = r#"<body>
            <a href="https://example.com/a">First &amp; foremost</a>
            <a href="https://example.com/a">duplicate URL</a>
            <a href="https://example.com/b"><img src="x.png"></a>
            <a href="/relative">relative link</a>
            <a href="https://example.com/c">Third <b>link</b></a>
        </body>"#;
        let links = WebFetchTool::extract_links(
            html,
            &reqwest::Url::parse("https://example.com/docs/page").unwrap(),
        );
        // Duplicate URL and empty-text anchor dropped; relative link resolved.
        assert_eq!(links.len(), 3);
        assert_eq!(links[0].0, "First & foremost");
        assert_eq!(links[0].1, "https://example.com/a");
        assert_eq!(links[1].1, "https://example.com/relative");
        assert_eq!(links[2].1, "https://example.com/c");
    }

    #[test]
    fn relative_single_quoted_and_unquoted_links_use_final_url() {
        let base = reqwest::Url::parse("https://example.org/docs/v2/page").unwrap();
        let html = r#"<body><a href='../next#part'>Next</a><a href=/root>Root</a><a href='javascript:alert(1)'>bad</a><a href='#local'>local</a></body>"#;
        let links = WebFetchTool::extract_links(html, &base);
        assert_eq!(
            links,
            vec![
                ("Next".into(), "https://example.org/docs/next".into()),
                ("Root".into(), "https://example.org/root".into())
            ]
        );
    }

    #[test]
    fn tiny_unicode_window_advances() {
        let (content, truncated, next) = window("中文", 0, 1).unwrap();
        assert!(truncated);
        assert!(content.starts_with('中'));
        assert_eq!(next, Some(3));
    }

    #[test]
    fn test_extract_title() {
        let html = "<html><head><title>NeoMind &amp; Docs</title></head><body>x</body></html>";
        assert_eq!(
            WebFetchTool::extract_title(html).as_deref(),
            Some("NeoMind & Docs")
        );
        assert!(WebFetchTool::extract_title("<body>no title</body>").is_none());
    }

    #[test]
    fn test_window_truncation_and_continuation() {
        let content = "0123456789".repeat(10); // 100 chars
        let (first, truncated, next) = window(&content, 0, 40).unwrap();
        assert!(truncated);
        assert_eq!(next, Some(40));
        assert!(first.starts_with("0123456789"));
        assert!(first.contains("offset=40"));

        let (second, truncated, next) = window(&content, 40, 40).unwrap();
        assert!(truncated);
        assert_eq!(next, Some(80));
        assert!(second.starts_with("0123456789"));

        let (third, truncated, next) = window(&content, 80, 40).unwrap();
        assert!(!truncated);
        assert_eq!(next, None);
        assert_eq!(third.len(), 20);

        assert!(window(&content, 100, 40).is_none());
        assert!(window(&content, 500, 40).is_none());
    }

    #[test]
    fn test_window_char_boundary_safe() {
        let content = "中文内容".repeat(10); // 40 chars, 120 bytes
        let (first, truncated, next) = window(&content, 0, 11).unwrap();
        assert!(truncated);
        // 11 would split a 3-byte char — the window must back off to a boundary.
        let next = next.unwrap();
        assert_eq!(next, 9);
        assert!(content.is_char_boundary(next));
        assert!(first.contains("pass offset=9 to continue"));
        // The visible text must contain no broken characters.
        let visible = &first[..first.find("...\n[truncated").unwrap()];
        assert!(!visible.contains('\u{FFFD}'));
    }
}
