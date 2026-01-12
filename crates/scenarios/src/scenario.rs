//! Scenario definitions for the NeoTalk platform.
//!
//! Scenarios represent business contexts that group devices and rules together,
//! making it easier for LLMs to understand and operate on device combinations.

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};

/// Unique identifier for a scenario.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScenarioId(pub Uuid);

impl ScenarioId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn from_string(s: &str) -> Result<Self, uuid::Error> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl Default for ScenarioId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ScenarioId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Scenario category.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScenarioCategory {
    /// Monitoring scenarios - observe and track device states
    Monitoring,
    /// Alert scenarios - trigger notifications based on conditions
    Alert,
    /// Automation scenarios - automatic responses to events
    Automation,
    /// Reporting scenarios - generate reports and analytics
    Reporting,
    /// Control scenarios - direct device control
    Control,
    /// Optimization scenarios - optimize device operations
    Optimization,
}

impl std::fmt::Display for ScenarioCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Monitoring => write!(f, "监控"),
            Self::Alert => write!(f, "告警"),
            Self::Automation => write!(f, "自动化"),
            Self::Reporting => write!(f, "报表"),
            Self::Control => write!(f, "控制"),
            Self::Optimization => write!(f, "优化"),
        }
    }
}

/// Environment type for the scenario.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Environment {
    /// Office building environment
    Office,
    /// Factory/Industrial environment
    Factory,
    /// Data center environment
    DataCenter,
    /// Smart home environment
    SmartHome,
    /// Outdoor/Field environment
    Outdoor,
    /// Laboratory environment
    Laboratory,
    /// Other/custom environment
    Other(String),
}

impl std::fmt::Display for Environment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Office => write!(f, "办公楼"),
            Self::Factory => write!(f, "工厂"),
            Self::DataCenter => write!(f, "数据中心"),
            Self::SmartHome => write!(f, "智能家居"),
            Self::Outdoor => write!(f, "户外"),
            Self::Laboratory => write!(f, "实验室"),
            Self::Other(s) => write!(f, "{}", s),
        }
    }
}

/// Additional metadata for a scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioMetadata {
    /// Category of the scenario
    pub category: ScenarioCategory,
    /// Environment type
    pub environment: Environment,
    /// Business description
    pub business_context: String,
    /// Additional tags for organization
    pub tags: Vec<String>,
    /// Priority level (1-10, higher = more important)
    pub priority: u8,
}

impl Default for ScenarioMetadata {
    fn default() -> Self {
        Self {
            category: ScenarioCategory::Monitoring,
            environment: Environment::Other(String::new()),
            business_context: String::new(),
            tags: Vec::new(),
            priority: 5,
        }
    }
}

/// A scenario representing a business context with devices and rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    /// Unique scenario identifier
    pub id: ScenarioId,
    /// Scenario name
    pub name: String,
    /// Description
    pub description: String,
    /// Associated device IDs
    pub devices: Vec<String>,
    /// Associated rule IDs (as strings for flexibility)
    pub rules: Vec<String>,
    /// Additional metadata
    pub metadata: ScenarioMetadata,
    /// When the scenario was created
    pub created_at: DateTime<Utc>,
    /// When the scenario was last updated
    pub updated_at: DateTime<Utc>,
    /// Whether the scenario is active
    pub is_active: bool,
}

impl Scenario {
    /// Create a new scenario.
    pub fn new(name: String, description: String) -> Self {
        let now = Utc::now();
        Self {
            id: ScenarioId::new(),
            name,
            description,
            devices: Vec::new(),
            rules: Vec::new(),
            metadata: ScenarioMetadata::default(),
            created_at: now,
            updated_at: now,
            is_active: true,
        }
    }

    /// Add a device to the scenario.
    pub fn add_device(&mut self, device_id: String) {
        if !self.devices.contains(&device_id) {
            self.devices.push(device_id);
            self.updated_at = Utc::now();
        }
    }

    /// Remove a device from the scenario.
    pub fn remove_device(&mut self, device_id: &str) {
        self.devices.retain(|d| d != device_id);
        self.updated_at = Utc::now();
    }

    /// Add a rule to the scenario.
    pub fn add_rule(&mut self, rule_id: String) {
        if !self.rules.contains(&rule_id) {
            self.rules.push(rule_id);
            self.updated_at = Utc::now();
        }
    }

    /// Remove a rule from the scenario.
    pub fn remove_rule(&mut self, rule_id: &str) {
        self.rules.retain(|r| r != rule_id);
        self.updated_at = Utc::now();
    }

    /// Set the metadata for the scenario.
    pub fn set_metadata(&mut self, metadata: ScenarioMetadata) {
        self.metadata = metadata;
        self.updated_at = Utc::now();
    }

    /// Generate an LLM-friendly description of this scenario.
    pub fn to_llm_prompt(&self) -> String {
        let mut prompt = format!(
            "## 场景: {}\n\
            **描述**: {}\n\
            **类型**: {}\n\
            **环境**: {}\n\
            **业务背景**: {}\n\
            **优先级**: {}/10\n\n",
            self.name,
            self.description,
            self.metadata.category,
            self.metadata.environment,
            self.metadata.business_context,
            self.metadata.priority
        );

        if !self.devices.is_empty() {
            prompt.push_str(&format!("**包含设备** ({}个):\n", self.devices.len()));
            for device in &self.devices {
                prompt.push_str(&format!("  - {}\n", device));
            }
            prompt.push('\n');
        }

        if !self.rules.is_empty() {
            prompt.push_str(&format!("**关联规则** ({}个):\n", self.rules.len()));
            for rule in &self.rules {
                prompt.push_str(&format!("  - {}\n", rule));
            }
            prompt.push('\n');
        }

        if !self.metadata.tags.is_empty() {
            prompt.push_str(&format!("**标签**: {}\n", self.metadata.tags.join(", ")));
        }

        prompt
    }

    /// Get a summary of the scenario.
    pub fn summary(&self) -> String {
        format!(
            "{} [{}]: {} 设备, {} 规则",
            self.name,
            self.metadata.category,
            self.devices.len(),
            self.rules.len()
        )
    }
}

/// Template for creating common scenarios.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioTemplate {
    /// Template name
    pub name: String,
    /// Template description
    pub description: String,
    /// Default category
    pub category: ScenarioCategory,
    /// Default environment
    pub environment: Environment,
    /// Business context template
    pub business_context_template: String,
    /// Suggested device types
    pub suggested_device_types: Vec<String>,
    /// Tags to apply
    pub tags: Vec<String>,
}

impl ScenarioTemplate {
    /// Create a scenario from this template.
    pub fn create_scenario(&self, custom_name: Option<String>) -> Scenario {
        let name = custom_name.unwrap_or_else(|| self.name.clone());
        let mut scenario = Scenario::new(name, self.description.clone());
        scenario.metadata = ScenarioMetadata {
            category: self.category.clone(),
            environment: self.environment.clone(),
            business_context: self.business_context_template.clone(),
            tags: self.tags.clone(),
            priority: 5,
        };
        scenario
    }
}

/// Predefined scenario templates.
pub struct ScenarioTemplates;

impl ScenarioTemplates {
    /// Data center temperature monitoring scenario.
    pub fn datacenter_temperature() -> ScenarioTemplate {
        ScenarioTemplate {
            name: "数据中心温度监控".to_string(),
            description: "监控数据中心机房的温度，防止过热导致设备故障".to_string(),
            category: ScenarioCategory::Monitoring,
            environment: Environment::DataCenter,
            business_context_template: "数据中心机房温度监控，当温度超过阈值时触发告警并启动空调设备".to_string(),
            suggested_device_types: vec!["temperature_sensor".to_string(), "hvac_controller".to_string()],
            tags: vec!["数据中心".to_string(), "温度监控".to_string(), "环境监控".to_string()],
        }
    }

    /// Office energy saving scenario.
    pub fn office_energy_saving() -> ScenarioTemplate {
        ScenarioTemplate {
            name: "办公室节能控制".to_string(),
            description: "基于时间和人员密度自动调节办公室照明和空调".to_string(),
            category: ScenarioCategory::Automation,
            environment: Environment::Office,
            business_context_template: "非工作时间自动关闭照明和调低空调温度，工作时间根据人员密度智能调节".to_string(),
            suggested_device_types: vec!["light_controller".to_string(), "hvac_controller".to_string(), "occupancy_sensor".to_string()],
            tags: vec!["办公室".to_string(), "节能".to_string(), "自动化".to_string()],
        }
    }

    /// Production line quality monitoring scenario.
    pub fn production_quality() -> ScenarioTemplate {
        ScenarioTemplate {
            name: "生产线质量监控".to_string(),
            description: "监控生产线设备状态和产品质量指标".to_string(),
            category: ScenarioCategory::Alert,
            environment: Environment::Factory,
            business_context_template: "实时监控生产线关键参数，异常时立即停机并通知相关人员".to_string(),
            suggested_device_types: vec!["quality_sensor".to_string(), "conveyor_controller".to_string(), "alert_system".to_string()],
            tags: vec!["工厂".to_string(), "质量控制".to_string(), "生产监控".to_string()],
        }
    }

    /// Smart home comfort scenario.
    pub fn smart_home_comfort() -> ScenarioTemplate {
        ScenarioTemplate {
            name: "智能家居舒适度控制".to_string(),
            description: "根据用户习惯和实时环境自动调节家居设备".to_string(),
            category: ScenarioCategory::Automation,
            environment: Environment::SmartHome,
            business_context_template: "学习用户习惯，自动调节温度、照明和窗帘，提供舒适的居住环境".to_string(),
            suggested_device_types: vec!["thermostat".to_string(), "smart_light".to_string(), "smart_blind".to_string()],
            tags: vec!["智能家居".to_string(), "舒适度".to_string(), "自动化".to_string()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scenario_id() {
        let id = ScenarioId::new();
        assert_eq!(id.0.get_version(), Some(uuid::Version::Random));
    }

    #[test]
    fn test_scenario_creation() {
        let scenario = Scenario::new(
            "Test Scenario".to_string(),
            "A test scenario".to_string(),
        );

        assert_eq!(scenario.name, "Test Scenario");
        assert_eq!(scenario.description, "A test scenario");
        assert!(scenario.devices.is_empty());
        assert!(scenario.rules.is_empty());
        assert!(scenario.is_active);
    }

    #[test]
    fn test_add_remove_devices() {
        let mut scenario = Scenario::new(
            "Test".to_string(),
            "Test".to_string(),
        );

        scenario.add_device("device1".to_string());
        scenario.add_device("device2".to_string());
        assert_eq!(scenario.devices.len(), 2);

        // Adding duplicate should not increase count
        scenario.add_device("device1".to_string());
        assert_eq!(scenario.devices.len(), 2);

        scenario.remove_device("device1");
        assert_eq!(scenario.devices.len(), 1);
        assert_eq!(scenario.devices[0], "device2");
    }

    #[test]
    fn test_add_remove_rules() {
        let mut scenario = Scenario::new(
            "Test".to_string(),
            "Test".to_string(),
        );

        scenario.add_rule("rule1".to_string());
        scenario.add_rule("rule2".to_string());
        assert_eq!(scenario.rules.len(), 2);

        scenario.remove_rule("rule1");
        assert_eq!(scenario.rules.len(), 1);
    }

    #[test]
    fn test_to_llm_prompt() {
        let mut scenario = Scenario::new(
            "温度监控".to_string(),
            "监控机房温度".to_string(),
        );
        scenario.add_device("sensor1".to_string());
        scenario.add_device("sensor2".to_string());
        scenario.add_rule("rule1".to_string());

        let prompt = scenario.to_llm_prompt();
        assert!(prompt.contains("温度监控"));
        assert!(prompt.contains("监控机房温度"));
        assert!(prompt.contains("sensor1"));
        assert!(prompt.contains("sensor2"));
        assert!(prompt.contains("rule1"));
    }

    #[test]
    fn test_scenario_summary() {
        let mut scenario = Scenario::new(
            "Test".to_string(),
            "Test".to_string(),
        );
        scenario.add_device("d1".to_string());
        scenario.add_rule("r1".to_string());

        let summary = scenario.summary();
        assert!(summary.contains("Test"));
        assert!(summary.contains("1 设备"));
        assert!(summary.contains("1 规则"));
    }

    #[test]
    fn test_template_scenario() {
        let template = ScenarioTemplates::datacenter_temperature();
        let scenario = template.create_scenario(Some("My DC".to_string()));

        assert_eq!(scenario.name, "My DC");
        assert_eq!(scenario.metadata.category, ScenarioCategory::Monitoring);
        assert_eq!(scenario.metadata.environment, Environment::DataCenter);
    }

    #[test]
    fn test_all_templates() {
        let _dc = ScenarioTemplates::datacenter_temperature();
        let _office = ScenarioTemplates::office_energy_saving();
        let _production = ScenarioTemplates::production_quality();
        let _home = ScenarioTemplates::smart_home_comfort();
    }
}
