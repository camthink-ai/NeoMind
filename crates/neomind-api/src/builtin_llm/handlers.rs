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

use axum::extract::{Query, State};
use serde_json::json;
use std::collections::HashMap;

use super::runtime::ensure_llama_server;
use neomind_agent::llm_backends::{get_instance_manager, LlmBackendInstanceManager};
use neomind_agent::LlmBackend;
use neomind_core::builtin_llm::manifest::{
    load_manifest, model_def, save_manifest, BuiltinModelDef, ModelManifest, BUILTIN_MODELS,
    BUILTIN_MODEL_ID,
};
use neomind_core::builtin_llm::variant::{model_file_name, Quant};
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

fn models_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("models")
}

// ---------------------------------------------------------------------------
// Pinned download source (HuggingFace)
// ---------------------------------------------------------------------------

/// HuggingFace repo + `resolve/main` prefix for the builtin LFM2.5-2.6B model.
///
/// VERIFIED 2026-08-19 via the HF API (`/api/models/LiquidAI/LFM2.5-2.6B-GGUF/
/// tree/main?expand=true`) + `curl -sIL` on each resolve URL. The LFS OID in
/// the API response and the `x-linked-etag` header agree with the shas below.
const HF_REPO: &str = "https://huggingface.co/LiquidAI/LFM2.5-2.6B-GGUF/resolve/main";

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

/// Which model to install, defaulting to the registry default.
fn resolve_model(model_id: Option<&str>) -> Result<&'static BuiltinModelDef, ErrorResponse> {
    let id = model_id.unwrap_or(BUILTIN_MODEL_ID);
    model_def(id).ok_or_else(|| {
        ErrorResponse::bad_request(format!(
            "unknown builtin model id: {id} (available: {})",
            BUILTIN_MODELS
                .iter()
                .map(|d| d.manifest.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    })
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
    def: &BuiltinModelDef,
) -> Result<Option<Quant>, ErrorResponse> {
    let Some(q) = cfg.quant_override.as_deref() else {
        return Ok(None);
    };
    if def.manifest.id != BUILTIN_MODEL_ID {
        return Err(ErrorResponse::bad_request(format!(
            "quant override is only supported for the default model ({BUILTIN_MODEL_ID})"
        )));
    }
    match q {
        "q4_k_m" => Ok(Some(Quant::Q4_K_M)),
        "q8_0" => Ok(Some(Quant::Q8_0)),
        "qad_q4_0" => Ok(Some(Quant::QAD_Q4_0)),
        other => Err(ErrorResponse::bad_request(format!(
            "unsupported builtin quant: {other}"
        ))),
    }
}

/// The installed model, if any (scan the registry — one builtin model is
/// installed at a time; the server runs one llama-server process).
///
/// Returns the *on-disk* manifest (its `file_name` is the real file — e.g.
/// Docker may pre-bundle QAD under the QAD name) plus the registry def (for
/// ctx/name/thinking flags).
pub fn installed_model(mdir: &Path) -> Option<(BuiltinModelDef, ModelManifest)> {
    BUILTIN_MODELS.iter().find_map(|def| {
        let manifest = load_manifest(mdir, &def.manifest.id).ok().flatten()?;
        manifest
            .model_path(mdir)
            .exists()
            .then(|| (def.clone(), manifest))
    })
}

/// Download source for a def: HF repo + file + sha. Every model downloads its
/// registry entry directly (LFM's entry is now QAD Q4_0, the default); the
/// env `quant_override` swaps LFM to q8_0/q4_k_m for power users.
fn resolve_source(cfg: &BuiltinConfig, def: &BuiltinModelDef) -> (String, String, String) {
    if def.manifest.id == BUILTIN_MODEL_ID {
        if let Ok(Some(quant)) = resolve_quant(cfg, def) {
            return (
                format!("{}/{}", HF_REPO, hf_file_name(quant)),
                hf_sha256(quant).to_string(),
                model_file_name(quant),
            );
        }
    }
    (
        format!(
            "https://huggingface.co/{}/resolve/main/{}",
            def.hf_repo, def.hf_file
        ),
        def.manifest.sha256.clone(),
        def.manifest.file_name.clone(),
    )
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// GET /api/builtin-llm/status
pub async fn status_handler(
    State(state): State<crate::server::types::ServerState>,
) -> HandlerResult<serde_json::Value> {
    let cfg = BuiltinConfig::from_env();

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

    // Downloading overrides running/stopped/not_configured. The lock is the
    // authoritative single-flight gate (per the task design decision); the
    // atomic covers the brief window between lock acquisition and the spawned
    // task actually running.
    let downloading = DL_ACTIVE.load(Ordering::SeqCst) || download_lock().try_lock().is_err();
    if downloading {
        let downloaded = DL_DOWNLOADED.load(Ordering::SeqCst);
        let total = DL_TOTAL.load(Ordering::SeqCst);
        let dl_id = dl_model().lock().unwrap().clone();
        return ok(json!({
            "installed": installed,
            "model_id": dl_id,
            "server_state": "downloading",
            "downloaded_bytes": downloaded,
            "total_bytes": if total > 0 { json!(total) } else { serde_json::Value::Null },
        }));
    }

    let Some(_) = installed_pair else {
        return ok(json!({
            "installed": false,
            "model_id": serde_json::Value::Null,
            "server_state": "not_configured",
            "downloaded_bytes": serde_json::Value::Null,
            "total_bytes": serde_json::Value::Null,
        }));
    };

    let file_size = model_path
        .and_then(|p| std::fs::metadata(&p).ok())
        .map(|m| m.len())
        .unwrap_or(0);
    let server_state = if health_check(cfg.port).await {
        "running"
    } else {
        "stopped"
    };

    // Effective ctx: override (env / restart API) else per-model default —
    // surfaced so the UI can show and edit it.
    let model_def_ctx = installed_pair
        .as_ref()
        .map(|(d, _)| d.default_ctx)
        .unwrap_or(0);
    let effective_ctx = cfg.effective_ctx(model_def_ctx);
    let (total_mb, available_mb) = memory_snapshot_mb();
    let min_ram_mb = installed_pair
        .as_ref()
        .map(|(d, _)| d.min_ram_mb)
        .unwrap_or(0);

    ok(json!({
        "installed": true,
        "model_id": model_id,
        "server_state": server_state,
        "downloaded_bytes": file_size,
        "total_bytes": file_size,
        "ctx": effective_ctx,
        "ctx_override": cfg.ctx,
        "default_ctx": model_def_ctx,
        "memory_ok": available_mb >= min_ram_mb,
        "min_ram_mb": min_ram_mb,
        "available_ram_mb": available_mb,
        "total_ram_mb": total_mb,
    }))
}

// ---------------------------------------------------------------------------
// Models list (registry)
// ---------------------------------------------------------------------------

/// GET /api/builtin-llm/models — the installable builtin models with
/// per-entry install state.
pub async fn models_handler(
    State(state): State<crate::server::types::ServerState>,
) -> HandlerResult<serde_json::Value> {
    let mdir = models_dir(&state.data_dir);
    let (total_mb, available_mb) = memory_snapshot_mb();
    let models: Vec<serde_json::Value> = BUILTIN_MODELS
        .iter()
        .map(|d| {
            let installed = load_manifest(&mdir, &d.manifest.id)
                .ok()
                .flatten()
                .map(|m| m.model_path(&mdir).exists())
                .unwrap_or(false);
            json!({
                "id": d.manifest.id,
                "name": d.display_name,
                "file_name": d.manifest.file_name,
                "quant": d.manifest.quant,
                "size_bytes": d.size_bytes,
                "default_ctx": d.default_ctx,
                "min_ram_mb": d.min_ram_mb,
                // Below the model's floor → the UI discourages the install.
                "memory_ok": available_mb >= d.min_ram_mb,
                "notes": d.notes,
                "recommended": d.recommended,
                "installed": installed,
            })
        })
        .collect();
    ok(json!({ "models": models, "default_model_id": BUILTIN_MODEL_ID }))
}

// ---------------------------------------------------------------------------
// Download
// ---------------------------------------------------------------------------

/// POST /api/builtin-llm/download — body `{ "model_id"?: string }`, default
/// model when omitted.
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

    let lock = download_lock();
    // OwnedMutexGuard: owns a clone of the Arc, so the guard can be moved into
    // the 'static background task (a borrowed guard would not live that long).
    let Ok(guard) = lock.clone().try_lock_owned() else {
        return ok(json!({ "started": false, "already_running": true }));
    };

    let model_id = payload.and_then(|Json| {
        Json.0
            .get("model_id")
            .and_then(|v| v.as_str())
            .map(str::to_string)
    });
    let def = resolve_model(model_id.as_deref())?;
    let (url, sha, file_name) = resolve_source(&cfg, def);
    let dest = models_dir(&state.data_dir)
        .join(&def.manifest.id)
        .join(&file_name);

    tracing::info!(
        model = %def.manifest.id,
        url = %url,
        dest = %dest.display(),
        "builtin llm: starting model download"
    );

    DL_ACTIVE.store(true, Ordering::SeqCst);
    DL_DOWNLOADED.store(0, Ordering::SeqCst);
    DL_TOTAL.store(0, Ordering::SeqCst);
    *dl_model().lock().unwrap() = def.manifest.id.clone();

    let state_for_task = state.clone();
    let cfg_for_task = cfg.clone();
    let model_id_task = def.manifest.id.clone();
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
            &model_id_task,
            &dest,
            &url,
            &sha,
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

async fn run_builtin_download(
    state: &crate::server::types::ServerState,
    cfg: &BuiltinConfig,
    model_id: &str,
    dest: &Path,
    url: &str,
    sha: &str,
    _active: DownloadActiveGuard,
) -> anyhow::Result<()> {
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
    let last_publish_ms = Arc::new(AtomicU64::new(0));
    let on_progress = move |p: DownloadProgress| {
        DL_DOWNLOADED.store(p.downloaded, Ordering::SeqCst);
        DL_TOTAL.store(p.total.unwrap_or(0), Ordering::SeqCst);
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
                downloaded: p.downloaded,
                total: p.total,
                status: "downloading".to_string(),
                error: None,
            });
        }
    };

    let result = download_with_resume(&client, url, dest, sha, Some(&on_progress)).await;

    match result {
        Ok(_) => {
            // Persist the manifest so bootstrap/status know the model exists.
            let mdir = models_dir(&state.data_dir);
            let manifest = ModelManifest {
                id: model_id.to_string(),
                version: "1.0".to_string(),
                file_name: dest
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| model_id.to_string()),
                sha256: sha.to_string(),
                quant: "q4_k_m".to_string(),
            };
            save_manifest(&mdir, &manifest)?;

            // Single-model invariant: the builtin server runs ONE llama-server
            // and installed_model() picks by registry order — a leftover model
            // from an earlier download would win over what the user just
            // chose. Remove every other builtin model dir.
            for other in BUILTIN_MODELS.iter() {
                if other.manifest.id != model_id {
                    let dir = mdir.join(&other.manifest.id);
                    if dir.exists() {
                        match std::fs::remove_dir_all(&dir) {
                            Ok(_) => tracing::info!(
                                model = %other.manifest.id,
                                "builtin llm: removed other model (single-model invariant)"
                            ),
                            Err(e) => tracing::warn!(
                                error = %e,
                                model = %other.manifest.id,
                                "builtin llm: failed to remove other model dir"
                            ),
                        }
                    }
                }
            }

            // If a server is still running the PREVIOUS model (its files were
            // just removed above), it owns the port — a fresh spawn would die
            // on bind while health-checks pass against the stale server
            // (port misattribution). Free the port first.
            if health_check(cfg.port).await {
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
            if let Ok(manager) = get_instance_manager() {
                match spawn_builtin_server(&state.data_dir, cfg, &manager).await {
                    Ok(endpoint) => {
                        tracing::info!(endpoint = %endpoint, "builtin llm: server started after download")
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "builtin llm: server spawn after download failed (run /restart)")
                    }
                }
            }
            Ok(())
        }
        Err(e) => {
            if let Some(bus) = &bus {
                bus.publish_sync(NeoMindEvent::ModelDownloadProgress {
                    model_id: model_id.to_string(),
                    downloaded: DL_DOWNLOADED.load(Ordering::SeqCst),
                    total: if DL_TOTAL.load(Ordering::SeqCst) > 0 {
                        Some(DL_TOTAL.load(Ordering::SeqCst))
                    } else {
                        None
                    },
                    status: "error".to_string(),
                    error: Some(e.to_string()),
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
) -> anyhow::Result<String> {
    let binary = ensure_llama_server(data_dir)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    let mdir = models_dir(data_dir);
    let (def, installed_manifest) = installed_model(&mdir)
        .ok_or_else(|| anyhow::anyhow!("model not installed (no manifest)"))?;
    let model_path = cfg
        .model_path
        .clone()
        .unwrap_or_else(|| installed_manifest.model_path(&mdir));
    if !model_path.exists() {
        anyhow::bail!("model file missing at {}", model_path.display());
    }

    // Per-model context: LFM runs its native 128K (cheap hybrid KV), other
    // models default to 32K so their KV fits comfortably — an explicit
    // override (NEOMIND_BUILTIN_LLM_CTX / restart ?ctx=) wins.
    let ctx = cfg.effective_ctx(def.default_ctx);
    let server_cfg = LlamaServerConfig {
        binary,
        model: model_path,
        port: cfg.port,
        ctx,
        ngl: cfg.ngl,
        threads: None,
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
            def.display_name.to_string(),
            LlmBackendType::LlamaCpp,
        ),
    };
    instance.is_builtin = true;
    // LFM2.5's thinking is integral (cannot be turned off); Qwen3.5 runs
    // non-thinking by default (faster + strongest tool-calling eval); Gemma
    // keeps its default thinking.
    instance.thinking_is_integral = def.manifest.id == BUILTIN_MODEL_ID;
    instance.thinking_enabled = def.default_thinking;
    instance.endpoint = Some(endpoint.clone());
    instance.model = def.manifest.id.clone();
    // Capabilities must be set here — the startup capability-refresh loop ran
    // before this instance existed, so a default (max_context=4096) would
    // surface as a tiny chat context window. Report the ctx we actually
    // spawned with (LFM 128K / Qwen/Gemma 32K).
    instance.capabilities.supports_streaming = true;
    instance.capabilities.supports_tools = true;
    instance.capabilities.supports_thinking = def.default_thinking;
    instance.capabilities.max_context = ctx as usize;
    let _ = manager.upsert_instance(instance).await;

    // 活跃策略:仅当没有任何活跃后端时设为活跃(「有后端不抢」)。
    if manager.get_active_instance().is_none() {
        let _ = manager.set_active(BUILTIN_INSTANCE_ID).await;
    }

    Ok(endpoint)
}

/// Best-effort kill of whatever process listens on `port` (unix only).
///
/// Used to free a wedged llama-server port before a fresh spawn. Relies on
/// `lsof`, which is present on macOS and most Linux distros; if unavailable
/// the subsequent spawn simply reports "address in use".
#[cfg(unix)]
fn kill_process_on_port(port: u16) {
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
fn kill_process_on_port(_port: u16) {}

// ---------------------------------------------------------------------------
// Delete
// ---------------------------------------------------------------------------

/// DELETE /api/builtin-llm/model
pub async fn delete_model_handler(
    State(state): State<crate::server::types::ServerState>,
) -> HandlerResult<serde_json::Value> {
    let cfg = BuiltinConfig::from_env();

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
    if let Ok(manager) = get_instance_manager() {
        if manager.get_instance(BUILTIN_INSTANCE_ID).is_some() {
            let _ = manager.remove_instance(BUILTIN_INSTANCE_ID).await;
        }
    }

    let model_id = installed_model(&models_dir(&state.data_dir))
        .map(|(_, m)| m.id)
        .unwrap_or_else(|| BUILTIN_MODEL_ID.to_string());
    let model_dir = models_dir(&state.data_dir).join(&model_id);
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
    }

    ok(json!({ "deleted": deleted }))
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
pub async fn restart_handler(
    State(state): State<crate::server::types::ServerState>,
    Query(params): Query<HashMap<String, String>>,
) -> HandlerResult<serde_json::Value> {
    // Optional ctx override (?ctx=N): applied for this spawn and persisted in
    // the process env so subsequent bootstraps keep it (matches the port
    // override pattern). Validated to a sane window; 0/garbage → 400.
    let mut cfg = BuiltinConfig::from_env();
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
                } else {
                    std::env::set_var("NEOMIND_BUILTIN_LLM_CTX", n.to_string());
                    cfg.ctx = Some(n);
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

    // A healthy server is normally left alone — EXCEPT when a ctx override
    // was requested that differs from the running server's actual n_ctx:
    // applying a new context requires a respawn.
    let current_n_ctx = query_server_n_ctx(cfg.port).await;
    let ctx_change_requested = params
        .get("ctx")
        .and_then(|v| v.trim().parse::<usize>().ok())
        .map(|want| current_n_ctx.map(|cur| cur != want).unwrap_or(true))
        .unwrap_or(false);
    if health_check(cfg.port).await && !ctx_change_requested {
        return ok(json!({
            "restarted": true,
            "already_running": true,
            "endpoint": format!("http://127.0.0.1:{}", cfg.port),
        }));
    }

    // Free a possibly-wedged port, then spawn fresh.
    kill_process_on_port(cfg.port);
    tokio::time::sleep(Duration::from_millis(300)).await;

    let manager = get_manager()?;
    match spawn_builtin_server(&state.data_dir, &cfg, &manager).await {
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
            ok(json!({ "restarted": true, "already_running": false, "endpoint": endpoint }))
        }
        Err(e) => Err(ErrorResponse::internal(format!(
            "builtin LLM restart failed: {}",
            e
        ))),
    }
}

// ---------------------------------------------------------------------------
// Activate
// ---------------------------------------------------------------------------

/// POST /api/builtin-llm/activate
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

#[cfg(test)]
mod tests {
    use super::*;
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
        static LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
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
