//! Repeated `--param key=value` parsing shared by model-facing commands.
//!
//! Small LLMs generate flat `key=value` flags far more reliably than nested
//! JSON blobs in shell quotes (eval 2026-09-01: first-shot flag hallucination
//! was the top failure mode). Commands that take a JSON blob (`--params`,
//! `--config`) accept repeated `--param k=v` entries as an additive, easier
//! path; when both are given the JSON object is parsed first and `--param`
//! entries override it key by key.
//!
//! Value typing is inferred with deliberately conservative rules — see
//! [`coerce_value`]. When exact typing matters (e.g. a string `"true"`), use
//! the JSON form instead.

use serde_json::{Map, Value};

/// Parse repeated `--param key=value` entries into a JSON object.
///
/// The first `=` separates key from value, so values may themselves contain
/// `=` (URLs, base64, comparison expressions). Entries are applied in order;
/// a later entry with the same key wins.
pub fn parse_kv_params(entries: &[String]) -> std::result::Result<Map<String, Value>, String> {
    let mut map = Map::new();
    for entry in entries {
        let (key, value) = entry.split_once('=').ok_or_else(|| {
            format!(
                "invalid --param '{}': expected key=value (first '=' separates key from value)",
                entry
            )
        })?;
        if key.is_empty() {
            return Err(format!("invalid --param '{}': empty key", entry));
        }
        map.insert(key.to_string(), coerce_value(value));
    }
    Ok(map)
}

/// Infer a JSON value from a bare flag string.
///
/// Rules (conservative on purpose — a wrong coercion silently corrupts data):
/// - exactly `true` / `false` → boolean
/// - integers without leading zeros (except `0` itself) → number
/// - decimals that round-trip through `f64` → number
/// - anything else (leading-zero IDs like `007`, empty string, text) → string
///
/// Escape hatch: pass the exact type via the JSON form of the flag.
pub fn coerce_value(value: &str) -> Value {
    if value == "true" {
        return Value::Bool(true);
    }
    if value == "false" {
        return Value::Bool(false);
    }
    if looks_like_number(value) {
        if let Ok(i) = value.parse::<i64>() {
            return Value::from(i);
        }
        if let Ok(f) = value.parse::<f64>() {
            if f.is_finite() && f.to_string() == value {
                return Value::from(f);
            }
        }
    }
    Value::String(value.to_string())
}

/// A string "looks like" a number only without leading zeros (`007` stays a
/// string — it is far more likely an ID than seven) and without a sign in
/// the middle. Scientific notation is left to the string path unless it
/// round-trips exactly, which the `f64` check above handles.
fn looks_like_number(value: &str) -> bool {
    let body = value
        .strip_prefix('-')
        .or_else(|| value.strip_prefix('+'))
        .unwrap_or(value);
    if body.is_empty() {
        return false;
    }
    let digits = body
        .strip_prefix('.')
        .map(|d| d.chars().all(|c| c.is_ascii_digit()) && !d.is_empty())
        .unwrap_or_else(|| {
            body.chars().all(|c| c.is_ascii_digit() || c == '.')
                && body.chars().any(|c| c.is_ascii_digit())
        });
    digits && !(body.len() > 1 && body.starts_with('0') && !body.starts_with("0."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(entries: &[&str]) -> std::result::Result<Map<String, Value>, String> {
        let owned: Vec<String> = entries.iter().map(|s| s.to_string()).collect();
        parse_kv_params(&owned)
    }

    #[test]
    fn parses_basic_pairs() {
        let map = parse(&["state=true", "speed=3"]).unwrap();
        assert_eq!(map["state"], Value::Bool(true));
        assert_eq!(map["speed"], Value::from(3));
    }

    #[test]
    fn value_may_contain_equals() {
        let map = parse(&["url=https://x.io/a?b=1&c=2"]).unwrap();
        assert_eq!(
            map["url"],
            Value::String("https://x.io/a?b=1&c=2".to_string())
        );
    }

    #[test]
    fn later_entry_overrides() {
        let map = parse(&["mode=fast", "mode=slow"]).unwrap();
        assert_eq!(map["mode"], Value::String("slow".to_string()));
    }

    #[test]
    fn leading_zero_id_stays_string() {
        let map = parse(&["id=007", "pin=0"]).unwrap();
        assert_eq!(map["id"], Value::String("007".to_string()));
        assert_eq!(map["pin"], Value::from(0));
    }

    #[test]
    fn decimal_round_trips() {
        let map = parse(&["threshold=1.5", "gain=-2.25"]).unwrap();
        assert_eq!(map["threshold"], Value::from(1.5));
        assert_eq!(map["gain"], Value::from(-2.25));
    }

    #[test]
    fn non_round_tripping_decimal_stays_string() {
        // "1.10" formats back as "1.1" — do not silently rewrite it.
        let map = parse(&["v=1.10"]).unwrap();
        assert_eq!(map["v"], Value::String("1.10".to_string()));
    }

    #[test]
    fn empty_value_and_empty_key() {
        let map = parse(&["note="]).unwrap();
        assert_eq!(map["note"], Value::String("".to_string()));
        assert!(parse(&["=x"]).is_err());
        assert!(parse(&["noequalsign"]).is_err());
    }

    #[test]
    fn text_values_stay_strings() {
        let map = parse(&["name=客厅灯", "expr=a>=b"]).unwrap();
        assert_eq!(map["name"], Value::String("客厅灯".to_string()));
        assert_eq!(map["expr"], Value::String("a>=b".to_string()));
    }

    #[test]
    fn bare_dash_and_dots_do_not_crash() {
        assert_eq!(coerce_value("-"), Value::String("-".to_string()));
        assert_eq!(coerce_value("."), Value::String(".".to_string()));
        assert_eq!(coerce_value("1e9"), Value::String("1e9".to_string()));
    }
}
