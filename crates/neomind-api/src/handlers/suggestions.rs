//! Enhanced Suggestions API handlers.
//!
//! Provides intelligent, context-aware suggestions based on:
//! - Actual devices in the system
//! - Recent user operations
//! - Learned patterns from agents
//! - Time/context awareness

use axum::{
    extract::{Query, State},
    response::Json as ResponseJson,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use chrono::Timelike;
use futures::future;

use crate::server::ServerState;
use neomind_storage::AgentFilter;
use neomind_devices::adapter::ConnectionStatus;

/// Suggestion item with enhanced metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestionItem {
    pub id: String,
    pub label: String,
    pub prompt: String,
    pub icon: String,
    pub category: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,  // Higher = more relevant
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,  // Additional context (e.g., "3 devices")
}

/// Suggestions response with context
#[derive(Debug, Serialize)]
pub struct SuggestionsResponse {
    pub suggestions: Vec<SuggestionItem>,
    pub context: SuggestionContext,
}

/// Context information about the suggestions
#[derive(Debug, Serialize)]
pub struct SuggestionContext {
    pub timestamp: i64,
    pub learned_patterns_count: usize,
    pub device_count: usize,
    pub time_context: Option<String>,
}

/// Query parameters for suggestions
#[derive(Debug, Deserialize)]
pub struct SuggestionsQuery {
    pub input: Option<String>,
    pub category: Option<String>,
    pub limit: Option<usize>,
}

/// Generate intelligent, dynamic suggestions
pub async fn get_suggestions_handler(
    State(state): State<ServerState>,
    Query(params): Query<SuggestionsQuery>,
) -> ResponseJson<SuggestionsResponse> {
    let input = params.input.unwrap_or_default();
    let category = params.category;
    let limit = params.limit.unwrap_or(20);

    let mut suggestions = Vec::new();

    // 1. Get time-based context suggestions (highest priority)
    let time_context = get_time_context();
    let time_suggestions = generate_time_based_suggestions(&state, &time_context).await;
    suggestions.extend(time_suggestions);

    // 2. Get device-based suggestions
    let device_suggestions = generate_device_based_suggestions(&state).await;
    suggestions.extend(device_suggestions);

    // 3. Get recent operation suggestions from sessions
    let recent_suggestions = generate_recent_operation_suggestions(&state).await;
    suggestions.extend(recent_suggestions);

    // 4. Get learned patterns from agents
    let pattern_suggestions = generate_pattern_based_suggestions(&state).await;
    let learned_patterns_count = pattern_suggestions.len();
    suggestions.extend(pattern_suggestions);

    // 5. Add general system suggestions (lower priority)
    let general_suggestions = generate_system_suggestions();
    suggestions.extend(general_suggestions);

    // Get device count for context
    let device_count = get_device_count(&state).await;

    // Filter by category if specified
    if let Some(cat) = category {
        suggestions.retain(|s| s.category == cat);
    }

    // Filter by input if provided
    if !input.is_empty() {
        let input_lower = input.to_lowercase();
        suggestions.retain(|s| {
            s.label.to_lowercase().contains(&input_lower)
                || s.prompt.to_lowercase().contains(&input_lower)
        });
    }

    // Sort by priority (descending) and limit
    suggestions.sort_by(|a, b| b.priority.unwrap_or(0).cmp(&a.priority.unwrap_or(0)));
    suggestions.truncate(limit);

    let context = SuggestionContext {
        timestamp: chrono::Utc::now().timestamp(),
        learned_patterns_count,
        device_count,
        time_context: Some(time_context),
    };

    ResponseJson(SuggestionsResponse {
        suggestions,
        context,
    })
}

/// Get current time context (morning, afternoon, evening, etc.)
fn get_time_context() -> String {
    let now = chrono::Local::now();
    let hour = now.hour();

    match hour {
        5..=11 => "morning".to_string(),
        12..=17 => "afternoon".to_string(),
        18..=22 => "evening".to_string(),
        _ => "night".to_string(),
    }
}

/// Get device count from the system
async fn get_device_count(state: &ServerState) -> usize {
    let registry = state.devices.service.get_registry().await;
    registry.list_devices().await.len()
}

/// Generate time-based context-aware suggestions
async fn generate_time_based_suggestions(state: &ServerState, time_context: &str) -> Vec<SuggestionItem> {
    let mut suggestions = Vec::new();
    let device_count = get_device_count(state).await;

    match time_context {
        "morning" => {
            // Morning: suggest turning on devices, checking status
            if device_count > 0 {
                suggestions.push(SuggestionItem {
                    id: "time-morning-turn-on".to_string(),
                    label: format!("打开所有设备 ({}个设备)", device_count),
                    prompt: "打开所有设备".to_string(),
                    icon: "Zap".to_string(),
                    category: "automation".to_string(),
                    priority: Some(80),
                    context: Some(format!("当前有{}个设备", device_count)),
                });

                suggestions.push(SuggestionItem {
                    id: "time-morning-check-status".to_string(),
                    label: "检查设备状态".to_string(),
                    prompt: "所有设备状态如何".to_string(),
                    icon: "Cpu".to_string(),
                    category: "device".to_string(),
                    priority: Some(70),
                    context: Some("早上例行检查".to_string()),
                });
            }
        }
        "evening" => {
            // Evening: suggest turning off devices, security check
            if device_count > 0 {
                suggestions.push(SuggestionItem {
                    id: "time-evening-turn-off".to_string(),
                    label: "关闭所有设备".to_string(),
                    prompt: "关闭所有不必要的设备".to_string(),
                    icon: "Zap".to_string(),
                    category: "automation".to_string(),
                    priority: Some(80),
                    context: Some("节能模式".to_string()),
                });

                suggestions.push(SuggestionItem {
                    id: "time-evening-security".to_string(),
                    label: "安全检查".to_string(),
                    prompt: "检查所有门窗传感器状态".to_string(),
                    icon: "AlertTriangle".to_string(),
                    category: "alert".to_string(),
                    priority: Some(75),
                    context: Some("夜间安全".to_string()),
                });
            }
        }
        "afternoon" => {
            // Afternoon: analytics, monitoring
            suggestions.push(SuggestionItem {
                id: "time-afternoon-analytics".to_string(),
                label: "查看今日数据".to_string(),
                prompt: "显示今天的设备使用情况统计".to_string(),
                icon: "TrendingUp".to_string(),
                category: "analytics".to_string(),
                priority: Some(70),
                context: Some("数据统计".to_string()),
            });
        }
        _ => {}
    }

    suggestions
}

/// Generate device-based suggestions from actual devices
async fn generate_device_based_suggestions(state: &ServerState) -> Vec<SuggestionItem> {
    let mut suggestions = Vec::new();

    let registry = state.devices.service.get_registry().await;
    let devices = registry.list_devices().await;

    if devices.is_empty() {
        // No devices: suggest adding one
        return vec![
            SuggestionItem {
                id: "device-add-first".to_string(),
                label: "添加第一个设备".to_string(),
                prompt: "如何添加新设备".to_string(),
                icon: "Cpu".to_string(),
                category: "device".to_string(),
                priority: Some(90),
                context: Some("开始使用".to_string()),
            }
        ];
    }

    // Group devices by type
    let mut device_types: HashMap<String, usize> = HashMap::new();
    for device in &devices {
        let dtype = device.device_type.clone();
        *device_types.entry(dtype).or_insert(0) += 1;
    }

    // Generate suggestions for each device type
    for (dtype, count) in device_types {
        let icon = match dtype.as_str() {
            t if t.contains("sensor") || t.contains("temperature") || t.contains("humidity") => "Cpu",
            t if t.contains("switch") || t.contains("light") || t.contains("plug") => "Zap",
            t if t.contains("camera") => "Bot",
            _ => "Cpu",
        };

        suggestions.push(SuggestionItem {
            id: format!("device-list-{}", dtype),
            label: format!("查看所有{} ({}个)", dtype, count),
            prompt: format!("列出所有{}", dtype),
            icon: icon.to_string(),
            category: "device".to_string(),
            priority: Some(60),
            context: Some(format!("{}个设备", count)),
        });

        // Specialized suggestions per device type
        if dtype.contains("temperature") || dtype.contains("sensor") || dtype.contains("humidity") {
            suggestions.push(SuggestionItem {
                id: format!("device-check-{}", dtype),
                label: format!("检查{}读数", dtype),
                prompt: format!("显示所有{}的最新数据", dtype),
                icon: "TrendingUp".to_string(),
                category: "analytics".to_string(),
                priority: Some(55),
                context: None,
            });
        }
    }

    // Count online devices in parallel to avoid N+1 query problem
    // Use join_all for concurrent status checks instead of sequential loop
    let online_count = {
        let device_ids: Vec<_> = devices.iter()
            .map(|d| d.device_id.clone())
            .collect();

        let status_futures: Vec<_> = device_ids.into_iter()
            .map(|device_id| {
                let service = state.devices.service.clone();
                async move {
                    tokio::time::timeout(
                        std::time::Duration::from_millis(100),
                        service.get_device_connection_status(&device_id)
                    ).await.ok().and_then(|s| if matches!(s, ConnectionStatus::Connected) { Some(1) } else { None })
                }
            })
            .collect();

        let results = future::join_all(status_futures).await;
        results.into_iter().flatten().count()
    };

    if online_count < devices.len() && !devices.is_empty() {
        suggestions.push(SuggestionItem {
            id: "device-offline-check".to_string(),
            label: format!("检查离线设备 ({}个)", devices.len() - online_count),
            prompt: "哪些设备离线了".to_string(),
            icon: "AlertTriangle".to_string(),
            category: "device".to_string(),
            priority: Some(70),
            context: Some(format!("{}/{} 在线", online_count, devices.len())),
        });
    }

    suggestions
}

/// Generate suggestions based on recent user operations
async fn generate_recent_operation_suggestions(state: &ServerState) -> Vec<SuggestionItem> {
    let mut suggestions = Vec::new();

    // Get recent sessions with info
    let sessions = state.agents.session_manager.list_sessions_with_info().await;

    if sessions.is_empty() {
        // New user: suggest getting started
        return vec![
            SuggestionItem {
                id: "recent-welcome".to_string(),
                label: "新手入门指南".to_string(),
                prompt: "你能做什么".to_string(),
                icon: "Lightbulb".to_string(),
                category: "general".to_string(),
                priority: Some(85),
                context: Some("开始探索".to_string()),
            }
        ];
    }

    // Analyze recent session titles to find common patterns
    let mut operation_counts: HashMap<String, usize> = HashMap::new();

    for session_info in sessions.iter().take(10) {  // Check last 10 sessions
        if let Some(ref title) = session_info.title
            && !title.is_empty() {
                *operation_counts.entry(title.clone()).or_insert(0) += 1;
            }
    }

    // Generate suggestions from common operations (top 3)
    let mut ops: Vec<_> = operation_counts.into_iter().collect();
    ops.sort_by(|a, b| b.1.cmp(&a.1));

    for (operation, count) in ops.into_iter().take(3) {
        if count > 1 {  // Only suggest if done more than once
            suggestions.push(SuggestionItem {
                id: format!("recent-{}", suggestions.len()),
                label: format!("再次: {}", operation),
                prompt: operation.clone(),
                icon: "History".to_string(),
                category: "agent".to_string(),
                priority: Some(65),
                context: Some(format!("使用了{}次", count)),
            });
        }
    }

    suggestions
}

/// Generate suggestions based on learned patterns from agents
async fn generate_pattern_based_suggestions(state: &ServerState) -> Vec<SuggestionItem> {
    let mut pattern_suggestions = Vec::new();

    let store = &state.agents.agent_store;
    let agents = match store.query_agents(AgentFilter::default()).await {
        Ok(a) => a,
        Err(_) => return pattern_suggestions,
    };

    let mut high_confidence_patterns: Vec<(String, f32)> = Vec::new();

    for agent in agents {
        for pattern in &agent.memory.learned_patterns {
            if pattern.confidence > 0.7 {
                if let Some(action) = pattern.data.get("action")
                    && let Some(action_str) = action.as_str() {
                        high_confidence_patterns.push((
                            format!("类似: {}", action_str),
                            pattern.confidence,
                        ));
                    }
                if !pattern.description.is_empty() {
                    high_confidence_patterns.push((
                        pattern.description.clone(),
                        pattern.confidence,
                    ));
                }
            }
        }
    }

    high_confidence_patterns.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut seen = std::collections::HashSet::new();
    high_confidence_patterns.retain(|(desc, _)| seen.insert(desc.clone()));

    for (description, confidence) in high_confidence_patterns.into_iter().take(3) {
        pattern_suggestions.push(SuggestionItem {
            id: format!("pattern-{}", pattern_suggestions.len()),
            label: description.clone(),
            prompt: description.clone(),
            icon: "History".to_string(),
            category: "agent".to_string(),
            priority: Some((confidence * 100.0) as i32),
            context: Some(format!("置信度: {:.0}%", confidence * 100.0)),
        });
    }

    pattern_suggestions
}

/// Generate general system suggestions
fn generate_system_suggestions() -> Vec<SuggestionItem> {
    vec![
        SuggestionItem {
            id: "system-automation-list".to_string(),
            label: "查看自动化规则".to_string(),
            prompt: "显示所有自动化规则".to_string(),
            icon: "Zap".to_string(),
            category: "automation".to_string(),
            priority: Some(40),
            context: None,
        },
        SuggestionItem {
            id: "system-automation-create".to_string(),
            label: "创建自动化".to_string(),
            prompt: "创建新的自动化规则".to_string(),
            icon: "Zap".to_string(),
            category: "automation".to_string(),
            priority: Some(35),
            context: None,
        },
        SuggestionItem {
            id: "system-alert-list".to_string(),
            label: "查看告警".to_string(),
            prompt: "显示当前告警".to_string(),
            icon: "AlertTriangle".to_string(),
            category: "alert".to_string(),
            priority: Some(40),
            context: None,
        },
        SuggestionItem {
            id: "system-analytics".to_string(),
            label: "数据分析".to_string(),
            prompt: "分析最近的数据趋势".to_string(),
            icon: "TrendingUp".to_string(),
            category: "analytics".to_string(),
            priority: Some(30),
            context: None,
        },
        SuggestionItem {
            id: "system-agent-status".to_string(),
            label: "Agent状态".to_string(),
            prompt: "查看所有Agent运行状态".to_string(),
            icon: "Bot".to_string(),
            category: "agent".to_string(),
            priority: Some(35),
            context: None,
        },
        SuggestionItem {
            id: "system-help".to_string(),
            label: "帮助".to_string(),
            prompt: "你能做什么".to_string(),
            icon: "Lightbulb".to_string(),
            category: "general".to_string(),
            priority: Some(20),
            context: None,
        },
    ]
}

/// Get suggestions categories
pub async fn get_suggestions_categories_handler() -> ResponseJson<Vec<String>> {
    ResponseJson(vec![
        "device".to_string(),
        "automation".to_string(),
        "alert".to_string(),
        "analytics".to_string(),
        "agent".to_string(),
        "settings".to_string(),
        "general".to_string(),
    ])
}
