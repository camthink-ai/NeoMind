//! Semantic parameter mapper for tool calls with multilingual support.
//!
//! This module provides intelligent mapping between natural language resource references
//! (device names, rule names, etc.) and their technical IDs. It supports both Chinese
//! and English, with automatic translation and fuzzy matching.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::context::{Resource, ResourceDataHelper, ResourceIndex};

/// Multilingual alias mappings for common terms.
const LOCATION_ALIASES: &[(&str, &[&str])] = &[
    ("客厅", &["living_room", "living", "livingroom", "lounge"]),
    ("卧室", &["bedroom", "bed_room", "sleeping_room"]),
    ("厨房", &["kitchen", "cook_room"]),
    ("浴室", &["bathroom", "bath", "washroom"]),
    ("卫生间", &["toilet", "restroom", "washroom"]),
    ("走廊", &["corridor", "hallway", "passage"]),
    ("玄关", &["entrance", "hallway", "foyer"]),
    ("书房", &["study", "study_room", "office"]),
    ("阳台", &["balcony", "terrace"]),
    ("车库", &["garage"]),
    ("庭院", &["yard", "garden", "courtyard"]),
];

const DEVICE_TYPE_ALIASES: &[(&str, &[&str])] = &[
    ("灯", &["light", "lamp", "lighting"]),
    ("空调", &["ac", "air_conditioner", "aircon", "climate"]),
    (
        "温度传感器",
        &["temp_sensor", "temperature_sensor", "thermometer"],
    ),
    ("湿度传感器", &["humidity_sensor", "hygrometer"]),
    ("窗帘", &["curtain", "blind", "shade"]),
    ("电视", &["tv", "television"]),
    ("音响", &["audio", "speaker", "sound_system"]),
    ("风扇", &["fan"]),
    ("加湿器", &["humidifier"]),
    ("净化器", &["purifier", "air_purifier"]),
    ("门锁", &["door_lock", "lock"]),
    ("摄像头", &["camera", "cam", "monitor"]),
];

/// Common nickname mappings for devices (Chinese -> Variants)
const DEVICE_NICKNAMES_CN: &[(&str, &[&str])] = &[
    // Light nicknames
    (
        "大灯",
        &["主灯", "吸顶灯", "顶灯", "main_light", "ceiling_light"],
    ),
    (
        "小灯",
        &["台灯", "辅助灯", "bedside_light", "auxiliary_light"],
    ),
    (
        "灯带",
        &["氛围灯", "led_light", "ambient_light", "strip_light"],
    ),
    ("筒灯", &["downlight", "spot_light", "spotlight"]),
    ("射灯", &["spot_light", "track_light"]),
    ("壁灯", &["wall_light", "wall_sconce"]),
    ("落地灯", &["floor_lamp", "standing_light"]),
    // AC nicknames
    ("冷气", &["空调", "ac", "aircon"]),
    ("暖气", &["地暖", "heating", "floor_heating"]),
    // Curtain nicknames
    (
        "智能窗帘",
        &["电动窗帘", "auto_curtain", "motorized_curtain"],
    ),
    // Security nicknames
    ("门铃", &["doorbell"]),
    ("可视门铃", &["video_doorbell", "smart_doorbell"]),
];

/// Common nickname mappings for devices (English -> Variants)
const DEVICE_NICKNAMES_EN: &[(&str, &[&str])] = &[
    ("main", &["primary", "master", "principal"]),
    ("primary", &["main", "master", "principal"]),
    ("master", &["main", "primary", "principal"]),
    ("secondary", &["aux", "auxiliary", "spare"]),
    ("aux", &["auxiliary", "secondary", "spare"]),
    ("bedside", &["nightstand", "bed", "reading"]),
    ("ceiling", &["overhead", "recessed"]),
    ("wall", &["sconce", "wall_mounted"]),
];

/// Compound word separators for splitting device names
#[allow(dead_code)]
const COMPOUND_SEPARATORS: &[char] = &[' ', '_', '-', '、', '·', '•'];

/// Mapping result from semantic name to technical ID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticMapping {
    /// Original natural language input
    pub original: String,
    /// Mapped technical ID
    pub technical_id: String,
    /// Match confidence (0-1)
    pub confidence: f32,
    /// Match type
    pub match_type: SemanticMatchType,
}

/// How the semantic mapping matched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SemanticMatchType {
    /// Exact name match
    Exact,
    /// Alias match
    Alias,
    /// Partial name match
    Partial,
    /// Location-based match
    Location,
    /// Capability-based match
    Capability,
    /// Translated match (Chinese <-> English)
    Translated,
    /// No match found
    NotFound,
}

/// Device semantic mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceMapping {
    /// Device name (natural language)
    pub name: String,
    /// Technical device ID
    pub device_id: String,
    /// Match type
    pub match_type: SemanticMatchType,
    /// Device location (if available)
    pub location: Option<String>,
    /// Device capabilities
    pub capabilities: Vec<String>,
}

/// Rule semantic mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleMapping {
    /// Rule name (natural language)
    pub name: String,
    /// Technical rule ID
    pub rule_id: String,
    /// Match type
    pub match_type: SemanticMatchType,
    /// Whether the rule is enabled
    pub enabled: bool,
}

/// Workflow semantic mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowMapping {
    /// Workflow name (natural language)
    pub name: String,
    /// Technical workflow ID
    pub workflow_id: String,
    /// Match type
    pub match_type: SemanticMatchType,
    /// Whether the workflow is enabled
    pub enabled: bool,
}

/// Mapping statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingStats {
    /// Total mappings performed
    pub total_mappings: usize,
    /// Successful mappings
    pub successful: usize,
    /// Failed mappings (fell back to original)
    pub failed: usize,
    /// Average confidence
    pub avg_confidence: f32,
}

/// Language detection result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Language {
    Chinese,
    English,
    Mixed,
    Unknown,
}

/// Enhanced semantic tool mapper with multilingual support.
pub struct SemanticToolMapper {
    /// Resource index for looking up devices and other resources
    resource_index: Arc<RwLock<ResourceIndex>>,
    /// Cache for rule name -> rule ID mappings
    rule_cache: Arc<RwLock<HashMap<String, RuleMapping>>>,
    /// Cache for workflow name -> workflow ID mappings
    workflow_cache: Arc<RwLock<HashMap<String, WorkflowMapping>>>,
    /// Mapping statistics
    stats: Arc<RwLock<MappingStats>>,
    /// Additional alias mappings
    alias_mappings: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl SemanticToolMapper {
    /// Create a new semantic tool mapper.
    pub fn new(resource_index: Arc<RwLock<ResourceIndex>>) -> Self {
        let mut alias_mappings = HashMap::new();

        // Build location alias mappings
        for (zh, en_list) in LOCATION_ALIASES {
            for en in *en_list {
                // Chinese -> English
                alias_mappings
                    .entry(zh.to_string())
                    .or_insert_with(Vec::new)
                    .push(en.to_string());
                // English -> Chinese
                alias_mappings
                    .entry(en.to_string())
                    .or_insert_with(Vec::new)
                    .push(zh.to_string());
            }
        }

        // Build device type alias mappings
        for (zh, en_list) in DEVICE_TYPE_ALIASES {
            for en in *en_list {
                alias_mappings
                    .entry(zh.to_string())
                    .or_insert_with(Vec::new)
                    .push(en.to_string());
                alias_mappings
                    .entry(en.to_string())
                    .or_insert_with(Vec::new)
                    .push(zh.to_string());
            }
        }

        Self {
            resource_index,
            rule_cache: Arc::new(RwLock::new(HashMap::new())),
            workflow_cache: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(MappingStats {
                total_mappings: 0,
                successful: 0,
                failed: 0,
                avg_confidence: 0.0,
            })),
            alias_mappings: Arc::new(RwLock::new(alias_mappings)),
        }
    }

    /// Detect the language of the input.
    pub fn detect_language(text: &str) -> Language {
        let chinese_chars = text
            .chars()
            .filter(|c| {
                let cp = *c as u32;
                (0x4E00..=0x9FFF).contains(&cp) || // CJK Unified Ideographs
            (0x3400..=0x4DBF).contains(&cp) || // CJK Extension A
            (0x20000..=0x2A6DF).contains(&cp) // CJK Extension B
            })
            .count();

        let english_chars = text.chars().filter(|c| c.is_ascii_alphabetic()).count();

        let total = chinese_chars + english_chars;
        if total == 0 {
            return Language::Unknown;
        }

        let chinese_ratio = chinese_chars as f64 / total as f64;
        let english_ratio = english_chars as f64 / total as f64;

        if chinese_ratio > 0.3 && english_ratio > 0.3 {
            Language::Mixed
        } else if chinese_ratio > 0.3 {
            Language::Chinese
        } else if english_ratio > 0.3 {
            Language::English
        } else {
            Language::Unknown
        }
    }

    /// Translate common terms between Chinese and English.
    pub fn translate_term(term: &str) -> Vec<String> {
        let mut translations = Vec::new();
        let term_lower = term.to_lowercase();

        // Check location aliases
        for (zh, en_list) in LOCATION_ALIASES {
            if term.contains(zh) || term_lower.contains(zh) {
                translations.extend(en_list.iter().map(|s| s.to_string()));
            }
            for en in *en_list {
                if term_lower.contains(en) {
                    translations.push((*zh).to_string());
                }
            }
        }

        // Check device type aliases
        for (zh, en_list) in DEVICE_TYPE_ALIASES {
            if term.contains(zh) || term_lower.contains(zh) {
                translations.extend(en_list.iter().map(|s| s.to_string()));
            }
            for en in *en_list {
                if term_lower.contains(en) {
                    translations.push((*zh).to_string());
                }
            }
        }

        translations
    }

    /// Expand a query with translations and aliases.
    pub async fn expand_query(&self, query: &str) -> Vec<String> {
        let mut expanded = Vec::new();
        expanded.push(query.to_string());

        // Add translations
        for translation in Self::translate_term(query) {
            if !expanded.contains(&translation) {
                expanded.push(translation);
            }
        }

        // Add custom aliases
        let aliases = self.alias_mappings.read().await;
        if let Some(alias_list) = aliases.get(query) {
            for alias in alias_list {
                if !expanded.contains(alias) {
                    expanded.push(alias.clone());
                }
            }
        }

        expanded
    }

    /// Decompose a compound device reference into location + device_type components.
    /// For example: "走廊灯" -> ["走廊", "灯"], "living room light" -> ["living room", "light"]
    fn decompose_compound_reference(reference: &str) -> Vec<(String, String)> {
        let mut combinations = Vec::new();

        // Try splitting by known locations first
        for (location_zh, location_en_list) in LOCATION_ALIASES {
            if reference.contains(location_zh) {
                // Split by the location
                let remainder = reference.replace(location_zh, "");
                if !remainder.is_empty() {
                    combinations.push((location_zh.to_string(), remainder.trim().to_string()));
                }
            }
            for location_en in *location_en_list {
                if reference.to_lowercase().contains(location_en) {
                    let remainder = reference.to_lowercase().replace(location_en, "");
                    if !remainder.is_empty() {
                        combinations.push((location_en.to_string(), remainder.trim().to_string()));
                    }
                }
            }
        }

        // Try splitting by known device types
        for (device_zh, device_en_list) in DEVICE_TYPE_ALIASES {
            if reference.contains(device_zh) {
                let remainder = reference.replace(device_zh, "");
                if !remainder.is_empty() {
                    combinations.push((remainder.trim().to_string(), device_zh.to_string()));
                }
            }
            for device_en in *device_en_list {
                if reference.to_lowercase().contains(device_en) {
                    let remainder = reference.to_lowercase().replace(device_en, "");
                    if !remainder.is_empty() {
                        combinations.push((remainder.trim().to_string(), device_en.to_string()));
                    }
                }
            }
        }

        // If no splits found, try character-based decomposition for Chinese
        if combinations.is_empty() && Self::detect_language(reference) == Language::Chinese {
            let chars: Vec<char> = reference.chars().collect();
            for i in 1..chars.len() {
                let part1: String = chars[..i].iter().collect();
                let part2: String = chars[i..].iter().collect();
                combinations.push((part1, part2));
            }
        }

        combinations
    }

    /// Expand nickname to known variants.
    fn expand_nickname(term: &str) -> Vec<String> {
        let mut variants = Vec::new();
        let term_lower = term.to_lowercase();

        // Check Chinese nicknames
        for (nickname, variants_list) in DEVICE_NICKNAMES_CN {
            if term.contains(nickname) || term_lower.contains(&nickname.to_lowercase()) {
                for variant in *variants_list {
                    if !term.contains(variant) {
                        variants.push(term.replace(nickname, variant));
                    }
                }
            }
        }

        // Check English nicknames
        for (nickname, variants_list) in DEVICE_NICKNAMES_EN {
            if term_lower.contains(nickname) {
                for variant in *variants_list {
                    if !term_lower.contains(variant) {
                        let expanded = term_lower.replace(nickname, variant);
                        variants.push(expanded);
                    }
                }
            }
        }

        variants
    }

    /// Resolve a device reference using component-based matching for compound phrases.
    async fn resolve_device_by_components(&self, device_ref: &str) -> Option<DeviceMapping> {
        let index = self.resource_index.read().await;

        // Decompose the reference into components
        let combinations = Self::decompose_compound_reference(device_ref);

        for (location_part, device_type_part) in combinations {
            // Search for devices matching the location
            let location_results = index.search_string(&location_part).await;

            for result in &location_results {
                let device_location = ResourceDataHelper::location(&result.resource.data);
                let device_name = &result.resource.name;

                // Check if the device also matches the type part
                let device_name_lower = device_name.to_lowercase();
                let type_part_lower = device_type_part.to_lowercase();

                // Expand type part with translations
                let type_translations = Self::translate_term(&device_type_part);
                let mut type_matches = device_name_lower.contains(&type_part_lower);

                for translation in &type_translations {
                    if device_name_lower.contains(&translation.to_lowercase()) {
                        type_matches = true;
                        break;
                    }
                }

                // Also check device type in resource data
                if let Some(device_data) = result.resource.as_device() {
                    if device_data.device_type.to_lowercase() == type_part_lower
                        || type_translations
                            .iter()
                            .any(|t| t.to_lowercase() == device_data.device_type.to_lowercase())
                    {
                        type_matches = true;
                    }
                }

                if type_matches || device_type_part.len() <= 2 {
                    return Some(DeviceMapping {
                        name: device_ref.to_string(),
                        device_id: result.resource.id.id.clone(),
                        match_type: SemanticMatchType::Location,
                        location: device_location,
                        capabilities: ResourceDataHelper::capabilities(&result.resource.data),
                    });
                }
            }
        }

        None
    }

    /// Map tool parameters from natural language to technical IDs.
    pub async fn map_tool_parameters(
        &self,
        tool_name: &str,
        raw_params: Value,
    ) -> Result<Value, String> {
        let mut params = raw_params;
        let mut mapping_applied = false;

        match tool_name {
            // Device control tools
            "device.control" | "control_device" | "control" => {
                let device_name = params
                    .get("device")
                    .or(params.get("device_id"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                if let Some(name) = device_name {
                    if let Some(mapping) = self.resolve_device(&name).await {
                        params["device_id"] = Value::String(mapping.device_id.clone());
                        params["_device_name"] = Value::String(name);
                        params["_match_type"] = Value::String(format!("{:?}", mapping.match_type));
                        mapping_applied = true;
                    }
                }
            }

            // Data query tools
            "data.query" | "query_data" | "query" => {
                let device_name = params
                    .get("device")
                    .or(params.get("device_id"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                if let Some(name) = device_name {
                    if let Some(mapping) = self.resolve_device(&name).await {
                        params["device_id"] = Value::String(mapping.device_id.clone());
                        params["_device_name"] = Value::String(name);
                        mapping_applied = true;
                    }
                }
            }

            // Device status query
            "device.status" | "query_device_status" => {
                let device_name = params
                    .get("device")
                    .or(params.get("device_id"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                if let Some(name) = device_name {
                    if let Some(mapping) = self.resolve_device(&name).await {
                        params["device_id"] = Value::String(mapping.device_id.clone());
                        mapping_applied = true;
                    }
                }
            }

            // Device configuration
            "device.config.set" | "set_device_config" => {
                let device_name = params
                    .get("device")
                    .or(params.get("device_id"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                if let Some(name) = device_name {
                    if let Some(mapping) = self.resolve_device(&name).await {
                        params["device_id"] = Value::String(mapping.device_id.clone());
                        mapping_applied = true;
                    }
                }
            }

            // Rule management tools
            "rule.delete" | "delete_rule" => {
                let rule_name = params
                    .get("rule")
                    .or(params.get("rule_id"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                if let Some(name) = rule_name {
                    if let Some(mapping) = self.resolve_rule(&name).await {
                        params["rule_id"] = Value::String(mapping.rule_id.clone());
                        params["_rule_name"] = Value::String(name);
                        mapping_applied = true;
                    }
                }
            }

            "rule.enable" | "enable_rule" | "rule.disable" | "disable_rule" => {
                let rule_name = params
                    .get("rule")
                    .or(params.get("rule_id"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                if let Some(name) = rule_name {
                    if let Some(mapping) = self.resolve_rule(&name).await {
                        params["rule_id"] = Value::String(mapping.rule_id.clone());
                        mapping_applied = true;
                    }
                }
            }

            "rule.update" | "update_rule" => {
                let rule_name = params
                    .get("rule")
                    .or(params.get("rule_id"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                if let Some(name) = rule_name {
                    if let Some(mapping) = self.resolve_rule(&name).await {
                        params["rule_id"] = Value::String(mapping.rule_id.clone());
                        mapping_applied = true;
                    }
                }
            }

            // Workflow tools
            "workflow.trigger" | "trigger_workflow" => {
                let wf_name = params
                    .get("workflow")
                    .or(params.get("workflow_id"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                if let Some(name) = wf_name {
                    if let Some(mapping) = self.resolve_workflow(&name).await {
                        params["workflow_id"] = Value::String(mapping.workflow_id.clone());
                        params["_workflow_name"] = Value::String(name);
                        mapping_applied = true;
                    }
                }
            }

            // Batch device control
            "devices.batch_control" | "batch_control_devices" => {
                if let Some(devices_array) = params.get_mut("devices") {
                    if let Some(devices) = devices_array.as_array_mut() {
                        for device_param in devices.iter_mut() {
                            let device_name = device_param
                                .get("device")
                                .or(device_param.get("device_id"))
                                .and_then(|v| v.as_str());

                            if let Some(name) = device_name {
                                if let Some(mapping) = self.resolve_device(name).await {
                                    *device_param = serde_json::json!({
                                        "device_id": mapping.device_id,
                                        "_device_name": name
                                    });
                                    mapping_applied = true;
                                }
                            }
                        }
                    }
                }
            }

            _ => {
                // Unknown tool - no mapping applied
            }
        }

        // Update statistics
        self.update_stats(mapping_applied).await;

        Ok(params)
    }

    /// Resolve a device reference to its technical ID with enhanced multilingual support.
    pub async fn resolve_device(&self, device_ref: &str) -> Option<DeviceMapping> {
        // First, try component-based matching for compound phrases (e.g., "走廊灯")
        if let Some(result) = self.resolve_device_by_components(device_ref).await {
            return Some(result);
        }

        let index = self.resource_index.read().await;

        // Expand query with translations and nicknames
        let mut expanded_queries = self.expand_query(device_ref).await;

        // Add nickname variants
        for nickname_variant in Self::expand_nickname(device_ref) {
            if !expanded_queries.contains(&nickname_variant) {
                expanded_queries.push(nickname_variant);
            }
        }

        // Also expand each query with nicknames
        let mut all_queries = Vec::new();
        for query in &expanded_queries {
            all_queries.push(query.clone());
            for nickname_variant in Self::expand_nickname(query) {
                if !all_queries.contains(&nickname_variant) {
                    all_queries.push(nickname_variant);
                }
            }
        }

        // Try each expanded query
        for query in &all_queries {
            let results = index.search_string(query).await;

            if !results.is_empty() {
                let best = &results[0];
                // Dynamic threshold based on match type
                let threshold = if query == device_ref {
                    0.7 // Higher threshold for direct match
                } else if all_queries.len() > 2 {
                    0.3 // Lower threshold for expanded searches
                } else {
                    0.4 // Standard threshold for translations
                };

                if best.score > threshold {
                    let device_id = best.resource.id.id.clone();
                    let location = ResourceDataHelper::location(&best.resource.data);

                    return Some(DeviceMapping {
                        name: device_ref.to_string(),
                        device_id,
                        match_type: if query == device_ref {
                            if best.score > 0.8 {
                                SemanticMatchType::Exact
                            } else {
                                SemanticMatchType::Partial
                            }
                        } else if all_queries
                            .iter()
                            .any(|q| !q.eq(device_ref) && !q.eq(&device_ref.to_lowercase()))
                        {
                            SemanticMatchType::Translated
                        } else {
                            SemanticMatchType::Alias
                        },
                        location,
                        capabilities: ResourceDataHelper::capabilities(&best.resource.data),
                    });
                }
            }
        }

        None
    }

    /// Resolve multiple devices at once.
    pub async fn resolve_devices(&self, device_refs: &[String]) -> Vec<DeviceMapping> {
        let mut mappings = Vec::new();

        for device_ref in device_refs {
            if let Some(mapping) = self.resolve_device(device_ref).await {
                mappings.push(mapping);
            } else {
                mappings.push(DeviceMapping {
                    name: device_ref.clone(),
                    device_id: device_ref.clone(),
                    match_type: SemanticMatchType::NotFound,
                    location: None,
                    capabilities: vec![],
                });
            }
        }

        mappings
    }

    /// Register a rule mapping with multilingual aliases.
    pub async fn register_rule(&self, rule_id: String, rule_name: String, enabled: bool) {
        let mut cache = self.rule_cache.write().await;
        cache.insert(
            rule_name.clone(),
            RuleMapping {
                name: rule_name.clone(),
                rule_id,
                match_type: SemanticMatchType::Exact,
                enabled,
            },
        );
    }

    /// Bulk register rules.
    pub async fn register_rules(&self, rules: Vec<(String, String, bool)>) {
        let mut cache = self.rule_cache.write().await;
        for (rule_id, rule_name, enabled) in rules {
            cache.insert(
                rule_name.clone(),
                RuleMapping {
                    name: rule_name,
                    rule_id,
                    match_type: SemanticMatchType::Exact,
                    enabled,
                },
            );
        }
    }

    /// Resolve a rule reference to its technical ID.
    pub async fn resolve_rule(&self, rule_ref: &str) -> Option<RuleMapping> {
        let cache = self.rule_cache.read().await;

        // Try exact match first
        if let Some(mapping) = cache.get(rule_ref) {
            return Some(mapping.clone());
        }

        // Try partial match
        for (name, mapping) in cache.iter() {
            if name.contains(rule_ref) || rule_ref.contains(name) {
                return Some(mapping.clone());
            }
        }

        None
    }

    /// Register a workflow mapping.
    pub async fn register_workflow(
        &self,
        workflow_id: String,
        workflow_name: String,
        enabled: bool,
    ) {
        let mut cache = self.workflow_cache.write().await;
        cache.insert(
            workflow_name.clone(),
            WorkflowMapping {
                name: workflow_name.clone(),
                workflow_id,
                match_type: SemanticMatchType::Exact,
                enabled,
            },
        );
    }

    /// Bulk register workflows.
    pub async fn register_workflows(&self, workflows: Vec<(String, String, bool)>) {
        let mut cache = self.workflow_cache.write().await;
        for (workflow_id, workflow_name, enabled) in workflows {
            cache.insert(
                workflow_name.clone(),
                WorkflowMapping {
                    name: workflow_name,
                    workflow_id,
                    match_type: SemanticMatchType::Exact,
                    enabled,
                },
            );
        }
    }

    /// Resolve a workflow reference to its technical ID.
    pub async fn resolve_workflow(&self, workflow_ref: &str) -> Option<WorkflowMapping> {
        let cache = self.workflow_cache.read().await;

        // Try exact match first
        if let Some(mapping) = cache.get(workflow_ref) {
            return Some(mapping.clone());
        }

        // Try partial match
        for (name, mapping) in cache.iter() {
            if name.contains(workflow_ref) || workflow_ref.contains(name) {
                return Some(mapping.clone());
            }
        }

        None
    }

    /// Register a device in the resource index.
    pub async fn register_device(&self, device: Resource) -> Result<(), String> {
        self.resource_index.write().await.register(device).await
    }

    /// Get all registered devices.
    pub async fn list_devices(&self) -> Vec<Resource> {
        self.resource_index.read().await.list_devices().await
    }

    /// Get available device names for LLM context (multilingual).
    pub async fn get_device_names_for_llm(&self) -> String {
        let devices = self.list_devices().await;

        if devices.is_empty() {
            return "暂无可用设备 / No devices available".to_string();
        }

        let mut text = String::from("可用设备 / Available Devices:\n");

        for device in &devices {
            let location = ResourceDataHelper::location(&device.data)
                .map(|l| format!(" ({})", l))
                .unwrap_or_default();

            text.push_str(&format!("- {}{}\n", device.name, location));

            // Show capabilities
            let caps = ResourceDataHelper::capabilities(&device.data);
            if !caps.is_empty() {
                text.push_str(&format!("  能力 / Capabilities: {}\n", caps.join(", ")));
            }
        }

        text
    }

    /// Get available rule names for LLM context.
    pub async fn get_rule_names_for_llm(&self) -> String {
        let cache = self.rule_cache.read().await;

        if cache.is_empty() {
            return "暂无可用规则 / No rules available".to_string();
        }

        let mut text = String::from("可用规则 / Available Rules:\n");

        for (_, mapping) in cache.iter() {
            let status = if mapping.enabled {
                "启用 / Enabled"
            } else {
                "禁用 / Disabled"
            };
            text.push_str(&format!("- {} ({})\n", mapping.name, status));
        }

        text
    }

    /// Get available workflow names for LLM context.
    pub async fn get_workflow_names_for_llm(&self) -> String {
        let cache = self.workflow_cache.read().await;

        if cache.is_empty() {
            return "暂无可用工作流 / No workflows available".to_string();
        }

        let mut text = String::from("可用工作流 / Available Workflows:\n");

        for (_, mapping) in cache.iter() {
            let status = if mapping.enabled {
                "启用 / Enabled"
            } else {
                "禁用 / Disabled"
            };
            text.push_str(&format!("- {} ({})\n", mapping.name, status));
        }

        text
    }

    /// Get complete semantic context for LLM prompt (multilingual).
    pub async fn get_semantic_context(&self) -> String {
        let mut context = String::new();

        context.push_str("## 资源语义映射 / Semantic Resource Mapping\n\n");
        context.push_str("### 支持的语言 / Supported Languages\n");
        context.push_str("- 中文 (Chinese): 客厅灯, 卧室空调, ...\n");
        context.push_str("- English: living room light, bedroom AC, ...\n\n");

        context.push_str("### 设备别名 / Device Aliases\n");
        context.push_str("- 灯 ↔ light / lamp\n");
        context.push_str("- 空调 ↔ AC / air conditioner\n");
        context.push_str("- 走廊 ↔ corridor / hallway\n");
        context.push_str("- 客厅 ↔ living room / lounge\n\n");

        context.push_str(&self.get_device_names_for_llm().await);
        context.push('\n');

        context.push_str(&self.get_rule_names_for_llm().await);
        context.push('\n');

        context.push_str(&self.get_workflow_names_for_llm().await);

        context
    }

    /// Update mapping statistics.
    async fn update_stats(&self, success: bool) {
        let mut stats = self.stats.write().await;
        stats.total_mappings += 1;
        if success {
            stats.successful += 1;
        } else {
            stats.failed += 1;
        }
    }

    /// Get mapping statistics.
    pub async fn get_stats(&self) -> MappingStats {
        self.stats.read().await.clone()
    }

    /// Clear all caches.
    pub async fn clear_caches(&self) {
        self.rule_cache.write().await.clear();
        self.workflow_cache.write().await.clear();
    }

    /// Periodic cache cleanup to prevent unbounded growth.
    ///
    /// This should be called on a timer (e.g., every 5 minutes) to keep cache sizes manageable.
    /// Since the cache doesn't track timestamps, this performs a full clear when size exceeds threshold.
    ///
    /// # Arguments
    /// * `max_cache_size` - Maximum entries per cache before cleanup (default: 1000)
    pub async fn periodic_cache_cleanup(&self, max_cache_size: Option<usize>) {
        let max_size = max_cache_size.unwrap_or(1000);

        let rule_size = self.rule_cache.read().await.len();
        let workflow_size = self.workflow_cache.read().await.len();

        // Clear caches if they exceed threshold
        if rule_size > max_size || workflow_size > max_size {
            tracing::info!(
                rule_cache_size = rule_size,
                workflow_cache_size = workflow_size,
                max_size = max_size,
                "Cache size exceeded threshold, performing cleanup"
            );
            self.clear_caches().await;
        }
    }

    /// Get suggestion for resolving an ambiguous reference.
    pub async fn suggest_resolution(&self, reference: &str) -> Option<String> {
        // Try device resolution first
        if let Some(mapping) = self.resolve_device(reference).await {
            return Some(format!(
                "设备: {} (ID: {})",
                mapping.name, mapping.device_id
            ));
        }

        // Try rule resolution
        if let Some(mapping) = self.resolve_rule(reference).await {
            return Some(format!("规则: {} (ID: {})", mapping.name, mapping.rule_id));
        }

        // Try workflow resolution
        if let Some(mapping) = self.resolve_workflow(reference).await {
            return Some(format!(
                "工作流: {} (ID: {})",
                mapping.name, mapping.workflow_id
            ));
        }

        // Search for similar devices
        let results = self
            .resource_index
            .read()
            .await
            .search_string(reference)
            .await;
        if !results.is_empty() {
            let suggestions: Vec<String> = results
                .iter()
                .take(3)
                .map(|r| r.resource.name.clone())
                .collect();
            return Some(format!(
                "您是不是指: {}? / Did you mean: {}?",
                suggestions.join(", "),
                suggestions.join(", ")
            ));
        }

        None
    }

    /// Add custom alias mapping.
    pub async fn add_alias(&self, from: String, to: String) {
        let mut aliases = self.alias_mappings.write().await;
        aliases.entry(from).or_insert_with(Vec::new).push(to);
    }

    /// Add bulk alias mappings.
    pub async fn add_aliases(&self, mappings: Vec<(String, String)>) {
        let mut aliases = self.alias_mappings.write().await;
        for (from, to) in mappings {
            aliases.entry(from).or_insert_with(Vec::new).push(to);
        }
    }
}

impl Clone for SemanticToolMapper {
    fn clone(&self) -> Self {
        Self {
            resource_index: Arc::clone(&self.resource_index),
            rule_cache: Arc::clone(&self.rule_cache),
            workflow_cache: Arc::clone(&self.workflow_cache),
            stats: Arc::clone(&self.stats),
            alias_mappings: Arc::clone(&self.alias_mappings),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_detection() {
        assert_eq!(
            SemanticToolMapper::detect_language("你好"),
            Language::Chinese
        );
        assert_eq!(
            SemanticToolMapper::detect_language("hello"),
            Language::English
        );
        // "你好你好world" has 4 Chinese chars out of 9 total (44%), exceeding the 30% threshold
        assert_eq!(
            SemanticToolMapper::detect_language("你好你好world"),
            Language::Mixed
        );
        assert_eq!(
            SemanticToolMapper::detect_language("123"),
            Language::Unknown
        );
    }

    #[test]
    fn test_translate_term() {
        let translations = SemanticToolMapper::translate_term("客厅灯");
        assert!(translations
            .iter()
            .any(|t| t.contains("living") || t.contains("light")));

        let translations = SemanticToolMapper::translate_term("bedroom");
        assert!(translations.iter().any(|t| t.contains("卧室")));
    }

    #[tokio::test]
    async fn test_multilingual_device_resolution() {
        let index = Arc::new(RwLock::new(ResourceIndex::new()));
        let mapper = SemanticToolMapper::new(index.clone());

        // Register test devices with Chinese names
        let devices = vec![
            Resource::device("light_living", "客厅灯", "switch")
                .with_alias("living room light")
                .with_location("客厅"),
            Resource::device("light_bedroom", "bedroom lamp", "switch").with_location("卧室"),
        ];

        for device in devices {
            index.write().await.register(device).await.unwrap();
        }

        // Test Chinese query
        let mapping = mapper.resolve_device("客厅灯").await;
        assert!(mapping.is_some());
        assert_eq!(mapping.unwrap().device_id, "light_living");

        // Test English query (should translate)
        let mapping = mapper.resolve_device("living room light").await;
        assert!(mapping.is_some());
    }

    #[tokio::test]
    async fn test_device_resolution_with_translation() {
        let index = Arc::new(RwLock::new(ResourceIndex::new()));
        let mapper = SemanticToolMapper::new(index.clone());

        // Register device with Chinese name
        let device = Resource::device("light_corridor", "走廊灯", "switch").with_location("走廊");
        index.write().await.register(device).await.unwrap();

        // Should match with English translation
        let mapping = mapper.resolve_device("corridor light").await;
        assert!(mapping.is_some());
        assert_eq!(mapping.unwrap().match_type, SemanticMatchType::Translated);
    }

    #[tokio::test]
    async fn test_multilingual_context_generation() {
        let index = Arc::new(RwLock::new(ResourceIndex::new()));
        let mapper = SemanticToolMapper::new(index.clone());

        index
            .write()
            .await
            .register(Resource::device("light_1", "客厅灯", "switch").with_location("客厅"))
            .await
            .unwrap();

        mapper
            .register_rules(vec![(
                "rule_001".to_string(),
                "温度报警规则".to_string(),
                true,
            )])
            .await;

        let context = mapper.get_semantic_context().await;
        assert!(context.contains("客厅灯"));
        assert!(context.contains("温度报警规则"));
        assert!(context.contains("Supported Languages"));
    }

    #[tokio::test]
    async fn test_custom_aliases() {
        let index = Arc::new(RwLock::new(ResourceIndex::new()));
        let mapper = SemanticToolMapper::new(index.clone());

        // Add custom alias
        mapper
            .add_alias("front_door".to_string(), "entrance_light".to_string())
            .await;

        // Verify alias was added
        let aliases = mapper.alias_mappings.read().await;
        assert!(aliases.contains_key("front_door"));
    }

    #[test]
    fn test_compound_decomposition() {
        // Test Chinese compound phrase decomposition
        let combinations = SemanticToolMapper::decompose_compound_reference("走廊灯");
        assert!(!combinations.is_empty());
        // Should contain ("走廊", "灯") or similar
        assert!(combinations
            .iter()
            .any(|(l, d)| l.contains("走廊") || d.contains("灯")));

        // Test English compound phrase decomposition
        let combinations = SemanticToolMapper::decompose_compound_reference("living room light");
        assert!(!combinations.is_empty());
        // Should contain location and device type
        assert!(combinations
            .iter()
            .any(|(l, d)| l.contains("living") || d.contains("light")));
    }

    #[test]
    fn test_nickname_expansion() {
        // Test Chinese nickname expansion
        let variants = SemanticToolMapper::expand_nickname("打开大灯");
        assert!(!variants.is_empty());
        // Should expand "大灯" to variants like "主灯", "吸顶灯", etc.

        // Test English nickname expansion
        let variants = SemanticToolMapper::expand_nickname("main light");
        assert!(!variants.is_empty());
        // Should expand "main" to variants like "primary", "master"
    }

    #[tokio::test]
    async fn test_compound_device_resolution() {
        let index = Arc::new(RwLock::new(ResourceIndex::new()));
        let mapper = SemanticToolMapper::new(index.clone());

        // Register a corridor light device
        let device = Resource::device("light_corridor", "走廊灯", "switch").with_location("走廊");
        index.write().await.register(device).await.unwrap();

        // Test compound phrase resolution - should decompose "走廊灯" into "走廊" + "灯"
        let mapping = mapper.resolve_device("走廊灯").await;
        assert!(mapping.is_some());
        let result = mapping.unwrap();
        assert_eq!(result.device_id, "light_corridor");

        // Test English equivalent
        let mapping = mapper.resolve_device("corridor light").await;
        assert!(mapping.is_some());
    }

    #[tokio::test]
    async fn test_nickname_resolution() {
        let index = Arc::new(RwLock::new(ResourceIndex::new()));
        let mapper = SemanticToolMapper::new(index.clone());

        // Register devices with nicknames
        let devices = vec![
            Resource::device("light_main_ceiling", "客厅主灯", "switch")
                .with_location("客厅")
                .with_alias("吸顶灯")
                .with_alias("顶灯"),
            Resource::device("light_bedside", "卧室台灯", "lamp")
                .with_location("卧室")
                .with_alias("小灯"),
        ];

        for device in devices {
            index.write().await.register(device).await.unwrap();
        }

        // Test nickname resolution - "大灯" should map to "主灯" variants
        let mapping = mapper.resolve_device("客厅大灯").await;
        assert!(mapping.is_some());

        // Test nickname resolution - "小灯" should map to bed-side lamp
        let mapping = mapper.resolve_device("卧室小灯").await;
        assert!(mapping.is_some());
    }
}
