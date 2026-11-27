use crate::types::CliResponse;
use crate::ApiClient;
use anyhow::Result;
use serde_json::json;

/// Percent-encode a single path segment (dashboard IDs may be non-ASCII names).
fn encode_path_segment(seg: &str) -> String {
    let mut out = String::with_capacity(seg.len());
    for b in seg.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

/// Widget types that normally expect a live `data_source` binding.
fn expects_data_source(component_type: &str) -> bool {
    matches!(
        component_type,
        "value-card"
            | "metric-card"
            | "counter"
            | "led-indicator"
            | "sparkline"
            | "progress-bar"
            | "line-chart"
            | "area-chart"
            | "bar-chart"
            | "pie-chart"
            | "toggle-switch"
            | "image-display"
            | "image-history"
    )
}

/// Collect IDs of newly persisted data-bound widgets that still lack `data_source`.
fn unbound_component_ids(components: &[serde_json::Value]) -> Vec<String> {
    components
        .iter()
        .filter_map(|c| {
            let ct = c.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if !expects_data_source(ct) {
                return None;
            }
            let has_ds = c
                .get("data_source")
                .or_else(|| c.get("dataSource"))
                .is_some_and(|v| !v.is_null() && v.as_object().map(|o| !o.is_empty()).unwrap_or(true));
            if has_ds {
                return None;
            }
            let id = c.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            Some(format!("{id} ({ct})"))
        })
        .collect()
}

/// Parse flexible expiration strings (`"24"`, `"24h"`, `"7d"`, `"30m"`, `"never"`, `"0"`)
/// into `Option<i64>` hours expected by `POST /api/dashboards/:id/share`.
fn parse_expires_in_hours(raw: &str) -> Result<Option<i64>> {
    let s = raw.trim().to_ascii_lowercase();
    if s.is_empty() || s == "never" || s == "none" || s == "0" {
        return Ok(None);
    }
    if let Ok(hours) = s.parse::<i64>() {
        if hours < 0 {
            anyhow::bail!("--expires must be positive hours (e.g. 24, 24h, 7d), got '{raw}'");
        }
        return Ok(if hours == 0 { None } else { Some(hours) });
    }
    if let Some(num) = s.strip_suffix('h') {
        let h: i64 = num.trim().parse().map_err(|_| {
            anyhow::anyhow!("invalid --expires '{raw}': use hours like 24, 24h, or days like 7d")
        })?;
        if h <= 0 {
            anyhow::bail!("--expires must be > 0, got '{raw}'");
        }
        return Ok(Some(h));
    }
    if let Some(num) = s.strip_suffix('d') {
        let d: i64 = num.trim().parse().map_err(|_| {
            anyhow::anyhow!("invalid --expires '{raw}': use hours like 24, 24h, or days like 7d")
        })?;
        if d <= 0 {
            anyhow::bail!("--expires must be > 0, got '{raw}'");
        }
        return Ok(Some(d.saturating_mul(24)));
    }
    if let Some(num) = s.strip_suffix('m') {
        let m: i64 = num.trim().parse().map_err(|_| {
            anyhow::anyhow!("invalid --expires '{raw}': use hours like 24, 24h, or days like 7d")
        })?;
        if m <= 0 {
            anyhow::bail!("--expires must be > 0, got '{raw}'");
        }
        return Ok(Some(((m + 59) / 60).max(1)));
    }
    anyhow::bail!("invalid --expires '{raw}': expected integer hours (e.g. 24), 24h, 7d, or never")
}

/// Normalize `--components` JSON: wrap a single component object into a 1-element array.
fn normalize_components_array(val: serde_json::Value) -> serde_json::Value {
    if val.is_object() {
        serde_json::Value::Array(vec![val])
    } else {
        val
    }
}

/// Compact grid-occupancy summary computed from a dashboard's components —
/// lets the agent place new widgets without a follow-up get or blind guessing.
/// Returns (occupied_rows_desc, next_free_y). Grid is 12 columns.
fn grid_summary(components: &[serde_json::Value]) -> (String, u64) {
    let mut occupied: Vec<bool> = Vec::new();
    for c in components {
        let pos = c.get("position");
        let y = pos
            .and_then(|p| p.get("y"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let h = pos
            .and_then(|p| p.get("h"))
            .and_then(|v| v.as_u64())
            .unwrap_or(1)
            .max(1);
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
                out.push(if start == i - 1 {
                    format!("{}", start)
                } else {
                    format!("{}-{}", start, i - 1)
                });
            } else {
                i += 1;
            }
        }
        out
    };
    (
        if rows.is_empty() {
            "none".to_string()
        } else {
            rows.join(",")
        },
        occupied.len() as u64,
    )
}

/// Extract the components array from a GET / add-components API response.
/// Handles unwrapped payloads directly and falls back to legacy nested `data`.
fn response_components(data: &serde_json::Value) -> Vec<serde_json::Value> {
    data.get("components")
        .or_else(|| {
            data.get("data").and_then(|d| {
                d.get("components")
                    .or_else(|| d.get("data").and_then(|dd| dd.get("components")))
            })
        })
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
    let enc_id = encode_path_segment(id);
    let mut data = client.get(&format!("/dashboards/{}", enc_id)).await?;
    let comps = response_components(&data);
    let (occupied, next_free_y) = grid_summary(&comps);
    if let Some(obj) = data.as_object_mut() {
        obj.insert(
            "grid".to_string(),
            json!({
                "columns": 12,
                "unit": "grid_cells",
                "occupied_rows": occupied,
                "next_free_y": next_free_y,
                "component_count": comps.len(),
            }),
        );
    }
    Ok(CliResponse::success(
        data,
        format!(
            "Dashboard retrieved — {} component(s); see data.grid for layout facts",
            comps.len()
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
        body["components"] = normalize_components_array(components_value);
    }

    let data = client.post("/dashboards", &body).await?;
    let comps = response_components(&data);
    let unbound = unbound_component_ids(&comps);
    let msg = if unbound.is_empty() {
        "Dashboard created".to_string()
    } else {
        format!(
            "Dashboard created — WARNING: {} widget(s) have no data_source bound: {}. Bind live metrics with `neomind dashboard update-component <ID> --component-id <CID> --set '{{\"data_source\":{{\"type\":\"telemetry\",\"source\":\"device\",\"id\":\"<device_id>\",\"field\":\"<metric>\",\"sourceId\":\"<device_id>\",\"metricId\":\"<metric>\"}}}}'`",
            unbound.len(),
            unbound.join(", ")
        )
    };

    Ok(CliResponse::success(data, msg))
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
        body["components"] = normalize_components_array(components_value);
    }

    let enc_id = encode_path_segment(id);
    let data = client.put(&format!("/dashboards/{}", enc_id), &body).await?;
    Ok(CliResponse::success(data, "Dashboard updated"))
}

/// Delete dashboard
pub async fn delete_dashboard(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let enc_id = encode_path_segment(id);
    client.delete(&format!("/dashboards/{}", enc_id)).await?;
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
    let enc_id = encode_path_segment(id);
    let enc_cid = encode_path_segment(component_id);
    let data = client
        .patch(
            &format!("/dashboards/{}/components/{}", enc_id, enc_cid),
            &body,
        )
        .await?;
    let updated_id = data
        .get("component_id")
        .or_else(|| data.get("data").and_then(|d| d.get("component_id")))
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
        "components": normalize_components_array(components),
    });
    let enc_id = encode_path_segment(id);
    let data = client
        .post(&format!("/dashboards/{}/components", enc_id), &body)
        .await?;
    let comps = response_components(&data);
    let ids: Vec<String> = comps
        .iter()
        .filter_map(|c| c.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .collect();
    let (_occupied, next_free_y) = grid_summary(&comps);
    let unbound = unbound_component_ids(&comps);
    let mut msg = format!(
        "Components added (total: {}): {} — types verified, next free row: y={} (no re-fetch needed)",
        comps.len(),
        if ids.is_empty() { "?".to_string() } else { ids.join(", ") },
        next_free_y
    );
    if !unbound.is_empty() {
        msg.push_str(&format!(
            " | WARNING: {} widget(s) have no data_source bound: {} — bind with `update-component`",
            unbound.len(),
            unbound.join(", ")
        ));
    }
    Ok(CliResponse::success(data, msg))
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
    let enc_id = encode_path_segment(id);
    let data = client
        .delete_with_body(&format!("/dashboards/{}/components", enc_id), &body)
        .await?;
    let inner = if data.get("removed").is_some() || data.get("remaining").is_some() {
        &data
    } else {
        data.get("data").unwrap_or(&data)
    };
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
        if let Some(hours) = parse_expires_in_hours(exp)? {
            body["expires_in_hours"] = json!(hours);
        }
    }

    let enc_id = encode_path_segment(id);
    let data = client
        .post(&format!("/dashboards/{}/share", enc_id), &body)
        .await?;
    Ok(CliResponse::success(data, "Dashboard shared"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn grid_summary_reports_occupied_and_next_free_y() {
        let comps = vec![
            json!({"position":{"x":0,"y":0,"w":4,"h":2}}),
            json!({"position":{"x":4,"y":2,"w":4,"h":1}}),
        ];
        let (occupied, next) = grid_summary(&comps);
        assert_eq!(occupied, "0-2");
        assert_eq!(next, 3);
    }

    #[test]
    fn parse_expires_in_hours_handles_common_forms() {
        assert_eq!(parse_expires_in_hours("24").unwrap(), Some(24));
        assert_eq!(parse_expires_in_hours("24h").unwrap(), Some(24));
        assert_eq!(parse_expires_in_hours("7d").unwrap(), Some(168));
        assert_eq!(parse_expires_in_hours("30m").unwrap(), Some(1));
        assert_eq!(parse_expires_in_hours("never").unwrap(), None);
        assert_eq!(parse_expires_in_hours("0").unwrap(), None);
        assert!(parse_expires_in_hours("invalid").is_err());
        assert!(parse_expires_in_hours("-5").is_err());
    }

    #[test]
    fn unbound_component_ids_flags_only_missing_data_bound_widgets() {
        let comps = vec![
            json!({"id": "c1", "type": "value-card", "data_source": {"type": "device", "sourceId": "d1"}}),
            json!({"id": "c2", "type": "line-chart"}),
            json!({"id": "c3", "type": "markdown-display", "config": {"content": "hello"}}),
        ];
        assert_eq!(unbound_component_ids(&comps), vec!["c2 (line-chart)"]);
    }
}
