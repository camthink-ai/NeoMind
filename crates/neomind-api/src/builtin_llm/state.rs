//! Builtin LLM bootstrap orchestration: find binary → locate model → spawn →
//! healthy → create/update builtin instance → active policy.
//!
//! Only *locates* an already-downloaded model. If the model is missing it
//! returns `BootstrapOutcome::ModelMissing` so the UI can guide the download;
//! bootstrap never downloads.

use std::path::Path;
use std::time::Duration;

use neomind_agent::llm_backends::LlmBackendInstanceManager;
use neomind_core::builtin_llm::{
    find::find_llama_server,
    manifest::{load_manifest, BUILTIN_MODEL_ID},
};
use neomind_storage::{LlmBackendInstance, LlmBackendType};

use super::config::BuiltinConfig;
use super::server::{LlamaServerConfig, LlamaServerProcess};

/// Stable instance id for the builtin bundled model (survives restarts).
pub const BUILTIN_INSTANCE_ID: &str = "builtin-lfm25-2.6b";

#[derive(Debug)]
pub enum BootstrapOutcome {
    /// Disabled via `NEOMIND_BUILTIN_LLM=off`.
    Disabled,
    /// A builtin instance already exists (idempotent restart) — nothing to do.
    ServerAlreadyRunning,
    /// Bundled server/model not present; UI should offer a guided download.
    ModelMissing,
    /// Server is up and the builtin instance was registered/activated.
    ServerReady { endpoint: String },
    /// Fatal orchestration error (no bundled binary, spawn unhealthy, …).
    Failed(String),
}

fn models_dir(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("models")
}

/// Orchestrate startup of the builtin LFM2.5-2.6B model.
///
/// Idempotent: if a builtin instance already exists (from a previous run), we
/// return `ServerAlreadyRunning` without touching it. Otherwise locate the
/// bundled llama-server + model, spawn, wait for healthy, upsert the builtin
/// instance, and set it active ONLY when no backend is already active
/// ("有后端不抢").
pub async fn bootstrap(
    data_dir: &Path,
    cfg: &BuiltinConfig,
    manager: &LlmBackendInstanceManager,
) -> BootstrapOutcome {
    if !cfg.enabled {
        return BootstrapOutcome::Disabled;
    }

    // 幂等:已有 builtin 实例 → 直接视为已就绪。
    // list_instances/get_instance/get_active_instance 均为同步方法,不 .await。
    if manager
        .list_instances()
        .iter()
        .any(|i| i.id == BUILTIN_INSTANCE_ID)
    {
        return BootstrapOutcome::ServerAlreadyRunning;
    }

    let binary = match find_llama_server() {
        Ok(b) => b,
        Err(e) => return BootstrapOutcome::Failed(format!("bundled server missing: {}", e)),
    };

    let mdir = models_dir(data_dir);
    let manifest = match load_manifest(&mdir, BUILTIN_MODEL_ID) {
        Ok(Some(m)) => m,
        Ok(None) => return BootstrapOutcome::ModelMissing,
        Err(e) => return BootstrapOutcome::Failed(format!("manifest read failed: {}", e)),
    };

    let model_path = cfg
        .model_path
        .clone()
        .unwrap_or_else(|| manifest.model_path(&mdir));
    if !model_path.exists() {
        return BootstrapOutcome::ModelMissing;
    }

    // spawn + healthy
    let server_cfg = LlamaServerConfig {
        binary,
        model: model_path,
        port: cfg.port,
        ctx: cfg.ctx,
        ngl: cfg.ngl,
        threads: None,
    };
    let mut proc = match LlamaServerProcess::spawn(&server_cfg) {
        Ok(p) => p,
        Err(e) => return BootstrapOutcome::Failed(format!("spawn failed: {}", e)),
    };
    if let Err(e) = proc.wait_healthy(Duration::from_secs(60)).await {
        let _ = proc.stop().await;
        return BootstrapOutcome::Failed(format!("server unhealthy: {}", e));
    }
    let endpoint = format!("http://127.0.0.1:{}", cfg.port);

    // 创建/更新 builtin 实例(get_instance 同步)。
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

    BootstrapOutcome::ServerReady { endpoint }
}
