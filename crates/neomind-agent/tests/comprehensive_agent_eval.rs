//! Comprehensive Agent Evaluation — 20 rounds × 15+ turns
//!
//! Evaluates:
//!   1. Tool System    — tool call accuracy, multi-tool, error recovery
//!   2. Memory System  — extraction, retention, cross-turn recall
//!   3. Context System — conversation continuity, long-context handling
//!   4. Task Completion — single-turn / multi-turn / complex resource creation
//!
//! The test self-hosts a sandbox NeoMind API server (fresh data dir, private
//! port, seeded devices) so the model operates against a REAL platform —
//! every `neomind` CLI call (in-process dispatch AND subprocess) targets it
//! via NEOMIND_API_BASE/NEOMIND_API_KEY. Without this, commands hit whatever
//! server happens to run on :9375 (or nothing) and the model's entire world
//! is error messages.
//!
//! Run (self-contained — no external env needed beyond the LLM backend):
//!   cargo test -p neomind-agent --test comprehensive_agent_eval -- --ignored --nocapture
//! Requires target/release/neomind (cargo build -p neomind-cli --release).

use std::sync::Arc;
use std::time::Instant;

use neomind_agent::llm_backends::{CloudConfig, CloudRuntime, OllamaConfig, OllamaRuntime};
use neomind_agent::session::SessionManager;
use neomind_agent::toolkit::{
    FileEditTool, FileWriteTool, ImageEditTool, MemoryTool, ShellConfig, ToolRegistryBuilder,
    WebFetchTool,
};
use neomind_core::llm::backend::LlmRuntime;

#[cfg(feature = "llamacpp")]
use neomind_agent::llm_backends::backends::llamacpp::{LlamaCppConfig, LlamaCppRuntime};

// ── sandbox platform ─────────────────────────────────────────────────

/// Self-hosted sandbox: `neomind serve` subprocess on a private port with a
/// fresh data dir, plus seeded devices so read turns have a real world.
mod sandbox {
    use std::io::Read;
    use std::sync::Mutex;

    static SERVER: Mutex<Option<std::process::Child>> = Mutex::new(None);

    fn serve_binary() -> std::path::PathBuf {
        let bin =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/release/neomind");
        if !bin.exists() {
            panic!(
                "sandbox needs {}; build it with: cargo build -p neomind-cli --release",
                bin.display()
            );
        }
        bin
    }

    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .expect("bind :0 for a free port")
            .local_addr()
            .expect("local addr")
            .port()
    }

    /// Start the sandbox server, point the whole test process at it, and
    /// seed the platform. Idempotent — the second call is a no-op.
    pub async fn start() {
        {
            let guard = SERVER.lock().unwrap();
            if guard.is_some() {
                return;
            }
        }

        let port = free_port();
        let data_dir =
            std::env::temp_dir().join(format!("neomind-eval-sbx-{}", std::process::id()));
        std::fs::create_dir_all(&data_dir).unwrap();
        let log_path = data_dir.join("serve.log");
        let log = std::fs::File::create(&log_path).unwrap();
        // The default-API-key banner is written on stderr — route it into
        // the same log or the key can never be harvested.
        let log_err = log.try_clone().unwrap();

        let mut child = std::process::Command::new(serve_binary())
            .arg("serve")
            .arg("--port")
            .arg(port.to_string())
            // Run from the sandbox dir: the storage layer falls back to a
            // CWD-relative legacy `data/` store when it finds one, which
            // would silently bind the sandbox to the repo's dev data.
            .current_dir(&data_dir)
            .env("NEOMIND_DATA_DIR", &data_dir)
            .stdout(log)
            .stderr(log_err)
            .spawn()
            .expect("spawn sandbox neomind serve");

        // Wait for HTTP readiness, then harvest the auto-generated default
        // API key from the first-boot banner in the log.
        let base = format!("http://127.0.0.1:{port}/api");
        let client = reqwest::Client::new();
        let mut ready = false;
        for _ in 0..60 {
            if client
                .get(format!("{base}/docs"))
                .timeout(std::time::Duration::from_secs(2))
                .send()
                .await
                .is_ok_and(|r| r.status().as_u16() == 200)
            {
                ready = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        if !ready {
            let _ = child.kill();
            panic!("sandbox server failed to become ready; log: {log_path:?}");
        }

        let mut log_text = String::new();
        if let Ok(mut f) = std::fs::File::open(&log_path) {
            let _ = f.read_to_string(&mut log_text);
        }
        let api_key = log_text
            .lines()
            .find_map(|l| {
                let idx = l.find("Key:")?;
                l[idx + 4..]
                    .split_whitespace()
                    .next()
                    .filter(|k| k.starts_with("nmk_"))
                    .map(String::from)
            })
            .unwrap_or_else(|| {
                let _ = child.kill();
                panic!("no default API key in sandbox log: {log_path:?}")
            });

        // Route EVERYTHING (in-process CLI dispatch + subprocess commands)
        // at the sandbox, and isolate all storage under its data dir.
        std::env::set_var("NEOMIND_DATA_DIR", &data_dir);
        std::env::set_var("NEOMIND_API_BASE", &base);
        std::env::set_var("NEOMIND_API_KEY", &api_key);
        // Scenarios use this to point web_fetch probes at a live local target.
        std::env::set_var("NEOMIND_EVAL_SANDBOX_PORT", port.to_string());

        *SERVER.lock().unwrap() = Some(child);
        eprintln!("sandbox platform ready on :{port} (data: {data_dir:?})");

        seed(&client, &base, &api_key).await;
    }

    /// Seed the world the eval's read turns assume: sensor_01 / sensor_02 /
    /// an office sensor, plus one threshold rule. All best-effort — a seed
    /// failure degrades realism, it must not kill the run.
    async fn seed(client: &reqwest::Client, base: &str, key: &str) {
        let auth = |r: reqwest::RequestBuilder| r.bearer_auth(key);
        let json_post = |url: String, body: serde_json::Value| auth(client.post(url).json(&body));

        // Register a generic sensor type (fresh data dirs ship only cameras).
        let _ = json_post(
            format!("{base}/device-types"),
            serde_json::json!({
                "device_type": "generic_sensor",
                "name": "Generic Sensor",
                "categories": ["sensor"],
            }),
        )
        .send()
        .await;

        // NOTE: telemetry seeding was attempted via webhook ingest but the
        // metrics land in the auto-onboard draft pipeline, not the
        // time-series store (verified 2026-09-09) — trend/history turns are
        // scored on command emission only. Known boundary.
        for (id, name) in [
            ("sensor_01", "办公室温湿度传感器"),
            ("sensor_02", "仓库温湿度传感器"),
            ("light_living", "客厅智能灯"),
        ] {
            let resp = json_post(
                format!("{base}/devices"),
                serde_json::json!({
                    "device_type": "generic_sensor",
                    "device_id": id,
                    "name": name,
                    "adapter_type": "mqtt",
                    "connection_config": {"topic": format!("neomind/devices/{id}")},
                }),
            )
            .send()
            .await;
            if !resp.as_ref().is_ok_and(|r| r.status().is_success()) {
                eprintln!("sandbox seed: device {id} failed: {resp:?}");
            }
        }

        let _ = json_post(
            format!("{base}/rules"),
            serde_json::json!({
                "name": "温度告警规则",
                "condition": {
                    "condition_type": "comparison",
                    "source": "device:sensor_01:temperature",
                    "operator": "greater_than",
                    "threshold": 35,
                },
                "actions": [{"type": "notify", "message": "温度超过35度"}],
            }),
        )
        .send()
        .await;

        // A tiny valid PNG for image_edit probes in the tools-breadth round.
        // 1x1 red pixel — enough for the tool's format sniffing.
        let png: [u8; 69] = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0x0D, 0x49, 0x48, 0x44, 0x52,
            0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0, 0x90, 0x77, 0x53, 0xDE, 0, 0, 0, 0x0C, 0x49,
            0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01,
            0x00, 0x18, 0xDD, 0x8D, 0xB0, 0, 0, 0, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60,
            0x82,
        ];
        let img_dir = std::env::temp_dir().join(format!("neomind-eval-sbx-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&img_dir);
        let _ = std::fs::write(img_dir.join("probe.png"), png);
    }

    /// Kill the sandbox server (call at test end; best-effort).
    pub fn stop() {
        if let Some(mut child) = SERVER.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

// ── helpers ───────────────────────────────────────────────────────────

fn ollama_up() -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 11434));
    std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(2)).is_ok()
}

/// llama.cpp standalone server (llama-server) mode, selected by setting
/// LLAMACPP_ENDPOINT (e.g. http://127.0.0.1:8080). The server has the model
/// loaded at startup; MODEL is forwarded in the request body, so give
/// llama-server a matching `--alias`.
///
/// Probes /props for the real context window and applies it as the
/// capabilities override — without it `max_context_length()` hardcodes 4096
/// and the session truncates history against a phantom budget, starving
/// cross-turn context and memory recall.
#[cfg(feature = "llamacpp")]
async fn llamacpp_llm() -> Arc<dyn LlmRuntime> {
    let endpoint = std::env::var("LLAMACPP_ENDPOINT").unwrap();
    let model = std::env::var("MODEL").unwrap_or_default();
    let runtime = LlamaCppRuntime::new(LlamaCppConfig {
        endpoint,
        model,
        timeout_secs: 240,
        api_key: None,
        cache_prompt: true,
    })
    .unwrap();
    let runtime = match runtime.detect_capabilities().await {
        Some(caps) => {
            eprintln!(
                "llama.cpp capabilities: n_ctx={}, tools={}, thinking={}, multimodal={}",
                caps.max_context,
                caps.supports_tools,
                caps.supports_thinking,
                caps.supports_multimodal
            );
            runtime.with_capabilities_override(
                caps.supports_multimodal,
                caps.supports_thinking,
                caps.supports_tools,
                caps.max_context,
            )
        }
        None => {
            eprintln!("warning: /props probe failed — assuming 4096 context");
            runtime
        }
    };
    Arc::new(runtime)
}

#[cfg(not(feature = "llamacpp"))]
async fn llamacpp_llm() -> Arc<dyn LlmRuntime> {
    panic!("LLAMACPP_ENDPOINT is set but the llamacpp feature is off — rebuild with --features llamacpp");
}

async fn new_session() -> (SessionManager, String) {
    let sm = SessionManager::memory();

    // Production-parity tool set. The real server's chat path registers the
    // full toolkit (shell first-class + standalone tools); without this the
    // session only carries the interaction tools from `Agent::new`
    // (ask_user / confirm_action / clarify_intent). A native-tools model
    // then can only ever ask questions — earlier scores came from models
    // emitting out-of-schema `shell` calls through the TEXT protocol, which
    // `parse_tool_calls` accepts but native OpenAI-tools backends never
    // produce. VisionTool is VLM-gated and extensions need installed
    // packages, so both stay out (matches a text-only production backend).
    let data_dir =
        std::path::PathBuf::from(std::env::var("NEOMIND_DATA_DIR").unwrap_or_else(|_| {
            std::env::temp_dir()
                .join("neomind-eval-data")
                .display()
                .to_string()
        }));
    let mut registry = ToolRegistryBuilder::new()
        .with_shell_tool(Some(ShellConfig {
            enabled: true,
            timeout_secs: 30,
            max_output_chars: 10_000,
        }))
        .build();
    registry.register(Arc::new(
        neomind_agent::toolkit::skill_tool::SkillTool::with_data_dir(
            sm.skill_registry(),
            data_dir.clone(),
        ),
    ));
    registry.register(Arc::new(WebFetchTool::new()));
    registry.register(Arc::new(FileWriteTool::new(data_dir.clone())));
    registry.register(Arc::new(FileEditTool::new(data_dir.clone())));
    registry.register(Arc::new(ImageEditTool::new(data_dir.clone())));
    let memory_store = neomind_storage::MarkdownMemoryStore::new(
        &neomind_storage::MemoryConfig::load().storage_path,
    );
    registry.register(Arc::new(MemoryTool::new(Arc::new(
        tokio::sync::RwLock::new(memory_store),
    ))));
    sm.set_tool_registry(Arc::new(registry)).await;

    let sid = sm.create_session().await.unwrap();

    // Memory recall is one of the measured dimensions — the R5/R10/R15
    // rounds plant facts and query them back. Enable the session memory
    // system so extraction ↔ recall is exercised end-to-end.
    let _ = sm.toggle_memory(&sid, true).await;

    let llm: Arc<dyn LlmRuntime> = if std::env::var("LLAMACPP_ENDPOINT").is_ok() {
        // llama.cpp standalone server mode (native tools path — run llama-server
        // with --jinja so the model's own chat template handles tool calls)
        llamacpp_llm().await
    } else if let Ok(api_key) = std::env::var("LLM_API_KEY") {
        // Cloud LLM mode. MODEL names containing "deepseek" use the built-in
        // DeepSeek provider (official endpoint, 128k context, native function
        // calling) — mirrors how production users configure DeepSeek. Other
        // models take the custom-endpoint path (LLM_ENDPOINT), which is the
        // proxy/vLLM scenario.
        let model = std::env::var("MODEL").unwrap_or("glm-5".into());
        let cfg = if model.to_lowercase().contains("deepseek") {
            CloudConfig::deepseek(api_key)
                .with_model(model)
                .with_timeout_secs(600)
        } else {
            let endpoint = std::env::var("LLM_ENDPOINT")
                .unwrap_or("https://open.bigmodel.cn/api/coding/paas/v4".into());
            CloudConfig::custom(api_key, endpoint)
                .with_model(model)
                .with_timeout_secs(600)
        };
        Arc::new(CloudRuntime::new(cfg).unwrap())
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
    // No manual format-teaching suffix needed anymore: every backend now
    // injects the text tool-calling protocol itself when the model reports
    // no native function calling (`llm_backends::text_tool_calls`), so this
    // eval measures the model, not the integration gap.
    (sm, sid)
}

// Process-wide turn telemetry — send() sees every turn but not the Metrics
// struct the scenarios own, so wall-clock latency and tool-turn counts
// accumulate here for the final report.
static TOTAL_ELAPSED_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static TURNS_WITH_TOOLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

// ── fairness view ─────────────────────────────────────────────────────
//
// The classic scoring rewards EMITTING the right domain command and gives
// zero for anything else — structurally penalizing models that investigate
// first (`--help` probes) or answer directly from context without a call.
// The transcript + fair rescore below re-judges every domain turn:
//   full command      → 1.0
//   exploration probe → 0.5  (help/list in the right domain)
//   direct answer     → 0.75 (no command, substantive reply)
//   nothing           → 0
// It is printed alongside the classic report, never replaces it.

#[derive(Clone)]
struct TurnRecord {
    query: String,
    commands: Vec<String>,
    tools: Vec<String>,
    content: String,
}

static TRANSCRIPT: std::sync::Mutex<Vec<TurnRecord>> = std::sync::Mutex::new(Vec::new());

fn expected_domain(query: &str) -> Option<&'static str> {
    let q = query.to_lowercase();
    let device =
        q.contains("设备") || q.contains("device") || q.contains("传感器") || q.contains("sensor");
    let rule = q.contains("规则") || q.contains("rule");
    let agent = q.contains("agent");
    if agent {
        Some("agent")
    } else if rule {
        Some("rule")
    } else if device {
        Some("device")
    } else {
        None
    }
}

fn fair_rescore() -> (f64, usize, usize) {
    let transcript = TRANSCRIPT.lock().unwrap();
    let (mut points, mut n, mut classic_hits) = (0.0f64, 0usize, 0usize);
    for t in transcript.iter() {
        let Some(domain) = expected_domain(&t.query) else {
            continue;
        };
        // Recall/summary/context turns are not command tasks.
        if t.query.contains("总结")
            || t.query.contains("summarize")
            || t.query.contains("我叫什么")
            || t.query.contains("my name")
            || t.query.contains("多少个传感器")
            || t.query.contains("how many sensors")
            || t.query.contains("摄像头")
            || t.query.contains("cameras?")
            || t.query.contains("阈值")
            || t.query.contains("threshold")
            || t.query.contains("通知方式")
            || t.query.contains("notification method")
            || t.query.contains("门禁")
            || t.query.contains("联系")
            || t.query.contains("contact")
        {
            continue;
        }
        n += 1;
        let prefix = format!("neomind {}", domain);
        let mut full = false;
        let mut expl = false;
        for c in &t.commands {
            let cl = c.to_lowercase();
            if cl.starts_with(&prefix) || cl.starts_with(&format!("neomind {} ", domain)) {
                if cl.contains("--help") {
                    expl = true;
                } else {
                    full = true;
                }
            }
        }
        if full {
            points += 1.0;
            classic_hits += 1;
        } else if expl {
            points += 0.5;
        } else if t.commands.is_empty() && t.content.chars().count() > 30 {
            points += 0.75;
        }
    }
    (points, n, classic_hits)
}

async fn send(sm: &SessionManager, sid: &str, msg: &str) -> MsgResult {
    let start = Instant::now();
    let resp = sm.process_message(sid, msg).await.unwrap();
    let elapsed = start.elapsed();
    TOTAL_ELAPSED_MS.fetch_add(
        elapsed.as_millis() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );
    // Extract CLI commands from shell tool calls
    let shell_commands: Vec<String> = resp
        .tool_calls
        .iter()
        .filter(|t| t.name == "shell")
        .filter_map(|t| t.arguments.get("command").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
        .collect();
    if !shell_commands.is_empty() {
        TURNS_WITH_TOOLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    TRANSCRIPT.lock().unwrap().push(TurnRecord {
        query: msg.to_string(),
        commands: shell_commands.clone(),
        tools: resp.tool_calls.iter().map(|t| t.name.clone()).collect(),
        content: resp.message.content.to_string(),
    });

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

    /// Did the model invoke the named tool at least once this turn?
    /// (For the non-shell breadth round: file_write/web_fetch/skill/memory/…)
    fn called_tool(&self, name: &str) -> bool {
        self.tool_calls.iter().any(|t| t == name)
    }
}

// ── metrics ───────────────────────────────────────────────────────────

#[derive(Default, Debug)]
struct Metrics {
    total_turns: usize,
    total_rounds: usize,

    // Tool system
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

// R6-R10: English mirrors of R1-R5 — the platform is bilingual; agent
// quality must hold in English too. Same structure and scoring, translated
// queries and English keyword checks.

async fn r6_device_management_en(sm: &SessionManager, sid: &str, m: &mut Metrics) -> Vec<String> {
    let mut notes = Vec::new();
    let checks: Vec<(&str, &str, Option<&str>)> = vec![
        ("List all devices", "device", Some("single")),
        (
            "Show the latest data for device sensor_01",
            "device",
            Some("single"),
        ),
        (
            "Show the temperature trend of sensor_01 over the last 24 hours",
            "device",
            None,
        ),
        ("What is the battery level of that device?", "", None), // context follow-up
        ("Turn on device light_living", "device", Some("single")),
        (
            "Check sensor_01 and sensor_02 at the same time",
            "device",
            Some("multi"),
        ),
        (
            "Analyze the online/offline status of all devices",
            "device",
            None,
        ),
        ("Which device did I just turn on?", "", None), // recall turn 5
        ("Show data for device nonexist999", "", None), // error recovery
        ("What is the temperature in my office?", "device", None),
        ("List all offline devices", "device", None),
        ("Turn off device light_living", "device", Some("single")),
        ("How is the signal strength of sensor_01?", "device", None),
        (
            "Compare the temperature of sensor_01 and sensor_02",
            "device",
            Some("multi"),
        ),
        ("Any device anomalies today?", "device", None),
    ];
    for (i, (q, domain, kind)) in checks.iter().enumerate() {
        let r = send(sm, sid, q).await;
        m.total_turns += 1;
        match (*domain, *kind) {
            ("device", Some("multi")) => {
                m.tools_total_expected += 2;
                m.multi_tool_attempts += 1;
                if r.domain_count("device") >= 2 {
                    m.tools_correct += 2;
                    m.multi_tool_success += 1;
                    notes.push(format!("T{}: ✅ multi-device", i + 1));
                } else if r.has_domain("device") {
                    m.tools_correct += 1;
                    notes.push(format!("T{}: ⚠️ partial multi-device", i + 1));
                } else {
                    notes.push(format!("T{}: ❌ multi-device", i + 1));
                }
            }
            ("device", _) => {
                m.tools_total_expected += 1;
                if r.has_domain("device") {
                    m.tools_correct += 1;
                    notes.push(format!("T{}: ✅ {}", i + 1, q));
                } else {
                    notes.push(format!("T{}: ❌ {}", i + 1, q));
                }
                if *kind == Some("single") {
                    m.single_turn_tasks += 1;
                    if r.has_domain("device") {
                        m.single_turn_success += 1;
                    }
                }
            }
            _ => {
                // context / recall / recovery turns
                if i == 3 {
                    m.context_followup_total += 1;
                    let ok = r.content.contains("sensor_01") || r.content.contains("battery");
                    if ok {
                        m.context_followup_success += 1;
                    }
                    notes.push(format!(
                        "T{}: {} context follow-up",
                        i + 1,
                        if ok { "✅" } else { "❌" }
                    ));
                } else if i == 7 {
                    m.context_followup_total += 1;
                    let ok = r.content.contains("light_living")
                        || r.content.contains("light")
                        || r.content.contains("living");
                    if ok {
                        m.context_followup_success += 1;
                    }
                    notes.push(format!(
                        "T{}: {} context recall",
                        i + 1,
                        if ok { "✅" } else { "❌" }
                    ));
                } else {
                    notes.push(format!(
                        "T{}: {} misc",
                        i + 1,
                        if r.content.is_empty() { "❌" } else { "✅" }
                    ));
                }
            }
        }
    }
    notes
}

async fn r7_rule_management_en(sm: &SessionManager, sid: &str, m: &mut Metrics) -> Vec<String> {
    let mut notes = Vec::new();
    let r = send(sm, sid, "List all automation rules").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    m.single_turn_tasks += 1;
    if r.has_domain("rule") {
        m.tools_correct += 1;
        m.single_turn_success += 1;
        notes.push("T1: ✅ list rules".into());
    } else {
        notes.push("T1: ❌ list rules".into());
    }

    let r = send(
        sm,
        sid,
        "Create a rule: send an alert when the temperature exceeds 35 degrees",
    )
    .await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    m.resource_creation_tasks += 1;
    if r.has_domain("rule") {
        m.tools_correct += 1;
        if r.content.contains("35") || r.content.contains("temperature") {
            m.resource_creation_success += 1;
            notes.push("T2: ✅ create temp rule".into());
        } else {
            notes.push("T2: ⚠️ rule created, content uncertain".into());
        }
    } else {
        notes.push("T2: ❌ create temp rule".into());
    }

    let r = send(
        sm,
        sid,
        "Create a rule: notify me when battery drops below 20%",
    )
    .await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    m.resource_creation_tasks += 1;
    if r.has_domain("rule") {
        m.tools_correct += 1;
        m.resource_creation_success += 1;
        notes.push("T3: ✅ create battery rule".into());
    } else {
        notes.push("T3: ❌ create battery rule".into());
    }

    let r = send(sm, sid, "Which rules did I just create?").await;
    m.total_turns += 1;
    m.context_followup_total += 1;
    let ok = r.content.contains("35")
        || r.content.contains("20")
        || r.content.contains("temperature")
        || r.content.contains("battery");
    if ok {
        m.context_followup_success += 1;
    }
    notes.push(format!(
        "T4: {} context recall rules",
        if ok { "✅" } else { "❌" }
    ));

    let r = send(
        sm,
        sid,
        "Create a rule: when temperature exceeds 30 and humidity drops below 40%, turn on the sprinkler system and notify me",
    )
    .await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    m.resource_creation_tasks += 1;
    if r.has_domain("rule") {
        m.tools_correct += 1;
        let complex = r.content.contains("30")
            && (r.content.contains("40") || r.content.contains("sprinkler"));
        if complex {
            m.resource_creation_success += 1;
            notes.push("T5: ✅ complex rule".into());
        } else {
            notes.push("T5: ⚠️ complex rule (partial)".into());
        }
    } else {
        notes.push("T5: ❌ complex rule".into());
    }

    let more: Vec<&str> = vec![
        "How many rules are there now?",
        "Delete the temperature alert rule",
        "Create a rule (without any condition)",
        "Disable the battery alert rule",
        "How many rules have I created in total?",
        "Create a rule: urgent notification when a device is offline for over 10 minutes",
        "List all disabled rules",
        "Create a rule: check all devices every day at 8am",
        "Change the temperature alert threshold to 38 degrees",
        "Delete all rules",
    ];
    for (i, q) in more.iter().enumerate() {
        let r = send(sm, sid, q).await;
        m.total_turns += 1;
        let needs_tool = !(q.contains("in total?") || q.starts_with("Create a rule ("));
        if needs_tool {
            m.tools_total_expected += 1;
        }
        if r.has_domain("rule") {
            if needs_tool {
                m.tools_correct += 1;
            }
            notes.push(format!("T{}: ✅ {}", 6 + i, q));
        } else {
            notes.push(format!("T{}: ❌ {}", 6 + i, q));
        }
    }
    notes
}

async fn r8_agent_management_en(sm: &SessionManager, sid: &str, m: &mut Metrics) -> Vec<String> {
    let mut notes = Vec::new();
    let r = send(sm, sid, "List all AI agents").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    if r.has_domain("agent") {
        m.tools_correct += 1;
        notes.push("T1: ✅ list agents".into());
    } else {
        notes.push("T1: ❌ list agents".into());
    }

    let r = send(
        sm,
        sid,
        "Create an agent called Temperature Patrol that runs every 5 minutes and checks all temperature sensors",
    )
    .await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    m.resource_creation_tasks += 1;
    if r.has_domain("agent") {
        m.tools_correct += 1;
        if r.content.contains("Temperature") || r.content.contains("Patrol") {
            m.resource_creation_success += 1;
            notes.push("T2: ✅ create temp agent".into());
        } else {
            notes.push("T2: ⚠️ agent create (uncertain)".into());
        }
    } else {
        notes.push("T2: ❌ create temp agent".into());
    }

    let r = send(
        sm,
        sid,
        "Create an agent: Battery Monitor, runs daily at 8am, checks battery levels and notifies",
    )
    .await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    m.resource_creation_tasks += 1;
    if r.has_domain("agent") {
        m.tools_correct += 1;
        m.resource_creation_success += 1;
        notes.push("T3: ✅ create battery agent".into());
    } else {
        notes.push("T3: ❌ create battery agent".into());
    }

    let r = send(sm, sid, "Show the details of the Temperature Patrol agent").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    if r.has_domain("agent") {
        m.tools_correct += 1;
        notes.push("T4: ✅ agent detail".into());
    } else {
        notes.push("T4: ❌ agent detail".into());
    }

    let r = send(sm, sid, "What is the second agent I created?").await;
    m.total_turns += 1;
    m.context_followup_total += 1;
    let ok = r.content.contains("Battery") || r.content.contains("battery");
    if ok {
        m.context_followup_success += 1;
    }
    notes.push(format!(
        "T5: {} context agent recall",
        if ok { "✅" } else { "❌" }
    ));

    let rest: Vec<(&str, bool)> = vec![
        ("Pause the Temperature Patrol agent", true),
        ("Show the execution history of the Temperature Patrol agent", true),
        ("Resume the Temperature Patrol agent", true),
        ("Create a smart ops agent that runs every 10 minutes, checks device status and creates alert rules on anomalies", true),
        ("List all agents and their status", true),
        ("Delete the Battery Monitor agent", true),
        ("How many agents are currently running?", false),
        ("Change the Temperature Patrol interval to 3 minutes", true),
        ("Delete all agents", true),
    ];
    for (i, (q, needs_tool)) in rest.iter().enumerate() {
        let r = send(sm, sid, q).await;
        m.total_turns += 1;
        if *needs_tool {
            m.tools_total_expected += 1;
        }
        if r.has_domain("agent") {
            if *needs_tool {
                m.tools_correct += 1;
            }
            notes.push(format!("T{}: ✅ {}", 6 + i, q));
        } else {
            notes.push(format!("T{}: ❌ {}", 6 + i, q));
        }
    }
    notes
}

async fn r9_cross_domain_en(sm: &SessionManager, sid: &str, m: &mut Metrics) -> Vec<String> {
    let mut notes = Vec::new();
    let r = send(sm, sid, "Check the status of all devices").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    if r.has_domain("device") {
        m.tools_correct += 1;
        notes.push("T1: ✅ check devices".into());
    } else {
        notes.push("T1: ❌ check devices".into());
    }

    let r = send(
        sm,
        sid,
        "Create an alert rule for devices with temperature above 30 degrees",
    )
    .await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    m.resource_creation_tasks += 1;
    if r.has_domain("rule") {
        m.tools_correct += 1;
        m.resource_creation_success += 1;
        notes.push("T2: ✅ cross-domain device→rule".into());
    } else {
        notes.push("T2: ❌ cross-domain device→rule".into());
    }

    let r = send(sm, sid, "Now create an agent to run that rule periodically").await;
    m.total_turns += 1;
    m.tools_total_expected += 1;
    m.resource_creation_tasks += 1;
    if r.has_domain("agent") {
        m.tools_correct += 1;
        m.resource_creation_success += 1;
        notes.push("T3: ✅ cross-domain rule→agent".into());
    } else {
        notes.push("T3: ❌ cross-domain rule→agent".into());
    }

    let r = send(sm, sid, "Summarize what I have just done").await;
    m.total_turns += 1;
    m.context_followup_total += 1;
    let ok = (r.content.contains("device") || r.content.contains("传感器"))
        && (r.content.contains("rule") || r.content.contains("规则"))
        && (r.content.contains("agent") || r.content.contains("Agent"));
    if ok {
        m.context_followup_success += 1;
    }
    notes.push(format!(
        "T4: {} cross-domain summary",
        if ok { "✅" } else { "❌" }
    ));

    let mixed: Vec<(&str, Option<&str>)> = vec![
        ("Show the battery status of all devices", Some("device")),
        (
            "Create a rule: notify when battery is below 15%",
            Some("rule"),
        ),
        ("List all current rules", Some("rule")),
        ("Was that battery rule created successfully?", None),
        (
            "Create an agent to check the battery once a day",
            Some("agent"),
        ),
        (
            "List all devices and all rules at the same time",
            Some("multi"),
        ),
        (
            "Pause the battery check agent we just created",
            Some("agent"),
        ),
        (
            "Inventory check: how many devices, rules and agents do I have?",
            None,
        ),
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
                    "T{}: {} multi-tool",
                    5 + i,
                    if d && ru { "✅" } else { "⚠️" }
                ));
            }
            Some(tool) => {
                m.tools_total_expected += 1;
                if r.has_domain(tool) {
                    m.tools_correct += 1;
                    notes.push(format!("T{}: ✅ {}", 5 + i, q));
                } else {
                    notes.push(format!("T{}: ❌ {}", 5 + i, q));
                }
            }
            None => {
                m.context_followup_total += 1;
                if r.content.len() > 20 {
                    m.context_followup_success += 1;
                }
                notes.push(format!("T{}: ✅ context", 5 + i));
            }
        }
    }
    notes
}

async fn r10_memory_context_stress_en(
    sm: &SessionManager,
    sid: &str,
    m: &mut Metrics,
) -> Vec<String> {
    let mut notes = Vec::new();
    let r = send(
        sm,
        sid,
        "Hi, my name is John Smith and I work at the Shanghai warehouse",
    )
    .await;
    m.total_turns += 1;
    notes.push(format!(
        "T1: {} intro",
        if r.content.contains("John") || r.content.contains("Hi") {
            "✅"
        } else {
            "⚠️"
        }
    ));

    let _r = send(
        sm,
        sid,
        "Our warehouse has 50 temperature sensors and 10 cameras",
    )
    .await;
    m.total_turns += 1;
    let _r = send(
        sm,
        sid,
        "Alert thresholds: temperature above 32 degrees, humidity above 80%",
    )
    .await;
    m.total_turns += 1;
    let _r = send(
        sm,
        sid,
        "Use SMS for notifications; call me by phone in emergencies",
    )
    .await;
    m.total_turns += 1;

    let recall_queries: Vec<(&str, Vec<&str>)> = vec![
        ("What is my name?", vec!["John", "Smith"]),
        ("Where do I work?", vec!["Shanghai", "warehouse"]),
        ("How many sensors do we have?", vec!["50"]),
        ("What is the temperature alert threshold?", vec!["32"]),
        (
            "How do you contact me in an emergency?",
            vec!["phone", "call"],
        ),
        ("What notification method did I say I prefer?", vec!["SMS"]),
        ("Do we have cameras? How many?", vec!["10", "cameras"]),
        (
            "Summarize everything I have told you",
            vec!["John", "Shanghai"],
        ),
        (
            "Create a temperature alert rule based on my requirements",
            vec!["32"],
        ),
        ("Create a humidity monitoring agent", vec!["80"]),
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
                r.content.chars().take(60).collect::<String>()
            ));
        }
        if i >= 8 {
            m.resource_creation_tasks += 1;
            if !r.shell_commands.is_empty() {
                m.resource_creation_success += 1;
            }
        }
    }

    let r = send(
        sm,
        sid,
        "Based on the thresholds I set earlier, create another humidity alert rule",
    )
    .await;
    m.total_turns += 1;
    m.resource_creation_tasks += 1;
    if r.has_domain("rule") {
        m.resource_creation_success += 1;
        notes.push("T15: ✅ rule from memory".into());
    } else {
        notes.push("T15: ❌ rule from memory".into());
    }
    notes
}

// R11: long-horizon round — 40 turns. Facts planted early, a wall of real
// tool traffic in the middle (filling the context with verbose results),
// recall probes at the end. Measures long-horizon memory + context rot.
async fn r11_long_horizon(sm: &SessionManager, sid: &str, m: &mut Metrics) -> Vec<String> {
    let mut notes = Vec::new();

    // T1-T5: plant durable facts interleaved with light queries.
    let _r = send(sm, sid, "记住几个信息:我叫李雷,负责北京3号仓库").await;
    m.total_turns += 1;
    let _r = send(sm, sid, "列出所有设备").await;
    m.total_turns += 1;
    let _r = send(sm, sid, "仓库有42个温湿度传感器、8个摄像头,门禁密码是8848").await;
    m.total_turns += 1;
    let _r = send(sm, sid, "查看设备 sensor_01 的最新数据").await;
    m.total_turns += 1;
    let _r = send(sm, sid, "告警策略:温度超过28度先发邮件,超过33度打电话").await;
    m.total_turns += 1;

    // T6-T28: the noise wall — varied tool traffic to fill the window with
    // real (verbose) results.
    let noise: Vec<&str> = vec![
        "列出所有设备",
        "查看规则列表",
        "创建规则:湿度超过75%时通知",
        "查看设备 sensor_02 的数据",
        "列出所有 Agent",
        "设备在线状态分析",
        "创建规则:每天早上7点检查传感器",
        "查看设备 light_living",
        "sensor_01 过去24小时温度趋势",
        "删除湿度告警规则",
        "创建Agent:每日巡检,每天9点执行",
        "列出所有规则",
        "查看 sensor_01 的电池电量",
        "关闭设备 light_living",
        "对比 sensor_01 和 sensor_02 的温度",
        "创建规则:设备离线10分钟告警",
        "查看所有Agent状态",
        "修改每日巡检Agent为8点执行",
        "列出离线设备",
        "查看传感器信号强度",
        "创建规则:rssi低于-80时通知",
        "列出所有设备类型",
        "设备异常检查",
        "查看告警历史",
    ];
    for q in &noise {
        let _r = send(sm, sid, q).await;
        m.total_turns += 1;
    }

    // T29-T40: long-horizon recall probes — the planted facts are now 25+
    // turns and a full context behind.
    let probes: Vec<(&str, Vec<&str>)> = vec![
        ("我叫什么名字?负责哪个仓库?", vec!["李雷", "北京", "3号"]),
        ("仓库有多少个温湿度传感器?", vec!["42"]),
        ("门禁密码是多少?", vec!["8848"]),
        ("温度超过多少度需要打电话?", vec!["33"]),
        ("温度28度以上应该做什么?", vec!["邮件"]),
        ("仓库有几个摄像头?", vec!["8"]),
        ("总结一下我最开始告诉你的所有信息", vec!["李雷", "8848"]),
        ("根据我最初的告警策略,创建一条33度打电话的规则", vec!["33"]),
        ("根据我的要求创建邮件提醒规则", vec!["28"]),
        ("我今天一共创建了多少条规则?", vec!["条", "规则"]),
    ];
    for (i, (q, keywords)) in probes.iter().enumerate() {
        let r = send(sm, sid, q).await;
        m.total_turns += 1;
        m.memory_recall_queries += 1;
        let hit = keywords.iter().any(|kw| r.content.contains(kw));
        if hit {
            m.memory_recall_success += 1;
            notes.push(format!("T{}: ✅ long-recall: {}", 29 + i, q));
        } else {
            notes.push(format!("T{}: ❌ long-recall: {}", 29 + i, q));
        }
        if i == 7 || i == 8 {
            m.resource_creation_tasks += 1;
            if r.has_domain("rule") {
                m.resource_creation_success += 1;
            }
        }
    }
    notes
}

// R12: tools-breadth round — direct probes for the non-shell tools the
// production registry carries (file_write / file_edit / web_fetch / skill /
// memory). The other rounds only ever exercise the shell CLI path.
async fn r12_tools_breadth(sm: &SessionManager, sid: &str, m: &mut Metrics) -> Vec<String> {
    let mut notes = Vec::new();
    let port = std::env::var("NEOMIND_EVAL_SANDBOX_PORT").unwrap_or_else(|_| "9375".into());

    let fetch_q = format!("获取 {port} 端口上本平台 API 文档页面的内容并简要总结");
    let probes: Vec<(&str, &str)> = vec![
        ("把当前设备清单保存到文件 device_report.md 里", "file_write"),
        (
            "把 device_report.md 文件里所有的 sensor_01 改成 sensor_99",
            "file_edit",
        ),
        (fetch_q.as_str(), "web_fetch"),
        (
            "记住一条重要信息:我们的紧急联系人是王工,电话13900000000",
            "memory",
        ),
        ("搜索并加载关于规则管理的技能指南", "skill"),
        ("把 probe.png 这张图片裁剪成正方形", "image_edit"),
    ];
    for (i, (q, tool)) in probes.iter().enumerate() {
        let r = send(sm, sid, q).await;
        m.total_turns += 1;
        m.tools_total_expected += 1;
        if r.called_tool(tool) {
            m.tools_correct += 1;
            notes.push(format!("T{}: ✅ {}", i + 1, tool));
        } else {
            notes.push(format!(
                "T{}: ❌ {} (called: {:?})",
                i + 1,
                tool,
                r.tool_calls
            ));
        }
    }

    // Contrast turns: same session, shell-domain asks — the model must
    // switch back and forth instead of latching onto the last tool.
    let shell_turns: Vec<(&str, &str)> = vec![
        ("列出所有设备", "device"),
        ("创建规则:温度超过30度时通知", "rule"),
        ("列出所有规则", "rule"),
        ("查看设备 sensor_01 最新数据", "device"),
        (
            "再把这些设备的清单追加到 device_report.md 文件末尾",
            "file_write",
        ),
        ("我们仓库的温度现在大概是多少?", "device"),
        ("总结一下这个文件里都有什么", "file_write"),
        ("把刚才创建的规则导出保存到 rules_export.md", "file_write"),
        ("对照技能指南,我刚才创建规则的姿势标准吗?", "skill"),
    ];
    for (i, (q, expect)) in shell_turns.iter().enumerate() {
        let r = send(sm, sid, q).await;
        m.total_turns += 1;
        m.tools_total_expected += 1;
        let ok = if ["device", "rule"].contains(expect) {
            r.has_domain(expect)
        } else {
            r.called_tool(expect)
        };
        if ok {
            m.tools_correct += 1;
            notes.push(format!("T{}: ✅ {}", 6 + i, expect));
        } else {
            notes.push(format!(
                "T{}: ❌ {} (called: {:?})",
                6 + i,
                expect,
                r.tool_calls
            ));
        }
    }
    notes
}

// Scenario dispatch: 12 distinct scenarios, cycling. ROUNDS=12 covers every
// scenario once; ROUNDS=24 (default) runs two full cycles.

async fn run_scenario(
    round: usize,
    sm: &SessionManager,
    sid: &str,
    m: &mut Metrics,
) -> Vec<String> {
    match round % 12 {
        0 => r1_device_management(sm, sid, m).await,
        1 => r2_rule_management(sm, sid, m).await,
        2 => r3_agent_management(sm, sid, m).await,
        3 => r4_cross_domain(sm, sid, m).await,
        4 => r5_memory_context_stress(sm, sid, m).await,
        5 => r6_device_management_en(sm, sid, m).await,
        6 => r7_rule_management_en(sm, sid, m).await,
        7 => r8_agent_management_en(sm, sid, m).await,
        8 => r9_cross_domain_en(sm, sid, m).await,
        9 => r10_memory_context_stress_en(sm, sid, m).await,
        10 => r11_long_horizon(sm, sid, m).await,
        11 => r12_tools_breadth(sm, sid, m).await,
        _ => unreachable!(),
    }
}

static SCENARIO_NAMES: &[&str] = &[
    "R01-设备管理",
    "R02-规则管理",
    "R03-Agent管理",
    "R04-跨域综合",
    "R05-记忆上下文",
    "R06-DeviceEN",
    "R07-RuleEN",
    "R08-AgentEN",
    "R09-CrossEN",
    "R10-MemoryEN",
    "R11-长程记忆40轮",
    "R12-工具广度",
    "R13-设备管理v2",
    "R14-规则管理v2",
    "R15-Agent管理v2",
    "R16-跨域综合v2",
    "R17-记忆上下文v2",
    "R18-DeviceEN-v2",
    "R19-RuleEN-v2",
    "R20-AgentEN-v2",
    "R21-CrossEN-v2",
    "R22-MemoryEN-v2",
    "R23-长程记忆v2",
    "R24-工具广度v2",
];

// ── Main test ─────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "Requires Ollama. cargo test -p neomind-agent --test comprehensive_agent_eval -- --ignored --nocapture"]
async fn comprehensive_20round_evaluation() -> anyhow::Result<()> {
    if !ollama_up()
        && std::env::var("LLM_API_KEY").is_err()
        && std::env::var("LLAMACPP_ENDPOINT").is_err()
    {
        eprintln!("Neither Ollama, llama.cpp, nor LLM_API_KEY available, skipping");
        return Ok(());
    }

    // Diagnostics: set EVAL_TRACE=1 to surface the crate's tracing output
    // (filter via RUST_LOG, default neomind_agent=debug) — shows the raw LLM
    // responses and parsed tool-call counts per turn.
    if std::env::var("EVAL_TRACE").is_ok() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "neomind_agent=debug".into()),
            )
            .with_writer(std::io::stderr)
            .try_init();
    }

    // Self-hosted sandbox platform: fresh data dir, private port, seeded
    // devices — the model's CLI calls operate on a real world instead of
    // 401s against whatever server happens to run on :9375.
    sandbox::start().await;

    let model = std::env::var("MODEL").unwrap_or("qwen3.5:2b".into());
    println!("\n{}", "═".repeat(70));
    println!("COMPREHENSIVE AGENT EVALUATION — 20 Rounds x 15+ Turns");
    println!("Model: {}", model);
    println!("{}\n", "═".repeat(70));

    let mut total_metrics = Metrics::default();
    let total_start = Instant::now();

    // ROUNDS env var limits the evaluation to the first N scenarios (quick mode).
    let max_rounds: usize = std::env::var("ROUNDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(SCENARIO_NAMES.len());

    for (idx, &name) in SCENARIO_NAMES.iter().enumerate().take(max_rounds) {
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
    let avg_ms = TOTAL_ELAPSED_MS.load(std::sync::atomic::Ordering::Relaxed)
        / total_metrics.total_turns.max(1) as u64;
    println!("  Avg Latency:     {avg_ms}ms/turn");

    println!("\n[Tool System]");
    println!(
        "  Tool Accuracy:       {:.1}% ({}/{})",
        total_metrics.tool_accuracy(),
        total_metrics.tools_correct,
        total_metrics.tools_total_expected
    );
    let turns_tools = TURNS_WITH_TOOLS.load(std::sync::atomic::Ordering::Relaxed);
    println!(
        "  Turns with Tools:    {}/{} ({:.0}%)",
        turns_tools,
        total_metrics.total_turns,
        turns_tools as f64 / total_metrics.total_turns as f64 * 100.0
    );
    println!(
        "  Multi-Tool Rate:     {}/{} ({:.0}%)",
        total_metrics.multi_tool_success,
        total_metrics.multi_tool_attempts,
        if total_metrics.multi_tool_attempts > 0 {
            total_metrics.multi_tool_success as f64 / total_metrics.multi_tool_attempts as f64
                * 100.0
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

    // Fairness view — re-judged domain turns (full command 1.0 / exploration
    // 0.5 / substantive direct answer 0.75). Printed alongside the classic
    // score: a large gap between the two views means the classic score was
    // penalizing investigation-first or answer-from-context behavior.
    let (fair_points, fair_n, classic_hits) = fair_rescore();
    if fair_n > 0 {
        println!("\n[Fairness View]");
        println!(
            "  Classic domain accuracy:  {:.1}% ({}/{})",
            classic_hits as f64 / fair_n as f64 * 100.0,
            classic_hits,
            fair_n
        );
        println!(
            "  Fair domain score:       {:.1}% ({:.1}/{})",
            fair_points / fair_n as f64 * 100.0,
            fair_points,
            fair_n
        );
        println!(
            "  Bias delta:              {:+.1}pp (negative = classic score penalized this model)",
            fair_points / fair_n as f64 * 100.0 - classic_hits as f64 / fair_n as f64 * 100.0
        );
    }

    println!("\n{}", "═".repeat(70));

    // Sanity checks — scale with ROUNDS (every scenario runs exactly 15
    // turns). ROUNDS=5 quick mode must not fail the 20-round expectation.
    let min_expected_turns = max_rounds * 15;
    assert!(
        total_metrics.total_turns >= min_expected_turns,
        "Should have {}+ turns across {} rounds, got {}",
        min_expected_turns,
        max_rounds,
        total_metrics.total_turns
    );

    sandbox::stop();
    Ok(())
}
