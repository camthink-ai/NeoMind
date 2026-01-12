//! Statistics API handlers.

use super::{ServerState, common::{HandlerResult, ok}};
use crate::models::ErrorResponse;
use axum::{extract::State, Json};
use serde_json::json;

// Re-export ConnectionStatus from mdl_format for DeviceInstance
use edge_ai_devices::mdl_format::ConnectionStatus as DeviceConnectionStatus;

/// System statistics summary.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SystemStats {
    /// Device statistics
    pub devices: DeviceStats,
    /// Rule statistics
    pub rules: RuleStats,
    /// Workflow statistics
    pub workflows: WorkflowStats,
    /// Alert statistics
    pub alerts: AlertStats,
    /// Command statistics
    pub commands: CommandStats,
    /// Decision statistics
    pub decisions: DecisionStats,
    /// System info
    pub system: SystemInfo,
}

/// Device statistics.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DeviceStats {
    /// Total devices
    pub total_devices: usize,
    /// Online devices
    pub online_devices: usize,
    /// Offline devices
    pub offline_devices: usize,
    /// Devices with metrics
    pub devices_with_metrics: usize,
}

/// Rule statistics.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RuleStats {
    /// Total rules
    pub total_rules: usize,
    /// Enabled rules
    pub enabled_rules: usize,
    /// Disabled rules
    pub disabled_rules: usize,
    /// Rules triggered today
    pub triggered_today: usize,
}

/// Workflow statistics.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WorkflowStats {
    /// Total workflows
    pub total_workflows: usize,
    /// Active workflows
    pub active_workflows: usize,
    /// Executions today
    pub executions_today: usize,
}

/// Alert statistics.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AlertStats {
    /// Total active alerts
    pub active_alerts: usize,
    /// Alerts by severity
    pub by_severity: serde_json::Value,
    /// Alerts today
    pub alerts_today: usize,
}

/// Command statistics.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CommandStats {
    /// Total commands
    pub total_commands: usize,
    /// Pending commands
    pub pending_commands: usize,
    /// Failed commands
    pub failed_commands: usize,
    /// Success rate
    pub success_rate: f32,
}

/// Decision statistics.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DecisionStats {
    /// Total decisions
    pub total_decisions: usize,
    /// Pending decisions
    pub pending_decisions: usize,
    /// Executed decisions
    pub executed_decisions: usize,
    /// Average confidence
    pub avg_confidence: f32,
}

/// System information.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SystemInfo {
    /// Uptime in seconds
    pub uptime_secs: i64,
    /// Server start time
    pub started_at: i64,
    /// Current time
    pub current_time: i64,
}

/// Get overall system statistics.
///
/// GET /api/stats/system
pub async fn get_system_stats_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    // Get device stats
    let devices = state.mqtt_device_manager.list_devices().await;
    let device_stats = DeviceStats {
        total_devices: devices.len(),
        online_devices: devices.iter().filter(|d| matches!(d.status, DeviceConnectionStatus::Online)).count(),
        offline_devices: devices.iter().filter(|d| !matches!(d.status, DeviceConnectionStatus::Online)).count(),
        devices_with_metrics: devices.iter().filter(|d| !d.current_values.is_empty()).count(),
    };

    // Get rule stats
    let rules = state.rule_engine.list_rules().await;
    let rule_stats = RuleStats {
        total_rules: rules.len(),
        enabled_rules: rules.iter().filter(|r| matches!(r.status, edge_ai_rules::RuleStatus::Active)).count(),
        disabled_rules: rules.iter().filter(|r| matches!(r.status, edge_ai_rules::RuleStatus::Disabled)).count(),
        triggered_today: 0, // TODO: Get from history
    };

    // Get workflow stats
    let workflow_stats = if let Some(engine) = state.workflow_engine.read().await.as_ref() {
        let workflows = engine.list_workflows().await.unwrap_or_default();
        WorkflowStats {
            total_workflows: workflows.len(),
            active_workflows: workflows.iter().filter(|w| w.enabled).count(),
            executions_today: 0, // TODO: Get from execution history
        }
    } else {
        WorkflowStats {
            total_workflows: 0,
            active_workflows: 0,
            executions_today: 0,
        }
    };

    // Get alert stats
    let all_alerts = state.alert_manager.list_alerts().await;
    let active_alerts: Vec<_> = all_alerts.into_iter()
        .filter(|a| matches!(a.status, edge_ai_alerts::AlertStatus::Active))
        .collect();
    let alert_stats = AlertStats {
        active_alerts: active_alerts.len(),
        by_severity: json!({
            "info": active_alerts.iter().filter(|a| matches!(a.severity, edge_ai_alerts::AlertSeverity::Info)).count(),
            "warning": active_alerts.iter().filter(|a| matches!(a.severity, edge_ai_alerts::AlertSeverity::Warning)).count(),
            "critical": active_alerts.iter().filter(|a| matches!(a.severity, edge_ai_alerts::AlertSeverity::Critical)).count(),
            "emergency": active_alerts.iter().filter(|a| matches!(a.severity, edge_ai_alerts::AlertSeverity::Emergency)).count(),
        }),
        alerts_today: 0, // TODO: Get from history
    };

    // Get command stats
    let command_stats = if let Some(manager) = &state.command_manager {
        let state_stats = manager.state.stats().await;
        CommandStats {
            total_commands: state_stats.total_count,
            pending_commands: state_stats.by_status.iter()
                .filter(|(s, _)| matches!(s, edge_ai_commands::command::CommandStatus::Pending | edge_ai_commands::command::CommandStatus::Queued))
                .map(|(_, c)| *c)
                .sum(),
            failed_commands: state_stats.by_status.iter()
                .filter(|(s, _)| matches!(s, edge_ai_commands::command::CommandStatus::Failed))
                .map(|(_, c)| *c)
                .sum(),
            success_rate: 0.0, // TODO: Calculate from history
        }
    } else {
        CommandStats {
            total_commands: 0,
            pending_commands: 0,
            failed_commands: 0,
            success_rate: 0.0,
        }
    };

    // Get decision stats
    let decision_stats = if let Some(store) = &state.decision_store {
        match store.stats().await {
            Ok(stats) => DecisionStats {
                total_decisions: stats.total_count,
                pending_decisions: stats.by_status.get("Proposed").copied().unwrap_or(0),
                executed_decisions: stats.by_status.get("Executed").copied().unwrap_or(0),
                avg_confidence: stats.avg_confidence,
            },
            Err(_) => DecisionStats {
                total_decisions: 0,
                pending_decisions: 0,
                executed_decisions: 0,
                avg_confidence: 0.0,
            },
        }
    } else {
        DecisionStats {
            total_decisions: 0,
            pending_decisions: 0,
            executed_decisions: 0,
            avg_confidence: 0.0,
        }
    };

    // System info
    let now = chrono::Utc::now().timestamp();
    let system_info = SystemInfo {
        uptime_secs: now - state.started_at,
        started_at: state.started_at,
        current_time: now,
    };

    let stats = SystemStats {
        devices: device_stats,
        rules: rule_stats,
        workflows: workflow_stats,
        alerts: alert_stats,
        commands: command_stats,
        decisions: decision_stats,
        system: system_info,
    };

    ok(json!({
        "stats": stats,
    }))
}

/// Get device-specific statistics.
///
/// GET /api/stats/devices
pub async fn get_device_stats_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let devices = state.mqtt_device_manager.list_devices().await;

    let devices_with_stats: Vec<serde_json::Value> = devices
        .into_iter()
        .map(|device| {
            let metrics_count = device.current_values.len();
            let is_online = matches!(device.status, DeviceConnectionStatus::Online);
            json!({
                "device_id": device.device_id,
                "device_type": device.device_type,
                "name": device.name.as_deref().unwrap_or(&device.device_id),
                "online": is_online,
                "metrics_count": metrics_count,
                "last_seen": device.last_seen.timestamp(),
            })
        })
        .collect();

    let online_count = devices_with_stats.iter()
        .filter(|d| d["online"].as_bool().unwrap_or(false))
        .count();

    ok(json!({
        "stats": {
            "total_devices": devices_with_stats.len(),
            "online_devices": online_count,
            "offline_devices": devices_with_stats.len() - online_count,
            "devices": devices_with_stats,
        }
    }))
}

/// Get rule-specific statistics.
///
/// GET /api/stats/rules
pub async fn get_rule_stats_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let rules = state.rule_engine.list_rules().await;

    let enabled_count = rules.iter().filter(|r| matches!(r.status, edge_ai_rules::RuleStatus::Active)).count();
    let disabled_count = rules.len() - enabled_count;

    // Group by type if available
    let by_type: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    ok(json!({
        "stats": {
            "total_rules": rules.len(),
            "enabled_rules": enabled_count,
            "disabled_rules": disabled_count,
            "by_type": by_type,
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_stats_serialization() {
        let stats = SystemStats {
            devices: DeviceStats {
                total_devices: 10,
                online_devices: 8,
                offline_devices: 2,
                devices_with_metrics: 7,
            },
            rules: RuleStats {
                total_rules: 5,
                enabled_rules: 3,
                disabled_rules: 2,
                triggered_today: 10,
            },
            workflows: WorkflowStats {
                total_workflows: 2,
                active_workflows: 1,
                executions_today: 5,
            },
            alerts: AlertStats {
                active_alerts: 1,
                by_severity: json!({"critical": 1}),
                alerts_today: 3,
            },
            commands: CommandStats {
                total_commands: 100,
                pending_commands: 5,
                failed_commands: 2,
                success_rate: 95.0,
            },
            decisions: DecisionStats {
                total_decisions: 20,
                pending_decisions: 3,
                executed_decisions: 15,
                avg_confidence: 0.85,
            },
            system: SystemInfo {
                uptime_secs: 3600,
                started_at: 1000000,
                current_time: 1004600,
            },
        };

        let json_str = serde_json::to_string(&stats).unwrap();
        assert!(json_str.contains("total_devices"));
        assert!(json_str.contains("total_rules"));
    }
}
