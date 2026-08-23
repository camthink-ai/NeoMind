use crate::types::{BuildMeta, CliResponse};
use crate::ApiClient;
use anyhow::Result;
use serde_json::json;


/// Compact grid-occupancy summary computed from a dashboard's components —
/// lets the agent place new widgets without a follow-up get or blind guessing.
/// Returns (occupied_rows_desc, next_free_y). Grid is 12 columns.
fn grid_summary(components: &[serde_json::Value]) -> (String, u64) {
    let mut occupied: Vec<bool> = Vec::new();
    for c in components {
        let pos = c.get("position");
        let y = pos.and_then(|p| p.get("y")).and_then(|v| v.as_u64()).unwrap_or(0);
        let h = pos.and_then(|p| p.get("h")).and_then(|v| v.as_u64()).unwrap_or(1).max(1);
        let bottom = y + h;
        if occupied.len() < bottom as usize {
            occupied.resize(bottom as usize, false);
        }
        for row in occupied.iter_mut().take(bottom as usize).skip(y as usize) {
            *row = true;
        }
    }
    let rows: Vec<String> = {
        let mut out = Vec::new();
        let mut i = 0;
        while i < occupied.len() {
            if occupied[i] {
                let start = i;
                while i < occupied.len() && occupied[i] {
                    i += 1;
                }
                out.push(if start == i - 1 { format!("{}", start) } else { format!("{}-{}", start, i - 1) });
            } else {
                i += 1;
            }
        }
        out
    };
    (
        if rows.is_empty() { "none".to_string() } else { rows.join(",") },
        occupied.len() as u64,
    )
}

/// Extract the components array from a GET / add-components API response
/// (handles the double-nested `data.data` the API wraps).
fn response_components(data: &serde_json::Value) -> Vec<serde_json::Value> {
    data.get("data")
        .and_then(|d| {
            d.get("components")
                .or_else(|| d.get("data").and_then(|dd| dd.get("components")))
        })
        .or_else(|| data.get("components"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}

/// List all dashboards with compact summary.
///
/// Returns only id, name, and component count per dashboard — NOT the full
/// component tree.  Full data is available via `neomind dashboard get <id>`.
/// This avoids returning 1MB+ JSON that gets truncated before the LLM can
/// count the dashboards.
pub async fn list_dashboards(client: &ApiClient) -> Result<CliResponse> {
    let data = client.get("/dashboards").await?;

    // Extract the dashboard array from nested API response
    let dashboards = data
        .get("dashboards")
        .or_else(|| data.get("data").and_then(|d| d.get("dashboards")))
        .and_then(|v| v.as_array());

    let Some(dashboards) = dashboards else {
        // Fallback: return raw data if structure is unexpected
        return Ok(CliResponse::success(data, "Dashboards listed"));
    };

    let total = dashboards.len();
    let summary: Vec<serde_json::Value> = dashboards
        .iter()
        .map(|d| {
            let id = d.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            let name = d
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("(unnamed)");
            let comp_count = d
                .get("components")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            // Collect component type distribution
            let type_counts: std::collections::BTreeMap<String, usize> = d
                .get("components")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    let mut map = std::collections::BTreeMap::new();
                    for c in arr {
                        if let Some(ct) = c.get("type").and_then(|v| v.as_str()) {
                            *map.entry(ct.to_string()).or_insert(0) += 1;
                        }
                    }
                    map
                })
                .unwrap_or_default();
            let component_types: Vec<serde_json::Value> = type_counts
                .iter()
                .map(|(t, c)| json!({ "type": t, "count": c }))
                .collect();

            json!({
                "id": id,
                "name": name,
                "components": comp_count,
                "component_types": component_types,
            })
        })
        .collect();

    Ok(CliResponse::success(
        json!({
            "total": total,
            "dashboards": summary,
        }),
        format!("{} dashboard(s) listed", total),
    ))
}

/// Get dashboard by ID (or name — server resolves both).
pub async fn get_dashboard(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client.get(&format!("/dashboards/{}", id)).await?;
    let comps = response_components(&data);
    let (occupied, next_free_y) = grid_summary(&comps);
    Ok(CliResponse::success(
        data,
        format!(
            "Dashboard retrieved — {} components, 12-col grid, occupied rows: {}, next free row: y={} (place new widgets at y={} to avoid overlap)",
            comps.len(),
            occupied,
            next_free_y,
            next_free_y
        ),
    ))
}

/// Create a new dashboard
pub async fn create_dashboard(
    client: &ApiClient,
    name: &str,
    description: Option<&str>,
    layout: Option<serde_json::Value>,
    components: Option<serde_json::Value>,
) -> Result<CliResponse> {
    let mut body = json!({
        "name": name,
    });
    if let Some(desc) = description {
        body["description"] = json!(desc);
    }
    if let Some(layout_value) = layout {
        body["layout"] = layout_value;
    }
    if let Some(components_value) = components {
        body["components"] = components_value;
    }

    let data = client.post("/dashboards", &body).await?;
    let dashboard_id = data
        .get("data")
        .and_then(|d| d.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let meta = BuildMeta {
        r#type: "dashboard".to_string(),
        action: "create".to_string(),
        entity_id: dashboard_id.clone(),
        entity_name: Some(name.to_string()),
        undo_command: format!("neomind dashboard delete {}", dashboard_id),
    };

    Ok(CliResponse::success_with_meta(
        data,
        "Dashboard created",
        meta,
    ))
}

/// Update dashboard
pub async fn update_dashboard(
    client: &ApiClient,
    id: &str,
    name: Option<&str>,
    description: Option<&str>,
    layout: Option<serde_json::Value>,
    components: Option<serde_json::Value>,
) -> Result<CliResponse> {
    let mut body = json!({});
    if let Some(n) = name {
        body["name"] = json!(n);
    }
    if let Some(desc) = description {
        body["description"] = json!(desc);
    }
    if let Some(layout_value) = layout {
        body["layout"] = layout_value;
    }
    if let Some(components_value) = components {
        body["components"] = components_value;
    }

    let data = client.put(&format!("/dashboards/{}", id), &body).await?;
    Ok(CliResponse::success(data, "Dashboard updated"))
}

/// Delete dashboard
pub async fn delete_dashboard(client: &ApiClient, id: &str) -> Result<CliResponse> {
    client.delete(&format!("/dashboards/{}", id)).await?;
    Ok(CliResponse::success(
        json!({ "id": id }),
        "Dashboard deleted",
    ))
}

/// Update ONE component of a dashboard (deep-merge patch)
pub async fn update_component(
    client: &ApiClient,
    id: &str,
    component_id: &str,
    set: serde_json::Value,
) -> Result<CliResponse> {
    let body = json!({ "set": set });
    let data = client
        .patch(&format!("/dashboards/{}/components/{}", id, component_id), &body)
        .await?;
    let updated_id = data
        .get("data")
        .and_then(|d| d.get("component_id"))
        .and_then(|v| v.as_str())
        .unwrap_or(component_id)
        .to_string();
    Ok(CliResponse::success(
        data,
        format!("Component '{}' updated", updated_id),
    ))
}

/// Add components to a dashboard (append mode)
pub async fn add_components(
    client: &ApiClient,
    id: &str,
    components: serde_json::Value,
) -> Result<CliResponse> {
    let body = json!({
        "components": components,
    });
    let data = client
        .post(&format!("/dashboards/{}/components", id), &body)
        .await?;
    let comps = response_components(&data);
    let ids: Vec<String> = comps
        .iter()
        .filter_map(|c| c.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .collect();
    let (_occupied, next_free_y) = grid_summary(&comps);
    Ok(CliResponse::success(
        data,
        format!(
            "Components added (total: {}): {} — types verified, next free row: y={} (no re-fetch needed)",
            comps.len(),
            if ids.is_empty() { "?".to_string() } else { ids.join(", ") },
            next_free_y
        ),
    ))
}

/// Remove components from a dashboard by ID
pub async fn remove_components(
    client: &ApiClient,
    id: &str,
    ids: serde_json::Value,
) -> Result<CliResponse> {
    let body = json!({
        "ids": ids,
    });
    let data = client
        .delete_with_body(&format!("/dashboards/{}/components", id), &body)
        .await?;
    let inner = data.get("data").unwrap_or(&data);
    let removed = inner["removed"].as_u64().unwrap_or(0);
    let remaining = inner["remaining"].as_u64().unwrap_or(0);
    Ok(CliResponse::success(
        data,
        format!("Removed {} component(s), {} remaining", removed, remaining),
    ))
}

/// Share dashboard
pub async fn share_dashboard(
    client: &ApiClient,
    id: &str,
    public: bool,
    expires: Option<&str>,
) -> Result<CliResponse> {
    let mut body = json!({
        "permissions": {"allow_interactive": public},
    });
    if let Some(exp) = expires {
        body["expires_in_hours"] = json!(exp);
    }

    let data = client
        .post(&format!("/dashboards/{}/share", id), &body)
        .await?;
    Ok(CliResponse::success(data, "Dashboard shared"))
}
