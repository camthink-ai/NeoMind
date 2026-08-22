use crate::types::{BuildMeta, CliResponse};
use crate::ApiClient;
use anyhow::Result;
use serde_json::json;

/// List configured LLM backends
pub async fn list_backends(client: &ApiClient) -> Result<CliResponse> {
    let mut data = client.get("/llm-backends").await?;

    // Add capability tags to each backend for easy scanning
    if let Some(backends) = data.get_mut("backends").and_then(|b| b.as_array_mut()) {
        for backend in backends.iter_mut() {
            let mut tags: Vec<String> = Vec::new();

            if backend
                .get("is_active")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                tags.push("active".to_string());
            }
            if let Some(caps) = backend.get("capabilities") {
                if caps
                    .get("multimodal")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    tags.push("multimodal".to_string());
                }
                if caps
                    .get("supports_images")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    tags.push("vision".to_string());
                }
                if caps
                    .get("function_calling")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    tags.push("tool-use".to_string());
                }
                if caps
                    .get("thinking_display")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    tags.push("thinking".to_string());
                }
                if caps
                    .get("streaming")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    tags.push("streaming".to_string());
                }
                if let Some(ctx) = caps.get("max_context").and_then(|v| v.as_u64()) {
                    if ctx > 0 {
                        tags.push(format!("ctx:{:.0}k", ctx as f64 / 1024.0));
                    }
                }
            }
            if let Some(healthy) = backend.get("healthy").and_then(|v| v.as_bool()) {
                if !healthy {
                    tags.push("unhealthy".to_string());
                }
            }

            backend["_tags"] = json!(tags);
        }
    }

    Ok(CliResponse::success(data, "LLM backends listed"))
}

/// Get LLM backend by ID
pub async fn get_backend(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client.get(&format!("/llm-backends/{}", id)).await?;
    Ok(CliResponse::success(data, "LLM backend retrieved"))
}

/// List available models from Ollama
///
/// `endpoint` overrides the Ollama server to query (the API defaults to
/// http://localhost:11434 when absent) — the skill teaches
/// `llm models --endpoint http://gpu-host:11434` for remote servers.
pub async fn list_ollama_models(client: &ApiClient, endpoint: Option<&str>) -> Result<CliResponse> {
    let path = match endpoint.map(str::trim).filter(|e| !e.is_empty()) {
        Some(e) => format!("/llm-backends/ollama/models?endpoint={}", urlencode(e)),
        None => "/llm-backends/ollama/models".to_string(),
    };
    let data = client.get(&path).await?;
    Ok(CliResponse::success(data, "Ollama models listed"))
}

/// Percent-encode a query value (keep URL-safe characters as-is).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Create a new LLM backend
pub async fn create_backend(
    client: &ApiClient,
    name: &str,
    backend_type: &str,
    endpoint: &str,
    model: &str,
    api_key: Option<&str>,
    temperature: Option<f64>,
) -> Result<CliResponse> {
    let mut body = json!({
        "name": name,
        "backend_type": backend_type,
        "endpoint": endpoint,
        "model": model,
    });
    if let Some(k) = api_key {
        body["api_key"] = json!(k);
    }
    if let Some(t) = temperature {
        body["temperature"] = json!(t);
    }

    let data = client.post("/llm-backends", &body).await?;
    let backend_id = data
        .get("data")
        .and_then(|d| d.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let meta = BuildMeta {
        r#type: "llm_backend".to_string(),
        action: "create".to_string(),
        entity_id: backend_id.clone(),
        entity_name: Some(name.to_string()),
        undo_command: format!("neomind llm delete {}", backend_id),
    };

    Ok(CliResponse::success_with_meta(
        data,
        "LLM backend created",
        meta,
    ))
}

/// Update an existing LLM backend
pub async fn update_backend(
    client: &ApiClient,
    id: &str,
    name: Option<&str>,
    model: Option<&str>,
    endpoint: Option<&str>,
    api_key: Option<&str>,
    temperature: Option<f64>,
) -> Result<CliResponse> {
    let mut body = json!({});
    if let Some(n) = name {
        body["name"] = json!(n);
    }
    if let Some(m) = model {
        body["model"] = json!(m);
    }
    if let Some(e) = endpoint {
        body["endpoint"] = json!(e);
    }
    if let Some(k) = api_key {
        body["api_key"] = json!(k);
    }
    if let Some(t) = temperature {
        body["temperature"] = json!(t);
    }

    let data = client.put(&format!("/llm-backends/{}", id), &body).await?;
    Ok(CliResponse::success(data, "LLM backend updated"))
}

/// Delete an LLM backend
pub async fn delete_backend(client: &ApiClient, id: &str) -> Result<CliResponse> {
    client.delete(&format!("/llm-backends/{}", id)).await?;
    Ok(CliResponse::success(
        json!({ "id": id }),
        "LLM backend deleted",
    ))
}

/// Activate an LLM backend (set as default)
pub async fn activate_backend(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let body = json!({});
    let data = client
        .post(&format!("/llm-backends/{}/activate", id), &body)
        .await?;
    Ok(CliResponse::success(data, "LLM backend activated"))
}

/// Test connection to an LLM backend
pub async fn test_backend(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let body = json!({});
    let data = client
        .post(&format!("/llm-backends/{}/test", id), &body)
        .await?;
    Ok(CliResponse::success(data, "LLM backend test completed"))
}
