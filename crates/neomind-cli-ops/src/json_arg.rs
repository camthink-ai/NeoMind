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
                        anyhow!(
                            "{}",
                            format_json_parse_error(
                                &retried,
                                &format!(
                                    "invalid JSON after quote normalize: {e2} (original: {e})"
                                )
                            )
                        )
                    });
                }
            }
            Err(anyhow!("{}", format_json_parse_error(&normalized, &msg)))
        }
    }
}

/// Enrich serde JSON errors with a short snippet and actionable hints.
/// Large-array flags (`--components` / `--ids`) get a 1–2 item batch tip.
pub fn format_json_parse_error(raw: &str, serde_msg: &str) -> String {
    let trimmed = raw.trim();
    let mut out = if serde_msg.starts_with("invalid JSON") {
        serde_msg.to_string()
    } else {
        format!("invalid JSON: {serde_msg}")
    };

    // Snippet around the reported line+column when present (`… at line L column C`).
    if let Some((line, col)) = parse_serde_line_col(serde_msg) {
        let snippet = json_error_snippet(trimmed, line, col);
        if !snippet.is_empty() {
            out.push_str("\nNear: ");
            out.push_str(&snippet);
        }
    }

    let lower = serde_msg.to_lowercase();
    if lower.contains("key must be a string") {
        out.push_str(
            "\nHint: a closing `}` is likely missing so the next `{…}` was read as a key — \
             finish each object before the next array element.",
        );
    } else if lower.contains("trailing characters") || lower.contains("expected") {
        out.push_str(
            "\nHint: check for an extra `}`/`]` or a leftover quote after the JSON value.",
        );
    }

    if trimmed.starts_with('[') {
        out.push_str(
            "\nHint: for large arrays (dashboard `--components` / `--ids`) prefer shell \
             `params: […]` and add only 1–2 components per call — then append again.",
        );
    } else {
        out.push_str(
            "\nHint: prefer shell structured `params` (object/array) over quoting JSON inside the command string.",
        );
    }
    out
}

/// Parse `(line, column)` from serde messages like `… at line 2 column 14`.
fn parse_serde_line_col(msg: &str) -> Option<(usize, usize)> {
    let line_marker = "line ";
    let col_marker = "column ";
    let line_idx = msg.rfind(line_marker)?;
    let after_line = &msg[line_idx + line_marker.len()..];
    let line_digits: String = after_line
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let line: usize = line_digits.parse().ok()?;
    let col_idx = after_line.find(col_marker)?;
    let after_col = &after_line[col_idx + col_marker.len()..];
    let col_digits: String = after_col
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let col: usize = col_digits.parse().ok()?;
    Some((line, col))
}

/// Absolute byte offset for serde's 1-based `(line, column)` in `raw`.
///
/// Serde's `column` is a **UTF-8 byte** index on the line (not a Unicode scalar
/// count). Walk by bytes, then [`str::floor_char_boundary`] so a mid-codepoint
/// column still yields a valid slice start for the Near snippet.
fn offset_for_line_col(raw: &str, line_1based: usize, column_1based: usize) -> Option<usize> {
    if line_1based == 0 || column_1based == 0 {
        return None;
    }
    let mut byte = 0usize;
    for (i, line) in raw.split_inclusive('\n').enumerate() {
        if i + 1 == line_1based {
            let line_bytes = if line.ends_with('\n') {
                line.len().saturating_sub(1)
            } else {
                line.len()
            };
            // 1-based byte column → 0-based offset within the line content.
            let col_bytes = column_1based.saturating_sub(1).min(line_bytes);
            let abs = byte + col_bytes;
            return Some(raw.floor_char_boundary(abs.min(raw.len())));
        }
        byte += line.len();
    }
    None
}

fn json_error_snippet(raw: &str, line_1based: usize, column_1based: usize) -> String {
    if raw.is_empty() {
        return String::new();
    }
    let focus = offset_for_line_col(raw, line_1based, column_1based).unwrap_or(0);
    let focus = focus.min(raw.len());
    let start = focus.saturating_sub(24);
    let end = (focus + 24).min(raw.len());
    // Stay on char boundaries.
    let start = raw.floor_char_boundary(start);
    let end = raw.ceil_char_boundary(end);
    let mut s = String::new();
    if start > 0 {
        s.push('…');
    }
    s.push_str(&raw[start..end]);
    if end < raw.len() {
        s.push('…');
    }
    s
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

pub use neomind_core::command_contract::{
    core_domain_contracts, find_domain_command_contract, find_domain_command_contract_for_argv,
    DomainCommandContract, DomainCommandSideEffect, ExpectedJsonKind, PrimaryJsonSpec,
};

/// Canonical `(domain, action) -> PrimaryJsonSpec` table shared by structured
/// shell `params` validation/injection and cross-check unit tests against clap.
///
/// For the 4 core business domains (Device / Rule / Dashboard / Agent), entries
/// stay locked to [`neomind_core::command_contract::CORE_DOMAIN_CONTRACTS`] via
/// `primary_json_spec` and anti-drift unit tests.
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
/// Consults [`neomind_core::command_contract::CORE_DOMAIN_CONTRACTS`] first for
/// the registered core business domains, falling back to [`PRIMARY_JSON_SPECS`]
/// for other CLI domains. Callers still XOR against an existing flag in `argv`.
pub fn primary_json_spec(argv: &[String]) -> Option<PrimaryJsonSpec> {
    if argv.len() < 3 || argv[0] != "neomind" {
        return None;
    }
    if let Some(contract) = find_domain_command_contract_for_argv(argv) {
        return contract.primary_json;
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
    fn malformed_array_error_includes_snippet_and_batch_hint() {
        // Missing `}` before next object → serde "key must be a string".
        let err = parse_json_arg(r#"[{"id":"a","type":"value-card"},{"id":"b"]"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid JSON"), "{err}");
        assert!(
            err.contains("1–2 components") || err.contains("1-2 components"),
            "batch hint missing: {err}"
        );
        assert!(err.contains("Near:") || err.contains("Hint:"), "{err}");
    }

    #[test]
    fn key_must_be_string_gets_brace_hint() {
        let msg = format_json_parse_error(
            r#"[{"a":1},{"b":2}]"#,
            "key must be a string at line 1 column 10",
        );
        assert!(
            msg.contains("closing `}`") || msg.contains("closing }"),
            "{msg}"
        );
        assert!(msg.contains("Near:"), "{msg}");
    }

    #[test]
    fn near_snippet_uses_line_and_column_not_absolute_column() {
        let raw = "{\n  \"a\": 1,\n  oops\n}";
        let msg = format_json_parse_error(raw, "expected `:` at line 3 column 3");
        assert!(msg.contains("Near:"), "{msg}");
        // Column 3 on line 3 is near `oops`, not byte offset 3 at the start.
        assert!(
            msg.contains("oops"),
            "snippet must resolve line+col to the failing line, got: {msg}"
        );
        assert_eq!(parse_serde_line_col("x at line 3 column 3"), Some((3, 3)));
        assert_eq!(offset_for_line_col(raw, 3, 3), Some(14)); // after "{\n  \"a\": 1,\n  "
    }

    #[test]
    fn near_snippet_treats_serde_column_as_utf8_byte_offset_with_chinese() {
        // Line 2 (no NL): `  "名":坏` — 名/坏 are 3-byte UTF-8 each.
        // Bytes: 0-1 SP, 2 `"`, 3-5 名, 6 `"`, 7 `:`, 8-10 坏
        // Serde "column 9" = 1-based byte → focus at byte 8 = start of 坏.
        // Char-index walking would treat column 9 as past EOL and miss 坏.
        let line2 = "  \"名\":坏";
        assert_eq!(line2.len(), 11);
        assert_eq!(&line2.as_bytes()[8..], "坏".as_bytes());
        let raw = format!("{{\n{line2}\n}}");
        let msg = format_json_parse_error(&raw, "expected value at line 2 column 9");
        assert!(msg.contains("Near:"), "{msg}");
        assert!(
            msg.contains('坏'),
            "Near must land on UTF-8 byte column (坏), got: {msg}"
        );
        let focus = offset_for_line_col(&raw, 2, 9).expect("offset");
        assert!(
            raw[focus..].starts_with('坏'),
            "focus={focus}, rest={:?}",
            &raw[focus..]
        );
        // Mid-codepoint column (byte 4 inside 名) aligns to the char start.
        let mid = offset_for_line_col(&raw, 2, 5).expect("mid");
        assert!(raw.is_char_boundary(mid));
        assert!(
            raw[mid..].starts_with('名'),
            "mid-codepoint column must floor to 名, rest={:?}",
            &raw[mid..]
        );
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

    /// P2-1 anti-drift check: every registered `DomainCommandContract` across
    /// the ten core domains must match:
    /// 1) real `Args::command()` clap subcommands and flags (including `key_params` flags)
    /// 2) `PRIMARY_JSON_SPECS` and `primary_json_spec()`
    /// 3) its own `shell_example` passes `validate_primary_json` and parses via clap
    /// 4) `dispatch::capability::classify_argv` side-effect classification.
    #[test]
    fn core_domain_command_contracts_align_with_clap_specs_and_capability() {
        use crate::dispatch::capability::{classify_argv, CliSideEffect};
        use clap::{CommandFactory, Parser};

        let root = crate::dispatch::commands::Args::command();

        for contract in core_domain_contracts() {
            let domain_cmd = root
                .get_subcommands()
                .find(|c| c.get_name() == contract.domain)
                .unwrap_or_else(|| {
                    panic!(
                        "contract domain `{}` missing from clap Args",
                        contract.domain
                    )
                });
            let action_cmd = domain_cmd
                .get_subcommands()
                .find(|c| c.get_name() == contract.action)
                .unwrap_or_else(|| {
                    panic!(
                        "contract action `{} {}` missing from clap Args",
                        contract.domain, contract.action
                    )
                });

            // Check every `--flag` listed in key_params exists in clap.
            for param in contract.key_params {
                if let Some(long) = param.name.strip_prefix("--") {
                    let exists = action_cmd
                        .get_arguments()
                        .any(|a| a.get_long() == Some(long));
                    assert!(
                        exists,
                        "key_param `{}` missing from clap `neomind {} {}`",
                        param.name, contract.domain, contract.action
                    );
                }
            }

            // Check primary_json matches PRIMARY_JSON_SPECS + primary_json_spec + clap.
            let base_argv = vec![
                "neomind".to_string(),
                contract.domain.to_string(),
                contract.action.to_string(),
            ];
            let table_spec = PRIMARY_JSON_SPECS
                .iter()
                .find(|(d, a, _)| *d == contract.domain && *a == contract.action)
                .map(|(_, _, spec)| *spec);
            assert_eq!(
                contract.primary_json, table_spec,
                "DomainCommandContract.primary_json drifted from PRIMARY_JSON_SPECS for `{} {}`",
                contract.domain, contract.action
            );
            assert_eq!(
                primary_json_spec(&base_argv),
                contract.primary_json,
                "primary_json_spec drifted for `{} {}`",
                contract.domain,
                contract.action
            );

            // Check shell_example: params validate, command + injected primary flag parses in clap,
            // and classify_argv matches contract.side_effect.
            let ex = contract.shell_example_value();
            let ex_cmd = ex["command"]
                .as_str()
                .expect("shell_example must have command string");
            let mut argv = tokenize_simple_shell_example(ex_cmd);
            if let Some(params) = ex.get("params") {
                contract.validate_primary_json(params).unwrap_or_else(|e| {
                    panic!(
                        "shell_example params failed contract validation for `{} {}`: {e}",
                        contract.domain, contract.action
                    )
                });
                let spec = contract
                    .primary_json
                    .expect("contract with params example must declare primary_json");
                argv.push(spec.flag.to_string());
                argv.push(params.to_string());
            }
            crate::dispatch::commands::Args::try_parse_from(argv.iter()).unwrap_or_else(|e| {
                panic!(
                    "shell_example argv {argv:?} for `{} {}` failed clap parse: {e}",
                    contract.domain, contract.action
                )
            });

            let expected_effect = match contract.side_effect {
                DomainCommandSideEffect::ReadOnly => CliSideEffect::ProvenReadOnly,
                DomainCommandSideEffect::Mutating => CliSideEffect::Mutating,
            };
            assert_eq!(
                classify_argv(&argv).unwrap_or_else(|e| panic!(
                    "classify_argv failed for `{} {}`: {e}",
                    contract.domain, contract.action
                )),
                expected_effect,
                "side_effect mismatch between DomainCommandContract and capability.rs for `{} {}`",
                contract.domain,
                contract.action
            );
        }
    }

    fn tokenize_simple_shell_example(cmd: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut in_quotes = false;
        for ch in cmd.chars() {
            match ch {
                '"' => in_quotes = !in_quotes,
                c if c.is_whitespace() && !in_quotes => {
                    if !cur.is_empty() {
                        out.push(std::mem::take(&mut cur));
                    }
                }
                c => cur.push(c),
            }
        }
        if !cur.is_empty() {
            out.push(cur);
        }
        out
    }
}
