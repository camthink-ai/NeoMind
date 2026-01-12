//! Global search handlers.

use axum::{extract::{Query, State}, response::Json};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::ServerState;
use crate::models::{ErrorResponse, common::ApiResponse};

/// Search query parameters.
#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    /// Search query string
    pub q: String,
    /// Search targets (comma-separated: devices,sessions,rules,alerts,workflows)
    #[serde(default = "default_targets")]
    pub targets: String,
    /// Maximum results per target
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_targets() -> String { "all".to_string() }
fn default_limit() -> usize { 10 }

/// Search result item.
#[derive(Debug, Clone, Serialize)]
pub struct SearchResultItem {
    /// Result type (device, session, rule, alert, workflow)
    pub result_type: String,
    /// Unique identifier
    pub id: String,
    /// Display title
    pub title: String,
    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Relevance score (0.0 to 1.0)
    pub score: f32,
    /// Matched field highlights
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlights: Option<Vec<String>>,
}

/// Combined search results.
#[derive(Debug, Serialize)]
pub struct SearchResults {
    /// Original query
    pub query: String,
    /// Total results found
    pub total_count: usize,
    /// Results by type
    pub results: Vec<SearchResultItem>,
    /// Per-type counts
    pub counts: serde_json::Value,
}

/// Global search endpoint.
///
/// GET /api/search?q=term&targets=devices,sessions,rules&limit=10
pub async fn global_search_handler(
    State(state): State<ServerState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<ApiResponse<SearchResults>>, ErrorResponse> {
    let query_lower = query.q.to_lowercase();
    let mut results = Vec::new();
    let mut counts = serde_json::Map::new();

    let targets = if query.targets == "all" {
        vec!["devices", "sessions", "rules", "alerts", "workflows"]
    } else {
        query.targets.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect()
    };

    // Search devices
    if targets.contains(&"devices") {
        let device_results = search_devices(&state, &query_lower, query.limit).await;
        let count = device_results.len();
        if !device_results.is_empty() {
            results.extend(device_results);
        }
        counts.insert("devices".to_string(), json!(count));
    }

    // Search sessions
    if targets.contains(&"sessions") {
        let session_results = search_sessions(&state, &query_lower, query.limit).await;
        let count = session_results.len();
        if !session_results.is_empty() {
            results.extend(session_results);
        }
        counts.insert("sessions".to_string(), json!(count));
    }

    // Search rules
    if targets.contains(&"rules") {
        let rule_results = search_rules(&state, &query_lower, query.limit).await;
        let count = rule_results.len();
        if !rule_results.is_empty() {
            results.extend(rule_results);
        }
        counts.insert("rules".to_string(), json!(count));
    }

    // Search alerts
    if targets.contains(&"alerts") {
        let alert_results = search_alerts(&state, &query_lower, query.limit).await;
        let count = alert_results.len();
        if !alert_results.is_empty() {
            results.extend(alert_results);
        }
        counts.insert("alerts".to_string(), json!(count));
    }

    // Sort by score descending
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    let total_count = results.len();

    Ok(Json(ApiResponse::success(SearchResults {
        query: query.q,
        total_count,
        results,
        counts: json!(counts),
    })))
}

/// Search suggestions endpoint.
///
/// GET /api/search/suggestions?q=term
pub async fn search_suggestions_handler(
    State(state): State<ServerState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ErrorResponse> {
    let query = match params.get("q") {
        Some(q) if !q.is_empty() => q.to_lowercase(),
        _ => return Ok(Json(ApiResponse::success(json!({
            "suggestions": [],
        })))),
    };

    let mut suggestions = Vec::new();

    // Device name suggestions
    let devices = state.mqtt_device_manager.list_devices().await;
    for device in devices {
        if device.device_id.to_lowercase().contains(&query)
            || device.name.as_ref().map(|n| n.to_lowercase().contains(&query)).unwrap_or(false)
        {
            suggestions.push(json!({
                "type": "device",
                "id": device.device_id,
                "title": device.name.as_ref().unwrap_or(&device.device_id),
                "url": format!("/api/devices/{}", device.device_id),
            }));
        }
    }

    // Limit suggestions
    suggestions.truncate(10);

    Ok(Json(ApiResponse::success(json!({
        "suggestions": suggestions,
    }))))
}

async fn search_devices(
    state: &ServerState,
    query: &str,
    limit: usize,
) -> Vec<SearchResultItem> {
    let devices = state.mqtt_device_manager.list_devices().await;
    let mut results = Vec::new();

    for device in devices {
        if results.len() >= limit {
            break;
        }

        let device_id_lower = device.device_id.to_lowercase();
        let name_lower = device.name.as_ref().map(|n| n.to_lowercase()).unwrap_or_default();
        let device_type_lower = device.device_type.to_lowercase();

        let (matches, score) = if device_id_lower.contains(query) || name_lower.contains(query) {
            let score = if device_id_lower == query || name_lower == query {
                1.0
            } else if device_id_lower.starts_with(query) || name_lower.starts_with(query) {
                0.9
            } else {
                0.7
            };
            (true, score)
        } else if device_type_lower.contains(query) {
            (true, 0.5)
        } else {
            (false, 0.0)
        };

        if matches {
            results.push(SearchResultItem {
                result_type: "device".to_string(),
                id: device.device_id.clone(),
                title: device.name.clone().unwrap_or_else(|| device.device_id.clone()),
                description: Some(format!("Type: {}", device.device_type)),
                score,
                highlights: None,
            });
        }
    }

    results
}

async fn search_sessions(
    state: &ServerState,
    query: &str,
    limit: usize,
) -> Vec<SearchResultItem> {
    let sessions = state.session_manager.list_sessions().await;
    let mut results = Vec::new();

    for session_id in sessions.into_iter().take(limit) {
        let session_id_lower = session_id.to_lowercase();

        if session_id_lower.contains(query) {
            results.push(SearchResultItem {
                result_type: "session".to_string(),
                id: session_id.clone(),
                title: session_id.clone(),
                description: None,
                score: if session_id_lower == query { 1.0 } else { 0.7 },
                highlights: None,
            });
        }
    }

    results
}

async fn search_rules(
    state: &ServerState,
    query: &str,
    limit: usize,
) -> Vec<SearchResultItem> {
    let rules = state.rule_engine.list_rules().await;
    let mut results = Vec::new();

    for rule in rules.into_iter().take(limit) {
        let name_lower = rule.name.to_lowercase();
        let id_lower = rule.id.to_string().to_lowercase();

        let (matches, score) = if name_lower.contains(query) {
            let score = if name_lower == query { 1.0 }
            else if name_lower.starts_with(query) { 0.9 }
            else { 0.7 };
            (true, score)
        } else if id_lower.contains(query) {
            (true, 0.5)
        } else {
            (false, 0.0)
        };

        if matches {
            results.push(SearchResultItem {
                result_type: "rule".to_string(),
                id: rule.id.to_string(),
                title: rule.name,
                description: Some(format!("Status: {:?}", rule.status)),
                score,
                highlights: None,
            });
        }
    }

    results
}

async fn search_alerts(
    state: &ServerState,
    query: &str,
    limit: usize,
) -> Vec<SearchResultItem> {
    let alerts = state.alert_manager.list_alerts().await;
    let mut results = Vec::new();

    for alert in alerts.into_iter().take(limit) {
        let title_lower = alert.title.to_lowercase();
        let message_lower = alert.message.to_lowercase();

        let (matches, score) = if title_lower.contains(query) {
            let score = if title_lower == query { 1.0 }
            else if title_lower.starts_with(query) { 0.9 }
            else { 0.7 };
            (true, score)
        } else if message_lower.contains(query) {
            (true, 0.5)
        } else {
            (false, 0.0)
        };

        if matches {
            results.push(SearchResultItem {
                result_type: "alert".to_string(),
                id: alert.id.to_string(),
                title: alert.title,
                description: Some(alert.message),
                score,
                highlights: None,
            });
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_query_defaults() {
        let query = SearchQuery {
            q: "test".to_string(),
            targets: default_targets(),
            limit: default_limit(),
        };

        assert_eq!(query.q, "test");
        assert_eq!(query.targets, "all");
        assert_eq!(query.limit, 10);
    }

    #[test]
    fn test_search_result_item_serialization() {
        let item = SearchResultItem {
            result_type: "device".to_string(),
            id: "device-1".to_string(),
            title: "Test Device".to_string(),
            description: Some("A test device".to_string()),
            score: 0.9,
            highlights: None,
        };

        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"device\""));
        assert!(json.contains("\"Test Device\""));
    }
}
