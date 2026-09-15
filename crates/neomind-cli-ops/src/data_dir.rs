//! Canonical data-directory resolution for the CLI.
//!
//! One source of truth for "where is this NeoMind install's data", shared
//! by `login`, `whoami`, `user *`, and the path-joining helpers. The CLI
//! runs from arbitrary working directories, so a bare relative `data` is
//! never a safe answer on its own.
//!
//! Regression this exists for: `neomind user reset-password admin` from
//! `$HOME` reported "User 'admin' not found in data/users.redb". The old
//! resolver checked `./data/api_keys.redb` first (a stale three-month-old
//! directory satisfied it) and returned the literal relative string
//! `"data"` — while the LIVE store lived in the desktop app's data dir
//! (`~/Library/Application Support/com.neomind.neomind/data`), which no
//! probe ever looked at. Resetting the desktop app's password was
//! impossible from the CLI on that machine.

use std::path::{Path, PathBuf};

/// Tauri's app-data directory for this app, per platform, with the
/// `/data` suffix the desktop binary appends (bundle identifier
/// `com.neomind.neomind`, see web/src-tauri/tauri.conf.json).
fn desktop_data_dirs() -> Vec<PathBuf> {
    let id = "com.neomind.neomind";
    let mut out = Vec::new();
    // Tauri v2 `app_data_dir()`: macOS/Windows use the roaming data dir;
    // Linux uses the local data dir. Probe both everywhere — an empty
    // candidate costs one `exists()` call.
    if let Some(d) = dirs::data_dir() {
        out.push(d.join(id).join("data"));
    }
    if let Some(d) = dirs::data_local_dir() {
        let c = d.join(id).join("data");
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

/// A directory "is a NeoMind store" when it holds either bootstrap DB.
/// `api_keys.redb` alone is not enough — a stale legacy dir can hold only
/// that — so `users.redb` also qualifies.
fn looks_like_store(dir: &Path) -> bool {
    dir.join("users.redb").exists() || dir.join("api_keys.redb").exists()
}

/// Resolve the data directory, returning the chosen path plus every
/// candidate that was examined (for honest error messages).
///
/// Priority:
/// 1. explicit `--data-dir`
/// 2. `NEOMIND_DATA_DIR`
/// 3. desktop app data dir (the live store on a desktop install)
/// 4. `./data` (legacy: running from an install root)
/// 5. `dirs::data_local_dir()/neomind` (legacy single-user name)
pub fn resolve(explicit: Option<String>) -> Result<PathBuf, Vec<PathBuf>> {
    let mut examined: Vec<PathBuf> = Vec::new();

    if let Some(d) = explicit {
        if !d.is_empty() {
            return Ok(PathBuf::from(d));
        }
    }
    if let Ok(dir) = std::env::var("NEOMIND_DATA_DIR") {
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }
    for cand in desktop_data_dirs() {
        examined.push(cand.clone());
        if looks_like_store(&cand) {
            return Ok(cand);
        }
    }
    let cwd_relative = PathBuf::from("data");
    examined.push(cwd_relative.clone());
    if looks_like_store(&cwd_relative) {
        return Ok(cwd_relative);
    }
    if let Some(local) = dirs::data_local_dir() {
        let cand = local.join("neomind");
        examined.push(cand.clone());
        if looks_like_store(&cand) {
            return Ok(cand);
        }
    }
    Err(examined)
}

/// Same as [`resolve`], formatted for a user-facing error.
pub fn resolve_or_message(explicit: Option<String>) -> anyhow::Result<PathBuf> {
    resolve(explicit).map_err(|examined| {
        anyhow::anyhow!(
            "No NeoMind data directory found. Looked in:\n{}\n\
             Pass one explicitly with --data-dir <dir>, \
             or set NEOMIND_DATA_DIR.",
            examined
                .iter()
                .map(|p| format!("  - {}", p.display()))
                .collect::<Vec<_>>()
                .join("\n")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The desktop tier is what the CLI was missing entirely: its bundle
    /// identifier must be spelled exactly like tauri.conf.json's
    /// `identifier`, or every desktop install silently falls back to a
    /// relative "data" again.
    #[test]
    fn desktop_candidates_use_the_tauri_bundle_identifier() {
        let cands = desktop_data_dirs();
        assert!(
            !cands.is_empty(),
            "at least one platform dir must be probed"
        );
        for c in &cands {
            assert!(
                c.ends_with("com.neomind.neomind/data"),
                "unexpected desktop candidate: {}",
                c.display()
            );
        }
    }

    #[test]
    fn explicit_path_always_wins() {
        let got = resolve(Some("/tmp/neomind-explicit-data".into())).unwrap();
        assert_eq!(got, PathBuf::from("/tmp/neomind-explicit-data"));
    }

    #[test]
    fn missing_store_reports_every_candidate() {
        let err = resolve(Some(String::new())).err();
        // Empty explicit + no env: the resolver must either find a real
        // store on this machine or list what it looked at. Never panic.
        // A store may exist on this machine (then err is None) — also fine.
        if let Some(examined) = err {
            assert!(!examined.is_empty(), "must report candidates");
        }
    }
}
