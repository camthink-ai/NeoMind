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
}

pub struct LlamaServerProcess {
    pub port: u16,
    child: tokio::process::Child,
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
            .arg("--nobrowser")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
        if let Some(n) = cfg.ngl {
            cmd.arg("-ngl").arg(n.to_string());
        }
        if let Some(t) = cfg.threads {
            cmd.arg("-t").arg(t.to_string());
        }
        let child = cmd.spawn()?;
        Ok(LlamaServerProcess {
            port: cfg.port,
            child,
        })
    }

    pub async fn wait_healthy(&mut self, timeout: Duration) -> anyhow::Result<()> {
        if wait_healthy_loop(self.port, timeout).await {
            Ok(())
        } else {
            anyhow::bail!(
                "llama-server on :{} did not become healthy in {:?}",
                self.port,
                timeout
            )
        }
    }

    pub async fn stop(mut self) -> anyhow::Result<()> {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
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
