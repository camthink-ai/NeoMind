//! Builtin llama-server runtime resolution (pure logic, no HTTP).
//!
//! The desktop/server binary is bundled (`neomind-llama-server` next to the
//! executable, or on PATH); when it is missing (bare `neomind serve`, dev
//! builds, installs that opted out of the runtime) the API layer downloads
//! the official llama.cpp prebuilt for the platform into a versioned cache
//! under `data/llama-server/<version>[-cuda]/`.

use std::path::PathBuf;

/// llama.cpp RELEASE tag the on-demand runtime downloads from. Must be a tag
/// that actually publishes `-bin-{platform}.tar.gz` assets. Bump policy:
/// keep to the newest tag whose OFFICIAL ubuntu builds still run on our
/// platform floor (Ubuntu 22.04, glibc 2.35, gcc-11 — verified by exec'ing
/// the ubuntu-x64 asset on a 22.04 host). The official ubuntu-CUDA builds
/// moved to a newer base (require glibc 2.38) and are therefore NOT used —
/// linux-x64 CUDA stays OUR self-hosted gcc-11 build (see
/// `linux_cuda_runtime_asset`).
///
/// # Bump runbook (all steps, in order)
///
/// 1. Bump this pin — CI's `llama-pin-sync` gate
///    (scripts/check_llama_pin_sync.py) also requires
///    `scripts/build-llama-server.sh`'s `LLAMA_CPP_TAG` and
///    `scripts/install.sh`'s `LLAMA_VERSION` defaults to move with it.
/// 2. Build + publish the `runtime-<tag>` release assets on
///    camthink-ai/NeoMind-Runtimes (Jetson via its CI workflow — keep
///    `-DGGML_NATIVE=OFF -DGGML_CPU_ARM_ARCH=armv8.2-a`; linux-cuda via
///    the on-device T4 recipe).
/// 3. Backfill `JETSON_RUNTIME_SHA256` / `LINUX_CUDA_RUNTIME_SHA256` here.
/// 4. Update `runtimes.json` in NeoMind-Runtimes (the standalone-upgrade
///    channel: `recommended` + per-variant `tag`/`asset`/`sha256`) — AFTER
///    the assets exist — then release NeoMind.
///
/// Runtime updates beyond this pin reach deployed customers through that
/// channel (see `builtin_llm::runtime_manager` in neomind-api), not through
/// compiled changes. Adding a new VARIANT (e.g. RKNN) splits into two very
/// different bills of work: the SERVER-BINARY layer (variant arm + asset
/// mapping + channel entry + build pipeline — the upgrade/revert/resolution
/// flow is variant-agnostic, and rkllm3-server is OpenAI-compatible so the
/// health/props surface fits) versus the MODEL layer, which is NOT: RKNN
/// models are chip-specific multi-file packages (.rkllm+.rknn+tokenizer),
/// not GGUF — catalog.json needs format/target_hardware fields,
/// `manifest.rs` must move in lockstep, and the GGUF-centric download/
/// import/spawn-arg paths need per-format arms (see runtimes/rknn/README.md
/// and its redistribution gate before any of that starts). Mirror/air-gap
/// deployments override the channel via `NEOMIND_RUNTIME_CHANNEL_URL`.
pub const LLAMA_CPP_VERSION: &str = "b11609";

/// Is this a well-formed llama.cpp release tag (`b` + 4-7 digits)?
///
/// Tags sourced from the remote runtime channel end up in cache-dir names
/// and download URLs — anything that doesn't match this shape is rejected
/// outright (path-injection guard: `../x`, `b12;rm`, padded/space variants).
pub fn validate_tag(tag: &str) -> bool {
    let digits = match tag.strip_prefix('b') {
        Some(d) => d,
        None => return false,
    };
    (4..=7).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit())
}

fn tag_number(tag: &str) -> Option<u64> {
    tag.strip_prefix('b').and_then(|n| n.parse::<u64>().ok())
}

/// Order llama.cpp release tags. Numeric when both sides are `bNNNN` (so
/// `b10` > `b2`); a non-numeric side sorts below numeric ones, and two
/// non-numeric tags fall back to string order. Never panics.
pub fn tag_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    match (tag_number(a), tag_number(b)) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (None, None) => a.cmp(b),
    }
}

/// Accelerated-runtime selection for the on-demand download.
///
/// CPU is the default on every platform. `cuda`
/// (env `NEOMIND_BUILTIN_RUNTIME_VARIANT=cuda`) selects a CUDA build where
/// one exists: the official llama.cpp prebuilt on Windows x64 (matching
/// cudart DLL bundle downloads alongside, so hosts need only an NVIDIA
/// driver), or OUR self-hosted build on Linux x64 (see
/// `LINUX_CUDA_RUNTIME_SHA256`) — llama.cpp ships no linux-x64 CUDA asset.
/// Other Linux/Jetson-armless platforms keep the CPU build and build from
/// source for GPU — EXCEPT Jetson (see `RuntimeVariant::Jetson`), where we
/// ship our own CUDA runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RuntimeVariant {
    #[default]
    Cpu,
    Cuda,
    /// Vulkan — covers AMD / Intel GPUs (and NVIDIA via its Vulkan ICD).
    /// Official llama.cpp assets (GNU-11 base, so 22.04-safe). Opt-in via
    /// `NEOMIND_BUILTIN_RUNTIME_VARIANT=vulkan` — deliberately NOT
    /// auto-detected: the Vulkan loader/ICD situation varies wildly
    /// (`libvulkan1` is absent on headless servers, virtio GPUs create
    /// /dev/dri false positives), while the auto-detectable NVIDIA case is
    /// better served by CUDA. Hosts need a Vulkan loader + a driver ICD
    /// (desktop stacks ship both; headless servers: install libvulkan1).
    Vulkan,
    /// NVIDIA Jetson (Orin-family, sm_87). Not an official llama.cpp asset —
    /// this is OUR gcc-11-built CUDA runtime, hosted on our GitHub Releases,
    /// SHA-pinned. Detected automatically on hosts with `/etc/nv_tegra_release`
    /// (Linux aarch64), or forced via `NEOMIND_BUILTIN_RUNTIME_VARIANT=jetson`.
    /// The official `ubuntu-arm64` CPU asset requires gcc-13 libstdc++
    /// (GLIBCXX_3.4.32); JetPack 6 ships gcc-11, so it fails to exec — that
    /// gap is exactly what this variant fills (verified end-to-end on a real
    /// Orin Nano 8GB, 2026-08-24).
    Jetson,
}

impl RuntimeVariant {
    /// Parse the env value (case-insensitive; anything unrecognized is CPU).
    pub fn parse(raw: &str) -> Self {
        if raw.trim().eq_ignore_ascii_case("cuda") {
            RuntimeVariant::Cuda
        } else if raw.trim().eq_ignore_ascii_case("vulkan") {
            RuntimeVariant::Vulkan
        } else if raw.trim().eq_ignore_ascii_case("jetson") {
            RuntimeVariant::Jetson
        } else {
            RuntimeVariant::Cpu
        }
    }

    /// Read `NEOMIND_BUILTIN_RUNTIME_VARIANT`.
    pub fn from_env() -> Self {
        std::env::var("NEOMIND_BUILTIN_RUNTIME_VARIANT")
            .map(|v| Self::parse(&v))
            .unwrap_or_default()
    }

    /// Resolve the runtime to fetch: explicit env override wins (both
    /// directions — `cuda` forces it on, `cpu` opts a GPU host out);
    /// otherwise auto-detect — Jetson via `/etc/nv_tegra_release`, and
    /// Linux x64 with a qualifying NVIDIA GPU (see `nvidia_probe_prefers_cuda`)
    /// gets our self-hosted CUDA build. Everything else keeps the CPU build.
    pub fn detect() -> Self {
        let env = Self::from_env();
        if env != RuntimeVariant::Cpu {
            return env;
        }
        if cfg!(target_os = "linux")
            && matches!(std::env::consts::ARCH, "aarch64" | "arm64")
            && std::path::Path::new("/etc/nv_tegra_release").exists()
        {
            return RuntimeVariant::Jetson;
        }
        if cfg!(target_os = "linux") && matches!(std::env::consts::ARCH, "x86_64" | "x86") {
            if let Some(probe) = NvidiaGpuProbe::probe() {
                if nvidia_probe_prefers_cuda(&probe) {
                    tracing::info!(
                        gpu = %probe.name,
                        compute_cap = ?probe.compute_cap,
                        driver = %probe.driver_version,
                        "builtin llm: NVIDIA GPU detected — using the CUDA runtime \
                         (NEOMIND_BUILTIN_RUNTIME_VARIANT=cpu to force the CPU build)"
                    );
                    return RuntimeVariant::Cuda;
                }
                tracing::debug!(
                    gpu = %probe.name,
                    compute_cap = ?probe.compute_cap,
                    driver = %probe.driver_version,
                    "builtin llm: NVIDIA GPU present but outside the CUDA build's \
                     sm_75..89 / CUDA-12-driver range — keeping the CPU build"
                );
            }
        }
        RuntimeVariant::Cpu
    }

    /// Cache-dir suffix so cpu/cuda/vulkan/jetson caches never mix.
    fn cache_suffix(self) -> &'static str {
        match self {
            RuntimeVariant::Cpu => "",
            RuntimeVariant::Cuda => "-cuda",
            RuntimeVariant::Vulkan => "-vulkan",
            RuntimeVariant::Jetson => "-jetson",
        }
    }
}

/// A successful `nvidia-smi` probe of the first GPU on the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NvidiaGpuProbe {
    pub name: String,
    /// (major, minor) e.g. (7, 5) for a Tesla T4.
    pub compute_cap: (u8, u8),
    pub driver_version: String,
}

impl NvidiaGpuProbe {
    /// Query name / compute capability / driver version via `nvidia-smi`.
    /// `None` on any failure (binary absent, non-zero exit, unparseable
    /// output, or a hang — a broken driver install can wedge nvidia-smi, so
    /// the probe runs under a 5s watchdog). Callers treat `None` as "no
    /// usable NVIDIA GPU", never as an error: auto-detection must not be
    /// able to break boot.
    pub fn probe() -> Option<Self> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let out = std::process::Command::new("nvidia-smi")
                .args([
                    "--query-gpu=name,compute_cap,driver_version",
                    "--format=csv,noheader",
                ])
                .output();
            let _ = tx.send(out);
        });
        let out = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .ok()?
            .ok()?;
        if !out.status.success() {
            return None;
        }
        // "Tesla T4, 7.5, 575.64.03" (first GPU only — multi-GPU hosts take
        // the first; heterogenous mixes are vanishingly rare).
        let line = String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()?
            .trim()
            .to_string();
        let mut parts = line.split(',').map(str::trim);
        let name = parts.next()?.to_string();
        let cc = parts.next()?;
        let driver_version = parts.next()?.to_string();
        let mut cc = cc.split('.');
        let major = cc.next()?.parse().ok()?;
        let minor = cc.next().and_then(|m| m.parse().ok()).unwrap_or(0);
        Some(Self {
            name,
            compute_cap: (major, minor),
            driver_version,
        })
    }
}

/// Pure decision: does a probe qualify for our self-hosted linux-x64 CUDA
/// build? The build carries SASS for sm_75..89 only, and its CUDA 12.4
/// runtime libs ride minor-version compatibility down to any CUDA 12 driver
/// (>= 525). Outside that range — V100 (sm_70), P100/Pascal (sm_60/61), or
/// a pre-CUDA-12 driver — choosing CUDA would install a runtime whose
/// kernels cannot execute on this host, so those stay on the CPU build
/// (where an operator can still force `cuda` via the env).
pub fn nvidia_probe_prefers_cuda(probe: &NvidiaGpuProbe) -> bool {
    let cc = probe.compute_cap.0 as u16 * 10 + probe.compute_cap.1 as u16;
    let driver_ok = probe
        .driver_version
        .split('.')
        .next()
        .and_then(|m| m.parse::<u32>().ok())
        .is_some_and(|major| major >= 525);
    (75..=89).contains(&cc) && driver_ok
}

/// Our Jetson runtime asset (not an official llama.cpp release). See
/// `RuntimeVariant::Jetson`. The tarball is the on-device build verified on
/// Orin Nano 8GB / NX-class hardware: thin llama-server shell + sibling .so
/// files (whole archive must be extracted together), $ORIGIN RUNPATH.
pub const JETSON_RUNTIME_BASE_URL: &str =
    "https://github.com/camthink-ai/NeoMind-Runtimes/releases/download";
/// Asset filename for our Jetson runtime at a given llama.cpp tag. Built
/// from the tag (not hardcoded) so a pin bump can never leave a stale
/// filename behind — CI's `check_llama_pin_sync.py` guards the pin itself.
pub fn jetson_runtime_asset(tag: &str) -> String {
    format!("llama-server-{tag}-linux-aarch64-jetson.tar.gz")
}
/// SHA-256 of the asset for the CURRENT `LLAMA_CPP_VERSION` pin.
/// Refilled per llama.cpp pin bump from the Runtimes build output.
/// b11609 rebuild (2026-10-14): the first CI build (ubuntu-22.04-arm,
/// Neoverse/ARMv9 host, GGML_NATIVE left on) carried SVE instructions in
/// libggml-cpu and SIGILL'd on every Orin — the asset was rebuilt NATIVELY
/// on an NG4520 (CUDA 12.6, armv8.2-a pinned) and smoke-tested on-device.
/// Future CI bumps must keep -DGGML_NATIVE=OFF -DGGML_CPU_ARM_ARCH=armv8.2-a
/// (fixed in NeoMind-Runtimes 26c9b92).
pub const JETSON_RUNTIME_SHA256: &str =
    "60bc22258cb34796f2d1eb3900c3a6ca90a21df922c3a8e4f37f89e270720b4d";

/// Our Linux x64 CUDA runtime asset (not an official llama.cpp release —
/// they ship CPU/Vulkan-only tarballs for linux-x64). Same hosting model as
/// the Jetson asset: built with the CUDA 12.4 toolkit (broadest driver
/// floor — same choice as the official Windows CUDA asset, so a ≥550 driver
/// suffices), archs 75/80/86/89 (Turing→Ada: T4, A100, A10, RTX 30xx,
/// L40S, RTX 40xx), gcc-11. The tarball carries the whole `bin/` tree with
/// the CUDA runtime libs (cudart/cublas/cublasLt) bundled next to the
/// thin-wrapper llama-server ($ORIGIN RUNPATH) — no CUDA toolkit needed on
/// the host, an NVIDIA driver alone works.
/// Asset filename for our Linux x64 CUDA runtime at a given llama.cpp tag
/// (not an official llama.cpp release — they ship CPU/Vulkan-only tarballs
/// for linux-x64). Same hosting model as the Jetson asset: built with the
/// CUDA 12.4 toolkit (broadest driver floor — same choice as the official
/// Windows CUDA asset, so a ≥550 driver suffices), archs 75/80/86/89
/// (Turing→Ada: T4, A100, A10, RTX 30xx, L40S, RTX 40xx), gcc-11. The
/// tarball carries the whole `bin/` tree with the CUDA runtime libs
/// (cudart/cublas/cublasLt) bundled next to the thin-wrapper llama-server
/// ($ORIGIN RUNPATH) — no CUDA toolkit needed on the host, an NVIDIA driver
/// alone works.
pub fn linux_cuda_runtime_asset(tag: &str) -> String {
    format!("llama-server-{tag}-linux-x64-cuda.tar.gz")
}
/// SHA-256 of the asset for the CURRENT `LLAMA_CPP_VERSION` pin (executable
/// download — hard pin, no slack). Artifact: on-device build on a real Tesla
/// T4 host (Ubuntu 22.04, CUDA 12.4 toolkit, gcc-11; CUDA backend init +
/// CPU-mode generation verified there 2026-10-14 — GPU-mode was VRAM-blocked
/// by co-tenants that day). Our build exists because the OFFICIAL
/// ubuntu-cuda assets moved to a glibc-2.38 base — this gcc-11 build keeps
/// the 22.04 floor while tracking the current `LLAMA_CPP_VERSION`. SHA for
/// any OTHER tag comes from the runtime channel (runtimes.json), not here.
pub const LINUX_CUDA_RUNTIME_SHA256: &str =
    "c1fcbf65aba46210ae6085a98175033a0b4cfc4adf6581bcb18beeafea560103";

/// Official release asset name for an OS/arch/variant, if one ships. Windows
/// ships `win-cpu-*` / `win-cuda-*` archives (zip); macos/linux ship `-bin-*`
/// tarballs. CUDA picks 12.4 (not 13.x) for the broadest driver floor.
pub fn llama_asset_name(os: &str, arch: &str, variant: RuntimeVariant) -> Option<&'static str> {
    match (os, arch) {
        (_, _) if variant == RuntimeVariant::Cuda => match (os, arch) {
            ("windows", "x86_64") | ("windows", "x86") => Some("win-cuda-12.4-x64"),
            // The OFFICIAL ubuntu-cuda builds need glibc 2.38 (Ubuntu 24.04
            // base) — unusable on our 22.04 floor — so Linux x64 uses OUR
            // gcc-11 self-hosted build (see llama_server_url_for).
            ("linux", "x86_64") | ("linux", "x86") => Some("linux-x64-cuda"),
            _ => None,
        },
        (_, _) if variant == RuntimeVariant::Vulkan => match (os, arch) {
            ("linux", "x86_64") | ("linux", "x86") => Some("ubuntu-vulkan-x64"),
            ("linux", "aarch64") | ("linux", "arm64") => Some("ubuntu-vulkan-arm64"),
            ("windows", "x86_64") | ("windows", "x86") => Some("win-vulkan-x64"),
            ("windows", "aarch64") | ("windows", "arm64") => Some("win-vulkan-arm64"),
            _ => None,
        },
        (_, _) if variant == RuntimeVariant::Jetson => match (os, arch) {
            ("linux", "aarch64") | ("linux", "arm64") => Some("jetson"),
            _ => None,
        },
        ("macos", "aarch64") | ("macos", "arm64") => Some("macos-arm64"),
        ("macos", "x86_64") | ("macos", "x86") => Some("macos-x64"),
        ("linux", "x86_64") | ("linux", "x86") => Some("ubuntu-x64"),
        ("linux", "aarch64") | ("linux", "arm64") => Some("ubuntu-arm64"),
        ("windows", "x86_64") | ("windows", "x86") => Some("win-cpu-x64"),
        ("windows", "aarch64") | ("windows", "arm64") => Some("win-cpu-arm64"),
        _ => None,
    }
}

/// Release URL for the resolved asset at the CURRENT pin. See
/// [`try_llama_server_url_for_tag`] for the tag-parameterized form used by
/// the standalone runtime upgrade (channel-sourced tags).
pub fn llama_server_url_for(os: &str, arch: &str, variant: RuntimeVariant) -> String {
    try_llama_server_url_for_tag(LLAMA_CPP_VERSION, os, arch, variant).unwrap_or_default()
}

/// Tag-parameterized release URL, `None` when the platform has no prebuilt
/// asset at all. The Jetson asset is OURS (hosted on our GitHub Releases
/// under the runtime tag, SHA-pinned); everything else is the official
/// llama.cpp release archive. `tag` MUST already be [`validate_tag`]-checked
/// by the caller when it comes from the runtime channel.
pub fn try_llama_server_url_for_tag(
    tag: &str,
    os: &str,
    arch: &str,
    variant: RuntimeVariant,
) -> Option<String> {
    let asset = llama_asset_name(os, arch, variant);
    // Our self-hosted assets (Jetson, linux-x64 CUDA): one release tag per
    // llama.cpp version in NeoMind-Runtimes.
    if variant == RuntimeVariant::Jetson {
        // {base}/releases/download/<runtime-<tag>>/<asset>
        Some(format!(
            "{}/runtime-{}/{}",
            JETSON_RUNTIME_BASE_URL,
            tag,
            jetson_runtime_asset(tag)
        ))
    } else if asset == Some("linux-x64-cuda") {
        Some(format!(
            "{}/runtime-{}/{}",
            JETSON_RUNTIME_BASE_URL,
            tag,
            linux_cuda_runtime_asset(tag)
        ))
    } else {
        asset.map(|a| llama_server_url_tagged(tag, a))
    }
}

/// SHA pin for OUR self-hosted runtime assets (executable downloads — hard
/// pin, no slack). Official llama.cpp assets move only when our
/// `LLAMA_CPP_VERSION` pin moves, so they stay unpinned.
pub fn llama_runtime_expected_sha(
    os: &str,
    arch: &str,
    variant: RuntimeVariant,
) -> Option<&'static str> {
    match (os, arch, variant) {
        (_, _, RuntimeVariant::Jetson) => Some(JETSON_RUNTIME_SHA256),
        ("linux", "x86_64" | "x86", RuntimeVariant::Cuda) => Some(LINUX_CUDA_RUNTIME_SHA256),
        _ => None,
    }
}

/// Windows assets are zips; everything else is a tar.gz.
pub fn llama_asset_is_zip(asset: &str) -> bool {
    asset.starts_with("win-")
}

/// Release archive URL for an asset at the CURRENT pin (e.g. `ubuntu-arm64`
/// → tar.gz, `win-cpu-x64` → zip).
pub fn llama_server_url(asset: &str) -> String {
    llama_server_url_tagged(LLAMA_CPP_VERSION, asset)
}

/// Tag-parameterized form of [`llama_server_url`].
pub fn llama_server_url_tagged(tag: &str, asset: &str) -> String {
    let ext = if llama_asset_is_zip(asset) {
        "zip"
    } else {
        "tar.gz"
    };
    format!(
        "https://github.com/ggml-org/llama.cpp/releases/download/{}/llama-{}-bin-{}.{}",
        tag, tag, asset, ext
    )
}

/// CUDA runtime DLL bundle for a CUDA asset (`cudart-llama-bin-…`), if one
/// exists for it. Extracted into the same cache dir so the DLLs sit next to
/// `llama-server.exe` — with the bundle, a plain NVIDIA driver suffices
/// (no CUDA toolkit install). CPU assets have none.
pub fn llama_cudart_url(asset: &str) -> Option<String> {
    llama_cudart_url_tagged(LLAMA_CPP_VERSION, asset)
}

/// Tag-parameterized form of [`llama_cudart_url`].
pub fn llama_cudart_url_tagged(tag: &str, asset: &str) -> Option<String> {
    if !asset.starts_with("win-cuda") {
        return None;
    }
    Some(format!(
        "https://github.com/ggml-org/llama.cpp/releases/download/{}/cudart-llama-bin-{}.zip",
        tag, asset
    ))
}

/// Marker file written after the cudart bundle is extracted, so a
/// partially-populated cache (binaries present, DLLs missing) doesn't
/// short-circuit as a cache hit.
pub fn llama_cudart_marker() -> &'static str {
    ".cudart-ok"
}

/// `llama-server` (unix) or `llama-server.exe` (windows) inside the archive.
pub fn llama_server_bin_name() -> &'static str {
    if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

/// Cached runtime filename (matches what find_llama_server looks for).
pub fn llama_server_cache_name() -> &'static str {
    if cfg!(windows) {
        "neomind-llama-server.exe"
    } else {
        "neomind-llama-server"
    }
}

/// Cache dir for a downloaded runtime at the CURRENT pin (versioned so a
/// pin bump re-downloads; variant-suffixed so cpu/cuda never mix).
pub fn llama_server_cache_dir(data_dir: &std::path::Path, variant: RuntimeVariant) -> PathBuf {
    llama_server_cache_dir_for_tag(data_dir, LLAMA_CPP_VERSION, variant)
}

/// Tag-parameterized form of [`llama_server_cache_dir`] — the standalone
/// runtime upgrade installs channel tags into their own versioned dir.
pub fn llama_server_cache_dir_for_tag(
    data_dir: &std::path::Path,
    tag: &str,
    variant: RuntimeVariant,
) -> PathBuf {
    data_dir
        .join("llama-server")
        .join(format!("{}{}", tag, variant.cache_suffix()))
}

/// Inverse of the cache-dir naming: parse a `data/llama-server/` entry name
/// back into `(tag, variant)`. `None` for temp dirs, unknown suffixes, or
/// malformed tags (safe to ignore/prune those).
pub fn parse_runtime_cache_name(name: &str) -> Option<(String, RuntimeVariant)> {
    let (tag, variant) = if let Some(t) = name.strip_suffix("-jetson") {
        (t, RuntimeVariant::Jetson)
    } else if let Some(t) = name.strip_suffix("-cuda") {
        (t, RuntimeVariant::Cuda)
    } else if let Some(t) = name.strip_suffix("-vulkan") {
        (t, RuntimeVariant::Vulkan)
    } else {
        (name, RuntimeVariant::Cpu)
    };
    validate_tag(tag).then(|| (tag.to_string(), variant))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jetson_url_includes_runtime_tag_segment() {
        // Regression: dropping the runtime-<tag> segment from the base URL
        // produced .../releases/download/<asset> → 404 on first real Jetson
        // auto-download (2026-08-25). The tag must sit between /download/ and
        // the asset filename.
        let url = llama_server_url_for("linux", "aarch64", RuntimeVariant::Jetson);
        assert!(url.starts_with(JETSON_RUNTIME_BASE_URL));
        assert!(
            url.contains(&format!("/releases/download/runtime-{LLAMA_CPP_VERSION}/")),
            "url missing runtime tag segment: {url}"
        );
        assert!(
            url.ends_with(&jetson_runtime_asset(LLAMA_CPP_VERSION)),
            "url: {url}"
        );
        // Non-Jetson linux-aarch64 must keep the official asset URL.
        let cpu = llama_server_url_for("linux", "aarch64", RuntimeVariant::Cpu);
        assert!(cpu.starts_with("https://github.com/ggml-org/llama.cpp/"));
    }

    #[test]
    fn asset_map_covers_native_platforms() {
        let cpu = RuntimeVariant::Cpu;
        assert_eq!(
            llama_asset_name("macos", "aarch64", cpu),
            Some("macos-arm64")
        );
        assert_eq!(llama_asset_name("macos", "x86_64", cpu), Some("macos-x64"));
        assert_eq!(llama_asset_name("linux", "x86_64", cpu), Some("ubuntu-x64"));
        assert_eq!(
            llama_asset_name("linux", "aarch64", cpu),
            Some("ubuntu-arm64")
        );
        assert_eq!(
            llama_asset_name("windows", "x86_64", cpu),
            Some("win-cpu-x64")
        );
        assert_eq!(
            llama_asset_name("windows", "aarch64", cpu),
            Some("win-cpu-arm64")
        );
        assert_eq!(llama_asset_name("linux", "s390x", cpu), None);
        assert_eq!(llama_asset_name("freebsd", "x86_64", cpu), None);
    }

    #[test]
    fn cuda_variant_maps_windows_x64_and_our_linux_build() {
        let cuda = RuntimeVariant::Cuda;
        assert_eq!(
            llama_asset_name("windows", "x86_64", cuda),
            Some("win-cuda-12.4-x64")
        );
        // No official llama.cpp CUDA asset for Linux — we self-host one.
        assert_eq!(
            llama_asset_name("linux", "x86_64", cuda),
            Some("linux-x64-cuda")
        );
        // These have no CUDA asset at all — CPU build + source-build pointer.
        assert_eq!(llama_asset_name("windows", "aarch64", cuda), None);
        assert_eq!(llama_asset_name("linux", "aarch64", cuda), None); // Jetson
    }

    #[test]
    fn nvidia_probe_decision_covers_supported_gpus() {
        let p = |name: &str, cc: (u8, u8), drv: &str| NvidiaGpuProbe {
            name: name.to_string(),
            compute_cap: cc,
            driver_version: drv.to_string(),
        };
        // Supported datacenter/consumer cards on CUDA-12 drivers.
        for (name, cc) in [
            ("Tesla T4", (7, 5)),
            ("A100-SXM4-40GB", (8, 0)),
            ("NVIDIA A10", (8, 6)),
            ("L40S", (8, 9)),
            ("RTX 4090", (8, 9)),
        ] {
            assert!(
                nvidia_probe_prefers_cuda(&p(name, cc, "575.64.03")),
                "{name} should prefer CUDA"
            );
        }
        // Driver floor: 525 is the first CUDA 12 driver; 535/550 fine.
        assert!(nvidia_probe_prefers_cuda(&p(
            "Tesla T4",
            (7, 5),
            "525.60.13"
        )));
        assert!(nvidia_probe_prefers_cuda(&p("Tesla T4", (7, 5), "550.54")));
        // Pre-CUDA-12 driver → CPU (the bundled 12.4 libs won't load).
        assert!(!nvidia_probe_prefers_cuda(&p("Tesla T4", (7, 5), "470.63")));
        // GPUs outside sm_75..89 (no SASS in our build) → CPU.
        assert!(!nvidia_probe_prefers_cuda(&p(
            "Tesla V100",
            (7, 0),
            "575.64.03"
        )));
        assert!(!nvidia_probe_prefers_cuda(&p(
            "Tesla P100",
            (6, 0),
            "575.64.03"
        )));
        // Future arch (Blackwell) → CPU until the build covers it.
        assert!(!nvidia_probe_prefers_cuda(&p(
            "RTX 5090",
            (12, 0),
            "575.64.03"
        )));
        // Garbage driver string → CPU, never a panic.
        assert!(!nvidia_probe_prefers_cuda(&p(
            "Tesla T4",
            (7, 5),
            "unknown"
        )));
    }

    #[test]
    fn variant_env_overrides_both_directions() {
        // Opt-out on a GPU host and opt-in on a non-GPU host must both win
        // over any auto-detection (`parse` + `from_env` are the only levers
        // `detect()` consults before probing).
        assert_eq!(RuntimeVariant::parse("cuda"), RuntimeVariant::Cuda);
        assert_eq!(RuntimeVariant::parse("CPU"), RuntimeVariant::Cpu);
        assert_eq!(RuntimeVariant::parse("jetson"), RuntimeVariant::Jetson);
        assert_eq!(RuntimeVariant::parse("Vulkan"), RuntimeVariant::Vulkan);
        assert_eq!(RuntimeVariant::parse("garbage"), RuntimeVariant::Cpu);
    }

    #[test]
    fn vulkan_uses_official_assets_and_never_autodetects() {
        let vk = RuntimeVariant::Vulkan;
        // Official llama.cpp assets on the GNU-11 (22.04-safe) base.
        assert_eq!(
            llama_asset_name("linux", "x86_64", vk),
            Some("ubuntu-vulkan-x64")
        );
        assert_eq!(
            llama_asset_name("linux", "aarch64", vk),
            Some("ubuntu-vulkan-arm64")
        );
        assert_eq!(
            llama_asset_name("windows", "x86_64", vk),
            Some("win-vulkan-x64")
        );
        assert_eq!(llama_asset_name("macos", "arm64", vk), None);
        // Official domain — unlike our self-hosted CUDA/Jetson assets.
        let url = llama_server_url_for("linux", "x86_64", vk);
        assert!(
            url.starts_with("https://github.com/ggml-org/llama.cpp/"),
            "{url}"
        );
        assert!(url.ends_with("ubuntu-vulkan-x64.tar.gz"), "{url}");
        // No cudart bundle for Vulkan (driver-provided loader/ICD).
        assert_eq!(llama_cudart_url("ubuntu-vulkan-x64"), None);
        // Dedicated cache dir so a vulkan download never mixes with cpu's.
        let d = llama_server_cache_dir(std::path::Path::new("/data"), vk);
        assert!(d.to_string_lossy().ends_with("-vulkan"), "{d:?}");
    }

    #[test]
    fn self_hosted_sha_pins_are_not_placeholders() {
        // Both self-hosted assets are hard SHA-pinned; an all-zeros value
        // means a pin bump shipped before its artifact was built/uploaded —
        // every download would fail the integrity check. Fail loudly here
        // instead of at first customer spawn.
        for (name, sha) in [
            ("JETSON_RUNTIME_SHA256", JETSON_RUNTIME_SHA256),
            ("LINUX_CUDA_RUNTIME_SHA256", LINUX_CUDA_RUNTIME_SHA256),
        ] {
            assert_eq!(sha.len(), 64, "{name} must be 64 hex chars");
            assert!(
                sha.chars().all(|c| c.is_ascii_hexdigit()),
                "{name} must be hex"
            );
            assert_ne!(
                sha,
                "0".repeat(64),
                "{name} is still the placeholder — build & publish the \
                 runtime-{LLAMA_CPP_VERSION} asset, then backfill the SHA"
            );
        }
    }

    #[test]
    fn linux_cuda_url_is_our_runtimes_release() {
        // Same release-tag shape as the Jetson asset: our NeoMind-Runtimes
        // repo, one `runtime-<tag>` release per llama.cpp version, SHA-pinned.
        let url = llama_server_url_for("linux", "x86_64", RuntimeVariant::Cuda);
        assert!(url.starts_with(JETSON_RUNTIME_BASE_URL));
        assert!(
            url.contains(&format!("/releases/download/runtime-{LLAMA_CPP_VERSION}/")),
            "url missing runtime tag segment: {url}"
        );
        assert!(
            url.ends_with(&linux_cuda_runtime_asset(LLAMA_CPP_VERSION)),
            "url: {url}"
        );
        // Self-hosted ⇒ SHA-pinned; aarch64 CUDA has no asset ⇒ no pin.
        assert_eq!(
            llama_runtime_expected_sha("linux", "x86_64", RuntimeVariant::Cuda),
            Some(LINUX_CUDA_RUNTIME_SHA256)
        );
        assert_eq!(
            llama_runtime_expected_sha("linux", "aarch64", RuntimeVariant::Cuda),
            None
        );
        // The bundled CUDA libs make a cudart bundle download unnecessary.
        assert_eq!(llama_cudart_url("linux-x64-cuda"), None);
        // CPU variant on the same platform keeps the official asset.
        let cpu = llama_server_url_for("linux", "x86_64", RuntimeVariant::Cpu);
        assert!(cpu.starts_with("https://github.com/ggml-org/llama.cpp/"));
    }

    #[test]
    fn url_has_pinned_tag_and_asset() {
        let u = llama_server_url("ubuntu-arm64");
        assert!(u.contains(&format!(
            "/{}/llama-{}-bin-ubuntu-arm64.tar.gz",
            LLAMA_CPP_VERSION, LLAMA_CPP_VERSION
        )));
    }

    #[test]
    fn cuda_urls_match_real_release_filenames() {
        // Exact filenames as published on the b10545 release page.
        let bin = llama_server_url("win-cuda-12.4-x64");
        assert_eq!(
            bin,
            format!("https://github.com/ggml-org/llama.cpp/releases/download/{0}/llama-{0}-bin-win-cuda-12.4-x64.zip", LLAMA_CPP_VERSION)
        );
        let cudart = llama_cudart_url("win-cuda-12.4-x64").expect("cudart for win-cuda");
        assert_eq!(
            cudart,
            format!("https://github.com/ggml-org/llama.cpp/releases/download/{0}/cudart-llama-bin-win-cuda-12.4-x64.zip", LLAMA_CPP_VERSION)
        );
        // CPU assets carry no cudart bundle.
        assert!(llama_cudart_url("win-cpu-x64").is_none());
        assert!(llama_cudart_url("ubuntu-x64").is_none());
    }

    #[test]
    fn cache_dir_is_versioned_and_variant_suffixed() {
        let root = std::path::Path::new("/data");
        assert_eq!(
            llama_server_cache_dir(root, RuntimeVariant::Cpu),
            PathBuf::from(format!("/data/llama-server/{LLAMA_CPP_VERSION}"))
        );
        assert_eq!(
            llama_server_cache_dir(root, RuntimeVariant::Cuda),
            PathBuf::from(format!("/data/llama-server/{LLAMA_CPP_VERSION}-cuda"))
        );
    }

    #[test]
    fn tagged_builders_accept_arbitrary_tags_and_parse_round_trips() {
        // Channel-sourced tags must flow through every builder unchanged.
        for tag in ["b10000", "b99999"] {
            let dir = llama_server_cache_dir_for_tag(
                std::path::Path::new("/data"),
                tag,
                RuntimeVariant::Jetson,
            );
            assert!(dir.to_string_lossy().ends_with(&format!("{tag}-jetson")));
            let (parsed_tag, parsed_variant) =
                parse_runtime_cache_name(&format!("{tag}-jetson")).expect("parses");
            assert_eq!(
                (parsed_tag.as_str(), parsed_variant),
                (tag, RuntimeVariant::Jetson)
            );

            let jetson =
                try_llama_server_url_for_tag(tag, "linux", "aarch64", RuntimeVariant::Jetson)
                    .expect("jetson url");
            assert!(jetson.ends_with(&jetson_runtime_asset(tag)), "{jetson}");

            let cpu = try_llama_server_url_for_tag(tag, "linux", "x86_64", RuntimeVariant::Cpu)
                .expect("cpu url");
            assert!(
                cpu.contains(&format!("/{tag}/llama-{tag}-bin-ubuntu-x64.tar.gz")),
                "{cpu}"
            );
            let cudart = llama_cudart_url_tagged(tag, "win-cuda-12.4-x64").expect("cudart");
            assert!(cudart.contains(&format!("/{tag}/cudart-llama-bin-win-cuda-12.4-x64.zip")));
        }
        // Unsupported platform → None (legacy wrapper degrades to "").
        assert!(
            try_llama_server_url_for_tag("b10000", "freebsd", "x86_64", RuntimeVariant::Cpu)
                .is_none()
        );
    }

    #[test]
    fn parse_runtime_cache_name_rejects_garbage() {
        // Temp/partial dirs and malformed tags are ignorable, not parseable.
        for bad in [
            "",
            ".tmp-download",
            "b12",
            "b12345678",
            "b1160a",
            "../escape",
            "-cuda",
        ] {
            assert!(parse_runtime_cache_name(bad).is_none(), "{bad:?}");
        }
        // Plain (cpu) names parse to the Cpu variant.
        assert_eq!(
            parse_runtime_cache_name("b11609"),
            Some(("b11609".to_string(), RuntimeVariant::Cpu))
        );
    }

    #[test]
    fn tag_cmp_is_numeric_for_b_tags() {
        use std::cmp::Ordering;
        assert_eq!(tag_cmp("b10", "b2"), Ordering::Greater);
        assert_eq!(tag_cmp("b2", "b10"), Ordering::Less);
        assert_eq!(tag_cmp("b11609", "b11609"), Ordering::Equal);
        assert_eq!(tag_cmp("b99999", "b11609"), Ordering::Greater);
        // Non-numeric sides sort below numeric ones, then by string.
        assert_eq!(tag_cmp("b1", "weird"), Ordering::Greater);
        assert_eq!(tag_cmp("weird", "b1"), Ordering::Less);
        assert_eq!(tag_cmp("alpha", "beta"), Ordering::Less);
    }

    #[test]
    fn validate_tag_blocks_injection_and_malformed() {
        for good in ["b1054", "b11609", "b9999999"] {
            assert!(validate_tag(good), "{good}");
        }
        for bad in [
            "",
            "b",
            "b12",
            "b12345678",
            "B11609",
            "b11609 ",
            " b11609",
            "b1160a",
            "../x",
            "b12;rm",
            "b/11609",
            "11609",
            "b11609\n",
        ] {
            assert!(!validate_tag(bad), "{bad:?}");
        }
    }

    #[test]
    fn variant_parse_is_forgiving() {
        assert_eq!(RuntimeVariant::parse("cuda"), RuntimeVariant::Cuda);
        assert_eq!(RuntimeVariant::parse(" CUDA "), RuntimeVariant::Cuda);
        assert_eq!(RuntimeVariant::parse(""), RuntimeVariant::Cpu);
        assert_eq!(RuntimeVariant::parse("cpu"), RuntimeVariant::Cpu);
        assert_eq!(RuntimeVariant::parse("jetson"), RuntimeVariant::Jetson);
        assert_eq!(RuntimeVariant::parse("JETSON"), RuntimeVariant::Jetson);
        assert_eq!(RuntimeVariant::parse("rockchip"), RuntimeVariant::Cpu); // unknown → cpu
    }
}
