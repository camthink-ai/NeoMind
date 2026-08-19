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
use super::server::{health_check, LlamaServerConfig, LlamaServerProcess};

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

    // 幂等:已有 builtin 实例 → 先探测端口。服务器仍健康 → 视为已就绪;
    // 服务器已死(重启后进程不在)→ 不短路,落入下方正常流程重新拉起。
    if manager
        .list_instances()
        .iter()
        .any(|i| i.id == BUILTIN_INSTANCE_ID)
        && health_check(cfg.port).await
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
    // wait_healthy 只证明端口有 /health 响应——可能是占用该端口的其他服务
    // (我们的子进程 bind 失败已退出)。确认 spawn 的子进程还活着,否则注册
    // 会指向别人的服务器,且 kill_process_on_port 会误杀无关进程。
    if !proc.is_alive() {
        let _ = proc.stop().await;
        return BootstrapOutcome::Failed(format!(
            "port {} in use — llama-server exited after bind (another server on that port?)",
            cfg.port
        ));
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Router};
    use neomind_storage::LlmBackendStore;
    use std::sync::Arc;

    fn test_store(tag: &str) -> Arc<LlmBackendStore> {
        let path = std::env::temp_dir().join(format!(
            "neomind-builtin-state-{}-{}.redb",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        LlmBackendStore::open(&path).expect("open test store")
    }

    async fn manager_with_builtin_instance(tag: &str) -> Arc<LlmBackendInstanceManager> {
        let manager = Arc::new(LlmBackendInstanceManager::new(test_store(tag)));
        let inst = LlmBackendInstance::new(
            BUILTIN_INSTANCE_ID.to_string(),
            "LFM2.5-2.6B (内置)".to_string(),
            LlmBackendType::LlamaCpp,
        );
        manager
            .upsert_instance(inst)
            .await
            .expect("upsert builtin instance");
        manager
            .set_active(BUILTIN_INSTANCE_ID)
            .await
            .expect("set builtin active");
        manager
    }

    fn pick_free_port() -> u16 {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    }

    #[tokio::test]
    async fn bootstrap_respawns_when_instance_exists_but_server_dead() {
        // 重启场景:实例记录存在(来自上一次运行)但端口无响应 → 不得短路成
        // ServerAlreadyRunning。必须继续正常流程(测试环境无 bundled binary,
        // 最终 Failed("bundled server missing"),正好证明短路被绕过)。
        let manager = manager_with_builtin_instance("stale").await;
        let cfg = BuiltinConfig {
            port: pick_free_port(),
            ..Default::default()
        };
        let data_dir =
            std::env::temp_dir().join(format!("neomind-builtin-state-dir-{}", std::process::id()));
        let outcome = bootstrap(&data_dir, &cfg, &manager).await;
        assert!(
            !matches!(outcome, BootstrapOutcome::ServerAlreadyRunning),
            "stale instance + dead server must NOT short-circuit (got {:?})",
            outcome
        );
    }

    #[tokio::test]
    async fn bootstrap_returns_already_running_when_server_healthy() {
        // 实例记录存在 + 端口健康 → 幂等短路 ServerAlreadyRunning,且不触碰
        // binary/model(bootstrap 在 find_llama_server 之前返回)。
        let router = Router::new().route("/health", get(|| async { "ok" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let h = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        tokio::time::sleep(Duration::from_millis(100)).await;

        let manager = manager_with_builtin_instance("healthy").await;
        let cfg = BuiltinConfig {
            port: addr.port(),
            ..Default::default()
        };
        let data_dir =
            std::env::temp_dir().join(format!("neomind-builtin-state-dir2-{}", std::process::id()));
        let outcome = bootstrap(&data_dir, &cfg, &manager).await;
        h.abort();
        assert!(
            matches!(outcome, BootstrapOutcome::ServerAlreadyRunning),
            "healthy server with instance record must short-circuit (got {:?})",
            outcome
        );
    }
}
