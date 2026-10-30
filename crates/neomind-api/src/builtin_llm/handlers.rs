//! REST handlers for `/api/builtin-llm/*`.
//!
//! Endpoints:
//! - `GET    /api/builtin-llm/status`   — installed + server_state + progress
//! - `POST   /api/builtin-llm/download` — single-flight background model download
//! - `DELETE /api/builtin-llm/model`    — stop server + delete model files
//! - `POST   /api/builtin-llm/restart`  — ensure the bundled llama-server is running
//! - `POST   /api/builtin-llm/activate` — `set_active(BUILTIN_INSTANCE_ID)`
//!
//! # Server-state derivation
//!
//! `bootstrap()` (state.rs) drops its `LlamaServerProcess` handle, so no live
//! handle exists to probe. We derive liveness from the port instead:
//! `server_state = "running"` iff `health_check(cfg.port)` is true, else
//! `"stopped"`. `"downloading"` is reported while the single-flight download
//! lock is active. `"not_configured"` when no manifest / model file exists.
//! `"error"` when the manifest cannot be read.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use axum::extract::{Extension, Json, Query, State};
use serde_json::json;
use std::collections::HashMap;

use super::runtime::ensure_llama_server;
use neomind_agent::llm_backends::{get_instance_manager, LlmBackendInstanceManager};
use neomind_agent::LlmBackend;
use neomind_core::builtin_llm::manifest::{
    load_manifest, model_def, save_manifest, BuiltinModelDef, MmprojFile, ModelManifest,
    BUILTIN_MODELS, BUILTIN_MODEL_ID,
};
use neomind_core::builtin_llm::variant::Quant;
use neomind_core::NeoMindEvent;
use neomind_storage::{LlmBackendInstance, LlmBackendType};

use super::config::BuiltinConfig;
use super::download::{download_with_resume, DownloadProgress};
use super::server::{health_check, LlamaServerConfig, LlamaServerProcess};
use super::state::BUILTIN_INSTANCE_ID;
use crate::handlers::common::{ok, HandlerResult};
use crate::models::ErrorResponse;

// ---------------------------------------------------------------------------
// Process-level single-flight download state
// ---------------------------------------------------------------------------

/// Serializes `POST /api/builtin-llm/download` so a concurrent request gets
/// `{ started: false, already_running: true }` instead of a double download.
static DOWNLOAD_LOCK: OnceLock<Arc<tokio::sync::Mutex<()>>> = OnceLock::new();

/// Authoritative "a download is in progress" flag for the status endpoint.
/// (The mutex above is the single-flight gate; this flag is the observable
/// state, updated even across the tiny window between lock acquisition and
/// the background task actually starting.)
static DL_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Whether a builtin model download is in flight — the runtime upgrade
/// refuses to start while this is true (mutual exclusion, the other
/// direction being `runtime::runtime_download_active`).
pub(crate) fn model_download_active() -> bool {
    DL_ACTIVE.load(Ordering::SeqCst)
}

/// Cross-module test serializer for the builtin-LLM process-globals
/// (download lock, UPGRADE_ACTIVE, DL_ACTIVE). Tests touching those must
/// hold this guard for their whole body.
#[cfg(test)]
pub(crate) fn builtin_llm_test_serial() -> &'static std::sync::Mutex<()> {
    static LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

/// Latest progress values reported by the download callback (0 = unknown).
static DL_DOWNLOADED: AtomicU64 = AtomicU64::new(0);
static DL_TOTAL: AtomicU64 = AtomicU64::new(0);

/// Model id currently downloading (for the status endpoint + events).
static DL_MODEL: OnceLock<std::sync::Mutex<String>> = OnceLock::new();
fn dl_model() -> &'static std::sync::Mutex<String> {
    DL_MODEL.get_or_init(|| std::sync::Mutex::new(BUILTIN_MODEL_ID.to_string()))
}

fn download_lock() -> Arc<tokio::sync::Mutex<()>> {
    DOWNLOAD_LOCK
        .get_or_init(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

/// Clears `DL_ACTIVE` on drop (including task panic) so the status endpoint
/// never sticks in "downloading" after a crashed download task.
struct DownloadActiveGuard;

impl Drop for DownloadActiveGuard {
    fn drop(&mut self) {
        DL_ACTIVE.store(false, Ordering::SeqCst);
    }
}

pub(crate) fn models_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("models")
}

// ---------------------------------------------------------------------------
// Pinned download source (HuggingFace)
// ---------------------------------------------------------------------------

/// Filename *in the HF repo*. Note: this differs from our local
/// [`model_file_name`] (`lfm25-2.6b-q4_k_m.gguf`) — the repo names files
/// `LFM2.5-2.6B-Q4_K_M.gguf`. We download from the HF name but store under
/// the local canonical name (the manifest + llama-server only care about the
/// local path).
fn hf_file_name(quant: Quant) -> &'static str {
    match quant {
        Quant::Q4_K_M => "LFM2.5-2.6B-Q4_K_M.gguf",
        Quant::Q8_0 => "LFM2.5-2.6B-Q8_0.gguf",
        Quant::QAD_Q4_0 => "LFM2.5-2.6B-QAD-Q4_0.gguf",
    }
}

/// Official LFS SHA256 for each quant (pinned 2026-08-20). LFS blobs are
/// content-addressed, so these stay valid unless the repo owner re-uploads the
/// file under the same name. If a download ever fails with a sha mismatch,
/// re-capture from the HF API and update both arms.
fn hf_sha256(quant: Quant) -> &'static str {
    match quant {
        Quant::Q4_K_M => "79fdf00351b46cf26f020aead28d01889886be87c55fa0eb907e6f9b00bfee14",
        Quant::Q8_0 => "36587fdf27bdfc69caf2637273679a0870ec155162161bde6fd16e8c70bdb757",
        Quant::QAD_Q4_0 => "a247afd6414918eac8e520a9e6137dc271235461ecbe1180462221d5b8d40b03",
    }
}

/// Convert a compiled-in curated def into the unified catalog shape, so the
/// picker / download / spawn treat remote and builtin models identically.
fn builtin_to_catalog(def: &BuiltinModelDef) -> super::catalog::CatalogModel {
    super::catalog::CatalogModel {
        id: def.manifest.id.clone(),
        display_name: def.display_name.to_string(),
        file_name: def.manifest.file_name.clone(),
        sha256: def.manifest.sha256.clone(),
        quant: def.manifest.quant.clone(),
        format: def.manifest.format.clone(),
        target_hardware: None,
        min_runtime: None,
        hf_repo: def.hf_repo.to_string(),
        hf_file: def.hf_file.to_string(),
        size_bytes: def.size_bytes,
        default_ctx: def.default_ctx,
        max_ctx: Some(def.max_ctx),
        default_thinking: def.default_thinking,
        min_ram_mb: def.min_ram_mb as u32,
        notes: def.notes.to_string(),
        recommended: def.recommended,
        temperature: def.temperature,
        top_p: def.top_p,
        top_k: def.top_k,
        min_p: def.min_p,
        repeat_penalty: def.repeat_penalty,
        mmproj: def.mmproj.clone(),
        role: def.role.map(str::to_string),
    }
}

// ---------------------------------------------------------------------------
// Vision-companion (VLM) role helpers
// ---------------------------------------------------------------------------

/// Stable instance id for the vision companion running `model_id` (e.g.
/// `builtin-vlm-minicpm-v-4.7-1b`). The `builtin-vlm-` prefix IS the role
/// marker — derived, never stored, so no schema migration can drift it.
pub(crate) fn companion_instance_id(model_id: &str) -> String {
    format!("builtin-vlm-{model_id}")
}

/// True when an instance id names a vision companion (not a chat backend).
pub(crate) fn is_companion_instance_id(id: &str) -> bool {
    id.starts_with("builtin-vlm-")
}

/// True when a model id names a vision-companion entry (registry or catalog
/// role == "vlm").
pub(crate) fn is_vlm_model(model_id: &str) -> bool {
    model_def(model_id)
        .map(|d| d.role == Some("vlm"))
        .unwrap_or(false)
        || super::catalog::catalog_model(model_id)
            .map(|m| m.role.as_deref() == Some("vlm"))
            .unwrap_or(false)
}

/// The installed vision companion, if any (scan on-disk ids for a vlm role —
/// main-model discovery deliberately ignores role, this one filters on it).
pub(crate) fn installed_companion(mdir: &Path) -> Option<InstalledModel> {
    let mut companion = installed_model_ids(mdir)
        .into_iter()
        .find(|id| is_vlm_model(id))
        .and_then(|id| installed_model_by_id(mdir, &id))?;
    // by-id resolution only knows the id — restamp the registry's display
    // name so status/UI show "MiniCPM-V-4.7-1B", not the slug.
    if let Some(def) = model_def(&companion.manifest.id) {
        companion.display_name = def.display_name.to_string();
    }
    Some(companion)
}

/// Why a companion spawn didn't happen (status surfaces it; none of these
/// are errors — the main model is unaffected).
#[derive(Debug)]
pub(crate) enum CompanionSkip {
    Disabled,
    NotInstalled,
    LowRam {
        available_mb: u64,
        need_mb: u64,
    },
    PortBusy(u16),
    /// Spawn attempted and failed (binary missing, unhealthy, …). Still not
    /// an error for the platform — the main model runs on without vision.
    Other(String),
}

impl std::fmt::Display for CompanionSkip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompanionSkip::Disabled => write!(f, "disabled (NEOMIND_BUILTIN_VLM=off)"),
            CompanionSkip::NotInstalled => write!(f, "no vision companion installed"),
            CompanionSkip::LowRam {
                available_mb,
                need_mb,
            } => write!(
                f,
                "skipped: low RAM (need ~{need_mb} MB, {available_mb} MB available)"
            ),
            CompanionSkip::PortBusy(p) => write!(f, "skipped: port {p} in use"),
            CompanionSkip::Other(msg) => write!(f, "spawn failed: {msg}"),
        }
    }
}

/// RAM gate for the companion: weights + projector + headroom must fit in
/// AVAILABLE memory (the main model already holds its share — this is the
/// marginal budget for a second resident server).
fn companion_ram_ok(available_mb: u64, need_mb: u64) -> bool {
    available_mb >= need_mb
}

/// Resolve a model by id: compiled-in curated def first, else the remote
/// catalog (so a model added to the Runtimes catalog is installable without a
/// product release). `None` → unknown id.
fn resolve_model(model_id: Option<&str>) -> Result<super::catalog::CatalogModel, ErrorResponse> {
    let id = model_id.unwrap_or(BUILTIN_MODEL_ID);
    // Compiled registry wins resolution outright — a stale remote catalog
    // can never strip a projector (or anything else) from a known model.
    if let Some(def) = model_def(id) {
        return Ok(builtin_to_catalog(def));
    }
    if let Some(m) = super::catalog::catalog_model(id) {
        return Ok(m);
    }
    Err(ErrorResponse::bad_request(format!(
        "unknown builtin model id: {id} (available: {})",
        BUILTIN_MODELS
            .iter()
            .map(|d| d.manifest.id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    )))
}

/// Resolve the quant to download for a given model: LFM keeps the existing
/// quant-override machinery (q4_k_m / q8_0 / qad_q4_0); other models ship a
/// single canonical quant.
/// Resolve an explicit quant override for LFM (env `quant_override`). `None`
/// = no override → the registry def wins (LFM downloads QAD Q4_0, the agreed
/// default, whose sha is verified — the old implicit Q4_K_M default had a
/// stale sha and downloads kept failing verification).
fn resolve_quant(
    cfg: &BuiltinConfig,
    def: &super::catalog::CatalogModel,
) -> Result<Option<Quant>, ErrorResponse> {
    let Some(q) = cfg.quant_override.as_deref() else {
        return Ok(None);
    };
    // Quant override is per-model, not "default-model only": the old
    // def.id != BUILTIN_MODEL_ID gate silently broke the feature for LFM
    // the day the default moved to MiniCPM (every override errored), and
    // accepting QAD for non-LFM models produced a Q4_K_M download verified
    // against the LFM QAD sha — a guaranteed 1.5 GB dead download per
    // attempt. The supported set travels with the model.
    let supported: &[&str] = if def.id == "lfm25-2.6b" {
        &["q4_k_m", "q8_0", "qad_q4_0"]
    } else {
        &["q4_k_m", "q8_0"]
    };
    if !supported.contains(&q) {
        return Err(ErrorResponse::bad_request(format!(
            "unsupported quant '{q}' for {} (supported: {})",
            def.id,
            supported.join("/")
        )));
    }
    match q {
        "q4_k_m" => Ok(Some(Quant::Q4_K_M)),
        "q8_0" => Ok(Some(Quant::Q8_0)),
        "qad_q4_0" => Ok(Some(Quant::QAD_Q4_0)),
        _ => unreachable!("filtered above"),
    }
}

/// effective_ctx (env override / per-model default) + the spawn-time RAM
/// guard in one step — every spawn AND status path goes through this so the
/// window the UI shows is the window the server actually gets. Models
/// without a KV cost model (Ling, catalog-only imports) pass through
/// uncapped.
///
/// A spawn REPLACES the llama-server on `port`, so the available-RAM clamp
/// adds that server's live RSS back (it frees on replacement) — without
/// this, a status poll or respawn evaluated while the old server held
/// ~1.2 GB floored the window to 8192 (2026-10-15 incident). Servers on
/// other ports (the vision companion during a main respawn) stay out of
/// the add-back. Status polling uses [`guarded_ctx_quiet`] so the cap
/// doesn't warn every poll.
pub(crate) fn guarded_ctx(
    cfg: &BuiltinConfig,
    port: u16,
    default_ctx: u32,
    def: Option<&BuiltinModelDef>,
) -> usize {
    guarded_ctx_ex(cfg, port, default_ctx, def, true)
}

/// Default context budget by device RAM tier, applied only when no explicit
/// override is set. Deliberately conservative — a small box at a huge window
/// thrashes and OOMs rather than answering slower.
fn tier_default_ctx(total_ram_mb: u64) -> usize {
    const GB: u64 = 1024;
    if total_ram_mb < 8 * GB {
        16 * 1024
    } else if total_ram_mb < 16 * GB {
        32 * 1024
    } else if total_ram_mb < 32 * GB {
        64 * 1024
    } else {
        usize::MAX // no tier cap — the model default governs
    }
}

/// [`guarded_ctx`] without the cap WARN — for status/display paths that
/// compute a hypothetical spawn every UI poll (3s); the warn belongs to
/// actual spawns only.
pub fn guarded_ctx_quiet(
    cfg: &BuiltinConfig,
    port: u16,
    default_ctx: u32,
    def: Option<&BuiltinModelDef>,
) -> usize {
    guarded_ctx_ex(cfg, port, default_ctx, def, false)
}

fn guarded_ctx_ex(
    cfg: &BuiltinConfig,
    port: u16,
    default_ctx: u32,
    def: Option<&BuiltinModelDef>,
    warn_on_cap: bool,
) -> usize {
    let (total_mb, available_mb) = memory_snapshot_mb();
    // Tier default when the user has NOT overridden: the policy layer under
    // the hard available-RAM guard below. Evidence from the 7.4 GB NG4520:
    // 32K+f16 OOM-killed llama-server 10× over two days; 16K was the stable
    // window (and 128K never loaded there at all). Conservative by RAM,
    // always further capped by the guard, always overridable.
    let requested = match cfg.ctx {
        Some(v) => v,
        None => (default_ctx as usize).min(tier_default_ctx(total_mb)),
    };
    let Some(d) = def else {
        return requested;
    };
    let Some(kv_per_token) = d.kv_bytes_per_token else {
        return requested;
    };
    // The registry cost is the f16 figure; the spawn default quantizes KV to
    // q8_0 (≈half), so model what is actually allocated — otherwise the
    // guard over-reserves ~675 MB at 32K and clamps spawns the box can fund
    // (observed: a 32K request clamped to 16K on the NG4520 with q8 active).
    let kv_per_token = if cfg.kv_quant {
        kv_per_token / 2
    } else {
        kv_per_token
    };
    // Replacement semantics: the live server ON THIS PORT frees when the new
    // spawn takes over, so count its RSS as available. Port-scoped: the
    // vision companion keeps running through a main respawn — its memory is
    // not about-to-free, and counting it inflated the budget on every
    // companion-box respawn (the RAM analogue of the VRAM planner's
    // per-port reclaim).
    let live_rss = super::server::live_llama_rss_bytes_on_port(port);
    // Resident weights include the vision projector when the model ships one
    // (~0.7–1 GB) — a budget that ignores it green-lights spawns that OOM
    // the moment llama-server mmaps the mtmd adapter.
    let mmproj_bytes = d.mmproj.as_ref().map(|m| m.size_bytes).unwrap_or(0);
    let capped = super::server::cap_ctx_by_ram(
        requested,
        total_mb * 1_048_576,
        available_mb * 1_048_576 + live_rss,
        d.size_bytes + mmproj_bytes,
        kv_per_token,
        d.kv_const_bytes.unwrap_or(0),
    );
    if capped < requested && warn_on_cap {
        tracing::warn!(
            model = %d.manifest.id,
            requested,
            capped,
            total_ram_mb = total_mb,
            "builtin llm: ctx capped by device RAM (KV cache would not fit)"
        );
    }
    capped
}

/// The installed model, if any (scan the registry — one builtin model is
/// installed at a time; the server runs one llama-server process).
///
/// Returns the *on-disk* manifest (its `file_name` is the real file — e.g.
/// Docker may pre-bundle QAD under the QAD name) plus the registry def (for
/// ctx/name/thinking flags).
/// Static parts of a CATALOG-ONLY model, leaked once per process. The
/// registry (`BuiltinModelDef`) is `&'static str`-shaped; catalog-only ids
/// synthesize into it via these memoized strings (bounded: one leak per id).
struct CatalogOnlyInfo {
    display_name: &'static str,
    hf_repo: &'static str,
    hf_file: &'static str,
    notes: &'static str,
    role: Option<&'static str>,
    mmproj: Option<neomind_core::builtin_llm::manifest::MmprojSource>,
    temperature: Option<f32>,
    top_p: Option<f32>,
    top_k: Option<u32>,
    min_p: Option<f32>,
    repeat_penalty: Option<f32>,
    default_ctx: u32,
    max_ctx: u32,
    size_bytes: u64,
    min_ram_mb: u64,
    default_thinking: bool,
}

fn leak_str(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

fn catalog_only_info(id: &str) -> Option<&'static CatalogOnlyInfo> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<String, Option<&'static CatalogOnlyInfo>>>> =
        OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(hit) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(id) {
        return *hit;
    }
    // Registry ids never take this path (their defs are already static).
    let computed: Option<Box<CatalogOnlyInfo>> = (|| {
        if BUILTIN_MODELS.iter().any(|d| d.manifest.id == id) {
            return None;
        }
        let cat =
            super::catalog::cached_catalog().and_then(|c| c.into_iter().find(|m| m.id == id))?;
        let default_ctx = cat.default_ctx;
        Some(Box::new(CatalogOnlyInfo {
            display_name: leak_str(&cat.display_name),
            hf_repo: leak_str(&cat.hf_repo),
            hf_file: leak_str(&cat.hf_file),
            notes: leak_str(&cat.notes),
            role: cat.role.as_deref().map(leak_str),
            mmproj: cat.mmproj.clone(),
            temperature: cat.temperature,
            top_p: cat.top_p,
            top_k: cat.top_k,
            min_p: cat.min_p,
            repeat_penalty: cat.repeat_penalty,
            default_ctx,
            max_ctx: cat.max_ctx.unwrap_or(default_ctx),
            size_bytes: cat.size_bytes,
            min_ram_mb: u64::from(cat.min_ram_mb),
            default_thinking: cat.default_thinking,
        }))
    })();
    // Leak once per id — the &'static return keeps downstream signatures
    // (&'static str fields) unchanged; bounded by the catalog size.
    let leaked: Option<&'static CatalogOnlyInfo> = computed.map(|b| &*Box::leak(b));
    cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id.to_string(), leaked);
    leaked
}

/// Synthesize a registry-shaped def for a CATALOG-ONLY model from its
/// on-disk manifest + the cached catalog. Bootstrap and restart validation
/// are registry-shaped; without this a catalog-only active model vanishes
/// on cold boot (state's ModelMissing) and restart?model= 400s. The KV
/// fields stay None — the RAM guard skips unknown models by design (the
/// GGUF-header cost model is a separate workstream). Returns None for
/// registry ids (they take the registry path) and models without a
/// manifest on disk.
pub(crate) fn catalog_only_def(mdir: &Path, id: &str) -> Option<BuiltinModelDef> {
    // Registry ids are served by their static def, never this path.
    if BUILTIN_MODELS.iter().any(|d| d.manifest.id == id) {
        return None;
    }
    let manifest = load_manifest(mdir, id).ok().flatten()?;
    if !manifest.model_path(mdir).exists() {
        return None;
    }
    // Catalog info when the remote catalog is warm; otherwise fall back to a
    // manifest-derived def so a cold boot can still run the active model
    // (2026-10-30: active pointer lfm3-1.7b was unusable until /models had
    // warmed the process cache, so the scan ran minicpm5-2b instead).
    let info = catalog_only_info(id);
    let ctx = info.map(|i| i.default_ctx).unwrap_or_else(|| {
        // Conservative offline default: never let a 1M-native GGUF become the
        // spawn default just because the catalog was cold. The user can raise
        // ctx explicitly; the RAM guard caps it further.
        let native = super::gguf::parse_gguf(&manifest.model_path(mdir))
            .ok()
            .and_then(|g| g.context_length)
            .and_then(|c| u32::try_from(c).ok())
            .unwrap_or(32_768);
        native.clamp(8_192, 32_768)
    });
    let size_bytes = info.map(|i| i.size_bytes).unwrap_or_else(|| {
        std::fs::metadata(manifest.model_path(mdir))
            .map(|m| m.len())
            .unwrap_or(0)
    });
    Some(BuiltinModelDef {
        manifest,
        display_name: info.map(|i| i.display_name).unwrap_or_else(|| leak_str(id)),
        hf_repo: info.map(|i| i.hf_repo).unwrap_or(""),
        hf_file: info.map(|i| i.hf_file).unwrap_or(""),
        size_bytes,
        default_ctx: ctx,
        max_ctx: info.map(|i| i.max_ctx).unwrap_or(ctx),
        kv_bytes_per_token: None,
        kv_const_bytes: None,
        temperature: info.and_then(|i| i.temperature),
        top_p: info.and_then(|i| i.top_p),
        top_k: info.and_then(|i| i.top_k),
        min_p: info.and_then(|i| i.min_p),
        repeat_penalty: info.and_then(|i| i.repeat_penalty),
        notes: info.map(|i| i.notes).unwrap_or(""),
        recommended: false,
        default_thinking: info.map(|i| i.default_thinking).unwrap_or(false),
        thinking_is_integral: false,
        min_ram_mb: info.map(|i| i.min_ram_mb).unwrap_or(0),
        mmproj: info.and_then(|i| i.mmproj.clone()),
        role: info.and_then(|i| i.role),
    })
}

pub fn installed_model(mdir: &Path) -> Option<(BuiltinModelDef, ModelManifest)> {
    // Active pointer wins: downloaded model files are kept across switches
    // (multi-model coexistence — switching must never re-download, the
    // single-model purge made every lfm↔minicpm flip cost 1.5 GB), so the
    // pointer is the only authority for "which one is active". Fall back to
    // the registry-order scan for single-model installs predating the
    // pointer file.
    if let Some(active) = read_active_model(mdir) {
        // The pointer may name a model that is NOT in the compiled registry:
        // remote-catalog models (LFM3 etc.) install under their catalog id and
        // must remain switchable/active. Resolving only against BUILTIN_MODELS
        // treated a perfectly valid catalog-only pointer as stale and fell
        // back to the first installed compiled model (2026-10-30: pointer
        // lfm3-1.7b silently ran minicpm5-2b, and the picker kept that name).
        let def = BUILTIN_MODELS
            .iter()
            .find(|d| d.manifest.id == active)
            .cloned()
            .or_else(|| catalog_only_def(mdir, &active));
        if let Some(def) = def {
            if let Ok(Some(manifest)) = load_manifest(mdir, &def.manifest.id) {
                if manifest.model_path(mdir).exists() {
                    return Some((def, manifest));
                }
            }
        }
        tracing::warn!(
            active = %active,
            "builtin llm: active-model pointer stale (model missing) — falling back to scan"
        );
    }
    BUILTIN_MODELS
        .iter()
        .find_map(|def| {
            let manifest = load_manifest(mdir, &def.manifest.id).ok().flatten()?;
            manifest
                .model_path(mdir)
                .exists()
                .then(|| (def.clone(), manifest))
        })
        // Catalog-only fallback (models added via the remote catalog): newest
        // on-disk manifest whose id isn't in the registry, synthesized into a
        // registry-shaped def. Without this a catalog-only active model
        // vanishes on cold boot (bootstrap's ModelMissing).
        .or_else(|| {
            let builtin: std::collections::HashSet<&str> = BUILTIN_MODELS
                .iter()
                .map(|d| d.manifest.id.as_str())
                .collect();
            let mut best: Option<(BuiltinModelDef, ModelManifest, std::time::SystemTime)> = None;
            let Ok(rd) = std::fs::read_dir(mdir) else {
                return None;
            };
            for entry in rd.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let id = entry.file_name().to_string_lossy().into_owned();
                if builtin.contains(id.as_str()) {
                    continue;
                }
                let Ok(manifest) = load_manifest(mdir, &id) else {
                    continue;
                };
                let Some(manifest) = manifest else {
                    continue;
                };
                if !manifest.model_path(mdir).exists() {
                    continue;
                }
                let mtime = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                if best.as_ref().is_none_or(|(_, _, best_t)| mtime >= *best_t) {
                    if let Some(def) = catalog_only_def(mdir, &id) {
                        best = Some((def, manifest, mtime));
                    }
                }
            }
            best.map(|(d, m, _)| (d, m))
        })
}

// ---------------------------------------------------------------------------
// Active-model pointer (data/models/.active-model)
// ---------------------------------------------------------------------------

/// The id of the model the builtin server should run. One line, plain text,
/// beside the model dirs — visible, debuggable, survives restarts.
const ACTIVE_MODEL_FILE: &str = ".active-model";

pub fn read_active_model(mdir: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(mdir.join(ACTIVE_MODEL_FILE)).ok()?;
    let id = raw.trim().to_string();
    (!id.is_empty()).then_some(id)
}

pub fn write_active_model(mdir: &Path, id: &str) {
    if let Err(e) = std::fs::write(mdir.join(ACTIVE_MODEL_FILE), format!("{id}\n")) {
        tracing::warn!(error = %e, id, "builtin llm: failed to persist active-model pointer");
    }
}

pub fn clear_active_model(mdir: &Path) {
    let _ = std::fs::remove_file(mdir.join(ACTIVE_MODEL_FILE));
}

/// Every builtin model whose files are on disk (for the status/picker UI).
pub fn installed_model_ids(mdir: &Path) -> Vec<String> {
    let mut ids: Vec<String> = BUILTIN_MODELS
        .iter()
        .filter(|def| {
            load_manifest(mdir, &def.manifest.id)
                .ok()
                .flatten()
                .is_some_and(|m| m.model_path(mdir).exists())
        })
        .map(|def| def.manifest.id.clone())
        .collect();
    // Catalog-only installs: manifest dirs whose id isn't in the registry.
    // Without these, restart?model=<catalog-id> 400s "not downloaded" even
    // though the files are on disk and the spawn path handles them fine.
    if let Ok(rd) = std::fs::read_dir(mdir) {
        for entry in rd.flatten() {
            let id = entry.file_name().to_string_lossy().into_owned();
            if ids.iter().any(|v| v == &id) || id.starts_with('.') {
                continue;
            }
            let is_installed = load_manifest(mdir, &id)
                .ok()
                .flatten()
                .is_some_and(|m| m.model_path(mdir).exists());
            if is_installed {
                ids.push(id);
            }
        }
    }
    ids
}

/// Default context for an imported GGUF: the header's context_length capped
/// at 128K (a header claiming 1M would OOM llama-server). Single source so
/// spawn, the picker and the import response never diverge.
pub fn import_default_ctx(model_path: &Path) -> u32 {
    super::gguf::parse_gguf(model_path)
        .ok()
        .and_then(|m| m.context_length)
        .and_then(|c| u32::try_from(c).ok())
        .map(|c| c.min(131_072))
        .unwrap_or(32768)
}
/// Owned view of the currently installed builtin-LLM model — builtin registry
/// OR a locally imported "custom" GGUF. Decouples the spawn/status paths from
/// the static `BuiltinModelDef` (whose string fields are `&'static`), so a
/// custom model participates in the single-model switch uniformly.
pub struct InstalledModel {
    pub manifest: ModelManifest,
    pub display_name: String,
    pub default_ctx: u32,
    /// Native context ceiling for display (None → fall back to default_ctx).
    pub max_ctx: Option<u32>,
    /// Per-model sampling point (official/measured). None (custom imports,
    /// older manifests) → platform-wide legacy default.
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<u32>,
    pub min_p: Option<f32>,
    pub repeat_penalty: Option<f32>,
    pub default_thinking: bool,
    /// Model property (see the registry field): thinking cannot be disabled.
    pub thinking_is_integral: bool,
    /// Vision projector recorded on the manifest (file presence is checked
    /// at spawn — a deleted file degrades to text-only).
    pub mmproj: Option<MmprojFile>,
    pub is_custom: bool,
}

/// The installed model: a builtin registry entry first, else the most recent
/// locally-imported custom model (`data/models/<id>/manifest.json`, id not in
/// the builtin registry).
pub fn installed_model_any(mdir: &Path) -> Option<InstalledModel> {
    if let Some((def, manifest)) = installed_model(mdir) {
        return Some(InstalledModel {
            manifest: manifest.clone(),
            display_name: def.display_name.to_string(),
            default_ctx: def.default_ctx,
            max_ctx: Some(def.max_ctx),
            temperature: def.temperature,
            top_p: def.top_p,
            top_k: def.top_k,
            min_p: def.min_p,
            repeat_penalty: def.repeat_penalty,
            default_thinking: def.default_thinking,
            thinking_is_integral: def.thinking_is_integral,
            mmproj: manifest.mmproj,
            is_custom: false,
        });
    }
    // Custom scan: newest manifest whose id isn't builtin.
    let builtin_ids: std::collections::HashSet<&str> = BUILTIN_MODELS
        .iter()
        .map(|d| d.manifest.id.as_str())
        .collect();
    let mut best: Option<(ModelManifest, std::time::SystemTime)> = None;
    if let Ok(entries) = std::fs::read_dir(mdir) {
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let Some(id) = dir.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            // Dot-dirs are import backups / internal state, not models.
            if id.starts_with('.') {
                continue;
            }
            if builtin_ids.contains(id) {
                continue;
            }
            let Ok(Some(manifest)) = load_manifest(mdir, id) else {
                continue;
            };
            if !manifest.model_path(mdir).exists() {
                continue;
            }
            let mtime = std::fs::metadata(&dir)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            if best.as_ref().map(|(_, t)| mtime > *t).unwrap_or(true) {
                best = Some((manifest, mtime));
            }
        }
    }
    let (manifest, _) = best?;
    let name = manifest.id.clone();
    let model_path = manifest.model_path(mdir);
    let ctx = import_default_ctx(&model_path);
    // The header's context_length IS the native ceiling — show it (display
    // only; the run default stays the capped conservative value above).
    let max = super::gguf::parse_gguf(&model_path)
        .ok()
        .and_then(|m| m.context_length)
        .and_then(|c| u32::try_from(c).ok())
        .or(Some(ctx));
    Some(InstalledModel {
        display_name: name,
        default_ctx: ctx,
        max_ctx: max,
        // Custom imports have no catalog entry — legacy global defaults.
        temperature: None,
        top_p: None,
        top_k: None,
        min_p: None,
        repeat_penalty: None,
        default_thinking: false,
        // Genuinely unknown provenance (a GGUF dropped in models/ with no
        // registry or catalog entry) — assume the thinking toggle works.
        // The IMPORT path goes through `installed_model_by_id`, which
        // resolves registry ids and keeps their integral-thinking property.
        thinking_is_integral: false,
        mmproj: manifest.mmproj.clone(),
        is_custom: true,
        manifest,
    })
}

/// Build an `InstalledModel` for a SPECIFIC id (import spawn path) — load its
/// manifest directly instead of relying on installed_model_any's preference
/// scan, so an import can spawn the new model before the old one is removed.
pub fn installed_model_by_id(mdir: &Path, id: &str) -> Option<InstalledModel> {
    let manifest = load_manifest(mdir, id).ok().flatten()?;
    if !manifest.model_path(mdir).exists() {
        return None;
    }
    let model_path = manifest.model_path(mdir);
    let ctx = import_default_ctx(&model_path);
    let max = super::gguf::parse_gguf(&model_path)
        .ok()
        .and_then(|m| m.context_length)
        .and_then(|c| u32::try_from(c).ok())
        .or(Some(ctx));
    // Catalog-only model? Try the cached catalog for its sampling point
    // (sync read of the last fetch; a cold process falls back to the global
    // defaults until the next /models refresh warms the cache).
    let cat = super::catalog::cached_catalog().and_then(|c| c.into_iter().find(|m| m.id == id));
    let (temperature, top_p, top_k, min_p, repeat_penalty) = cat
        .as_ref()
        .map(|m| (m.temperature, m.top_p, m.top_k, m.min_p, m.repeat_penalty))
        .unwrap_or((None, None, None, None, None));
    // Display name and thinking default come from the catalog too — the
    // slug + `false` fallback made every catalog-only model show its id and
    // lose its thinking default on this path.
    let display_name = cat
        .as_ref()
        .map(|m| m.display_name.clone())
        .unwrap_or_else(|| id.to_string());
    let default_thinking = cat
        .as_ref()
        .map(|m| m.default_thinking)
        .unwrap_or_else(|| neomind_core::llm::capability::detect_thinking(id));
    Some(InstalledModel {
        display_name,
        default_ctx: ctx,
        max_ctx: max,
        temperature,
        top_p,
        top_k,
        min_p,
        repeat_penalty,
        default_thinking,
        // If this id names a registry model (importing an LFM/Ling GGUF
        // under its real id), keep that model's integral-thinking property —
        // otherwise the import recreates the "toggle that does nothing"
        // state the registry field exists to prevent.
        thinking_is_integral: model_def(id)
            .map(|d| d.thinking_is_integral)
            .unwrap_or(false),
        mmproj: manifest.mmproj.clone(),
        is_custom: true,
        manifest,
    })
}

/// Download source for a def: HF repo + file + sha. Every model downloads its
/// registry entry directly; the env `quant_override` selects a different
/// quant. resolve_quant self-validates per model (supported-set gate), so no
/// default-model-only gate here: this branch previously (a) hardcoded
/// LiquidAI's repo — a MiniCPM override went looking for MiniCPM files in
/// the LFM repo and 404'd — and (b) blocked quant overrides for every
/// non-default model including catalog-only ones.
fn resolve_source(
    cfg: &BuiltinConfig,
    def: &super::catalog::CatalogModel,
) -> (String, String, String) {
    if let Ok(Some(quant)) = resolve_quant(cfg, def) {
        return (
            super::sources::hf_file_url(&def.hf_repo, &quant_file_name(&def.id, quant)),
            quant_sha256(&def.id, quant).to_string(),
            quant_local_file_name(&def.id, quant),
        );
    }
    (
        super::sources::hf_file_url(&def.hf_repo, &def.hf_file),
        def.sha256.clone(),
        def.file_name.clone(),
    )
}

/// Download source for the model's vision projector: `(url, sha256, local
/// file_name)`. Deliberately IGNORES the quant override — the projector is
/// quant-independent (it wraps the vision tower, not the text weights), so
/// every quant of a model shares the same mmproj file.
fn resolve_mmproj_source(def: &super::catalog::CatalogModel) -> Option<(String, String, String)> {
    let mm = def.mmproj.as_ref()?;
    Some((
        super::sources::hf_file_url(&mm.hf_repo, &mm.hf_file),
        mm.sha256.clone(),
        mm.file_name.clone(),
    ))
}

/// Local file name for a quant-override download: the model's own prefix
/// (the old shared helper hardcoded the LFM prefix — MiniCPM overrides
/// landed in models/minicpm5-2b/ under lfm-named files).
fn quant_local_file_name(model_id: &str, quant: Quant) -> String {
    format!("{}-{}.gguf", model_id, quant.key())
}

/// Official per-quant file names for quant-override downloads of the
/// default model (verified against the openbmb repo listing 2026-09-14).
fn quant_file_name(model_id: &str, quant: Quant) -> String {
    match (model_id, quant) {
        ("minicpm5-2b", Quant::Q4_K_M) => "MiniCPM5-2B-Q4_K_M.gguf".to_string(),
        ("minicpm5-2b", Quant::Q8_0) => "MiniCPM5-2B-Q8_0.gguf".to_string(),
        // LFM default (model_id "lfm25-2.6b") and any future default: the
        // hf_* tables carry the official names.
        _ => hf_file_name(quant).to_string(),
    }
}

/// Official LFS SHA256 per quant for the default model (captured from the
/// repo's LFS pointers 2026-09-14; blobs are content-addressed so these
/// hold unless the owner re-uploads under the same name).
fn quant_sha256(model_id: &str, quant: Quant) -> &'static str {
    match (model_id, quant) {
        ("minicpm5-2b", Quant::Q4_K_M) => {
            "ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd"
        }
        ("minicpm5-2b", Quant::Q8_0) => {
            "c5415f8989bf88a8288f1b55a3cc371af53c07b0faa220a63bd7a990cfaba078"
        }
        _ => hf_sha256(quant),
    }
}

// ---------------------------------------------------------------------------
// Local import (open catalog)
// ---------------------------------------------------------------------------

fn slugify(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if out.ends_with('-') {
            continue;
        } else {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "imported-model".to_string()
    } else {
        out
    }
}

fn sha256_of_file(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = std::fs::File::open(path).map_err(|e| format!("open: {e}"))?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf).map_err(|e| format!("read: {e}"))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// POST /api/builtin-llm/import-local — body `{ "path": "/abs/to/model.gguf" }`.
///
/// The open-catalog local channel: register an arbitrary GGUF into the builtin
/// model directory (validated + header-parsed), then participate in the
/// single-model switch exactly like a downloaded builtin — other model dirs are
/// removed, the port freed, and the server respawned for the imported model.
#[utoipa::path(
    post,
    path = "/api/builtin-llm/import-local",
    tag = "builtin-llm",
    request_body = serde_json::Value,
    responses(
        (status = 200, description = "Existing local model imported"),
    )
)]
pub async fn import_local_handler(
    State(state): State<crate::server::types::ServerState>,
    Json(req): Json<serde_json::Value>,
) -> HandlerResult<serde_json::Value> {
    let path = req
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ErrorResponse::bad_request("field 'path' required"))?;
    let src = std::path::Path::new(path);
    if !src.is_file() {
        return Err(ErrorResponse::bad_request(format!("not a file: {path}")));
    }
    import_gguf_from_path(&state, src).await
}

/// Stream an uploaded GGUF to a temp file, then run it through the same
/// import pipeline as `import-local` (validation, manifest, spawn,
/// rollback-on-failure). The temp copy is always removed afterwards — the
/// import itself copies into `data/models/<id>/`.
///
/// POST /api/builtin-llm/upload-model (multipart, field "file")
#[utoipa::path(
    post,
    path = "/api/builtin-llm/upload-model",
    tag = "builtin-llm",
    responses(
        (status = 200, description = "GGUF uploaded multipart (12GB limit)"),
    )
)]
pub async fn upload_model_handler(
    State(state): State<crate::server::types::ServerState>,
    mut multipart: axum::extract::Multipart,
) -> HandlerResult<serde_json::Value> {
    // Data dir for the temp upload (same tree the import copies into).
    let uploads_dir = std::path::Path::new(&state.data_dir)
        .join("models")
        .join("uploads");
    std::fs::create_dir_all(&uploads_dir)
        .map_err(|e| ErrorResponse::internal(format!("mkdir uploads: {e}")))?;

    // Find the "file" field and stream it to disk. Streaming (not buffering)
    // matters: GGUFs run to ~5 GB and must not be held in memory.
    let mut tmp_path: Option<PathBuf> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ErrorResponse::bad_request(format!("multipart: {e}")))?
    {
        if field.name() != Some("file") {
            continue; // ignore unknown fields (e.g. future metadata)
        }
        let file_name = field
            .file_name()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "upload.gguf".to_string());
        // Only keep the file name — drop any client path parts.
        let safe_name = std::path::Path::new(&file_name)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("upload.gguf")
            .to_string();
        let dest = uploads_dir.join(format!("{}-{}", uuid::Uuid::new_v4().simple(), safe_name));
        let mut file = tokio::io::BufWriter::new(
            tokio::fs::File::create(&dest)
                .await
                .map_err(|e| ErrorResponse::internal(format!("create temp file: {e}")))?,
        );
        use tokio::io::AsyncWriteExt;
        let mut stream = field;
        while let Some(chunk) = stream
            .chunk()
            .await
            .map_err(|e| ErrorResponse::bad_request(format!("read upload: {e}")))?
        {
            file.write_all(&chunk)
                .await
                .map_err(|e| ErrorResponse::internal(format!("write upload: {e}")))?;
        }
        file.flush()
            .await
            .map_err(|e| ErrorResponse::internal(format!("flush upload: {e}")))?;
        drop(file);
        tmp_path = Some(dest);
        break; // single-file upload
    }
    let tmp =
        tmp_path.ok_or_else(|| ErrorResponse::bad_request("multipart field 'file' required"))?;

    // Import from the temp path; always clean it up (the import copies).
    let result = import_gguf_from_path(&state, &tmp).await;
    let _ = tokio::fs::remove_file(&tmp).await;
    // Also sweep any leftovers from failed older uploads (best-effort).
    // Expire only STALE leftovers (>24h by mtime). Sweeping every file in
    // the directory unlinked the temp copy of any CONCURRENT upload, whose
    // import then failed on a missing file.
    let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(24 * 60 * 60);
    let _ = std::fs::read_dir(&uploads_dir).map(|rd| {
        for e in rd.flatten() {
            let stale = e
                .metadata()
                .and_then(|m| m.modified())
                .map(|t| t < cutoff)
                .unwrap_or(false);
            if stale {
                let _ = std::fs::remove_file(e.path());
            }
        }
    });
    result
}

/// Shared import pipeline: validate the GGUF at `src`, copy it into
/// `data/models/<id>/`, write the manifest, restart the builtin server on
/// the imported model — rolling back to the previous model on any failure.
async fn import_gguf_from_path(
    state: &crate::server::types::ServerState,
    src: &Path,
) -> HandlerResult<serde_json::Value> {
    let meta = super::gguf::parse_gguf(src)
        .map_err(|e| ErrorResponse::bad_request(format!("not a valid GGUF: {e}")))?;

    // A projector file is a VALID GGUF (it would parse above and then spawn
    // a llama-server with no language model in it). architecture 'clip' is
    // the mmproj signature — reject with a pointer at what the file is.
    if meta.architecture.as_deref() == Some("clip") {
        return Err(ErrorResponse::bad_request(
            "this file is a vision projector (mmproj, architecture 'clip'), not a runnable \
             model — vision projectors download automatically with models that ship one",
        ));
    }

    let mdir = models_dir(&state.data_dir);
    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("model");
    let id = slugify(meta.name.as_deref().unwrap_or(stem));
    // A slug colliding with a curated builtin id (e.g. general.name 'Gemma4-E2B'
    // → 'gemma4-e2b') would overwrite the builtin's manifest/file in place —
    // corrupting the curated entry and orphaning the real model. Reject.
    if BUILTIN_MODELS.iter().any(|d| d.manifest.id == id) {
        return Err(ErrorResponse::bad_request(format!(
            "imported model id '{id}' collides with a built-in model — rename the GGUF's general.name (or the file) so it doesn't map to a curated id"
        )));
    }
    let file_name = format!("{id}.gguf");
    let dest = mdir.join(&id).join(&file_name);
    // Same-id re-import: the copy below would overwrite the working model in
    // place — a failed spawn would then lose it. Back the existing dir aside
    // FIRST; restored on failure, discarded on success.
    let backup_dir = mdir.join(format!(".{id}-old"));
    let had_existing = mdir.join(&id).exists();
    if had_existing {
        let _ = std::fs::remove_dir_all(&backup_dir);
        std::fs::rename(mdir.join(&id), &backup_dir)
            .map_err(|e| ErrorResponse::internal(format!("backup existing model: {e}")))?;
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ErrorResponse::internal(format!("mkdir: {e}")))?;
    }
    if let Err(e) = std::fs::copy(src, &dest) {
        // Restore the backed-up model so a failed copy never leaves a hole.
        let _ = std::fs::remove_dir_all(mdir.join(&id));
        if had_existing {
            let _ = std::fs::rename(&backup_dir, mdir.join(&id));
        }
        return Err(ErrorResponse::internal(format!(
            "copy to {}: {e}",
            dest.display()
        )));
    }

    let post_copy = (|| -> Result<ModelManifest, ErrorResponse> {
        let sha =
            sha256_of_file(&dest).map_err(|e| ErrorResponse::internal(format!("sha256: {e}")))?;
        let quant = meta.quant.clone().unwrap_or_else(|| "imported".to_string());
        Ok(ModelManifest {
            id: id.clone(),
            version: "1.0".to_string(),
            file_name: file_name.clone(),
            sha256: sha,
            quant,
            // The import pipeline is GGUF-only today; an RKNN/other-format
            // import gets its own validated path when such models ship.
            format: neomind_core::builtin_llm::manifest::MODEL_FORMAT_GGUF.to_string(),
            // Imported GGUFs carry no projector (imports are rejected for
            // clip-arch files); vision is a catalog-download feature.
            mmproj: None,
        })
    })();
    let manifest = match post_copy {
        Ok(m) => m,
        Err(e) => {
            // Restore the backed-up model — a failure between copy and spawn
            // must not strand the previous model in the backup dir.
            let _ = std::fs::remove_dir_all(mdir.join(&id));
            if had_existing {
                let _ = std::fs::rename(&backup_dir, mdir.join(&id));
            }
            return Err(e);
        }
    };
    if let Err(e) = save_manifest(&mdir, &manifest) {
        let _ = std::fs::remove_dir_all(mdir.join(&id));
        if had_existing {
            let _ = std::fs::rename(&backup_dir, mdir.join(&id));
        }
        return Err(ErrorResponse::internal(format!("manifest: {e}")));
    }

    // Free the port the previous model's server may still hold, then spawn the
    // IMPORTED model explicitly (preferred_id) — the old model dirs are only
    // removed AFTER the new one is confirmed live.
    let cfg = BuiltinConfig::from_env();
    if super::server::health_check(cfg.port).await {
        kill_process_on_port(cfg.port);
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    // The ctx the server will actually run with (env override wins, RAM
    // guard caps) — registry id if the import names one, else uncapped.
    let spawn_ctx = guarded_ctx(&cfg, cfg.port, import_default_ctx(&dest), model_def(&id));

    let spawned_endpoint = match get_instance_manager() {
        Ok(manager) => match spawn_builtin_server(&state.data_dir, &cfg, &manager, Some(&id)).await
        {
            Ok(endpoint) => {
                tracing::info!(endpoint = %endpoint, model = %id, "builtin llm: server started after local import");
                // Multi-model coexistence: keep other models on disk; the
                // pointer decides which runs. Same-id backup still goes.
                write_active_model(&mdir, &id);
                let _ = std::fs::remove_dir_all(&backup_dir);
                Some(endpoint)
            }
            Err(e) => {
                // A failed spawn rolls the import back: delete the new (bad)
                // model, restore the previous one (same-id backup or the intact
                // other dirs), and RESTART the previous server so the working
                // model is back online — not just its files.
                tracing::warn!(error = %e, model = %id, "builtin llm: import spawn failed — restoring previous model");
                let _ = std::fs::remove_dir_all(mdir.join(&id));
                if had_existing {
                    let _ = std::fs::rename(&backup_dir, mdir.join(&id));
                }
                if let Ok(manager) = get_instance_manager() {
                    if let Err(restart_err) =
                        spawn_builtin_server(&state.data_dir, &cfg, &manager, None).await
                    {
                        tracing::warn!(error = %restart_err, "builtin llm: failed to restart previous model after import rollback");
                    }
                }
                return Err(ErrorResponse::internal(format!(
                    "imported model could not start ({e}) — rolled back; the previous model was restored"
                )));
            }
        },
        Err(e) => {
            // Transient manager failure: the import is VALID — keep the files
            // (a /restart can serve it) and report an error rather than a
            // misleading success.
            tracing::warn!(error = %e, model = %id, "builtin llm: import spawn skipped (instance manager unavailable) — files kept, use /restart");
            return Err(ErrorResponse::internal(format!(
                "instance manager unavailable ({e}) — model files installed but not started; run /restart once the server is fully up"
            )));
        }
    };

    ok(json!({
        "success": true,
        "model_id": id,
        "name": meta.name.unwrap_or_else(|| id.clone()),
        // The ctx the server actually runs with (env override wins).
        "ctx": spawn_ctx,
        "quant": manifest.quant,
        "arch": meta.architecture,
        "installed": true,
        "endpoint": spawned_endpoint,
    }))
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// GET /api/builtin-llm/status
#[utoipa::path(
    get,
    path = "/api/builtin-llm/status",
    tag = "builtin-llm",
    responses(
        (status = 200, description = "Bundled LLM download/runtime status"),
    )
)]
pub async fn status_handler(
    State(state): State<crate::server::types::ServerState>,
) -> HandlerResult<serde_json::Value> {
    let cfg = BuiltinConfig::from_env();

    // Runtime layer (variant/tag/channel) — additive block consumed by the
    // settings card; peek-only, never fetches (cheap for the 3s poll).
    let runtime = super::runtime_manager::runtime_snapshot(&state.data_dir).await;

    // manifest + model_path + installed, independent of download state so the
    // "downloading" branch below can still report `installed` truthfully.
    let mdir = models_dir(&state.data_dir);
    let installed_pair = installed_model(&mdir);
    let model_id = installed_pair.as_ref().map(|(_, m)| m.id.clone());
    let installed = installed_pair.is_some();
    let model_path = installed_pair.as_ref().map(|(_, m)| {
        cfg.model_path
            .clone()
            .unwrap_or_else(|| m.model_path(&mdir))
    });

    // Vision companion: reported in EVERY branch (downloading /
    // not_configured included) — it runs independently of the main model,
    // and the UI's companion row must not blink out mid-download or after a
    // main-model delete. Null when the feature is off or none installed
    // (old and new frontends agree "absent").
    let companion = if !cfg.vlm_enabled {
        serde_json::Value::Null
    } else {
        match installed_companion(&mdir) {
            Some(c) => {
                // Effective ctx from the REGISTERED instance (what the
                // companion actually runs with) when present.
                // Never "—": instance truth first, then the env override,
                // then the model's default — same "effective" contract as
                // the main ctx display.
                let inst_ctx = get_instance_manager()
                    .ok()
                    .and_then(|m| {
                        m.list_instances()
                            .into_iter()
                            .find(|i| is_companion_instance_id(&i.id))
                            .map(|i| i.capabilities.max_context)
                    })
                    .filter(|c| *c > 0)
                    .or(cfg.vlm_ctx)
                    .or(Some(c.default_ctx as usize));
                json!({
                    "model_id": c.manifest.id,
                    "name": c.display_name,
                    "port": cfg.vlm_port,
                    "running": health_check(cfg.vlm_port).await,
                    "ctx": inst_ctx,
                    // Same engine-fact pair the main row shows: effective KV
                    // (dedicated override else shared) and the slot count
                    // (the companion is deliberately single-slot).
                    "kv_cache": if cfg.vlm_kv_quant.unwrap_or(cfg.kv_quant) { "q8_0" } else { "f16" },
                    "parallel": 1,
                    // Last spawn's offload decision for the companion's own
                    // port (null before its first spawn in this process).
                    "gpu_offload": super::vram::last_decision_on(cfg.vlm_port),
                })
            }
            None => serde_json::Value::Null,
        }
    };

    // Downloading overrides running/stopped/not_configured. The lock is the
    // authoritative single-flight gate (per the task design decision); the
    // atomic covers the brief window between lock acquisition and the spawned
    // task actually running.
    let downloading = DL_ACTIVE.load(Ordering::SeqCst) || download_lock().try_lock().is_err();
    if downloading {
        let downloaded = DL_DOWNLOADED.load(Ordering::SeqCst);
        let total = DL_TOTAL.load(Ordering::SeqCst);
        let dl_id = dl_model().lock().unwrap_or_else(|e| e.into_inner()).clone();
        return ok(json!({
            "installed": installed,
            "model_id": dl_id,
            "server_state": "downloading",
            "companion": companion,
            "runtime": runtime,
            "downloaded_bytes": downloaded,
            "total_bytes": if total > 0 { json!(total) } else { serde_json::Value::Null },
        }));
    }

    let Some(_) = installed_pair else {
        return ok(json!({
            "installed": false,
            "model_id": serde_json::Value::Null,
            "server_state": "not_configured",
            "companion": companion,
            "runtime": runtime,
            "downloaded_bytes": serde_json::Value::Null,
            "total_bytes": serde_json::Value::Null,
        }));
    };

    let file_size = model_path
        .and_then(|p| std::fs::metadata(&p).ok())
        .map(|m| m.len())
        .unwrap_or(0);
    let bootstrap_err = super::runtime_manager::bootstrap_error();
    let server_state = if health_check(cfg.port).await {
        "running"
    } else if bootstrap_err.is_some() {
        // Bootstrap (incl. runtime download) failed and the server never
        // came up — "stopped" hid the reason; the frontend's existing
        // error-pill branch renders it once `error` is present.
        "error"
    } else {
        "stopped"
    };

    // Effective ctx: override (env / restart API) else per-model default —
    // surfaced so the UI can show and edit it. Quiet: this endpoint is
    // polled every few seconds by the UI; the cap warn belongs to actual
    // spawn paths.
    let model_def_ctx = installed_pair
        .as_ref()
        .map(|(d, _)| d.default_ctx)
        .unwrap_or(0);
    let effective_ctx = guarded_ctx_quiet(
        &cfg,
        cfg.port,
        model_def_ctx,
        installed_pair.as_ref().map(|(d, _)| d),
    );
    let (total_mb, available_mb) = memory_snapshot_mb();
    let min_ram_mb = installed_pair
        .as_ref()
        .map(|(d, _)| d.min_ram_mb)
        .unwrap_or(0);

    ok(json!({
        "installed": true,
        "model_id": model_id,
        // Multi-model coexistence: everything on disk + which one the
        // pointer selects, so the picker can offer instant switches
        // (restart?model=<id>) instead of a re-download for downloaded ones.
        "installed_models": installed_model_ids(&mdir),
        "active_model": read_active_model(&mdir),
        // Registry display name for the installed model — the frontend used
        // to guess it from a hardcoded id→name map that drifted from the
        // registry (e.g. MiniCPM missing entirely).
        "model_name": installed_pair.as_ref().map(|(d, _)| d.display_name),
        "server_state": server_state,
        "error": if server_state == "error" { bootstrap_err } else { None },
        "runtime": runtime,
        "downloaded_bytes": file_size,
        "total_bytes": file_size,
        "ctx": effective_ctx,
        "ctx_override": cfg.ctx,
        // KV cache quantization the current spawn uses (spawn-time env).
        "kv_cache": if cfg.kv_quant { "q8_0" } else { "f16" },
        // Slot count the current spawn uses; null = flag omitted (upstream
        // default, 4 slots on current builds).
        "parallel": cfg.parallel,
        // Last spawn's GPU offload decision for the main port (ngl, layer
        // count, free VRAM at decision time, and WHY — the 2026-10-27 T4
        // "why is the builtin slow" was answerable only over ssh).
        "gpu_offload": super::vram::last_decision_on(cfg.port),
        "default_ctx": model_def_ctx,
        "memory_ok": available_mb >= min_ram_mb,
        "min_ram_mb": min_ram_mb,
        "available_ram_mb": available_mb,
        "total_ram_mb": total_mb,
        "companion": companion,
    }))
}

// ---------------------------------------------------------------------------
// Models list (registry)
// ---------------------------------------------------------------------------

/// Fill `mmproj` on catalog entries that lack it from the compiled registry
/// entry of the same id. The remote catalog is the DISPLAY list while
/// downloads resolve through the registry first — without this, a stale
/// catalog predating the mmproj field shows no vision badge yet still
/// downloads the projector (the registry wins download resolution).
fn inherit_registry_mmproj(base: &mut [super::catalog::CatalogModel]) {
    for m in base.iter_mut() {
        if m.mmproj.is_none() {
            m.mmproj = model_def(&m.id).and_then(|d| d.mmproj.clone());
        }
    }
}

/// GET /api/builtin-llm/models — the installable builtin models with
/// per-entry install state.
#[utoipa::path(
    get,
    path = "/api/builtin-llm/models",
    tag = "builtin-llm",
    responses(
        (status = 200, description = "Catalog entries for the bundled model"),
    )
)]
pub async fn models_handler(
    State(state): State<crate::server::types::ServerState>,
) -> HandlerResult<serde_json::Value> {
    let mdir = models_dir(&state.data_dir);
    let (_total_mb, available_mb) = memory_snapshot_mb();
    // Source of truth: the REMOTE catalog when reachable (new models ship as
    // a JSON edit), else the compiled-in curated set. A network failure must
    // never degrade the list — offline fallback is the full builtin default.
    let mut catalog_fallback = false;
    let mut base: Vec<super::catalog::CatalogModel> = match super::catalog::fetch_catalog().await {
        Some(catalog) => {
            tracing::debug!(
                count = catalog.len(),
                "builtin llm: using remote model catalog"
            );
            catalog
        }
        None => {
            tracing::debug!("builtin llm: catalog unreachable — using compiled-in curated models");
            catalog_fallback = true;
            BUILTIN_MODELS.iter().map(builtin_to_catalog).collect()
        }
    };
    inherit_registry_mmproj(&mut base);
    let mut models: Vec<serde_json::Value> = base
        .iter()
        .map(|d| {
            let installed = load_manifest(&mdir, &d.id)
                .ok()
                .flatten()
                .map(|m| m.model_path(&mdir).exists())
                .unwrap_or(false);
            json!({
                "id": d.id,
                "name": d.display_name,
                "file_name": d.file_name,
                "quant": d.quant,
                // Model package format + hardware constraints — consumed by
                // the eligibility filter once non-gguf formats ship; the
                // wizard can already display them.
                "format": d.format,
                "target_hardware": d.target_hardware,
                // Minimum llama.cpp runtime tag (wizard badge + download
                // gate); null = no floor.
                "min_runtime": d.min_runtime,
                "size_bytes": d.size_bytes,
                "default_ctx": d.default_ctx,
                // Native ceiling for display; null on older catalogs (the
                // frontend falls back to default_ctx).
                "max_ctx": d.max_ctx,
                "min_ram_mb": d.min_ram_mb,
                // Below the model's floor → the UI discourages the install.
                "memory_ok": available_mb >= d.min_ram_mb as u64,
                "notes": d.notes,
                "recommended": d.recommended,
                "installed": installed,
                // Vision projector ships with the model — the wizard badge
                // and the picker derive it from this, not from notes text.
                "vision": d.mmproj.is_some(),
                // "vlm" = vision companion entry (wizard groups it apart;
                // spawn registers it as builtin-vlm-*, never chat-active).
                "role": d.role.clone(),
                "custom": false,
            })
        })
        .collect();

    // Open catalog local channel: every imported GGUF whose id is not
    // represented by the catalog/registry above is listed so the picker shows
    // it alongside the curated set.
    let builtin_ids: std::collections::HashSet<&str> = BUILTIN_MODELS
        .iter()
        .map(|d| d.manifest.id.as_str())
        .collect();
    // Ids already represented by the catalog-derived list. Without this a
    // remote-catalog model (LFM3) is listed TWICE: once as its catalog card
    // and once as a "本地导入" card (2026-10-30).
    let listed_ids: std::collections::HashSet<String> = models
        .iter()
        .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(str::to_string))
        .collect();
    if let Ok(entries) = std::fs::read_dir(&mdir) {
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let Some(id) = dir.file_name().and_then(|s| s.to_str()).map(str::to_string) else {
                continue;
            };
            // Dot-dirs are import backups / internal state, not models.
            if id.starts_with('.') {
                continue;
            }
            if builtin_ids.contains(id.as_str()) || listed_ids.contains(&id) {
                continue;
            }
            let Ok(Some(m)) = load_manifest(&mdir, &id) else {
                continue;
            };
            if !m.model_path(&mdir).exists() {
                continue;
            }
            let ctx = super::gguf::parse_gguf(&m.model_path(&mdir))
                .ok()
                .and_then(|g| g.context_length)
                .and_then(|c| u32::try_from(c).ok())
                .unwrap_or(32768);
            let size = std::fs::metadata(m.model_path(&mdir))
                .map(|x| x.len())
                .unwrap_or(0);
            models.push(json!({
                "id": m.id,
                "name": m.id,
                "file_name": m.file_name,
                "quant": m.quant,
                "size_bytes": size,
                "default_ctx": ctx,
                "min_ram_mb": 0,
                "memory_ok": true,
                "notes": "本地导入的模型",
                "recommended": false,
                "installed": true,
                "vision": false,
                "custom": true,
            }));
        }
    }
    // `catalog_fallback`: the remote catalog was unreachable and this list is
    // the compiled-in curated set — the wizard shows an "offline catalog"
    // badge instead of silently pretending this is the live catalog.
    ok(json!({
        "models": models,
        "default_model_id": BUILTIN_MODEL_ID,
        "catalog_fallback": catalog_fallback,
    }))
}

// ---------------------------------------------------------------------------
// Download
// ---------------------------------------------------------------------------

/// POST /api/builtin-llm/download — body `{ "model_id"?: string }`, default
/// model when omitted.
/// POST /api/builtin-llm/download/cancel — stop the in-flight model
/// download. The partial file is kept, so re-downloading the same model
/// resumes; the lock is released once the task winds down, and a different
/// model can be downloaded immediately after.
#[utoipa::path(
    post,
    path = "/api/builtin-llm/download/cancel",
    tag = "builtin-llm",
    responses(
        (status = 200, description = "Download cancelled"),
    )
)]
pub async fn download_cancel_handler(
    State(_state): State<crate::server::types::ServerState>,
) -> HandlerResult<serde_json::Value> {
    if !DL_ACTIVE.load(Ordering::SeqCst) {
        return ok(json!({ "cancelled": false, "active": false }));
    }
    super::download::request_download_cancel();
    ok(json!({ "cancelled": true }))
}

#[utoipa::path(
    post,
    path = "/api/builtin-llm/download",
    tag = "builtin-llm",
    request_body = serde_json::Value,
    responses(
        (status = 200, description = "Model download started (optional selection body)"),
    )
)]
pub async fn download_handler(
    State(state): State<crate::server::types::ServerState>,
    payload: Option<axum::Json<serde_json::Value>>,
) -> HandlerResult<serde_json::Value> {
    let cfg = BuiltinConfig::from_env();
    if !cfg.enabled {
        return Err(ErrorResponse::bad_request(
            "builtin LLM disabled (NEOMIND_BUILTIN_LLM=off)",
        ));
    }
    // Mutual exclusion with the runtime upgrade (the other direction is in
    // upgrade_runtime): a model download's post-download spawn could race
    // the upgrade's stop/respawn otherwise.
    if super::runtime_manager::upgrade_active() {
        return Err(ErrorResponse::conflict(
            "a llama.cpp runtime upgrade is in flight — retry after it finishes",
        ));
    }

    let lock = download_lock();
    // OwnedMutexGuard: owns a clone of the Arc, so the guard can be moved into
    // the 'static background task (a borrowed guard would not live that long).
    let Ok(guard) = lock.clone().try_lock_owned() else {
        return ok(json!({ "started": false, "already_running": true }));
    };

    let model_id = payload.and_then(|json| {
        json.0
            .get("model_id")
            .and_then(|v| v.as_str())
            .map(str::to_string)
    });
    // Ensure the remote catalog is fetched before resolving — a cold process
    // restarting and downloading a catalog-only model would otherwise fail
    // (resolve_model consults the process-local cache only).
    let _ = super::catalog::fetch_catalog().await;
    let def = resolve_model(model_id.as_deref())?;
    // Runtime floor (e.g. LFM3 needs >= b11960): refuse with guidance while
    // the active runtime is older — the error points at the upgrade dialog.
    if let Some(min_runtime) = def
        .min_runtime
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let active = super::runtime_manager::active_runtime_tag(&state.data_dir).await;
        super::runtime_manager::runtime_gate(active.as_deref(), min_runtime)
            .map_err(ErrorResponse::conflict)?;
    }
    let (url, sha, file_name) = resolve_source(&cfg, &def);
    // Vision projector source (quant-override independent — see
    // resolve_mmproj_source). None = text-only model.
    let mmproj_src = resolve_mmproj_source(&def);
    let quant_label = def.quant.clone();
    // Role split: a vlm entry becomes the vision companion (own port, own
    // instance, never the active chat model) — every spawn/switch action
    // below must route accordingly.
    let is_vlm = def.role.as_deref() == Some("vlm");
    let mdir = models_dir(&state.data_dir);
    let dest = mdir.join(&def.id).join(&file_name);

    // Multi-model coexistence: model files are never removed on switch, so
    // "download" of an on-disk model is really a SWITCH — pointer + respawn,
    // zero bytes re-downloaded, never the old fake "downloading 0%"
    // ceremony. Gate on the REQUESTED model's own manifest AND file (a
    // tampered/missing file still takes the self-healing download path).
    let on_disk = load_manifest(&mdir, &def.id)
        .ok()
        .flatten()
        .is_some_and(|m| m.model_path(&mdir).exists());
    if on_disk {
        // Companion on-disk = "make sure it runs", never a main-model
        // switch: the active-model pointer and the main port are untouched.
        if is_vlm {
            if let Ok(manager) = get_instance_manager() {
                match ensure_companion(&state.data_dir, &cfg, &manager).await {
                    Ok(ep) => {
                        tracing::info!(endpoint = %ep, "builtin llm: vision companion running after install request")
                    }
                    Err(skip) => {
                        tracing::warn!(reason = %skip, "builtin llm: vision companion not running")
                    }
                }
            }
            return ok(json!({
                "started": false,
                "already_running": false,
                "already_installed": true,
                "switched": false,
                "companion": true
            }));
        }
        let switching = read_active_model(&mdir).as_deref() != Some(def.id.as_str());
        if switching {
            write_active_model(&mdir, &def.id);
            if health_check(cfg.port).await {
                kill_process_on_port(cfg.port);
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
            if let Ok(manager) = get_instance_manager() {
                if spawn_builtin_server(&state.data_dir, &cfg, &manager, Some(&def.id))
                    .await
                    .is_ok()
                {
                    // The builtin may be the ACTIVE chat backend — its
                    // session-manager snapshot still carries the OLD model
                    // (prompt budget + chat dropdown name). Re-push, same as
                    // the restart handler does after a respawn.
                    let builtin_active = manager
                        .get_active_instance()
                        .map(|i| i.id == BUILTIN_INSTANCE_ID)
                        .unwrap_or(false);
                    if builtin_active {
                        if let Some(instance) = manager.get_instance(BUILTIN_INSTANCE_ID) {
                            let backend = LlmBackend::LlamaCpp {
                                endpoint: instance
                                    .endpoint
                                    .clone()
                                    .unwrap_or_else(|| format!("http://127.0.0.1:{}", cfg.port)),
                                model: instance.model.clone(),
                                capabilities: Some(convert_capabilities(&instance.capabilities)),
                            };
                            if let Err(e) =
                                state.agents.session_manager.set_llm_backend(backend).await
                            {
                                tracing::warn!(
                                    error = %e,
                                    "builtin llm: failed to refresh session backend after switch"
                                );
                            }
                        }
                    }
                }
            }
            tracing::info!(model = %def.id, "builtin llm: switched active model (files kept)");
        }
        return ok(json!({
            "started": false,
            "already_running": false,
            "already_installed": true,
            "switched": switching
        }));
    }

    tracing::info!(
        model = %def.id,
        url = %url,
        dest = %dest.display(),
        "builtin llm: starting model download"
    );

    // A stale cancel from a previous run must not kill this fresh download.
    super::download::clear_download_cancel();
    DL_ACTIVE.store(true, Ordering::SeqCst);
    DL_DOWNLOADED.store(0, Ordering::SeqCst);
    DL_TOTAL.store(0, Ordering::SeqCst);
    *dl_model().lock().unwrap_or_else(|e| e.into_inner()) = def.id.clone();

    let state_for_task = state.clone();
    let cfg_for_task = cfg.clone();
    let model_id_task = def.id.clone();
    let mmproj_src_task = mmproj_src.clone();
    let quant_task = quant_label.clone();
    tokio::spawn(async move {
        // `guard` single-flights concurrent POSTs; `_active` flips DL_ACTIVE
        // back off on finish/panic (or earlier — run_builtin_download drops it
        // right after the model lands, so the post-download spawn/runtime
        // download doesn't keep the UI stuck at "downloading 100%").
        let _active = DownloadActiveGuard;
        let _guard = guard;
        match run_builtin_download(
            &state_for_task,
            &cfg_for_task,
            DownloadJob {
                model_id: model_id_task,
                dest,
                url,
                sha,
                quant: quant_task,
                format: def.format.clone(),
                mmproj_src: mmproj_src_task,
                companion: is_vlm,
            },
            _active,
        )
        .await
        {
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "builtin llm: download failed"),
        }
    });

    ok(json!({ "started": true, "already_running": false }))
}

/// Pass-through payload for [`run_builtin_download`] — grouped so the
/// signature stays clippy-sized while the call site reads as one bundle.
struct DownloadJob {
    model_id: String,
    dest: PathBuf,
    url: String,
    sha: String,
    quant: String,
    /// Model package format (catalog entry; written into the manifest).
    format: String,
    mmproj_src: Option<(String, String, String)>,
    companion: bool,
}

async fn run_builtin_download(
    state: &crate::server::types::ServerState,
    cfg: &BuiltinConfig,
    job: DownloadJob,
    _active: DownloadActiveGuard,
) -> anyhow::Result<()> {
    let DownloadJob {
        model_id,
        dest,
        url,
        sha,
        quant,
        format: model_format,
        mmproj_src,
        companion,
    } = job;
    let model_id = model_id.as_str();
    let dest = dest.as_path();
    let url = url.as_str();
    let sha = sha.as_str();
    let quant = quant.as_str();
    let client = reqwest::Client::new();
    let bus = state.event_bus();

    // Clone for the callback — the closure is `move`, so without a clone the
    // outer `bus` would be moved in and unavailable for the complete/error
    // events below.
    let bus_for_cb = bus.clone();
    // The downloader calls on_progress per network chunk (8-64KB) — that can
    // be 30-250 events/sec. Atomics update cheaply every chunk, but the WS
    // publish is throttled to ~250ms so the frontend doesn't re-render storm
    // (that manifested as UI flicker during downloads).
    //
    // Phase offset: vision models download two files (GGUF then mmproj)
    // under ONE progress bar. Phase 2 adds the main file's final size to
    // both counters so the bar keeps climbing instead of restarting at 0
    // (a resetting bar reads as "download broke and began again").
    let phase_offset = Arc::new(AtomicU64::new(0));
    let last_publish_ms = Arc::new(AtomicU64::new(0));
    let on_progress = {
        let bus_for_cb = bus_for_cb.clone();
        let phase_offset = phase_offset.clone();
        move |p: DownloadProgress| {
            let offset = phase_offset.load(Ordering::SeqCst);
            let downloaded = p.downloaded + offset;
            let total = p.total.map(|t| t + offset);
            DL_DOWNLOADED.store(downloaded, Ordering::SeqCst);
            DL_TOTAL.store(total.unwrap_or(0), Ordering::SeqCst);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let last = last_publish_ms.load(Ordering::Relaxed);
            if now.saturating_sub(last) < 250 && last != 0 {
                return;
            }
            last_publish_ms.store(now, Ordering::Relaxed);
            if let Some(bus) = &bus_for_cb {
                bus.publish_sync(NeoMindEvent::ModelDownloadProgress {
                    model_id: model_id.to_string(),
                    downloaded,
                    total,
                    status: "downloading".to_string(),
                    error: None,
                });
            }
        }
    };

    let result = download_with_resume(&client, url, dest, sha, Some(&on_progress)).await;

    match result {
        Ok(_) => {
            let mdir = models_dir(&state.data_dir);

            // Vision projector phase — AFTER the main GGUF so a projector
            // failure never strands a manifest-less model dir. Failure is a
            // degrade, not an abort: the text model is installed and usable,
            // the manifest just records no mmproj (spawn stays text-only,
            // the wizard shows no vision badge).
            let mut mmproj_installed: Option<MmprojFile> = None;
            if let Some((mp_url, mp_sha, mp_file)) = mmproj_src.as_ref() {
                let mp_dest = mdir.join(model_id).join(mp_file);
                // metadata (not the downloader's return) is the truth even
                // when the file already existed (resume-complete returns 0).
                phase_offset.store(
                    std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0),
                    Ordering::SeqCst,
                );
                match download_with_resume(&client, mp_url, &mp_dest, mp_sha, Some(&on_progress))
                    .await
                {
                    Ok(_) => {
                        mmproj_installed = Some(MmprojFile {
                            file_name: mp_file.clone(),
                            sha256: mp_sha.clone(),
                        });
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            model = model_id,
                            "builtin llm: mmproj download failed — installing text-only"
                        );
                        phase_offset.store(0, Ordering::SeqCst);
                    }
                }
            }

            // Persist the manifest so bootstrap/status know the model exists.
            let manifest = ModelManifest {
                id: model_id.to_string(),
                version: "1.0".to_string(),
                file_name: dest
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| model_id.to_string()),
                sha256: sha.to_string(),
                quant: quant.to_string(),
                format: model_format.clone(),
                mmproj: mmproj_installed,
            };
            save_manifest(&mdir, &manifest)?;

            // Multi-model coexistence: downloaded files are NEVER removed on
            // switch — the active-model pointer is the sole authority for
            // which one runs (the old purge made every switch cost a full
            // 1.5 GB re-download; observed 2026-10-15 lfm↔minicpm).
            // The pointer is a MAIN-model concept — a companion download
            // must never move it.
            if !companion {
                write_active_model(&mdir, model_id);
            }

            // If a server is still running the PREVIOUS model (its files were
            // just removed above), it owns the port — a fresh spawn would die
            // on bind while health-checks pass against the stale server
            // (port misattribution). Free the port first. Main-model path
            // only — a companion never touches the main port.
            if !companion && health_check(cfg.port).await {
                tracing::info!(
                    port = cfg.port,
                    "builtin llm: stopping previous-model server before spawn"
                );
                kill_process_on_port(cfg.port);
                tokio::time::sleep(Duration::from_millis(300)).await;
            }

            // Model is on disk — release DL_ACTIVE so the status endpoint stops
            // reporting "downloading" (the wizard flips to installed → ready /
            // auto-activate). The spawn below (which may download the llama-
            // server runtime on first use) then runs without keeping the UI
            // frozen at 100%.
            drop(_active);
            super::runtime_manager::record_download_error(None);

            if let Some(bus) = &bus {
                bus.publish_sync(NeoMindEvent::ModelDownloadProgress {
                    model_id: model_id.to_string(),
                    downloaded: DL_DOWNLOADED.load(Ordering::SeqCst),
                    total: Some(DL_TOTAL.load(Ordering::SeqCst)),
                    status: "complete".to_string(),
                    error: None,
                });
            }

            // Bring the builtin instance + active policy up (spawn server,
            // upsert instance with is_builtin/thinking_is_integral, set_active
            // only when nothing else is active). Failures here are logged, not
            // fatal — the model is downloaded and restart/status can recover.
            // A companion download brings up the COMPANION instead (own port,
            // never active).
            if let Ok(manager) = get_instance_manager() {
                if companion {
                    match ensure_companion(&state.data_dir, cfg, &manager).await {
                        Ok(endpoint) => {
                            tracing::info!(endpoint = %endpoint, "builtin llm: vision companion started after download")
                        }
                        Err(skip) => {
                            tracing::warn!(reason = %skip, "builtin llm: vision companion not started after download")
                        }
                    }
                } else {
                    match spawn_builtin_server(&state.data_dir, cfg, &manager, None).await {
                        Ok(endpoint) => {
                            tracing::info!(endpoint = %endpoint, "builtin llm: server started after download")
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "builtin llm: server spawn after download failed (run /restart)")
                        }
                    }
                }
            }
            Ok(())
        }
        Err(e) => {
            // Remember the failure text (status `sources.last_download_error`)
            // unless the user cancelled — a cancel is not a network problem.
            if !super::download::download_cancel_requested() {
                super::runtime_manager::record_download_error(Some(e.to_string()));
            }
            if let Some(bus) = &bus {
                let cancelled = super::download::download_cancel_requested();
                bus.publish_sync(NeoMindEvent::ModelDownloadProgress {
                    model_id: model_id.to_string(),
                    downloaded: DL_DOWNLOADED.load(Ordering::SeqCst),
                    total: if DL_TOTAL.load(Ordering::SeqCst) > 0 {
                        Some(DL_TOTAL.load(Ordering::SeqCst))
                    } else {
                        None
                    },
                    status: if cancelled {
                        "cancelled".to_string()
                    } else {
                        "error".to_string()
                    },
                    error: Some(if cancelled {
                        "cancelled by user".to_string()
                    } else {
                        e.to_string()
                    }),
                });
            }
            Err(e)
        }
    }
}

// ---------------------------------------------------------------------------
// Server spawn + instance registration (shared by download + restart)
// ---------------------------------------------------------------------------

/// Find binary + model, spawn, wait healthy, upsert the builtin instance
/// (`is_builtin`, `thinking_is_integral`, endpoint `http://127.0.0.1:<port>`),
/// and set it active ONLY when no backend is already active ("有后端不抢").
///
/// Mirrors `state.rs::bootstrap()` but bypasses the "instance already exists →
/// ServerAlreadyRunning" short-circuit, so this is the right entry point for
/// restart / post-download bring-up when the server may be down despite an
/// instance record existing.
async fn spawn_builtin_server(
    data_dir: &Path,
    cfg: &BuiltinConfig,
    manager: &LlmBackendInstanceManager,
    preferred_id: Option<&str>,
) -> anyhow::Result<String> {
    // Model check BEFORE the runtime fetch — same ordering contract as
    // bootstrap: the llama-server download belongs to the user's decision to
    // install a builtin model, never to a restart hitting a model-less host.
    let mdir = models_dir(data_dir);
    // `preferred_id` lets the import spawn the NEW model explicitly instead of
    // relying on installed_model_any's preference scan (which favors a present
    // builtin / newest custom) — so we can remove the old model only AFTER the
    // new one is confirmed live.
    let installed = match preferred_id {
        Some(id) => installed_model_by_id(&mdir, id)
            .ok_or_else(|| anyhow::anyhow!("model {id} not installed (no manifest)"))?,
        None => installed_model_any(&mdir)
            .ok_or_else(|| anyhow::anyhow!("model not installed (no manifest)"))?,
    };
    let binary = ensure_llama_server(data_dir)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    let model_path = cfg
        .model_path
        .clone()
        .unwrap_or_else(|| installed.manifest.model_path(&mdir));
    if !model_path.exists() {
        anyhow::bail!("model file missing at {}", model_path.display());
    }

    // Context: per-model default (128K on RAM-rich hosts) with the RAM
    // guard capping what the device can fund; an explicit override
    // (NEOMIND_BUILTIN_LLM_CTX / restart ?ctx=) picks the starting point
    // but the guard still applies.
    let def = model_def(&installed.manifest.id);
    let ctx = guarded_ctx(cfg, cfg.port, installed.default_ctx, def);
    // Vision projector: the manifest must record it AND the file must be
    // present — a deleted/failed projector degrades to a text-only spawn,
    // never a failed model. An explicit NEOMIND_BUILTIN_MODEL_PATH override
    // opts out entirely (the operator owns their file layout).
    let mmproj = if cfg.model_path.is_some() {
        None
    } else {
        match installed.manifest.mmproj_path(&mdir) {
            Some(p) if p.exists() => Some(p),
            Some(p) => {
                tracing::warn!(
                    path = %p.display(),
                    "builtin llm: mmproj recorded but file missing — spawning text-only"
                );
                None
            }
            None => None,
        }
    };
    // Captured before the config consumes the path (the capability stamp
    // below needs "did we attach a projector", not the path itself).
    let mmproj_attached = mmproj.is_some();
    // GPU offload sizing — same contract as bootstrap: explicit pin wins,
    // auto-size to free VRAM on discrete NVIDIA GPUs, None elsewhere.
    let ngl = super::vram::resolve_spawn_ngl(&super::vram::NglQuery {
        explicit: cfg.ngl,
        binary: &binary,
        model_path: &model_path,
        mmproj: mmproj.as_deref(),
        ctx,
        port: cfg.port,
        kv_bytes_per_token_f16: def.and_then(|d| d.kv_bytes_per_token),
        kv_const_bytes: def.and_then(|d| d.kv_const_bytes),
        kv_quant: cfg.kv_quant,
    });
    let server_cfg = LlamaServerConfig {
        binary,
        model: model_path,
        port: cfg.port,
        ctx,
        ngl,
        threads: None,
        temperature: installed.temperature,
        top_p: installed.top_p,
        top_k: installed.top_k,
        min_p: installed.min_p,
        repeat_penalty: installed.repeat_penalty,
        kv_quant: cfg.kv_quant,
        parallel: cfg.parallel,
        mmproj,
    };
    let mut proc = LlamaServerProcess::spawn(&server_cfg)?;
    if let Err(e) = proc.wait_healthy(Duration::from_secs(60)).await {
        let _ = proc.stop().await;
        anyhow::bail!("server unhealthy: {}", e);
    }
    // Same port-misattribution guard as state.rs::bootstrap: wait_healthy can
    // succeed against a foreign server on our port while our child died on
    // bind. Verify the spawned child is alive before registering it.
    if !proc.is_alive() {
        let _ = proc.stop().await;
        anyhow::bail!(
            "port {} in use — llama-server exited after bind (another server on that port?)",
            cfg.port
        );
    }
    let endpoint = format!("http://127.0.0.1:{}", cfg.port);

    let mut instance = match manager.get_instance(BUILTIN_INSTANCE_ID) {
        Some(mut i) => {
            i.endpoint = Some(endpoint.clone());
            i
        }
        None => LlmBackendInstance::new(
            BUILTIN_INSTANCE_ID.to_string(),
            installed.display_name.clone(),
            LlmBackendType::LlamaCpp,
        ),
    };
    instance.is_builtin = true;
    // The builtin instance id is STABLE across model switches, so the name
    // pinned at first creation kept showing the OLD model in the chat picker
    // (`name` renders before `model` there) — restamp it per spawn.
    instance.name = installed.display_name.clone();
    // LFM2.5's thinking is integral (cannot be turned off); this is a MODEL
    // property, not "is the default" — the comparison against
    // BUILTIN_MODEL_ID silently flipped when the default moved to MiniCPM,
    // mis-flagging LFM installs on the restart path (bootstrap hardcodes
    // the id correctly).
    instance.thinking_is_integral = installed.thinking_is_integral;
    instance.thinking_enabled = installed.default_thinking;
    instance.endpoint = Some(endpoint.clone());
    instance.model = installed.manifest.id.clone();
    // Capabilities must be set here — the startup capability-refresh loop ran
    // before this instance existed, so a default (max_context=4096) would
    // surface as a tiny chat context window. The ctx we spawned with is the
    // TOTAL KV pool; the server is now healthy, so report the per-slot
    // share (auto slot count divides it at runtime).
    instance.capabilities.supports_streaming = true;
    instance.capabilities.supports_tools = true;
    instance.capabilities.supports_thinking = installed.default_thinking;
    instance.capabilities.max_context = effective_spawn_ctx(cfg.port, ctx).await;
    // Vision capability, stamped AT SPAWN so the chat picker / image-upload
    // gate reflects it immediately — the alternative (build_runtime's /props
    // probe) only lands on the first chat round. /props is the authority: a
    // projector that failed to mount leaves modalities.vision off while our
    // `mmproj` variable still says attached; manifest presence is only the
    // fallback for runtimes that don't report modalities at all.
    //
    // `multimodal_source = "runtime_api"` matters as much as the value:
    // get_instance() runs ensure_instance_capabilities, which re-derives
    // supports_multimodal from the MODEL NAME (registry/heuristic tables of
    // cloud model names — "gemma4-e2b" isn't in them) and would clobber an
    // observed value back to false on every read. runtime_api-sourced
    // values are exempt; user overrides still win over both.
    let multimodal = query_server_modalities_vision(cfg.port)
        .await
        .unwrap_or(mmproj_attached);
    instance.capabilities.supports_multimodal = multimodal;
    instance.capabilities.multimodal_source = Some("runtime_api".to_string());
    // Per-model sampling: applies both as server-side defaults (above) and
    // on the request side (the instance fields are what chat sends).
    if let Some(t) = installed.temperature {
        instance.temperature = t;
    }
    if let Some(p) = installed.top_p {
        instance.top_p = p;
    }
    if let Some(k) = installed.top_k {
        instance.top_k = k as usize;
    }
    let _ = manager.upsert_instance(instance).await;

    // 活跃策略:仅当没有任何活跃后端时设为活跃(「有后端不抢」)。
    if manager.get_active_instance().is_none() {
        let _ = manager.set_active(BUILTIN_INSTANCE_ID).await;
    }

    Ok(endpoint)
}

/// File size in MB (0 when unreadable) — the companion RAM gate prefers the
/// bytes actually on disk over catalog numbers (imports, quant overrides).
fn metadata_len_mb(p: &Path) -> u64 {
    std::fs::metadata(p)
        .map(|m| m.len() / (1024 * 1024))
        .unwrap_or(0)
}

/// Spawn the installed vision companion on its own port and register the
/// `builtin-vlm-<id>` instance. Mirrors `spawn_builtin_server` with three
/// deliberate differences: its own port (`cfg.vlm_port`), its own instance
/// id, and it NEVER calls set_active — the companion serves the vision tool
/// and chat image-delegation, not chat itself.
async fn spawn_companion_server(
    data_dir: &Path,
    cfg: &BuiltinConfig,
    manager: &LlmBackendInstanceManager,
) -> Result<String, CompanionSkip> {
    if !cfg.vlm_enabled {
        return Err(CompanionSkip::Disabled);
    }
    let mdir = models_dir(data_dir);
    let installed = installed_companion(&mdir).ok_or(CompanionSkip::NotInstalled)?;

    // Marginal RAM gate: the main model already holds its (guarded) share —
    // this budget is the SECOND server's weights + projector + headroom
    // against what is left. Skipping is a status-visible degrade, never an
    // OOM gamble.
    const COMPANION_HEADROOM_MB: u64 = 768; // KV (small window on a 1B VLM) + compute buffers
    let weights_mb = metadata_len_mb(&installed.manifest.model_path(&mdir));
    let mmproj_mb = installed
        .manifest
        .mmproj_path(&mdir)
        .map(|p| metadata_len_mb(&p))
        .unwrap_or(0);
    let need_mb = weights_mb + mmproj_mb + COMPANION_HEADROOM_MB;
    let (_total_mb, available_mb) = memory_snapshot_mb();
    if !companion_ram_ok(available_mb, need_mb) {
        tracing::warn!(
            model = %installed.manifest.id,
            available_mb, need_mb,
            "builtin llm: vision companion skipped — not enough RAM for a second server"
        );
        return Err(CompanionSkip::LowRam {
            available_mb,
            need_mb,
        });
    }

    // A healthy listener already on the port: ours and registered →
    // idempotent success; anything else → skip rather than fight for the
    // bind (the main spawn's port-misattribution lesson applies).
    let endpoint = format!("http://127.0.0.1:{}", cfg.vlm_port);
    if health_check(cfg.vlm_port).await {
        let ours = manager.list_instances().iter().any(|i| {
            is_companion_instance_id(&i.id) && i.endpoint.as_deref() == Some(endpoint.as_str())
        });
        if ours {
            return Ok(endpoint);
        }
        return Err(CompanionSkip::PortBusy(cfg.vlm_port));
    }

    let binary = match ensure_llama_server(data_dir).await {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(error = %e, "builtin llm: vision companion spawn failed (no llama-server)");
            return Err(CompanionSkip::Other(format!(
                "llama-server unavailable: {e}"
            )));
        }
    };
    let model_path = installed.manifest.model_path(&mdir);
    if !model_path.exists() {
        return Err(CompanionSkip::NotInstalled);
    }
    // Dedicated companion overrides DECOUPLE the engine knobs: without
    // them the main-model NEOMIND_BUILTIN_LLM_CTX/KV/NGL silently moved the
    // companion too (raising the main to 128K also raised the companion's
    // KV bill for no describe-workload benefit).
    let mut ccfg = cfg.clone();
    if let Some(v) = cfg.vlm_ctx {
        ccfg.ctx = Some(v);
    }
    if let Some(kv) = cfg.vlm_kv_quant {
        ccfg.kv_quant = kv;
    }
    if let Some(n) = cfg.vlm_ngl {
        ccfg.ngl = Some(n);
    }
    let ctx = guarded_ctx(
        &ccfg,
        cfg.vlm_port,
        installed.default_ctx,
        model_def(&installed.manifest.id),
    );
    // Same projector resolution rules as the main spawn.
    let mmproj = match installed.manifest.mmproj_path(&mdir) {
        Some(p) if p.exists() => Some(p),
        Some(p) => {
            tracing::warn!(
                path = %p.display(),
                "builtin llm: companion mmproj recorded but file missing — spawning text-only"
            );
            None
        }
        None => None,
    };
    let mmproj_attached = mmproj.is_some();
    // GPU offload sizing with the COMPANION's own scope: its port (reclaim
    // must not count the still-running main server's VRAM as freeing) and
    // the merged cfg (dedicated VLM_NGL/KV overrides applied above).
    let ngl = super::vram::resolve_spawn_ngl(&super::vram::NglQuery {
        explicit: ccfg.ngl,
        binary: &binary,
        model_path: &model_path,
        mmproj: mmproj.as_deref(),
        ctx,
        port: cfg.vlm_port,
        kv_bytes_per_token_f16: model_def(&installed.manifest.id)
            .and_then(|d| d.kv_bytes_per_token),
        kv_const_bytes: model_def(&installed.manifest.id).and_then(|d| d.kv_const_bytes),
        kv_quant: ccfg.kv_quant,
    });
    let server_cfg = LlamaServerConfig {
        binary,
        model: model_path,
        port: cfg.vlm_port,
        ctx,
        ngl,
        threads: None,
        temperature: installed.temperature,
        top_p: installed.top_p,
        top_k: installed.top_k,
        min_p: installed.min_p,
        repeat_penalty: installed.repeat_penalty,
        kv_quant: ccfg.kv_quant,
        parallel: Some(1),
        mmproj,
    };
    let mut proc = match LlamaServerProcess::spawn(&server_cfg) {
        Ok(p) => p,
        Err(e) => return Err(CompanionSkip::Other(format!("companion spawn failed: {e}"))),
    };
    if let Err(e) = proc.wait_healthy(Duration::from_secs(60)).await {
        let _ = proc.stop().await;
        return Err(CompanionSkip::Other(format!("companion unhealthy: {e}")));
    }
    // Same port-misattribution guard as the main spawn.
    if !proc.is_alive() {
        let _ = proc.stop().await;
        return Err(CompanionSkip::PortBusy(cfg.vlm_port));
    }

    // Register — the companion is describe-only: no tool calls, and it is
    // NEVER set active (chat backends are main models; the vision tool and
    // image delegation find it through supports_multimodal).
    let inst_id = companion_instance_id(&installed.manifest.id);
    let mut instance = match manager.get_instance(&inst_id) {
        Some(mut i) => {
            i.endpoint = Some(endpoint.clone());
            i
        }
        None => LlmBackendInstance::new(
            inst_id.clone(),
            installed.display_name.clone(),
            LlmBackendType::LlamaCpp,
        ),
    };
    instance.is_builtin = true;
    instance.name = installed.display_name.clone();
    instance.thinking_is_integral = installed.thinking_is_integral;
    instance.thinking_enabled = installed.default_thinking;
    instance.endpoint = Some(endpoint.clone());
    instance.model = installed.manifest.id.clone();
    instance.capabilities.supports_streaming = true;
    instance.capabilities.supports_tools = false;
    instance.capabilities.supports_thinking = installed.default_thinking;
    instance.capabilities.max_context = effective_spawn_ctx(cfg.vlm_port, ctx).await;
    let multimodal = query_server_modalities_vision(cfg.vlm_port)
        .await
        .unwrap_or(mmproj_attached);
    instance.capabilities.supports_multimodal = multimodal;
    instance.capabilities.multimodal_source = Some("runtime_api".to_string());
    let _ = manager.upsert_instance(instance).await;

    tracing::info!(
        model = %installed.manifest.id,
        port = cfg.vlm_port,
        "builtin llm: vision companion ready"
    );
    Ok(endpoint)
}

/// Idempotent companion bring-up (bootstrap + restart): heal orphan/stale
/// instances, spawn when down. Every failure is a logged skip — the MAIN
/// model's outcome is never affected.
pub(crate) async fn ensure_companion(
    data_dir: &Path,
    cfg: &BuiltinConfig,
    manager: &LlmBackendInstanceManager,
) -> Result<String, CompanionSkip> {
    if !cfg.vlm_enabled {
        return Err(CompanionSkip::Disabled);
    }
    let mdir = models_dir(data_dir);

    // The companion that WOULD run now (first installed vlm entry).
    let chosen = installed_companion(&mdir).map(|m| m.manifest.id.clone());

    // Orphan/stale heal: companion instances whose model is gone — or that
    // name a DIFFERENT vlm than the chosen one (single-companion semantics;
    // a switched-away model would otherwise linger as a dead entry in the
    // include=vlm listing).
    for inst in manager.list_instances() {
        if !is_companion_instance_id(&inst.id) {
            continue;
        }
        let stale = match (&chosen, &inst.model) {
            (Some(c), m) => m != c,
            (None, _) => true,
        } || installed_model_by_id(&mdir, &inst.model)
            .map(|m| !is_vlm_model(&m.manifest.id))
            .unwrap_or(true);
        if stale {
            tracing::info!(instance = %inst.id, "builtin llm: removing stale companion instance");
            let _ = manager.force_remove_instance(&inst.id).await;
        }
    }

    // Idempotent fast path: healthy port + our instance registered.
    if health_check(cfg.vlm_port).await {
        let endpoint = format!("http://127.0.0.1:{}", cfg.vlm_port);
        let ours = manager.list_instances().iter().any(|i| {
            is_companion_instance_id(&i.id) && i.endpoint.as_deref() == Some(endpoint.as_str())
        });
        if ours {
            return Ok(endpoint);
        }
    }

    spawn_companion_server(data_dir, cfg, manager).await
}

/// Best-effort kill of whatever process listens on `port` (unix only).
///
/// Used to free a wedged llama-server port before a fresh spawn. Relies on
/// `lsof`, which is present on macOS and most Linux distros; if unavailable
/// the subsequent spawn simply reports "address in use".
#[cfg(unix)]
pub(crate) fn kill_process_on_port(port: u16) {
    use std::process::Command;
    if let Ok(out) = Command::new("lsof")
        .arg("-ti")
        .arg(format!("tcp:{}", port))
        .arg("-sTCP:LISTEN")
        .output()
    {
        if out.status.success() {
            if let Ok(pids) = String::from_utf8(out.stdout) {
                for pid in pids.split_whitespace() {
                    tracing::info!(pid, port, "builtin llm: killing process on port");
                    let _ = Command::new("kill").arg(pid).status();
                }
            }
        }
    }
}

#[cfg(not(unix))]
pub(crate) fn kill_process_on_port(_port: u16) {}

// ---------------------------------------------------------------------------
// Delete
// ---------------------------------------------------------------------------

/// DELETE /api/builtin-llm/model
#[utoipa::path(
    delete,
    path = "/api/builtin-llm/model",
    tag = "builtin-llm",
    responses(
        (status = 200, description = "Bundled model files deleted"),
    )
)]
pub async fn delete_model_handler(
    State(state): State<crate::server::types::ServerState>,
    Query(params): Query<HashMap<String, String>>,
) -> HandlerResult<serde_json::Value> {
    let cfg = BuiltinConfig::from_env();
    let mdir = models_dir(&state.data_dir);

    // `?model=<id>`: delete THAT model. Absent = the legacy contract —
    // delete the active MAIN model (what the shipped UI does today; keeping
    // it byte-compatible for every existing caller).
    let model_id = match params.get("model") {
        Some(m) => {
            let m = m.trim().to_string();
            if m.is_empty() {
                return Err(ErrorResponse::bad_request("model must not be empty"));
            }
            if !installed_model_ids(&mdir).contains(&m) {
                return Err(ErrorResponse::bad_request(format!(
                    "model '{m}' is not installed (installed: {})",
                    installed_model_ids(&mdir).join(", ")
                )));
            }
            m
        }
        None => installed_model(&mdir)
            .map(|(_, m)| m.id)
            .unwrap_or_else(|| BUILTIN_MODEL_ID.to_string()),
    };

    // Companion target: stop ITS port + drop ITS instance; the main model,
    // its instance and the main port are untouched.
    if is_vlm_model(&model_id) {
        if health_check(cfg.vlm_port).await {
            tracing::info!(
                port = cfg.vlm_port,
                "builtin llm: stopping vision companion for delete"
            );
            kill_process_on_port(cfg.vlm_port);
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        if let Ok(manager) = get_instance_manager() {
            let id = companion_instance_id(&model_id);
            if manager.get_instance(&id).is_some() {
                if let Err(e) = manager.force_remove_instance(&id).await {
                    return Err(ErrorResponse::internal(format!(
                        "Failed to remove companion instance: {e}"
                    )));
                }
            }
        }
        let model_dir = mdir.join(&model_id);
        let deleted = match std::fs::remove_dir_all(&model_dir) {
            Ok(_) => true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => {
                tracing::warn!(error = %e, path = %model_dir.display(), "builtin llm: failed to remove companion dir");
                false
            }
        };
        return ok(json!({ "deleted": deleted, "model_id": model_id, "companion": true }));
    }

    // Stop the server first (best-effort; no process handle survives bootstrap).
    if health_check(cfg.port).await {
        tracing::info!(
            port = cfg.port,
            "builtin llm: stopping server for model delete"
        );
        kill_process_on_port(cfg.port);
        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    // Drop the manager instance so state stays consistent (a stale builtin
    // instance pointing at a deleted model would break the next download's
    // bootstrap bring-up, and activate would reference a dead model).
    // force_remove: the builtin is usually the ACTIVE backend, which plain
    // remove_instance refuses to delete — the previously swallowed failure
    // left an orphan instance listed (and selectable) in the chat picker
    // forever. Fail loudly BEFORE the files go: files deleted + instance
    // stuck is exactly the inconsistent state this prevents.
    if let Ok(manager) = get_instance_manager() {
        if manager.get_instance(BUILTIN_INSTANCE_ID).is_some() {
            if let Err(e) = manager.force_remove_instance(BUILTIN_INSTANCE_ID).await {
                tracing::error!(error = %e, "builtin llm: failed to remove backend instance");
                return Err(ErrorResponse::internal(format!(
                    "Failed to remove builtin backend instance: {e}"
                )));
            }
        }
    }

    let model_dir = mdir.join(&model_id);
    let deleted = match std::fs::remove_dir_all(&model_dir) {
        Ok(_) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => {
            tracing::warn!(error = %e, path = %model_dir.display(), "builtin llm: failed to remove model dir");
            false
        }
    };

    if deleted {
        tracing::info!("builtin llm: model deleted");
        // Multi-model coexistence: the deleted model must not stay the
        // active pointer — clear it; installed_model's scan fallback picks
        // the remaining model (if any) on the next bootstrap.
        if read_active_model(&mdir).as_deref() == Some(model_id.as_str()) {
            clear_active_model(&mdir);
        }
    }

    ok(json!({ "deleted": deleted, "model_id": model_id }))
}

// ---------------------------------------------------------------------------
// Restart
// ---------------------------------------------------------------------------

/// POST /api/builtin-llm/restart
///
/// Because bootstrap drops the process handle, a true "kill a healthy server
/// and start a new one" is not possible — this endpoint therefore guarantees
/// the *end state*: a running, healthy builtin server. If the server is
/// already healthy it reports `already_running` and does nothing (never
/// destroys a working server). If it is down/wedged, it frees the port
/// (best-effort) and spawns a fresh one.
#[utoipa::path(
    post,
    path = "/api/builtin-llm/restart",
    tag = "builtin-llm",
    responses(
        (status = 200, description = "llama.cpp server restarted"),
    )
)]
pub async fn restart_handler(
    State(state): State<crate::server::types::ServerState>,
    Query(params): Query<HashMap<String, String>>,
) -> HandlerResult<serde_json::Value> {
    // Optional model switch (?model=<id>): multi-model coexistence — model
    // files stay on disk, switching is pointer + respawn, never a
    // re-download. Validated against the downloaded set; unknown/missing →
    // 400 pointing at the download flow.
    let mdir_switch = models_dir(&state.data_dir);
    let model_change_requested = match params.get("model") {
        Some(want) => {
            let want = want.trim().to_string();
            if want.is_empty() {
                return Err(ErrorResponse::bad_request("model must not be empty"));
            }
            if !installed_model_ids(&mdir_switch).contains(&want) {
                return Err(ErrorResponse::bad_request(format!(
                    "model '{want}' is not downloaded — download it first (installed: {})",
                    installed_model_ids(&mdir_switch).join(", ")
                )));
            }
            let changing = read_active_model(&mdir_switch).as_deref() != Some(want.as_str());
            if changing {
                write_active_model(&mdir_switch, &want);
            }
            changing
        }
        None => false,
    };

    // Optional ctx override (?ctx=N): applied for this spawn and persisted in
    // the process env so subsequent bootstraps keep it (matches the port
    // override pattern). Validated to a sane window; 0/garbage → 400.
    let mut cfg = BuiltinConfig::from_env();
    // API-owned engine knobs are persisted beside the models so a service
    // restart cannot silently revert them (ctx 32768 -> 8192 on 2026-10-30).
    let overrides_path = state.data_dir.join("builtin_engine.json");
    let mut overrides = super::config::EngineOverrides::load(&overrides_path);
    let mut overrides_dirty = false;
    // The installed model's own default — used to give `?ctx=<default>` a
    // "reset to default" meaning (clears the override instead of pinning it).
    let model_default_ctx: Option<u32> =
        installed_model(&models_dir(&state.data_dir)).map(|(d, _)| d.default_ctx);
    if let Some(ctx_str) = params.get("ctx") {
        match ctx_str.trim().parse::<usize>() {
            Ok(n) if (1024..=1_048_576).contains(&n) => {
                if model_default_ctx == Some(n as u32) {
                    // Requested == the model default → clear any override.
                    std::env::remove_var("NEOMIND_BUILTIN_LLM_CTX");
                    cfg.ctx = None;
                    overrides.ctx = None;
                    overrides_dirty = true;
                } else {
                    std::env::set_var("NEOMIND_BUILTIN_LLM_CTX", n.to_string());
                    cfg.ctx = Some(n);
                    overrides.ctx = Some(n);
                    overrides_dirty = true;
                }
            }
            _ => {
                return Err(ErrorResponse::bad_request(
                    "invalid ctx (expected 1024..=1048576)",
                ));
            }
        }
    }
    if !cfg.enabled {
        return Err(ErrorResponse::bad_request(
            "builtin LLM disabled (NEOMIND_BUILTIN_LLM=off)",
        ));
    }
    // Mutual exclusion with the runtime upgrade (completes the pair with
    // download_handler's guard): a mid-upgrade restart would race the
    // upgrade's own stop/respawn step.
    if super::runtime_manager::upgrade_active() {
        return Err(ErrorResponse::conflict(
            "a llama.cpp runtime upgrade is in flight — retry after it finishes",
        ));
    }

    // A healthy server is normally left alone — EXCEPT when a ctx override
    // was requested that differs from the running server's actual n_ctx:
    // applying a new context requires a respawn.
    // Optional engine knobs (?parallel=N, ?kv=q8_0|f16): same env-backed
    // persistence pattern as ?ctx (process env; survives until a service
    // restart, where systemd env wins). parallel=0 omits `--parallel`
    // entirely (upstream default: 4 slots, unified KV); kv=f16 disables
    // the q8_0 KV + flash-attention default.
    let mut engine_change_requested = false;
    if let Some(p_str) = params.get("parallel") {
        match p_str.trim().parse::<usize>() {
            Ok(0) => {
                engine_change_requested |= cfg.parallel.is_some();
                std::env::set_var("NEOMIND_BUILTIN_LLM_PARALLEL", "0");
                cfg.parallel = None;
                overrides.parallel = Some(0);
                overrides_dirty = true;
            }
            Ok(n) if (1..=16).contains(&n) => {
                engine_change_requested |= cfg.parallel != Some(n);
                std::env::set_var("NEOMIND_BUILTIN_LLM_PARALLEL", n.to_string());
                cfg.parallel = Some(n);
                overrides.parallel = Some(n);
                overrides_dirty = true;
            }
            _ => {
                return Err(ErrorResponse::bad_request(
                    "invalid parallel (expected 0..=16; 0 = upstream default)",
                ));
            }
        }
    }
    if let Some(kv_str) = params.get("kv") {
        match kv_str.trim().to_ascii_lowercase().as_str() {
            "q8_0" | "q8" => {
                engine_change_requested |= !cfg.kv_quant;
                std::env::remove_var("NEOMIND_BUILTIN_LLM_KV");
                cfg.kv_quant = true;
                overrides.kv_quant = Some(true);
                overrides_dirty = true;
            }
            "f16" | "off" => {
                engine_change_requested |= cfg.kv_quant;
                std::env::set_var("NEOMIND_BUILTIN_LLM_KV", "f16");
                cfg.kv_quant = false;
                overrides.kv_quant = Some(false);
                overrides_dirty = true;
            }
            _ => {
                return Err(ErrorResponse::bad_request("invalid kv (expected q8_0|f16)"));
            }
        }
    }

    // Optional GPU offload pin (?ngl=N or ?ngl=auto). `auto` clears the pin
    // so the VRAM planner sizes the offload; N passes an explicit -ngl.
    if let Some(v) = params.get("ngl") {
        let v = v.trim();
        if v.eq_ignore_ascii_case("auto") || v.is_empty() {
            engine_change_requested = true;
            std::env::remove_var("NEOMIND_BUILTIN_MODEL_NGL");
            cfg.ngl = None;
            overrides.ngl = None;
            overrides_dirty = true;
        } else {
            match v.parse::<u16>() {
                Ok(n) => {
                    engine_change_requested |= cfg.ngl != Some(n);
                    std::env::set_var("NEOMIND_BUILTIN_MODEL_NGL", n.to_string());
                    cfg.ngl = Some(n);
                    overrides.ngl = Some(n);
                    overrides_dirty = true;
                }
                Err(_) => {
                    return Err(ErrorResponse::bad_request(
                        "invalid ngl (expected 0..=65535 or auto)",
                    ));
                }
            }
        }
    }

    // Companion-only engine knobs (?vlm_ctx= / ?vlm_kv=): persisted the
    // same env-backed way; a change respawns ONLY the companion below —
    // the main server must not bounce because a describe-model knob moved.
    let mut vlm_change_requested = false;
    if let Some(v) = params.get("vlm_ctx") {
        match v.trim().parse::<usize>() {
            Ok(n) if (1024..=1_048_576).contains(&n) => {
                vlm_change_requested |= cfg.vlm_ctx != Some(n);
                std::env::set_var("NEOMIND_BUILTIN_VLM_CTX", n.to_string());
                cfg.vlm_ctx = Some(n);
                overrides.vlm_ctx = Some(n);
                overrides_dirty = true;
            }
            _ => {
                return Err(ErrorResponse::bad_request(
                    "invalid vlm_ctx (expected 1024..=1048576)",
                ));
            }
        }
    }
    if let Some(kv_str) = params.get("vlm_kv") {
        match kv_str.trim().to_ascii_lowercase().as_str() {
            "q8_0" | "q8" => {
                vlm_change_requested |= cfg.vlm_kv_quant != Some(true);
                std::env::remove_var("NEOMIND_BUILTIN_VLM_KV");
                cfg.vlm_kv_quant = Some(true);
                overrides.vlm_kv_quant = Some(true);
                overrides_dirty = true;
            }
            "f16" | "off" => {
                vlm_change_requested |= cfg.vlm_kv_quant != Some(false);
                std::env::set_var("NEOMIND_BUILTIN_VLM_KV", "f16");
                cfg.vlm_kv_quant = Some(false);
                overrides.vlm_kv_quant = Some(false);
                overrides_dirty = true;
            }
            _ => {
                return Err(ErrorResponse::bad_request(
                    "invalid vlm_kv (expected q8_0|f16)",
                ));
            }
        }
    }

    // Companion GPU-layer override (?vlm_ngl=N | auto).
    if let Some(v) = params.get("vlm_ngl") {
        let v = v.trim();
        if v.eq_ignore_ascii_case("auto") || v.is_empty() {
            vlm_change_requested = true;
            std::env::remove_var("NEOMIND_BUILTIN_VLM_NGL");
            cfg.vlm_ngl = None;
            overrides.vlm_ngl = None;
            overrides_dirty = true;
        } else {
            match v.parse::<u16>() {
                Ok(n) => {
                    vlm_change_requested |= cfg.vlm_ngl != Some(n);
                    std::env::set_var("NEOMIND_BUILTIN_VLM_NGL", n.to_string());
                    cfg.vlm_ngl = Some(n);
                    overrides.vlm_ngl = Some(n);
                    overrides_dirty = true;
                }
                Err(_) => {
                    return Err(ErrorResponse::bad_request(
                        "invalid vlm_ngl (expected 0..=65535 or auto)",
                    ));
                }
            }
        }
    }

    if overrides_dirty {
        if let Err(e) = overrides.save(&overrides_path) {
            tracing::warn!(
                error = %e,
                path = %overrides_path.display(),
                "builtin llm: failed to persist engine overrides"
            );
        }
    }

    // Companion knob change alone: respawn ONLY the companion and return —
    // no main-server bounce, no full stop_all (which would kill the main).
    if vlm_change_requested {
        let manager = get_manager()?;
        if health_check(cfg.vlm_port).await {
            kill_process_on_port(cfg.vlm_port);
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        match ensure_companion(&state.data_dir, &cfg, &manager).await {
            Ok(ep) => {
                return ok(json!({
                    "restarted": true,
                    "already_running": false,
                    "companion_endpoint": ep,
                    "companion_only": true,
                }));
            }
            Err(skip) => {
                return Err(ErrorResponse::internal(format!(
                    "vision companion restart failed: {skip}"
                )));
            }
        }
    }

    let current_n_ctx = query_server_n_ctx(cfg.port).await;
    let ctx_change_requested = params
        .get("ctx")
        .and_then(|v| v.trim().parse::<usize>().ok())
        .map(|want| current_n_ctx.map(|cur| cur != want).unwrap_or(true))
        .unwrap_or(false);
    if health_check(cfg.port).await
        && !ctx_change_requested
        && !model_change_requested
        && !engine_change_requested
    {
        return ok(json!({
            "restarted": true,
            "already_running": true,
            "endpoint": format!("http://127.0.0.1:{}", cfg.port),
        }));
    }

    // Free a possibly-wedged port, then spawn fresh. Kill BOTH ways: the
    // in-process registry catches servers still LOADING (not yet bound, so
    // a port-only kill misses them — they then hang in CUDA init while the
    // replacement binds, leaving an orphan that eats nvmap and confuses the
    // next restart; observed 2026-10-15), and the port kill catches
    // foreign/legacy listeners.
    crate::builtin_llm::server::stop_all_llama_servers();
    kill_process_on_port(cfg.port);
    tokio::time::sleep(Duration::from_millis(500)).await;

    let manager = get_manager()?;
    match spawn_builtin_server(&state.data_dir, &cfg, &manager, None).await {
        Ok(endpoint) => {
            // The builtin may be the ACTIVE chat backend — its session-manager
            // snapshot still carries the OLD endpoint/capabilities (prompt
            // budget + Context X/Y display). Re-push the backend when active.
            let builtin_active = manager
                .get_active_instance()
                .map(|i| i.id == BUILTIN_INSTANCE_ID)
                .unwrap_or(false);
            if builtin_active {
                if let Some(instance) = manager.get_instance(BUILTIN_INSTANCE_ID) {
                    let backend = LlmBackend::LlamaCpp {
                        endpoint: instance
                            .endpoint
                            .clone()
                            .unwrap_or_else(|| endpoint.clone()),
                        model: instance.model.clone(),
                        capabilities: Some(convert_capabilities(&instance.capabilities)),
                    };
                    if let Err(e) = state.agents.session_manager.set_llm_backend(backend).await {
                        tracing::warn!(error = %e, "Failed to refresh session backend after ctx change");
                    }
                }
            }
            // stop_all_llama_servers above killed the companion too — bring
            // it back whenever one is installed (idempotent; skip reasons
            // logged, never fatal to the restart).
            match ensure_companion(&state.data_dir, &cfg, &manager).await {
                Ok(ep) => {
                    tracing::info!(endpoint = %ep, "builtin llm: vision companion restored after restart")
                }
                Err(skip) => {
                    tracing::info!(reason = %skip, "builtin llm: vision companion not restored")
                }
            }
            ok(json!({ "restarted": true, "already_running": false, "endpoint": endpoint }))
        }
        Err(e) => {
            let msg = e.to_string();
            // "no manifest" / "model file missing" mean the model was never
            // (successfully) downloaded — a 404 pointing the caller at the
            // download flow, not a 500.
            if msg.contains("no manifest") || msg.contains("model file missing") {
                Err(ErrorResponse::not_found(format!(
                    "builtin LLM restart failed: {msg}"
                )))
            } else {
                Err(ErrorResponse::internal(format!(
                    "builtin LLM restart failed: {msg}"
                )))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Activate
// ---------------------------------------------------------------------------

/// POST /api/builtin-llm/activate
#[utoipa::path(
    post,
    path = "/api/builtin-llm/activate",
    tag = "builtin-llm",
    responses(
        (status = 200, description = "Bundled backend marked active"),
    )
)]
pub async fn activate_handler(
    State(state): State<crate::server::types::ServerState>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_manager()?;
    let instance = manager.get_instance(BUILTIN_INSTANCE_ID).ok_or_else(|| {
        ErrorResponse::not_found(format!(
            "builtin instance {} not found — download the model first",
            BUILTIN_INSTANCE_ID
        ))
    })?;

    manager
        .set_active(BUILTIN_INSTANCE_ID)
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    // Surface the builtin to chat sessions + persist as default, mirroring
    // `llm_backends::activate_backend_handler` (builtin is always LlamaCpp).
    let endpoint = instance
        .endpoint
        .clone()
        .unwrap_or_else(|| format!("http://127.0.0.1:{}", BuiltinConfig::from_env().port));
    let backend = LlmBackend::LlamaCpp {
        endpoint,
        model: instance.model.clone(),
        capabilities: Some(convert_capabilities(&instance.capabilities)),
    };
    state
        .agents
        .session_manager
        .set_llm_backend(backend)
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    let settings_request = crate::config::LlmSettingsRequest {
        backend: "llamacpp".to_string(),
        model: instance.model.clone(),
        endpoint: instance.endpoint.clone(),
        api_key: None,
    };
    if let Err(e) = state.save_llm_config(&settings_request).await {
        tracing::warn!(error = %e, "Failed to save LLM config after builtin activate");
    }

    ok(json!({
        "id": BUILTIN_INSTANCE_ID,
        "message": "Builtin backend activated",
    }))
}

/// One-shot system memory snapshot for the installability check (MB).
fn memory_snapshot_mb() -> (u64, u64) {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let total = sys.total_memory() / (1024 * 1024);
    let mut available = sys.available_memory() / (1024 * 1024);
    if available == 0 {
        // Some platforms (observed: macOS with System::new) report 0
        // available — fall back to total instead of falsely blocking every
        // model. Linux (the main deployment target) reports real numbers.
        available = total;
    }
    (total, available)
}

/// Read the running llama-server's vision modality from /props
/// (`modalities.vision` — present only when a projector mounted). `None`
/// when unreachable or the field is absent (older runtimes); callers fall
/// back to what they know about the spawn.
pub(super) async fn query_server_modalities_vision(port: u16) -> Option<bool> {
    let url = format!("http://127.0.0.1:{}/props", port);
    let resp = reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .ok()?;
    let v: serde_json::Value = resp.json().await.ok()?;
    v.get("modalities")?.get("vision")?.as_bool()
}

/// Read the running llama-server's actual context size from /props.
/// `None` when the server is unreachable or the field is absent.
async fn query_server_n_ctx(port: u16) -> Option<usize> {
    let url = format!("http://127.0.0.1:{}/props", port);
    let resp = reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .ok()?;
    let v: serde_json::Value = resp.json().await.ok()?;
    v.get("default_generation_settings")
        .and_then(|g| g.get("n_ctx"))
        .and_then(|n| n.as_u64())
        .map(|n| n as usize)
        .or_else(|| v.get("n_ctx").and_then(|n| n.as_u64()).map(|n| n as usize))
}

/// The context the running builtin llama-server actually serves: /props
/// `n_ctx` verbatim — the server's own per-request truth. Falls back to
/// the requested ctx when the server can't be queried. (An earlier version
/// divided by the auto slot count, which silently shrank the window 4×
/// once /props reported multiple slots — the 8K-wedge class.)
pub(super) async fn effective_spawn_ctx(port: u16, requested_ctx: usize) -> usize {
    let url = format!("http://127.0.0.1:{}/props", port);
    let props: Option<serde_json::Value> = async {
        let resp = reqwest::Client::new()
            .get(&url)
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .ok()?;
        resp.json().await.ok()
    }
    .await;
    let Some(v) = props else {
        return requested_ctx;
    };
    let n_ctx = v
        .get("default_generation_settings")
        .and_then(|g| g.get("n_ctx"))
        .and_then(|n| n.as_u64())
        .map(|n| n as usize)
        .or_else(|| v.get("n_ctx").and_then(|n| n.as_u64()).map(|n| n as usize))
        .unwrap_or(requested_ctx);
    n_ctx
}

/// Convert storage `BackendCapabilities` to core `BackendCapabilities`
/// (same shape as `llm_backends::activate_backend_handler`).
fn convert_capabilities(
    caps: &neomind_storage::BackendCapabilities,
) -> neomind_core::BackendCapabilities {
    neomind_core::BackendCapabilities {
        streaming: caps.supports_streaming,
        multimodal: caps.supports_multimodal,
        function_calling: caps.supports_tools,
        thinking_display: caps.supports_thinking,
        max_context: Some(caps.max_context),
        multiple_models: false,
        modalities: Vec::new(),
        supports_images: caps.supports_multimodal,
        reasoning: neomind_core::ReasoningCapabilities::default(),
    }
}

/// Get the global instance manager (error instead of panic).
fn get_manager() -> Result<Arc<LlmBackendInstanceManager>, ErrorResponse> {
    get_instance_manager().map_err(|e| ErrorResponse::internal(e.to_string()))
}

// ---------------------------------------------------------------------------
// Runtime management (standalone llama.cpp upgrades — see runtime_manager)
// ---------------------------------------------------------------------------

/// Stop + respawn the builtin engine (main + companion) so a newly
/// installed/reverted runtime actually takes effect. Skipped (Ok) when no
/// model is installed — nothing to spawn. Mirrors the tail of
/// `restart_handler` (dual kill, session-backend re-push, companion
/// restore).
async fn restart_builtin_engine(state: &crate::server::types::ServerState) -> Result<(), String> {
    let mdir = models_dir(&state.data_dir);
    if installed_model(&mdir).is_none() {
        return Ok(()); // nothing installed — the next spawn picks the new runtime up
    }
    let cfg = BuiltinConfig::from_env();
    crate::builtin_llm::server::stop_all_llama_servers();
    kill_process_on_port(cfg.port);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let manager = get_manager().map_err(|e| e.message)?;
    match spawn_builtin_server(&state.data_dir, &cfg, &manager, None).await {
        Ok(_) => {
            // The builtin may be the ACTIVE chat backend — re-push so its
            // session-manager snapshot (endpoint/capabilities) refreshes.
            let builtin_active = manager
                .get_active_instance()
                .map(|i| i.id == BUILTIN_INSTANCE_ID)
                .unwrap_or(false);
            if builtin_active {
                if let Some(instance) = manager.get_instance(BUILTIN_INSTANCE_ID) {
                    let backend = LlmBackend::LlamaCpp {
                        endpoint: instance.endpoint.clone().unwrap_or_default(),
                        model: instance.model.clone(),
                        capabilities: Some(convert_capabilities(&instance.capabilities)),
                    };
                    if let Err(e) = state.agents.session_manager.set_llm_backend(backend).await {
                        tracing::warn!(error = %e, "Failed to refresh session backend after runtime change");
                    }
                }
            }
            match ensure_companion(&state.data_dir, &cfg, &manager).await {
                Ok(ep) => {
                    tracing::info!(endpoint = %ep, "builtin llm: companion restored after runtime change")
                }
                Err(skip) => {
                    tracing::info!(reason = %skip, "builtin llm: companion not restored")
                }
            }
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}

fn publish_runtime_event(
    bus: &Option<Arc<neomind_core::eventbus::EventBus>>,
    phase: &str,
    target_tag: Option<&str>,
    error: Option<&str>,
) {
    if let Some(bus) = bus {
        let _ = bus.publish_sync(NeoMindEvent::RuntimeUpgradeProgress {
            phase: phase.to_string(),
            variant: Some(
                super::runtime_manager::variant_name(&super::runtime_manager::effective_variant())
                    .to_string(),
            ),
            target_tag: target_tag.map(str::to_string),
            current_tag: None,
            downloaded: None,
            total: None,
            message: None,
            error: error.map(str::to_string),
        });
    }
}

/// GET /api/builtin-llm/runtime — resolved/installed runtimes, the bundled
/// identity, and the channel recommendation (`update_available`).
#[utoipa::path(
    get,
    path = "/api/builtin-llm/runtime",
    tag = "builtin-llm",
    responses(
        (status = 200, description = "llama.cpp runtime status + channel recommendation"),
    )
)]
pub async fn runtime_status_handler(
    State(state): State<crate::server::types::ServerState>,
) -> HandlerResult<serde_json::Value> {
    ok(super::runtime_manager::runtime_snapshot(&state.data_dir).await)
}

/// POST /api/builtin-llm/runtime/check — force a channel refresh (admin).
#[utoipa::path(
    post,
    path = "/api/builtin-llm/runtime/check",
    tag = "builtin-llm",
    responses(
        (status = 200, description = "Channel re-fetched; snapshot returned"),
    )
)]
pub async fn runtime_check_handler(
    State(state): State<crate::server::types::ServerState>,
    Extension(admin): Extension<crate::auth_users::SessionInfo>,
) -> HandlerResult<serde_json::Value> {
    if admin.role != crate::auth_users::UserRole::Admin {
        return Err(ErrorResponse::bad_request("Admin access required"));
    }
    let _ = super::runtime_manager::fetch_channel(true).await;
    ok(super::runtime_manager::runtime_snapshot(&state.data_dir).await)
}

/// POST /api/builtin-llm/runtime/variant — persist (or clear, "auto"/"")
/// the runtime variant override (admin). Effective on the next spawn.
#[utoipa::path(
    post,
    path = "/api/builtin-llm/runtime/variant",
    tag = "builtin-llm",
    request_body = serde_json::Value,
    responses(
        (status = 200, description = "Override persisted"),
    )
)]
pub async fn runtime_variant_handler(
    Extension(admin): Extension<crate::auth_users::SessionInfo>,
    payload: Option<Json<serde_json::Value>>,
) -> HandlerResult<serde_json::Value> {
    if admin.role != crate::auth_users::UserRole::Admin {
        return Err(ErrorResponse::bad_request("Admin access required"));
    }
    let value = payload
        .and_then(|Json(v)| {
            v.get("variant")
                .and_then(|x| x.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default();
    super::runtime_manager::set_persisted_variant(&value).map_err(ErrorResponse::bad_request)?;
    ok(json!({ "saved": value }))
}

/// POST /api/builtin-llm/runtime/upgrade — install the channel-recommended
/// runtime (admin). Returns immediately; progress flows via
/// `RuntimeUpgradeProgress` events and `GET /api/builtin-llm/runtime`.
#[utoipa::path(
    post,
    path = "/api/builtin-llm/runtime/upgrade",
    tag = "builtin-llm",
    responses(
        (status = 200, description = "Upgrade started"),
    )
)]
pub async fn runtime_upgrade_handler(
    State(state): State<crate::server::types::ServerState>,
    Extension(admin): Extension<crate::auth_users::SessionInfo>,
) -> HandlerResult<serde_json::Value> {
    if admin.role != crate::auth_users::UserRole::Admin {
        return Err(ErrorResponse::bad_request("Admin access required"));
    }
    if super::runtime_manager::upgrade_active() {
        return ok(json!({ "started": false, "already_running": true }));
    }
    if model_download_active() {
        return Err(ErrorResponse::conflict(
            "a model download is in flight — retry after it finishes",
        ));
    }
    let task_state = state.clone();
    let bus = state.event_bus();
    tokio::spawn(async move {
        let data_dir = task_state.data_dir.clone();
        match super::runtime_manager::upgrade_runtime(&data_dir, bus.clone()).await {
            Ok((_from, to)) => {
                publish_runtime_event(&bus, "restarting", Some(&to), None);
                match restart_builtin_engine(&task_state).await {
                    Ok(()) => {
                        tracing::info!(tag = %to, "builtin llm: runtime upgraded to {to}");
                        publish_runtime_event(&bus, "done", Some(&to), None);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "builtin llm: restart after runtime upgrade failed");
                        publish_runtime_event(&bus, "error", Some(&to), Some(&e));
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "builtin llm: runtime upgrade failed");
                publish_runtime_event(&bus, "error", None, Some(&e));
            }
        }
    });
    ok(json!({ "started": true, "already_running": false }))
}

/// POST /api/builtin-llm/runtime/revert — delete the NEWEST cached runtime
/// (admin); resolution falls back to the previous cache / the bundled
/// binary, then the engine restarts onto it.
#[utoipa::path(
    post,
    path = "/api/builtin-llm/runtime/revert",
    tag = "builtin-llm",
    responses(
        (status = 200, description = "Newest runtime cache removed"),
    )
)]
pub async fn runtime_revert_handler(
    State(state): State<crate::server::types::ServerState>,
    Extension(admin): Extension<crate::auth_users::SessionInfo>,
) -> HandlerResult<serde_json::Value> {
    if admin.role != crate::auth_users::UserRole::Admin {
        return Err(ErrorResponse::bad_request("Admin access required"));
    }
    let removed =
        super::runtime_manager::revert_runtime(&state.data_dir).map_err(ErrorResponse::conflict)?;
    let bus = state.event_bus();
    publish_runtime_event(&bus, "restarting", None, None);
    restart_builtin_engine(&state)
        .await
        .map_err(ErrorResponse::internal)?;
    ok(json!({ "reverted": true, "removed_tag": removed }))
}

#[cfg(test)]
// Several tests hold the `test_serial()` std guard and the tokio download
// lock across the whole test body ON PURPOSE — that serialization (and, for
// the download tests, holding the lock while awaiting) is the point.
#[allow(clippy::await_holding_lock)]
mod tests {
    use super::*;

    #[test]
    fn tier_default_ctx_buckets() {
        // RAM-tier defaults double per tier; >=32GB leaves the model default.
        assert_eq!(tier_default_ctx(4096), 16 * 1024);
        assert_eq!(tier_default_ctx(7 * 1024), 16 * 1024); // NG4520 7.4 GB
        assert_eq!(tier_default_ctx(8 * 1024), 32 * 1024);
        assert_eq!(tier_default_ctx(16 * 1024), 64 * 1024);
        assert_eq!(tier_default_ctx(32 * 1024), usize::MAX);
    }
    use crate::server::ServerState;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use axum::Json;
    use axum::Router;
    use std::path::PathBuf;
    use tower::ServiceExt;

    fn temp_data_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "neomind-builtin-api-{}-{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The tests below share process-global download state (DOWNLOAD_LOCK /
    /// DL_ACTIVE). tokio runs tests in parallel on separate threads, so a
    /// lock-holding test would leak "downloading" into a concurrent status
    /// test. Serialize all of them through one mutex (same pattern as
    /// config.rs's env_lock).
    fn test_serial() -> &'static std::sync::Mutex<()> {
        // Shared with runtime_manager's integration test via the pub(crate)
        // helper below: the upgrade orchestration and these handler tests
        // mutate the same process-globals (UPGRADE_ACTIVE, DL_ACTIVE, the
        // download lock) and must not run concurrently.
        builtin_llm_test_serial()
    }

    /// Pin the builtin port to a just-freed ephemeral port for the duration
    /// of the closure. The status handler health-checks `cfg.port` — with the
    /// default 8081 the tests collided with any live dev instance's
    /// llama-server (reported "running" instead of "stopped", and the panic
    /// poisoned the shared serial lock for the following tests).
    async fn with_ephemeral_port<F, Fut>(f: F) -> serde_json::Value
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = serde_json::Value>,
    {
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").expect("ephemeral bind");
            l.local_addr().expect("addr").port()
        };
        let prev = std::env::var("NEOMIND_BUILTIN_LLM_PORT").ok();
        std::env::set_var("NEOMIND_BUILTIN_LLM_PORT", port.to_string());
        let out = f().await;
        match prev {
            Some(v) => std::env::set_var("NEOMIND_BUILTIN_LLM_PORT", v),
            None => std::env::remove_var("NEOMIND_BUILTIN_LLM_PORT"),
        }
        out
    }

    async fn status_body(data_dir: PathBuf) -> serde_json::Value {
        with_ephemeral_port(|| async {
            let mut state = ServerState::new_for_testing().await;
            state.data_dir = data_dir;
            let app = Router::new()
                .route("/api/builtin-llm/status", get(super::status_handler))
                .with_state(state);
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/builtin-llm/status")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap();
            serde_json::from_slice(&body).unwrap()
        })
        .await
    }

    #[tokio::test]
    async fn status_reports_not_installed_when_empty_dir() {
        let _g = test_serial().lock().unwrap();
        let v = status_body(temp_data_dir("empty")).await;
        assert_eq!(v["data"]["installed"], false);
        assert_eq!(v["data"]["server_state"], "not_configured");
        assert!(v["data"]["model_id"].is_null());
    }

    #[tokio::test]
    async fn status_reports_installed_stopped_when_manifest_and_model_exist() {
        let _g = test_serial().lock().unwrap();
        use neomind_core::builtin_llm::manifest::{save_manifest, ModelManifest, BUILTIN_MODEL_ID};

        let dir = temp_data_dir("installed");
        let mdir = dir.join("models");
        std::fs::create_dir_all(mdir.join(BUILTIN_MODEL_ID)).unwrap();
        // Fake model file (non-empty so file_size is reported).
        std::fs::write(
            mdir.join(BUILTIN_MODEL_ID).join("lfm25-2.6b-q4_k_m.gguf"),
            b"fake-model",
        )
        .unwrap();
        save_manifest(
            &mdir,
            &ModelManifest {
                id: BUILTIN_MODEL_ID.to_string(),
                version: "1.0".to_string(),
                file_name: "lfm25-2.6b-q4_k_m.gguf".to_string(),
                sha256: "abc".to_string(),
                quant: "q4_k_m".to_string(),
                format: neomind_core::builtin_llm::manifest::MODEL_FORMAT_GGUF.to_string(),
                mmproj: None,
            },
        )
        .expect("save manifest");

        let v = status_body(dir).await;
        assert_eq!(v["data"]["installed"], true);
        // Nothing listens on the builtin port → stopped (not running).
        assert_eq!(v["data"]["server_state"], "stopped");
        assert_eq!(v["data"]["model_id"], BUILTIN_MODEL_ID);
    }

    #[tokio::test]
    async fn status_reports_downloading_when_lock_active() {
        let _g = test_serial().lock().unwrap();
        // Hold the download lock so the status endpoint reports "downloading".
        let _lock = download_lock();
        let _guard = _lock.lock().await;
        let v = status_body(temp_data_dir("dl")).await;
        assert_eq!(v["data"]["server_state"], "downloading");
        assert_eq!(v["data"]["installed"], false);
    }

    #[tokio::test]
    async fn download_returns_already_running_when_lock_held() {
        let _g = test_serial().lock().unwrap();
        let state = ServerState::new_for_testing().await;
        let _lock = download_lock();
        let _guard = _lock.lock().await;
        // Direct handler call (matches crate test style for handlers).
        let Json(resp) = download_handler(State(state), None).await.unwrap();
        let data = resp.data.unwrap();
        assert_eq!(data["started"], false);
        assert_eq!(data["already_running"], true);
    }
}

#[cfg(test)]
mod quant_override_tests {
    use super::*;

    fn cfg_with_quant(q: &str) -> BuiltinConfig {
        BuiltinConfig {
            quant_override: Some(q.to_string()),
            ..Default::default()
        }
    }

    fn minicpm_def() -> super::super::catalog::CatalogModel {
        let def = model_def("minicpm5-2b").expect("registry entry");
        builtin_to_catalog(def)
    }

    fn lfm_def() -> super::super::catalog::CatalogModel {
        let def = model_def("lfm25-2.6b").expect("registry entry");
        builtin_to_catalog(def)
    }

    /// QAD on the MiniCPM default used to sail through resolve_quant and
    /// produce a Q4_K_M download verified against the LFM QAD sha — a
    /// guaranteed 1.5 GB dead download per attempt. It must be REJECTED.
    #[test]
    fn qad_rejected_for_minicpm() {
        let err = resolve_quant(&cfg_with_quant("qad_q4_0"), &minicpm_def()).unwrap_err();
        assert!(
            err.message.contains("unsupported quant"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn quant_set_follows_model() {
        assert!(resolve_quant(&cfg_with_quant("qad_q4_0"), &lfm_def()).is_ok());
        assert!(resolve_quant(&cfg_with_quant("q8_0"), &minicpm_def()).is_ok());
        assert!(resolve_quant(&cfg_with_quant("q4_k_m"), &minicpm_def()).is_ok());
    }

    /// A MiniCPM quant override must verify against the OFFICIAL openbmb
    /// sha (not the LFM table) and land under the model's OWN local name
    /// (the old shared helper hardcoded the lfm25 prefix).
    #[test]
    fn minicpm_override_source_and_name() {
        let (url, sha, file) = resolve_source(&cfg_with_quant("q8_0"), &minicpm_def());
        assert!(url.contains("openbmb/MiniCPM5-2B-GGUF"), "url: {url}");
        assert!(url.ends_with("MiniCPM5-2B-Q8_0.gguf"), "url: {url}");
        assert_eq!(
            sha,
            "c5415f8989bf88a8288f1b55a3cc371af53c07b0faa220a63bd7a990cfaba078"
        );
        assert!(
            file.starts_with("minicpm5-2b-") && file.ends_with(".gguf"),
            "local name must follow the model, got: {file}"
        );
    }

    /// LFM remains on its own official repo + shas through the override path.
    #[test]
    fn lfm_override_source_unchanged() {
        let (url, sha, _) = resolve_source(&cfg_with_quant("q8_0"), &lfm_def());
        assert!(url.contains("LiquidAI/LFM2.5-2.6B-GGUF"), "url: {url}");
        assert_eq!(
            sha,
            "36587fdf27bdfc69caf2637273679a0870ec155162161bde6fd16e8c70bdb757"
        );
    }

    // ---- mmproj (vision projector) source resolution ----

    fn gemma_def() -> super::super::catalog::CatalogModel {
        let def = model_def("gemma4-e2b").expect("registry entry");
        builtin_to_catalog(def)
    }

    /// The projector source is pinned to the model's own repo under its
    /// canonical local name. `resolve_mmproj_source` takes no cfg — the
    /// projector is quant-independent by construction (there is no override
    /// path to drift onto), unlike the text weights above.
    #[test]
    fn mmproj_source_pins_repo_sha_and_local_name() {
        let (url, sha, file) = resolve_mmproj_source(&gemma_def()).expect("gemma mmproj");
        assert_eq!(
            url,
            "https://huggingface.co/google/gemma-4-E2B-it-qat-q4_0-gguf/resolve/main/gemma-4-E2B-it-mmproj.gguf"
        );
        assert_eq!(
            sha,
            "021059cce659fe7f9170d5599761d7bbaf644b798dab9503aca30dc43e6beb14"
        );
        assert_eq!(file, "gemma-4-E2B-it-mmproj.gguf");
    }

    #[test]
    fn mmproj_source_none_for_text_models() {
        assert!(resolve_mmproj_source(&lfm_def()).is_none());
        assert!(resolve_mmproj_source(&minicpm_def()).is_none());
    }

    fn stale_catalog_entry(id: &str) -> super::super::catalog::CatalogModel {
        super::super::catalog::CatalogModel {
            id: id.to_string(),
            display_name: id.to_string(),
            file_name: "x.gguf".to_string(),
            sha256: "deadbeef".to_string(),
            quant: "q4_k_m".to_string(),
            format: "gguf".to_string(),
            target_hardware: None,
            min_runtime: None,
            hf_repo: "r/x".to_string(),
            hf_file: "x.gguf".to_string(),
            size_bytes: 1,
            default_ctx: 32768,
            max_ctx: None,
            default_thinking: false,
            min_ram_mb: 0,
            notes: String::new(),
            recommended: false,
            temperature: None,
            top_p: None,
            top_k: None,
            min_p: None,
            repeat_penalty: None,
            mmproj: None,
            role: None,
        }
    }

    /// The remote catalog is the display list while downloads resolve
    /// through the registry — a stale entry must regain the registry's
    /// projector or the wizard hides the vision badge yet downloads it.
    #[test]
    fn stale_catalog_inherits_registry_mmproj_for_display() {
        let mut base = vec![
            stale_catalog_entry("gemma4-e2b"),
            stale_catalog_entry("some-catalog-only-id"),
        ];
        inherit_registry_mmproj(&mut base);
        assert!(
            base[0].mmproj.is_some(),
            "registry vision model must regain its projector"
        );
        assert!(
            base[1].mmproj.is_none(),
            "catalog-only ids keep the catalog's word (no inheritance source)"
        );
    }

    // ---- vision-companion identity helpers ----

    #[test]
    fn companion_instance_id_roundtrip() {
        assert_eq!(
            companion_instance_id("minicpm-v-4.7-1b"),
            "builtin-vlm-minicpm-v-4.7-1b"
        );
        assert!(is_companion_instance_id("builtin-vlm-minicpm-v-4.7-1b"));
        // The MAIN builtin id must never match the companion prefix — a
        // sloppy prefix check would hide the main chat backend from lists.
        assert!(!is_companion_instance_id(BUILTIN_INSTANCE_ID));
        assert!(!is_companion_instance_id("openai-gpt4o"));
        assert!(!is_companion_instance_id(""));
    }

    #[test]
    fn vlm_role_resolves_from_registry_and_not_from_main_models() {
        assert!(is_vlm_model("minicpm-v-4.7-1b"), "registry companion entry");
        for id in ["minicpm5-2b", "lfm25-2.6b", "gemma4-e2b"] {
            assert!(!is_vlm_model(id), "{id} is a main model");
        }
    }

    #[test]
    fn companion_ram_gate_is_a_plain_floor() {
        assert!(companion_ram_ok(3000, 2500));
        assert!(!companion_ram_ok(2000, 2500));
        assert!(companion_ram_ok(2500, 2500), "exactly enough is enough");
    }
}
