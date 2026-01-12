//! Multi-Broker Manager
//!
//! Manages multiple MQTT broker connections (both internal and external).
//! Each broker has its own MqttDeviceManager instance that handles
//! device discovery, metric updates, and commands.

use crate::mqtt_v2::{MqttDeviceManager, MqttManagerConfig};
use crate::DeviceError;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Multi-Broker Manager
///
/// Manages multiple MQTT broker connections, allowing devices to connect
/// through different brokers (internal, external, cloud, etc.).
pub struct MultiBrokerManager {
    /// Map of broker_id -> MqttDeviceManager
    brokers: Arc<RwLock<HashMap<String, Arc<MqttDeviceManager>>>>,

    /// Storage directory for persistence
    storage_dir: Option<String>,
}

impl MultiBrokerManager {
    /// Create a new multi-broker manager
    pub fn new() -> Self {
        Self {
            brokers: Arc::new(RwLock::new(HashMap::new())),
            storage_dir: None,
        }
    }

    /// Set storage directory for all broker managers
    pub fn with_storage_dir(mut self, dir: impl Into<String>) -> Self {
        self.storage_dir = Some(dir.into());
        self
    }

    /// Add a broker connection
    ///
    /// # Arguments
    /// * `broker_id` - Unique identifier for this broker
    /// * `config` - MQTT broker configuration
    pub async fn add_broker(
        &self,
        broker_id: impl Into<String>,
        config: MqttManagerConfig,
    ) -> Result<(), DeviceError> {
        let broker_id = broker_id.into();
        let mut managers = self.brokers.write().await;

        // Check if broker already exists
        if managers.contains_key(&broker_id) {
            return Err(DeviceError::AlreadyExists(format!(
                "Broker already exists: {}",
                broker_id
            )));
        }

        // Create new MqttDeviceManager for this broker
        let mut manager = MqttDeviceManager::new(broker_id.clone(), config);
        if let Some(ref dir) = self.storage_dir {
            manager = manager.with_storage_dir(dir);
        }

        // Initialize the manager
        manager.initialize().await?;

        let manager = Arc::new(manager);
        managers.insert(broker_id.clone(), manager.clone());

        tracing::info!(
            broker_id = %broker_id,
            "Added broker connection"
        );

        Ok(())
    }

    /// Remove a broker connection
    ///
    /// This will disconnect the broker and clean up resources.
    pub async fn remove_broker(&self, broker_id: &str) -> Result<(), DeviceError> {
        let mut managers = self.brokers.write().await;

        if let Some(manager) = managers.remove(broker_id) {
            // Disconnect the broker
            manager.disconnect().await?;
            tracing::info!(
                broker_id = %broker_id,
                "Removed broker connection"
            );
            Ok(())
        } else {
            Err(DeviceError::NotFoundStr(format!(
                "Broker not found: {}",
                broker_id
            )))
        }
    }

    /// Get a broker by ID
    pub async fn get_broker(&self, broker_id: &str) -> Option<Arc<MqttDeviceManager>> {
        self.brokers.read().await.get(broker_id).cloned()
    }

    /// Get all broker IDs
    pub async fn list_brokers(&self) -> Vec<String> {
        self.brokers.read().await.keys().cloned().collect()
    }

    /// Connect all brokers
    pub async fn connect_all(&self) -> Result<(), DeviceError> {
        let managers = self.brokers.read().await;
        let mut errors = Vec::new();

        for (broker_id, manager) in managers.iter() {
            if let Err(e) = manager.connect().await {
                errors.push(format!("{}: {}", broker_id, e));
            }
        }

        if !errors.is_empty() {
            tracing::warn!(
                errors = %errors.join(", "),
                "Some brokers failed to connect"
            );
        }

        Ok(())
    }

    /// Disconnect all brokers
    pub async fn disconnect_all(&self) {
        let managers = self.brokers.read().await;
        for manager in managers.values() {
            let _ = manager.disconnect().await;
        }
    }

    /// Get the number of active brokers
    pub async fn broker_count(&self) -> usize {
        self.brokers.read().await.len()
    }

    /// Get connection status for all brokers
    pub async fn get_all_status(&self) -> HashMap<String, String> {
        let managers = self.brokers.read().await;
        let mut status = HashMap::new();

        for (broker_id, manager) in managers.iter() {
            let connection_status = manager.connection_status().await;
            status.insert(
                broker_id.clone(),
                format!("{:?}", connection_status),
            );
        }

        status
    }

    /// Start a specific broker connection
    pub async fn start_broker(&self, broker_id: &str) -> Result<(), DeviceError> {
        let manager = self
            .brokers
            .read()
            .await
            .get(broker_id)
            .cloned()
            .ok_or_else(|| DeviceError::NotFoundStr(format!("Broker not found: {}", broker_id)))?;

        manager.connect().await?;
        tracing::info!(broker_id = %broker_id, "Started broker connection");
        Ok(())
    }

    /// Stop a specific broker connection
    pub async fn stop_broker(&self, broker_id: &str) -> Result<(), DeviceError> {
        let manager = self
            .brokers
            .read()
            .await
            .get(broker_id)
            .cloned()
            .ok_or_else(|| DeviceError::NotFoundStr(format!("Broker not found: {}", broker_id)))?;

        manager.disconnect().await?;
        tracing::info!(broker_id = %broker_id, "Stopped broker connection");
        Ok(())
    }
}

impl Default for MultiBrokerManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_add_broker() {
        let manager = MultiBrokerManager::new();
        let config = MqttManagerConfig::new("localhost");

        assert!(manager.add_broker("test-1", config.clone()).await.is_ok());
        assert!(manager.list_brokers().await.contains(&"test-1".to_string()));
        assert_eq!(manager.broker_count().await, 1);
    }

    #[tokio::test]
    async fn test_duplicate_broker() {
        let manager = MultiBrokerManager::new();
        let config = MqttManagerConfig::new("localhost");

        assert!(manager.add_broker("test-1", config.clone()).await.is_ok());
        assert!(manager
            .add_broker("test-1", config)
            .await
            .is_err());

        assert_eq!(manager.broker_count().await, 1);
    }

    #[tokio::test]
    async fn test_remove_broker() {
        let manager = MultiBrokerManager::new();
        let config = MqttManagerConfig::new("localhost");

        manager.add_broker("test-1", config).await.unwrap();
        assert_eq!(manager.broker_count().await, 1);

        assert!(manager.remove_broker("test-1").await.is_ok());
        assert_eq!(manager.broker_count().await, 0);
    }
}
