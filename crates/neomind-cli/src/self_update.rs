//! `neomind upgrade` + `neomind uninstall` — self-management of the server
//! binary (Linux/systemd deployments), mirroring `scripts/install.sh`.
//!
//! `upgrade`: check latest release → download the host-arch server tarball →
//! verify the new binary's version → back up the current binary → swap in →
//! restart the systemd service if one is running. Best-effort web-frontend
//! swap when the web dir exists.
//!
//! `upgrade --apply-staged`: root-side apply step of the web-triggered
//! upgrade. Reads the manifest the API staged under the data dir, applies it
//! (same backup/swap/web-swap/restart sequence), and cleans up. Invoked by
//! the `neomind-upgrade-apply.service` helper unit — not interactive.
//!
//! `uninstall`: stop + disable the service, remove the binary + service unit;
//! `--purge` also deletes the data + web dirs.
//!
//! On non-Linux hosts these print a hint to re-run `install.sh` (the installer
//! handles macOS launchd etc.).
//!
//! Release/semver/download/apply primitives live in
//! `neomind_api::upgrade::common` so the web-triggered path (API staging) and
//! this CLI share one implementation.

use anyhow::{anyhow, Context, Result};
use std::path::PathBuf;

use neomind_api::upgrade::common as up;
use neomind_core::brand::APP_VERSION;

const DATA_DIR: &str = "/var/lib/neomind";

pub async fn run_upgrade(version: Option<String>, yes: bool) -> Result<()> {
    if !cfg!(target_os = "linux") {
        eprintln!("`neomind upgrade` targets Linux (systemd) deployments.");
        eprintln!(
            "On other OS, re-run the installer:\n  curl -fsSL https://raw.githubusercontent.com/{}/main/scripts/install.sh | sh",
            up::REPO
        );
        return Ok(());
    }

    println!("NeoMind {APP_VERSION} — checking for updates...");

    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .read_timeout(std::time::Duration::from_secs(60))
        .user_agent(neomind_core::brand::user_agent())
        .build()?;

    // 1. Resolve and strictly validate target version (explicit pin, else latest release tag).
    let raw_target = match &version {
        Some(v) => v.clone(),
        None => up::github_latest_version(&client).await?,
    };
    let target = up::validate_semver_version(&raw_target)
        .map_err(|e| anyhow!("invalid target version: {e}"))?;

    // 2. Skip if already on the latest (unless an explicit --version was given).
    if version.is_none() && !up::is_newer(&target, APP_VERSION) {
        println!("Already up to date ({APP_VERSION}).");
        return Ok(());
    }
    println!("Upgrade available: {APP_VERSION} → v{target}");

    if !yes {
        use std::io::IsTerminal;
        if !std::io::stdin().is_terminal() {
            // Non-interactive caller (agent/script/pipe): refuse instead of
            // blocking on a stdin that will never answer.
            anyhow::bail!(
                "Upgrade to v{target} available — re-run with --yes to proceed non-interactively"
            );
        }
        println!("\nProceed with upgrade? [y/N] ");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Aborted.");
            return Ok(());
        }
    }

    let arch = up::host_arch();
    let server_asset = up::server_tarball_filename(arch)?;

    // Fetch release checksums when available (HTTP 200 → strictly enforced).
    let release_checksums = up::fetch_release_checksums_optional(&client, &target).await?;

    // 3. Download + extract the server tarball into a private (0700) temp dir.
    let tmp_dir = up::create_upgrade_tempdir()?;
    let tmp = tmp_dir.path();

    let url = up::server_tarball_url(&target, arch)?;
    println!("Downloading {url}");
    let tarball = tmp.join("neomind.tar.gz");
    up::download_to_file(&client, &url, &tarball, None).await?;

    if let Some(checksums) = release_checksums.as_ref() {
        match checksums.get(&server_asset) {
            Some(expected_sha) => {
                up::verify_file_sha256(&tarball, expected_sha)?;
                println!("Verified SHA-256 checksum for {server_asset}");
            }
            None if !up::allow_unverified_upgrade() => {
                return Err(anyhow!(
                    "release checksums.txt does not contain an entry for {server_asset}"
                ));
            }
            None => {
                eprintln!(
                    "⚠️ {server_asset} missing from checksums.txt (continuing due to NEOMIND_ALLOW_UNVERIFIED_UPGRADE=1)"
                );
            }
        }
    }

    println!("Extracting...");
    up::extract_tar_gz(&tarball, tmp)?;

    let new_bin = tmp.join("neomind");
    let new_runner = tmp.join("neomind-extension-runner");
    if !new_bin.exists() {
        return Err(anyhow!(
            "extracted tarball has no `neomind` binary — aborting (service untouched)"
        ));
    }

    // 4. Verify the downloaded binary reports the target version before touching
    //    anything (safety: never swap in a wrong/older/foreign binary).
    let vstr = up::verify_binary_version(&new_bin, &target)?;
    println!("Verified downloaded binary: {vstr}");

    // 5. Locate the running binary + detect a systemd service.
    let install_dir = up::install_dir_from_exe();
    let svc_active = systemctl_is_active();

    let sudo = up::sudo_prefix();

    // 6. Stop the service (if running) before swapping.
    if svc_active {
        println!("Stopping neomind service...");
        let _ = up::run_shell(sudo.as_deref(), &["systemctl", "stop", "neomind"]);
    }

    // 7+8. Back up + atomically install (shared with the staged path).
    let runner_arg = new_runner.exists().then_some(new_runner.as_path());
    println!("Installing → {}", install_dir.join("neomind").display());
    let outcome = up::apply_binaries(
        sudo.as_deref(),
        &install_dir,
        &new_bin,
        runner_arg,
        APP_VERSION,
    )?;

    // 9. Best-effort web-frontend swap (streamed to disk, never buffered in RAM).
    if up::web_dir().is_dir() {
        let web_url = up::web_tarball_url(&target)?;
        let web_tgz = tmp.join("neomind-web.tar.gz");
        if up::download_to_file(&client, &web_url, &web_tgz, None)
            .await
            .is_ok()
        {
            let web_ok = match release_checksums
                .as_ref()
                .and_then(|c| c.get("neomind-web.tar.gz"))
            {
                Some(expected_sha) => match up::verify_file_sha256(&web_tgz, expected_sha) {
                    Ok(()) => true,
                    Err(e) => {
                        eprintln!("⚠️ frontend tarball checksum verification failed: {e}");
                        false
                    }
                },
                None => true,
            };
            if web_ok && up::apply_web_dir(sudo.as_deref(), &web_tgz).is_ok() {
                println!("Frontend updated → {}", up::web_dir().display());
            }
        }
    }

    // 10. Restart the service (if it was running).
    if svc_active {
        println!("Starting neomind service...");
        let _ = up::run_shell(sudo.as_deref(), &["systemctl", "start", "neomind"]);
    }

    // 11. Temp dir auto-cleaned on drop.
    drop(tmp_dir);

    println!("\nVerifying...");
    if svc_active {
        if systemctl_is_active() {
            println!("✅ neomind upgraded to v{target} (service active)");
        } else {
            eprintln!("⚠️ service did not come back up — check: sudo systemctl status neomind");
        }
    } else {
        println!(
            "✅ neomind upgraded to v{target} (no systemd service detected — restart manually)"
        );
    }
    print_rollback_hint(&install_dir, &outcome);
    Ok(())
}

/// `neomind upgrade --apply-staged` — apply what the API staged.
///
/// Runs as root via `neomind-upgrade-apply.service` (started by its `.path`
/// unit when the API writes the trigger file). The staging dir follows the
/// same data-dir resolution as the API (`NEOMIND_DATA_DIR` else `data/`, set
/// as WorkingDirectory/Environment by the installer's units), so both sides
/// agree without configuration.
pub fn run_apply_staged() -> Result<()> {
    if !cfg!(target_os = "linux") {
        return Err(anyhow!("--apply-staged targets Linux systemd deployments"));
    }

    let staging_root = up::staging_root(&neomind_core::paths::data_dir());

    // Consume the trigger FIRST (re-arm even on failure), but keep its
    // content: the API writes "<version> <staged_at-ms>" into it, and that
    // named version — not "whichever staging dir has the newest timestamp" —
    // is the one to apply. Selecting by timestamp alone lets a stale or
    // junk staging dir with a bogus future staged_at hijack the apply.
    let trigger_path = staging_root.join(up::APPLY_TRIGGER_FILE);
    let trigger_content = std::fs::read_to_string(&trigger_path).ok();
    let _ = std::fs::remove_file(&trigger_path);
    let wanted_version = match trigger_content
        .as_deref()
        .and_then(|s| s.split_whitespace().next())
    {
        Some(raw) => Some(
            up::validate_semver_version(raw)
                .map_err(|e| anyhow!("invalid version in apply trigger: {e}"))?,
        ),
        None => None,
    };

    let manifest_path = find_staged_manifest(&staging_root, wanted_version.as_deref())?;
    let manifest: up::StagingManifest =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path)?)
            .context("staging manifest is not valid JSON")?;
    let version = up::validate_semver_version(&manifest.version)
        .map_err(|e| anyhow!("invalid version in staging manifest: {e}"))?;
    let arch = up::validate_arch(&manifest.arch)?;
    if arch != up::host_arch() {
        return Err(anyhow!(
            "staged arch {arch} does not match host arch {}",
            up::host_arch()
        ));
    }
    let staging = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("staging manifest has no parent dir"))?
        .to_path_buf();
    println!(
        "Applying staged upgrade v{} (staged {}s ago, arch {}{})",
        version,
        chrono_now_millis().saturating_sub(manifest.staged_at) / 1000,
        arch,
        wanted_version
            .as_ref()
            .map(|v| format!(", requested by trigger: v{v}"))
            .unwrap_or_default()
    );

    // Resolve all manifest paths strictly inside `staging`, rejecting traversal
    // and symlinks before touching or executing anything as root.
    let staged_bin = up::resolve_staged_file(&staging, &manifest.server_binary)?;
    let staged_runner = match manifest.extension_runner.as_deref() {
        Some(r) => Some(up::resolve_staged_file(&staging, r)?),
        None => None,
    };
    let web_tarball = match manifest.web_tarball.as_deref() {
        Some(w) => Some(up::resolve_staged_file(&staging, w)?),
        None => None,
    };

    // Root privilege boundary: fetch checksums.txt directly over HTTPS from the
    // pinned GitHub release URL (never trusting unprivileged manifest.json) and
    // verify SHA-256 BEFORE executing verify_binary_version or installing.
    let _verified_tmp: Option<up::UpgradeTempDir>;
    let (new_bin, new_runner, release_checksums) = if up::allow_unverified_upgrade() {
        eprintln!(
            "⚠️ NEOMIND_ALLOW_UNVERIFIED_UPGRADE=1 is set — skipping release SHA-256 verification"
        );
        _verified_tmp = None;
        (staged_bin, staged_runner, None)
    } else {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(30))
            .read_timeout(std::time::Duration::from_secs(60))
            .user_agent(neomind_core::brand::user_agent())
            .build()?;
        let checksums = block_on_async(up::fetch_release_checksums(&client, &version))
            .with_context(|| {
                format!(
                    "refusing unverified root apply for v{version}: could not fetch official checksums.txt \
                     (set NEOMIND_ALLOW_UNVERIFIED_UPGRADE=1 in root env to override)"
                )
            })?;

        let server_asset = up::server_tarball_filename(arch)?;
        if let Some(expected_tarball_sha) = checksums.get(&server_asset) {
            // Verify the staged tarball against GitHub's checksums.txt and extract
            // into a root-owned 0700 directory so an unprivileged `neomind` user
            // cannot tamper with extracted binaries (TOCTOU protection).
            let tarball_name = manifest.server_tarball.as_deref().unwrap_or("neomind.tar.gz");
            let staged_tarball = up::resolve_staged_file(&staging, tarball_name)?;
            up::verify_file_sha256(&staged_tarball, expected_tarball_sha)?;
            println!("Verified SHA-256 checksum for {server_asset}");

            let root_tmp = up::create_upgrade_tempdir()?;
            up::extract_tar_gz(&staged_tarball, root_tmp.path())?;
            let extracted_bin = up::resolve_staged_file(root_tmp.path(), "neomind")?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&extracted_bin, std::fs::Permissions::from_mode(0o755))?;
            }
            let extracted_runner = if root_tmp.path().join("neomind-extension-runner").exists() {
                Some(up::resolve_staged_file(
                    root_tmp.path(),
                    "neomind-extension-runner",
                )?)
            } else {
                None
            };
            _verified_tmp = Some(root_tmp);
            (extracted_bin, extracted_runner, Some(checksums))
        } else if let Some(expected_bin_sha) = checksums.get(&manifest.server_binary) {
            up::verify_file_sha256(&staged_bin, expected_bin_sha)?;
            if let (Some(runner_name), Some(runner_path)) =
                (manifest.extension_runner.as_deref(), staged_runner.as_ref())
            {
                let expected_runner_sha = checksums.get(runner_name).ok_or_else(|| {
                    anyhow!("checksums.txt missing entry for staged runner {runner_name:?}")
                })?;
                up::verify_file_sha256(runner_path, expected_runner_sha)?;
            }
            println!("Verified SHA-256 checksum for {}", manifest.server_binary);
            _verified_tmp = None;
            (staged_bin, staged_runner, Some(checksums))
        } else {
            return Err(anyhow!(
                "release checksums.txt for v{version} has no entry for {server_asset} — aborting"
            ));
        }
    };

    // If a web tarball is staged and we are enforcing checksums, verify it too.
    if let (Some(web_tgz), Some(checksums)) = (web_tarball.as_ref(), release_checksums.as_ref()) {
        let expected_web_sha = checksums.get("neomind-web.tar.gz").ok_or_else(|| {
            anyhow!("release checksums.txt for v{version} has no entry for neomind-web.tar.gz")
        })?;
        up::verify_file_sha256(web_tgz, expected_web_sha)?;
        println!("Verified SHA-256 checksum for neomind-web.tar.gz");
    }

    let vstr = up::verify_binary_version(&new_bin, &version)?;
    println!("Verified staged binary: {vstr}");

    let install_dir = up::install_dir_from_exe();
    let sudo = up::sudo_prefix();

    let outcome = up::apply_binaries(
        sudo.as_deref(),
        &install_dir,
        &new_bin,
        new_runner.as_deref(),
        APP_VERSION,
    )?;
    println!("Installed → {}", install_dir.join("neomind").display());

    if let Some(web_tgz) = web_tarball.as_ref() {
        match up::apply_web_dir(sudo.as_deref(), web_tgz) {
            Ok(_) => println!("Frontend updated → {}", up::web_dir().display()),
            Err(e) => eprintln!("⚠️ frontend swap failed (binary upgrade still applied): {e}"),
        }
    }

    // Restart the service it was triggered from (unit exists on disk even if
    // the API stopped it mid-shutdown).
    if systemctl_is_active() || std::path::Path::new("/etc/systemd/system/neomind.service").exists()
    {
        println!("Restarting neomind service...");
        up::run_shell(sudo.as_deref(), &["systemctl", "restart", "neomind"])?;
    }

    // Clean the staging dir this manifest came from (keep others — an older
    // staged version might still be wanted for a manual rollback-by-retry).
    let _ = std::fs::remove_dir_all(&staging);

    println!("✅ staged upgrade to v{} applied", version);
    print_rollback_hint(&install_dir, &outcome);
    Ok(())
}

/// The manifest the trigger asked for (`upgrade/v<version>/manifest.json`);
/// when the trigger carried no version, fall back to the newest `staged_at`.
/// Stale/junk staging dirs can never hijack a named apply.
fn find_staged_manifest(root: &std::path::Path, wanted: Option<&str>) -> Result<PathBuf> {
    if let Some(v) = wanted {
        up::validate_semver_version(v)
            .map_err(|e| anyhow!("invalid requested version: {e}"))?;
    }
    let entries = std::fs::read_dir(root)
        .with_context(|| format!("no staging dir at {} — nothing to apply", root.display()))?;

    let mut newest: Option<(i64, PathBuf)> = None;
    for entry in entries.flatten() {
        // Never follow symlinks inside staging_root.
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if !ft.is_dir() {
            continue;
        }
        let manifest = entry.path().join("manifest.json");
        if std::fs::symlink_metadata(&manifest)
            .map(|m| !m.file_type().is_file())
            .unwrap_or(true)
        {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&manifest) else {
            continue;
        };
        let Ok(m) = serde_json::from_str::<up::StagingManifest>(&content) else {
            continue;
        };
        let Ok(clean_ver) = up::validate_semver_version(&m.version) else {
            continue;
        };
        if let Some(v) = wanted {
            if clean_ver == v {
                return Ok(manifest);
            }
        }
        if newest
            .as_ref()
            .map(|(t, _)| m.staged_at > *t)
            .unwrap_or(true)
        {
            newest = Some((m.staged_at, manifest));
        }
    }
    if let Some(v) = wanted {
        return Err(anyhow!(
            "trigger requested v{v} but no such staging exists under {}",
            root.display()
        ));
    }
    newest
        .map(|(_, p)| p)
        .ok_or_else(|| anyhow!("no staged upgrade manifest under {}", root.display()))
}

pub async fn run_uninstall(purge: bool, yes: bool) -> Result<()> {
    if !cfg!(target_os = "linux") {
        eprintln!("`neomind uninstall` targets Linux. On other OS, remove the binary + service files manually.");
        return Ok(());
    }

    println!("This will stop + disable the neomind service and remove the binary + service unit.");
    if purge {
        println!(
            "⚠️ --purge will ALSO DELETE {} and {} (irreversible).",
            DATA_DIR,
            up::web_dir().display()
        );
    }
    if !yes {
        use std::io::IsTerminal;
        if !std::io::stdin().is_terminal() {
            anyhow::bail!(
                "Uninstall requires confirmation — re-run with --yes to proceed non-interactively"
            );
        }
        println!("\nProceed? [y/N] ");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Aborted.");
            return Ok(());
        }
    }

    let sudo = up::sudo_prefix();

    // 1. Stop + disable the systemd service (best-effort).
    println!("Stopping + disabling neomind service...");
    let _ = up::run_shell(sudo.as_deref(), &["systemctl", "stop", "neomind"]);
    let _ = up::run_shell(sudo.as_deref(), &["systemctl", "disable", "neomind"]);

    // 2. Remove the service units + reload (main unit + the upgrade helper
    //    service/path pair).
    let _ = up::run_shell(
        sudo.as_deref(),
        &["systemctl", "disable", "--now", up::APPLY_PATH_UNIT_NAME],
    );
    let _ = up::run_shell(
        sudo.as_deref(),
        &["rm", "-f", "/etc/systemd/system/neomind.service"],
    );
    let _ = up::run_shell(
        sudo.as_deref(),
        &[
            "rm",
            "-f",
            &format!("/etc/systemd/system/{}", up::APPLY_UNIT_NAME),
        ],
    );
    let _ = up::run_shell(
        sudo.as_deref(),
        &[
            "rm",
            "-f",
            &format!("/etc/systemd/system/{}", up::APPLY_PATH_UNIT_NAME),
        ],
    );
    let _ = up::run_shell(sudo.as_deref(), &["systemctl", "daemon-reload"]);

    // 3. Remove the binaries (current + .bak rollback copies) from the install dir.
    let dir = up::install_dir_from_exe();
    println!("Removing binaries from {}", dir.display());
    let _ = up::run_shell(
        sudo.as_deref(),
        &["rm", "-f", "--", &dir.join("neomind").to_string_lossy()],
    );
    let _ = up::run_shell(
        sudo.as_deref(),
        &[
            "rm",
            "-f",
            "--",
            &dir.join("neomind-extension-runner").to_string_lossy(),
        ],
    );
    for bak in find_backup_binaries(&dir) {
        let bak_str = bak.to_string_lossy();
        let _ = up::run_shell(sudo.as_deref(), &["rm", "-f", "--", &bak_str]);
    }

    // 4. Optional purge of data + web.
    if purge {
        println!("Removing data dir {DATA_DIR} (--purge)");
        let _ = up::run_shell(sudo.as_deref(), &["rm", "-rf", DATA_DIR]);
        let _ = up::run_shell(
            sudo.as_deref(),
            &["rm", "-rf", &up::web_dir().to_string_lossy()],
        );
    }

    println!("✅ NeoMind uninstalled.");
    if !purge {
        println!("(data dir {DATA_DIR} retained — remove manually or re-run with --purge)");
    }
    Ok(())
}

// ---- helpers ----

fn systemctl_is_active() -> bool {
    std::process::Command::new("systemctl")
        .args(["is-active", "--quiet", "neomind"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn print_rollback_hint(install_dir: &std::path::Path, outcome: &up::ApplyOutcome) {
    let cur_bin = install_dir.join("neomind");
    let cur_runner = install_dir.join("neomind-extension-runner");
    match &outcome.backup_runner {
        Some(bak_runner) => println!(
            "Rollback: sudo cp -a {} {} && sudo cp -a {} {} && sudo systemctl restart neomind",
            outcome.backup_binary.display(),
            cur_bin.display(),
            bak_runner.display(),
            cur_runner.display(),
        ),
        None => println!(
            "Rollback: sudo cp -a {} {} && sudo systemctl restart neomind",
            outcome.backup_binary.display(),
            cur_bin.display(),
        ),
    }
}

fn chrono_now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Execute an async future from synchronous `run_apply_staged`, supporting both
/// an existing multi-threaded Tokio runtime (`#[tokio::main]`) and standalone callers.
fn block_on_async<F: std::future::Future>(fut: F) -> F::Output {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| handle.block_on(fut))
    } else {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build temporary tokio runtime")
            .block_on(fut)
    }
}

/// Enumerate `neomind*.bak*` files inside `dir` without invoking a shell.
fn find_backup_binaries(dir: &std::path::Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name_str = name.to_str()?;
            if name_str.starts_with("neomind") && name_str.contains(".bak") {
                Some(entry.path())
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_manifest(dir: &std::path::Path, version: &str, staged_at: i64) {
        let d = dir.join(format!("v{version}"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("manifest.json"),
            serde_json::json!({
                "version": version,
                "arch": "arm64",
                "staged_at": staged_at,
                "server_binary": "neomind",
            })
            .to_string(),
        )
        .unwrap();
    }

    #[test]
    fn named_trigger_selects_its_version_not_the_newest() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("upgrade");
        std::fs::create_dir_all(&root).unwrap();
        // Junk staging with a bogus future timestamp + the real target.
        write_manifest(&root, "9.9.9", i64::MAX);
        write_manifest(&root, "0.9.20", 1000);

        let picked =
            find_staged_manifest(&root, Some("0.9.20")).expect("named manifest must be found");
        assert!(picked.ends_with("v0.9.20/manifest.json"));

        let err = find_staged_manifest(&root, Some("8.8.8")).unwrap_err();
        assert!(err.to_string().contains("no such staging"));
    }

    #[test]
    fn without_trigger_version_falls_back_to_newest() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("upgrade");
        std::fs::create_dir_all(&root).unwrap();
        write_manifest(&root, "0.9.19", 1000);
        write_manifest(&root, "0.9.20", 2000);

        let picked = find_staged_manifest(&root, None).expect("fallback must find newest");
        assert!(picked.ends_with("v0.9.20/manifest.json"));
    }

    #[test]
    fn empty_staging_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("upgrade");
        std::fs::create_dir_all(&root).unwrap();
        assert!(find_staged_manifest(&root, None).is_err());
        assert!(find_staged_manifest(&root, Some("0.9.20")).is_err());
    }

    #[test]
    fn find_staged_manifest_rejects_invalid_version() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("upgrade");
        std::fs::create_dir_all(&root).unwrap();
        write_manifest(&root, "0.9.20", 1000);
        assert!(find_staged_manifest(&root, Some("../etc")).is_err());
        assert!(find_staged_manifest(&root, Some("0.9.20?x=1")).is_err());
    }

    #[test]
    fn find_backup_binaries_matches_only_neomind_bak_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("neomind.1.3.0.bak"), b"").unwrap();
        std::fs::write(dir.join("neomind-extension-runner.1.3.0.bak"), b"").unwrap();
        std::fs::write(dir.join("neomind"), b"").unwrap();
        std::fs::write(dir.join("other.bak"), b"").unwrap();

        let mut found: Vec<String> = find_backup_binaries(dir)
            .into_iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        found.sort();
        assert_eq!(
            found,
            vec!["neomind-extension-runner.1.3.0.bak", "neomind.1.3.0.bak"]
        );
    }
}
