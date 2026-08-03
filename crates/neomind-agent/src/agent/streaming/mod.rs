//! Streaming response processing — re-exports from sub-modules.
//!
//! This module was decomposed from a single 2495-line file into focused sub-modules:
//! - `stream_core`: text-only streaming with multi-round ReAct loop
//! - `stream_multimodal`: multimodal (text + images) streaming
//! - `intent`: user intent detection (list-only dead end)
//! - `cache`: tool result caching
//! - `thinking`: thinking content cleanup
//! - `tool_detect`: JSON/XML tool call detection
//! - `sanitize`: base64 stripping, truncation
//! - `dedup`: tool result deduplication
//! - `result_format`: tool result formatting
//! - `context`: context window management
//! - `resolve`: cached argument resolution
//! - `tool_exec`: tool execution with retry

// Sub-modules
mod cache;
mod context;
mod dedup;
mod intent;
mod resolve;
mod result_format;
mod sanitize;
mod stream_core;
mod stream_multimodal;
mod thinking;
mod tool_detect;
mod tool_exec;

// Re-exports from neomind_core
pub use neomind_core::llm::compaction::{CompactionConfig, MessagePriority};

// Public API re-exports (preserving all original import paths)
pub use cache::ToolResultCache;
pub use result_format::format_tool_results;
pub use stream_core::{
    events_to_string_stream, process_stream_events_with_safeguards, StreamSafeguards,
};
pub use stream_multimodal::process_multimodal_stream_events_with_safeguards;
pub use thinking::cleanup_thinking_content;

/// `StreamExt::next().await` wrapped in `tokio::time::timeout`.
///
/// Every LLM stream loop previously did `while let Some(x) = stream.next().await`
/// with only an *in-loop* elapsed-timeout check. When the upstream stream
/// stalls (zero chunks — e.g. the LLM backend never yields), `next()` blocks
/// forever and the in-loop check is never reached, hanging the whole chat/eval
/// turn. This helper bounds each `next()` so a stall yields `None` and the
/// loop force-completes instead of hanging.
///
/// `Ok(Some(item))` = a chunk arrived; `Ok(None)` = stream ended normally;
/// `Err(_)` = stalled for `dur` with zero chunks → treat as end of stream.
pub async fn next_chunk_or_timeout<S>(stream: &mut S, dur: std::time::Duration) -> Option<S::Item>
where
    S: futures::Stream + Unpin,
{
    match tokio::time::timeout(dur, futures::StreamExt::next(stream)).await {
        Ok(Some(item)) => Some(item),
        Ok(None) => None,
        Err(_elapsed) => {
            tracing::warn!(
                "Stream stalled: no chunks for {:?}, forcing completion",
                dur
            );
            None
        }
    }
}

// Re-exports for internal crate use and test access
pub(crate) use resolve::resolve_cached_arguments;
pub(crate) use sanitize::{sanitize_tool_result_for_prompt, truncate_result_utf8};

#[cfg(test)]
mod stall_tests {
    use super::next_chunk_or_timeout;
    use std::time::Duration;

    /// A zero-chunk stall (upstream LLM never yields) must force-break via
    /// the timeout instead of hanging forever. Regression for the 8h hang on
    /// the `dashboard/components` eval case (stream_core PHASE 1 loop).
    #[tokio::test]
    async fn stalled_stream_times_out() {
        let mut stalled: futures::stream::Pending<Result<(String, bool), &'static str>> =
            futures::stream::pending();
        let started = std::time::Instant::now();
        let got = next_chunk_or_timeout(&mut stalled, Duration::from_millis(50)).await;
        assert!(got.is_none(), "stalled stream must yield None on timeout");
        assert!(
            started.elapsed() >= Duration::from_millis(40),
            "must actually wait for the timeout, not return instantly"
        );
    }

    /// A finite stream still yields normally through the wrapper.
    #[tokio::test]
    async fn finite_stream_yields() {
        let chunks: Vec<Result<(String, bool), &'static str>> = vec![
            Ok(("hi".to_string(), false)),
            Ok(("there".to_string(), false)),
        ];
        let mut s = futures::stream::iter(chunks);
        let mut n = 0;
        while let Some(item) = next_chunk_or_timeout(&mut s, Duration::from_secs(1)).await {
            assert!(item.is_ok());
            n += 1;
        }
        assert_eq!(n, 2);
    }
}

#[cfg(test)]
use sanitize::{humanize_bytes, is_large_base64_string};
#[cfg(test)]
use tool_detect::detect_json_tool_calls;

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    // Use std::result::Result for test data (not the crate's Result alias)
    type TestResult<T> = std::result::Result<T, &'static str>;

    /// Test scenario 1: Pure content response (no thinking, no tools)
    #[tokio::test]
    async fn test_pure_content_stream() {
        let chunks: Vec<TestResult<(String, bool)>> = vec![
            Ok(("你好，我是".to_string(), false)),
            Ok(("NeoMind助手".to_string(), false)),
            Ok(("。".to_string(), false)),
        ];

        let mut stream = futures::stream::iter(chunks);

        let mut full_content = String::new();
        while let Some(result) = stream.next().await {
            if let Ok((text, is_thinking)) = result {
                assert!(!is_thinking, "Should not be thinking");
                full_content.push_str(&text);
            }
        }

        assert_eq!(full_content, "你好，我是NeoMind助手。");
        println!("Pure content stream test passed: {}", full_content);
    }

    /// Test scenario 2: Thinking + content response
    #[tokio::test]
    async fn test_thinking_then_content_stream() {
        let chunks: Vec<TestResult<(String, bool)>> = vec![
            Ok(("让我分析一下".to_string(), true)),
            Ok(("这个问题".to_string(), true)),
            Ok(("好的，我来回答".to_string(), false)),
            Ok(("这是答案".to_string(), false)),
        ];

        let mut stream = futures::stream::iter(chunks);

        let mut thinking_content = String::new();
        let mut actual_content = String::new();

        while let Some(result) = stream.next().await {
            if let Ok((text, is_thinking)) = result {
                if is_thinking {
                    thinking_content.push_str(&text);
                } else {
                    actual_content.push_str(&text);
                }
            }
        }

        assert_eq!(thinking_content, "让我分析一下这个问题");
        assert_eq!(actual_content, "好的，我来回答这是答案");
        println!("Thinking + content stream test passed");
        println!("  Thinking: {}", thinking_content);
        println!("  Content: {}", actual_content);
    }

    /// Test scenario 3: Content followed by tool call
    #[tokio::test]
    async fn test_content_with_tool_call() {
        let chunks: Vec<TestResult<(String, bool)>> = vec![
            Ok(("让我帮您".to_string(), false)),
            Ok(("查询设备".to_string(), false)),
            Ok((
                "<tool_calls><invoke name=\"list_devices\"></invoke></tool_calls>".to_string(),
                false,
            )),
        ];

        let mut stream = futures::stream::iter(chunks);

        let mut content_before_tools = String::new();
        let mut buffer = String::new();
        let mut tool_calls_found = false;

        while let Some(result) = stream.next().await {
            if let Ok((text, is_thinking)) = result {
                assert!(!is_thinking, "Should not be thinking in this test");
                buffer.push_str(&text);

                // Check for tool calls
                if let Some(tool_start) = buffer.find("<tool_calls>") {
                    content_before_tools.push_str(&buffer[..tool_start]);
                    if let Some(_tool_end) = buffer.find("</tool_calls>") {
                        tool_calls_found = true;
                        break;
                    }
                }
            }
        }

        assert_eq!(content_before_tools, "让我帮您查询设备");
        assert!(tool_calls_found, "Tool calls should be detected");
        println!("Content with tool call test passed");
        println!("  Content before tools: {}", content_before_tools);
    }

    /// Test scenario 4: Thinking + content + tool call
    #[tokio::test]
    async fn test_thinking_content_tool_call() {
        let chunks: Vec<TestResult<(String, bool)>> = vec![
            Ok(("用户想查询设备".to_string(), true)),
            Ok(("需要调用list_devices".to_string(), true)),
            Ok(("好的，我来".to_string(), false)),
            Ok(("查询一下".to_string(), false)),
            Ok((
                "<tool_calls><invoke name=\"list_devices\"></invoke></tool_calls>".to_string(),
                false,
            )),
        ];

        let mut stream = futures::stream::iter(chunks);

        let mut thinking = String::new();
        let mut content = String::new();
        let mut has_tool_calls = false;

        while let Some(result) = stream.next().await {
            if let Ok((text, is_thinking)) = result {
                if is_thinking {
                    thinking.push_str(&text);
                } else {
                    content.push_str(&text);
                    if text.contains("<tool_calls>") {
                        has_tool_calls = true;
                    }
                }
            }
        }

        assert_eq!(thinking, "用户想查询设备需要调用list_devices");
        assert!(content.contains("好的，我来查询一下"));
        assert!(has_tool_calls, "Should have tool calls");
        println!("Thinking + content + tool call test passed");
    }

    /// Test scenario 5: Empty content with thinking (edge case for think=true models)
    #[tokio::test]
    async fn test_thinking_only_no_content() {
        let chunks: Vec<TestResult<(String, bool)>> = vec![
            Ok(("这是我的思考过程".to_string(), true)),
            Ok(("继续思考".to_string(), true)),
        ];

        let mut stream = futures::stream::iter(chunks);

        let mut thinking = String::new();
        let mut content = String::new();

        while let Some(result) = stream.next().await {
            if let Ok((text, is_thinking)) = result {
                if is_thinking {
                    thinking.push_str(&text);
                } else {
                    content.push_str(&text);
                }
            }
        }

        assert_eq!(thinking, "这是我的思考过程继续思考");
        assert!(
            content.is_empty(),
            "Content should be empty for thinking-only response"
        );
        println!("Thinking-only test passed");
        println!("  Thinking: {}", thinking);
    }

    /// Test scenario 6: Content split across multiple chunks with Chinese characters
    #[tokio::test]
    async fn test_multibyte_chunk_handling() {
        let chunks: Vec<TestResult<(String, bool)>> = vec![
            // Split in middle of multi-byte sequence (shouldn't happen but test robustness)
            Ok(("你好".to_string(), false)),
            Ok(("世界".to_string(), false)),
            Ok(("，这是".to_string(), false)),
            Ok(("一个测试".to_string(), false)),
        ];

        let mut stream = futures::stream::iter(chunks);

        let mut full_content = String::new();
        while let Some(result) = stream.next().await {
            if let Ok((text, is_thinking)) = result {
                assert!(!is_thinking);
                full_content.push_str(&text);
            }
        }

        assert_eq!(full_content, "你好世界，这是一个测试");
        println!("Multi-byte chunk handling test passed");
        println!("  Content: {}", full_content);
    }

    /// Test scenario 7: Tool call with arguments
    #[tokio::test]
    async fn test_tool_call_with_arguments() {
        let tool_xml = r#"<tool_calls><invoke name="set_device_state">
<parameter name="device_id">lamp_1</parameter>
<parameter name="state">on</parameter>
</invoke></tool_calls>"#;

        let chunks: Vec<TestResult<(String, bool)>> = vec![
            Ok(("好的，我来帮您".to_string(), false)),
            Ok((tool_xml.to_string(), false)),
        ];

        let mut stream = futures::stream::iter(chunks);

        let mut content = String::new();
        let mut buffer = String::new();

        while let Some(result) = stream.next().await {
            if let Ok((text, _)) = result {
                buffer.push_str(&text);

                if let Some(tool_start) = buffer.find("<tool_calls>") {
                    content.push_str(&buffer[..tool_start]);
                    if buffer.contains("</tool_calls>") {
                        break;
                    }
                }
            }
        }

        assert_eq!(content, "好的，我来帮您");
        assert!(buffer.contains("<invoke name=\"set_device_state\">"));
        assert!(buffer.contains("<parameter name=\"device_id\">lamp_1</parameter>"));
        println!("Tool call with arguments test passed");
    }

    /// Test scenario 8: Empty chunks handling
    #[tokio::test]
    async fn test_empty_chunk_handling() {
        let chunks: Vec<TestResult<(String, bool)>> = vec![
            Ok(("开始".to_string(), false)),
            Ok(("".to_string(), false)), // Empty chunk
            Ok(("继续".to_string(), false)),
            Ok(("".to_string(), false)), // Another empty chunk
            Ok(("结束".to_string(), false)),
        ];

        let mut stream = futures::stream::iter(chunks);

        let mut full_content = String::new();
        while let Some(result) = stream.next().await {
            if let Ok((text, _)) = result {
                full_content.push_str(&text);
            }
        }

        // Empty chunks should be included but not cause issues
        assert!(full_content.contains("开始"));
        assert!(full_content.contains("继续"));
        assert!(full_content.contains("结束"));
        println!("Empty chunk handling test passed");
        println!("  Content: {}", full_content);
    }

    /// Test tool parser
    #[test]
    fn test_tool_parser() {
        let input = r#"{"name": "test_tool", "arguments": {"param1": "value1"}}"#;

        let result = crate::agent::tool_parser::parse_tool_calls(input);
        assert!(result.is_ok(), "Should parse tool calls successfully");

        let (_remaining, calls) = result.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "test_tool");
        assert_eq!(calls[0].arguments["param1"], "value1");
        println!("Tool parser test passed");
    }

    /// Test token estimation
    #[test]
    fn test_token_estimation() {
        let english = "Hello world, this is a test";
        let chinese = "你好世界，这是一个测试";

        let english_tokens = crate::agent::tokenizer::estimate_tokens(english);
        let chinese_tokens = crate::agent::tokenizer::estimate_tokens(chinese);

        // Rough estimation: ~4 chars per token for English, ~1.8 tokens per Chinese char
        assert!(english_tokens > 0 && english_tokens < 20);
        // Chinese: ~12 chars × 1.8 × 1.1 buffer ≈ 24 tokens
        assert!(chinese_tokens > 10 && chinese_tokens < 30);

        println!("Token estimation test passed");
        println!(
            "  English ({} chars): ~{} tokens",
            english.chars().count(),
            english_tokens
        );
        println!(
            "  Chinese ({} chars): ~{} tokens",
            chinese.chars().count(),
            chinese_tokens
        );
    }

    /// Test tool cache key generation
    #[test]
    fn test_cache_key_generation() {
        let key1 = ToolResultCache::make_key("list_devices", &serde_json::json!({}));
        let key2 = ToolResultCache::make_key("list_devices", &serde_json::json!(null));
        let key3 = ToolResultCache::make_key("list_devices", &serde_json::json!({}));

        assert_eq!(key1, key3, "Same args should produce same key");
        assert_ne!(key1, key2, "Different args should produce different keys");

        println!("Cache key generation test passed");
    }

    /// Test that malformed tool call JSON is not detected as tool calls
    /// This prevents false positives from JSON like [{"name":"[...]"}]
    #[test]
    fn test_malformed_tool_call_detection() {
        // Case 1: name field contains nested JSON array (should NOT be detected as tool call)
        let malformed1 = r#"[{"name":"[{"name":"device_discover","arguments":{}}]"}]"#;
        assert!(
            detect_json_tool_calls(malformed1).is_none(),
            "Should not detect malformed tool call with nested JSON array in name field"
        );

        // Case 2: name field contains nested JSON object (should NOT be detected as tool call)
        let malformed2 = r#"[{"name":"{"tool":"test"}"}]"#;
        assert!(
            detect_json_tool_calls(malformed2).is_none(),
            "Should not detect malformed tool call with nested JSON object in name field"
        );

        // Case 3: valid tool call (SHOULD be detected)
        let valid = r#"[{"name":"device_discover","arguments":{}}]"#;
        let result = detect_json_tool_calls(valid);
        assert!(result.is_some(), "Should detect valid tool call");
        let (_, json, _) = result.unwrap();
        assert_eq!(json, valid);

        // Case 4: valid tool call with different name field (SHOULD be detected)
        let valid2 = r#"[{"tool":"list_devices","params":{}}]"#;
        assert!(
            detect_json_tool_calls(valid2).is_some(),
            "Should detect valid tool call with 'tool' field"
        );

        // Case 5: valid tool call with function field (SHOULD be detected)
        let valid3 = r#"[{"function":"get_status","arguments":{}}]"#;
        assert!(
            detect_json_tool_calls(valid3).is_some(),
            "Should detect valid tool call with 'function' field"
        );

        println!("Malformed tool call detection test passed");
    }

    /// Run all streaming tests and print summary
    #[test]
    fn run_all_streaming_tests() {
        println!("\n=== Running LLM Streaming Tests ===\n");

        println!("Test Coverage:");
        println!("  1. Pure content response (no thinking, no tools)");
        println!("  2. Thinking + content response");
        println!("  3. Content followed by tool call");
        println!("  4. Thinking + content + tool call");
        println!("  5. Empty content with thinking (edge case)");
        println!("  6. Multi-byte chunk handling (Chinese)");
        println!("  7. Tool call with arguments");
        println!("  8. Empty chunks handling");
        println!("  9. Tool parser");
        println!(" 10. Token estimation");
        println!(" 11. Cache key generation");
        println!(" 12. Malformed tool call detection");
        println!("\n=== Test Suite Complete ===\n");
    }

    // -----------------------------------------------------------------------
    // Base64 stripping tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_sanitize_small_result_passes_through() {
        let result = r#"{"device_name":"test","battery":"100%"}"#;
        assert_eq!(sanitize_tool_result_for_prompt(result), result);
    }

    #[test]
    fn test_sanitize_json_with_data_image_url() {
        let result = serde_json::json!({
            "device_name": "NE101",
            "battery": "100%",
            "image_data": "data:image/jpeg;base64,/9j/4AAQSkZJRgABAQ"
        })
        .to_string();

        let sanitized = sanitize_tool_result_for_prompt(&result);
        assert!(
            !sanitized.contains("base64"),
            "Should strip base64 data URL"
        );
        assert!(
            !sanitized.contains("/9j/4AAQ"),
            "Should strip image content"
        );
        assert!(
            sanitized.contains("image data"),
            "Should have image data placeholder"
        );
        assert!(
            sanitized.contains("device_name"),
            "Should preserve non-image fields"
        );
        assert!(sanitized.contains("NE101"), "Should preserve device name");
        assert!(sanitized.contains("100%"), "Should preserve battery info");
    }

    #[test]
    fn test_sanitize_json_with_large_base64_string() {
        // Create a JSON with a large base64 string (>10KB)
        let fake_base64: String = "ABCDEFGHijklmnop+/=".repeat(600); // ~13KB
        let result = serde_json::json!({
            "device_name": "Camera",
            "firmware": "v1.7",
            "base64_data": fake_base64
        })
        .to_string();

        let sanitized = sanitize_tool_result_for_prompt(&result);
        assert!(!sanitized.contains("ABCDEFGH"), "Should strip large base64");
        assert!(
            sanitized.contains("base64 data"),
            "Should have base64 placeholder"
        );
        assert!(sanitized.contains("Camera"), "Should preserve device name");
        assert!(sanitized.contains("v1.7"), "Should preserve firmware");
    }

    #[test]
    fn test_sanitize_nested_json_with_base64() {
        let result = serde_json::json!({
            "device": {
                "name": "NE101",
                "info": {
                    "battery": "85%",
                    "image": "data:image/png;base64,iVBORw0KGgo="
                }
            }
        })
        .to_string();

        let sanitized = sanitize_tool_result_for_prompt(&result);
        assert!(sanitized.contains("NE101"), "Should preserve nested text");
        assert!(sanitized.contains("85%"), "Should preserve battery");
        assert!(!sanitized.contains("iVBOR"), "Should strip nested base64");
        assert!(sanitized.contains("image data"), "Should have placeholder");
    }

    #[test]
    fn test_sanitize_text_with_data_image_url() {
        let text = "Device: Camera\nBattery: 100%\nImage: data:image/jpeg;base64,/9j/4AAQSkZJRgABAQ==\nStatus: OK";

        let sanitized = sanitize_tool_result_for_prompt(text);
        assert!(!sanitized.contains("/9j/"), "Should strip image data");
        assert!(sanitized.contains("Camera"), "Should preserve text");
        assert!(sanitized.contains("100%"), "Should preserve battery");
        assert!(
            sanitized.contains("Status: OK"),
            "Should preserve other text"
        );
    }

    #[test]
    fn test_sanitize_no_base64_large_result_passes_through() {
        // Large result without base64 should be preserved
        let large_data: String = "x".repeat(5000);
        let result = format!(r#"{{"data": "{}"}}"#, large_data);

        let sanitized = sanitize_tool_result_for_prompt(&result);
        assert_eq!(sanitized, result, "Should pass through non-base64 data");
    }

    #[test]
    fn test_truncate_utf8_safe() {
        // Chinese text truncation
        let text = "你好世界这是一段中文测试文本用于验证UTF8安全截断功能";
        let truncated = truncate_result_utf8(text, 5);
        assert!(truncated.starts_with("你好世界这"));
        assert!(truncated.contains("truncated"));

        // Text shorter than max
        let short = "hello";
        assert_eq!(truncate_result_utf8(short, 100), short);
    }

    #[test]
    fn test_humanize_bytes() {
        assert_eq!(humanize_bytes(500), "500B");
        assert_eq!(humanize_bytes(1024), "1.0KB");
        assert_eq!(humanize_bytes(1536), "1.5KB");
        assert_eq!(humanize_bytes(1048576), "1.0MB");
        assert_eq!(humanize_bytes(2621440), "2.5MB");
    }

    #[test]
    fn test_is_large_base64_string() {
        // Too small
        assert!(!is_large_base64_string("abc123"));

        // Large valid base64
        let large_b64: String = "ABCDEFGHijklmnop+/=".repeat(600);
        assert!(is_large_base64_string(&large_b64));

        // Large but not base64 (contains invalid chars)
        let not_b64 = "hello world! ".repeat(1000);
        assert!(!is_large_base64_string(&not_b64));
    }
}
