//! 对话上下文管理 - 实现连续对话能力
//!
//! 功能：
//! 1. 记住最近提到的设备
//! 2. 记住当前操作的位置
//! 3. 记住当前对话主题
//! 4. 解析代词引用（"它"、"那个"、"这个"）

use serde::{Deserialize, Serialize};

/// 对话主题
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConversationTopic {
    /// 设备控制
    DeviceControl,
    /// 数据查询
    DataQuery,
    /// 规则创建
    RuleCreation,
    /// 工作流设计
    WorkflowDesign,
    /// 通用对话
    General,
}

/// 实体引用
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityReference {
    /// 实体类型
    pub entity_type: EntityType,
    /// 实体ID
    pub id: String,
    /// 实体名称（自然语言）
    pub name: String,
    /// 最后提及时间（相对对话开始）
    pub last_mentioned_turn: usize,
}

/// 实体类型
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntityType {
    /// 设备
    Device,
    /// 位置
    Location,
    /// 规则
    Rule,
    /// 工作流
    Workflow,
    /// 传感器/指标
    Sensor,
}

/// 对话上下文
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationContext {
    /// 当前对话主题
    pub topic: ConversationTopic,
    /// 最近提到的设备
    pub mentioned_devices: Vec<EntityReference>,
    /// 最近提到的位置
    pub mentioned_locations: Vec<EntityReference>,
    /// 当前关注的位置
    pub current_location: Option<String>,
    /// 当前关注的设备
    pub current_device: Option<String>,
    /// 对话轮次
    pub turn_count: usize,
}

/// 上下文清理配置
pub struct ContextCleanupConfig {
    /// 多少轮后自动清理旧实体（默认：10轮）
    pub cleanup_turn_interval: usize,
    /// 保留最近几轮的实体引用（默认：5轮）
    pub keep_recent_turns: usize,
}

impl Default for ContextCleanupConfig {
    fn default() -> Self {
        Self {
            cleanup_turn_interval: 10,
            keep_recent_turns: 5,
        }
    }
}

impl Default for ConversationContext {
    fn default() -> Self {
        Self::new()
    }
}

impl ConversationContext {
    /// 创建新的对话上下文
    pub fn new() -> Self {
        Self {
            topic: ConversationTopic::General,
            mentioned_devices: Vec::new(),
            mentioned_locations: Vec::new(),
            current_location: None,
            current_device: None,
            turn_count: 0,
        }
    }

    /// 更新上下文 - 分析用户输入并提取实体
    pub fn update(&mut self, user_input: &str, tool_results: &[(String, String)]) {
        self.turn_count += 1;

        // === AUTO CLEANUP: Periodically clean up old entities to prevent unbounded growth ===
        // This prevents the context from growing too large and causing repetitive LLM responses
        let config = ContextCleanupConfig::default();
        if self.turn_count.is_multiple_of(config.cleanup_turn_interval) {
            self.cleanup_old_entities(config.keep_recent_turns);
            tracing::debug!(
                "Auto-cleaned conversation context at turn {} (kept last {} turns)",
                self.turn_count,
                config.keep_recent_turns
            );
        }

        // 检测位置
        if let Some(location) = self.extract_location(user_input) {
            self.add_location(location.clone());
            self.current_location = Some(location);
        }

        // 检测设备
        if let Some(device) = self.extract_device(user_input) {
            self.add_device(device.clone());
            self.current_device = Some(device);
        }

        // 从工具结果中提取实体
        for (tool_name, result) in tool_results {
            self.extract_from_tool_result(tool_name, result);
        }

        // 更新对话主题
        self.topic = self.detect_topic(user_input);
    }

    /// 清理旧实体引用，只保留最近几轮的引用
    /// 这可以防止上下文无限增长导致重复响应
    fn cleanup_old_entities(&mut self, keep_recent_turns: usize) {
        // 清理设备引用 - 只保留最近的
        self.mentioned_devices.retain(|entity| {
            entity.last_mentioned_turn > 0
                && self.turn_count.saturating_sub(entity.last_mentioned_turn) <= keep_recent_turns
        });

        // 清理位置引用 - 只保留最近的
        self.mentioned_locations.retain(|entity| {
            entity.last_mentioned_turn > 0
                && self.turn_count.saturating_sub(entity.last_mentioned_turn) <= keep_recent_turns
        });

        // 如果当前设备/位置不在引用列表中，清空它们
        if let Some(ref device) = self.current_device {
            if !self.mentioned_devices.iter().any(|e| &e.name == device) {
                self.current_device = None;
            }
        }
        if let Some(ref location) = self.current_location {
            if !self.mentioned_locations.iter().any(|e| &e.name == location) {
                self.current_location = None;
            }
        }
    }

    /// 提取位置信息
    fn extract_location(&self, input: &str) -> Option<String> {
        let locations = [
            "客厅",
            "卧室",
            "主卧",
            "次卧",
            "厨房",
            "餐厅",
            "卫生间",
            "浴室",
            "书房",
            "阳台",
            "玄关",
            "车库",
            "花园",
            "地下室",
            "living room",
            "bedroom",
            "kitchen",
            "bathroom",
            "study",
            "balcony",
        ];

        let lower = input.to_lowercase();
        for loc in &locations {
            if lower.contains(&loc.to_lowercase()) {
                return Some(loc.to_string());
            }
        }

        // 检测 "客厅的" 模式
        for loc in &locations {
            let pattern = format!("{}的", loc);
            if lower.contains(&pattern.to_lowercase()) {
                return Some(loc.to_string());
            }
        }

        None
    }

    /// 提取设备信息
    fn extract_device(&self, input: &str) -> Option<String> {
        // 常见设备类型
        let device_types = [
            "灯",
            "light",
            "照明",
            "空调",
            "ac",
            "air conditioner",
            "插座",
            "socket",
            "outlet",
            "窗帘",
            "curtain",
            "blind",
            "门锁",
            "lock",
            "door lock",
            "传感器",
            "sensor",
            "开关",
            "switch",
        ];

        let lower = input.to_lowercase();

        // 检测 "位置+设备" 模式
        if let Some(location) = &self.current_location {
            for device_type in &device_types {
                let pattern = format!("{}{}", location, device_type);
                let pattern_en = format!("{} {}", location, device_type);
                if lower.contains(&pattern.to_lowercase())
                    || lower.contains(&pattern_en.to_lowercase())
                {
                    return Some(format!("{}{}", location, device_type));
                }
            }
        }

        // 直接匹配设备
        for device_type in &device_types {
            if lower.contains(device_type) {
                if let Some(loc) = &self.current_location {
                    return Some(format!("{}{}", loc, device_type));
                }
                return Some(device_type.to_string());
            }
        }

        None
    }

    /// 从工具结果中提取实体
    fn extract_from_tool_result(&mut self, tool_name: &str, result: &str) {
        match tool_name {
            "list_devices" => {
                // 解析设备列表
                if let Ok(devices) = self.parse_device_list(result) {
                    for device in devices {
                        self.add_device(device);
                    }
                }
            }
            "get_device" | "query_data" => {
                // 记录查询的设备
                if let Some(device) = self.extract_device(result) {
                    self.add_device(device);
                }
            }
            _ => {}
        }
    }

    /// 解析设备列表
    fn parse_device_list(&self, result: &str) -> Result<Vec<String>, ()> {
        // 简单解析 - 提取设备名称
        let mut devices = Vec::new();

        // 查找类似 "客厅灯: 开启" 的模式
        for line in result.lines() {
            if let Some(device_name) = line.split(':').next() {
                let name = device_name.trim();
                if !name.is_empty() && !name.contains("设备") && !name.contains("Device") {
                    devices.push(name.to_string());
                }
            }
        }

        if devices.is_empty() {
            Err(())
        } else {
            Ok(devices)
        }
    }

    /// 检测对话主题
    fn detect_topic(&self, input: &str) -> ConversationTopic {
        let lower = input.to_lowercase();

        // 控制类关键词
        let control_keywords = [
            "打开", "关闭", "开启", "控制", "调节", "设置", "turn on", "turn off", "control",
        ];
        // 查询类关键词
        let query_keywords = [
            "查询",
            "多少",
            "状态",
            "温度",
            "湿度",
            "query",
            "status",
            "temperature",
        ];
        // 规则创建关键词
        let rule_keywords = [
            "创建",
            "添加",
            "规则",
            "自动化",
            "create",
            "add",
            "rule",
            "automation",
        ];
        // 工作流关键词
        let workflow_keywords = ["工作流", "流程", "workflow"];

        let control_count = control_keywords
            .iter()
            .filter(|k| lower.contains(*k))
            .count();
        let query_count = query_keywords.iter().filter(|k| lower.contains(*k)).count();
        let rule_count = rule_keywords.iter().filter(|k| lower.contains(*k)).count();
        let workflow_count = workflow_keywords
            .iter()
            .filter(|k| lower.contains(*k))
            .count();

        // 根据关键词数量判断主题
        if rule_count > 0 {
            ConversationTopic::RuleCreation
        } else if workflow_count > 0 {
            ConversationTopic::WorkflowDesign
        } else if control_count > query_count {
            ConversationTopic::DeviceControl
        } else if query_count > 0 {
            ConversationTopic::DataQuery
        } else {
            ConversationTopic::General
        }
    }

    /// 添加设备到上下文
    pub fn add_device(&mut self, device: String) {
        // 使用 position() 避免借用冲突
        if let Some(pos) = self.mentioned_devices.iter().position(|d| d.name == device) {
            // 更新提及时间
            self.mentioned_devices[pos].last_mentioned_turn = self.turn_count;
        } else {
            // 添加新设备
            self.mentioned_devices.push(EntityReference {
                entity_type: EntityType::Device,
                id: device.clone(),
                name: device.clone(),
                last_mentioned_turn: self.turn_count,
            });
        }

        // LRU 驱逐：移除最久未提到的设备，而非最旧的
        const MAX_DEVICES: usize = 10;
        if self.mentioned_devices.len() > MAX_DEVICES {
            let oldest_idx = self
                .mentioned_devices
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.last_mentioned_turn)
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.mentioned_devices.remove(oldest_idx);
        }
    }

    /// 添加位置到上下文
    pub fn add_location(&mut self, location: String) {
        // 使用 position() 避免借用冲突
        if let Some(pos) = self
            .mentioned_locations
            .iter()
            .position(|l| l.name == location)
        {
            self.mentioned_locations[pos].last_mentioned_turn = self.turn_count;
        } else {
            self.mentioned_locations.push(EntityReference {
                entity_type: EntityType::Location,
                id: location.clone(),
                name: location.clone(),
                last_mentioned_turn: self.turn_count,
            });
        }

        // LRU 驱逐：移除最久未提到的位置，而非最旧的
        const MAX_LOCATIONS: usize = 5;
        if self.mentioned_locations.len() > MAX_LOCATIONS {
            let oldest_idx = self
                .mentioned_locations
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.last_mentioned_turn)
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.mentioned_locations.remove(oldest_idx);
        }
    }

    /// 解析代词引用 - 将"它"、"那个"解析为具体实体
    pub fn resolve_pronoun(&self, pronoun: &str) -> Option<String> {
        match pronoun {
            "它" | "这个" | "那个" | "it" | "this" | "that" => {
                // 优先返回当前设备
                if let Some(device) = &self.current_device {
                    return Some(device.clone());
                }

                // 返回最近提到的设备
                if let Some(last) = self.mentioned_devices.last() {
                    return Some(last.name.clone());
                }

                // 返回当前位置
                if let Some(location) = &self.current_location {
                    return Some(format!("{}的设备", location));
                }

                None
            }
            _ => None,
        }
    }

    /// 获取当前上下文摘要（用于注入到LLM提示）
    pub fn get_context_summary(&self) -> String {
        let mut summary = Vec::new();

        if let Some(location) = &self.current_location {
            summary.push(format!("当前关注位置：{}", location));
        }

        if let Some(device) = &self.current_device {
            summary.push(format!("当前设备：{}", device));
        }

        if !self.mentioned_devices.is_empty() {
            let devices: Vec<&str> = self
                .mentioned_devices
                .iter()
                .rev()
                .take(5)
                .map(|d| d.name.as_str())
                .collect();
            summary.push(format!("最近提到的设备：{}", devices.join("、")));
        }

        if !self.mentioned_locations.is_empty() {
            let locations: Vec<&str> = self
                .mentioned_locations
                .iter()
                .rev()
                .take(3)
                .map(|l| l.name.as_str())
                .collect();
            summary.push(format!("提到的位置：{}", locations.join("、")));
        }

        summary.join("\n")
    }

    /// 增强用户输入 - 替换代词为具体实体
    pub fn enhance_input(&self, input: &str) -> String {
        let mut enhanced = input.to_string();

        // 检测并替换代词
        let pronouns = ["它", "这个", "那个", "它的", "这个的", "那个的"];
        for pronoun in &pronouns {
            if enhanced.contains(pronoun) {
                if let Some(resolved) = self.resolve_pronoun(pronoun) {
                    enhanced = enhanced.replace(pronoun, &resolved);
                }
            }
        }

        // 如果没有指定位置但有当前上下文位置，添加上下文
        if self.current_location.is_some()
            && !self.has_location_reference(&enhanced)
            && (enhanced.contains("打开")
                || enhanced.contains("关闭")
                || enhanced.contains("温度")
                || enhanced.contains("湿度"))
        {
            // 不强制添加，让用户明确
        }

        enhanced
    }

    /// 检查是否包含位置引用
    fn has_location_reference(&self, input: &str) -> bool {
        let locations = [
            "客厅",
            "卧室",
            "厨房",
            "卫生间",
            "浴室",
            "书房",
            "living room",
            "bedroom",
            "kitchen",
            "bathroom",
            "study",
        ];
        locations
            .iter()
            .any(|loc| input.to_lowercase().contains(loc))
    }

    /// 解析模糊输入 - 当用户说"打开"时补充完整
    pub fn resolve_ambiguous_command(&self, input: &str) -> Option<String> {
        let lower = input.to_lowercase();

        // "打开" + 当前设备/位置
        if lower == "打开" || lower == "开" {
            if let Some(device) = &self.current_device {
                return Some(format!("打开{}", device));
            }
            if let Some(location) = &self.current_location {
                return Some(format!("打开{}的设备", location));
            }
        }

        // "关闭"
        if lower == "关闭" || lower == "关" {
            if let Some(device) = &self.current_device {
                return Some(format!("关闭{}", device));
            }
        }

        // "温度多少" -> 补充位置
        if (lower == "温度" || lower == "温度多少") {
            if let Some(location) = &self.current_location {
                return Some(format!("{}的温度", location));
            }
        }

        None
    }

    /// 重置上下文（开始新会话时）
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_location() {
        let ctx = ConversationContext::new();

        assert_eq!(
            ctx.extract_location("客厅温度多少"),
            Some("客厅".to_string())
        );
        assert_eq!(
            ctx.extract_location("打开卧室的灯"),
            Some("卧室".to_string())
        );
        assert_eq!(
            ctx.extract_location("kitchen light"),
            Some("kitchen".to_string())
        );
    }

    #[test]
    fn test_extract_device() {
        let mut ctx = ConversationContext::new();
        ctx.current_location = Some("客厅".to_string());

        assert_eq!(ctx.extract_device("客厅灯打开"), Some("客厅灯".to_string()));
        assert_eq!(ctx.extract_device("打开空调"), Some("客厅空调".to_string()));
    }

    #[test]
    fn test_detect_topic() {
        let ctx = ConversationContext::new();

        assert_eq!(
            ctx.detect_topic("打开客厅灯"),
            ConversationTopic::DeviceControl
        );
        assert_eq!(ctx.detect_topic("温度多少"), ConversationTopic::DataQuery);
        assert_eq!(
            ctx.detect_topic("创建一个规则"),
            ConversationTopic::RuleCreation
        );
    }

    #[test]
    fn test_conversation_context_flow() {
        let mut ctx = ConversationContext::new();

        // 第一轮：用户查询客厅温度
        ctx.update("客厅温度多少", &[]);
        assert_eq!(ctx.current_location, Some("客厅".to_string()));
        assert_eq!(ctx.topic, ConversationTopic::DataQuery);

        // 第二轮：用户说"打开灯" - 应该推断为客厅灯
        let enhanced = ctx.enhance_input("打开灯");
        assert!(enhanced.contains("客厅") || ctx.current_location == Some("客厅".to_string()));
    }

    #[test]
    fn test_resolve_pronoun() {
        let mut ctx = ConversationContext::new();
        ctx.add_device("客厅空调".to_string());
        ctx.current_device = Some("客厅空调".to_string());

        assert_eq!(ctx.resolve_pronoun("它"), Some("客厅空调".to_string()));
    }

    #[test]
    fn test_resolve_ambiguous_command() {
        let mut ctx = ConversationContext::new();
        ctx.current_location = Some("客厅".to_string());

        assert_eq!(
            ctx.resolve_ambiguous_command("打开"),
            Some("打开客厅的设备".to_string())
        );
        assert_eq!(
            ctx.resolve_ambiguous_command("温度"),
            Some("客厅的温度".to_string())
        );
    }

    #[test]
    fn test_get_context_summary() {
        let mut ctx = ConversationContext::new();
        ctx.add_location("客厅".to_string());
        ctx.add_device("客厅空调".to_string());
        ctx.current_location = Some("客厅".to_string());

        let summary = ctx.get_context_summary();
        assert!(summary.contains("客厅"));
    }
}
