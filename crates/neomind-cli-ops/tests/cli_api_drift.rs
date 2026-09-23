//! CLI → API field drift.
//!
//! The CLI is a pure HTTP client: it builds a JSON body and posts it. The API
//! deserializes into a struct — and none of those use `deny_unknown_fields`,
//! so a key the API does not declare is **accepted and discarded**. The CLI
//! reports success and the field never lands anywhere.
//!
//! That is not hypothetical. Three shipped parameters went this way:
//! `--enable-tool-chaining` (its field had been removed from the API DTOs),
//! `--message-type` (the CLI wrote `type`, the struct declared
//! `message_type`), and `--auto-approve` (no receiving field existed at all).
//! None had a test that could notice, because a dropped field leaves no trace
//! — not in the response, not in a log, not in a stored row.
//!
//! The scan lives in `scripts/check_cli_api_drift.py` (same split as
//! `skill_cli_drift`): source-shape work belongs somewhere with real text
//! handling, and an earlier attempt to do it in Rust quietly saw a fraction of
//! its input — a guard that cannot see most of what it guards reads as green.
//!
//! Run locally:
//!     cargo test -p neomind-cli-ops --test cli_api_drift

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

#[derive(serde::Deserialize)]
struct Manifest {
    declared_count: usize,
    written_count: usize,
    /// body key → the CLI file that writes it
    orphans: BTreeMap<String, String>,
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crate lives two levels under the root")
        .to_path_buf()
}

fn manifest() -> Option<Manifest> {
    let root = workspace_root();
    let script = root.join("scripts").join("check_cli_api_drift.py");
    if !script.exists() {
        return None;
    }
    let tmp =
        std::env::temp_dir().join(format!("neomind_cli_api_drift_{}.json", std::process::id()));
    let status = Command::new("python3")
        .arg(&script)
        .arg("--root")
        .arg(&root)
        .arg("--out")
        .arg(&tmp)
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }
    let text = std::fs::read_to_string(&tmp).ok()?;
    let _ = std::fs::remove_file(&tmp);
    serde_json::from_str(&text).ok()
}

#[test]
fn every_body_key_the_cli_writes_is_declared_somewhere() {
    let Some(m) = manifest() else {
        eprintln!("no python3 / script missing — skipping the drift check");
        return;
    };

    // A scan that found almost nothing would report "no orphans" and look
    // green. Assert it actually read the tree before trusting its answer.
    assert!(
        m.declared_count > 100,
        "the scan found only {} declared fields — it is not reading the tree, \
         so its verdict means nothing",
        m.declared_count
    );
    assert!(
        m.written_count > 10,
        "the scan found only {} body keys — same problem",
        m.written_count
    );

    assert!(
        m.orphans.is_empty(),
        "these keys are written into request bodies but no struct declares them. \
         The API has no `deny_unknown_fields`, so each is silently dropped:\n{}",
        m.orphans
            .iter()
            .map(|(k, f)| format!("  {k}  (written in {f})"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
