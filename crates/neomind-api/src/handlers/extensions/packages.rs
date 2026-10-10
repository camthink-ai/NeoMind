//! `packages` handlers — split from the former extensions.rs monolith.

use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;
use std::path::PathBuf;

use base64::engine::general_purpose::STANDARD;
use serde_json::json;

use crate::handlers::common::{ok, HandlerResult};
use crate::models::error::ErrorResponse;
use crate::server::ServerState;
use neomind_storage::ExtensionRecord;

use super::*;

/// POST /api/extensions/upload
/// Upload and install an extension package (.nep file).
/// Note: This endpoint requires the .nep file to be manually uploaded to the data directory first.
/// POST body: { "file_path": "/path/to/package.nep" }
pub async fn upload_extension_package_handler(
    State(state): State<ServerState>,
    Json(req): Json<UploadPackageRequest>,
) -> HandlerResult<serde_json::Value> {
    use neomind_core::extension::package::ExtensionPackage;

    let file_path = resolve_confined_package_path(&req.file_path)?;

    // Load the package
    let package = ExtensionPackage::load(&file_path)
        .await
        .map_err(|e| ErrorResponse::bad_request(format!("Invalid package: {}", e)))?;

    let ext_id = package.manifest.id.clone();
    let version = package.manifest.version.clone();
    let name = package.manifest.name.clone();

    tracing::info!(
        extension_id = %ext_id,
        version = %version,
        name = %name,
        checksum = %package.checksum,
        size = package.size,
        "Processing extension package upload"
    );

    // Check if extension is already registered
    let runtime = &state.extensions.runtime;
    let is_registered = runtime.contains(&ext_id).await;

    if is_registered {
        // Unregister existing version first
        tracing::info!("Extension {} already registered, will replace", ext_id);
        runtime.unregister(&ext_id).await.map_err(|e| {
            ErrorResponse::internal(format!("Failed to unregister existing: {}", e))
        })?;
    }

    // Install the package
    let data_dir = neomind_core::paths::data_dir()
        .to_string_lossy()
        .to_string();
    let target_dir = PathBuf::from(data_dir).join("extensions");

    let install_result = package
        .install(&target_dir)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Installation failed: {}", e)))?;

    tracing::info!(
        extension_id = %install_result.extension_id,
        binary_path = %install_result.binary_path.display(),
        manifest_path = %install_result.manifest_path.display(),
        frontend_dir = ?install_result.frontend_dir,
        components_count = install_result.components.len(),
        "Package installed successfully"
    );

    // Load and register the extension binary (handles both isolated and in-process)
    let _metadata = runtime
        .load(&install_result.binary_path)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to load extension binary: {}", e)))?;

    // Save to storage
    let store = state.extensions.store.clone();
    {
        let record = ExtensionRecord::new(
            ext_id.clone(),
            name.clone(),
            install_result.binary_path.to_string_lossy().to_string(),
            package.manifest.extension_type.clone(),
            version.clone(),
        )
        .with_description(package.manifest.description.clone())
        .with_author(package.manifest.author.clone())
        .with_checksum(Some(install_result.checksum.clone()))
        .with_auto_start(true)
        .with_frontend_path(
            install_result
                .frontend_dir
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
        );

        if let Err(e) = store.save(&record) {
            tracing::warn!("Failed to save extension to storage: {}", e);
        }
    }

    // Rebuild tool registry so the new extension's tools are visible to the LLM
    state.refresh_extension_tools().await;

    // Build response
    ok(serde_json::json!({
        "message": "Extension package installed successfully",
        "extension_id": ext_id,
        "name": name,
        "version": version,
        "description": package.manifest.description,
        "author": package.manifest.author,
        "checksum": install_result.checksum,
        "binary_path": install_result.binary_path.to_string_lossy(),
        "manifest_path": install_result.manifest_path.to_string_lossy(),
        "frontend_dir": install_result.frontend_dir.as_ref()
            .map(|p| p.to_string_lossy().to_string()),
        "components_count": install_result.components.len(),
        "components": install_result.components.iter().map(|c| json!({
            "type": c.component_type,
            "name": c.name,
            "description": c.description,
            "category": c.category
        })).collect::<Vec<_>>(),
        "replaced": is_registered
    }))
}

/// POST /api/extensions/package/validate
/// Validate an extension package without installing.
pub async fn validate_extension_package_handler(
    Json(req): Json<ValidatePackageRequest>,
) -> HandlerResult<serde_json::Value> {
    use neomind_core::extension::package::ExtensionPackage;

    let file_path = resolve_confined_package_path(&req.file_path)?;

    let package = ExtensionPackage::load(&file_path)
        .await
        .map_err(|e| ErrorResponse::bad_request(format!("Invalid package: {}", e)))?;

    let platform = detect_platform();
    let has_binary = package.get_binary_path().is_some();
    let has_frontend = package.manifest.frontend.is_some();
    let components_count = package
        .manifest
        .frontend
        .as_ref()
        .map(|f| f.components.len())
        .unwrap_or(0);

    ok(serde_json::json!({
        "valid": true,
        "format": package.manifest.format,
        "format_version": package.manifest.format_version,
        "extension_id": package.manifest.id,
        "name": package.manifest.name,
        "version": package.manifest.version,
        "description": package.manifest.description,
        "author": package.manifest.author,
        "license": package.manifest.license,
        "current_platform": platform,
        "has_binary_for_platform": has_binary,
        "has_frontend": has_frontend,
        "components_count": components_count,
        "capabilities": package.manifest.capabilities,
        "permissions": package.manifest.permissions,
        "checksum": package.checksum,
        "size": package.size
    }))
}

/// Upload package request
#[derive(Debug, Deserialize)]
pub struct UploadPackageRequest {
    pub file_path: String,
}

/// Validate package request
#[derive(Debug, Deserialize)]
pub struct ValidatePackageRequest {
    pub file_path: String,
}

/// DELETE /api/extensions/:id/uninstall
/// Completely uninstall an extension (remove all files).
#[utoipa::path(
    delete,
    path = "/api/extensions/{id}/uninstall",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
    ),
    responses(
        (status = 200, description = "Extension uninstalled and files removed"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn uninstall_extension_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Prevent path traversal via id (used in remove_dir_all below)
    validate_extension_id(&id)?;

    let runtime = &state.extensions.runtime;

    // Check if extension exists
    let exists = runtime.contains(&id).await;
    let ext_info = if exists { runtime.get(&id).await } else { None };

    // Unregister from memory
    if exists {
        runtime
            .unregister(&id)
            .await
            .map_err(|e| ErrorResponse::internal(format!("Failed to unregister: {}", e)))?;
    }

    // Mark as uninstalled in storage
    let store = state.extensions.store.clone();
    {
        if let Err(e) = store.mark_uninstalled(&id) {
            tracing::warn!("Failed to mark extension as uninstalled: {}", e);
        }
    }

    // Clean up extension directory
    let data_dir = neomind_core::paths::data_dir()
        .to_string_lossy()
        .to_string();
    let extensions_dir = PathBuf::from(data_dir).join("extensions");
    let ext_dir = extensions_dir.join(&id);

    let mut removed_files = Vec::new();
    if ext_dir.exists() {
        tracing::info!(
            "Removing extension directory (data/ preserved): {}",
            ext_dir.display()
        );
        // Preserve the platform-guaranteed private `data/` subdir — it holds
        // user state (pipelines, face libraries, licenses) that must survive
        // uninstall; everything else (package files) goes.
        let preserved_data_dir = ext_dir.join("data");
        let mut entries = tokio::fs::read_dir(&ext_dir).await.map_err(|e| {
            ErrorResponse::internal(format!("Failed to read extension directory: {}", e))
        })?;
        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            ErrorResponse::internal(format!("Failed to read extension directory: {}", e))
        })? {
            let path = entry.path();
            if path == preserved_data_dir {
                continue;
            }
            if path.is_dir() {
                tokio::fs::remove_dir_all(&path).await.map_err(|e| {
                    ErrorResponse::internal(format!("Failed to remove extension directory: {}", e))
                })?;
            } else {
                tokio::fs::remove_file(&path).await.map_err(|e| {
                    ErrorResponse::internal(format!("Failed to remove extension file: {}", e))
                })?;
            }
        }
        removed_files.push(ext_dir.to_string_lossy().to_string());
    }

    // Clean up extension metrics
    cleanup_extension_metrics(&state, &id).await;

    // Rebuild tool registry to remove the uninstalled extension's tools
    state.refresh_extension_tools().await;

    ok(serde_json::json!({
        "message": "Extension uninstalled completely",
        "extension_id": id,
        "name": ext_info.map(|info| info.metadata.name),
        "removed_files": removed_files,
        "note": "All extension files, including frontend components, have been removed"
    }))
}

/// POST /api/extensions/upload/file
/// Upload an extension package file directly (.nep format).
///
/// This endpoint accepts a JSON body with base64-encoded file data.
///
/// Request body:
/// ```json
/// {
///   "data": "<base64-encoded .nep file>",
///   "filename": "extension.nep"
/// }
/// ```
///
/// Example with curl:
/// ```bash
/// # First encode the file to base64
/// BASE64_DATA=$(base64 -w 0 extension.nep)
/// curl -X POST http://localhost:9375/api/extensions/upload/file \
///   -H "Content-Type: application/json" \
///   -d "{\"data\": \"$BASE64_DATA\"}"
/// ```
#[derive(utoipa::ToSchema, Debug, serde::Deserialize)]
pub struct UploadExtensionFileRequest {
    /// Base64-encoded .nep file data
    pub data: String,
    /// Optional filename
    pub filename: Option<String>,
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/api/extensions/upload/file",
    tag = "extensions",
    request_body = UploadExtensionFileRequest,
    responses(
        (status = 200, description = "Package file accepted for staging (100MB limit)"),
    )
)]
pub async fn upload_extension_file_handler(
    State(state): State<ServerState>,
    Json(req): Json<UploadExtensionFileRequest>,
) -> HandlerResult<serde_json::Value> {
    // Log upload request details
    let data_len = req.data.len();
    let filename = req.filename.as_deref().unwrap_or("unknown");
    tracing::info!(
        "Extension upload request received: filename={}, base64_size={}MB",
        filename,
        data_len / 1_000_000
    );

    // Decode base64 data
    let body_bytes = STANDARD
        .decode(&req.data)
        .map_err(|e| ErrorResponse::bad_request(format!("Invalid base64 data: {}", e)))?;

    tracing::info!(
        "Base64 decoded successfully: binary_size={}MB",
        body_bytes.len() / 1_000_000
    );

    // Check if this looks like a ZIP file
    if body_bytes.len() < 4 {
        return Err(ErrorResponse::bad_request(
            "File too small to be a valid package",
        ));
    }

    let zip_magic: &[u8] = &[0x50, 0x4B, 0x03, 0x04];
    let zip_empty: &[u8] = &[0x50, 0x4B, 0x05, 0x06];
    let zip_spanned: &[u8] = &[0x50, 0x4B, 0x07, 0x08];

    let is_zip = body_bytes.starts_with(zip_magic)
        || body_bytes.starts_with(zip_empty)
        || body_bytes.starts_with(zip_spanned);

    if !is_zip {
        return Err(ErrorResponse::bad_request(
            "File is not a valid ZIP archive",
        ));
    }

    // Prepare target directory
    let data_dir = neomind_core::paths::data_dir()
        .to_string_lossy()
        .to_string();
    let target_dir = PathBuf::from(data_dir).join("extensions");

    // Step 1: Parse the package to get extension ID (validation only, no install yet)
    // This allows us to check if the extension is registered BEFORE overwriting files
    let body_bytes_for_validate = body_bytes.clone();
    let (ext_id, version) = tokio::task::spawn_blocking(move || {
        use neomind_core::extension::package::ExtensionPackage;
        let package = ExtensionPackage::from_bytes(body_bytes_for_validate)
            .map_err(|e| format!("Package parse error: {}", e))?;
        Ok::<_, String>((
            package.manifest.id.clone(),
            package.manifest.version.clone(),
        ))
    })
    .await
    .map_err(|e| ErrorResponse::internal(format!("Task join error: {}", e)))?
    .map_err(|e| ErrorResponse::internal(format!("Package validation failed: {}", e)))?;

    tracing::info!(
        extension_id = %ext_id,
        version = %version,
        "Package validated successfully"
    );

    // Step 2: Check if already registered and unload FIRST (before overwriting files)
    // This is critical on macOS where overwriting a dylib that's in use can cause issues
    let runtime = state.extensions.runtime.clone();
    let is_registered = runtime.contains(&ext_id).await;

    if is_registered {
        tracing::info!(
            "Extension {} already registered, unloading before update",
            ext_id
        );
        runtime.unload(&ext_id).await.map_err(|e| {
            ErrorResponse::internal(format!("Failed to unload existing extension: {}", e))
        })?;
        // Wait a moment for the process to fully terminate and release file handles
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }

    // Step 3: Now install the package (safe to overwrite files)
    // body_bytes is no longer needed after this point, move it directly
    let target_dir_clone = target_dir.clone();
    let install_result = tokio::task::spawn_blocking(move || {
        use neomind_core::extension::package::ExtensionPackage;
        ExtensionPackage::install_sync(&body_bytes, &target_dir_clone)
    })
    .await
    .map_err(|e| {
        tracing::error!("install_sync task join error for {}: {}", ext_id, e);
        ErrorResponse::internal(format!("Task join error: {}", e))
    })?
    .map_err(|e| {
        tracing::error!("install_sync failed for {}: {} (kind={:?})", ext_id, e, e);
        ErrorResponse::internal(format!("Installation failed: {}", e))
    })?;

    tracing::info!(
        extension_id = %install_result.extension_id,
        binary_path = %install_result.binary_path.display(),
        frontend_dir = ?install_result.frontend_dir,
        components_count = install_result.components.len(),
        "Package installed successfully"
    );

    // Step 4: Load and register the extension binary with process isolation
    let metadata = runtime
        .load(&install_result.binary_path)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to load extension binary: {}", e)))?;

    // Determine extension type from binary path
    let extension_type = install_result
        .binary_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| if e == "wasm" { "wasm" } else { "native" })
        .unwrap_or("native")
        .to_string();

    // Save to storage
    let store = state.extensions.store.clone();
    {
        let record = ExtensionRecord::new(
            ext_id.clone(),
            metadata.name.clone(),
            install_result.binary_path.to_string_lossy().to_string(),
            extension_type,
            version.clone(),
        )
        .with_description(metadata.description.clone())
        .with_author(metadata.author.clone())
        .with_checksum(Some(install_result.checksum.clone()))
        .with_auto_start(true)
        .with_frontend_path(
            install_result
                .frontend_dir
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
        );

        if let Err(e) = store.save(&record) {
            tracing::warn!("Failed to save extension to storage: {}", e);
        }
    }

    // Rebuild tool registry so the new extension's tools are visible to the LLM
    state.refresh_extension_tools().await;

    // Build response
    ok(serde_json::json!({
        "message": "Extension package installed successfully",
        "extension_id": ext_id,
        "name": metadata.name,
        "version": version,
        "description": metadata.description,
        "author": metadata.author,
        "checksum": install_result.checksum,
        "binary_path": install_result.binary_path.to_string_lossy(),
        "manifest_path": install_result.manifest_path.to_string_lossy(),
        "frontend_dir": install_result.frontend_dir.as_ref()
            .map(|p| p.to_string_lossy().to_string()),
        "components_count": install_result.components.len(),
        "components": install_result.components.iter().map(|c| json!({
            "type": c.component_type,
            "name": c.name,
            "description": c.description,
            "category": c.category
        })).collect::<Vec<_>>(),
        "replaced": is_registered
    }))
}

/// POST /api/extensions/sync
///
/// Manually trigger extension synchronization from /extensions/ directory.
/// Register an on-disk-installed extension with the runtime: unregister the
/// old instance if any, load the binary, and upsert the ExtensionRecord
/// carrying the previous user config forward (same contract as the
/// marketplace install path). Shared by the sync handler and the startup
/// cache scan so they can't drift apart again.
pub(crate) async fn register_installed_package(
    state: &ServerState,
    pkg: &crate::server::install_service::InstalledPackage,
) -> Result<(), String> {
    let runtime = state.extensions.runtime.clone();

    // Replace a registered instance (mirrors the marketplace flow). An
    // unregister failure is fatal there; here we propagate the error too —
    // a half-replaced extension is worse than a reported failure.
    if runtime.contains(&pkg.extension_id).await {
        runtime
            .unregister(&pkg.extension_id)
            .await
            .map_err(|e| format!("failed to unregister old instance: {e}"))?;
    }

    let metadata = runtime
        .load(&pkg.binary_path)
        .await
        .map_err(|e| format!("failed to load extension: {e}"))?;

    let extension_type = pkg
        .binary_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| if e == "wasm" { "wasm" } else { "native" })
        .unwrap_or("native")
        .to_string();

    let store = state.extensions.store.clone();
    let preserved_config = store
        .load(&pkg.extension_id)
        .ok()
        .flatten()
        .and_then(|old| old.config.clone());
    {
        let mut record = ExtensionRecord::new(
            pkg.extension_id.clone(),
            metadata.name.clone(),
            pkg.binary_path.to_string_lossy().to_string(),
            extension_type,
            pkg.version.clone(),
        )
        .with_description(metadata.description.clone())
        .with_author(metadata.author.clone())
        .with_checksum(Some(pkg.checksum.clone()))
        .with_auto_start(true)
        .with_frontend_path(
            pkg.frontend_dir
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
        );
        if let Some(cfg) = preserved_config.clone() {
            record = record.with_config(cfg);
        }
        if let Err(e) = store.save(&record) {
            tracing::warn!(
                extension_id = %pkg.extension_id,
                error = %e,
                "Failed to save extension record after sync install"
            );
        }
    }

    state.refresh_extension_tools().await;

    if let Some(cfg) = preserved_config {
        if let Err(e) = runtime.send_config_update(&pkg.extension_id, &cfg).await {
            tracing::warn!(
                extension_id = %pkg.extension_id,
                error = %e,
                "Failed to apply preserved config after sync install"
            );
        }
    }

    Ok(())
}

#[utoipa::path(
    post,
    path = "/api/extensions/sync",
    tag = "extensions",
    responses(
        (status = 200, description = "Extensions directory re-scanned"),
    )
)]
pub async fn sync_extensions_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    use crate::server::ExtensionInstallService;

    let data_dir = neomind_core::paths::data_dir()
        .to_string_lossy()
        .to_string();
    let extensions_dir = std::path::PathBuf::from(&data_dir).join("extensions");
    // The cache IS the extensions dir: the marketplace download path already
    // drops .nep files there. (This used to scan a CWD-relative "extensions/"
    // that had nothing to do with the data dir.)
    let install_service = ExtensionInstallService::new(&extensions_dir, &extensions_dir);

    let report = install_service
        .sync_nep_cache()
        .await
        .map_err(|e| ErrorResponse::internal(format!("Sync failed: {}", e)))?;

    // Disk install is only half the job — register what changed. This sync
    // used to report "installed: N" while doing neither half.
    let mut registered = 0usize;
    let mut errors: Vec<serde_json::Value> = Vec::new();
    for pkg in &report.installed_packages {
        match register_installed_package(&state, pkg).await {
            Ok(()) => registered += 1,
            Err(e) => errors.push(serde_json::json!({
                "extension_id": pkg.extension_id,
                "error": e,
            })),
        }
    }

    ok(serde_json::json!({
        "message": "Extensions synchronized",
        "scanned": report.scanned,
        "installed": report.installed,
        "upgraded": report.upgraded,
        "skipped": report.skipped,
        "failed": report.failed,
        "registered": registered,
        "errors": errors,
    }))
}

/// GET /api/extensions/sync-status
#[utoipa::path(
    get,
    path = "/api/extensions/sync-status",
    tag = "extensions",
    responses(
        (status = 200, description = "Last sync result"),
    )
)]
pub async fn get_sync_status_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    use neomind_core::extension::package::ExtensionPackage;

    let nep_cache_dir = std::path::PathBuf::from("extensions");
    let data_dir = neomind_core::paths::data_dir()
        .to_string_lossy()
        .to_string();
    let install_dir = std::path::PathBuf::from(data_dir).join("extensions");

    let mut nep_packages = Vec::new();

    if nep_cache_dir.exists() {
        if let Ok(mut entries) = std::fs::read_dir(&nep_cache_dir) {
            while let Some(Ok(entry)) = entries.next() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("nep") {
                    continue;
                }

                let package_info = match ExtensionPackage::load(&path).await {
                    Ok(package) => {
                        let ext_id = package.manifest.id.clone();
                        serde_json::json!({
                            "filename": path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown"),
                            "extension_id": ext_id,
                            "version": package.manifest.version,
                            "name": package.manifest.name,
                            "installed": install_dir.join(&ext_id).exists(),
                            "size": path.metadata().map(|m| m.len()).unwrap_or(0),
                        })
                    }
                    Err(_) => {
                        serde_json::json!({
                            "filename": path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown"),
                            "error": "Failed to load package",
                        })
                    }
                };
                nep_packages.push(package_info);
            }
        }
    }

    ok(serde_json::json!({
        "nep_packages": nep_packages,
        "nep_cache_dir": nep_cache_dir.to_string_lossy().to_string(),
        "install_dir": install_dir.to_string_lossy().to_string(),
    }))
}
