//! Shared release/semver/download primitives for server self-upgrade.
//!
//! Used by three callers:
//! - the interactive CLI path (`neomind upgrade`) in neomind-cli,
//! - the staged web-triggered path (stage phase, this crate's `service`),
//! - the root apply step (`neomind upgrade --apply-staged` via the systemd
//!   helper unit) in neomind-cli.
//!
//! Pure logic only: no printing, no stdin, no assumption about privileges.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

/// GitHub repo that hosts releases (server tarballs + web tarball + OTA manifest).
pub const REPO: &str = "camthink-ai/NeoMind";

/// Hardcoded fallbacks mirroring `scripts/install.sh` (the installer's
/// generated unit is the real source of truth; these are the same defaults).
pub const DEFAULT_INSTALL_DIR: &str = "/usr/local/bin";
pub const DEFAULT_WEB_DIR: &str = "/var/www/neomind";

/// Download size cap for release tarballs (server bundle + web bundle).
/// The server tarball is ~100-200 MB; 2 GB leaves generous headroom for
/// bundles that grow (e.g. bundled models).
pub const MAX_RELEASE_DOWNLOAD_SIZE: u64 = 2 * 1024 * 1024 * 1024;

/// Name of the systemd helper units that run the privileged apply step.
/// The `.service` does the work as root; the `.path` watches for the trigger
/// file (written by this process) and starts the service — no sudo/polkit,
/// which matters because the API's own unit sets `NoNewPrivileges=true`
/// (sudo from inside it can never gain root).
pub const APPLY_UNIT_NAME: &str = "neomind-upgrade-apply.service";
pub const APPLY_PATH_UNIT_NAME: &str = "neomind-upgrade-apply.path";

/// Trigger file (inside the staging root) whose creation activates
/// [`APPLY_PATH_UNIT_NAME`]. Removed by the apply step first thing so the
/// path unit re-arms for the next upgrade.
pub const APPLY_TRIGGER_FILE: &str = "apply.trigger";

/// Staging manifest written by the stage phase, read by `--apply-staged`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StagingManifest {
    /// Target version (no leading `v`).
    pub version: String,
    /// Host arch the tarball was downloaded for ("amd64" | "arm64").
    pub arch: String,
    /// Epoch millis when staging completed.
    pub staged_at: i64,
    /// Relative path (within the staging dir) of the extracted `neomind` binary.
    pub server_binary: String,
    /// Relative path of the extracted `neomind-extension-runner`, if present.
    pub extension_runner: Option<String>,
    /// Relative path of the web tarball, if one was staged.
    pub web_tarball: Option<String>,
}

// ---------------------------------------------------------------------------
// Version parsing / comparison
// ---------------------------------------------------------------------------

/// Parse the leading `major.minor.patch` ints out of a (possibly dirty)
/// version string. Non-digit chars act as separators; missing parts are 0.
pub fn semver(v: &str) -> [u64; 3] {
    let v = v.trim_start_matches('v');
    let mut parts = v.split(|c: char| !c.is_ascii_digit());
    let major = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    [major, minor, patch]
}

/// True when `target` is strictly newer than `current` (pre-release suffixes
/// are ignored, same as the CLI path always did).
pub fn is_newer(target: &str, current: &str) -> bool {
    let t = semver(target);
    let c = semver(current);
    t[0] > c[0] || (t[0] == c[0] && t[1] > c[1]) || (t[0] == c[0] && t[1] == c[1] && t[2] > c[2])
}

// ---------------------------------------------------------------------------
// Release metadata + URLs
// ---------------------------------------------------------------------------

/// Release asset arch component for the current host.
pub fn host_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "amd64"
    }
}

/// `releases/download` URL for the host-arch Linux server tarball.
pub fn server_tarball_url(version: &str, arch: &str) -> String {
    format!(
        "https://github.com/{}/releases/download/v{}/neomind-server-linux-{}.tar.gz",
        REPO, version, arch
    )
}

/// `releases/download` URL for the unversioned web-frontend tarball
/// (unversioned on purpose so `releases/latest/download/...` always works).
pub fn web_tarball_url(version: &str) -> String {
    format!(
        "https://github.com/{}/releases/download/v{}/neomind-web.tar.gz",
        REPO, version
    )
}

/// Resolve the latest release tag + release-notes body from the GitHub API
/// (tag has no leading `v`; body is markdown, may be absent).
pub async fn github_latest_release(client: &reqwest::Client) -> Result<(String, Option<String>)> {
    let resp: serde_json::Value = client
        .get(format!("https://api.github.com/repos/{}/releases/latest", REPO))
        .send()
        .await?
        .error_for_status()
        .context("GitHub latest-release lookup failed")?
        .json()
        .await?;
    let tag = resp["tag_name"]
        .as_str()
        .ok_or_else(|| anyhow!("release has no tag_name"))?;
    let body = resp["body"].as_str().map(str::to_string);
    Ok((tag.trim_start_matches('v').to_string(), body))
}

/// Resolve the latest release tag from the GitHub API (no leading `v`).
pub async fn github_latest_version(client: &reqwest::Client) -> Result<String> {
    Ok(github_latest_release(client).await?.0)
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// Web-frontend dir the server serves static files from: `NEOMIND_WEB_DIR`
/// when set (the installer unit exports it), else `/var/www/neomind`.
///
/// Note: the pre-web-upgrade CLI hardcoded the default and ignored the env
/// var; both callers now go through here.
pub fn web_dir() -> PathBuf {
    match std::env::var_os("NEOMIND_WEB_DIR") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(DEFAULT_WEB_DIR),
    }
}

/// Root of the writable staging area (inside the data dir the API can write).
pub fn staging_root(data_dir: &Path) -> PathBuf {
    data_dir.join("upgrade")
}

/// Staging dir for one specific target version.
pub fn staging_dir(data_dir: &Path, version: &str) -> PathBuf {
    staging_root(data_dir).join(format!("v{}", version))
}

// ---------------------------------------------------------------------------
// Download + extract + verify
// ---------------------------------------------------------------------------

/// Stream `url` to `dest` (a file path), enforcing
/// [`MAX_RELEASE_DOWNLOAD_SIZE`] via Content-Length and a running counter.
///
/// `on_progress` fires per chunk with `(downloaded, total)`; callers throttle
/// event publishing themselves (the builtin-LLM downloader pattern).
pub async fn download_to_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    mut on_progress: Option<&mut (dyn FnMut(u64, Option<u64>) + Send)>,
) -> Result<u64> {
    use futures::StreamExt;
    use tokio::io::AsyncWriteExt;

    let resp = client
        .get(url)
        .send()
        .await?
        .error_for_status()
        .with_context(|| format!("download failed: {url}"))?;

    let declared_total = resp.content_length();
    if let Some(len) = declared_total {
        if len > MAX_RELEASE_DOWNLOAD_SIZE {
            return Err(anyhow!(
                "release asset too large ({len} bytes > {} cap)",
                MAX_RELEASE_DOWNLOAD_SIZE
            ));
        }
    }

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let mut file = tokio::fs::File::create(dest)
        .await
        .with_context(|| format!("cannot create {}", dest.display()))?;
    let mut downloaded: u64 = 0;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("download stream error")?;
        downloaded += chunk.len() as u64;
        if downloaded > MAX_RELEASE_DOWNLOAD_SIZE {
            let _ = tokio::fs::remove_file(dest).await;
            return Err(anyhow!(
                "release asset exceeded {} cap while streaming",
                MAX_RELEASE_DOWNLOAD_SIZE
            ));
        }
        file.write_all(&chunk).await?;
        if let Some(cb) = on_progress.as_deref_mut() {
            cb(downloaded, declared_total);
        }
    }
    file.flush().await?;
    Ok(downloaded)
}

/// Extract a `.tar.gz` into `dest_dir` (created if missing).
pub fn extract_tar_gz(tarball: &Path, dest_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dest_dir)?;
    let file = std::fs::File::open(tarball)
        .with_context(|| format!("cannot open {}", tarball.display()))?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    archive.unpack(dest_dir).with_context(|| {
        format!(
            "tar extract failed: {} → {}",
            tarball.display(),
            dest_dir.display()
        )
    })?;
    Ok(())
}

/// Run `bin --version` and require the output to reference `expected`.
///
/// This is the only content check before swapping binaries in (same policy
/// as the CLI path): it catches wrong-arch, truncated, and foreign binaries.
pub fn verify_binary_version(bin: &Path, expected: &str) -> Result<String> {
    let out = std::process::Command::new(bin)
        .arg("--version")
        .output()
        .with_context(|| format!("could not run the downloaded binary {}", bin.display()))?;
    let vstr = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !vstr.ends_with(expected) && !vstr.contains(&format!(" {expected}")) {
        return Err(anyhow!(
            "downloaded binary version mismatch: got {:?}, expected v{expected} — aborting",
            vstr
        ));
    }
    Ok(vstr)
}

// ---------------------------------------------------------------------------
// Apply-phase helpers (run as root via the systemd helper unit)
// ---------------------------------------------------------------------------

/// Run a command, optionally prefixing with sudo; error on non-zero exit.
/// Sync on purpose: only called from the CLI (interactive or root apply),
/// never from async API handlers.
pub fn run_shell(sudo: Option<&str>, args: &[&str]) -> Result<()> {
    let mut cmd = match sudo {
        Some(s) => {
            let mut c = std::process::Command::new(s);
            c.args(args);
            c
        }
        None => {
            let (first, rest) = args
                .split_first()
                .ok_or_else(|| anyhow!("empty command"))?;
            let mut c = std::process::Command::new(first);
            c.args(rest);
            c
        }
    };
    let status = cmd
        .status()
        .with_context(|| format!("failed to run: {args:?}"))?;
    if !status.success() {
        return Err(anyhow!("command failed ({args:?}): {status}"));
    }
    Ok(())
}

/// `None` when already root, else `Some("sudo")` when sudo exists.
pub fn sudo_prefix() -> Option<String> {
    let is_root = std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u32>().ok())
        .map(|u| u == 0)
        .unwrap_or(false);
    if is_root {
        None
    } else if std::process::Command::new("which")
        .arg("sudo")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        Some("sudo".to_string())
    } else {
        eprintln!(
            "⚠️ this operation needs root (run as root or with sudo); continuing best-effort"
        );
        None
    }
}

/// Result of a successful apply: where the rollback backups landed.
pub struct ApplyOutcome {
    pub backup_binary: PathBuf,
    pub backup_runner: Option<PathBuf>,
}

/// Back up + atomically install the staged binaries (step shared by the CLI
/// interactive path and `--apply-staged`). `sudo` may be `None` when root.
///
/// Safety: `install -m 755` replaces atomically; the running process keeps
/// executing the old inode until restarted.
pub fn apply_binaries(
    sudo: Option<&str>,
    install_dir: &Path,
    new_bin: &Path,
    new_runner: Option<&Path>,
    current_version: &str,
) -> Result<ApplyOutcome> {
    let cur_bin = install_dir.join("neomind");
    let cur_runner = install_dir.join("neomind-extension-runner");
    let bak_bin = install_dir.join(format!("neomind.{current_version}.bak"));
    let bak_runner = install_dir.join(format!("neomind-extension-runner.{current_version}.bak"));

    if cur_bin.exists() {
        run_shell(
            sudo,
            &["cp", "-a", &cur_bin.to_string_lossy(), &bak_bin.to_string_lossy()],
        )?;
    }
    if new_runner.is_some() && cur_runner.exists() {
        run_shell(
            sudo,
            &[
                "cp",
                "-a",
                &cur_runner.to_string_lossy(),
                &bak_runner.to_string_lossy(),
            ],
        )?;
    }

    run_shell(
        sudo,
        &[
            "install",
            "-m",
            "755",
            &new_bin.to_string_lossy(),
            &cur_bin.to_string_lossy(),
        ],
    )?;
    if let Some(runner) = new_runner {
        // Only swap the runner when one is already installed — a standalone
        // binary-only install shouldn't grow a runner it never had.
        if cur_runner.exists() {
            run_shell(
                sudo,
                &[
                    "install",
                    "-m",
                    "755",
                    &runner.to_string_lossy(),
                    &cur_runner.to_string_lossy(),
                ],
            )?;
        }
    }

    Ok(ApplyOutcome {
        backup_binary: bak_bin,
        backup_runner: if bak_runner.exists() {
            Some(bak_runner)
        } else {
            None
        },
    })
}

/// Stage-swap `web_tarball` into the live web dir (best-effort, atomic-ish).
///
/// Ownership follows `install.sh` (www-data preferred, neomind fallback) —
/// the old CLI path chowned to neomind unconditionally, which broke nginx
/// reads on www-data-owned installs.
///
/// Extraction happens as the INVOKING user in a dir they can write (next to
/// the tarball — the CLI temp dir or the API staging dir), then the result
/// is moved into place with sudo. Extracting directly into a sudo-created
/// stage would fail for a non-root caller (root-owned dir, non-root unpack).
pub fn apply_web_dir(sudo: Option<&str>, web_tarball: &Path) -> Result<bool> {
    let web = web_dir();
    let web_str = web.to_string_lossy().to_string();
    let stage = format!("{}.new.{}", web_str, std::process::id());
    let old = format!("{}.old.{}", web_str, std::process::id());

    let extract_dir = web_tarball
        .parent()
        .unwrap_or_else(|| Path::new("/tmp"))
        .join(format!("neomind-web-extract-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&extract_dir);
    extract_tar_gz(web_tarball, &extract_dir)?;

    run_shell(sudo, &["rm", "-rf", &stage])?;
    // `mv` falls back to copy across filesystems (e.g. /tmp → /var/www).
    let into_stage = run_shell(sudo, &["mv", &extract_dir.to_string_lossy(), &stage]);
    // No-op after a successful mv; cleans up when it failed.
    let _ = std::fs::remove_dir_all(&extract_dir);
    into_stage?;
    let owner = if std::process::Command::new("id")
        .arg("www-data")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        "www-data:www-data"
    } else {
        "neomind:neomind"
    };
    run_shell(sudo, &["chown", "-R", owner, &stage])?;
    run_shell(sudo, &["rm", "-rf", &old])?;
    run_shell(sudo, &["mv", &web_str, &old])?;
    let moved = run_shell(sudo, &["mv", &stage, &web_str]);
    run_shell(sudo, &["rm", "-rf", &old])?;
    moved.map(|_| true)
}

/// Locate the install dir from the running binary (canonicalized), with the
/// hardcoded default as fallback for exotic launch paths.
pub fn install_dir_from_exe() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_INSTALL_DIR))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_parses_dirty_versions() {
        assert_eq!(semver("0.9.21"), [0, 9, 21]);
        assert_eq!(semver("v1.2.3"), [1, 2, 3]);
        assert_eq!(semver("1.2.3-rc.1"), [1, 2, 3]);
        assert_eq!(semver("2"), [2, 0, 0]);
    }

    #[test]
    fn is_newer_compares_componentwise() {
        assert!(is_newer("0.9.22", "0.9.21"));
        assert!(is_newer("1.0.0", "0.9.99"));
        assert!(!is_newer("0.9.21", "0.9.21"));
        assert!(!is_newer("0.9.20", "0.9.21"));
        assert!(!is_newer("0.10.0-rc1", "0.10.0")); // pre-release NOT newer
    }

    #[test]
    fn urls_have_expected_shape() {
        assert_eq!(
            server_tarball_url("0.9.22", "arm64"),
            "https://github.com/camthink-ai/NeoMind/releases/download/v0.9.22/neomind-server-linux-arm64.tar.gz"
        );
        assert_eq!(
            web_tarball_url("0.9.22"),
            "https://github.com/camthink-ai/NeoMind/releases/download/v0.9.22/neomind-web.tar.gz"
        );
    }

    #[test]
    fn staging_paths_nest_under_data_dir() {
        let root = staging_root(Path::new("/var/lib/neomind"));
        assert_eq!(root, Path::new("/var/lib/neomind/upgrade"));
        assert_eq!(
            staging_dir(Path::new("/var/lib/neomind"), "0.9.22"),
            Path::new("/var/lib/neomind/upgrade/v0.9.22")
        );
    }

    #[test]
    fn manifest_round_trips() {
        let m = StagingManifest {
            version: "0.9.22".into(),
            arch: "amd64".into(),
            staged_at: 12345,
            server_binary: "neomind".into(),
            extension_runner: Some("neomind-extension-runner".into()),
            web_tarball: Some("neomind-web.tar.gz".into()),
        };
        let json = serde_json::to_string(&m).unwrap();
        let back: StagingManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.version, "0.9.22");
        assert_eq!(back.web_tarball.as_deref(), Some("neomind-web.tar.gz"));
    }
}
