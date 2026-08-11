//! Remote Instance Management API Handlers
//!
//! REST API endpoints for managing remote NeoMind backend instances.

use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;
use serde_json::json;

use super::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::ErrorResponse;
use neomind_storage::InstanceRecord;

/// Request to create a remote instance
#[derive(Debug, Deserialize)]
pub struct CreateInstanceRequest {
    /// Display name
    pub name: String,
    /// Backend URL (e.g., "http://192.168.1.50:9375")
    pub url: String,
    /// API key for the remote instance
    pub api_key: Option<String>,
}

/// Request to update a remote instance
#[derive(Debug, Deserialize)]
pub struct UpdateInstanceRequest {
    /// Display name
    pub name: Option<String>,
    /// Backend URL
    pub url: Option<String>,
    /// API key
    pub api_key: Option<String>,
}

/// List all instances (API keys masked, encrypted keys included)
pub async fn list_instances_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let instances: Vec<_> = state
        .instance_store
        .load_all()?
        .iter()
        .map(|i| i.for_response())
        .collect();

    ok(json!({
        "instances": instances,
    }))
}

/// Get a single instance by ID (API key masked, encrypted key included)
pub async fn get_instance_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let instance = state
        .instance_store
        .load_instance(&id)?
        .ok_or_else(|| ErrorResponse::not_found(format!("Instance {}", id)))?;

    ok(json!(instance.for_response()))
}

/// Create a new remote instance
pub async fn create_instance_handler(
    State(state): State<ServerState>,
    Json(req): Json<CreateInstanceRequest>,
) -> HandlerResult<serde_json::Value> {
    let instance = InstanceRecord::new(req.name, req.url, req.api_key);
    instance.validate().map_err(ErrorResponse::bad_request)?;

    let id = instance.id.clone();
    state.instance_store.save_instance(&instance)?;

    // Load back to get consistent state
    let saved = state
        .instance_store
        .load_instance(&id)?
        .ok_or_else(|| ErrorResponse::internal("Failed to load saved instance"))?;

    ok(json!(saved.for_response()))
}

/// Update an existing instance
pub async fn update_instance_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateInstanceRequest>,
) -> HandlerResult<serde_json::Value> {
    let mut instance = state
        .instance_store
        .load_instance(&id)?
        .ok_or_else(|| ErrorResponse::not_found(format!("Instance {}", id)))?;

    if let Some(name) = req.name {
        instance.name = name;
    }
    if let Some(url) = req.url {
        instance.url = url;
    }
    if let Some(api_key) = req.api_key {
        if api_key.is_empty() {
            instance.api_key = None;
        } else {
            instance.api_key = Some(api_key);
        }
    }

    instance.validate().map_err(ErrorResponse::bad_request)?;

    state.instance_store.save_instance(&instance)?;

    ok(json!(instance.for_response()))
}

/// Delete an instance
pub async fn delete_instance_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    state.instance_store.delete_instance(&id).map_err(|e| {
        if e.to_string().contains("Cannot delete") {
            ErrorResponse::bad_request(e.to_string())
        } else {
            ErrorResponse::from(e)
        }
    })?;

    ok(json!({"deleted": true}))
}

/// Test connectivity to a remote instance (health check proxy)
pub async fn test_instance_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let instance = state
        .instance_store
        .load_instance(&id)?
        .ok_or_else(|| ErrorResponse::not_found(format!("Instance {}", id)))?;

    let health_url = format!("{}/api/health", instance.url.trim_end_matches('/'));

    // SSRF protection: reject loopback / link-local / cloud-metadata targets.
    // Without this, an authenticated user can probe internal services.
    let url_lower = instance.url.to_lowercase();
    const SSRF_TARGETS: &[&str] = &[
        "//localhost",
        "//127.0.0.1",
        "//[::1]",
        "//0.0.0.0",
        "169.254.169.254", // AWS/GCP/Azure cloud metadata
    ];
    for target in SSRF_TARGETS {
        if url_lower.contains(target) {
            return Err(ErrorResponse::bad_request(format!(
                "Instance URL targets a blocked address ({}). \
                 Use the local API for localhost instances.",
                target
            )));
        }
    }

    let start = std::time::Instant::now();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ErrorResponse::internal(format!("HTTP client error: {}", e)))?;

    let mut request = client.get(&health_url);
    if let Some(ref api_key) = instance.api_key {
        request = request.header("X-API-Key", api_key);
    }

    let result = request.send().await;
    let latency_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(resp) if resp.status().is_success() => {
            let _ = state.instance_store.update_status(&id, "online");
            ok(json!({
                "success": true,
                "latency_ms": latency_ms,
                "status": "online",
            }))
        }
        Ok(resp) => {
            let status = resp.status().as_u16();
            let _ = state.instance_store.update_status(&id, "offline");
            ok(json!({
                "success": false,
                "latency_ms": latency_ms,
                "status": "offline",
                "error": format!("HTTP {}", status),
            }))
        }
        Err(e) => {
            let _ = state.instance_store.update_status(&id, "offline");
            ok(json!({
                "success": false,
                "latency_ms": latency_ms,
                "status": "offline",
                "error": e.to_string(),
            }))
        }
    }
}
