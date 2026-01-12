//! Scenario manager for creating and managing scenarios.
//!
//! The scenario manager provides CRUD operations for scenarios
//! and integrates with devices and rules.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use chrono::Utc;

use super::scenario::{
    Scenario, ScenarioId, ScenarioTemplate, ScenarioTemplates, ScenarioMetadata,
    ScenarioCategory, Environment,
};
use super::error::{Error, Result};

/// Manager for scenarios.
pub struct ScenarioManager {
    /// All scenarios by ID
    scenarios: Arc<RwLock<HashMap<ScenarioId, Scenario>>>,
    /// Scenarios indexed by name
    by_name: Arc<RwLock<HashMap<String, ScenarioId>>>,
    /// Scenarios indexed by tag
    by_tag: Arc<RwLock<HashMap<String, Vec<ScenarioId>>>>,
}

impl ScenarioManager {
    /// Create a new scenario manager.
    pub fn new() -> Self {
        Self {
            scenarios: Arc::new(RwLock::new(HashMap::new())),
            by_name: Arc::new(RwLock::new(HashMap::new())),
            by_tag: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new scenario.
    pub async fn create_scenario(
        &self,
        name: String,
        description: String,
    ) -> Result<Scenario> {
        let scenario = Scenario::new(name, description);
        self.add_scenario(scenario.clone()).await?;
        Ok(scenario)
    }

    /// Create a scenario from a template.
    pub async fn create_from_template(
        &self,
        template: ScenarioTemplate,
        custom_name: Option<String>,
    ) -> Result<Scenario> {
        let mut scenario = template.create_scenario(custom_name);
        scenario.id = ScenarioId::new();
        scenario.created_at = Utc::now();
        scenario.updated_at = Utc::now();
        self.add_scenario(scenario.clone()).await?;
        Ok(scenario)
    }

    /// Add a scenario to the manager.
    pub async fn add_scenario(&self, scenario: Scenario) -> Result<()> {
        let id = scenario.id.clone();
        let name = scenario.name.clone();
        let tags = scenario.metadata.tags.clone();

        // Remove old scenario with same name if exists
        if let Some(old_id) = self.by_name.read().await.get(&name) {
            self.remove_scenario(old_id).await.ok();
        }

        // Add to main storage
        self.scenarios.write().await.insert(id.clone(), scenario.clone());

        // Update name index
        self.by_name.write().await.insert(name, id.clone());

        // Update tag index
        let mut by_tag = self.by_tag.write().await;
        for tag in tags {
            by_tag.entry(tag).or_insert_with(Vec::new).push(id.clone());
        }

        Ok(())
    }

    /// Get a scenario by ID.
    pub async fn get_scenario(&self, id: &ScenarioId) -> Option<Scenario> {
        self.scenarios.read().await.get(id).cloned()
    }

    /// Get a scenario by name.
    pub async fn get_by_name(&self, name: &str) -> Option<Scenario> {
        if let Some(id) = self.by_name.read().await.get(name) {
            self.get_scenario(id).await
        } else {
            None
        }
    }

    /// List all scenarios.
    pub async fn list_scenarios(&self) -> Vec<Scenario> {
        self.scenarios
            .read()
            .await
            .values()
            .cloned()
            .collect()
    }

    /// List active scenarios only.
    pub async fn list_active(&self) -> Vec<Scenario> {
        self.scenarios
            .read()
            .await
            .values()
            .filter(|s| s.is_active)
            .cloned()
            .collect()
    }

    /// List scenarios by category.
    pub async fn list_by_category(&self, category: &ScenarioCategory) -> Vec<Scenario> {
        self.scenarios
            .read()
            .await
            .values()
            .filter(|s| &s.metadata.category == category)
            .cloned()
            .collect()
    }

    /// List scenarios by environment.
    pub async fn list_by_environment(&self, environment: &Environment) -> Vec<Scenario> {
        self.scenarios
            .read()
            .await
            .values()
            .filter(|s| &s.metadata.environment == environment)
            .cloned()
            .collect()
    }

    /// List scenarios by tag.
    pub async fn list_by_tag(&self, tag: &str) -> Vec<Scenario> {
        let by_tag = self.by_tag.read().await;
        if let Some(ids) = by_tag.get(tag) {
            let scenarios = self.scenarios.read().await;
            ids.iter()
                .filter_map(|id| scenarios.get(id).cloned())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Update a scenario.
    pub async fn update_scenario(&self, id: &ScenarioId, mut scenario: Scenario) -> Result<Scenario> {
        if !self.scenarios.read().await.contains_key(id) {
            return Err(Error::NotFound(format!("Scenario not found: {}", id)));
        }

        scenario.id = id.clone();
        scenario.updated_at = Utc::now();

        self.scenarios.write().await.insert(id.clone(), scenario.clone());

        // Update indices
        self.by_name.write().await.insert(scenario.name.clone(), id.clone());

        Ok(scenario)
    }

    /// Update scenario metadata.
    pub async fn update_metadata(
        &self,
        id: &ScenarioId,
        metadata: ScenarioMetadata,
    ) -> Result<()> {
        let mut scenarios = self.scenarios.write().await;
        if let Some(scenario) = scenarios.get_mut(id) {
            scenario.metadata = metadata;
            scenario.updated_at = Utc::now();
            Ok(())
        } else {
            Err(Error::NotFound(format!("Scenario not found: {}", id)))
        }
    }

    /// Activate a scenario.
    pub async fn activate(&self, id: &ScenarioId) -> Result<()> {
        let mut scenarios = self.scenarios.write().await;
        if let Some(scenario) = scenarios.get_mut(id) {
            scenario.is_active = true;
            scenario.updated_at = Utc::now();
            Ok(())
        } else {
            Err(Error::NotFound(format!("Scenario not found: {}", id)))
        }
    }

    /// Deactivate a scenario.
    pub async fn deactivate(&self, id: &ScenarioId) -> Result<()> {
        let mut scenarios = self.scenarios.write().await;
        if let Some(scenario) = scenarios.get_mut(id) {
            scenario.is_active = false;
            scenario.updated_at = Utc::now();
            Ok(())
        } else {
            Err(Error::NotFound(format!("Scenario not found: {}", id)))
        }
    }

    /// Remove a scenario.
    pub async fn remove_scenario(&self, id: &ScenarioId) -> Result<()> {
        let scenario = self.scenarios.write().await.remove(id);
        if let Some(scenario) = scenario {
            // Remove from name index
            self.by_name.write().await.remove(&scenario.name);

            // Remove from tag indices
            let mut by_tag = self.by_tag.write().await;
            for tag in &scenario.metadata.tags {
                if let Some(ids) = by_tag.get_mut(tag) {
                    ids.retain(|id| *id != scenario.id);
                }
            }

            Ok(())
        } else {
            Err(Error::NotFound(format!("Scenario not found: {}", id)))
        }
    }

    /// Add a device to a scenario.
    pub async fn add_device_to_scenario(
        &self,
        id: &ScenarioId,
        device_id: String,
    ) -> Result<()> {
        let mut scenarios = self.scenarios.write().await;
        if let Some(scenario) = scenarios.get_mut(id) {
            scenario.add_device(device_id);
            Ok(())
        } else {
            Err(Error::NotFound(format!("Scenario not found: {}", id)))
        }
    }

    /// Remove a device from a scenario.
    pub async fn remove_device_from_scenario(
        &self,
        id: &ScenarioId,
        device_id: &str,
    ) -> Result<()> {
        let mut scenarios = self.scenarios.write().await;
        if let Some(scenario) = scenarios.get_mut(id) {
            scenario.remove_device(device_id);
            Ok(())
        } else {
            Err(Error::NotFound(format!("Scenario not found: {}", id)))
        }
    }

    /// Add a rule to a scenario.
    pub async fn add_rule_to_scenario(
        &self,
        id: &ScenarioId,
        rule_id: String,
    ) -> Result<()> {
        let mut scenarios = self.scenarios.write().await;
        if let Some(scenario) = scenarios.get_mut(id) {
            scenario.add_rule(rule_id);
            Ok(())
        } else {
            Err(Error::NotFound(format!("Scenario not found: {}", id)))
        }
    }

    /// Remove a rule from a scenario.
    pub async fn remove_rule_from_scenario(
        &self,
        id: &ScenarioId,
        rule_id: &str,
    ) -> Result<()> {
        let mut scenarios = self.scenarios.write().await;
        if let Some(scenario) = scenarios.get_mut(id) {
            scenario.remove_rule(rule_id);
            Ok(())
        } else {
            Err(Error::NotFound(format!("Scenario not found: {}", id)))
        }
    }

    /// Get all scenarios containing a specific device.
    pub async fn get_scenarios_with_device(&self, device_id: &str) -> Vec<Scenario> {
        self.scenarios
            .read()
            .await
            .values()
            .filter(|s| s.devices.contains(&device_id.to_string()))
            .cloned()
            .collect()
    }

    /// Get all scenarios containing a specific rule.
    pub async fn get_scenarios_with_rule(&self, rule_id: &str) -> Vec<Scenario> {
        self.scenarios
            .read()
            .await
            .values()
            .filter(|s| s.rules.contains(&rule_id.to_string()))
            .cloned()
            .collect()
    }

    /// Generate an LLM prompt for a scenario.
    pub async fn get_llm_prompt(&self, id: &ScenarioId) -> Result<String> {
        if let Some(scenario) = self.get_scenario(id).await {
            Ok(scenario.to_llm_prompt())
        } else {
            Err(Error::NotFound(format!("Scenario not found: {}", id)))
        }
    }

    /// Get scenario statistics.
    pub async fn get_stats(&self) -> ScenarioStats {
        let scenarios = self.scenarios.read().await;
        let total = scenarios.len();
        let active = scenarios.values().filter(|s| s.is_active).count();

        let mut by_category: HashMap<String, usize> = HashMap::new();
        let mut by_environment: HashMap<String, usize> = HashMap::new();

        for scenario in scenarios.values() {
            *by_category
                .entry(format!("{}", scenario.metadata.category))
                .or_insert(0) += 1;
            *by_environment
                .entry(format!("{}", scenario.metadata.environment))
                .or_insert(0) += 1;
        }

        ScenarioStats {
            total,
            active,
            inactive: total - active,
            by_category,
            by_environment,
        }
    }

    /// Initialize with default templates.
    pub async fn init_with_templates(&self) -> Result<()> {
        let templates = vec![
            ScenarioTemplates::datacenter_temperature(),
            ScenarioTemplates::office_energy_saving(),
            ScenarioTemplates::production_quality(),
            ScenarioTemplates::smart_home_comfort(),
        ];

        for template in templates {
            self.create_from_template(template, None).await?;
        }

        Ok(())
    }
}

impl Default for ScenarioManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Scenario statistics.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScenarioStats {
    /// Total number of scenarios
    pub total: usize,
    /// Number of active scenarios
    pub active: usize,
    /// Number of inactive scenarios
    pub inactive: usize,
    /// Scenarios by category
    pub by_category: HashMap<String, usize>,
    /// Scenarios by environment
    pub by_environment: HashMap<String, usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_scenario_manager_creation() {
        let manager = ScenarioManager::new();

        let scenario = manager
            .create_scenario("Test".to_string(), "Description".to_string())
            .await
            .unwrap();

        assert_eq!(scenario.name, "Test");
        assert!(manager.get_scenario(&scenario.id).await.is_some());
    }

    #[tokio::test]
    async fn test_get_by_name() {
        let manager = ScenarioManager::new();

        let scenario = manager
            .create_scenario("Unique Name".to_string(), "Description".to_string())
            .await
            .unwrap();

        let found = manager.get_by_name("Unique Name").await;
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, scenario.id);
    }

    #[tokio::test]
    async fn test_list_scenarios() {
        let manager = ScenarioManager::new();

        manager
            .create_scenario("Scenario 1".to_string(), "Desc 1".to_string())
            .await
            .unwrap();
        manager
            .create_scenario("Scenario 2".to_string(), "Desc 2".to_string())
            .await
            .unwrap();

        let all = manager.list_scenarios().await;
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_activate_deactivate() {
        let manager = ScenarioManager::new();

        let scenario = manager
            .create_scenario("Test".to_string(), "Desc".to_string())
            .await
            .unwrap();

        manager.deactivate(&scenario.id).await.unwrap();
        let s = manager.get_scenario(&scenario.id).await.unwrap();
        assert!(!s.is_active);

        manager.activate(&scenario.id).await.unwrap();
        let s = manager.get_scenario(&scenario.id).await.unwrap();
        assert!(s.is_active);
    }

    #[tokio::test]
    async fn test_add_device_to_scenario() {
        let manager = ScenarioManager::new();

        let scenario = manager
            .create_scenario("Test".to_string(), "Desc".to_string())
            .await
            .unwrap();

        manager
            .add_device_to_scenario(&scenario.id, "device1".to_string())
            .await
            .unwrap();

        let s = manager.get_scenario(&scenario.id).await.unwrap();
        assert_eq!(s.devices.len(), 1);
        assert_eq!(s.devices[0], "device1");
    }

    #[tokio::test]
    async fn test_create_from_template() {
        let manager = ScenarioManager::new();

        let scenario = manager
            .create_from_template(
                ScenarioTemplates::datacenter_temperature(),
                Some("My Datacenter".to_string()),
            )
            .await
            .unwrap();

        assert_eq!(scenario.name, "My Datacenter");
        assert_eq!(scenario.metadata.category, ScenarioCategory::Monitoring);
        assert_eq!(scenario.metadata.environment, Environment::DataCenter);
    }

    #[tokio::test]
    async fn test_list_by_category() {
        let manager = ScenarioManager::new();

        let s1 = manager
            .create_scenario("Monitoring".to_string(), "Desc".to_string())
            .await
            .unwrap();
        let s1_id = s1.id.clone();
        manager
            .update_metadata(&s1_id, ScenarioMetadata {
                category: ScenarioCategory::Monitoring,
                ..Default::default()
            })
            .await
            .unwrap();

        let s2 = manager
            .create_scenario("Automation".to_string(), "Desc".to_string())
            .await
            .unwrap();
        let s2_id = s2.id.clone();
        manager
            .update_metadata(&s2_id, ScenarioMetadata {
                category: ScenarioCategory::Automation,
                ..Default::default()
            })
            .await
            .unwrap();

        let monitoring = manager.list_by_category(&ScenarioCategory::Monitoring).await;
        assert_eq!(monitoring.len(), 1);

        let automation = manager.list_by_category(&ScenarioCategory::Automation).await;
        assert_eq!(automation.len(), 1);
    }

    #[tokio::test]
    async fn test_get_stats() {
        let manager = ScenarioManager::new();

        manager
            .create_scenario("Scenario 1".to_string(), "Desc".to_string())
            .await
            .unwrap();

        let mut s2 = manager
            .create_scenario("Scenario 2".to_string(), "Desc".to_string())
            .await
            .unwrap();
        manager.deactivate(&s2.id).await.unwrap();

        let stats = manager.get_stats().await;
        assert_eq!(stats.total, 2);
        assert_eq!(stats.active, 1);
        assert_eq!(stats.inactive, 1);
    }
}
