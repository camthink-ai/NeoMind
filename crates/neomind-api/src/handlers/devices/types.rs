//! Device type management.

use axum::{
    extract::{Path, State},
    Json,
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use neomind_devices::registry::DeviceTypeTemplate;

use super::models::{
    CommandDefinitionDto, DeviceTypeDto, MetricDefinitionDto, ParameterDefinitionDto,
};
use crate::handlers::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::ErrorResponse;

/// List device types.
/// Uses new DeviceService - now includes metrics and commands
#[utoipa::path(
    get,
    path = "/api/device-types",
    tag = "device-types",
    responses(
        (status = 200, description = "Registered device-type templates"),
    )
)]
pub async fn list_device_types_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let templates = state.devices.service.list_templates();
    let dtos: Vec<DeviceTypeDto> = templates
        .into_iter()
        .map(|t| {
            let mode_str = match t.mode {
                neomind_devices::registry::DeviceTypeMode::Simple => "simple",
                neomind_devices::registry::DeviceTypeMode::Full => "full",
            };

            // Convert metrics to DTO
            let metrics: Vec<MetricDefinitionDto> = t
                .metrics
                .iter()
                .map(|m| MetricDefinitionDto {
                    name: m.name.clone(),
                    display_name: m.display_name.clone(),
                    data_type: format!("{:?}", m.data_type),
                    unit: if m.unit.is_empty() {
                        None
                    } else {
                        Some(m.unit.clone())
                    },
                    min: m.min,
                    max: m.max,
                })
                .collect();

            // Convert commands to DTO
            let commands: Vec<CommandDefinitionDto> = t
                .commands
                .iter()
                .map(|c| CommandDefinitionDto {
                    name: c.name.clone(),
                    display_name: c.display_name.clone(),
                    parameters: c
                        .parameters
                        .iter()
                        .map(|p| ParameterDefinitionDto {
                            name: p.name.clone(),
                            display_name: p.display_name.clone(),
                            data_type: format!("{:?}", p.data_type),
                        })
                        .collect(),
                })
                .collect();

            DeviceTypeDto {
                device_type: t.device_type.clone(),
                name: t.name.clone(),
                description: t.description.clone(),
                categories: t.categories.clone(),
                mode: mode_str.to_string(),
                metrics,
                commands,
                metric_count: Some(t.metrics.len()),
                command_count: Some(t.commands.len()),
                builtin_version: t.builtin_version.clone(),
                ct_prov: t.ct_prov.clone(),
            }
        })
        .collect();

    ok(json!({
        "device_types": dtos,
        "count": dtos.len(),
    }))
}

/// Get device type details.
/// Uses new DeviceService - returns simplified format (direct metrics/commands)
#[utoipa::path(
    get,
    path = "/api/device-types/{id}",
    tag = "device-types",
    params(
        ("id" = String, Path, description = "Device type id"),
    ),
    responses(
        (status = 200, description = "One device-type template"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_device_type_handler(
    State(state): State<ServerState>,
    Path(device_type): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let template = state
        .devices
        .service
        .get_template(&device_type)
        .ok_or_else(|| ErrorResponse::not_found("DeviceType"))?;

    let mode_str = match template.mode {
        neomind_devices::registry::DeviceTypeMode::Simple => "simple",
        neomind_devices::registry::DeviceTypeMode::Full => "full",
    };

    // Return in new simplified format (direct metrics/commands arrays).
    // Include store_raw + default_offline_timeout_secs so exports round-trip
    // (GET is the path the UI export uses; omitting them drops these settings).
    ok(json!({
        "device_type": template.device_type,
        "name": template.name,
        "description": template.description,
        "categories": template.categories,
        "mode": mode_str,
        "metrics": template.metrics,
        "uplink_samples": template.uplink_samples,
        "commands": template.commands,
        "default_offline_timeout_secs": template.default_offline_timeout_secs,
        "store_raw": template.store_raw,
        "builtin_version": template.builtin_version,
        "ct_prov": template.ct_prov,
        "metric_count": template.metrics.len(),
        "command_count": template.commands.len(),
    }))
}

/// Register a new device type.
/// Uses new DeviceService - accepts simplified format (direct metrics/commands)
#[utoipa::path(
    post,
    path = "/api/device-types",
    tag = "device-types",
    request_body = DeviceTypeTemplate,
    responses(
        (status = 200, description = "Template registered"),
    )
)]
pub async fn register_device_type_handler(
    State(state): State<ServerState>,
    Json(template): Json<DeviceTypeTemplate>,
) -> HandlerResult<serde_json::Value> {
    // Save ID before moving the template
    let type_id = template.device_type.clone();
    let type_name = template.name.clone();

    // Register the template directly (already in simplified format)
    state
        .devices
        .service
        .register_template(template)
        .await
        .map_err(|e| {
            ErrorResponse::bad_request(format!("Failed to register device type: {}", e))
        })?;

    ok(json!({
        "registered": true,
        "device_type": type_id,
        "name": type_name,
    }))
}

/// Delete a device type.
/// Uses new DeviceService
#[utoipa::path(
    delete,
    path = "/api/device-types/{id}",
    tag = "device-types",
    params(
        ("id" = String, Path, description = "Device type id"),
    ),
    responses(
        (status = 200, description = "Template deleted"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn delete_device_type_handler(
    State(state): State<ServerState>,
    Path(device_type): Path<String>,
) -> HandlerResult<serde_json::Value> {
    state
        .devices
        .service
        .unregister_template(&device_type)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to delete device type: {}", e)))?;

    ok(json!({
        "device_type": device_type,
        "deleted": true,
    }))
}

/// Validate a device type definition without registering it.
/// Accepts simplified format (direct metrics/commands)
#[utoipa::path(
    put,
    path = "/api/device-types",
    tag = "device-types",
    request_body = DeviceTypeTemplate,
    responses(
        (status = 200, description = "Validation report for a template (nothing saved)"),
    )
)]
pub async fn validate_device_type_handler(
    Json(template): Json<DeviceTypeTemplate>,
) -> HandlerResult<serde_json::Value> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Validate required fields
    if template.device_type.is_empty() {
        errors.push("device_type 不能为空".to_string());
    } else if !template
        .device_type
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        errors.push("device_type 只能包含字母、数字、下划线和连字符".to_string());
    }

    if template.name.is_empty() {
        errors.push("name 不能为空".to_string());
    }

    // Validate categories
    for category in &template.categories {
        if category.is_empty() {
            warnings.push("categories 中包含空字符串".to_string());
        }
    }

    // Validate metrics (simplified structure - direct array)
    for (idx, metric) in template.metrics.iter().enumerate() {
        if metric.name.is_empty() {
            errors.push(format!("metrics[{}]: name 不能为空", idx));
        }
        if metric.display_name.is_empty() {
            warnings.push(format!("metrics[{}]: display_name 为空", idx));
        }
        // Validate data type
        match metric.data_type {
            neomind_devices::MetricDataType::Integer
            | neomind_devices::MetricDataType::Float
            | neomind_devices::MetricDataType::String
            | neomind_devices::MetricDataType::Boolean
            | neomind_devices::MetricDataType::Binary
            | neomind_devices::MetricDataType::Enum { .. }
            | neomind_devices::MetricDataType::Array { .. } => {}
        }
        // Validate min/max for numeric types
        if matches!(
            metric.data_type,
            neomind_devices::MetricDataType::Integer | neomind_devices::MetricDataType::Float
        ) {
            let (min, max) = (metric.min, metric.max);
            if let (Some(min_val), Some(max_val)) = (min, max) {
                if min_val > max_val {
                    errors.push(format!(
                        "metrics[{}]: min ({}) 不能大于 max ({})",
                        idx, min_val, max_val
                    ));
                }
            }
        }
    }

    // Validate commands (simplified structure - direct array)
    for (idx, command) in template.commands.iter().enumerate() {
        if command.name.is_empty() {
            errors.push(format!("commands[{}]: name 不能为空", idx));
        }
        if command.payload_template.is_empty() {
            warnings.push(format!("commands[{}]: payload_template 为空", idx));
        }
        // Validate parameters
        for (pidx, param) in command.parameters.iter().enumerate() {
            if param.name.is_empty() {
                errors.push(format!(
                    "commands[{}].parameters[{}]: name 不能为空",
                    idx, pidx
                ));
            }
        }
    }

    // Check for duplicate metric names
    let mut metric_names = std::collections::HashSet::new();
    for metric in &template.metrics {
        if !metric_names.insert(&metric.name) {
            errors.push(format!("metrics: 存在重复的指标名称 '{}'", metric.name));
        }
    }

    // Check for duplicate command names
    let mut command_names = std::collections::HashSet::new();
    for command in &template.commands {
        if !command_names.insert(&command.name) {
            errors.push(format!("commands: 存在重复的命令名称 '{}'", command.name));
        }
    }

    if errors.is_empty() {
        ok(json!({
            "valid": true,
            "warnings": warnings,
            "message": "设备类型定义有效"
        }))
    } else {
        ok(json!({
            "valid": false,
            "errors": errors,
            "warnings": warnings,
            "message": format!("发现 {} 个错误", errors.len())
        }))
    }
}

// ============================================================================
// CLOUD DEVICE TYPE IMPORT
// ============================================================================

/// Configuration for cloud device type repository
const CLOUD_REPO: &str = "camthink-ai/NeoMind-DeviceTypes";
const CLOUD_BRANCH: &str = "main";
const CLOUD_BASE_URL: &str = "https://raw.githubusercontent.com/camthink-ai/NeoMind-DeviceTypes";
const MAX_CLOUD_IMPORT_ITEMS: usize = 50;
const MAX_CLOUD_FETCH_CONCURRENCY: usize = 8;
const MAX_CLOUD_RESPONSE_BYTES: usize = 512 * 1024;
const CLOUD_LIST_CACHE_TTL: Duration = Duration::from_secs(300);

static CLOUD_HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn cloud_http_client() -> &'static reqwest::Client {
    CLOUD_HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("NeoMind-DeviceType-Importer")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

struct CloudListCacheEntry {
    fetched_at: Instant,
    payload: serde_json::Value,
}

static CLOUD_LIST_CACHE: OnceLock<tokio::sync::RwLock<Option<CloudListCacheEntry>>> =
    OnceLock::new();

fn cloud_list_cache() -> &'static tokio::sync::RwLock<Option<CloudListCacheEntry>> {
    CLOUD_LIST_CACHE.get_or_init(|| tokio::sync::RwLock::new(None))
}

/// Strictly validate cloud repository branch / device_type_id against
/// `1..=64` ASCII `[A-Za-z0-9_-]` to prevent URL path traversal / query injection.
fn is_valid_cloud_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Read a reqwest response body with a strict byte cap.
async fn read_capped_bytes(
    mut response: reqwest::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    if let Some(len) = response.content_length() {
        if len > max_bytes as u64 {
            return Err(format!(
                "Response too large ({} bytes, max {})",
                len, max_bytes
            ));
        }
    }
    let mut buf = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?
    {
        if buf.len().saturating_add(chunk.len()) > max_bytes {
            return Err(format!("Response exceeded max size of {} bytes", max_bytes));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

/// Cloud device type metadata (in index.json)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CloudDeviceType {
    pub device_type: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
}

/// Index file structure for cloud device types
#[derive(Debug, Deserialize)]
struct CloudDeviceTypesIndex {
    version: String,
    #[serde(default)]
    #[allow(dead_code)]
    last_updated: Option<String>,
    device_types: Vec<CloudDeviceType>,
}

/// Response for listing cloud device types
#[derive(Debug, Serialize)]
pub struct CloudDeviceTypesResponse {
    pub device_types: Vec<CloudDeviceType>,
    pub total: usize,
}

/// Request for importing selected device types
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct CloudImportRequest {
    pub device_types: Vec<String>,
    #[serde(default)]
    pub branch: Option<String>,
}

/// Details about a failed import
#[derive(Debug, Serialize)]
pub struct ImportFailure {
    pub device_type: String,
    pub reason: String,
}

/// Response for import operation
#[derive(Debug, Serialize)]
pub struct CloudImportResponse {
    pub imported: usize,
    pub skipped: usize,
    pub failed: usize,
    #[serde(default)]
    pub failures: Vec<ImportFailure>,
}

/// List available device types from cloud repository
/// Uses raw.githubusercontent.com to read index.json (avoids GitHub API rate limits)
///
/// GET /api/device-types/cloud/list
#[utoipa::path(
    get,
    path = "/api/device-types/cloud/list",
    tag = "device-types",
    responses(
        (status = 200, description = "Cloud device-type repository listing"),
    )
)]
pub async fn list_cloud_device_types_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    // Serve from 5-minute in-memory TTL cache to avoid outbound GitHub amplification
    {
        let cache = cloud_list_cache().read().await;
        if let Some(entry) = cache.as_ref() {
            if entry.fetched_at.elapsed() < CLOUD_LIST_CACHE_TTL {
                return ok(entry.payload.clone());
            }
        }
    }

    // Use raw.githubusercontent.com to avoid GitHub API rate limits
    let index_url = format!("{}/{}/types/index.json", CLOUD_BASE_URL, CLOUD_BRANCH);

    let client = cloud_http_client();

    let response = match client
        .get(&index_url)
        .timeout(Duration::from_secs(10))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Failed to connect to cloud repository: {}", e);
            // Return empty list on network error (graceful degradation)
            return ok(json!({
                "device_types": [],
                "total": 0,
                "error": "network_error",
                "message": "Unable to connect to cloud repository. Please check your internet connection."
            }));
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        tracing::error!("Cloud repository returned status {}", status);
        // Return empty list if fetch fails (graceful degradation)
        return ok(json!({
            "device_types": [],
            "total": 0,
            "error": format!("http_error_{}", status.as_u16()),
            "message": "Failed to fetch device types from cloud repository."
        }));
    }

    let body = match read_capped_bytes(response, MAX_CLOUD_RESPONSE_BYTES).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("Failed to read cloud index: {}", e);
            return ok(json!({
                "device_types": [],
                "total": 0,
                "error": "parse_error",
                "message": "Unable to read cloud repository response."
            }));
        }
    };

    let index: CloudDeviceTypesIndex = match serde_json::from_slice(&body) {
        Ok(i) => i,
        Err(e) => {
            tracing::error!("Failed to parse cloud index: {}", e);
            // Return empty list on parse error (graceful degradation)
            return ok(json!({
                "device_types": [],
                "total": 0,
                "error": "parse_error",
                "message": "Unable to parse cloud repository response."
            }));
        }
    };

    let payload = json!({
        "device_types": index.device_types,
        "total": index.device_types.len(),
        "index_version": index.version,
    });

    {
        let mut cache = cloud_list_cache().write().await;
        *cache = Some(CloudListCacheEntry {
            fetched_at: Instant::now(),
            payload: payload.clone(),
        });
    }

    ok(payload)
}

/// Result of fetching a single device type from cloud
#[derive(Debug)]
struct FetchedDeviceType {
    id: String,
    template: Option<DeviceTypeTemplate>,
    error: Option<String>,
}

/// Fetch a single device type from cloud
async fn fetch_device_type(
    client: &reqwest::Client,
    repo: &str,
    branch: &str,
    device_type_id: &str,
) -> FetchedDeviceType {
    let file_url = format!(
        "https://raw.githubusercontent.com/{}/{}/types/{}.json",
        repo, branch, device_type_id
    );

    let response = match client.get(&file_url).send().await {
        Ok(r) => r,
        Err(e) => {
            return FetchedDeviceType {
                id: device_type_id.to_string(),
                template: None,
                error: Some(format!("Network error: {}", e)),
            };
        }
    };

    if !response.status().is_success() {
        return FetchedDeviceType {
            id: device_type_id.to_string(),
            template: None,
            error: Some(format!("HTTP status: {}", response.status())),
        };
    }

    let bytes = match read_capped_bytes(response, MAX_CLOUD_RESPONSE_BYTES).await {
        Ok(b) => b,
        Err(e) => {
            return FetchedDeviceType {
                id: device_type_id.to_string(),
                template: None,
                error: Some(e),
            };
        }
    };

    let content = match String::from_utf8(bytes) {
        Ok(c) => c,
        Err(e) => {
            return FetchedDeviceType {
                id: device_type_id.to_string(),
                template: None,
                error: Some(format!("Invalid UTF-8 response: {}", e)),
            };
        }
    };

    match serde_json::from_str::<DeviceTypeTemplate>(&content) {
        Ok(template) => FetchedDeviceType {
            id: device_type_id.to_string(),
            template: Some(template),
            error: None,
        },
        Err(e) => {
            tracing::error!("Failed to parse device type '{}': {}", device_type_id, e);
            FetchedDeviceType {
                id: device_type_id.to_string(),
                template: None,
                error: Some(format!("Invalid JSON: {}", e)),
            }
        }
    }
}

/// Validate a `CloudImportRequest` before performing any outbound HTTP requests.
fn validate_cloud_import_request(request: &CloudImportRequest) -> Result<&str, ErrorResponse> {
    let branch = request.branch.as_deref().unwrap_or(CLOUD_BRANCH);
    if !is_valid_cloud_identifier(branch) {
        return Err(ErrorResponse::bad_request(
            "Invalid branch name: must be 1..=64 ASCII characters [A-Za-z0-9_-]",
        ));
    }
    if request.device_types.len() > MAX_CLOUD_IMPORT_ITEMS {
        return Err(ErrorResponse::bad_request(format!(
            "Too many device types requested: {} (max {})",
            request.device_types.len(),
            MAX_CLOUD_IMPORT_ITEMS
        )));
    }
    for id in &request.device_types {
        if !is_valid_cloud_identifier(id) {
            return Err(ErrorResponse::bad_request(format!(
                "Invalid device_type_id '{}': must be 1..=64 ASCII characters [A-Za-z0-9_-]",
                id
            )));
        }
    }
    Ok(branch)
}

/// Import selected device types from cloud
///
/// POST /api/device-types/cloud/import
#[utoipa::path(
    post,
    path = "/api/device-types/cloud/import",
    tag = "device-types",
    request_body = CloudImportRequest,
    responses(
        (status = 200, description = "Templates imported from the cloud repository"),
    )
)]
pub async fn import_cloud_device_types_handler(
    State(state): State<ServerState>,
    Json(request): Json<CloudImportRequest>,
) -> HandlerResult<CloudImportResponse> {
    let branch = validate_cloud_import_request(&request)?.to_string();
    let client = cloud_http_client();

    let mut imported = 0;
    let mut failed = 0;
    let mut failures = Vec::new();

    // Fetch device types with bounded concurrency
    let fetched: Vec<FetchedDeviceType> = futures::stream::iter(request.device_types)
        .map(|id| {
            let branch = branch.clone();
            async move { fetch_device_type(client, CLOUD_REPO, &branch, &id).await }
        })
        .buffer_unordered(MAX_CLOUD_FETCH_CONCURRENCY)
        .collect()
        .await;

    // Process fetched device types
    for result in fetched {
        let device_type_id = result.id;

        let template = match result.template {
            Some(t) => t,
            None => {
                failed += 1;
                if let Some(error) = result.error {
                    failures.push(ImportFailure {
                        device_type: device_type_id.clone(),
                        reason: error,
                    });
                }
                continue;
            }
        };

        // Check if already exists
        let exists = state
            .devices
            .service
            .get_template(&template.device_type)
            .is_some();

        // Save device_type ID before moving template
        let device_type_id_for_log = template.device_type.clone();

        // Register the device type (register_template is upsert - will overwrite if exists)
        match state.devices.service.register_template(template).await {
            Ok(()) => {
                imported += 1;
                if exists {
                    tracing::info!(
                        "Successfully updated device type from cloud: {}",
                        device_type_id_for_log
                    );
                } else {
                    tracing::info!(
                        "Successfully imported device type: {}",
                        device_type_id_for_log
                    );
                }
            }
            Err(e) => {
                failed += 1;
                failures.push(ImportFailure {
                    device_type: device_type_id_for_log,
                    reason: format!("Registration failed: {}", e),
                });
            }
        }
    }

    ok(CloudImportResponse {
        imported,
        skipped: 0, // No longer skipping - existing types are updated
        failed,
        failures,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_identifier_accepts_safe_names_and_rejects_traversal() {
        for valid in ["main", "ne101_camera", "release-v1_4", "A", &"a".repeat(64)] {
            assert!(
                is_valid_cloud_identifier(valid),
                "expected valid cloud identifier: {valid}"
            );
        }

        for invalid in [
            "",
            &"a".repeat(65),
            "../etc/passwd",
            "..",
            ".",
            "main/extra",
            "foo.json",
            "main?foo=bar",
            "main#frag",
            "foo%2fbar",
            "foo bar",
            "main\\sub",
        ] {
            assert!(
                !is_valid_cloud_identifier(invalid),
                "expected invalid cloud identifier to be rejected: {invalid:?}"
            );
        }
    }

    #[test]
    fn cloud_import_request_validation_rejects_traversal_and_oversized_batches() {
        let bad_branch = CloudImportRequest {
            device_types: vec!["ne101_camera".to_string()],
            branch: Some("../../other-org/repo".to_string()),
        };
        assert!(validate_cloud_import_request(&bad_branch).is_err());

        let bad_id = CloudImportRequest {
            device_types: vec!["../secret?token=1".to_string()],
            branch: Some("main".to_string()),
        };
        assert!(validate_cloud_import_request(&bad_id).is_err());

        let too_many = CloudImportRequest {
            device_types: (0..=MAX_CLOUD_IMPORT_ITEMS)
                .map(|i| format!("type_{i}"))
                .collect(),
            branch: Some("main".to_string()),
        };
        assert!(validate_cloud_import_request(&too_many).is_err());

        let valid = CloudImportRequest {
            device_types: vec!["ne101_camera".to_string(), "ne301-camera".to_string()],
            branch: Some("main".to_string()),
        };
        assert_eq!(validate_cloud_import_request(&valid).unwrap(), "main");
    }
}
