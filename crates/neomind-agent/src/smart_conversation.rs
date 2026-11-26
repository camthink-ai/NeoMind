//! 智能对话中间层
//!
//! 在 LLM 处理之前/之后进行决策，实现：
//! 1. 信息不足时拦截并追问
//! 2. 危险操作前拦截并确认
//! 3. 意图模糊时拦截并澄清

/// 用户意图分析结果
#[derive(Debug, Clone)]
pub struct IntentAnalysis {
    /// 是否信息不足
    pub missing_info: Option<String>,
    /// 是否危险操作
    pub requires_confirmation: Option<String>,
    /// 是否意图模糊
    pub ambiguous: Option<String>,
    /// 是否直接执行
    pub can_proceed: bool,
}

/// 智能对话管理器
pub struct SmartConversationManager;

impl SmartConversationManager {
    pub fn new() -> Self {
        Self
    }

    /// 分析用户输入，判断是否需要拦截
    pub fn analyze_input(&self, user_input: &str) -> IntentAnalysis {
        let input_lower = user_input.to_lowercase();

        // 1. 检测危险操作 - 必须确认
        if self.is_dangerous_operation(&input_lower) {
            return IntentAnalysis {
                missing_info: None,
                requires_confirmation: Some(format!(
                    "确定要{}吗？此操作不可恢复。回复'确认'继续。",
                    user_input
                )),
                ambiguous: None,
                can_proceed: false,
            };
        }

        // 2. 检测信息不足 - 需要追问
        if let Some(question) = self.detect_missing_info(user_input, &input_lower) {
            return IntentAnalysis {
                missing_info: Some(question),
                requires_confirmation: None,
                ambiguous: None,
                can_proceed: false,
            };
        }

        // 3. 检测意图模糊 - 需要澄清
        if let Some(clarification) = self.detect_ambiguous_intent(user_input, &input_lower) {
            return IntentAnalysis {
                missing_info: None,
                requires_confirmation: None,
                ambiguous: Some(clarification),
                can_proceed: false,
            };
        }

        // 可以直接执行
        IntentAnalysis {
            missing_info: None,
            requires_confirmation: None,
            ambiguous: None,
            can_proceed: true,
        }
    }

    /// 检测是否为危险操作
    fn is_dangerous_operation(&self, input: &str) -> bool {
        // 使用简单的字符串匹配
        let dangerous_keywords = [
            "删除所有",
            "删除全部",
            "关闭所有",
            "关闭全部",
            "清空规则",
            "重置系统",
            "批量删除",
            "删除全部规则",
            "删除所有规则",
        ];

        for keyword in &dangerous_keywords {
            if input.contains(keyword) {
                return true;
            }
        }

        false
    }

    /// 检测信息是否不足（仅对极短的裸开关指令生效，避免误伤 Agent/规则/工具配置等完整指令）
    fn detect_missing_info(&self, _original: &str, lower: &str) -> Option<String> {
        let trimmed = lower.trim();
        let is_short_bare_control = trimmed.chars().count() <= 8
            && (trimmed.contains("打开") || trimmed.contains("关闭") || trimmed.contains("开启"))
            && !trimmed.contains("agent")
            && !trimmed.contains("工具")
            && !trimmed.contains("告警")
            && !trimmed.contains("规则")
            && !trimmed.contains("通知");
        if is_short_bare_control {
            // 检查是否指定了位置或具体设备
            let locations = ["客厅", "卧室", "厨房", "浴室", "书房", "阳台"];
            let has_specific_location = locations.iter().any(|loc| trimmed.contains(loc));

            // 具体设备名称（包含位置前缀的）
            let specific_devices = [
                "客厅灯",
                "卧室空调",
                "厨房灯",
                "浴室灯",
                "主灯",
                "筒灯",
                "射灯",
            ];
            let has_specific_device = specific_devices.iter().any(|dev| trimmed.contains(dev));

            if !has_specific_location && !has_specific_device {
                return Some(
                    "请问要控制哪个位置的设备？例如：客厅灯、卧室空调、厨房灯等。".to_string(),
                );
            }
        }

        // 查询数据相关 - 只检查非常短的输入
        if lower == "温度" || lower == "湿度" {
            return Some("请问要查看哪个房间的温湿度？例如：客厅、卧室、厨房等。".to_string());
        }

        None
    }

    /// 检测意图是否模糊
    fn detect_ambiguous_intent(&self, original: &str, lower: &str) -> Option<String> {
        // 单个词输入通常是模糊的
        if original.len() < 5 {
            if lower.contains("温度") {
                return Some("您是想查看当前温度，还是设置温度阈值？".to_string());
            }
            if lower.contains("灯") {
                return Some("您是想查看灯的状态，还是控制灯的开关？".to_string());
            }
            if lower.contains("空调") {
                return Some("您是想查看空调状态，还是调节空调温度？".to_string());
            }
        }

        None
    }
}

impl Default for SmartConversationManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_dangerous_operation() {
        let manager = SmartConversationManager::new();

        assert!(manager.is_dangerous_operation("删除所有规则"));
        assert!(manager.is_dangerous_operation("关闭所有设备"));
        assert!(manager.is_dangerous_operation("批量删除规则"));
        assert!(!manager.is_dangerous_operation("打开客厅灯"));
        assert!(!manager.is_dangerous_operation("查看温度"));
    }

    #[test]
    fn test_detect_missing_info() {
        let manager = SmartConversationManager::new();

        let result1 = manager.detect_missing_info("打开灯", "打开灯");
        assert!(result1.is_some());

        let result2 = manager.detect_missing_info("打开客厅灯", "打开客厅灯");
        assert!(result2.is_none());

        let result3 = manager.detect_missing_info("温度", "温度");
        assert!(result3.is_some());
    }

    #[test]
    fn test_analyze_input() {
        let manager = SmartConversationManager::new();

        // 危险操作
        let analysis1 = manager.analyze_input("删除所有规则");
        assert!(!analysis1.can_proceed);
        assert!(analysis1.requires_confirmation.is_some());

        // 信息不足
        let analysis2 = manager.analyze_input("打开灯");
        assert!(!analysis2.can_proceed);
        assert!(analysis2.missing_info.is_some());

        // 正常输入
        let analysis3 = manager.analyze_input("列出所有设备");
        assert!(analysis3.can_proceed);
    }
}
