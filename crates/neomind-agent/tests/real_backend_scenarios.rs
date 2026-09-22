//! Scenarios that need a live LLM backend — the parts of the agent the
//! mock-driven suites cannot reach: an agent actually firing on its schedule,
//! and a real model filling an output contract.
//!
//! Run with:
//!   NEOMIND_TEST_MODEL=qwen3.5:4b \
//!     cargo test -p neomind-agent --test real_backend_scenarios -- --ignored

use std::sync::Arc;
use std::time::{Duration, Instant};

use neomind_agent::ai_agent::{
    AgentExecutor, AgentExecutorConfig, AgentScheduler, AiAgentManager, SchedulerConfig,
};
use neomind_agent::llm_backends::backends::ollama::{OllamaConfig, OllamaRuntime};
use neomind_core::eventbus::EventBus;
use neomind_core::llm::backend::LlmRuntime;
use neomind_storage::{AgentStore, AiAgent, TimeSeriesStore};

/// The model these tests drive. Override with `NEOMIND_TEST_MODEL`.
fn test_model() -> String {
    std::env::var("NEOMIND_TEST_MODEL")
        .or_else(|_| std::env::var("MODEL"))
        .unwrap_or_else(|_| "qwen3.5:4b".to_string())
}

fn ollama_endpoint() -> String {
    std::env::var("OLLAMA_ENDPOINT").unwrap_or_else(|_| "http://localhost:11434".to_string())
}

fn ollama_available() -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 11434));
    std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(2)).is_ok()
}

/// An executor wired to a real model, with in-memory storage so a test can read
/// back what the run produced.
async fn live_executor(
    store: Arc<AgentStore>,
    time_series: Arc<TimeSeriesStore>,
) -> AgentExecutor {
    let model = test_model();
    live_executor_with_model(store, time_series, &model).await
}

/// [`live_executor`] with an explicit model — the vision scenario pins a
/// known-multimodal one instead of trusting NEOMIND_TEST_MODEL.
async fn live_executor_with_model(
    store: Arc<AgentStore>,
    time_series: Arc<TimeSeriesStore>,
    model: &str,
) -> AgentExecutor {
    let runtime = OllamaRuntime::new(OllamaConfig {
        endpoint: ollama_endpoint(),
        model: model.to_string(),
        timeout_secs: 120,
    })
    .expect("ollama runtime");

    AgentExecutor::new(AgentExecutorConfig {
        store,
        time_series_storage: Some(time_series),
        device_service: None,
        event_bus: Some(Arc::new(EventBus::new())),
        message_manager: None,
        llm_runtime: Some(Arc::new(runtime) as Arc<dyn LlmRuntime>),
        llm_backend_store: None,
        extension_registry: None,
        tool_registry: None,
        memory_store: None,
        backend_semaphores: None,
        skill_registry: None,
        execution_semaphore: None,
    })
    .await
    .expect("executor")
}

fn agent_json(id: &str, extra: serde_json::Value) -> AiAgent {
    let mut base = serde_json::json!({
        "id": id,
        "name": id,
        "user_prompt": "看一下车间现在有没有异常，没有就说正常。",
        "resources": [],
        "schedule": { "schedule_type": "interval", "interval_seconds": 10 },
        "status": "active",
        "created_at": 0,
        "updated_at": 0,
        "execution_mode": "focused",
        "stats": {
            "total_executions": 0, "successful_executions": 0,
            "failed_executions": 0, "avg_duration_ms": 0, "last_duration_ms": null
        },
        "memory": {},
    });
    for (k, v) in extra.as_object().expect("extra is an object") {
        base[k] = v.clone();
    }
    serde_json::from_value(base).expect("agent fixture")
}


/// Restart path: an interval agent that already exists in storage when the
/// manager starts must be picked up by `reload_active_agents` and fire on its
/// own. A live instance showed exactly this agent sitting dormant for 30+
/// minutes after a restart — this test decides whether the reload path is
/// broken in code or was a stale-binary artifact.
#[tokio::test]
#[ignore = "Requires Ollama LLM backend"]
async fn restart_reload_reschedules_existing_interval_agent() {
    if !ollama_available() {
        println!("⚠️  Ollama not running — skipping");
        return;
    }

    // The agent exists BEFORE the manager starts — as after a real restart.
    let store = AgentStore::memory().expect("agent store");
    let agent = agent_json(
        "restart-reload-agent",
        serde_json::json!({
            "schedule": { "schedule_type": "interval", "interval_seconds": 5 },
            "execution_mode": "structured",
            "output_schema": [
                { "name": "status", "field_type": { "type": "enum", "values": ["正常", "异常"] } }
            ],
        }),
    );
    store.save_agent(&agent).await.expect("seed pre-existing agent");

    let ts = TimeSeriesStore::memory().expect("ts");
    let manager = AiAgentManager::new(AgentExecutorConfig {
        store: store.clone(),
        time_series_storage: Some(ts),
        device_service: None,
        event_bus: Some(Arc::new(EventBus::new())),
        message_manager: None,
        llm_runtime: Some(Arc::new(OllamaRuntime::new(OllamaConfig {
            endpoint: ollama_endpoint(),
            model: test_model(),
            timeout_secs: 120,
        }).expect("runtime")) as Arc<dyn LlmRuntime>),
        llm_backend_store: None,
        extension_registry: None,
        tool_registry: None,
        memory_store: None,
        backend_semaphores: None,
        skill_registry: None,
        execution_semaphore: None,
    })
    .await
    .expect("manager");
    manager.start().await.expect("start — reload + scheduler");

    // Interval 5s; give the reload + tick path generous room.
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut records = Vec::new();
    while Instant::now() < deadline {
        records = store
            .get_agent_executions(&agent.id, 10)
            .await
            .unwrap_or_default();
        if !records.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    assert!(
        !records.is_empty(),
        "a pre-existing interval agent MUST be rescheduled by startup reload and fire"
    );
}


fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("neomind_agent=debug")
        .with_writer(std::io::stderr)
        .try_init();
}

/// A 32×32 solid-RED PNG, hand-assembled — no fixture files, no image crate.
const RED_PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAIAAAD8GO2jAAAAKElEQVR4nO3NsQ0AAAzCMP5/un0CNkuZ41wybXsHAAAAAAAAAAAAxR4yw/wuPL6QkAAAAABJRU5ErkJggg==";

/// The flagship S1 scenario against a real vision model, through the FULL
/// production path: an event carries the image as a data URI, collection
/// extracts it, the structured inference attaches it as a multimodal part,
/// and the published field reflects what is actually IN the picture.
/// Red in, 红色 out — a text-only or blind path cannot fake this.
#[tokio::test]
#[ignore = "Requires an Ollama vision model"]
async fn vision_agent_reads_the_actual_pixels() {
    if !ollama_available() {
        println!("⚠️  Ollama not running — skipping");
        return;
    }

    let store = AgentStore::memory().expect("store");
    let ts = TimeSeriesStore::memory().expect("ts");
    let agent = agent_json(
        "vision-live",
        serde_json::json!({
            "schedule": { "schedule_type": "manual" },
            "user_prompt": "看这张图片，回答画面的主色。",
            "execution_mode": "structured",
            "output_schema": [
                { "name": "dominant_color", "field_type": { "type": "enum", "values": ["红色", "蓝色", "绿色", "白色"] } }
            ],
        }),
    );
    store.save_agent(&agent).await.expect("seed");

    // qwen3.5:4b is multimodal (verified in live runs this session); pinning
    // it keeps the test meaningful regardless of NEOMIND_TEST_MODEL.
    let executor = live_executor_with_model(store, ts.clone(), "qwen3.5:4b").await;

    use neomind_agent::ai_agent::executor::{DataSourceRef, EventTriggerData};
    let event = EventTriggerData {
        source: DataSourceRef {
            source_type: "device".into(),
            source_id: "cam-red".into(),
            field: "values.image".into(),
        },
        value: neomind_core::event::MetricValue::String(format!(
            "data:image/png;base64,{RED_PNG_B64}"
        )),
        timestamp: 0,
    };

    init_tracing();
    let run = executor
        .execute_agent(agent, Some(event), None)
        .await
        .expect("vision run completes");
    eprintln!("vision run error field: {:?}", run.error);

    ts.flush().expect("flush");
    let published = ts
        .query_latest("ai:vision-live", "dominant_color")
        .await
        .expect("read back")
        .expect("field published");
    assert_eq!(
        published.value, serde_json::json!("红色"),
        "the model must have seen the actual pixels"
    );
}

/// A scheduled agent fires on its own — no invoke, no event — and leaves an
/// execution record behind. The model is real; only the clock is short.
#[tokio::test]
#[ignore = "Requires Ollama LLM backend"]
async fn scheduled_agent_fires_on_its_interval() {
    if !ollama_available() {
        println!("⚠️  Ollama not running — skipping");
        return;
    }

    let store = AgentStore::memory().expect("memory store");
    let time_series = TimeSeriesStore::memory().expect("memory timeseries");
    let agent = agent_json("scheduled-live", serde_json::json!({}));
    store.save_agent(&agent).await.expect("save agent");

    let executor = Arc::new(live_executor(store.clone(), time_series).await);
    let scheduler = AgentScheduler::new(SchedulerConfig {
        tick_interval_ms: 250,
        max_concurrent: 2,
        max_concurrent_per_backend: 2,
        default_timezone: None,
    })
    .await
    .expect("scheduler");
    scheduler.schedule_agent(agent.clone()).await.expect("schedule");
    scheduler.start(executor.clone()).await.expect("start");

    // The interval is 10s; give the tick path generous headroom before failing.
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut records = Vec::new();
    while Instant::now() < deadline {
        records = store
            .get_agent_executions(&agent.id, 10)
            .await
            .unwrap_or_default();
        if !records.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let _ = scheduler.stop().await;

    assert!(
        !records.is_empty(),
        "a scheduled agent must fire on its own within its interval"
    );
    let record = &records[0];
    assert!(
        record.error.is_none(),
        "the scheduled run must complete without error: {:?}",
        record.error
    );
    assert!(
        !record.decision_process.conclusion.is_empty(),
        "a real model must produce a conclusion"
    );
}

/// M2-2 against a real model: a *reasoning* agent's conclusion comes back as the
/// declared fields and lands in telemetry under `ai:{id}:{field}`.
#[tokio::test]
#[ignore = "Requires Ollama LLM backend"]
async fn reasoning_agent_publishes_its_output_contract() {
    if !ollama_available() {
        println!("⚠️  Ollama not running — skipping");
        return;
    }

    let store = AgentStore::memory().expect("memory store");
    let time_series = TimeSeriesStore::memory().expect("memory timeseries");
    let agent = agent_json(
        "contract-live",
        serde_json::json!({
            "user_prompt": "判断车间状态：正常或异常，并给一个 0-100 的评分。",
            "output_schema": [
                { "name": "status", "field_type": { "type": "enum", "values": ["正常", "异常"] } },
                { "name": "score", "field_type": { "type": "number" } }
            ],
        }),
    );
    store.save_agent(&agent).await.expect("save agent");

    let executor = live_executor(store.clone(), time_series.clone()).await;
    executor
        .execute_agent(agent.clone(), None, None)
        .await
        .expect("run completes");

    // The contract step publishes `ai:{agent_id}:{field}` — read it back from
    // telemetry, the same place a dashboard would.
    let namespace = format!("ai:{}", agent.id);
    let mut published = Vec::new();
    for field in ["status", "score"] {
        if let Ok(Some(point)) = time_series.query_latest(&namespace, field).await {
            published.push((field, point.value));
        }
    }

    assert!(
        !published.is_empty(),
        "a reasoning agent's output contract must reach telemetry under {namespace}:*; got {published:?}"
    );
}
