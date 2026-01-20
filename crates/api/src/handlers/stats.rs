//! Statistics API handlers.

use super::{
    ServerState,
    common::{HandlerResult, ok},
};
use axum::extract::State;
use serde_json::json;

// Re-export ConnectionStatus from mdl_format for DeviceInstance

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

/// GPU information.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GpuInfo {
    /// GPU name
    pub name: String,
    /// GPU vendor (nvidia, amd, intel, apple, other)
    pub vendor: String,
    /// Total memory in MB (if available)
    pub total_memory_mb: Option<u64>,
    /// Driver version (if available)
    pub driver_version: Option<String>,
}

/// System information.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SystemInfo {
    /// Version string
    pub version: String,
    /// Uptime in seconds
    pub uptime: i64,
    /// Platform (e.g., "linux", "darwin", "windows")
    pub platform: String,
    /// Architecture (e.g., "x86_64", "aarch64")
    pub arch: String,
    /// CPU core count
    pub cpu_count: usize,
    /// Total memory in bytes
    pub total_memory: u64,
    /// Used memory in bytes
    pub used_memory: u64,
    /// Free memory in bytes
    pub free_memory: u64,
    /// Available memory in bytes
    pub available_memory: u64,
    /// GPU information (if detected)
    pub gpus: Vec<GpuInfo>,
}

/// Get overall system statistics.
///
/// GET /api/stats/system
pub async fn get_system_stats_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    // Calculate start of today (UTC midnight)
    let now = chrono::Utc::now();
    let start_of_today = now.date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp();

    // Get device stats using DeviceService
    let configs = state.device_service.list_devices().await;
    // Get current metrics to count devices with metrics
    let mut devices_with_metrics = 0;
    for config in &configs {
        if let Ok(metrics) = state
            .device_service
            .get_current_metrics(&config.device_id)
            .await
            && !metrics.is_empty() {
                devices_with_metrics += 1;
            }
    }

    // Get online/offline device counts from connection status tracking
    use edge_ai_devices::adapter::ConnectionStatus;
    let online_devices = state.device_service.get_devices_by_status(ConnectionStatus::Connected).await.len();
    let offline_devices = state.device_service.get_devices_by_status(ConnectionStatus::Disconnected).await.len();

    let device_stats = DeviceStats {
        total_devices: configs.len(),
        online_devices,
        offline_devices,
        devices_with_metrics,
    };

    // Get rule stats
    let rules = state.rule_engine.list_rules().await;
    let triggered_today = if let Some(store) = &state.rule_history_store {
        store.count_since(start_of_today).unwrap_or(0) as usize
    } else {
        0
    };
    let rule_stats = RuleStats {
        total_rules: rules.len(),
        enabled_rules: rules
            .iter()
            .filter(|r| matches!(r.status, edge_ai_rules::RuleStatus::Active))
            .count(),
        disabled_rules: rules
            .iter()
            .filter(|r| matches!(r.status, edge_ai_rules::RuleStatus::Disabled))
            .count(),
        triggered_today,
    };

    // Get workflow stats
    let workflow_stats = if let Some(engine) = state.workflow_engine.read().await.as_ref() {
        let workflows = engine.list_workflows().await.unwrap_or_default();
        let executions_today = if let Some(store) = &state.workflow_history_store {
            store.count_since(start_of_today).unwrap_or(0) as usize
        } else {
            0
        };
        WorkflowStats {
            total_workflows: workflows.len(),
            active_workflows: workflows.iter().filter(|w| w.enabled).count(),
            executions_today,
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
    let active_alerts: Vec<_> = all_alerts
        .into_iter()
        .filter(|a| matches!(a.status, edge_ai_alerts::AlertStatus::Active))
        .collect();

    // Get today's alerts count from history store
    let alerts_today = if let Some(store) = &state.alert_store {
        store.count_since(start_of_today).unwrap_or(0) as usize
    } else {
        0
    };

    let alert_stats = AlertStats {
        active_alerts: active_alerts.len(),
        by_severity: json!({
            "info": active_alerts.iter().filter(|a| matches!(a.severity, edge_ai_alerts::AlertSeverity::Info)).count(),
            "warning": active_alerts.iter().filter(|a| matches!(a.severity, edge_ai_alerts::AlertSeverity::Warning)).count(),
            "critical": active_alerts.iter().filter(|a| matches!(a.severity, edge_ai_alerts::AlertSeverity::Critical)).count(),
            "emergency": active_alerts.iter().filter(|a| matches!(a.severity, edge_ai_alerts::AlertSeverity::Emergency)).count(),
        }),
        alerts_today,
    };

    // Get command stats
    let command_stats = if let Some(manager) = &state.command_manager {
        let state_stats = manager.state.stats().await;
        let total_commands = state_stats.total_count;
        let failed_commands = state_stats
            .by_status
            .iter()
            .filter(|(s, _)| matches!(s, edge_ai_commands::command::CommandStatus::Failed))
            .map(|(_, c)| *c)
            .sum();
        let success_rate = if total_commands > 0 {
            ((total_commands - failed_commands) as f32 / total_commands as f32 * 100.0)
        } else {
            0.0
        };
        CommandStats {
            total_commands,
            pending_commands: state_stats
                .by_status
                .iter()
                .filter(|(s, _)| {
                    matches!(
                        s,
                        edge_ai_commands::command::CommandStatus::Pending
                            | edge_ai_commands::command::CommandStatus::Queued
                    )
                })
                .map(|(_, c)| *c)
                .sum(),
            failed_commands,
            success_rate,
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
    let uptime = now - state.started_at;

    // Get system platform info
    let platform = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();

    // Get CPU count
    let cpu_count = std::thread::available_parallelism()
        .map(|c| c.get())
        .unwrap_or(1);

    // Get memory info using sysinfo crate
    let (total_memory, used_memory, free_memory, available_memory) = {
        let mut sys = sysinfo::System::new();
        sys.refresh_memory();
        (
            sys.total_memory(),
            sys.used_memory(),
            sys.free_memory(),
            sys.available_memory(),
        )
    };

    // Detect GPUs
    let gpus = detect_gpus();

    // Version from env or default
    let version = env!("CARGO_PKG_VERSION");

    let system_info = SystemInfo {
        version: version.to_string(),
        uptime,
        platform,
        arch,
        cpu_count,
        total_memory,
        used_memory,
        free_memory,
        available_memory,
        gpus,
    };

    let stats = SystemStats {
        devices: device_stats,
        rules: rule_stats,
        workflows: workflow_stats,
        alerts: alert_stats,
        commands: command_stats,
        decisions: decision_stats,
        system: system_info.clone(),
    };

    ok(json!({
        "stats": stats,
        "version": system_info.version,
        "uptime": system_info.uptime,
        "platform": system_info.platform,
        "arch": system_info.arch,
        "cpu_count": system_info.cpu_count,
        "total_memory": system_info.total_memory,
        "used_memory": system_info.used_memory,
        "free_memory": system_info.free_memory,
        "available_memory": system_info.available_memory,
        "gpus": system_info.gpus,
    }))
}

/// Detect GPUs on the system.
fn detect_gpus() -> Vec<GpuInfo> {
    let mut gpus = Vec::new();

    // Try to detect NVIDIA GPUs using nvidia-smi
    if let Ok(output) = std::process::Command::new("nvidia-smi")
        .arg("--query-gpu=name,memory.total,driver_version")
        .arg("--format=csv,noheader,nounits")
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.trim().split(',').collect();
                if parts.len() >= 3 {
                    let name = parts[0].trim().to_string();
                    let memory_mb = parts[1].trim().parse::<u64>().ok();
                    let driver_version = Some(parts[2].trim().to_string());
                    gpus.push(GpuInfo {
                        name,
                        vendor: "nvidia".to_string(),
                        total_memory_mb: memory_mb,
                        driver_version,
                    });
                }
            }
            return gpus;
        }
    }

    // Try to detect AMD GPUs using rocm-smi or lspci
    if let Ok(output) = std::process::Command::new("rocm-smi")
        .arg("--showproductname")
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Parse rocm-smi output for GPU names
            for line in stdout.lines() {
                if line.contains("Card series") || line.contains("GPU") {
                    let name = line
                        .split(':')
                        .last()
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|| "AMD GPU".to_string());
                    gpus.push(GpuInfo {
                        name,
                        vendor: "amd".to_string(),
                        total_memory_mb: None,
                        driver_version: None,
                    });
                }
            }
            if !gpus.is_empty() {
                return gpus;
            }
        }
    }

    // Try to detect Apple Silicon GPUs
    if std::env::consts::OS == "macos" && std::env::consts::ARCH == "aarch64" {
        if let Ok(output) = std::process::Command::new("system_profiler")
            .arg("SPDisplaysDataType")
            .arg("-json")
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // Look for Apple GPU in the output
                if stdout.contains("Apple") && stdout.contains("GPU") {
                    gpus.push(GpuInfo {
                        name: "Apple Silicon GPU".to_string(),
                        vendor: "apple".to_string(),
                        total_memory_mb: None,
                        driver_version: None,
                    });
                }
                return gpus;
            }
        }
    }

    // Fallback: try lspci for basic GPU detection (Linux)
    if std::env::consts::OS == "linux" {
        if let Ok(output) = std::process::Command::new("lspci")
            .arg("-nn")
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    if line.contains("VGA compatible controller")
                        || line.contains("3D controller")
                        || line.contains("Display")
                    {
                        // Extract GPU name
                        if let Some(colon_pos) = line.find(':') {
                            let name_part = &line[colon_pos + 1..];
                            let name = name_part
                                .split('(')
                                .next()
                                .map(|s| s.trim().to_string())
                                .unwrap_or_else(|| "Unknown GPU".to_string());

                            // Determine vendor
                            let vendor = if line.contains("NVIDIA") || line.contains("10de") {
                                "nvidia"
                            } else if line.contains("AMD") || line.contains("1002") {
                                "amd"
                            } else if line.contains("Intel") || line.contains("8086") {
                                "intel"
                            } else {
                                "other"
                            };

                            gpus.push(GpuInfo {
                                name,
                                vendor: vendor.to_string(),
                                total_memory_mb: None,
                                driver_version: None,
                            });
                        }
                    }
                }
            }
        }
    }

    gpus
}

/// Get device-specific statistics.
///
/// GET /api/stats/devices
pub async fn get_device_stats_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let configs = state.device_service.list_devices().await;

    let mut devices_with_stats = Vec::new();
    for config in configs {
        // Get metrics count for this device
        let metrics_count = state
            .device_service
            .get_current_metrics(&config.device_id)
            .await
            .map(|m| m.len())
            .unwrap_or(0);

        // Get actual connection status from DeviceService
        let device_status = state.device_service.get_device_status(&config.device_id).await;
        let is_online = device_status.is_connected();

        devices_with_stats.push(json!({
            "device_id": config.device_id,
            "device_type": config.device_type,
            "name": config.name,
            "online": is_online,
            "metrics_count": metrics_count,
            "last_seen": device_status.last_seen,
        }));
    }

    let online_count = devices_with_stats
        .iter()
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

    let enabled_count = rules
        .iter()
        .filter(|r| matches!(r.status, edge_ai_rules::RuleStatus::Active))
        .count();
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
                version: "0.1.0".to_string(),
                uptime: 3600,
                platform: "linux".to_string(),
                arch: "x86_64".to_string(),
                cpu_count: 8,
                total_memory: 16_000_000_000,
                used_memory: 8_000_000_000,
                free_memory: 4_000_000_000,
                available_memory: 8_000_000_000,
                gpus: vec![],
            },
        };

        let json_str = serde_json::to_string(&stats).unwrap();
        assert!(json_str.contains("total_devices"));
        assert!(json_str.contains("total_rules"));
    }
}
