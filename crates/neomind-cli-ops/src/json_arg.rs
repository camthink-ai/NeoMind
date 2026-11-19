//! Quote-tolerant JSON parsing for CLI string flags (`--set`, `--params`, …).
//!
//! Models often leave wrapping quotes or a trailing `'`/`"` after shell-style
//! quoting. Strict `serde_json::from_str` then fails with
//! `trailing characters at line 1 column N`. Normalize once here so every
//! domain handler shares the same contract.

use anyhow::{anyhow, Result};
use serde_json::Value;

/// Parse a CLI JSON argument with common quote-wrapping tolerance.
///
/// Accepts already-clean JSON, one or two layers of matching outer
/// `'…'` / `"…"`, and a leftover unpaired trailing/leading quote that
/// produces serde's "trailing characters" error.
pub fn parse_json_arg(raw: &str) -> Result<Value> {
    let normalized = normalize_json_arg(raw);
    match serde_json::from_str(&normalized) {
        Ok(v) => Ok(v),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("trailing characters") || msg.contains("expected") {
                let retried = strip_unpaired_edge_quotes(&normalized);
                if retried != normalized {
                    return serde_json::from_str(&retried).map_err(|e2| {
                        anyhow!("invalid JSON after quote normalize: {e2} (original: {e})")
                    });
                }
            }
            Err(anyhow!("invalid JSON: {e}"))
        }
    }
}

/// Same as [`parse_json_arg`] but returns `String` errors (capability / classify).
pub fn parse_json_arg_str(raw: &str) -> std::result::Result<Value, String> {
    parse_json_arg(raw).map_err(|e| e.to_string())
}

fn normalize_json_arg(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    // Smart / typographic quotes → ASCII.
    s = s
        .replace(['\u{2018}', '\u{2019}', '\u{201A}', '\u{201B}'], "'")
        .replace(['\u{201C}', '\u{201D}', '\u{201E}', '\u{201F}'], "\"");
    for _ in 0..2 {
        let next = strip_matching_outer_quotes(&s);
        if next == s {
            break;
        }
        s = next;
    }
    s.trim().to_string()
}

fn strip_matching_outer_quotes(s: &str) -> String {
    let bytes = s.as_bytes();
    if bytes.len() < 2 {
        return s.to_string();
    }
    let (first, last) = (bytes[0], bytes[bytes.len() - 1]);
    if (first == b'\'' && last == b'\'') || (first == b'"' && last == b'"') {
        return s[1..s.len() - 1].to_string();
    }
    s.to_string()
}

fn strip_unpaired_edge_quotes(s: &str) -> String {
    let mut out = s.trim().to_string();
    while out.ends_with('\'') || out.ends_with('"') || out.ends_with('`') {
        out.pop();
        out = out.trim_end().to_string();
    }
    while out.starts_with('\'') || out.starts_with('"') || out.starts_with('`') {
        out.remove(0);
        out = out.trim_start().to_string();
    }
    out
}

/// Which primary JSON flag a `neomind …` argv expects for structured shell
/// `params` injection. Multi-flag create/update commands return the most
/// common primary payload flag; callers still XOR against an existing flag.
pub fn primary_json_flag(argv: &[String]) -> Option<&'static str> {
    if argv.len() < 3 || argv[0] != "neomind" {
        return None;
    }
    let domain = argv[1].as_str();
    let action = argv[2].as_str();
    match (domain, action) {
        ("extension", "invoke") | ("device", "control") => Some("--params"),
        ("extension", "config") | ("dashboard", "update-component") => Some("--set"),
        ("dashboard", "add-components") => Some("--components"),
        ("dashboard", "remove-components") => Some("--ids"),
        ("dashboard", "create") | ("dashboard", "update") => Some("--components"),
        ("rule", "create") | ("rule", "update") => Some("--body"),
        ("transform", "create")
        | ("transform", "test")
        | ("transform", "test-code")
        | ("agent", "invoke") => Some("--input"),
        ("push", "create") | ("push", "update") | ("message", "channel-create") => Some("--config"),
        ("connector", "create") | ("connector", "update") => Some("--config"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn clean_object_parses() {
        let v = parse_json_arg(r#"{"position":{"w":2,"h":2}}"#).unwrap();
        assert_eq!(v["position"]["w"], 2);
    }

    #[test]
    fn wrapping_single_quotes_stripped() {
        let v = parse_json_arg("'{\"position\":{\"w\":2,\"h\":2}}'").unwrap();
        assert_eq!(v, json!({"position":{"w":2,"h":2}}));
    }

    #[test]
    fn trailing_single_quote_tolerated() {
        // Live failure class: tokenizer left a dangling `'`.
        let v = parse_json_arg(r#"{"position":{"x":0,"y":12,"w":2,"h":2}}'"#).unwrap();
        assert_eq!(v["position"]["h"], 2);
    }

    #[test]
    fn wrapping_double_quotes_stripped() {
        let v = parse_json_arg(r#""{"a":1}""#).unwrap();
        assert_eq!(v, json!({"a": 1}));
    }

    #[test]
    fn smart_quotes_normalized() {
        let raw = "\u{201C}{\"a\":1}\u{201D}";
        let v = parse_json_arg(raw).unwrap();
        assert_eq!(v, json!({"a": 1}));
    }

    #[test]
    fn primary_json_flag_covers_hot_paths() {
        let dash = vec![
            "neomind".into(),
            "dashboard".into(),
            "update-component".into(),
            "d1".into(),
        ];
        assert_eq!(primary_json_flag(&dash), Some("--set"));
        let device = vec![
            "neomind".into(),
            "device".into(),
            "control".into(),
            "d1".into(),
            "reboot".into(),
        ];
        assert_eq!(primary_json_flag(&device), Some("--params"));
        let ext = vec![
            "neomind".into(),
            "extension".into(),
            "invoke".into(),
            "weather".into(),
            "get_weather".into(),
        ];
        assert_eq!(primary_json_flag(&ext), Some("--params"));
    }
}
