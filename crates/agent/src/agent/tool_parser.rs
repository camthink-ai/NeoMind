//! Tool call parser for extracting tool calls from LLM responses.

use serde_json::Value;
use uuid::Uuid;

use super::types::ToolCall;
use crate::error::{AgentError, Result};

/// Parse tool calls from LLM response text.
///
/// **Supported formats**:
/// 1. JSON array format: [{"name": "tool1", "arguments": {...}}, {"name": "tool2", "arguments": {...}}]
/// 2. JSON object format: {"name": "tool_name", "arguments": {...}}
/// 3. XML format: <tool_calls><invoke name="tool_name"><parameter name="key" value="val"/></invoke></tool_calls>
///
/// Returns the remaining text along with any parsed tool calls.
pub fn parse_tool_calls(text: &str) -> Result<(String, Vec<ToolCall>)> {
    let mut content = text.to_string();
    let mut tool_calls = Vec::new();

    // First, try to parse XML format: <tool_calls><invoke name="tool_name">...</invoke></tool_calls>
    if let Some(start) = text.find("<tool_calls>") {
        if let Some(end) = text.find("</tool_calls>") {
            let xml_section = &text[start..end + 13]; // 13 = len("</tool_calls>")
            content = format!("{}{}", &text[..start], &text[end + 13..]);

            // Parse <invoke name="..."> entries
            let mut remaining = xml_section;
            while let Some(invoke_start) = remaining.find("<invoke") {
                let invoke_end = match remaining.find("</invoke>") {
                    Some(pos) => pos,
                    None => break,
                };

                let invoke_section = &remaining[invoke_start..invoke_end + 8]; // 8 = len("</invoke>")

                // Extract tool name from <invoke name="tool_name">
                if let Some(name_start) = invoke_section.find("name=\"") {
                    let name_section = &invoke_section[name_start + 6..];
                    if let Some(name_end) = name_section.find('"') {
                        let tool_name = &name_section[..name_end];

                        // Extract parameters from <parameter name="key" value="value"/>
                        let mut arguments = serde_json::Map::new();
                        let mut param_remaining = invoke_section;
                        while let Some(param_start) = param_remaining.find("<parameter") {
                            let param_end = match param_remaining.find("/>") {
                                Some(pos) => pos,
                                None => break,
                            };
                            let param_section = &param_remaining[param_start..param_end + 2];

                            // Extract parameter name
                            let param_name = if let Some(pn_start) = param_section.find("name=\"") {
                                let pn_section = &param_section[pn_start + 6..];
                                if let Some(pn_end) = pn_section.find('"') {
                                    pn_section[..pn_end].to_string()
                                } else {
                                    continue;
                                }
                            } else {
                                continue;
                            };

                            // Extract parameter value
                            let param_value = if let Some(pv_start) = param_section.find("value=\"") {
                                let pv_section = &param_section[pv_start + 7..];
                                if let Some(pv_end) = pv_section.find('"') {
                                    pv_section[..pv_end].to_string()
                                } else {
                                    continue;
                                }
                            } else {
                                continue;
                            };

                            arguments.insert(param_name, Value::String(param_value));
                            param_remaining = &param_remaining[param_end + 2..];
                        }

                        tool_calls.push(ToolCall {
                            name: tool_name.to_string(),
                            id: Uuid::new_v4().to_string(),
                            arguments: Value::Object(arguments),
                            result: None,
                        });
                    }
                }

                remaining = &remaining[invoke_end + 8..];
            }

            if !tool_calls.is_empty() {
                return Ok((content.trim().to_string(), tool_calls));
            }
        }
    }

    // Second, try to parse JSON array format: [{"name": "tool1", "arguments": {...}}, ...]
    if let Some(start) = text.find('[') {
        // Find the matching closing bracket by counting brackets
        let mut bracket_count = 0;
        let mut array_end = start;

        for (i, c) in text[start..].char_indices() {
            if c == '[' {
                bracket_count += 1;
            } else if c == ']' {
                bracket_count -= 1;
                if bracket_count == 0 {
                    array_end = start + i + 1;
                    break;
                }
            }
        }

        if array_end > start {
            let json_str = &text[start..array_end];
            if let Ok(array) = serde_json::from_str::<Vec<Value>>(json_str) {
                content = text[..start].trim().to_string();

                for value in array {
                    if let Some(tool_name) = value
                        .get("name")
                        .or_else(|| value.get("tool"))
                        .or_else(|| value.get("function"))
                        .and_then(|v| v.as_str())
                    {
                        let arguments = value
                            .get("arguments")
                            .or_else(|| value.get("params"))
                            .or_else(|| value.get("parameters"))
                            .cloned()
                            .unwrap_or_else(|| {
                                let mut args = serde_json::Map::new();
                                if let Some(obj) = value.as_object() {
                                    for (k, v) in obj {
                                        if k != "name" && k != "tool" && k != "function"
                                            && k != "arguments" && k != "params" && k != "parameters" {
                                            args.insert(k.clone(), v.clone());
                                        }
                                    }
                                }
                                Value::Object(args)
                            });

                        tool_calls.push(ToolCall {
                            name: tool_name.to_string(),
                            id: Uuid::new_v4().to_string(),
                            arguments,
                            result: None,
                        });
                    }
                }

                if !tool_calls.is_empty() {
                    return Ok((content, tool_calls));
                }
            }
        }
    }

    // Second, try to parse JSON object format: {"name": "tool_name", "arguments": {...}}
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
                let tool_name = value
                    .get("tool")
                    .or_else(|| value.get("function"))
                    .or_else(|| value.get("name"))
                    .and_then(|v| v.as_str());

                if let Some(name) = tool_name {
                    // Extract arguments
                    let arguments = value
                        .get("arguments")
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
                        result: None, // Will be populated after execution
                    };
                    tool_calls.push(call);
                    content = text[..match_start].trim().to_string();
                    return Ok((content, tool_calls));
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
    let start = content
        .find('{')
        .ok_or_else(|| crate::error::invalid_input("No JSON object found"))?;

    let end = content
        .rfind('}')
        .ok_or_else(|| crate::error::invalid_input("No JSON object end found"))?;

    let json_str = &content[start..=end];

    let value: Value = serde_json::from_str(json_str)
        .map_err(|e| crate::error::invalid_input(format!("Invalid JSON: {}", e)))?;

    let tool_name = value
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| crate::error::invalid_input("Missing 'name' field"))?
        .to_string();

    let arguments = value
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));

    Ok((tool_name, arguments))
}

/// Remove tool call markers from response for memory storage.
pub fn remove_tool_calls_from_response(response: &str) -> String {
    let mut result = response.to_string();

    // Remove XML format: <tool_calls>...</tool_calls>
    while let Some(start) = result.find("<tool_calls>") {
        if let Some(end) = result.find("</tool_calls>") {
            result.replace_range(start..end + 13, "");
            continue;
        }
        break;
    }

    // Remove JSON array format: [{...}, {...}]
    while let Some(start) = result.find('[') {
        let mut bracket_count = 0;
        let mut end = start;

        for (i, c) in result[start..].char_indices() {
            if c == '[' {
                bracket_count += 1;
            } else if c == ']' {
                bracket_count -= 1;
                if bracket_count == 0 {
                    end = start + i + 1;
                    break;
                }
            }
        }

        if end > start {
            // Check if it's a tool call array
            let json_str = &result[start..end];
            if let Ok(array) = serde_json::from_str::<Vec<Value>>(json_str) {
                if array.iter().any(|v| v.get("name").is_some() || v.get("tool").is_some()) {
                    result.replace_range(start..end, "");
                    continue;
                }
            }
        }
        break;
    }

    // Remove JSON object format: {"name": "tool_name", ...}
    while let Some(start) = result.find('{') {
        let mut brace_count = 0;
        let mut end = start;

        for (i, c) in result[start..].char_indices() {
            if c == '{' {
                brace_count += 1;
            } else if c == '}' {
                brace_count -= 1;
                if brace_count == 0 {
                    end = start + i + 1;
                    break;
                }
            }
        }

        if end > start {
            let json_str = &result[start..end];
            if let Ok(value) = serde_json::from_str::<Value>(json_str) {
                if value.get("name").is_some() || value.get("tool").is_some() {
                    result.replace_range(start..end, "");
                    continue;
                }
            }
        }
        break;
    }

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_json_array_tool_calls() {
        let text = "我来查询。 [{\"name\": \"list_devices\", \"arguments\": {}}, {\"name\": \"list_rules\", \"arguments\": {}}]";
        let (content, calls) = parse_tool_calls(text).unwrap();

        assert_eq!(content.trim(), "我来查询。");
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "list_devices");
        assert_eq!(calls[1].name, "list_rules");
    }

    #[test]
    fn test_parse_json_object_tool_call() {
        let text = "I'll help. {\"name\": \"list_devices\", \"arguments\": {}}";
        let (content, calls) = parse_tool_calls(text).unwrap();

        assert_eq!(content.trim(), "I'll help.");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "list_devices");
    }

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
    fn test_remove_json_array_tool_calls() {
        let response = "Here's the result [{\"name\":\"test\"}] done.";
        let cleaned = remove_tool_calls_from_response(response);

        assert!(cleaned.contains("Here's the result"));
        assert!(cleaned.contains("done"));
    }

    #[test]
    fn test_remove_json_object_tool_calls() {
        let response = "Here's the result {\"name\":\"test\"} done.";
        let cleaned = remove_tool_calls_from_response(response);

        assert!(cleaned.contains("Here's the result"));
        assert!(cleaned.contains("done"));
    }

    #[test]
    fn test_parse_tool_calls_with_arguments() {
        let text = r#"{"name": "query_data", "arguments": {"device_id": "sensor1", "metric": "temperature"}}"#;
        let (content, calls) = parse_tool_calls(text).unwrap();

        assert!(content.trim().is_empty());
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "query_data");
        assert_eq!(calls[0].arguments["device_id"], "sensor1");
        assert_eq!(calls[0].arguments["metric"], "temperature");
    }

    #[test]
    fn test_parse_multiple_tool_calls() {
        let text = r#"[{"name": "list_devices", "arguments": {}}, {"name": "list_rules", "arguments": {}}]"#;
        let (content, calls) = parse_tool_calls(text).unwrap();

        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "list_devices");
        assert_eq!(calls[1].name, "list_rules");
    }
}
