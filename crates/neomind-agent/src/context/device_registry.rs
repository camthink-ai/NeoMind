//! Device registry with alias and location mapping.
//!
//! This module provides natural language device name resolution,
//! enabling the LLM to understand "客厅灯" → "light_living_1".

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Device alias mapping for natural language understanding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceAlias {
    /// Human-readable name
    pub name: String,
    /// Internal device ID
    pub device_id: String,
    /// Device type (sensor, switch, etc.)
    pub device_type: String,
    /// Location (living room, bedroom, etc.)
    pub location: Option<String>,
    /// Device capabilities (temperature, humidity, on/off, etc.)
    pub capabilities: Vec<String>,
    /// Aliases for fuzzy matching
    pub aliases: Vec<String>,
    /// Whether this device was matched in a query
    #[serde(skip)]
    pub matched: bool,
}

impl DeviceAlias {
    /// Create a new device alias.
    pub fn new(
        name: impl Into<String>,
        device_id: impl Into<String>,
        device_type: impl Into<String>,
    ) -> Self {
        let name = name.into();
        let device_id = device_id.into();
        let device_type = device_type.into();

        Self {
            name: name.clone(),
            device_id,
            device_type,
            location: None,
            capabilities: Vec::new(),
            aliases: vec![name.clone()],
            matched: false,
        }
    }

    /// Add location.
    pub fn with_location(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }

    /// Add capability.
    pub fn with_capability(mut self, capability: impl Into<String>) -> Self {
        self.capabilities.push(capability.into());
        self
    }

    /// Add alias.
    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    /// Check if this device matches a query term.
    pub fn matches(&self, term: &str) -> bool {
        let term_lower = term.to_lowercase();

        // Check name
        if self.name.to_lowercase().contains(&term_lower) {
            return true;
        }

        // Check device_id
        if self.device_id.to_lowercase().contains(&term_lower) {
            return true;
        }

        // Check aliases
        for alias in &self.aliases {
            if alias.to_lowercase().contains(&term_lower) {
                return true;
            }
        }

        // Check location
        if let Some(loc) = &self.location {
            if loc.to_lowercase().contains(&term_lower) {
                return true;
            }
        }

        false
    }

    /// Check if this device has a specific capability.
    pub fn has_capability(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|c| {
            c.to_lowercase() == capability.to_lowercase()
                || c.to_lowercase()
                    .contains(capability.to_lowercase().as_str())
        })
    }
}

/// Device capability descriptor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCapability {
    /// Capability name
    pub name: String,
    /// Capability type
    pub capability_type: CapabilityType,
    /// Valid values/range
    pub values: Option<CapabilityValues>,
}

/// Type of device capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilityType {
    /// Metric - readable value (temperature, humidity, etc.)
    Metric,
    /// Command - writable action (on/off, set value, etc.)
    Command,
    /// Property - readable/writable state
    Property,
}

/// Valid values for a capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CapabilityValues {
    /// Boolean (on/off, true/false)
    Bool,
    /// Numeric range
    Range {
        min: f64,
        max: f64,
        step: Option<f64>,
    },
    /// Enum values
    Enum(Vec<String>),
    /// Any string
    String,
}

/// Device location for grouping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceLocation {
    /// Location name
    pub name: String,
    /// Location type (room, zone, etc.)
    pub location_type: String,
    /// Parent location (for hierarchical locations)
    pub parent: Option<String>,
}

/// Device registry for managing device aliases and mappings.
pub struct DeviceRegistry {
    /// All registered devices
    devices: Arc<RwLock<HashMap<String, DeviceAlias>>>,
    /// Location index
    location_index: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// Capability index
    capability_index: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// Common aliases for capability names
    capability_aliases: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl DeviceRegistry {
    /// Create a new device registry.
    pub fn new() -> Self {
        let registry = Self {
            devices: Arc::new(RwLock::new(HashMap::new())),
            location_index: Arc::new(RwLock::new(HashMap::new())),
            capability_index: Arc::new(RwLock::new(HashMap::new())),
            capability_aliases: Arc::new(RwLock::new(Self::default_capability_aliases())),
        };

        // Initialize common aliases
        tokio::spawn(async move {
            // Would load from persistent storage in production
        });

        registry
    }

    /// Default capability aliases for natural language matching.
    fn default_capability_aliases() -> HashMap<String, Vec<String>> {
        let mut aliases = HashMap::new();

        // Temperature aliases
        aliases.insert(
            "temperature".to_string(),
            vec!["温度".to_string(), "气温".to_string(), "temp".to_string()],
        );

        // Humidity aliases
        aliases.insert(
            "humidity".to_string(),
            vec!["湿度".to_string(), "humid".to_string()],
        );

        // Light/on-off aliases
        aliases.insert(
            "power".to_string(),
            vec![
                "电源".to_string(),
                "开关".to_string(),
                "on".to_string(),
                "off".to_string(),
                "打开".to_string(),
                "关闭".to_string(),
            ],
        );

        // Brightness aliases
        aliases.insert(
            "brightness".to_string(),
            vec!["亮度".to_string(), "明暗".to_string(), "bright".to_string()],
        );

        aliases
    }

    /// Register a device.
    pub async fn register(&self, device: DeviceAlias) -> Result<(), String> {
        let device_id = device.device_id.clone();

        // Update devices map
        {
            let mut devices = self.devices.write().await;
            devices.insert(device_id.clone(), device.clone());
        }

        // Update location index
        if let Some(ref location) = device.location {
            let mut index = self.location_index.write().await;
            index
                .entry(location.clone())
                .or_insert_with(Vec::new)
                .push(device_id.clone());
        }

        // Update capability index
        for cap in &device.capabilities {
            let mut index = self.capability_index.write().await;
            index
                .entry(cap.clone())
                .or_insert_with(Vec::new)
                .push(device_id.clone());
        }

        Ok(())
    }

    /// Resolve device IDs from a natural language query.
    pub fn resolve_from_query(&self, _query: &str) -> Vec<DeviceAlias> {
        // This is a synchronous method that reads from the Arc
        // In async context, we'd use self.devices.read().await
        // For now, return empty and make the async version
        Vec::new()
    }

    /// Resolve devices from query (async version).
    pub async fn resolve_from_query_async(&self, query: &str) -> Vec<DeviceAlias> {
        let devices = self.devices.read().await;
        let mut resolved = Vec::new();

        let query_lower = query.to_lowercase();

        // First pass: direct matches
        for device in devices.values() {
            if device.matches(&query_lower) {
                let mut d = device.clone();
                d.matched = true;
                resolved.push(d);
            }
        }

        // Second pass: capability-based inference
        if resolved.is_empty() {
            // Check for capability keywords
            let aliases = self.capability_aliases.read().await;
            for (capability, alias_list) in aliases.iter() {
                for alias in alias_list {
                    if query_lower.contains(alias) {
                        // Find devices with this capability
                        let capability_devices: Vec<_> = devices
                            .values()
                            .filter(|d| d.has_capability(capability))
                            .map(|d| {
                                let mut dev = d.clone();
                                dev.matched = true;
                                dev
                            })
                            .collect();

                        resolved.extend(capability_devices);
                        break;
                    }
                }
            }
        }

        resolved
    }

    /// Get device by ID.
    pub async fn get(&self, device_id: &str) -> Option<DeviceAlias> {
        let devices = self.devices.read().await;
        devices.get(device_id).cloned()
    }

    /// Get all devices.
    pub async fn list(&self) -> Vec<DeviceAlias> {
        let devices = self.devices.read().await;
        devices.values().cloned().collect()
    }

    /// Get devices by location.
    pub async fn list_by_location(&self, location: &str) -> Vec<DeviceAlias> {
        let devices = self.devices.read().await;
        devices
            .values()
            .filter(|d| d.location.as_ref().is_some_and(|l| l == location))
            .cloned()
            .collect()
    }

    /// Get devices by capability.
    pub async fn list_by_capability(&self, capability: &str) -> Vec<DeviceAlias> {
        let devices = self.devices.read().await;

        // Check both direct capability and aliases
        let mut result: Vec<DeviceAlias> = devices
            .values()
            .filter(|d| d.has_capability(capability))
            .cloned()
            .collect();

        if result.is_empty() {
            let aliases = self.capability_aliases.read().await;
            for (cap, alias_list) in aliases.iter() {
                if alias_list.iter().any(|a| a == capability) {
                    result = devices
                        .values()
                        .filter(|d| d.has_capability(cap))
                        .cloned()
                        .collect();
                    break;
                }
            }
        }

        result
    }

    /// Add capability alias.
    pub async fn add_capability_alias(&self, capability: String, alias: String) {
        let mut aliases = self.capability_aliases.write().await;
        aliases
            .entry(capability)
            .or_insert_with(Vec::new)
            .push(alias);
    }
}

impl Default for DeviceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test helper: Initialize with sample devices for testing.
    async fn init_sample_devices(registry: &DeviceRegistry) {
        let sample_devices = vec![
            DeviceAlias::new("客厅温度传感器", "sensor_1", "dht22_sensor")
                .with_location("客厅")
                .with_capability("temperature")
                .with_capability("humidity")
                .with_alias("温度传感器")
                .with_alias("温度"),
            DeviceAlias::new("客厅灯", "light_living_1", "switch")
                .with_location("客厅")
                .with_capability("power")
                .with_capability("brightness")
                .with_alias("灯")
                .with_alias("客厅灯"),
            DeviceAlias::new("卧室温度传感器", "sensor_2", "dht22_sensor")
                .with_location("卧室")
                .with_capability("temperature")
                .with_capability("humidity")
                .with_alias("温度传感器")
                .with_alias("温度"),
            DeviceAlias::new("卧室灯", "light_bedroom_1", "switch")
                .with_location("卧室")
                .with_capability("power")
                .with_capability("brightness")
                .with_alias("灯")
                .with_alias("卧室灯"),
            DeviceAlias::new("厨房湿度传感器", "sensor_3", "dht22_sensor")
                .with_location("厨房")
                .with_capability("humidity")
                .with_alias("湿度传感器")
                .with_alias("湿度"),
        ];

        for device in sample_devices {
            registry.register(device).await.ok();
        }
    }

    #[tokio::test]
    async fn test_device_registry() {
        let registry = DeviceRegistry::new();

        let device = DeviceAlias::new("测试设备", "test_1", "sensor")
            .with_location("客厅")
            .with_capability("temperature");

        registry.register(device).await.unwrap();

        let retrieved = registry.get("test_1").await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().location, Some("客厅".to_string()));
    }

    #[tokio::test]
    async fn test_list_by_location() {
        let registry = DeviceRegistry::new();
        init_sample_devices(&registry).await;

        let living_room = registry.list_by_location("客厅").await;
        assert!(!living_room.is_empty());
        assert!(living_room
            .iter()
            .all(|d| d.location == Some("客厅".to_string())));
    }

    #[tokio::test]
    async fn test_list_by_capability() {
        let registry = DeviceRegistry::new();
        init_sample_devices(&registry).await;

        let temp_sensors = registry.list_by_capability("temperature").await;
        assert!(!temp_sensors.is_empty());
    }

    #[tokio::test]
    async fn test_resolve_from_query() {
        let registry = DeviceRegistry::new();
        init_sample_devices(&registry).await;

        // Test temperature query
        let results = registry.resolve_from_query_async("温度").await;
        assert!(!results.is_empty());

        // Test specific location
        let results = registry.resolve_from_query_async("客厅灯").await;
        assert!(!results.is_empty());
    }
}
