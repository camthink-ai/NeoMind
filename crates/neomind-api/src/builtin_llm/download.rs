//! Resumable GGUF download with SHA256 verification.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::AsyncWriteExt;

// User-cancel flag for the active download. Checked between chunks; the
// partial .part file is KEPT (resume semantics) so re-downloading the same
// model continues where it left off.
static DOWNLOAD_CANCELLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Request cancellation of the in-flight download (no-op when idle).
pub fn request_download_cancel() {
    DOWNLOAD_CANCELLED.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Whether cancellation was requested (the download loop also polls this).
pub fn download_cancel_requested() -> bool {
    DOWNLOAD_CANCELLED.load(std::sync::atomic::Ordering::SeqCst)
}

/// Clear the flag — called when a NEW download starts, so a stale cancel
/// request from a previous run doesn't kill the fresh one.
pub fn clear_download_cancel() {
    DOWNLOAD_CANCELLED.store(false, std::sync::atomic::Ordering::SeqCst);
}

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

    let mut existing = tokio::fs::metadata(&part)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    let mut req = client.get(url);
    if existing > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={}-", existing));
    }
    let mut resp = req.send().await?;
    let mut status = resp.status();

    // Handle HTTP 416 Range Not Satisfiable explicitly so a `.part` that
    // already holds the complete file (crash right before rename) or outlived
    // a replaced remote artifact does not permanently wedge every future retry.
    if existing > 0 && status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        let remote_total = resp
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit('/').next())
            .and_then(|v| v.parse::<u64>().ok());

        if remote_total == Some(existing) {
            let part_for_hash = part.clone();
            let actual = tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
                use sha2::{Digest, Sha256};
                use std::io::{BufReader, Read};
                let mut hasher = Sha256::new();
                let mut reader =
                    BufReader::with_capacity(64 * 1024, std::fs::File::open(&part_for_hash)?);
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    let n = reader.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    hasher.update(&buf[..n]);
                }
                Ok(hex::encode(hasher.finalize()))
            })
            .await??;

            if expected_sha256.is_empty() || actual.eq_ignore_ascii_case(expected_sha256) {
                tokio::fs::rename(&part, dest).await?;
                if let Some(cb) = on_progress {
                    cb(DownloadProgress {
                        downloaded: existing,
                        total: Some(existing),
                    });
                }
                return Ok(existing);
            }
        }

        // Stale/oversized/corrupt `.part` — remove it and retry cleanly from byte 0.
        let _ = tokio::fs::remove_file(&part).await;
        existing = 0;
        resp = client.get(url).send().await?;
        status = resp.status();
    }

    if !status.is_success() {
        anyhow::bail!(
            "download failed with HTTP {} for {}",
            status,
            url
        );
    }
    if existing > 0
        && status != reqwest::StatusCode::PARTIAL_CONTENT
        && status != reqwest::StatusCode::OK
    {
        anyhow::bail!(
            "unexpected HTTP {} while resuming download for {}",
            status,
            url
        );
    }

    // Total size for progress. On a 206 (resumed) response `content_length()`
    // is the REMAINING bytes, not the full file — without fixing it the bar
    // reports 100% as soon as downloaded surpasses that remaining amount.
    let total_hint = if status == reqwest::StatusCode::PARTIAL_CONTENT {
        resp.content_length().map(|rem| existing + rem).or_else(|| {
            resp.headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.rsplit('/').next())
                .and_then(|v| v.parse::<u64>().ok())
        })
    } else {
        resp.content_length()
    };
    let total = if status == reqwest::StatusCode::PARTIAL_CONTENT {
        total_hint.or(Some(existing))
    } else {
        total_hint
    };

    use sha2::{Digest, Sha256};
    let (mut file, mut hasher, initial_downloaded) =
        if existing > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT {
            // 校验起点:前 existing 字节已写入 part,流式喂进 hasher(避免整读多 GB 文件),
            // 放到 spawn_blocking 避免阻塞 async worker 线程。
            let part_for_hash = part.clone();
            let hasher = tokio::task::spawn_blocking(move || -> anyhow::Result<Sha256> {
                use std::io::{BufReader, Read};
                let mut hasher = Sha256::new();
                let mut reader =
                    BufReader::with_capacity(64 * 1024, std::fs::File::open(&part_for_hash)?);
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    let n = reader.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    hasher.update(&buf[..n]);
                }
                Ok(hasher)
            })
            .await??;
            let file = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&part)
                .await?;
            (file, hasher, existing)
        } else {
            // 服务端忽略 Range 返回 200 OK(或首次下载) → 仅在 200 OK 时截断重写,
            // 并将进度计数重置为 0。
            let file = tokio::fs::File::create(&part).await?;
            (file, Sha256::new(), 0)
        };

    let counter = AtomicU64::new(initial_downloaded);
    let body = resp.bytes_stream();
    use futures::StreamExt;
    let mut stream = std::pin::pin!(body);
    // Per-chunk stall timeout: the client has NO total timeout (a multi-GB
    // model legitimately streams for hours), but without any bound a wedged
    // connection parked stream.next() forever — the cancel flag (polled only
    // between chunks) never got a chance and the single-flight lock was held
    // indefinitely.
    const STALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
    loop {
        let chunk = match tokio::time::timeout(STALL_TIMEOUT, stream.next()).await {
            Ok(c) => c,
            Err(_) => {
                let _ = file.flush().await;
                anyhow::bail!("download stalled: no data for 60s");
            }
        };
        let Some(chunk) = chunk else { break };
        if download_cancel_requested() {
            // Keep the partial file — resume on next attempt.
            let _ = file.flush().await;
            anyhow::bail!("cancelled by user");
        }
        let chunk = chunk?;
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        let d = counter.fetch_add(chunk.len() as u64, Ordering::SeqCst) + chunk.len() as u64;
        if let Some(cb) = on_progress {
            cb(DownloadProgress {
                downloaded: d,
                total,
            });
        }
    }
    file.flush().await?;

    if !expected_sha256.is_empty() {
        let got = format!("{:x}", hasher.finalize());
        if !got.eq_ignore_ascii_case(expected_sha256) {
            drop(file);
            let _ = tokio::fs::remove_file(&part).await;
            anyhow::bail!("sha256 mismatch: expected {} got {}", expected_sha256, got);
        }
    }
    let size = file.metadata().await?.len();
    drop(file);
    tokio::fs::rename(&part, dest).await?;
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

    #[tokio::test]
    async fn http_error_fails_without_truncating_existing_part() {
        let router = Router::new().route(
            "/err",
            get(|| async {
                (
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    "service unavailable error page",
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let h = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let client = reqwest::Client::new();
        let dir = std::env::temp_dir().join(format!("builtin-dl4-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("model.gguf");
        let part = dir.join("model.gguf.part");
        std::fs::write(&part, b"existing-partial-data").unwrap();

        let url = format!("http://{}/err", addr);
        let err = download_with_resume(&client, &url, &dest, "", None)
            .await
            .expect_err("non-2xx must fail");
        assert!(err.to_string().contains("503"), "got: {err}");
        assert!(!dest.exists(), "must not create dest on HTTP error");
        assert_eq!(
            std::fs::read(&part).unwrap(),
            b"existing-partial-data",
            "existing .part must not be truncated or overwritten on 4xx/5xx"
        );
        let _ = std::fs::remove_dir_all(&dir);
        h.abort();
    }

    #[tokio::test]
    async fn http_416_recovers_complete_or_oversized_part() {
        let data: &'static [u8] = b"complete-model-payload";
        let router = Router::new().route(
            "/m",
            get(move |headers: axum::http::HeaderMap| async move {
                if headers.contains_key(axum::http::header::RANGE) {
                    axum::http::Response::builder()
                        .status(axum::http::StatusCode::RANGE_NOT_SATISFIABLE)
                        .header(
                            axum::http::header::CONTENT_RANGE,
                            format!("bytes */{}", data.len()),
                        )
                        .body(axum::body::Body::empty())
                        .unwrap()
                } else {
                    axum::http::Response::builder()
                        .status(axum::http::StatusCode::OK)
                        .body(axum::body::Body::from(data))
                        .unwrap()
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let h = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let client = reqwest::Client::new();
        let url = format!("http://{}/m", addr);

        // Case 1: `.part` already holds the complete file -> completes via rename.
        let dir1 = std::env::temp_dir().join(format!("builtin-dl416a-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir1);
        std::fs::create_dir_all(&dir1).unwrap();
        let dest1 = dir1.join("model.gguf");
        std::fs::write(dir1.join("model.gguf.part"), data).unwrap();
        let n1 = download_with_resume(&client, &url, &dest1, "", None)
            .await
            .unwrap();
        assert_eq!(n1, data.len() as u64);
        assert_eq!(std::fs::read(&dest1).unwrap(), data);
        let _ = std::fs::remove_dir_all(&dir1);

        // Case 2: `.part` is oversized/stale -> removes `.part` and restarts cleanly.
        let dir2 = std::env::temp_dir().join(format!("builtin-dl416b-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir2);
        std::fs::create_dir_all(&dir2).unwrap();
        let dest2 = dir2.join("model.gguf");
        std::fs::write(
            dir2.join("model.gguf.part"),
            b"stale-oversized-partial-file-longer-than-remote",
        )
        .unwrap();
        let n2 = download_with_resume(&client, &url, &dest2, "", None)
            .await
            .unwrap();
        assert_eq!(n2, data.len() as u64);
        assert_eq!(std::fs::read(&dest2).unwrap(), data);
        let _ = std::fs::remove_dir_all(&dir2);

        h.abort();
    }

    #[tokio::test]
    async fn full_200_response_resets_progress_counter_to_zero() {
        let data: &'static [u8] = b"0123456789abcdef";
        let (url, h) = start_server(data).await;
        let client = reqwest::Client::new();
        let dir = std::env::temp_dir().join(format!("builtin-dl5-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("model.gguf");
        let part = dir.join("model.gguf.part");
        std::fs::write(&part, b"stale-prefix-1234").unwrap();

        let max_seen = AtomicU64::new(0);
        let n = download_with_resume(
            &client,
            &url,
            &dest,
            "",
            Some(&|p| {
                max_seen.fetch_max(p.downloaded, Ordering::SeqCst);
            }),
        )
        .await
        .unwrap();
        assert_eq!(n, data.len() as u64);
        assert_eq!(
            max_seen.load(Ordering::SeqCst),
            data.len() as u64,
            "200 OK must count from 0, not from stale .part size"
        );
        let _ = std::fs::remove_dir_all(&dir);
        h.abort();
    }
}
