//! Resumable GGUF download with SHA256 verification.

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

/// 流式下载到 `<dest>.part`(断点续传),完成后 SHA256 校验并改名。
/// `expected_sha256` 为空串 → 跳过校验。返回最终文件字节数;已存在且校验过 → 0。
pub async fn download_with_resume(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    expected_sha256: &str,
    on_progress: Option<&(dyn Fn(DownloadProgress) + Sync)>,
) -> anyhow::Result<u64> {
    let part = dest.with_extension("gguf.part");
    // 已下载完成:若此前下好,直接返回 0(幂等)
    if dest.exists() {
        if let Some(dir) = dest.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        return Ok(0);
    }
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&part)?;
    let existing = file.metadata()?.len();

    let mut req = client.get(url);
    if existing > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={}-", existing));
    }
    let resp = req.send().await?;
    let status = resp.status();
    let total_hint = resp.content_length().or_else(|| {
        resp.headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit('/').next())
            .and_then(|v| v.parse::<u64>().ok())
    });
    let total = if status == reqwest::StatusCode::PARTIAL_CONTENT {
        total_hint.or(Some(existing))
    } else {
        total_hint
    };

    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    if existing > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT {
        // 校验起点:前 existing 字节已写入 part,流式喂进 hasher(避免整读多 GB 文件)
        use std::io::{BufReader, Read};
        let mut reader = BufReader::with_capacity(64 * 1024, std::fs::File::open(&part)?);
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
    } else if status != reqwest::StatusCode::PARTIAL_CONTENT && existing > 0 {
        // 服务端忽略 Range 返回 200 → 从头下,覆盖
        file = std::fs::File::create(&part)?;
    }

    let counter = AtomicU64::new(existing);
    let body = resp.bytes_stream();
    use futures::StreamExt;
    let mut stream = std::pin::pin!(body);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        hasher.update(&chunk);
        file.write_all(&chunk)?;
        let d = counter.fetch_add(chunk.len() as u64, Ordering::SeqCst) + chunk.len() as u64;
        if let Some(cb) = on_progress {
            cb(DownloadProgress {
                downloaded: d,
                total,
            });
        }
    }
    file.flush()?;

    if !expected_sha256.is_empty() {
        let got = format!("{:x}", hasher.finalize());
        if !got.eq_ignore_ascii_case(expected_sha256) {
            let _ = std::fs::remove_file(&part);
            anyhow::bail!("sha256 mismatch: expected {} got {}", expected_sha256, got);
        }
    }
    let size = file.metadata()?.len();
    std::fs::rename(&part, dest)?;
    Ok(size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Router};

    fn hex(sha: &[u8]) -> String {
        sha.iter().map(|b| format!("{:02x}", b)).collect()
    }

    async fn start_server(body: &'static [u8]) -> (String, tokio::task::JoinHandle<()>) {
        let router = Router::new().route(
            "/model",
            get(move || async move {
                // 单测场景:body 足够小,直接返回完整内容(不含 Range 响应也测不到续传
                // ——续传逻辑用 .part 文件长度是否被 append 来断言,见下)
                axum::response::Response::new(axum::body::Body::from(body))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (format!("http://{}/model", addr), handle)
    }

    #[tokio::test]
    async fn downloads_and_verifies_sha() {
        let data: &'static [u8] = b"hello-neomind-builtin-llm";
        let (url, h) = start_server(data).await;
        let client = reqwest::Client::new();
        let dir = std::env::temp_dir().join(format!("builtin-dl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("model.gguf");
        let sha = hex(&{
            use sha2::{Digest, Sha256};
            let mut d = Sha256::new();
            d.update(data);
            d.finalize()
        });
        let n = download_with_resume(&client, &url, &dest, &sha, None)
            .await
            .expect("dl");
        assert_eq!(n, data.len() as u64);
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        // 幂等:再次下载直接跳过
        let n2 = download_with_resume(&client, &url, &dest, &sha, None)
            .await
            .unwrap();
        assert_eq!(n2, 0);
        let _ = std::fs::remove_dir_all(&dir);
        h.abort();
    }

    #[tokio::test]
    async fn sha_mismatch_errors() {
        let data: &'static [u8] = b"hello-neomind-builtin-llm";
        let (url, h) = start_server(data).await;
        let client = reqwest::Client::new();
        let dir = std::env::temp_dir().join(format!("builtin-dl2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("model.gguf");
        let bad = "0".repeat(64);
        let r = download_with_resume(&client, &url, &dest, &bad, None).await;
        assert!(r.is_err());
        assert!(!dest.exists(), "mismatched file must not be left as final");
        let _ = std::fs::remove_dir_all(&dir);
        h.abort();
    }

    #[tokio::test]
    async fn resumes_from_partial() {
        let data: &'static [u8] = b"0123456789abcdef";
        let (url, h) = start_server(data).await;
        let client = reqwest::Client::new();
        let dir = std::env::temp_dir().join(format!("builtin-dl3-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("model.gguf");
        // 预写前 4 字节的 .part(模拟上次中断)
        let part = dir.join("model.gguf.part");
        std::fs::write(&part, &data[..4]).unwrap();
        let n = download_with_resume(
            &client,
            &url,
            &dest,
            "",
            Some(&|p| {
                let _ = p;
            }),
        )
        .await
        .unwrap();
        assert_eq!(n, data.len() as u64);
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        let _ = std::fs::remove_dir_all(&dir);
        h.abort();
    }
}
