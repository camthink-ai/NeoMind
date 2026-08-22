use crate::types::{BuildMeta, CliResponse};
use crate::ApiClient;
use anyhow::Result;
use serde_json::json;

/// List all transforms with compact summary.
///
/// Returns id, name, scope, and output_prefix per transform.
/// Full JS code is available via `neomind transform get <id>`.
pub async fn list_transforms(client: &ApiClient) -> Result<CliResponse> {
    let data = client.get("/automations/transforms").await?;

    let transforms = extract_list_array(&data, "transforms");

    let Some(transforms) = transforms else {
        return Ok(CliResponse::success(data, "Transforms listed"));
    };

    let total = transforms.len();
    let summary: Vec<serde_json::Value> = transforms
        .iter()
        .map(|t| {
            json!({
                "id": t.get("id").and_then(|v| v.as_str()).unwrap_or("?"),
                "name": t.get("name").and_then(|v| v.as_str()).unwrap_or("(unnamed)"),
                "scope": t.get("scope"),
                "output_prefix": t.get("output_prefix").and_then(|v| v.as_str()).unwrap_or(""),
            })
        })
        .collect();

    Ok(CliResponse::success(
        json!({ "total": total, "transforms": summary }),
        format!("{} transform(s) listed", total),
    ))
}

/// Helper: extract an array from API response, trying common nesting patterns.
fn extract_list_array(data: &serde_json::Value, key: &str) -> Option<Vec<serde_json::Value>> {
    data.as_array()
        .cloned()
        .or_else(|| data.get(key).and_then(|v| v.as_array()).cloned())
        .or_else(|| data.get("data").and_then(|d| d.as_array()).cloned())
        .or_else(|| {
            data.get("data")
                .and_then(|d| d.get(key))
                .and_then(|v| v.as_array())
                .cloned()
        })
}

/// Get transform by ID
pub async fn get_transform(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client.get(&format!("/automations/{}", id)).await?;
    Ok(CliResponse::success(data, "Transform retrieved"))
}

/// Recent execution records of a transform (status / error / output summary).
pub async fn get_transform_executions(
    client: &ApiClient,
    id: &str,
    limit: Option<usize>,
) -> Result<CliResponse> {
    let lim = limit.unwrap_or(10);
    let data = client
        .get(&format!("/automations/{}/executions?limit={}", id, lim))
        .await?;
    let count = data
        .get("data")
        .and_then(|d| d.get("count"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    Ok(CliResponse::success(
        data,
        format!("{} execution record(s)", count),
    ))
}

/// Parse scope string to JSON matching API's TransformScope serde format
/// "global" → "global"
/// "device_type:TH" → {"device_type": "TH"}
/// "device:TH_8f072f7d" → {"device": "TH_8f072f7d"}
fn scope_to_json(scope: &str) -> serde_json::Value {
    if scope.starts_with("device_type:") {
        let parts: Vec<&str> = scope.splitn(2, ':').collect();
        json!({"device_type": parts[1]})
    } else if scope.starts_with("device:") {
        let parts: Vec<&str> = scope.splitn(2, ':').collect();
        json!({"device": parts[1]})
    } else {
        json!(scope)
    }
}

/// Create a new transform
pub async fn create_transform(
    client: &ApiClient,
    name: &str,
    scope: &str,
    js_code: &str,
    output_prefix: Option<&str>,
    description: Option<&str>,
    enabled: Option<bool>,
) -> Result<CliResponse> {
    let definition = json!({
        "scope": scope_to_json(scope),
        "js_code": js_code,
        "output_prefix": output_prefix.unwrap_or("transform"),
    });
    let body = json!({
        "name": name,
        "description": description.unwrap_or(""),
        "enabled": enabled.unwrap_or(true),
        "type": "transform",
        "definition": definition,
    });
    let data = client.post("/automations", &body).await?;
    let transform_id = data
        .get("data")
        .and_then(|d| d.get("automation"))
        .and_then(|a| a.get("metadata"))
        .and_then(|m| m.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let meta = BuildMeta {
        r#type: "transform".to_string(),
        action: "create".to_string(),
        entity_id: transform_id.to_string(),
        entity_name: Some(name.to_string()),
        undo_command: format!("neomind transform delete {}", transform_id),
    };
    Ok(CliResponse::success_with_meta(
        data,
        "Transform created",
        meta,
    ))
}

/// Update transform
#[allow(clippy::too_many_arguments)]
pub async fn update_transform(
    client: &ApiClient,
    id: &str,
    name: Option<&str>,
    description: Option<&str>,
    js_code: Option<&str>,
    scope: Option<&str>,
    output_prefix: Option<&str>,
    enabled: Option<bool>,
) -> Result<CliResponse> {
    let mut definition = json!({});
    if let Some(code) = js_code {
        definition["js_code"] = json!(code);
    }
    if let Some(s) = scope {
        definition["scope"] = scope_to_json(s);
    }
    if let Some(op) = output_prefix {
        definition["output_prefix"] = json!(op);
    }
    let mut body = json!({});
    if let Some(n) = name {
        body["name"] = json!(n);
    }
    if let Some(d) = description {
        body["description"] = json!(d);
    }
    if let Some(e) = enabled {
        body["enabled"] = json!(e);
    }
    if definition
        .as_object()
        .map(|o| !o.is_empty())
        .unwrap_or(false)
    {
        body["definition"] = definition;
    }
    let data = client.put(&format!("/automations/{}", id), &body).await?;
    Ok(CliResponse::success(data, "Transform updated"))
}

/// Delete transform
pub async fn delete_transform(client: &ApiClient, id: &str) -> Result<CliResponse> {
    client.delete(&format!("/automations/{}", id)).await?;
    Ok(CliResponse::success(
        json!({ "id": id }),
        "Transform deleted",
    ))
}

/// Enable a transform (unified form, equivalent to `update <id> --enabled true`).
pub async fn enable_transform(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let body = json!({ "enabled": true });
    let data = client.put(&format!("/automations/{}", id), &body).await?;
    Ok(CliResponse::success(data, "Transform enabled"))
}

/// Disable a transform (unified form, equivalent to `update <id> --enabled false`).
pub async fn disable_transform(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let body = json!({ "enabled": false });
    let data = client.put(&format!("/automations/{}", id), &body).await?;
    Ok(CliResponse::success(data, "Transform disabled"))
}

/// List virtual metrics from transforms
pub async fn list_virtual_metrics(client: &ApiClient) -> Result<CliResponse> {
    let data = client.get("/automations/transforms/metrics").await?;
    Ok(CliResponse::success(data, "Virtual metrics listed"))
}

/// Test transform code
pub async fn test_transform_code(
    client: &ApiClient,
    code: &str,
    input_data: serde_json::Value,
) -> Result<CliResponse> {
    let body = json!({
        "code": code,
        "input_data": input_data,
    });
    let data = client
        .post("/automations/transforms/test-code", &body)
        .await?;
    // Flatten: API returns {success:false, error:"..."} on execution failure
    // wrapped inside the outer success response — detect and surface as error
    if data.get("success").and_then(|v| v.as_bool()) == Some(false) {
        let error_msg = data
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Transform code execution failed");
        return Ok(CliResponse::error(error_msg, "TRANSFORM_TEST_FAILED"));
    }
    Ok(CliResponse::success(data, "Transform code tested"))
}

/// List transform data sources
pub async fn list_transform_data_sources(client: &ApiClient) -> Result<CliResponse> {
    let data = client.get("/automations/transforms/data-sources").await?;
    Ok(CliResponse::success(data, "Transform data sources listed"))
}
