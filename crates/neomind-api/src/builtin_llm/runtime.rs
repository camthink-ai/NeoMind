//! On-demand llama-server runtime: bundled binary first, else download the
//! official llama.cpp prebuilt for the platform into a versioned cache.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use neomind_core::builtin_llm::find::find_llama_server;
use neomind_core::builtin_llm::runtime::{
    llama_asset_is_zip, llama_asset_name, llama_server_bin_name, llama_server_cache_dir,
    llama_server_cache_name, llama_server_url, LLAMA_CPP_VERSION,
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
    if let Some(existing) = find_runtime_binary(&cache_dir) {
        return Ok(existing);
    }

    let lock = runtime_lock();
    let _guard = lock.lock().await;
    // Re-check under the lock (another task may have just downloaded it).
    if let Some(existing) = find_runtime_binary(&cache_dir) {
        return Ok(existing);
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
    let result = download_runtime(&url, &cache_dir, llama_asset_is_zip(&asset)).await;
    RUNTIME_DL_ACTIVE.store(false, Ordering::SeqCst);
    result
}

/// Whether a runtime download is in flight (for diagnostics).
pub fn runtime_download_active() -> bool {
    RUNTIME_DL_ACTIVE.load(Ordering::SeqCst)
}

async fn download_runtime(url: &str, cache_dir: &Path, is_zip: bool) -> Result<PathBuf, String> {
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

    // Extract the WHOLE archive into the cache dir — the llama-server binary
    // is a thin wrapper that dlopens sibling libs (macOS libllama-server-impl
    // .dylib, Windows .dll), so a single-binary extract dies on exec.
    let _ = std::fs::remove_dir_all(cache_dir);
    std::fs::create_dir_all(cache_dir).map_err(|e| format!("create cache dir: {e}"))?;
    extract_archive(&bytes, cache_dir, is_zip).map_err(|e| e.to_string())?;
    let bin = find_runtime_binary(cache_dir)
        .ok_or_else(|| "llama-server not found after extract".to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&bin, perms).map_err(|e| format!("chmod runtime: {e}"))?;
    }
    let _ = std::fs::remove_dir_all(&tmp);

    tracing::info!(dest = %bin.display(), "builtin llm: llama-server runtime ready");
    Ok(bin)
}

/// Recursively locate the llama-server binary inside the cache dir.
fn find_runtime_binary(dir: &Path) -> Option<PathBuf> {
    let name = llama_server_bin_name();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.file_name().map(|n| n == name).unwrap_or(false) {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Extract every entry of the release archive into `out_dir` (paths
/// preserved, so wrapper binaries find their sibling libs).
fn extract_archive(bytes: &[u8], out_dir: &Path, is_zip: bool) -> anyhow::Result<()> {
    if is_zip {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            let rel = entry.name().trim_start_matches('/');
            let dest = out_dir.join(rel);
            if entry.is_dir() {
                std::fs::create_dir_all(&dest)?;
                continue;
            }
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = std::fs::File::create(&dest)?;
            std::io::copy(&mut entry, &mut out)?;
        }
        Ok(())
    } else {
        use flate2::read::GzDecoder;
        let gz = GzDecoder::new(bytes);
        let mut ar = tar::Archive::new(gz);
        ar.unpack(out_dir)?;
        Ok(())
    }
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
        extract_archive(&tar_gz, &out, false).expect("extract");
        let bin = find_runtime_binary(&out).expect("find binary");
        assert_eq!(bin, out.join("bin/llama-server"));
        assert_eq!(std::fs::read_to_string(&bin).unwrap(), "probe");
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn extracts_llama_server_from_zip() {
        use std::io::Write;
        let mut zip_buf = std::io::Cursor::new(Vec::new());
        {
            let mut zw: zip::ZipWriter<&mut std::io::Cursor<Vec<u8>>> = zip::ZipWriter::new(&mut zip_buf);
            zw.start_file(format!("bin/{}", llama_server_bin_name()), zip::write::SimpleFileOptions::default())
                .unwrap();
            zw.write_all(b"probe-exe").unwrap();
            zw.finish().unwrap();
        }
        let out = std::env::temp_dir().join("llama-extract-zip-test");
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&out).unwrap();
        extract_archive(zip_buf.get_ref(), &out, true).expect("extract zip");
        let bin = find_runtime_binary(&out).expect("find binary");
        assert_eq!(bin, out.join(format!("bin/{}", llama_server_bin_name())));
        assert_eq!(std::fs::read_to_string(&bin).unwrap(), "probe-exe");
        let _ = std::fs::remove_dir_all(&out);
    }
}
