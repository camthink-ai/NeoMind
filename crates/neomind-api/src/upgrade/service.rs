//! Staged self-upgrade orchestration (the API side of the two-phase design).
//!
//! Mirrors the builtin-LLM download task skeleton: `try_lock_owned`
//! single-flight, `tokio::spawn` background task, atomics + mutex snapshot
//! for the status endpoint, and throttled `SystemUpgradeProgress` events on
//! the event bus. The task never replaces binaries itself — it stages the
//! release under the data dir and hands off to the root helper unit.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};

use neomind_core::brand::APP_VERSION;
use neomind_core::event::NeoMindEvent;
use neomind_core::eventbus::EventBus;

use super::common;
use super::env::{self, DeploymentEnv};

/// How long a release check result stays fresh (GitHub API is rate-limited
/// to 60 req/h per unauthenticated IP).
const CHECK_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

/// Progress-event publish throttle (same value the builtin-LLM downloader
/// uses; per-chunk publishing storms the WS frontend into re-render flicker).
const EVENT_THROTTLE_MS: u64 = 250;

/// How long to wait after writing the apply trigger before considering the
/// hand-off complete. The root path unit normally picks the trigger up within
/// milliseconds and the apply ends by restarting this very process.
const APPLY_HANDOFF_WAIT: Duration = Duration::from_secs(5);

/// Leave the WS a moment to flush the `restarting` event before the helper
/// unit tears the process down.
const RESTART_GRACE: Duration = Duration::from_secs(1);

/// Phase names published in events + status. Stable string contract with the
/// frontend (`ServerUpgradeDialog` switches on these).
pub mod phase {
    pub const IDLE: &str = "idle";
    pub const CHECKING: &str = "checking";
    pub const DOWNLOADING: &str = "downloading";
    pub const VERIFYING: &str = "verifying";
    pub const STAGED: &str = "staged";
    pub const APPLYING: &str = "applying";
    pub const RESTARTING: &str = "restarting";
    pub const DONE: &str = "done";
    pub const ERROR: &str = "error";
}

/// Result of `UpgradeState::check` — the payload of
/// `GET /api/system/upgrade/check`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CheckInfo {
    /// Whether web-triggered upgrade can run here at all.
    pub supported: bool,
    /// "docker" | "systemd" | "unsupported"
    pub deployment: &'static str,
    /// Whether the root helper unit + sudoers drop-in are installed.
    pub helper_available: bool,
    pub current_version: String,
    /// Latest release tag, when the GitHub lookup succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_version: Option<String>,
    /// Release-notes markdown for `latest_version` (GitHub release body).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_notes: Option<String>,
    /// `latest_version` is strictly newer than `current_version`.
    pub available: bool,
    /// Operator-facing hint when upgrade cannot proceed (docker/unsupported/
    /// helper missing). Already localized in both languages, newline-separated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// Point-in-time snapshot of the upgrade task — payload of
/// `GET /api/system/upgrade/status`.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct UpgradeStatus {
    /// Whether an upgrade task is currently in flight.
    pub running: bool,
    /// Current phase (see the `phase` module; `idle` when never run).
    pub phase: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_version: Option<String>,
    pub downloaded: u64,
    pub total: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

struct UpgradeStatusShared {
    phase: Mutex<String>,
    target_version: Mutex<Option<String>>,
    downloaded: AtomicU64,
    total: AtomicU64,
    error: Mutex<Option<String>>,
    /// Flips the phase back to `idle`-ish terminal bookkeeping on task end.
    running: AtomicBool,
}

impl UpgradeStatusShared {
    fn new() -> Self {
        Self {
            phase: Mutex::new(phase::IDLE.to_string()),
            target_version: Mutex::new(None),
            downloaded: AtomicU64::new(0),
            total: AtomicU64::new(0),
            error: Mutex::new(None),
            running: AtomicBool::new(false),
        }
    }

    fn set_phase(&self, p: &str) {
        *self.phase.lock().unwrap() = p.to_string();
    }

    fn set_error(&self, msg: String) {
        self.set_phase(phase::ERROR);
        *self.error.lock().unwrap() = Some(msg);
    }

    fn snapshot(&self) -> UpgradeStatus {
        UpgradeStatus {
            running: self.running.load(Ordering::SeqCst),
            phase: self.phase.lock().unwrap().clone(),
            target_version: self.target_version.lock().unwrap().clone(),
            downloaded: self.downloaded.load(Ordering::SeqCst),
            total: self.total.load(Ordering::SeqCst),
            error: self.error.lock().unwrap().clone(),
        }
    }
}

struct CheckCacheEntry {
    fetched_at: std::time::Instant,
    info: CheckInfo,
}

/// Per-process upgrade coordinator, held on `ServerState`.
#[derive(Clone)]
pub struct UpgradeState {
    lock: Arc<tokio::sync::Mutex<()>>,
    shared: Arc<UpgradeStatusShared>,
    check_cache: Arc<tokio::sync::Mutex<Option<CheckCacheEntry>>>,
}

impl UpgradeState {
    pub fn new() -> Self {
        Self {
            lock: Arc::new(tokio::sync::Mutex::new(())),
            shared: Arc::new(UpgradeStatusShared::new()),
            check_cache: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    /// Current task status (never blocks longer than a mutex guard).
    pub fn status(&self) -> UpgradeStatus {
        self.shared.snapshot()
    }

    /// Release check, cached for [`CHECK_CACHE_TTL`]; `force` bypasses.
    pub async fn check(&self, force: bool) -> CheckInfo {
        if !force {
            if let Some(entry) = self.check_cache.lock().await.as_ref() {
                if entry.fetched_at.elapsed() < CHECK_CACHE_TTL {
                    return entry.info.clone();
                }
            }
        }

        let deployment = env::detect();
        let mut notes = deployment.upgrade_hint().map(str::to_string);
        if deployment == DeploymentEnv::Systemd && !env::helper_available() {
            notes = Some(
                "此部署缺少在线升级辅助组件，请在服务器上重新运行一次安装脚本以启用\n\
                 This install predates the upgrade helper; re-run scripts/install.sh once to enable web upgrade"
                    .to_string(),
            );
        }

        let client = http_client();
        let (latest, release_notes) = common::github_latest_release(&client)
            .await
            .map(|(v, b)| (Some(v), b))
            .unwrap_or((None, None));
        let available = latest
            .as_deref()
            .map(|l| common::is_newer(l, APP_VERSION))
            .unwrap_or(false);

        let info = CheckInfo {
            // Docker/unsupported environments can never web-upgrade; on
            // systemd the helper is required to actually apply.
            supported: deployment == DeploymentEnv::Systemd && env::helper_available(),
            deployment: deployment.as_str(),
            helper_available: env::helper_available(),
            current_version: APP_VERSION.to_string(),
            latest_version: latest,
            release_notes,
            available,
            notes,
        };
        *self.check_cache.lock().await = Some(CheckCacheEntry {
            fetched_at: std::time::Instant::now(),
            info: info.clone(),
        });
        info
    }

    /// Kick off a staged upgrade. Returns `(started, reason)`:
    /// - `(true, _)` — background task launched,
    /// - `(false, "already_running")` — single-flight refused a second task,
    /// - `(false, "up_to_date")` — no explicit version and nothing newer,
    /// - `(false, "unsupported…")` — environment/helper missing.
    pub async fn start(
        &self,
        data_dir: PathBuf,
        event_bus: Option<Arc<EventBus>>,
        version: Option<String>,
    ) -> (bool, String) {
        // OwnedMutexGuard: owns a clone of the Arc, so the guard can move
        // into the 'static background task (builtin-LLM downloader pattern).
        let Ok(guard) = self.lock.clone().try_lock_owned() else {
            return (false, "already_running".to_string());
        };

        // Environment gate before spawning (fail fast with an actionable reason).
        let deployment = env::detect();
        if deployment != DeploymentEnv::Systemd {
            return (
                false,
                format!(
                    "upgrade not supported in deployment '{}'",
                    deployment.as_str()
                ),
            );
        }
        if !env::helper_available() {
            return (
                false,
                "upgrade helper not installed — re-run scripts/install.sh".to_string(),
            );
        }

        let shared = self.shared.clone();
        shared.running.store(true, Ordering::SeqCst);
        *shared.error.lock().unwrap() = None;
        shared.downloaded.store(0, Ordering::SeqCst);
        shared.total.store(0, Ordering::SeqCst);
        shared.set_phase(phase::CHECKING);

        // `run_staged_upgrade` takes ownership of the bus; keep a clone so
        // the failure branch below can still push the ERROR event to the WS.
        let bus_for_err = event_bus.clone();
        tokio::spawn(async move {
            let _guard = guard;
            match run_staged_upgrade(&data_dir, event_bus, version, &shared).await {
                Ok(()) => {}
                Err(e) => {
                    tracing::warn!(error = %e, "web-triggered upgrade failed");
                    shared.set_error(e.to_string());
                    let target = shared.target_version.lock().unwrap().clone();
                    publish_progress(
                        &bus_for_err,
                        phase::ERROR,
                        target.as_deref(),
                        None,
                        None,
                        None,
                        Some(&e.to_string()),
                    );
                }
            }
            // `restarting` keeps running=true on purpose: the frontend should
            // treat the restart window as in-flight until the server answers
            // again with the new version. Only terminal non-restart outcomes
            // (done normally only reachable via apply verification, or error)
            // clear the flag — error already set its phase above.
            if shared.phase.lock().unwrap().as_str() != phase::RESTARTING {
                shared.running.store(false, Ordering::SeqCst);
            }
        });

        (true, String::new())
    }
}

impl Default for UpgradeState {
    fn default() -> Self {
        Self::new()
    }
}

/// The background task body: resolve → download → verify → stage → trigger.
async fn run_staged_upgrade(
    data_dir: &std::path::Path,
    event_bus: Option<Arc<EventBus>>,
    version: Option<String>,
    shared: &UpgradeStatusShared,
) -> Result<()> {
    let client = http_client();

    // 1. Resolve target version.
    let target = match version.as_deref() {
        Some(v) => v.trim_start_matches('v').to_string(),
        None => common::github_latest_version(&client).await?,
    };
    *shared.target_version.lock().unwrap() = Some(target.clone());
    publish_progress(
        &event_bus,
        phase::CHECKING,
        Some(&target),
        None,
        None,
        None,
        None,
    );

    let staging = common::staging_dir(data_dir, &target);
    if staging.exists() {
        tokio::fs::remove_dir_all(&staging)
            .await
            .with_context(|| format!("cannot clear stale staging {}", staging.display()))?;
    }
    tokio::fs::create_dir_all(&staging).await?;

    // 2. Download the server tarball (streamed, capped, progress events).
    shared.set_phase(phase::DOWNLOADING);
    let arch = common::host_arch();
    let server_url = common::server_tarball_url(&target, arch);
    let tarball = staging.join("neomind.tar.gz");
    {
        let bus = event_bus.clone();
        let target_for_cb = target.clone();
        let mut last_publish_ms: u64 = 0;
        let mut on_progress = move |downloaded: u64, total: Option<u64>| {
            shared.downloaded.store(downloaded, Ordering::SeqCst);
            if let Some(t) = total {
                shared.total.store(t, Ordering::SeqCst);
            }
            let now = now_epoch_ms();
            let throttle =
                now.saturating_sub(last_publish_ms) < EVENT_THROTTLE_MS && last_publish_ms != 0;
            if throttle {
                return;
            }
            last_publish_ms = now;
            publish_progress(
                &bus,
                phase::DOWNLOADING,
                Some(&target_for_cb),
                Some(downloaded),
                total,
                None,
                None,
            );
        };
        common::download_to_file(&client, &server_url, &tarball, Some(&mut on_progress)).await?;
    }

    // 3. Extract + verify (blocking work off the async threads).
    shared.set_phase(phase::VERIFYING);
    publish_progress(
        &event_bus,
        phase::VERIFYING,
        Some(&target),
        None,
        None,
        None,
        None,
    );
    let staging_for_block = staging.clone();
    let extracted = tokio::task::spawn_blocking(move || -> Result<PathBuf> {
        common::extract_tar_gz(&tarball, &staging_for_block)?;
        let bin = staging_for_block.join("neomind");
        if !bin.exists() {
            return Err(anyhow!(
                "extracted tarball has no `neomind` binary — aborting (service untouched)"
            ));
        }
        // tar may drop exec bits depending on how the release was packed.
        make_executable(&bin)?;
        Ok(bin)
    })
    .await
    .context("extract task panicked")??;
    let new_bin = extracted;
    let new_runner = {
        let runner = staging.join("neomind-extension-runner");
        if runner.exists() {
            make_executable(&runner)?;
            Some(runner)
        } else {
            None
        }
    };
    let verify_bin = new_bin.clone();
    let verify_target = target.clone();
    let vstr = tokio::task::spawn_blocking(move || {
        common::verify_binary_version(&verify_bin, &verify_target)
    })
    .await
    .context("verify task panicked")??;
    tracing::info!(version = %vstr, "web upgrade: staged binary verified");

    // 4. Stage the web tarball too when the server actually serves one.
    let web_tarball = if common::web_dir().is_dir() {
        let web_url = common::web_tarball_url(&target);
        let dest = staging.join("neomind-web.tar.gz");
        let bus = event_bus.clone();
        let target_for_cb = target.clone();
        let mut last_publish_ms: u64 = 0;
        let mut on_progress = move |downloaded: u64, total: Option<u64>| {
            shared.downloaded.store(downloaded, Ordering::SeqCst);
            if let Some(t) = total {
                shared.total.store(t, Ordering::SeqCst);
            }
            let now = now_epoch_ms();
            let throttle =
                now.saturating_sub(last_publish_ms) < EVENT_THROTTLE_MS && last_publish_ms != 0;
            if throttle {
                return;
            }
            last_publish_ms = now;
            publish_progress(
                &bus,
                phase::DOWNLOADING,
                Some(&target_for_cb),
                Some(downloaded),
                total,
                Some("frontend"),
                None,
            );
        };
        match common::download_to_file(&client, &web_url, &dest, Some(&mut on_progress)).await {
            Ok(_) => Some(dest),
            // Web tarball is an optional asset — a missing one must not block
            // the (more important) backend binary upgrade.
            Err(e) => {
                tracing::warn!(error = %e, "web upgrade: frontend tarball not staged (continuing)");
                None
            }
        }
    } else {
        None
    };

    // 5. Write the manifest the root helper reads.
    let manifest = common::StagingManifest {
        version: target.clone(),
        arch: arch.to_string(),
        staged_at: chrono_now_millis(),
        server_binary: "neomind".to_string(),
        extension_runner: new_runner
            .as_ref()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string()),
        web_tarball: web_tarball
            .as_ref()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string()),
    };
    let manifest_path = staging.join("manifest.json");
    tokio::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)
        .await
        .with_context(|| format!("cannot write {}", manifest_path.display()))?;

    shared.set_phase(phase::STAGED);
    publish_progress(
        &event_bus,
        phase::STAGED,
        Some(&target),
        None,
        None,
        None,
        None,
    );

    // 6. Hand off to the root helper by writing the trigger file the
    //    `neomind-upgrade-apply.path` unit watches. No sudo involved — the
    //    API's own unit sets NoNewPrivileges=true, so sudo could never gain
    //    root from here; the path unit is inotify-based and needs nothing
    //    beyond write access to the staging root. Emit `restarting` FIRST:
    //    once the helper swaps binaries it restarts this process, and events
    //    published after that would never reach the WS.
    shared.set_phase(phase::APPLYING);
    publish_progress(
        &event_bus,
        phase::APPLYING,
        Some(&target),
        None,
        None,
        None,
        None,
    );
    tokio::time::sleep(RESTART_GRACE).await;
    shared.set_phase(phase::RESTARTING);
    publish_progress(
        &event_bus,
        phase::RESTARTING,
        Some(&target),
        None,
        None,
        Some("applying staged upgrade — service is restarting"),
        None,
    );

    let staging_root = common::staging_root(data_dir);
    tokio::fs::create_dir_all(&staging_root).await?;
    let trigger = staging_root.join(common::APPLY_TRIGGER_FILE);
    tokio::fs::write(&trigger, format!("{} {}", target, chrono_now_millis()))
        .await
        .with_context(|| format!("cannot write apply trigger {}", trigger.display()))?;
    tracing::info!(
        trigger = %trigger.display(),
        "web upgrade: apply trigger written — waiting for the root helper"
    );

    // Give the helper a moment to pick the trigger up. The normal outcome is
    // that this process is restarted by the apply step and never gets here;
    // surviving past the wait means the path unit is not running (disabled,
    // broken) — surface that instead of leaving the UI in "restarting".
    tokio::time::sleep(APPLY_HANDOFF_WAIT).await;
    if trigger.exists() {
        // Still there: the apply step removes it first thing, so the path
        // unit never fired. Best-effort cleanup so a later retry can re-arm.
        let _ = tokio::fs::remove_file(&trigger).await;
        return Err(anyhow!(
            "apply trigger was not consumed — is {} enabled? (systemctl status {})",
            common::APPLY_PATH_UNIT_NAME,
            common::APPLY_PATH_UNIT_NAME
        ));
    }
    Ok(())
}

/// Best-effort `chmod +x`.
fn make_executable(path: &std::path::Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms)?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn chrono_now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(neomind_core::brand::user_agent())
        .build()
        .expect("reqwest client with 30s timeout")
}

/// Publish a progress event (best-effort — no bus means no listener).
#[allow(clippy::too_many_arguments)]
fn publish_progress(
    bus: &Option<Arc<EventBus>>,
    phase: &str,
    target_version: Option<&str>,
    downloaded: Option<u64>,
    total: Option<u64>,
    message: Option<&str>,
    error: Option<&str>,
) {
    let Some(bus) = bus else { return };
    bus.publish_sync(NeoMindEvent::SystemUpgradeProgress {
        phase: phase.to_string(),
        current_version: APP_VERSION.to_string(),
        target_version: target_version.map(str::to_string),
        downloaded,
        total,
        message: message.map(str::to_string),
        error: error.map(str::to_string),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_starts_idle() {
        let s = UpgradeState::new();
        let st = s.status();
        assert!(!st.running);
        assert_eq!(st.phase, phase::IDLE);
        assert!(st.error.is_none());
    }

    #[tokio::test]
    async fn start_refuses_when_helper_missing() {
        // On any dev/test host the helper unit/sudoers do not exist, so the
        // environment gate must refuse to spawn (macOS: unsupported; Linux
        // CI: no /etc/sudoers.d entry). Either way: not started.
        let s = UpgradeState::new();
        let (started, reason) = s.start(PathBuf::from("/tmp"), None, None).await;
        assert!(!started);
        assert!(!reason.is_empty());
        assert_eq!(s.status().phase, phase::IDLE);
    }

    #[tokio::test]
    async fn check_reports_current_version() {
        let s = UpgradeState::new();
        // force=true would hit the network; use the cache path instead.
        // (First call fetches — in CI without network latest_version is
        // None and that's still a valid CheckInfo.)
        let info = s.check(false).await;
        assert_eq!(info.current_version, APP_VERSION);
        assert!(matches!(
            info.deployment,
            "docker" | "systemd" | "unsupported"
        ));
    }
}
