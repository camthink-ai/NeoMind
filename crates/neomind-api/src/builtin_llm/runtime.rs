//! On-demand llama-server runtime resolution and installation.
//!
//! Resolution is "newest valid wins" (see `runtime_manager::pick_runtime`):
//! among the bundled binary and the pin-floored cache dirs under
//! `data/llama-server/`, the highest llama.cpp tag wins — with the compiled
//! `LLAMA_CPP_VERSION` as the floor and an offline fallback to the bundled
//! binary. The standalone upgrade (`runtime_manager::upgrade_runtime`)
//! installs channel-recommended tags through the same `install_runtime`
//! path used by boot.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use neomind_core::builtin_llm::find::find_llama_server;
use neomind_core::builtin_llm::runtime::{
    llama_asset_is_zip, llama_asset_name, llama_cudart_marker, llama_cudart_url_tagged,
    llama_runtime_expected_sha, llama_server_bin_name, llama_server_cache_dir_for_tag,
    try_llama_server_url_for_tag, RuntimeVariant, LLAMA_CPP_VERSION,
};

use super::runtime_manager::{self, Decision};

/// Single-flight gate so concurrent ensure_llama_server calls don't download
/// the runtime twice (shared with the standalone upgrade flow).
static RUNTIME_DL: OnceLock<Arc<tokio::sync::Mutex<()>>> = OnceLock::new();
static RUNTIME_DL_ACTIVE: AtomicBool = AtomicBool::new(false);

pub(crate) fn runtime_lock() -> Arc<tokio::sync::Mutex<()>> {
    RUNTIME_DL
        .get_or_init(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

/// Resolve a runnable `neomind-llama-server` (newest-valid-wins matrix —
/// see the module doc and `runtime_manager::pick_runtime`):
///
/// 1. newest pin-floored cache dir for the effective variant (ties vs the
///    bundled binary go to the cache; an unknown bundled tag yields);
/// 2. bundled binary when its tag is >= the pin (e.g. right after a
///    platform upgrade);
/// 3. bundled older than the pin → try the pin download first, fall back to
///    the bundled binary when offline (identical to the old behavior);
/// 4. no bundled binary → download the pin runtime (bare `neomind serve`).
///
/// A cache pick is exec-verified once per process; a failure ejects that
/// cache dir and resolution retries with the next candidate. Variant
/// selection: env > persisted setting > auto-detect (Jetson via
/// `/etc/nv_tegra_release`, Linux x64 + qualifying NVIDIA GPU → our
/// self-hosted CUDA build). A CUDA/Vulkan download or exec failure degrades
/// the PROCESS to the CPU build — never persisted, so the user's variant
/// choice survives a transient driver failure.
pub async fn ensure_llama_server(data_dir: &Path) -> Result<PathBuf, String> {
    let mut allow_eject = true;
    loop {
        let variant = runtime_manager::effective_variant();

        // Bundled candidate. On Jetson hosts a PATH/bundled binary may be
        // the OFFICIAL ubuntu-arm64 build, which requires gcc-13 libstdc++
        // (GLIBCXX_3.4.32) — JetPack 6 ships gcc-11, so it fails to exec.
        // Don't trust it; exec-check and drop to the cache/download path.
        let bundled = match find_llama_server() {
            Ok(b) => {
                if variant == RuntimeVariant::Jetson && exec_check(&b).await.is_err() {
                    tracing::warn!(
                        bin = %b.display(),
                        "builtin llm: found llama-server on PATH but it fails to exec on this \
                         Jetson (official ubuntu-arm64 build needs gcc-13?) — using the \
                         Jetson runtime instead"
                    );
                    None
                } else {
                    let tag = runtime_manager::bundled_tag(&b).await;
                    Some((b, tag))
                }
            }
            Err(_) => None,
        };

        let best = runtime_manager::entries_for_variant(data_dir, variant)
            .into_iter()
            .next();

        // Reduce the matrix to either "use this binary" or "download the
        // pin runtime" — verification/ejection of cache picks happens below.
        enum Picked {
            Bin { path: PathBuf, from_cache: bool },
            DownloadPin,
        }
        let picked = match runtime_manager::pick_runtime(best, bundled) {
            Decision::Cache { path: dir, .. } => match find_runtime_binary(&dir) {
                Some(b) => Picked::Bin {
                    path: b,
                    from_cache: true,
                },
                None if allow_eject => {
                    // Incomplete cache (binary missing) — eject and retry
                    // with the next candidate / the pin download.
                    allow_eject = false;
                    tracing::warn!(
                        dir = %dir.display(),
                        "builtin llm: cache dir has no llama-server binary — ejecting"
                    );
                    let _ = std::fs::remove_dir_all(&dir);
                    runtime_manager::invalidate_resolution_caches();
                    continue;
                }
                None => {
                    return Err(format!(
                        "cache dir {} is incomplete and no fallback remains",
                        dir.display()
                    ))
                }
            },
            Decision::Bundled(path) => Picked::Bin {
                path,
                from_cache: false,
            },
            Decision::TryPinThenBundled(path) => {
                match ensure_runtime_variant(data_dir, variant).await {
                    Ok(bin) => Picked::Bin {
                        path: bin,
                        from_cache: false,
                    },
                    // Offline (or the pin asset is gone): the older bundled
                    // binary still beats failing the boot — zero regression
                    // vs. the old bundled-first behavior.
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "builtin llm: pin runtime unavailable — falling back to the older \
                             bundled binary"
                        );
                        Picked::Bin {
                            path,
                            from_cache: false,
                        }
                    }
                }
            }
            Decision::TryPin => Picked::DownloadPin,
        };

        if let Picked::Bin { path, from_cache } = picked {
            // Cache picks are exec-verified once per process (bundled ones
            // were either Jetson-gated above or ship with the platform).
            if !from_cache || runtime_manager::is_verified(&path) {
                return Ok(path);
            }
            match exec_check(&path).await {
                Ok(()) => {
                    runtime_manager::mark_verified(&path);
                    return Ok(path);
                }
                Err(e) if allow_eject => {
                    allow_eject = false;
                    tracing::warn!(
                        bin = %path.display(),
                        error = %e,
                        "builtin llm: cached llama-server failed its exec check — ejecting \
                         the cache dir and retrying with the next candidate"
                    );
                    if let Some(dir) = path.parent() {
                        let _ = std::fs::remove_dir_all(dir);
                    }
                    let _ = std::fs::remove_file(&path);
                    runtime_manager::invalidate_resolution_caches();
                    continue;
                }
                Err(e) => return Err(format!("cached llama-server failed its exec check: {e}")),
            }
        }

        // Picked::DownloadPin — bare `neomind serve` path.
        return match ensure_runtime_variant(data_dir, variant).await {
            Ok(bin) => Ok(bin),
            // CUDA/Vulkan are opt-in, but a dead backend helps nobody —
            // degrade this PROCESS to the CPU build instead of failing
            // the boot. Not persisted (the env/setting overrides stay
            // untouched), so a restart retries the accelerated path.
            Err(err) if matches!(variant, RuntimeVariant::Cuda | RuntimeVariant::Vulkan) => {
                tracing::warn!(
                    error = %err,
                    "builtin llm: {} runtime unusable — falling back to the CPU build for \
                     this process; unset the variant override or fix the driver/loader to \
                     retry",
                    if llama_asset_name(std::env::consts::OS, std::env::consts::ARCH, variant)
                        .is_some()
                    {
                        "download or exec failed"
                    } else {
                        "no prebuilt for this platform"
                    }
                );
                runtime_manager::degrade_to_cpu_for_process();
                ensure_runtime_variant(data_dir, RuntimeVariant::Cpu).await
            }
            Err(err) => Err(err),
        };
    }
}

/// Download (or reuse from cache) the runtime for ONE variant at the
/// CURRENT pin — the boot path. The standalone upgrade calls
/// [`install_runtime`] directly with channel-sourced tags/SHAs.
async fn ensure_runtime_variant(
    data_dir: &Path,
    variant: RuntimeVariant,
) -> Result<PathBuf, String> {
    let expected_sha =
        llama_runtime_expected_sha(std::env::consts::OS, std::env::consts::ARCH, variant);
    install_runtime(
        data_dir,
        variant,
        LLAMA_CPP_VERSION,
        expected_sha,
        None,
        None,
    )
    .await
}

/// Install (or reuse) the runtime for ONE variant at an arbitrary tag.
///
/// `expected_sha` hard-pins executable downloads (our self-hosted assets);
/// `url_override` bypasses the tag-derived URL (channel mirrors / tests);
/// `on_progress` receives streamed byte counts for progress events.
pub(crate) async fn install_runtime(
    data_dir: &Path,
    variant: RuntimeVariant,
    tag: &str,
    expected_sha: Option<&str>,
    url_override: Option<&str>,
    on_progress: Option<&mut (dyn FnMut(u64, Option<u64>) + Send)>,
) -> Result<PathBuf, String> {
    let cache_dir = llama_server_cache_dir_for_tag(data_dir, tag, variant);
    // A CUDA cache is only complete with its cudart DLLs — a partial one
    // (crashed between the two downloads) must not short-circuit. (Our
    // linux-x64 CUDA tarball bundles the libs, but the marker is written
    // after the exec check all the same.) Vulkan needs no extra bundle.
    let cache_complete = |dir: &Path| {
        find_runtime_binary(dir).is_some()
            && (matches!(
                variant,
                RuntimeVariant::Cpu | RuntimeVariant::Vulkan | RuntimeVariant::Jetson
            ) || dir.join(llama_cudart_marker()).exists())
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
    let url = match url_override {
        Some(u) => u.to_string(),
        None => {
            try_llama_server_url_for_tag(tag, std::env::consts::OS, std::env::consts::ARCH, variant)
                .unwrap_or_default()
        }
    };
    if url.is_empty() {
        return Err(format!(
            "no runtime URL for {}/{} at {tag}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
    }
    tracing::info!(url = %url, variant = ?variant, tag = %tag, "builtin llm: downloading llama-server runtime");

    RUNTIME_DL_ACTIVE.store(true, Ordering::SeqCst);
    // Fresh cache for the primary archive; a cudart bundle (if any) then
    // extracts INTO it without wiping.
    let result = download_runtime(
        &url,
        &cache_dir,
        llama_asset_is_zip(asset),
        true,
        expected_sha,
        on_progress,
    )
    .await;
    let result = match result {
        Ok(bin) => match llama_cudart_url_tagged(tag, asset) {
            // CUDA builds dlopen cudart/cublas DLLs — the bundle puts them
            // next to llama-server.exe so a driver-only host just works.
            Some(cudart_url) => {
                match download_runtime(&cudart_url, &cache_dir, true, false, None, None).await {
                    Ok(_) => {
                        // A CUDA build with no (or too old) NVIDIA driver
                        // fails to load cudart/cublas DLLs — detect that HERE
                        // with a fast `--version` exec so the caller can fall
                        // back to CPU, instead of dying at first spawn.
                        match exec_check(&bin).await {
                            Ok(()) => {
                                let _ =
                                    std::fs::write(cache_dir.join(llama_cudart_marker()), b"ok");
                                Ok(bin)
                            }
                            Err(e) => Err(format!("CUDA runtime exec check failed: {e}")),
                        }
                    }
                    Err(e) => Err(e),
                }
            }
            None => {
                // Our Jetson build deserves the same exec sanity gate as
                // CUDA — catch a broken download at install time, not spawn.
                if variant == RuntimeVariant::Jetson {
                    exec_check(&bin).await.map(|_| bin.clone()).map_err(|e| {
                        format!(
                            "Jetson runtime exec check failed (SHA matched but the binary \
                                 does not run — build/runtime mismatch): {e}"
                        )
                    })
                } else if variant == RuntimeVariant::Cuda {
                    // Our linux-x64 CUDA tarball bundles the CUDA runtime
                    // libs, so there is no cudart bundle to download — but
                    // `cache_complete` requires the marker for the Cuda
                    // variant, so exec-check (broken download / immediate
                    // loader failure) and stamp it. A driverless host can
                    // still pass --version (CUDA inits lazily); that failure
                    // surfaces at first spawn and degrades to CPU via the
                    // caller's fallback.
                    exec_check(&bin)
                        .await
                        .map(|_| {
                            let _ = std::fs::write(cache_dir.join(llama_cudart_marker()), b"ok");
                            bin.clone()
                        })
                        .map_err(|e| format!("CUDA runtime exec check failed: {e}"))
                } else {
                    Ok(bin)
                }
            }
        },
        Err(e) => Err(e),
    };
    RUNTIME_DL_ACTIVE.store(false, Ordering::SeqCst);
    result
}

/// Whether a runtime download is in flight (for diagnostics + mutual
/// exclusion with model downloads / restarts).
pub fn runtime_download_active() -> bool {
    RUNTIME_DL_ACTIVE.load(Ordering::SeqCst)
}

/// `wipe` clears the cache dir first — true for the primary archive, false
/// for the cudart bundle (it must land in the freshly populated dir).
/// Streams with per-chunk progress (SHA updated incrementally).
pub(crate) async fn download_runtime(
    url: &str,
    cache_dir: &Path,
    is_zip: bool,
    wipe: bool,
    expected_sha: Option<&str>,
    on_progress: Option<&mut (dyn FnMut(u64, Option<u64>) + Send)>,
) -> Result<PathBuf, String> {
    use futures::StreamExt;
    use sha2::{Digest, Sha256};

    let mut on_progress = on_progress;
    let client = reqwest::Client::new();
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
    let total = resp.content_length();
    let mut stream = resp.bytes_stream();
    let mut hasher = Sha256::new();
    let mut bytes: Vec<u8> = Vec::with_capacity(32 * 1024 * 1024);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("read llama.cpp runtime: {e}"))?;
        hasher.update(&chunk);
        bytes.extend_from_slice(&chunk);
        if let Some(cb) = on_progress.as_mut() {
            cb(bytes.len() as u64, total);
        }
    }

    if let Some(want) = expected_sha {
        let out = hasher.finalize();
        let got = out.iter().map(|b| format!("{b:02x}")).collect::<String>();
        if !got.eq_ignore_ascii_case(want) {
            return Err(format!(
                "runtime SHA-256 mismatch: expected {want}, got {got} — refusing to install a \
                 tampered/truncated executable"
            ));
        }
    }

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

    tracing::info!(dest = %bin.display(), "builtin llm: llama-server runtime ready");
    Ok(bin)
}

/// Fast exec sanity check — `<binary> --version` must exit 0 within 10s.
/// Catches missing/old NVIDIA drivers (cudart DLL load failure) at download
/// time instead of at first spawn.
pub(crate) async fn exec_check(bin: &Path) -> Result<(), String> {
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
            String::from_utf8_lossy(&status.stderr)
                .chars()
                .take(200)
                .collect::<String>()
        )),
        Ok(Err(e)) => Err(format!("spawn: {e}")),
        Err(_) => Err("--version timed out after 10s".to_string()),
    }
}

/// Recursively locate the llama-server binary inside the cache dir.
pub(crate) fn find_runtime_binary(dir: &Path) -> Option<PathBuf> {
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
        let d = llama_server_cache_dir_for_tag(
            std::path::Path::new("/tmp/data"),
            LLAMA_CPP_VERSION,
            RuntimeVariant::Cpu,
        );
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
