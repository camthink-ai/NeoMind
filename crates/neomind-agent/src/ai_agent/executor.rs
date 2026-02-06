//! AI Agent executor - runs agents and records decision processes.

use neomind_core::{EventBus, MetricValue, NeoMindEvent, message::{Content, ContentPart, Message, MessageRole}};
use neomind_storage::{
    AgentMemory, AgentStore, AgentExecutionRecord, AiAgent, DataCollected,
    Decision, DecisionProcess, ExecutionResult as StorageExecutionResult, ExecutionStatus,
    GeneratedReport, ReasoningStep, TrendPoint, AgentResource, ResourceType, LearnedPattern,
    // New conversation types
    ConversationTurn, TurnInput, TurnOutput,
    LlmBackendStore,
};
use neomind_devices::DeviceService;
use neomind_messages::MessageManager;
use neomind_llm::{OllamaConfig, OllamaRuntime, CloudConfig, CloudRuntime};
use neomind_core::llm::backend::LlmRuntime;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use futures::future::join_all;

use crate::LlmBackend;
use crate::error::{NeoMindError, Result as AgentResult};
use crate::prompts::CONVERSATION_CONTEXT_ZH;

/// Internal representation of image content for multimodal LLM messages.
#[allow(dead_code)]
enum ImageContent {
    Url(String),
    Base64(String, String), // (_data, _mime_type)
}

/// Extract command name from decision description.
/// Supports formats like "execute command: turn_on_light" or "execute: open_valve"
fn extract_command_from_description(description: &str) -> Option<String> {
    // Use a helper to find patterns case-insensitively
    // and return the trimmed result as a String
    fn find_and_extract(text: &str, pattern: &str, pattern_len: usize) -> Option<String> {
        let text_lower = text.to_lowercase();
        if let Some(idx) = text_lower.find(pattern) {
            let after = &text[idx + pattern_len..];
            // Trim leading whitespace and extract first word
            let cmd = after.split_whitespace().next().unwrap_or(after);
            // Trim trailing non-alphanumeric characters (except underscore)
            let cmd = cmd.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_');
            if !cmd.is_empty() {
                return Some(cmd.to_string());
            }
        }
        None
    }

    // Try patterns in order of specificity
    find_and_extract(description, "command:", 8)
        .or_else(|| find_and_extract(description, "execute:", 8))
        .or_else(|| find_and_extract(description, "execute ", 8))
}

/// Extract device ID from decision description.
/// Supports formats like "on device: thermostat" or "device: sensor1"
fn extract_device_from_description(description: &str) -> Option<String> {
    fn find_and_extract(text: &str, pattern: &str, pattern_len: usize) -> Option<String> {
        let text_lower = text.to_lowercase();
        if let Some(idx) = text_lower.find(pattern) {
            let after = &text[idx + pattern_len..];
            let device = after.split_whitespace().next().unwrap_or(after);
            let device = device.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_');
            if !device.is_empty() {
                return Some(device.to_string());
            }
        }
        None
    }

    find_and_extract(description, "device:", 7)
        .or_else(|| find_and_extract(description, "device ", 7))
        .or_else(|| find_and_extract(description, "on ", 3))
}

/// Get time context string for LLM prompts.
/// This provides the LLM with current time information for better temporal understanding.
/// Loads timezone from settings if available, otherwise uses default.
fn get_time_context() -> String {
    use neomind_storage::SettingsStore;

    const SETTINGS_DB_PATH: &str = "data/settings.redb";

    // Try to load timezone from settings
    let timezone = SettingsStore::open(SETTINGS_DB_PATH)
        .ok()
        .and_then(|store| store.load_global_timezone().ok())
        .flatten()
        .unwrap_or_else(|| "Asia/Shanghai".to_string());

    let now = chrono::Utc::now();

    // Parse timezone
    let tz = timezone.parse::<chrono_tz::Tz>()
        .unwrap_or(chrono_tz::Tz::Asia__Shanghai);

    // Get current time in the configured timezone
    let local_now = now.with_timezone(&tz);

    // Format various time components
    let utc_time = now.format("%Y-%m-%d %H:%M:%S UTC").to_string();
    let local_time = local_now.format("%Y-%m-%d %H:%M:%S").to_string();
    let date_str = local_now.format("%Y年%m月%d日").to_string();
    let day_of_week = local_now.format("%A").to_string(); // Monday, Tuesday, etc.

    // Get time period description - use format to get hour value
    let hour_str = local_now.format("%H").to_string();
    let hour: u32 = hour_str.parse().unwrap_or(12);
    let time_period = match hour {
        5..=11 => "上午",
        12..=13 => "中午",
        14..=17 => "下午",
        18..=22 => "晚上",
        _ => "夜间",
    };

    format!(
        "### 当前时间信息\n\
         - UTC时间: {}\n\
         - 本地时间: {} ({})\n\
         - 日期: {}\n\
         - 星期: {}\n\
         - 时段: {}",
        utc_time, local_time, timezone, date_str, day_of_week, time_period
    )
}

/// Extract JSON from mixed text (when model outputs text before JSON).
/// Returns the extracted JSON string if found, None otherwise.
fn extract_json_from_mixed_text(text: &str) -> Option<String> {
    // Find the first '{' that starts a JSON object
    let start_idx = text.find('{')?;
    let potential_json = &text[start_idx..];

    // Count braces to find the matching closing brace
    let mut open_braces = 0;
    let mut in_string = false;
    let mut escape_next = false;
    let mut end_idx = 0;

    for (i, ch) in potential_json.chars().enumerate() {
        match ch {
            '\\' if in_string => escape_next = true,
            '"' if !escape_next => in_string = !in_string,
            '{' if !in_string => open_braces += 1,
            '}' if !in_string => {
                open_braces -= 1;
                if open_braces == 0 {
                    end_idx = i + 1;
                    break;
                }
            }
            _ => {}
        }
        if escape_next && ch != '\\' {
            escape_next = false;
        }
    }

    if end_idx > 0 {
        let json_str = &potential_json[..end_idx];
        // Validate it's actually JSON
        if serde_json::from_str::<serde_json::Value>(json_str).is_ok() {
            return Some(json_str.to_string());
        }
    }

    None
}

/// Attempts to recover a truncated JSON string by finding the last complete object.
/// Returns Some((recovered_json, was_truncated)) if recovery was possible,
/// None if the JSON is beyond recovery.
fn try_recover_truncated_json(json_str: &str) -> Option<(String, bool)> {
    let trimmed = json_str.trim();

    // First, try to close any open objects/arrays
    let mut recovered = trimmed.to_string();
    let mut open_braces: usize = 0;
    let mut open_brackets: usize = 0;
    let mut in_string = false;
    let mut escape_next = false;

    for ch in trimmed.chars() {
        match ch {
            '\\' if in_string => escape_next = true,
            '"' if !escape_next => in_string = !in_string,
            '{' if !in_string => open_braces += 1,
            '}' if !in_string => open_braces = open_braces.saturating_sub(1),
            '[' if !in_string => open_brackets += 1,
            ']' if !in_string => open_brackets = open_brackets.saturating_sub(1),
                            _ => {}
        }
        if escape_next && ch != '\\' {
            escape_next = false;
        }
    }

    // If no unclosed braces, JSON might be complete
    if open_braces == 0 && open_brackets == 0 {
        // Still might be truncated mid-string, try parsing
        if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
            return Some((trimmed.to_string(), false));
        }
    }

    // Try to close the objects
    for _ in 0..open_brackets {
        recovered.push(']');
    }
    for _ in 0..open_braces {
        recovered.push('}');
    }

    // Check if recovered JSON is valid
    if serde_json::from_str::<serde_json::Value>(&recovered).is_ok() {
        return Some((recovered, true));
    }

    // Try more aggressive recovery: find the last complete "step" object
    // This handles cases where the JSON is truncated in the middle of reasoning_steps
    if let Some(last_complete_idx) = trimmed.rfind(r#"  }"#) {
        let truncated = &trimmed[..last_complete_idx + 4];
        // Try to close the arrays and objects
        let mut closed = truncated.to_string();
        if trimmed.contains("reasoning_steps") {
            closed.push_str("\n  ]");
        }
        if trimmed.contains("decisions") {
            closed.push_str(",\n  \"decisions\": []");
        }
        closed.push_str("\n}");
        if serde_json::from_str::<serde_json::Value>(&closed).is_ok() {
            return Some((closed, true));
        }
    }

    // Last resort: return None to signal using raw text fallback
    None
}

/// Extract semantic patterns from decisions based on Claude Code's approach.
/// Returns abstract patterns {symptom, cause, solution} instead of raw history.
fn extract_semantic_patterns(
    decisions: &[Decision],
    situation_analysis: &str,
    _data: &[DataCollected],
    baselines: &HashMap<String, f64>,
) -> Vec<LearnedPattern> {
    let mut patterns = Vec::new();
    let now = chrono::Utc::now().timestamp();

    for decision in decisions {
        if decision.decision_type.is_empty() {
            continue;
        }

        // Extract pattern type
        let pattern_type = match decision.decision_type.as_str() {
            "alert" => "anomaly_detection",
            "command" => "automated_control",
            "info" => "information_logging",
            _ => "general_pattern",
        };

        // Extract symptom (what condition triggered this)
        let symptom = extract_symptom(situation_analysis, decision);

        // Extract threshold/value if applicable
        let threshold = extract_threshold_from_data(_data, baselines);

        // Build pattern data
        let pattern_data = serde_json::json!({
            "symptom": symptom,
            "action": decision.action,
            "threshold": threshold,
            "trigger_conditions": extract_trigger_conditions(decision),
        });

        // Default confidence: higher for alerts and commands
        let confidence = match decision.decision_type.as_str() {
            "alert" | "command" => 0.8,
            _ => 0.6,
        };

        // Optimize ID allocation with pre-allocated capacity
        let id = format!("{}:{}", pattern_type, now);

        let pattern = LearnedPattern {
            id,
            pattern_type: pattern_type.to_string(),
            description: extract_semantic_description(decision, &symptom),
            confidence,
            learned_at: now,
            data: pattern_data,
        };

        patterns.push(pattern);
    }

    patterns
}

/// Extract the symptom (condition) that triggered a decision.
/// Returns static string slices where possible to avoid allocations.
fn extract_symptom(
    situation_analysis: &str,
    decision: &Decision,
) -> String {
    // Try to extract from situation analysis - use static strings for common cases
    if !situation_analysis.is_empty() {
        // Look for key phrases indicating conditions
        if situation_analysis.contains("超过") || situation_analysis.contains("高于") {
            return "数值超过阈值".to_string();
        }
        if situation_analysis.contains("低于") {
            return "数值低于阈值".to_string();
        }
        if situation_analysis.contains("异常") || situation_analysis.contains("不正常") {
            return "检测到异常状态".to_string();
        }
        if situation_analysis.contains("正常") || situation_analysis.contains("稳定") {
            return "状态正常".to_string();
        }
    }

    // Fallback to decision type - use static strings
    match decision.decision_type.as_str() {
        "alert" => "检测到需要告警的情况".to_string(),
        "command" => "满足自动化执行条件".to_string(),
        _ => "常规检查".to_string(),
    }
}

/// Extract numeric threshold from data and baselines.
fn extract_threshold_from_data(
    data: &[DataCollected],
    baselines: &HashMap<String, f64>,
) -> Option<f64> {
    // Try to extract numeric value from decision description
    for item in data {
        if let Some(val) = item.values.get("value")
            && let Some(num) = val.as_f64() {
                // Check if baseline exists
                if let Some(&baseline) = baselines.get(&item.source) {
                    let deviation = ((num - baseline) / baseline * 100.0).abs();
                    if deviation > 10.0 {
                        return Some(deviation);
                    }
                }
            }
    }
    None
}

/// Extract trigger conditions from decision.
fn extract_trigger_conditions(decision: &Decision) -> serde_json::Value {
    let mut conditions = Vec::new();

    // Use a fixed confidence since Decision doesn't have one
    conditions.push("verified_action".to_string());

    if !decision.action.is_empty() {
        conditions.push(format!("action:{}", decision.action));
    }

    serde_json::json!(conditions)
}

/// Extract semantic description (abstract, not specific).
fn extract_semantic_description(decision: &Decision, symptom: &str) -> String {
    // Convert specific descriptions to abstract patterns
    let desc = &decision.description;

    // Pattern: "温度传感器1显示25度" -> "温度异常触发告警"
    if desc.contains("温度") || desc.contains("temp") {
        return format!("温度{} - {}", symptom, decision.action);
    }
    if desc.contains("湿度") || desc.contains("humidity") {
        return format!("湿度{} - {}", symptom, decision.action);
    }
    if desc.contains("压力") || desc.contains("pressure") {
        return format!("压力{} - {}", symptom, decision.action);
    }

    // Generic abstract description
    format!("{} - {}", symptom, decision.action)
}

/// Build medium-term summary for 24h context compression.
#[allow(dead_code)]
fn build_medium_term_summary(
    memory: &AgentMemory,
    _current_analysis: &str,
    current_conclusion: &str,
) -> String {
    let mut parts = Vec::new();

    // Key metrics tracked
    if !memory.baselines.is_empty() {
        parts.push(format!(
            "基线指标: {}",
            memory.baselines
                .iter()
                .map(|(k, v)| format!("{}={:.1}", k, v))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    // Pattern summary - optimized to avoid intermediate Vec allocation
    if !memory.learned_patterns.is_empty() {
        let pattern_types: std::collections::HashSet<_> = memory
            .learned_patterns
            .iter()
            .map(|p| p.pattern_type.as_str())
            .collect();
        parts.push(format!(
            "已识别模式: {}",
            pattern_types.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }

    // Current status
    if !current_conclusion.is_empty() {
        parts.push(format!("当前状态: {}", current_conclusion));
    }

    parts.join("; ")
}

/// Check if context needs compaction based on token estimation.
#[allow(dead_code)]
fn should_compact_context(history_context: &str, threshold_chars: usize) -> bool {
    // Rough estimation: 1 token ≈ 3 characters for Chinese/English mixed
    let estimated_tokens = history_context.chars().count() / 3;
    estimated_tokens > threshold_chars
}

/// Clean and truncate text to prevent storing repetitive/looping LLM output.
/// - Detects and removes repetitive patterns (same phrase appearing 3+ times)
/// - Truncates to max_chars
/// - Removes common LLM artifacts (internal monologue, formatting codes)
fn clean_and_truncate_text(text: &str, max_chars: usize) -> String {
    if text.is_empty() {
        return String::new();
    }

    // First, check for obvious repetition patterns
    // If a short phrase (10-50 chars) appears 3+ times, it's likely stuck in a loop
    let chars: Vec<char> = text.chars().collect();
    let char_count = chars.len();

    // Quick check for extreme repetition (same char repeated > 50 times)
    let mut streak = 1;
    for i in 1..char_count.min(1000) {
        if chars[i] == chars[i-1] {
            streak += 1;
            if streak > 50 {
                // High repetition detected, truncate early
                let truncated: String = chars.iter().take(i.saturating_sub(20)).collect();
                return format!("{}...[内容过长，已截断]", truncated);
            }
        } else {
            streak = 1;
        }
    }

    // Check for phrase-level repetition using sliding window
    let text_lower = text.to_lowercase();
    for window_size in [10, 15, 20, 30, 50].iter() {
        if char_count < *window_size * 3 {
            continue;
        }

        let mut phrase_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for i in 0..=(char_count.saturating_sub(*window_size)) {
            let phrase: String = chars.iter()
                .skip(i)
                .take(*window_size)
                .collect::<String>()
                .to_lowercase();

            if !phrase.chars().all(|c| c.is_whitespace()) {
                *phrase_counts.entry(phrase).or_insert(0) += 1;
            }
        }

        // If any phrase appears 3+ times, truncate at first occurrence
        for (phrase, count) in phrase_counts.iter() {
            if *count >= 3 && phrase.len() > 10 {
                // Find first occurrence and truncate
                if let Some(pos) = text_lower.find(phrase) {
                    let safe_pos = pos.saturating_sub(50);
                    let truncated: String = chars.iter().take(safe_pos).collect();
                    return if truncated.chars().count() > max_chars {
                        format!("{}...", truncated.chars().take(max_chars).collect::<String>())
                    } else {
                        truncated
                    };
                }
            }
        }
    }

    // No repetition detected, just truncate if too long
    if char_count > max_chars {
        let truncated: String = chars.iter().take(max_chars).collect();
        format!("{}...", truncated)
    } else {
        text.to_string()
    }
}

/// Compact history context while preserving key information.
///
/// COMPRESSION STRATEGY:
/// 1. Ultra-compact format - no section titles, minimal punctuation
/// 2. Merge similar information - don't repeat the same conclusion
/// 3. Use codes instead of descriptions - "T30" instead of "温度超过30度"
/// 4. Selective retention - only most relevant info
///
/// Target: < 200 characters for small models (qwen3:1.7b)
#[allow(dead_code)]
fn compact_history_context(
    _history_context: &str,
    memory: &AgentMemory,
) -> String {
    let mut parts = Vec::new();

    // === STRATEGY 1: Recent trend (最简化的趋势) ===
    // Instead of listing each execution, show the pattern
    if !memory.short_term.summaries.is_empty() {
        let last_3: Vec<_> = memory.short_term.summaries
            .iter()
            .rev()
            .take(3)
            .collect();

        // Count patterns
        let success_count = last_3.iter().filter(|s| s.success).count();
        let total = last_3.len();

        // Get most recent conclusion (most relevant)
        if let Some(latest) = last_3.first() {
            // Ultra-compact: "最近: 3次执行, 2次成功, 最新结论: ..."
            parts.push(format!(
                "最近{}次: {}成功, {}",
                total,
                success_count,
                truncate_to(&latest.conclusion, 30) // 只取前30字
            ));
        }
    }

    // === STRATEGY 2: Most important pattern (单一最重要模式) ===
    // Instead of all patterns, just show the highest confidence one
    let patterns = if !memory.long_term.patterns.is_empty() {
        &memory.long_term.patterns
    } else {
        &memory.learned_patterns
    };

    if !patterns.is_empty()
        && let Some(best) = patterns.iter().max_by(|a, b| {
            a.confidence.partial_cmp(&b.confidence).unwrap_or(std::cmp::Ordering::Equal)
        }) {
            // Ultra-compact: "模式: 温度>30度告警 (80%)"
            parts.push(format!(
                "模式: {} ({}%)",
                truncate_to(&best.description, 25),
                (best.confidence * 100.0) as u32
            ));
        }

    // === STRATEGY 3: Key baseline (关键基线) ===
    // Only show baselines that are relevant to common metrics
    if !memory.baselines.is_empty() {
        // Show at most 2 most relevant baselines
        let baseline_items: Vec<_> = memory.baselines
            .iter()
            .take(2)
            .collect();

        if !baseline_items.is_empty() {
            let baseline_str = baseline_items
                .iter()
                .map(|(k, v)| format!("{}={}", k, **v as i32))
                .collect::<Vec<_>>()
                .join(",");
            parts.push(format!("基线: {}", baseline_str));
        }
    }

    // Join with minimal separator
    if parts.is_empty() {
        String::new()
    } else {
        parts.join(" | ")
    }
}

/// Truncate text to max_chars, adding "..." if needed
fn truncate_to(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(max_chars.saturating_sub(3)).collect();
        truncated + "..."
    }
}

/// Event data for triggering agent execution.
#[derive(Clone, Debug)]
pub struct EventTriggerData {
    pub device_id: String,
    pub metric: String,
    pub value: MetricValue,
    pub timestamp: i64,
}

/// Configuration for agent executor.
#[derive(Clone)]
pub struct AgentExecutorConfig {
    /// Agent store
    pub store: Arc<AgentStore>,
    /// Time series storage for data collection
    pub time_series_storage: Option<Arc<neomind_storage::TimeSeriesStore>>,
    /// Device service for command execution
    pub device_service: Option<Arc<DeviceService>>,
    /// Event bus for event subscription
    pub event_bus: Option<Arc<EventBus>>,
    /// Message manager for sending notifications (replaces AlertManager)
    pub message_manager: Option<Arc<MessageManager>>,
    /// LLM runtime for intent analysis (default)
    pub llm_runtime: Option<Arc<dyn neomind_core::llm::backend::LlmRuntime + Send + Sync>>,
    /// LLM backend store for per-agent backend lookup
    pub llm_backend_store: Option<Arc<LlmBackendStore>>,
}

/// Context for agent execution.
#[derive(Clone)]
pub struct ExecutionContext {
    /// Agent being executed
    pub agent: AiAgent,
    /// Trigger type (schedule, event, manual)
    pub trigger_type: String,
    /// Current event data (if event-triggered)
    pub event_data: Option<serde_json::Value>,
    /// LLM backend for decision making
    pub llm_backend: Option<LlmBackend>,
    /// Execution ID for event tracking
    pub execution_id: String,
}

/// Result of agent execution.
pub struct AgentExecutionResult {
    /// Execution record
    pub record: AgentExecutionRecord,
    /// Updated memory
    pub memory: AgentMemory,
    /// Success status
    pub success: bool,
}

/// AI Agent executor - handles execution of user-defined agents.
pub struct AgentExecutor {
    /// Agent store
    store: Arc<AgentStore>,
    /// Time series storage for data collection
    time_series_storage: Option<Arc<neomind_storage::TimeSeriesStore>>,
    /// Device service for command execution
    device_service: Option<Arc<DeviceService>>,
    /// Event bus for publishing events
    event_bus: Option<Arc<EventBus>>,
    /// Message manager for sending notifications (replaces AlertManager)
    message_manager: Option<Arc<MessageManager>>,
    /// Configuration
    _config: AgentExecutorConfig,
    /// LLM runtime (default)
    llm_runtime: Option<Arc<dyn neomind_core::llm::backend::LlmRuntime + Send + Sync>>,
    /// LLM backend store for per-agent backend lookup
    llm_backend_store: Option<Arc<LlmBackendStore>>,
    /// Event-triggered agents cache
    event_agents: Arc<RwLock<HashMap<String, AiAgent>>>,
    /// Track recent executions to prevent duplicates (agent_id, device_id, metric -> timestamp)
    recent_executions: Arc<RwLock<HashMap<(String, String, String), i64>>>,
    /// LLM runtime cache: backend_id -> runtime
    /// Key format: "{backend_type}:{endpoint}:{model}" for cache invalidation
    llm_runtime_cache: Arc<RwLock<HashMap<String, Arc<dyn neomind_core::llm::backend::LlmRuntime + Send + Sync>>>>,
}

/// Calculate relevance score for a conversation turn based on current context.
///
/// Scoring factors (inspired by MemoryOS heat-based approach):
/// - Time decay (30%): exp(-0.03 * age_hours) - recent turns score higher
/// - Success reference (20%): successful turns are more valuable
/// - Device overlap (30%): turns involving same devices are more relevant
/// - Trigger similarity (20%): same trigger type suggests similar context
///
/// Returns a score between 0.0 (irrelevant) and 1.0 (highly relevant).
fn score_turn_relevance(
    turn: &ConversationTurn,
    current_data: &[DataCollected],
    current_trigger: &str,
) -> f64 {
    let mut score = 0.0;
    let now = chrono::Utc::now().timestamp();
    let age_hours = ((now - turn.timestamp).max(0) as f64) / 3600.0;

    // Factor 1: Time decay (30% weight) - exponential decay
    // Recent turns (< 1 hour) get close to full score
    // Old turns (> 24 hours) get minimal score
    let recency_score = (-0.03 * age_hours).exp().max(0.0).min(1.0);
    score += recency_score * 0.3;

    // Factor 2: Success reference (20% weight)
    // Successful executions are more valuable as positive examples
    if turn.success {
        score += 0.2;
    }

    // Factor 3: Device overlap (30% weight)
    // Turns that handled the same devices are highly relevant
    let current_devices: std::collections::HashSet<_> = current_data
        .iter()
        .map(|d| d.source.as_str())
        .collect();

    let turn_devices: std::collections::HashSet<_> = turn
        .input
        .data_collected
        .iter()
        .map(|d| d.source.as_str())
        .collect();

    if !current_devices.is_empty() {
        let overlap = current_devices
            .intersection(&turn_devices)
            .count();
        let union = current_devices
            .union(&turn_devices)
            .count();
        let jaccard = if union > 0 {
            overlap as f64 / union as f64
        } else {
            0.0
        };
        score += jaccard * 0.3;
    }

    // Factor 4: Trigger similarity (20% weight)
    // Same trigger type suggests similar execution context
    if !current_trigger.is_empty() && turn.trigger_type == current_trigger {
        score += 0.2;
    }

    // Clamp to [0, 1]
    score.min(1.0).max(0.0)
}

impl AgentExecutor {
    /// Create a new agent executor.
    pub async fn new(config: AgentExecutorConfig) -> AgentResult<Self> {
        let llm_runtime = config.llm_runtime.clone();
        let llm_backend_store = config.llm_backend_store.clone();
        let message_manager = config.message_manager.clone();
        Ok(Self {
            store: config.store.clone(),
            time_series_storage: config.time_series_storage.clone(),
            device_service: config.device_service.clone(),
            event_bus: config.event_bus.clone(),
            message_manager,
            _config: config,
            llm_runtime,
            llm_backend_store,
            event_agents: Arc::new(RwLock::new(HashMap::new())),
            recent_executions: Arc::new(RwLock::new(HashMap::new())),
            llm_runtime_cache: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Set the LLM runtime for intent parsing.
    pub async fn set_llm_runtime(&mut self, llm: Arc<dyn neomind_core::llm::backend::LlmRuntime + Send + Sync>) {
        self.llm_runtime = Some(llm);
    }

    /// Get the agent store.
    pub fn store(&self) -> Arc<AgentStore> {
        self.store.clone()
    }

    /// Send a progress event for an agent execution.
    async fn send_progress(
        &self,
        agent_id: &str,
        execution_id: &str,
        stage: &str,
        stage_label: &str,
        details: Option<&str>,
    ) {
        if let Some(ref bus) = self.event_bus {
            let _ = bus.publish(neomind_core::NeoMindEvent::AgentProgress {
                agent_id: agent_id.to_string(),
                execution_id: execution_id.to_string(),
                stage: stage.to_string(),
                stage_label: stage_label.to_string(),
                progress: None,
                details: details.map(|d| d.to_string()),
                timestamp: chrono::Utc::now().timestamp(),
            }).await;
        }
    }

    /// Send a thinking event for an agent execution.
    async fn send_thinking(
        &self,
        agent_id: &str,
        execution_id: &str,
        step_number: u32,
        description: &str,
    ) {
        if let Some(ref bus) = self.event_bus {
            let _ = bus.publish(neomind_core::NeoMindEvent::AgentThinking {
                agent_id: agent_id.to_string(),
                execution_id: execution_id.to_string(),
                step_number,
                step_type: "progress".to_string(),
                description: description.to_string(),
                details: None,
                timestamp: chrono::Utc::now().timestamp(),
            }).await;
        }
    }

    /// Build a cache key for LLM runtime based on backend configuration.
    fn build_runtime_cache_key(backend_type: &str, endpoint: &str, model: &str) -> String {
        format!("{}|{}|{}", backend_type, endpoint, model)
    }

    /// Get the LLM runtime for a specific agent.
    /// If the agent has a specific backend ID configured, use that.
    /// Otherwise, fall back to the default runtime.
    ///
    /// Runtimes are cached by backend configuration to avoid repeated initialization.
    pub async fn get_llm_runtime_for_agent(
        &self,
        agent: &AiAgent,
    ) -> Result<Option<Arc<dyn LlmRuntime + Send + Sync>>, NeoMindError> {
        // If agent has a specific backend ID, try to use it
        if let Some(ref backend_id) = agent.llm_backend_id
            && let Some(ref store) = self.llm_backend_store
                && let Ok(Some(backend)) = store.load_instance(backend_id) {
                    use neomind_storage::LlmBackendType;

                    // Build cache key
                    let endpoint = backend.endpoint.clone().unwrap_or_default();
                    let model = backend.model.clone();
                    let cache_key = Self::build_runtime_cache_key(
                        format!("{:?}", backend.backend_type).as_str(),
                        endpoint.as_str(),
                        model.as_str()
                    );

                    // Check cache first
                    {
                        let cache = self.llm_runtime_cache.read().await;
                        if let Some(runtime) = cache.get(&cache_key) {
                            tracing::debug!(
                                agent_id = %agent.id,
                                backend = %backend_id,
                                "LLM runtime cache hit"
                            );
                            return Ok(Some(runtime.clone()));
                        }
                    }

                    // Cache miss - create new runtime
                    tracing::debug!(
                        agent_id = %agent.id,
                        backend = %backend_id,
                        "LLM runtime cache miss, creating new runtime"
                    );

                    let runtime = match backend.backend_type {
                        LlmBackendType::Ollama => {
                            let endpoint = backend.endpoint.clone().unwrap_or_else(|| "http://localhost:11434".to_string());
                            let model = backend.model.clone();
                            let timeout = std::env::var("OLLAMA_TIMEOUT_SECS")
                                .ok()
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(120);
                            OllamaRuntime::new(
                                OllamaConfig::new(&model)
                                    .with_endpoint(&endpoint)
                                    .with_timeout_secs(timeout)
                            )
                                .map(|rt| Arc::new(rt) as Arc<dyn LlmRuntime + Send + Sync>)
                        }
                        LlmBackendType::OpenAi => {
                            let api_key = backend.api_key.clone().unwrap_or_default();
                            let endpoint = backend.endpoint.clone().unwrap_or_else(|| "https://api.openai.com/v1".to_string());
                            let model = backend.model.clone();
                            let timeout = std::env::var("OPENAI_TIMEOUT_SECS")
                                .ok()
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(60);
                            CloudRuntime::new(
                                CloudConfig::custom(&api_key, &endpoint)
                                    .with_model(&model)
                                    .with_timeout_secs(timeout)
                            )
                                .map(|rt| Arc::new(rt) as Arc<dyn LlmRuntime + Send + Sync>)
                        }
                        LlmBackendType::Anthropic => {
                            let api_key = backend.api_key.clone().unwrap_or_default();
                            let _endpoint = backend.endpoint.clone().unwrap_or_else(|| "https://api.anthropic.com/v1".to_string());
                            let model = backend.model.clone();
                            let timeout = std::env::var("ANTHROPIC_TIMEOUT_SECS")
                                .ok()
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(60);
                            CloudRuntime::new(
                                CloudConfig::anthropic(&api_key)
                                    .with_model(&model)
                                    .with_timeout_secs(timeout)
                            )
                                .map(|rt| Arc::new(rt) as Arc<dyn LlmRuntime + Send + Sync>)
                        }
                        LlmBackendType::Google => {
                            let api_key = backend.api_key.clone().unwrap_or_default();
                            let _endpoint = backend.endpoint.clone().unwrap_or_else(|| "https://generativelanguage.googleapis.com/v1beta".to_string());
                            let model = backend.model.clone();
                            let timeout = std::env::var("GOOGLE_TIMEOUT_SECS")
                                .ok()
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(60);
                            CloudRuntime::new(
                                CloudConfig::google(&api_key)
                                    .with_model(&model)
                                    .with_timeout_secs(timeout)
                            )
                                .map(|rt| Arc::new(rt) as Arc<dyn LlmRuntime + Send + Sync>)
                        }
                        LlmBackendType::XAi => {
                            let api_key = backend.api_key.clone().unwrap_or_default();
                            let _endpoint = backend.endpoint.clone().unwrap_or_else(|| "https://api.x.ai/v1".to_string());
                            let model = backend.model.clone();
                            let timeout = std::env::var("XAI_TIMEOUT_SECS")
                                .ok()
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(60);
                            CloudRuntime::new(
                                CloudConfig::grok(&api_key)
                                    .with_model(&model)
                                    .with_timeout_secs(timeout)
                            )
                                .map(|rt| Arc::new(rt) as Arc<dyn LlmRuntime + Send + Sync>)
                        }
                    };

                    match runtime {
                        Ok(rt) => {
                            // Store in cache
                            let mut cache = self.llm_runtime_cache.write().await;
                            cache.insert(cache_key, rt.clone());
                            tracing::info!(
                                agent_id = %agent.id,
                                backend = %backend_id,
                                "LLM runtime created and cached"
                            );
                            return Ok(Some(rt));
                        }
                        Err(e) => {
                            tracing::warn!(
                                agent_id = %agent.id,
                                backend_type = ?backend.backend_type,
                                error = %e,
                                "Failed to create LLM runtime for agent '{}'", agent.name
                            );
                        }
                    }
                }

        // Fall back to default runtime
        Ok(self.llm_runtime.clone())
    }

    /// Parse user intent from natural language using LLM or keyword-based fallback.
    pub async fn parse_intent(&self, user_prompt: &str) -> AgentResult<neomind_storage::ParsedIntent> {
        // Try LLM-based parsing if available
        if let Some(ref llm) = self.llm_runtime
            && let Ok(intent) = self.parse_intent_with_llm(llm, user_prompt).await {
                return Ok(intent);
            }

        // Fall back to keyword-based parsing
        self.parse_intent_keywords(user_prompt).await
    }

    /// Parse intent using LLM.
    async fn parse_intent_with_llm(
        &self,
        llm: &Arc<dyn neomind_core::llm::backend::LlmRuntime + Send + Sync>,
        user_prompt: &str,
    ) -> AgentResult<neomind_storage::ParsedIntent> {
        use neomind_core::llm::backend::{LlmInput, GenerationParams};

        // Get current time context for temporal understanding
        let time_context = get_time_context();

        let system_prompt = format!(
            r#"You are an intent parser for IoT automation. Analyze the user's request and extract:
1. Intent type: Monitoring, ReportGeneration, AnomalyDetection, Control, or Automation
2. Target metrics: temperature, humidity, power, etc.
3. Conditions: any thresholds or comparison operators
4. Actions: what actions to take when conditions are met

{}

Respond in JSON format:
{{
  "intent_type": "Monitoring|ReportGeneration|AnomalyDetection|Control|Automation",
  "target_metrics": ["metric1", "metric2"],
  "conditions": ["condition1", "condition2"],
  "actions": ["action1", "action2"],
  "confidence": 0.9
}}"#,
            time_context
        );

        let messages = vec![
            Message::new(MessageRole::System, Content::text(system_prompt)),
            Message::new(MessageRole::User, Content::text(user_prompt)),
        ];

        let input = LlmInput {
            messages,
            params: GenerationParams {
                temperature: Some(0.3),
                max_tokens: Some(500),
                ..Default::default()
            },
            model: None,
            stream: false,
            tools: Some(Vec::new()),
        };

        // Add timeout for LLM generation (5 minutes max)
        const LLM_TIMEOUT_SECS: u64 = 300;
        let llm_result = match tokio::time::timeout(
            std::time::Duration::from_secs(LLM_TIMEOUT_SECS),
            llm.generate(input)
        ).await {
            Ok(result) => result,
            Err(_) => {
                tracing::warn!(
                    "LLM intent parsing timed out after {}s",
                    LLM_TIMEOUT_SECS
                );
                return Err(NeoMindError::Llm(format!("LLM timeout after {}s", LLM_TIMEOUT_SECS)));
            }
        };

        match llm_result {
            Ok(output) => {
                // Try to parse JSON from LLM output
                let json_str = output.text.trim();
                // Extract JSON if it's wrapped in markdown code blocks
                let json_str = if json_str.contains("```json") {
                    json_str.split("```json").nth(1)
                        .and_then(|s| s.split("```").next())
                        .unwrap_or(json_str)
                        .trim()
                } else if json_str.contains("```") {
                    json_str.split("```").nth(1)
                        .unwrap_or(json_str)
                        .trim()
                } else {
                    json_str
                };

                serde_json::from_str(json_str)
                    .map_err(|_| NeoMindError::Llm("Failed to parse LLM intent response".to_string()))
            }
            Err(_) => Err(NeoMindError::Llm("LLM call failed".to_string())),
        }
    }

    /// Parse intent using keyword-based fallback.
    async fn parse_intent_keywords(&self, user_prompt: &str) -> AgentResult<neomind_storage::ParsedIntent> {
        let prompt_lower = user_prompt.to_lowercase();

        let (intent_type, confidence) = if prompt_lower.contains("报告") || prompt_lower.contains("汇总") || prompt_lower.contains("每天") {
            (neomind_storage::IntentType::ReportGeneration, 0.8)
        } else if prompt_lower.contains("异常") || prompt_lower.contains("检测") {
            (neomind_storage::IntentType::AnomalyDetection, 0.8)
        } else if prompt_lower.contains("控制") || prompt_lower.contains("开关") {
            (neomind_storage::IntentType::Control, 0.7)
        } else {
            (neomind_storage::IntentType::Monitoring, 0.7)
        };

        let target_metrics = extract_metrics(&prompt_lower);
        let conditions = extract_conditions(&prompt_lower);
        let actions = extract_actions(&prompt_lower);

        Ok(neomind_storage::ParsedIntent {
            intent_type,
            target_metrics,
            conditions,
            actions,
            confidence,
        })
    }

    /// Check if an event should trigger any agent and execute it.
    pub async fn check_and_trigger_event(
        &self,
        device_id: String,
        metric: &str,
        value: &MetricValue,
    ) -> AgentResult<()> {
        // Refresh event-triggered agents cache
        self.refresh_event_agents().await;

        let event_agents = self.event_agents.read().await;

        tracing::debug!(
            device_id = %device_id,
            metric = %metric,
            event_agent_count = event_agents.len(),
            "[EVENT] Checking device event against {} event-triggered agents",
            event_agents.len()
        );

        // Clone device_id for use in spawned tasks
        let device_id_for_spawn = device_id.clone();

        // Clean up old entries from recent_executions (older than 60 seconds)
        let now = chrono::Utc::now().timestamp();
        let mut recent = self.recent_executions.write().await;
        recent.retain(|_, &mut timestamp| now - timestamp < 60);
        drop(recent);

        for (_agent_id, agent) in event_agents.iter() {
            // Check if this agent has event-based schedule
            if matches!(agent.schedule.schedule_type, neomind_storage::ScheduleType::Event) {
                // Check if agent's event filter matches this event
                if self.matches_event_filter(agent, &device_id, metric, value).await {
                    // Check for duplicate execution within the last 5 seconds
                    let key = (agent.id.clone(), device_id.clone(), metric.to_string());
                    let recent = self.recent_executions.read().await;
                    let is_duplicate = recent.get(&key)
                        .map(|&timestamp| now - timestamp < 5)
                        .unwrap_or(false);
                    drop(recent);

                    if is_duplicate {
                        tracing::debug!(
                            agent_name = %agent.name,
                            device_id = %device_id,
                            metric = %metric,
                            "Skipping duplicate event-triggered execution (within 5 seconds)"
                        );
                        continue;
                    }

                    // Mark this execution as recent
                    {
                        let mut recent = self.recent_executions.write().await;
                        recent.insert(key, now);
                    }

                    tracing::info!(
                        agent_name = %agent.name,
                        device_id = %device_id,
                        metric = %metric,
                        "Event-triggered agent execution"
                    );

                    // Clone the agent and event data for execution
                    let agent_clone = agent.clone();
                    let metric_clone = metric.to_string();
                    let value_clone = value.clone();
                    let device_id_for_task = device_id_for_spawn.clone();
                    let timestamp = chrono::Utc::now().timestamp();

                    // Spawn full agent execution in background
                    let executor_store = self.store.clone();
                    let executor_time_series = self.time_series_storage.clone();
                    let executor_device = self.device_service.clone();
                    let executor_event_bus = self.event_bus.clone();
                    let executor_message_manager = self.message_manager.clone();
                    let executor_llm = self.llm_runtime.clone();
                    let executor_llm_store = self.llm_backend_store.clone();
                    let agent_id_for_log = agent.id.clone();

                    tokio::spawn(async move {
                        // Create event trigger data
                        let event_trigger_data = EventTriggerData {
                            device_id: device_id_for_task,
                            metric: metric_clone,
                            value: value_clone,
                            timestamp,
                        };

                        // Create a new executor for this event-triggered execution
                        let executor_config = AgentExecutorConfig {
                            store: executor_store.clone(),
                            time_series_storage: executor_time_series.clone(),
                            device_service: executor_device.clone(),
                            event_bus: executor_event_bus.clone(),
                            message_manager: executor_message_manager,
                            llm_runtime: executor_llm,
                            llm_backend_store: executor_llm_store,
                        };

                        match AgentExecutor::new(executor_config).await {
                            Ok(executor) => {
                                tracing::debug!(
                                    agent_id = %agent_id_for_log,
                                    trigger_device = %event_trigger_data.device_id,
                                    trigger_metric = %event_trigger_data.metric,
                                    "Executing event-triggered agent with event data"
                                );

                                // Execute the agent with event data (includes the triggering metric value directly)
                                match executor.execute_agent_with_event(agent_clone, event_trigger_data).await {
                                    Ok(record) => {
                                        tracing::info!(
                                            agent_id = %agent_id_for_log,
                                            execution_id = %record.id,
                                            status = ?record.status,
                                            "Event-triggered agent execution completed"
                                        );
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            agent_id = %agent_id_for_log,
                                            error = %e,
                                            "Event-triggered agent execution failed"
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!(
                                    agent_id = %agent_id_for_log,
                                    error = %e,
                                    "Failed to create executor for event-triggered agent"
                                );
                            }
                        }
                    });
                }
            }
        }

        Ok(())
    }

    /// Check if an event matches the agent's event filter.
    async fn matches_event_filter(
        &self,
        agent: &AiAgent,
        device_id: &str,
        metric: &str,
        _value: &MetricValue,
    ) -> bool {
        // Build the expected resource IDs for this event
        let device_metric_id = format!("{}:{}", device_id, metric);

        // Check each resource to see if it matches this event
        let has_matching_resource = agent.resources.iter().any(|r| {
            match r.resource_type {
                ResourceType::Device => {
                    // Device resource matches if device_id matches exactly
                    r.resource_id == device_id
                }
                ResourceType::Metric => {
                    // Metric resource matches if:
                    // 1. Exact "device_id:metric" match, OR
                    // 2. Metric-only resource (no colon) matches metric name exactly
                    if r.resource_id.contains(':') {
                        // Resource has device prefix - require exact match
                        r.resource_id == device_metric_id
                    } else {
                        // Resource is metric-only - match if metric name matches exactly
                        r.resource_id == metric
                    }
                }
                _ => false,
            }
        });

        // Agent matches if:
        // 1. It has a matching resource, OR
        // 2. Resources are empty (trigger on all events)
        let matches = has_matching_resource || agent.resources.is_empty();

        tracing::trace!(
            agent_name = %agent.name,
            device_id = %device_id,
            metric = %metric,
            has_matching_resource = has_matching_resource,
            resources_empty = agent.resources.is_empty(),
            matches = matches,
            "[EVENT] Agent {} event filter check: has_matching_resource={}, matches={}",
            agent.name,
            has_matching_resource,
            matches
        );

        matches
    }

    /// Refresh the cache of event-triggered agents.
    async fn refresh_event_agents(&self) {
        let filter = neomind_storage::AgentFilter {
            status: Some(neomind_storage::AgentStatus::Active),
            ..Default::default()
        };

        if let Ok(agents) = self.store.query_agents(filter).await {
            let total_active = agents.len();
            let event_agents: HashMap<String, AiAgent> = agents
                .into_iter()
                .filter(|a| matches!(a.schedule.schedule_type, neomind_storage::ScheduleType::Event))
                .map(|a| (a.id.clone(), a))
                .collect();

            let mut cache = self.event_agents.write().await;
            let previous_count = cache.len();
            *cache = event_agents;

            tracing::debug!(
                total_active_agents = total_active,
                event_triggered_agents = cache.len(),
                previous_count = previous_count,
                "[EVENT] Refreshed event-triggered agents cache"
            );

            // Log each event-triggered agent for debugging
            for (id, agent) in cache.iter() {
                tracing::debug!(
                    agent_id = %id,
                    agent_name = %agent.name,
                    resource_count = agent.resources.len(),
                    "[EVENT] Event-triggered agent: {} with {} resources",
                    agent.name,
                    agent.resources.len()
                );
            }
        }
    }

    /// Execute an agent and record the full decision process.
    pub async fn execute_agent(&self, agent: AiAgent) -> AgentResult<AgentExecutionRecord> {
        let agent_id = agent.id.clone();
        let agent_name = agent.name.clone();
        let execution_id = uuid::Uuid::new_v4().to_string();
        let start_time = std::time::Instant::now();
        let timestamp = chrono::Utc::now().timestamp();

        // Update agent status to executing
        self.store
            .update_agent_status(&agent_id, neomind_storage::AgentStatus::Executing, None)
            .await
            .map_err(|e| NeoMindError::Storage(format!("Failed to update status: {}", e)))?;

        // Create execution context
        let context = ExecutionContext {
            agent: agent.clone(),
            trigger_type: "manual".to_string(),
            event_data: None,
            llm_backend: None,
            execution_id: execution_id.clone(),
        };

        // Emit agent execution started event
        tracing::info!(
            agent_id = %agent_id,
            execution_id = %execution_id,
            has_event_bus = self.event_bus.is_some(),
            "Emitting AgentExecutionStarted event"
        );
        if let Some(ref bus) = self.event_bus {
            let _ = bus.publish(NeoMindEvent::AgentExecutionStarted {
                agent_id: agent_id.clone(),
                agent_name: agent_name.clone(),
                execution_id: execution_id.clone(),
                trigger_type: "manual".to_string(),
                timestamp,
            }).await;
            tracing::info!("AgentExecutionStarted event published");
        } else {
            tracing::warn!("No event_bus available, agent events will not be published");
        }

        // Execute with error handling for stability
        let execution_result = self.execute_with_retry(context).await;

        let duration_ms = start_time.elapsed().as_millis() as u64;

        // Build execution record
        let (decision_process_for_turn, success) = match &execution_result {
            Ok((dp, _)) => (Some(dp.clone()), true),
            Err(_) => (None, false),
        };

        let record = match execution_result {
            Ok((decision_process, result)) => {
                // Update stats
                let _ = self.store
                    .update_agent_stats(&agent_id, true, duration_ms)
                    .await;

                AgentExecutionRecord {
                    id: execution_id.clone(),
                    agent_id: agent_id.clone(),
                    timestamp,
                    trigger_type: "manual".to_string(),
                    status: ExecutionStatus::Completed,
                    decision_process,
                    result: Some(result),
                    duration_ms,
                    error: None,
                }
            }
            Err(e) => {
                // Update stats with failure
                let _ = self.store
                    .update_agent_stats(&agent_id, false, duration_ms)
                    .await;

                AgentExecutionRecord {
                    id: execution_id.clone(),
                    agent_id: agent_id.clone(),
                    timestamp,
                    trigger_type: "manual".to_string(),
                    status: ExecutionStatus::Failed,
                    decision_process: DecisionProcess {
                        situation_analysis: format!("Execution failed: {}", e),
                        data_collected: vec![],
                        reasoning_steps: vec![],
                        decisions: vec![],
                        conclusion: format!("Failed: {}", e),
                        confidence: 0.0,
                    },
                    result: None,
                    duration_ms,
                    error: Some(e.to_string()),
                }
            }
        };

        // Save execution record and conversation turn in a single transaction
        let turn = decision_process_for_turn.as_ref().map(|dp| {
            self.create_conversation_turn(
                execution_id.clone(),
                "manual".to_string(),
                dp.data_collected.clone(),
                None, // event_data
                dp,
                duration_ms,
                success,
            )
        });

        self.store
            .save_execution_with_conversation(&record, Some(&agent_id), turn.as_ref())
            .await
            .map_err(|e| NeoMindError::Storage(format!("Failed to save execution: {}", e)))?;

        tracing::debug!(
            agent_id = %agent_id,
            execution_id = %execution_id,
            "Execution and conversation turn saved successfully"
        );

        // Reset agent status based on result
        let new_status = if record.status == ExecutionStatus::Completed {
            neomind_storage::AgentStatus::Active
        } else {
            neomind_storage::AgentStatus::Error
        };

        let _ = self.store
            .update_agent_status(&agent_id, new_status, record.error.clone())
            .await;

        // Emit agent execution completed event
        let completion_timestamp = chrono::Utc::now().timestamp();
        if let Some(ref bus) = self.event_bus {
            let _ = bus.publish(NeoMindEvent::AgentExecutionCompleted {
                agent_id: agent_id.clone(),
                execution_id: execution_id.clone(),
                success: record.status == ExecutionStatus::Completed,
                duration_ms: record.duration_ms,
                error: record.error.clone(),
                timestamp: completion_timestamp,
            }).await;
        }

        tracing::info!(
            agent_id = %agent_id,
            agent_name = %agent_name,
            execution_id = %execution_id,
            status = ?record.status,
            duration_ms = record.duration_ms,
            "Agent execution completed"
        );

        Ok(record)
    }

    /// Execute an agent with event trigger data.
    /// This method passes the triggering event data directly to avoid storage delays.
    pub async fn execute_agent_with_event(
        &self,
        agent: AiAgent,
        event_data: EventTriggerData,
    ) -> AgentResult<AgentExecutionRecord> {
        let agent_id = agent.id.clone();
        let agent_name = agent.name.clone();
        let execution_id = uuid::Uuid::new_v4().to_string();
        let start_time = std::time::Instant::now();
        let timestamp = chrono::Utc::now().timestamp();

        // Update agent status to executing
        self.store
            .update_agent_status(&agent_id, neomind_storage::AgentStatus::Executing, None)
            .await
            .map_err(|e| NeoMindError::Storage(format!("Failed to update status: {}", e)))?;

        // Clone event data for later use (before moving)
        let event_device_id = event_data.device_id.clone();
        let event_metric_name = event_data.metric.clone();
        let event_value_json = serde_json::to_value(&event_data.value).unwrap_or_default();
        let event_timestamp = event_data.timestamp;

        // Create execution context with event data
        let context = ExecutionContext {
            agent: agent.clone(),
            trigger_type: format!("event:{}", event_metric_name),
            event_data: Some(serde_json::json!({
                "device_id": event_device_id,
                "metric": event_metric_name,
                "value": event_value_json,
                "timestamp": event_timestamp,
            })),
            llm_backend: None,
            execution_id: execution_id.clone(),
        };

        // Emit agent execution started event
        tracing::info!(
            agent_id = %agent_id,
            execution_id = %execution_id,
            trigger_device = %event_device_id,
            trigger_metric = %event_metric_name,
            has_event_bus = self.event_bus.is_some(),
            "Emitting AgentExecutionStarted event (event-triggered)"
        );
        if let Some(ref bus) = self.event_bus {
            let _ = bus.publish(NeoMindEvent::AgentExecutionStarted {
                agent_id: agent_id.clone(),
                agent_name: agent_name.clone(),
                execution_id: execution_id.clone(),
                trigger_type: format!("event:{}", event_metric_name),
                timestamp,
            }).await;
        }

        // Execute with error handling for stability
        let execution_result = self.execute_with_retry_and_event(context, event_data).await;

        let duration_ms = start_time.elapsed().as_millis() as u64;

        // Build execution record
        let (decision_process_for_turn, success) = match &execution_result {
            Ok((dp, _)) => (Some(dp.clone()), true),
            Err(_) => (None, false),
        };

        let record = match execution_result {
            Ok((decision_process, result)) => {
                // Update stats
                let _ = self.store
                    .update_agent_stats(&agent_id, true, duration_ms)
                    .await;

                AgentExecutionRecord {
                    id: execution_id.clone(),
                    agent_id: agent_id.clone(),
                    timestamp,
                    trigger_type: format!("event:{}", event_metric_name),
                    status: ExecutionStatus::Completed,
                    decision_process,
                    result: Some(result),
                    duration_ms,
                    error: None,
                }
            }
            Err(e) => {
                // Update stats with failure
                let _ = self.store
                    .update_agent_stats(&agent_id, false, duration_ms)
                    .await;

                AgentExecutionRecord {
                    id: execution_id.clone(),
                    agent_id: agent_id.clone(),
                    timestamp,
                    trigger_type: format!("event:{}", event_metric_name),
                    status: ExecutionStatus::Failed,
                    decision_process: DecisionProcess {
                        situation_analysis: format!("Execution failed: {}", e),
                        data_collected: vec![],
                        reasoning_steps: vec![],
                        decisions: vec![],
                        conclusion: format!("Failed: {}", e),
                        confidence: 0.0,
                    },
                    result: None,
                    duration_ms,
                    error: Some(e.to_string()),
                }
            }
        };

        // Save execution record and conversation turn in a single transaction
        let turn = decision_process_for_turn.as_ref().map(|dp| {
            self.create_conversation_turn(
                execution_id.clone(),
                format!("event:{}", event_metric_name),
                dp.data_collected.clone(),
                Some(serde_json::json!({
                    "device_id": event_device_id,
                    "metric": event_metric_name,
                    "value": event_value_json,
                })),
                dp,
                duration_ms,
                success,
            )
        });

        self.store
            .save_execution_with_conversation(&record, Some(&agent_id), turn.as_ref())
            .await
            .map_err(|e| NeoMindError::Storage(format!("Failed to save execution: {}", e)))?;

        // Reset agent status based on result
        let new_status = if record.status == ExecutionStatus::Completed {
            neomind_storage::AgentStatus::Active
        } else {
            neomind_storage::AgentStatus::Error
        };

        let _ = self.store
            .update_agent_status(&agent_id, new_status, record.error.clone())
            .await;

        // Emit agent execution completed event
        let completion_timestamp = chrono::Utc::now().timestamp();
        if let Some(ref bus) = self.event_bus {
            let _ = bus.publish(NeoMindEvent::AgentExecutionCompleted {
                agent_id: agent_id.clone(),
                execution_id: execution_id.clone(),
                success: record.status == ExecutionStatus::Completed,
                duration_ms: record.duration_ms,
                error: record.error.clone(),
                timestamp: completion_timestamp,
            }).await;
        }

        tracing::info!(
            agent_id = %agent_id,
            agent_name = %agent_name,
            execution_id = %execution_id,
            trigger_device = %event_device_id,
            trigger_metric = %event_metric_name,
            status = ?record.status,
            duration_ms = record.duration_ms,
            "Event-triggered agent execution completed"
        );

        Ok(record)
    }

    /// Execute multiple agents in parallel for improved performance.
    ///
    /// This is especially useful for multi-agent scenarios where agents
    /// have independent tasks and can run concurrently.
    ///
    /// # Example
    /// ```text
    /// let agents = vec![monitor_agent, executor_agent, analyst_agent];
    /// let results = executor.execute_agents_parallel(agents).await?;
    /// // Results are returned in the same order as input agents
    /// ```
    pub async fn execute_agents_parallel(
        &self,
        agents: Vec<AiAgent>,
    ) -> AgentResult<Vec<AgentExecutionRecord>> {
        use futures::future::join_all;

        // Sort agents by priority (higher priority first)
        let mut sorted_agents = agents;
        sorted_agents.sort_by(|a, b| b.priority.cmp(&a.priority));

        let executor_ref = self;
        let futures: Vec<_> = sorted_agents
            .into_iter()
            .map(|agent| executor_ref.execute_agent(agent))
            .collect();

        let results = join_all(futures).await;

        // Collect results, converting any errors into a combined error
        let mut records = Vec::new();
        let mut errors = Vec::new();

        for result in results {
            match result {
                Ok(record) => records.push(record),
                Err(e) => errors.push(e),
            }
        }

        if !errors.is_empty() {
            tracing::warn!(
                count = errors.len(),
                "Some agents failed during parallel execution"
            );
        }

        if records.is_empty() && !errors.is_empty() {
            return Err(NeoMindError::Storage(format!(
                "All {} agents failed. First error: {}",
                errors.len(),
                errors[0]
            )));
        }

        Ok(records)
    }

    /// Execute with retry for stability.
    async fn execute_with_retry(
        &self,
        context: ExecutionContext,
    ) -> AgentResult<(DecisionProcess, StorageExecutionResult)> {
        let max_retries = 3u32;
        let mut last_error = None;

        for attempt in 0..=max_retries {
            match self.execute_internal(context.clone()).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    tracing::warn!(
                        agent_id = %context.agent.id,
                        attempt = attempt + 1,
                        max_retries = max_retries + 1,
                        error = %e,
                        "Agent execution failed, retrying"
                    );
                    last_error = Some(e);

                    if attempt < max_retries {
                        let delay_ms = 100 * (2_u64.pow(attempt));
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            NeoMindError::Llm("Max retries exceeded".to_string())
        }))
    }

    /// Execute with retry for stability (with event data).
    async fn execute_with_retry_and_event(
        &self,
        context: ExecutionContext,
        event_data: EventTriggerData,
    ) -> AgentResult<(DecisionProcess, StorageExecutionResult)> {
        let max_retries = 3u32;
        let mut last_error = None;

        for attempt in 0..=max_retries {
            match self.execute_internal_with_event(context.clone(), event_data.clone()).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    tracing::warn!(
                        agent_id = %context.agent.id,
                        attempt = attempt + 1,
                        max_retries = max_retries + 1,
                        error = %e,
                        "Agent execution failed, retrying"
                    );
                    last_error = Some(e);

                    if attempt < max_retries {
                        let delay_ms = 100 * (2_u64.pow(attempt));
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            NeoMindError::Llm("Max retries exceeded".to_string())
        }))
    }

    /// Internal execution logic.
    async fn execute_internal(
        &self,
        context: ExecutionContext,
    ) -> AgentResult<(DecisionProcess, StorageExecutionResult)> {
        let mut agent = context.agent;
        let agent_id = agent.id.clone();
        let execution_id = context.execution_id.clone();

        // Progress: Collecting data
        self.send_progress(&agent_id, &execution_id, "collecting", "Collecting data", Some("Gathering sensor data...")).await;

        // Step 1: Collect data
        let data_collected = self.collect_data(&agent).await?;

        // Send thinking events for each data source collected
        let mut step_num = 1;
        for data in &data_collected {
            self.send_thinking(&agent_id, &execution_id, step_num,
                &format!("Collected data source: {}", data.source)
            ).await;
            step_num += 1;
            // Small delay for visual effect
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        // Progress: Analyzing
        self.send_progress(&agent_id, &execution_id, "analyzing", "Analyzing",
            Some(&format!("Analyzing {} data points...", data_collected.len()))
        ).await;

        // Step 1.5: Parse intent if not already done
        let parsed_intent = if agent.parsed_intent.is_none() {
            match self.parse_intent(&agent.user_prompt).await {
                Ok(intent) => {
                    // Update agent with parsed intent
                    let _ = self.store.update_agent_parsed_intent(&agent.id, Some(intent.clone())).await;
                    Some(intent)
                }
                Err(e) => {
                    tracing::warn!(agent_id = %agent.id, error = %e, "Failed to parse intent, using default");
                    None
                }
            }
        } else {
            agent.parsed_intent.clone()
        };

        // Update agent reference with parsed intent
        if let Some(ref intent) = parsed_intent {
            agent.parsed_intent = Some(intent.clone());
        }

        // Step 2: Analyze situation with LLM
        let (situation_analysis, reasoning_steps, decisions, conclusion) =
            self.analyze_situation_with_intent(&agent, &data_collected, parsed_intent.as_ref(), &context.execution_id).await?;

        // Send thinking event for analysis completion
        self.send_thinking(&agent_id, &execution_id, step_num,
            &format!("Analysis completed: Generated {} decision(s)", decisions.len())
        ).await;
        step_num += 1;

        // Progress: Executing decisions
        self.send_progress(&agent_id, &execution_id, "executing", "Executing decisions",
            Some(&format!("Executing {} decision(s)...", decisions.len()))
        ).await;

        // Send initial executing status
        self.send_thinking(&agent_id, &execution_id, step_num,
            &format!("Starting execution of {} decision(s)", decisions.len())
        ).await;
        step_num += 1;

        // Step 3: Execute decisions
        let (actions_executed, notifications_sent) =
            self.execute_decisions(&agent, &decisions).await?;

        // Send thinking events for each action executed
        for action in &actions_executed {
            self.send_thinking(&agent_id, &execution_id, step_num,
                &format!("Executing: {} -> {}", action.action_type, action.target)
            ).await;
            step_num += 1;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        // Send thinking events for notifications
        for notification in &notifications_sent {
            self.send_thinking(&agent_id, &execution_id, step_num,
                &format!("Sending notification: {}", notification.message)
            ).await;
            step_num += 1;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        // Send completion event for executing stage
        if actions_executed.is_empty() && notifications_sent.is_empty() {
            self.send_thinking(&agent_id, &execution_id, step_num,
                "Execution completed: No additional actions required"
            ).await;
        } else {
            self.send_thinking(&agent_id, &execution_id, step_num,
                &format!("Execution completed: {} action(s), {} notification(s)", actions_executed.len(), notifications_sent.len())
            ).await;
        }

        // Step 4: Generate report if needed
        let report = self.maybe_generate_report(&agent, &data_collected).await?;

        // Step 5: Update memory with learnings
        // Determine success based on whether we had any major errors
        let memory_success = true; // We got here successfully, update_memory will store the result
        let updated_memory = self.update_memory(
            &agent,
            &data_collected,
            &decisions,
            &situation_analysis,
            &conclusion,
            &execution_id,
            memory_success,
        ).await?;

        // Save updated memory
        self.store
            .update_agent_memory(&agent.id, updated_memory.clone())
            .await
            .map_err(|e| NeoMindError::Storage(format!("Failed to update memory: {}", e)))?;

        // Calculate confidence from reasoning
        let confidence = if reasoning_steps.is_empty() {
            0.5
        } else {
            reasoning_steps.iter().map(|s| s.confidence).sum::<f32>() / reasoning_steps.len() as f32
        };

        // Truncate text fields before storing in DecisionProcess
        // This prevents unbounded growth in storage (execution records accumulate)
        let cleaned_situation = clean_and_truncate_text(&situation_analysis, 500);
        let cleaned_conclusion = clean_and_truncate_text(&conclusion, 200);

        // Clean reasoning step descriptions
        let cleaned_steps: Vec<neomind_storage::ReasoningStep> = reasoning_steps
            .into_iter()
            .map(|mut step| {
                step.description = clean_and_truncate_text(&step.description, 150);
                step
            })
            .collect();

        // Clean decision fields
        let cleaned_decisions: Vec<neomind_storage::Decision> = decisions
            .into_iter()
            .map(|mut dec| {
                dec.description = clean_and_truncate_text(&dec.description, 150);
                dec.rationale = clean_and_truncate_text(&dec.rationale, 150);
                dec.expected_outcome = clean_and_truncate_text(&dec.expected_outcome, 150);
                dec
            })
            .collect();

        let decision_process = DecisionProcess {
            situation_analysis: cleaned_situation,
            data_collected,
            reasoning_steps: cleaned_steps,
            decisions: cleaned_decisions,
            conclusion: cleaned_conclusion,
            confidence,
        };

        let success_rate = if actions_executed.is_empty() {
            1.0
        } else {
            let success_count = actions_executed.iter().filter(|a| a.success).count() as f32;
            success_count / actions_executed.len() as f32
        };

        let execution_result = StorageExecutionResult {
            actions_executed,
            report,
            notifications_sent,
            summary: conclusion,
            success_rate,
        };

        Ok((decision_process, execution_result))
    }

    /// Internal execution logic with event data.
    async fn execute_internal_with_event(
        &self,
        context: ExecutionContext,
        event_data: EventTriggerData,
    ) -> AgentResult<(DecisionProcess, StorageExecutionResult)> {
        let mut agent = context.agent;
        let agent_id = agent.id.clone();
        let execution_id = context.execution_id.clone();
        let mut step_num = 1u32;

        // Progress: Collecting data
        self.send_progress(&agent_id, &execution_id, "collecting", "Collecting data", Some("Gathering sensor data...")).await;

        // Step 1: Collect data including event data
        let data_collected = self.collect_data_with_event(&agent, &event_data).await?;

        // Send thinking events for each data source collected
        for data in &data_collected {
            self.send_thinking(&agent_id, &execution_id, step_num,
                &format!("📡 收集 {}: {} 个数据点", data.source, data.data_type)
            ).await;
            step_num += 1;
            // Small delay for visual effect
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        // Progress: Analyzing
        self.send_progress(&agent_id, &execution_id, "analyzing", "Analyzing",
            Some(&format!("Analyzing {} data points...", data_collected.len()))
        ).await;

        // Step 1.5: Parse intent if not already done
        let parsed_intent = if agent.parsed_intent.is_none() {
            match self.parse_intent(&agent.user_prompt).await {
                Ok(intent) => {
                    // Update agent with parsed intent
                    let _ = self.store.update_agent_parsed_intent(&agent.id, Some(intent.clone())).await;
                    Some(intent)
                }
                Err(e) => {
                    tracing::warn!(agent_id = %agent.id, error = %e, "Failed to parse intent, using default");
                    None
                }
            }
        } else {
            agent.parsed_intent.clone()
        };

        // Update agent reference with parsed intent
        if let Some(ref intent) = parsed_intent {
            agent.parsed_intent = Some(intent.clone());
        }

        // Step 2: Analyze situation with LLM
        let (situation_analysis, reasoning_steps, decisions, conclusion) =
            self.analyze_situation_with_intent(&agent, &data_collected, parsed_intent.as_ref(), &context.execution_id).await?;

        // Send thinking event for analysis completion
        self.send_thinking(&agent_id, &execution_id, step_num,
            &format!("Analysis completed: Generated {} decision(s)", decisions.len())
        ).await;
        step_num += 1;

        // Progress: Executing decisions
        self.send_progress(&agent_id, &execution_id, "executing", "Executing decisions",
            Some(&format!("Executing {} decision(s)...", decisions.len()))
        ).await;

        // Send initial executing status
        self.send_thinking(&agent_id, &execution_id, step_num,
            &format!("Starting execution of {} decision(s)", decisions.len())
        ).await;
        step_num += 1;

        // Step 3: Execute decisions
        let (actions_executed, notifications_sent) =
            self.execute_decisions(&agent, &decisions).await?;

        // Send thinking events for each action executed
        for action in &actions_executed {
            self.send_thinking(&agent_id, &execution_id, step_num,
                &format!("Executing: {} -> {}", action.action_type, action.target)
            ).await;
            step_num += 1;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        // Send thinking events for notifications
        for notification in &notifications_sent {
            self.send_thinking(&agent_id, &execution_id, step_num,
                &format!("Sending notification: {}", notification.message)
            ).await;
            step_num += 1;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        // Send completion event for executing stage
        if actions_executed.is_empty() && notifications_sent.is_empty() {
            self.send_thinking(&agent_id, &execution_id, step_num,
                "Execution completed: No additional actions required"
            ).await;
        } else {
            self.send_thinking(&agent_id, &execution_id, step_num,
                &format!("Execution completed: {} action(s), {} notification(s)", actions_executed.len(), notifications_sent.len())
            ).await;
        }

        // Step 4: Generate report if needed
        let report = self.maybe_generate_report(&agent, &data_collected).await?;

        // Step 5: Update memory with learnings
        // Determine success based on whether we had any major errors
        let memory_success = true; // We got here successfully, update_memory will store the result
        let updated_memory = self.update_memory(
            &agent,
            &data_collected,
            &decisions,
            &situation_analysis,
            &conclusion,
            &execution_id,
            memory_success,
        ).await?;

        // Save updated memory
        self.store
            .update_agent_memory(&agent.id, updated_memory.clone())
            .await
            .map_err(|e| NeoMindError::Storage(format!("Failed to update memory: {}", e)))?;

        // Calculate confidence from reasoning
        let confidence = if reasoning_steps.is_empty() {
            0.5
        } else {
            reasoning_steps.iter().map(|s| s.confidence).sum::<f32>() / reasoning_steps.len() as f32
        };

        // Truncate text fields before storing in DecisionProcess
        // This prevents unbounded growth in storage (execution records accumulate)
        let cleaned_situation = clean_and_truncate_text(&situation_analysis, 500);
        let cleaned_conclusion = clean_and_truncate_text(&conclusion, 200);

        // Clean reasoning step descriptions
        let cleaned_steps: Vec<neomind_storage::ReasoningStep> = reasoning_steps
            .into_iter()
            .map(|mut step| {
                step.description = clean_and_truncate_text(&step.description, 150);
                step
            })
            .collect();

        // Clean decision fields
        let cleaned_decisions: Vec<neomind_storage::Decision> = decisions
            .into_iter()
            .map(|mut dec| {
                dec.description = clean_and_truncate_text(&dec.description, 150);
                dec.rationale = clean_and_truncate_text(&dec.rationale, 150);
                dec.expected_outcome = clean_and_truncate_text(&dec.expected_outcome, 150);
                dec
            })
            .collect();

        let decision_process = DecisionProcess {
            situation_analysis: cleaned_situation,
            data_collected,
            reasoning_steps: cleaned_steps,
            decisions: cleaned_decisions,
            conclusion: cleaned_conclusion,
            confidence,
        };

        let success_rate = if actions_executed.is_empty() {
            1.0
        } else {
            let success_count = actions_executed.iter().filter(|a| a.success).count() as f32;
            success_count / actions_executed.len() as f32
        };

        let execution_result = StorageExecutionResult {
            actions_executed,
            report,
            notifications_sent,
            summary: conclusion,
            success_rate,
        };

        Ok((decision_process, execution_result))
    }

    /// Collect real data from time series storage.
    /// Uses parallel queries for improved performance when collecting multiple metrics.
    async fn collect_data(&self, agent: &AiAgent) -> AgentResult<Vec<DataCollected>> {
        let timestamp = chrono::Utc::now().timestamp();

        // Split resources by type for parallel processing
        let metric_resources: Vec<_> = agent.resources.iter()
            .filter(|r| r.resource_type == ResourceType::Metric)
            .cloned()
            .collect();

        let device_resources: Vec<_> = agent.resources.iter()
            .filter(|r| r.resource_type == ResourceType::Device)
            .map(|r| r.resource_id.clone())
            .collect();

        // Collect metric data in parallel
        let metric_data = self.collect_metric_data_parallel(agent, metric_resources, timestamp).await?;

        // Collect device data in parallel
        let device_data = self.collect_device_data_parallel(agent, device_resources, timestamp).await?;

        // Combine all data
        let mut data = metric_data;
        data.extend(device_data);

        // Add condensed memory context
        let memory_data = self.collect_memory_summary(agent, timestamp)?;
        if let Some(mem_data) = memory_data {
            data.push(mem_data);
        }

        // If no data collected, add a placeholder
        if data.is_empty() {
            data.push(DataCollected {
                source: "system".to_string(),
                data_type: "info".to_string(),
                values: serde_json::json!({"message": "No data sources configured"}),
                timestamp,
            });
        }

        Ok(data)
    }

    /// Collect data from multiple metric resources in parallel.
    async fn collect_metric_data_parallel(
        &self,
        _agent: &AiAgent,  // Reserved for future use
        resources: Vec<AgentResource>,
        timestamp: i64,
    ) -> AgentResult<Vec<DataCollected>> {
        let storage = self.time_series_storage.clone().ok_or(NeoMindError::validation(
            "Time series storage not available".to_string()
        ))?;

        // Create parallel futures for each metric resource
        let collect_futures: Vec<_> = resources.into_iter()
            .filter_map(|resource| {
                // Parse device_id and metric from resource_id (format: "device_id:metric_name")
                let parts: Vec<&str> = resource.resource_id.split(':').collect();
                if parts.len() != 2 {
                    return None;
                }
                let (device_id, metric_name) = (parts[0], parts[1]);

                // Extract config
                let time_range_minutes = resource.config
                    .get("data_collection")
                    .and_then(|dc| dc.get("time_range_minutes"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(60);

                let include_history = resource.config
                    .get("data_collection")
                    .and_then(|dc| dc.get("include_history"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let max_points = resource.config
                    .get("data_collection")
                    .and_then(|dc| dc.get("max_points"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1000) as usize;

                let include_trend = resource.config
                    .get("data_collection")
                    .and_then(|dc| dc.get("include_trend"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let include_baseline = resource.config
                    .get("data_collection")
                    .and_then(|dc| dc.get("include_baseline"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                // Clone necessary data for the async block
                let resource_id = resource.resource_id.clone();
                let storage_clone = storage.clone();
                let metric_name = metric_name.to_string();
                let device_id = device_id.to_string();

                Some(async move {
                    Self::collect_single_metric(
                        storage_clone,
                        &device_id,
                        &metric_name,
                        resource_id,
                        time_range_minutes,
                        include_history,
                        max_points,
                        include_trend,
                        include_baseline,
                        timestamp,
                    ).await
                })
            })
            .collect();

        // Execute all queries in parallel with timeout
        // Each query gets a maximum of 10 seconds to complete
        const QUERY_TIMEOUT_SECS: u64 = 10;

        let timeout_futures: Vec<_> = collect_futures.into_iter()
            .map(|fut| async move {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(QUERY_TIMEOUT_SECS),
                    fut
                ).await {
                    Ok(result) => result,
                    Err(_) => {
                        tracing::warn!("Data collection query timed out after {}s", QUERY_TIMEOUT_SECS);
                        Err(NeoMindError::Llm(format!("Query timeout after {}s", QUERY_TIMEOUT_SECS)))
                    }
                }
            })
            .collect();

        let results = join_all(timeout_futures).await;

        // Filter out errors and collect successful results
        let collected: Vec<_> = results.into_iter()
            .filter_map(|r| r.ok())
            .flatten()
            .collect();
        Ok(collected)
    }

    /// Collect data from a single metric resource.
    async fn collect_single_metric(
        storage: Arc<neomind_storage::TimeSeriesStore>,
        device_id: &str,
        metric_name: &str,
        resource_id: String,
        time_range_minutes: u64,
        include_history: bool,
        max_points: usize,
        _include_trend: bool,  // Reserved for future use
        _include_baseline: bool,  // Reserved for future use
        timestamp: i64,
    ) -> AgentResult<Option<DataCollected>> {
        let end_time = chrono::Utc::now().timestamp();
        let start_time = end_time - ((time_range_minutes * 60) as i64);

        let result = storage.query_range(device_id, metric_name, start_time, end_time).await
            .map_err(|e| NeoMindError::Storage(format!("Query failed: {}", e)))?;

        if result.points.is_empty() {
            return Ok(None);
        }

        let latest = &result.points[result.points.len() - 1];

        // Check if this is an image metric
        let is_image = is_image_metric(metric_name, &latest.value);
        let (image_url, image_base64, image_mime) = if is_image {
            extract_image_data(&latest.value)
        } else {
            (None, None, None)
        };

        // Build values JSON - construct once with all conditional fields
        let mut values_json = serde_json::json!({
            "value": latest.value,
            "timestamp": latest.timestamp,
            "points_count": result.points.len(),
            "time_range_minutes": time_range_minutes,
            "_is_image": is_image,
        });

        // Add image metadata if applicable
        if let Some(url) = &image_url {
            values_json["image_url"] = serde_json::json!(url);
        }
        if let Some(base64) = &image_base64 {
            values_json["image_base64"] = serde_json::json!(base64);
        }
        if let Some(mime) = &image_mime {
            values_json["image_mime_type"] = serde_json::json!(mime);
        }

        // Include history if configured and not an image
        if include_history && !is_image && result.points.len() > 1 {
            let history_limit = max_points.min(result.points.len());
            let start_idx = if result.points.len() > history_limit {
                result.points.len() - history_limit
            } else {
                0
            };

            let history_values: Vec<_> = result.points[start_idx..]
                .iter()
                .map(|p| serde_json::json!({
                    "value": p.value,
                    "timestamp": p.timestamp
                }))
                .collect();

            // Calculate statistics for numeric values
            let stats = calculate_stats(&result.points[start_idx..]).map(|nums| serde_json::json!({
                    "min": nums.min,
                    "max": nums.max,
                    "avg": nums.avg,
                    "count": nums.count
                }));

            values_json["history"] = serde_json::json!(history_values);
            values_json["history_count"] = serde_json::json!(history_values.len());
            if let Some(s) = stats {
                values_json["stats"] = s;
            }
        }

        Ok(Some(DataCollected {
            source: resource_id,
            data_type: metric_name.to_string(),
            values: values_json,
            timestamp,
        }))
    }

    /// Collect data from multiple device resources in parallel.
    async fn collect_device_data_parallel(
        &self,
        _agent: &AiAgent,  // Reserved for future use
        device_ids: Vec<String>,
        timestamp: i64,
    ) -> AgentResult<Vec<DataCollected>> {
        let device_service = self.device_service.as_ref()
            .ok_or(NeoMindError::validation("Device service not available".to_string()))?;

        let storage = self.time_series_storage.clone()
            .ok_or(NeoMindError::validation("Time series storage not available".to_string()))?;

        // Collect device info and metrics in parallel with timeout
        const QUERY_TIMEOUT_SECS: u64 = 10;

        let timeout_futures: Vec<_> = device_ids.into_iter()
            .map(|device_id| {
                let device_service = device_service.clone();
                let storage = storage.clone();
                async move {
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(QUERY_TIMEOUT_SECS),
                        Self::collect_single_device_data(device_service, storage, &device_id, timestamp)
                    ).await {
                        Ok(result) => result,
                        Err(_) => {
                            tracing::warn!(device_id = %device_id, "Device data collection timed out after {}s", QUERY_TIMEOUT_SECS);
                            Ok(Vec::new()) // Return empty result on timeout
                        }
                    }
                }
            })
            .collect();

        let results = join_all(timeout_futures).await;
        let collected: Vec<_> = results.into_iter()
            .filter_map(|r| r.ok())
            .flat_map(|v| v.into_iter())
            .collect();
        Ok(collected)
    }

    /// Collect data from a single device resource.
    async fn collect_single_device_data(
        device_service: Arc<DeviceService>,
        storage: Arc<neomind_storage::TimeSeriesStore>,
        device_id: &str,
        timestamp: i64,
    ) -> AgentResult<Vec<DataCollected>> {
        let mut data = Vec::new();

        // Get device info
        if let Some(device) = device_service.get_device(device_id).await {
            let device_values = serde_json::json!({
                "device_id": device.device_id,
                "device_type": device.device_type,
                "name": device.name,
                "adapter_type": device.adapter_type,
            });

            data.push(DataCollected {
                source: device_id.to_string(),
                data_type: "device_info".to_string(),
                values: device_values,
                timestamp,
            });

            // Try to get image metrics
            let end_time = chrono::Utc::now().timestamp();
            let start_time = end_time - (300); // Last 5 minutes

            let potential_metrics = vec![
                "values.image", "image", "snapshot", "values.snapshot",
                "camera.image", "camera.snapshot",
                "picture", "values.picture",
                "frame", "values.frame",
            ];

            // Try each metric until we find an image
            for metric_name in potential_metrics {
                if let Ok(result) = storage.query_range(device_id, metric_name, start_time, end_time).await
                    && !result.points.is_empty() {
                        let latest = &result.points[result.points.len() - 1];
                        let is_image = is_image_metric(metric_name, &latest.value);

                        if is_image {
                            let (image_url, image_base64, image_mime) = extract_image_data(&latest.value);

                            let values_json = serde_json::json!({
                                "value": latest.value,
                                "timestamp": latest.timestamp,
                                "points_count": result.points.len(),
                                "_is_image": true,
                                "image_url": image_url,
                                "image_base64": image_base64,
                                "image_mime_type": image_mime,
                            });

                            data.push(DataCollected {
                                source: format!("{}:{}", device_id, metric_name),
                                data_type: metric_name.to_string(),
                                values: values_json,
                                timestamp,
                            });

                            break; // Found an image, stop looking
                        }
                    }
            }
        }

        Ok(data)
    }

    /// Collect condensed memory summary.
    fn collect_memory_summary(
        &self,
        agent: &AiAgent,
        timestamp: i64,
    ) -> AgentResult<Option<DataCollected>> {
        if agent.memory.state_variables.is_empty() {
            return Ok(None);
        }

        let mut memory_summary = serde_json::Map::new();

        // Add last conclusion only
        if let Some(conclusion) = agent.memory.state_variables.get("last_conclusion").and_then(|v| v.as_str()) {
            memory_summary.insert("last_conclusion".to_string(), serde_json::json!(conclusion));
        }

        // Add condensed recent analyses (only conclusions)
        if let Some(analyses) = agent.memory.state_variables.get("recent_analyses").and_then(|v| v.as_array()) {
            let condensed: Vec<_> = analyses.iter()
                .take(2)
                .filter_map(|a| {
                    a.get("conclusion")
                        .and_then(|c| c.as_str())
                        .filter(|s| !s.is_empty())
                        .map(|c| serde_json::json!(c))
                })
                .collect();
            if !condensed.is_empty() {
                memory_summary.insert("recent_conclusions".to_string(), serde_json::json!(condensed));
            }
        }

        // Add execution count
        if let Some(count) = agent.memory.state_variables.get("total_executions").and_then(|v| v.as_i64()) {
            memory_summary.insert("total_executions".to_string(), serde_json::json!(count));
        }

        if memory_summary.is_empty() {
            Ok(None)
        } else {
            Ok(Some(DataCollected {
                source: "memory".to_string(),
                data_type: "summary".to_string(),
                values: serde_json::to_value(memory_summary).unwrap_or_default(),
                timestamp,
            }))
        }
    }

    /// Collect data including the triggering event data.
    /// This ensures that the event that triggered the agent is included in the analysis.
    async fn collect_data_with_event(
        &self,
        agent: &AiAgent,
        event_data: &EventTriggerData,
    ) -> AgentResult<Vec<DataCollected>> {
        let mut data = Vec::new();
        let _timestamp = chrono::Utc::now().timestamp();  // Reserved for future use

        // First, add the triggering event data directly
        let event_value_json = serde_json::to_value(&event_data.value).unwrap_or_default();

        // Check if the event value is an image
        let is_image = is_image_metric(&event_data.metric, &event_value_json);
        let (image_url, image_base64, image_mime) = if is_image {
            extract_image_data(&event_value_json)
        } else {
            (None, None, None)
        };

        let mut event_values = serde_json::json!({
            "value": event_data.value,
            "timestamp": event_data.timestamp,
            "_is_event_data": true,
        });

        // Add image metadata if applicable
        if is_image {
            event_values["_is_image"] = serde_json::json!(true);
            if let Some(ref url) = image_url {
                event_values["image_url"] = serde_json::json!(url);
            }
            if let Some(ref base64) = image_base64 {
                event_values["image_base64"] = serde_json::json!(base64);
            }
            if let Some(ref mime) = image_mime {
                event_values["image_mime_type"] = serde_json::json!(mime);
            }

            tracing::info!(
                device_id = %event_data.device_id,
                metric = %event_data.metric,
                has_url = image_url.is_some(),
                has_base64 = image_base64.is_some(),
                mime = ?image_mime,
                "Adding event-triggered image data to collection"
            );
        }

        data.push(DataCollected {
            source: format!("{}:{}", event_data.device_id, event_data.metric),
            data_type: event_data.metric.clone(),
            values: event_values,
            timestamp: event_data.timestamp,
        });

        // Then collect other data from regular sources
        let regular_data = self.collect_data(agent).await?;

        // Add regular data (excluding duplicates)
        for item in regular_data {
            // Skip if it's the "No data sources configured" placeholder
            if item.data_type == "info" && item.source == "system" {
                continue;
            }
            // Skip if it's the same event we already added
            if item.source == format!("{}:{}", event_data.device_id, event_data.metric) {
                continue;
            }
            data.push(item);
        }

        tracing::debug!(
            agent_id = %agent.id,
            event_device = %event_data.device_id,
            event_metric = %event_data.metric,
            total_data_count = data.len(),
            event_is_image = is_image,
            "Collected data including event trigger"
        );

        Ok(data)
    }

    /// Analyze situation using LLM or rule-based logic.
    async fn analyze_situation_with_intent(
        &self,
        agent: &AiAgent,
        data: &[DataCollected],
        parsed_intent: Option<&neomind_storage::ParsedIntent>,
        execution_id: &str,
    ) -> AgentResult<(String, Vec<ReasoningStep>, Vec<Decision>, String)> {
        tracing::info!(
            agent_id = %agent.id,
            agent_name = %agent.name,
            data_count = data.len(),
            execution_id = %execution_id,
            "[ANALYZE] Starting situation analysis"
        );

        match self.get_llm_runtime_for_agent(agent).await {
            Ok(Some(llm)) => {
                tracing::info!(
                    agent_id = %agent.id,
                    "LLM runtime available, performing LLM-based analysis"
                );
                match self.analyze_with_llm(llm, agent, data, parsed_intent, execution_id).await {
                    Ok(result) => {
                        tracing::info!(
                            agent_id = %agent.id,
                            "LLM-based analysis completed successfully"
                        );
                        return Ok(result);
                    }
                    Err(e) => {
                        tracing::warn!(
                            agent_id = %agent.id,
                            error = %e,
                            "LLM analysis failed, falling back to rule-based"
                        );
                    }
                }
            }
            Ok(None) => {
                tracing::warn!(
                    agent_id = %agent.id,
                    "No LLM runtime configured, falling back to rule-based analysis"
                );
            }
            Err(e) => {
                tracing::warn!(
                    agent_id = %agent.id,
                    error = %e,
                    "Failed to get LLM runtime, falling back to rule-based"
                );
            }
        }

        // Fall back to rule-based logic
        self.analyze_rule_based(agent, data, parsed_intent).await
    }

    /// Analyze situation using LLM for intelligent decision making.
    async fn analyze_with_llm(
        &self,
        llm: Arc<dyn neomind_core::llm::backend::LlmRuntime + Send + Sync>,
        agent: &AiAgent,
        data: &[DataCollected],
        parsed_intent: Option<&neomind_storage::ParsedIntent>,
        execution_id: &str,
    ) -> AgentResult<(String, Vec<ReasoningStep>, Vec<Decision>, String)> {
        use neomind_core::llm::backend::{LlmInput, GenerationParams};

        let current_time = chrono::Utc::now();
        let time_str = current_time.format("%Y-%m-%d %H:%M:%S UTC").to_string();
        let _timestamp = current_time.timestamp();

        tracing::info!(
            agent_id = %agent.id,
            data_count = data.len(),
            execution_id,
            current_time = %time_str,
            "Calling LLM for situation analysis..."
        );

        // Check if any data contains images
        let _has_images = data.iter().any(|d| {
            d.values.get("_is_image").and_then(|v| v.as_bool()).unwrap_or(false)
        });

        // Collect image parts first to check if we actually have valid image data
        let image_parts: Vec<_> = data.iter()
            .filter_map(|d| {
                let is_image = d.values.get("_is_image").and_then(|v| v.as_bool()).unwrap_or(false);
                if !is_image {
                    return None;
                }

                // Try to get image URL first
                if let Some(url) = d.values.get("image_url").and_then(|v| v.as_str()) {
                    return Some((
                        d.source.clone(),
                        d.data_type.clone(),
                        ImageContent::Url(url.to_string())
                    ));
                }

                // Try to get base64 data
                if let Some(base64) = d.values.get("image_base64").and_then(|v| v.as_str()) {
                    let mime = d.values.get("image_mime_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("image/jpeg");
                    return Some((
                        d.source.clone(),
                        d.data_type.clone(),
                        ImageContent::Base64(base64.to_string(), mime.to_string())
                    ));
                }

                None
            })
            .collect();

        // Check if LLM supports vision/multimodal
        let llm_supports_vision = llm.capabilities().supports_images;

        // Only use multimodal mode if we have valid images AND LLM supports vision
        let has_valid_images = !image_parts.is_empty() && llm_supports_vision;

        // Log when images are available but LLM doesn't support vision
        if !image_parts.is_empty() && !llm_supports_vision {
            tracing::warn!(
                agent_id = %agent.id,
                image_count = image_parts.len(),
                "Agent has image data but LLM doesn't support vision. Images will be ignored."
            );
        }

        // Build text data summary for non-image data
        // IMPORTANT: Filter out memory-related data to avoid confusing small models
        let max_metrics = 6;
        let text_data_summary: Vec<_> = data.iter()
            .filter(|d| {
                // Exclude images
                if d.values.get("_is_image").and_then(|v| v.as_bool()).unwrap_or(false) {
                    return false;
                }
                // Exclude memory-related data types (confuses small models)
                let data_type_lower = d.data_type.to_lowercase();
                !matches!(
                    data_type_lower.as_str(),
                    "summary" | "memory" | "state_variables" | "baselines" | "patterns"
                )
            })
            .take(max_metrics)
            .map(|d| {
                // Create a more compact representation of values
                let value_str = if let Some(v) = d.values.get("value") {
                    format!("{}", v) // Compact value representation
                } else if let Some(v) = d.values.get("history") {
                    format!("[历史数据: {}个点]", v.as_array().map(|a| a.len()).unwrap_or(0))
                } else {
                    // Fallback to compact JSON - use character-safe truncation
                    let json_str = serde_json::to_string(&d.values).unwrap_or_default();
                    if json_str.chars().count() > 200 {
                        // Truncate at character boundary, not byte boundary
                        json_str.chars().take(200).collect::<String>() + "..."
                    } else {
                        json_str
                    }
                };
                format!("- {}: {} = {}", d.source, d.data_type, value_str)
            })
            .collect();

        // Build intent context
        let _intent_context = if let Some(intent) = parsed_intent.or(agent.parsed_intent.as_ref()) {
            format!(
                "\n意图类型: {:?}\n目标指标: {:?}\n条件: {:?}\n动作: {:?}",
                intent.intent_type, intent.target_metrics, intent.conditions, intent.actions
            )
        } else {
            "".to_string()
        };

        // Build history context from conversation turns and memory
        let mut history_parts = Vec::new();

        // Add memory summary if available
        if !agent.memory.state_variables.is_empty() {
            // Get recent analyses from memory
            if let Some(analyses) = agent.memory.state_variables.get("recent_analyses").and_then(|v| v.as_array())
                && !analyses.is_empty() {
                    let summary: Vec<_> = analyses.iter()
                        .take(1) // Reduced to 1 for small models
                        .filter_map(|a| {
                            a.get("analysis").and_then(|an| an.as_str()).map(|txt| {
                                let conclusion = a.get("conclusion")
                                    .and_then(|c| c.as_str())
                                    .unwrap_or("");
                                if !conclusion.is_empty() {
                                    format!("- 分析: {} | 结论: {}", txt, conclusion)
                                } else {
                                    format!("- 分析: {}", txt)
                                }
                            })
                        })
                        .collect();

                    if !summary.is_empty() {
                        history_parts.push(format!(
                            "\n## 历史分析 (最近{}次)\n{}",
                            summary.len(),
                            summary.join("\n")
                        ));
                    }
                }

            // === SEMANTIC PATTERNS (Long-term memory) ===
            // Use learned_patterns instead of raw decision_patterns
            // Organized by pattern_type for better context
            if !agent.memory.learned_patterns.is_empty() {
                // Group patterns by type and show only the best from each category
                let mut pattern_groups: std::collections::HashMap<&str, Vec<&LearnedPattern>> = std::collections::HashMap::new();
                for pattern in &agent.memory.learned_patterns {
                    pattern_groups
                        .entry(pattern.pattern_type.as_str())
                        .or_default()
                        .push(pattern);
                }

                // Take only high-confidence patterns (>= 0.7) from each category
                let mut semantic_patterns = Vec::new();
                for (category, patterns) in pattern_groups.iter() {
                    if let Some(&best) = patterns.iter()
                        .filter(|p| p.confidence >= 0.7)
                        .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap_or(std::cmp::Ordering::Equal))
                    {
                        semantic_patterns.push(format!("- [{}] {} (置信度: {:.0}%)",
                            category, best.description, best.confidence * 100.0));
                    }
                }

                if !semantic_patterns.is_empty() {
                    history_parts.push(format!(
                        "\n## 已验证的决策模式\n{}",
                        semantic_patterns.join("\n")
                    ));
                }
            }

            // === BASELINES (Reference values) ===
            if !agent.memory.baselines.is_empty() {
                let baseline_info: Vec<_> = agent.memory.baselines
                    .iter()
                    .take(3) // Reduced for small models
                    .map(|(metric, value)| format!("- {}: 基线值 {:.2}", metric, value))
                    .collect();
                history_parts.push(format!(
                    "\n## 指标基线\n{}",
                    baseline_info.join("\n")
                ));
            }
        }

        // === CONTEXT MANAGEMENT ===
        // === HISTORY CONTEXT - DISABLED for small models ===
        // The compressed history context is NOT used to avoid confusing qwen3:1.7b
        let _history_context = "";  // Intentionally unused

        // === USER MESSAGES (用户发送的消息) ===
        // Build user messages context for adding to user message (not system message)
        let user_messages_for_user_msg = if !agent.user_messages.is_empty() {
            let user_msgs_text: Vec<String> = agent.user_messages
                .iter()
                .enumerate()
                .map(|(i, msg)| {
                    let timestamp_str = chrono::DateTime::from_timestamp(msg.timestamp, 0)
                        .map(|dt| dt.format("%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| "??".to_string());
                    format!("{}. [{}] {}", i + 1, timestamp_str, msg.content)
                })
                .collect();

            Some(format!(
                "## ⚠️ 用户最新指令 (必须严格遵循)\n\n\
                用户在运行期间发送了以下消息，这些消息包含对执行策略的更新。\
                **请务必将这些指令作为最高优先级，覆盖初始配置中的任何冲突规则：**\n\n\
                {}\n",
                user_msgs_text.join("\n")
            ))
        } else {
            None
        };

        // === SEMANTIC MEMORY CONTEXT ===
        // Compress memory into meaning-preserving format that small models can understand
        let memory_context = {
            let mut parts = Vec::new();

            // 1. Recent success pattern (learned from what works)
            if !agent.memory.short_term.summaries.is_empty() {
                let last_3: Vec<_> = agent.memory.short_term.summaries.iter().rev().take(3).collect();
                let success_rate = last_3.iter().filter(|s| s.success).count() as f32 / last_3.len() as f32;

                if success_rate >= 0.8 {
                    parts.push("Recent: Success pattern established".to_string());
                } else if success_rate <= 0.3 {
                    parts.push("Recent: Multiple failures, needs new approach".to_string());
                }
            }

            // 2. Action patterns (what actions typically work)
            if !agent.memory.learned_patterns.is_empty() {
                let high_confidence: Vec<_> = agent.memory.learned_patterns.iter()
                    .filter(|p| p.confidence >= 0.75)
                    .collect();

                if !high_confidence.is_empty() {
                    let pattern_summary = high_confidence.iter()
                        .map(|p| truncate_to(&p.description, 20))
                        .collect::<Vec<_>>()
                        .join(", ");
                    parts.push(format!("Patterns: {}", pattern_summary));
                }
            }

            // 3. Baseline anomalies (if current values deviate significantly)
            if !agent.memory.baselines.is_empty() && !data.is_empty() {
                // Check if any current data significantly deviates from baseline
                for (metric, baseline) in agent.memory.baselines.iter().take(2) {
                    for d in data.iter().take(3) {
                        if let Some(val) = d.values.get("value").and_then(|v| v.as_f64())
                            && (val - baseline).abs() / baseline.abs().max(0.1) > 0.3 {
                                parts.push(format!("Anomaly: {} changed significantly", metric));
                                break;
                            }
                    }
                }
            }

            if parts.is_empty() {
                String::new()
            } else {
                format!("[Memory: {}]", parts.join(" | "))
            }
        };

        // === SYSTEM PROMPT - Restore original working structure ===
        // This was the proven working format - don't over-engineer it
        let role_prompt = "You are an IoT automation assistant. Output ONLY valid JSON. No other text.";

        // Get current time context for temporal understanding
        let time_context = get_time_context();

        let system_prompt = if has_valid_images {
            format!(
                "{}\n\n{}\n\n# 输出格式 - 仅输出JSON，不要输出其他任何文字\n{{\n  \"situation_analysis\": \"图像内容描述\",\n  \"reasoning_steps\": [{{\"step\": 1, \"description\": \"分析步骤\", \"confidence\": 0.9}}],\n  \"decisions\": [{{\"decision_type\": \"info\", \"description\": \"描述\", \"action\": \"log\", \"rationale\": \"理由\", \"confidence\": 0.8}}],\n  \"conclusion\": \"结论\"\n}}\n\n# 用户指令\n{}\n\n# 决策类型\n- info: 仅记录观察\n- alert: 检测到目标，发送告警\n- command: 执行设备指令",
                role_prompt, time_context, agent.user_prompt
            )
        } else {
            format!(
                "{}\n\n{}\n\n# 输出格式 - 仅输出JSON，不要输出其他任何文字\n{{\n  \"situation_analysis\": \"情况分析\",\n  \"reasoning_steps\": [{{\"step\": 1, \"description\": \"步骤\", \"confidence\": 0.9}}],\n  \"decisions\": [{{\"decision_type\": \"info\", \"description\": \"描述\", \"action\": \"log\", \"rationale\": \"理由\", \"confidence\": 0.8}}],\n  \"conclusion\": \"结论\"\n}}\n\n# 用户指令\n{}\n\n# 决策类型\n- info: 仅记录\n- alert: 发送告警\n- command: 执行指令",
                role_prompt, time_context, agent.user_prompt
            )
        };

        // === CONTEXT MANAGEMENT ===
        // For image analysis, include minimal memory context
        let memory_context_for_msg = if !memory_context.is_empty() {
            format!("\n\n# 历史参考\n{}", memory_context)
        } else {
            String::new()
        };

        // Build messages - multimodal if images present
        let messages = if has_valid_images {
            let mut parts = vec![ContentPart::text(
                format!("## 当前数据\n{}\n\n重要：只输出JSON格式，不要有任何其他文字。",
                    if text_data_summary.is_empty() {
                        "仅有图像数据".to_string()
                    } else {
                        text_data_summary.join("\n")
                    }
                )
            )];

            // Add images
            for (source, _data_type, image_content) in &image_parts {
                if let ImageContent::Base64(data, mime) = image_content {
                    parts.push(ContentPart::image_base64(data.clone(), mime.clone()));
                    tracing::debug!(source = %source, mime = %mime, "Adding base64 image to LLM message");
                }
            }

            // Add memory context and user messages
            if !memory_context_for_msg.is_empty() {
                parts.push(ContentPart::text(memory_context_for_msg));
            }
            if let Some(ref user_msgs) = user_messages_for_user_msg {
                parts.push(ContentPart::text(format!("\n\n{}", user_msgs)));
            }

            vec![
                Message::new(MessageRole::System, Content::text(system_prompt)),
                Message::from_parts(MessageRole::User, parts),
            ]
        } else {
            // Text-only message
            let data_summary = if text_data_summary.is_empty() {
                "No data available".to_string()
            } else {
                text_data_summary.join("\n")
            };

            let mut user_msg_content = format!(
                "## 当前数据\n{}\n\n只输出JSON，不要有其他文字。",
                data_summary
            );

            if !memory_context_for_msg.is_empty() {
                user_msg_content = format!("{}\n\n{}", user_msg_content, memory_context_for_msg);
            }
            if let Some(ref user_msgs) = user_messages_for_user_msg {
                user_msg_content = format!("{}\n\n{}", user_msg_content, user_msgs);
            }

            vec![
                Message::new(MessageRole::System, Content::text(system_prompt)),
                Message::new(MessageRole::User, Content::text(user_msg_content)),
            ]
        };

        let input = LlmInput {
            messages,
            params: GenerationParams {
                temperature: Some(0.7),
                max_tokens: Some(5000), // Balanced for speed and completeness
                ..Default::default()
            },
            model: None,
            stream: false,
            tools: Some(Vec::new()),
        };

        // Add timeout for LLM generation (5 minutes max)
        const LLM_TIMEOUT_SECS: u64 = 300;
        let llm_result = match tokio::time::timeout(
            std::time::Duration::from_secs(LLM_TIMEOUT_SECS),
            llm.generate(input)
        ).await {
            Ok(result) => result,
            Err(_) => {
                tracing::warn!(
                    agent_id = %agent.id,
                    "LLM generation timed out after {}s",
                    LLM_TIMEOUT_SECS
                );
                return Err(NeoMindError::Llm(format!("LLM timeout after {}s", LLM_TIMEOUT_SECS)));
            }
        };

        match llm_result {
            Ok(output) => {
                let json_str = output.text.trim();
                // Extract JSON if wrapped in markdown
                let json_str = if json_str.contains("```json") {
                    json_str.split("```json").nth(1)
                        .and_then(|s| s.split("```").next())
                        .unwrap_or(json_str)
                        .trim()
                } else if json_str.contains("```") {
                    json_str.split("```").nth(1)
                        .unwrap_or(json_str)
                        .trim()
                } else {
                    json_str
                };

                // Parse the LLM response
                #[derive(serde::Deserialize)]
                struct LlmResponse {
                    #[serde(default)]
                    situation_analysis: String,
                    #[serde(default)]
                    reasoning_steps: Vec<ReasoningFromLlm>,
                    #[serde(default)]
                    decisions: Vec<DecisionFromLlm>,
                    #[serde(default)]
                    conclusion: String,
                }

                #[derive(serde::Deserialize)]
                struct ReasoningFromLlm {
                    #[serde(alias = "step_number", default)]
                    step: serde_json::Value,
                    #[serde(alias = "output", default)]
                    description: String,
                    #[serde(default)]
                    confidence: f32,
                }

                // Helper to extract step number from either string or number
                fn extract_step_number(value: &serde_json::Value, default: u32) -> u32 {
                    match value {
                        serde_json::Value::Number(n) => n.as_u64().unwrap_or(default as u64) as u32,
                        serde_json::Value::String(s) => s.parse().unwrap_or(default),
                        _ => default,
                    }
                }

                #[derive(serde::Deserialize)]
                struct DecisionFromLlm {
                    #[serde(default)]
                    decision_type: String,
                    #[serde(default)]
                    description: String,
                    #[serde(default)]
                    action: String,
                    #[serde(default)]
                    rationale: String,
                    #[serde(default)]
                    confidence: f32,
                }

                match serde_json::from_str::<LlmResponse>(json_str) {
                    Ok(response) => {
                        let reasoning_steps: Vec<neomind_storage::ReasoningStep> = response.reasoning_steps
                            .into_iter()
                            .enumerate()
                            .map(|(_i, step)| neomind_storage::ReasoningStep {
                                step_number: extract_step_number(&step.step, (_i + 1) as u32),
                                description: step.description,
                                step_type: "llm_analysis".to_string(),
                                input: Some(text_data_summary.join("\n")),
                                output: response.situation_analysis.clone(),
                                confidence: step.confidence,
                            })
                            .collect();

                        let decisions: Vec<neomind_storage::Decision> = response.decisions
                            .into_iter()
                            .map(|d| neomind_storage::Decision {
                                decision_type: d.decision_type,
                                description: d.description,
                                action: d.action,
                                rationale: d.rationale,
                                expected_outcome: response.conclusion.clone(),
                            })
                            .collect();

                        // Emit AgentThinking events for each reasoning step
                        if let Some(ref bus) = self.event_bus {
                            let event_timestamp = chrono::Utc::now().timestamp();
                            for step in &reasoning_steps {
                                let _ = bus.publish(NeoMindEvent::AgentThinking {
                                    agent_id: agent.id.clone(),
                                    execution_id: execution_id.to_string(),
                                    step_number: step.step_number,
                                    step_type: step.step_type.clone(),
                                    description: step.description.clone(),
                                    details: None,
                                    timestamp: event_timestamp,
                                }).await;
                            }

                            // Emit AgentDecision events for each decision
                            for decision in &decisions {
                                let _ = bus.publish(NeoMindEvent::AgentDecision {
                                    agent_id: agent.id.clone(),
                                    execution_id: execution_id.to_string(),
                                    description: decision.description.clone(),
                                    rationale: decision.rationale.clone(),
                                    action: decision.action.clone(),
                                    confidence: 0.8_f32,
                                    timestamp: event_timestamp,
                                }).await;
                            }
                        }

                        Ok((
                            response.situation_analysis,
                            reasoning_steps,
                            decisions,
                            response.conclusion,
                        ))
                    }
                    Err(parse_error) => {
                        tracing::warn!(
                            error = %parse_error,
                            response_preview = %json_str.chars().take(500).collect::<String>(),
                            "Failed to parse LLM JSON response, attempting recovery"
                        );

                        // STEP 1: Try to extract JSON from mixed text (model output text before JSON)
                        if let Some(extracted_json) = extract_json_from_mixed_text(json_str) {
                            tracing::info!(
                                agent_id = %agent.id,
                                extracted_len = extracted_json.len(),
                                "Successfully extracted JSON from mixed text response"
                            );
                            match serde_json::from_str::<LlmResponse>(&extracted_json) {
                                Ok(response) => {
                                    let reasoning_steps: Vec<neomind_storage::ReasoningStep> = response.reasoning_steps
                                        .into_iter()
                                        .enumerate()
                                        .map(|(_i, step)| neomind_storage::ReasoningStep {
                                            step_number: extract_step_number(&step.step, (_i + 1) as u32),
                                            description: step.description,
                                            step_type: "llm_analysis".to_string(),
                                            input: Some(text_data_summary.join("\n")),
                                            output: response.situation_analysis.clone(),
                                            confidence: step.confidence,
                                        })
                                        .collect();

                                    let decisions: Vec<neomind_storage::Decision> = response.decisions
                                        .into_iter()
                                        .map(|decision| neomind_storage::Decision {
                                            decision_type: decision.decision_type,
                                            description: decision.description,
                                            action: decision.action,
                                            rationale: decision.rationale,
                                            expected_outcome: format!("Confidence: {:.0}%", decision.confidence * 100.0),
                                        })
                                        .collect();

                                    return Ok((
                                        response.situation_analysis,
                                        reasoning_steps,
                                        decisions,
                                        response.conclusion,
                                    ));
                                }
                                Err(_) => {
                                    tracing::warn!("Extracted JSON failed to parse as LlmResponse");
                                }
                            }
                        }

                        // STEP 2: Try to recover truncated JSON by finding the last complete object
                        let recovered = try_recover_truncated_json(json_str);

                        if let Some((recovered_json, was_truncated)) = recovered {
                            if was_truncated {
                                tracing::info!(
                                    agent_id = %agent.id,
                                    "Successfully recovered truncated JSON response"
                                );
                            }
                            match serde_json::from_str::<LlmResponse>(&recovered_json) {
                                Ok(response) => {
                                    let reasoning_steps: Vec<neomind_storage::ReasoningStep> = response.reasoning_steps
                                        .into_iter()
                                        .enumerate()
                                        .map(|(_i, step)| neomind_storage::ReasoningStep {
                                            step_number: extract_step_number(&step.step, (_i + 1) as u32),
                                            description: step.description,
                                            step_type: "llm_analysis".to_string(),
                                            input: Some(text_data_summary.join("\n")),
                                            output: response.situation_analysis.clone(),
                                            confidence: step.confidence,
                                        })
                                        .collect();

                                    let decisions: Vec<neomind_storage::Decision> = response.decisions
                                        .into_iter()
                                        .map(|decision| neomind_storage::Decision {
                                            decision_type: decision.decision_type,
                                            description: decision.description,
                                            action: decision.action,
                                            rationale: decision.rationale,
                                            expected_outcome: format!("Confidence: {:.0}%", decision.confidence * 100.0),
                                        })
                                        .collect();

                                    return Ok((
                                        response.situation_analysis,
                                        reasoning_steps,
                                        decisions,
                                        if was_truncated {
                                            format!("{} (Response was truncated, some content may be incomplete)", response.conclusion)
                                        } else {
                                            response.conclusion
                                        },
                                    ));
                                }
                                Err(e) => {
                                    tracing::debug!(error = %e, "Recovered JSON still failed to parse, trying lenient extraction");
                                }
                            }
                        }

                        // Lenient extraction: parse as Value and extract fields (handles different LLM JSON shapes)
                        if let Ok(value) = serde_json::from_str::<serde_json::Value>(json_str)
                            && let Some(obj) = value.as_object() {
                                let situation_analysis: String = obj
                                    .get("situation_analysis")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let conclusion: String = obj
                                    .get("conclusion")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let mut reasoning_steps = Vec::new();
                                if let Some(arr) = obj.get("reasoning_steps").and_then(|v| v.as_array()) {
                                    for (i, item) in arr.iter().enumerate() {
                                        let step_num = (i + 1) as u32;
                                        let description: String = item
                                            .get("description")
                                            .and_then(|v| v.as_str())
                                            .or_else(|| item.get("output").and_then(|v| v.as_str()))
                                            .unwrap_or("")
                                            .to_string();
                                        if description.is_empty() {
                                            continue;
                                        }
                                        let confidence = item
                                            .get("confidence")
                                            .and_then(|v| v.as_f64())
                                            .unwrap_or(0.8) as f32;
                                        reasoning_steps.push(neomind_storage::ReasoningStep {
                                            step_number: step_num,
                                            description,
                                            step_type: "llm_analysis".to_string(),
                                            input: Some(text_data_summary.join("\n")),
                                            output: situation_analysis.clone(),
                                            confidence,
                                        });
                                    }
                                }
                                let mut decisions = Vec::new();
                                if let Some(arr) = obj.get("decisions").and_then(|v| v.as_array()) {
                                    for item in arr {
                                        let decision_type = item.get("decision_type").and_then(|v| v.as_str()).unwrap_or("analysis").to_string();
                                        let description = item.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                        let action = item.get("action").and_then(|v| v.as_str()).unwrap_or("review").to_string();
                                        let rationale = item.get("rationale").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                        decisions.push(neomind_storage::Decision {
                                            decision_type,
                                            description,
                                            action,
                                            rationale,
                                            expected_outcome: conclusion.clone(),
                                        });
                                    }
                                }
                                if !situation_analysis.is_empty() || !conclusion.is_empty() {
                                    tracing::info!(
                                        agent_id = %agent.id,
                                        "Extracted decision process from JSON via lenient parsing"
                                    );
                                    return Ok((
                                        if situation_analysis.is_empty() { conclusion.chars().take(500).collect::<String>() } else { situation_analysis.clone() },
                                        if reasoning_steps.is_empty() {
                                            vec![neomind_storage::ReasoningStep {
                                                step_number: 1,
                                                description: "LLM analysis completed".to_string(),
                                                step_type: "llm_analysis".to_string(),
                                                input: Some(format!("{} data sources", data.len())),
                                                output: situation_analysis.clone(),
                                                confidence: 0.7,
                                            }]
                                        } else {
                                            reasoning_steps
                                        },
                                        if decisions.is_empty() {
                                            vec![neomind_storage::Decision {
                                                decision_type: "analysis".to_string(),
                                                description: "See situation analysis for details".to_string(),
                                                action: "review".to_string(),
                                                rationale: "LLM provided structured analysis".to_string(),
                                                expected_outcome: conclusion.clone(),
                                            }]
                                        } else {
                                            decisions
                                        },
                                        if conclusion.is_empty() { "分析完成。".to_string() } else { conclusion },
                                    ));
                                }
                            }

                        // Final fallback: use raw text - show actual content, not placeholder
                        let raw_text = output.text.trim();
                        let situation_analysis = if raw_text.chars().count() > 1000 {
                            raw_text.chars().take(1000).collect::<String>() + "..."
                        } else {
                            raw_text.to_string()
                        };
                        let char_count = raw_text.chars().count();
                        let conclusion = if char_count > 500 {
                            raw_text.chars().skip(char_count.saturating_sub(500)).collect::<String>()
                                + "..."
                        } else {
                            raw_text.to_string()
                        };

                        let reasoning_steps = vec![
                            neomind_storage::ReasoningStep {
                                step_number: 1,
                                description: if situation_analysis.chars().count() > 200 {
                                    situation_analysis.chars().take(200).collect::<String>() + "..."
                                } else {
                                    situation_analysis.clone()
                                },
                                step_type: "llm_analysis".to_string(),
                                input: Some(format!("{} data sources", data.len())),
                                output: situation_analysis.clone(),
                                confidence: 0.7,
                            }
                        ];

                        let decisions = vec![
                            neomind_storage::Decision {
                                decision_type: "analysis".to_string(),
                                description: "See situation analysis for details".to_string(),
                                action: "review".to_string(),
                                rationale: "LLM provided text response instead of structured JSON".to_string(),
                                expected_outcome: "Manual review of analysis recommended".to_string(),
                            }
                        ];

                        tracing::info!(
                            agent_id = %agent.id,
                            raw_response_length = raw_text.len(),
                            "Using raw LLM response as fallback (content preserved)"
                        );

                        Ok((
                            situation_analysis,
                            reasoning_steps,
                            decisions,
                            conclusion,
                        ))
                    }
                }
            }
            Err(e) => {
                tracing::error!(
                    agent_id = %agent.id,
                    error = %e,
                    error_details = ?e,
                    "LLM generation failed - check LLM backend configuration and connectivity"
                );
                Err(NeoMindError::Llm(format!("LLM generation failed: {}", e)))
            }
        }
    }

    /// Rule-based analysis fallback.
    async fn analyze_rule_based(
        &self,
        agent: &AiAgent,
        data: &[DataCollected],
        parsed_intent: Option<&neomind_storage::ParsedIntent>,
    ) -> AgentResult<(String, Vec<ReasoningStep>, Vec<Decision>, String)> {
        let mut reasoning_steps = Vec::new();
        let mut decisions = Vec::new();

        // Step 1: Understand the situation
        let situation_analysis = format!(
            "Analyzing {} data points for agent '{}'",
            data.len(),
            agent.name
        );

        reasoning_steps.push(ReasoningStep {
            step_number: 1,
            description: "Collect and analyze input data".to_string(),
            step_type: "data_collection".to_string(),
            input: Some(format!("{} data sources", data.len())),
            output: format!("Data collected from {} sources", data.len()),
            confidence: 1.0,
        });

        // Step 2: Evaluate conditions based on parsed intent
        let intent = parsed_intent.or(agent.parsed_intent.as_ref());
        if let Some(intent) = intent {
            for condition in &intent.conditions {
                let result = self.evaluate_condition(condition, data).await;

                reasoning_steps.push(ReasoningStep {
                    step_number: reasoning_steps.len() as u32 + 1,
                    description: format!("Evaluate condition: {}", condition),
                    step_type: "condition_eval".to_string(),
                    input: Some(condition.clone()),
                    output: format!("Condition result: {}", result),
                    confidence: 0.8,
                });

                if result {
                    decisions.push(Decision {
                        decision_type: "condition_met".to_string(),
                        description: format!("Condition '{}' is met", condition),
                        action: "trigger_actions".to_string(),
                        rationale: format!("The condition '{}' evaluated to true", condition),
                        expected_outcome: "Execute defined actions".to_string(),
                    });
                }
            }
        }

        // Step 3: Determine actions
        let empty_actions = vec![];
        let actions = intent.map(|i| &i.actions).unwrap_or(&empty_actions);
        if !decisions.is_empty() {
            for action in actions {
                reasoning_steps.push(ReasoningStep {
                    step_number: reasoning_steps.len() as u32 + 1,
                    description: format!("Plan action: {}", action),
                    step_type: "action_planning".to_string(),
                    input: Some(action.clone()),
                    output: format!("Action '{}' queued for execution", action),
                    confidence: 0.7,
                });
            }
        }

        let conclusion = if decisions.is_empty() {
            "No actions required - conditions not met".to_string()
        } else {
            format!("{} action(s) to be executed", decisions.len())
        };

        Ok((situation_analysis, reasoning_steps, decisions, conclusion))
    }

    /// Evaluate a condition against collected data.
    async fn evaluate_condition(&self, condition: &str, data: &[DataCollected]) -> bool {
        let condition_lower = condition.to_lowercase();

        // Check if any data meets the condition
        for data_item in data {
            if let Some(value) = data_item.values.get("value")
                && let Some(num) = value.as_f64() {
                    if condition_lower.contains("大于") || condition_lower.contains(">") || condition_lower.contains("超过") {
                        if let Some(threshold) = extract_threshold(&condition_lower) {
                            return num > threshold;
                        }
                    } else if (condition_lower.contains("小于") || condition_lower.contains("<") || condition_lower.contains("低于"))
                        && let Some(threshold) = extract_threshold(&condition_lower) {
                            return num < threshold;
                        }
                }
        }

        false
    }

    /// Execute decisions - real command execution.
    async fn execute_decisions(
        &self,
        agent: &AiAgent,
        decisions: &[Decision],
    ) -> AgentResult<(Vec<neomind_storage::ActionExecuted>, Vec<neomind_storage::NotificationSent>)> {
        let mut actions_executed = Vec::new();
        let mut notifications_sent = Vec::new();

        for decision in decisions {
            // Execute actions based on decision type
            if decision.decision_type == "condition_met" {
                // Execute commands defined in agent resources
                if let Some(ref device_service) = self.device_service {
                    for resource in &agent.resources {
                        if resource.resource_type == ResourceType::Command {
                            // Parse device_id and command from resource_id
                            // Format: "device_id:command_name"
                            let parts: Vec<&str> = resource.resource_id.split(':').collect();
                            if parts.len() == 2 {
                                let device_id = parts[0];
                                let command_name = parts[1];

                                // Get parameters from resource config
                                let parameters = resource.config.get("parameters")
                                    .and_then(|v| v.as_object())
                                    .cloned()
                                    .unwrap_or_default();

                                // Convert parameters to HashMap for DeviceService
                                let params_map: std::collections::HashMap<String, serde_json::Value> = parameters
                                    .into_iter()
                                    .collect();

                                // Actually execute the command via DeviceService
                                let execution_result = device_service.send_command(
                                    device_id,
                                    command_name,
                                    params_map,
                                ).await;

                                let (success, result) = match execution_result {
                                    Ok(_) => (true, Some("Command sent successfully".to_string())),
                                    Err(e) => {
                                        tracing::warn!(
                                            agent_id = %agent.id,
                                            device_id = %device_id,
                                            command = %command_name,
                                            error = %e,
                                            "Failed to send command"
                                        );
                                        (false, Some(format!("Failed: {}", e)))
                                    }
                                };

                                // Re-create parameters for ActionExecuted record
                                let parameters_for_record = resource.config.get("parameters")
                                    .and_then(|v| v.as_object())
                                    .cloned()
                                    .unwrap_or_default();

                                actions_executed.push(neomind_storage::ActionExecuted {
                                    action_type: "device_command".to_string(),
                                    description: format!("Execute {} on {}", command_name, device_id),
                                    target: device_id.to_string(),
                                    parameters: serde_json::to_value(parameters_for_record).unwrap_or_default(),
                                    success,
                                    result,
                                });
                            }
                        }
                    }
                }

                // Send notifications for alert actions
                let should_send_alert = agent.parsed_intent.as_ref()
                    .map(|i| i.actions.iter().any(|a| {
                        a.contains("alert") || a.contains("notification") ||
                        a.contains("报警") || a.contains("通知")
                    }))
                    .unwrap_or(false);

                // Debug log for notification trigger
                tracing::debug!(
                    agent_id = %agent.id,
                    should_send_alert,
                    has_parsed_intent = agent.parsed_intent.is_some(),
                    actions = ?agent.parsed_intent.as_ref().map(|i| &i.actions),
                    has_message_manager = self.message_manager.is_some(),
                    "Checking if alert should be sent"
                );

                if should_send_alert {
                    self.send_alert_for_decision(agent, decision, &mut notifications_sent).await;
                }
            }

            // NEW: Send alert for alert-type decisions regardless of parsed_intent
            // Check if this decision is an alert decision
            let is_alert_decision = decision.decision_type.to_lowercase().contains("alert") ||
                                   decision.action.to_lowercase().contains("alert") ||
                                   decision.action.to_lowercase().contains("报警") ||
                                   decision.action.to_lowercase().contains("notify") ||
                                   decision.action.to_lowercase().contains("通知");

            if is_alert_decision {
                tracing::info!(
                    agent_id = %agent.id,
                    decision_type = %decision.decision_type,
                    decision_action = %decision.action,
                    "Alert-type decision detected, sending notification"
                );
                self.send_alert_for_decision(agent, decision, &mut notifications_sent).await;
            }

            // Execute specific actions based on decision.action
            if decision.action.to_lowercase().contains("execute_command") ||
               decision.action.to_lowercase().contains("command") ||
               decision.action.to_lowercase().contains("执行指令") ||
               decision.action.to_lowercase().contains("控制") {
                // Execute commands defined in agent resources
                if let Some(ref device_service) = self.device_service {
                    // Check if decision.description specifies which commands to execute
                    // Format: "execute command: turn_on_light" or "执行指令: open_valve"
                    let mentioned_command = extract_command_from_description(&decision.description);
                    let mentioned_device = extract_device_from_description(&decision.description);

                    let commands_to_execute: Vec<_> = agent.resources.iter()
                        .filter(|r| r.resource_type == ResourceType::Command)
                        .filter(|r| {
                            // Filter by mentioned command if specified
                            if let Some(ref cmd_name) = mentioned_command {
                                r.resource_id.ends_with(&format!(":{}", cmd_name)) ||
                                r.resource_id.contains(cmd_name)
                            } else if let Some(ref dev_id) = mentioned_device {
                                r.resource_id.starts_with(&format!("{}:", dev_id))
                            } else {
                                true // No filter, include all commands (safe default)
                            }
                        })
                        .collect();

                    if commands_to_execute.is_empty() {
                        tracing::warn!(
                            agent_id = %agent.id,
                            decision_description = %decision.description,
                            "No matching commands found for execution"
                        );
                    } else {
                        tracing::info!(
                            agent_id = %agent.id,
                            command_count = commands_to_execute.len(),
                            "Executing {} command(s) from decision",
                            commands_to_execute.len()
                        );
                    }

                    for resource in commands_to_execute {
                        // Parse device_id and command from resource_id
                        // Format: "device_id:command_name"
                        let parts: Vec<&str> = resource.resource_id.split(':').collect();
                        if parts.len() == 2 {
                            let device_id = parts[0];
                            let command_name = parts[1];

                            // Get parameters from resource config
                            let parameters = resource.config.get("parameters")
                                .and_then(|v| v.as_object())
                                .cloned()
                                .unwrap_or_default();

                            // Convert parameters to HashMap for DeviceService
                            let params_map: std::collections::HashMap<String, serde_json::Value> = parameters
                                .into_iter()
                                .collect();

                            tracing::info!(
                                agent_id = %agent.id,
                                device_id = %device_id,
                                command = %command_name,
                                "Executing command from decision action"
                            );

                            // Actually execute the command via DeviceService
                            let execution_result = device_service.send_command(
                                device_id,
                                command_name,
                                params_map,
                            ).await;

                            let (success, result) = match execution_result {
                                Ok(_) => (true, Some("Command sent successfully".to_string())),
                                Err(e) => {
                                    tracing::warn!(
                                        agent_id = %agent.id,
                                        device_id = %device_id,
                                        command = %command_name,
                                        error = %e,
                                        "Failed to send command"
                                    );
                                    (false, Some(format!("Failed: {}", e)))
                                }
                            };

                            // Re-create parameters for ActionExecuted record
                            let parameters_for_record = resource.config.get("parameters")
                                .and_then(|v| v.as_object())
                                .cloned()
                                .unwrap_or_default();

                            actions_executed.push(neomind_storage::ActionExecuted {
                                action_type: "device_command".to_string(),
                                description: format!("Execute {} on {} (triggered by decision: {})", command_name, device_id, decision.action),
                                target: device_id.to_string(),
                                parameters: serde_json::to_value(parameters_for_record).unwrap_or_default(),
                                success,
                                result,
                            });
                        }
                    }
                }
            }
        }

        Ok((actions_executed, notifications_sent))
    }

    /// Send an alert for a specific decision.
    async fn send_alert_for_decision(
        &self,
        agent: &AiAgent,
        decision: &neomind_storage::Decision,
        notifications_sent: &mut Vec<neomind_storage::NotificationSent>,
    ) {
        let alert_message = format!("Agent '{}' - {}: {}", agent.name, decision.decision_type, decision.description);

        // Send via MessageManager if available
        if let Some(ref message_manager) = self.message_manager {
            use neomind_messages::{Message, MessageSeverity};

            // Determine severity based on decision type
            let severity = if decision.decision_type.to_lowercase().contains("critical") ||
                             decision.decision_type.to_lowercase().contains("emergency") ||
                             decision.decision_type.to_lowercase().contains("紧急") {
                MessageSeverity::Critical
            } else if decision.decision_type.to_lowercase().contains("warning") ||
                       decision.decision_type.to_lowercase().contains("警告") {
                MessageSeverity::Warning
            } else {
                MessageSeverity::Info
            };

            // Set source_type to "agent" for better tracking
            let mut msg = Message::alert(
                severity,
                format!("Agent Alert: {}", agent.name),
                alert_message.clone(),
                agent.id.clone(),
            );
            msg.source_type = "agent".to_string();

            tracing::info!(
                agent_id = %agent.id,
                alert_message = %alert_message,
                severity = ?severity,
                "Sending message via MessageManager"
            );

            match message_manager.create_message(msg).await {
                Ok(msg) => {
                    notifications_sent.push(neomind_storage::NotificationSent {
                        channel: "message_manager".to_string(),
                        recipient: "configured_channels".to_string(),
                        message: alert_message,
                        sent_at: chrono::Utc::now().timestamp(),
                        success: true,
                    });
                    tracing::info!(
                        agent_id = %agent.id,
                        message_id = %msg.id.to_string(),
                        "Message sent via MessageManager successfully"
                    );
                }
                Err(e) => {
                    notifications_sent.push(neomind_storage::NotificationSent {
                        channel: "message_manager".to_string(),
                        recipient: "configured_channels".to_string(),
                        message: alert_message.clone(),
                        sent_at: chrono::Utc::now().timestamp(),
                        success: false,
                    });
                    tracing::warn!(
                        agent_id = %agent.id,
                        error = %e,
                        "Failed to send message via MessageManager"
                    );
                }
            }
        } else {
            // Fallback: Publish event to EventBus if MessageManager not available
            tracing::warn!(
                agent_id = %agent.id,
                "MessageManager not available, using EventBus fallback"
            );
            if let Some(ref bus) = self.event_bus {
                let _ = bus.publish(NeoMindEvent::MessageCreated {
                    message_id: uuid::Uuid::new_v4().to_string(),
                    title: format!("Agent Alert: {}", agent.name),
                    severity: "info".to_string(),
                    message: decision.description.clone(),
                    timestamp: chrono::Utc::now().timestamp(),
                }).await;

                notifications_sent.push(neomind_storage::NotificationSent {
                    channel: "event_bus".to_string(),
                    recipient: "event_subscribers".to_string(),
                    message: alert_message,
                    sent_at: chrono::Utc::now().timestamp(),
                    success: true,
                });
            }
        }
    }

    /// Generate report if needed.
    async fn maybe_generate_report(
        &self,
        agent: &AiAgent,
        data: &[DataCollected],
    ) -> AgentResult<Option<GeneratedReport>> {
        // Only generate reports for report generation agents
        if let Some(ref intent) = agent.parsed_intent
            && matches!(
                intent.intent_type,
                neomind_storage::IntentType::ReportGeneration
            ) {
                let content = self.generate_report_content(agent, data).await?;

                return Ok(Some(GeneratedReport {
                    report_type: "summary".to_string(),
                    content,
                    data_summary: data
                        .iter()
                        .map(|d| neomind_storage::DataSummary {
                            source: d.source.clone(),
                            metric: d.data_type.clone(),
                            count: 1,
                            statistics: d.values.clone(),
                        })
                        .collect(),
                    generated_at: chrono::Utc::now().timestamp(),
                }));
            }

        Ok(None)
    }

    /// Generate report content.
    async fn generate_report_content(&self, agent: &AiAgent, data: &[DataCollected]) -> AgentResult<String> {
        let mut report = format!("# {} - 报告\n\n", agent.name);
        report.push_str(&format!("生成时间: {}\n\n", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S")));

        report.push_str("## 数据摘要\n\n");
        for data_item in data {
            report.push_str(&format!("- **{}**: {}\n", data_item.source, data_item.values));
        }

        report.push_str("\n## 分析结果\n\n");
        if let Some(ref intent) = agent.parsed_intent {
            report.push_str(&format!("意图类型: {:?}\n", intent.intent_type));
            report.push_str(&format!("目标指标: {:?}\n", intent.target_metrics));
        }

        report.push_str("\n## 结论\n\n");
        report.push_str(&agent.user_prompt);

        Ok(report)
    }

    /// Update agent memory with learnings from this execution.
    /// Uses hierarchical memory architecture (MemGPT/Letta style):
    /// - Working Memory: Current execution (cleared after each execution)
    /// - Short-Term Memory: Recent summaries (auto-archived when full)
    /// - Long-Term Memory: Important patterns (retrieved by relevance)
    async fn update_memory(
        &self,
        agent: &AiAgent,
        data: &[DataCollected],
        decisions: &[Decision],
        situation_analysis: &str,
        conclusion: &str,
        execution_id: &str,
        success: bool,
    ) -> AgentResult<AgentMemory> {
        let mut memory = agent.memory.clone();

        // === HIERARCHICAL MEMORY UPDATE ===

        // 1. Update Working Memory with current analysis
        let cleaned_analysis = clean_and_truncate_text(situation_analysis, 500);
        let cleaned_conclusion = clean_and_truncate_text(conclusion, 200);
        memory.set_working_analysis(cleaned_analysis.clone(), cleaned_conclusion.clone());

        // 2. Prepare decision summaries for Short-Term Memory
        let decision_summaries: Vec<String> = decisions
            .iter()
            .filter(|d| !d.description.is_empty())
            .map(|d| clean_and_truncate_text(&d.description, 100))
            .collect();

        // Debug: log what we're about to save
        tracing::info!(
            agent_id = %agent.id,
            execution_id = %execution_id,
            analysis_len = cleaned_analysis.len(),
            conclusion_len = cleaned_conclusion.len(),
            decisions_count = decision_summaries.len(),
            "About to add to short_term memory"
        );

        memory.add_to_short_term(
            execution_id.to_string(),
            cleaned_analysis,
            cleaned_conclusion,
            decision_summaries,
            success,
        );

        // Debug: log what was added
        tracing::info!(
            agent_id = %agent.id,
            execution_id = %execution_id,
            short_term_count = memory.short_term.summaries.len(),
            "Short-term memory updated"
        );

        // 3. Add patterns to Long-Term Memory
        if !decisions.is_empty() {
            let semantic_patterns = extract_semantic_patterns(
                decisions,
                situation_analysis,
                data,
                &memory.baselines,
            );

            for pattern in semantic_patterns {
                memory.add_pattern(pattern);
            }
        }

        // === TREND AND BASELINE TRACKING ===
        let is_numeric_data = |data_type: &str| {
            !matches!(data_type, "device_info" | "state" | "info")
        };

        for data_item in data {
            if !is_numeric_data(&data_item.data_type) {
                continue;
            }

            if let Some(value) = data_item.values.get("value")
                && let Some(num) = value.as_f64() {
                    // Add to trend data (limit to 1000 points)
                    memory.trend_data.push(TrendPoint {
                        timestamp: data_item.timestamp,
                        metric: data_item.source.clone(),
                        value: num,
                        context: Some(serde_json::json!(data_item.data_type)),
                    });

                    if memory.trend_data.len() > 1000 {
                        memory.trend_data = memory.trend_data.split_off(memory.trend_data.len() - 1000);
                    }

                    // Update baseline using exponential moving average
                    let baseline = memory.baselines.entry(data_item.source.clone()).or_insert(num);
                    *baseline = *baseline * 0.9 + num * 0.1;
                }
        }

        // === LEGACY STATE_VARIABLES (for backward compatibility) ===
        // Track execution count
        let execution_count = memory.state_variables
            .get("total_executions")
            .and_then(|v| v.as_i64())
            .unwrap_or(0) + 1;
        memory.state_variables.insert(
            "total_executions".to_string(),
            serde_json::json!(execution_count),
        );

        // Store metrics we've seen
        for data_item in data {
            if is_numeric_data(&data_item.data_type) {
                let metrics_seen = memory.state_variables
                    .entry("metrics_seen".to_string())
                    .or_insert(serde_json::json!([]));
                if let Some(arr) = metrics_seen.as_array_mut() {
                    let metric_ref = data_item.source.clone();
                    if !arr.iter().any(|v| v.as_str() == Some(&metric_ref)) {
                        arr.push(serde_json::json!(metric_ref));
                    }
                }
            }
        }

        memory.updated_at = chrono::Utc::now().timestamp();

        tracing::debug!(
            memory_usage = %memory.memory_usage_summary(),
            execution_id = %execution_id,
            success = success,
            "Agent memory updated (hierarchical)"
        );

        Ok(memory)
    }

    /// Build LLM messages with conversation history for context-aware execution.
    pub fn build_conversation_messages(
        &self,
        agent: &AiAgent,
        current_data: &[DataCollected],
        event_data: Option<serde_json::Value>,
    ) -> Vec<Message> {
        let mut messages = Vec::new();

        // 1. Generic system prompt with conversation context
        let role_prompt = "你是一个 NeoMind 智能物联网系统的自动化助手。根据用户的指令分析数据、做出决策并执行相应操作。";

        // Get current time context for temporal understanding
        let time_context = get_time_context();

        let system_prompt = format!(
            "{}\n\n{}\n\n## 你的任务\n{}\n\n{}",
            role_prompt,
            time_context,
            agent.user_prompt,
            CONVERSATION_CONTEXT_ZH
        );
        messages.push(Message::system(system_prompt));

        // 2. Add user messages as important context - these are the user's latest instructions
        // User messages take priority over initial configuration and historical patterns
        if !agent.user_messages.is_empty() {
            let user_msgs_text: Vec<String> = agent.user_messages
                .iter()
                .enumerate()
                .map(|(i, msg)| {
                    let timestamp_str = chrono::DateTime::from_timestamp(msg.timestamp, 0)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_else(|| "Unknown".to_string());
                    format!("{}. [{}] {}", i + 1, timestamp_str, msg.content)
                })
                .collect();

            messages.push(Message::system(format!(
                "## ⚠️ 用户最新指令 (必须严格遵循)\n\n\
                用户在运行期间发送了以下消息，这些消息包含对执行策略的更新。\
                **请务必将这些指令作为最高优先级，覆盖初始配置中的任何冲突规则：**\n\n\
                {}\n\n\
                请在分析当前情况时，严格按照上述用户指令进行决策。",
                user_msgs_text.join("\n")
            )));
        }

        // 3. Add conversation summary if available
        if let Some(ref summary) = agent.conversation_summary {
            messages.push(Message::system(format!(
                "## 历史对话摘要\n\n{}",
                summary
            )));
        }

        // 4. Add recent conversation turns as context with intelligent filtering
        // Use relevance scoring to select the most valuable conversation turns
        let context_window = agent.context_window_size;
        let current_trigger = if event_data.is_some() { "event" } else { "scheduled" };

        // Score all turns by relevance and select top N
        let mut scored_turns: Vec<_> = agent.conversation_history
            .iter()
            .map(|turn| {
                let score = score_turn_relevance(turn, current_data, current_trigger);
                (turn, score)
            })
            .collect();

        // Filter out turns with very low relevance (< 0.15) to save context space
        scored_turns.retain(|(_, score)| *score >= 0.15);

        // Sort by relevance score (descending) and take top N
        scored_turns.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let recent_turns: Vec<_> = scored_turns
            .into_iter()
            .take(context_window)
            .collect();

        if !recent_turns.is_empty() {
            messages.push(Message::system(format!(
                "## 之前的执行历史 (最近 {} 次)\n\n请参考以下历史记录，避免重复告警，追踪趋势变化。",
                recent_turns.len()
            )));

            // Add each turn as context (in reverse order since we sorted by relevance desc)
            // recent_turns is Vec<(&ConversationTurn, f64)>
            for (i, (turn, _score)) in recent_turns.iter().rev().enumerate() {
                let timestamp_str = chrono::DateTime::from_timestamp(turn.timestamp, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_else(|| "Unknown".to_string());

                let turn_context = format!(
                    "### 历史执行 #{} ({})\n触发方式: {}\n分析: {}\n结论: {}",
                    i + 1,
                    timestamp_str,
                    turn.trigger_type,
                    turn.output.situation_analysis,
                    turn.output.conclusion
                );

                messages.push(Message::system(turn_context));

                // Add decisions if any
                if !turn.output.decisions.is_empty() {
                    let decisions_summary: Vec<String> = turn.output.decisions
                        .iter()
                        .map(|d| format!("- {}", d.description))
                        .collect();
                    messages.push(Message::system(format!(
                        "历史决策:\n{}",
                        decisions_summary.join("\n")
                    )));
                }
            }

            messages.push(Message::system(
                "## 当前执行\n\n请参考上述历史，分析当前情况。特别注意：\n\
                - 与之前数据相比的变化趋势\n\
                - 之前报告的问题是否持续\n\
                - 避免重复相同的分析或决策".to_string()
            ));
        }

        // 5. Current execution data
        let data_text = if current_data.is_empty() {
            "无数据".to_string()
        } else {
            current_data
                .iter()
                .map(|d| format!("- {}: {}", d.source, d.data_type))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let current_input = format!(
            "## 当前数据\n\n数据来源:\n{}\n\n触发方式: {}\n\n请分析当前情况并做出决策。",
            data_text,
            if event_data.is_some() { "事件触发" } else { "定时/手动" }
        );

        messages.push(Message::user(current_input));

        messages
    }

    /// Create a conversation turn from execution results.
    /// Truncates long text to prevent unbounded memory growth.
    pub fn create_conversation_turn(
        &self,
        execution_id: String,
        trigger_type: String,
        input_data: Vec<DataCollected>,
        event_data: Option<serde_json::Value>,
        decision_process: &DecisionProcess,
        duration_ms: u64,
        success: bool,
    ) -> ConversationTurn {
        // Clean and truncate before storing in conversation history
        // Conversation history can have up to 20 entries, so we need to be conservative
        let clean_situation = clean_and_truncate_text(&decision_process.situation_analysis, 300);
        let clean_conclusion = clean_and_truncate_text(&decision_process.conclusion, 150);

        // Also truncate reasoning step descriptions
        let cleaned_steps: Vec<neomind_storage::ReasoningStep> = decision_process.reasoning_steps
            .iter()
            .map(|step| neomind_storage::ReasoningStep {
                description: clean_and_truncate_text(&step.description, 100),
                ..step.clone()
            })
            .collect();

        // Truncate decision descriptions
        let cleaned_decisions: Vec<neomind_storage::Decision> = decision_process.decisions
            .iter()
            .map(|dec| neomind_storage::Decision {
                description: clean_and_truncate_text(&dec.description, 100),
                rationale: clean_and_truncate_text(&dec.rationale, 100),
                expected_outcome: clean_and_truncate_text(&dec.expected_outcome, 100),
                ..dec.clone()
            })
            .collect();

        ConversationTurn {
            execution_id,
            timestamp: chrono::Utc::now().timestamp(),
            trigger_type,
            input: TurnInput {
                data_collected: input_data,
                event_data,
            },
            output: TurnOutput {
                situation_analysis: clean_situation,
                reasoning_steps: cleaned_steps,
                decisions: cleaned_decisions,
                conclusion: clean_conclusion,
            },
            duration_ms,
            success,
        }
    }
}

/// Calculate statistics from time series data points.
/// Returns None if no numeric values are found.
fn calculate_stats(points: &[neomind_storage::DataPoint]) -> Option<Stats> {
    let nums: Vec<f64> = points.iter()
        .filter_map(|p| p.value.as_f64())
        .collect();

    if nums.is_empty() {
        return None;
    }

    let min_val = nums.iter().fold(f64::INFINITY, |a, &b| a.min(b));
    let max_val = nums.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    let avg_val = nums.iter().sum::<f64>() / nums.len() as f64;

    Some(Stats {
        min: min_val,
        max: max_val,
        avg: avg_val,
        count: nums.len(),
    })
}

/// Statistics for numeric data.
struct Stats {
    min: f64,
    max: f64,
    avg: f64,
    count: usize,
}

/// Check if a metric value contains image data.
fn is_image_metric(metric_name: &str, value: &serde_json::Value) -> bool {
    // Check metric name for image-related keywords
    let name_indicates_image = metric_name.to_lowercase().contains("image")
        || metric_name.to_lowercase().contains("snapshot")
        || metric_name.to_lowercase().contains("photo")
        || metric_name.to_lowercase().contains("picture")
        || metric_name.to_lowercase().contains("camera")
        || metric_name.to_lowercase().contains("video")
        || metric_name.to_lowercase().contains("frame");

    if name_indicates_image {
        return true;
    }

    // Check value for URL or base64 data
    if let Some(s) = value.as_str() {
        // Check for URL
        if s.starts_with("http://") || s.starts_with("https://") {
            return true;
        }
        // Check for base64 image data
        if s.starts_with("data:image/") {
            return true;
        }
        // Check for common base64 prefixes without data URL scheme
        if s.len() > 100 && (s.contains("/9j/") || s.contains("iVBORw0KGgo")) {
            // /9j/ is JPEG magic number in base64
            // iVBORw0KGgo is PNG magic number in base64
            return true;
        }
        false
    } else if let Some(obj) = value.as_object() {
        // Check for image_url, url, base64, or data fields
        obj.contains_key("image_url")
            || obj.contains_key("url")
            || obj.contains_key("base64")
            || obj.contains_key("data")
            || obj.contains_key("image_data")
    } else {
        false
    }
}

/// Extract image data from a metric value.
/// Returns (url, base64_data, mime_type) - at most one will be Some.
fn extract_image_data(value: &serde_json::Value) -> (Option<String>, Option<String>, Option<String>) {
    if let Some(s) = value.as_str() {
        if s.starts_with("http://") || s.starts_with("https://") {
            (Some(s.to_string()), None, None)
        } else if s.starts_with("data:image/") {
            // Parse data URL: data:image/<mime>;base64,<data>
            if let Some(rest) = s.strip_prefix("data:image/") {
                let parts: Vec<&str> = rest.splitn(2, ';').collect();
                if parts.len() == 2 {
                    let mime_type = parts[0].to_string();
                    if let Some(data) = parts[1].strip_prefix("base64,") {
                        (None, Some(data.to_string()), Some(mime_type))
                    } else {
                        (None, Some(parts[1].to_string()), Some(mime_type))
                    }
                } else {
                    (None, Some(rest.to_string()), Some("image/jpeg".to_string()))
                }
            } else {
                (None, Some(s.to_string()), Some("image/jpeg".to_string()))
            }
        } else if s.len() > 100 && (s.contains("/9j/") || s.contains("iVBORw0KGgo")) {
            // Raw base64 image data
            let mime_type = if s.contains("iVBORw0KGgo") {
                "image/png"
            } else {
                "image/jpeg"
            };
            (None, Some(s.to_string()), Some(mime_type.to_string()))
        } else {
            (None, None, None)
        }
    } else if let Some(obj) = value.as_object() {
        // Try various field names
        if let Some(url) = obj.get("image_url").or(obj.get("url")).and_then(|v| v.as_str()) {
            return (Some(url.to_string()), None, None);
        }
        if let Some(base64) = obj.get("base64").or(obj.get("data")).or(obj.get("image_data")).and_then(|v| v.as_str()) {
            let mime = obj.get("mime_type").or(obj.get("type")).and_then(|v| v.as_str())
                .unwrap_or("image/jpeg");
            return (None, Some(base64.to_string()), Some(mime.to_string()));
        }
        (None, None, None)
    } else {
        (None, None, None)
    }
}

/// Helper function to extract metrics from text.
fn extract_metrics(text: &str) -> Vec<String> {
    let mut metrics = Vec::new();

    if text.contains("温度") {
        metrics.push("temperature".to_string());
    }
    if text.contains("湿度") {
        metrics.push("humidity".to_string());
    }
    if text.contains("能耗") || text.contains("功率") || text.contains("电量") {
        metrics.push("power".to_string());
    }
    if text.contains("光照") {
        metrics.push("illuminance".to_string());
    }
    if text.contains("气压") {
        metrics.push("pressure".to_string());
    }

    metrics
}

/// Helper function to extract conditions from text.
fn extract_conditions(text: &str) -> Vec<String> {
    let mut conditions = Vec::new();

    // Look for patterns like "大于30", "小于50", "超过", "低于"
    if (text.contains("大于") || text.contains("超过"))
        && let Some(start) = text.find("大于").or_else(|| text.find("超过")) {
            let end = start + 2;
            if end + 10 <= text.len() {
                conditions.push(text[start..end + 10].to_string());
            }
        }

    if (text.contains("小于") || text.contains("低于"))
        && let Some(start) = text.find("小于").or_else(|| text.find("低于")) {
            let end = start + 2;
            if end + 10 <= text.len() {
                conditions.push(text[start..end + 10].to_string());
            }
        }

    conditions
}

/// Helper function to extract actions from text.
fn extract_actions(text: &str) -> Vec<String> {
    let mut actions = Vec::new();

    if text.contains("报警") || text.contains("通知") {
        actions.push("send_alert".to_string());
    }
    if text.contains("开关") || text.contains("控制") {
        actions.push("send_command".to_string());
    }
    if text.contains("生成报告") {
        actions.push("generate_report".to_string());
    }

    actions
}

/// Helper function to extract threshold value from condition text.
fn extract_threshold(text: &str) -> Option<f64> {
    // Find numbers in the text
    let nums: Vec<f64> = text
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();

    nums.first().copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_metrics() {
        let text = "监控温度和湿度，如果温度大于30度就报警";
        let metrics = extract_metrics(text);
        assert!(metrics.contains(&"temperature".to_string()));
        assert!(metrics.contains(&"humidity".to_string()));
    }

    #[test]
    fn test_extract_threshold() {
        assert_eq!(extract_threshold("大于30"), Some(30.0));
        assert_eq!(extract_threshold("温度超过35.5"), Some(35.5));
    }
}
