//! Extract structured JSON fields from model text for [`OutputContract`].

use serde_json::{Map, Value};

use crate::model::OutputField;

/// Strip hallucinated `<tool_call>...</tool_call>` XML tags (common with Qwen-family
/// local models when asked to produce structured JSON in a zero-tool prompt).
fn strip_tool_call_tags(text: &str) -> String {
    text.replace("<tool_call>", "")
        .replace("</tool_call>", "")
        .replace("<|tool_call|>", "")
        .replace("<|/tool_call|>", "")
        .trim()
        .to_string()
}

/// Find the first syntactically balanced `{ ... }` JSON object slice in `s`,
/// ignoring any stray trailing characters (such as model typos like `}}"}}`).
fn extract_balanced_json_object(s: &str) -> Option<Value> {
    let bytes = s.as_bytes();
    let mut start = 0usize;
    while let Some(rel) = s[start..].find('{') {
        let open_idx = start + rel;
        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;
        let mut i = open_idx;
        while i < bytes.len() {
            let b = bytes[i];
            if in_string {
                if escaped {
                    escaped = false;
                } else if b == b'\\' {
                    escaped = true;
                } else if b == b'"' {
                    in_string = false;
                }
            } else {
                match b {
                    b'"' => in_string = true,
                    b'{' => depth += 1,
                    b'}' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            let candidate = &s[open_idx..=i];
                            if let Ok(v) = serde_json::from_str::<Value>(candidate) {
                                if v.is_object() {
                                    return Some(v);
                                }
                            }
                            break;
                        }
                    }
                    _ => {}
                }
            }
            i += 1;
        }
        start = open_idx + 1;
    }
    None
}

/// If a model wrapped its output in a fake tool-call envelope such as
/// `{"name": "reactor", "arguments": {"status": "normal", ...}}`, unwrap `arguments`.
fn unwrap_tool_call_envelope(v: Value) -> Value {
    let Some(obj) = v.as_object() else {
        return v;
    };
    if obj.contains_key("name") && obj.len() <= 3 {
        if let Some(args) = obj.get("arguments").or_else(|| obj.get("parameters")) {
            if args.is_object() {
                return args.clone();
            }
            if let Some(s) = args.as_str() {
                if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                    if parsed.is_object() {
                        return parsed;
                    }
                }
            }
        }
    }
    v
}

/// Pull a JSON object from model text (raw object, fenced ```json block, or
/// hallucinated `<tool_call>{"name": ..., "arguments": {...}}</tool_call>`).
pub fn extract_json_object(text: &str) -> Option<Value> {
    let stripped = strip_tool_call_tags(text);
    let trimmed = stripped.trim();

    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        if v.is_object() {
            return Some(unwrap_tool_call_envelope(v));
        }
    }
    // Fenced block
    if let Some(start) = trimmed.find("```") {
        let after = &trimmed[start + 3..];
        let after = after
            .strip_prefix("json")
            .or_else(|| after.strip_prefix("JSON"))
            .unwrap_or(after)
            .trim_start();
        if let Some(end) = after.find("```") {
            let body = after[..end].trim();
            if let Ok(v) = serde_json::from_str::<Value>(body) {
                if v.is_object() {
                    return Some(unwrap_tool_call_envelope(v));
                }
            }
            if let Some(v) = extract_balanced_json_object(body) {
                return Some(unwrap_tool_call_envelope(v));
            }
        }
    }
    // Balanced {...} slice (resilient to stray trailing `"}}` typos)
    if let Some(v) = extract_balanced_json_object(trimmed) {
        return Some(unwrap_tool_call_envelope(v));
    }
    // Fallback: first '{' .. last '}'
    if let Some(i) = trimmed.find('{') {
        if let Some(j) = trimmed.rfind('}') {
            if j > i {
                if let Ok(v) = serde_json::from_str::<Value>(&trimmed[i..=j]) {
                    if v.is_object() {
                        return Some(unwrap_tool_call_envelope(v));
                    }
                }
            }
        }
    }
    None
}

/// Clean a raw model response for human-readable `conclusion` / `brief_text`.
///
/// If the model emitted a `<tool_call>...</tool_call>` wrapper or pure JSON
/// envelope, extract its human-readable summary/details instead of surfacing
/// raw XML tool-call syntax to the user.
pub fn clean_model_response(text: &str, structured: Option<&Value>) -> String {
    let trimmed = text.trim();
    let had_tool_call_tag = trimmed.contains("<tool_call>")
        || trimmed.contains("</tool_call>")
        || trimmed.contains("<|tool_call|>");
    if !had_tool_call_tag {
        return text.to_string();
    }

    let extracted = structured
        .cloned()
        .or_else(|| extract_json_object(trimmed));
    if let Some(Value::Object(map)) = extracted {
        let mut parts: Vec<String> = Vec::new();
        for key in ["summary", "conclusion", "details", "analysis", "message", "description"] {
            if let Some(val) = map.get(key).and_then(|v| v.as_str()) {
                let s = val.trim();
                if !s.is_empty() && !parts.iter().any(|p| p == s) {
                    parts.push(s.to_string());
                }
            }
        }
        if !parts.is_empty() {
            return parts.join("\n\n");
        }
        let kv: Vec<String> = map
            .iter()
            .map(|(k, v)| match v {
                Value::String(s) => format!("{k}: {s}"),
                other => format!("{k}: {other}"),
            })
            .collect();
        if !kv.is_empty() {
            return kv.join("\n");
        }
    }

    strip_tool_call_tags(trimmed)
}

/// Keep only fields named in the output contract (plus optional confidence).
pub fn project_output_fields(raw: &Value, fields: &[OutputField]) -> Value {
    let Some(obj) = raw.as_object() else {
        return Value::Object(Map::new());
    };
    let mut out = Map::new();
    for f in fields {
        if let Some(v) = obj.get(&f.name) {
            out.insert(f.name.clone(), v.clone());
        }
    }
    if let Some(c) = obj.get("confidence") {
        out.insert("confidence".into(), c.clone());
    }
    Value::Object(out)
}

/// System-prompt appendix when the agent declares output fields.
pub fn structured_system_addon(fields: &[OutputField]) -> String {
    if fields.is_empty() {
        return String::new();
    }
    let mut lines = String::from(
        "\nAlso respond with exactly ONE plain JSON object (no <tool_call> tags, no function-call wrapper, no markdown fence required) containing these top-level keys:\n",
    );
    for f in fields {
        lines.push_str(&format!("- \"{}\"", f.name));
        if let Some(d) = &f.description {
            lines.push_str(&format!(" — {d}"));
        }
        lines.push('\n');
    }
    lines.push_str(
        "You may include \"confidence\" (0..1). Put the JSON object last so it can be parsed.\n",
    );
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::OutputField;

    #[test]
    fn extracts_raw_object() {
        let v = extract_json_object(r#"{"temp": 21.5, "ok": true}"#).unwrap();
        assert_eq!(v["temp"], 21.5);
    }

    #[test]
    fn extracts_fenced_and_projects() {
        let text = "Analysis done.\n```json\n{\"temp\": 18, \"extra\": 1}\n```\n";
        let raw = extract_json_object(text).unwrap();
        let projected = project_output_fields(
            &raw,
            &[OutputField {
                name: "temp".into(),
                depends_on: vec![],
                description: None,
            }],
        );
        assert_eq!(projected["temp"], 18);
        assert!(projected.get("extra").is_none());
    }

    #[test]
    fn unwraps_malformed_tool_call_envelope_and_cleans_text() {
        let raw_output = r#"<tool_call>{"name": "reactor", "arguments": {"status": "normal", "summary": "高空俯瞰画面显示大量人群在大型建筑广场活动。", "details": "地面为浅色石材地砖，无异常。"}}"}}</tool_call>"#;
        let extracted = extract_json_object(raw_output).expect("should unwrap tool_call arguments");
        assert_eq!(extracted["status"], "normal");
        assert_eq!(
            extracted["summary"],
            "高空俯瞰画面显示大量人群在大型建筑广场活动。"
        );

        let cleaned = clean_model_response(raw_output, Some(&extracted));
        assert!(!cleaned.contains("<tool_call>"));
        assert!(!cleaned.contains("reactor"));
        assert!(cleaned.contains("高空俯瞰画面显示大量人群在大型建筑广场活动。"));
        assert!(cleaned.contains("地面为浅色石材地砖，无异常。"));
    }
}
