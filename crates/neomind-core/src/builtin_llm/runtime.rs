//! Builtin llama-server runtime resolution (pure logic, no HTTP).
//!
//! The desktop/server binary is bundled (`neomind-llama-server` next to the
//! executable, or on PATH); when it is missing (bare `neomind serve`, dev
//! builds, installs that opted out of the runtime) the API layer downloads
//! the official llama.cpp prebuilt for the platform into a versioned cache
//! under `data/llama-server/<version>[-cuda]/`.

use std::path::PathBuf;

/// llama.cpp RELEASE tag the on-demand runtime downloads from. Must be a tag
/// that actually publishes `-bin-{platform}.tar.gz` assets (b10524 and earlier
/// are git tags WITHOUT release binaries — that's why this differs from
/// `scripts/build-llama-server.sh`'s source-build pin of b10524). Bump the two
/// together whenever the source pin moves to a tag with binaries.
pub const LLAMA_CPP_VERSION: &str = "b10545";

/// Accelerated-runtime selection for the on-demand download.
///
/// CPU is the default on every platform. `cuda`
/// (env `NEOMIND_BUILTIN_RUNTIME_VARIANT=cuda`) selects an official CUDA
/// prebuilt where llama.cpp ships one (Windows x64 at this pin) — the
/// matching cudart DLL bundle is downloaded alongside, so hosts need only an
/// NVIDIA driver, no CUDA toolkit. Linux/Jetson have no official CUDA asset
/// (see `llama_asset_name`); those platforms keep the CPU build and build
/// from source for GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RuntimeVariant {
    #[default]
    Cpu,
    Cuda,
}

impl RuntimeVariant {
    /// Parse the env value (case-insensitive; anything unrecognized is CPU).
    pub fn parse(raw: &str) -> Self {
        if raw.trim().eq_ignore_ascii_case("cuda") {
            RuntimeVariant::Cuda
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

    /// Cache-dir suffix so cpu/cuda caches never mix.
    fn cache_suffix(self) -> &'static str {
        match self {
            RuntimeVariant::Cpu => "",
            RuntimeVariant::Cuda => "-cuda",
        }
    }
}

/// Official release asset name for an OS/arch/variant, if one ships. Windows
/// ships `win-cpu-*` / `win-cuda-*` archives (zip); macos/linux ship `-bin-*`
/// tarballs. CUDA picks 12.4 (not 13.x) for the broadest driver floor.
pub fn llama_asset_name(os: &str, arch: &str, variant: RuntimeVariant) -> Option<&'static str> {
    match (os, arch) {
        (_, _) if variant == RuntimeVariant::Cuda => match (os, arch) {
            ("windows", "x86_64") | ("windows", "x86") => Some("win-cuda-12.4-x64"),
            // No official CUDA prebuilt for Linux (CPU/Vulkan only) or Jetson
            // — GPU there means building via scripts/build-llama-server.sh.
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

/// Windows assets are zips; everything else is a tar.gz.
pub fn llama_asset_is_zip(asset: &str) -> bool {
    asset.starts_with("win-")
}

/// Release archive URL for an asset (e.g. `ubuntu-arm64` → tar.gz,
/// `win-cpu-x64` → zip).
pub fn llama_server_url(asset: &str) -> String {
    let ext = if llama_asset_is_zip(asset) {
        "zip"
    } else {
        "tar.gz"
    };
    format!(
        "https://github.com/ggml-org/llama.cpp/releases/download/{}/llama-{}-bin-{}.{}",
        LLAMA_CPP_VERSION, LLAMA_CPP_VERSION, asset, ext
    )
}

/// CUDA runtime DLL bundle for a CUDA asset (`cudart-llama-bin-…`), if one
/// exists for it. Extracted into the same cache dir so the DLLs sit next to
/// `llama-server.exe` — with the bundle, a plain NVIDIA driver suffices
/// (no CUDA toolkit install). CPU assets have none.
pub fn llama_cudart_url(asset: &str) -> Option<String> {
    if !asset.starts_with("win-cuda") {
        return None;
    }
    Some(format!(
        "https://github.com/ggml-org/llama.cpp/releases/download/{}/cudart-llama-bin-{}.zip",
        LLAMA_CPP_VERSION, asset
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

/// Cache dir for a downloaded runtime (versioned so a pin bump re-downloads;
/// variant-suffixed so cpu/cuda never mix).
pub fn llama_server_cache_dir(data_dir: &std::path::Path, variant: RuntimeVariant) -> PathBuf {
    data_dir
        .join("llama-server")
        .join(format!("{}{}", LLAMA_CPP_VERSION, variant.cache_suffix()))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn cuda_variant_only_maps_windows_x64() {
        let cuda = RuntimeVariant::Cuda;
        assert_eq!(
            llama_asset_name("windows", "x86_64", cuda),
            Some("win-cuda-12.4-x64")
        );
        // No official CUDA asset for these — CPU build + source-build pointer.
        assert_eq!(llama_asset_name("windows", "aarch64", cuda), None);
        assert_eq!(llama_asset_name("linux", "x86_64", cuda), None);
        assert_eq!(llama_asset_name("linux", "aarch64", cuda), None); // Jetson
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
    fn variant_parse_is_forgiving() {
        assert_eq!(RuntimeVariant::parse("cuda"), RuntimeVariant::Cuda);
        assert_eq!(RuntimeVariant::parse(" CUDA "), RuntimeVariant::Cuda);
        assert_eq!(RuntimeVariant::parse(""), RuntimeVariant::Cpu);
        assert_eq!(RuntimeVariant::parse("cpu"), RuntimeVariant::Cpu);
        assert_eq!(RuntimeVariant::parse("jetson"), RuntimeVariant::Cpu); // unknown → cpu
    }
}
