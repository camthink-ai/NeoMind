//! Frontend Component API handlers.
//!
//! Provides endpoints for:
//! - Browsing the community component marketplace (GitHub-hosted index)
//! - Installing components from the marketplace
//! - Manual component installation (multipart upload)
//! - Listing, serving, and deleting installed components

use axum::extract::{Multipart, Path, State};
use axum::response::Response;
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use std::io::Read;

use crate::handlers::common::{ok, HandlerResult};
use crate::models::error::ErrorResponse;
use crate::server::ServerState;

use neomind_storage::frontend_components::{ComponentManifest, MarketIndex};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Base URL for marketplace content.
/// Override via `NEOMIND_MARKET_URL` env var to use a mirror (e.g. GitHub proxy).
/// Examples:
///   - Default: https://raw.githubusercontent.com/camthink-ai/NeoMind-Dashboard-Components
///   - Mirror:  https://ghfast.top/https://raw.githubusercontent.com/camthink-ai/NeoMind-Dashboard-Components
fn market_base_url() -> String {
    std::env::var("NEOMIND_MARKET_URL").unwrap_or_else(|_| {
        "https://raw.githubusercontent.com/camthink-ai/NeoMind-Dashboard-Components".to_string()
    })
}
const MARKET_BRANCH: &str = "main";

/// Component IDs reserved for built-in components — cannot be overwritten.
const RESERVED_IDS: &[&str] = &[
    "value-card",
    "led-indicator",
    "sparkline",
    "progress-bar",
    "line-chart",
    "area-chart",
    "bar-chart",
    "pie-chart",
    "toggle-switch",
    "image-display",
    "image-history",
    "web-display",
    "markdown-display",
    "map-display",
    "video-display",
    "custom-layer",
    "agent-monitor-widget",
    "ai-analyst",
];

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Validate a component ID:
/// - Must not be empty
/// - Must not be a reserved built-in ID
/// - Must not contain path traversal characters
fn validate_component_id(id: &str) -> Result<(), ErrorResponse> {
    if id.is_empty() {
        return Err(ErrorResponse::bad_request("Component ID cannot be empty"));
    }
    if RESERVED_IDS.contains(&id) {
        return Err(ErrorResponse::bad_request(format!(
            "Component ID '{}' is reserved for built-in components",
            id
        )));
    }
    if id.contains('.') || id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err(ErrorResponse::bad_request(
            "Component ID contains invalid characters",
        ));
    }
    Ok(())
}

/// Publish a lifecycle event for a frontend component.
async fn publish_lifecycle_event(state: &ServerState, component_id: &str, lifecycle_state: &str) {
    if let Some(bus) = &state.core.event_bus {
        let _ = bus
            .publish(neomind_core::event::NeoMindEvent::Custom {
                event_type: "FrontendComponentLifecycle".to_string(),
                data: serde_json::json!({
                    "component_id": component_id,
                    "state": lifecycle_state,
                }),
            })
            .await;
    }
}

/// Build a reqwest client with sensible defaults.
fn http_client() -> Result<reqwest::Client, ErrorResponse> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| ErrorResponse::internal(format!("Failed to build HTTP client: {}", e)))
}

/// Upper bound on a marketplace download. The manual-upload path is gated by
/// `MAX_REQUEST_BODY_SIZE` (10 MB), so we mirror that here — without a cap,
/// `resp.bytes().await` buffers the entire response into memory and a
/// compromised marketplace URL (or `NEOMIND_MARKET_URL` env override) could
/// OOM the server or fill `data/frontend-components/` within the 15s timeout.
const MARKETPLACE_DOWNLOAD_CAP_BYTES: usize = 10 * 1024 * 1024;

/// Read a response body into a `Vec<u8>` with a hard byte cap.
///
/// Pre-checks `Content-Length` (when the server advertises one) for a fast
/// rejection, then streams the body chunk-by-chunk so a server that omits
/// `Content-Length` (or under-reports it) is still caught mid-stream rather
/// than after buffering gigabytes. Returns the bytes accumulated so far are
/// discarded on cap-overflow so we don't hold a giant allocation while
/// constructing the error response.
async fn collect_capped(resp: reqwest::Response, label: &str) -> Result<Vec<u8>, ErrorResponse> {
    // Fast path: trust server-advertised length when present.
    if let Some(len) = resp.content_length() {
        if len as usize > MARKETPLACE_DOWNLOAD_CAP_BYTES {
            return Err(ErrorResponse::bad_request(format!(
                "{} is too large ({} bytes); marketplace download cap is {} bytes",
                label, len, MARKETPLACE_DOWNLOAD_CAP_BYTES
            )));
        }
    }

    use futures::StreamExt;
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result
            .map_err(|e| ErrorResponse::internal(format!("Failed to read {}: {}", label, e)))?;
        // Check before extending so we never exceed the cap in memory.
        if buf.len().saturating_add(chunk.len()) > MARKETPLACE_DOWNLOAD_CAP_BYTES {
            return Err(ErrorResponse::bad_request(format!(
                "{} exceeded the {} byte download cap during streaming",
                label, MARKETPLACE_DOWNLOAD_CAP_BYTES
            )));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

/// Fetch the marketplace index from GitHub.
async fn fetch_market_index(client: &reqwest::Client) -> Result<MarketIndex, ErrorResponse> {
    let cache_buster = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let index_url = format!(
        "{}/{}/index.json?t={}",
        market_base_url(),
        MARKET_BRANCH,
        cache_buster
    );

    let response = client
        .get(&index_url)
        .header("User-Agent", "NeoMind-Component-Marketplace")
        .header("Cache-Control", "no-cache")
        .send()
        .await
        .map_err(|e| {
            tracing::error!("Failed to fetch marketplace index: {}", e);
            ErrorResponse::internal(format!("Network error: {}", e))
        })?;

    if !response.status().is_success() {
        let status = response.status();
        tracing::error!("Marketplace index returned status {}", status);
        return Err(ErrorResponse::internal(format!(
            "Marketplace returned HTTP {}",
            status
        )));
    }

    response.json::<MarketIndex>().await.map_err(|e| {
        tracing::error!("Failed to parse marketplace index: {}", e);
        ErrorResponse::internal(format!("Invalid marketplace index: {}", e))
    })
}

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct MarketInstallRequest {
    pub component_id: String,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET `/api/frontend-components/market/list`
///
/// Fetch the community component index from GitHub.
/// Public endpoint — no authentication required.
pub async fn market_list_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let client = match http_client() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to create HTTP client: {:?}", e);
            return ok(json!({
                "components": [],
                "total": 0,
                "error": "network_error",
                "message": "Unable to create HTTP client"
            }));
        }
    };

    match fetch_market_index(&client).await {
        Ok(index) => ok(json!({
            "components": index.components,
            "total": index.components.len(),
            "market_version": index.version,
        })),
        Err(_) => ok(json!({
            "components": [],
            "total": 0,
            "error": "network_error",
            "message": "Unable to connect to component marketplace. Please check your internet connection."
        })),
    }
}

/// GET `/api/frontend-components/updates`
///
/// Compare installed community components against the latest marketplace index
/// and return those with a newer version available.
///
/// Protected endpoint — requires authentication.
/// Tolerates network failures: returns `{ updates: [], count: 0, error: "network_error" }`
/// rather than an HTTP error so the UI can degrade gracefully.
pub async fn check_updates_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let store = state.frontend_component_store.clone();
    let installed = match tokio::task::spawn_blocking(move || store.list_all())
        .await
        .map_err(|e| ErrorResponse::internal(format!("List task failed: {}", e)))?
    {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("Failed to list installed components: {}", e);
            return ok(json!({
                "updates": [],
                "count": 0,
                "error": "internal_error",
                "message": "Failed to read installed components"
            }));
        }
    };

    // Fast path: nothing installed → no updates possible.
    if installed.is_empty() {
        return ok(json!({ "updates": [], "count": 0 }));
    }

    let client = match http_client() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to create HTTP client: {:?}", e);
            return ok(json!({
                "updates": [],
                "count": 0,
                "error": "network_error"
            }));
        }
    };

    let index = match fetch_market_index(&client).await {
        Ok(idx) => idx,
        Err(e) => {
            tracing::warn!("Failed to fetch marketplace index for update check: {}", e);
            return ok(json!({
                "updates": [],
                "count": 0,
                "error": "network_error"
            }));
        }
    };

    let mut updates: Vec<serde_json::Value> = Vec::new();
    for manifest in &installed {
        if let Some(entry) = index.components.iter().find(|c| c.id == manifest.id) {
            if manifest.version != entry.version {
                updates.push(json!({
                    "id": manifest.id,
                    "name": manifest.name,
                    "current_version": manifest.version,
                    "latest_version": entry.version,
                }));
            }
        }
    }

    let count = updates.len();
    ok(json!({ "updates": updates, "count": count }))
}

/// POST `/api/frontend-components/market/install`
///
/// Download and install a component from the community marketplace.
/// Protected endpoint — requires authentication.
pub async fn market_install_handler(
    State(state): State<ServerState>,
    Json(req): Json<MarketInstallRequest>,
) -> HandlerResult<serde_json::Value> {
    let component_id = req.component_id.trim().to_string();
    validate_component_id(&component_id)?;

    let client = match http_client() {
        Ok(c) => c,
        Err(e) => {
            return ok(json!({
                "success": false,
                "error": format!("Failed to create HTTP client: {}", e)
            }));
        }
    };

    // Fetch marketplace index
    let index = match fetch_market_index(&client).await {
        Ok(idx) => idx,
        Err(e) => {
            tracing::error!("Failed to fetch marketplace index: {}", e);
            return ok(json!({
                "success": false,
                "error": format!("Network error: Unable to connect to component marketplace. {}", e)
            }));
        }
    };

    // Find the component entry
    let entry = match index.components.iter().find(|c| c.id == component_id) {
        Some(e) => e,
        None => {
            return ok(json!({
                "success": false,
                "error": format!("Component '{}' not found in marketplace", component_id)
            }));
        }
    };

    // Download manifest and bundle in parallel
    let manifest_url = entry.manifest_url.clone();
    let bundle_url = entry.bundle_url.clone();

    let (manifest_result, bundle_result) = tokio::join!(
        client
            .get(&manifest_url)
            .header("User-Agent", "NeoMind-Component-Marketplace")
            .send(),
        client
            .get(&bundle_url)
            .header("User-Agent", "NeoMind-Component-Marketplace")
            .send()
    );

    let manifest_resp = match manifest_result {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            return ok(json!({
                "success": false,
                "error": format!("Failed to download manifest: HTTP {}", r.status())
            }));
        }
        Err(e) => {
            tracing::error!("Failed to download manifest: {}", e);
            return ok(json!({
                "success": false,
                "error": "Network error: Unable to download component manifest. Please check your internet connection."
            }));
        }
    };

    let bundle_resp = match bundle_result {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            return ok(json!({
                "success": false,
                "error": format!("Failed to download bundle: HTTP {}", r.status())
            }));
        }
        Err(e) => {
            tracing::error!("Failed to download bundle: {}", e);
            return ok(json!({
                "success": false,
                "error": "Network error: Unable to download component bundle. Please check your internet connection."
            }));
        }
    };

    let manifest_text = match collect_capped(manifest_resp, "manifest").await {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(e) => {
                return ok(json!({
                    "success": false,
                    "error": format!("Manifest is not valid UTF-8: {}", e)
                }));
            }
        },
        Err(e) => {
            return ok(json!({
                "success": false,
                "error": e.to_string()
            }));
        }
    };

    let bundle_bytes = match collect_capped(bundle_resp, "bundle").await {
        Ok(b) => b,
        Err(e) => {
            return ok(json!({
                "success": false,
                "error": e.to_string()
            }));
        }
    };

    // Parse manifest
    let mut manifest: ComponentManifest = match serde_json::from_str(&manifest_text) {
        Ok(m) => m,
        Err(e) => {
            return ok(json!({
                "success": false,
                "error": format!("Invalid manifest JSON: {}", e)
            }));
        }
    };

    // Validate manifest ID matches requested ID
    if manifest.id != component_id {
        return ok(json!({
            "success": false,
            "error": format!("Manifest ID '{}' does not match requested component ID '{}'", manifest.id, component_id)
        }));
    }

    // Set install timestamp
    manifest.installed_at = chrono::Utc::now().timestamp();
    manifest.source = Some("marketplace".to_string());

    // Install via store (blocking filesystem operation)
    let store = state.frontend_component_store.clone();
    let id_for_event = manifest.id.clone();
    let manifest_for_response = manifest.clone();
    let install_result =
        tokio::task::spawn_blocking(move || store.install(&manifest, &bundle_bytes)).await;

    match install_result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            return ok(json!({
                "success": false,
                "error": format!("Failed to install component: {}", e)
            }));
        }
        Err(e) => {
            return ok(json!({
                "success": false,
                "error": format!("Install task failed: {}", e)
            }));
        }
    }

    // Publish lifecycle event
    publish_lifecycle_event(&state, &id_for_event, "installed").await;

    tracing::info!(
        component_id = %id_for_event,
        "Community component installed from marketplace"
    );

    ok(json!({
        "component": manifest_for_response
    }))
}

/// POST `/api/frontend-components` (multipart)
///
/// Manual component installation via multipart upload.
///
/// Supports two modes:
/// 1. **ZIP package**: single `package` field containing a `.zip` with `manifest.json` + `bundle.js`
/// 2. **Separate files**: `manifest` (JSON text) + `bundle` (JS bytes) fields
///
/// Protected endpoint with 5 MB body limit.
pub async fn install_component_handler(
    State(state): State<ServerState>,
    mut multipart: Multipart,
) -> HandlerResult<serde_json::Value> {
    let mut manifest_text: Option<String> = None;
    let mut bundle_bytes: Option<Vec<u8>> = None;
    let mut package_bytes: Option<Vec<u8>> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ErrorResponse::bad_request(format!("Multipart error: {}", e)))?
    {
        let name = field.name().unwrap_or("").to_string();

        match name.as_str() {
            "package" => {
                package_bytes = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|e| {
                            ErrorResponse::bad_request(format!(
                                "Failed to read package field: {}",
                                e
                            ))
                        })?
                        .to_vec(),
                );
            }
            "manifest" => {
                manifest_text = Some(field.text().await.map_err(|e| {
                    ErrorResponse::bad_request(format!("Failed to read manifest field: {}", e))
                })?);
            }
            "bundle" => {
                bundle_bytes = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|e| {
                            ErrorResponse::bad_request(format!(
                                "Failed to read bundle field: {}",
                                e
                            ))
                        })?
                        .to_vec(),
                );
            }
            _ => {}
        }
    }

    // Mode 1: ZIP package
    if let Some(zip_data) = package_bytes {
        let (m_text, b_bytes) =
            tokio::task::spawn_blocking(move || extract_zip_contents(&zip_data))
                .await
                .map_err(|e| {
                    ErrorResponse::internal(format!("ZIP extraction task failed: {}", e))
                })??;

        manifest_text = Some(m_text);
        bundle_bytes = Some(b_bytes);
    }

    let manifest_text = manifest_text
        .ok_or_else(|| ErrorResponse::bad_request("Missing 'manifest' or 'package' field"))?;
    let bundle_bytes = bundle_bytes
        .ok_or_else(|| ErrorResponse::bad_request("Missing 'bundle' or 'package' field"))?;

    install_from_parts(&state, manifest_text, bundle_bytes, "manual upload").await
}

#[derive(serde::Deserialize)]
pub struct InstallFromPathRequest {
    pub file_path: String,
}

/// POST `/api/frontend-components/from-path`
///
/// Install a community component from a `.zip` package that already sits
/// on the server, inside the data directory. Mirrors the extensions
/// upload API's `file_path` pattern: edge deployments often receive
/// packages via scp/USB, and a phone browser has no way to pick a file
/// that lives on the box.
pub async fn install_component_from_path_handler(
    State(state): State<ServerState>,
    Json(req): Json<InstallFromPathRequest>,
) -> HandlerResult<serde_json::Value> {
    let path = crate::handlers::extensions::resolve_confined_package_path(&req.file_path)?;

    if path.extension().and_then(|e| e.to_str()) != Some("zip") {
        return Err(ErrorResponse::bad_request(
            "file_path must point to a .zip package",
        ));
    }

    let zip_data = tokio::fs::read(&path)
        .await
        .map_err(|e| ErrorResponse::bad_request(format!("Failed to read package file: {}", e)))?;

    if zip_data.len() > MARKETPLACE_DOWNLOAD_CAP_BYTES {
        return Err(ErrorResponse::bad_request(format!(
            "Package too large ({} bytes); local install cap is {} bytes",
            zip_data.len(),
            MARKETPLACE_DOWNLOAD_CAP_BYTES
        )));
    }

    let (manifest_text, bundle_bytes) =
        tokio::task::spawn_blocking(move || extract_zip_contents(&zip_data))
            .await
            .map_err(|e| ErrorResponse::internal(format!("ZIP extraction task failed: {}", e)))??;

    install_from_parts(&state, manifest_text, bundle_bytes, "server path").await
}

/// Shared install tail for every manual install entry point (multipart
/// upload, server path): parse + validate the manifest, persist via the
/// store, publish the lifecycle event.
async fn install_from_parts(
    state: &ServerState,
    manifest_text: String,
    bundle_bytes: Vec<u8>,
    via: &str,
) -> HandlerResult<serde_json::Value> {
    // Parse manifest
    let mut manifest: ComponentManifest = serde_json::from_str(&manifest_text)
        .map_err(|e| ErrorResponse::bad_request(format!("Invalid manifest JSON: {}", e)))?;

    validate_component_id(&manifest.id)?;

    // Set install timestamp
    manifest.installed_at = chrono::Utc::now().timestamp();
    manifest.source = Some("local".to_string());

    // Install via store (blocking filesystem operation)
    let store = state.frontend_component_store.clone();
    let id_for_event = manifest.id.clone();
    let manifest_for_response = manifest.clone();
    tokio::task::spawn_blocking(move || store.install(&manifest, &bundle_bytes))
        .await
        .map_err(|e| ErrorResponse::internal(format!("Install task failed: {}", e)))?
        .map_err(|e| ErrorResponse::internal(format!("Failed to install component: {}", e)))?;

    // Publish lifecycle event
    publish_lifecycle_event(state, &id_for_event, "installed").await;

    tracing::info!(
        component_id = %id_for_event,
        via = %via,
        "Community component installed"
    );

    ok(json!({
        "component": manifest_for_response
    }))
}

/// Extract `manifest.json` and `bundle.js` from a ZIP archive.
fn extract_zip_contents(zip_data: &[u8]) -> Result<(String, Vec<u8>), ErrorResponse> {
    let reader = std::io::Cursor::new(zip_data);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|e| ErrorResponse::bad_request(format!("Invalid ZIP file: {}", e)))?;

    let mut manifest_text: Option<String> = None;
    let mut bundle_bytes: Option<Vec<u8>> = None;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| ErrorResponse::bad_request(format!("Failed to read ZIP entry: {}", e)))?;

        // Normalize path: strip directory prefixes
        let name = file.name().to_string();
        let filename = name.rsplit('/').next().unwrap_or(&name).to_string();

        match filename.as_str() {
            "manifest.json" => {
                let mut text = String::new();
                file.read_to_string(&mut text).map_err(|e| {
                    ErrorResponse::bad_request(format!("Failed to read manifest.json: {}", e))
                })?;
                manifest_text = Some(text);
            }
            "bundle.js" => {
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes).map_err(|e| {
                    ErrorResponse::bad_request(format!("Failed to read bundle.js: {}", e))
                })?;
                bundle_bytes = Some(bytes);
            }
            _ => {}
        }
    }

    let manifest_text = manifest_text
        .ok_or_else(|| ErrorResponse::bad_request("ZIP must contain manifest.json"))?;
    let bundle_bytes =
        bundle_bytes.ok_or_else(|| ErrorResponse::bad_request("ZIP must contain bundle.js"))?;

    Ok((manifest_text, bundle_bytes))
}

/// GET `/api/frontend-components`
///
/// List all installed community components.
/// Protected endpoint — requires authentication.
pub async fn list_components_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let store = state.frontend_component_store.clone();
    let components = tokio::task::spawn_blocking(move || store.list_all())
        .await
        .map_err(|e| ErrorResponse::internal(format!("List task failed: {}", e)))?
        .map_err(|e| ErrorResponse::internal(format!("Failed to list components: {}", e)))?;

    // Surface the static catalogue of built-in widget types (mirror of
    // `builtInTypes` in `Renderers.tsx`) so CLI callers and agents can
    // enumerate every available dashboard component without guessing.
    let builtin_types: Vec<serde_json::Value> = neomind_core::dashboard::BUILTIN_WIDGET_TYPES
        .iter()
        .map(|t| {
            json!({
                "type_id": t.type_id,
                "display_name": t.display_name,
                "category": t.category,
            })
        })
        .collect();

    ok(json!({
        "components": components,
        "total": components.len(),
        "builtin_types": builtin_types,
        "builtin_count": builtin_types.len(),
    }))
}

/// GET `/api/frontend-components/:id`
///
/// Get a single component manifest by ID.
/// Checks built-in components first, then installed community components.
/// Protected endpoint — requires authentication.
pub async fn get_component_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Check built-in components first
    if let Some(builtin) = builtin_component_list().into_iter().find(|c| c.id == id) {
        return ok(json!({ "component": builtin }));
    }

    // Check installed community components
    let store = state.frontend_component_store.clone();
    let id_clone = id.clone();
    let manifest = tokio::task::spawn_blocking(move || store.load_manifest(&id_clone))
        .await
        .map_err(|e| ErrorResponse::internal(format!("Task failed: {}", e)))?
        .map_err(|e| ErrorResponse::internal(format!("Failed to load component: {}", e)))?;

    match manifest {
        Some(m) => ok(json!({ "component": m })),
        None => Err(ErrorResponse::not_found(format!(
            "Component '{}' not found",
            id
        ))),
    }
}

/// Return metadata for all built-in (static) dashboard components.
/// These are hardcoded in the frontend and never stored in the database.
fn builtin_component_list() -> Vec<ComponentManifest> {
    use neomind_storage::frontend_components::SizeConstraints;

    let sc = |min_w, min_h, def_w, def_h, max_w, max_h| SizeConstraints {
        min_w,
        min_h,
        default_w: def_w,
        default_h: def_h,
        max_w,
        max_h,
    };

    // -- Indicators --
    let value_card = ComponentManifest {
        id: "value-card".into(),
        name: json!("Value Card"),
        description: json!("Display a single value with optional unit and trend"),
        icon: "box".into(),
        category: "indicators".into(),
        version: "1.0.0".into(),
        author: Some("NeoMind".into()),
        size_constraints: sc(2, 1, 2, 2, 6, 4),
        has_data_source: true,
        max_data_sources: Some(1),
        has_display_config: true,
        has_actions: false,
        has_device_binding: false,
        device_type_filter: vec![],
        config_schema: Some(json!({
            "display": {
                "type": "object",
                "properties": {
                    "unit": {"type": "string", "description": "Display unit (e.g. °C, %, V)"},
                    "format": {"type": "string", "description": "Number format (e.g. .1f, .0f)"},
                    "color": {"type": "string", "description": "Primary color"},
                    "showTrend": {"type": "boolean", "description": "Show trend indicator"}
                }
            },
            "config": {
                "type": "object",
                "properties": {
                    "variant": {"type": "string", "description": "Card variant style"}
                }
            }
        })),
        default_config: Some(json!({"display": {"showTrend": true}})),
        variants: None,
        global_name: "value_card".into(),
        export_name: None,
        installed_at: 0,
        source: None,
    };

    let led_indicator = ComponentManifest {
        id: "led-indicator".into(),
        name: json!("LED Indicator"),
        description: json!("Binary status indicator with color states"),
        icon: "box".into(),
        category: "indicators".into(),
        version: "1.0.0".into(),
        author: Some("NeoMind".into()),
        size_constraints: sc(1, 1, 2, 1, 4, 2),
        has_data_source: true,
        max_data_sources: Some(1),
        has_display_config: true,
        has_actions: false,
        has_device_binding: false,
        device_type_filter: vec![],
        config_schema: Some(json!({
            "display": {
                "type": "object",
                "properties": {
                    "color": {"type": "string", "description": "Primary color"},
                    "valueMap": {"type": "object", "description": "Map data values to display states"},
                    "defaultState": {"type": "string", "description": "Default display state"}
                }
            },
            "config": {"type": "object", "properties": {}}
        })),
        default_config: None,
        variants: None,
        global_name: "led_indicator".into(),
        export_name: None,
        installed_at: 0,
        source: None,
    };

    let sparkline = ComponentManifest {
        id: "sparkline".into(),
        name: json!("Sparkline"),
        description: json!("Mini trend chart showing recent data history"),
        icon: "box".into(),
        category: "indicators".into(),
        version: "1.0.0".into(),
        author: Some("NeoMind".into()),
        size_constraints: sc(2, 1, 4, 2, 8, 4),
        has_data_source: true,
        max_data_sources: Some(1),
        has_display_config: true,
        has_actions: false,
        has_device_binding: false,
        device_type_filter: vec![],
        config_schema: Some(json!({
            "display": {
                "type": "object",
                "properties": {
                    "color": {"type": "string", "description": "Line color"},
                    "fill": {"type": "boolean", "description": "Fill area under the line"},
                    "showThreshold": {"type": "boolean", "description": "Show threshold line"},
                    "threshold": {"type": "number", "description": "Threshold value"}
                }
            },
            "config": {
                "type": "object",
                "properties": {
                    "curved": {"type": "boolean", "description": "Use curved line"}
                }
            }
        })),
        default_config: Some(json!({"display": {"fill": true}, "config": {"curved": true}})),
        variants: None,
        global_name: "sparkline".into(),
        export_name: None,
        installed_at: 0,
        source: None,
    };

    let progress_bar = ComponentManifest {
        id: "progress-bar".into(),
        name: json!("Progress Bar"),
        description: json!("Progress indicator with min/max range"),
        icon: "box".into(),
        category: "indicators".into(),
        version: "1.0.0".into(),
        author: Some("NeoMind".into()),
        size_constraints: sc(2, 1, 4, 1, 12, 2),
        has_data_source: true,
        max_data_sources: Some(1),
        has_display_config: true,
        has_actions: false,
        has_device_binding: false,
        device_type_filter: vec![],
        config_schema: Some(json!({
            "display": {
                "type": "object",
                "properties": {
                    "color": {"type": "string", "description": "Bar color"},
                    "max": {"type": "number", "description": "Maximum value (default 100)"},
                    "warningThreshold": {"type": "number", "description": "Warning threshold percentage"},
                    "dangerThreshold": {"type": "number", "description": "Danger threshold percentage"}
                }
            },
            "config": {
                "type": "object",
                "properties": {
                    "variant": {"type": "string", "description": "Bar variant style"}
                }
            }
        })),
        default_config: Some(json!({"display": {"max": 100}})),
        variants: None,
        global_name: "progress_bar".into(),
        export_name: None,
        installed_at: 0,
        source: None,
    };

    // -- Charts --
    let line_chart = ComponentManifest {
        id: "line-chart".into(),
        name: json!("Line Chart"),
        description: json!("Time-series line chart for telemetry data"),
        icon: "box".into(),
        category: "charts".into(),
        version: "1.0.0".into(),
        author: Some("NeoMind".into()),
        size_constraints: sc(3, 2, 6, 4, 12, 8),
        has_data_source: true,
        max_data_sources: Some(5),
        has_display_config: true,
        has_actions: false,
        has_device_binding: false,
        device_type_filter: vec![],
        config_schema: Some(json!({
            "display": {
                "type": "object",
                "properties": {
                    "unit": {"type": "string", "description": "Axis unit label"},
                    "showLegend": {"type": "boolean", "description": "Show legend"},
                    "showGrid": {"type": "boolean", "description": "Show grid lines"},
                    "showTooltip": {"type": "boolean", "description": "Show tooltip on hover"},
                    "fillArea": {"type": "boolean", "description": "Fill area under lines"}
                }
            },
            "config": {
                "type": "object",
                "properties": {
                    "smooth": {"type": "boolean", "description": "Use smooth (curved) lines"},
                    "timeWindow": {"type": "string", "description": "Default time window for data"},
                    "aggregate": {"type": "string", "description": "Aggregation method (raw, avg, min, max)"}
                }
            }
        })),
        default_config: Some(
            json!({"display": {"showLegend": true, "showGrid": true}, "config": {"smooth": true}}),
        ),
        variants: None,
        global_name: "line_chart".into(),
        export_name: None,
        installed_at: 0,
        source: None,
    };

    let area_chart = ComponentManifest {
        id: "area-chart".into(),
        name: json!("Area Chart"),
        description: json!("Filled area chart for trend visualization"),
        icon: "box".into(),
        category: "charts".into(),
        version: "1.0.0".into(),
        author: Some("NeoMind".into()),
        size_constraints: sc(3, 2, 6, 4, 12, 8),
        has_data_source: true,
        max_data_sources: Some(5),
        has_display_config: true,
        has_actions: false,
        has_device_binding: false,
        device_type_filter: vec![],
        config_schema: Some(json!({
            "display": {
                "type": "object",
                "properties": {
                    "unit": {"type": "string", "description": "Axis unit label"},
                    "showLegend": {"type": "boolean", "description": "Show legend"},
                    "showGrid": {"type": "boolean", "description": "Show grid lines"},
                    "showTooltip": {"type": "boolean", "description": "Show tooltip on hover"},
                    "fillArea": {"type": "boolean", "description": "Fill area under lines"}
                }
            },
            "config": {
                "type": "object",
                "properties": {
                    "smooth": {"type": "boolean", "description": "Use smooth (curved) lines"},
                    "timeWindow": {"type": "string", "description": "Default time window for data"},
                    "aggregate": {"type": "string", "description": "Aggregation method"}
                }
            }
        })),
        default_config: Some(
            json!({"display": {"showLegend": true, "fillArea": true}, "config": {"smooth": true}}),
        ),
        variants: None,
        global_name: "area_chart".into(),
        export_name: None,
        installed_at: 0,
        source: None,
    };

    let bar_chart = ComponentManifest {
        id: "bar-chart".into(),
        name: json!("Bar Chart"),
        description: json!("Bar chart for categorical or time-series data"),
        icon: "box".into(),
        category: "charts".into(),
        version: "1.0.0".into(),
        author: Some("NeoMind".into()),
        size_constraints: sc(3, 2, 6, 4, 12, 8),
        has_data_source: true,
        max_data_sources: Some(5),
        has_display_config: true,
        has_actions: false,
        has_device_binding: false,
        device_type_filter: vec![],
        config_schema: Some(json!({
            "display": {
                "type": "object",
                "properties": {
                    "unit": {"type": "string", "description": "Axis unit label"},
                    "showLegend": {"type": "boolean", "description": "Show legend"},
                    "showGrid": {"type": "boolean", "description": "Show grid lines"},
                    "showTooltip": {"type": "boolean", "description": "Show tooltip on hover"},
                    "horizontal": {"type": "boolean", "description": "Horizontal bar orientation"}
                }
            },
            "config": {"type": "object", "properties": {}}
        })),
        default_config: Some(json!({"display": {"showLegend": true, "showGrid": true}})),
        variants: None,
        global_name: "bar_chart".into(),
        export_name: None,
        installed_at: 0,
        source: None,
    };

    let pie_chart = ComponentManifest {
        id: "pie-chart".into(),
        name: json!("Pie Chart"),
        description: json!("Pie chart for distribution/proportion data"),
        icon: "box".into(),
        category: "charts".into(),
        version: "1.0.0".into(),
        author: Some("NeoMind".into()),
        size_constraints: sc(2, 2, 4, 4, 8, 8),
        has_data_source: true,
        max_data_sources: Some(1),
        has_display_config: true,
        has_actions: false,
        has_device_binding: false,
        device_type_filter: vec![],
        config_schema: Some(json!({
            "display": {
                "type": "object",
                "properties": {
                    "showLabels": {"type": "boolean", "description": "Show labels on slices"},
                    "showLegend": {"type": "boolean", "description": "Show legend"},
                    "showTooltip": {"type": "boolean", "description": "Show tooltip on hover"},
                    "innerRadius": {"type": "number", "description": "Inner radius for donut (0-1)"}
                }
            },
            "config": {"type": "object", "properties": {}}
        })),
        default_config: Some(json!({"display": {"showLabels": true, "showLegend": true}})),
        variants: None,
        global_name: "pie_chart".into(),
        export_name: None,
        installed_at: 0,
        source: None,
    };

    let radar_chart = ComponentManifest {
        id: "radar-chart".into(),
        name: json!("Radar Chart"),
        description: json!("Radar chart for multi-dimensional data"),
        icon: "box".into(),
        category: "charts".into(),
        version: "1.0.0".into(),
        author: Some("NeoMind".into()),
        size_constraints: sc(3, 3, 4, 4, 8, 8),
        has_data_source: true,
        max_data_sources: Some(1),
        has_display_config: true,
        has_actions: false,
        has_device_binding: false,
        device_type_filter: vec![],
        config_schema: Some(json!({
            "display": {
                "type": "object",
                "properties": {
                    "showLegend": {"type": "boolean", "description": "Show legend"},
                    "showTooltip": {"type": "boolean", "description": "Show tooltip on hover"}
                }
            },
            "config": {"type": "object", "properties": {}}
        })),
        default_config: None,
        variants: None,
        global_name: "radar_chart".into(),
        export_name: None,
        installed_at: 0,
        source: None,
    };

    // -- Controls --
    let toggle_switch = ComponentManifest {
        id: "toggle-switch".into(),
        name: json!("Toggle Switch"),
        description: json!("On/off switch for device control"),
        icon: "box".into(),
        category: "controls".into(),
        version: "1.0.0".into(),
        author: Some("NeoMind".into()),
        size_constraints: sc(1, 1, 2, 1, 4, 2),
        has_data_source: true,
        max_data_sources: Some(1),
        has_display_config: true,
        has_actions: true,
        has_device_binding: false,
        device_type_filter: vec![],
        config_schema: Some(json!({
            "display": {
                "type": "object",
                "properties": {
                    "label": {"type": "string", "description": "Switch label text"}
                }
            },
            "config": {"type": "object", "properties": {}}
        })),
        default_config: None,
        variants: None,
        global_name: "toggle_switch".into(),
        export_name: None,
        installed_at: 0,
        source: None,
    };

    // -- Display --
    let markdown_display = ComponentManifest {
        id: "markdown-display".into(),
        name: json!("Markdown Display"),
        description: json!("Render markdown content in dashboard"),
        icon: "box".into(),
        category: "display".into(),
        version: "1.0.0".into(),
        author: Some("NeoMind".into()),
        size_constraints: sc(2, 2, 4, 3, 12, 8),
        has_data_source: false,
        max_data_sources: Some(0),
        has_display_config: true,
        has_actions: false,
        has_device_binding: false,
        device_type_filter: vec![],
        config_schema: Some(json!({
            "display": {"type": "object", "properties": {}},
            "config": {
                "type": "object",
                "properties": {
                    "content": {"type": "string", "description": "Markdown content to render"}
                }
            }
        })),
        default_config: Some(json!({"config": {"content": "# Title\n\nContent here"}})),
        variants: None,
        global_name: "markdown_display".into(),
        export_name: None,
        installed_at: 0,
        source: None,
    };

    let image_display = ComponentManifest {
        id: "image-display".into(),
        name: json!("Image Display"),
        description: json!("Display images from URL or data source"),
        icon: "box".into(),
        category: "display".into(),
        version: "1.0.0".into(),
        author: Some("NeoMind".into()),
        size_constraints: sc(2, 2, 4, 3, 12, 8),
        has_data_source: true,
        max_data_sources: Some(1),
        has_display_config: true,
        has_actions: false,
        has_device_binding: false,
        device_type_filter: vec![],
        config_schema: Some(json!({
            "display": {
                "type": "object",
                "properties": {
                    "fit": {"type": "string", "description": "Image fit mode (cover, contain, fill)"},
                    "caption": {"type": "string", "description": "Image caption text"}
                }
            },
            "config": {
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "Static image URL"}
                }
            }
        })),
        default_config: None,
        variants: None,
        global_name: "image_display".into(),
        export_name: None,
        installed_at: 0,
        source: None,
    };

    let image_history = ComponentManifest {
        id: "image-history".into(),
        name: json!("Image History"),
        description: json!("Browse historical images with timeline"),
        icon: "box".into(),
        category: "display".into(),
        version: "1.0.0".into(),
        author: Some("NeoMind".into()),
        size_constraints: sc(3, 3, 6, 5, 12, 8),
        has_data_source: true,
        max_data_sources: Some(1),
        has_display_config: true,
        has_actions: false,
        has_device_binding: false,
        device_type_filter: vec![],
        config_schema: Some(json!({
            "display": {
                "type": "object",
                "properties": {
                    "fit": {"type": "string", "description": "Image fit mode"},
                    "showTimestamp": {"type": "boolean", "description": "Show timestamp on images"}
                }
            },
            "config": {
                "type": "object",
                "properties": {
                    "limit": {"type": "number", "description": "Max images to display"},
                    "timeRange": {"type": "string", "description": "Time range for history"}
                }
            }
        })),
        default_config: None,
        variants: None,
        global_name: "image_history".into(),
        export_name: None,
        installed_at: 0,
        source: None,
    };

    let web_display = ComponentManifest {
        id: "web-display".into(),
        name: json!("Web Display"),
        description: json!("Embed external web pages in dashboard"),
        icon: "box".into(),
        category: "display".into(),
        version: "1.0.0".into(),
        author: Some("NeoMind".into()),
        size_constraints: sc(3, 2, 6, 4, 12, 8),
        has_data_source: true,
        max_data_sources: Some(1),
        has_display_config: true,
        has_actions: false,
        has_device_binding: false,
        device_type_filter: vec![],
        config_schema: Some(json!({
            "display": {
                "type": "object",
                "properties": {
                    "sandbox": {"type": "string", "description": "Sandbox restrictions for iframe"},
                    "allowFullscreen": {"type": "boolean", "description": "Allow fullscreen mode"}
                }
            },
            "config": {
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "URL to embed"}
                }
            }
        })),
        default_config: None,
        variants: None,
        global_name: "web_display".into(),
        export_name: None,
        installed_at: 0,
        source: None,
    };

    // -- Spatial --
    let map_display = ComponentManifest {
        id: "map-display".into(),
        name: json!("Map Display"),
        description: json!("Display locations on an interactive map"),
        icon: "box".into(),
        category: "spatial".into(),
        version: "1.0.0".into(),
        author: Some("NeoMind".into()),
        size_constraints: sc(3, 3, 6, 4, 12, 8),
        has_data_source: true,
        max_data_sources: Some(10),
        has_display_config: true,
        has_actions: false,
        has_device_binding: false,
        device_type_filter: vec![],
        config_schema: Some(json!({
            "display": {
                "type": "object",
                "properties": {
                    "center": {"type": "object", "description": "Map center {lat, lng}"},
                    "zoom": {"type": "number", "description": "Initial zoom level"},
                    "markerColor": {"type": "string", "description": "Default marker color"}
                }
            },
            "config": {"type": "object", "properties": {}}
        })),
        default_config: None,
        variants: None,
        global_name: "map_display".into(),
        export_name: None,
        installed_at: 0,
        source: None,
    };

    let video_display = ComponentManifest {
        id: "video-display".into(),
        name: json!("Video Display"),
        description: json!("Display live video or RTSP stream"),
        icon: "box".into(),
        category: "spatial".into(),
        version: "1.0.0".into(),
        author: Some("NeoMind".into()),
        size_constraints: sc(3, 2, 6, 4, 12, 8),
        has_data_source: true,
        max_data_sources: Some(1),
        has_display_config: true,
        has_actions: false,
        has_device_binding: false,
        device_type_filter: vec![],
        config_schema: Some(json!({
            "display": {
                "type": "object",
                "properties": {
                    "type": {"type": "string", "description": "Video source type (rtsp, hls, webrtc)"},
                    "autoplay": {"type": "boolean", "description": "Auto-play video"},
                    "muted": {"type": "boolean", "description": "Mute audio"},
                    "controls": {"type": "boolean", "description": "Show video controls"}
                }
            },
            "config": {
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "Video stream URL"}
                }
            }
        })),
        default_config: None,
        variants: None,
        global_name: "video_display".into(),
        export_name: None,
        installed_at: 0,
        source: None,
    };

    let custom_layer = ComponentManifest {
        id: "custom-layer".into(),
        name: json!("Custom Layer"),
        description: json!("Custom overlay layer with icons and text"),
        icon: "box".into(),
        category: "spatial".into(),
        version: "1.0.0".into(),
        author: Some("NeoMind".into()),
        size_constraints: sc(3, 3, 6, 4, 12, 8),
        has_data_source: true,
        max_data_sources: Some(20),
        has_display_config: true,
        has_actions: false,
        has_device_binding: false,
        device_type_filter: vec![],
        config_schema: Some(json!({
            "display": {
                "type": "object",
                "properties": {
                    "backgroundType": {"type": "string", "description": "Background type (image, color, map)"},
                    "gridSize": {"type": "number", "description": "Grid cell size in pixels"}
                }
            },
            "config": {"type": "object", "properties": {}}
        })),
        default_config: None,
        variants: None,
        global_name: "custom_layer".into(),
        export_name: None,
        installed_at: 0,
        source: None,
    };

    // -- Business --
    let agent_monitor_widget = ComponentManifest {
        id: "agent-monitor-widget".into(),
        name: json!("Agent Monitor"),
        description: json!("AI agent activity monitor with conversation view"),
        icon: "box".into(),
        category: "business".into(),
        version: "1.0.0".into(),
        author: Some("NeoMind".into()),
        size_constraints: sc(3, 3, 6, 5, 12, 8),
        has_data_source: true,
        max_data_sources: Some(1),
        has_display_config: false,
        has_actions: false,
        has_device_binding: false,
        device_type_filter: vec![],
        config_schema: Some(json!({
            "display": {"type": "object", "properties": {}},
            "config": {"type": "object", "properties": {}}
        })),
        default_config: None,
        variants: None,
        global_name: "agent_monitor_widget".into(),
        export_name: None,
        installed_at: 0,
        source: None,
    };

    let ai_analyst = ComponentManifest {
        id: "ai-analyst".into(),
        name: json!("AI Analyst"),
        description: json!("AI-powered data analysis and insights"),
        icon: "box".into(),
        category: "business".into(),
        version: "1.0.0".into(),
        author: Some("NeoMind".into()),
        size_constraints: sc(3, 3, 4, 5, 12, 8),
        has_data_source: true,
        max_data_sources: Some(1),
        has_display_config: true,
        has_actions: false,
        has_device_binding: false,
        device_type_filter: vec![],
        config_schema: Some(json!({
            "display": {"type": "object", "properties": {}},
            "config": {"type": "object", "properties": {}}
        })),
        default_config: None,
        variants: None,
        global_name: "ai_analyst".into(),
        export_name: None,
        installed_at: 0,
        source: None,
    };

    vec![
        value_card,
        led_indicator,
        sparkline,
        progress_bar,
        line_chart,
        area_chart,
        bar_chart,
        pie_chart,
        radar_chart,
        toggle_switch,
        markdown_display,
        image_display,
        image_history,
        web_display,
        map_display,
        video_display,
        custom_layer,
        agent_monitor_widget,
        ai_analyst,
    ]
}

/// GET `/api/frontend-components/:id/bundle`
///
/// Serve a component's bundle.js file.
/// Public endpoint — allows unauthenticated loading of component bundles.
pub async fn get_bundle_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<Response, ErrorResponse> {
    validate_component_id(&id)?;

    let store = state.frontend_component_store.clone();
    let id_for_path = id.clone();
    let bundle_path = tokio::task::spawn_blocking(move || store.get_bundle_path(&id_for_path))
        .await
        .map_err(|e| ErrorResponse::internal(format!("Task failed: {}", e)))?;

    let bundle_path = bundle_path.ok_or_else(|| {
        ErrorResponse::not_found(format!("Bundle not found for component '{}'", id))
    })?;

    let bytes = tokio::task::spawn_blocking(move || std::fs::read(&bundle_path))
        .await
        .map_err(|e| ErrorResponse::internal(format!("Read task failed: {}", e)))?
        .map_err(|e| ErrorResponse::internal(format!("Failed to read bundle: {}", e)))?;

    Ok(Response::builder()
        .status(200)
        .header("content-type", "application/javascript")
        .header("cache-control", "no-cache")
        .body(axum::body::Body::from(bytes))
        .unwrap())
}

/// DELETE `/api/frontend-components/:id`
///
/// Uninstall (delete) a community component.
/// Protected endpoint — requires authentication.
pub async fn uninstall_component_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    validate_component_id(&id)?;

    // Check component exists
    let store = state.frontend_component_store.clone();
    let exists = tokio::task::spawn_blocking({
        let store = store.clone();
        let id = id.clone();
        move || store.exists(&id)
    })
    .await
    .map_err(|e| ErrorResponse::internal(format!("Task failed: {}", e)))?;

    if !exists {
        return Err(ErrorResponse::not_found(format!(
            "Component '{}' is not installed",
            id
        )));
    }

    // Delete via store
    let id_for_event = id.clone();
    tokio::task::spawn_blocking(move || store.delete(&id))
        .await
        .map_err(|e| ErrorResponse::internal(format!("Delete task failed: {}", e)))?
        .map_err(|e| ErrorResponse::internal(format!("Failed to delete component: {}", e)))?;

    // Publish lifecycle event
    publish_lifecycle_event(&state, &id_for_event, "uninstalled").await;

    tracing::info!(
        component_id = %id_for_event,
        "Component uninstalled"
    );

    ok(json!({
        "success": true,
        "component_id": id_for_event,
        "state": "uninstalled"
    }))
}
