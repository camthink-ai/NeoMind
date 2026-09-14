//! Builtin LLM bootstrap orchestration: find binary → locate model → spawn →
//! healthy → create/update builtin instance → active policy.
//!
//! Only *locates* an already-downloaded model. If the model is missing it
//! returns `BootstrapOutcome::ModelMissing` so the UI can guide the download;
//! bootstrap never downloads.

use std::path::Path;
use std::time::Duration;

use neomind_agent::llm_backends::LlmBackendInstanceManager;
use neomind_core::builtin_llm::manifest::{
    default_model_def, model_def, BuiltinModelDef, ModelManifest,
};
use neomind_storage::{LlmBackendInstance, LlmBackendType};

use super::config::BuiltinConfig;
use super::handlers::installed_model;
use super::runtime::ensure_llama_server;
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

/// Decide whether a llama-server /props `model_path` belongs to this
/// install (file under our data dir). Canonicalized so symlinks (the
/// persistent smoke env keeps /tmp alive via a symlink) compare correctly.

/// True when an endpoint string points at the given loopback port, e.g.
/// "http://127.0.0.1:8081/v1" for port 8081. Accepts localhost / 127.0.0.1 /
/// [::1] and ignores scheme/path.
fn endpoint_is_loopback_port(endpoint: &str, port: u16) -> bool {
    let rest = endpoint
        .strip_prefix("https://")
        .or_else(|| endpoint.strip_prefix("http://"))
        .unwrap_or(endpoint);
    let host_port = rest.split(['/', '?']).next().unwrap_or("");
    let (host, p) = match host_port.rsplit_once(':') {
        Some(hp) => hp,
        None => return false,
    };
    if p != port.to_string() {
        return false;
    }
    matches!(host, "127.0.0.1" | "localhost" | "[::1]")
}

/// One warn per affected custom backend per process start.
fn warn_custom_backends_on_legacy_port(manager: &LlmBackendInstanceManager) {
    for inst in manager.list_instances() {
        if inst.is_builtin {
            continue;
        }
        if let Some(ep) = inst.endpoint.as_deref() {
            if endpoint_is_loopback_port(ep, 8081) {
                tracing::warn!(
                    category = "llm",
                    backend_id = %inst.id,
                    endpoint = %ep,
                    "Custom backend points at the builtin llama-server's OLD default \
                     port. The builtin now serves on 29375 (or NEOMIND_BUILTIN_LLM_PORT); \
                     update this backend's endpoint or it will fail once the pre-upgrade \
                     process is gone"
                );
            }
        }
    }
}

fn props_model_is_ours(model_path: Option<&str>, data_dir: &Path) -> bool {
    let Some(mp) = model_path else { return false };
    let mp = std::path::Path::new(mp);
    let mp = mp.canonicalize().unwrap_or_else(|_| mp.to_path_buf());
    let dd = data_dir
        .canonicalize()
        .unwrap_or_else(|_| data_dir.to_path_buf());
    mp.starts_with(dd)
}

/// Best-effort reclaim of a llama-server left on the OLD default port by a
/// previous version. Kills ONLY if the listener's reported model path lives
/// under our data dir (an unrelated service on the port is left alone).
async fn reclaim_legacy_llama_port(port: u16, data_dir: &Path) {
    if !super::server::health_check(port).await {
        return; // nothing there — the common case
    }
    let body = match reqwest::Client::new()
        .get(format!("http://127.0.0.1:{}/props", port))
        .timeout(Duration::from_secs(2))
        .send()
        .await
    {
        Ok(r) => r,
        Err(_) => {
            tracing::debug!(
                port,
                "legacy-port listener has no /props — not ours, leaving it"
            );
            return;
        }
    };
    let v: serde_json::Value = match body.json().await {
        Ok(v) => v,
        Err(_) => return,
    };
    let model_path = v.get("model_path").and_then(|m| m.as_str());
    if props_model_is_ours(model_path, data_dir) {
        tracing::info!(
            port,
            "Reclaiming legacy llama-server on the old default port"
        );
        super::handlers::kill_process_on_port(port);
    } else {
        tracing::debug!(
            port,
            "legacy-port listener serves a foreign model — untouched"
        );
    }
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

    // [legacy-port advisory] Custom (non-builtin) backends pointing at the
    // OLD builtin port keep working only while the pre-upgrade orphan lives,
    // then fail with connection-refused after the next reboot — a delayed,
    // hard-to-trace breakage. We deliberately do NOT auto-rewrite: an 8081
    // endpoint may be the user's own llama.cpp instance (loopback-only
    // match narrows it, but proof is impossible). Warn with the fix instead.
    warn_custom_backends_on_legacy_port(manager);

    // 幂等:已有 builtin 实例 → 先探测端口。服务器仍健康 → 视为已就绪;
    // 服务器已死(重启后进程不在)→ 不短路,落入下方正常流程重新拉起。
    if manager
        .list_instances()
        .iter()
        .any(|i| i.id == BUILTIN_INSTANCE_ID)
        && health_check(cfg.port).await
    {
        // Refresh capabilities even on the already-running path — a restart
        // otherwise leaves an instance registered with the storage default
        // max_context (4096) from before this instance existed.
        if let Some(inst) = manager.get_instance(BUILTIN_INSTANCE_ID) {
            let mut updated = inst;
            if let Some(def) = model_def(&updated.model).or_else(|| Some(default_model_def())) {
                updated.capabilities.supports_streaming = true;
                updated.capabilities.supports_tools = true;
                updated.capabilities.supports_thinking = def.default_thinking;
                // Model property — refreshed alongside the rest: the spawn
                // path sets it, so a short-circuited restart used to keep
                // whatever a PREVIOUS build stamped (e.g. Ling imported
                // before the registry field existed).
                updated.thinking_is_integral = def.thinking_is_integral;
                // Record what the server ACTUALLY runs with: an explicit
                // override (NEOMIND_BUILTIN_LLM_CTX / restart API) beats the
                // per-model default — stamping the bare default here used to
                // silently rewrite a raised ctx back down (and the agent's
                // history budget shrank to match the phantom smaller window).
                updated.capabilities.max_context = cfg.effective_ctx(def.default_ctx) as usize;
                let _ = manager.upsert_instance(updated).await;
            }
        }
        return BootstrapOutcome::ServerAlreadyRunning;
    }

    // Model check FIRST: a host with no installed model must not fetch the
    // llama-server runtime — the download belongs to the user's decision to
    // install a builtin model (harmless-but-wrong on bundled-binary hosts,
    // a real uninvited download on source builds).
    let mdir = models_dir(data_dir);
    let Some((def, manifest)): Option<(BuiltinModelDef, ModelManifest)> = installed_model(&mdir)
    else {
        return BootstrapOutcome::ModelMissing;
    };

    // [port migration] Upgrades from the old default (8081) can leave an
    // orphaned llama-server there — our new spawn goes to the new port, the
    // orphan keeps ~2 GB of model RAM hostage until reboot. Reclaim it, but
    // ONLY when we can prove the listener is ours: 8081 is a hot dev port,
    // so /props must report a model file that lives under OUR data dir
    // before kill_process_on_port is allowed to touch it.
    if cfg.port != 8081 {
        reclaim_legacy_llama_port(8081, data_dir).await;
    }

    let binary = match ensure_llama_server(data_dir).await {
        Ok(b) => b,
        Err(e) => return BootstrapOutcome::Failed(format!("llama-server unavailable: {}", e)),
    };
    let model_path = cfg
        .model_path
        .clone()
        .unwrap_or_else(|| manifest.model_path(&mdir));
    if !model_path.exists() {
        return BootstrapOutcome::ModelMissing;
    }

    // spawn + healthy — ctx: explicit override (NEOMIND_BUILTIN_LLM_CTX /
    // restart API) wins over the per-model default.
    let effective_ctx = cfg.effective_ctx(def.default_ctx);
    let server_cfg = LlamaServerConfig {
        binary,
        model: model_path,
        port: cfg.port,
        ctx: effective_ctx,
        ngl: cfg.ngl,
        threads: None,
        // Per-model sampling defaults → server-side --temp/--top-p/--top-k.
        temperature: def.temperature,
        top_p: def.top_p,
        top_k: def.top_k,
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
            def.display_name.to_string(),
            LlmBackendType::LlamaCpp,
        ),
    };
    instance.is_builtin = true;
    // Integral thinking is a MODEL property (LFM's template ignores any
    // reasoning toggle), not "is this the default model" — the default
    // moving off LFM must not flip this flag for LFM installs.
    instance.thinking_is_integral = def.thinking_is_integral;
    instance.thinking_enabled = def.default_thinking;
    instance.endpoint = Some(endpoint.clone());
    instance.model = def.manifest.id.clone();
    // Same as handlers::spawn_builtin_server — the capability refresh loop
    // predates this instance; set the real ctx so chat shows 128K/32K.
    instance.capabilities.supports_streaming = true;
    instance.capabilities.supports_tools = true;
    instance.capabilities.supports_thinking = def.default_thinking;
    instance.capabilities.max_context = effective_ctx;
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

#[cfg(test)]
mod legacy_port_tests {
    use super::*;

    #[test]
    fn props_model_is_ours_bounds() {
        let dir = tempfile::tempdir().unwrap();
        let dd = dir.path().to_path_buf();
        // Inside our data dir → ours. The file must EXIST for canonicalize
        // to resolve macOS's /var → /private/var symlink (a /props
        // model_path always points at a loaded, existing file).
        let model = dd.join("models/minicpm5/file.gguf");
        std::fs::create_dir_all(model.parent().unwrap()).unwrap();
        std::fs::write(&model, b"gguf").unwrap();
        assert!(props_model_is_ours(Some(model.to_str().unwrap()), &dd));
        // Outside → foreign (the common-port innocent-process case).
        assert!(!props_model_is_ours(Some("/opt/other/model.gguf"), &dd));
        // Missing field / wrong type → never kill.
        assert!(!props_model_is_ours(None, &dd));
        assert!(!props_model_is_ours(Some(""), &dd));
    }

    /// Nothing listening on the legacy port → silent no-op (the common case
    /// on fresh installs and already-migrated machines).
    #[tokio::test]
    async fn reclaim_is_noop_when_port_free() {
        // Bind then drop to find a definitely-free port... instead use a
        // port in the dynamic range nothing sane occupies; health_check
        // failing is the contract being exercised.
        let dir = tempfile::tempdir().unwrap();
        reclaim_legacy_llama_port(59999, dir.path()).await; // must not panic
    }
}

#[cfg(test)]
mod legacy_endpoint_tests {
    use super::*;

    #[test]
    fn loopback_port_matching() {
        // The shapes a user actually pastes into a custom backend:
        for ep in [
            "http://127.0.0.1:8081",
            "http://127.0.0.1:8081/v1",
            "http://localhost:8081",
            "http://localhost:8081/v1",
            "http://[::1]:8081",
        ] {
            assert!(endpoint_is_loopback_port(ep, 8081), "should match: {ep}");
        }
        // Non-matches: other ports, non-loopback hosts (a user's own
        // llama.cpp on a LAN box must NOT be flagged), no port.
        for ep in [
            "http://127.0.0.1:29375",
            "http://127.0.0.1:8080",
            "http://192.168.1.5:8081",
            "http://127.0.0.1",
            "ollama",
        ] {
            assert!(
                !endpoint_is_loopback_port(ep, 8081),
                "should NOT match: {ep}"
            );
        }
    }
}
