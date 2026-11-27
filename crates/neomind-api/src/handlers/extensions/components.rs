//! `components` handlers — split from the former extensions.rs monolith.

use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};

use crate::handlers::common::{ok, HandlerResult};
use crate::models::error::ErrorResponse;
use crate::server::ServerState;

use super::*;

/// Size constraints for dashboard components.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SizeConstraints {
    pub min_w: u32,
    pub min_h: u32,
    pub default_w: u32,
    pub default_h: u32,
    pub max_w: u32,
    pub max_h: u32,
    pub preserve_aspect: Option<bool>,
}

/// Data binding configuration for dashboard components.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataBindingConfig {
    pub extension_metric: Option<String>,
    pub extension_command: Option<String>,
    pub required_fields: Vec<String>,
}

/// Dashboard component definition from manifest.
/// Uses String for category to be compatible with neomind-core's definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardComponentDef {
    #[serde(rename = "type")]
    pub component_type: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub icon: Option<String>,
    pub bundle_path: String,
    pub export_name: String,
    /// Global variable name for the bundle (used for script tag loading)
    #[serde(default)]
    pub global_name: Option<String>,
    #[serde(default)]
    pub size_constraints: SizeConstraints,
    #[serde(default)]
    pub has_data_source: bool,
    #[serde(default)]
    pub has_display_config: bool,
    #[serde(default)]
    pub has_actions: bool,
    #[serde(default)]
    pub max_data_sources: u8,
    /// Whether this component supports device binding (receives deviceContext)
    #[serde(default)]
    pub has_device_binding: bool,
    pub config_schema: Option<serde_json::Value>,
    pub data_source_schema: Option<serde_json::Value>,
    pub default_config: Option<serde_json::Value>,
    #[serde(default)]
    pub variants: Vec<String>,
    #[serde(default)]
    pub data_binding: Option<DataBindingConfig>,
    /// Other fields that we don't parse (e.g., examples, etc.)
    #[serde(flatten)]
    pub _other: serde_json::Value,
}

/// Dashboard component DTO for API responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardComponentDto {
    /// Component type identifier
    #[serde(rename = "type")]
    pub component_type: String,
    /// Display name
    pub name: String,
    /// Description
    pub description: String,
    /// Component category
    pub category: String,
    /// Icon name (lucide-react)
    pub icon: Option<String>,
    /// Bundle URL (resolved)
    pub bundle_url: String,
    /// Export name in bundle
    pub export_name: String,
    /// Global variable name for the bundle (used for script tag loading)
    pub global_name: Option<String>,
    /// Size constraints
    pub size_constraints: SizeConstraintsDto,
    /// Whether this component accepts a data source
    pub has_data_source: bool,
    /// Whether this component has display configuration
    pub has_display_config: bool,
    /// Whether this component has actions
    pub has_actions: bool,
    /// Maximum number of data sources
    pub max_data_sources: u8,
    /// JSON Schema for component configuration
    pub config_schema: Option<serde_json::Value>,
    /// JSON Schema for data source binding
    pub data_source_schema: Option<serde_json::Value>,
    /// Default configuration values
    pub default_config: Option<serde_json::Value>,
    /// Component variants
    pub variants: Vec<String>,
    /// Data binding configuration
    pub data_binding: DataBindingDto,
    /// Data source allowed types (e.g., ["device"])
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_source_allowed_types: Option<Vec<String>>,
    /// Whether this component supports device binding (receives deviceContext)
    #[serde(default)]
    pub has_device_binding: bool,
    /// Extension ID
    pub extension_id: String,
}

/// Size constraints DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SizeConstraintsDto {
    pub min_w: u32,
    pub min_h: u32,
    pub default_w: u32,
    pub default_h: u32,
    pub max_w: u32,
    pub max_h: u32,
    pub preserve_aspect: Option<bool>,
}

impl From<SizeConstraints> for SizeConstraintsDto {
    fn from(c: SizeConstraints) -> Self {
        Self {
            min_w: c.min_w,
            min_h: c.min_h,
            default_w: c.default_w,
            default_h: c.default_h,
            max_w: c.max_w,
            max_h: c.max_h,
            preserve_aspect: c.preserve_aspect,
        }
    }
}

/// Data binding DTO.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DataBindingDto {
    pub extension_metric: Option<String>,
    pub extension_command: Option<String>,
    pub required_fields: Vec<String>,
}

impl From<DataBindingConfig> for DataBindingDto {
    fn from(c: DataBindingConfig) -> Self {
        Self {
            extension_metric: c.extension_metric,
            extension_command: c.extension_command,
            required_fields: c.required_fields,
        }
    }
}

/// Response for dashboard components list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardComponentsResponse {
    /// Extension ID
    pub extension_id: String,
    /// Extension name
    pub extension_name: String,
    /// Dashboard components provided by this extension
    pub components: Vec<DashboardComponentDto>,
}

/// Extension manifest JSON structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionManifest {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// Frontend configuration with components
    #[serde(default)]
    pub frontend: Option<FrontendConfigDef>,
    /// Other fields that we don't parse
    #[serde(flatten)]
    pub _other: serde_json::Value,
}

/// Frontend configuration in manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontendConfigDef {
    /// Dashboard components provided by this extension
    #[serde(default)]
    pub components: Vec<DashboardComponentDef>,
    /// Standalone pages contributed into the main app (routes + nav)
    #[serde(default)]
    pub pages: Vec<neomind_core::extension::package::ExtensionPageDef>,
}

/// GET /api/extensions/:id/components
/// Get dashboard components provided by an extension.
#[utoipa::path(
    get,
    path = "/api/extensions/{id}/components",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
    ),
    responses(
        (status = 200, description = "Frontend components contributed by an extension"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_extension_components_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<DashboardComponentsResponse> {
    // Check if extension exists using unified service
    let info = state
        .extensions
        .runtime
        .get(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    // Try to load manifest from extension directory
    let components = load_extension_components(&id, info.path.as_ref()).unwrap_or_default();

    let extension_name = info.metadata.name.clone();

    ok(DashboardComponentsResponse {
        extension_id: id,
        extension_name,
        components,
    })
}

/// Locate and parse an extension's manifest.json (shared discovery logic).
pub(crate) fn load_extension_manifest(
    extension_id: &str,
    file_path: Option<&std::path::PathBuf>,
) -> Option<ExtensionManifest> {
    // Log path configuration for debugging
    let data_dir = neomind_core::paths::data_dir()
        .to_string_lossy()
        .to_string();
    tracing::debug!(
        extension_id = %extension_id,
        data_dir = %data_dir,
        "Loading extension components (NEOMIND_DATA_DIR set)"
    );

    // If no file_path provided, try to find extension in data directory
    let file_path = if let Some(fp) = file_path {
        fp.clone()
    } else {
        // Try to find extension in data/extensions directory
        let data_dir = neomind_core::paths::data_dir()
            .to_string_lossy()
            .to_string();
        std::path::PathBuf::from(data_dir)
            .join("extensions")
            .join(extension_id)
    };

    tracing::debug!(
        extension_id = %extension_id,
        file_path = %file_path.display(),
        "Loading dashboard components for extension"
    );

    // Get the extension directory
    // For legacy format: file_path = extensions/xxx.<binary> -> ext_dir = extensions/
    // For .nep format: file_path = extensions/xxx/binaries/<platform>/lib.ext -> ext_dir should be extensions/xxx/
    // Determine the extension directory
    // If file_path is a directory, use it directly
    // If file_path is a file, use its parent directory
    let ext_dir = if file_path.is_dir() {
        file_path.clone()
    } else {
        file_path.parent()?.to_path_buf()
    };

    tracing::debug!(ext_dir = %ext_dir.display(), "Extension directory");

    // Determine the extension root directory
    // Check if we're in a .nep format (contains "binaries" directory)
    let extension_root = {
        let components: Vec<_> = ext_dir.components().collect();
        let binaries_idx = components.iter().position(|c| {
            if let std::path::Component::Normal(os_str) = c {
                os_str.to_str().map(|s| s == "binaries").unwrap_or(false)
            } else {
                false
            }
        });

        if let Some(idx) = binaries_idx {
            // .nep format: go up to the directory containing "binaries"
            let root: std::path::PathBuf = components[..idx].iter().collect();
            tracing::debug!(root = %root.display(), "Detected .nep format");
            root
        } else {
            // Legacy format: use parent as-is
            tracing::debug!(root = %ext_dir.display(), "Detected legacy format");
            ext_dir.to_path_buf()
        }
    };

    tracing::debug!(extension_root = %extension_root.display(), "Extension root directory");

    // Try multiple manifest locations in order:
    // 1. extension_root/manifest.json (.nep format)
    // 2. ext_dir/manifest.json (legacy format, same dir as library)
    // 3. ext_dir/{extension_id}/manifest.json
    // 4. ext_dir/{extension_name}/manifest.json
    let extension_name = extension_id
        .strip_prefix("neomind.")
        .unwrap_or(extension_id)
        .replace('.', "-");

    let manifest_paths = vec![
        extension_root.join("manifest.json"),
        ext_dir.join("manifest.json"),
        ext_dir.join(extension_id).join("manifest.json"),
        ext_dir.join(&extension_name).join("manifest.json"),
    ];

    tracing::debug!(
        paths = ?manifest_paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        "Trying manifest paths"
    );

    // Try each manifest path
    let mut manifest_content = None;
    for manifest_path in &manifest_paths {
        if manifest_path.exists() {
            match std::fs::read_to_string(manifest_path) {
                Ok(content) => {
                    manifest_content = Some(content);
                    break;
                }
                Err(e) => {
                    tracing::warn!(
                        manifest_path = %manifest_path.display(),
                        error = %e,
                        "Failed to read manifest.json"
                    );
                }
            }
        }
    }

    let manifest_content = manifest_content?;

    tracing::debug!(
        extension_id = %extension_id,
        content_len = manifest_content.len(),
        "Found manifest.json"
    );

    // Parse manifest
    let manifest: ExtensionManifest = match serde_json::from_str(&manifest_content) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(
                extension_id = %extension_id,
                error = %e,
                "Failed to parse manifest.json"
            );
            return None;
        }
    };

    Some(manifest)
}

/// Load dashboard components from an extension manifest.
pub(crate) fn load_extension_components(
    extension_id: &str,
    file_path: Option<&std::path::PathBuf>,
) -> Option<Vec<DashboardComponentDto>> {
    let manifest = load_extension_manifest(extension_id, file_path)?;

    // Convert component definitions to DTOs
    let base_url = format!("/api/extensions/{}/assets", extension_id);

    // Get components from frontend.components
    let components: Vec<DashboardComponentDef> =
        manifest.frontend.map(|f| f.components).unwrap_or_default();

    let components: Vec<DashboardComponentDto> = components
        .into_iter()
        .map(|def| DashboardComponentDto {
            component_type: def.component_type,
            name: def.name,
            description: def.description,
            category: def.category,
            icon: def.icon,
            bundle_url: format!("{}/{}", base_url, def.bundle_path.trim_start_matches('/')),
            export_name: def.export_name,
            global_name: def.global_name,
            size_constraints: SizeConstraintsDto::from(def.size_constraints),
            has_data_source: def.has_data_source,
            has_display_config: def.has_display_config,
            has_actions: def.has_actions,
            max_data_sources: def.max_data_sources,
            config_schema: def.config_schema,
            data_source_schema: def.data_source_schema,
            default_config: def.default_config,
            variants: def.variants,
            data_binding: def
                .data_binding
                .map(DataBindingDto::from)
                .unwrap_or_default(),
            data_source_allowed_types: def
                ._other
                .get("dataSourceAllowedTypes")
                .or_else(|| def._other.get("data_source_allowed_types"))
                .and_then(|v| serde_json::from_value(v.clone()).ok()),
            has_device_binding: def.has_device_binding,
            extension_id: extension_id.to_string(),
        })
        .collect();

    Some(components)
}

/// DTO for a page contributed by an extension into the main app.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExtensionPageDto {
    pub extension_id: String,
    /// Page type ID (unique per extension)
    #[serde(rename = "type")]
    pub page_type: String,
    pub title: String,
    pub description: String,
    /// Route path inside the main app
    pub route: String,
    /// Nav placement ("main" or a section id like "devices"); None = route only
    pub nav_section: Option<String>,
    pub nav_label: Option<String>,
    pub icon: Option<String>,
    pub bundle_url: String,
    pub export_name: String,
    pub global_name: Option<String>,
}

/// Load contributed pages from an extension manifest.
pub(crate) fn load_extension_pages(
    extension_id: &str,
    file_path: Option<&std::path::PathBuf>,
) -> Option<Vec<ExtensionPageDto>> {
    let manifest = load_extension_manifest(extension_id, file_path)?;
    let pages = manifest.frontend.map(|f| f.pages).unwrap_or_default();
    if pages.is_empty() {
        return None;
    }
    let base_url = format!("/api/extensions/{}/assets", extension_id);
    let dtos = pages
        .into_iter()
        .map(|def| ExtensionPageDto {
            extension_id: extension_id.to_string(),
            page_type: def.page_type,
            title: def.title,
            description: def.description,
            route: def.route,
            nav_section: def.nav_section,
            nav_label: def.nav_label,
            icon: def.icon,
            bundle_url: format!("{}/{}", base_url, def.bundle_path.trim_start_matches('/')),
            export_name: def.export_name,
            global_name: def.global_name,
        })
        .collect();
    Some(dtos)
}

/// GET /api/extensions/pages
/// List all pages contributed by installed extensions (public metadata,
/// same policy as dashboard-components).
#[utoipa::path(
    get,
    path = "/api/extensions/pages",
    tag = "extensions",
    responses(
        (status = 200, description = "All extension-contributed pages"),
    )
)]
pub async fn get_extension_pages_handler(
    State(state): State<ServerState>,
) -> HandlerResult<Vec<ExtensionPageDto>> {
    let mut all_pages = Vec::new();
    let all_extensions = state.extensions.runtime.list().await;
    for info in all_extensions {
        if let Some(pages) = load_extension_pages(&info.metadata.id, info.path.as_ref()) {
            all_pages.extend(pages);
        }
    }
    ok(all_pages)
}

/// Maximum allowed static asset size served from an extension directory (16 MiB).
const MAX_EXTENSION_ASSET_BYTES: u64 = 16 * 1024 * 1024;

/// Resolve and validate an extension asset path strictly within `ext_dir/frontend/`
/// or `ext_dir/assets/`.
///
/// Rejects path traversal (`..`, `.`), backslashes, leading slashes, NUL bytes,
/// and any attempt to reach private extension directories (`data/`, `config/`,
/// `binaries/`, `models/`, `manifest.json`). Fails closed on canonicalization
/// errors so symlinks escaping the allowed subtree can never be served.
async fn resolve_extension_asset_path(
    ext_dir: &std::path::Path,
    asset_path: &str,
) -> Result<std::path::PathBuf, ErrorResponse> {
    if asset_path.is_empty()
        || asset_path.contains('\0')
        || asset_path.contains('\\')
        || asset_path.starts_with('/')
        || asset_path.contains("..")
    {
        return Err(ErrorResponse::bad_request("Invalid asset path"));
    }

    let mut segments = asset_path.split('/');
    let Some(first_seg) = segments.next() else {
        return Err(ErrorResponse::bad_request("Invalid asset path"));
    };

    if first_seg.is_empty()
        || first_seg == "."
        || first_seg == ".."
        || segments.any(|seg| seg.is_empty() || seg == "." || seg == "..")
    {
        return Err(ErrorResponse::bad_request("Invalid asset path"));
    }

    // Explicitly block private extension subtrees and manifest.json.
    if matches!(
        first_seg,
        "data" | "config" | "binaries" | "models" | "manifest.json"
    ) {
        return Err(ErrorResponse::bad_request("Invalid asset path"));
    }

    // Map strictly into ext_dir/frontend/ or ext_dir/assets/.
    // Manifests declare bundle_path either prefixed with `frontend/` / `assets/`
    // or relative to `frontend/` (e.g. `dist/bundle.js`).
    let (allowed_dir, candidate_file) = match first_seg {
        "frontend" => {
            let rest = asset_path
                .strip_prefix("frontend/")
                .filter(|r| !r.is_empty())
                .ok_or_else(|| ErrorResponse::bad_request("Invalid asset path"))?;
            let dir = ext_dir.join("frontend");
            let file = dir.join(rest);
            (dir, file)
        }
        "assets" => {
            let rest = asset_path
                .strip_prefix("assets/")
                .filter(|r| !r.is_empty())
                .ok_or_else(|| ErrorResponse::bad_request("Invalid asset path"))?;
            let dir = ext_dir.join("assets");
            let file = dir.join(rest);
            (dir, file)
        }
        _ => {
            let dir = ext_dir.join("frontend");
            let file = dir.join(asset_path);
            (dir, file)
        }
    };

    // Fail closed on canonicalize errors: both the allowed root and target file
    // must exist and canonicalize, and the resolved file must stay inside the
    // canonical allowed subtree (defeating symlink escapes).
    let Ok(real_allowed_dir) = tokio::fs::canonicalize(&allowed_dir).await else {
        return Err(ErrorResponse::not_found("Asset not found"));
    };
    let Ok(real_file) = tokio::fs::canonicalize(&candidate_file).await else {
        return Err(ErrorResponse::not_found("Asset not found"));
    };

    if !real_file.starts_with(&real_allowed_dir) {
        return Err(ErrorResponse::bad_request("Invalid asset path"));
    }

    Ok(real_file)
}

/// Serve a validated asset from `ext_dir` (factored out for direct unit testing).
async fn serve_extension_asset_from_dir(
    ext_dir: &std::path::Path,
    asset_path: &str,
) -> Result<axum::response::Response, ErrorResponse> {
    use axum::body::Body;
    use axum::http::{header, StatusCode};

    let real_file = resolve_extension_asset_path(ext_dir, asset_path).await?;

    let metadata = tokio::fs::metadata(&real_file)
        .await
        .map_err(|_| ErrorResponse::not_found("Asset not found"))?;
    if !metadata.is_file() {
        return Err(ErrorResponse::not_found("Asset not found"));
    }
    if metadata.len() > MAX_EXTENSION_ASSET_BYTES {
        return Err(ErrorResponse::bad_request(format!(
            "Asset exceeds maximum size of {} bytes",
            MAX_EXTENSION_ASSET_BYTES
        )));
    }

    // Read file asynchronously (never block the tokio runtime with std::fs::read).
    let content = tokio::fs::read(&real_file)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to read asset: {}", e)))?;
    if content.len() as u64 > MAX_EXTENSION_ASSET_BYTES {
        return Err(ErrorResponse::bad_request(format!(
            "Asset exceeds maximum size of {} bytes",
            MAX_EXTENSION_ASSET_BYTES
        )));
    }

    // Determine content type based on file extension (case-insensitive).
    let ext_lower = real_file
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());
    let mime_type = match ext_lower.as_deref() {
        Some("js" | "cjs" | "mjs") => "application/javascript",
        Some("json") => "application/json",
        Some("css") => "text/css",
        Some("html" | "htm") => "text/html",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        _ => "application/octet-stream",
    };

    // Build response. Use no-cache so the client always revalidates:
    // extension bundles change on every rebuild and a stale max-age cache
    // (the previous "public, max-age=3600") made iterating in dev painful —
    // Tauri WKWebView would serve a 1-hour-stale bundle even after we just
    // dropped a new one in place. Bundle is small (tens of KB); re-fetch on
    // each navigation is negligible.
    let mut builder = axum::response::Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime_type)
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff");

    if matches!(ext_lower.as_deref(), Some("html" | "htm" | "svg")) {
        builder = builder.header(header::CONTENT_SECURITY_POLICY, "sandbox");
    }

    builder
        .body(Body::from(content))
        .map_err(|e| ErrorResponse::internal(format!("Failed to build response: {}", e)))
}

/// GET /api/extensions/:id/assets/*
/// Serve static assets from extension directory.
pub async fn serve_extension_asset_handler(
    Path((id, asset_path)): Path<(String, String)>,
) -> Result<axum::response::Response, ErrorResponse> {
    validate_extension_id(&id)?;

    // Extension directory is always data/extensions/{id}
    let data_dir = neomind_core::paths::data_dir()
        .to_string_lossy()
        .to_string();
    let ext_dir = std::path::PathBuf::from(data_dir)
        .join("extensions")
        .join(&id);

    serve_extension_asset_from_dir(&ext_dir, &asset_path).await
}

/// GET /api/extensions/dashboard-components
/// Get all dashboard components from all registered extensions.
///
/// This endpoint only returns components from extensions that are currently registered.
/// When an extension is unregistered, its components will no longer appear.
#[utoipa::path(
    get,
    path = "/api/extensions/dashboard-components",
    tag = "extensions",
    responses(
        (status = 200, description = "Dashboard components contributed by every extension"),
    )
)]
pub async fn get_all_dashboard_components_handler(
    State(state): State<ServerState>,
) -> HandlerResult<Vec<DashboardComponentDto>> {
    let mut all_components = Vec::new();

    // Load components only from registered extensions
    let all_extensions = state.extensions.runtime.list().await;
    for info in all_extensions {
        if let Some(components) = load_extension_components(&info.metadata.id, info.path.as_ref()) {
            all_components.extend(components);
        }
    }

    ok(all_components)
}

#[cfg(test)]
mod asset_security_tests {
    use super::*;
    use axum::http::header;

    #[tokio::test]
    async fn rejects_private_dirs_manifest_and_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        let ext_dir = tmp.path();

        std::fs::create_dir_all(ext_dir.join("data")).unwrap();
        std::fs::write(ext_dir.join("data/license.key"), "SECRET-KEY").unwrap();
        std::fs::create_dir_all(ext_dir.join("config")).unwrap();
        std::fs::write(ext_dir.join("config/x.json"), "{}").unwrap();
        std::fs::create_dir_all(ext_dir.join("models")).unwrap();
        std::fs::write(ext_dir.join("models/a.onnx"), b"onnx").unwrap();
        std::fs::create_dir_all(ext_dir.join("binaries")).unwrap();
        std::fs::write(ext_dir.join("binaries/ext.so"), b"elf").unwrap();
        std::fs::write(ext_dir.join("manifest.json"), r#"{"id":"demo"}"#).unwrap();
        std::fs::create_dir_all(ext_dir.join("frontend/dist")).unwrap();
        std::fs::write(ext_dir.join("frontend/dist/bundle.js"), "console.log(1);").unwrap();
        std::fs::write(ext_dir.join("frontend/page.html"), "<html></html>").unwrap();
        std::fs::create_dir_all(ext_dir.join("assets")).unwrap();
        std::fs::write(ext_dir.join("assets/logo.svg"), "<svg></svg>").unwrap();

        for forbidden in [
            "data/license.key",
            "config/x.json",
            "models/a.onnx",
            "binaries/ext.so",
            "manifest.json",
            "../manifest.json",
            "frontend/../data/license.key",
            "assets/../config/x.json",
            "/etc/passwd",
            "frontend\\dist\\bundle.js",
            "frontend/dist/bundle.js\0.png",
        ] {
            assert!(
                serve_extension_asset_from_dir(ext_dir, forbidden)
                    .await
                    .is_err(),
                "must reject asset path: {forbidden}"
            );
        }

        // Legitimate frontend/ prefixed path
        let resp = serve_extension_asset_from_dir(ext_dir, "frontend/dist/bundle.js")
            .await
            .expect("frontend/dist/bundle.js must resolve");
        assert_eq!(
            resp.headers()
                .get(header::X_CONTENT_TYPE_OPTIONS)
                .and_then(|v| v.to_str().ok()),
            Some("nosniff")
        );
        assert!(resp
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .is_none());

        // Legitimate relative-to-frontend path (matches manifest bundle_path: "dist/bundle.js")
        let resp_rel = serve_extension_asset_from_dir(ext_dir, "dist/bundle.js")
            .await
            .expect("dist/bundle.js relative to frontend/ must resolve");
        assert_eq!(
            resp_rel
                .headers()
                .get(header::X_CONTENT_TYPE_OPTIONS)
                .and_then(|v| v.to_str().ok()),
            Some("nosniff")
        );

        // Legitimate assets/ SVG gets nosniff + sandbox CSP
        let resp_svg = serve_extension_asset_from_dir(ext_dir, "assets/logo.svg")
            .await
            .expect("assets/logo.svg must resolve");
        assert_eq!(
            resp_svg
                .headers()
                .get(header::X_CONTENT_TYPE_OPTIONS)
                .and_then(|v| v.to_str().ok()),
            Some("nosniff")
        );
        assert_eq!(
            resp_svg
                .headers()
                .get(header::CONTENT_SECURITY_POLICY)
                .and_then(|v| v.to_str().ok()),
            Some("sandbox")
        );

        // HTML also gets sandbox CSP
        let resp_html = serve_extension_asset_from_dir(ext_dir, "frontend/page.html")
            .await
            .expect("frontend/page.html must resolve");
        assert_eq!(
            resp_html
                .headers()
                .get(header::CONTENT_SECURITY_POLICY)
                .and_then(|v| v.to_str().ok()),
            Some("sandbox")
        );
    }
}
