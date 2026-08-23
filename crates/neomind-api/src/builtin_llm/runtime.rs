//! On-demand llama-server runtime: bundled binary first, else download the
//! official llama.cpp prebuilt for the platform into a versioned cache.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use neomind_core::builtin_llm::find::find_llama_server;
use neomind_core::builtin_llm::runtime::{
    llama_asset_is_zip, llama_asset_name, llama_cudart_marker, llama_cudart_url,
    llama_server_bin_name, llama_server_cache_dir, llama_server_url,
    RuntimeVariant, LLAMA_CPP_VERSION,
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
/// 2. previously downloaded cache (`data/llama-server/<version>[-cuda]/`);
/// 3. download the official prebuilt for this platform (and variant), extract
///    `llama-server`, cache it, and return it.
///
/// With `NEOMIND_BUILTIN_RUNTIME_VARIANT=cuda` on Windows x64 the CUDA build
/// is fetched plus the cudart DLL bundle (driver-only hosts). Platforms with
/// no official CUDA prebuilt (Linux/Jetson) surface a clear error naming the
/// env to unset and the source-build script.
pub async fn ensure_llama_server(data_dir: &Path) -> Result<PathBuf, String> {
    // Fast path — bundled binary, no network.
    if let Ok(b) = find_llama_server() {
        return Ok(b);
    }

    let mut variant = RuntimeVariant::from_env();
    match ensure_runtime_variant(data_dir, variant).await {
        Ok(bin) => Ok(bin),
        Err(err) if variant == RuntimeVariant::Cuda => {
            // CUDA is opt-in, but a dead backend helps nobody — degrade to
            // the CPU build (always available) instead of failing the boot.
            // Sticky via env so a restart doesn't re-download CUDA and hit
            // the same wall (same override pattern as port/ctx).
            tracing::warn!(
                error = %err,
                "builtin llm: CUDA runtime unusable ({}) — falling back to the CPU build for this process; unset NEOMIND_BUILTIN_RUNTIME_VARIANT or fix the driver to retry CUDA",
                if llama_asset_name(std::env::consts::OS, std::env::consts::ARCH, variant).is_some() {
                    "download or exec failed"
                } else {
                    "no official prebuilt for this platform"
                }
            );
            std::env::set_var("NEOMIND_BUILTIN_RUNTIME_VARIANT", "cpu");
            variant = RuntimeVariant::Cpu;
            ensure_runtime_variant(data_dir, variant).await
        }
        Err(err) => Err(err),
    }
}

/// Download (or reuse from cache) the runtime for ONE variant — the fallback
/// chain lives in `ensure_llama_server` above.
async fn ensure_runtime_variant(data_dir: &Path, variant: RuntimeVariant) -> Result<PathBuf, String> {
    let cache_dir = llama_server_cache_dir(data_dir, variant);
    // A CUDA cache is only complete with its cudart DLLs — a partial one
    // (crashed between the two downloads) must not short-circuit.
    let cache_complete = |dir: &Path| {
        find_runtime_binary(dir).is_some()
            && (variant == RuntimeVariant::Cpu || dir.join(llama_cudart_marker()).exists())
    };
    if cache_complete(&cache_dir) {
        return Ok(find_runtime_binary(&cache_dir).expect("checked above"));
    }

    let lock = runtime_lock();
    let _guard = lock.lock().await;
    // Re-check under the lock (another task may have just downloaded it).
    if cache_complete(&cache_dir) {
        return Ok(find_runtime_binary(&cache_dir).expect("checked above"));
    }

    let asset = llama_asset_name(std::env::consts::OS, std::env::consts::ARCH, variant)
        .ok_or_else(|| {
            if variant == RuntimeVariant::Cuda {
                format!(
                    "no official llama.cpp CUDA prebuilt for {}/{} — unset \
                         NEOMIND_BUILTIN_RUNTIME_VARIANT to use the CPU build, or build one \
                         via scripts/build-llama-server.sh",
                    std::env::consts::OS,
                    std::env::consts::ARCH
                )
            } else {
                format!(
                    "no official llama.cpp prebuilt for {}/{} — bundle neomind-llama-server \
                         or build via scripts/build-llama-server.sh",
                    std::env::consts::OS,
                    std::env::consts::ARCH
                )
            }
        })?;
    let url = llama_server_url(asset);
    tracing::info!(url = %url, variant = ?variant, "builtin llm: downloading llama-server runtime");

    RUNTIME_DL_ACTIVE.store(true, Ordering::SeqCst);
    // Fresh cache for the primary archive; a cudart bundle (if any) then
    // extracts INTO it without wiping.
    let result = download_runtime(&url, &cache_dir, llama_asset_is_zip(&asset), true).await;
    let result = match result {
        Ok(bin) => match llama_cudart_url(asset) {
            // CUDA builds dlopen cudart/cublas DLLs — the bundle puts them
            // next to llama-server.exe so a driver-only host just works.
            Some(cudart_url) => {
                match download_runtime(&cudart_url, &cache_dir, true, false).await {
                    Ok(_) => {
                        // A CUDA build with no (or too old) NVIDIA driver
                        // fails to load cudart/cublas DLLs — detect that HERE
                        // with a fast `--version` exec so the caller can fall
                        // back to CPU, instead of dying at first spawn.
                        match exec_check(&bin).await {
                            Ok(()) => {
                                let _ = std::fs::write(cache_dir.join(llama_cudart_marker()), b"ok");
                                Ok(bin)
                            }
                            Err(e) => Err(format!("CUDA runtime exec check failed: {e}")),
                        }
                    }
                    Err(e) => Err(e),
                }
            }
            None => Ok(bin),
        },
        Err(e) => Err(e),
    };
    RUNTIME_DL_ACTIVE.store(false, Ordering::SeqCst);
    result
}

/// Whether a runtime download is in flight (for diagnostics).
pub fn runtime_download_active() -> bool {
    RUNTIME_DL_ACTIVE.load(Ordering::SeqCst)
}

/// `wipe` clears the cache dir first — true for the primary archive, false
/// for the cudart bundle (it must land in the freshly populated dir).
async fn download_runtime(
    url: &str,
    cache_dir: &Path,
    is_zip: bool,
    wipe: bool,
) -> Result<PathBuf, String> {
    let client = reqwest::Client::new();
    let tmp = std::env::temp_dir().join(format!("neomind-llama-runtime-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| format!("create tmp dir: {e}"))?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("download llama.cpp runtime: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "download llama.cpp runtime: HTTP {}",
            resp.status()
        ));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("read llama.cpp runtime: {e}"))?;

    // Extract the WHOLE archive into the cache dir — the llama-server binary
    // is a thin wrapper that dlopens sibling libs (macOS libllama-server-impl
    // .dylib, Windows .dll), so a single-binary extract dies on exec.
    if wipe {
        let _ = std::fs::remove_dir_all(cache_dir);
    }
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

/// Fast exec sanity check — `<binary> --version` must exit 0 within 10s.
/// Catches missing/old NVIDIA drivers (cudart DLL load failure) at download
/// time instead of at first spawn.
async fn exec_check(bin: &Path) -> Result<(), String> {
    use tokio::time::{timeout, Duration};
    let out = tokio::process::Command::new(bin)
        .arg("--version")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();
    match timeout(Duration::from_secs(10), out).await {
        Ok(Ok(status)) if status.status.success() => Ok(()),
        Ok(Ok(status)) => Err(format!(
            "exit {}: {}",
            status.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&status.stderr).chars().take(200).collect::<String>()
        )),
        Ok(Err(e)) => Err(format!("spawn: {e}")),
        Err(_) => Err("--version timed out after 10s".to_string()),
    }
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
        let d = llama_server_cache_dir(std::path::Path::new("/tmp/data"), RuntimeVariant::Cpu);
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
            .append_data(
                &mut header,
                "bin/llama-server",
                std::io::Cursor::new(b"probe"),
            )
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
            let mut zw: zip::ZipWriter<&mut std::io::Cursor<Vec<u8>>> =
                zip::ZipWriter::new(&mut zip_buf);
            zw.start_file(
                format!("bin/{}", llama_server_bin_name()),
                zip::write::SimpleFileOptions::default(),
            )
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
