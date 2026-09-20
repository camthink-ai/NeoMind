//! Scheduler-level cooperative cancellation: stop() must abort in-flight
//! executions and kill their shell subprocesses (process group) quickly.
//!
//! Gated on the `test-utils` feature (MockLlmRuntime lives there); run via
//! `cargo test -p neomind-agent --features test-utils --test scheduler_cancellation_test`.

use std::time::Duration;

use neomind_agent::toolkit::shell::{ShellConfig, ShellTool};
use neomind_agent::toolkit::ToolRegistry;

/// Verifies that `scheduler.stop()` aborts a long-running agent execution.
///
/// Marked `#[ignore]` because it requires:
///   - A running storage backend (redb files in data/)
///   - A configured LLM backend
///   - A test agent that uses ShellTool to run `sleep 60`
///
/// Run with: `cargo test -p neomind-agent --test cancellation_test -- --ignored`
///
/// Expected behavior:
///   1. Spawn agent that calls `sleep 60`.
///   2. After 1s, call `scheduler.stop()`.
///   3. Assert `scheduler.stop()` returns within ~10s (not 60s).
///   4. Assert no `sleep 60` processes remain (subprocess killed by PidKillGuard).
#[tokio::test]
async fn scheduler_stop_aborts_long_running_execution() {
    use neomind_agent::ai_agent::{
        AgentExecutor, AgentExecutorConfig, AgentScheduler, SchedulerConfig,
    };
    use neomind_agent::testing_helpers::mock_llm::{MockLlmRuntime, MockResponse};
    use neomind_storage::AiAgent;

    fn sleep_running() -> bool {
        std::process::Command::new("pgrep")
            .args(["-f", "sleep 60"])
            .output()
            .map(|o| o.status.success() && !o.stdout.is_empty())
            .unwrap_or(false)
    }

    // Agent: free-mode, interval-scheduled, so the tick path spawns an
    // execution whose first LLM turn (mocked) is a `sleep 60` shell call.
    let agent: AiAgent = serde_json::from_value(serde_json::json!({
        "id": "cancel-test-agent",
        "name": "cancel-test",
        "user_prompt": "Run the sleep command.",
        "resources": [],
        "schedule": { "schedule_type": "interval", "interval_seconds": 1 },
        "status": "active",
        "created_at": 0,
        "updated_at": 0,
        "stats": {
            "total_executions": 0, "successful_executions": 0,
            "failed_executions": 0, "avg_duration_ms": 0, "last_duration_ms": null
        },
        "memory": {},
        "execution_mode": "free",
    }))
    .unwrap();

    let dir = std::env::temp_dir().join(format!(
        "nm-cancel-sched-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let store = neomind_storage::AgentStore::open(dir.join("agents.redb")).unwrap();
    store.save_agent(&agent).await.unwrap();

    let mock = std::sync::Arc::new(
        MockLlmRuntime::with_default(
            vec![
                // one warm-up turn: the analyzer spends the first scripted
                // response before the tool loop sees traffic
                MockResponse::text("warm-up"),
                MockResponse::tool_call("shell", serde_json::json!({ "command": "sleep 60" })),
            ],
            // after the aborted round nothing more matters; keep the loop finite
            MockResponse::text("done"),
        )
        .with_function_calling(),
    );
    let executor = AgentExecutor::new(AgentExecutorConfig {
        store,
        time_series_storage: None,
        device_service: None,
        event_bus: None,
        message_manager: None,
        llm_runtime: Some(mock),
        llm_backend_store: None,
        extension_registry: None,
        tool_registry: {
            let mut registry = ToolRegistry::new();
            registry.register(std::sync::Arc::new(ShellTool::new(ShellConfig {
                enabled: true,
                timeout_secs: 120,
                max_output_chars: 10_000,
            })));
            Some(std::sync::Arc::new(registry))
        },
        memory_store: None,
        backend_semaphores: None,
        skill_registry: None,
        execution_semaphore: None,
    })
    .await
    .unwrap();

    let scheduler = AgentScheduler::new(SchedulerConfig {
        tick_interval_ms: 100,
        max_concurrent: 4,
        max_concurrent_per_backend: 4,
        default_timezone: None,
    })
    .await
    .unwrap();
    scheduler.schedule_agent(agent).await.unwrap();
    scheduler
        .start(std::sync::Arc::new(executor))
        .await
        .unwrap();

    let _ = tracing_subscriber::fmt()
        .with_env_filter("neomind_agent=debug")
        .with_writer(std::io::stderr)
        .try_init();
    // 1. Wait until the tick path has actually spawned the execution and the
    //    shell tool has started `sleep 60` (bounded; on hang this fails the
    //    test instead of silently passing).
    let mut spawned = false;
    for _ in 0..200 {
        if sleep_running() {
            spawned = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(spawned, "execution never started a `sleep 60` subprocess");

    // 2. stop() must return quickly (aborts in-flight handles; the aborted
    //    future unwinds and PidKillGuard kills the process group).
    let t0 = std::time::Instant::now();
    scheduler.stop().await.unwrap();
    let stop_elapsed = t0.elapsed();

    // 3. The subprocess must be gone shortly after (killpg is immediate on
    //    unwind, but allow a grace window for reaping).
    for _ in 0..50 {
        if !sleep_running() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        stop_elapsed < Duration::from_secs(10),
        "scheduler.stop() took {:?} (>=10s) — abort path broken",
        stop_elapsed
    );
    assert!(!sleep_running(), "`sleep 60` subprocess survived stop()");
    let _ = std::fs::remove_dir_all(&dir);
}
