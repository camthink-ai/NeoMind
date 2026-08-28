//! Comprehensive Agent Evaluation — 20 rounds × 15+ turns
//!
//! Evaluates:
//!   1. Tool System    — tool call accuracy, multi-tool, error recovery
//!   2. Memory System  — extraction, retention, cross-turn recall
//!   3. Context System — conversation continuity, long-context handling
//!   4. Task Completion — single-turn / multi-turn / complex resource creation
//!
//! Run:
//!   cargo test -p neomind-agent --test comprehensive_agent_eval -- --ignored --nocapture

use std::sync::Arc;
use std::time::Instant;

use neomind_agent::llm_backends::{CloudConfig, CloudRuntime, OllamaConfig, OllamaRuntime};
use neomind_agent::session::SessionManager;
use neomind_core::llm::backend::LlmRuntime;

// ── helpers ───────────────────────────────────────────────────────────

fn ollama_up() -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 11434));
    std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(2)).is_ok()
}

async fn new_session() -> (SessionManager, String) {
    let sm = SessionManager::memory();
    let sid = sm.create_session().await.unwrap();

    let llm: Arc<dyn LlmRuntime> = if let Ok(api_key) = std::env::var("LLM_API_KEY") {
        // Cloud LLM mode (GLM-5, OpenAI, etc.)
        let endpoint = std::env::var("LLM_ENDPOINT")
            .unwrap_or("https://open.bigmodel.cn/api/coding/paas/v4".into());
        let model = std::env::var("MODEL").unwrap_or("glm-5".into());
        Arc::new(
            CloudRuntime::new(
                CloudConfig::custom(api_key, endpoint)
                    .with_model(model)
                    .with_timeout_secs(180),
            )
            .unwrap(),
        )
    } else {
        // Local Ollama mode
        let model = std::env::var("MODEL").unwrap_or("qwen3.5:2b".into());
        let endpoint = std::env::var("OLLAMA_ENDPOINT").unwrap_or("http://localhost:11434".into());
        Arc::new(
            OllamaRuntime::new(OllamaConfig {
                endpoint,
                model,
                timeout_secs: 180,
            })
            .unwrap(),
        )
    };
    sm.get_session(&sid)
        .await
        .unwrap()
        .set_custom_llm(llm)
        .await;
    (sm, sid)
}

async fn send(sm: &SessionManager, sid: &str, msg: &str) -> MsgResult {
    let start = Instant::now();
    let resp = sm.process_message(sid, msg).await.unwrap();
    let elapsed = start.elapsed();
    // Extract CLI commands from shell tool calls
    let shell_commands: Vec<String> = resp
        .tool_calls
        .iter()
        .filter(|t| t.name == "shell")
        .filter_map(|t| t.arguments.get("command").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
        .collect();

    MsgResult {
        content: resp.message.content.to_string(),
        tool_calls: resp.tool_calls.iter().map(|t| t.name.clone()).collect(),
        shell_commands,
        tool_results: resp
            .tool_calls
            .iter()
            .filter_map(|t| t.result.clone())
            .map(|v| v.to_string())
            .collect(),
        tools_used: resp.tools_used.clone(),
        memory_used: resp.memory_context_used,
        processing_ms: resp.processing_time_ms,
        elapsed_ms: elapsed.as_millis() as u64,
    }
}

#[allow(dead_code)]
struct MsgResult {
    content: String,
    tool_calls: Vec<String>,
    /// All shell commands executed (e.g. ["neomind device list"])
    shell_commands: Vec<String>,
    tool_results: Vec<String>,
    tools_used: Vec<String>,
    memory_used: bool,
    processing_ms: u64,
    elapsed_ms: u64,
}

impl MsgResult {
    /// Check if any shell command targets the given CLI domain.
    /// e.g. has_domain("device") matches "neomind device list", "neomind device get ..."
    fn has_domain(&self, domain: &str) -> bool {
        let prefix = format!("neomind {} ", domain);
        let exact = format!("neomind {}", domain);
        self.shell_commands
            .iter()
            .any(|c| c.starts_with(&prefix) || *c == exact)
    }

    /// Count how many shell commands target the given CLI domain.
    fn domain_count(&self, domain: &str) -> usize {
        let prefix = format!("neomind {} ", domain);
        let exact = format!("neomind {}", domain);
        self.shell_commands
            .iter()
            .filter(|c| c.starts_with(&prefix) || **c == exact)
            .count()
    }
}

// ── metrics ───────────────────────────────────────────────────────────

#[derive(Default, Debug)]
struct Metrics {
    total_turns: usize,
    total_rounds: usize,

    // Tool system
    turns_with_tools: usize,
    tools_correct: usize,
    tools_total_expected: usize,
    multi_tool_attempts: usize,
    multi_tool_success: usize,

    // Memory system
    memory_recalled_turns: usize, // turns where memory_context_used=true
    memory_recall_queries: usize, // explicit recall queries
    memory_recall_success: usize, // recall produced correct info

    // Context system
    context_followup_total: usize,
    context_followup_success: usize,

    // Task completion
    single_turn_tasks: usize,
    single_turn_success: usize,
    multi_turn_tasks: usize,
    multi_turn_success: usize,
    resource_creation_tasks: usize,
    resource_creation_success: usize,

    // Performance
    total_elapsed_ms: u64,
}

impl Metrics {
    fn tool_accuracy(&self) -> f64 {
        if self.tools_total_expected == 0 {
            0.0
        } else {
            self.tools_correct as f64 / self.tools_total_expected as f64 * 100.0
        }
    }
    fn single_turn_rate(&self) -> f64 {
        if self.single_turn_tasks == 0 {
            0.0
        } else {
            self.single_turn_success as f64 / self.single_turn_tasks as f64 * 100.0
        }
    }
    fn multi_turn_rate(&self) -> f64 {
        if self.multi_turn_tasks == 0 {
            0.0
        } else {
            self.multi_turn_success as f64 / self.multi_turn_tasks as f64 * 100.0
        }
    }
    fn resource_creation_rate(&self) -> f64 {
        if self.resource_creation_tasks == 0 {
            0.0
        } else {
            self.resource_creation_success as f64 / self.resource_creation_tasks as f64 * 100.0
        }
    }
    fn context_continuity(&self) -> f64 {
        if self.context_followup_total == 0 {
            0.0
        } else {
            self.context_followup_success as f64 / self.context_followup_total as f64 * 100.0
        }
    }
    fn memory_recall_rate(&self) -> f64 {
        if self.memory_recall_queries == 0 {
            0.0
        } else {
            self.memory_recall_success as f64 / self.memory_recall_queries as f64 * 100.0
        }
    }
    fn avg_latency(&self) -> u64 {
        if self.total_turns == 0 {
            0
        } else {
            self.total_elapsed_ms / (self.total_turns as u64)
        }
    }
}

// ── Test scenarios ────────────────────────────────────────────────────
//
// Each scenario = one "round" of 15+ turns
// Returns (metrics_for_this_round, per_turn_notes)

// R1 — Device management: list, query, control, history
async fn r1_device_management(sm: &SessionManager, sid: &str, m: &mut Metrics) -> Vec<String> {
    let mut notes = Vec::new();

    // 1 — list devices (single-turn tool)
    let r = send(sm, sid, "列出所有设备").await;
    m.total_turns += 1;
    if r.has_domain("device") {
        m.tools_correct += 1;
    }
    m.tools_total_expected += 1;
    m.turns_with_tools += if r.shell_commands.is_empty() { 0 } else { 1 };
    m.single_turn_tasks += 1;
    if r.has_domain("device") {
        m.single_turn_success += 1;
        notes.push("T1: ✅ list devices".to_string());
    } else {
        notes.push("T1: ❌ list devices".to_string());
    }

    // 2 — query specific device
    let r = send(sm, sid, "查看设备 sensor_01 的最新数据").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    if r.has_domain("device") {
        m.tools_correct += 1;
        m.single_turn_success += 1;
        notes.push("T2: ✅ device data".to_string());
    } else {
        notes.push("T2: ❌ device data".to_string());
    }
    m.single_turn_tasks += 1;

    // 3 — history trend (single-turn complex)
    let r = send(sm, sid, "查看设备 sensor_01 过去24小时的温度变化趋势").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    if r.has_domain("device") {
        m.tools_correct += 1;
        m.single_turn_success += 1;
        notes.push("T3: ✅ history trend".to_string());
    } else {
        notes.push("T3: ❌ history trend".to_string());
    }
    m.single_turn_tasks += 1;

    // 4 — context follow-up (refer to previous)
    let r = send(sm, sid, "刚才那个设备的电池电量呢？").await;
    m.total_turns += 1;
    m.context_followup_total += 1;
    let refers_prev = r.content.contains("sensor_01")
        || r.content.contains("电池")
        || r.content.contains("battery");
    if refers_prev {
        m.context_followup_success += 1;
        notes.push("T4: ✅ context follow-up".to_string());
    } else {
        notes.push("T4: ❌ context follow-up".to_string());
    }

    // 5 — device control (single-turn)
    let r = send(sm, sid, "打开设备 light_living").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    m.single_turn_tasks += 1;
    if r.has_domain("device") {
        m.tools_correct += 1;
        m.single_turn_success += 1;
        notes.push("T5: ✅ device control".to_string());
    } else {
        notes.push("T5: ❌ device control".to_string());
    }

    // 6 — multi-device query
    let r = send(sm, sid, "同时查看 sensor_01 和 sensor_02 的数据").await;
    m.total_turns += 1;
    m.tools_total_expected += 2;
    if r.domain_count("device") >= 2 {
        m.tools_correct += 2;
        m.multi_tool_success += 1;
        notes.push("T6: ✅ multi-device".to_string());
    } else if r.has_domain("device") {
        m.tools_correct += 1;
        notes.push("T6: ⚠️ partial multi-device".to_string());
    } else {
        notes.push("T6: ❌ multi-device".to_string());
    }
    m.multi_tool_attempts += 1;

    // 7 — device analysis
    let r = send(sm, sid, "分析所有设备的在线离线状态").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    if r.has_domain("device") {
        m.tools_correct += 1;
        notes.push("T7: ✅ device analysis".to_string());
    } else {
        notes.push("T7: ❌ device analysis".to_string());
    }

    // 8 — context: refer to turn 5
    let r = send(sm, sid, "我刚才打开了什么设备？").await;
    m.total_turns += 1;
    m.context_followup_total += 1;
    if r.content.contains("light_living") || r.content.contains("灯") || r.content.contains("客厅")
    {
        m.context_followup_success += 1;
        notes.push("T8: ✅ context recall (turn 5)".to_string());
    } else {
        notes.push("T8: ❌ context recall".to_string());
    }

    // 9 — error recovery: invalid device
    let r = send(sm, sid, "查看设备 nonexist999 的数据").await;
    m.total_turns += 1;
    let graceful =
        !r.content.is_empty() && !r.content.contains("panic") && !r.content.contains("error");
    if graceful {
        notes.push("T9: ✅ graceful error handling".to_string());
    } else {
        notes.push("T9: ⚠️ error handling".to_string());
    }

    // 10 — natural language device query
    let r = send(sm, sid, "我办公室的温度是多少？").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    if r.has_domain("device") {
        m.tools_correct += 1;
        notes.push("T10: ✅ NL device query".to_string());
    } else {
        notes.push("T10: ❌ NL device query".to_string());
    }

    // 11-15: extended device interactions
    let r = send(sm, sid, "列出所有离线设备").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    if r.has_domain("device") {
        m.tools_correct += 1;
        notes.push("T11: ✅ offline filter".to_string());
    } else {
        notes.push("T11: ❌ offline filter".to_string());
    }

    let r = send(sm, sid, "关闭设备 light_living").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    m.single_turn_tasks += 1;
    if r.has_domain("device") {
        m.tools_correct += 1;
        m.single_turn_success += 1;
        notes.push("T12: ✅ turn off".to_string());
    } else {
        notes.push("T12: ❌ turn off".to_string());
    }

    let r = send(sm, sid, "sensor_01 的信号强度怎么样").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    if r.has_domain("device") {
        m.tools_correct += 1;
        notes.push("T13: ✅ signal query".to_string());
    } else {
        notes.push("T13: ❌ signal query".to_string());
    }

    let r = send(sm, sid, "帮我对比一下 sensor_01 和 sensor_02 的温度数据").await;
    m.total_turns += 1;
    m.tools_total_expected += 2;
    m.multi_tool_attempts += 1;
    if r.domain_count("device") >= 2 {
        m.tools_correct += 2;
        m.multi_tool_success += 1;
        notes.push("T14: ✅ compare devices".to_string());
    } else if r.has_domain("device") {
        m.tools_correct += 1;
        notes.push("T14: ⚠️ partial compare".to_string());
    } else {
        notes.push("T14: ❌ compare devices".to_string());
    }

    let r = send(sm, sid, "今天设备有什么异常吗？").await;
    m.total_turns += 1;
    notes.push(format!(
        "T15: {} device anomaly check",
        if r.has_domain("device") {
            "✅"
        } else {
            "⚠️"
        }
    ));

    notes
}

// R2 — Rule management: list, create, delete, complex DSL
async fn r2_rule_management(sm: &SessionManager, sid: &str, m: &mut Metrics) -> Vec<String> {
    let mut notes = Vec::new();

    // 1 — list rules
    let r = send(sm, sid, "列出所有自动化规则").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    m.single_turn_tasks += 1;
    if r.has_domain("rule") {
        m.tools_correct += 1;
        m.single_turn_success += 1;
        notes.push("T1: ✅ list rules".to_string());
    } else {
        notes.push("T1: ❌ list rules".to_string());
    }

    // 2 — create temp rule (resource creation)
    let r = send(sm, sid, "创建一个规则：当温度超过35度时发送告警通知").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    m.resource_creation_tasks += 1;
    if r.has_domain("rule") {
        m.tools_correct += 1;
        let has_dsl =
            r.content.contains("RULE") || r.content.contains("温度") || r.content.contains("35");
        if has_dsl {
            m.resource_creation_success += 1;
            notes.push("T2: ✅ create temp rule".to_string());
        } else {
            notes.push("T2: ⚠️ rule created but content uncertain".to_string());
        }
    } else {
        notes.push("T2: ❌ create temp rule".to_string());
    }

    // 3 — create battery rule
    let r = send(sm, sid, "创建规则：电池电量低于20%时发通知").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    m.resource_creation_tasks += 1;
    if r.has_domain("rule") {
        m.tools_correct += 1;
        m.resource_creation_success += 1;
        notes.push("T3: ✅ create battery rule".to_string());
    } else {
        notes.push("T3: ❌ create battery rule".to_string());
    }

    // 4 — context: refer to rule just created
    let r = send(sm, sid, "刚才我创建了什么规则？").await;
    m.total_turns += 1;
    m.context_followup_total += 1;
    if r.content.contains("温度")
        || r.content.contains("35")
        || r.content.contains("电池")
        || r.content.contains("20")
    {
        m.context_followup_success += 1;
        notes.push("T4: ✅ context recall rules".to_string());
    } else {
        notes.push("T4: ❌ context recall rules".to_string());
    }

    // 5 — create complex multi-condition rule (complex resource)
    let r = send(
        sm,
        sid,
        "创建规则：当温度超过30度并且湿度低于40%时，自动打开喷淋系统，并用微信通知我",
    )
    .await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    m.resource_creation_tasks += 1;
    if r.has_domain("rule") {
        m.tools_correct += 1;
        let complex = r.content.contains("30")
            && (r.content.contains("40")
                || r.content.contains("喷淋")
                || r.content.contains("微信"));
        if complex {
            m.resource_creation_success += 1;
            notes.push("T5: ✅ complex rule".to_string());
        } else {
            notes.push("T5: ⚠️ complex rule (partial)".to_string());
        }
    } else {
        notes.push("T5: ❌ complex rule".to_string());
    }

    // 6 — list rules again to verify
    let r = send(sm, sid, "现在有多少条规则？").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    if r.has_domain("rule") {
        m.tools_correct += 1;
        notes.push("T6: ✅ count rules".to_string());
    } else {
        notes.push("T6: ❌ count rules".to_string());
    }

    // 7 — delete a rule
    let r = send(sm, sid, "删除温度告警规则").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    if r.has_domain("rule") {
        m.tools_correct += 1;
        notes.push("T7: ✅ delete rule".to_string());
    } else {
        notes.push("T7: ❌ delete rule".to_string());
    }

    // 8 — try invalid rule
    let r = send(sm, sid, "创建一个规则（不提供任何条件）").await;
    m.total_turns += 1;
    let handled = !r.content.is_empty();
    notes.push(format!(
        "T8: {} invalid rule handling",
        if handled { "✅" } else { "❌" }
    ));

    // 9 — disable rule
    let r = send(sm, sid, "禁用电池电量告警规则").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    if r.has_domain("rule") {
        m.tools_correct += 1;
        notes.push("T9: ✅ disable rule".to_string());
    } else {
        notes.push("T9: ❌ disable rule".to_string());
    }

    // 10 — context followup
    let r = send(sm, sid, "我一共创建了几条规则？当前还有几条？").await;
    m.total_turns += 1;
    m.context_followup_total += 1;
    let mentions_count = r.content.contains("1")
        || r.content.contains("2")
        || r.content.contains("3")
        || r.content.contains("条");
    if mentions_count {
        m.context_followup_success += 1;
        notes.push("T10: ✅ rule count context".to_string());
    } else {
        notes.push("T10: ❌ rule count context".to_string());
    }

    let rule_queries: Vec<&str> = vec![
        "创建规则：当设备离线超过10分钟时发送紧急通知",
        "列出所有被禁用的规则",
        "创建规则：每天早上8点自动检查所有设备状态",
        "把温度告警规则的阈值改成38度",
        "删除所有规则",
    ];
    for (i, q) in rule_queries.iter().enumerate() {
        let r = send(sm, sid, q).await;
        m.total_turns += 1;
        if i == 0 || i == 2 {
            m.resource_creation_tasks += 1;
        }
        if r.has_domain("rule") {
            m.tools_correct += 1;
            m.tools_total_expected += 1;
            if i == 0 || i == 2 {
                m.resource_creation_success += 1;
            }
            notes.push(format!("T{}: ✅ {}", 11 + i, q));
        } else {
            notes.push(format!("T{}: ❌ {}", 11 + i, q));
        }
    }

    notes
}

// R3 — Agent management: list, create, control, executions
async fn r3_agent_management(sm: &SessionManager, sid: &str, m: &mut Metrics) -> Vec<String> {
    let mut notes = Vec::new();

    // 1 — list agents
    let r = send(sm, sid, "列出所有AI Agent").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    m.single_turn_tasks += 1;
    if r.has_domain("agent") {
        m.tools_correct += 1;
        m.single_turn_success += 1;
        notes.push("T1: ✅ list agents".to_string());
    } else {
        notes.push("T1: ❌ list agents".to_string());
    }

    // 2 — create agent (resource creation)
    let r = send(
        sm,
        sid,
        "创建一个Agent叫温度巡检，每5分钟执行一次，检查所有温度传感器",
    )
    .await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    m.resource_creation_tasks += 1;
    if r.has_domain("agent") {
        m.tools_correct += 1;
        let has_name = r.content.contains("温度") || r.content.contains("巡检");
        if has_name {
            m.resource_creation_success += 1;
            notes.push("T2: ✅ create temp agent".to_string());
        } else {
            notes.push("T2: ⚠️ agent create (uncertain)".to_string());
        }
    } else {
        notes.push("T2: ❌ create temp agent".to_string());
    }

    // 3 — create another agent
    let r = send(
        sm,
        sid,
        "创建Agent：电池监控，每天8点执行，检查电池电量并通知",
    )
    .await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    m.resource_creation_tasks += 1;
    if r.has_domain("agent") {
        m.tools_correct += 1;
        m.resource_creation_success += 1;
        notes.push("T3: ✅ create battery agent".to_string());
    } else {
        notes.push("T3: ❌ create battery agent".to_string());
    }

    // 4 — get agent detail
    let r = send(sm, sid, "查看温度巡检Agent的详细信息").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    if r.has_domain("agent") {
        m.tools_correct += 1;
        notes.push("T4: ✅ agent detail".to_string());
    } else {
        notes.push("T4: ❌ agent detail".to_string());
    }

    // 5 — context followup
    let r = send(sm, sid, "我刚才创建的第二个Agent是什么？").await;
    m.total_turns += 1;
    m.context_followup_total += 1;
    if r.content.contains("电池") || r.content.contains("battery") {
        m.context_followup_success += 1;
        notes.push("T5: ✅ context agent recall".to_string());
    } else {
        notes.push("T5: ❌ context agent recall".to_string());
    }

    // 6 — control agent (pause)
    let r = send(sm, sid, "暂停温度巡检Agent").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    m.single_turn_tasks += 1;
    if r.has_domain("agent") {
        m.tools_correct += 1;
        m.single_turn_success += 1;
        notes.push("T6: ✅ pause agent".to_string());
    } else {
        notes.push("T6: ❌ pause agent".to_string());
    }

    // 7 — get executions
    let r = send(sm, sid, "查看温度巡检Agent的执行历史").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    if r.has_domain("agent") {
        m.tools_correct += 1;
        notes.push("T7: ✅ agent executions".to_string());
    } else {
        notes.push("T7: ❌ agent executions".to_string());
    }

    // 8 — resume
    let r = send(sm, sid, "恢复温度巡检Agent").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    if r.has_domain("agent") {
        m.tools_correct += 1;
        notes.push("T8: ✅ resume agent".to_string());
    } else {
        notes.push("T8: ❌ resume agent".to_string());
    }

    // 9 — create complex agent with tool chaining
    let r = send(
        sm,
        sid,
        "创建一个智能运维Agent，每10分钟执行，启用工具链，检查设备状态，发现异常自动创建规则告警",
    )
    .await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    m.resource_creation_tasks += 1;
    if r.has_domain("agent") {
        m.tools_correct += 1;
        m.resource_creation_success += 1;
        notes.push("T9: ✅ complex agent create".to_string());
    } else {
        notes.push("T9: ❌ complex agent create".to_string());
    }

    // 10-15
    for (i, q) in [
        "列出所有Agent及其运行状态",
        "删除电池监控Agent",
        "查看智能运维Agent的详细信息",
        "我目前有几个正在运行的Agent？",
        "修改温度巡检Agent的执行间隔为3分钟",
        "删除所有Agent",
    ]
    .iter()
    .enumerate()
    {
        let r = send(sm, sid, q).await;
        m.total_turns += 1;
        let needs_tool = i != 3;
        if needs_tool {
            m.tools_total_expected += 1;
        }
        if r.has_domain("agent") {
            if needs_tool {
                m.tools_correct += 1;
            }
            notes.push(format!("T{}: ✅ {}", 10 + i, q));
        } else {
            notes.push(format!("T{}: ❌ {}", 10 + i, q));
        }
        if i == 3 {
            m.context_followup_total += 1;
            if r.content.contains("个") || r.content.contains("运行") {
                m.context_followup_success += 1;
            }
        }
    }

    notes
}

// R4 — Cross-domain: device + rule + agent in same conversation
async fn r4_cross_domain(sm: &SessionManager, sid: &str, m: &mut Metrics) -> Vec<String> {
    let mut notes = Vec::new();

    // Multi-turn workflow: check devices → create rule → create agent
    // 1
    let r = send(sm, sid, "帮我查看所有设备的状态").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    m.multi_turn_tasks += 1;
    if r.has_domain("device") {
        m.tools_correct += 1;
        notes.push("T1: ✅ check devices".to_string());
    } else {
        notes.push("T1: ❌ check devices".to_string());
    }

    // 2
    let r = send(sm, sid, "有没有温度超过30度的设备？").await;
    m.total_turns += 1;
    notes.push(format!(
        "T2: {} temp check",
        if r.has_domain("device") {
            "✅"
        } else {
            "⚠️"
        }
    ));
    if r.has_domain("device") {
        m.tools_correct += 1;
        m.tools_total_expected += 1;
    }

    // 3 — create rule based on context
    let r = send(sm, sid, "给温度超标的设备创建一个告警规则").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    m.resource_creation_tasks += 1;
    if r.has_domain("rule") {
        m.tools_correct += 1;
        m.resource_creation_success += 1;
        notes.push("T3: ✅ cross-domain: device→rule".to_string());
    } else {
        notes.push("T3: ❌ cross-domain: device→rule".to_string());
    }

    // 4 — create agent to monitor
    let r = send(sm, sid, "再创建一个Agent来定期执行这个规则").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    m.resource_creation_tasks += 1;
    if r.has_domain("agent") {
        m.tools_correct += 1;
        m.resource_creation_success += 1;
        notes.push("T4: ✅ cross-domain: rule→agent".to_string());
    } else {
        notes.push("T4: ❌ cross-domain: rule→agent".to_string());
    }

    // 5 — verify context across domains
    let r = send(sm, sid, "总结一下我刚才做了什么操作？").await;
    m.total_turns += 1;
    m.context_followup_total += 1;
    let mentions = (r.content.contains("设备") || r.content.contains("device"))
        && (r.content.contains("规则") || r.content.contains("rule") || r.content.contains("告警"))
        && (r.content.contains("Agent") || r.content.contains("agent"));
    if mentions {
        m.context_followup_success += 1;
        notes.push("T5: ✅ cross-domain summary".to_string());
    } else {
        notes.push("T5: ❌ cross-domain summary".to_string());
    }

    // multi-turn workflow considered successful if >=3 tool calls hit
    let workflow_tools = notes.iter().filter(|n| n.contains("✅")).count();
    if workflow_tools >= 3 {
        m.multi_turn_success += 1;
    }

    // 6-15: mixed queries
    let mixed: Vec<(&str, Option<&str>)> = vec![
        ("查看所有设备的电池状态", Some("device")),
        ("创建规则：电池低于15%时通知", Some("rule")),
        ("列出当前所有规则", Some("rule")),
        ("那个电池规则创建好了吗？", None),
        ("创建Agent每天检查一次电池", Some("agent")),
        ("同时列出所有设备和所有规则", Some("multi")),
        ("对比温度数据和规则数量", None),
        ("暂停刚创建的电池检查Agent", Some("agent")),
        ("查看所有Agent的状态", Some("agent")),
        ("清点一下：我有多少设备、多少规则、多少Agent", None),
    ];

    for (i, (q, expected)) in mixed.iter().enumerate() {
        let r = send(sm, sid, q).await;
        m.total_turns += 1;
        match expected {
            Some("multi") => {
                m.tools_total_expected += 2;
                m.multi_tool_attempts += 1;
                let d = r.has_domain("device");
                let ru = r.has_domain("rule");
                if d {
                    m.tools_correct += 1;
                }
                if ru {
                    m.tools_correct += 1;
                }
                if d && ru {
                    m.multi_tool_success += 1;
                }
                notes.push(format!(
                    "T{}: {} multi-tool (d={}, r={})",
                    6 + i,
                    if d && ru { "OK" } else { "PARTIAL" },
                    d,
                    ru
                ));
            }
            Some(tool) => {
                m.tools_total_expected += 1;
                if r.has_domain(tool) {
                    m.tools_correct += 1;
                    notes.push(format!("T{}: OK {}", 6 + i, q));
                } else {
                    notes.push(format!("T{}: FAIL {}", 6 + i, q));
                }
            }
            None => {
                m.context_followup_total += 1;
                let ok = !r.content.is_empty() && r.content.len() > 20;
                if ok {
                    m.context_followup_success += 1;
                    notes.push(format!("T{}: OK {}", 6 + i, q));
                } else {
                    notes.push(format!("T{}: PARTIAL {}", 6 + i, q));
                }
            }
        }
    }

    notes
}

// R5 — Memory & context stress test
async fn r5_memory_context_stress(sm: &SessionManager, sid: &str, m: &mut Metrics) -> Vec<String> {
    let mut notes = Vec::new();

    // Plant information across turns
    let r = send(sm, sid, "你好，我叫张三，我在上海仓库工作").await;
    m.total_turns += 1;
    notes.push(format!(
        "T1: {} intro",
        if r.content.contains("张三") || r.content.contains("你好") {
            "✅"
        } else {
            "⚠️"
        }
    ));

    let _r = send(sm, sid, "我们仓库有50个温湿度传感器，10个摄像头").await;
    m.total_turns += 1;
    notes.push("T2: device info planted".to_string());

    let _r = send(sm, sid, "告警阈值：温度超过32度，湿度超过80%").await;
    m.total_turns += 1;
    notes.push("T3: ✅ thresholds planted".into());

    let _r = send(sm, sid, "通知方式用短信，紧急情况打电话").await;
    m.total_turns += 1;
    notes.push("T4: ✅ notification prefs planted".into());

    // Now test recall
    let recall_queries = [
        ("我叫什么名字？", vec!["张三"]),
        ("我在哪里工作？", vec!["上海", "仓库"]),
        ("我们有多少个传感器？", vec!["50"]),
        ("温度告警阈值是多少？", vec!["32"]),
        ("紧急情况怎么联系我？", vec!["电话", "打电话"]),
        ("我之前说的通知方式是什么？", vec!["短信"]),
        ("我们仓库有摄像头吗？有几个？", vec!["10", "摄像头"]),
        ("帮我总结一下我告诉你的所有信息", vec!["张三", "上海"]),
        ("根据我的要求创建一个温度告警规则", vec!["32"]), // should use 32 from memory
        ("创建一个湿度监控Agent", vec!["80"]),            // should use 80 from memory
    ];

    for (i, (q, keywords)) in recall_queries.iter().enumerate() {
        let r = send(sm, sid, q).await;
        m.total_turns += 1;
        m.memory_recall_queries += 1;

        let hit = keywords.iter().any(|kw| r.content.contains(kw));
        if hit {
            m.memory_recall_success += 1;
            notes.push(format!("T{}: ✅ recall: {}", 5 + i, q));
        } else {
            notes.push(format!(
                "T{}: ❌ recall: {} (got: {:?})",
                5 + i,
                q,
                r.content.chars().take(80).collect::<String>()
            ));
        }

        if i >= 8 {
            m.resource_creation_tasks += 1;
            if !r.shell_commands.is_empty() {
                m.resource_creation_success += 1;
            }
        }
    }

    // Additional turns to reach 15
    let r = send(sm, sid, "根据我之前设定的阈值，再创建一个湿度告警规则").await;
    m.total_turns += 1;
    m.resource_creation_tasks += 1;
    if r.has_domain("rule") {
        m.resource_creation_success += 1;
        notes.push("T15: ✅ rule from memory".to_string());
    } else {
        notes.push("T15: ❌ rule from memory".to_string());
    }

    notes
}

// R6-20: Reuse R1-R5 patterns with variations
// Use a simple match dispatch in the main test

async fn run_scenario(
    round: usize,
    sm: &SessionManager,
    sid: &str,
    m: &mut Metrics,
) -> Vec<String> {
    match round {
        0 | 5 | 10 | 15 => r1_device_management(sm, sid, m).await,
        1 | 6 | 11 | 16 => r2_rule_management(sm, sid, m).await,
        2 | 7 | 12 | 17 => r3_agent_management(sm, sid, m).await,
        3 | 8 | 13 | 18 => r4_cross_domain(sm, sid, m).await,
        4 | 9 | 14 | 19 => r5_memory_context_stress(sm, sid, m).await,
        _ => unreachable!(),
    }
}

static SCENARIO_NAMES: &[&str] = &[
    "R01-设备管理",
    "R02-规则管理",
    "R03-Agent管理",
    "R04-跨域综合",
    "R05-记忆上下文",
    "R06-设备管理v2",
    "R07-规则管理v2",
    "R08-Agent管理v2",
    "R09-跨域综合v2",
    "R10-记忆上下文v2",
    "R11-设备管理v3",
    "R12-规则管理v3",
    "R13-Agent管理v3",
    "R14-跨域综合v3",
    "R15-记忆上下文v3",
    "R16-设备管理v4",
    "R17-规则管理v4",
    "R18-Agent管理v4",
    "R19-跨域综合v4",
    "R20-记忆上下文v4",
];

// ── Main test ─────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "Requires Ollama. cargo test -p neomind-agent --test comprehensive_agent_eval -- --ignored --nocapture"]
async fn comprehensive_20round_evaluation() -> anyhow::Result<()> {
    if !ollama_up() && std::env::var("LLM_API_KEY").is_err() {
        eprintln!("Neither Ollama nor LLM_API_KEY available, skipping");
        return Ok(());
    }

    let model = std::env::var("MODEL").unwrap_or("qwen3.5:2b".into());
    println!("\n{}", "═".repeat(70));
    println!("COMPREHENSIVE AGENT EVALUATION — 20 Rounds x 15+ Turns");
    println!("Model: {}", model);
    println!("{}\n", "═".repeat(70));

    let mut total_metrics = Metrics::default();
    let total_start = Instant::now();

    for (idx, &name) in SCENARIO_NAMES.iter().enumerate() {
        println!("\n{}", "─".repeat(60));
        println!("Round {}/20: {}", idx + 1, name);
        println!("{}", "─".repeat(60));

        let (sm, sid) = new_session().await;
        let mut round_metrics = Metrics {
            total_rounds: 1,
            ..Default::default()
        };

        let notes = run_scenario(idx, &sm, &sid, &mut round_metrics).await;

        // Print round summary
        for note in &notes {
            println!("  {}", note);
        }
        let round_turns = round_metrics.total_turns;
        let round_tool_acc = round_metrics.tool_accuracy();
        println!(
            "\n  Round {} Summary: {} turns, tool accuracy {:.0}%",
            idx + 1,
            round_turns,
            round_tool_acc
        );

        // Accumulate
        total_metrics.total_rounds += 1;
        total_metrics.total_turns += round_metrics.total_turns;
        total_metrics.turns_with_tools += round_metrics.turns_with_tools;
        total_metrics.tools_correct += round_metrics.tools_correct;
        total_metrics.tools_total_expected += round_metrics.tools_total_expected;
        total_metrics.multi_tool_attempts += round_metrics.multi_tool_attempts;
        total_metrics.multi_tool_success += round_metrics.multi_tool_success;
        total_metrics.memory_recalled_turns += round_metrics.memory_recalled_turns;
        total_metrics.memory_recall_queries += round_metrics.memory_recall_queries;
        total_metrics.memory_recall_success += round_metrics.memory_recall_success;
        total_metrics.context_followup_total += round_metrics.context_followup_total;
        total_metrics.context_followup_success += round_metrics.context_followup_success;
        total_metrics.single_turn_tasks += round_metrics.single_turn_tasks;
        total_metrics.single_turn_success += round_metrics.single_turn_success;
        total_metrics.multi_turn_tasks += round_metrics.multi_turn_tasks;
        total_metrics.multi_turn_success += round_metrics.multi_turn_success;
        total_metrics.resource_creation_tasks += round_metrics.resource_creation_tasks;
        total_metrics.resource_creation_success += round_metrics.resource_creation_success;
        total_metrics.total_elapsed_ms += round_metrics.total_elapsed_ms;
    }

    let total_elapsed = total_start.elapsed();

    // ── Final Report ──────────────────────────────────────────────────
    println!("\n\n{}", "═".repeat(70));
    println!("COMPREHENSIVE EVALUATION REPORT");
    println!("{}", "═".repeat(70));

    println!("\n[Scale]");
    println!("  Rounds:          {}", total_metrics.total_rounds);
    println!("  Total Turns:     {}", total_metrics.total_turns);
    println!("  Total Time:      {:.1}s", total_elapsed.as_secs_f64());
    println!("  Avg Latency:     {}ms/turn", total_metrics.avg_latency());

    println!("\n[Tool System]");
    println!(
        "  Tool Accuracy:       {:.1}% ({}/{})",
        total_metrics.tool_accuracy(),
        total_metrics.tools_correct,
        total_metrics.tools_total_expected
    );
    println!(
        "  Turns with Tools:    {}/{} ({:.0}%)",
        total_metrics.turns_with_tools,
        total_metrics.total_turns,
        total_metrics.turns_with_tools as f64 / total_metrics.total_turns as f64 * 100.0
    );
    println!(
        "  Multi-Tool Rate:     {}/{} ({:.0}%)",
        total_metrics.multi_tool_success,
        total_metrics.multi_tool_attempts,
        if total_metrics.multi_tool_attempts > 0 {
            total_metrics.multi_tool_success as f64 / total_metrics.multi_tool_attempts as f64
                * 100.0
        } else {
            0.0
        }
    );

    println!("\n[Memory System]");
    println!(
        "  Memory Recall Rate:  {:.0}% ({}/{})",
        total_metrics.memory_recall_rate(),
        total_metrics.memory_recall_success,
        total_metrics.memory_recall_queries
    );

    println!("\n[Context System]");
    println!(
        "  Context Continuity:  {:.0}% ({}/{})",
        total_metrics.context_continuity(),
        total_metrics.context_followup_success,
        total_metrics.context_followup_total
    );

    println!("\n[Task Completion]");
    println!(
        "  Single-Turn Rate:    {:.0}% ({}/{})",
        total_metrics.single_turn_rate(),
        total_metrics.single_turn_success,
        total_metrics.single_turn_tasks
    );
    println!(
        "  Multi-Turn Rate:     {:.0}% ({}/{})",
        total_metrics.multi_turn_rate(),
        total_metrics.multi_turn_success,
        total_metrics.multi_turn_tasks
    );
    println!(
        "  Resource Creation:   {:.0}% ({}/{})",
        total_metrics.resource_creation_rate(),
        total_metrics.resource_creation_success,
        total_metrics.resource_creation_tasks
    );

    // Overall score
    let weights = [
        (total_metrics.tool_accuracy(), 0.30),
        (total_metrics.single_turn_rate(), 0.15),
        (total_metrics.multi_turn_rate(), 0.15),
        (total_metrics.resource_creation_rate(), 0.15),
        (total_metrics.context_continuity(), 0.15),
        (total_metrics.memory_recall_rate(), 0.10),
    ];
    let overall: f64 = weights.iter().map(|(score, w)| score * w).sum::<f64>() / 100.0;

    println!("\n[Overall Score: {:.1}/100]", overall * 100.0);
    println!("   Weights: Tool(30%) + SingleTurn(15%) + MultiTurn(15%) + Resource(15%) + Context(15%) + Memory(10%)");

    let grade = match overall {
        x if x >= 0.85 => "A - Excellent",
        x if x >= 0.70 => "B - Good",
        x if x >= 0.55 => "C - Adequate",
        x if x >= 0.40 => "D - Needs Improvement",
        _ => "F - Critical Issues",
    };
    println!("   Grade: {}", grade);

    println!("\n{}", "═".repeat(70));

    // Sanity checks
    assert!(
        total_metrics.total_turns >= 300,
        "Should have 300+ turns across 20 rounds, got {}",
        total_metrics.total_turns
    );

    Ok(())
}
