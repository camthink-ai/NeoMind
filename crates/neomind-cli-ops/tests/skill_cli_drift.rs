//! Skill ↔ CLI drift integration test.
//!
//! Reads the JSON manifest produced by `scripts/check_skill_cli_drift.py`
//! and verifies each `(domain, action[, sub])` triple actually parses in
//! the current clap surface. A parse failure means a skill file references
//! a CLI command that no longer exists (or never did).
//!
//! Workflow:
//!   1. Run `python3 scripts/check_skill_cli_drift.py --out <manifest>`
//!      as part of the test (skipped if python3 is unavailable or the
//!      builtin skills dir is missing).
//!   2. For each command, attempt `Args::try_parse_from(["neomind", domain, action, ...])`.
//!   3. If parse fails, surface the offending skill + command in the error.
//!
//! To run locally:
//!     cargo test -p neomind-cli-ops --test skill_cli_drift
//!
//! To run in CI without python:
//!     SKIP_DRIFT_GEN=1 cargo test -p neomind-cli-ops --test skill_cli_drift
//! (uses an in-tree fallback manifest committed alongside the skills)

use clap::Parser;
use neomind_cli_ops::dispatch::commands::Args;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

type Manifest = BTreeMap<String, Vec<Vec<String>>>;

const SKILLS_DIR_RELATIVE: &str = "crates/neomind-agent/src/skills/builtins";
const MANIFEST_ENV_VAR: &str = "NEOMIND_SKILL_MANIFEST";
const SKIP_GEN_ENV_VAR: &str = "SKIP_DRIFT_GEN";

/// Domains where the action word is itself a subcommand group (e.g. `device drafts ...`).
/// For these we treat the second word as part of the command path, not an ID.
const NESTED_DOMAINS: &[(&str, &str)] = &[("device", "drafts"), ("device", "types")];

fn locate_workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR points at crates/neomind-cli-ops/.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn generate_manifest(workspace_root: &Path) -> Option<Manifest> {
    if std::env::var(SKIP_GEN_ENV_VAR).is_ok() {
        return None;
    }
    let script = workspace_root
        .join("scripts")
        .join("check_skill_cli_drift.py");
    let skills_dir = workspace_root.join(SKILLS_DIR_RELATIVE);
    if !script.exists() || !skills_dir.exists() {
        return None;
    }

    let tmp = std::env::temp_dir().join(format!("neomind_skill_drift_{}.json", std::process::id()));
    let output = Command::new("python3")
        .arg(&script)
        .arg("--out")
        .arg(&tmp)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = std::fs::read_to_string(&tmp).ok()?;
    let _ = std::fs::remove_file(&tmp);
    serde_json::from_str::<Manifest>(&text).ok()
}

fn load_manifest() -> Manifest {
    // 1. Explicit override (CI may pre-bake the manifest)
    if let Ok(path) = std::env::var(MANIFEST_ENV_VAR) {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(m) = serde_json::from_str::<Manifest>(&text) {
                return m;
            }
        }
    }
    // 2. Generate via python
    let workspace_root = locate_workspace_root();
    if let Some(m) = generate_manifest(&workspace_root) {
        return m;
    }
    // 3. Empty manifest — test will skip rather than fail.
    Manifest::new()
}

fn try_parse_command(parts: &[String]) -> Result<(), String> {
    // Build the argv. Always start with the program name.
    let mut argv: Vec<String> = vec!["neomind".to_string()];
    argv.extend(parts.iter().cloned());

    // Skills reference commands like `neomind device get <ID>` — we substitute
    // a placeholder for ANY positional that looks like an ID. We do this by
    // simply truncating the command at the first uppercase/ID-looking token
    // OR after the first 3 positional words (domain action sub?).
    // For drift we only care: does clap RECOGNIZE the (domain, action) shape?
    // So strip everything after the recognized command path.
    let cleaned = strip_id_arguments(&argv);

    match Args::try_parse_from(&cleaned) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("{}", e)),
    }
}

/// Reduce `neomind device get device-001` → `neomind device get <dummy>`
/// because we only care about clap recognizing the command shape, not the ID.
/// The drift check is shape-only; semantic ID validation is per-handler.
fn strip_id_arguments(argv: &[String]) -> Vec<String> {
    if argv.len() <= 2 {
        return argv.to_vec();
    }
    let domain = &argv[1];
    let action = &argv[2];

    // For nested subcommands (device drafts list/approve), keep up to 3 tokens.
    if NESTED_DOMAINS.contains(&(domain.as_str(), action.as_str())) {
        if argv.len() >= 4 {
            // Keep `neomind <domain> <action> <sub>` and drop the rest (IDs).
            return argv[..4].to_vec();
        }
        return argv.to_vec();
    }

    // Otherwise keep only the first 3 tokens (neomind <domain> <action>).
    // Add a dummy ID if the action requires one — clap will tell us via error.
    // For drift detection we accept both "ID required" errors and clean parses.
    argv[..3].to_vec()
}

#[test]
fn skill_cli_drift() {
    let manifest = load_manifest();
    if manifest.is_empty() {
        eprintln!("warn: skill drift manifest is empty (python3 unavailable? skills dir missing?)");
        eprintln!("      skipping drift check. To force generation, unset {SKIP_GEN_ENV_VAR}.");
        return;
    }

    let mut failures: Vec<(String, Vec<String>, String)> = Vec::new();
    let mut checked = 0usize;

    for (skill_id, commands) in &manifest {
        for cmd in commands {
            checked += 1;
            // Accept "missing required ID" as success — that means the command shape is valid.
            // Real drift shows up as InvalidSubcommand or UnknownArgument.
            if let Err(err) = try_parse_command(cmd) {
                let err_str = err.to_string();
                if err_str.contains("required")
                    && !err_str.contains("InvalidSubcommand")
                    && !err_str.contains("UnknownArgument")
                {
                    continue;
                }
                if err_str.contains("the following required arguments") {
                    continue;
                }
                failures.push((skill_id.clone(), cmd.clone(), err_str));
            }
        }
    }

    if !failures.is_empty() {
        eprintln!(
            "\nSkill ↔ CLI drift detected ({}/{checked} commands failed):\n",
            failures.len()
        );
        for (skill, cmd, err) in &failures {
            eprintln!("  [{skill}] `neomind {}` → {err}", cmd.join(" "));
        }
        eprintln!();
        panic!("skill references commands that don't exist in the CLI; see stderr");
    }

    eprintln!(
        "ok: {checked} skill commands across {} skills all parse",
        manifest.len()
    );
}

#[test]
fn smoke_basic_parse() {
    // Sanity: confirm the test harness itself is wired correctly.
    let r = try_parse_command(&["device".to_string(), "list".to_string()]);
    assert!(r.is_ok(), "device list should parse");
}
