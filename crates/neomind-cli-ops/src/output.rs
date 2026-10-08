use crate::types::{CliResponse, OutputFormat};

/// Render a response to a string. PURE — callers print the return value.
/// (The old version printed AND returned; in-process callers like the tauri
/// pre-dispatch then printed it again, doubling every command's output.)
pub fn format_output(response: &CliResponse, format: OutputFormat) -> String {
    match format {
        OutputFormat::Json => serde_json::to_string_pretty(response).unwrap_or_default(),
        OutputFormat::Human => format_human(response),
    }
}

fn format_human(resp: &CliResponse) -> String {
    if resp.success {
        let mut out = String::new();
        if let Some(msg) = &resp.message {
            out.push_str(&format!("✅ {}\n", msg));
        }
        if let Some(data) = &resp.data {
            format_value(data, &mut out, 0);
        }
        out
    } else {
        let mut out = format!("❌ {}\n", resp.error.as_deref().unwrap_or("Unknown error"));
        if let Some(suggestion) = &resp.suggestion {
            out.push_str(&format!("💡 Suggestion: {}\n", suggestion));
        }
        out
    }
}

/// Recursively format a JSON value for human-readable display.
fn format_value(value: &serde_json::Value, out: &mut String, indent: usize) {
    let prefix = "  ".repeat(indent);

    match value {
        serde_json::Value::Object(obj) => {
            for (key, val) in obj {
                match val {
                    serde_json::Value::String(s) => {
                        out.push_str(&format!("{}{}: {}\n", prefix, key, s));
                    }
                    serde_json::Value::Number(_) | serde_json::Value::Bool(_) => {
                        out.push_str(&format!("{}{}: {}\n", prefix, key, val));
                    }
                    serde_json::Value::Array(arr) if !arr.is_empty() && arr[0].is_object() => {
                        // Array of objects → table-like display
                        out.push_str(&format!("{}{}:\n", prefix, key));
                        for (i, item) in arr.iter().enumerate() {
                            if i > 0 {
                                out.push_str(&format!("{}  ---\n", prefix));
                            }
                            format_value(item, out, indent + 2);
                        }
                    }
                    serde_json::Value::Array(arr) if !arr.is_empty() => {
                        // Array of primitives → single line
                        let items: Vec<String> = arr.iter().map(|v| format!("{}", v)).collect();
                        out.push_str(&format!("{}{}: [{}]\n", prefix, key, items.join(", ")));
                    }
                    serde_json::Value::Object(_) => {
                        out.push_str(&format!("{}{}:\n", prefix, key));
                        format_value(val, out, indent + 1);
                    }
                    serde_json::Value::Null => {}
                    serde_json::Value::Array(_) => {
                        out.push_str(&format!("{}{}: []\n", prefix, key));
                    }
                }
            }
        }
        serde_json::Value::Array(arr) if !arr.is_empty() && arr[0].is_object() => {
            for (i, item) in arr.iter().enumerate() {
                if i > 0 {
                    out.push_str(&format!("{}---\n", prefix));
                }
                format_value(item, out, indent);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_format_output_json() {
        let response = CliResponse::success(json!({"id": "123", "name": "test"}), "Success");
        let output = format_output(&response, OutputFormat::Json);

        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["message"], "Success");
        assert_eq!(parsed["data"]["id"], "123");
        assert_eq!(parsed["data"]["name"], "test");
    }

    #[test]
    fn test_format_output_human_success() {
        let response =
            CliResponse::success(json!({"id": "123", "count": 42}), "Operation completed");
        let output = format_output(&response, OutputFormat::Human);

        assert!(output.contains("✅"));
        assert!(output.contains("Operation completed"));
        assert!(output.contains("id: 123"));
        assert!(output.contains("count: 42"));
    }

    #[test]
    fn test_format_output_human_error() {
        let response = CliResponse::error("Something failed", "ERR_001");
        let output = format_output(&response, OutputFormat::Human);

        assert!(output.contains("❌"));
        assert!(output.contains("Something failed"));
    }

    #[test]
    fn test_format_output_human_error_with_suggestion() {
        let response = CliResponse::error_with_suggestion(
            "Device not found",
            "NOT_FOUND",
            "Run 'neomind device list' to see available devices",
        );
        let output = format_output(&response, OutputFormat::Human);

        assert!(output.contains("❌"));
        assert!(output.contains("Device not found"));
        assert!(output.contains("Suggestion"));
        assert!(output.contains("neomind device list"));
    }

    #[test]
    fn test_format_output_human_no_data() {
        let response = CliResponse::success(json!(null), "Done");
        let output = format_output(&response, OutputFormat::Human);

        assert!(output.contains("✅"));
        assert!(output.contains("Done"));
    }

    #[test]
    fn test_format_output_human_with_boolean() {
        let response = CliResponse::success(json!({"enabled": true, "active": false}), "Status");
        let output = format_output(&response, OutputFormat::Human);

        assert!(output.contains("enabled: true"));
        assert!(output.contains("active: false"));
    }

    #[test]
    fn test_format_output_json_error() {
        let response = CliResponse::error("Error message", "E001");
        let output = format_output(&response, OutputFormat::Json);

        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["success"], false);
        assert_eq!(parsed["error"], "Error message");
        assert_eq!(parsed["code"], "E001");
    }
}
