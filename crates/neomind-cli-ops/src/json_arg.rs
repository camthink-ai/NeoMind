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

/// Expected top-level JSON container kind for a CLI command's primary JSON flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedJsonKind {
    Object,
    Array,
}

/// Primary JSON flag specification for a `neomind …` command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrimaryJsonSpec {
    pub flag: &'static str,
    pub kind: ExpectedJsonKind,
}

/// Canonical `(domain, action) -> PrimaryJsonSpec` table shared by structured
/// shell `params` validation/injection and cross-check unit tests against clap.
pub const PRIMARY_JSON_SPECS: &[(&str, &str, PrimaryJsonSpec)] = &[
    (
        "extension",
        "invoke",
        PrimaryJsonSpec {
            flag: "--params",
            kind: ExpectedJsonKind::Object,
        },
    ),
    (
        "device",
        "control",
        PrimaryJsonSpec {
            flag: "--params",
            kind: ExpectedJsonKind::Object,
        },
    ),
    (
        "device",
        "create",
        PrimaryJsonSpec {
            flag: "--config",
            kind: ExpectedJsonKind::Object,
        },
    ),
    (
        "device",
        "update",
        PrimaryJsonSpec {
            flag: "--config",
            kind: ExpectedJsonKind::Object,
        },
    ),
    (
        "extension",
        "config",
        PrimaryJsonSpec {
            flag: "--set",
            kind: ExpectedJsonKind::Object,
        },
    ),
    (
        "dashboard",
        "update-component",
        PrimaryJsonSpec {
            flag: "--set",
            kind: ExpectedJsonKind::Object,
        },
    ),
    (
        "dashboard",
        "add-components",
        PrimaryJsonSpec {
            flag: "--components",
            kind: ExpectedJsonKind::Array,
        },
    ),
    (
        "dashboard",
        "remove-components",
        PrimaryJsonSpec {
            flag: "--ids",
            kind: ExpectedJsonKind::Array,
        },
    ),
    (
        "dashboard",
        "create",
        PrimaryJsonSpec {
            flag: "--components",
            kind: ExpectedJsonKind::Array,
        },
    ),
    (
        "dashboard",
        "update",
        PrimaryJsonSpec {
            flag: "--components",
            kind: ExpectedJsonKind::Array,
        },
    ),
    (
        "rule",
        "create",
        PrimaryJsonSpec {
            flag: "--body",
            kind: ExpectedJsonKind::Object,
        },
    ),
    (
        "rule",
        "update",
        PrimaryJsonSpec {
            flag: "--body",
            kind: ExpectedJsonKind::Object,
        },
    ),
    (
        "rule",
        "test",
        PrimaryJsonSpec {
            flag: "--input",
            kind: ExpectedJsonKind::Object,
        },
    ),
    (
        "transform",
        "test-code",
        PrimaryJsonSpec {
            flag: "--input",
            kind: ExpectedJsonKind::Object,
        },
    ),
    (
        "push",
        "create",
        PrimaryJsonSpec {
            flag: "--config",
            kind: ExpectedJsonKind::Object,
        },
    ),
    (
        "push",
        "update",
        PrimaryJsonSpec {
            flag: "--config",
            kind: ExpectedJsonKind::Object,
        },
    ),
    (
        "message",
        "channel-create",
        PrimaryJsonSpec {
            flag: "--config",
            kind: ExpectedJsonKind::Object,
        },
    ),
    (
        "message",
        "channel-update",
        PrimaryJsonSpec {
            flag: "--config",
            kind: ExpectedJsonKind::Object,
        },
    ),
];

/// Primary JSON flag + expected container kind for a `neomind …` argv.
/// Callers still XOR against an existing flag in `argv`.
pub fn primary_json_spec(argv: &[String]) -> Option<PrimaryJsonSpec> {
    if argv.len() < 3 || argv[0] != "neomind" {
        return None;
    }
    let domain = argv[1].as_str();
    let action = argv[2].as_str();
    PRIMARY_JSON_SPECS
        .iter()
        .find(|(d, a, _)| *d == domain && *a == action)
        .map(|(_, _, spec)| *spec)
}

/// Which primary JSON flag a `neomind …` argv expects for structured shell
/// `params` injection. Multi-flag create/update commands return the most
/// common primary payload flag; callers still XOR against an existing flag.
pub fn primary_json_flag(argv: &[String]) -> Option<&'static str> {
    primary_json_spec(argv).map(|s| s.flag)
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

    #[test]
    fn primary_json_spec_distinguishes_object_vs_array_and_drops_invalid_mappings() {
        let add = vec![
            "neomind".into(),
            "dashboard".into(),
            "add-components".into(),
            "d1".into(),
        ];
        assert_eq!(
            primary_json_spec(&add),
            Some(PrimaryJsonSpec {
                flag: "--components",
                kind: ExpectedJsonKind::Array,
            })
        );
        let rem = vec![
            "neomind".into(),
            "dashboard".into(),
            "remove-components".into(),
            "d1".into(),
        ];
        assert_eq!(
            primary_json_spec(&rem),
            Some(PrimaryJsonSpec {
                flag: "--ids",
                kind: ExpectedJsonKind::Array,
            })
        );
        let ch_upd = vec![
            "neomind".into(),
            "message".into(),
            "channel-update".into(),
            "--name".into(),
            "ops".into(),
        ];
        assert_eq!(
            primary_json_spec(&ch_upd),
            Some(PrimaryJsonSpec {
                flag: "--config",
                kind: ExpectedJsonKind::Object,
            })
        );
        // `transform create` has no --input flag and must not map to one.
        let tr_create = vec!["neomind".into(), "transform".into(), "create".into()];
        assert_eq!(primary_json_spec(&tr_create), None);
    }

    /// Mechanical anti-drift check: every entry in `PRIMARY_JSON_SPECS` must
    /// match a real clap subcommand and a real `--<flag>` argument on `Args`.
    #[test]
    fn primary_json_specs_match_real_clap_definitions() {
        use clap::CommandFactory;
        let root = crate::dispatch::commands::Args::command();
        for (domain, action, spec) in PRIMARY_JSON_SPECS {
            let domain_cmd = root
                .get_subcommands()
                .find(|c| c.get_name() == *domain)
                .unwrap_or_else(|| panic!("domain `{domain}` missing from clap Args"));
            let action_cmd = domain_cmd
                .get_subcommands()
                .find(|c| c.get_name() == *action)
                .unwrap_or_else(|| panic!("action `{domain} {action}` missing from clap Args"));
            let want_long = spec
                .flag
                .strip_prefix("--")
                .expect("primary flag must start with --");
            let has_flag = action_cmd
                .get_arguments()
                .any(|a| a.get_long() == Some(want_long));
            assert!(
                has_flag,
                "flag `{}` does not exist on clap command `neomind {domain} {action}`",
                spec.flag
            );
            let argv = vec!["neomind".to_string(), (*domain).into(), (*action).into()];
            assert_eq!(
                primary_json_flag(&argv),
                Some(spec.flag),
                "primary_json_flag must resolve `{domain} {action}` to `{}`",
                spec.flag
            );
        }
    }
}
