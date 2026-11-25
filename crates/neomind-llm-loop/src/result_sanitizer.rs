//! Tool-result sanitizing for prompts: base64/image stripping and UTF-8-safe
//! truncation. Moved verbatim from `neomind-agent/src/agent/streaming/sanitize.rs`
//! (M0 extraction).

/// Minimum size (bytes) for a result to be considered large enough to strip base64 from.
pub(crate) const BASE64_STRIP_THRESHOLD: usize = 4096;

// ---------------------------------------------------------------------------
// Base64 / image stripping for tool result display
// ---------------------------------------------------------------------------

/// Strip base64/image data from a tool result for safe display and LLM prompts.
///
/// Base64 image data wastes LLM context tokens and causes the model to reproduce
/// raw data in its response. This function:
/// - For JSON results: walks the tree and replaces base64/image strings with `[image data, {size}]`
/// - For text with `data:image/...` URLs: replaces URLs with size markers
/// - Preserves all non-binary data (numbers, text, metadata)
pub fn sanitize_tool_result_for_prompt(result: &str) -> String {
    // Fast path: small results without base64 indicators pass through
    if result.len() < BASE64_STRIP_THRESHOLD
        && !result.contains("base64")
        && !result.contains("data:image/")
    {
        return result.to_string();
    }

    // Try JSON path: parse, strip, re-serialize
    if result.starts_with('{') || result.starts_with('[') {
        if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(result) {
            if strip_base64_from_json_value(&mut value) {
                if let Ok(stripped) = serde_json::to_string(&value) {
                    return stripped;
                }
            }
        }
        // JSON parse succeeded but no base64 found, or serialization failed — fall through
    }

    // Text containing data:image URLs
    if result.contains("data:image/") {
        return replace_data_image_urls(result);
    }

    result.to_string()
}

/// Recursively strip base64/image data from a JSON value tree.
/// Returns `true` if any value was modified.
pub(crate) fn strip_base64_from_json_value(value: &mut serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            let mut modified = false;

            // Collect keys whose values are base64/image data
            let replacements: Vec<(String, serde_json::Value)> = map
                .iter()
                .filter_map(|(k, v)| {
                    let s = v.as_str()?;
                    if s.starts_with("data:image/") {
                        return Some((
                            k.clone(),
                            serde_json::json!(format!("[image data, {}]", humanize_bytes(s.len()))),
                        ));
                    }
                    if is_large_base64_string(s) {
                        return Some((
                            k.clone(),
                            serde_json::json!(format!(
                                "[base64 data, {}]",
                                humanize_bytes(s.len())
                            )),
                        ));
                    }
                    None
                })
                .collect();

            for (key, replacement) in replacements {
                map.insert(key, replacement);
                modified = true;
            }

            // Recurse into child values
            for v in map.values_mut() {
                if strip_base64_from_json_value(v) {
                    modified = true;
                }
            }
            modified
        }
        serde_json::Value::Array(arr) => {
            let mut modified = false;
            for v in arr.iter_mut() {
                if strip_base64_from_json_value(v) {
                    modified = true;
                }
            }
            modified
        }
        _ => false,
    }
}

/// Check if a string looks like large base64 data (>10KB, valid base64 alphabet).
pub fn is_large_base64_string(s: &str) -> bool {
    if s.len() <= 10_000 {
        return false;
    }
    // Sample first 200 chars to check base64 alphabet.
    // MUST use char-based slicing — byte slicing panics on multi-byte UTF-8
    // (e.g., skill docs containing '→' at byte boundary 200).
    let sample: String = s.chars().take(200).collect();
    sample
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
}

/// Replace `data:image/...;base64,...` URLs in a text with size markers.
pub(crate) fn replace_data_image_urls(text: &str) -> String {
    let mut result = text.to_string();
    while let Some(start) = result.find("data:image/") {
        // Find end of the URL (whitespace, quote, or end of string)
        let end = result[start..]
            .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
            .map(|i| start + i)
            .unwrap_or(result.len());
        let data_len = end - start;
        let replacement = format!("[image data, {}]", humanize_bytes(data_len));
        result.replace_range(start..end, &replacement);
    }
    result
}

/// Format byte count as human-readable string (e.g., "2.3MB", "512B").
pub fn humanize_bytes(bytes: usize) -> String {
    const KB: usize = 1024;
    const MB: usize = 1024 * KB;
    if bytes >= MB {
        format!("{:.1}MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1}KB", bytes as f64 / KB as f64)
    } else {
        format!("{}B", bytes)
    }
}

/// UTF-8 safe truncation for tool result text.
/// Returns the truncated string with an ellipsis suffix if truncated.
///
/// Uses byte length as a fast-path: if byte length <= max_chars, char count
/// is guaranteed to be within limit too (each char is at least 1 byte).
pub fn truncate_result_utf8(result: &str, max_chars: usize) -> String {
    // Fast path: byte length <= max_chars means char count <= max_chars
    if result.len() <= max_chars {
        return result.to_string();
    }
    // Slow path: need actual char count
    let char_count = result.chars().count();
    if char_count <= max_chars {
        return result.to_string();
    }
    let truncated: String = result.chars().take(max_chars).collect();
    format!("{}... (truncated, total {} chars)", truncated, char_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: byte-slicing at fixed offset 200 panicked when the offset
    /// landed inside a multi-byte UTF-8 char (e.g., '→' in skill docs).
    /// Reproducer for the eval hang on 2026-07-01 where loading
    /// `agent-management.md` (which contains "Create → Active Pattern")
    /// triggered `byte index 200 is not a char boundary` inside
    /// `is_large_base64_string`. Chat session was poisoned thereafter.
    #[test]
    fn test_is_large_base64_multibyte_at_boundary() {
        // Build a >10 KB string whose byte 200 falls inside '→' (3 bytes).
        // Prefix is ASCII to push past the 10 KB threshold, then place the
        // arrow exactly where byte 200 lands.
        let mut s = String::with_capacity(12_000);
        s.push_str(&"a".repeat(198)); // bytes 0..198 = ASCII
        s.push('→'); // bytes 198..201 (3-byte char)
        s.push_str(&"a".repeat(12_000 - 201));
        assert_eq!(s.len(), 12_000);
        // Must not panic; the sample is non-ASCII so returns false.
        assert!(!is_large_base64_string(&s));
    }

    /// Same regression guard for the twin implementation in `types.rs`
    /// (`ContentPart::looks_like_base64`) which had identical byte-slicing.
    #[test]
    fn test_truncate_result_utf8_keeps_char_boundary() {
        // Sanity: truncate_result_utf8 already used chars().take — verify
        // multi-byte content at the cut point doesn't panic.
        let s: String = "中".repeat(500);
        let out = truncate_result_utf8(&s, 100);
        assert!(out.contains("... (truncated,"));
        assert!(!out.contains('\u{FFFD}')); // no replacement char from bad slicing
    }
}
