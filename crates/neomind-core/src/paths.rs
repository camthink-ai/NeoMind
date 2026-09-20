//! Canonical data-directory resolution for every persistent store.
//!
//! Historical state this module fixes: all redb stores were opened with
//! cwd-relative `"data/*.redb"` literals while extension install paths (and
//! a handful of agent helpers) honored `NEOMIND_DATA_DIR` — a split-brain
//! under custom data dirs, and the root cause behind "extensions silently
//! vanished after an upgrade" incidents. Every store now resolves through
//! [`data_dir`] / [`store_path`].
//!
//! Legacy compatibility: with `NEOMIND_DATA_DIR` set, if a store file
//! exists ONLY at the old cwd-relative `data/<file>` location we keep
//! using it, so existing deployments never lose data across an upgrade.
//! Fresh installs get everything inside the data dir. Migrating is a
//! plain file move; a warn (once per file) points the operator at it.

use std::path::{Path, PathBuf};

/// The platform's data directory: `$NEOMIND_DATA_DIR` when set (non-empty),
/// else the cwd-relative `"data"` default (what install.sh's systemd unit
/// guarantees via `WorkingDirectory`).
pub fn data_dir() -> PathBuf {
    match std::env::var_os("NEOMIND_DATA_DIR") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from("data"),
    }
}

/// Strict-override mode: `NEOMIND_STRICT_DATA_DIR` (any non-empty value)
/// disables the legacy cwd-relative fallback entirely. The fallback exists
/// so an operator who upgrades and points `NEOMIND_DATA_DIR` at a fresh
/// location does not silently start empty while their data still sits in
/// `./data/` — but it also means a stray cwd `data/` SILENTLY splits stores
/// across two trees (the exact trap a verification run fell into: temp
/// canonical dir + repo checkout cwd → half the stores followed the env,
/// half followed the legacy probe). Strict mode is the deterministic
/// override for tests, tooling, and operators who want exactly that.
pub fn strict_data_dir() -> bool {
    matches!(std::env::var_os("NEOMIND_STRICT_DATA_DIR"), Some(v) if !v.is_empty())
}

/// Resolve the path of a store file (e.g. `"extensions.redb"`, `"memory"`),
/// with the legacy cwd-relative fallback described in the module docs.
///
/// Env unset ⇒ always the historical path — zero behavior change for the
/// default deployment. `NEOMIND_STRICT_DATA_DIR` set ⇒ never falls back.
pub fn store_path(file: &str) -> PathBuf {
    match std::env::var_os("NEOMIND_DATA_DIR") {
        Some(v) if !v.is_empty() => {
            let canonical = data_dir().join(file);
            let legacy = Path::new("data").join(file);
            resolve(&canonical, &legacy, file, strict_data_dir())
        }
        _ => Path::new("data").join(file),
    }
}

/// Stores (file names) this process redirected to the legacy cwd-relative
/// path. Server startup logs this list prominently: a split-brain data dir
/// is invisible otherwise until data "goes missing".
pub fn legacy_redirected_stores() -> Vec<String> {
    WARNED.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Decision core, parameterized on strictness so tests cover both modes
/// without touching process-global env vars.
fn resolve(canonical: &Path, legacy: &Path, file: &str, strict: bool) -> PathBuf {
    if strict {
        canonical.to_path_buf()
    } else {
        resolve_with_legacy_fallback(canonical, legacy, file)
    }
}

fn resolve_with_legacy_fallback(canonical: &Path, legacy: &Path, file: &str) -> PathBuf {
    if canonical.exists() || !legacy.exists() {
        canonical.to_path_buf()
    } else {
        // Warn once per file — several call sites resolve per process.
        let mut warned = WARNED.lock().unwrap_or_else(|e| e.into_inner());
        if !warned.iter().any(|f| f == file) {
            warned.push(file.to_string());
            tracing::warn!(
                file,
                legacy = %legacy.display(),
                canonical = %canonical.display(),
                "Store found at the legacy cwd-relative path; using it for \
                 compatibility. Move the file to the canonical path to migrate."
            );
        }
        legacy.to_path_buf()
    }
}

static WARNED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nm-core-paths-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn canonical_wins_when_it_exists() {
        let tmp = scratch("c");
        for d in ["custom", "legacy"] {
            std::fs::create_dir_all(tmp.join(d)).unwrap();
            std::fs::write(tmp.join(d).join("x.redb"), b"").unwrap();
        }
        let got = resolve_with_legacy_fallback(
            &tmp.join("custom/x.redb"),
            &tmp.join("legacy/x.redb"),
            "x.redb",
        );
        assert_eq!(got, tmp.join("custom/x.redb"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn canonical_wins_when_neither_exists() {
        let tmp = scratch("n");
        let got = resolve_with_legacy_fallback(
            &tmp.join("custom/x.redb"),
            &tmp.join("legacy/x.redb"),
            "x.redb",
        );
        assert_eq!(got, tmp.join("custom/x.redb"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn legacy_used_when_only_it_exists() {
        let tmp = scratch("l");
        std::fs::create_dir_all(tmp.join("legacy")).unwrap();
        std::fs::write(tmp.join("legacy/x.redb"), b"").unwrap();
        let got = resolve_with_legacy_fallback(
            &tmp.join("custom/x.redb"),
            &tmp.join("legacy/x.redb"),
            "x.redb",
        );
        assert_eq!(got, tmp.join("legacy/x.redb"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn strict_mode_never_falls_back_even_when_only_legacy_exists() {
        let tmp = scratch("s");
        std::fs::create_dir_all(tmp.join("legacy")).unwrap();
        std::fs::write(tmp.join("legacy/x.redb"), b"").unwrap();
        let got = resolve(
            &tmp.join("custom/x.redb"),
            &tmp.join("legacy/x.redb"),
            "x.redb",
            true,
        );
        assert_eq!(got, tmp.join("custom/x.redb"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn non_strict_still_falls_back() {
        let tmp = scratch("ns");
        std::fs::create_dir_all(tmp.join("legacy")).unwrap();
        std::fs::write(tmp.join("legacy/x.redb"), b"").unwrap();
        let got = resolve(
            &tmp.join("custom/x.redb"),
            &tmp.join("legacy/x.redb"),
            "x.redb",
            false,
        );
        assert_eq!(got, tmp.join("legacy/x.redb"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn join_semantics() {
        assert_eq!(
            Path::new("data").join("a.redb"),
            PathBuf::from("data/a.redb")
        );
    }
}
