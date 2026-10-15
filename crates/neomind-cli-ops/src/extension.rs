use crate::types::CliResponse;
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

    Ok(CliResponse::success(data, "Extension installed"))
}

/// Install extension from marketplace
pub async fn install_extension_market(
    client: &ApiClient,
    extension_id: &str,
    version: Option<&str>,
) -> Result<CliResponse> {
    // Field is `id` — the handler's MarketplaceInstallRequest. This sent
    // `extension_id` since inception, so every CLI market-install failed
    // with "missing field `id`" before the request reached any install
    // logic (found 2026-10-15 verifying the chat-driven install path).
    let mut body = json!({
        "id": extension_id,
    });
    if let Some(v) = version {
        body["version"] = json!(v);
    }

    let data = client.post("/extensions/market/install", &body).await?;

    Ok(CliResponse::success(
        data,
        "Extension installed from marketplace",
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
pub async fn list_marketplace(
    client: &ApiClient,
    filter: Option<&str>,
    verbose: bool,
) -> Result<CliResponse> {
    let data = client.get("/extensions/market/list").await?;
    if verbose {
        return Ok(CliResponse::success(data, "Marketplace extensions listed"));
    }
    // Compact by default: the raw registry is ~48 KB (26 extensions × full
    // metadata + URLs) and drowned small models' context when the agent ran
    // it — twice in one turn (2026-10-15). One line per extension is what
    // both a human and a 2B model actually need to pick an id to install;
    // `-v` keeps the full dump one flag away.
    let compact = compact_market_list(&data, filter);
    Ok(CliResponse::success(
        compact,
        "Marketplace extensions listed (compact; -v for full metadata)",
    ))
}

/// Project the raw market registry into {total, matched, extensions:[{id,
/// name, version, description}]}, optionally filtered by a case-insensitive
/// substring over id/name/description. Pure — unit-tested.
pub fn compact_market_list(raw: &serde_json::Value, filter: Option<&str>) -> serde_json::Value {
    // The registry nests: {success, data:{extensions:[...]}} — unwrap both.
    let list = raw
        .pointer("/data/extensions")
        .or_else(|| raw.pointer("/extensions"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let needle = filter.map(str::to_lowercase);
    let mut out = Vec::new();
    for e in &list {
        let desc = e
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or_default()
            .to_string();
        let entry = json!({
            "id": e.get("id").cloned().unwrap_or(serde_json::Value::Null),
            "name": e.get("name").cloned().unwrap_or(serde_json::Value::Null),
            "version": e.get("version").cloned().unwrap_or(serde_json::Value::Null),
            "description": desc,
        });
        if let Some(n) = &needle {
            let hay = format!(
                "{} {} {}",
                entry["id"].as_str().unwrap_or_default(),
                entry["name"].as_str().unwrap_or_default(),
                entry["description"].as_str().unwrap_or_default(),
            )
            .to_lowercase();
            if !hay.contains(n) {
                continue;
            }
        }
        out.push(entry);
    }
    json!({
        "total": list.len(),
        "matched": out.len(),
        "extensions": out,
    })
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

#[cfg(test)]
mod market_list_tests {
    use super::compact_market_list;
    use serde_json::json;

    fn raw() -> serde_json::Value {
        json!({
            "success": true,
            "data": { "extensions": [
                { "id": "weather-forecast", "name": "Weather Forecast", "version": "2.14.0",
                  "description": "Real-time weather", "metadata_url": "https://long.url/..." },
                { "id": "yolo-video", "name": "YOLO Video", "version": "2.0.0",
                  "description": "Object detection", "metadata_url": "https://long.url/..." },
            ]}
        })
    }

    #[test]
    fn compact_keeps_one_line_fields_only() {
        let out = compact_market_list(&raw(), None);
        assert_eq!(out["total"], 2);
        assert_eq!(out["matched"], 2);
        let e = &out["extensions"][0];
        assert_eq!(e["id"], "weather-forecast");
        assert_eq!(e["version"], "2.14.0");
        assert!(e.get("metadata_url").is_none(), "URLs and bulk must be dropped");
    }

    #[test]
    fn filter_matches_case_insensitively_across_fields() {
        // By id fragment, by name word, by description word.
        assert_eq!(compact_market_list(&raw(), Some("weather".into()))["matched"], 1);
        assert_eq!(compact_market_list(&raw(), Some("Forecast".into()))["matched"], 1);
        assert_eq!(compact_market_list(&raw(), Some("detection".into()))["matched"], 1);
        assert_eq!(compact_market_list(&raw(), Some("nomatch".into()))["matched"], 0);
    }

    #[test]
    fn tolerates_flat_shape_and_missing_fields() {
        let flat = json!({ "extensions": [ { "id": "x" } ] });
        let out = compact_market_list(&flat, None);
        assert_eq!(out["total"], 1);
        assert_eq!(out["extensions"][0]["id"], "x");
    }
}
