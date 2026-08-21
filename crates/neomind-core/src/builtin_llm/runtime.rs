//! Builtin llama-server runtime resolution (pure logic, no HTTP).
//!
//! The desktop/server binary is bundled (`neomind-llama-server` next to the
//! executable, or on PATH); when it is missing (bare `neomind serve`, dev
//! builds, installs that opted out of the runtime) the API layer downloads
//! the official llama.cpp prebuilt for the platform into a versioned cache
//! under `data/llama-server/<version>/`.

use std::path::PathBuf;

/// llama.cpp RELEASE tag the on-demand runtime downloads from. Must be a tag
/// that actually publishes `-bin-{platform}.tar.gz` assets (b10524 and earlier
/// are git tags WITHOUT release binaries — that's why this differs from
/// `scripts/build-llama-server.sh`'s source-build pin of b10524). Bump the two
/// together whenever the source pin moves to a tag with binaries.
pub const LLAMA_CPP_VERSION: &str = "b10545";

/// Official release asset name for an OS/arch, if one ships. Windows ships
/// `win-cpu-*` archives (zip); macos/linux ship `-bin-*` tarballs.
pub fn llama_asset_name(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
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
    let ext = if llama_asset_is_zip(asset) { "zip" } else { "tar.gz" };
    format!(
        "https://github.com/ggml-org/llama.cpp/releases/download/{}/llama-{}-bin-{}.{}",
        LLAMA_CPP_VERSION, LLAMA_CPP_VERSION, asset, ext
    )
}

/// `llama-server` (unix) or `llama-server.exe` (windows) inside the archive.
pub fn llama_server_bin_name() -> &'static str {
    if cfg!(windows) { "llama-server.exe" } else { "llama-server" }
}

/// Cached runtime filename (matches what find_llama_server looks for).
pub fn llama_server_cache_name() -> &'static str {
    if cfg!(windows) { "neomind-llama-server.exe" } else { "neomind-llama-server" }
}

/// Cache dir for a downloaded runtime (versioned so a pin bump re-downloads).
pub fn llama_server_cache_dir(data_dir: &std::path::Path) -> PathBuf {
    data_dir.join("llama-server").join(LLAMA_CPP_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_map_covers_native_platforms() {
        assert_eq!(llama_asset_name("macos", "aarch64"), Some("macos-arm64"));
        assert_eq!(llama_asset_name("macos", "x86_64"), Some("macos-x64"));
        assert_eq!(llama_asset_name("linux", "x86_64"), Some("ubuntu-x64"));
        assert_eq!(llama_asset_name("linux", "aarch64"), Some("ubuntu-arm64"));
        assert_eq!(llama_asset_name("windows", "x86_64"), Some("win-cpu-x64"));
        assert_eq!(llama_asset_name("windows", "aarch64"), Some("win-cpu-arm64"));
        assert_eq!(llama_asset_name("linux", "s390x"), None);
        assert_eq!(llama_asset_name("freebsd", "x86_64"), None);
    }

    #[test]
    fn url_has_pinned_tag_and_asset() {
        let u = llama_server_url("ubuntu-arm64");
        assert!(u.contains(&format!("/{}/llama-{}-bin-ubuntu-arm64.tar.gz", LLAMA_CPP_VERSION, LLAMA_CPP_VERSION)));
    }
}
