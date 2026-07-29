//! Statistics API handlers.

use super::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::error::ErrorResponse;
use axum::extract::State;
use axum::Json;
use serde_json::json;

#[allow(unused_imports)]
use futures::future::join_all;

// Re-export ConnectionStatus from mdl_format for DeviceInstance

/// System statistics summary.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SystemStats {
    /// Device statistics
    pub devices: DeviceStats,
    /// Rule statistics
    pub rules: RuleStats,
    /// Alert statistics
    pub alerts: AlertStats,
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
    /// Global CPU usage (0..100), sampled over a short interval
    pub cpu_usage: f32,
    /// GPU information (if detected)
    pub gpus: Vec<GpuInfo>,
    /// Mounted filesystems (storage usage)
    pub disks: Vec<DiskInfo>,
    /// Network interfaces (traffic)
    pub networks: Vec<NetInfo>,
}

/// One mounted filesystem.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DiskInfo {
    /// Device / label name (e.g. "Macintosh HD", "/dev/sda1")
    pub name: String,
    /// Mount point (e.g. "/", "/home")
    pub mount: String,
    /// Total bytes
    pub total: u64,
    /// Used bytes (total - available)
    pub used: u64,
    /// Available bytes
    pub available: u64,
}

/// One network interface.
#[derive(Debug, Clone, serde::Serialize)]
pub struct NetInfo {
    /// Interface name (e.g. "en0", "eth0")
    pub name: String,
    /// First IPv4/IPv6 address, if any
    pub ip: String,
    /// MAC address
    pub mac: String,
    /// Total bytes received (cumulative)
    pub rx_bytes: u64,
    /// Total bytes transmitted (cumulative)
    pub tx_bytes: u64,
}

/// Get overall system statistics.
///
/// GET /api/stats/system
pub async fn get_system_stats_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    use std::time::Duration;

    // Try to get from cache first (5 second TTL)
    let cache_key = "stats:system";
    if let Some(cached) = state.response_cache.get(cache_key) {
        // Return cached response
        let cached_value: serde_json::Value = serde_json::from_str(&cached.body).map_err(|e| {
            crate::models::ErrorResponse::internal(format!(
                "Failed to parse cached response: {}",
                e
            ))
        })?;
        return Ok(Json(crate::models::common::ApiResponse::success(
            cached_value,
        )));
    }

    // Calculate start of today (UTC midnight)
    let now = chrono::Utc::now();
    let start_of_today = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| ErrorResponse::internal("Failed to calculate start of day"))?
        .and_utc()
        .timestamp();

    // Fetch all independent data sources in parallel
    let (device_stats_result, rule_stats_result, all_messages) = tokio::join!(
        async {
            // Device stats
            let configs = state.devices.service.list_devices();
            let metrics_futures: Vec<_> = configs
                .iter()
                .map(|c| state.devices.service.get_current_metrics(&c.device_id))
                .collect();
            let metrics_results = futures::future::join_all(metrics_futures).await;
            let devices_with_metrics = metrics_results
                .iter()
                .filter(|r| r.as_ref().is_ok_and(|m| !m.is_empty()))
                .count();

            use neomind_devices::adapter::ConnectionStatus;
            let online = state
                .devices
                .service
                .get_devices_by_status(ConnectionStatus::Connected)
                .await
                .len();
            let offline = state
                .devices
                .service
                .get_devices_by_status(ConnectionStatus::Disconnected)
                .await
                .len();

            DeviceStats {
                total_devices: configs.len(),
                online_devices: online,
                offline_devices: offline,
                devices_with_metrics,
            }
        },
        async {
            // Rule stats
            let rules = state.automation.rule_engine.list_rules().await;
            let triggered_today = if let Some(ref store) = state.automation.rule_store {
                store.count_history_since(start_of_today).unwrap_or(0) as usize
            } else {
                0
            };
            RuleStats {
                total_rules: rules.len(),
                enabled_rules: rules.iter().filter(|r| r.enabled).count(),
                disabled_rules: rules.iter().filter(|r| !r.enabled).count(),
                triggered_today,
            }
        },
        // Messages
        state.core.message_manager.list_messages(),
    );

    let device_stats = device_stats_result;
    let rule_stats = rule_stats_result;
    let active_messages: Vec<_> = all_messages
        .into_iter()
        .filter(|m| matches!(m.status, neomind_messages::MessageStatus::Active))
        .collect();

    // Get today's messages count
    let start_of_day = chrono::Utc::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| ErrorResponse::internal("Failed to calculate start of day"))?
        .and_utc();
    let messages_today = active_messages
        .iter()
        .filter(|m| m.timestamp >= start_of_day)
        .count();

    let alert_stats = AlertStats {
        active_alerts: active_messages.len(),
        by_severity: json!({
            "info": active_messages.iter().filter(|m| matches!(m.severity, neomind_messages::MessageSeverity::Info)).count(),
            "warning": active_messages.iter().filter(|m| matches!(m.severity, neomind_messages::MessageSeverity::Warning)).count(),
            "critical": active_messages.iter().filter(|m| matches!(m.severity, neomind_messages::MessageSeverity::Critical)).count(),
            "emergency": active_messages.iter().filter(|m| matches!(m.severity, neomind_messages::MessageSeverity::Emergency)).count(),
        }),
        alerts_today: messages_today,
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

    // Get memory + CPU info using sysinfo. CPU usage needs two samples spaced
    // in time (first refresh establishes a baseline; the value is meaningful on
    // the next refresh), so take a ~200ms sample. The 5s response cache means
    // this only runs once per cache window.
    let (total_memory, used_memory, free_memory, available_memory, cpu_usage) = {
        let mut sys = sysinfo::System::new();
        sys.refresh_memory();
        sys.refresh_cpu_usage();
        tokio::time::sleep(Duration::from_millis(200)).await;
        sys.refresh_cpu_usage();
        (
            sys.total_memory(),
            sys.used_memory(),
            sys.free_memory(),
            sys.available_memory(),
            sys.global_cpu_usage(),
        )
    };

    // Mounted filesystems (storage usage)
    let disks: Vec<DiskInfo> = {
        let dl = sysinfo::Disks::new_with_refreshed_list();
        // macOS APFS surfaces the same container at multiple mount points
        // (e.g. "/" and "/System/Volumes/Data"); dedup by (total, used).
        let mut seen = std::collections::HashSet::new();
        dl.iter()
            .filter_map(|d| {
                let total = d.total_space();
                let available = d.available_space();
                let used = total.saturating_sub(available);
                if !seen.insert((total, used)) {
                    return None;
                }
                Some(DiskInfo {
                    name: d.name().to_string_lossy().to_string(),
                    mount: d.mount_point().to_string_lossy().to_string(),
                    total,
                    used,
                    available,
                })
            })
            .collect()
    };

    // Network interfaces (traffic). Skip loopback.
    let networks: Vec<NetInfo> = {
        let nets = sysinfo::Networks::new_with_refreshed_list();
        nets.iter()
            .filter_map(|(name, net)| {
                if name.starts_with("lo") {
                    return None;
                }
                // Only include interfaces with a routable IPv4 address — skips
                // macOS's many IPv6 link-local / virtual interfaces (utun*, awdl*,
                // llw*, bridge*, stf*, etc.) so the panel shows real connections.
                let ip = net
                    .ip_networks()
                    .iter()
                    .find(|n| n.addr.is_ipv4())
                    .map(|n| n.addr.to_string())?;
                Some(NetInfo {
                    name: name.to_string(),
                    ip,
                    mac: net.mac_address().to_string(),
                    rx_bytes: net.total_received(),
                    tx_bytes: net.total_transmitted(),
                })
            })
            .collect()
    };

    // Detect GPUs (lazy: first call triggers detection, subsequent calls use cache)
    let gpus = match state.gpu_info.get() {
        Some(gpus) => gpus.clone(),
        None => {
            let gpus = detect_gpus();
            // Another request may have already populated it — set() is fine to ignore
            let _ = state.gpu_info.set(gpus.clone());
            tracing::info!("GPU information cached: {} GPU(s) detected", gpus.len());
            gpus
        }
    };

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
        cpu_usage,
        gpus,
        disks,
        networks,
    };

    let stats = SystemStats {
        devices: device_stats,
        rules: rule_stats,
        alerts: alert_stats,
        system: system_info.clone(),
    };

    let response = ok(json!({
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
        "cpu_usage": system_info.cpu_usage,
        "gpus": system_info.gpus,
        "disks": system_info.disks,
        "networks": system_info.networks,
    }))?;

    // Cache the response for 5 seconds
    state.response_cache.put(
        cache_key.to_string(),
        crate::cache::CachedResponse {
            body: serde_json::to_string(&response.0.data).unwrap_or_default(),
            content_type: "application/json".to_string(),
        },
        Some(Duration::from_secs(5)),
    );

    Ok(response)
}

/// Detect GPUs on the system.
pub fn detect_gpus() -> Vec<GpuInfo> {
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
                        .next_back()
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
        if let Ok(output) = std::process::Command::new("lspci").arg("-nn").output() {
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
    let configs = state.devices.service.list_devices();

    let mut devices_with_stats = Vec::new();
    for config in configs {
        // Get metrics count for this device
        let metrics_count = state
            .devices
            .service
            .get_current_metrics(&config.device_id)
            .await
            .map(|m| m.len())
            .unwrap_or(0);

        // Get actual connection status from DeviceService
        let device_status = state
            .devices
            .service
            .get_device_status(&config.device_id)
            .await;
        let is_online = device_status.is_connected_within(
            state
                .devices
                .service
                .effective_offline_timeout(&config.device_id),
        );

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
    let rules = state.automation.rule_engine.list_rules().await;

    let enabled_count = rules.iter().filter(|r| r.enabled).count();
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
            alerts: AlertStats {
                active_alerts: 1,
                by_severity: json!({"critical": 1}),
                alerts_today: 3,
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
                cpu_usage: 42.0,
                disks: vec![],
                networks: vec![],
            },
        };

        let json_str = serde_json::to_string(&stats).unwrap();
        assert!(json_str.contains("total_devices"));
        assert!(json_str.contains("total_rules"));
    }
}
