//! Tool call parser for extracting tool calls from LLM responses.

use serde_json::Value;
use uuid::Uuid;

use super::types::ToolCall;
use crate::error::{AgentError, Result};

/// Parse tool calls from LLM response text.
///
/// Supports both XML format (<tool_calls><invoke name="...">)
/// and JSON format ({"tool": "...", "arguments": {...}}).
/// Returns the remaining text along with any parsed tool calls.
pub fn parse_tool_calls(text: &str) -> Result<(String, Vec<ToolCall>)> {
    let mut content = text.to_string();
    let mut tool_calls = Vec::new();

    // First, try to parse XML format: <tool_calls><invoke name="tool_name">...</invoke></tool_calls>
    if let Some(start) = text.find("<tool_calls>") {
        if let Some(end) = text.find("</tool_calls>") {
            let tool_calls_str = &text[start..end + 13];
            content = text[..start].trim().to_string();

            // Find all <invoke> tags within <tool_calls>
            let mut remaining = tool_calls_str;
            while let Some(invoke_start) = remaining.find("<invoke") {
                // Extract the tool name from <invoke name="...">
                if let Some(name_start) = remaining[invoke_start..].find("name=\"") {
                    let name_start = invoke_start + name_start + 6; // 6 = len("name=\"")
                    let name_part = &remaining[name_start..];
                    if let Some(name_end) = name_part.find('"') {
                        let tool_name = &name_part[..name_end];

                        // Extract parameters from <parameter name="..." value="..."/>
                        let mut arguments = serde_json::Map::new();

                        // Find parameters after the invoke tag
                        let after_invoke = &remaining[name_start + name_end..];
                        let mut param_search = after_invoke;

                        while let Some(param_start) = param_search.find("<parameter") {
                            let param_section = &param_search[param_start..];

                            // Extract parameter name
                            let param_name = if let Some(n_start) = param_section.find("name=\"") {
                                let n_start = n_start + 6;
                                let n_part = &param_section[n_start..];
                                if let Some(n_end) = n_part.find('"') {
                                    &n_part[..n_end]
                                } else {
                                    ""
                                }
                            } else {
                                ""
                            };

                            // Extract parameter value
                            let param_value = if let Some(v_start) = param_section.find("value=\"") {
                                let v_start = v_start + 7;
                                let v_part = &param_section[v_start..];
                                if let Some(v_end) = v_part.find('"') {
                                    &v_part[..v_end]
                                } else {
                                    ""
                                }
                            } else if let Some(v_start) = param_section.find(">") {
                                // Try content between tags: <parameter name="x">value</parameter>
                                let v_start = v_start + 1;
                                let v_part = &param_section[v_start..];
                                if let Some(v_end) = v_part.find("</parameter>") {
                                    &v_part[..v_end]
                                } else {
                                    ""
                                }
                            } else {
                                ""
                            };

                            if !param_name.is_empty() {
                                // Try to parse as JSON value, otherwise use string
                                if let Ok(json_val) = serde_json::from_str::<Value>(param_value) {
                                    arguments.insert(param_name.to_string(), json_val);
                                } else {
                                    arguments.insert(param_name.to_string(), Value::String(param_value.to_string()));
                                }
                            }

                            // Move past this parameter tag
                            if let Some(tag_end) = param_search[param_start..].find(">") {
                                param_search = &param_search[param_start + tag_end + 1..];
                            } else {
                                break;
                            }
                        }

                        let call = ToolCall {
                            name: tool_name.to_string(),
                            id: Uuid::new_v4().to_string(),
                            arguments: Value::Object(arguments),
                            result: None,  // Will be populated after execution
                        };
                        tool_calls.push(call);
                    }
                }

                // Move past this invoke tag
                if let Some(invoke_end) = remaining.find("</invoke>") {
                    remaining = &remaining[invoke_end + 9..]; // 9 = len("</invoke>")
                } else {
                    break;
                }
            }

            return Ok((content, tool_calls));
        }
    }

    // Fallback: Try to parse JSON format
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
                        result: None,  // Will be populated after execution
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

    #[test]
    fn test_parse_xml_tool_calls_empty() {
        let text = "I'll list the devices for you. <tool_calls>\n<invoke name=\"list_devices\">\n</invoke>\n</tool_calls>";
        let (content, calls) = parse_tool_calls(text).unwrap();

        assert_eq!(content.trim(), "I'll list the devices for you.");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "list_devices");
        assert!(calls[0].arguments.as_object().unwrap().is_empty());
    }

    #[test]
    fn test_parse_xml_tool_calls_with_params() {
        let text = "<tool_calls>\n<invoke name=\"query_data\">\n<parameter name=\"device_id\" value=\"sensor1\"/>\n</invoke>\n</tool_calls>";
        let (content, calls) = parse_tool_calls(text).unwrap();

        assert!(content.trim().is_empty() || content.trim() == "I'll help you.");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "query_data");
        assert_eq!(calls[0].arguments["device_id"], "sensor1");
    }

    #[test]
    fn test_parse_xml_tool_calls_multiple() {
        let text = "<tool_calls>\n<invoke name=\"list_devices\">\n</invoke>\n<invoke name=\"list_rules\">\n</invoke>\n</tool_calls>";
        let (content, calls) = parse_tool_calls(text).unwrap();

        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "list_devices");
        assert_eq!(calls[1].name, "list_rules");
    }
}
