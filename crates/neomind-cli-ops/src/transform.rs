use crate::types::CliResponse;
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
                "scope": t.get("scope").cloned().unwrap_or(serde_json::Value::Null),
                "output_prefix": t.get("output_prefix").cloned().unwrap_or(serde_json::Value::Null),
                "enabled": t.get("enabled").cloned().unwrap_or(serde_json::Value::Null),
                "execution_count": t.get("execution_count").cloned().unwrap_or(serde_json::Value::Null),
                "last_executed": t.get("last_executed").cloned().unwrap_or(serde_json::Value::Null),
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
        .get("count")
        .or_else(|| data.get("data").and_then(|d| d.get("count")))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    Ok(CliResponse::success(
        data,
        format!("{} execution record(s)", count),
    ))
}

/// Parse scope string to JSON matching API's TransformScope serde format.
///
/// Accepted forms:
/// - `"global"` → `"global"`
/// - `"device_type:TH"` → `{"device_type": "TH"}`
/// - `"device:TH_8f072f7d"` → `{"device": "TH_8f072f7d"}`
///
/// Bare `"device"` / `"device_type"` (no id after `:`) are rejected — that
/// pattern burned an entire eval case wall-clock on Spark overnight
/// (NM-322 `sr-clear-transform[zh]`): the model kept mutating `--code`
/// while the opaque API deserialize error never named the bad scope.
fn parse_transform_scope(scope: &str) -> Result<serde_json::Value, String> {
    let s = scope.trim();
    if s.is_empty() {
        return Err(
            "Invalid --scope (empty). Use `global`, `device:<device_id>`, or `device_type:<type>`."
                .to_string(),
        );
    }
    if s.eq_ignore_ascii_case("global") {
        return Ok(json!("global"));
    }
    if let Some(rest) = s.strip_prefix("device_type:") {
        if rest.is_empty() {
            return Err(
                "Invalid --scope `device_type:` (missing type). Example: --scope device_type:ne101_camera."
                    .to_string(),
            );
        }
        return Ok(json!({ "device_type": rest }));
    }
    if let Some(rest) = s.strip_prefix("device:") {
        if rest.is_empty() {
            return Err(
                "Invalid --scope `device:` (missing device id). Example: --scope device:sensor-001."
                    .to_string(),
            );
        }
        return Ok(json!({ "device": rest }));
    }
    // Overnight wedge: `--scope device` / `--scope device_type` with no id.
    if s.eq_ignore_ascii_case("device") || s.eq_ignore_ascii_case("device_type") {
        return Err(format!(
            "Invalid --scope `{s}`. Bare `{s}` is not a scope — use `device:<device_id>` \
             (e.g. --scope device:sensor-001), `device_type:<type>`, or `global`. \
             Do not keep rewriting --code for this error; fix --scope first."
        ));
    }
    Err(format!(
        "Invalid --scope `{s}`. Expected `global`, `device:<device_id>`, or `device_type:<type>`."
    ))
}

fn invalid_scope_response(err: String) -> CliResponse {
    CliResponse::error_with_suggestion(
        err,
        "INVALID_SCOPE",
        "Fix --scope to one of: global | device:<id> | device_type:<type>. \
         Example: neomind transform create --name batt100 --scope device:sensor-001 \
         --code \"return input.battery * 100\" --output-prefix thousands",
    )
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
    let scope_json = match parse_transform_scope(scope) {
        Ok(v) => v,
        Err(e) => return Ok(invalid_scope_response(e)),
    };
    let definition = json!({
        "scope": scope_json,
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
        .get("automation")
        .or_else(|| data.get("data").and_then(|d| d.get("automation")))
        // The create response carries the id directly under `automation`
        // ({"automation":{"id":...}}); the metadata path was legacy. Prefer
        // both before giving up — "unknown" in the success hint sent users
        // (and models) to `transform executions unknown`.
        .and_then(|a| {
            a.get("id").and_then(|v| v.as_str()).or_else(|| {
                a.get("metadata")
                    .and_then(|m| m.get("id"))
                    .and_then(|v| v.as_str())
            })
        })
        .unwrap_or("unknown")
        .to_string();
    Ok(CliResponse::success(
        data,
        // Transforms only run when data flows — teach where to look instead
        // of leaving the workflow at "created, now what".
        format!(
            "Transform created. It runs on new data points; check results with: neomind transform executions {}",
            transform_id
        ),
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
        match parse_transform_scope(s) {
            Ok(v) => definition["scope"] = v,
            Err(e) => return Ok(invalid_scope_response(e)),
        }
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
    Ok(CliResponse::success(data, "Transform code tested"))
}

/// List transform data sources
pub async fn list_transform_data_sources(client: &ApiClient) -> Result<CliResponse> {
    let data = client.get("/automations/transforms/data-sources").await?;
    Ok(CliResponse::success(data, "Transform data sources listed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_scope_accepts_global_device_and_device_type() {
        assert_eq!(parse_transform_scope("global").unwrap(), json!("global"));
        assert_eq!(
            parse_transform_scope("device:sensor-001").unwrap(),
            json!({ "device": "sensor-001" })
        );
        assert_eq!(
            parse_transform_scope("device_type:ne101_camera").unwrap(),
            json!({ "device_type": "ne101_camera" })
        );
    }

    #[test]
    fn parse_scope_rejects_bare_device_overnight_wedge() {
        // Positive: overnight failure mode must be caught before API.
        let err = parse_transform_scope("device").unwrap_err();
        assert!(err.contains("Bare `device`"), "{err}");
        assert!(err.contains("device:sensor-001") || err.contains("device:<"), "{err}");

        let err2 = parse_transform_scope("device_type").unwrap_err();
        assert!(err2.contains("Bare `device_type`"), "{err2}");
    }

    #[test]
    fn parse_scope_rejects_empty_suffix_and_unknown() {
        assert!(parse_transform_scope("device:").unwrap_err().contains("missing"));
        assert!(parse_transform_scope("device_type:")
            .unwrap_err()
            .contains("missing"));
        assert!(parse_transform_scope("fleet").unwrap_err().contains("Expected"));
        assert!(parse_transform_scope("").unwrap_err().contains("empty"));
    }
}
