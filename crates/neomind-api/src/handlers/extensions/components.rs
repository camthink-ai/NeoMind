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

/// Load dashboard components from extension manifest.
pub(crate) fn load_extension_components(
    // Log path configuration for debugging
    extension_id: &str,
    file_path: Option<&std::path::PathBuf>,
) -> Option<Vec<DashboardComponentDto>> {
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
    // For legacy format: file_path = extensions/xxx.wasm -> ext_dir = extensions/
    // For .nep format: file_path = extensions/xxx/binaries/wasm/extension.wasm -> ext_dir should be extensions/xxx/
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

    tracing::debug!(
        extension_id = %extension_id,
        components_count = manifest.frontend.as_ref().map(|f| f.components.len()).unwrap_or(0),
        "Parsed manifest.json"
    );

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

/// GET /api/extensions/:id/assets/*
/// Serve static assets from extension directory.
pub async fn serve_extension_asset_handler(
    Path((id, asset_path)): Path<(String, String)>,
) -> Result<axum::response::Response, ErrorResponse> {
    use axum::body::Body;
    use axum::http::{header, StatusCode};

    // Prevent directory traversal AND absolute-path escape via asset_path.
    // `PathBuf::join` REPLACES the base when the argument is absolute — an
    // asset_path of "/etc/passwd" (no ".." anywhere) sailed through the old
    // check and served the file. Reject absolute paths, dot segments, and
    // verify the resolved file stays inside the extension dir.
    validate_extension_id(&id)?;
    if asset_path.contains("..")
        || asset_path.starts_with('/')
        || asset_path.starts_with('\\')
        || asset_path.split('/').any(|seg| seg == ".")
    {
        return Err(ErrorResponse::bad_request("Invalid asset path"));
    }

    // Extension directory is always data/extensions/{id}
    let data_dir = neomind_core::paths::data_dir()
        .to_string_lossy()
        .to_string();
    let ext_dir = std::path::PathBuf::from(data_dir)
        .join("extensions")
        .join(&id);

    let asset_file = ext_dir.join(&asset_path);

    // Check if file exists
    if !asset_file.exists() {
        return Err(ErrorResponse::not_found("Asset not found"));
    }
    // Belt-and-suspenders: the resolved file MUST live under the extension
    // dir (canonicalized comparison defeats every remaining join trick).
    if let (Ok(real_file), Ok(real_dir)) = (
        std::fs::canonicalize(&asset_file),
        std::fs::canonicalize(&ext_dir),
    ) {
        if !real_file.starts_with(&real_dir) {
            return Err(ErrorResponse::bad_request("Invalid asset path"));
        }
    }

    // Read file content
    let content = match std::fs::read(&asset_file) {
        Ok(c) => c,
        Err(e) => {
            return Err(ErrorResponse::internal(format!(
                "Failed to read asset: {}",
                e
            )))
        }
    };

    // Determine content type based on file extension
    let mime_type = asset_file
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| match ext {
            "js" | "cjs" | "mjs" => "application/javascript",
            "json" => "application/json",
            "css" => "text/css",
            "html" => "text/html",
            "svg" => "image/svg+xml",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "woff" => "font/woff",
            "woff2" => "font/woff2",
            "ttf" => "font/ttf",
            _ => "application/octet-stream",
        })
        .unwrap_or("application/octet-stream");

    // Build response. Use no-cache so the client always revalidates:
    // extension bundles change on every rebuild and a stale max-age cache
    // (the previous "public, max-age=3600") made iterating in dev painful —
    // Tauri WKWebView would serve a 1-hour-stale bundle even after we just
    // dropped a new one in place. Bundle is small (tens of KB); re-fetch on
    // each navigation is negligible.
    axum::response::Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime_type)
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(content))
        .map_err(|e| ErrorResponse::internal(format!("Failed to build response: {}", e)))
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
