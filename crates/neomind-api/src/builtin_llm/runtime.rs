//! On-demand llama-server runtime: bundled binary first, else download the
//! official llama.cpp prebuilt for the platform into a versioned cache.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use neomind_core::builtin_llm::find::find_llama_server;
use neomind_core::builtin_llm::runtime::{
    llama_asset_name, llama_server_cache_dir, llama_server_url, LLAMA_CPP_VERSION,
};

/// Single-flight gate so concurrent ensure_llama_server calls don't download
/// the runtime twice.
static RUNTIME_DL: OnceLock<Arc<tokio::sync::Mutex<()>>> = OnceLock::new();
static RUNTIME_DL_ACTIVE: AtomicBool = AtomicBool::new(false);

fn runtime_lock() -> Arc<tokio::sync::Mutex<()>> {
    RUNTIME_DL
        .get_or_init(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

/// Resolve a runnable `neomind-llama-server`:
/// 1. bundled binary (next to the executable, or PATH) — always wins;
/// 2. previously downloaded cache (`data/llama-server/<version>/`);
/// 3. download the official prebuilt tarball for this platform, extract
///    `llama-server`, cache it, and return it.
///
/// Unsupported platforms (no official prebuilt — e.g. Windows, Jetson/CUDA)
/// surface a clear error pointing at the source build.
pub async fn ensure_llama_server(data_dir: &Path) -> Result<PathBuf, String> {
    // Fast path — bundled binary, no network.
    if let Ok(b) = find_llama_server() {
        return Ok(b);
    }

    let cache_dir = llama_server_cache_dir(data_dir);
    let cached = cache_dir.join("neomind-llama-server");
    if cached.exists() {
        return Ok(cached);
    }

    let lock = runtime_lock();
    let _guard = lock.lock().await;
    // Re-check under the lock (another task may have just downloaded it).
    if cached.exists() {
        return Ok(cached);
    }

    let asset = llama_asset_name(std::env::consts::OS, std::env::consts::ARCH).ok_or_else(|| {
        format!(
            "no official llama.cpp prebuilt for {}/{} — bundle neomind-llama-server or build via scripts/build-llama-server.sh",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;
    let url = llama_server_url(asset);
    tracing::info!(url = %url, "builtin llm: downloading llama-server runtime");

    RUNTIME_DL_ACTIVE.store(true, Ordering::SeqCst);
    let result = download_runtime(&url, &cache_dir).await;
    RUNTIME_DL_ACTIVE.store(false, Ordering::SeqCst);
    result
}

/// Whether a runtime download is in flight (for diagnostics).
pub fn runtime_download_active() -> bool {
    RUNTIME_DL_ACTIVE.load(Ordering::SeqCst)
}

async fn download_runtime(url: &str, cache_dir: &Path) -> Result<PathBuf, String> {
    let client = reqwest::Client::new();
    let tmp = std::env::temp_dir().join(format!(
        "neomind-llama-runtime-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| format!("create tmp dir: {e}"))?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("download llama.cpp runtime: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("download llama.cpp runtime: HTTP {}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("read llama.cpp runtime: {e}"))?;

    // Extract `llama-server` from the release tarball.
    let bin = extract_llama_server(&bytes, &tmp).map_err(|e| e.to_string())?;

    std::fs::create_dir_all(cache_dir).map_err(|e| format!("create cache dir: {e}"))?;
    let dest = cache_dir.join("neomind-llama-server");
    std::fs::rename(&bin, &dest).map_err(|e| format!("move runtime into cache: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&dest, perms).map_err(|e| format!("chmod runtime: {e}"))?;
    }
    let _ = std::fs::remove_dir_all(&tmp);

    tracing::info!(dest = %dest.display(), "builtin llm: llama-server runtime ready");
    Ok(dest)
}

fn extract_llama_server(tar_gz: &[u8], out_dir: &Path) -> anyhow::Result<PathBuf> {
    use flate2::read::GzDecoder;
    let gz = GzDecoder::new(tar_gz);
    let mut ar = tar::Archive::new(gz);
    let entries = ar.entries()?;
    let mut found: Option<PathBuf> = None;
    for entry in entries {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        if path.file_name().map(|n| n == "llama-server").unwrap_or(false) {
            let dest = out_dir.join("llama-server");
            entry.unpack(&dest)?;
            found = Some(dest);
            break;
        }
    }
    found.ok_or_else(|| anyhow::anyhow!("llama-server not found in release tarball"))
}

/// Version tag for diagnostics/logging.
pub fn runtime_version() -> &'static str {
    LLAMA_CPP_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_dir_is_versioned() {
        let d = llama_server_cache_dir(std::path::Path::new("/tmp/data"));
        let s = d.to_string_lossy();
        assert!(s.contains("llama-server"));
        assert!(s.ends_with(LLAMA_CPP_VERSION));
    }

    #[test]
    fn extracts_llama_server_from_tarball() {
        // Build a tiny in-memory tar.gz containing a llama-server entry.
        use std::io::Write;
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(5);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, "bin/llama-server", std::io::Cursor::new(b"probe"))
            .unwrap();
        let uncompressed = builder.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&uncompressed).unwrap();
        let tar_gz = gz.finish().unwrap();
        let _ = std::fs::remove_file(std::env::temp_dir().join("llama-probe"));

        let out = std::env::temp_dir().join("llama-extract-test");
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&out).unwrap();
        let bin = extract_llama_server(&tar_gz, &out).expect("extract");
        assert_eq!(bin, out.join("llama-server"));
        assert_eq!(std::fs::read_to_string(&bin).unwrap(), "probe");
        let _ = std::fs::remove_dir_all(&out);
    }
}
