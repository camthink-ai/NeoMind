//! Web search tool — keyless structured search over Bing's RSS output.
//!
//! Channel choice (measured 2026-11-05 from both the Mac dev box and the T4
//! server): DuckDuckGo's no-script pages serve an anomaly challenge to both
//! networks, Baidu serves a captcha to scripted clients, and Bing's HTML mode
//! returns off-topic filler for pure-Chinese queries. Bing's `format=rss`
//! output is the one keyless channel that returns clean, stable
//! title/link/description/pubDate items — excellent for English queries,
//! unreliable for pure-Chinese ones (the tool description tells the model to
//! retry with English keywords, which measured as the effective workaround).
//! If this channel ever degrades, the fix is a new parse function in this
//! file — the model-facing schema never changes.

use async_trait::async_trait;
use serde_json::Value;
use std::sync::LazyLock;
use std::time::Instant;

use neomind_core::tools::ToolCategory;

use super::error::{Result, ToolError};
use super::timeouts;
use super::tool::{object_schema, Tool, ToolOutput};

/// Search endpoint. `format=rss` is the stable, script-friendly surface.
const BING_SEARCH_URL: &str = "https://www.bing.com/search?format=rss";

/// Results per engine request.
const FETCH_RESULTS: usize = 10;

/// Default / maximum results returned to the model.
const DEFAULT_COUNT: usize = 5;
const MAX_COUNT: usize = 10;

/// Maximum response body size (1 MB), mirroring web_fetch.
const MAX_RESPONSE_BODY: usize = 1024 * 1024;

/// Minimum spacing between engine requests. Engines rate-limit scripted
/// clients aggressively; the tool is called rarely, so a flat 1s floor is
/// enough. The tokio Mutex is held across the wait, which also serializes
/// concurrent searches from parallel tool calls.
const MIN_REQUEST_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

/// A browser-like User-Agent. The plain "NeoMind-Agent/1.0" UA gets served
/// degraded pages by some engines; this one was verified against the live
/// endpoint.
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
                         AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

static RE_ITEM: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?s)<item>(.*?)</item>").unwrap());
static RE_TITLE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?s)<title>(.*?)</title>").unwrap());
static RE_LINK: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?s)<link>(.*?)</link>").unwrap());
static RE_DESCRIPTION: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?s)<description>(.*?)</description>").unwrap());
static RE_PUB_DATE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?s)<pubDate>(.*?)</pubDate>").unwrap());
static RE_TAG: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"<[^>]+>").unwrap());
static RE_WS: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"\s+").unwrap());
static RE_NUM_ENTITY: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"&#(\d+);").unwrap());
static THROTTLE: LazyLock<tokio::sync::Mutex<Option<Instant>>> =
    LazyLock::new(|| tokio::sync::Mutex::new(None));

/// One parsed search result.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    /// Publication date as reported by the engine (RFC 822 string, may be absent).
    pub published: Option<String>,
}

/// Web search tool — returns structured results without any API key or
/// configuration. The engine is fixed; the model only supplies a query, so
/// there is no URL-shaped input anywhere in this tool (nothing for the model
/// to probe the network with — keep it that way).
pub struct WebSearchTool {
    client: reqwest::Client,
}

impl WebSearchTool {
    pub fn new() -> Self {
        // Same defense-in-depth redirect policy as web_fetch: follow, but
        // never into a private address. The endpoint is engine-controlled,
        // but a redirected engine response should never land on the LAN.
        let redirect_policy = reqwest::redirect::Policy::custom(|attempt| {
            let url = attempt.url().clone();
            if neomind_core::net::is_private_host(url.host_str().unwrap_or_default()) {
                return attempt.error(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("Redirect to private address '{}' blocked", url),
                ));
            }
            if attempt.previous().len() >= 5 {
                return attempt.error(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Too many redirects",
                ));
            }
            attempt.follow()
        });

        let client = reqwest::Client::builder()
            .timeout(timeouts::web_search())
            .redirect(redirect_policy)
            .build()
            .expect("Failed to build reqwest client");
        Self { client }
    }

    /// Serialize engine requests with a flat spacing floor.
    async fn throttle(&self) {
        let mut last = THROTTLE.lock().await;
        if let Some(prev) = *last {
            let elapsed = prev.elapsed();
            if elapsed < MIN_REQUEST_INTERVAL {
                tokio::time::sleep(MIN_REQUEST_INTERVAL - elapsed).await;
            }
        }
        *last = Some(Instant::now());
    }

    /// Run one engine query and parse the items out of the response.
    async fn search_once(&self, query: &str) -> Result<Vec<SearchResult>> {
        let is_zh = contains_cjk(query);
        let mut url = format!(
            "{}&q={}&count={}",
            BING_SEARCH_URL,
            urlencode(query),
            FETCH_RESULTS
        );
        if is_zh {
            // Measured: zh-led queries get the real Chinese index with
            // mkt=zh-CN; without it the RSS backend returns MSDN filler.
            url.push_str("&mkt=zh-CN");
        }

        let accept_language = if is_zh {
            "zh-CN,zh;q=0.9,en;q=0.5"
        } else {
            "en-US,en;q=0.9"
        };

        let response = self
            .client
            .get(&url)
            .header("User-Agent", USER_AGENT)
            .header("Accept-Language", accept_language)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    ToolError::Timeout
                } else {
                    ToolError::Execution(format!("Search request failed: {}", e))
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            return Err(ToolError::Execution(format!(
                "Search engine returned HTTP {} — the channel may be rate-limited; wait or try fewer searches",
                status.as_u16()
            )));
        }

        let body = response
            .bytes()
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to read search response: {}", e)))?;
        if body.len() > MAX_RESPONSE_BODY {
            return Err(ToolError::Execution(format!(
                "Search response too large ({} bytes)",
                body.len()
            )));
        }

        Ok(parse_rss_items(&String::from_utf8_lossy(&body)))
    }
}

impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse RSS `<item>` entries into results. Tolerates missing fields; skips
/// entries without both a title and an http(s) link.
pub(crate) fn parse_rss_items(xml: &str) -> Vec<SearchResult> {
    let mut out = Vec::new();
    for cap in RE_ITEM.captures_iter(xml) {
        let item = &cap[1];
        let title = RE_TITLE
            .captures(item)
            .map(|c| text_content(&c[1]))
            .unwrap_or_default();
        let url = RE_LINK
            .captures(item)
            .map(|c| text_content(&c[1]).trim().to_string())
            .unwrap_or_default();
        if title.is_empty() || !(url.starts_with("http://") || url.starts_with("https://")) {
            continue;
        }
        let snippet = RE_DESCRIPTION
            .captures(item)
            .map(|c| text_content(&c[1]))
            .unwrap_or_default();
        let published = RE_PUB_DATE
            .captures(item)
            .map(|c| text_content(&c[1]))
            .filter(|s| !s.is_empty());
        out.push(SearchResult {
            title,
            url,
            snippet,
            published,
        });
    }
    out
}

/// Strip tags, decode the common named entities plus numeric `&#NNN;`
/// entities, and compress whitespace. CDATA markers are dropped.
pub(crate) fn text_content(raw: &str) -> String {
    let no_cdata = raw.trim_start_matches("<![CDATA[").trim_end_matches("]]>");
    let no_tags = RE_TAG.replace_all(no_cdata, "");
    let decoded_named = no_tags
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");
    let decoded_numeric = RE_NUM_ENTITY.replace_all(&decoded_named, |caps: &regex::Captures| {
        char::from_u32(caps[1].parse::<u32>().unwrap_or(0xFFFD))
            .map(|c| c.to_string())
            .unwrap_or_else(|| "\u{FFFD}".to_string())
    });
    RE_WS.replace_all(&decoded_numeric, " ").trim().to_string()
}

/// Percent-encode a query for a URL query string (space becomes `+`).
pub(crate) fn urlencode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", byte)),
        }
    }
    out
}

/// True when the query contains CJK characters (drives mkt/Accept-Language).
pub(crate) fn contains_cjk(query: &str) -> bool {
    query.chars().any(|c| {
        matches!(c,
            '\u{3400}'..='\u{4DBF}'   // CJK ext A
            | '\u{4E00}'..='\u{9FFF}' // CJK unified
            | '\u{F900}'..='\u{FAFF}' // CJK compat
            | '\u{3040}'..='\u{30FF}' // hiragana/katakana
            | '\u{AC00}'..='\u{D7AF}' // hangul
        )
    })
}

/// Host part of an http(s) URL, for the `source` field.
pub(crate) fn url_host(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = &rest[..end];
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    /// Read-only search — re-running after an interruption cannot double-apply.
    fn replay_safety(&self) -> crate::toolkit::tool::ReplaySafety {
        crate::toolkit::tool::ReplaySafety::Safe
    }

    fn description(&self) -> &str {
        r#"Search the web and return structured results (title, url, snippet, source, date). No configuration needed.

Workflow:
1. Search once with a focused query, then open 1-3 of the most relevant URLs with `web_fetch` to read the source.
2. Search again with different keywords (at most 1-2 more times) ONLY if key facts are missing or sources conflict.
3. Cite source URLs in your final answer. Never present a snippet as a verified quote from the page — open the page first.
4. If results look off-topic, retry once with better keywords — English keywords work best for non-English topics. If the tool errors, stop searching and state what you could not verify.

Snippets are short: fetch the page for anything you rely on.
For NeoMind platform data (devices, rules, agents, telemetry) use `shell` (`neomind ...`), never web search."#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "query": {
                    "type": "string",
                    "description": "Search keywords. English keywords give the best results; for non-English topics prefer an English query"
                },
                "count": {
                    "type": "number",
                    "description": "Maximum results to return (default 5, max 10)"
                }
            }),
            vec!["query".to_string()],
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let query = args["query"]
            .as_str()
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .ok_or_else(|| ToolError::InvalidArguments("query is required".into()))?;

        let count = args["count"]
            .as_u64()
            .unwrap_or(DEFAULT_COUNT as u64)
            .clamp(1, MAX_COUNT as u64) as usize;

        tracing::info!(query = %query, count = count, "Web search");

        self.throttle().await;
        let results = self.search_once(query).await?;

        if results.is_empty() {
            // Loud failure: an empty list would read to the model as
            // "this information does not exist online".
            return Ok(ToolOutput::error(format!(
                "Search returned no usable results for {:?}. This is a channel/result failure, NOT proof the information is missing. Retry once with different keywords (English works best), then report the gap honestly if it persists.",
                query
            )));
        }

        let results: Vec<_> = results
            .into_iter()
            .take(count)
            .map(|r| {
                let mut item = serde_json::json!({
                    "title": r.title,
                    "url": r.url,
                    "snippet": r.snippet,
                    "source": url_host(&r.url).unwrap_or_default(),
                });
                if let Some(p) = r.published {
                    item["published"] = serde_json::Value::String(p);
                }
                item
            })
            .collect();

        Ok(ToolOutput::success(serde_json::json!({
            "query": query,
            "engine": "bing",
            "count": results.len(),
            "results": results,
            // Prompt forwarding, not a schema field for the model: keep the
            // drill-down loop moving toward web_fetch.
            "next_step": "Open the most relevant URL(s) with web_fetch to read the source before drawing conclusions.",
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real capture (2026-11-05) of Bing RSS output, trimmed to two items.
    const RSS_FIXTURE: &str = r#"<?xml version="1.0" encoding="utf-8" ?><rss version="2.0"><channel><title>Bing: rust programming language</title><link>http://www.bing.com:80/search?q=rust+programming+language</link><description>Search results</description><item><title>Rust Programming Language</title><link>https://rust-lang.org/</link><description>Rust is blazingly fast and memory-efficient: with no runtime or garbage collector.</description><pubDate>Wed, 04 Nov 2026 11:02:00 GMT</pubDate></item><item><title>Rust (programming language) - Wikipedia</title><link>https://en.wikipedia.org/wiki/Rust_(programming_language)</link><description>Rust is a general-purpose programming language that emphasizes performance &amp; type safety.</description><pubDate>Wed, 04 Nov 2026 02:12:00 GMT</pubDate></item></channel></rss>"#;

    #[test]
    fn test_parse_rss_fixture() {
        let items = parse_rss_items(RSS_FIXTURE);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Rust Programming Language");
        assert_eq!(items[0].url, "https://rust-lang.org/");
        assert_eq!(
            items[0].published.as_deref(),
            Some("Wed, 04 Nov 2026 11:02:00 GMT")
        );
        // Named entity decoded inside snippet.
        assert!(items[1].snippet.contains("performance & type safety"));
    }

    #[test]
    fn test_parse_rss_skips_incomplete_items() {
        let xml = r#"<rss><channel>
            <item><title>Only title</title></item>
            <item><link>https://example.com/</link><description>no title</description></item>
            <item><title>Ok</title><link>javascript:alert(1)</link></item>
            <item><title>Good</title><link>https://example.com/x</link></item>
        </channel></rss>"#;
        let items = parse_rss_items(xml);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Good");
    }

    #[test]
    fn test_parse_rss_non_rss_body() {
        assert!(parse_rss_items("<html><body>anomaly page</body></html>").is_empty());
        assert!(parse_rss_items("").is_empty());
    }

    #[test]
    fn test_parse_rss_cdata() {
        let xml = r#"<rss><channel><item><title><![CDATA[Some & title]]></title><link>https://example.com/</link></item></channel></rss>"#;
        let items = parse_rss_items(xml);
        assert_eq!(items[0].title, "Some & title");
    }

    #[test]
    fn test_text_content_entities_and_tags() {
        assert_eq!(text_content("<b>a &amp; b</b>"), "a & b");
        assert_eq!(text_content("&#20013;&#25991;"), "中文");
        assert_eq!(text_content("  multi\n line  text  "), "multi line text");
    }

    #[test]
    fn test_urlencode() {
        assert_eq!(urlencode("rust lang"), "rust+lang");
        assert_eq!(urlencode("a.b-c_d~e"), "a.b-c_d~e");
        // UTF-8 bytes, uppercase hex.
        assert_eq!(urlencode("中"), "%E4%B8%AD");
        assert_eq!(urlencode("q&a=b"), "q%26a%3Db");
    }

    #[test]
    fn test_contains_cjk() {
        assert!(contains_cjk("变频空调 故障"));
        assert!(contains_cjk("スマートホーム"));
        assert!(!contains_cjk("mqtt keepalive"));
        assert!(!contains_cjk("café"));
    }

    #[test]
    fn test_url_host() {
        assert_eq!(
            url_host("https://en.wikipedia.org/wiki/Rust"),
            Some("en.wikipedia.org".to_string())
        );
        assert_eq!(
            url_host("http://example.com"),
            Some("example.com".to_string())
        );
        assert_eq!(url_host("ftp://x"), None);
    }

    #[test]
    fn test_tool_name_and_schema() {
        let tool = WebSearchTool::new();
        assert_eq!(tool.name(), "web_search");
        let params = tool.parameters();
        assert_eq!(
            params["required"].as_array().unwrap().as_slice(),
            &[serde_json::json!("query")]
        );
        // Invariant: no URL-shaped parameter ever — the model must not be
        // able to point this tool at arbitrary hosts.
        let props = params["properties"].as_object().unwrap();
        assert!(props.contains_key("query"));
        assert!(!props.iter().any(|(k, _)| k.contains("url")));
    }
}
