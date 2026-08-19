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

use axum::extract::State;
use serde_json::json;

use neomind_agent::llm_backends::{get_instance_manager, LlmBackendInstanceManager};
use neomind_agent::LlmBackend;
use neomind_core::builtin_llm::find::find_llama_server;
use neomind_core::builtin_llm::manifest::{
    load_manifest, save_manifest, ModelManifest, BUILTIN_MODEL_ID,
};
use neomind_core::builtin_llm::variant::{default_quant, model_file_name, Quant};
use neomind_core::extension::accel::detect_variant;
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
    }
}

/// Official LFS SHA256 for each quant (pinned 2026-08-19). LFS blobs are
/// content-addressed, so these stay valid unless the repo owner re-uploads the
/// file under the same name. If a download ever fails with a sha mismatch,
/// re-capture from the HF API and update both arms.
fn hf_sha256(quant: Quant) -> &'static str {
    match quant {
        Quant::Q4_K_M => "79fdf00351b46cf26f020aead28d01889886be87c55fa0eb907e6f9b00bfee14",
        Quant::Q8_0 => "36587fdf27bdfc69caf2637273679a0870ec155162161bde6fd16e8c70bdb757",
    }
}

/// The HF URL for a quant's GGUF.
fn hf_url(quant: Quant) -> String {
    format!("{}/{}", HF_REPO, hf_file_name(quant))
}

/// Resolve the quant to download: user override → `default_quant(os, variant)`.
fn resolve_quant(cfg: &BuiltinConfig) -> Result<Quant, ErrorResponse> {
    match cfg.quant_override.as_deref() {
        Some(q) if q.eq_ignore_ascii_case("q4_k_m") => Ok(Quant::Q4_K_M),
        Some(q) if q.eq_ignore_ascii_case("q8_0") => Ok(Quant::Q8_0),
        Some(other) => Err(ErrorResponse::bad_request(format!(
            "unsupported builtin quant: {}",
            other
        ))),
        None => Ok(default_quant(std::env::consts::OS, detect_variant())),
    }
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
    let (manifest, model_path, installed) = match load_manifest(&mdir, BUILTIN_MODEL_ID) {
        Ok(Some(m)) => {
            let p = cfg
                .model_path
                .clone()
                .unwrap_or_else(|| m.model_path(&mdir));
            let inst = p.exists();
            (Some(m), Some(p), inst)
        }
        Ok(None) => (None, None, false),
        Err(e) => {
            tracing::warn!(error = %e, "builtin llm: manifest read failed");
            return ok(json!({
                "installed": false,
                "model_id": serde_json::Value::Null,
                "server_state": "error",
                "downloaded_bytes": serde_json::Value::Null,
                "total_bytes": serde_json::Value::Null,
            }));
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
        return ok(json!({
            "installed": installed,
            "model_id": BUILTIN_MODEL_ID,
            "server_state": "downloading",
            "downloaded_bytes": downloaded,
            "total_bytes": if total > 0 { json!(total) } else { serde_json::Value::Null },
        }));
    }

    let Some(_manifest) = manifest else {
        return ok(json!({
            "installed": false,
            "model_id": serde_json::Value::Null,
            "server_state": "not_configured",
            "downloaded_bytes": serde_json::Value::Null,
            "total_bytes": serde_json::Value::Null,
        }));
    };

    if !installed {
        return ok(json!({
            "installed": false,
            "model_id": BUILTIN_MODEL_ID,
            "server_state": "not_configured",
            "downloaded_bytes": serde_json::Value::Null,
            "total_bytes": serde_json::Value::Null,
        }));
    }

    let file_size = model_path
        .and_then(|p| std::fs::metadata(&p).ok())
        .map(|m| m.len())
        .unwrap_or(0);
    let server_state = if health_check(cfg.port).await {
        "running"
    } else {
        "stopped"
    };

    ok(json!({
        "installed": true,
        "model_id": BUILTIN_MODEL_ID,
        "server_state": server_state,
        "downloaded_bytes": file_size,
        "total_bytes": file_size,
    }))
}

// ---------------------------------------------------------------------------
// Download
// ---------------------------------------------------------------------------

/// POST /api/builtin-llm/download
pub async fn download_handler(
    State(state): State<crate::server::types::ServerState>,
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

    let quant = resolve_quant(&cfg)?;
    let file_name = model_file_name(quant);
    let url = hf_url(quant);
    let sha = hf_sha256(quant).to_string();
    let dest = models_dir(&state.data_dir)
        .join(BUILTIN_MODEL_ID)
        .join(&file_name);

    tracing::info!(
        url = %url,
        dest = %dest.display(),
        "builtin llm: starting model download"
    );

    DL_ACTIVE.store(true, Ordering::SeqCst);
    DL_DOWNLOADED.store(0, Ordering::SeqCst);
    DL_TOTAL.store(0, Ordering::SeqCst);

    let state_for_task = state.clone();
    let cfg_for_task = cfg.clone();
    tokio::spawn(async move {
        // Hold both guards for the whole download: `guard` single-flights
        // concurrent POSTs, `_active` flips DL_ACTIVE back off on finish/panic.
        let _active = DownloadActiveGuard;
        let _guard = guard;
        match run_builtin_download(&state_for_task, &cfg_for_task, &dest, &url, &sha, quant).await {
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "builtin llm: download failed"),
        }
    });

    ok(json!({ "started": true, "already_running": false }))
}

async fn run_builtin_download(
    state: &crate::server::types::ServerState,
    cfg: &BuiltinConfig,
    dest: &Path,
    url: &str,
    sha: &str,
    quant: Quant,
) -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    let bus = state.event_bus();

    // Clone for the callback — the closure is `move`, so without a clone the
    // outer `bus` would be moved in and unavailable for the complete/error
    // events below.
    let bus_for_cb = bus.clone();
    let on_progress = move |p: DownloadProgress| {
        DL_DOWNLOADED.store(p.downloaded, Ordering::SeqCst);
        DL_TOTAL.store(p.total.unwrap_or(0), Ordering::SeqCst);
        if let Some(bus) = &bus_for_cb {
            bus.publish_sync(NeoMindEvent::ModelDownloadProgress {
                model_id: BUILTIN_MODEL_ID.to_string(),
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
                id: BUILTIN_MODEL_ID.to_string(),
                version: "1.0".to_string(),
                file_name: dest
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| model_file_name(quant)),
                sha256: sha.to_string(),
                quant: quant.key().to_string(),
            };
            save_manifest(&mdir, &manifest)?;

            if let Some(bus) = &bus {
                bus.publish_sync(NeoMindEvent::ModelDownloadProgress {
                    model_id: BUILTIN_MODEL_ID.to_string(),
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
                    model_id: BUILTIN_MODEL_ID.to_string(),
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
    let binary = find_llama_server().map_err(|e| anyhow::anyhow!(e))?;
    let mdir = models_dir(data_dir);
    let manifest = load_manifest(&mdir, BUILTIN_MODEL_ID)?
        .ok_or_else(|| anyhow::anyhow!("model not installed (no manifest)"))?;
    let model_path = cfg
        .model_path
        .clone()
        .unwrap_or_else(|| manifest.model_path(&mdir));
    if !model_path.exists() {
        anyhow::bail!("model file missing at {}", model_path.display());
    }

    let server_cfg = LlamaServerConfig {
        binary,
        model: model_path,
        port: cfg.port,
        ctx: cfg.ctx,
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
            "LFM2.5-2.6B (内置)".to_string(),
            LlmBackendType::LlamaCpp,
        ),
    };
    instance.is_builtin = true;
    instance.thinking_is_integral = true;
    instance.endpoint = Some(endpoint.clone());
    instance.model = BUILTIN_MODEL_ID.to_string();
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

    let model_dir = models_dir(&state.data_dir).join(BUILTIN_MODEL_ID);
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
) -> HandlerResult<serde_json::Value> {
    let cfg = BuiltinConfig::from_env();
    if !cfg.enabled {
        return Err(ErrorResponse::bad_request(
            "builtin LLM disabled (NEOMIND_BUILTIN_LLM=off)",
        ));
    }

    if health_check(cfg.port).await {
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

    async fn status_body(data_dir: PathBuf) -> serde_json::Value {
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
        let Json(resp) = download_handler(State(state)).await.unwrap();
        let data = resp.data.unwrap();
        assert_eq!(data["started"], false);
        assert_eq!(data["already_running"], true);
    }
}
