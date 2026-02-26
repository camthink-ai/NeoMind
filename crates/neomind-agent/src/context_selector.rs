//! Intelligent context selector for LLM agent.
//!
//! This module analyzes user queries and selects relevant context
//! (device types, rules, metrics) for efficient LLM processing.

use std::collections::HashSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use neomind_devices::mdl_format::DeviceTypeDefinition;
use neomind_rules::{dsl::RuleCondition, RuleEngine};

// Import configuration for default max_tokens
use neomind_core::config::agent_env_vars;

/// Intent analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentAnalysis {
    /// Primary intent type
    pub intent_type: IntentType,
    /// Confidence score (0-1)
    pub confidence: f32,
    /// Extracted entities
    pub entities: Vec<Entity>,
    /// Suggested context scope
    pub context_scope: ContextScope,
}

/// Intent type classification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntentType {
    /// Query device status or metrics
    DeviceQuery,
    /// Query or analyze rules
    RuleQuery,
    /// Create or modify rules
    RuleCreation,
    /// Control devices
    DeviceControl,
    /// Alert or notification related
    AlertManagement,
    /// General inquiry
    General,
    /// Unknown intent
    Unknown,
}

/// Extracted entity from user query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entity {
    /// Entity type
    pub entity_type: EntityType,
    /// Entity value (e.g., device ID, metric name)
    pub value: String,
    /// Position in query (character offset)
    pub position: usize,
}

/// Entity type classification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityType {
    /// Device identifier
    DeviceId,
    /// Metric name
    Metric,
    /// Device type
    DeviceType,
    /// Rule name
    RuleName,
    /// Command name
    Command,
    /// Threshold value
    Threshold,
    /// Time duration
    Duration,
}

/// Context scope suggestion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextScope {
    /// Minimal context - just relevant device
    Minimal,
    /// Standard context - device + related rules
    Standard,
    /// Extended context - all related devices and rules
    Extended,
    /// Full context - all available information
    Full,
}

/// Intent analyzer for user queries.
pub struct IntentAnalyzer {
    /// Known device IDs
    device_ids: Arc<RwLock<HashSet<String>>>,
    /// Known metrics
    metrics: Arc<RwLock<HashSet<String>>>,
    /// Known device types
    device_types: Arc<RwLock<HashSet<String>>>,
    /// Known commands
    commands: Arc<RwLock<HashSet<String>>>,
}

impl IntentAnalyzer {
    /// Create a new intent analyzer.
    pub fn new() -> Self {
        Self {
            device_ids: Arc::new(RwLock::new(HashSet::new())),
            metrics: Arc::new(RwLock::new(HashSet::new())),
            device_types: Arc::new(RwLock::new(HashSet::new())),
            commands: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Register known device IDs.
    pub async fn register_device_ids(&self, ids: Vec<String>) {
        let mut device_ids = self.device_ids.write().await;
        device_ids.extend(ids);
    }

    /// Register known metrics.
    pub async fn register_metrics(&self, metrics: Vec<String>) {
        let mut metric_set = self.metrics.write().await;
        metric_set.extend(metrics);
    }

    /// Register known device types.
    pub async fn register_device_types(&self, types: Vec<String>) {
        let mut type_set = self.device_types.write().await;
        type_set.extend(types);
    }

    /// Register known commands.
    pub async fn register_commands(&self, commands: Vec<String>) {
        let mut cmd_set = self.commands.write().await;
        cmd_set.extend(commands);
    }

    /// Analyze a user query to determine intent.
    pub async fn analyze(&self, query: &str) -> IntentAnalysis {
        let query_lower = query.to_lowercase();

        // Detect intent type based on keywords
        let intent_type = self.detect_intent_type(&query_lower);

        // Extract entities
        let entities = self.extract_entities(query, &query_lower).await;

        // Calculate confidence
        let confidence = self.calculate_confidence(&intent_type, &entities);

        // Determine context scope
        let context_scope = self.determine_scope(&intent_type, &entities);

        IntentAnalysis {
            intent_type,
            confidence,
            entities,
            context_scope,
        }
    }

    /// Detect intent type from query.
    fn detect_intent_type(&self, query: &str) -> IntentType {
        // Rule creation keywords (check before rule query)
        if (query.contains("创建") || query.contains("新建") || query.contains("添加"))
            && (query.contains("规则") || query.contains("rule"))
            || query.contains("create rule")
            || query.contains("add rule")
            || query.contains("new rule")
        {
            return IntentType::RuleCreation;
        }

        // Device control keywords
        if query.contains("控制")
            || query.contains("执行")
            || query.contains("开启")
            || query.contains("关闭")
            || query.contains("control")
            || query.contains("execute")
            || query.contains("turn on")
            || query.contains("turn off")
        {
            return IntentType::DeviceControl;
        }

        // Rule query keywords
        if query.contains("规则") || query.contains("rule") {
            return IntentType::RuleQuery;
        }

        // Alert keywords
        if query.contains("告警")
            || query.contains("警报")
            || query.contains("通知")
            || query.contains("alert")
            || query.contains("alarm")
            || query.contains("notify")
        {
            return IntentType::AlertManagement;
        }

        // Device query keywords
        if query.contains("设备")
            || query.contains("传感器")
            || query.contains("温度")
            || query.contains("湿度")
            || query.contains("查询")
            || query.contains("状态")
            || query.contains("device")
            || query.contains("sensor")
            || query.contains("temperature")
            || query.contains("humidity")
            || query.contains("query")
            || query.contains("status")
        {
            return IntentType::DeviceQuery;
        }

        IntentType::General
    }

    /// Extract entities from query.
    async fn extract_entities(&self, query: &str, query_lower: &str) -> Vec<Entity> {
        // Pre-allocate with estimated capacity based on collection sizes
        let device_ids = self.device_ids.read().await;
        let metrics = self.metrics.read().await;
        let device_types = self.device_types.read().await;
        let commands = self.commands.read().await;

        // Estimate: assume ~10% of items will match
        let estimated_entities = (device_ids.len()
            + metrics.len()
            + device_types.len()
            + commands.len())
            / 10
            + 1;
        let mut entities = Vec::with_capacity(estimated_entities.min(50));

        // Find device IDs
        for device_id in &*device_ids {
            if let Some(pos) = query_lower.find(&device_id.to_lowercase()) {
                entities.push(Entity {
                    entity_type: EntityType::DeviceId,
                    value: device_id.clone(),
                    position: pos,
                });
            }
        }

        // Find metrics
        for metric in &*metrics {
            if let Some(pos) = query_lower.find(&metric.to_lowercase()) {
                entities.push(Entity {
                    entity_type: EntityType::Metric,
                    value: metric.clone(),
                    position: pos,
                });
            }
        }

        // Find device types
        for device_type in &*device_types {
            if let Some(pos) = query_lower.find(&device_type.to_lowercase()) {
                entities.push(Entity {
                    entity_type: EntityType::DeviceType,
                    value: device_type.clone(),
                    position: pos,
                });
            }
        }

        // Find commands
        for command in &*commands {
            if let Some(pos) = query_lower.find(&command.to_lowercase()) {
                entities.push(Entity {
                    entity_type: EntityType::Command,
                    value: command.clone(),
                    position: pos,
                });
            }
        }

        // Extract numbers as thresholds
        let mut num_start = None;
        for (i, c) in query.char_indices() {
            if c.is_ascii_digit() || c == '-' {
                if num_start.is_none() {
                    num_start = Some(i);
                }
            } else if let Some(start) = num_start {
                if c.is_whitespace() || !c.is_ascii_digit() {
                    entities.push(Entity {
                        entity_type: EntityType::Threshold,
                        value: query[start..i].to_string(),
                        position: start,
                    });
                    num_start = None;
                }
            }
        }
        if let Some(start) = num_start {
            entities.push(Entity {
                entity_type: EntityType::Threshold,
                value: query[start..].to_string(),
                position: start,
            });
        }

        entities
    }

    /// Calculate confidence score for the analysis.
    fn calculate_confidence(&self, intent_type: &IntentType, entities: &[Entity]) -> f32 {
        let mut confidence = 0.5;

        // Increase confidence if we found entities
        confidence += (entities.len() as f32 * 0.1).min(0.3);

        // Adjust based on intent type
        match intent_type {
            IntentType::Unknown => confidence -= 0.2,
            IntentType::General => confidence -= 0.1,
            _ => {}
        }

        confidence.clamp(0.0, 1.0)
    }

    /// Determine appropriate context scope.
    fn determine_scope(&self, intent_type: &IntentType, entities: &[Entity]) -> ContextScope {
        match intent_type {
            IntentType::DeviceQuery => {
                if entities
                    .iter()
                    .any(|e| e.entity_type == EntityType::DeviceId)
                {
                    ContextScope::Standard
                } else {
                    ContextScope::Minimal
                }
            }
            IntentType::RuleQuery => ContextScope::Standard,
            IntentType::RuleCreation => ContextScope::Extended,
            IntentType::DeviceControl => ContextScope::Minimal,
            IntentType::AlertManagement => ContextScope::Standard,
            IntentType::General => ContextScope::Minimal,
            IntentType::Unknown => ContextScope::Minimal,
        }
    }
}

impl Default for IntentAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

/// Context bundle containing relevant information for LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBundle {
    /// Device type definitions
    pub device_types: Vec<DeviceTypeReference>,
    /// Rule definitions
    pub rules: Vec<RuleReference>,
    /// Available commands
    pub commands: Vec<CommandReference>,
    /// Total estimated token count
    pub estimated_tokens: usize,
}

/// Reference to a device type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceTypeReference {
    /// Device type ID
    pub device_type: String,
    /// Device name
    pub name: String,
    /// Relevant metrics
    pub metrics: Vec<String>,
    /// Available commands
    pub commands: Vec<String>,
}

/// Reference to a rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleReference {
    /// Rule ID
    pub rule_id: String,
    /// Rule name
    pub name: String,
    /// Condition description
    pub condition: String,
    /// Associated device
    pub device_id: String,
}

/// Reference to a command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandReference {
    /// Device type
    pub device_type: String,
    /// Command name
    pub command: String,
    /// Command description
    pub description: String,
}

/// Context selector for building relevant context.
pub struct ContextSelector {
    /// Intent analyzer
    analyzer: IntentAnalyzer,
    /// Available device types
    device_types: Arc<RwLock<Vec<DeviceTypeDefinition>>>,
    /// Rule engine
    rule_engine: Arc<RwLock<Option<Arc<RuleEngine>>>>,
    /// Maximum tokens in context
    max_tokens: usize,
}

impl ContextSelector {
    /// Create a new context selector.
    pub fn new() -> Self {
        Self {
            analyzer: IntentAnalyzer::new(),
            device_types: Arc::new(RwLock::new(Vec::new())),
            rule_engine: Arc::new(RwLock::new(None)),
            max_tokens: agent_env_vars::context_selector_tokens(),
        }
    }

    /// Set maximum token budget.
    pub fn with_max_tokens(mut self, tokens: usize) -> Self {
        self.max_tokens = tokens;
        self
    }

    /// Set available device types.
    pub async fn set_device_types(&self, device_types: Vec<DeviceTypeDefinition>) {
        let mut dt = self.device_types.write().await;
        *dt = device_types;
    }

    /// Set rule engine.
    pub async fn set_rule_engine(&self, engine: Arc<RuleEngine>) {
        let mut re = self.rule_engine.write().await;
        *re = Some(engine);
    }

    /// Register device types with the intent analyzer.
    pub async fn register_with_analyzer(&self) {
        let device_types = self.device_types.read().await;

        let _device_ids: Vec<String> = device_types
            .iter()
            .flat_map(|dt| {
                let ids = vec![dt.device_type.clone()];
                let metrics: Vec<String> =
                    dt.uplink.metrics.iter().map(|m| m.name.clone()).collect();
                let commands: Vec<String> = dt
                    .downlink
                    .commands
                    .iter()
                    .map(|c| c.name.clone())
                    .collect();
                ids.into_iter().chain(metrics).chain(commands)
            })
            .collect();

        let ids: Vec<String> = device_types
            .iter()
            .map(|dt| dt.device_type.clone())
            .collect();
        let metrics: Vec<String> = device_types
            .iter()
            .flat_map(|dt| dt.uplink.metrics.iter().map(|m| m.name.clone()))
            .collect();
        let types: Vec<String> = device_types
            .iter()
            .map(|dt| dt.device_type.clone())
            .collect();
        let commands: Vec<String> = device_types
            .iter()
            .flat_map(|dt| dt.downlink.commands.iter().map(|c| c.name.clone()))
            .collect();

        self.analyzer.register_device_ids(ids).await;
        self.analyzer.register_metrics(metrics).await;
        self.analyzer.register_device_types(types).await;
        self.analyzer.register_commands(commands).await;
    }

    /// Select context for a query.
    pub async fn select_context(&self, query: &str) -> (IntentAnalysis, ContextBundle) {
        // Analyze intent
        let analysis = self.analyzer.analyze(query).await;

        // Build context bundle based on analysis
        let bundle = self.build_bundle(&analysis).await;

        (analysis, bundle)
    }

    /// Build context bundle from intent analysis.
    async fn build_bundle(&self, analysis: &IntentAnalysis) -> ContextBundle {
        let device_types = self.device_types.read().await;

        // Extract device IDs from entities
        let target_devices: HashSet<String> = analysis
            .entities
            .iter()
            .filter(|e| e.entity_type == EntityType::DeviceId)
            .map(|e| e.value.clone())
            .collect();

        let mut device_refs = Vec::new();
        let mut rule_refs = Vec::new();
        let mut command_refs = Vec::new();
        let mut token_count = 0;

        // Select relevant device types
        for dt in &*device_types {
            let include = match analysis.context_scope {
                ContextScope::Minimal => target_devices.contains(&dt.device_type),
                ContextScope::Standard => {
                    target_devices.is_empty() || target_devices.contains(&dt.device_type)
                }
                ContextScope::Extended | ContextScope::Full => true,
            };

            if include {
                let metrics: Vec<String> =
                    dt.uplink.metrics.iter().map(|m| m.name.clone()).collect();
                let commands: Vec<String> = dt
                    .downlink
                    .commands
                    .iter()
                    .map(|c| c.name.clone())
                    .collect();

                token_count += dt.device_type.len() + metrics.len() * 10 + commands.len() * 10;

                device_refs.push(DeviceTypeReference {
                    device_type: dt.device_type.clone(),
                    name: dt.name.clone(),
                    metrics,
                    commands,
                });
            }
        }

        // Select relevant rules
        if let Some(engine) = self.rule_engine.read().await.as_ref() {
            let rules = engine.list_rules().await;

            for rule in rules {
                // Extract device_id from condition for matching
                let device_id = match &rule.condition {
                    RuleCondition::Device { device_id, .. }
                    | RuleCondition::DeviceRange { device_id, .. } => Some(device_id.clone()),
                    RuleCondition::Extension { extension_id, .. }
                    | RuleCondition::ExtensionRange { extension_id, .. } => {
                        Some(extension_id.clone())
                    }
                    _ => None, // Complex conditions don't have a single device
                };

                let include = match analysis.context_scope {
                    ContextScope::Minimal => false,
                    ContextScope::Standard => {
                        if let Some(ref did) = device_id {
                            target_devices.contains(did)
                        } else {
                            false
                        }
                    }
                    ContextScope::Extended | ContextScope::Full => true,
                };

                if include {
                    token_count += 50; // Estimate per rule

                    // Build condition description
                    let condition_desc = match &rule.condition {
                        RuleCondition::Device {
                            device_id,
                            metric,
                            operator,
                            threshold,
                        } => {
                            format!(
                                "{}.{} {} {}",
                                device_id,
                                metric,
                                operator.as_str(),
                                threshold
                            )
                        }
                        RuleCondition::Extension {
                            extension_id,
                            metric,
                            operator,
                            threshold,
                        } => {
                            format!(
                                "{}.{} {} {}",
                                extension_id,
                                metric,
                                operator.as_str(),
                                threshold
                            )
                        }
                        RuleCondition::DeviceRange {
                            device_id,
                            metric,
                            min,
                            max,
                        } => {
                            format!("{}.{} BETWEEN {} AND {}", device_id, metric, min, max)
                        }
                        RuleCondition::ExtensionRange {
                            extension_id,
                            metric,
                            min,
                            max,
                        } => {
                            format!("{}.{} BETWEEN {} AND {}", extension_id, metric, min, max)
                        }
                        RuleCondition::And(conditions) | RuleCondition::Or(conditions) => {
                            format!(
                                "(complex condition with {} sub-conditions)",
                                conditions.len()
                            )
                        }
                        RuleCondition::Not(_) => "(NOT condition)".to_string(),
                    };

                    rule_refs.push(RuleReference {
                        rule_id: rule.id.to_string(),
                        name: rule.name.clone(),
                        condition: condition_desc,
                        device_id: device_id.unwrap_or_else(|| "(complex)".to_string()),
                    });
                }
            }
        }

        // Build command references
        for dt in &*device_types {
            for cmd in &dt.downlink.commands {
                command_refs.push(CommandReference {
                    device_type: dt.device_type.clone(),
                    command: cmd.name.clone(),
                    description: cmd.display_name.clone(),
                });
            }
        }

        ContextBundle {
            device_types: device_refs,
            rules: rule_refs,
            commands: command_refs,
            estimated_tokens: token_count,
        }
    }

    /// Get intent analyzer reference.
    pub fn analyzer(&self) -> &IntentAnalyzer {
        &self.analyzer
    }
}

impl Default for ContextSelector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_intent_detection() {
        let analyzer = IntentAnalyzer::new();

        let result = analyzer.analyze("创建一个规则").await;
        assert_eq!(result.intent_type, IntentType::RuleCreation);
    }

    #[tokio::test]
    async fn test_entity_extraction() {
        let analyzer = IntentAnalyzer::new();
        analyzer
            .register_device_ids(vec!["sensor-1".to_string()])
            .await;

        let result = analyzer.analyze("查询 sensor-1 的温度").await;
        assert!(!result.entities.is_empty());
    }

    #[test]
    fn test_context_scope() {
        assert_eq!(
            IntentAnalyzer::new().determine_scope(&IntentType::RuleCreation, &[]),
            ContextScope::Extended
        );
    }

    #[tokio::test]
    async fn test_context_bundle() {
        let selector = ContextSelector::new();
        let (analysis, bundle) = selector.select_context("查询状态").await;

        assert_eq!(analysis.intent_type, IntentType::DeviceQuery);
        assert_eq!(bundle.estimated_tokens, 0); // No devices registered
    }
}
