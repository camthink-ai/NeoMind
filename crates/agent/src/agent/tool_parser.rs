//! Tool call parser for extracting tool calls from LLM responses.

use serde_json::Value;
use uuid::Uuid;

use super::types::ToolCall;
use crate::error::{AgentError, Result};

/// Parse tool calls from LLM response text.
///
/// Looks for JSON objects containing tool/function information
/// and returns the remaining text along with any parsed tool calls.
pub fn parse_tool_calls(text: &str) -> Result<(String, Vec<ToolCall>)> {
    let mut content = text.to_string();
    let mut tool_calls = Vec::new();

    // Find all JSON objects in the text
    if let Some(match_start) = text.find('{') {
        // Find the matching closing brace by counting braces
        let mut brace_count = 0;
        let mut json_end = match_start;

        for (i, c) in text[match_start..].char_indices() {
            if c == '{' {
                brace_count += 1;
            } else if c == '}' {
                brace_count -= 1;
                if brace_count == 0 {
                    json_end = match_start + i + 1;
                    break;
                }
            }
        }

        if json_end > match_start {
            let json_str = &text[match_start..json_end];
            if let Ok(value) = serde_json::from_str::<Value>(json_str) {
                // Check for "tool" or "function" or "name" key
                let tool_name = value.get("tool")
                    .or_else(|| value.get("function"))
                    .or_else(|| value.get("name"))
                    .and_then(|v| v.as_str());

                if let Some(name) = tool_name {
                    // Extract arguments
                    let arguments = value.get("arguments")
                        .or_else(|| value.get("params"))
                        .or_else(|| value.get("parameters"))
                        .cloned()
                        .unwrap_or_else(|| {
                            // If no explicit arguments field, use the whole value except tool name
                            let mut args = value.clone();
                            if let Some(obj) = args.as_object_mut() {
                                obj.remove("tool");
                                obj.remove("function");
                                obj.remove("name");
                            }
                            args
                        });

                    let call = ToolCall {
                        name: name.to_string(),
                        id: Uuid::new_v4().to_string(),
                        arguments,
                    };
                    tool_calls.push(call);
                    content = text[..match_start].trim().to_string();
                }
            }
        }
    }

    Ok((content, tool_calls))
}

/// Parse tool call from JSON content (for streaming).
///
/// Looks for {"name": "tool_name", "arguments": {...}} format.
pub fn parse_tool_call_json(content: &str) -> Result<(String, Value)> {
    let content = content.trim();

    // Try to find JSON object
    let start = content.find('{')
        .ok_or_else(|| crate::error::invalid_input("No JSON object found"))?;

    let end = content.rfind('}')
        .ok_or_else(|| crate::error::invalid_input("No JSON object end found"))?;

    let json_str = &content[start..=end];

    let value: Value = serde_json::from_str(json_str)
        .map_err(|e| crate::error::invalid_input(format!("Invalid JSON: {}", e)))?;

    let tool_name = value.get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| crate::error::invalid_input("Missing 'name' field"))?
        .to_string();

    let arguments = value.get("arguments")
        .cloned()
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));

    Ok((tool_name, arguments))
}

/// Remove tool call markers from response for memory storage.
pub fn remove_tool_calls_from_response(response: &str) -> String {
    let mut result = response.to_string();

    // Remove <tool_calls>...</tool_calls> blocks
    while let Some(start) = result.find("<tool_calls>") {
        if let Some(end) = result.find("</tool_calls>") {
            result.replace_range(start..=end + 11, "");
        } else {
            break;
        }
    }

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tool_calls_with_json() {
        let text = "I'll help you with that. {\"tool\": \"list_devices\", \"arguments\": {}}";
        let (content, calls) = parse_tool_calls(text).unwrap();

        assert_eq!(content.trim(), "I'll help you with that.");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "list_devices");
    }

    #[test]
    fn test_parse_tool_calls_no_json() {
        let text = "Hello, how can I help you today?";
        let (content, calls) = parse_tool_calls(text).unwrap();

        assert_eq!(content, text);
        assert_eq!(calls.len(), 0);
    }

    #[test]
    fn test_parse_tool_call_json() {
        let json = r#"{"name": "query_data", "arguments": {"device_id": "sensor1"}}"#;
        let (name, args) = parse_tool_call_json(json).unwrap();

        assert_eq!(name, "query_data");
        assert_eq!(args["device_id"], "sensor1");
    }

    #[test]
    fn test_remove_tool_calls_from_response() {
        let response = "Here's the result <tool_calls>{\"tool\":\"test\"}</tool_calls> done.";
        let cleaned = remove_tool_calls_from_response(response);

        assert!(!cleaned.contains("<tool_calls>"));
        assert!(!cleaned.contains("</tool_calls>"));
        assert!(cleaned.contains("done"));
    }

    #[test]
    fn test_remove_nested_tool_calls() {
        let response = "Text <tool_calls>{\"a\":1}</tool_calls> more <tool_calls>{\"b\":2}</tool_calls> end";
        let cleaned = remove_tool_calls_from_response(response);

        assert!(!cleaned.contains("<tool_calls>"));
        assert!(cleaned.contains("Text"));
        assert!(cleaned.contains("more"));
        assert!(cleaned.contains("end"));
    }
}
