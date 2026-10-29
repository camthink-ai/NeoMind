//! Standalone llama.cpp runtime management: enumeration, update channel,
//! upgrade/revert orchestration, and the variant resolution layer.
//!
//! The runtime resolution order is "newest valid wins" (see `pick_runtime`)
//! among the bundled binary and the versioned cache dirs under
//! `data/llama-server/`, with the compiled-in `LLAMA_CPP_VERSION` as the
//! cache floor. The update channel (`runtimes.json` in the NeoMind-Runtimes
//! repo, overridable via env/mirror settings) recommends newer tags; the
//! upgrade flow installs them into their own versioned cache dir — no root,
//! no install-dir writes, works identically under systemd / docker / bare
//! serve / the desktop app.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use neomind_core::brand::APP_VERSION;
use neomind_core::builtin_llm::find::find_llama_server;
use neomind_core::builtin_llm::runtime::{
    llama_asset_is_zip, llama_asset_name, llama_server_cache_dir_for_tag, parse_runtime_cache_name,
    tag_cmp, try_llama_server_url_for_tag, validate_tag, RuntimeVariant, LLAMA_CPP_VERSION,
};
use neomind_core::event::NeoMindEvent;
use neomind_core::eventbus::EventBus;

/// Where the recommended-runtime channel lives. Mirrors must serve the same
/// JSON shape; override via env (operators) — a settings-layer override
/// joins here when the mirror-config workstream lands.
pub const RUNTIME_CHANNEL_URL: &str =
    "https://raw.githubusercontent.com/camthink-ai/NeoMind-Runtimes/main/runtimes.json";
const CHANNEL_TTL: Duration = Duration::from_secs(300);
const CHANNEL_TIMEOUT: Duration = Duration::from_secs(15);

const VARIANT_ENV: &str = "NEOMIND_BUILTIN_RUNTIME_VARIANT";
pub const VARIANT_SETTING_KEY: &str = "builtin_runtime_variant";

/// Self-hosted variants (our NeoMind-Runtimes builds) — SHA-pinned channel
/// entries. Everything else uses official llama.cpp assets via `recommended`.
fn channel_variant_key(variant: RuntimeVariant) -> Option<&'static str> {
    match variant {
        RuntimeVariant::Jetson => Some("jetson"),
        RuntimeVariant::Cuda
            if matches!(
                (std::env::consts::OS, std::env::consts::ARCH),
                ("linux", "x86_64") | ("linux", "x86")
            ) =>
        {
            Some("linux-cuda")
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Variant resolution: env > persisted setting > auto-detect
// ---------------------------------------------------------------------------

/// Process-local "degraded to CPU" flag set when a CUDA/Vulkan runtime
/// proved unusable. Deliberately NOT the old sticky env write — that would
/// permanently override the user's persisted variant choice.
static VARIANT_DEGRADED_CPU: AtomicBool = AtomicBool::new(false);

pub fn degrade_to_cpu_for_process() {
    VARIANT_DEGRADED_CPU.store(true, Ordering::SeqCst);
}

/// The variant this process resolves runtimes for.
///
/// Priority: explicit env (`NEOMIND_BUILTIN_RUNTIME_VARIANT`; `cpu` is an
/// explicit opt-out, unrecognized values fall through like today) →
/// persisted setting (`builtin_runtime_variant`, set from the UI) →
/// process-local degradation → auto-detection.
pub fn effective_variant() -> RuntimeVariant {
    if let Ok(raw) = std::env::var(VARIANT_ENV) {
        let trimmed = raw.trim();
        if trimmed.eq_ignore_ascii_case("cpu") {
            return RuntimeVariant::Cpu;
        }
        let parsed = RuntimeVariant::parse(trimmed);
        if parsed != RuntimeVariant::Cpu {
            return parsed;
        }
    }
    if let Some(v) = persisted_variant_override() {
        return v;
    }
    if VARIANT_DEGRADED_CPU.load(Ordering::SeqCst) {
        return RuntimeVariant::Cpu;
    }
    RuntimeVariant::detect()
}

/// Read the persisted variant override. Empty/absent = auto; unknown values
/// are ignored (never silently disable detection on a typo).
fn persisted_variant_override() -> Option<RuntimeVariant> {
    let raw = load_variant_setting().ok()?.unwrap_or_default();
    parse_strict(&raw)
}

/// Strict parse for user-entered values — unlike `RuntimeVariant::parse`
/// (which maps garbage to CPU), unknown input is rejected.
fn parse_strict(raw: &str) -> Option<RuntimeVariant> {
    let t = raw.trim();
    if t.eq_ignore_ascii_case("cpu") {
        Some(RuntimeVariant::Cpu)
    } else if t.eq_ignore_ascii_case("cuda") {
        Some(RuntimeVariant::Cuda)
    } else if t.eq_ignore_ascii_case("vulkan") {
        Some(RuntimeVariant::Vulkan)
    } else if t.eq_ignore_ascii_case("jetson") {
        Some(RuntimeVariant::Jetson)
    } else {
        None
    }
}

fn load_variant_setting() -> std::io::Result<Option<String>> {
    let store = crate::config::open_settings_store()
        .map_err(|e| std::io::Error::other(format!("open settings store: {e}")))?;
    store
        .load(VARIANT_SETTING_KEY)
        .map_err(|e| std::io::Error::other(format!("read setting: {e}")))
}

/// Persist (or clear, when `value` is empty) the variant override.
pub fn set_persisted_variant(value: &str) -> Result<(), String> {
    if !value.trim().is_empty() && parse_strict(value).is_none() {
        return Err(format!(
            "unknown variant '{value}' — expected cpu | cuda | vulkan | jetson"
        ));
    }
    let store = crate::config::open_settings_store().map_err(|e| format!("open settings: {e}"))?;
    store
        .save(VARIANT_SETTING_KEY, value.trim())
        .map_err(|e| format!("save setting: {e}"))
}

// ---------------------------------------------------------------------------
// Runtime enumeration
// ---------------------------------------------------------------------------

/// One installable/installed runtime, as reported by the status API.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RuntimeInfo {
    pub variant: String,
    pub tag: String,
    /// "bundled" | "cache"
    pub source: &'static str,
    pub path: String,
}

/// Parsed cache dirs under `data/llama-server/` — all variants, no floor.
/// Malformed names are skipped (never trust a dir we can't parse).
pub fn cache_entries(data_dir: &Path) -> Vec<(String, RuntimeVariant, PathBuf)> {
    let root = data_dir.join("llama-server");
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some((tag, variant)) = parse_runtime_cache_name(&name) {
                if e.path().is_dir() {
                    out.push((tag, variant, e.path()));
                }
            }
        }
    }
    out.sort_by(|a, b| tag_cmp(&b.0, &a.0));
    out
}

/// Cache candidates for ONE variant, filtered to `tag >= pin` (the pin is
/// the floor — an older cache must never block the pin's own download path
/// after a platform upgrade). Below-floor dirs are pruned on sight.
pub fn entries_for_variant(data_dir: &Path, variant: RuntimeVariant) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for (tag, v, path) in cache_entries(data_dir) {
        if v != variant {
            continue;
        }
        if tag_cmp(&tag, LLAMA_CPP_VERSION) == std::cmp::Ordering::Less {
            tracing::debug!(
                dir = %path.display(),
                "builtin llm: cache below the compiled pin — pruning"
            );
            let _ = std::fs::remove_dir_all(&path);
            continue;
        }
        out.push((tag, path));
    }
    out.sort_by(|a, b| tag_cmp(&b.0, &a.0));
    out
}

/// Keep the active tag plus the newest other same-variant dir; prune the
/// rest so `data/llama-server/` doesn't accumulate one 20-80 MB tree per
/// historical upgrade. Other variants' dirs are left alone (a variant
/// switch back should not force a re-download).
pub fn prune_cache(data_dir: &Path, variant: RuntimeVariant, keep_tag: &str) {
    let entries = entries_for_variant(data_dir, variant);
    // entries are sorted newest-first. Keep the newest (normally == keep_tag)
    // and the second-newest as the revert target; drop everything older.
    // `keep_tag` is therefore informational only — the index already encodes
    // the retention rule.
    for (i, (tag, path)) in entries.iter().enumerate() {
        if i <= 1 {
            continue;
        }
        tracing::info!(dir = %path.display(), tag = %tag, keep = %keep_tag,
            "builtin llm: pruning old runtime cache");
        let _ = std::fs::remove_dir_all(path);
    }
}

// ---------------------------------------------------------------------------
// Bundled binary identity (exec-parsed once per process)
// ---------------------------------------------------------------------------

/// Memoized `--version` tag of the bundled binary. `None` inside means
/// "not probed yet"; `Some(None)` = probed, unparseable.
static BUNDLED_TAG: OnceLock<Mutex<Option<Option<String>>>> = OnceLock::new();
/// Cache-picked binaries that already passed an exec check this process.
static VERIFIED: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

fn bundled_tag_slot() -> &'static Mutex<Option<Option<String>>> {
    BUNDLED_TAG.get_or_init(|| Mutex::new(None))
}

fn verified_slot() -> &'static Mutex<HashSet<PathBuf>> {
    VERIFIED.get_or_init(|| Mutex::new(HashSet::new()))
}

pub fn mark_verified(path: &Path) {
    verified_slot()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(path.to_path_buf());
}

pub fn is_verified(path: &Path) -> bool {
    verified_slot()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(path)
}

/// Forget memoized bundled-tag/exec-verification state — after an upgrade,
/// revert, or cache ejection the next resolution must re-derive.
pub fn invalidate_resolution_caches() {
    if let Some(m) = BUNDLED_TAG.get() {
        *m.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
    verified_slot()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
}

/// Exec the bundled binary's `--version` once and extract its `bNNNN` tag.
pub async fn bundled_tag(path: &Path) -> Option<String> {
    {
        let guard = bundled_tag_slot().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cached) = guard.as_ref() {
            return cached.clone();
        }
    }
    let probed = exec_version_tag(path).await;
    *bundled_tag_slot().lock().unwrap_or_else(|e| e.into_inner()) = Some(probed.clone());
    probed
}

/// Run `<bin> --version` and pull the first `b\d{4,7}` token out of its
/// output. Official and our builds both print e.g. `version: 5668 (b11609)`.
async fn exec_version_tag(bin: &Path) -> Option<String> {
    // kill_on_drop is load-bearing: a binary that does NOT answer --version
    // by exiting (a wrapper that starts serving anyway, a hung loader, our
    // integration tests' fake server) would otherwise survive the timeout as
    // an orphan holding the model port — the next real spawn then fails to
    // bind. 3s is generous for a real `llama-server --version` (exits in ms).
    let out = tokio::time::timeout(
        Duration::from_secs(3),
        tokio::process::Command::new(bin)
            .arg("--version")
            .kill_on_drop(true)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    first_tag_in(&text)
}

/// First well-formed `bNNNN` substring — manual scan (no regex dep).
pub(crate) fn first_tag_in(text: &str) -> Option<String> {
    if let Some(t) = first_b_tag_in(text) {
        return Some(t);
    }
    // llama.cpp 0.8.x changed the banner to `version: 0.8.0 (build 11960,
    // commit ...)` — no `b` prefix on the build number. Synthesize the
    // canonical b-tag from a `(build NNNN)` group so bundled-binary version
    // parsing keeps working across the format change.
    build_number_in(text).map(|n| format!("b{n}"))
}

fn first_b_tag_in(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'b' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            let candidate = &text[i..j];
            if validate_tag(candidate) {
                return Some(candidate.to_string());
            }
            i = j;
        } else {
            i += 1;
        }
    }
    None
}

/// The `build NNNN` (4-7 digits) group of a 0.8.x banner.
fn build_number_in(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let mut rest = lower.as_str();
    while let Some(idx) = rest.find("build ") {
        let after = &rest[idx + 6..];
        let end = after
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after.len());
        let digits = &after[..end];
        if (4..=7).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit()) {
            return Some(digits.to_string());
        }
        rest = &rest[idx + 6..];
    }
    None
}

// ---------------------------------------------------------------------------
// Resolution matrix (pure — unit-testable without touching the network)
// ---------------------------------------------------------------------------

/// What `ensure_llama_server` should do. Mirrors the plan's matrix:
/// 1. best cache wins when its tag >= bundled's (ties: cache wins; an
///    unknown bundled tag yields to any cache — the stale-install.sh case);
/// 2. bundled wins when its (known) tag >= pin — e.g. right after a
///    platform upgrade shipped a newer bundled runtime;
/// 3. bundled older than the pin → try the pin download first, fall back to
///    bundled when offline (zero regression vs. today);
/// 4. no bundled → pin download (the bare `neomind serve` path).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Decision {
    Cache { tag: String, path: PathBuf },
    Bundled(PathBuf),
    TryPinThenBundled(PathBuf),
    TryPin,
}

pub(crate) fn pick_runtime(
    best_cache: Option<(String, PathBuf)>,
    bundled: Option<(PathBuf, Option<String>)>,
) -> Decision {
    if let Some((tag, path)) = best_cache {
        let cache_wins = bundled
            .as_ref()
            .and_then(|(_, t)| t.as_deref())
            .is_none_or(|bt| tag_cmp(&tag, bt) != std::cmp::Ordering::Less);
        if cache_wins {
            return Decision::Cache { tag, path };
        }
        // Cache older than the (known-tag) bundled → bundled wins. Bundled
        // >= pin is implied: caches are pin-floored, so bundled > cache >= pin.
    }
    match bundled {
        Some((path, Some(t))) if tag_cmp(&t, LLAMA_CPP_VERSION) == std::cmp::Ordering::Less => {
            Decision::TryPinThenBundled(path)
        }
        Some((path, _)) => Decision::Bundled(path),
        None => Decision::TryPin,
    }
}

// ---------------------------------------------------------------------------
// Failure observability (status `sources`/error state)
// ---------------------------------------------------------------------------

static BOOTSTRAP_ERROR: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static LAST_DOWNLOAD_ERROR: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn err_slot(cell: &'static OnceLock<Mutex<Option<String>>>) -> &'static Mutex<Option<String>> {
    cell.get_or_init(|| Mutex::new(None))
}

/// Last bootstrap failure text (None after a successful bootstrap/spawn) —
/// surfaced as `server_state: "error"` + `error` on the status API so "已停止"
/// finally explains itself (e.g. runtime download failed offline).
pub fn record_bootstrap_error(err: Option<String>) {
    *err_slot(&BOOTSTRAP_ERROR)
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = err;
}

pub fn bootstrap_error() -> Option<String> {
    err_slot(&BOOTSTRAP_ERROR)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Last model-download failure text (cleared on a successful download) —
/// for the status `sources` block.
pub fn record_download_error(err: Option<String>) {
    *err_slot(&LAST_DOWNLOAD_ERROR)
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = err;
}

pub fn last_download_error() -> Option<String> {
    err_slot(&LAST_DOWNLOAD_ERROR)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// The runtime tag resolution would hand to a spawn right now — the public
/// form used by the model-download gate (`min_runtime`). `None` = active
/// runtime has no parseable version (never blocks: gate on known data only).
pub async fn active_runtime_tag(data_dir: &Path) -> Option<String> {
    let variant = effective_variant();
    current_active_tag(data_dir, variant).await
}

/// Pure gate: does the active runtime satisfy a model's `min_runtime`?
/// `None` active = unknown version → allowed (never block on unknown data).
pub(crate) fn runtime_gate(active: Option<&str>, min_runtime: &str) -> Result<(), String> {
    match active {
        Some(active) if tag_cmp(active, min_runtime) == std::cmp::Ordering::Less => Err(format!(
            "this model requires llama.cpp >= {min_runtime} but the active runtime is {active} — \
             upgrade it in Settings → LLM Backends → Built-in → llama.cpp runtime first"
        )),
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Update channel (runtimes.json)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ChannelVariantEntry {
    pub tag: String,
    /// Explicit asset filename (defaults to the tag-derived name).
    #[serde(default)]
    pub asset: Option<String>,
    /// SHA-256 — MANDATORY for self-hosted entries (executable download).
    #[serde(default)]
    pub sha256: Option<String>,
    /// Full URL override (mirror / test fixture).
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ChannelNotes {
    #[serde(default)]
    pub zh: Option<String>,
    #[serde(default)]
    pub en: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct RuntimeChannelDoc {
    #[serde(default)]
    pub schema: u32,
    /// Recommended tag for official-asset variants (cpu/vulkan/mac/win).
    #[serde(default)]
    pub recommended: Option<String>,
    /// Per-platform recommended overrides — the key is `"<os>/<arch>"`
    /// (e.g. "linux/aarch64"). Wins over `recommended` for that platform;
    /// needed because upstream floors move per-arch (the 0.8.x official
    /// ubuntu-arm64 builds jumped to glibc 2.38 while x64 stayed at 2.34 —
    /// 22.04 arm64 hosts must hold back while x64 moves forward).
    #[serde(default)]
    pub recommended_by_platform: HashMap<String, String>,
    #[serde(default)]
    pub variants: HashMap<String, ChannelVariantEntry>,
    #[serde(default)]
    pub min_platform: Option<String>,
    /// Optional base override for OFFICIAL llama.cpp asset URLs (CN mirror
    /// or a test fixture). Replaces `https://github.com/ggml-org/llama.cpp`
    /// as the prefix of `/releases/download/...` paths.
    #[serde(default)]
    pub official_base: Option<String>,
    #[serde(default)]
    pub notes: Option<ChannelNotes>,
}

/// `(fetched_at, doc)`; `None` inside = negative-cached (last fetch failed).
type ChannelCacheState = Option<(Instant, Option<Arc<RuntimeChannelDoc>>)>;

static CHANNEL_CACHE: OnceLock<Mutex<ChannelCacheState>> = OnceLock::new();

fn channel_slot() -> &'static Mutex<ChannelCacheState> {
    CHANNEL_CACHE.get_or_init(|| Mutex::new(None))
}

/// Effective channel URL: setting > env > default (see `sources.rs`).
pub fn channel_url() -> String {
    super::sources::runtime_channel_url()
}

/// Last-known channel doc WITHOUT any fetch — for cheap status polls. The
/// actual fetch happens on explicit check/upgrade (or the first `fetch_channel`).
pub fn peek_channel() -> Option<Arc<RuntimeChannelDoc>> {
    channel_slot()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|(_, doc)| doc.clone())
}

/// Fetch the channel doc (5-min TTL, negative-cached on failure so an
/// offline host doesn't hammer the network). `None` = unreachable/invalid —
/// callers degrade to "cannot check for updates", never an error.
pub async fn fetch_channel(force: bool) -> Option<Arc<RuntimeChannelDoc>> {
    {
        let slot = channel_slot().lock().unwrap_or_else(|e| e.into_inner());
        if !force {
            if let Some((at, doc)) = slot.as_ref() {
                if at.elapsed() < CHANNEL_TTL {
                    return doc.clone();
                }
            }
        }
    }
    let fetched = fetch_channel_now().await;
    *channel_slot().lock().unwrap_or_else(|e| e.into_inner()) =
        Some((Instant::now(), fetched.clone()));
    fetched
}

async fn fetch_channel_now() -> Option<Arc<RuntimeChannelDoc>> {
    let url = channel_url();
    let client = reqwest::Client::builder()
        .timeout(CHANNEL_TIMEOUT)
        .user_agent(neomind_core::brand::user_agent())
        .build()
        .ok()?;
    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => match resp.text().await {
            Ok(body) => match serde_json::from_str::<RuntimeChannelDoc>(&body) {
                Ok(doc) => {
                    tracing::debug!(url = %url, "builtin llm: runtime channel refreshed");
                    Some(Arc::new(doc))
                }
                Err(e) => {
                    tracing::warn!(url = %url, error = %e, "builtin llm: runtime channel JSON invalid");
                    None
                }
            },
            Err(e) => {
                tracing::warn!(url = %url, error = %e, "builtin llm: runtime channel body unreadable");
                None
            }
        },
        Ok(resp) => {
            tracing::warn!(url = %url, status = %resp.status(), "builtin llm: runtime channel unreachable");
            None
        }
        Err(e) => {
            tracing::warn!(url = %url, error = %e, "builtin llm: runtime channel unreachable");
            None
        }
    }
}

/// https-only, except explicit loopback (local test fixtures / mirrors on
/// the same host). Anything else is refused before it reaches a download.
fn url_allowed(u: &str) -> bool {
    if u.starts_with("https://") {
        return true;
    }
    if let Some(rest) = u.strip_prefix("http://") {
        // Strip any :port, then IPv6 brackets ("[::1]:8080" → "::1").
        let host = rest.split('/').next().unwrap_or_default();
        let host = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
        let host = host.trim_matches(|c| c == '[' || c == ']');
        return host == "127.0.0.1" || host == "localhost" || host == "::1";
    }
    false
}

/// Where an upgrade for `variant` would come from, per the channel doc.
#[derive(Debug, Clone)]
pub struct UpgradeTarget {
    pub tag: String,
    pub url: String,
    pub sha256: Option<String>,
}

pub fn resolve_upgrade_target(
    doc: &RuntimeChannelDoc,
    variant: RuntimeVariant,
) -> Result<UpgradeTarget, String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    if let Some(key) = channel_variant_key(variant) {
        let entry = doc
            .variants
            .get(key)
            .ok_or_else(|| format!("channel has no '{key}' entry yet"))?;
        if !validate_tag(&entry.tag) {
            return Err(format!(
                "channel entry '{key}' has an invalid tag '{}' — refusing",
                entry.tag
            ));
        }
        // Self-hosted executable downloads are hard SHA-pinned, whether the
        // SHA comes from the compiled constant (pin path) or the channel.
        let sha = entry
            .sha256
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                format!("channel entry '{key}' has no sha256 — refusing an unpinned executable download")
            })?
            .to_string();
        let url = if let Some(u) = entry
            .url
            .as_deref()
            .map(str::trim)
            .filter(|u| !u.is_empty())
        {
            u.to_string()
        } else if let Some(asset) = entry.asset.as_deref() {
            format!(
                "{}/runtime-{}/{}",
                neomind_core::builtin_llm::runtime::JETSON_RUNTIME_BASE_URL,
                entry.tag,
                asset
            )
        } else {
            try_llama_server_url_for_tag(&entry.tag, os, arch, variant)
                .ok_or_else(|| format!("no asset URL for variant '{key}'"))?
        };
        if !url_allowed(&url) {
            return Err(format!(
                "channel entry '{key}' URL must be https (or loopback http): {url}"
            ));
        }
        Ok(UpgradeTarget {
            tag: entry.tag.clone(),
            url,
            sha256: Some(sha),
        })
    } else {
        let platform_key = format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH);
        let tag = doc
            .recommended_by_platform
            .get(&platform_key)
            .cloned()
            .or_else(|| doc.recommended.clone())
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| "channel has no recommended tag".to_string())?
            .to_string();
        if !validate_tag(&tag) {
            return Err(format!(
                "channel recommended tag '{tag}' is invalid — refusing"
            ));
        }
        let asset = llama_asset_name(os, arch, variant)
            .ok_or_else(|| "no prebuilt asset for this platform/variant".to_string())?;
        let ext = if llama_asset_is_zip(asset) {
            "zip"
        } else {
            "tar.gz"
        };
        let base = doc
            .official_base
            .as_deref()
            .map(str::trim)
            .filter(|b| !b.is_empty())
            .unwrap_or("https://github.com/ggml-org/llama.cpp");
        let url = format!("{base}/releases/download/{tag}/llama-{tag}-bin-{asset}.{ext}");
        if !url_allowed(&url) {
            return Err(format!(
                "channel official_base must be https (or loopback http): {base}"
            ));
        }
        // Official llama.cpp assets stay unpinned (immutable per release
        // tag; the recommendation itself is our curation).
        Ok(UpgradeTarget {
            tag,
            url,
            sha256: None,
        })
    }
}

/// Platform floor gate: refuse to install a runtime the current platform
/// version can't be trusted to manage.
pub fn channel_platform_gate(doc: &RuntimeChannelDoc) -> Result<(), String> {
    if let Some(min) = doc
        .min_platform
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if crate::upgrade::common::is_newer(min, APP_VERSION) {
            return Err(format!(
                "the channel recommends a runtime that needs platform version >= {min} \
                 (current {APP_VERSION}) — upgrade NeoMind first"
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Status snapshot (GET /api/builtin-llm/runtime + status `runtime` block)
// ---------------------------------------------------------------------------

/// Best-effort snapshot of the runtime layer. Cheap: all data comes from
/// memoized bundled-tag state, the dir listing, and the channel cache (a
/// non-forced `fetch_channel` normally returns the cached value).
pub async fn runtime_snapshot(data_dir: &Path) -> serde_json::Value {
    let snap_s = super::sources::snapshot();
    let variant = effective_variant();
    let vname = variant_name(&variant);

    // Bundled identity (Jetson bundled binaries that can't exec are not a
    // usable candidate; report them as unusable rather than hiding them).
    let bundled_path = find_llama_server().ok();
    let bundled_tag = match &bundled_path {
        Some(p) => bundled_tag(p).await,
        None => None,
    };

    let caches = cache_entries(data_dir);
    let installed: Vec<serde_json::Value> = caches
        .iter()
        .map(|(tag, v, path)| {
            serde_json::json!({
                "variant": variant_name(v),
                "tag": tag,
                "source": "cache",
                "path": path.to_string_lossy(),
            })
        })
        .collect();

    // What resolution WOULD pick right now, without network side effects.
    let same_variant: Vec<(String, PathBuf)> = entries_for_variant(data_dir, variant);
    let bundled_candidate = bundled_path.clone().map(|p| (p, bundled_tag.clone()));
    let active = match pick_runtime(same_variant.first().cloned(), bundled_candidate) {
        Decision::Cache { tag, path } => serde_json::json!({
            "variant": vname, "tag": tag, "source": "cache",
            "path": path.to_string_lossy(),
        }),
        Decision::Bundled(p) => serde_json::json!({
            "variant": vname,
            "tag": bundled_tag.clone().unwrap_or_else(|| "unknown".into()),
            "source": "bundled",
            "path": p.to_string_lossy(),
        }),
        Decision::TryPinThenBundled(p) => serde_json::json!({
            "variant": vname,
            "tag": bundled_tag.clone().unwrap_or_else(|| "unknown".into()),
            "source": "bundled",
            "path": p.to_string_lossy(),
            "note": "bundled is older than the platform pin; a newer runtime will be downloaded on next spawn",
        }),
        Decision::TryPin => serde_json::json!({
            "variant": vname, "tag": LLAMA_CPP_VERSION, "source": "pin-download",
            "path": llama_server_cache_dir_for_tag(data_dir, LLAMA_CPP_VERSION, variant).to_string_lossy(),
        }),
    };

    // Channel + upgrade availability — peek only, never fetch: this
    // snapshot feeds the 3s status poll and must stay cheap/offline-safe.
    let channel = peek_channel();
    let channel_state = if channel.is_some() {
        "ok"
    } else {
        "unreachable"
    };
    let mut recommended = serde_json::Value::Null;
    let mut update_available = false;
    let mut channel_error: Option<String> = None;
    if let Some(doc) = &channel {
        match resolve_upgrade_target(doc, variant) {
            Ok(target) => {
                let active_tag = active
                    .get("tag")
                    .and_then(|t| t.as_str())
                    .unwrap_or(LLAMA_CPP_VERSION)
                    .to_string();
                update_available = tag_cmp(&target.tag, &active_tag) == std::cmp::Ordering::Greater;
                recommended = serde_json::json!({
                    "tag": target.tag,
                    "notes_zh": doc.notes.as_ref().and_then(|n| n.zh.as_deref()),
                    "notes_en": doc.notes.as_ref().and_then(|n| n.en.as_deref()),
                });
            }
            Err(e) => channel_error = Some(e),
        }
    }

    serde_json::json!({
        "active": active,
        "bundled": {
            "present": bundled_path.is_some(),
            "tag": bundled_tag,
            "usable": bundled_path.is_some(), // Jetson exec-gate applies at resolution
            "path": bundled_path.map(|p| p.to_string_lossy().into_owned()),
        },
        "installed": installed,
        "detected_variant": vname,
        "override_variant": persisted_variant_override().map(|v| variant_name(&v)),
        "pin": LLAMA_CPP_VERSION,
        "recommended": recommended,
        "update_available": update_available,
        "channel": channel_state,
        "sources": {
            "hf_base_url": snap_s.hf_base_url,
            "hf_custom": snap_s.hf_custom,
            "catalog_url": snap_s.catalog_url,
            "catalog_custom": snap_s.catalog_custom,
            "runtime_channel_url": snap_s.runtime_channel_url,
            "runtime_channel_custom": snap_s.runtime_channel_custom,
            "last_download_error": last_download_error(),
        },
        "channel_error": channel_error,
        "upgrading": upgrade_active() || super::runtime::runtime_download_active(),
    })
}

pub fn variant_name(v: &RuntimeVariant) -> &'static str {
    match v {
        RuntimeVariant::Cpu => "cpu",
        RuntimeVariant::Cuda => "cuda",
        RuntimeVariant::Vulkan => "vulkan",
        RuntimeVariant::Jetson => "jetson",
    }
}

// ---------------------------------------------------------------------------
// Upgrade / revert
// ---------------------------------------------------------------------------

/// Optional payload bits of a progress event — keeps `publish` under the
/// clippy argument budget without a builder.
pub(crate) struct EventExtras {
    pub downloaded: Option<u64>,
    pub total: Option<u64>,
    pub message: Option<&'static str>,
    pub error: Option<String>,
}

fn publish(
    bus: &Option<Arc<EventBus>>,
    phase: &str,
    variant: &str,
    target: Option<&str>,
    current: Option<&str>,
    extras: EventExtras,
) {
    if let Some(bus) = bus {
        let _ = bus.publish_sync(NeoMindEvent::RuntimeUpgradeProgress {
            phase: phase.to_string(),
            variant: Some(variant.to_string()),
            target_tag: target.map(str::to_string),
            current_tag: current.map(str::to_string),
            downloaded: extras.downloaded,
            total: extras.total,
            message: extras.message.map(str::to_string),
            error: extras.error,
        });
    }
}

/// Orchestration-level single-flight for the whole upgrade (check →
/// download → restart); distinct from `runtime_download_active`, which only
/// covers the install step.
static UPGRADE_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Is an upgrade orchestration running right now? (For the status API.)
pub fn upgrade_active() -> bool {
    UPGRADE_ACTIVE.load(Ordering::SeqCst)
}

/// Clears `UPGRADE_ACTIVE` on drop (including task panic).
struct UpgradeActiveGuard;

impl Drop for UpgradeActiveGuard {
    fn drop(&mut self) {
        UPGRADE_ACTIVE.store(false, Ordering::SeqCst);
    }
}

/// Install the channel-recommended runtime for the effective variant into
/// its versioned cache dir. Returns `(from_tag, to_tag)`. The caller
/// restarts the llama-server child afterwards (see the handler) —
/// installing does not touch any running process.
pub async fn upgrade_runtime(
    data_dir: &Path,
    bus: Option<Arc<EventBus>>,
) -> Result<(String, String), String> {
    if UPGRADE_ACTIVE.swap(true, Ordering::SeqCst) {
        return Err("a runtime upgrade is already in flight".to_string());
    }
    let _guard = UpgradeActiveGuard;

    let variant = effective_variant();
    let vname = variant_name(&variant).to_string();

    // Mutual exclusion with the model download path. NOTE: no runtime_lock
    // acquisition HERE — install_runtime takes that lock itself, and a
    // tokio Mutex is not reentrant (holding it here deadlocked the very
    // first integration run). Orchestration-level single-flight is the
    // UPGRADE_ACTIVE guard above.
    if super::handlers::model_download_active() {
        return Err("a model download is in flight — retry after it finishes".to_string());
    }
    if super::runtime::runtime_download_active() {
        return Err("another runtime download is already in flight".to_string());
    }

    publish(
        &bus,
        "checking",
        &vname,
        None,
        None,
        EventExtras {
            downloaded: None,
            total: None,
            message: None,
            error: None,
        },
    );
    let current_tag = current_active_tag(data_dir, variant).await;

    let doc = fetch_channel(true)
        .await
        .ok_or_else(|| "update channel unreachable — cannot check for a new runtime".to_string())?;
    channel_platform_gate(&doc)?;
    let target = resolve_upgrade_target(&doc, variant)?;
    if let Some(cur) = current_tag.as_deref() {
        if tag_cmp(&target.tag, cur) != std::cmp::Ordering::Greater {
            return Err(format!(
                "recommended runtime {cur} is not newer than the active one — nothing to do"
            ));
        }
    }
    if !disk_space_ok(data_dir) {
        return Err(
            "not enough free disk space for the runtime archive (need ~2x its size free)"
                .to_string(),
        );
    }

    publish(
        &bus,
        "downloading",
        &vname,
        Some(&target.tag),
        current_tag.as_deref(),
        EventExtras {
            downloaded: Some(0),
            total: None,
            message: None,
            error: None,
        },
    );
    let bus_for_cb = bus.clone();
    let variant_for_cb = variant;
    let tag_for_cb = target.tag.clone();
    let mut last_pub: Instant = Instant::now() - Duration::from_secs(1); // publish the first chunk immediately
    let mut progress_cb = move |downloaded: u64, total: Option<u64>| {
        let now = Instant::now();
        if now.duration_since(last_pub) < Duration::from_millis(250) {
            return;
        }
        last_pub = now;
        publish(
            &bus_for_cb,
            "downloading",
            variant_name(&variant_for_cb),
            Some(&tag_for_cb),
            None,
            EventExtras {
                downloaded: Some(downloaded),
                total,
                message: None,
                error: None,
            },
        );
    };

    super::runtime::install_runtime(
        data_dir,
        variant,
        &target.tag,
        target.sha256.as_deref(),
        Some(&target.url),
        Some(&mut progress_cb),
    )
    .await
    .inspect_err(|e| {
        publish(
            &bus,
            "error",
            &vname,
            Some(&target.tag),
            None,
            EventExtras {
                downloaded: None,
                total: None,
                message: None,
                error: Some(e.clone()),
            },
        );
    })?;

    publish(
        &bus,
        "verifying",
        &vname,
        Some(&target.tag),
        current_tag.as_deref(),
        EventExtras {
            downloaded: None,
            total: None,
            message: None,
            error: None,
        },
    );
    invalidate_resolution_caches();
    prune_cache(data_dir, variant, &target.tag);
    Ok((current_tag.unwrap_or_default(), target.tag))
}

/// Tag resolution would hand to a spawn right now (for event payloads).
async fn current_active_tag(data_dir: &Path, variant: RuntimeVariant) -> Option<String> {
    let bundled_path = find_llama_server().ok();
    let bundled_tag = match &bundled_path {
        Some(p) => bundled_tag(p).await,
        None => None,
    };
    let best = entries_for_variant(data_dir, variant).into_iter().next();
    match pick_runtime(best, bundled_path.map(|p| (p, bundled_tag.clone()))) {
        Decision::Cache { tag, .. } => Some(tag),
        Decision::Bundled(_) | Decision::TryPinThenBundled(_) => bundled_tag,
        Decision::TryPin => Some(LLAMA_CPP_VERSION.to_string()),
    }
}

/// Delete the NEWEST cached runtime for the effective variant — resolution
/// falls back to the previous cache or the bundled binary on next spawn.
pub fn revert_runtime(data_dir: &Path) -> Result<String, String> {
    let variant = effective_variant();
    let entries = entries_for_variant(data_dir, variant);
    let (tag, path) = entries.first().ok_or_else(|| {
        "no downloaded runtime to revert (bundled/pin runtime in use)".to_string()
    })?;
    let tag = tag.clone();
    std::fs::remove_dir_all(path).map_err(|e| format!("remove {}: {e}", path.display()))?;
    invalidate_resolution_caches();
    tracing::info!(tag = %tag, "builtin llm: reverted runtime cache entry");
    Ok(tag)
}

/// ≥ ~400 MB free where the data dir lives (runtime archives are 20-80 MB;
/// 2x headroom covers extract-while-downloading).
fn disk_space_ok(data_dir: &Path) -> bool {
    use sysinfo::Disks;
    let disks = Disks::new_with_refreshed_list();
    let canonical = data_dir
        .canonicalize()
        .unwrap_or_else(|_| data_dir.to_path_buf());
    let mut best: Option<(usize, u64)> = None; // (mount prefix len, available)
    for d in disks.list() {
        let mount = d.mount_point();
        if canonical.starts_with(mount) {
            let len = mount.as_os_str().len();
            if best.as_ref().is_none_or(|(l, _)| len >= *l) {
                best = Some((len, d.available_space()));
            }
        }
    }
    match best {
        Some((_, avail)) => avail >= 400 * 1024 * 1024,
        None => true, // can't tell — don't block the upgrade on it
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- resolution matrix -------------------------------------------------

    fn cache(tag: &str) -> Option<(String, PathBuf)> {
        Some((tag.to_string(), PathBuf::from(format!("/x/{tag}"))))
    }
    fn bundled(tag: Option<&str>) -> Option<(PathBuf, Option<String>)> {
        Some((
            PathBuf::from("/bin/neomind-llama-server"),
            tag.map(str::to_string),
        ))
    }

    #[test]
    fn matrix_cache_newer_than_stale_bundled_wins() {
        // The original bug: install.sh left b10545 bundled, upgrade cached
        // b11609 → the cache must win.
        let d = pick_runtime(cache("b11609"), bundled(Some("b10545")));
        assert!(matches!(d, Decision::Cache { tag, .. } if tag == "b11609"));
    }

    #[test]
    fn matrix_unknown_bundled_yields_to_cache_but_survives_alone() {
        assert!(matches!(
            pick_runtime(cache("b11609"), bundled(None)),
            Decision::Cache { .. }
        ));
        // Unknown tag + no cache → today's bundled-first semantics.
        assert!(matches!(
            pick_runtime(None, bundled(None)),
            Decision::Bundled(_)
        ));
    }

    #[test]
    fn matrix_ties_go_to_cache() {
        let d = pick_runtime(cache("b11609"), bundled(Some("b11609")));
        assert!(matches!(d, Decision::Cache { .. }));
    }

    #[test]
    fn matrix_newer_bundled_beats_older_cache() {
        // Platform upgrade shipped a bundled runtime newer than the cache.
        // Pin-relative so a pin bump can't silently flip the fixture's
        // meaning (b11700 < pin b11960 made this assert TryPinThenBundled).
        let d = pick_runtime(cache("b10000"), bundled(Some(LLAMA_CPP_VERSION)));
        assert!(matches!(d, Decision::Bundled(_)));
    }

    #[test]
    fn matrix_bundled_below_pin_tries_download_then_falls_back() {
        let d = pick_runtime(None, bundled(Some("b10545")));
        assert!(matches!(d, Decision::TryPinThenBundled(_)));
        // ...but a pin-floored cache still beats it.
        let d = pick_runtime(cache("b11609"), bundled(Some("b10545")));
        assert!(matches!(d, Decision::Cache { .. }));
    }

    #[test]
    fn matrix_no_bundled_downloads_pin() {
        assert!(matches!(pick_runtime(None, None), Decision::TryPin));
        assert!(matches!(
            pick_runtime(None, bundled(Some(LLAMA_CPP_VERSION))),
            Decision::Bundled(_)
        ));
    }

    // ---- tag extraction ----------------------------------------------------

    #[test]
    fn first_tag_finds_official_version_strings() {
        assert_eq!(
            first_tag_in("version: 5668 (b11609) built with gcc"),
            Some("b11609".to_string())
        );
        assert_eq!(first_tag_in("ggml_cuda_init: found"), None);
        assert_eq!(first_tag_in(""), None);
        // Reject malformed neighbors, keep scanning.
        assert_eq!(
            first_tag_in("b1 then b11609 later"),
            Some("b11609".to_string())
        );
    }

    #[test]
    fn first_tag_parses_the_08x_banner() {
        // llama.cpp >= 0.8.0: `version: %s (build %d, commit %s)`.
        assert_eq!(
            first_tag_in("version: 0.8.0 (build 11960, commit 3f2a1bc)"),
            Some("b11960".to_string())
        );
        // An explicit b-tag still wins over a synthesized one.
        assert_eq!(
            first_tag_in("version: 0.8.0 (b11960) built with gcc"),
            Some("b11960".to_string())
        );
        // Random "build 12"/"build 12345678" noise must not synthesize.
        assert_eq!(first_tag_in("rebuild 12 times; built 12345678 units"), None);
    }

    // ---- strict variant parse ----------------------------------------------

    #[test]
    fn strict_variant_parse_rejects_garbage() {
        assert_eq!(parse_strict("cuda"), Some(RuntimeVariant::Cuda));
        assert_eq!(parse_strict(" CPU "), Some(RuntimeVariant::Cpu));
        assert_eq!(parse_strict("jetson"), Some(RuntimeVariant::Jetson));
        assert_eq!(parse_strict(""), None);
        assert_eq!(parse_strict("rockchip"), None);
    }

    // ---- channel doc -------------------------------------------------------

    fn doc(json: &str) -> RuntimeChannelDoc {
        serde_json::from_str(json).expect("valid doc")
    }

    #[test]
    fn channel_doc_parses_and_gates() {
        let d = doc(r#"{"schema":1,"recommended":"b11700",
                "variants":{"jetson":{"tag":"b11610","sha256":"60bc22258cb34796f2d1eb3900c3a6ca90a21df922c3a8e4f37f89e270720b4d"}},
                "min_platform":"0.8.0","notes":{"zh":"修复","en":"fixes"}}"#);
        assert_eq!(d.recommended.as_deref(), Some("b11700"));
        assert!(d.variants.contains_key("jetson"));
        assert!(channel_platform_gate(&d).is_ok());
        let too_new = doc(r#"{"recommended":"b11700","min_platform":"99.0.0"}"#);
        assert!(channel_platform_gate(&too_new).is_err());
    }

    #[test]
    fn channel_doc_tolerates_missing_optionals() {
        let d = doc("{}");
        assert!(d.recommended.is_none());
        assert!(d.variants.is_empty());
        assert!(channel_platform_gate(&d).is_ok());
    }

    // Channel-key mapping is host-dependent; assert the Jetson arm directly
    // and the official-asset default for CPU regardless of host.
    #[test]
    fn channel_target_resolution() {
        let d = doc(r#"{"recommended":"b11700",
                "variants":{
                  "jetson":{"tag":"b11610","sha256":"abc"},
                  "linux-cuda":{"tag":"b11610","sha256":"def","url":"https://mirror.example/x.tar.gz"}
                }}"#);
        // CPU (and any non-self-hosted variant) → official recommended.
        let t = resolve_upgrade_target(&d, RuntimeVariant::Cpu).expect("cpu target");
        assert_eq!(t.tag, "b11700");
        assert!(t.url.starts_with("https://github.com/ggml-org/llama.cpp/"));
        assert!(t.sha256.is_none());

        // Jetson → channel entry, SHA mandatory.
        let t = resolve_upgrade_target(&d, RuntimeVariant::Jetson).expect("jetson target");
        assert_eq!(t.tag, "b11610");
        assert_eq!(t.sha256.as_deref(), Some("abc"));

        // Missing entry → clear error.
        let empty = doc(r#"{"recommended":"b11700"}"#);
        assert!(resolve_upgrade_target(&empty, RuntimeVariant::Jetson).is_err());
        // Missing SHA on a self-hosted entry → refused.
        let no_sha = doc(r#"{"variants":{"jetson":{"tag":"b11610"}}}"#);
        assert!(resolve_upgrade_target(&no_sha, RuntimeVariant::Jetson).is_err());
        // Injection-shaped tags → refused.
        let evil = doc(r#"{"recommended":"../escape","variants":{}}"#);
        assert!(resolve_upgrade_target(&evil, RuntimeVariant::Cpu).is_err());
    }

    #[test]
    fn channel_platform_override_wins_for_this_host_only() {
        let os = std::env::consts::OS;
        let arch = std::env::consts::ARCH;
        let d = doc(&format!(
            r#"{{"recommended":"b11960","recommended_by_platform":{{"{os}/{arch}":"b11609"}}}}"#
        ));
        // This host's override wins…
        let t = resolve_upgrade_target(&d, RuntimeVariant::Cpu).expect("target");
        assert_eq!(t.tag, "b11609");
        // …and an override for a DIFFERENT platform does not apply.
        let other = if format!("{os}/{arch}") == "linux/x86_64" {
            "linux/aarch64"
        } else {
            "linux/x86_64"
        };
        let d2 = doc(&format!(
            r#"{{"recommended":"b11960","recommended_by_platform":{{"{other}":"b11609"}}}}"#
        ));
        let t2 = resolve_upgrade_target(&d2, RuntimeVariant::Cpu).expect("target");
        assert_eq!(t2.tag, "b11960");
        // An invalid platform override fails loudly (authoring bug — same
        // policy as an invalid global recommendation).
        let d3 = doc(&format!(
            r#"{{"recommended":"b11960","recommended_by_platform":{{"{os}/{arch}":"../escape"}}}}"#
        ));
        assert!(resolve_upgrade_target(&d3, RuntimeVariant::Cpu).is_err());
    }

    // ---- min_runtime gate ----------------------------------------------------

    #[test]
    fn runtime_gate_matrix() {
        use super::runtime_gate as gate;
        // Active older than the floor → refused, message names both tags.
        let err = gate(Some("b11609"), "b11960").unwrap_err();
        assert!(err.contains("b11960") && err.contains("b11609"), "{err}");
        // Active at/above the floor → OK. Numeric (b10 > b2) not string order.
        assert!(gate(Some("b11960"), "b11960").is_ok());
        assert!(gate(Some("b11972"), "b11960").is_ok());
        assert!(gate(Some("b12000"), "b11960").is_ok());
        // Unknown active version → permissive (never block on unknown data).
        assert!(gate(None, "b11960").is_ok());
    }

    // ---- cache enumeration ---------------------------------------------------

    #[test]
    fn entries_respect_variant_and_prune_below_pin() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("llama-server");
        for name in [
            LLAMA_CPP_VERSION.to_string(),         // pin, cpu — kept
            format!("{LLAMA_CPP_VERSION}-jetson"), // pin, jetson — other variant
            "b10545".to_string(),                  // below pin, cpu — pruned
            "b99999".to_string(),                  // future, cpu — kept
            ".tmp-download".to_string(),           // unparseable — skipped
        ] {
            std::fs::create_dir_all(root.join(&name)).unwrap();
            std::fs::write(root.join(&name).join("llama-server"), b"x").ok();
        }
        let cpu = entries_for_variant(tmp.path(), RuntimeVariant::Cpu);
        let tags: Vec<String> = cpu.iter().map(|(t, _)| t.clone()).collect();
        assert_eq!(
            tags,
            vec!["b99999".to_string(), LLAMA_CPP_VERSION.to_string()]
        );
        // Below-pin dir got pruned on sight.
        assert!(!root.join("b10545").exists());
        // Unparseable dirs are skipped, not deleted.
        assert!(root.join(".tmp-download").exists());
    }

    #[test]
    fn prune_keeps_newest_two_same_variant() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("llama-server");
        for tag in ["b90000", "b80000", "b70000"] {
            std::fs::create_dir_all(root.join(tag)).unwrap();
        }
        prune_cache(tmp.path(), RuntimeVariant::Cpu, "b90000");
        assert!(root.join("b90000").exists());
        assert!(root.join("b80000").exists());
        assert!(!root.join("b70000").exists());
    }

    #[test]
    fn revert_removes_newest_only() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("llama-server");
        for tag in ["b90000", "b80000"] {
            std::fs::create_dir_all(root.join(tag)).unwrap();
        }
        let removed = revert_runtime(tmp.path()).expect("revert");
        assert_eq!(removed, "b90000");
        assert!(!root.join("b90000").exists());
        assert!(root.join("b80000").exists());
        // Nothing left to revert on an empty dir.
        let tmp2 = tempfile::tempdir().unwrap();
        assert!(revert_runtime(tmp2.path()).is_err());
    }

    // ---- end-to-end upgrade against a local channel ------------------------
    // Serves runtimes.json + a fake asset archive from a loopback HTTP
    // server, then drives the full upgrade_runtime chain offline.

    async fn serve_files(
        listener: tokio::net::TcpListener,
        files: std::sync::Arc<HashMap<String, Vec<u8>>>,
    ) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        loop {
            let (mut sock, _) = match listener.accept().await {
                Ok(x) => x,
                Err(_) => break,
            };
            let files = files.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split(' ').nth(1).unwrap_or("").to_string();
                let (status, body) = match files.get(&path) {
                    Some(b) => ("200 OK", b.clone()),
                    None => ("404 Not Found", Vec::new()),
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = sock.write_all(head.as_bytes()).await;
                let _ = sock.write_all(&body).await;
            });
        }
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)] // serializing the WHOLE async test
    async fn upgrade_end_to_end_against_local_channel() {
        use std::io::Write;
        // Same serializer as the handler tests: this test drives UPGRADE_ACTIVE
        // / the download globals, which download_handler's conflict guard
        // observes — running them in parallel made those tests flake.
        let _serial = super::super::handlers::builtin_llm_test_serial()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tag = "b99999";
        // Fake llama-server: a shell script that reports the tag and exits 0
        // (what exec_check / version parsing expect).
        let script = "#!/bin/sh\necho \"version: 9999 (b99999)\"\nexit 0\n";
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(script.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(
                &mut header,
                "bin/llama-server",
                std::io::Cursor::new(script),
            )
            .unwrap();
        let uncompressed = builder.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&uncompressed).unwrap();
        let archive = gz.finish().unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let asset = llama_asset_name(
            std::env::consts::OS,
            std::env::consts::ARCH,
            RuntimeVariant::Cpu,
        )
        .expect("cpu asset on the test host");
        let ext = if llama_asset_is_zip(asset) {
            "zip"
        } else {
            "tar.gz"
        };
        let channel_json = format!(
            r#"{{"schema":1,"recommended":"{tag}","official_base":"http://{addr}","min_platform":"0.0.1"}}"#
        );
        let mut files: HashMap<String, Vec<u8>> = HashMap::new();
        files.insert("/runtimes.json".to_string(), channel_json.into_bytes());
        files.insert(
            format!("/releases/download/{tag}/llama-{tag}-bin-{asset}.{ext}"),
            archive.clone(),
        );
        // Windows hosts would need a zip archive; skip there (CI + dev are
        // mac/linux) rather than building a second fixture.
        if ext == "zip" {
            return;
        }
        let server = tokio::spawn(serve_files(listener, std::sync::Arc::new(files)));

        // Point the channel + variant at the fixture (serial: env + global
        // channel cache are process-wide).
        std::env::set_var(
            "NEOMIND_RUNTIME_CHANNEL_URL",
            format!("http://{addr}/runtimes.json"),
        );
        std::env::set_var("NEOMIND_BUILTIN_RUNTIME_VARIANT", "cpu");

        let tmp = tempfile::tempdir().unwrap();
        let result = upgrade_runtime(tmp.path(), None).await;

        // Cleanup first so a failed assertion doesn't leak env/channel state.
        std::env::remove_var("NEOMIND_RUNTIME_CHANNEL_URL");
        std::env::remove_var("NEOMIND_BUILTIN_RUNTIME_VARIANT");
        *channel_slot().lock().unwrap_or_else(|e| e.into_inner()) = None;
        invalidate_resolution_caches();
        server.abort();

        let (_from, to) = result.expect("upgrade succeeds");
        assert_eq!(to, tag);
        let cache = llama_server_cache_dir_for_tag(tmp.path(), tag, RuntimeVariant::Cpu);
        assert!(cache.is_dir(), "cache dir {}", cache.display());
        assert!(super::super::runtime::find_runtime_binary(&cache).is_some());
        // And resolution now picks it up (no bundled binary in the test env,
        // so the new cache is simply the newest candidate).
        assert!(entries_for_variant(tmp.path(), RuntimeVariant::Cpu)
            .first()
            .is_some_and(|(t, _)| t == tag));
    }
}
