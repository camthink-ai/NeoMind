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

/// Collect all syntactically balanced top-level `{ ... }` JSON objects in `s`,
/// ignoring any stray trailing characters (such as model typos like `}}"}}`).
fn extract_all_balanced_json_objects(s: &str) -> Vec<Value> {
    let bytes = s.as_bytes();
    let mut results = Vec::new();
    let mut start = 0usize;
    while start < bytes.len() {
        let Some(rel) = s[start..].find('{') else {
            break;
        };
        let open_idx = start + rel;
        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;
        let mut i = open_idx;
        let mut matched_end = None;
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
                                    results.push(unwrap_tool_call_envelope(v));
                                    matched_end = Some(i + 1);
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
        start = matched_end.unwrap_or(open_idx + 1);
    }
    results
}

/// Collect all JSON objects from fenced ```json blocks in `s`.
fn extract_fenced_json_objects(s: &str) -> Vec<Value> {
    let mut results = Vec::new();
    let mut rest = s;
    while let Some(start) = rest.find("```") {
        let after = &rest[start + 3..];
        let after = after
            .strip_prefix("json")
            .or_else(|| after.strip_prefix("JSON"))
            .unwrap_or(after)
            .trim_start();
        let Some(end) = after.find("```") else {
            break;
        };
        let body = after[..end].trim();
        if let Ok(v) = serde_json::from_str::<Value>(body) {
            if v.is_object() {
                results.push(unwrap_tool_call_envelope(v));
            } else if let Some(arr) = v.as_array() {
                for item in arr {
                    if item.is_object() {
                        results.push(unwrap_tool_call_envelope(item.clone()));
                    }
                }
            }
        } else {
            results.extend(extract_all_balanced_json_objects(body));
        }
        rest = &after[end + 3..];
    }
    results
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

/// Pull the best JSON object from model text for the declared `fields`.
///
/// Prefers the candidate object that matches the most declared output field names
/// (breaking ties by choosing the **last** object, matching the system prompt's
/// "Put the JSON object last" instruction).
pub fn extract_json_for_fields(text: &str, fields: &[OutputField]) -> Option<Value> {
    let stripped = strip_tool_call_tags(text);
    let trimmed = stripped.trim();

    let mut candidates: Vec<Value> = Vec::new();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        if v.is_object() {
            candidates.push(unwrap_tool_call_envelope(v));
        } else if let Some(arr) = v.as_array() {
            for item in arr {
                if item.is_object() {
                    candidates.push(unwrap_tool_call_envelope(item.clone()));
                }
            }
        }
    }
    candidates.extend(extract_fenced_json_objects(trimmed));
    candidates.extend(extract_all_balanced_json_objects(trimmed));

    if candidates.is_empty() {
        if let Some(i) = trimmed.find('{') {
            if let Some(j) = trimmed.rfind('}') {
                if j > i {
                    if let Ok(v) = serde_json::from_str::<Value>(&trimmed[i..=j]) {
                        if v.is_object() {
                            candidates.push(unwrap_tool_call_envelope(v));
                        }
                    }
                }
            }
        }
    }

    if candidates.is_empty() {
        return None;
    }
    if fields.is_empty() {
        return candidates.into_iter().last();
    }

    // Pick candidate with highest number of matched declared fields; ties go to later objects.
    let mut best_idx = candidates.len() - 1;
    let mut best_score = 0usize;
    for (idx, cand) in candidates.iter().enumerate() {
        let score = count_matching_fields(cand, fields);
        if score >= best_score && score > 0 {
            best_score = score;
            best_idx = idx;
        }
    }
    Some(candidates.swap_remove(best_idx))
}

fn count_matching_fields(val: &Value, fields: &[OutputField]) -> usize {
    let Some(obj) = val.as_object() else {
        return 0;
    };
    fields
        .iter()
        .filter(|f| {
            obj.contains_key(&f.name)
                || obj.keys().any(|k| k.eq_ignore_ascii_case(&f.name))
        })
        .count()
}

/// Pull a JSON object from model text (raw object, fenced ```json block, or
/// hallucinated `<tool_call>{"name": ..., "arguments": {...}}</tool_call>`).
pub fn extract_json_object(text: &str) -> Option<Value> {
    extract_json_for_fields(text, &[])
}

/// Clean a raw model response for human-readable `conclusion` / `brief_text`.
///
/// If the model emitted a `<tool_call>...</tool_call>` wrapper, extract its
/// human-readable summary/details instead of surfacing raw XML tool-call syntax
/// to the user.
pub fn clean_model_response(text: &str, structured: Option<&Value>) -> String {
    let trimmed = text.trim();
    let had_tool_call_tag = trimmed.contains("<tool_call>")
        || trimmed.contains("</tool_call>")
        || trimmed.contains("<|tool_call|>");
    if !had_tool_call_tag {
        return text.to_string();
    }

    let extracted = extract_json_object(trimmed).or_else(|| structured.cloned());
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
        if let Some(v) = obj.get(&f.name).or_else(|| {
            obj.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(&f.name))
                .map(|(_, v)| v)
        }) {
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

    #[test]
    fn extracts_json_followed_by_verdict_text() {
        let raw_output = r#"{ "status": "正常", "summary": "高空俯瞰画面显示大量人群活动。", "analysis": [ { "key": "人员", "details": "正常" } ], "recommendations": [] } Verdict: 画面正常，人群活动常规，无异常。"#;
        let fields = vec![
            OutputField {
                name: "status".into(),
                depends_on: vec![],
                description: None,
            },
            OutputField {
                name: "summary".into(),
                depends_on: vec![],
                description: None,
            },
        ];
        let extracted = extract_json_for_fields(raw_output, &fields).expect("should parse json");
        let projected = project_output_fields(&extracted, &fields);
        assert_eq!(projected["status"], "正常");
        assert_eq!(projected["summary"], "高空俯瞰画面显示大量人群活动。");
    }
}
