//! 智能追问优化模块
//!
//! 改进点：
//! 1. 上下文感知 - 利用 ConversationContext 避免重复询问
//! 2. 动态追问生成 - 基于可用设备列表生成更自然的追问
//! 3. 多意图检测 - 检测一句话中的多个意图
//! 4. 追问优先级 - 只追问最关键的信息
//! 5. 资源索引集成 - 自动从 ResourceIndex 获取设备信息

use super::conversation_context::{ConversationContext, ConversationTopic};
use crate::context::ResourceIndex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// 追问优先级
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum FollowUpPriority {
    /// 低 - 可选信息
    Low = 0,
    /// 中 - 有助于更好的回答
    Medium = 1,
    /// 高 - 必需信息
    High = 2,
    /// 紧急 - 阻塞操作
    Critical = 3,
}

/// 追问类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FollowUpType {
    /// 缺少位置信息
    MissingLocation,
    /// 缺少设备信息
    MissingDevice,
    /// 缺少参数值
    MissingValue,
    /// 意图不明确
    AmbiguousIntent,
    /// 危险操作确认
    DangerousOperation,
    /// 多意图澄清
    MultipleIntents,
    /// 时间范围不明确
    MissingTimeRange,
}

/// 追问项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowUpItem {
    /// 追问类型
    pub followup_type: FollowUpType,
    /// 优先级
    pub priority: FollowUpPriority,
    /// 问题文本
    pub question: String,
    /// 建议的回复选项（可选）
    pub suggestions: Vec<String>,
    /// 是否可以继续执行（降级模式）
    pub can_proceed_degraded: bool,
}

/// 多意图检测结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedIntent {
    /// 意图描述
    pub description: String,
    /// 置信度 0-1
    pub confidence: f32,
    /// 相关的设备/位置
    pub entities: Vec<String>,
}

/// 增强的追问分析结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowUpAnalysis {
    /// 是否可以直接执行
    pub can_proceed: bool,
    /// 追问项列表（按优先级排序）
    pub followups: Vec<FollowUpItem>,
    /// 检测到的意图（如果意图明确）
    pub detected_intents: Vec<DetectedIntent>,
    /// 降级执行建议（如果追问被忽略）
    pub fallback_suggestion: Option<String>,
}

/// 可用设备信息（用于动态生成追问）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableDevice {
    pub id: String,
    pub name: String,
    pub location: String,
    pub device_type: String,
    pub capabilities: Vec<String>, // e.g., ["on_off", "brightness", "color_temp"]
}

/// 智能追问管理器
pub struct SmartFollowUpManager {
    /// 资源索引（用于动态获取设备信息）
    resource_index: Arc<RwLock<ResourceIndex>>,
    /// 可用设备列表（缓存）
    available_devices: Vec<AvailableDevice>,
    /// 追问历史（避免重复追问）
    asked_questions: Vec<String>,
    /// 最大追问次数
    max_followups: usize,
}

impl SmartFollowUpManager {
    /// 创建新的智能追问管理器（需要资源索引）
    pub fn with_resource_index(resource_index: Arc<RwLock<ResourceIndex>>) -> Self {
        Self {
            resource_index,
            available_devices: Vec::new(),
            asked_questions: Vec::new(),
            max_followups: 2,
        }
    }

    /// 创建默认的管理器（无资源索引，使用手动设置）
    pub fn new() -> Self {
        Self {
            resource_index: Arc::new(RwLock::new(ResourceIndex::new())),
            available_devices: Vec::new(),
            asked_questions: Vec::new(),
            max_followups: 2,
        }
    }

    /// 从资源索引刷新设备列表
    pub async fn refresh_devices(&mut self) {
        let index = self.resource_index.read().await;
        let resources = index.list_devices().await;

        self.available_devices = resources
            .into_iter()
            .filter_map(|r| {
                // 只处理设备类型的资源
                let device_data = r.as_device()?;

                Some(AvailableDevice {
                    id: r.id.id.clone(),
                    name: r.name.clone(),
                    location: device_data.location.clone().unwrap_or_default(),
                    device_type: device_data.device_type.clone(),
                    capabilities: device_data
                        .capabilities
                        .iter()
                        .map(|c| c.name.clone())
                        .collect(),
                })
            })
            .collect();
    }

    /// 设置可用设备列表（保留向后兼容）
    pub fn set_available_devices(&mut self, devices: Vec<AvailableDevice>) {
        self.available_devices = devices;
    }

    /// 基于模糊搜索获取设备建议
    /// 返回与查询相关的设备名称列表
    pub async fn get_device_suggestions(&self, query: &str, limit: usize) -> Vec<String> {
        let index = self.resource_index.read().await;
        let results = index.search_string(query).await;

        results
            .into_iter()
            .take(limit)
            .map(|r| r.resource.name.clone())
            .collect()
    }

    /// 基于模糊搜索获取位置建议
    pub async fn get_location_suggestions(&self, query: &str, limit: usize) -> Vec<String> {
        let index = self.resource_index.read().await;
        let results = index.search_string(query).await;

        // 提取唯一的位置名称
        let mut locations: Vec<String> = results
            .into_iter()
            .filter_map(|r| r.resource.as_device().and_then(|d| d.location.clone()))
            .collect();

        // 去重并限制数量
        locations.sort();
        locations.dedup();
        locations.truncate(limit);
        locations
    }

    /// 分析用户输入，判断是否需要追问
    pub fn analyze_input(
        &mut self,
        user_input: &str,
        context: &ConversationContext,
    ) -> FollowUpAnalysis {
        let mut followups = Vec::new();
        let mut intents = Vec::new();
        let mut can_proceed = true;

        // 1. 检测危险操作（最高优先级）
        if let Some(dangerous) = self.detect_dangerous_operation(user_input) {
            followups.push(dangerous);
            can_proceed = false;
        }

        // 2. 检测多意图
        if let Some(multiple) = self.detect_multiple_intents(user_input) {
            followups.push(multiple);
            can_proceed = false;
        }

        // 3. 上下文感知的缺失信息检测
        let missing_info = self.detect_missing_info_aware(user_input, context);

        // 4. 意图模糊检测（结合上下文）
        let ambiguous = self.detect_ambiguous_aware(user_input, context);

        followups.extend(missing_info);
        followups.extend(ambiguous);

        // 5. 检测意图
        if !user_input.is_empty() {
            intents.extend(self.detect_intents(user_input, context));
        }

        // 6. 按优先级排序
        followups.sort_by(|a, b| b.priority.cmp(&a.priority));

        // 7. 限制追问数量
        if followups.len() > self.max_followups {
            followups.truncate(self.max_followups);
        }

        // 8. 判断是否可以继续执行
        if !followups.is_empty() {
            // 检查是否所有追问都可以降级执行
            can_proceed = followups.iter().all(|f| f.can_proceed_degraded);
        }

        // 9. 生成降级建议
        let fallback = if !followups.is_empty() && can_proceed {
            self.generate_fallback_suggestion(&followups, user_input, context)
        } else {
            None
        };

        FollowUpAnalysis {
            can_proceed,
            followups,
            detected_intents: intents,
            fallback_suggestion: fallback,
        }
    }

    /// 检测危险操作
    fn detect_dangerous_operation(&self, input: &str) -> Option<FollowUpItem> {
        let lower = input.to_lowercase();

        // 危险操作模式
        let dangerous_patterns = [
            ("删除所有", FollowUpType::DangerousOperation),
            ("删除全部", FollowUpType::DangerousOperation),
            ("关闭所有", FollowUpType::DangerousOperation),
            ("清空规则", FollowUpType::DangerousOperation),
            ("重置系统", FollowUpType::DangerousOperation),
            ("批量删除", FollowUpType::DangerousOperation),
            ("delete all", FollowUpType::DangerousOperation),
        ];

        for (pattern, ftype) in dangerous_patterns {
            if lower.contains(pattern) {
                return Some(FollowUpItem {
                    followup_type: ftype,
                    priority: FollowUpPriority::Critical,
                    question: format!(
                        "⚠️ 确定要「{}」吗？此操作不可恢复。\n请回复「确认」继续，或「取消」放弃。",
                        input
                    ),
                    suggestions: vec!["确认".to_string(), "取消".to_string()],
                    can_proceed_degraded: false,
                });
            }
        }

        None
    }

    /// 检测多意图
    fn detect_multiple_intents(&self, input: &str) -> Option<FollowUpItem> {
        // 检测连接词（中文连接词不需要空格）
        let multi_intent_markers = [
            ("然后", "和"),
            ("之后", "之后"),
            ("接着", "接着"),
            ("并且", "并且"),
            (",然后", "和"),
            ("，然后", "和"),
            (" and ", "and"),
            (" then ", "then"),
        ];

        let lower = input.to_lowercase();
        for (marker, _) in &multi_intent_markers {
            if lower.contains(marker) {
                // 提取多个意图
                let parts: Vec<&str> = input.split(marker).collect();
                if parts.len() >= 2 {
                    let first = parts.first().unwrap_or(&"").trim();
                    let second = parts.get(1).unwrap_or(&"").trim();

                    // 确保两个部分都有实际内容
                    if !first.is_empty() && !second.is_empty() {
                        return Some(FollowUpItem {
                            followup_type: FollowUpType::MultipleIntents,
                            priority: FollowUpPriority::Medium,
                            question: format!(
                                "检测到您想要执行多个操作：\n1. {}\n2. {}\n\n是否按顺序执行？",
                                first, second
                            ),
                            suggestions: vec!["按顺序执行".to_string(), "只执行第一个".to_string()],
                            can_proceed_degraded: true,
                        });
                    }
                }
            }
        }

        None
    }

    /// 上下文感知的缺失信息检测
    fn detect_missing_info_aware(
        &self,
        input: &str,
        context: &ConversationContext,
    ) -> Vec<FollowUpItem> {
        let mut followups = Vec::new();
        let lower = input.to_lowercase();

        // 设备控制类
        if lower.contains("打开") || lower.contains("关闭") || lower.contains("开启") {
            // 检查是否缺少位置信息
            let has_location = self.has_location_info(input, context);

            if !has_location && !self.available_devices.is_empty() {
                // 生成基于可用设备的动态追问
                let locations = self.get_available_locations();
                let suggestions = if locations.len() <= 4 {
                    locations.clone()
                } else {
                    locations.iter().take(4).cloned().collect()
                };

                followups.push(FollowUpItem {
                    followup_type: FollowUpType::MissingLocation,
                    priority: FollowUpPriority::High,
                    question: format!(
                        "请问要控制哪个位置的设备？\n可用位置：{}",
                        suggestions.join("、")
                    ),
                    suggestions,
                    can_proceed_degraded: false,
                });
            }
        }

        // 温度设置类
        if lower.contains("设置")
            && lower.contains("温度")
            && !lower.contains(|c: char| c.is_ascii_digit())
        {
            followups.push(FollowUpItem {
                followup_type: FollowUpType::MissingValue,
                priority: FollowUpPriority::High,
                question: "请问要设置多少度？\n建议范围：16-30°C".to_string(),
                suggestions: vec!["26度".to_string(), "24度".to_string(), "28度".to_string()],
                can_proceed_degraded: false,
            });
        }

        // 查询温湿度
        if (lower == "温度" || lower == "湿度" || lower == "温湿度")
            && context.current_location.is_none()
        {
            let locations = self.get_available_locations();
            if !locations.is_empty() {
                followups.push(FollowUpItem {
                    followup_type: FollowUpType::MissingLocation,
                    priority: FollowUpPriority::High,
                    question: format!(
                        "请问要查看哪个位置的温湿度？\n可用位置：{}",
                        locations.join("、")
                    ),
                    suggestions: locations.iter().take(3).cloned().collect(),
                    can_proceed_degraded: true, // 可以显示所有位置的数据
                });
            }
        }

        followups
    }

    /// 上下文感知的意图模糊检测
    fn detect_ambiguous_aware(
        &self,
        input: &str,
        context: &ConversationContext,
    ) -> Vec<FollowUpItem> {
        let mut followups = Vec::new();
        let lower = input.to_lowercase();

        // 简短输入 + 有上下文 -> 利用上下文推断
        if input.len() < 5 && context.current_device.is_some() {
            // 有上下文，不需要追问
            return followups;
        }

        // "温度" 单独出现
        if lower == "温度" || lower == "气温" {
            followups.push(FollowUpItem {
                followup_type: FollowUpType::AmbiguousIntent,
                priority: FollowUpPriority::Medium,
                question: "您是想：\n1. 查看当前温度\n2. 设置温度\n3. 创建温度相关的自动化规则"
                    .to_string(),
                suggestions: vec!["查看当前温度".to_string(), "设置温度".to_string()],
                can_proceed_degraded: true, // 默认为查看
            });
        }

        // "灯" 单独出现
        if lower == "灯" || lower == "灯光" {
            followups.push(FollowUpItem {
                followup_type: FollowUpType::AmbiguousIntent,
                priority: FollowUpPriority::Medium,
                question: "您是想：\n1. 查看灯的状态\n2. 控制灯的开关\n3. 调节灯的亮度".to_string(),
                suggestions: vec![
                    "查看状态".to_string(),
                    "打开灯".to_string(),
                    "关闭灯".to_string(),
                ],
                can_proceed_degraded: true,
            });
        }

        followups
    }

    /// 检测意图
    fn detect_intents(&self, input: &str, context: &ConversationContext) -> Vec<DetectedIntent> {
        let mut intents = Vec::new();
        let lower = input.to_lowercase();

        // 查询意图
        if lower.contains("查询")
            || lower.contains("查看")
            || lower.contains("多少")
            || lower.contains("温度")
            || lower.contains("湿度")
            || lower.contains("状态")
        {
            intents.push(DetectedIntent {
                description: "查询信息".to_string(),
                confidence: if lower.contains("查询") || lower.contains("查看") {
                    0.9
                } else {
                    0.7
                },
                entities: context
                    .mentioned_devices
                    .iter()
                    .map(|d| d.name.clone())
                    .collect(),
            });
        }

        // 控制意图
        if lower.contains("打开") || lower.contains("关闭") || lower.contains("开启") {
            intents.push(DetectedIntent {
                description: "设备控制".to_string(),
                confidence: 0.95,
                entities: context
                    .mentioned_devices
                    .iter()
                    .map(|d| d.name.clone())
                    .collect(),
            });
        }

        // 设置意图
        if lower.contains("设置") || lower.contains("调节") || lower.contains("调到") {
            intents.push(DetectedIntent {
                description: "参数设置".to_string(),
                confidence: 0.9,
                entities: vec![],
            });
        }

        // 创建规则意图
        if (lower.contains("创建") || lower.contains("添加") || lower.contains("新建"))
            && (lower.contains("规则") || lower.contains("自动化"))
        {
            intents.push(DetectedIntent {
                description: "创建自动化规则".to_string(),
                confidence: 0.95,
                entities: vec![],
            });
        }

        intents
    }

    /// 检查输入是否包含位置信息
    fn has_location_info(&self, input: &str, context: &ConversationContext) -> bool {
        let locations = [
            "客厅",
            "卧室",
            "厨房",
            "浴室",
            "卫生间",
            "书房",
            "阳台",
            "living room",
            "bedroom",
            "kitchen",
            "bathroom",
            "study",
        ];

        let lower = input.to_lowercase();
        let has_explicit = locations.iter().any(|loc| lower.contains(loc));

        // 检查上下文中的位置
        let has_context = context.current_location.is_some();

        has_explicit || has_context
    }

    /// 获取可用位置列表
    fn get_available_locations(&self) -> Vec<String> {
        let mut locations: std::collections::HashSet<String> = std::collections::HashSet::new();

        for device in &self.available_devices {
            if !device.location.is_empty() {
                locations.insert(device.location.clone());
            }
        }

        let mut result: Vec<String> = locations.into_iter().collect();
        result.sort();
        result
    }

    /// 生成降级执行建议
    fn generate_fallback_suggestion(
        &self,
        followups: &[FollowUpItem],
        _original_input: &str,
        context: &ConversationContext,
    ) -> Option<String> {
        if followups.is_empty() {
            return None;
        }

        // 如果只是缺少位置，但有上下文位置
        if followups
            .iter()
            .any(|f| f.followup_type == FollowUpType::MissingLocation)
        {
            if let Some(loc) = &context.current_location {
                return Some(format!("我理解您可能是指「{}」，是否继续？", loc));
            }
        }

        // 如果是模糊意图
        if followups
            .iter()
            .any(|f| f.followup_type == FollowUpType::AmbiguousIntent)
            && context.topic == ConversationTopic::DataQuery
        {
            return Some("我可以先为您查询当前状态".to_string());
        }

        None
    }

    /// 增强版分析：使用模糊搜索提供更智能的建议
    /// 这是 analyze_input 的异步版本，能够利用 ResourceIndex 的模糊搜索能力
    pub async fn analyze_input_with_search(
        &mut self,
        user_input: &str,
        context: &ConversationContext,
    ) -> FollowUpAnalysis {
        let mut analysis = self.analyze_input(user_input, context);

        // 如果有缺失位置的追问，使用模糊搜索增强建议
        for followup in &mut analysis.followups {
            if followup.followup_type == FollowUpType::MissingLocation
                || followup.followup_type == FollowUpType::MissingDevice
            {
                // 使用模糊搜索获取更相关的建议
                let suggestions = self.get_device_suggestions(user_input, 4).await;
                if !suggestions.is_empty() {
                    followup.suggestions = suggestions;
                    followup.question = format!(
                        "{}\n相关设备：{}",
                        followup
                            .question
                            .lines()
                            .next()
                            .unwrap_or(&followup.question),
                        followup.suggestions.join("、")
                    );
                }
            }
        }

        analysis
    }

    /// 清空追问历史
    pub fn clear_history(&mut self) {
        self.asked_questions.clear();
    }

    /// 记录已追问的问题
    pub fn record_asked(&mut self, question: &str) {
        self.asked_questions.push(question.to_string());
    }
}

impl Default for SmartFollowUpManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_context() -> ConversationContext {
        let mut ctx = ConversationContext::new();
        ctx.current_location = Some("客厅".to_string());
        ctx.current_device = Some("客厅空调".to_string());
        ctx
    }

    fn create_manager_with_devices() -> SmartFollowUpManager {
        let mut manager = SmartFollowUpManager::new();
        manager.set_available_devices(vec![
            AvailableDevice {
                id: "1".to_string(),
                name: "客厅灯".to_string(),
                location: "客厅".to_string(),
                device_type: "light".to_string(),
                capabilities: vec!["on_off".to_string(), "brightness".to_string()],
            },
            AvailableDevice {
                id: "2".to_string(),
                name: "卧室空调".to_string(),
                location: "卧室".to_string(),
                device_type: "ac".to_string(),
                capabilities: vec!["on_off".to_string(), "temperature".to_string()],
            },
        ]);
        manager
    }

    #[test]
    fn test_detect_dangerous_operation() {
        let mut manager = SmartFollowUpManager::new();
        let ctx = create_test_context();

        let analysis = manager.analyze_input("删除所有规则", &ctx);

        assert!(!analysis.can_proceed);
        assert_eq!(analysis.followups.len(), 1);
        assert_eq!(
            analysis.followups[0].followup_type,
            FollowUpType::DangerousOperation
        );
        assert_eq!(analysis.followups[0].priority, FollowUpPriority::Critical);
    }

    #[test]
    fn test_context_aware_missing_info() {
        let mut manager = create_manager_with_devices();
        let ctx = create_test_context(); // 有上下文位置

        // 有上下文时，"打开灯"应该不追问
        let analysis = manager.analyze_input("打开灯", &ctx);
        let location_followups: Vec<_> = analysis
            .followups
            .iter()
            .filter(|f| f.followup_type == FollowUpType::MissingLocation)
            .collect();

        assert_eq!(location_followups.len(), 0);
    }

    #[test]
    fn test_missing_info_without_context() {
        let mut manager = create_manager_with_devices();
        let ctx = ConversationContext::new(); // 无上下文

        let analysis = manager.analyze_input("打开灯", &ctx);

        let location_followups: Vec<_> = analysis
            .followups
            .iter()
            .filter(|f| f.followup_type == FollowUpType::MissingLocation)
            .collect();

        assert!(!location_followups.is_empty());
        assert!(location_followups[0].question.contains("客厅"));
        assert!(location_followups[0].question.contains("卧室"));
    }

    #[test]
    fn test_detect_multiple_intents() {
        let mut manager = SmartFollowUpManager::new();
        let ctx = create_test_context();

        let analysis = manager.analyze_input("打开客厅灯然后关闭卧室灯", &ctx);

        // 多意图检测到，但可以降级执行
        let multi_intent: Vec<_> = analysis
            .followups
            .iter()
            .filter(|f| f.followup_type == FollowUpType::MultipleIntents)
            .collect();

        assert!(!multi_intent.is_empty());
        // 多意图追问可以降级执行（can_proceed_degraded = true）
        assert!(analysis.can_proceed); // 因为所有追问都可以降级执行
    }

    #[test]
    fn test_intent_detection() {
        let mut manager = SmartFollowUpManager::new();
        let ctx = create_test_context();

        let analysis = manager.analyze_input("查询客厅温度", &ctx);

        assert!(!analysis.detected_intents.is_empty());
        assert!(analysis
            .detected_intents
            .iter()
            .any(|i| i.description.contains("查询")));
    }

    #[test]
    fn test_followup_priority_ordering() {
        let mut manager = SmartFollowUpManager::new();
        let ctx = create_test_context();

        let analysis = manager.analyze_input("打开灯", &ctx);

        // 验证追问按优先级排序（高到低）
        for i in 1..analysis.followups.len() {
            assert!(analysis.followups[i - 1].priority >= analysis.followups[i].priority);
        }
    }

    #[test]
    fn test_fallback_suggestion_with_context() {
        let mut manager = create_manager_with_devices();
        let mut ctx = ConversationContext::new();
        ctx.current_location = Some("客厅".to_string());

        let analysis = manager.analyze_input("打开灯", &ctx);

        // 降级建议应该利用上下文位置
        if let Some(fallback) = analysis.fallback_suggestion {
            assert!(fallback.contains("客厅"));
        }
    }
}
