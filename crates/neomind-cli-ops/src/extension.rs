use crate::types::{BuildMeta, CliResponse};
use crate::ApiClient;
use anyhow::Result;
use serde_json::json;

/// List all extensions with compact summary.
///
/// Returns id, name, version, status, and enabled flag per extension.
/// Full config is available via `neomind extension get <id>`.
pub async fn list_extensions(client: &ApiClient) -> Result<CliResponse> {
    let data = client.get("/extensions").await?;

    let extensions = data
        .as_array()
        .or_else(|| data.get("extensions").and_then(|v| v.as_array()))
        .or_else(|| {
            data.get("data").and_then(|d| d.as_array()).or_else(|| {
                data.get("data")
                    .and_then(|d| d.get("extensions"))
                    .and_then(|v| v.as_array())
            })
        });

    let Some(extensions) = extensions else {
        return Ok(CliResponse::success(data, "Extensions listed"));
    };

    let total = extensions.len();
    let summary: Vec<serde_json::Value> = extensions
        .iter()
        .map(|e| {
            json!({
                "id": e.get("id").and_then(|v| v.as_str()).unwrap_or("?"),
                "name": e.get("name").and_then(|v| v.as_str()).unwrap_or("(unnamed)"),
                "version": e.get("version").and_then(|v| v.as_str()).unwrap_or("?"),
                "status": e.get("status").and_then(|v| v.as_str()).unwrap_or("unknown"),
                "enabled": e.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
                "description": e.get("description").and_then(|v| v.as_str()).unwrap_or(""),
            })
        })
        .collect();

    Ok(CliResponse::success(
        json!({ "total": total, "extensions": summary }),
        format!("{} extension(s) listed", total),
    ))
}

/// Percent-encode a URL path segment. A raw `/` inside an extension ID
/// splits the request path into extra segments, which escapes the
/// `/extensions/{id}` route entirely — the web SPA fallback then answers
/// 200 with HTML, and the API client reads that as success.
fn encode_segment(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    for b in id.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Get extension by ID
pub async fn get_extension(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client
        .get(&format!("/extensions/{}", encode_segment(id)))
        .await?;
    Ok(CliResponse::success(data, "Extension retrieved"))
}

/// Get extension health status
pub async fn get_extension_status(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let resp = client
        .get(&format!("/extensions/{}/health", encode_segment(id)))
        .await?;
    // Extract inner data to avoid double-wrapping (API already has success/data structure)
    let data = resp.get("data").cloned().unwrap_or(resp);
    Ok(CliResponse::success(data, "Extension status retrieved"))
}

/// Get extension logs
pub async fn get_extension_logs(
    client: &ApiClient,
    id: &str,
    lines: Option<usize>,
) -> Result<CliResponse> {
    let path = if let Some(n) = lines {
        format!("/extensions/{}/logs?lines={}", encode_segment(id), n)
    } else {
        format!("/extensions/{}/logs", encode_segment(id))
    };
    let data = client.get(&path).await?;
    Ok(CliResponse::success(data, "Extension logs retrieved"))
}

/// Install extension from file
pub async fn install_extension_file(client: &ApiClient, file_path: &str) -> Result<CliResponse> {
    // Extension upload API expects JSON body with base64-encoded file data
    use std::io::Read;
    let mut file = std::fs::File::open(file_path)?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &buf);
    let filename = std::path::Path::new(file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("extension.nep")
        .to_string();

    let body = json!({
        "data": b64,
        "filename": filename,
    });
    let data = client.post("/extensions/upload/file", &body).await?;
    let ext_id = data
        .get("data")
        .and_then(|d| d.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let meta = BuildMeta {
        r#type: "extension".to_string(),
        action: "install".to_string(),
        entity_id: ext_id.clone(),
        entity_name: ext_id.clone().into(),
        undo_command: format!("neomind extension uninstall {}", ext_id),
    };

    Ok(CliResponse::success_with_meta(
        data,
        "Extension installed",
        meta,
    ))
}

/// Install extension from marketplace
pub async fn install_extension_market(
    client: &ApiClient,
    extension_id: &str,
    version: Option<&str>,
) -> Result<CliResponse> {
    let mut body = json!({
        "extension_id": extension_id,
    });
    if let Some(v) = version {
        body["version"] = json!(v);
    }

    let data = client.post("/extensions/market/install", &body).await?;
    let ext_id = data
        .get("data")
        .and_then(|d| d.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or(extension_id)
        .to_string();

    let meta = BuildMeta {
        r#type: "extension".to_string(),
        action: "install".to_string(),
        entity_id: ext_id.clone(),
        entity_name: ext_id.clone().into(),
        undo_command: format!("neomind extension uninstall {}", ext_id),
    };

    Ok(CliResponse::success_with_meta(
        data,
        "Extension installed from marketplace",
        meta,
    ))
}

/// Uninstall extension
pub async fn uninstall_extension(client: &ApiClient, id: &str) -> Result<CliResponse> {
    client
        .delete(&format!("/extensions/{}/uninstall", id))
        .await?;
    Ok(CliResponse::success(
        json!({ "id": id }),
        "Extension uninstalled",
    ))
}

/// Reload an extension (restart from file)
pub async fn reload_extension(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client
        .post_raw(&format!("/extensions/{}/reload", id))
        .await?;
    Ok(CliResponse::success(data, "Extension reloaded"))
}

/// List marketplace extensions
pub async fn list_marketplace(client: &ApiClient) -> Result<CliResponse> {
    let data = client.get("/extensions/market/list").await?;
    Ok(CliResponse::success(data, "Marketplace extensions listed"))
}

/// Get extension configuration
pub async fn get_extension_config(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client.get(&format!("/extensions/{}/config", id)).await?;
    Ok(CliResponse::success(data, "Extension config retrieved"))
}

/// Update extension configuration
pub async fn update_extension_config(
    client: &ApiClient,
    id: &str,
    config: serde_json::Value,
) -> Result<CliResponse> {
    let data = client
        .put(&format!("/extensions/{}/config", id), &config)
        .await?;
    Ok(CliResponse::success(data, "Extension config updated"))
}
