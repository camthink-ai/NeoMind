//! `marketplace` handlers — split from the former extensions.rs monolith.

use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::json;

use crate::handlers::common::{ok, HandlerResult};
use crate::models::error::ErrorResponse;
use crate::server::types::MAX_EXTENSION_DOWNLOAD_SIZE;
use crate::server::ServerState;
use futures::StreamExt;
use neomind_storage::ExtensionRecord;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;
// ============================================================================

/// Configuration for cloud extension marketplace
const MARKET_BRANCH: &str = "main";
const DEFAULT_EXTENSION_MARKET_BASE_URL: &str =
    "https://raw.githubusercontent.com/camthink-ai/NeoMind-Extensions";

/// Effective extension-marketplace base URL.
///
/// Precedence: saved value (Settings → Preferences, admin) >
/// `NEOMIND_EXTENSION_MARKET_URL` env > built-in default. The default host
/// (`raw.githubusercontent.com`) is often unreachable from CN networks —
/// before this existed the source was hardcoded with NO override at all,
/// while the component market and LLM catalog both had env overrides.
/// Mirrors follow the component-market shape:
/// `https://ghfast.top/https://raw.githubusercontent.com/camthink-ai/...`
pub(crate) fn extension_market_base_url() -> String {
    let saved = neomind_storage::SettingsStore::open_default()
        .ok()
        .and_then(|s| s.load("extension_market_url").ok().flatten());
    if let Some(url) = saved {
        if let Some(clean) = normalize_market_url(&url) {
            return clean;
        }
    }
    if let Ok(url) = std::env::var("NEOMIND_EXTENSION_MARKET_URL") {
        if let Some(clean) = normalize_market_url(&url) {
            return clean;
        }
    }
    DEFAULT_EXTENSION_MARKET_BASE_URL.to_string()
}

/// Fall back to the release-level `checksums.txt` for a package sha256.
///
/// The marketplace index does not carry per-build sha256; the Extensions CI
/// uploads a `checksums.txt` (sha256sum format) to the SAME release as the
/// .nep assets, so integrity data is at least as fresh as the package
/// itself. Derives the checksum URL from the package URL and looks up the
/// matching filename.
async fn fetch_release_checksum(
    client: &reqwest::Client,
    package_url: &str,
    nep_filename: &str,
) -> Option<String> {
    let base = package_url.rsplit_once('/')?.0;
    let url = format!("{base}/checksums.txt");
    let text = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .header("User-Agent", "NeoMind-Extension-Marketplace")
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .text()
        .await
        .ok()?;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(sha), Some(name)) = (parts.next(), parts.next()) else {
            continue;
        };
        if name == nep_filename && sha.len() == 64 {
            return Some(sha.to_string());
        }
    }
    None
}

/// Resolve a client-provided package/library path, confining it to the
/// server's data directory.
///
/// `file_path` used to accept any host path, which made the register/upload/
/// validate endpoints a read-and-try-load primitive for arbitrary files on
/// the machine for anyone holding credentials. Relative paths resolve
/// against the data dir; absolute paths must land inside it (after
/// canonicalization, so `..` and symlinks can't escape).
pub(crate) fn resolve_confined_package_path(raw: &str) -> Result<PathBuf, ErrorResponse> {
    let data_dir = neomind_core::paths::data_dir()
        .to_string_lossy()
        .to_string();
    let data_root = std::path::PathBuf::from(&data_dir)
        .canonicalize()
        .unwrap_or_else(|_| std::path::PathBuf::from(&data_dir));

    let candidate = PathBuf::from(raw);
    let joined = if candidate.is_absolute() {
        candidate
    } else {
        data_root.join(candidate)
    };
    let canonical = joined
        .canonicalize()
        .map_err(|_| ErrorResponse::not_found(format!("Package file not found: {}", raw)))?;
    if !canonical.starts_with(&data_root) {
        return Err(ErrorResponse::bad_request(
            "file_path must point inside the server data directory",
        ));
    }
    Ok(canonical)
}

/// Accept an http(s) URL, trimmed of trailing slashes. Empty or non-URL
/// values fall back to `None` (→ the built-in default).
pub(crate) fn normalize_market_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if (trimmed.starts_with("https://") || trimmed.starts_with("http://"))
        && trimmed.len() > "https://".len()
    {
        Some(trimmed.to_string())
    } else {
        None
    }
}

/// Cloud extension metadata from index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudExtension {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    /// URL to full metadata (can also be specified as metadata_path)
    #[serde(default, alias = "metadata_path")]
    pub metadata_url: Option<String>,
    /// Frontend component info from index
    #[serde(default)]
    pub frontend: Option<FrontendInfo>,
    /// Available builds by platform
    #[serde(default)]
    pub builds: HashMap<String, ExtensionBuild>,
}

/// Frontend info from marketplace index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontendInfo {
    #[serde(default)]
    pub components: Vec<String>,
    #[serde(default)]
    pub entrypoint: Option<String>,
}

/// Full extension metadata from marketplace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceExtensionMetadata {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub readme_url: Option<String>,

    /// Extension type: native, wasm, frontend-only
    #[serde(default = "default_extension_type")]
    #[serde(rename = "type")]
    pub extension_type: String,

    /// ABI version for native extensions
    #[serde(default)]
    pub abi_version: u32,

    /// SDK version used to build the extension
    #[serde(default)]
    pub sdk_version: String,

    /// Keywords for search
    #[serde(default)]
    pub keywords: Vec<String>,

    /// .nep package URL (if available as a package instead of individual binaries)
    #[serde(default)]
    pub package_url: Option<String>,

    /// Package SHA256 checksum (for .nep packages)
    #[serde(default)]
    pub package_sha256: Option<String>,

    #[serde(default)]
    pub capabilities: ExtensionCapabilities,

    /// Commands at top level (for backward compatibility, merged into capabilities)
    #[serde(default)]
    pub commands: Vec<CommandInfo>,

    /// Metrics at top level (for backward compatibility, merged into capabilities)
    #[serde(default)]
    pub metrics: Vec<MetricInfo>,

    #[serde(default)]
    pub builds: HashMap<String, ExtensionBuild>,

    #[serde(default)]
    pub requirements: ExtensionRequirements,

    /// Safety/isolation settings (can also be specified as isolation)
    #[serde(default, alias = "isolation")]
    pub safety: ExtensionSafety,

    /// Configuration parameters for the extension
    #[serde(default)]
    pub config_parameters: Vec<ConfigParameterInfo>,

    /// Dashboard components provided by this extension
    #[serde(default)]
    pub dashboard_components: Vec<DashboardComponentInfo>,

    /// Frontend components (for backward compatibility)
    #[serde(default)]
    pub frontend: Option<FrontendInfo>,

    /// Permissions required by the extension
    #[serde(default)]
    pub permissions: Vec<String>,
}

fn default_extension_type() -> String {
    "native".to_string()
}

/// Extension capabilities from metadata
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtensionCapabilities {
    #[serde(default)]
    pub tools: Vec<ToolDescriptor>,
    #[serde(default)]
    pub metrics: Vec<MetricInfo>,
    #[serde(default)]
    pub commands: Vec<CommandInfo>,
    /// Streaming capability (for video/image processing extensions)
    #[serde(default)]
    pub streaming: Option<StreamingCapability>,
}

/// Tool descriptor from marketplace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescriptor {
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub parameters: serde_json::Value,
    #[serde(default)]
    pub returns: Option<String>,
}

/// Metric info from marketplace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricInfo {
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    /// Data type (can also be specified as "type")
    #[serde(default, alias = "type")]
    pub data_type: String,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
}

/// Command info from marketplace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandInfo {
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub parameters: serde_json::Value,
}

/// Streaming capability definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingCapability {
    pub mode: String,
    pub direction: String,
    #[serde(default)]
    pub supported_data_types: Vec<String>,
    #[serde(default)]
    pub max_chunk_size: usize,
    #[serde(default)]
    pub preferred_chunk_size: usize,
    #[serde(default)]
    pub max_concurrent_sessions: usize,
}

/// Configuration parameter info from marketplace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigParameterInfo {
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "type", default)]
    pub param_type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    #[serde(default)]
    pub minimum: Option<f64>,
    #[serde(default)]
    pub maximum: Option<f64>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    #[serde(rename = "enum")]
    pub enum_values: Vec<String>,
}

/// Dashboard component info from marketplace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardComponentInfo {
    #[serde(rename = "type")]
    pub component_type: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub bundle_path: Option<String>,
    #[serde(default)]
    pub export_name: Option<String>,
}

/// Extension build info for different platforms
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionBuild {
    pub url: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub size: usize,
    /// For WASM: URL to the JSON metadata sidecar file
    #[serde(default)]
    pub json_url: Option<String>,
    /// For WASM: SHA256 of the JSON file
    #[serde(default)]
    pub json_sha256: Option<String>,
}

/// Extension requirements
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtensionRequirements {
    #[serde(default)]
    pub min_neomind_version: String,
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub api_keys: Vec<String>,
}

/// Extension safety limits
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtensionSafety {
    #[serde(default)]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub max_memory_mb: usize,
}

/// Response for listing marketplace extensions
#[derive(Debug, Serialize)]
pub struct MarketplaceListResponse {
    pub extensions: Vec<CloudExtension>,
    pub total: usize,
}

/// Request to install an extension from marketplace
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct MarketplaceInstallRequest {
    pub id: String,
    #[serde(default)]
    pub version: Option<String>,
}

/// Response for install operation
#[derive(Debug, Serialize)]
pub struct MarketplaceInstallResponse {
    pub success: bool,
    pub extension_id: String,
    pub downloaded: bool,
    pub installed: bool,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

/// GET /api/extensions/market/list
///
/// List available extensions from the marketplace
#[utoipa::path(
    get,
    path = "/api/extensions/market/list",
    tag = "extensions",
    responses(
        (status = 200, description = "Marketplace catalog"),
    )
)]
pub async fn list_marketplace_extensions_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    // Use timestamp-only cache-busting to avoid CDN caching issues
    // without requiring version sync between repos
    let cache_buster = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let index_url = format!(
        "{}/{}/extensions/index.json?t={}",
        extension_market_base_url().as_str(),
        MARKET_BRANCH,
        cache_buster
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ErrorResponse::internal(format!("Failed to build HTTP client: {}", e)))?;

    let response = match client
        .get(&index_url)
        .header("User-Agent", "NeoMind-Extension-Marketplace")
        .header("Cache-Control", "no-cache")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Failed to connect to marketplace: {}", e);
            return ok(json!({
                "extensions": [],
                "total": 0,
                "error": "network_error",
                "message": "Unable to connect to extension marketplace. Please check your internet connection."
            }));
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        tracing::error!("Marketplace returned status {}", status);
        return ok(json!({
            "extensions": [],
            "total": 0,
            "error": format!("http_error_{}", status.as_u16()),
        }));
    }

    #[derive(Deserialize)]
    struct MarketIndex {
        version: String,
        extensions: Vec<CloudExtension>,
    }

    let index: MarketIndex = match response.json().await {
        Ok(i) => i,
        Err(e) => {
            tracing::error!("Failed to parse marketplace index: {}", e);
            return ok(json!({
                "extensions": [],
                "total": 0,
                "error": "parse_error",
            }));
        }
    };

    ok(json!({
        "extensions": index.extensions,
        "total": index.extensions.len(),
        "market_version": index.version,
    }))
}

/// GET /api/extensions/market/:id
///
/// Get detailed metadata for a specific extension from marketplace
#[utoipa::path(
    get,
    path = "/api/extensions/market/{id}",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
    ),
    responses(
        (status = 200, description = "Marketplace listing detail"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_marketplace_extension_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<MarketplaceExtensionMetadata> {
    validate_extension_id(&id)?;
    let metadata_url = format!(
        "{}/{}/extensions/{}/metadata.json",
        extension_market_base_url().as_str(),
        MARKET_BRANCH,
        id
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ErrorResponse::internal(format!("Failed to build HTTP client: {}", e)))?;

    let response = client
        .get(&metadata_url)
        .header("User-Agent", "NeoMind-Extension-Marketplace")
        .send()
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to fetch metadata: {}", e)))?;

    if !response.status().is_success() {
        return Err(ErrorResponse::not_found(format!(
            "Extension {} not found in marketplace",
            id
        )));
    }

    let metadata: MarketplaceExtensionMetadata = response
        .json()
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to parse metadata: {}", e)))?;

    ok(metadata)
}

/// Response for the README fetch — `content` is `null` when the README
/// doesn't exist (README is optional, so a missing one is not an error).
#[derive(Debug, serde::Serialize)]
pub struct ExtensionReadmeResponse {
    pub content: Option<String>,
}

/// GET /api/extensions/market/:id/readme
///
/// Fetch the README.md content for a specific extension from the marketplace.
/// Returns `{ content: null }` when the README does not exist or the fetch
/// fails — README is optional, so this best-effort endpoint never reports a
/// hard error (the frontend just hides the README section).
#[utoipa::path(
    get,
    path = "/api/extensions/market/{id}/readme",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
    ),
    responses(
        (status = 200, description = "Marketplace listing README (markdown)"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_marketplace_extension_readme_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<ExtensionReadmeResponse> {
    validate_extension_id(&id)?;
    let readme_url = format!(
        "{}/{}/extensions/{}/README.md",
        extension_market_base_url().as_str(),
        MARKET_BRANCH,
        id
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ErrorResponse::internal(format!("Failed to build HTTP client: {}", e)))?;

    let response = client
        .get(&readme_url)
        .header("User-Agent", "NeoMind-Extension-Marketplace")
        .send()
        .await;

    // README missing (or transient fetch failure) is normal → content: null.
    let content = match response {
        Ok(r) if r.status().is_success() => r.text().await.ok(),
        _ => None,
    };

    ok(ExtensionReadmeResponse { content })
}

/// Detect current platform for extension download
/// Returns platform string in hyphen format (e.g., "darwin-aarch64")
/// This matches the format used in marketplace metadata `builds` keys
pub(crate) fn detect_platform() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "darwin-aarch64"
    }

    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "darwin-x86_64"
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "linux-x86_64"
    }

    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        "linux-aarch64"
    }

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "windows-x86_64"
    }

    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    {
        "windows-aarch64"
    }

    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "aarch64")
    )))]
    {
        "unknown"
    }
}

/// Select the best marketplace `builds` key for the current platform + variant.
///
/// Candidate order: variant-specific key(s) → base platform → `wasm` (universal).
/// Returns the first key present in `available_keys`, or `None` if nothing matches.
///
/// Pure function (no I/O) so it can be unit-tested without network/state.
/// Caller (marketplace install) is responsible for the "unknown platform" guard
/// and for producing a user-facing error when this returns `None`.
fn select_build_key(
    available_keys: &std::collections::HashSet<&str>,
    base_platform: &str,
    variant: neomind_core::extension::accel::Variant,
) -> Option<String> {
    let mut candidates = neomind_core::extension::accel::fallback_keys(base_platform, variant);
    candidates.push("wasm".to_string());
    candidates
        .into_iter()
        .find(|k| available_keys.contains(k.as_str()))
}

/// Compute SHA256 checksum of file content
/// RAII guard that removes a path when dropped (best-effort). Used to make
/// sure a downloaded temp file is cleaned up on every exit path (success,
/// error, panic) without a manual `remove_file` at each return.
struct AutoRemove(std::path::PathBuf);
impl Drop for AutoRemove {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Stream a reqwest response body to a temp file, enforcing a max size.
/// Returns the temp file path. Size is enforced via Content-Length (when
/// reported) AND a running byte counter during streaming (defends against
/// wrong/missing Content-Length headers). Never buffers the whole body in
/// memory — each chunk is written and dropped. Caller owns the returned
/// path and is responsible for removing it (typically via [`AutoRemove`]).
async fn download_to_temp_file(
    response: reqwest::Response,
    max_size: u64,
    label: &str,
) -> Result<std::path::PathBuf, String> {
    if let Some(len) = response.content_length() {
        if len > max_size {
            return Err(format!(
                "Package too large: {} bytes reported (max {} bytes)",
                len, max_size
            ));
        }
    }
    let tmp_path = std::env::temp_dir().join(format!(
        "neomind-{}-{}-{}.tmp",
        label,
        std::process::id(),
        tmp_counter()
    ));
    let mut file = tokio::fs::File::create(&tmp_path)
        .await
        .map_err(|e| format!("Failed to create temp file: {}", e))?;
    let mut stream = response.bytes_stream();
    let mut total: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            // Clean up the partial temp file on stream error.
            let _ = std::fs::remove_file(&tmp_path);
            format!("Download stream error: {}", e)
        })?;
        total += chunk.len() as u64;
        if total > max_size {
            drop(file);
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(format!(
                "Package exceeded max size of {} bytes during download (got {})",
                max_size, total
            ));
        }
        file.write_all(&chunk).await.map_err(|e| {
            // Clean up the partial temp file on write failure (disk full,
            // quota, perms) — the chunk-error and oversize paths already
            // do this; without it a failed install leaks up to max_size bytes.
            let _ = std::fs::remove_file(&tmp_path);
            format!("Failed to write temp file: {}", e)
        })?;
    }
    file.flush().await.map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("Failed to flush temp file: {}", e)
    })?;
    tracing::info!("Downloaded {} bytes to {}", total, tmp_path.display());
    Ok(tmp_path)
}

/// Monotonic counter for unique temp file names across concurrent installs.
fn tmp_counter() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static C: AtomicU64 = AtomicU64::new(0);
    C.fetch_add(1, Ordering::SeqCst)
}

fn compute_sha256(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Stream-hash a file's contents for SHA256 verification without buffering
/// the whole file in memory. Used by the download path to verify large
/// binaries (ORT libs, model blobs) that were streamed to a temp file.
fn compute_sha256_of_file(path: &std::path::Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut hasher = Sha256::new();
    let mut f = std::fs::File::open(path)?;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// POST /api/extensions/market/install
///
/// Download and install an extension from the marketplace
#[utoipa::path(
    post,
    path = "/api/extensions/market/install",
    tag = "extensions",
    request_body = MarketplaceInstallRequest,
    responses(
        (status = 200, description = "Marketplace extension downloaded and installed"),
    )
)]
pub async fn install_marketplace_extension_handler(
    State(state): State<ServerState>,
    Json(req): Json<MarketplaceInstallRequest>,
) -> HandlerResult<MarketplaceInstallResponse> {
    let install_start = std::time::Instant::now();
    let runtime = &state.extensions.runtime;

    // Same id grammar as the local endpoints: req.id is interpolated into
    // a URL path — `../..` would turn the marketplace client into a
    // limited arbitrary-GET gadget against whatever host the (admin-set)
    // market base points at.
    validate_extension_id(&req.id)?;
    tracing::info!(extension_id = %req.id, "Starting marketplace extension install");

    // First fetch metadata to get download URL
    // Add cache-busting to avoid GitHub CDN serving stale content
    let cache_buster = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let metadata_url = format!(
        "{}/{}/extensions/{}/metadata.json?t={}",
        extension_market_base_url().as_str(),
        MARKET_BRANCH,
        req.id,
        cache_buster
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| ErrorResponse::internal(format!("Failed to build HTTP client: {}", e)))?;

    // Fetch metadata
    let metadata: MarketplaceExtensionMetadata = match client
        .get(&metadata_url)
        .header("User-Agent", "NeoMind-Extension-Marketplace")
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => match r.json().await {
            Ok(m) => m,
            Err(e) => {
                return ok(MarketplaceInstallResponse {
                    success: false,
                    extension_id: req.id,
                    downloaded: false,
                    installed: false,
                    path: None,
                    error: Some(format!("Failed to parse metadata: {}", e)),
                });
            }
        },
        Ok(r) => {
            return ok(MarketplaceInstallResponse {
                success: false,
                extension_id: req.id.clone(),
                downloaded: false,
                installed: false,
                path: None,
                error: Some(format!("Extension not found: {}", r.status())),
            });
        }
        Err(e) => {
            return ok(MarketplaceInstallResponse {
                success: false,
                extension_id: req.id.clone(),
                downloaded: false,
                installed: false,
                path: None,
                error: Some(format!("Network error: {}", e)),
            });
        }
    };

    // Detect platform + hardware variant (jetson/cuda/cpu) so jetson/cuda
    // installs download the accelerated .nep build, not the plain CPU one.
    // Mirrors the legacy binary branch below (which already uses select_build_key);
    // the .nep branch previously skipped variant selection entirely.
    let platform = detect_platform();
    let variant = neomind_core::extension::accel::detect_variant();

    // Build platform-specific .nep package URL from builds metadata
    // The package_url field in metadata is hardcoded to darwin_aarch64, so we ignore it
    // and use the correct URL from builds for the current platform
    let (package_url, expected_sha256): (Option<String>, Option<String>) = if platform != "unknown"
    {
        let available_keys: std::collections::HashSet<&str> =
            metadata.builds.keys().map(|s| s.as_str()).collect();
        select_build_key(&available_keys, platform, variant)
            .and_then(|key| {
                metadata.builds.get(key.as_str()).map(|b| {
                    (
                        Some(b.url.clone()),
                        if b.sha256.is_empty() {
                            None
                        } else {
                            Some(b.sha256.clone())
                        },
                    )
                })
            })
            .unwrap_or((None, None))
    } else {
        (
            metadata.package_url.clone(),
            metadata.package_sha256.clone(),
        )
    };

    // Check if .nep package is available (preferred method)
    if let Some(ref package_url) = package_url {
        tracing::info!(
            "Downloading .nep package for extension {} from {}",
            req.id,
            package_url
        );

        // Download the .nep package
        let package_response = match client
            .get(package_url)
            .header("User-Agent", "NeoMind-Extension-Marketplace")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return ok(MarketplaceInstallResponse {
                    success: false,
                    extension_id: req.id.clone(),
                    downloaded: false,
                    installed: false,
                    path: None,
                    error: Some(format!("Failed to download package: {}", e)),
                });
            }
        };

        if !package_response.status().is_success() {
            return ok(MarketplaceInstallResponse {
                success: false,
                extension_id: req.id.clone(),
                downloaded: false,
                installed: false,
                path: None,
                error: Some(format!(
                    "Package download failed: {}",
                    package_response.status()
                )),
            });
        }

        // Stream the package body to a temp file (never buffer the whole
        // .nep in memory — large ML model bundles would OOM). The temp file
        // is cleaned up via _guard on every exit path.
        let tmp_path =
            match download_to_temp_file(package_response, MAX_EXTENSION_DOWNLOAD_SIZE, &req.id)
                .await
            {
                Ok(p) => p,
                Err(msg) => {
                    return ok(MarketplaceInstallResponse {
                        success: false,
                        extension_id: req.id.clone(),
                        downloaded: false,
                        installed: false,
                        path: None,
                        error: Some(msg),
                    });
                }
            };
        let _guard = AutoRemove(tmp_path.clone());

        // Verify it's a valid ZIP file by reading only the magic bytes from
        // the temp file (don't load the whole package to check 4 bytes).
        let mut magic_buf = [0u8; 4];
        if let Ok(mut magic_file) = tokio::fs::File::open(&tmp_path).await {
            // read_exact errors (file < 4 bytes) → magic_buf stays partial/zero → not a zip.
            let _ = magic_file.read_exact(&mut magic_buf).await;
        }
        let is_zip = magic_buf == [0x50, 0x4B, 0x03, 0x04]
            || magic_buf == [0x50, 0x4B, 0x05, 0x06]
            || magic_buf == [0x50, 0x4B, 0x07, 0x08];

        if !is_zip {
            return ok(MarketplaceInstallResponse {
                success: false,
                extension_id: req.id.clone(),
                downloaded: false,
                installed: false,
                path: None,
                error: Some("Downloaded file is not a valid .nep package (ZIP format)".to_string()),
            });
        }

        // The marketplace index does not carry sha256 yet (the release
        // pipeline computes checksums but never publishes them), so the
        // verify branch below is effectively dead for today's metadata.
        // Surface that loudly instead of installing silently unverified, and
        // give strict deployments a fail-closed switch.
        let mut expected_sha256 = expected_sha256;
        if expected_sha256.is_none() {
            let filename = package_url.rsplit('/').next().unwrap_or("");
            if let Some(sha) = fetch_release_checksum(&client, package_url, filename).await {
                tracing::info!(
                    extension_id = %req.id,
                    "Package SHA256 sourced from the release checksums.txt"
                );
                expected_sha256 = Some(sha);
            }
        }
        if expected_sha256.is_none() {
            let strict = std::env::var("NEOMIND_STRICT_PACKAGE_SHA256")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            if strict {
                let _ = std::fs::remove_file(&tmp_path);
                return ok(MarketplaceInstallResponse {
                    success: false,
                    extension_id: req.id.clone(),
                    downloaded: true,
                    installed: false,
                    path: None,
                    error: Some(
                        "Marketplace metadata carries no sha256 and NEOMIND_STRICT_PACKAGE_SHA256 is enabled — refusing to install an unverified package"
                            .to_string(),
                    ),
                });
            }
            tracing::warn!(
                extension_id = %req.id,
                url = %package_url,
                "Marketplace metadata carries no sha256 — integrity check skipped (set NEOMIND_STRICT_PACKAGE_SHA256=1 to refuse unverified packages)"
            );
        }

        // Verify SHA256 when the marketplace metadata provides one. Defends
        // against CDN/transport corruption or a swapped package being loaded
        // into the process via dlopen. The legacy binary branch already does
        // this; the .nep branch previously only checked the 4-byte ZIP magic.
        if let Some(ref expected) = expected_sha256 {
            match compute_sha256_of_file(&tmp_path) {
                Ok(actual) if actual == *expected => {
                    tracing::debug!(extension_id = %req.id, "Package SHA256 verified");
                }
                Ok(actual) => {
                    return ok(MarketplaceInstallResponse {
                        success: false,
                        extension_id: req.id.clone(),
                        downloaded: true,
                        installed: false,
                        path: None,
                        error: Some(format!(
                            "Package SHA256 mismatch: expected {}, got {} — refusing to install",
                            expected, actual
                        )),
                    });
                }
                Err(e) => {
                    // Fail CLOSED: the whole point of the pinned sha is that
                    // an unverifiable artifact never reaches dlopen. A read
                    // error means we cannot prove integrity — refuse.
                    tracing::error!(
                        extension_id = %req.id,
                        error = %e,
                        "Failed to compute package SHA256 — refusing to install"
                    );
                    let _ = std::fs::remove_file(&tmp_path);
                    return Err(ErrorResponse::internal(format!(
                        "Failed to verify package integrity (sha256 read error: {e}) — install aborted"
                    )));
                }
            }
        }

        // Prepare target directory
        let data_dir = neomind_core::paths::data_dir()
            .to_string_lossy()
            .to_string();
        let target_dir = PathBuf::from(data_dir).join("extensions");

        // Install from the temp file — streams the ZIP from disk, not memory.
        // (install_from_file reads manifest + validates internally, so the old
        // from_bytes() pre-check is redundant and dropped.)
        let tmp_path_clone = tmp_path.clone();
        let target_dir_clone = target_dir.clone();
        let install_result = tokio::task::spawn_blocking(move || {
            use neomind_core::extension::package::ExtensionPackage;
            ExtensionPackage::install_from_file(&tmp_path_clone, &target_dir_clone)
        })
        .await;
        // _guard drops here on early return / end of scope → removes tmp_path.

        match install_result {
            Ok(Ok(result)) => {
                let ext_id = result.extension_id.clone();
                let version = result.version.clone();

                tracing::info!(
                    extension_id = %ext_id,
                    version = %version,
                    binary_path = %result.binary_path.display(),
                    elapsed_ms = install_start.elapsed().as_millis() as u64,
                    "Package downloaded and extracted, starting extension load"
                );

                // Check if already registered and unregister if needed
                let is_registered = runtime.contains(&ext_id).await;

                if is_registered {
                    tracing::info!("Extension {} already registered, will replace", ext_id);
                    if let Err(e) = runtime.unregister(&ext_id).await {
                        return ok(MarketplaceInstallResponse {
                            success: false,
                            extension_id: req.id.clone(),
                            downloaded: true,
                            installed: false,
                            path: Some(result.binary_path.to_string_lossy().to_string()),
                            error: Some(format!("Failed to unregister existing extension: {}", e)),
                        });
                    }
                }

                // Load and register the extension binary (unified handles isolated/in-process)
                match runtime.load(&result.binary_path).await {
                    Ok(ext_metadata) => {
                        // Determine extension type from binary path
                        let extension_type = result
                            .binary_path
                            .extension()
                            .and_then(|e| e.to_str())
                            .map(|e| if e == "wasm" { "wasm" } else { "native" })
                            .unwrap_or("native")
                            .to_string();

                        // Save to storage. An upgrade/reinstall overwrites the
                        // whole row — carry the previous user config (YOLO
                        // thresholds, bindings, …) into the new record so it
                        // survives, instead of resetting to defaults.
                        let store = state.extensions.store.clone();
                        let preserved_config = store
                            .load(&ext_id)
                            .ok()
                            .flatten()
                            .and_then(|old| old.config.clone());
                        {
                            let mut record = ExtensionRecord::new(
                                ext_id.clone(),
                                ext_metadata.name.clone(),
                                result.binary_path.to_string_lossy().to_string(),
                                extension_type,
                                version.clone(),
                            )
                            .with_description(ext_metadata.description.clone())
                            .with_author(ext_metadata.author.clone())
                            .with_checksum(Some(result.checksum.clone()))
                            .with_auto_start(true)
                            .with_frontend_path(
                                result
                                    .frontend_dir
                                    .as_ref()
                                    .map(|p| p.to_string_lossy().to_string()),
                            );
                            if let Some(cfg) = preserved_config.clone() {
                                record = record.with_config(cfg);
                            }

                            if let Err(e) = store.save(&record) {
                                tracing::warn!("Failed to save extension to storage: {}", e);
                            }
                        }

                        // Rebuild tool registry so the new extension's tools are visible to the LLM
                        state.refresh_extension_tools().await;

                        // Push the preserved config to the freshly-started
                        // runtime — without this the running process stays on
                        // defaults until the next restart (same gap the reload
                        // path closes with its send_config_update).
                        if let Some(cfg) = preserved_config {
                            if let Err(e) = state
                                .extensions
                                .runtime
                                .send_config_update(&ext_id, &cfg)
                                .await
                            {
                                tracing::warn!(
                                    extension_id = %ext_id,
                                    error = %e,
                                    "Failed to apply preserved config after market upgrade"
                                );
                            }
                        }

                        ok(MarketplaceInstallResponse {
                            success: true,
                            extension_id: ext_id,
                            downloaded: true,
                            installed: true,
                            path: Some(result.binary_path.to_string_lossy().to_string()),
                            error: None,
                        })
                    }
                    Err(e) => ok(MarketplaceInstallResponse {
                        success: false,
                        extension_id: req.id.clone(),
                        downloaded: true,
                        installed: false,
                        path: Some(result.binary_path.to_string_lossy().to_string()),
                        error: Some(format!("Failed to load extension binary: {}", e)),
                    }),
                }
            }
            Ok(Err(e)) => ok(MarketplaceInstallResponse {
                success: false,
                extension_id: req.id.clone(),
                downloaded: true,
                installed: false,
                path: None,
                error: Some(format!("Package installation failed: {}", e)),
            }),
            Err(e) => ok(MarketplaceInstallResponse {
                success: false,
                extension_id: req.id.clone(),
                downloaded: true,
                installed: false,
                path: None,
                error: Some(format!("Task join error: {}", e)),
            }),
        }
    } else {
        // Fall back to platform-specific binary download with variant fallback.
        // Variant chain (via accel): e.g. Jetson → linux-aarch64-jetson → linux-aarch64 → wasm.
        let variant = neomind_core::extension::accel::detect_variant();

        // "unknown" platform is only acceptable for pure-WASM extensions.
        if platform == "unknown" && !metadata.builds.contains_key("wasm") {
            return ok(MarketplaceInstallResponse {
                success: false,
                extension_id: req.id,
                downloaded: false,
                installed: false,
                path: None,
                error: Some("Unsupported platform".to_string()),
            });
        }

        let available_keys: std::collections::HashSet<&str> =
            metadata.builds.keys().map(|s| s.as_str()).collect();
        let build_key = select_build_key(&available_keys, platform, variant).ok_or_else(|| {
            ErrorResponse::bad_request(format!(
                "No compatible build for {} (variant={:?}); available builds: {:?}",
                platform,
                variant,
                metadata.builds.keys().collect::<Vec<_>>()
            ))
        })?;

        // Get build info for the selected platform/variant/WASM.
        // SAFE: select_build_key only returns a key present in `available_keys`,
        // which we built from metadata.builds.keys() — so .get() is guaranteed Some.
        // (If select_build_key is ever refactored to emit keys NOT derived from
        // available_keys, this invariant breaks — keep it subset-bound.)
        let build = metadata
            .builds
            .get(build_key.as_str())
            .expect("select_build_key invariant: returned key must exist in metadata.builds");

        // Determine if this is a WASM build for file extension logic (used later)
        let is_wasm = build_key == "wasm";

        // Download the extension binary
        tracing::info!("Downloading extension {} from {}", req.id, build.url);

        let download_response = client
            .get(&build.url)
            .send()
            .await
            .map_err(|e| ErrorResponse::internal(format!("Download failed: {}", e)))?;

        if !download_response.status().is_success() {
            return ok(MarketplaceInstallResponse {
                success: false,
                extension_id: req.id,
                downloaded: false,
                installed: false,
                path: None,
                error: Some(format!("Download failed: {}", download_response.status())),
            });
        }

        // Stream the binary to a temp file (don't buffer large ORT/model
        // blobs in memory). _guard removes the temp file on every exit path.
        let tmp_path =
            download_to_temp_file(download_response, MAX_EXTENSION_DOWNLOAD_SIZE, &req.id)
                .await
                .map_err(ErrorResponse::internal)?;
        let _guard = AutoRemove(tmp_path.clone());

        // Verify SHA256 if provided — hash the temp file in a streaming fashion.
        if !build.sha256.is_empty() {
            let checksum = compute_sha256_of_file(&tmp_path).map_err(|e| {
                ErrorResponse::internal(format!("Failed to hash downloaded file: {}", e))
            })?;
            if checksum != build.sha256 {
                return ok(MarketplaceInstallResponse {
                    success: false,
                    extension_id: req.id,
                    downloaded: false,
                    installed: false,
                    path: None,
                    error: Some(format!(
                        "Checksum verification failed: expected {}, got {}",
                        build.sha256, checksum
                    )),
                });
            }
        }

        // Determine file extension and naming based on type
        let (ext, wasm_filename, json_filename) = if is_wasm {
            (
                ".wasm",
                format!("{}.wasm", req.id.replace("-", "_")),
                format!("{}.json", req.id.replace("-", "_")),
            )
        } else if platform.starts_with("darwin") {
            (".dylib", String::new(), String::new())
        } else if platform.starts_with("linux") {
            (".so", String::new(), String::new())
        } else if platform.starts_with("windows") {
            (".dll", String::new(), String::new())
        } else {
            ("", String::new(), String::new())
        };

        // Create extensions directory using NEOMIND_DATA_DIR for consistency
        let data_dir = neomind_core::paths::data_dir()
            .to_string_lossy()
            .to_string();
        let extensions_dir = PathBuf::from(data_dir).join("extensions");

        std::fs::create_dir_all(&extensions_dir).map_err(|e| {
            ErrorResponse::internal(format!("Failed to create extensions directory: {}", e))
        })?;

        tracing::info!(
            extensions_dir = %extensions_dir.display(),
            "Installing extension from legacy binary format"
        );

        // Write the extension file(s)
        let (file_path, json_path) = if is_wasm {
            // WASM: write both .wasm and .json files
            let wasm_path = extensions_dir.join(&wasm_filename);

            std::fs::copy(&tmp_path, &wasm_path)
                .map_err(|e| ErrorResponse::internal(format!("Failed to copy WASM file: {}", e)))?;

            // Download and write JSON sidecar
            let json_path = extensions_dir.join(&json_filename);

            if let Some(json_url) = &build.json_url {
                let json_response =
                    client.get(json_url).send().await.map_err(|e| {
                        ErrorResponse::internal(format!("JSON download failed: {}", e))
                    })?;

                if json_response.status().is_success() {
                    let json_bytes = json_response.bytes().await.map_err(|e| {
                        ErrorResponse::internal(format!("Failed to read JSON: {}", e))
                    })?;

                    // Verify JSON SHA256 if provided
                    if let Some(ref expected_sha) = build.json_sha256 {
                        if !expected_sha.is_empty() {
                            let json_checksum = compute_sha256(&json_bytes);
                            if json_checksum != *expected_sha {
                                // Clean up on verification failure
                                let _ = std::fs::remove_file(&wasm_path);
                                return ok(MarketplaceInstallResponse {
                                    success: false,
                                    extension_id: req.id,
                                    downloaded: true,
                                    installed: false,
                                    path: None,
                                    error: Some("JSON checksum verification failed".to_string()),
                                });
                            }
                        }
                    }

                    std::fs::write(&json_path, &json_bytes).map_err(|e| {
                        ErrorResponse::internal(format!("Failed to write JSON file: {}", e))
                    })?;
                } else {
                    // Copy local JSON if download fails (fallback)
                    let local_json = format!(
                        "extensions/{}/{}.json",
                        req.id.replace("-", "_"),
                        req.id.replace("-", "_")
                    );
                    if PathBuf::from(&local_json).exists() {
                        let _ = std::fs::copy(&local_json, &json_path);
                    }
                }
            }

            tracing::info!(
                "WASM extension downloaded to: {:?} + {:?}",
                wasm_path,
                json_path
            );
            (wasm_path, Some(json_path))
        } else {
            // Native: write single binary file
            let filename = format!("libneomind_extension_{}{}", req.id, ext);
            let file_path = extensions_dir.join(&filename);

            std::fs::copy(&tmp_path, &file_path).map_err(|e| {
                ErrorResponse::internal(format!("Failed to copy extension file: {}", e))
            })?;

            tracing::info!("Extension downloaded to: {:?}", file_path);
            (file_path, None)
        };

        match runtime.load(&file_path).await {
            Ok(_) => {
                // Save to persistent storage
                let store = state.extensions.store.clone();
                {
                    let record = ExtensionRecord::new(
                        metadata.id.clone(),
                        metadata.name.clone(),
                        file_path.to_string_lossy().to_string(),
                        String::new(),
                        metadata.version.clone(),
                    )
                    .with_description(Some(metadata.description.clone()))
                    .with_author(Some(metadata.author.clone()))
                    .with_auto_start(true);

                    if let Err(e) = store.save(&record) {
                        tracing::warn!("Failed to save extension to storage: {}", e);
                    }
                }

                tracing::info!("Extension {} installed successfully", req.id);

                // Rebuild tool registry so the new extension's tools are visible to the LLM
                state.refresh_extension_tools().await;

                ok(MarketplaceInstallResponse {
                    success: true,
                    extension_id: req.id,
                    downloaded: true,
                    installed: true,
                    path: Some(file_path.to_string_lossy().to_string()),
                    error: None,
                })
            }
            Err(e) => {
                // Clean up the downloaded file(s) on failure
                let _ = std::fs::remove_file(&file_path);
                if let Some(ref jp) = json_path {
                    let _ = std::fs::remove_file(jp);
                }

                ok(MarketplaceInstallResponse {
                    success: false,
                    extension_id: req.id,
                    downloaded: true,
                    installed: false,
                    path: None,
                    error: Some(format!("Failed to load extension: {}", e)),
                })
            }
        }
    }
}

/// GET /api/extensions/market/updates
///
/// Check for updates for installed extensions
#[utoipa::path(
    get,
    path = "/api/extensions/market/updates",
    tag = "extensions",
    responses(
        (status = 200, description = "Available updates for installed extensions"),
    )
)]
pub async fn check_marketplace_updates_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let installed = state.extensions.runtime.list().await;

    let mut updates = Vec::new();

    // For each installed extension, check if there's a newer version in marketplace
    for ext_info in installed {
        let ext_id = ext_info.metadata.id.clone();

        // Fetch metadata from marketplace
        let metadata_url = format!(
            "{}/{}/extensions/{}/metadata.json",
            extension_market_base_url().as_str(),
            MARKET_BRANCH,
            ext_id
        );

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build();

        if let Ok(client) = client {
            if let Ok(response) = client
                .get(&metadata_url)
                .header("User-Agent", "NeoMind-Extension-Marketplace")
                .send()
                .await
            {
                if response.status().is_success() {
                    if let Ok(metadata) = response.json::<MarketplaceExtensionMetadata>().await {
                        // Semver comparison, and only upgrades count. The old
                        // string `!=` flagged downgrades and build-suffix
                        // mismatches as "updates" — combined with extensions
                        // that hardcoded their version, some entries showed a
                        // permanent false "update available".
                        let newer = matches!(
                            (
                                metadata.version.parse::<semver::Version>(),
                                ext_info.metadata.version.parse::<semver::Version>(),
                            ),
                            (Ok(new), Ok(cur)) if new > cur
                        );
                        if newer {
                            updates.push(serde_json::json!({
                                "id": ext_id,
                                "name": ext_info.metadata.name,
                                "current_version": ext_info.metadata.version.to_string(),
                                "latest_version": metadata.version,
                                "categories": metadata.categories,
                            }));
                        }
                    }
                }
            }
        }
    }

    ok(json!({
        "updates_available": updates,
        "count": updates.len(),
    }))
}

// ============================================================================
// Extension Configuration API (V2)
// ============================================================================

#[cfg(test)]
mod select_build_key_tests {
    use super::select_build_key;
    use neomind_core::extension::accel::Variant;
    use std::collections::HashSet;

    fn keys(v: &[&'static str]) -> HashSet<&'static str> {
        v.iter().copied().collect()
    }

    #[test]
    fn jetson_picks_jetson_build_when_present() {
        let avail = keys(&["linux-aarch64-jetson", "linux-aarch64"]);
        assert_eq!(
            select_build_key(&avail, "linux-aarch64", Variant::Jetson),
            Some("linux-aarch64-jetson".to_string())
        );
    }

    #[test]
    fn jetson_falls_back_to_plain_when_no_jetson_build() {
        let avail = keys(&["linux-aarch64"]);
        assert_eq!(
            select_build_key(&avail, "linux-aarch64", Variant::Jetson),
            Some("linux-aarch64".to_string())
        );
    }

    #[test]
    fn cpu_picks_plain_build() {
        let avail = keys(&["linux-aarch64"]);
        assert_eq!(
            select_build_key(&avail, "linux-aarch64", Variant::Cpu),
            Some("linux-aarch64".to_string())
        );
    }

    #[test]
    fn native_preferred_over_wasm_when_both_present() {
        let avail = keys(&["linux-aarch64", "wasm"]);
        assert_eq!(
            select_build_key(&avail, "linux-aarch64", Variant::Jetson),
            Some("linux-aarch64".to_string())
        );
    }

    #[test]
    fn wasm_picked_for_pure_wasm_extension() {
        let avail = keys(&["wasm"]);
        assert_eq!(
            select_build_key(&avail, "linux-aarch64", Variant::Cpu),
            Some("wasm".to_string())
        );
    }

    #[test]
    fn no_match_returns_none() {
        let avail = keys(&["windows-x86_64"]);
        assert_eq!(
            select_build_key(&avail, "linux-aarch64", Variant::Jetson),
            None
        );
    }
}
