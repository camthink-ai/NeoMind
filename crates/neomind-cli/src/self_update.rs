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
        .timeout(std::time::Duration::from_secs(30))
        .user_agent(neomind_core::brand::user_agent())
        .build()?;

    // 1. Resolve target version (explicit pin, else latest release tag).
    let target = match &version {
        Some(v) => v.trim_start_matches('v').to_string(),
        None => up::github_latest_version(&client).await?,
    };

    // 2. Skip if already on the latest (unless an explicit --version was given).
    if version.is_none() && !up::is_newer(&target, APP_VERSION) {
        println!("Already up to date ({APP_VERSION}).");
        return Ok(());
    }
    println!("Upgrade available: {APP_VERSION} → v{target}");

    if !yes {
        println!("\nProceed with upgrade? [y/N] ");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Aborted.");
            return Ok(());
        }
    }

    let arch = up::host_arch();

    // 3. Download + extract the server tarball into a temp dir.
    let tmp =
        std::env::temp_dir().join(format!("neomind-upgrade-{}-{}", std::process::id(), target));
    if tmp.exists() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    std::fs::create_dir_all(&tmp)?;

    let url = up::server_tarball_url(&target, arch);
    println!("Downloading {url}");
    let tarball = tmp.join("neomind.tar.gz");
    up::download_to_file(&client, &url, &tarball, None).await?;

    println!("Extracting...");
    up::extract_tar_gz(&tarball, &tmp)?;

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

    // 9. Best-effort web-frontend swap (only if the web dir + a web tarball exist).
    if up::web_dir().is_dir() {
        let web_url = up::web_tarball_url(&target);
        if let Ok(resp) = client.get(&web_url).send().await {
            if resp.status().is_success() {
                if let Ok(bytes) = resp.bytes().await {
                    let web_tgz = tmp.join("neomind-web.tar.gz");
                    if std::fs::write(&web_tgz, &bytes).is_ok()
                        && up::apply_web_dir(sudo.as_deref(), &web_tgz).is_ok()
                    {
                        println!("Frontend updated → {}", up::web_dir().display());
                    }
                }
            }
        }
    }

    // 10. Restart the service (if it was running).
    if svc_active {
        println!("Starting neomind service...");
        let _ = up::run_shell(sudo.as_deref(), &["systemctl", "start", "neomind"]);
    }

    // 11. Clean up temp + verify.
    let _ = std::fs::remove_dir_all(&tmp);

    println!("\nVerifying...");
    if svc_active {
        if systemctl_is_active() {
            println!("✅ neomind upgraded to v{target} (service active)");
        } else {
            eprintln!("⚠️ service did not come back up — check: sudo systemctl status neomind");
        }
    } else {
        println!("✅ neomind upgraded to v{target} (no systemd service detected — restart manually)");
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
    let wanted_version = trigger_content
        .as_deref()
        .and_then(|s| s.split_whitespace().next())
        .map(str::to_string);

    let manifest_path = find_staged_manifest(&staging_root, wanted_version.as_deref())?;
    let manifest: up::StagingManifest = serde_json::from_str(&std::fs::read_to_string(
        &manifest_path,
    )?)
    .context("staging manifest is not valid JSON")?;
    let staging = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("staging manifest has no parent dir"))?
        .to_path_buf();
    println!(
        "Applying staged upgrade v{} (staged {}s ago, arch {}{})",
        manifest.version,
        chrono_now_millis().saturating_sub(manifest.staged_at) / 1000,
        manifest.arch,
        wanted_version
            .as_ref()
            .map(|v| format!(", requested by trigger: v{v}"))
            .unwrap_or_default()
    );

    // Re-verify before touching anything — the manifest is on disk, the
    // staging dir could have been tampered with or truncated.
    let new_bin = staging.join(&manifest.server_binary);
    if !new_bin.exists() {
        return Err(anyhow!(
            "staged binary missing: {} — aborting (service untouched)",
            new_bin.display()
        ));
    }
    let vstr = up::verify_binary_version(&new_bin, &manifest.version)?;
    println!("Verified staged binary: {vstr}");

    let new_runner = manifest
        .extension_runner
        .as_ref()
        .map(|r| staging.join(r))
        .filter(|p| p.exists());
    let web_tarball = manifest
        .web_tarball
        .as_ref()
        .map(|w| staging.join(w))
        .filter(|p| p.exists());

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

    println!("✅ staged upgrade to v{} applied", manifest.version);
    print_rollback_hint(&install_dir, &outcome);
    Ok(())
}

/// The manifest the trigger asked for (`upgrade/v<version>/manifest.json`);
/// when the trigger carried no version, fall back to the newest `staged_at`.
/// Stale/junk staging dirs can never hijack a named apply.
fn find_staged_manifest(root: &std::path::Path, wanted: Option<&str>) -> Result<PathBuf> {
    let entries = std::fs::read_dir(root)
        .with_context(|| format!("no staging dir at {} — nothing to apply", root.display()))?;

    let mut newest: Option<(i64, PathBuf)> = None;
    for entry in entries.flatten() {
        let manifest = entry.path().join("manifest.json");
        let Ok(content) = std::fs::read_to_string(&manifest) else {
            continue;
        };
        let Ok(m) = serde_json::from_str::<up::StagingManifest>(&content) else {
            continue;
        };
        if let Some(v) = wanted {
            if m.version == v {
                return Ok(manifest);
            }
        }
        if newest.as_ref().map(|(t, _)| m.staged_at > *t).unwrap_or(true) {
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
    let _ = up::run_shell(sudo.as_deref(), &["rm", "-f", &dir.join("neomind").to_string_lossy()]);
    let _ = up::run_shell(
        sudo.as_deref(),
        &[
            "rm",
            "-f",
            &dir.join("neomind-extension-runner").to_string_lossy(),
        ],
    );
    let _ = up::run_shell(
        sudo.as_deref(),
        &["sh", "-c", &format!("rm -f {}/neomind*.bak*", dir.display())],
    );

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
        println!(
            "(data dir {DATA_DIR} retained — remove manually or re-run with --purge)"
        );
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
}
