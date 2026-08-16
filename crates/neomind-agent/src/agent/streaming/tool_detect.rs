/// Detect JSON tool calls in buffer.
///
/// Looks for JSON array format: [{"name": "tool", "arguments": {...}}, ...]
/// Returns Some((start_pos, json_text, remaining_buffer)) if found, None otherwise.
///
/// [scan-advance] The detector used to anchor on the FIRST '[' in the whole
/// buffer and return None forever if that span wasn't a tool call — so any
/// innocuous JSON array earlier in the response (e.g. `trend: [1,2,3]` before
/// a tool call) permanently blinded it: the tool call streamed to the user as
/// visible content and never executed. Candidates are now iterated — a span
/// that parses but isn't a tool call advances the scan to its end.
pub(crate) fn detect_json_tool_calls(buffer: &str) -> Option<(usize, String, String)> {
    let mut search_from = 0usize;

    loop {
        // Find the next '[' that might start a JSON array
        let start = buffer[search_from..].find('[')? + search_from;

        // Find the matching closing ']' while properly handling:
        // 1. String literals (skip brackets inside "...")
        // 2. Escape sequences (skip escaped characters like \")
        let chars: Vec<char> = buffer[start..].chars().collect();
        let mut bracket_count = 0isize;
        let mut in_string = false;
        let mut end = None;
        let mut i = 0;

        while i < chars.len() {
            let c = chars[i];
            if in_string {
                if c == '\\' {
                    // Skip escaped character
                    i += 2;
                    continue;
                } else if c == '"' {
                    in_string = false;
                }
            } else {
                match c {
                    '"' => in_string = true,
                    '[' => bracket_count += 1,
                    ']' => {
                        bracket_count -= 1;
                        if bracket_count == 0 {
                            // Calculate byte offset from char index
                            let byte_offset: usize = chars[..=i].iter().map(|c| c.len_utf8()).sum();
                            end = Some(start + byte_offset);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            i += 1;
        }

        // No matching close bracket yet — incomplete; wait for more chunks.
        let end = end?;

        // Extract the JSON array
        let json_str = buffer[start..end].to_string();

        // A parseable span that isn't a tool call is NOT a terminal answer:
        // advance past it and look for the next candidate.
        let is_tool_call = (|| {
            // Check if it looks like a tool call (has "name", "tool", or "function" key)
            if !json_str.contains("\"name\"")
                && !json_str.contains("\"tool\"")
                && !json_str.contains("\"function\"")
            {
                return None;
            }

            // Verify it's valid JSON
            let json_value = serde_json::from_str::<serde_json::Value>(&json_str).ok()?;

            // Validate that at least one element has a valid string "name" field
            // This prevents false positives from malformed JSON like [{"name":"[...]"}]
            if let Some(arr) = json_value.as_array() {
                let has_valid_tool_call = arr.iter().any(|item| {
                    if let Some(obj) = item.as_object() {
                        // Check if "name", "tool", or "function" field exists and is a valid string
                        let name_value = obj
                            .get("name")
                            .or_else(|| obj.get("tool"))
                            .or_else(|| obj.get("function"));

                        if let Some(name) = name_value {
                            if let Some(name_str) = name.as_str() {
                                // Ensure the name is a simple string (not a JSON string containing nested JSON)
                                // A valid tool name should not start with '[' or '{'
                                let trimmed = name_str.trim();
                                return !trimmed.starts_with('[') && !trimmed.starts_with('{');
                            }
                        }
                    }
                    false
                });

                if has_valid_tool_call {
                    return Some(());
                }
            }
            None
        })();

        if is_tool_call.is_some() {
            // Return start position, the JSON, and remaining buffer
            let remaining = buffer[end..].to_string();
            return Some((start, json_str, remaining));
        }

        // Not a tool call — try the next '[' after this span.
        search_from = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advances_past_innocuous_leading_array() {
        // A plain data array before the tool call used to permanently
        // blind the detector (first-'[' anchor, non-tool span → None
        // forever — the tool call streamed as visible content).
        let buffer =
            r#"trend: [1, 2, 3]. Now calling: [{"name": "shell", "arguments": {"command": "ls"}}]"#;
        let (start, json, remaining) =
            detect_json_tool_calls(buffer).expect("must detect past the data array");
        assert!(start > buffer.find("Now calling").unwrap());
        assert!(json.contains("\"shell\""));
        assert!(remaining.is_empty() || !remaining.contains('['));
    }

    #[test]
    fn brackets_inside_string_arguments_do_not_break_the_span() {
        let buffer = r#"[{"name": "shell", "arguments": {"command": "ls foo[1].txt"}}]"#;
        let (_, json, _) =
            detect_json_tool_calls(buffer).expect("in-string [ must not break matching");
        assert!(json.contains("foo[1].txt"));
    }

    #[test]
    fn still_none_on_no_tool_call() {
        assert!(detect_json_tool_calls("just text, no brackets").is_none());
        assert!(detect_json_tool_calls("[1, 2, 3] plain data only").is_none());
    }
}
