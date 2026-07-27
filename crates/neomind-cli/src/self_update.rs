//! `neomind upgrade` + `neomind uninstall` — self-management of the server
//! binary (Linux/systemd deployments), mirroring `scripts/install.sh`.
//!
//! `upgrade`: check latest release → download the host-arch server tarball →
//! verify the new binary's version → back up the current binary → swap in →
//! restart the systemd service if one is running. Best-effort web-frontend
//! swap when `/var/www/neomind` exists.
//!
//! `uninstall`: stop + disable the service, remove the binary + service unit;
//! `--purge` also deletes the data + web dirs.
//!
//! On non-Linux hosts these print a hint to re-run `install.sh` (the installer
//! handles macOS launchd etc.).

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

const REPO: &str = "camthink-ai/NeoMind";
const DEFAULT_INSTALL_DIR: &str = "/usr/local/bin";
const DATA_DIR: &str = "/var/lib/neomind";
const WEB_DIR: &str = "/var/www/neomind";

pub async fn run_upgrade(version: Option<String>, yes: bool) -> Result<()> {
    use neomind_core::brand::APP_VERSION;

    if !cfg!(target_os = "linux") {
        eprintln!("`neomind upgrade` targets Linux (systemd) deployments.");
        eprintln!(
            "On other OS, re-run the installer:\n  curl -fsSL https://raw.githubusercontent.com/{}/main/scripts/install.sh | sh",
            REPO
        );
        return Ok(());
    }

    println!("NeoMind {} — checking for updates...", APP_VERSION);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent(neomind_core::brand::user_agent())
        .build()?;

    // 1. Resolve target version (explicit pin, else latest release tag).
    let target = match &version {
        Some(v) => v.trim_start_matches('v').to_string(),
        None => {
            let resp: serde_json::Value = client
                .get(format!(
                    "https://api.github.com/repos/{}/releases/latest",
                    REPO
                ))
                .send()
                .await?
                .json()
                .await?;
            resp["tag_name"]
                .as_str()
                .ok_or_else(|| anyhow!("release has no tag_name"))?
                .trim_start_matches('v')
                .to_string()
        }
    };

    // 2. Skip if already on the latest (unless an explicit --version was given).
    if version.is_none() && !is_newer(&target, APP_VERSION) {
        println!("Already up to date ({}).", APP_VERSION);
        return Ok(());
    }
    println!("Upgrade available: {} → v{}", APP_VERSION, target);

    if !yes {
        println!("\nProceed with upgrade? [y/N] ");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Aborted.");
            return Ok(());
        }
    }

    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "amd64"
    };

    // 3. Download + extract the server tarball into a temp dir.
    let tmp =
        std::env::temp_dir().join(format!("neomind-upgrade-{}-{}", std::process::id(), target));
    if tmp.exists() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    std::fs::create_dir_all(&tmp)?;

    let url = format!(
        "https://github.com/{}/releases/download/v{}/neomind-server-linux-{}.tar.gz",
        REPO, target, arch
    );
    println!("Downloading {}", url);
    let bytes = client
        .get(&url)
        .send()
        .await?
        .error_for_status()
        .context("download failed")?
        .bytes()
        .await?;
    let tarball = tmp.join("neomind.tar.gz");
    std::fs::write(&tarball, &bytes)?;

    println!("Extracting...");
    run(Command::new("tar").args(["xzf", tarball_str(&tarball)?, "-C", path_str(&tmp)?]))?;

    let new_bin = tmp.join("neomind");
    let new_runner = tmp.join("neomind-extension-runner");
    if !new_bin.exists() {
        return Err(anyhow!(
            "extracted tarball has no `neomind` binary — aborting (service untouched)"
        ));
    }

    // 4. Verify the downloaded binary reports the target version before touching
    //    anything (safety: never swap in a wrong/older/foreign binary).
    let vout = Command::new(new_bin.as_os_str())
        .arg("--version")
        .output()
        .context("could not run the downloaded binary")?;
    let vstr = String::from_utf8_lossy(&vout.stdout).trim().to_string();
    if !vstr.ends_with(&target) && !vstr.contains(&format!(" {}", target)) {
        return Err(anyhow!(
            "downloaded binary version mismatch: got {:?}, expected v{} — aborting",
            vstr,
            target
        ));
    }
    println!("Verified downloaded binary: {}", vstr);

    // 5. Locate the running binary + detect a systemd service.
    let cur_bin = std::env::current_exe()
        .context("cannot resolve current binary path")?
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_INSTALL_DIR).join("neomind"));
    let install_dir = cur_bin
        .parent()
        .context("binary has no parent dir")?
        .to_path_buf();
    let cur_runner = install_dir.join("neomind-extension-runner");
    let svc_active = Command::new("systemctl")
        .args(["is-active", "--quiet", "neomind"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    let sudo = sudo_prefix();

    // 6. Stop the service (if running) before swapping.
    if svc_active {
        println!("Stopping neomind service...");
        let _ = sh(sudo.as_deref(), &["systemctl", "stop", "neomind"]);
    }

    // 7. Back up the current binaries for rollback.
    let bak_bin = install_dir.join(format!("neomind.{}.bak", APP_VERSION));
    let bak_runner = install_dir.join(format!("neomind-extension-runner.{}.bak", APP_VERSION));
    if cur_bin.exists() {
        println!("Backup → {}", bak_bin.display());
        let _ = sh(
            sudo.as_deref(),
            &["cp", "-a", path_str(&cur_bin)?, path_str(&bak_bin)?],
        );
    }
    if cur_runner.exists() {
        let _ = sh(
            sudo.as_deref(),
            &["cp", "-a", path_str(&cur_runner)?, path_str(&bak_runner)?],
        );
    }

    // 8. Install (atomic, mode 755).
    println!("Installing → {}", cur_bin.display());
    sh(
        sudo.as_deref(),
        &[
            "install",
            "-m",
            "755",
            path_str(&new_bin)?,
            path_str(&cur_bin)?,
        ],
    )?;
    if new_runner.exists() && cur_runner.exists() {
        sh(
            sudo.as_deref(),
            &[
                "install",
                "-m",
                "755",
                path_str(&new_runner)?,
                path_str(&cur_runner)?,
            ],
        )?;
    }

    // 9. Best-effort web-frontend swap (only if the web dir + a web tarball exist).
    if Path::new(WEB_DIR).is_dir() {
        let web_url = format!(
            "https://github.com/{}/releases/download/v{}/neomind-web-{}.tar.gz",
            REPO, target, target
        );
        if let Ok(resp) = client.get(&web_url).send().await {
            if resp.status().is_success() {
                if let Ok(bytes) = resp.bytes().await {
                    let web_tgz = tmp.join("neomind-web.tar.gz");
                    if std::fs::write(&web_tgz, &bytes).is_ok() {
                        let stage = format!("{}.new.{}", WEB_DIR, std::process::id());
                        let _ = sh(sudo.as_deref(), &["rm", "-rf", &stage]);
                        let _ = sh(sudo.as_deref(), &["mkdir", "-p", &stage]);
                        let _ = sh(
                            sudo.as_deref(),
                            &["tar", "xzf", path_str(&web_tgz)?, "-C", &stage],
                        );
                        let _ = sh(sudo.as_deref(), &["chown", "-R", "neomind:neomind", &stage]);
                        let old = format!("{}.old.{}", WEB_DIR, std::process::id());
                        let _ = sh(sudo.as_deref(), &["rm", "-rf", &old]);
                        let _ = sh(sudo.as_deref(), &["mv", WEB_DIR, &old]);
                        let _ = sh(sudo.as_deref(), &["mv", &stage, WEB_DIR]);
                        let _ = sh(sudo.as_deref(), &["rm", "-rf", &old]);
                        println!("Frontend updated → {}", WEB_DIR);
                    }
                }
            }
        }
    }

    // 10. Restart the service (if it was running).
    if svc_active {
        println!("Starting neomind service...");
        let _ = sh(sudo.as_deref(), &["systemctl", "start", "neomind"]);
    }

    // 11. Clean up temp + verify.
    let _ = std::fs::remove_dir_all(&tmp);

    println!("\nVerifying...");
    if svc_active {
        let active = Command::new("systemctl")
            .args(["is-active", "--quiet", "neomind"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if active {
            println!("✅ neomind upgraded to v{} (service active)", target);
        } else {
            eprintln!("⚠️ service did not come back up — check: sudo systemctl status neomind");
        }
    } else {
        println!(
            "✅ neomind upgraded to v{} (no systemd service detected — restart manually)",
            target
        );
    }
    println!(
        "Rollback: sudo cp -a {} {} && sudo cp -a {} {} && sudo systemctl restart neomind",
        bak_bin.display(),
        cur_bin.display(),
        bak_runner.display(),
        cur_runner.display()
    );
    Ok(())
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
            DATA_DIR, WEB_DIR
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

    let sudo = sudo_prefix();

    // 1. Stop + disable the systemd service (best-effort).
    println!("Stopping + disabling neomind service...");
    let _ = sh(sudo.as_deref(), &["systemctl", "stop", "neomind"]);
    let _ = sh(sudo.as_deref(), &["systemctl", "disable", "neomind"]);

    // 2. Remove the service unit + reload.
    let _ = sh(
        sudo.as_deref(),
        &["rm", "-f", "/etc/systemd/system/neomind.service"],
    );
    let _ = sh(sudo.as_deref(), &["systemctl", "daemon-reload"]);

    // 3. Remove the binaries (current + .bak rollback copies) from the install dir.
    let cur_bin = std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .unwrap_or_else(|| PathBuf::from(format!("{}/neomind", DEFAULT_INSTALL_DIR)));
    let dir = cur_bin
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_INSTALL_DIR));
    println!("Removing binaries from {}", dir.display());
    let _ = sh(sudo.as_deref(), &["rm", "-f", path_str(&cur_bin)?]);
    let _ = sh(
        sudo.as_deref(),
        &[
            "rm",
            "-f",
            &dir.join("neomind-extension-runner").to_string_lossy(),
        ],
    );
    let _ = sh(
        sudo.as_deref(),
        &[
            "sh",
            "-c",
            &format!("rm -f {}/neomind*.bak*", dir.display()),
        ],
    );

    // 4. Optional purge of data + web.
    if purge {
        println!("Removing data dir {} (--purge)", DATA_DIR);
        let _ = sh(sudo.as_deref(), &["rm", "-rf", DATA_DIR]);
        let _ = sh(sudo.as_deref(), &["rm", "-rf", WEB_DIR]);
    }

    println!("✅ NeoMind uninstalled.");
    if !purge {
        println!(
            "(data dir {} retained — remove manually or re-run with --purge)",
            DATA_DIR
        );
    }
    Ok(())
}

// ---- helpers ----

fn sudo_prefix() -> Option<String> {
    // Need sudo unless already root.
    let is_root = Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<u32>()
                .ok()
        })
        .map(|u| u == 0)
        .unwrap_or(false);
    if is_root {
        None
    } else if which("sudo") {
        Some("sudo".to_string())
    } else {
        eprintln!(
            "⚠️ this operation needs root (run as root or with sudo); continuing best-effort"
        );
        None
    }
}

fn which(bin: &str) -> bool {
    Command::new("which")
        .arg(bin)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run a command, optionally prefixing with sudo; error on non-zero exit.
fn sh(sudo: Option<&str>, args: &[&str]) -> Result<()> {
    let mut cmd = match sudo {
        Some(s) => {
            let mut c = Command::new(s);
            c.args(args);
            c
        }
        None => {
            let (first, rest) = args.split_first().ok_or_else(|| anyhow!("empty command"))?;
            let mut c = Command::new(first);
            c.args(rest);
            c
        }
    };
    let status = cmd
        .status()
        .with_context(|| format!("failed to run: {:?}", args))?;
    if !status.success() {
        return Err(anyhow!("command failed ({:?}): {}", args, status));
    }
    Ok(())
}

fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd.status().context("command failed to start")?;
    if !status.success() {
        return Err(anyhow!("command exited {}", status));
    }
    Ok(())
}

fn path_str(p: &Path) -> Result<&str> {
    p.to_str().ok_or_else(|| anyhow!("non-UTF-8 path: {:?}", p))
}

fn tarball_str(p: &PathBuf) -> Result<&str> {
    path_str(p)
}

fn semver(v: &str) -> [u64; 3] {
    let v = v.trim_start_matches('v');
    let mut parts = v.split(|c: char| !c.is_ascii_digit());
    let major = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    [major, minor, patch]
}

fn is_newer(target: &str, current: &str) -> bool {
    let t = semver(target);
    let c = semver(current);
    t[0] > c[0] || (t[0] == c[0] && t[1] > c[1]) || (t[0] == c[0] && t[1] == c[1] && t[2] > c[2])
}
