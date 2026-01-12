//! Device manager for centralized device lifecycle management.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::mdl::{
    DeviceId, DynDevice, DeviceError, DeviceInfo,
};

/// Device manager that handles device registration, discovery, and access.
pub struct DeviceManager {
    /// Registered devices indexed by ID
    devices: Arc<RwLock<HashMap<DeviceId, DynDevice>>>,
    /// Name to ID mapping for quick lookup
    name_index: Arc<RwLock<HashMap<String, DeviceId>>>,
}

impl Default for DeviceManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceManager {
    /// Create a new device manager.
    pub fn new() -> Self {
        Self {
            devices: Arc::new(RwLock::new(HashMap::new())),
            name_index: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a new device.
    pub async fn register(&self, device: DynDevice) -> Result<(), DeviceError> {
        let id = device.id().clone();
        let name = device.name().to_string();

        let mut devices = self.devices.write().await;
        let mut name_index = self.name_index.write().await;

        if devices.contains_key(&id) {
            return Err(DeviceError::InvalidParameter(format!(
                "Device with ID {} already registered",
                id
            )));
        }

        devices.insert(id.clone(), device);
        name_index.insert(name, id);

        Ok(())
    }

    /// Unregister a device by ID.
    pub async fn unregister(&self, id: &DeviceId) -> Result<DynDevice, DeviceError> {
        let mut devices = self.devices.write().await;

        let device = devices
            .remove(id)
            .ok_or_else(|| DeviceError::NotFound(id.clone()))?;

        // Remove from name index
        let mut name_index = self.name_index.write().await;
        name_index.remove(device.name());

        Ok(device)
    }

    /// Get a device by ID.
    pub async fn get(&self, id: &DeviceId) -> Option<DynDevice> {
        let devices = self.devices.read().await;
        devices.get(id).cloned()
    }

    /// Get a device by name.
    pub async fn get_by_name(&self, name: &str) -> Option<DynDevice> {
        let id = {
            let name_index = self.name_index.read().await;
            name_index.get(name).cloned()
        };
        if let Some(id) = id {
            self.get(&id).await
        } else {
            None
        }
    }

    /// List all registered devices.
    pub async fn list(&self) -> Vec<DeviceInfo> {
        let devices = self.devices.read().await;
        let mut result = Vec::new();

        for device in devices.values() {
            result.push(device.info());
        }

        result
    }

    /// List devices by type.
    pub async fn list_by_type(&self, device_type: super::mdl::DeviceType) -> Vec<DeviceInfo> {
        let devices = self.devices.read().await;
        devices
            .values()
            .filter(|d| d.device_type() == device_type)
            .map(|d| d.info())
            .collect()
    }

    /// List connected devices only.
    pub async fn list_connected(&self) -> Vec<DeviceInfo> {
        let devices = self.devices.read().await;
        let mut result = Vec::new();

        for device in devices.values() {
            if device.is_connected().await {
                result.push(device.info());
            }
        }

        result
    }

    /// Get the total number of registered devices.
    pub async fn count(&self) -> usize {
        let devices = self.devices.read().await;
        devices.len()
    }

    /// Connect to a device by ID.
    pub async fn connect(&self, id: &DeviceId) -> Result<(), DeviceError> {
        // Check if device exists
        let _devices = self.devices.read().await;

        if !_devices.contains_key(id) {
            return Err(DeviceError::NotFound(id.clone()));
        }

        // The device should handle its own connection state
        // This is a limitation of the current design - we'd need interior mutability

        Ok(())
    }

    /// Disconnect from a device by ID.
    pub async fn disconnect(&self, id: &DeviceId) -> Result<(), DeviceError> {
        // Check if device exists
        let _device = self
            .get(id)
            .await
            .ok_or_else(|| DeviceError::NotFound(id.clone()))?;

        // Similar limitation as connect()
        // Device should manage its own connection

        Ok(())
    }

    /// Read a metric from a device.
    pub async fn read_metric(
        &self,
        device_id: &DeviceId,
        metric: &str,
    ) -> Result<super::mdl::MetricValue, DeviceError> {
        let device = self
            .get(device_id)
            .await
            .ok_or_else(|| DeviceError::NotFound(device_id.clone()))?;

        device.read_metric(metric).await
    }

    /// Write a command to a device.
    pub async fn write_command(
        &self,
        device_id: &DeviceId,
        command: &super::mdl::Command,
    ) -> Result<Option<super::mdl::MetricValue>, DeviceError> {
        let device = self
            .get(device_id)
            .await
            .ok_or_else(|| DeviceError::NotFound(device_id.clone()))?;

        device.write_command(command).await
    }

    /// Read a metric from a device by name.
    pub async fn read_metric_by_name(
        &self,
        device_name: &str,
        metric: &str,
    ) -> Result<super::mdl::MetricValue, DeviceError> {
        let device = self
            .get_by_name(device_name)
            .await
            .ok_or_else(|| DeviceError::InvalidParameter(format!(
                "Device not found: {}",
                device_name
            )))?;

        device.read_metric(metric).await
    }

    /// Get device state.
    pub async fn get_state(&self, id: &DeviceId) -> Result<super::mdl::DeviceState, DeviceError> {
        let device = self
            .get(id)
            .await
            .ok_or_else(|| DeviceError::NotFound(id.clone()))?;

        Ok(device.state().await)
    }

    /// Check if a device exists.
    pub async fn exists(&self, id: &DeviceId) -> bool {
        let devices = self.devices.read().await;
        devices.contains_key(id)
    }

    /// Clear all registered devices.
    pub async fn clear(&self) {
        let mut devices = self.devices.write().await;
        let mut name_index = self.name_index.write().await;
        devices.clear();
        name_index.clear();
    }
}

/// Device group for organizing related devices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceGroup {
    /// Unique group identifier
    pub id: String,
    /// Group name
    pub name: String,
    /// Group description
    pub description: Option<String>,
    /// Device IDs in this group
    pub devices: Vec<DeviceId>,
    /// Group metadata
    pub metadata: HashMap<String, String>,
}

impl DeviceGroup {
    /// Create a new device group.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: None,
            devices: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Add a device to the group.
    pub fn add_device(&mut self, device_id: DeviceId) {
        if !self.devices.contains(&device_id) {
            self.devices.push(device_id);
        }
    }

    /// Remove a device from the group.
    pub fn remove_device(&mut self, device_id: &DeviceId) {
        self.devices.retain(|id| id != device_id);
    }

    /// Set the description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }
}

/// Group manager for organizing devices into logical groups.
pub struct GroupManager {
    groups: Arc<RwLock<HashMap<String, DeviceGroup>>>,
}

impl Default for GroupManager {
    fn default() -> Self {
        Self::new()
    }
}

impl GroupManager {
    pub fn new() -> Self {
        Self {
            groups: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new group.
    pub async fn create(&self, group: DeviceGroup) -> Result<(), DeviceError> {
        let mut groups = self.groups.write().await;
        groups.insert(group.id.clone(), group);
        Ok(())
    }

    /// Get a group by ID.
    pub async fn get(&self, id: &str) -> Option<DeviceGroup> {
        let groups = self.groups.read().await;
        groups.get(id).cloned()
    }

    /// List all groups.
    pub async fn list(&self) -> Vec<DeviceGroup> {
        let groups = self.groups.read().await;
        groups.values().cloned().collect()
    }

    /// Delete a group.
    pub async fn delete(&self, id: &str) -> Result<(), DeviceError> {
        let mut groups = self.groups.write().await;
        groups
            .remove(id)
            .ok_or_else(|| DeviceError::InvalidParameter(format!("Group not found: {}", id)))?;
        Ok(())
    }

    /// Add a device to a group.
    pub async fn add_device_to_group(
        &self,
        group_id: &str,
        device_id: DeviceId,
    ) -> Result<(), DeviceError> {
        let mut groups = self.groups.write().await;
        let group = groups
            .get_mut(group_id)
            .ok_or_else(|| DeviceError::InvalidParameter(format!("Group not found: {}", group_id)))?;
        group.add_device(device_id);
        Ok(())
    }

    /// Remove a device from a group.
    pub async fn remove_device_from_group(
        &self,
        group_id: &str,
        device_id: &DeviceId,
    ) -> Result<(), DeviceError> {
        let mut groups = self.groups.write().await;
        let group = groups
            .get_mut(group_id)
            .ok_or_else(|| DeviceError::InvalidParameter(format!("Group not found: {}", group_id)))?;
        group.remove_device(device_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mqtt::MqttDevice;
    use crate::mdl::{MetricDefinition, MetricDataType};

    #[tokio::test]
    async fn test_device_manager_register() {
        let manager = DeviceManager::new();

        let metrics = vec![MetricDefinition {
            name: "temperature".to_string(),
            description: "Temperature".to_string(),
            data_type: MetricDataType::Float,
            unit: Some("°C".to_string()),
            read_only: true,
            min: None,
            max: None,
        }];

        let device = Arc::new(MqttDevice::sensor(
            "TestDevice",
            "localhost",
            "test",
            metrics,
        ));

        manager.register(device).await.unwrap();
        assert_eq!(manager.count().await, 1);

        let found = manager.get_by_name("TestDevice").await;
        assert!(found.is_some());
    }

    #[tokio::test]
    async fn test_device_manager_list() {
        let manager = DeviceManager::new();

        let metrics = vec![MetricDefinition {
            name: "value".to_string(),
            description: "Value".to_string(),
            data_type: MetricDataType::Integer,
            unit: None,
            read_only: true,
            min: None,
            max: None,
        }];

        let device1 = Arc::new(MqttDevice::sensor("Device1", "localhost", "d1", metrics.clone()));
        let device2 = Arc::new(MqttDevice::sensor("Device2", "localhost", "d2", metrics));

        manager.register(device1).await.unwrap();
        manager.register(device2).await.unwrap();

        let list = manager.list().await;
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn test_group_manager() {
        let group_mgr = GroupManager::new();

        let group = DeviceGroup::new("group1", "Test Group")
            .with_description("A test group");

        group_mgr.create(group).await.unwrap();

        let retrieved = group_mgr.get("group1").await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "Test Group");

        let list = group_mgr.list().await;
        assert_eq!(list.len(), 1);
    }
}
