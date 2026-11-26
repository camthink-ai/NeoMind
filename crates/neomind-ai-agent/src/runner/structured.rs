//! Extract structured JSON fields from model text for [`OutputContract`].

use serde_json::{Map, Value};

use crate::model::OutputField;

/// Pull a JSON object from model text (raw object or fenced ```json block).
pub fn extract_json_object(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        if v.is_object() {
            return Some(v);
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
                    return Some(v);
                }
            }
        }
    }
    // First {...} slice
    if let Some(i) = trimmed.find('{') {
        if let Some(j) = trimmed.rfind('}') {
            if j > i {
                if let Ok(v) = serde_json::from_str::<Value>(&trimmed[i..=j]) {
                    if v.is_object() {
                        return Some(v);
                    }
                }
            }
        }
    }
    None
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
        "\nAlso respond with exactly ONE JSON object (no markdown fence required) containing:\n",
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
}
