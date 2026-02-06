//! Core business-scenario oriented tools for NeoMind.
//!
//! These tools are designed around user workflows rather than technical categorization:
//! - Device discovery and exploration
//! - Device data querying and analysis
//! - Device control operations
//! - Rule creation from conversation context
//!
//! Design principles:
//! 1. Device-centric - devices are the core resource
//! 2. Conversation flow oriented - tools follow natural dialogue patterns
//! 3. Industry agnostic - business logic is pluggable via industry-specific plugins

use std::sync::Arc;
use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;

use crate::error::Result as ToolResult;
use crate::error::ToolError;
use crate::tool::{
    Tool, ToolOutput,
    array_property, boolean_property, number_property, object_schema,
    string_property,
};
use neomind_core::tools::{ToolCategory, ToolRelationships, UsageScenario};

// Import device types for real adapter
use neomind_devices::DeviceTypeTemplate;

// ============================================================================
// Shared Types and State
// ============================================================================

/// Device information from the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub device_type: String,
    pub location: Option<String>,
    pub status: String,
    pub tags: Vec<String>,
    pub capabilities: DeviceCapabilities,
    pub latest_data: Option<HashMap<String, f64>>,
}

/// Device capabilities (metrics and commands).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCapabilities {
    pub metrics: Vec<MetricInfo>,
    pub commands: Vec<CommandInfo>,
}

/// Metric information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricInfo {
    pub name: String,
    pub display_name: String,
    pub unit: String,
    pub data_type: String,
}

/// Command information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandInfo {
    pub name: String,
    pub display_name: String,
    pub parameters: Vec<ParameterInfo>,
}

/// Parameter information for commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterInfo {
    pub name: String,
    pub display_name: String,
    pub data_type: String,
    pub default_value: Option<Value>,
}

/// Device group for organized listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceGroup {
    pub name: String,
    pub count: usize,
    pub devices: Vec<DeviceInfo>,
}

/// Device discovery summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverySummary {
    pub total: usize,
    pub online: usize,
    pub offline: usize,
    pub by_type: HashMap<String, usize>,
    pub by_location: HashMap<String, usize>,
}

// ============================================================================
// Device Registry Trait - Abstraction for device data source
// ============================================================================

/// Async trait for device registry operations.
/// This allows tools to work with both mock (testing) and real (production) device sources.
#[async_trait]
pub trait DeviceRegistryTrait: Send + Sync {
    /// Get all devices.
    async fn get_all(&self) -> Vec<DeviceInfo>;

    /// Find device by ID.
    async fn find_by_id(&self, id: &str) -> Option<DeviceInfo>;

    /// Find devices by location.
    async fn find_by_location(&self, location: &str) -> Vec<DeviceInfo>;

    /// Find devices by type.
    async fn find_by_type(&self, device_type: &str) -> Vec<DeviceInfo>;

    /// Find devices by tag.
    async fn find_by_tag(&self, tag: &str) -> Vec<DeviceInfo>;

    /// Find devices matching a filter.
    async fn find_by_filter(&self, filter: &DeviceFilter) -> Vec<DeviceInfo>;

    /// Update device data (for control operations).
    async fn update_device_data(&self, id: &str, data: HashMap<String, f64>);
}

// ============================================================================
// Real Device Registry Adapter - Connects to actual DeviceService
// ============================================================================

/// Adapter that connects to the real DeviceService from neomind_devices.
pub struct RealDeviceRegistryAdapter {
    /// Reference to the device service
    device_service: Arc<neomind_devices::DeviceService>,
    /// Cache for device type templates
    template_cache: Arc<RwLock<HashMap<String, DeviceTypeTemplate>>>,
}

impl RealDeviceRegistryAdapter {
    /// Create a new adapter with device service.
    pub fn new(device_service: Arc<neomind_devices::DeviceService>) -> Self {
        Self {
            device_service,
            template_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Convert DeviceConfig to DeviceInfo.
    async fn to_device_info(&self, config: &neomind_devices::DeviceConfig) -> Option<DeviceInfo> {
        // Get template for this device type
        let template = self.get_template_cached(&config.device_type).await?;

        // Get connection status - default to unknown if not available
        let status = match self.device_service.get_device_connection_status(&config.device_id).await {
            neomind_devices::ConnectionStatus::Connected => "online".to_string(),
            neomind_devices::ConnectionStatus::Disconnected => "offline".to_string(),
            neomind_devices::ConnectionStatus::Connecting => "connecting".to_string(),
            neomind_devices::ConnectionStatus::Reconnecting => "reconnecting".to_string(),
            neomind_devices::ConnectionStatus::Error => "error".to_string(),
        };

        // For now, don't fetch latest data (can be added later with TimeSeries integration)
        let latest_data = None;

        // Extract location from connection config extra metadata
        let location = config.connection_config.extra.get("location")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Extract tags from connection config extra metadata
        let tags = config.connection_config.extra.get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect())
            .unwrap_or_default();

        Some(DeviceInfo {
            id: config.device_id.clone(),
            name: config.name.clone(),
            device_type: config.device_type.clone(),
            location,
            status,
            tags,
            capabilities: DeviceCapabilities {
                metrics: template
                    .metrics
                    .iter()
                    .map(|m| MetricInfo {
                        name: m.name.clone(),
                        display_name: m.display_name.clone(),
                        unit: m.unit.clone(),
                        data_type: format!("{:?}", m.data_type),
                    })
                    .collect(),
                commands: template
                    .commands
                    .iter()
                    .map(|c| CommandInfo {
                        name: c.name.clone(),
                        display_name: c.display_name.clone(),
                        parameters: c
                            .parameters
                            .iter()
                            .map(|p| ParameterInfo {
                                name: p.name.clone(),
                                display_name: p.display_name.clone(),
                                data_type: format!("{:?}", p.data_type),
                                default_value: p.default_value.as_ref().and_then(|v| {
                                    // Convert MetricValue to serde_json::Value
                                    match v {
                                        neomind_devices::MetricValue::String(s) => Some(Value::String(s.clone())),
                                        neomind_devices::MetricValue::Float(f) => serde_json::Number::from_f64(*f).map(Value::Number),
                                        neomind_devices::MetricValue::Integer(i) => Some(Value::Number(serde_json::Number::from(*i))),
                                        neomind_devices::MetricValue::Boolean(b) => Some(Value::Bool(*b)),
                                        neomind_devices::MetricValue::Array(a) => {
                                            // Convert array to JSON
                                            let json_arr: Vec<Value> = a.iter().map(|v| match v {
                                                neomind_devices::MetricValue::String(s) => Value::String(s.clone()),
                                                neomind_devices::MetricValue::Integer(i) => Value::Number(serde_json::Number::from(*i)),
                                                neomind_devices::MetricValue::Float(f) => {
                                                    serde_json::Number::from_f64(*f)
                                                        .map(Value::Number)
                                                        .unwrap_or(Value::Null)
                                                },
                                                neomind_devices::MetricValue::Boolean(b) => Value::Bool(*b),
                                                _ => Value::Null,
                                            }).collect();
                                            Some(Value::Array(json_arr))
                                        }
                                        neomind_devices::MetricValue::Null => Some(Value::Null),
                                        neomind_devices::MetricValue::Binary(_) => None, // Skip binary for now
                                    }
                                }),
                            })
                            .collect(),
                    })
                    .collect(),
            },
            latest_data,
        })
    }

    /// Get template with caching.
    async fn get_template_cached(&self, device_type: &str) -> Option<DeviceTypeTemplate> {
        // Check cache first
        {
            let cache = self.template_cache.read().await;
            if let Some(template) = cache.get(device_type) {
                return Some(template.clone());
            }
        }

        // Fetch from service
        let template = self.device_service.get_template(device_type).await?;

        // Cache it
        let mut cache = self.template_cache.write().await;
        cache.insert(device_type.to_string(), template.clone());
        Some(template)
    }
}

#[async_trait]
impl DeviceRegistryTrait for RealDeviceRegistryAdapter {
    async fn get_all(&self) -> Vec<DeviceInfo> {
        let devices = self.device_service.list_devices().await;
        let mut result = Vec::new();

        for config in devices {
            if let Some(info) = self.to_device_info(&config).await {
                result.push(info);
            }
        }

        result
    }

    async fn find_by_id(&self, id: &str) -> Option<DeviceInfo> {
        let config = self.device_service.get_device(id).await?;
        self.to_device_info(&config).await
    }

    async fn find_by_location(&self, location: &str) -> Vec<DeviceInfo> {
        let all = self.get_all().await;
        all.into_iter()
            .filter(|d| d.location.as_deref() == Some(location))
            .collect()
    }

    async fn find_by_type(&self, device_type: &str) -> Vec<DeviceInfo> {
        let devices = self.device_service.list_devices_by_type(device_type).await;
        let mut result = Vec::new();

        for config in devices {
            if let Some(info) = self.to_device_info(&config).await {
                result.push(info);
            }
        }

        result
    }

    async fn find_by_tag(&self, tag: &str) -> Vec<DeviceInfo> {
        let all = self.get_all().await;
        all.into_iter()
            .filter(|d| d.tags.iter().any(|t| t == tag))
            .collect()
    }

    async fn find_by_filter(&self, filter: &DeviceFilter) -> Vec<DeviceInfo> {
        let all = self.get_all().await;
        all.into_iter()
            .filter(|d| filter.matches(d))
            .collect()
    }

    async fn update_device_data(&self, _id: &str, _data: HashMap<String, f64>) {
        // Device updates are handled through the device service's send_command method
        // This is a no-op for the read-only registry
    }
}

// Type alias for convenience
pub type DeviceRegistryAdapter = Arc<dyn DeviceRegistryTrait>;

// ============================================================================
// Mock Device Registry - FOR TESTING ONLY
// ============================================================================
pub struct MockDeviceRegistry {
    devices: Arc<RwLock<Vec<DeviceInfo>>>,
}

impl MockDeviceRegistry {
    pub fn new() -> Self {
        // Create sample devices representing different scenarios
        let devices = vec![
            // Living room devices
            DeviceInfo {
                id: "sensor_temp_living".to_string(),
                name: "客厅温度传感器".to_string(),
                device_type: "DHT22".to_string(),
                location: Some("客厅".to_string()),
                status: "online".to_string(),
                tags: vec!["sensor".to_string(), "temperature".to_string()],
                capabilities: DeviceCapabilities {
                    metrics: vec![
                        MetricInfo {
                            name: "temperature".to_string(),
                            display_name: "温度".to_string(),
                            unit: "°C".to_string(),
                            data_type: "float".to_string(),
                        },
                        MetricInfo {
                            name: "humidity".to_string(),
                            display_name: "湿度".to_string(),
                            unit: "%".to_string(),
                            data_type: "float".to_string(),
                        },
                    ],
                    commands: vec![],
                },
                latest_data: Some({
                    let mut map = HashMap::new();
                    map.insert("temperature".to_string(), 26.5);
                    map.insert("humidity".to_string(), 60.0);
                    map
                }),
            },
            DeviceInfo {
                id: "light_living_main".to_string(),
                name: "客厅主灯".to_string(),
                device_type: "SmartBulb".to_string(),
                location: Some("客厅".to_string()),
                status: "online".to_string(),
                tags: vec!["actuator".to_string(), "light".to_string()],
                capabilities: DeviceCapabilities {
                    metrics: vec![
                        MetricInfo {
                            name: "state".to_string(),
                            display_name: "状态".to_string(),
                            unit: "".to_string(),
                            data_type: "boolean".to_string(),
                        },
                        MetricInfo {
                            name: "brightness".to_string(),
                            display_name: "亮度".to_string(),
                            unit: "%".to_string(),
                            data_type: "integer".to_string(),
                        },
                    ],
                    commands: vec![
                        CommandInfo {
                            name: "turn_on".to_string(),
                            display_name: "打开".to_string(),
                            parameters: vec![],
                        },
                        CommandInfo {
                            name: "turn_off".to_string(),
                            display_name: "关闭".to_string(),
                            parameters: vec![],
                        },
                        CommandInfo {
                            name: "set_brightness".to_string(),
                            display_name: "设置亮度".to_string(),
                            parameters: vec![
                                ParameterInfo {
                                    name: "brightness".to_string(),
                                    display_name: "亮度".to_string(),
                                    data_type: "integer".to_string(),
                                    default_value: Some(Value::Number(100.into())),
                                },
                            ],
                        },
                    ],
                },
                latest_data: Some({
                    let mut map = HashMap::new();
                    map.insert("state".to_string(), 0.0); // off
                    map.insert("brightness".to_string(), 0.0);
                    map
                }),
            },
            // Bedroom devices
            DeviceInfo {
                id: "sensor_temp_bedroom".to_string(),
                name: "卧室温度传感器".to_string(),
                device_type: "DHT22".to_string(),
                location: Some("卧室".to_string()),
                status: "online".to_string(),
                tags: vec!["sensor".to_string(), "temperature".to_string()],
                capabilities: DeviceCapabilities {
                    metrics: vec![
                        MetricInfo {
                            name: "temperature".to_string(),
                            display_name: "温度".to_string(),
                            unit: "°C".to_string(),
                            data_type: "float".to_string(),
                        },
                        MetricInfo {
                            name: "humidity".to_string(),
                            display_name: "湿度".to_string(),
                            unit: "%".to_string(),
                            data_type: "float".to_string(),
                        },
                    ],
                    commands: vec![],
                },
                latest_data: Some({
                    let mut map = HashMap::new();
                    map.insert("temperature".to_string(), 24.0);
                    map.insert("humidity".to_string(), 55.0);
                    map
                }),
            },
            DeviceInfo {
                id: "ac_bedroom".to_string(),
                name: "卧室空调".to_string(),
                device_type: "AirConditioner".to_string(),
                location: Some("卧室".to_string()),
                status: "online".to_string(),
                tags: vec!["actuator".to_string(), "hvac".to_string()],
                capabilities: DeviceCapabilities {
                    metrics: vec![
                        MetricInfo {
                            name: "power_state".to_string(),
                            display_name: "电源状态".to_string(),
                            unit: "".to_string(),
                            data_type: "boolean".to_string(),
                        },
                        MetricInfo {
                            name: "current_temp".to_string(),
                            display_name: "当前温度".to_string(),
                            unit: "°C".to_string(),
                            data_type: "float".to_string(),
                        },
                        MetricInfo {
                            name: "target_temp".to_string(),
                            display_name: "目标温度".to_string(),
                            unit: "°C".to_string(),
                            data_type: "float".to_string(),
                        },
                    ],
                    commands: vec![
                        CommandInfo {
                            name: "turn_on".to_string(),
                            display_name: "打开".to_string(),
                            parameters: vec![],
                        },
                        CommandInfo {
                            name: "turn_off".to_string(),
                            display_name: "关闭".to_string(),
                            parameters: vec![],
                        },
                        CommandInfo {
                            name: "set_temperature".to_string(),
                            display_name: "设置温度".to_string(),
                            parameters: vec![
                                ParameterInfo {
                                    name: "temperature".to_string(),
                                    display_name: "温度".to_string(),
                                    data_type: "float".to_string(),
                                    default_value: Some(Value::Number(24.into())),
                                },
                            ],
                        },
                    ],
                },
                latest_data: Some({
                    let mut map = HashMap::new();
                    map.insert("power_state".to_string(), 0.0);
                    map.insert("current_temp".to_string(), 24.0);
                    map.insert("target_temp".to_string(), 24.0);
                    map
                }),
            },
            // Kitchen devices
            DeviceInfo {
                id: "sensor_temp_kitchen".to_string(),
                name: "厨房温度传感器".to_string(),
                device_type: "DHT22".to_string(),
                location: Some("厨房".to_string()),
                status: "online".to_string(),
                tags: vec!["sensor".to_string(), "temperature".to_string()],
                capabilities: DeviceCapabilities {
                    metrics: vec![
                        MetricInfo {
                            name: "temperature".to_string(),
                            display_name: "温度".to_string(),
                            unit: "°C".to_string(),
                            data_type: "float".to_string(),
                        },
                    ],
                    commands: vec![],
                },
                latest_data: Some({
                    let mut map = HashMap::new();
                    map.insert("temperature".to_string(), 28.0);
                    map
                }),
            },
            DeviceInfo {
                id: "light_kitchen".to_string(),
                name: "厨房灯".to_string(),
                device_type: "SmartBulb".to_string(),
                location: Some("厨房".to_string()),
                status: "online".to_string(),
                tags: vec!["actuator".to_string(), "light".to_string()],
                capabilities: DeviceCapabilities {
                    metrics: vec![],
                    commands: vec![
                        CommandInfo {
                            name: "turn_on".to_string(),
                            display_name: "打开".to_string(),
                            parameters: vec![],
                        },
                        CommandInfo {
                            name: "turn_off".to_string(),
                            display_name: "关闭".to_string(),
                            parameters: vec![],
                        },
                    ],
                },
                latest_data: None,
            },
            // Bathroom
            DeviceInfo {
                id: "light_bathroom".to_string(),
                name: "浴室灯".to_string(),
                device_type: "SmartBulb".to_string(),
                location: Some("浴室".to_string()),
                status: "offline".to_string(),
                tags: vec!["actuator".to_string(), "light".to_string()],
                capabilities: DeviceCapabilities {
                    metrics: vec![],
                    commands: vec![
                        CommandInfo {
                            name: "turn_on".to_string(),
                            display_name: "打开".to_string(),
                            parameters: vec![],
                        },
                        CommandInfo {
                            name: "turn_off".to_string(),
                            display_name: "关闭".to_string(),
                            parameters: vec![],
                        },
                    ],
                },
                latest_data: None,
            },
        ];

        Self {
            devices: Arc::new(RwLock::new(devices)),
        }
    }

    pub async fn get_all(&self) -> Vec<DeviceInfo> {
        self.devices.read().await.clone()
    }

    pub async fn find_by_id(&self, id: &str) -> Option<DeviceInfo> {
        self.devices
            .read()
            .await
            .iter()
            .find(|d| d.id == id)
            .cloned()
    }

    pub async fn find_by_filter(&self, filter: &DeviceFilter) -> Vec<DeviceInfo> {
        let devices = self.devices.read().await;
        devices
            .iter()
            .filter(|d| filter.matches(d))
            .cloned()
            .collect()
    }

    pub async fn find_by_location(&self, location: &str) -> Vec<DeviceInfo> {
        let devices = self.devices.read().await;
        devices
            .iter()
            .filter(|d| d.location.as_deref() == Some(location))
            .cloned()
            .collect()
    }

    pub async fn find_by_type(&self, device_type: &str) -> Vec<DeviceInfo> {
        let devices = self.devices.read().await;
        devices
            .iter()
            .filter(|d| d.device_type == device_type)
            .cloned()
            .collect()
    }

    pub async fn find_by_tag(&self, tag: &str) -> Vec<DeviceInfo> {
        let devices = self.devices.read().await;
        devices
            .iter()
            .filter(|d| d.tags.contains(&tag.to_string()))
            .cloned()
            .collect()
    }

    pub async fn update_device_data(&self, id: &str, data: HashMap<String, f64>) {
        let mut devices = self.devices.write().await;
        if let Some(device) = devices.iter_mut().find(|d| d.id == id) {
            device.latest_data = Some(data);
        }
    }
}

impl Default for MockDeviceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// Implement DeviceRegistryTrait for MockDeviceRegistry
#[async_trait]
impl DeviceRegistryTrait for MockDeviceRegistry {
    async fn get_all(&self) -> Vec<DeviceInfo> {
        self.get_all().await
    }

    async fn find_by_id(&self, id: &str) -> Option<DeviceInfo> {
        self.find_by_id(id).await
    }

    async fn find_by_filter(&self, filter: &DeviceFilter) -> Vec<DeviceInfo> {
        self.find_by_filter(filter).await
    }

    async fn find_by_location(&self, location: &str) -> Vec<DeviceInfo> {
        self.find_by_location(location).await
    }

    async fn find_by_type(&self, device_type: &str) -> Vec<DeviceInfo> {
        self.find_by_type(device_type).await
    }

    async fn find_by_tag(&self, tag: &str) -> Vec<DeviceInfo> {
        self.find_by_tag(tag).await
    }

    async fn update_device_data(&self, id: &str, data: HashMap<String, f64>) {
        self.update_device_data(id, data).await
    }
}

/// Device filter for discovery.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DeviceFilter {
    pub r#type: Option<String>,
    pub location: Option<String>,
    pub status: Option<String>,
    pub tags: Option<Vec<String>>,
    pub name_contains: Option<String>,
}

impl DeviceFilter {
    pub fn matches(&self, device: &DeviceInfo) -> bool {
        if let Some(ref t) = self.r#type
            && device.device_type != *t {
                return false;
            }
        if let Some(ref loc) = self.location
            && device.location.as_ref() != Some(loc) {
                return false;
            }
        if let Some(ref status) = self.status
            && device.status != *status {
                return false;
            }
        if let Some(ref tags) = self.tags
            && !tags.iter().all(|t| device.tags.contains(t)) {
                return false;
            }
        if let Some(ref name) = self.name_contains
            && !device.name.contains(name) && !device.id.contains(name) {
                return false;
            }
        true
    }
}

// ============================================================================
// Tool 1: device.discover
// ============================================================================

/// Device discovery tool - the entry point for device exploration.
pub struct DeviceDiscoverTool {
    registry: DeviceRegistryAdapter,
}

impl DeviceDiscoverTool {
    /// Create a new device discover tool with a registry adapter.
    pub fn new(registry: DeviceRegistryAdapter) -> Self {
        Self { registry }
    }

    /// Create a mock tool for testing (uses MockDeviceRegistry).
    #[cfg(test)]
    pub fn mock() -> Self {
        Self::new(Arc::new(MockDeviceRegistry::new()))
    }

    /// Create with real device service.
    pub fn with_real_device_service(device_service: Arc<neomind_devices::DeviceService>) -> Self {
        let adapter = Arc::new(RealDeviceRegistryAdapter::new(device_service));
        Self::new(adapter)
    }

    fn group_devices(&self, devices: Vec<DeviceInfo>, group_by: &str) -> Vec<DeviceGroup> {
        if group_by == "none" || devices.is_empty() {
            return vec![DeviceGroup {
                name: "所有设备".to_string(),
                count: devices.len(),
                devices,
            }];
        }

        let mut groups: HashMap<String, Vec<DeviceInfo>> = HashMap::new();

        for device in devices {
            let key = match group_by {
                "type" => device.device_type.clone(),
                "location" => device.location.clone().unwrap_or_else(|| "未知".to_string()),
                "status" => match device.status.as_str() {
                    "online" => "在线".to_string(),
                    "offline" => "离线".to_string(),
                    _ => device.status.clone(),
                },
                _ => "其他".to_string(),
            };
            groups.entry(key).or_default().push(device);
        }

        let mut result: Vec<DeviceGroup> = groups
            .into_iter()
            .map(|(name, devices)| DeviceGroup {
                name,
                count: devices.len(),
                devices,
            })
            .collect();
        result.sort_by(|a, b| a.name.cmp(&b.name));
        result
    }

    fn calculate_summary(&self, devices: &[DeviceInfo]) -> DiscoverySummary {
        let total = devices.len();
        let online = devices.iter().filter(|d| d.status == "online").count();
        let offline = total - online;

        let mut by_type = HashMap::new();
        let mut by_location = HashMap::new();

        for device in devices {
            *by_type.entry(device.device_type.clone()).or_insert(0) += 1;
            if let Some(ref loc) = device.location {
                *by_location.entry(loc.clone()).or_insert(0) += 1;
            }
        }

        DiscoverySummary {
            total,
            online,
            offline,
            by_type,
            by_location,
        }
    }
}

// Default impl is only available in test mode (requires mock data)
#[cfg(test)]
impl Default for DeviceDiscoverTool {
    fn default() -> Self {
        Self::mock()
    }
}

#[async_trait]
impl Tool for DeviceDiscoverTool {
    fn name(&self) -> &str {
        "device.discover"
    }

    fn description(&self) -> &str {
        "发现和列出系统中的所有设备。支持按位置、类型、状态过滤和分组。\
        这是探索系统设备能力的入口工具。\
        \
        用法示例: \
        - '有什么设备？' → 列出所有设备 \
        - '客厅有哪些设备？' → 按位置过滤 \
        - '有哪些传感器？' → 按类型过滤 \
        - '在线的设备有哪些？' → 按状态过滤"
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "filter": object_schema(serde_json::json!({
                    "description": "过滤条件",
                    "properties": {
                        "type": string_property("设备类型，如'sensor'、'actuator'、'DHT22'等"),
                        "location": string_property("位置，如'客厅'、'卧室'、'厨房'等"),
                        "status": string_property("状态：'online'在线、'offline'离线"),
                        "tags": array_property("string", "标签过滤，如['sensor', 'temperature']"),
                        "name_contains": string_property("名称包含关键词")
                    }
                }), vec![]),
                "group_by": string_property("分组方式：'type'按类型、'location'按位置、'status'按状态、'none'不分组。默认'none'"),
                "include_data_preview": boolean_property("是否包含最新数据预览。默认true"),
                "include_capabilities": boolean_property("是否包含设备能力（指标和命令）。默认true")
            }),
            vec![],
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Device
    }

    fn scenarios(&self) -> Vec<UsageScenario> {
        vec![
            UsageScenario {
                description: "用户询问有什么设备".to_string(),
                example_query: "有什么设备？".to_string(),
                suggested_call: Some("device.discover()".to_string()),
            },
            UsageScenario {
                description: "用户询问特定位置的设备".to_string(),
                example_query: "客厅有哪些设备？".to_string(),
                suggested_call: Some("device.discover({filter: {location: '客厅'}, group_by: 'type'})".to_string()),
            },
            UsageScenario {
                description: "用户询问特定类型的设备".to_string(),
                example_query: "有哪些传感器？".to_string(),
                suggested_call: Some("device.discover({filter: {tags: ['sensor']}, group_by: 'location'})".to_string()),
            },
            UsageScenario {
                description: "用户询问离线设备".to_string(),
                example_query: "哪些设备离线了？".to_string(),
                suggested_call: Some("device.discover({filter: {status: 'offline'}})".to_string()),
            },
        ]
    }

    async fn execute(&self, args: Value) -> ToolResult<ToolOutput> {
        // Parse filter
        let filter = if let Some(filter_obj) = args.get("filter").and_then(|v| v.as_object()) {
            DeviceFilter {
                r#type: filter_obj.get("type").and_then(|v| v.as_str()).map(String::from),
                location: filter_obj.get("location").and_then(|v| v.as_str()).map(String::from),
                status: filter_obj.get("status").and_then(|v| v.as_str()).map(String::from),
                tags: filter_obj.get("tags").and_then(|v| v.as_array()).map(|arr| {
                    arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
                }),
                name_contains: filter_obj.get("name_contains").and_then(|v| v.as_str()).map(String::from),
            }
        } else {
            DeviceFilter::default()
        };

        // Get filtered devices
        let mut devices = self.registry.find_by_filter(&filter).await;

        // Apply data preview option
        let include_data_preview = args
            .get("include_data_preview")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        if !include_data_preview {
            for device in &mut devices {
                device.latest_data = None;
            }
        }

        // Apply capabilities option
        let include_capabilities = args
            .get("include_capabilities")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        if !include_capabilities {
            for device in &mut devices {
                device.capabilities = DeviceCapabilities {
                    metrics: vec![],
                    commands: vec![],
                };
            }
        }

        // Get grouping option
        let group_by = args
            .get("group_by")
            .and_then(|v| v.as_str())
            .unwrap_or("none");

        // Group devices
        let groups = self.group_devices(devices.clone(), group_by);

        // Calculate summary
        let summary = self.calculate_summary(&devices);

        // Build response
        let result = serde_json::json!({
            "groups": groups,
            "summary": {
                "total": summary.total,
                "online": summary.online,
                "offline": summary.offline,
                "by_type": summary.by_type,
                "by_location": summary.by_location
            },
            "filter_applied": filter != DeviceFilter::default()
        });

        Ok(ToolOutput::success(result))
    }
}

// ============================================================================
// Tool 2: device.query
// ============================================================================

/// Time range specification for queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: Option<String>,
    pub end: Option<String>,
}

impl TimeRange {
    pub fn relative(hours_ago: i64) -> Self {
        Self {
            start: Some(format!("{}小时前", hours_ago)),
            end: None,
        }
    }

    pub fn absolute(start: i64, end: i64) -> Self {
        Self {
            start: Some(start.to_string()),
            end: Some(end.to_string()),
        }
    }
}

/// Data point with timestamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPoint {
    pub timestamp: i64,
    pub value: f64,
}

/// Metric query result with statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricQueryResult {
    pub metric: String,
    pub display_name: String,
    pub unit: String,
    pub current: Option<f64>,
    pub history: Vec<DataPoint>,
    pub stats: Option<MetricStatistics>,
    pub analysis_hint: Option<String>,
}

/// Statistics for metric data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricStatistics {
    pub avg: f64,
    pub min: f64,
    pub max: f64,
    pub count: usize,
    pub trend: Option<String>,
}

/// Device query tool - queries device data with optional aggregation.
pub struct DeviceQueryTool {
    registry: DeviceRegistryAdapter,
}

impl DeviceQueryTool {
    pub fn new(registry: DeviceRegistryAdapter) -> Self {
        Self { registry }
    }

    pub fn mock() -> Self {
        Self::new(Arc::new(MockDeviceRegistry::new()))
    }

    /// Generate analysis hint based on data trend.
    fn generate_analysis_hint(&self, result: &MetricQueryResult) -> Option<String> {
        let stats = result.stats.as_ref()?;
        let trend = stats.trend.as_ref()?;

        Some(match trend.as_str() {
            "rising" => format!("{}呈上升趋势，当前值{:.1}{}", result.display_name, result.current.unwrap_or(0.0), result.unit),
            "falling" => format!("{}呈下降趋势，当前值{:.1}{}", result.display_name, result.current.unwrap_or(0.0), result.unit),
            "stable" => format!("{}保持稳定，平均值{:.1}{}", result.display_name, stats.avg, result.unit),
            _ => format!("{}：平均值{:.1}{}，范围{:.1}-{}{}", result.display_name, stats.avg, result.unit, stats.min, stats.max, result.unit),
        })
    }

    /// Calculate statistics from data points.
    fn calculate_stats(&self, data: &[DataPoint]) -> Option<MetricStatistics> {
        if data.is_empty() {
            return None;
        }

        let values: Vec<f64> = data.iter().map(|d| d.value).collect();
        let count = values.len();
        let avg = values.iter().sum::<f64>() / count as f64;
        let min = values.iter().fold(f64::INFINITY, |a, &b| a.min(b));
        let max = values.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));

        // Detect trend
        let trend = if count >= 2 {
            let first = values.first().unwrap();
            let last = values.last().unwrap();
            let change = last - first;
            let pct_change = (change / first.abs().max(0.01)) * 100.0;

            if pct_change > 5.0 {
                Some("rising".to_string())
            } else if pct_change < -5.0 {
                Some("falling".to_string())
            } else {
                Some("stable".to_string())
            }
        } else {
            None
        };

        Some(MetricStatistics {
            avg,
            min,
            max,
            count,
            trend,
        })
    }

    /// Generate mock historical data.
    fn generate_mock_history(&self, current: f64, points: usize) -> Vec<DataPoint> {
        let now = chrono::Utc::now().timestamp();
        let interval = 3600; // 1 hour

        (0..points)
            .map(|i| {
                let variation = (i as f64 - points as f64 / 2.0) * 0.3;
                DataPoint {
                    timestamp: now - (points - i) as i64 * interval,
                    value: current + variation,
                }
            })
            .collect()
    }
}

impl Default for DeviceQueryTool {
    fn default() -> Self {
        Self::mock()
    }
}

#[async_trait]
impl Tool for DeviceQueryTool {
    fn name(&self) -> &str {
        "device.query"
    }

    fn description(&self) -> &str {
        "查询设备的实时或历史数据。支持查询单个或多个指标，可指定时间范围和聚合方式。\
        查询结果包含统计信息和趋势分析提示。\
        \
        用法示例: \
        - '客厅温度多少？' → 查询当前温度 \
        - '过去24小时的温度数据' → 查询历史数据 \
        - '所有传感器数据' → 查询所有指标"
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "device_id": string_property("设备ID，支持模糊匹配。如'sensor_temp'会匹配所有包含该字符串的设备"),
                "metrics": array_property("string", "要查询的指标列表，如['temperature', 'humidity']。不指定则返回所有可用指标"),
                "time_range": object_schema(serde_json::json!({
                    "description": "时间范围",
                    "properties": {
                        "start": string_property("开始时间，支持相对时间'1h前'或时间戳"),
                        "end": string_property("结束时间，默认为当前时间")
                    }
                }), vec![]),
                "aggregation": string_property("聚合方式：'raw'原始数据、'avg'平均值、'min'最小值、'max'最大值。默认'raw'"),
                "limit": number_property("返回数据点数量限制。默认24个点")
            }),
            vec!["device_id".to_string()],
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Device
    }

    fn scenarios(&self) -> Vec<UsageScenario> {
        vec![
            UsageScenario {
                description: "用户询问当前温度".to_string(),
                example_query: "客厅温度多少？".to_string(),
                suggested_call: Some("device.query({device_id: 'sensor_temp_living', metrics: ['temperature']})".to_string()),
            },
            UsageScenario {
                description: "用户询问历史数据".to_string(),
                example_query: "过去24小时的温度数据".to_string(),
                suggested_call: Some("device.query({device_id: 'sensor_temp_living', metrics: ['temperature'], time_range: {start: '24h前'}, limit: 24})".to_string()),
            },
            UsageScenario {
                description: "用户询问所有指标".to_string(),
                example_query: "传感器有哪些数据？".to_string(),
                suggested_call: Some("device.query({device_id: 'sensor_temp_living'})".to_string()),
            },
        ]
    }

    async fn execute(&self, args: Value) -> ToolResult<ToolOutput> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments("device_id is required".to_string()))?;

        // Find device(s) - support fuzzy matching
        let devices = self.registry.get_all().await;
        let matched_devices: Vec<_> = devices
            .iter()
            .filter(|d| d.id.contains(device_id) || d.name.contains(device_id))
            .collect();

        if matched_devices.is_empty() {
            return Ok(ToolOutput::error_with_metadata(
                format!("未找到设备: {}", device_id),
                serde_json::json!({"device_id": device_id, "hint": "使用 device.discover() 查看可用设备"}),
            ));
        }

        // Use first match for now (could support multiple in future)
        let device = &matched_devices[0];

        // Get metrics to query
        let metrics_param = args.get("metrics").and_then(|v| v.as_array());
        let metrics_to_query: Vec<_> = if let Some(metrics_arr) = metrics_param {
            metrics_arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        } else {
            // Query all available metrics
            device
                .capabilities
                .metrics
                .iter()
                .map(|m| m.name.clone())
                .collect()
        };

        // Get limit
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(24) as usize;

        // Build results
        let mut results = Vec::new();

        for metric_name in &metrics_to_query {
            // Find metric info
            let metric_info = device
                .capabilities
                .metrics
                .iter()
                .find(|m| &m.name == metric_name);

            let (display_name, unit) = if let Some(info) = metric_info {
                (info.display_name.clone(), info.unit.clone())
            } else {
                (metric_name.clone(), "".to_string())
            };

            // Get current value
            let current = device
                .latest_data
                .as_ref()
                .and_then(|data| data.get(metric_name).copied());

            // Generate mock history
            let base_value = current.unwrap_or(25.0);
            let history = self.generate_mock_history(base_value, limit);

            // Calculate stats
            let stats = self.calculate_stats(&history);

            // Generate analysis hint
            let result = MetricQueryResult {
                metric: metric_name.clone(),
                display_name,
                unit,
                current,
                history,
                stats: stats.clone(),
                analysis_hint: None,
            };

            let analysis_hint = self.generate_analysis_hint(&result);

            results.push(MetricQueryResult {
                analysis_hint,
                ..result
            });
        }

        Ok(ToolOutput::success(serde_json::json!({
            "device_id": device.id,
            "device_name": device.name,
            "device_location": device.location,
            "metrics": results,
            "queried_at": chrono::Utc::now().to_rfc3339()
        })))
    }
}

// ============================================================================
// Tool 3: device.control
// ============================================================================

/// Control command types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlCommand {
    pub command: String,
    pub value: Option<Value>,
    pub parameters: Option<HashMap<String, Value>>,
}

/// Control result for a single device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlResult {
    pub device_id: String,
    pub device_name: String,
    pub success: bool,
    pub error: Option<String>,
    pub new_state: Option<Value>,
}

/// Batch control result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchControlResult {
    pub command: String,
    pub total_targets: usize,
    pub successful: usize,
    pub failed: usize,
    pub results: Vec<ControlResult>,
    pub confirmation: String,
}

/// Device control tool - controls single or multiple devices.
pub struct DeviceControlTool {
    registry: DeviceRegistryAdapter,
}

impl DeviceControlTool {
    pub fn new(registry: DeviceRegistryAdapter) -> Self {
        Self { registry }
    }

    pub fn mock() -> Self {
        Self::new(Arc::new(MockDeviceRegistry::new()))
    }

    /// Find devices matching target specification.
    async fn find_targets(&self, args: &Value) -> ToolResult<Vec<DeviceInfo>> {
        let devices = self.registry.get_all().await;

        // Method 1: Direct device_id
        if let Some(device_id) = args.get("device_id").and_then(|v| v.as_str()) {
            let filtered: Vec<_> = devices
                .iter()
                .filter(|d| d.id == device_id || d.name.contains(device_id))
                .cloned()
                .collect();
            if !filtered.is_empty() {
                return Ok(filtered);
            }
        }

        // Method 2: Multiple device_ids
        if let Some(device_ids) = args.get("device_ids").and_then(|v| v.as_array()) {
            let ids: Vec<_> = device_ids
                .iter()
                .filter_map(|v| v.as_str())
                .collect();
            let filtered: Vec<_> = devices
                .iter()
                .filter(|d| ids.iter().any(|id| &d.id == id || d.name.contains(id)))
                .cloned()
                .collect();
            if !filtered.is_empty() {
                return Ok(filtered);
            }
        }

        // Method 3: Filter by location/type
        if let Some(filter) = args.get("filter").and_then(|v| v.as_object()) {
            let device_filter = DeviceFilter {
                r#type: filter.get("type").and_then(|v| v.as_str()).map(String::from),
                location: filter.get("location").and_then(|v| v.as_str()).map(String::from),
                status: None,
                tags: None,
                name_contains: None,
            };
            return Ok(self.registry.find_by_filter(&device_filter).await);
        }

        // If no targets found and not using filter, return error
        if devices.is_empty() {
            return Err(ToolError::NotFound("No devices found".to_string()));
        }

        // Default: return all devices
        Ok(devices)
    }

    /// Execute control command on a device.
    async fn execute_command(
        &self,
        device: &DeviceInfo,
        command: &str,
        value: Option<&Value>,
        parameters: Option<&HashMap<String, Value>>,
    ) -> ControlResult {
        // Check if device supports this command
        let command_supported = device
            .capabilities
            .commands
            .iter()
            .any(|c| c.name == command);

        if !command_supported {
            return ControlResult {
                device_id: device.id.clone(),
                device_name: device.name.clone(),
                success: false,
                error: Some(format!("设备不支持命令: {}", command)),
                new_state: None,
            };
        }

        // Execute command (mock implementation)
        match command {
            "turn_on" => {
                if let Some(mut data) = device.latest_data.clone() {
                    data.insert("state".to_string(), 1.0);
                    if device.device_type == "AirConditioner" {
                        data.insert("power_state".to_string(), 1.0);
                    }
                    self.registry.update_device_data(&device.id, data).await;
                }

                ControlResult {
                    device_id: device.id.clone(),
                    device_name: device.name.clone(),
                    success: true,
                    error: None,
                    new_state: Some(serde_json::json!({"power": "on"})),
                }
            }
            "turn_off" => {
                if let Some(mut data) = device.latest_data.clone() {
                    data.insert("state".to_string(), 0.0);
                    if device.device_type == "AirConditioner" {
                        data.insert("power_state".to_string(), 0.0);
                    }
                    self.registry.update_device_data(&device.id, data).await;
                }

                ControlResult {
                    device_id: device.id.clone(),
                    device_name: device.name.clone(),
                    success: true,
                    error: None,
                    new_state: Some(serde_json::json!({"power": "off"})),
                }
            }
            "set_temperature" | "set_value" | "set_brightness" => {
                let new_value = value
                    .and_then(|v| v.as_f64())
                    .or_else(|| parameters.and_then(|p| p.get("value"))?.as_f64())
                    .unwrap_or(0.0);

                if let Some(mut data) = device.latest_data.clone() {
                    if command == "set_temperature" {
                        data.insert("target_temp".to_string(), new_value);
                    } else if command == "set_brightness" {
                        data.insert("brightness".to_string(), new_value);
                    }
                    self.registry.update_device_data(&device.id, data).await;
                }

                ControlResult {
                    device_id: device.id.clone(),
                    device_name: device.name.clone(),
                    success: true,
                    error: None,
                    new_state: Some(serde_json::json!({command: new_value})),
                }
            }
            _ => ControlResult {
                device_id: device.id.clone(),
                device_name: device.name.clone(),
                success: false,
                error: Some(format!("未知命令: {}", command)),
                new_state: None,
            },
        }
    }

    /// Generate natural language confirmation message.
    fn generate_confirmation(&self, result: &BatchControlResult) -> String {
        let command_desc = match result.command.as_str() {
            "turn_on" => "打开",
            "turn_off" => "关闭",
            "set_temperature" => "设置温度",
            "set_value" => "设置",
            "set_brightness" => "设置亮度",
            _ => "控制",
        };

        let device_names: Vec<&str> = result
            .results
            .iter()
            .filter(|r| r.success)
            .map(|r| r.device_name.as_str())
            .collect();

        if device_names.is_empty() {
            return "❌ 控制失败，没有成功执行任何设备".to_string();
        }

        if result.total_targets == 1 {
            format!(
                "✓ 已{}{}",
                command_desc,
                device_names.first().unwrap_or(&"设备")
            )
        } else {
            // Group by location for better message
            let locations: std::collections::HashSet<&str> = result
                .results
                .iter()
                .filter(|r| r.success)
                .filter_map(|r| r.device_name.split(' ').nth(1))
                .collect();

            if locations.len() > 1 {
                format!(
                    "✓ 已{}{}个设备: {}",
                    command_desc,
                    result.successful,
                    device_names.join("、")
                )
            } else {
                format!(
                    "✓ 已{}{}的{}个设备",
                    command_desc,
                    locations.iter().next().unwrap_or(&""),
                    result.successful
                )
            }
        }
    }
}

impl Default for DeviceControlTool {
    fn default() -> Self {
        Self::mock()
    }
}

#[async_trait]
impl Tool for DeviceControlTool {
    fn name(&self) -> &str {
        "device.control"
    }

    fn description(&self) -> &str {
        "控制单个或多个设备。支持通过设备ID、设备列表或过滤条件指定目标设备。\
        \
        支持的命令: \
        - turn_on/toggle: 打开设备 \
        - turn_off: 关闭设备 \
        - set_temperature: 设置空调温度 \
        - set_brightness: 设置灯光亮度 \
        - set_value: 设置通用值 \
        \
        用法示例: \
        - '打开客厅的灯' → 单设备控制 \
        - '打开所有灯' → 批量控制 \
        - '把空调设为26度' → 带参数控制"
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "device_id": string_property("单个设备ID (与device_ids/filter互斥)"),
                "device_ids": array_property("string", "多个设备ID列表 (与device_id/filter互斥)"),
                "filter": object_schema(serde_json::json!({
                    "description": "过滤条件 (与device_id/device_ids互斥)",
                    "properties": {
                        "location": string_property("位置过滤，如'客厅'"),
                        "type": string_property("设备类型过滤，如'light'")
                    }
                }), vec![]),
                "command": string_property("控制命令：turn_on, turn_off, set_temperature, set_brightness, set_value"),
                "value": object_schema(serde_json::json!({
                    "description": "命令值 (set_temperature/set_brightness等需要)"
                }), vec![]),
                "parameters": object_schema(serde_json::json!({
                    "description": "命令参数对象"
                }), vec![])
            }),
            vec!["command".to_string()],
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Device
    }

    fn scenarios(&self) -> Vec<UsageScenario> {
        vec![
            UsageScenario {
                description: "用户打开单个设备".to_string(),
                example_query: "打开客厅的灯".to_string(),
                suggested_call: Some("device.control({device_id: 'light_living_main', command: 'turn_on'})".to_string()),
            },
            UsageScenario {
                description: "用户批量控制".to_string(),
                example_query: "打开所有灯".to_string(),
                suggested_call: Some("device.control({filter: {type: 'light'}, command: 'turn_on'})".to_string()),
            },
            UsageScenario {
                description: "用户设置空调温度".to_string(),
                example_query: "把空调设为26度".to_string(),
                suggested_call: Some("device.control({device_id: 'ac_bedroom', command: 'set_temperature', value: {temperature: 26}})".to_string()),
            },
        ]
    }

    async fn execute(&self, args: Value) -> ToolResult<ToolOutput> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments("command is required".to_string()))?;

        // Find target devices
        let targets = self.find_targets(&args).await?;

        // Get value and parameters
        let value = args.get("value");
        let parameters = args
            .get("parameters")
            .and_then(|v| v.as_object())
            .map(|obj| {
                let mut map = HashMap::new();
                for (k, v) in obj {
                    map.insert(k.clone(), v.clone());
                }
                map
            });

        // Execute command on all targets
        let mut results = Vec::new();
        let mut successful = 0;
        let mut failed = 0;

        for device in targets {
            let result = self
                .execute_command(&device, command, value, parameters.as_ref())
                .await;
            if result.success {
                successful += 1;
            } else {
                failed += 1;
            }
            results.push(result);
        }

        let batch_result = BatchControlResult {
            command: command.to_string(),
            total_targets: results.len(),
            successful,
            failed,
            results,
            confirmation: String::new(), // Will be filled below
        };

        // Generate confirmation
        let confirmation = self.generate_confirmation(&batch_result);

        Ok(ToolOutput::success(serde_json::json!({
            "command": batch_result.command,
            "total_targets": batch_result.total_targets,
            "successful": batch_result.successful,
            "failed": batch_result.failed,
            "results": batch_result.results,
            "confirmation": confirmation
        })))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_mock_registry() -> Arc<MockDeviceRegistry> {
        Arc::new(MockDeviceRegistry::new())
    }

    #[tokio::test]
    async fn test_device_discover_all() {
        let registry = create_mock_registry();
        let tool = DeviceDiscoverTool::new(registry);

        let result = tool.execute(serde_json::json!({})).await.unwrap();
        assert!(result.success);

        let data = &result.data;
        assert_eq!(data["summary"]["total"], 7);
        assert_eq!(data["summary"]["online"], 6);
        assert_eq!(data["summary"]["offline"], 1);
    }

    #[tokio::test]
    async fn test_device_discover_filter_by_location() {
        let registry = create_mock_registry();
        let tool = DeviceDiscoverTool::new(registry);

        let result = tool
            .execute(serde_json::json!({
                "filter": {"location": "客厅"},
                "group_by": "type"
            }))
            .await
            .unwrap();
        assert!(result.success);

        let data = &result.data;
        assert!(data["groups"].as_array().unwrap().len() > 0);
    }

    #[tokio::test]
    async fn test_device_query_single_metric() {
        let registry = create_mock_registry();
        let tool = DeviceQueryTool::new(registry);

        let result = tool
            .execute(serde_json::json!({
                "device_id": "sensor_temp_living",
                "metrics": ["temperature"]
            }))
            .await
            .unwrap();
        assert!(result.success);

        let data = &result.data;
        assert_eq!(data["device_id"], "sensor_temp_living");
        assert!(data["metrics"].as_array().unwrap().len() > 0);
    }

    #[tokio::test]
    async fn test_device_control_single_device() {
        let registry = create_mock_registry();
        let tool = DeviceControlTool::new(registry);

        let result = tool
            .execute(serde_json::json!({
                "device_id": "light_living_main",
                "command": "turn_on"
            }))
            .await
            .unwrap();
        assert!(result.success);

        let data = &result.data;
        assert_eq!(data["successful"], 1);
        assert!(data["confirmation"].as_str().unwrap().contains("打开"));
    }

    #[tokio::test]
    async fn test_device_control_batch() {
        let registry = create_mock_registry();
        let tool = DeviceControlTool::new(registry);

        // Use location filter since multiple devices share locations
        // Or filter by device_id pattern which supports fuzzy matching
        let result = tool
            .execute(serde_json::json!({
                "device_id": "light",  // Will match all devices with "light" in id/name
                "command": "turn_on"
            }))
            .await
            .unwrap();
        assert!(result.success);

        let data = &result.data;
        assert!(data["total_targets"].as_u64().unwrap() >= 2);
    }

    #[tokio::test]
    async fn test_rule_from_context_simple() {
        let tool = RuleFromContextTool::mock();

        let result = tool
            .execute(serde_json::json!({
                "description": "温度超过50度时告警"
            }))
            .await
            .unwrap();
        assert!(result.success);

        let data = &result.data;
        assert_eq!(data["rule"]["metric"], "temperature");
        assert_eq!(data["rule"]["condition"]["operator"], ">");
        assert_eq!(data["rule"]["condition"]["threshold"], 50.0);
        assert!(data["rule"]["dsl"].as_str().unwrap().contains("RULE"));
        assert!(data["confidence"].as_f64().unwrap() > 0.5);
    }

    #[tokio::test]
    async fn test_rule_from_context_with_duration() {
        let tool = RuleFromContextTool::mock();

        let result = tool
            .execute(serde_json::json!({
                "description": "温度持续5分钟超过30度时开风扇"
            }))
            .await
            .unwrap();
        assert!(result.success);

        let data = &result.data;
        assert_eq!(data["rule"]["condition"]["threshold"], 30.0);
        assert_eq!(data["rule"]["condition"]["for_duration"], 300); // 5 minutes in seconds
        assert!(data["rule"]["actions"].as_array().unwrap().iter()
            .any(|a| a["action_type"] == "execute"));
    }

    #[tokio::test]
    async fn test_rule_from_context_humidity() {
        let tool = RuleFromContextTool::mock();

        let result = tool
            .execute(serde_json::json!({
                "description": "湿度低于30%时告警并开启加湿器"
            }))
            .await
            .unwrap();
        assert!(result.success);

        let data = &result.data;
        assert_eq!(data["rule"]["metric"], "humidity");
        assert_eq!(data["rule"]["condition"]["operator"], "<");
        assert_eq!(data["rule"]["condition"]["threshold"], 30.0);
    }
}

// ============================================================================
// Tool 4: device.analyze
// ============================================================================

/// Analysis types supported by device.analyze.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisType {
    /// Analysis type identifier
    pub analysis_type: String,
    /// Display name
    pub display_name: String,
    /// Description
    pub description: String,
}

impl AnalysisType {
    pub fn trend() -> Self {
        Self {
            analysis_type: "trend".to_string(),
            display_name: "趋势分析".to_string(),
            description: "分析数据变化趋势，识别上升、下降或稳定模式".to_string(),
        }
    }

    pub fn anomaly() -> Self {
        Self {
            analysis_type: "anomaly".to_string(),
            display_name: "异常检测".to_string(),
            description: "检测数据中的异常点和离群值".to_string(),
        }
    }

    pub fn prediction() -> Self {
        Self {
            analysis_type: "prediction".to_string(),
            display_name: "趋势预测".to_string(),
            description: "基于历史数据预测未来走势".to_string(),
        }
    }

    pub fn comparison() -> Self {
        Self {
            analysis_type: "comparison".to_string(),
            display_name: "对比分析".to_string(),
            description: "与历史同期数据进行对比".to_string(),
        }
    }

    pub fn summary() -> Self {
        Self {
            analysis_type: "summary".to_string(),
            display_name: "数据摘要".to_string(),
            description: "生成数据的统计摘要和关键洞察".to_string(),
        }
    }
}

/// Analysis result with insights and recommendations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    /// Analysis type performed
    pub analysis_type: String,
    /// Device analyzed
    pub device_id: String,
    /// Metric analyzed
    pub metric: String,
    /// Time period analyzed
    pub time_period: String,
    /// Key findings
    pub findings: Vec<String>,
    /// Insights derived from analysis
    pub insights: Vec<String>,
    /// Recommendations based on analysis
    pub recommendations: Vec<String>,
    /// Confidence score (0-1)
    pub confidence: f64,
    /// Supporting data
    pub supporting_data: Option<Value>,
}

/// Device analyze tool - uses statistical analysis to provide insights on device data.
pub struct DeviceAnalyzeTool {
    registry: DeviceRegistryAdapter,
}

impl DeviceAnalyzeTool {
    pub fn new(registry: DeviceRegistryAdapter) -> Self {
        Self { registry }
    }

    pub fn mock() -> Self {
        Self::new(Arc::new(MockDeviceRegistry::new()))
    }

    /// Perform trend analysis on metric data.
    fn analyze_trend(&self, data: &[DataPoint], metric: &MetricInfo) -> AnalysisResult {
        if data.is_empty() {
            return AnalysisResult {
                analysis_type: "trend".to_string(),
                device_id: String::new(),
                metric: metric.name.clone(),
                time_period: "无数据".to_string(),
                findings: vec!["暂无数据可分析".to_string()],
                insights: vec![],
                recommendations: vec![],
                confidence: 0.0,
                supporting_data: None,
            };
        }

        let first = data.first().unwrap().value;
        let last = data.last().unwrap().value;
        let change = last - first;
        let pct_change = if first.abs() > 0.001 {
            (change / first.abs()) * 100.0
        } else {
            0.0
        };

        let (trend_desc, color) = if pct_change > 10.0 {
            ("明显上升", "📈")
        } else if pct_change > 3.0 {
            ("缓慢上升", "📈")
        } else if pct_change < -10.0 {
            ("明显下降", "📉")
        } else if pct_change < -3.0 {
            ("缓慢下降", "📉")
        } else {
            ("保持稳定", "➡️")
        };

        let findings = vec![
            format!("{} 从 {:.1}{} 变化到 {:.1}{}", metric.display_name, first, metric.unit, last, metric.unit),
            format!("变化幅度: {:+.1}{} ({:+.1}%)", change, metric.unit, pct_change),
        ];

        let insights = vec![format!("趋势: {} {}", color, trend_desc)];

        let mut recommendations = vec![];

        // Add specific recommendations based on trend
        if metric.name.contains("temperature") || metric.name.contains("temp") {
            if pct_change > 5.0 {
                recommendations.push("温度持续上升，建议检查空调设置或通风".to_string());
                recommendations.push("考虑设置高温告警规则".to_string());
            } else if pct_change < -5.0 {
                recommendations.push("温度持续下降，注意保暖或检查加热设备".to_string());
            }
        } else if metric.name.contains("humidity") {
            if last > 70.0 {
                recommendations.push("湿度过高，建议开启除湿功能".to_string());
            } else if last < 30.0 {
                recommendations.push("湿度过低，建议开启加湿功能".to_string());
            }
        }

        AnalysisResult {
            analysis_type: "trend".to_string(),
            device_id: String::new(),
            metric: metric.name.clone(),
            time_period: format!("最近{}个数据点", data.len()),
            findings,
            insights,
            recommendations,
            confidence: if pct_change.abs() > 3.0 { 0.9 } else { 0.6 },
            supporting_data: Some(serde_json::json!({
                "first_value": first,
                "last_value": last,
                "change": change,
                "pct_change": pct_change
            })),
        }
    }

    /// Perform anomaly detection on metric data.
    fn analyze_anomaly(&self, data: &[DataPoint], metric: &MetricInfo) -> AnalysisResult {
        if data.len() < 3 {
            return AnalysisResult {
                analysis_type: "anomaly".to_string(),
                device_id: String::new(),
                metric: metric.name.clone(),
                time_period: "数据不足".to_string(),
                findings: vec!["数据点不足，无法进行异常检测".to_string()],
                insights: vec![],
                recommendations: vec![],
                confidence: 0.0,
                supporting_data: None,
            };
        }

        // Calculate mean and standard deviation
        let values: Vec<f64> = data.iter().map(|d| d.value).collect();
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        let variance = values.iter().map(|&v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
        let std_dev = variance.sqrt();

        // Define anomaly threshold (2 standard deviations)
        let threshold = 2.0 * std_dev;
        let anomalies: Vec<_> = data.iter()
            .enumerate()
            .filter(|&(_, d)| (d.value - mean).abs() > threshold)
            .map(|(i, d)| (i, d.value, d.timestamp))
            .collect();

        let mut findings = vec![
            format!("数据点总数: {}", data.len()),
            format!("平均值: {:.1}{}", mean, metric.unit),
            format!("标准差: {:.1}{}", std_dev, metric.unit),
        ];

        let mut insights = vec![];
        let mut recommendations = vec![];

        if anomalies.is_empty() {
            findings.push("未检测到异常数据点".to_string());
            insights.push("✓ 数据波动正常，无明显异常".to_string());
        } else {
            findings.push(format!("检测到 {} 个异常数据点", anomalies.len()));
            insights.push(format!("⚠️ 发现 {} 个数据点超出正常范围", anomalies.len()));

            for (i, value, _ts) in &anomalies {
                insights.push(format!("  - 数据点#{}: {:.1}{} (偏差 {:.1}σ)",
                    i + 1, value, metric.unit, (value - mean) / std_dev));
            }

            recommendations.push("建议检查设备在异常时间点的运行状态".to_string());
            if metric.name.contains("temperature") {
                recommendations.push("考虑设置温度异常告警规则".to_string());
            }
        }

        AnalysisResult {
            analysis_type: "anomaly".to_string(),
            device_id: String::new(),
            metric: metric.name.clone(),
            time_period: format!("最近{}个数据点", data.len()),
            findings,
            insights,
            recommendations,
            confidence: if anomalies.is_empty() { 0.95 } else { 0.85 },
            supporting_data: Some(serde_json::json!({
                "mean": mean,
                "std_dev": std_dev,
                "anomaly_count": anomalies.len(),
                "anomalies": anomalies
            })),
        }
    }

    /// Perform summary analysis on metric data.
    fn analyze_summary(&self, data: &[DataPoint], metric: &MetricInfo) -> AnalysisResult {
        if data.is_empty() {
            return AnalysisResult {
                analysis_type: "summary".to_string(),
                device_id: String::new(),
                metric: metric.name.clone(),
                time_period: "无数据".to_string(),
                findings: vec!["暂无数据".to_string()],
                insights: vec![],
                recommendations: vec![],
                confidence: 0.0,
                supporting_data: None,
            };
        }

        let values: Vec<f64> = data.iter().map(|d| d.value).collect();
        let min = values.iter().fold(f64::INFINITY, |a, &b| a.min(b));
        let max = values.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
        let avg = values.iter().sum::<f64>() / values.len() as f64;
        let current = values.last().unwrap();

        let range = max - min;

        let findings = vec![
            format!("当前值: {:.1}{}", current, metric.unit),
            format!("平均值: {:.1}{}", avg, metric.unit),
            format!("范围: {:.1}{} - {:.1}{} (波动 {:.1}{})", min, metric.unit, max, metric.unit, range, metric.unit),
            format!("数据点数: {}", data.len()),
        ];

        let mut insights = vec![
            format!("{} 当前处于 {:.1}{}", metric.display_name, current, metric.unit),
        ];

        if range > avg * 0.3 {
            insights.push(format!("数据波动较大 ({:.1}{}， {:.1}% 平均值)",
                range, metric.unit, (range / avg) * 100.0));
        } else {
            insights.push(format!("数据波动较小 ({:.1}{}， {:.1}% 平均值)",
                range, metric.unit, (range / avg) * 100.0));
        }

        let mut recommendations = vec![];

        // Contextual recommendations
        if metric.name.contains("temperature") {
            if *current > 28.0 {
                recommendations.push("当前温度较高，建议开启空调或风扇".to_string());
            } else if *current < 18.0 {
                recommendations.push("当前温度较低，建议开启暖气或加热设备".to_string());
            }
        } else if metric.name.contains("humidity") {
            if *current > 70.0 {
                recommendations.push("湿度较高，建议开启除湿功能".to_string());
            } else if *current < 30.0 {
                recommendations.push("湿度较低，建议开启加湿功能".to_string());
            }
        }

        AnalysisResult {
            analysis_type: "summary".to_string(),
            device_id: String::new(),
            metric: metric.name.clone(),
            time_period: format!("最近{}个数据点", data.len()),
            findings,
            insights,
            recommendations,
            confidence: 1.0,
            supporting_data: Some(serde_json::json!({
                "min": min,
                "max": max,
                "avg": avg,
                "current": current,
                "range": range
            })),
        }
    }
}

impl Default for DeviceAnalyzeTool {
    fn default() -> Self {
        Self::mock()
    }
}

#[async_trait]
impl Tool for DeviceAnalyzeTool {
    fn name(&self) -> &str {
        "device.analyze"
    }

    fn description(&self) -> &str {
        "使用LLM分析设备数据，发现趋势、异常、模式和预测。支持趋势分析、异常检测、数据摘要等多种分析类型。\
        \
        用法示例: \
        - '分析温度趋势' → 分析温度变化趋势 \
        - '检测异常数据' → 检测数据中的异常点 \
        - '数据摘要' → 生成统计摘要和洞察"
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "device_id": string_property("设备ID，支持模糊匹配"),
                "metric": string_property("要分析的指标名称，如'temperature'。不指定则分析所有指标"),
                "analysis_type": string_property("分析类型：'trend'趋势分析、'anomaly'异常检测、'summary'数据摘要。默认'summary'"),
                "limit": number_property("要分析的数据点数量，默认24")
            }),
            vec!["device_id".to_string()],
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Device
    }

    fn scenarios(&self) -> Vec<UsageScenario> {
        vec![
            UsageScenario {
                description: "用户询问数据趋势".to_string(),
                example_query: "分析温度趋势".to_string(),
                suggested_call: Some("device.analyze({device_id: 'sensor_temp_living', metric: 'temperature', analysis_type: 'trend'})".to_string()),
            },
            UsageScenario {
                description: "用户要求检测异常".to_string(),
                example_query: "检测异常数据".to_string(),
                suggested_call: Some("device.analyze({device_id: 'sensor_temp_living', analysis_type: 'anomaly'})".to_string()),
            },
            UsageScenario {
                description: "用户要求分析数据".to_string(),
                example_query: "分析一下传感器数据".to_string(),
                suggested_call: Some("device.analyze({device_id: 'sensor_temp_living', analysis_type: 'summary'})".to_string()),
            },
        ]
    }

    async fn execute(&self, args: Value) -> ToolResult<ToolOutput> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments("device_id is required".to_string()))?;

        // Find device
        let devices = self.registry.get_all().await;
        let device = devices
            .iter()
            .find(|d| d.id.contains(device_id) || d.name.contains(device_id))
            .ok_or_else(|| ToolError::NotFound(format!("Device '{}' not found", device_id)))?;

        // Get analysis type
        let analysis_type = args
            .get("analysis_type")
            .and_then(|v| v.as_str())
            .unwrap_or("summary");

        // Get metric filter
        let metric_filter = args.get("metric").and_then(|v| v.as_str());

        // Get limit
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(24) as usize;

        // Filter metrics
        let metrics_to_analyze: Vec<_> = if let Some(filter) = metric_filter {
            device.capabilities.metrics.iter()
                .filter(|m| m.name == filter)
                .collect()
        } else {
            device.capabilities.metrics.iter().collect()
        };

        if metrics_to_analyze.is_empty() {
            return Ok(ToolOutput::error_with_metadata(
                format!("设备 '{}' 没有指标 '{}'", device.name, metric_filter.unwrap_or("")),
                serde_json::json!({"available_metrics": device.capabilities.metrics.iter().map(|m| &m.name).collect::<Vec<_>>()}),
            ));
        }

        // Generate mock data for analysis
        let base_value = device.latest_data.as_ref()
            .and_then(|data| {
                metrics_to_analyze.first().and_then(|m| data.get(&m.name).copied())
            })
            .unwrap_or(25.0);

        let data_points: Vec<DataPoint> = (0..limit)
            .map(|i| {
                let variation = (i as f64 - limit as f64 / 2.0) * 0.5;
                DataPoint {
                    timestamp: chrono::Utc::now().timestamp() - (limit - i) as i64 * 3600,
                    value: base_value + variation,
                }
            })
            .collect();

        // Perform analysis
        let result = match analysis_type {
            "trend" => self.analyze_trend(&data_points, metrics_to_analyze.first().unwrap()),
            "anomaly" => self.analyze_anomaly(&data_points, metrics_to_analyze.first().unwrap()),
            _ => self.analyze_summary(&data_points, metrics_to_analyze.first().unwrap()),
        };

        let mut result = result;
        result.device_id = device.id.clone();

        Ok(ToolOutput::success(serde_json::to_value(result).unwrap()))
    }
}

// ============================================================================
// Rule From Context Tool
// ============================================================================

/// Rule definition extracted from natural language context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedRuleDefinition {
    /// Generated rule name.
    pub name: String,
    /// Rule description.
    pub description: String,
    /// Device ID for the condition.
    pub device_id: String,
    /// Metric name to monitor.
    pub metric: String,
    /// Comparison operator.
    pub operator: String,
    /// Threshold value.
    pub threshold: f64,
    /// Duration for FOR clause (seconds).
    pub for_duration: Option<u64>,
    /// Actions to execute.
    pub actions: Vec<RuleActionDef>,
    /// Generated DSL.
    pub dsl: String,
    /// Confidence score (0-1).
    pub confidence: f64,
    /// Any missing information.
    pub missing_info: Vec<String>,
}

/// Rule action definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleActionDef {
    /// Action type: notify, execute, log.
    pub action_type: String,
    /// Action parameters.
    pub params: serde_json::Value,
}

/// Tool for creating rules from natural language context.
///
/// This tool extracts rule information from natural language descriptions
/// and generates structured rule definitions with DSL.
pub struct RuleFromContextTool {
    registry: DeviceRegistryAdapter,
}

impl RuleFromContextTool {
    /// Create a new rule from context tool.
    pub fn new(registry: DeviceRegistryAdapter) -> Self {
        Self { registry }
    }

    /// Create with a mock registry for testing.
    pub fn mock() -> Self {
        Self::new(Arc::new(MockDeviceRegistry::new()))
    }

    /// Extract rule information from natural language description.
    fn extract_from_description(&self, description: &str) -> ExtractedRuleDefinition {
        let desc_lower = description.to_lowercase();

        // Extract threshold value
        let threshold = self.extract_threshold(&desc_lower);

        // Extract operator
        let operator = self.extract_operator(&desc_lower);

        // Extract device/metric
        let (device_id, metric) = self.extract_device_metric(&desc_lower);

        // Extract actions
        let actions = self.extract_actions(&desc_lower);

        // Extract duration
        let for_duration = self.extract_duration(&desc_lower);

        // Generate rule name
        let name = self.generate_rule_name(&metric, threshold, &operator);

        // Generate description
        let description_text = format!("当{}{}{}时触发", metric, operator, threshold);

        // Generate DSL
        let dsl = self.generate_dsl(&name, &device_id, &metric, &operator, threshold, for_duration.as_ref(), &actions);

        // Check for missing info
        let mut missing_info = vec![];
        if device_id.is_empty() {
            missing_info.push("设备ID未指定".to_string());
        }
        if metric.is_empty() {
            missing_info.push("监控指标未指定".to_string());
        }
        if actions.is_empty() {
            missing_info.push("触发动作未指定".to_string());
        }

        let confidence = if missing_info.is_empty() { 0.9 } else { 0.5 };

        ExtractedRuleDefinition {
            name,
            description: description_text,
            device_id,
            metric,
            operator,
            threshold,
            for_duration,
            actions,
            dsl,
            confidence,
            missing_info,
        }
    }

    /// Extract threshold value from description.
    fn extract_threshold(&self, desc: &str) -> f64 {
        // Look for numeric patterns
        let re = regex::Regex::new(r"(\d+\.?\d*)\s*(度|°|℃|%|摄氏度|百分比)").unwrap();
        if let Some(caps) = re.captures(desc) {
            return caps.get(1).and_then(|m| m.as_str().parse().ok()).unwrap_or(50.0);
        }

        // Simple number extraction
        let re = regex::Regex::new(r"(\d+\.?\d*)").unwrap();
        re.captures(desc)
            .and_then(|c| c.get(1))
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(50.0)
    }

    /// Extract comparison operator from description.
    fn extract_operator(&self, desc: &str) -> String {
        if desc.contains("超过") || desc.contains("大于") || desc.contains("高于") {
            ">".to_string()
        } else if desc.contains("低于") || desc.contains("小于") || desc.contains("少于") {
            "<".to_string()
        } else if desc.contains("至少") || desc.contains("大于等于") {
            ">=".to_string()
        } else if desc.contains("不超过") || desc.contains("小于等于") {
            "<=".to_string()
        } else {
            ">".to_string()  // Default
        }
    }

    /// Extract device and metric from description.
    fn extract_device_metric(&self, desc: &str) -> (String, String) {
        // Metric patterns
        if desc.contains("温度") {
            ("sensor_temp_living".to_string(), "temperature".to_string())
        } else if desc.contains("湿度") {
            ("sensor_humidity_living".to_string(), "humidity".to_string())
        } else if desc.contains("光") || desc.contains("亮度") {
            ("sensor_light_living".to_string(), "illuminance".to_string())
        } else if desc.contains("二氧化碳") || desc.contains("CO2") {
            ("sensor_co2_living".to_string(), "co2".to_string())
        } else {
            ("sensor_temp_living".to_string(), "temperature".to_string())  // Default
        }
    }

    /// Extract actions from description.
    fn extract_actions(&self, desc: &str) -> Vec<RuleActionDef> {
        let mut actions = vec![];

        // Check for notification
        if desc.contains("告警") || desc.contains("通知") || desc.contains("提醒") {
            actions.push(RuleActionDef {
                action_type: "notify".to_string(),
                params: serde_json::json!({"message": "规则触发告警"}),
            });
        }

        // Check for device control
        if desc.contains("开风扇") || desc.contains("启动风扇") {
            actions.push(RuleActionDef {
                action_type: "execute".to_string(),
                params: serde_json::json!({"device": "fan_living", "command": "turn_on"}),
            });
        } else if desc.contains("开空调") {
            actions.push(RuleActionDef {
                action_type: "execute".to_string(),
                params: serde_json::json!({"device": "ac_living", "command": "turn_on"}),
            });
        } else if desc.contains("开灯") {
            actions.push(RuleActionDef {
                action_type: "execute".to_string(),
                params: serde_json::json!({"device": "light_living", "command": "turn_on"}),
            });
        } else if desc.contains("关") {
            actions.push(RuleActionDef {
                action_type: "execute".to_string(),
                params: serde_json::json!({"device": "unknown", "command": "turn_off"}),
            });
        }

        // Default: add notify if no actions found
        if actions.is_empty() {
            actions.push(RuleActionDef {
                action_type: "notify".to_string(),
                params: serde_json::json!({"message": "规则已触发"}),
            });
        }

        actions
    }

    /// Extract duration from description.
    fn extract_duration(&self, desc: &str) -> Option<u64> {
        let re = regex::Regex::new(r"(\d+)\s*(秒|second|分钟|minute|小时|hour)").unwrap();
        if let Some(caps) = re.captures(desc) {
            let value = caps.get(1).and_then(|m| m.as_str().parse().ok()).unwrap_or(1);
            let unit = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            match unit {
                "秒" | "second" | "seconds" => Some(value),
                "分钟" | "minute" | "minutes" => Some(value * 60),
                "小时" | "hour" | "hours" => Some(value * 3600),
                _ => None,
            }
        } else if desc.contains("持续") {
            Some(300)  // Default 5 minutes
        } else {
            None
        }
    }

    /// Generate rule name from components.
    fn generate_rule_name(&self, metric: &str, threshold: f64, operator: &str) -> String {
        let op_text = match operator {
            ">" => "超过",
            "<" => "低于",
            ">=" => "达到",
            "<=" => "不超过",
            _ => "变化",
        };
        format!("{}{}{}告警", metric, op_text, threshold)
    }

    /// Generate DSL from components.
    fn generate_dsl(
        &self,
        name: &str,
        device_id: &str,
        metric: &str,
        operator: &str,
        threshold: f64,
        for_duration: Option<&u64>,
        actions: &[RuleActionDef],
    ) -> String {
        let mut dsl = format!("RULE \"{}\"\n", name);

        // WHEN clause
        dsl.push_str(&format!("WHEN {}.{} {} {}\n", device_id, metric, operator, threshold));

        // FOR clause
        if let Some(duration) = for_duration {
            let (value, unit) = if *duration >= 3600 {
                (*duration / 3600, "hours")
            } else if *duration >= 60 {
                (*duration / 60, "minutes")
            } else {
                (*duration, "seconds")
            };
            dsl.push_str(&format!("FOR {} {}\n", value, unit));
        }

        // DO clause
        dsl.push_str("DO\n");
        for action in actions {
            match action.action_type.as_str() {
                "notify" => {
                    let msg = action.params.get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("规则触发");
                    dsl.push_str(&format!("  NOTIFY \"{}\"\n", msg));
                }
                "execute" => {
                    let device = action.params.get("device")
                        .and_then(|d| d.as_str())
                        .unwrap_or("unknown");
                    let cmd = action.params.get("command")
                        .and_then(|c| c.as_str())
                        .unwrap_or("turn_on");
                    dsl.push_str(&format!("  EXECUTE {}.{}\n", device, cmd));
                }
                "log" => {
                    dsl.push_str("  LOG alert\n");
                }
                _ => {}
            }
        }
        dsl.push_str("END");

        dsl
    }
}

impl Default for RuleFromContextTool {
    fn default() -> Self {
        Self::mock()
    }
}

#[async_trait]
impl Tool for RuleFromContextTool {
    fn name(&self) -> &str {
        "rule.from_context"
    }

    fn description(&self) -> &str {
        "从自然语言描述中提取规则信息，生成结构化的规则定义和DSL。\
        \
        支持从对话上下文中理解用户意图，自动提取：\
        - 监控设备和指标（如：温度、湿度）\
        - 触发条件（如：超过50度）\
        - 持续时间（如：持续5分钟）\
        - 触发动作（如：告警、开启设备）\
        \
        用法示例: \
        - '温度超过50度时告警' → 生成高温告警规则 \
        - '温度持续5分钟超过30度时开风扇' → 生成带持续时间的规则"
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "description": string_property("自然语言规则描述，如'温度超过50度时告警'"),
                "context_devices": array_property("string", "可选：上下文中的设备ID列表，用于验证"),
                "confirm": boolean_property("是否确认创建规则，默认false仅预览")
            }),
            vec!["description".to_string()],
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Rule
    }

    fn scenarios(&self) -> Vec<UsageScenario> {
        vec![
            UsageScenario {
                description: "从自然语言创建高温告警规则".to_string(),
                example_query: "温度超过50度时告警".to_string(),
                suggested_call: Some(r#"{"name": "rule.from_context", "arguments": {"description": "温度超过50度时告警"}}"#.to_string()),
            },
            UsageScenario {
                description: "创建带持续时间和动作的规则".to_string(),
                example_query: "温度持续5分钟超过30度时开风扇".to_string(),
                suggested_call: Some(r#"{"name": "rule.from_context", "arguments": {"description": "温度持续5分钟超过30度时开风扇"}}"#.to_string()),
            },
            UsageScenario {
                description: "创建多动作规则".to_string(),
                example_query: "湿度低于30%时告警并开启加湿器".to_string(),
                suggested_call: Some(r#"{"name": "rule.from_context", "arguments": {"description": "湿度低于30%时告警并开启加湿器"}}"#.to_string()),
            },
        ]
    }

    fn relationships(&self) -> ToolRelationships {
        ToolRelationships {
            call_after: vec!["device.discover".to_string()],
            output_to: vec!["create_rule".to_string()],
            exclusive_with: vec![],
        }
    }

    async fn execute(&self, args: Value) -> ToolResult<ToolOutput> {
        self.validate_args(&args)?;

        let description = args["description"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("description must be a string".to_string()))?;

        // Extract rule from description
        let mut rule_def = self.extract_from_description(description);

        // Validate device context if provided
        if let Some(devices) = args.get("context_devices").and_then(|d| d.as_array()) {
            let device_ids: Vec<&str> = devices.iter()
                .filter_map(|d| d.as_str())
                .collect();
            if !device_ids.contains(&rule_def.device_id.as_str()) {
                rule_def.missing_info.push(format!("警告: 指定设备 '{}' 不在上下文设备列表中", rule_def.device_id));
                rule_def.confidence = (rule_def.confidence - 0.2).max(0.1);
            }
        }

        // Format response
        let response = serde_json::json!({
            "rule": {
                "name": rule_def.name,
                "description": rule_def.description,
                "dsl": rule_def.dsl,
                "device_id": rule_def.device_id,
                "metric": rule_def.metric,
                "condition": {
                    "operator": rule_def.operator,
                    "threshold": rule_def.threshold,
                    "for_duration": rule_def.for_duration,
                },
                "actions": rule_def.actions,
            },
            "confidence": rule_def.confidence,
            "missing_info": rule_def.missing_info,
            "next_steps": if rule_def.missing_info.is_empty() {
                vec!["确认无误后，调用 create_rule 创建规则".to_string()]
            } else {
                vec![format!("请补充缺失信息: {}", rule_def.missing_info.join(", "))]
            }
        });

        Ok(ToolOutput::success(response))
    }
}
