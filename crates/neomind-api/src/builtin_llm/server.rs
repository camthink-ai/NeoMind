//! Spawn + health-poll the bundled llama-server process.

use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct LlamaServerConfig {
    pub binary: PathBuf,
    pub model: PathBuf,
    pub port: u16,
    pub ctx: usize,
    pub ngl: Option<u16>,
    pub threads: Option<usize>,
    /// Per-model sampling defaults (official/model-card tuned). Passed as
    /// server-side defaults (--temp/--top-p/--top-k): any request that omits
    /// sampling gets the model's best-known point instead of the llama.cpp
    /// generic default.
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<u32>,
}

#[derive(Clone)]
pub struct LlamaServerProcess {
    pub port: u16,
    child: std::sync::Arc<tokio::sync::Mutex<tokio::process::Child>>,
}

/// Live llama-server handles spawned by this process (for graceful stop).
static LLAMA_SERVERS: std::sync::Mutex<Vec<LlamaServerProcess>> = std::sync::Mutex::new(Vec::new());

/// Stop every llama-server this process spawned. Called on graceful
/// shutdown; `kill_on_drop` covers the abnormal paths (crash, SIGKILL).
/// Idempotent.
pub fn stop_all_llama_servers() {
    let mut guard = LLAMA_SERVERS.lock().unwrap_or_else(|e| e.into_inner());
    let servers: Vec<LlamaServerProcess> = std::mem::take(&mut *guard);
    for s in servers {
        if let Ok(mut child) = s.child.try_lock() {
            // best-effort: kill_on_drop remains the backstop if locked
            let _ = child.start_kill();
        }
    }
}

pub async fn health_check(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{}/health", port);
    match reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(2))
        .send()
        .await
    {
        Ok(r) => r.status().is_success(),
        Err(_) => false,
    }
}

/// 循环探测直到健康或超时。供 wait_healthy 与测试共用。
pub async fn wait_healthy_loop(port: u16, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if health_check(port).await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    false
}

/// Like [`wait_healthy_loop`], but ALSO returns early (false) when OUR child
/// exits before the port answers — the classic bind-failure case: a foreign
/// server already holds the port, the health probe succeeds against IT while
/// our llama-server died silently. Without the child check, `wait_healthy`
/// reported success against a server that was never ours (the port
/// misattribution the callers then guard with is_alive — a guard with a
/// startup race: a slow-starting child hasn't attempted the bind yet, so
/// try_wait() still says "running" and the misattribution slips through).
async fn wait_healthy_loop_checking_child(
    port: u16,
    timeout: Duration,
    child: &mut tokio::process::Child,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        // Child died before the port answered → bind failure (or crash):
        // fail immediately, never "succeed" against a foreign listener.
        if let Ok(Some(_)) = child.try_wait() {
            return false;
        }
        if health_check(port).await {
            // Settle window: a healthy answer may come from a FOREIGN
            // listener while our child is still starting and about to die
            // on bind (python3-style startup can take hundreds of ms). Stay
            // in the window long enough that a bind failure would surface
            // (~500 ms), re-checking both child liveness and the port.
            let settle = tokio::time::Instant::now() + Duration::from_millis(500);
            let mut died = false;
            while tokio::time::Instant::now() < settle {
                if let Ok(Some(_)) = child.try_wait() {
                    died = true;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            if died {
                return false;
            }
            // Our child survived the settle window with the port healthy —
            // accept. (A legit llama-server never exits this fast.)
            return true;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    false
}

impl LlamaServerProcess {
    pub fn spawn(cfg: &LlamaServerConfig) -> anyhow::Result<Self> {
        let mut cmd = tokio::process::Command::new(&cfg.binary);
        cmd.arg("-m")
            .arg(&cfg.model)
            .arg("-c")
            .arg(cfg.ctx.to_string())
            .arg("--port")
            .arg(cfg.port.to_string())
            .arg("--host")
            .arg("127.0.0.1")
            // `--no-webui`(非旧名 `--nobrowser`,当前 llama.cpp 已改名,旧参数报 invalid argument)
            .arg("--no-webui")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if let Some(n) = cfg.ngl {
            cmd.arg("-ngl").arg(n.to_string());
        }
        if let Some(t) = cfg.threads {
            cmd.arg("-t").arg(t.to_string());
        }
        if let Some(t) = cfg.temperature {
            cmd.arg("--temp").arg(format!("{t:.2}"));
        }
        if let Some(p) = cfg.top_p {
            cmd.arg("--top-p").arg(format!("{p:.2}"));
        }
        if let Some(k) = cfg.top_k {
            cmd.arg("--top-k").arg(k.to_string());
        }
        // [deterministic cleanup] The process must die with the server even
        // on abnormal termination (crash, kill -9, force-quit): without
        // kill_on_drop, a dropped handle leaked a model-loaded llama-server
        // (~2 GB) until reboot on EVERY restart path that didn't go through
        // an explicit stop(). Combined with the global registry below, the
        // handle is kept alive as long as the server runs, and kill_on_drop
        // makes process exit alone sufficient — SIGKILL included.
        cmd.kill_on_drop(true);
        let child = cmd.spawn()?;
        let proc = LlamaServerProcess {
            port: cfg.port,
            child: std::sync::Arc::new(tokio::sync::Mutex::new(child)),
        };
        // Register globally so a graceful shutdown can stop it explicitly
        // (faster and cleaner than waiting for process-exit reap) — see
        // `stop_all_llama_servers`.
        LLAMA_SERVERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(proc.clone_handle());
        Ok(proc)
    }

    pub async fn wait_healthy(&mut self, timeout: Duration) -> anyhow::Result<()> {
        let mut child = match self.child.try_lock() {
            Ok(c) => c,
            Err(_) => anyhow::bail!("llama-server handle is being stopped concurrently"),
        };
        if wait_healthy_loop_checking_child(self.port, timeout, &mut child).await {
            Ok(())
        } else {
            anyhow::bail!(
                "llama-server on :{} did not become healthy in {:?}",
                self.port,
                timeout
            )
        }
    }

    /// Whether the spawned child process is still alive.
    ///
    /// `wait_healthy` only probes the port — if another server already holds it,
    /// our child dies on bind while the probe succeeds against the foreign
    /// server. Callers must verify `is_alive()` after `wait_healthy` before
    /// trusting the child / registering an instance for it.
    pub fn is_alive(&mut self) -> bool {
        match self.child.try_lock() {
            Ok(mut child) => matches!(child.try_wait(), Ok(None)),
            Err(_) => true, // someone is stopping/waiting it — treat as alive
        }
    }

    /// Handle for the global registry (shares the same child).
    fn clone_handle(&self) -> LlamaServerProcess {
        LlamaServerProcess {
            port: self.port,
            child: self.child.clone(),
        }
    }

    pub async fn stop(self) -> anyhow::Result<()> {
        if let Ok(mut child) = self.child.try_lock() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Router};
    use std::time::Duration;

    #[tokio::test]
    async fn health_check_true_when_server_up() {
        let router = Router::new().route("/health", get(|| async { "ok" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let h = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        // 稍等 server 就绪
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(health_check(addr.port()).await);
        h.abort();
    }

    #[tokio::test]
    async fn health_check_false_when_nothing_listening() {
        // 挑一个几乎肯定没人监听的端口:绑定后立刻释放再测,极小概率冲突
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        assert!(!health_check(port).await);
    }

    #[tokio::test]
    async fn wait_healthy_polls_until_ready() {
        let router = Router::new().route("/health", get(|| async { "ok" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let h = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        // 不构造 LlamaServerProcess(其 child 字段私有、无法手工构造),
        // 直接测公开的 wait_healthy_loop(port, timeout):wait_healthy 只依赖 port。
        assert!(wait_healthy_loop(addr.port(), Duration::from_secs(3)).await);
        h.abort();
    }
}
