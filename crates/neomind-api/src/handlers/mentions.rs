//! Server-side @-mention search + reference preflight (1.3 iter C).
//!
//! Plan §4.3: full accessible-object search with pagination, duplicate-name
//! disambiguation, and per-type partial failure. Client-side first-page
//! filtering is forbidden — this endpoint is the sole search source.

use axum::extract::{Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::handlers::chat_refs::{resolve_references, RefResolution};
use crate::handlers::common::{ok, HandlerResult};
use crate::handlers::ServerState;
use crate::models::error::ErrorResponse;
use crate::models::ChatReference;
const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 50;

#[derive(Debug, Deserialize)]
pub struct MentionSearchQuery {
    /// Substring over name or id (case-insensitive). Empty = browse first page.
    #[serde(default)]
    pub q: String,
    /// Comma-separated: device,rule,transform,agent. Empty/omit = all.
    #[serde(default)]
    pub types: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MentionItem {
    #[serde(rename = "type")]
    pub r#type: String,
    pub id: String,
    pub label: String,
    /// First 8 chars of id for duplicate-name rows.
    #[serde(rename = "shortId")]
    pub short_id: String,
    /// Type-specific disambiguation (device_type, enabled, status, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// True when another accessible hit shares this label (case-insensitive).
    #[serde(rename = "duplicateName")]
    pub duplicate_name: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct MentionTypeErrors {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transform: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dashboard: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MentionSearchResponse {
    pub items: Vec<MentionItem>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
    #[serde(rename = "hasMore")]
    pub has_more: bool,
    #[serde(rename = "nextOffset")]
    pub next_offset: Option<usize>,
    /// Per-type collection errors — other types still return.
    #[serde(rename = "typeErrors")]
    pub type_errors: MentionTypeErrors,
}

#[derive(Debug, Deserialize)]
pub struct MentionResolveRequest {
    pub references: Vec<ChatReference>,
}

#[derive(Debug, Serialize)]
pub struct MentionResolveResponse {
    pub resolutions: Vec<RefResolution>,
    /// True when every reference resolved successfully.
    pub ok: bool,
}

/// GET /api/mentions/search
pub async fn search_mentions_handler(
    State(state): State<ServerState>,
    Query(query): Query<MentionSearchQuery>,
) -> HandlerResult<MentionSearchResponse> {
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = query.offset.unwrap_or(0);
    let wanted = parse_types(query.types.as_deref());
    let q = query.q.trim().to_string();

    let mut candidates: Vec<(i32, MentionItem)> = Vec::new();
    let mut type_errors = MentionTypeErrors::default();

    if wanted.contains("device") {
        match collect_devices(&state, &q) {
            Ok(items) => candidates.extend(items),
            Err(e) => type_errors.device = Some(e),
        }
    }
    if wanted.contains("rule") {
        match collect_rules(&state, &q).await {
            Ok(items) => candidates.extend(items),
            Err(e) => type_errors.rule = Some(e),
        }
    }
    if wanted.contains("transform") {
        match collect_transforms(&state, &q).await {
            Ok(items) => candidates.extend(items),
            Err(e) => type_errors.transform = Some(e),
        }
    }
    if wanted.contains("agent") {
        match collect_agents(&state, &q).await {
            Ok(items) => candidates.extend(items),
            Err(e) => type_errors.agent = Some(e),
        }
    }
    if wanted.contains("dashboard") {
        match collect_dashboards(&state, &q) {
            Ok(items) => candidates.extend(items),
            Err(e) => type_errors.dashboard = Some(e),
        }
    }

    // Higher score first; then type order; then label.
    candidates.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| type_rank(&a.1.r#type).cmp(&type_rank(&b.1.r#type)))
            .then_with(|| a.1.label.to_lowercase().cmp(&b.1.label.to_lowercase()))
            .then_with(|| a.1.id.cmp(&b.1.id))
    });

    let mut items: Vec<MentionItem> = candidates.into_iter().map(|(_, item)| item).collect();
    mark_duplicate_names(&mut items);

    let total = items.len();
    let page: Vec<MentionItem> = items.into_iter().skip(offset).take(limit).collect();
    let has_more = offset + page.len() < total;
    let next_offset = if has_more {
        Some(offset + page.len())
    } else {
        None
    };

    ok(MentionSearchResponse {
        items: page,
        total,
        limit,
        offset,
        has_more,
        next_offset,
        type_errors,
    })
}

/// POST /api/mentions/resolve — preflight invalid/stale references before send.
pub async fn resolve_mentions_handler(
    State(state): State<ServerState>,
    Json(body): Json<MentionResolveRequest>,
) -> HandlerResult<MentionResolveResponse> {
    // Entry-point rejection (not silent bounding): a preflight over the cap
    // is a client bug — reject explicitly so it cannot masquerade as ok_all.
    if body.references.len() > crate::handlers::chat_refs::MAX_REFERENCES {
        return Err(ErrorResponse::bad_request(format!(
            "references exceed the {}-reference cap (got {})",
            crate::handlers::chat_refs::MAX_REFERENCES,
            body.references.len()
        )));
    }
    let resolutions = resolve_references(&state, &body.references).await;
    let ok_all = resolutions.iter().all(|r| r.ok);
    ok(MentionResolveResponse {
        resolutions,
        ok: ok_all,
    })
}

fn parse_types(raw: Option<&str>) -> std::collections::HashSet<&'static str> {
    let all = ["device", "rule", "transform", "agent", "dashboard"];
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return all.into_iter().collect();
    };
    if raw.eq_ignore_ascii_case("all") {
        return all.into_iter().collect();
    }
    let mut set = std::collections::HashSet::new();
    for part in raw.split(',') {
        match part.trim().to_ascii_lowercase().as_str() {
            "device" | "devices" => {
                set.insert("device");
            }
            "rule" | "rules" => {
                set.insert("rule");
            }
            "transform" | "transforms" => {
                set.insert("transform");
            }
            "agent" | "agents" => {
                set.insert("agent");
            }
            "dashboard" | "dashboards" => {
                set.insert("dashboard");
            }
            _ => {}
        }
    }
    if set.is_empty() {
        all.into_iter().collect()
    } else {
        set
    }
}

fn type_rank(t: &str) -> u8 {
    match t {
        "device" => 0,
        "rule" => 1,
        "transform" => 2,
        "agent" => 3,
        "dashboard" => 4,
        _ => 9,
    }
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

/// Match score: exact > prefix > substring. `None` = no match.
pub(crate) fn match_score(label: &str, id: &str, q: &str) -> Option<i32> {
    if q.is_empty() {
        return Some(0);
    }
    let ql = q.to_lowercase();
    let ll = label.to_lowercase();
    let il = id.to_lowercase();
    if ll == ql || il == ql {
        Some(300)
    } else if ll.starts_with(&ql) || il.starts_with(&ql) {
        Some(200)
    } else if ll.contains(&ql) || il.contains(&ql) {
        Some(100)
    } else {
        None
    }
}

fn mark_duplicate_names(items: &mut [MentionItem]) {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for item in items.iter() {
        *counts.entry(item.label.to_lowercase()).or_insert(0) += 1;
    }
    for item in items.iter_mut() {
        item.duplicate_name = counts.get(&item.label.to_lowercase()).copied().unwrap_or(0) > 1;
    }
}

fn collect_devices(state: &ServerState, q: &str) -> Result<Vec<(i32, MentionItem)>, String> {
    let configs = state.devices.service.list_devices();
    let mut out = Vec::new();
    for config in configs {
        let label = if config.name.is_empty() {
            config.device_id.clone()
        } else {
            config.name.clone()
        };
        let Some(score) = match_score(&label, &config.device_id, q) else {
            continue;
        };
        out.push((
            score,
            MentionItem {
                r#type: "device".into(),
                id: config.device_id.clone(),
                label,
                short_id: short_id(&config.device_id),
                hint: Some(format!("{} · {}", config.device_type, config.adapter_type)),
                duplicate_name: false,
            },
        ));
    }
    Ok(out)
}

async fn collect_rules(state: &ServerState, q: &str) -> Result<Vec<(i32, MentionItem)>, String> {
    let rules = state.automation.rule_engine.list_rules().await;
    let mut out = Vec::new();
    for rule in rules {
        let id = rule.id.to_string();
        let label = if rule.name.is_empty() {
            id.clone()
        } else {
            rule.name.clone()
        };
        let Some(score) = match_score(&label, &id, q) else {
            continue;
        };
        out.push((
            score,
            MentionItem {
                r#type: "rule".into(),
                id: id.clone(),
                label,
                short_id: short_id(&id),
                hint: Some(if rule.enabled {
                    "enabled".into()
                } else {
                    "disabled".into()
                }),
                duplicate_name: false,
            },
        ));
    }
    Ok(out)
}

async fn collect_transforms(
    state: &ServerState,
    q: &str,
) -> Result<Vec<(i32, MentionItem)>, String> {
    let Some(store) = state.automation.automation_store.as_ref() else {
        return Err("automation store unavailable".into());
    };
    let transforms = store
        .list_automations()
        .await
        .map_err(|e| format!("list failed: {e}"))?;
    let mut out = Vec::new();
    for t in transforms {
        let id = t.metadata.id.clone();
        let label = if t.metadata.name.is_empty() {
            id.clone()
        } else {
            t.metadata.name.clone()
        };
        let Some(score) = match_score(&label, &id, q) else {
            continue;
        };
        let enabled = if t.metadata.enabled {
            "enabled"
        } else {
            "disabled"
        };
        out.push((
            score,
            MentionItem {
                r#type: "transform".into(),
                id: id.clone(),
                label,
                short_id: short_id(&id),
                hint: Some(format!("{} · {}", enabled, t.scope.as_str())),
                duplicate_name: false,
            },
        ));
    }
    Ok(out)
}

async fn collect_agents(state: &ServerState, q: &str) -> Result<Vec<(i32, MentionItem)>, String> {
    // V2-only: list via HostedAiAgentRuntime (no AiAgent query_agents).
    let Ok(rt) = state.require_clean_slate().await else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for (spec, _plan) in rt.list() {
        let label = if spec.name.is_empty() {
            spec.id.clone()
        } else {
            spec.name.clone()
        };
        let Some(score) = match_score(&label, &spec.id, q) else {
            continue;
        };
        let status = rt
            .runtime_status(&spec.id)
            .map(|s| format!("{s:?}").to_lowercase())
            .unwrap_or_else(|| "unknown".into());
        out.push((
            score,
            MentionItem {
                r#type: "agent".into(),
                id: spec.id.clone(),
                label,
                short_id: short_id(&spec.id),
                hint: Some(status),
                duplicate_name: false,
            },
        ));
    }
    Ok(out)
}

fn collect_dashboards(state: &ServerState, q: &str) -> Result<Vec<(i32, MentionItem)>, String> {
    let dashboards = state
        .dashboard_store
        .list_all()
        .map_err(|e| format!("list failed: {e}"))?;
    let mut out = Vec::new();
    for d in dashboards {
        let label = if d.name.trim().is_empty() {
            d.id.clone()
        } else {
            d.name.clone()
        };
        let Some(score) = match_score(&label, &d.id, q) else {
            continue;
        };
        out.push((
            score,
            MentionItem {
                r#type: "dashboard".into(),
                id: d.id.clone(),
                label,
                short_id: short_id(&d.id),
                hint: Some(format!("{} components", d.components.len())),
                duplicate_name: false,
            },
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_score_ranks_exact_prefix_substring() {
        assert_eq!(
            match_score("温湿度传感器", "dev-1", "温湿度传感器"),
            Some(300)
        );
        assert_eq!(match_score("温湿度传感器", "dev-1", "温湿"), Some(200));
        assert_eq!(match_score("机房温湿度传感器", "dev-1", "温湿"), Some(100));
        assert_eq!(match_score("其他", "dev-1", "温湿"), None);
        assert_eq!(match_score("x", "ABC-9", "abc"), Some(200));
        assert_eq!(match_score("any", "id", ""), Some(0));
    }

    #[test]
    fn parse_types_defaults_and_filters() {
        assert_eq!(parse_types(None).len(), 5);
        assert_eq!(parse_types(Some("")).len(), 5);
        assert_eq!(parse_types(Some("device,agent")).len(), 2);
        assert!(parse_types(Some("rules")).contains("rule"));
        assert!(parse_types(Some("dashboards")).contains("dashboard"));
        assert_eq!(parse_types(Some("nope")).len(), 5);
    }

    #[test]
    fn mark_duplicate_names_flags_case_insensitive() {
        let mut items = vec![
            MentionItem {
                r#type: "device".into(),
                id: "a".into(),
                label: "Sensor".into(),
                short_id: "a".into(),
                hint: None,
                duplicate_name: false,
            },
            MentionItem {
                r#type: "device".into(),
                id: "b".into(),
                label: "sensor".into(),
                short_id: "b".into(),
                hint: None,
                duplicate_name: false,
            },
            MentionItem {
                r#type: "rule".into(),
                id: "c".into(),
                label: "Other".into(),
                short_id: "c".into(),
                hint: None,
                duplicate_name: false,
            },
        ];
        mark_duplicate_names(&mut items);
        assert!(items[0].duplicate_name);
        assert!(items[1].duplicate_name);
        assert!(!items[2].duplicate_name);
    }
}
